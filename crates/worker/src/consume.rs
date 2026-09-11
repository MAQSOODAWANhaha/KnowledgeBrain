//! oxana `default` consumer: convert only (ticket 09).

#[cfg(not(unix))]
compile_error!("the Worker subprocess supervision contract requires Unix process groups");

use async_trait::async_trait;
use platform::{
    BidAuthoringJobPayloadV2, BidAuthoringV2Queue, ContentGenerateJobV2,
    ContentGenerateOperationV2, DatatableJob, DefaultQueue, DocumentProcessJob, DocxComposeJobV2,
    ExtractJob, HousekeepJob, ImageMultimodalJob, IndexDeleteJob, KbDeleteJob,
    KnowledgeSemanticIndexV2Job, ListDeleteJob, ListReparseJob, LowQueue, ManualProcessJob,
    MultimodalQueue, PostProcessJob, PostprocessQueue, QuestionJob, RequirementSetCompileJobV2,
    SubmissionExportJobV2, SummaryJob, SummaryQueue, TenderDocumentProcessJobV2, VersionCloneJob,
    WikiFinalizeJob, WikiIngestJob, WikiQueue,
};
use sqlx::PgPool;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const MAX_EXPORT_INPUT_BYTES: usize = 64 * 1024 * 1024;
const MAX_FROZEN_ASSET_COUNT: usize = 2_048;
const MAX_FROZEN_ASSET_TOTAL_BYTES: u64 = 256 * 1024 * 1024;
const MAX_FROZEN_ASSET_TOTAL_PIXELS: u64 = 300_000_000;
const MAX_PDF_ATTACHMENT_BYTES: usize = 128 * 1024 * 1024;
const MAX_TENDER_DOCUMENT_BYTES: usize = 50 * 1024 * 1024;
const MAX_PDF_ATTACHMENT_PAGES: usize = 1_000;
const MAX_RASTER_TOTAL_BYTES: usize = 256 * 1024 * 1024;
const MAX_RENDER_OUTPUT_BYTES: usize = 256 * 1024 * 1024;
const MAX_EXPORT_BLOCK_OCCURRENCES: usize = 100_000;
const MAX_EXPORT_TABLE_ROWS: u64 = 100_000;
const MAX_EXPORT_TABLE_CELLS: u64 = 100_000;
const MAX_EXPORT_FORM_DEFINITIONS: usize = 10_000;
const MAX_EXPORT_ATTACHMENT_PREPARATIONS: usize = 2_048;
const MAX_EXPORT_REFERENCE_WORK_BYTES: u64 = 512 * 1024 * 1024;
const RENDER_TIMEOUT_SECONDS: u64 = 120;
const HANDLER_CLEANUP_MARGIN: std::time::Duration = std::time::Duration::from_secs(30);
const TERMINAL_PERSISTENCE_RESERVE: std::time::Duration = std::time::Duration::from_secs(5);
const TASK_ABORT_DRAIN_RESERVE: std::time::Duration = std::time::Duration::from_millis(100);
const TENDER_HANDLER_HARD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30 * 60);
const REQUIREMENT_HANDLER_HARD_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(45 * 60);
const DOCX_COMPOSE_HANDLER_HARD_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(45 * 60);
const SUBMISSION_EXPORT_HANDLER_HARD_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(30 * 60);
const CONTENT_GENERATE_HANDLER_HARD_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(45 * 60);
const CONTENT_MATCH_HANDLER_HARD_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(10 * 60);
const SUBMISSION_RENDER_HELPER_ARG: &str = "--kb-submission-render-helper-v1";
const PDF_RASTER_HELPER_ARG: &str = "--kb-pdf-raster-helper-v1";

#[derive(Clone)]
pub struct AppCtx {
    pub pool: Option<PgPool>,
    /// External SIGINT/SIGTERM only. Fatal children must never set this token.
    pub shutdown: CancellationToken,
    /// Process-local cancellation used to stop siblings after a fatal child.
    pub root_cancel: CancellationToken,
}

pub struct SubmissionExportV2Worker {
    pool: Option<PgPool>,
    shutdown: CancellationToken,
}

impl oxana::FromContext<AppCtx> for SubmissionExportV2Worker {
    fn from_context(ctx: &AppCtx) -> Self {
        Self {
            pool: ctx.pool.clone(),
            shutdown: ctx.shutdown.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct HandlerDeadline {
    hard: tokio::time::Instant,
    cleanup: tokio::time::Instant,
}

impl HandlerDeadline {
    fn from_now(hard_timeout: std::time::Duration) -> Self {
        let hard = tokio::time::Instant::now() + hard_timeout;
        Self {
            hard,
            cleanup: hard + HANDLER_CLEANUP_MARGIN,
        }
    }

    fn early_cleanup(self) -> tokio::time::Instant {
        std::cmp::min(
            tokio::time::Instant::now() + HANDLER_CLEANUP_MARGIN,
            self.cleanup,
        )
    }
}

#[derive(Debug)]
enum OwnedHandlerCompletion {
    Completed(Result<(), JobErr>),
    TimedOut,
    ShuttingDown,
}

#[derive(Debug)]
struct OwnedHandlerRun {
    completion: OwnedHandlerCompletion,
    cleanup_error: Option<String>,
    cleanup_deadline: tokio::time::Instant,
    #[cfg(test)]
    teardown_deadline: tokio::time::Instant,
}

fn teardown_deadline_for_effect(
    cleanup_deadline: tokio::time::Instant,
    requires_effect: bool,
) -> tokio::time::Instant {
    if requires_effect {
        cleanup_deadline
            .checked_sub(TERMINAL_PERSISTENCE_RESERVE)
            .unwrap_or(cleanup_deadline)
    } else {
        cleanup_deadline
    }
}

async fn terminalize_until<F, T>(
    cleanup_deadline: tokio::time::Instant,
    label: &str,
    future: F,
) -> Result<T, JobErr>
where
    F: std::future::Future<Output = Result<T, JobErr>>,
{
    let terminal_deadline = std::cmp::min(
        tokio::time::Instant::now() + TERMINAL_PERSISTENCE_RESERVE,
        cleanup_deadline,
    );
    tokio::time::timeout_at(terminal_deadline, future)
        .await
        .map_err(|_| JobErr(format!("{label} exceeded the terminal persistence reserve")))?
}

async fn cleanup_tracker_until(
    cleanup: Option<&platform::StagedObjectCleanupTracker>,
    deadline: tokio::time::Instant,
) -> Option<String> {
    let cleanup = cleanup.filter(|tracker| tracker.has_pending())?;
    match tokio::time::timeout_at(deadline, cleanup.cleanup_pending()).await {
        Ok(Ok(())) => None,
        Ok(Err(error)) => Some(error),
        Err(_) => Some("staged object cleanup exceeded the absolute cleanup deadline".into()),
    }
}

async fn join_cancelled_handler(
    handle: &mut tokio::task::JoinHandle<Result<(), JobErr>>,
    local_cancel: &CancellationToken,
    cleanup: Option<&platform::StagedObjectCleanupTracker>,
    cleanup_deadline: tokio::time::Instant,
) -> Option<String> {
    local_cancel.cancel();
    let abort_deadline = cleanup_deadline
        .checked_sub(TASK_ABORT_DRAIN_RESERVE)
        .unwrap_or(cleanup_deadline);
    if tokio::time::timeout_at(abort_deadline, &mut *handle)
        .await
        .is_err()
    {
        handle.abort();
        let _ = tokio::time::timeout_at(cleanup_deadline, &mut *handle).await;
    }
    cleanup_tracker_until(cleanup, cleanup_deadline).await
}

async fn run_owned_handler<F>(
    future: F,
    deadline: HandlerDeadline,
    shutdown: CancellationToken,
    local_cancel: CancellationToken,
    cleanup: Option<&platform::StagedObjectCleanupTracker>,
    completed_error_requires_effect: fn(&JobErr) -> bool,
) -> OwnedHandlerRun
where
    F: std::future::Future<Output = Result<(), JobErr>> + Send + 'static,
{
    let mut handle = tokio::spawn(future);
    let completion = tokio::select! {
        biased;
        () = shutdown.cancelled() => OwnedHandlerCompletion::ShuttingDown,
        () = tokio::time::sleep_until(deadline.hard) => OwnedHandlerCompletion::TimedOut,
        result = &mut handle => OwnedHandlerCompletion::Completed(
            result.unwrap_or_else(|error| Err(JobErr(format!("handler task join failed: {error}"))))
        ),
    };
    let absolute_cleanup = deadline.early_cleanup();
    let requires_effect = match &completion {
        OwnedHandlerCompletion::TimedOut => true,
        OwnedHandlerCompletion::Completed(Err(error)) => completed_error_requires_effect(error),
        OwnedHandlerCompletion::Completed(Ok(())) | OwnedHandlerCompletion::ShuttingDown => false,
    };
    let work_cleanup = teardown_deadline_for_effect(absolute_cleanup, requires_effect);
    let cleanup_error = match &completion {
        OwnedHandlerCompletion::Completed(_) => cleanup_tracker_until(cleanup, work_cleanup).await,
        OwnedHandlerCompletion::ShuttingDown => {
            join_cancelled_handler(&mut handle, &local_cancel, cleanup, work_cleanup).await
        }
        OwnedHandlerCompletion::TimedOut => {
            local_cancel.cancel();
            let abort_deadline = work_cleanup
                .checked_sub(TASK_ABORT_DRAIN_RESERVE)
                .unwrap_or(work_cleanup);
            if tokio::time::timeout_at(abort_deadline, &mut handle)
                .await
                .is_err()
            {
                handle.abort();
                let _ = tokio::time::timeout_at(work_cleanup, &mut handle).await;
            }
            None
        }
    };
    OwnedHandlerRun {
        completion,
        cleanup_error,
        cleanup_deadline: absolute_cleanup,
        #[cfg(test)]
        teardown_deadline: work_cleanup,
    }
}

async fn stage_export_object(
    pool: &PgPool,
    staging_id: Uuid,
    digest: &str,
    media_type: &str,
    bytes: &[u8],
    actor: &str,
) -> Result<String, JobErr> {
    if bytes.is_empty() || bytes.len() > MAX_RENDER_OUTPUT_BYTES {
        return Err(JobErr(
            "staged export object exceeds the byte budget".into(),
        ));
    }
    let object_ref = platform::object_ref(digest);
    let byte_length =
        i64::try_from(bytes.len()).map_err(|_| JobErr("rendered object too large".into()))?;
    platform::stage_object_upload(
        pool,
        staging_id,
        &object_ref,
        digest,
        media_type,
        byte_length,
        actor,
    )
    .await
    .map_err(non_agent_sql_error)?;
    if let Err(error) = platform::write_blob_off_runtime(digest, bytes) {
        let _ = platform::schedule_object_upload_cleanup(staging_id).await;
        return Err(JobErr(format!(
            "TRANSIENT_HANDLER:write rendered object: {error}"
        )));
    }
    Ok(object_ref)
}

fn validate_frozen_asset_metadata(values: &[serde_json::Value]) -> Result<(), JobErr> {
    if values.len() > MAX_FROZEN_ASSET_COUNT {
        return Err(JobErr("frozen render asset count exceeds budget".into()));
    }
    let mut identities = std::collections::HashSet::with_capacity(values.len());
    let mut total_bytes = 0u64;
    let mut total_pixels = 0u64;
    let mut total_pdf_pages = 0u64;
    for value in values {
        let identity = value
            .get("asset_revision_id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| JobErr("frozen asset identity metadata missing".into()))?;
        if !identities.insert(identity) {
            return Err(JobErr("duplicate frozen asset identity".into()));
        }
        let media_type = value
            .get("media_type")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| JobErr("frozen asset media type metadata missing".into()))?;
        let byte_length = value
            .get("byte_length")
            .and_then(serde_json::Value::as_u64)
            .filter(|length| *length > 0)
            .ok_or_else(|| JobErr("frozen asset byte length metadata missing".into()))?;
        total_bytes = total_bytes
            .checked_add(byte_length)
            .ok_or_else(|| JobErr("frozen asset byte budget overflow".into()))?;
        if total_bytes > MAX_FROZEN_ASSET_TOTAL_BYTES {
            return Err(JobErr(
                "frozen render assets exceed aggregate byte budget".into(),
            ));
        }
        let width = value.get("width_px").and_then(serde_json::Value::as_u64);
        let height = value.get("height_px").and_then(serde_json::Value::as_u64);
        let page_count = value.get("page_count").and_then(serde_json::Value::as_u64);
        if media_type.starts_with("image/") {
            let (Some(width), Some(height), None) = (width, height, page_count) else {
                return Err(JobErr("frozen image dimension metadata incomplete".into()));
            };
            let pixels = width
                .checked_mul(height)
                .ok_or_else(|| JobErr("frozen asset pixel metadata overflow".into()))?;
            total_pixels = total_pixels
                .checked_add(pixels)
                .ok_or_else(|| JobErr("frozen asset pixel budget overflow".into()))?;
        } else if media_type == "application/pdf" {
            let (None, None, Some(pages)) = (width, height, page_count) else {
                return Err(JobErr("frozen PDF page metadata incomplete".into()));
            };
            if pages == 0 || pages > MAX_PDF_ATTACHMENT_PAGES as u64 {
                return Err(JobErr("frozen PDF page count exceeds budget".into()));
            }
            total_pdf_pages = total_pdf_pages
                .checked_add(pages)
                .ok_or_else(|| JobErr("frozen PDF page count overflow".into()))?;
        } else if width.is_some() || height.is_some() || page_count.is_some() {
            return Err(JobErr("non-visual frozen asset has visual metadata".into()));
        }
        if total_pixels > MAX_FROZEN_ASSET_TOTAL_PIXELS
            || total_pdf_pages > MAX_PDF_ATTACHMENT_PAGES as u64
        {
            return Err(JobErr(
                "frozen render assets exceed aggregate page or pixel budget".into(),
            ));
        }
    }
    Ok(())
}

fn validate_submission_export_metadata(input: &serde_json::Value) -> Result<(), JobErr> {
    let assets = input
        .get("assets")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| JobErr("frozen export assets missing".into()))?;
    validate_frozen_asset_metadata(assets)?;
    let mut asset_metadata = std::collections::HashMap::with_capacity(assets.len());
    for asset in assets {
        let id = asset
            .get("asset_revision_id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| JobErr("frozen asset identity metadata missing".into()))?;
        let bytes = asset
            .get("byte_length")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| JobErr("frozen asset byte length metadata missing".into()))?;
        let pixels = match (
            asset.get("width_px").and_then(serde_json::Value::as_u64),
            asset.get("height_px").and_then(serde_json::Value::as_u64),
        ) {
            (Some(width), Some(height)) => width
                .checked_mul(height)
                .ok_or_else(|| JobErr("frozen asset occurrence pixels overflow".into()))?,
            _ => 0,
        };
        asset_metadata.insert(id, (bytes, pixels));
    }

    let blocks = input
        .get("workspace")
        .and_then(|workspace| workspace.get("blocks"))
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| JobErr("frozen export blocks missing".into()))?;
    if blocks.len() > MAX_EXPORT_BLOCK_OCCURRENCES {
        return Err(JobErr("frozen export block count exceeds budget".into()));
    }
    let mut repeated_bytes = 0u64;
    let mut occurrence_pixels = 0u64;
    let mut table_rows = 0u64;
    let mut table_cells = 0u64;
    for block in blocks {
        let content = block.get("content").unwrap_or(&serde_json::Value::Null);
        if let Some(asset_id) = content
            .get("asset_revision_id")
            .and_then(serde_json::Value::as_str)
        {
            let (bytes, pixels) = asset_metadata
                .get(asset_id)
                .ok_or_else(|| JobErr("frozen block references missing asset metadata".into()))?;
            repeated_bytes = repeated_bytes
                .checked_add(*bytes)
                .ok_or_else(|| JobErr("frozen asset reference byte estimate overflow".into()))?;
            occurrence_pixels = occurrence_pixels
                .checked_add(*pixels)
                .ok_or_else(|| JobErr("frozen asset occurrence pixel estimate overflow".into()))?;
        }
        if content.get("type").and_then(serde_json::Value::as_str) == Some("table") {
            let rows = content
                .get("row_count")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| JobErr("frozen table row metadata missing".into()))?;
            let columns = content
                .get("column_count")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| JobErr("frozen table column metadata missing".into()))?;
            let declared_cells = rows
                .checked_mul(columns)
                .ok_or_else(|| JobErr("frozen table cell count overflow".into()))?;
            let cells = content
                .get("cells")
                .and_then(serde_json::Value::as_array)
                .ok_or_else(|| JobErr("frozen table cells missing".into()))?;
            if u64::try_from(cells.len()).map_err(|_| JobErr("table cell count overflow".into()))?
                > declared_cells
            {
                return Err(JobErr("frozen table cells exceed declared grid".into()));
            }
            for cell in cells {
                let row = cell
                    .get("row")
                    .and_then(serde_json::Value::as_u64)
                    .ok_or_else(|| JobErr("table row missing".into()))?;
                let column = cell
                    .get("column")
                    .and_then(serde_json::Value::as_u64)
                    .ok_or_else(|| JobErr("table column missing".into()))?;
                let rowspan = cell
                    .get("rowspan")
                    .and_then(serde_json::Value::as_u64)
                    .ok_or_else(|| JobErr("table rowspan missing".into()))?;
                let colspan = cell
                    .get("colspan")
                    .and_then(serde_json::Value::as_u64)
                    .ok_or_else(|| JobErr("table colspan missing".into()))?;
                if rowspan == 0
                    || colspan == 0
                    || row.checked_add(rowspan).is_none_or(|end| end > rows)
                    || column.checked_add(colspan).is_none_or(|end| end > columns)
                    || rowspan.checked_mul(colspan).is_none()
                {
                    return Err(JobErr("frozen table span metadata invalid".into()));
                }
            }
            table_rows = table_rows
                .checked_add(rows)
                .ok_or_else(|| JobErr("aggregate table row count overflow".into()))?;
            table_cells = table_cells
                .checked_add(declared_cells)
                .ok_or_else(|| JobErr("aggregate table cell count overflow".into()))?;
        }
    }
    let forms = input
        .get("form_definitions")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| JobErr("frozen form definitions missing".into()))?;
    let preparations = input
        .get("attachment_preparations")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| JobErr("frozen attachment preparations missing".into()))?;
    if forms.len() > MAX_EXPORT_FORM_DEFINITIONS
        || preparations.len() > MAX_EXPORT_ATTACHMENT_PREPARATIONS
        || table_rows > MAX_EXPORT_TABLE_ROWS
        || table_cells > MAX_EXPORT_TABLE_CELLS
        || occurrence_pixels > MAX_FROZEN_ASSET_TOTAL_PIXELS
    {
        return Err(JobErr("frozen export aggregate work exceeds budget".into()));
    }
    let mut prepared_pages = 0u64;
    let mut prepared_pixels = 0u64;
    for preparation in preparations {
        let pages = preparation
            .get("page_assets")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| JobErr("attachment preparation page metadata missing".into()))?;
        prepared_pages = prepared_pages
            .checked_add(
                u64::try_from(pages.len())
                    .map_err(|_| JobErr("prepared page count overflow".into()))?,
            )
            .ok_or_else(|| JobErr("prepared page count overflow".into()))?;
        for page in pages {
            let geometry = page
                .get("geometry")
                .ok_or_else(|| JobErr("prepared page geometry missing".into()))?;
            let width = geometry
                .get("width_px")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| JobErr("prepared page width missing".into()))?;
            let height = geometry
                .get("height_px")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| JobErr("prepared page height missing".into()))?;
            prepared_pixels = prepared_pixels
                .checked_add(
                    width
                        .checked_mul(height)
                        .ok_or_else(|| JobErr("prepared page pixels overflow".into()))?,
                )
                .ok_or_else(|| JobErr("prepared page pixels overflow".into()))?;
        }
    }
    let estimated_work = repeated_bytes
        .checked_add(
            prepared_pages
                .checked_mul(256 * 1024)
                .ok_or_else(|| JobErr("prepared page output estimate overflow".into()))?,
        )
        .and_then(|value| value.checked_add(table_cells.checked_mul(256)?))
        .ok_or_else(|| JobErr("final export work estimate overflow".into()))?;
    if prepared_pages > MAX_PDF_ATTACHMENT_PAGES as u64
        || prepared_pixels > MAX_FROZEN_ASSET_TOTAL_PIXELS
        || estimated_work > MAX_EXPORT_REFERENCE_WORK_BYTES
    {
        return Err(JobErr("frozen export aggregate work exceeds budget".into()));
    }
    Ok(())
}

async fn load_frozen_layout_assets(
    input: &serde_json::Value,
    cancel: &CancellationToken,
) -> Result<Vec<bidding::render_v2::FrozenLayoutAssetV2>, JobErr> {
    use sha2::{Digest, Sha256};
    let values = input
        .get("assets")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| JobErr("frozen export assets missing".into()))?;
    validate_frozen_asset_metadata(values)?;
    let mut assets = Vec::with_capacity(values.len());
    let mut decoded_pixels = 0u64;
    for value in values {
        let asset_revision_id = value
            .get("asset_revision_id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| JobErr("frozen asset revision identity missing".into()))?
            .to_owned();
        let sha256 = value
            .get("sha256")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| JobErr("frozen asset digest missing".into()))?
            .to_owned();
        let media_type = value
            .get("media_type")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| JobErr("frozen asset media type missing".into()))?
            .to_owned();
        let file_name = value
            .get("file_name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("资产")
            .to_owned();
        let bytes = if media_type.starts_with("image/") {
            read_blob_in_helper(&sha256, bidding::render_v2::MAX_FROZEN_IMAGE_BYTES, cancel)
                .await
                .map_err(|error| {
                    JobErr(format!("read frozen asset {asset_revision_id}: {error}"))
                })?
        } else {
            Vec::new()
        };
        if media_type.starts_with("image/") {
            if bytes.is_empty()
                || bytes.len() > bidding::render_v2::MAX_FROZEN_IMAGE_BYTES
                || hex::encode(Sha256::digest(&bytes)) != sha256
            {
                return Err(JobErr(format!(
                    "frozen image asset missing, oversized, or digest-mismatched: {asset_revision_id}"
                )));
            }
            let (width, height) =
                bidding::render_v2::frozen_image_dimensions(&bytes).map_err(JobErr)?;
            decoded_pixels = decoded_pixels
                .checked_add(u64::from(width) * u64::from(height))
                .ok_or_else(|| JobErr("decoded image pixel budget overflow".into()))?;
            if decoded_pixels > MAX_FROZEN_ASSET_TOTAL_PIXELS {
                return Err(JobErr(
                    "decoded images exceed aggregate pixel budget".into(),
                ));
            }
        }
        assets.push(bidding::render_v2::FrozenLayoutAssetV2 {
            asset_revision_id,
            sha256,
            media_type,
            file_name,
            bytes: Arc::new(bytes),
        });
    }
    Ok(assets)
}

async fn abort_task_until<T>(
    task: &mut tokio::task::JoinHandle<T>,
    deadline: tokio::time::Instant,
) {
    task.abort();
    let _ = tokio::time::timeout_at(deadline, &mut *task).await;
}

async fn kill_and_reap_child(
    child: &mut tokio::process::Child,
    label: &str,
    deadline: tokio::time::Instant,
) -> Result<(), JobErr> {
    let kill_error = child.start_kill().err();
    tokio::time::timeout_at(deadline, child.wait())
        .await
        .map_err(|_| JobErr(format!("reap {label} exceeded cleanup deadline")))?
        .map_err(|error| JobErr(format!("reap {label}: {error}")))?;
    if let Some(error) = kill_error
        && error.kind() != std::io::ErrorKind::InvalidInput
    {
        return Err(JobErr(format!("kill {label}: {error}")));
    }
    Ok(())
}

async fn kill_helper_group_and_reap_child(
    child: &mut tokio::process::Child,
    label: &str,
    deadline: tokio::time::Instant,
) -> Result<(), JobErr> {
    let Some(pid) = child.id() else {
        return kill_and_reap_child(child, label, deadline).await;
    };
    let group_result = i32::try_from(pid)
        .map_err(|_| JobErr(format!("{label} process id exceeds i32")))
        .and_then(|pid| {
            // SAFETY: `killpg` is called with a valid signal constant and the
            // checked process-group leader PID assigned at helper spawn.
            let result = unsafe { libc::killpg(pid, libc::SIGKILL) };
            if result == 0 {
                Ok(())
            } else {
                Err(JobErr(format!(
                    "kill process group for {label}: {}",
                    std::io::Error::last_os_error()
                )))
            }
        });
    if group_result.is_err() {
        child
            .start_kill()
            .map_err(|error| JobErr(format!("kill direct child for {label}: {error}")))?;
    }
    tokio::time::timeout_at(deadline, child.wait())
        .await
        .map_err(|_| JobErr(format!("reap {label} exceeded cleanup deadline")))?
        .map_err(|error| JobErr(format!("reap {label}: {error}")))?;
    Ok(())
}

async fn rasterize_pdf_pages(
    bytes: &[u8],
    expected_geometry: &[(u32, u32)],
    cancel: &CancellationToken,
) -> Result<Vec<Vec<u8>>, JobErr> {
    if bytes.is_empty() || bytes.len() > MAX_PDF_ATTACHMENT_BYTES {
        return Err(JobErr(
            "frozen PDF attachment exceeds the source byte budget".into(),
        ));
    }
    if expected_geometry.is_empty() || expected_geometry.len() > MAX_PDF_ATTACHMENT_PAGES {
        return Err(JobErr(
            "trusted PDF rasterizer expected page count is invalid".into(),
        ));
    }
    let geometry = serde_json::to_string(expected_geometry)
        .map_err(|error| JobErr(format!("serialize PDF geometry: {error}")))?;
    let framed = run_helper_capture(
        &[PDF_RASTER_HELPER_ARG.into(), geometry],
        bytes.to_vec(),
        MAX_RASTER_TOTAL_BYTES + 8 + expected_geometry.len() * 8,
        std::time::Duration::from_secs(RENDER_TIMEOUT_SECONDS),
        cancel,
        "PDF raster helper",
    )
    .await?;
    if framed.len() < 4 {
        return Err(JobErr("PDF raster helper framing is truncated".into()));
    }
    let page_count = u32::from_be_bytes(framed[0..4].try_into().unwrap()) as usize;
    if page_count != expected_geometry.len() {
        return Err(JobErr(
            "trusted PDF rasterizer returned an invalid page count".into(),
        ));
    }
    let mut offset = 4usize;
    let mut pages = Vec::with_capacity(page_count);
    let mut total_bytes = 0usize;
    for (index, expected) in expected_geometry.iter().enumerate() {
        let length_bytes = framed
            .get(offset..offset + 8)
            .ok_or_else(|| JobErr("PDF raster helper framing is truncated".into()))?;
        let length = usize::try_from(u64::from_be_bytes(length_bytes.try_into().unwrap()))
            .map_err(|_| JobErr("PDF raster page length overflows usize".into()))?;
        offset += 8;
        let page = framed
            .get(offset..offset + length)
            .ok_or_else(|| JobErr("PDF raster helper page is truncated".into()))?
            .to_vec();
        offset += length;
        total_bytes = total_bytes
            .checked_add(length)
            .ok_or_else(|| JobErr("rasterized PDF pages exceed byte budget".into()))?;
        if total_bytes > MAX_RASTER_TOTAL_BYTES {
            return Err(JobErr("rasterized PDF pages exceed byte budget".into()));
        }
        let actual = bidding::render_v2::frozen_image_dimensions(&page).map_err(JobErr)?;
        if actual != *expected {
            return Err(JobErr(format!(
                "trusted PDF rasterizer geometry mismatch at page {index}"
            )));
        }
        pages.push(page);
    }
    if offset != framed.len() {
        return Err(JobErr(
            "PDF raster helper framing has trailing bytes".into(),
        ));
    }
    Ok(pages)
}

async fn prepare_pdf_attachments(
    pool: &PgPool,
    job: &SubmissionExportJobV2,
    input: &serde_json::Value,
    cancel: &CancellationToken,
    cleanup_tracker: &platform::StagedObjectCleanupTracker,
) -> Result<(), JobErr> {
    use sha2::{Digest, Sha256};
    const ACTOR: &str = "system:submission-export-v2";
    let blocks = input
        .get("workspace")
        .and_then(|value| value.get("blocks"))
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| JobErr("frozen export blocks missing".into()))?;
    let assets = input
        .get("assets")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| JobErr("frozen export assets missing".into()))?;
    let preparations = input
        .get("attachment_preparations")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| JobErr("frozen attachment preparations missing".into()))?;
    validate_frozen_asset_metadata(assets)?;
    let mut handled = std::collections::HashSet::new();
    let mut raster_total_pages = 0u64;
    let mut raster_total_pixels = 0u64;
    let mut raster_total_bytes = 0usize;
    for preparation in preparations {
        let page_assets = preparation
            .get("page_assets")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| JobErr("attachment preparation page metadata missing".into()))?;
        raster_total_pages = raster_total_pages
            .checked_add(
                u64::try_from(page_assets.len())
                    .map_err(|_| JobErr("prepared page count overflow".into()))?,
            )
            .ok_or_else(|| JobErr("prepared page count overflow".into()))?;
        for page in page_assets {
            let geometry = page
                .get("geometry")
                .ok_or_else(|| JobErr("prepared page geometry missing".into()))?;
            let width = geometry
                .get("width_px")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| JobErr("prepared page width missing".into()))?;
            let height = geometry
                .get("height_px")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| JobErr("prepared page height missing".into()))?;
            raster_total_pixels = raster_total_pixels
                .checked_add(
                    width
                        .checked_mul(height)
                        .ok_or_else(|| JobErr("prepared page pixel overflow".into()))?,
                )
                .ok_or_else(|| JobErr("prepared page pixel overflow".into()))?;
        }
    }
    if raster_total_pages > MAX_PDF_ATTACHMENT_PAGES as u64
        || raster_total_pixels > MAX_FROZEN_ASSET_TOTAL_PIXELS
    {
        return Err(JobErr(
            "prepared PDF geometry exceeds aggregate budget".into(),
        ));
    }
    for block in blocks {
        let content = block.get("content").unwrap_or(&serde_json::Value::Null);
        if block.get("kind").and_then(serde_json::Value::as_str) != Some("attachment_ref")
            || content
                .get("render_mode")
                .and_then(serde_json::Value::as_str)
                != Some("embedded_pages")
        {
            continue;
        }
        let source_id = content
            .get("asset_revision_id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| JobErr("frozen PDF attachment identity missing".into()))?;
        if !handled.insert(source_id.to_owned())
            || preparations.iter().any(|value| {
                value
                    .get("source_asset_revision_id")
                    .and_then(serde_json::Value::as_str)
                    == Some(source_id)
            })
        {
            continue;
        }
        let asset = assets
            .iter()
            .find(|value| {
                value
                    .get("asset_revision_id")
                    .and_then(serde_json::Value::as_str)
                    == Some(source_id)
            })
            .ok_or_else(|| JobErr(format!("frozen PDF attachment asset {source_id} missing")))?;
        if asset.get("media_type").and_then(serde_json::Value::as_str) != Some("application/pdf") {
            continue;
        }
        let source_uuid = Uuid::parse_str(source_id)
            .map_err(|_| JobErr("frozen PDF attachment UUID invalid".into()))?;
        let source_sha = asset
            .get("sha256")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| JobErr("frozen PDF attachment digest missing".into()))?
            .to_owned();
        let source_length = asset
            .get("byte_length")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| JobErr("frozen PDF byte length metadata missing".into()))?;
        if source_length > MAX_PDF_ATTACHMENT_BYTES as u64 {
            return Err(JobErr(
                "frozen PDF attachment exceeds the source byte budget".into(),
            ));
        }
        let read_sha = source_sha.clone();
        let source_bytes = read_blob_in_helper(&read_sha, MAX_PDF_ATTACHMENT_BYTES, cancel)
            .await
            .map_err(|error| {
                error.0.strip_prefix("TRANSIENT_HANDLER:").map_or_else(
                    || JobErr(format!("read frozen PDF attachment: {error}")),
                    |message| JobErr(format!("TRANSIENT_HANDLER:{message}")),
                )
            })?;
        if u64::try_from(source_bytes.len()).ok() != Some(source_length)
            || hex::encode(Sha256::digest(&source_bytes)) != source_sha
        {
            return Err(JobErr(
                "frozen PDF attachment length or digest mismatch".into(),
            ));
        }
        let geometry =
            bidding::render_v2::frozen_pdf_raster_geometry(&source_bytes).map_err(JobErr)?;
        let declared_pages = asset
            .get("page_count")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| JobErr("frozen PDF page count metadata missing".into()))?;
        if u64::try_from(geometry.len()).ok() != Some(declared_pages) {
            return Err(JobErr("frozen PDF page count metadata mismatch".into()));
        }
        raster_total_pages = raster_total_pages
            .checked_add(declared_pages)
            .ok_or_else(|| JobErr("rasterized PDF page count overflow".into()))?;
        for (width, height) in &geometry {
            raster_total_pixels = raster_total_pixels
                .checked_add(u64::from(*width) * u64::from(*height))
                .ok_or_else(|| JobErr("rasterized PDF pixel budget overflow".into()))?;
        }
        if raster_total_pages > MAX_PDF_ATTACHMENT_PAGES as u64
            || raster_total_pixels > MAX_FROZEN_ASSET_TOTAL_PIXELS
        {
            return Err(JobErr(
                "rasterized PDF geometry exceeds aggregate budget".into(),
            ));
        }
        let pages = rasterize_pdf_pages(&source_bytes, &geometry, cancel).await?;
        let mut total_pixels = 0u64;
        for page in &pages {
            raster_total_bytes = raster_total_bytes
                .checked_add(page.len())
                .ok_or_else(|| JobErr("rasterized PDF aggregate bytes overflow".into()))?;
            if raster_total_bytes > MAX_RASTER_TOTAL_BYTES {
                return Err(JobErr(
                    "rasterized PDF pages exceed aggregate byte budget".into(),
                ));
            }
            let (width, height) =
                bidding::render_v2::frozen_image_dimensions(page).map_err(JobErr)?;
            total_pixels = total_pixels
                .checked_add(
                    u64::from(width)
                        .checked_mul(u64::from(height))
                        .ok_or_else(|| JobErr("rasterized PDF pixel budget overflow".into()))?,
                )
                .ok_or_else(|| JobErr("rasterized PDF pixel budget overflow".into()))?;
            if total_pixels > MAX_FROZEN_ASSET_TOTAL_PIXELS {
                return Err(JobErr(
                    "rasterized PDF pages exceed aggregate pixel budget".into(),
                ));
            }
            i64::try_from(page.len())
                .map_err(|_| JobErr("rasterized PDF page exceeds size limit".into()))?;
            i32::try_from(width)
                .map_err(|_| JobErr("rasterized PDF page width exceeds limit".into()))?;
            i32::try_from(height)
                .map_err(|_| JobErr("rasterized PDF page height exceeds limit".into()))?;
        }
        let preparation_id = Uuid::new_v4();
        let mut cleanup = cleanup_tracker.guard();
        let mut page_item_ids = Vec::with_capacity(pages.len());
        let mut staging_ids = Vec::with_capacity(pages.len());
        let mut object_refs = Vec::with_capacity(pages.len());
        let mut digests = Vec::with_capacity(pages.len());
        let mut media_types = Vec::with_capacity(pages.len());
        let mut byte_lengths = Vec::with_capacity(pages.len());
        let mut widths = Vec::with_capacity(pages.len());
        let mut heights = Vec::with_capacity(pages.len());
        for page in pages {
            let (width, height) = bidding::render_v2::frozen_image_dimensions(&page)
                .expect("raster page metadata was preflighted");
            let digest = hex::encode(Sha256::digest(&page));
            let staging_id = Uuid::new_v4();
            cleanup.register(staging_id);
            match stage_export_object(pool, staging_id, &digest, "image/png", &page, ACTOR).await {
                Ok(object_ref) => {
                    page_item_ids.push(Uuid::new_v4());
                    staging_ids.push(staging_id);
                    object_refs.push(object_ref);
                    digests.push(digest);
                    media_types.push("image/png".to_owned());
                    byte_lengths.push(
                        i64::try_from(page.len()).expect("raster page length was preflighted"),
                    );
                    widths.push(i32::try_from(width).expect("raster width was preflighted"));
                    heights.push(i32::try_from(height).expect("raster height was preflighted"));
                }
                Err(error) => {
                    for staged in &staging_ids {
                        if platform::schedule_object_upload_cleanup(*staged)
                            .await
                            .is_ok()
                        {
                            cleanup.disarm(*staged);
                        }
                    }
                    return Err(error);
                }
            }
        }
        let result = bidding::bid_authoring_v2::publish_pdf_attachment_preparation_v2(
            pool,
            bidding::bid_authoring_v2::PublishPdfAttachmentPreparationV2 {
                request_artifact_id: job.request.request_artifact_id,
                request_revision: job.request.request_revision,
                frozen_input_sha256: &job.request.frozen_input_sha256,
                source_asset_revision_id: source_uuid,
                preparation_id,
                page_item_ids: &page_item_ids,
                staging_ids: &staging_ids,
                object_refs: &object_refs,
                content_sha256s: &digests,
                media_types: &media_types,
                byte_lengths: &byte_lengths,
                widths_px: &widths,
                heights_px: &heights,
            },
        )
        .await;
        match result {
            Ok(value) => {
                if value.get("replayed").and_then(serde_json::Value::as_bool) == Some(true) {
                    for staged in &staging_ids {
                        if platform::schedule_object_upload_cleanup(*staged)
                            .await
                            .is_ok()
                        {
                            cleanup.disarm(*staged);
                        }
                    }
                } else {
                    cleanup.disarm_all();
                }
            }
            Err(error) => {
                for staged in &staging_ids {
                    if platform::schedule_object_upload_cleanup(*staged)
                        .await
                        .is_ok()
                    {
                        cleanup.disarm(*staged);
                    }
                }
                return Err(non_agent_sql_error(error));
            }
        }
    }
    Ok(())
}

pub fn run_submission_render_helper(arguments: &[String]) -> Result<(), String> {
    use std::io::{Read, Write};
    if arguments.first().map(String::as_str) != Some(SUBMISSION_RENDER_HELPER_ARG) {
        return Err("invalid submission render helper arguments".into());
    }
    #[cfg(debug_assertions)]
    if let Some(delay) = std::env::var("KNOWLEDGEBRAIN_TEST_RENDER_HELPER_DELAY_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
    {
        std::thread::sleep(std::time::Duration::from_millis(delay));
    }
    // The four-argument form is retained only as a direct lifecycle test seam;
    // production uses bounded stdin/stdout so the parent performs no filesystem I/O.
    if arguments.len() == 4 {
        let input_path = std::path::Path::new(&arguments[1]);
        let output_path = std::path::Path::new(&arguments[2]);
        let format = arguments[3].as_str();
        let input = std::fs::read(input_path).map_err(|error| error.to_string())?;
        let rendered = render_submission_helper_bytes(&input, format)?;
        std::fs::write(output_path, rendered).map_err(|error| error.to_string())?;
        return Ok(());
    }
    if arguments.len() != 2 {
        return Err("invalid submission render helper arguments".into());
    }
    let mut input = Vec::new();
    std::io::stdin()
        .take((MAX_EXPORT_INPUT_BYTES + 1) as u64)
        .read_to_end(&mut input)
        .map_err(|error| error.to_string())?;
    if input.len() > MAX_EXPORT_INPUT_BYTES {
        return Err("submission render helper input exceeds budget".into());
    }
    let rendered = render_submission_helper_bytes(&input, &arguments[1])?;
    let mut stdout = std::io::stdout().lock();
    stdout
        .write_all(&rendered)
        .map_err(|error| error.to_string())?;
    stdout.flush().map_err(|error| error.to_string())
}

fn render_submission_helper_bytes(input: &[u8], format: &str) -> Result<Vec<u8>, String> {
    if input.is_empty() || input.len() > MAX_EXPORT_INPUT_BYTES {
        return Err("submission render helper input exceeds budget".into());
    }
    if !matches!(format, "docx" | "pdf") {
        return Err("invalid submission render helper format".into());
    }
    let layout: bidding::render_v2::LayoutDocumentV2 =
        serde_json::from_slice(input).map_err(|error| error.to_string())?;
    let rendered = match format {
        "docx" => bidding::render_v2::render_docx(&layout),
        "pdf" => bidding::render_v2::render_pdf(&layout),
        _ => unreachable!(),
    }?;
    if rendered.is_empty() || rendered.len() > MAX_RENDER_OUTPUT_BYTES {
        return Err("submission render helper output exceeds budget".into());
    }
    Ok(rendered)
}

pub fn run_pdf_raster_helper(arguments: &[String]) -> Result<(), String> {
    use std::io::{Read, Write};
    if arguments.len() != 2 || arguments[0] != PDF_RASTER_HELPER_ARG {
        return Err("invalid PDF raster helper arguments".into());
    }
    let geometry: Vec<(u32, u32)> =
        serde_json::from_str(&arguments[1]).map_err(|error| error.to_string())?;
    if geometry.is_empty() || geometry.len() > MAX_PDF_ATTACHMENT_PAGES {
        return Err("invalid PDF raster helper geometry".into());
    }
    let mut pdf = Vec::new();
    std::io::stdin()
        .take((MAX_PDF_ATTACHMENT_BYTES + 1) as u64)
        .read_to_end(&mut pdf)
        .map_err(|error| error.to_string())?;
    if pdf.is_empty() || pdf.len() > MAX_PDF_ATTACHMENT_BYTES {
        return Err("PDF raster helper input exceeds budget".into());
    }
    struct WorkDirectory(std::path::PathBuf);
    impl Drop for WorkDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let path = std::env::temp_dir().join(format!("kb-pdf-raster-helper-{}", Uuid::new_v4()));
    std::fs::create_dir(&path).map_err(|error| error.to_string())?;
    std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    let directory = WorkDirectory(path);
    let input_path = directory.0.join("source.pdf");
    let output_prefix = directory.0.join("page");
    std::fs::write(&input_path, pdf).map_err(|error| error.to_string())?;
    let status = std::process::Command::new("pdftoppm")
        .arg("-png")
        .arg("-cropbox")
        .arg("-r")
        .arg("144")
        .arg("-f")
        .arg("1")
        .arg("-l")
        .arg((MAX_PDF_ATTACHMENT_PAGES + 1).to_string())
        .arg(&input_path)
        .arg(&output_prefix)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|error| error.to_string())?;
    if !status.success() {
        return Err("trusted PDF rasterizer failed".into());
    }
    let mut paths = std::fs::read_dir(&directory.0)
        .map_err(|error| error.to_string())?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("png"))
        .collect::<Vec<_>>();
    paths.sort_by_key(|path| {
        path.file_stem()
            .and_then(|value| value.to_str())
            .and_then(|value| value.rsplit('-').next())
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(u32::MAX)
    });
    if paths.len() != geometry.len() {
        return Err("trusted PDF rasterizer returned an invalid page count".into());
    }
    let mut pages = Vec::with_capacity(paths.len());
    let mut total = 0usize;
    for (path, expected) in paths.into_iter().zip(geometry) {
        let page = std::fs::read(path).map_err(|error| error.to_string())?;
        total = total
            .checked_add(page.len())
            .ok_or_else(|| "PDF raster output byte count overflow".to_string())?;
        if total > MAX_RASTER_TOTAL_BYTES {
            return Err("PDF raster output exceeds budget".into());
        }
        if bidding::render_v2::frozen_image_dimensions(&page)? != expected {
            return Err("trusted PDF rasterizer geometry mismatch".into());
        }
        pages.push(page);
    }
    let mut stdout = std::io::stdout().lock();
    stdout
        .write_all(&(pages.len() as u32).to_be_bytes())
        .map_err(|error| error.to_string())?;
    for page in pages {
        stdout
            .write_all(&(page.len() as u64).to_be_bytes())
            .and_then(|_| stdout.write_all(&page))
            .map_err(|error| error.to_string())?;
    }
    stdout.flush().map_err(|error| error.to_string())
}

const OBJECT_READ_HELPER_ARG: &str = "--kb-object-read-helper-v1";

pub fn run_object_read_helper(arguments: &[String]) -> Result<(), String> {
    use std::io::Write;
    if arguments.len() != 3 || arguments[0] != OBJECT_READ_HELPER_ARG {
        return Err("invalid object read helper arguments".into());
    }
    let digest = &arguments[1];
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err("invalid object read digest".into());
    }
    let max_bytes = arguments[2]
        .parse::<usize>()
        .map_err(|_| "invalid object read byte budget".to_string())?;
    let bytes = platform::read_blob(digest).map_err(|error| error.to_string())?;
    if bytes.is_empty() || bytes.len() > max_bytes {
        return Err("object read helper output exceeds budget".into());
    }
    let mut stdout = std::io::stdout().lock();
    stdout
        .write_all(&bytes)
        .map_err(|error| error.to_string())?;
    stdout.flush().map_err(|error| error.to_string())
}

const OBJECT_WRITE_HELPER_ARG: &str = "--kb-object-write-helper-v1";

pub fn run_object_write_helper(arguments: &[String]) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    use std::io::{Read, Write};
    if arguments.len() != 3 || arguments[0] != OBJECT_WRITE_HELPER_ARG {
        return Err("invalid object write arguments".into());
    }
    let digest = &arguments[1];
    let length = arguments[2]
        .parse::<usize>()
        .map_err(|_| "invalid object write length")?;
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        || length == 0
        || length == usize::MAX
    {
        return Err("invalid object write identity".into());
    }
    let mut bytes = Vec::new();
    std::io::stdin()
        .lock()
        .take(length as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() != length || hex::encode(Sha256::digest(&bytes)) != *digest {
        return Err("object write digest or length mismatch".into());
    }
    platform::write_blob_off_runtime(digest, &bytes).map_err(|e| e.to_string())?;
    std::io::stdout()
        .lock()
        .write_all(b"ok")
        .map_err(|e| e.to_string())
}

struct HelperCompositionObjects;
#[async_trait]
impl bidding::docx_composition::runtime::ObjectIo for HelperCompositionObjects {
    async fn read(
        &self,
        sha: &str,
        max_bytes: usize,
        cancel: &CancellationToken,
    ) -> Result<Vec<u8>, bidding::agent_error::AgentError> {
        read_blob_in_helper(sha, max_bytes, cancel)
            .await
            .map_err(|e| bidding::agent_error::AgentError::new("INTERNAL", e.0))
    }
    async fn write(
        &self,
        sha: &str,
        bytes: &[u8],
        cancel: &CancellationToken,
    ) -> Result<(), bidding::agent_error::AgentError> {
        run_helper_capture(
            &[
                OBJECT_WRITE_HELPER_ARG.into(),
                sha.into(),
                bytes.len().to_string(),
            ],
            bytes.to_vec(),
            2,
            std::time::Duration::from_secs(5 * 60),
            cancel,
            "object write helper",
        )
        .await
        .map(|_| ())
        .map_err(|e| bidding::agent_error::AgentError::new("INTERNAL", e.0))
    }
}

async fn run_helper_capture(
    arguments: &[String],
    input: Vec<u8>,
    max_output_bytes: usize,
    timeout: std::time::Duration,
    cancel: &CancellationToken,
    label: &str,
) -> Result<Vec<u8>, JobErr> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let executable = std::env::current_exe()
        .map_err(|error| JobErr(format!("resolve {label} executable: {error}")))?;
    let mut command = tokio::process::Command::new(executable);
    command
        .args(arguments)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    command.process_group(0);
    let mut child = command
        .spawn()
        .map_err(|error| JobErr(format!("start {label}: {error}")))?;
    let deadline = tokio::time::Instant::now() + timeout;
    let work_deadline = deadline
        .checked_sub(TASK_ABORT_DRAIN_RESERVE)
        .unwrap_or(deadline);
    let mut stdin = match child.stdin.take() {
        Some(stdin) => stdin,
        None => {
            kill_helper_group_and_reap_child(&mut child, &format!("invalid {label}"), deadline)
                .await?;
            return Err(JobErr(format!("{label} stdin unavailable")));
        }
    };
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            kill_helper_group_and_reap_child(&mut child, &format!("invalid {label}"), deadline)
                .await?;
            return Err(JobErr(format!("{label} stdout unavailable")));
        }
    };
    let mut writer = tokio::spawn(async move {
        stdin.write_all(&input).await?;
        stdin.shutdown().await
    });
    let mut reader = tokio::spawn(async move {
        let mut output = Vec::new();
        stdout
            .take((max_output_bytes + 1) as u64)
            .read_to_end(&mut output)
            .await?;
        Ok::<_, std::io::Error>(output)
    });
    let output = tokio::select! {
        biased;
        () = cancel.cancelled() => {
            kill_helper_group_and_reap_child(&mut child, &format!("cancelled {label}"), deadline).await?;
            abort_task_until(&mut writer, deadline).await;
            abort_task_until(&mut reader, deadline).await;
            return Err(JobErr("WORKER_SHUTDOWN".into()));
        }
        () = tokio::time::sleep_until(work_deadline) => {
            kill_helper_group_and_reap_child(&mut child, &format!("timed out {label}"), deadline).await?;
            abort_task_until(&mut writer, deadline).await;
            abort_task_until(&mut reader, deadline).await;
            return Err(JobErr(format!("{label} timed out")));
        }
        joined = &mut reader => match joined {
            Ok(Ok(output)) => Ok(output),
            Ok(Err(error)) => Err(JobErr(format!("read {label} output: {error}"))),
            Err(error) => Err(JobErr(format!("join {label} output: {error}"))),
        },
    };
    let output = match output {
        Ok(output) => output,
        Err(error) => {
            let _ =
                kill_helper_group_and_reap_child(&mut child, &format!("failed {label}"), deadline)
                    .await;
            abort_task_until(&mut writer, deadline).await;
            return Err(error);
        }
    };
    if output.len() > max_output_bytes {
        kill_helper_group_and_reap_child(&mut child, &format!("oversized {label}"), deadline)
            .await?;
        abort_task_until(&mut writer, deadline).await;
        return Err(JobErr(format!("{label} output exceeds budget")));
    }
    let writer_result = tokio::select! {
        biased;
        () = cancel.cancelled() => {
            kill_helper_group_and_reap_child(&mut child, &format!("cancelled {label}"), deadline).await?;
            abort_task_until(&mut writer, deadline).await;
            return Err(JobErr("WORKER_SHUTDOWN".into()));
        }
        () = tokio::time::sleep_until(work_deadline) => {
            kill_helper_group_and_reap_child(&mut child, &format!("timed out {label}"), deadline).await?;
            abort_task_until(&mut writer, deadline).await;
            return Err(JobErr(format!("{label} timed out")));
        }
        joined = &mut writer => match joined {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(JobErr(format!("write {label} input: {error}"))),
            Err(error) => Err(JobErr(format!("join {label} input: {error}"))),
        },
    };
    if let Err(error) = writer_result {
        let _ = kill_helper_group_and_reap_child(&mut child, &format!("failed {label}"), deadline)
            .await;
        return Err(error);
    }
    let status = tokio::select! {
        biased;
        () = cancel.cancelled() => {
            kill_helper_group_and_reap_child(&mut child, &format!("cancelled {label}"), deadline).await?;
            return Err(JobErr("WORKER_SHUTDOWN".into()));
        }
        () = tokio::time::sleep_until(work_deadline) => {
            kill_helper_group_and_reap_child(&mut child, &format!("timed out {label}"), deadline).await?;
            return Err(JobErr(format!("{label} timed out")));
        }
        status = child.wait() => status.map_err(|error| JobErr(format!("wait {label}: {error}"))),
    };
    let status = match status {
        Ok(status) => status,
        Err(error) => {
            let _ =
                kill_helper_group_and_reap_child(&mut child, &format!("failed {label}"), deadline)
                    .await;
            return Err(error);
        }
    };
    if !status.success() {
        return Err(JobErr(format!("{label} failed")));
    }
    if output.is_empty() {
        return Err(JobErr(format!("{label} returned no bytes")));
    }
    Ok(output)
}

async fn read_blob_in_helper(
    digest: &str,
    max_bytes: usize,
    cancel: &CancellationToken,
) -> Result<Vec<u8>, JobErr> {
    let result = run_helper_capture(
        &[
            OBJECT_READ_HELPER_ARG.into(),
            digest.into(),
            max_bytes.to_string(),
        ],
        Vec::new(),
        max_bytes,
        std::time::Duration::from_secs(5 * 60),
        cancel,
        "object read helper",
    )
    .await;
    result.map_err(|error| {
        if error.0.contains("exceeds budget") {
            error
        } else {
            JobErr(format!("TRANSIENT_HANDLER:{}", error.0))
        }
    })
}

async fn render_submission_in_helper(
    layout: bidding::render_v2::LayoutDocumentV2,
    format: &str,
    cancel: &CancellationToken,
) -> Result<(Vec<u8>, &'static str), JobErr> {
    let input = serde_json::to_vec(&layout)
        .map_err(|error| JobErr(format!("serialize render helper input: {error}")))?;
    if input.is_empty() || input.len() > MAX_EXPORT_INPUT_BYTES {
        return Err(JobErr("render helper input exceeds budget".into()));
    }
    let bytes = run_helper_capture(
        &[SUBMISSION_RENDER_HELPER_ARG.into(), format.into()],
        input,
        MAX_RENDER_OUTPUT_BYTES,
        std::time::Duration::from_secs(RENDER_TIMEOUT_SECONDS),
        cancel,
        "render helper",
    )
    .await?;
    let media_type = if format == "docx" {
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
    } else {
        "application/pdf"
    };
    Ok((bytes, media_type))
}

async fn process_submission_export_v2(
    pool: &PgPool,
    job: &SubmissionExportJobV2,
    cancel: CancellationToken,
    cleanup_tracker: platform::StagedObjectCleanupTracker,
) -> Result<(), JobErr> {
    use sha2::{Digest, Sha256};
    const ACTOR: &str = "system:submission-export-v2";
    let preflight_input = bidding::bid_authoring_v2::load_submission_export_input_v2(
        pool,
        job.request.request_artifact_id,
        job.request.request_revision,
        &job.request.frozen_input_sha256,
    )
    .await
    .map_err(non_agent_sql_error)?;
    if serde_json::to_vec(&preflight_input)
        .map_err(|error| JobErr(format!("serialize frozen export input: {error}")))?
        .len()
        > MAX_EXPORT_INPUT_BYTES
    {
        return Err(JobErr("frozen export input exceeds the byte budget".into()));
    }
    validate_submission_export_metadata(&preflight_input)?;
    prepare_pdf_attachments(pool, job, &preflight_input, &cancel, &cleanup_tracker)
        .await
        .map_err(|error| {
            error.0.strip_prefix("TRANSIENT_HANDLER:").map_or_else(
                || JobErr(format!("ATTACHMENT_PREPARATION_FAILED: {}", error.0)),
                |message| JobErr(format!("TRANSIENT_HANDLER:{message}")),
            )
        })?;
    let font_digest = hex::encode(Sha256::digest(bidding::render_v2::PDF_FONT_BYTES));
    let font_staging_id = Uuid::new_v4();
    let mut font_cleanup = cleanup_tracker.guard();
    font_cleanup.register(font_staging_id);
    let font_ref = stage_export_object(
        pool,
        font_staging_id,
        &font_digest,
        "font/otf",
        bidding::render_v2::PDF_FONT_BYTES,
        ACTOR,
    )
    .await?;
    let prepared = match bidding::bid_authoring_v2::prepare_submission_export_v2(
        pool,
        &job.request,
        bidding::bid_authoring_v2::SubmissionExportFontV2 {
            staging_id: font_staging_id,
            object_ref: &font_ref,
            sha256: &font_digest,
            media_type: "font/otf",
        },
        Uuid::new_v4(),
        Uuid::new_v4(),
        ACTOR,
    )
    .await
    {
        Ok(value) => value,
        Err(error) => {
            if platform::schedule_object_upload_cleanup(font_staging_id)
                .await
                .is_ok()
            {
                font_cleanup.disarm(font_staging_id);
            }
            return Err(non_agent_sql_error(error));
        }
    };
    if prepared
        .get("replayed")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
        || prepared.get("render_snapshot_sha256").is_none()
    {
        if platform::schedule_object_upload_cleanup(font_staging_id)
            .await
            .is_ok()
        {
            font_cleanup.disarm(font_staging_id);
        }
    } else {
        font_cleanup.disarm(font_staging_id);
    }
    if prepared.get("render_snapshot_sha256").is_none() {
        return Ok(());
    }
    let manifest_id = prepared
        .get("artifact_id")
        .and_then(serde_json::Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or_else(|| JobErr("prepared manifest identity missing".into()))?;
    let manifest_sha = prepared
        .get("sha256")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| JobErr("prepared manifest digest missing".into()))?;
    let snapshot_id = prepared
        .get("render_snapshot_id")
        .and_then(serde_json::Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or_else(|| JobErr("prepared render snapshot identity missing".into()))?;
    let input = bidding::bid_authoring_v2::load_submission_manifest_render_input_v2(
        pool,
        manifest_id,
        manifest_sha,
    )
    .await
    .map_err(non_agent_sql_error)?;
    if serde_json::to_vec(&input)
        .map_err(|error| JobErr(format!("serialize frozen export input: {error}")))?
        .len()
        > MAX_EXPORT_INPUT_BYTES
    {
        return Err(JobErr("frozen export input exceeds the byte budget".into()));
    }
    validate_submission_export_metadata(&input)?;
    let request = input
        .get("request")
        .ok_or_else(|| JobErr("export request identity missing".into()))?;
    let workspace = input
        .get("workspace")
        .ok_or_else(|| JobErr("frozen export workspace missing".into()))?;
    let title = input
        .get("project_title")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("投标文件");
    let watermark = request
        .get("mode_options")
        .and_then(|value| value.get("watermark"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let assets = load_frozen_layout_assets(&input, &cancel).await?;
    let forms = input
        .get("form_definitions")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| JobErr("frozen form definitions missing".into()))?;
    let preparations = input
        .get("attachment_preparations")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| JobErr("frozen attachment preparations missing".into()))?;
    let layout = bidding::render_v2::layout_from_workspace_with_resources(
        title,
        workspace,
        &assets,
        forms,
        preparations,
        watermark,
    )
    .map_err(JobErr)?;
    let format = request
        .get("format")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| JobErr("frozen export format invalid".into()))?
        .to_owned();
    let rendered = render_submission_in_helper(layout, &format, &cancel).await?;
    let (bytes, media_type) = rendered;
    if bytes.is_empty() || bytes.len() > MAX_RENDER_OUTPUT_BYTES {
        return Err(JobErr("rendered output exceeds the byte budget".into()));
    }
    let output_digest = hex::encode(Sha256::digest(&bytes));
    let output_staging_id = Uuid::new_v4();
    let mut output_cleanup = cleanup_tracker.guard();
    output_cleanup.register(output_staging_id);
    let output_ref = stage_export_object(
        pool,
        output_staging_id,
        &output_digest,
        media_type,
        &bytes,
        ACTOR,
    )
    .await?;
    let output_id = Uuid::new_v4();
    let result = bidding::bid_authoring_v2::publish_submission_export_v2(
        pool,
        &job.request,
        bidding::bid_authoring_v2::SubmissionExportFontV2 {
            staging_id: font_staging_id,
            object_ref: &font_ref,
            sha256: &font_digest,
            media_type: "font/otf",
        },
        snapshot_id,
        manifest_id,
        bidding::bid_authoring_v2::SubmissionExportOutputV2 {
            staging_id: output_staging_id,
            artifact_id: output_id,
            object_ref: &output_ref,
            sha256: &output_digest,
            media_type,
            byte_length: i64::try_from(bytes.len())
                .map_err(|_| JobErr("rendered object too large".into()))?,
        },
        ACTOR,
    )
    .await;
    match result {
        Err(error) => {
            if platform::schedule_object_upload_cleanup(output_staging_id)
                .await
                .is_ok()
            {
                output_cleanup.disarm(output_staging_id);
            }
            Err(non_agent_sql_error(error))
        }
        Ok(identity) => {
            let persisted_output_id = identity
                .get("artifact_id")
                .and_then(serde_json::Value::as_str)
                .and_then(|raw| Uuid::parse_str(raw).ok());
            if persisted_output_id == Some(output_id)
                || platform::schedule_object_upload_cleanup(output_staging_id)
                    .await
                    .is_ok()
            {
                output_cleanup.disarm(output_staging_id);
            }
            Ok(())
        }
    }
}

#[async_trait]
impl oxana::Worker<SubmissionExportJobV2> for SubmissionExportV2Worker {
    type Error = JobErr;
    fn max_retries(&self, _job: &SubmissionExportJobV2) -> u32 {
        platform::BID_AUTHORING_V2_MAX_RETRIES
    }
    fn retry_delay(&self, _job: &SubmissionExportJobV2, retries: u32) -> u64 {
        platform::BidAuthoringV2OxanaPolicy::retry_delay_seconds(retries)
    }
    async fn process(
        &self,
        job: SubmissionExportJobV2,
        _ctx: &oxana::JobContext,
    ) -> Result<(), Self::Error> {
        let deadline = HandlerDeadline::from_now(SUBMISSION_EXPORT_HANDLER_HARD_TIMEOUT);
        let Some(pool) = &self.pool else {
            return Err(JobErr("postgres not configured".into()));
        };
        let cleanup =
            platform::StagedObjectCleanupTracker::new(pool, "system:submission-export-v2");
        let local_cancel = CancellationToken::new();
        let pipeline_pool = pool.clone();
        let pipeline_job = job.clone();
        let pipeline_cancel = local_cancel.clone();
        let pipeline_cleanup = cleanup.clone();
        let run = run_owned_handler(
            async move {
                if bid_request_is_terminal(&pipeline_pool, pipeline_job.request.request_artifact_id)
                    .await?
                {
                    return Ok(());
                }
                process_submission_export_v2(
                    &pipeline_pool,
                    &pipeline_job,
                    pipeline_cancel,
                    pipeline_cleanup,
                )
                .await
            },
            deadline,
            self.shutdown.clone(),
            local_cancel,
            Some(&cleanup),
            |error| !error.0.starts_with("TRANSIENT_HANDLER:"),
        )
        .await;
        let cleanup_deadline = run.cleanup_deadline;
        let cleanup_error = run.cleanup_error;
        let result = match run.completion {
            OwnedHandlerCompletion::ShuttingDown => {
                return Err(JobErr(cleanup_error.map_or_else(
                    || "WORKER_SHUTDOWN".into(),
                    |error| format!("WORKER_SHUTDOWN; cleanup failed: {error}"),
                )));
            }
            OwnedHandlerCompletion::TimedOut => {
                terminalize_non_agent_failure_until(
                    pool,
                    &job.request,
                    NonAgentTerminalFailure::SubmissionExport("SUBMISSION_EXPORT_TIMEOUT"),
                    cleanup_deadline,
                    "submission export timeout",
                )
                .await?;
                if let Some(error) = cleanup_tracker_until(Some(&cleanup), cleanup_deadline).await {
                    tracing::warn!(%error, "submission export timed out and terminalized; cleanup remains pending");
                }
                return Ok(());
            }
            OwnedHandlerCompletion::Completed(result) => result,
        };
        if let Err(error) = &result {
            if let Some(message) = error.0.strip_prefix("TRANSIENT_HANDLER:") {
                return Err(JobErr(message.to_owned()));
            }
            let error_code = if error.0.starts_with("ATTACHMENT_PREPARATION_FAILED:") {
                "ATTACHMENT_PREPARATION_FAILED"
            } else {
                "RENDERER_FAILED"
            };
            let already_terminal = terminalize_non_agent_failure_until(
                pool,
                &job.request,
                NonAgentTerminalFailure::SubmissionExport(error_code),
                cleanup_deadline,
                "submission export failure",
            )
            .await
            .map_err(|failure| JobErr(format!("submission export failed ({error}); {failure}")))?;
            if already_terminal {
                return cleanup_error.map_or(Ok(()), |cleanup| {
                    Err(JobErr(format!("terminal export cleanup failed: {cleanup}")))
                });
            }
            return cleanup_error.map_or(Ok(()), |cleanup| {
                Err(JobErr(format!(
                    "submission export terminalized after {error}; cleanup failed: {cleanup}"
                )))
            });
        }
        match (result, cleanup_error) {
            (Ok(()), Some(error)) => Err(JobErr(format!(
                "submission export completed but cleanup failed: {error}"
            ))),
            (result, _) => result,
        }
    }
}

pub struct DocumentProcessWorker {
    pool: Option<PgPool>,
}

impl oxana::FromContext<AppCtx> for DocumentProcessWorker {
    fn from_context(ctx: &AppCtx) -> Self {
        Self {
            pool: ctx.pool.clone(),
        }
    }
}

#[derive(Debug)]
pub struct JobErr(String);

impl std::fmt::Display for JobErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for JobErr {}

fn non_agent_sql_error(error: sqlx::Error) -> JobErr {
    let deterministic = match &error {
        sqlx::Error::RowNotFound
        | sqlx::Error::ColumnIndexOutOfBounds { .. }
        | sqlx::Error::ColumnNotFound(_)
        | sqlx::Error::ColumnDecode { .. }
        | sqlx::Error::Decode(_)
        | sqlx::Error::TypeNotFound { .. } => true,
        sqlx::Error::Database(database) => database.code().is_some_and(|code| {
            code.starts_with("22")
                || code.starts_with("23")
                || matches!(code.as_ref(), "P0001" | "P0002")
        }),
        _ => false,
    };
    if deterministic {
        JobErr(error.to_string())
    } else {
        JobErr(format!("TRANSIENT_HANDLER:{error}"))
    }
}

async fn bid_request_is_terminal(pool: &PgPool, request_artifact_id: Uuid) -> Result<bool, JobErr> {
    let status = bidding::bid_authoring_v2::async_request_status_v2(pool, request_artifact_id)
        .await
        .map_err(non_agent_sql_error)?;
    Ok(matches!(status.as_deref(), Some("succeeded" | "failed")))
}

async fn require_bid_request_terminal(
    pool: &PgPool,
    request_artifact_id: Uuid,
    operation: &str,
) -> Result<(), JobErr> {
    if bid_request_is_terminal(pool, request_artifact_id).await? {
        Ok(())
    } else {
        Err(JobErr(format!(
            "{operation} terminal transition returned without settling the request"
        )))
    }
}

enum NonAgentTerminalFailure<'a> {
    SubmissionExport(&'a str),
    TenderDocument(&'a str),
}

async fn terminalize_non_agent_failure_until(
    pool: &PgPool,
    request: &platform::BidAuthoringRequestIdentityV2,
    failure: NonAgentTerminalFailure<'_>,
    cleanup_deadline: tokio::time::Instant,
    label: &str,
) -> Result<bool, JobErr> {
    terminalize_until(cleanup_deadline, label, async {
        if bid_request_is_terminal(pool, request.request_artifact_id).await? {
            return Ok(true);
        }
        let result = match failure {
            NonAgentTerminalFailure::SubmissionExport(code) => {
                bidding::bid_authoring_v2::mark_submission_export_failed_v2(
                    pool,
                    request.request_artifact_id,
                    request.request_revision,
                    &request.frozen_input_sha256,
                    code,
                )
                .await
            }
            NonAgentTerminalFailure::TenderDocument(code) => {
                bidding::bid_authoring_v2::mark_tender_document_failed_v2(
                    pool,
                    request.request_artifact_id,
                    request.request_revision,
                    &request.frozen_input_sha256,
                    code,
                )
                .await
            }
        };
        result.map_err(|error| JobErr(format!("{label}: terminal transition failed: {error}")))?;
        require_bid_request_terminal(pool, request.request_artifact_id, label).await?;
        Ok(false)
    })
    .await
}

struct HelperTenderObjectReader;

#[async_trait]
impl bidding::tender_process::TenderObjectReader for HelperTenderObjectReader {
    async fn read(
        &self,
        sha256: &str,
        cancel: &CancellationToken,
    ) -> Result<Vec<u8>, bidding::tender_process::TenderDocumentProcessError> {
        read_blob_in_helper(sha256, MAX_TENDER_DOCUMENT_BYTES, cancel)
            .await
            .map_err(|error| {
                bidding::tender_process::TenderDocumentProcessError::Unavailable(error.to_string())
            })
    }
}

pub struct TenderDocumentProcessV2Worker {
    pool: Option<PgPool>,
    shutdown: CancellationToken,
}

impl oxana::FromContext<AppCtx> for TenderDocumentProcessV2Worker {
    fn from_context(ctx: &AppCtx) -> Self {
        Self {
            pool: ctx.pool.clone(),
            shutdown: ctx.shutdown.clone(),
        }
    }
}

#[async_trait]
impl oxana::Worker<TenderDocumentProcessJobV2> for TenderDocumentProcessV2Worker {
    type Error = JobErr;

    fn max_retries(&self, _job: &TenderDocumentProcessJobV2) -> u32 {
        platform::BID_AUTHORING_V2_MAX_RETRIES
    }

    fn retry_delay(&self, _job: &TenderDocumentProcessJobV2, retries: u32) -> u64 {
        platform::BidAuthoringV2OxanaPolicy::retry_delay_seconds(retries)
    }

    async fn process(
        &self,
        job: TenderDocumentProcessJobV2,
        _ctx: &oxana::JobContext,
    ) -> Result<(), Self::Error> {
        let deadline = HandlerDeadline::from_now(TENDER_HANDLER_HARD_TIMEOUT);
        let Some(pool) = &self.pool else {
            return Err(JobErr("postgres not configured".into()));
        };
        let request_artifact_id = job.request.request_artifact_id;
        let payload = BidAuthoringJobPayloadV2::TenderDocumentProcess {
            request: job.request.clone(),
            project_id: job.project_id,
            document_revision_id: job.document_revision_id,
        };
        let cleanup =
            platform::StagedObjectCleanupTracker::new(pool, "system:tender-document-process-v2");
        let service = bidding::tender_process::TenderDocumentProcessService::new(
            bidding::tender_process::PgTenderDocumentProcessRepository::with_cleanup_tracker(
                pool.clone(),
                cleanup.clone(),
                std::sync::Arc::new(HelperTenderObjectReader),
            ),
            bidding::tender_process::DocReaderGrpcTenderSourceConverter,
            bidding::tender_process::ExistingTenderVisionEnricher,
            bidding::tender_process::InactiveTenderProcessTransport,
        );
        let local_cancel = CancellationToken::new();
        let pipeline_cancel = local_cancel.clone();
        let pipeline_pool = pool.clone();
        let run = run_owned_handler(
            async move {
                if bid_request_is_terminal(&pipeline_pool, request_artifact_id).await? {
                    return Ok(());
                }
                service
                    .process(&payload, &pipeline_cancel)
                    .await
                    .map(|_| ())
                    .map_err(|error| match error {
                        bidding::tender_process::TenderDocumentProcessError::Unavailable(
                            message,
                        ) => JobErr(format!("TRANSIENT_HANDLER:{message}")),
                        error => JobErr(error.to_string()),
                    })
            },
            deadline,
            self.shutdown.clone(),
            local_cancel,
            Some(&cleanup),
            |error| !error.0.starts_with("TRANSIENT_HANDLER:"),
        )
        .await;
        let cleanup_deadline = run.cleanup_deadline;
        let cleanup_error = run.cleanup_error;
        let outcome = match run.completion {
            OwnedHandlerCompletion::ShuttingDown => {
                return Err(JobErr(cleanup_error.map_or_else(
                    || "WORKER_SHUTDOWN".into(),
                    |error| format!("WORKER_SHUTDOWN; cleanup failed: {error}"),
                )));
            }
            OwnedHandlerCompletion::TimedOut => {
                terminalize_non_agent_failure_until(
                    pool,
                    &job.request,
                    NonAgentTerminalFailure::TenderDocument("TENDER_DOCUMENT_PROCESS_TIMEOUT"),
                    cleanup_deadline,
                    "tender process timeout",
                )
                .await?;
                if let Some(error) = cleanup_tracker_until(Some(&cleanup), cleanup_deadline).await {
                    tracing::warn!(%error, "tender processing timed out and terminalized; cleanup remains pending");
                }
                return Ok(());
            }
            OwnedHandlerCompletion::Completed(outcome) => outcome,
        };
        match outcome {
            Ok(()) => cleanup_error.map_or(Ok(()), |error| {
                Err(JobErr(format!("tender process cleanup failed: {error}")))
            }),
            Err(error) => {
                if let Some(message) = error.0.strip_prefix("TRANSIENT_HANDLER:") {
                    return Err(JobErr(message.to_owned()));
                }
                tracing::warn!(request_artifact_id = %request_artifact_id, %error,
                    "tender document processing failed deterministically");
                let already_terminal = terminalize_non_agent_failure_until(
                    pool,
                    &job.request,
                    NonAgentTerminalFailure::TenderDocument("AGENT_OUTPUT_INVALID"),
                    cleanup_deadline,
                    "tender process failure",
                )
                .await
                .map_err(|failure| JobErr(format!("tender parse failed ({error}); {failure}")))?;
                if already_terminal {
                    return cleanup_error.map_or(Ok(()), |cleanup| {
                        Err(JobErr(format!("terminal tender cleanup failed: {cleanup}")))
                    });
                }
                cleanup_error.map_or(Ok(()), |cleanup| {
                    Err(JobErr(format!(
                        "tender process terminalized after {error}; cleanup failed: {cleanup}"
                    )))
                })
            }
        }
    }
}

pub struct RequirementSetCompileV2Worker {
    pool: Option<PgPool>,
    shutdown: CancellationToken,
}

impl oxana::FromContext<AppCtx> for RequirementSetCompileV2Worker {
    fn from_context(ctx: &AppCtx) -> Self {
        Self {
            pool: ctx.pool.clone(),
            shutdown: ctx.shutdown.clone(),
        }
    }
}

#[async_trait]
impl oxana::Worker<RequirementSetCompileJobV2> for RequirementSetCompileV2Worker {
    type Error = JobErr;

    fn max_retries(&self, _job: &RequirementSetCompileJobV2) -> u32 {
        platform::BID_AUTHORING_V2_MAX_RETRIES
    }

    fn retry_delay(&self, _job: &RequirementSetCompileJobV2, retries: u32) -> u64 {
        platform::BidAuthoringV2OxanaPolicy::retry_delay_seconds(retries)
    }

    async fn process(
        &self,
        job: RequirementSetCompileJobV2,
        _ctx: &oxana::JobContext,
    ) -> Result<(), Self::Error> {
        let Some(pool) = &self.pool else {
            return Err(JobErr("postgres not configured".into()));
        };
        let cancel = CancellationToken::new();
        let pipeline_cancel = cancel.clone();
        let pool = pool.clone();
        let run = run_owned_handler(
            async move {
                bidding::tender_analysis::postgres::execute(
                    &pool,
                    &job.request,
                    &pipeline_cancel,
                    &HelperTenderObjectReader,
                )
                .await
                .map(|_| ())
                .map_err(|e| JobErr(e.to_string()))
            },
            HandlerDeadline::from_now(REQUIREMENT_HANDLER_HARD_TIMEOUT),
            self.shutdown.clone(),
            cancel,
            None,
            |error| !error.0.starts_with("INTERNAL:"),
        )
        .await;
        match run.completion {
            OwnedHandlerCompletion::Completed(value) => value,
            OwnedHandlerCompletion::ShuttingDown => Err(JobErr("WORKER_SHUTDOWN".into())),
            OwnedHandlerCompletion::TimedOut => Err(JobErr(
                "analysis cleanup timed out; fenced checkpoint retained".into(),
            )),
        }
    }
}

pub struct DocxComposeV2Worker {
    pool: Option<PgPool>,
    shutdown: CancellationToken,
}

impl oxana::FromContext<AppCtx> for DocxComposeV2Worker {
    fn from_context(ctx: &AppCtx) -> Self {
        Self {
            pool: ctx.pool.clone(),
            shutdown: ctx.shutdown.clone(),
        }
    }
}

#[async_trait]
impl oxana::Worker<DocxComposeJobV2> for DocxComposeV2Worker {
    type Error = JobErr;

    fn max_retries(&self, _job: &DocxComposeJobV2) -> u32 {
        platform::BID_AUTHORING_V2_MAX_RETRIES
    }

    fn retry_delay(&self, _job: &DocxComposeJobV2, retries: u32) -> u64 {
        platform::BidAuthoringV2OxanaPolicy::retry_delay_seconds(retries)
    }

    async fn process(
        &self,
        job: DocxComposeJobV2,
        _ctx: &oxana::JobContext,
    ) -> Result<(), Self::Error> {
        let Some(pool) = &self.pool else {
            return Err(JobErr("postgres not configured".into()));
        };
        let cleanup = platform::StagedObjectCleanupTracker::new(pool, "system:docx-compose-v2");
        let pipeline_cleanup = cleanup.clone();
        let cancel = CancellationToken::new();
        let pipeline_cancel = cancel.clone();
        let pool = pool.clone();
        let run = run_owned_handler(
            async move {
                bidding::docx_composition::runtime::execute(
                    &pool,
                    &job,
                    &pipeline_cancel,
                    &HelperCompositionObjects,
                    &pipeline_cleanup,
                )
                .await
                .map(|_| ())
                .map_err(|e| JobErr(e.to_string()))
            },
            HandlerDeadline::from_now(DOCX_COMPOSE_HANDLER_HARD_TIMEOUT),
            self.shutdown.clone(),
            cancel,
            Some(&cleanup),
            |error| !error.0.starts_with("INTERNAL:"),
        )
        .await;
        // The shared timeout branch leaves cleanup to the domain worker.
        if let Some(error) = cleanup_tracker_until(Some(&cleanup), run.cleanup_deadline).await {
            return Err(JobErr(error));
        }
        if let Some(error) = run.cleanup_error {
            return Err(JobErr(error));
        }
        match run.completion {
            OwnedHandlerCompletion::Completed(value) => value,
            OwnedHandlerCompletion::ShuttingDown => Err(JobErr("WORKER_SHUTDOWN".into())),
            OwnedHandlerCompletion::TimedOut => Err(JobErr(
                "composition cleanup timed out; fenced checkpoint retained".into(),
            )),
        }
    }
}

fn is_obsolete_effect(error: &str) -> bool {
    let message = error
        .strip_prefix("error returned from database: ")
        .unwrap_or(error);
    matches!(
        message.split([':', ';']).next().unwrap_or_default(),
        "REQUEST_ATTEMPT_SUPERSEDED" | "REQUEST_OBSOLETE"
    )
}

pub struct ContentGenerateV2Worker {
    pool: Option<PgPool>,
    shutdown: CancellationToken,
}

impl oxana::FromContext<AppCtx> for ContentGenerateV2Worker {
    fn from_context(ctx: &AppCtx) -> Self {
        Self {
            pool: ctx.pool.clone(),
            shutdown: ctx.shutdown.clone(),
        }
    }
}

fn stable_candidate_uuid(parts: &[&str]) -> Uuid {
    use sha2::{Digest, Sha256};
    let mut digest = Sha256::new();
    for part in parts {
        digest.update((part.len() as u64).to_be_bytes());
        digest.update(part.as_bytes());
    }
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest.finalize()[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct GeneratedEvidenceRange {
    start: usize,
    end: usize,
    bundle: String,
    item: String,
}

fn collect_generated_evidence_ranges(
    nodes: &[bidding::content_block::RichNode],
    offset: &mut usize,
    ranges: &mut Vec<GeneratedEvidenceRange>,
) -> Result<(), String> {
    use bidding::content_block::{Inline, ListItem, Paragraph, RichNode, TextMark};
    fn inlines(
        values: &[Inline],
        offset: &mut usize,
        ranges: &mut Vec<GeneratedEvidenceRange>,
    ) -> Result<(), String> {
        for value in values {
            if let Inline::Text { text, marks } = value {
                let start = *offset;
                *offset += text.len();
                let end = *offset;
                if marks
                    .iter()
                    .any(|mark| matches!(mark, TextMark::Code | TextMark::Link { .. }))
                {
                    return Err("generated Content blocks forbid code and link marks".into());
                }
                let refs = marks
                    .iter()
                    .filter_map(|mark| {
                        if let TextMark::EvidenceRef {
                            evidence_bundle_id,
                            evidence_item_id,
                            ..
                        } = mark
                        {
                            Some(GeneratedEvidenceRange {
                                start,
                                end,
                                bundle: evidence_bundle_id.to_string(),
                                item: evidence_item_id.to_string(),
                            })
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>();
                if !text.trim().is_empty()
                    && refs.is_empty()
                    && !text.trim_start().starts_with("【待人工补充】")
                    && !text.trim_start().starts_with("[NO_EVIDENCE]")
                {
                    return Err("generated text without evidence_ref must be an explicit no-evidence placeholder".into());
                }
                ranges.extend(refs);
            }
        }
        Ok(())
    }
    for node in nodes {
        match node {
            RichNode::Paragraph { content } => inlines(content, offset, ranges)?,
            RichNode::HorizontalRule => {}
            RichNode::CodeBlock { .. } => {
                return Err("generated Content blocks forbid code blocks".into());
            }
            RichNode::Blockquote { content } => {
                for paragraph in content {
                    let Paragraph::Paragraph { content } = paragraph;
                    inlines(content, offset, ranges)?;
                }
            }
            RichNode::BulletList { content } | RichNode::OrderedList { content } => {
                for item in content {
                    let ListItem::ListItem { content } = item;
                    for paragraph in content {
                        let Paragraph::Paragraph { content } = paragraph;
                        inlines(content, offset, ranges)?;
                    }
                }
            }
        }
    }
    Ok(())
}

fn block_generated_evidence_ranges(
    block: &bidding::content_block::BlockContent,
) -> Result<(usize, Vec<GeneratedEvidenceRange>), String> {
    let mut offset = 0;
    let mut ranges = Vec::new();
    match block {
        bidding::content_block::BlockContent::RichText { nodes } => {
            collect_generated_evidence_ranges(nodes, &mut offset, &mut ranges)?
        }
        bidding::content_block::BlockContent::Table { cells, .. } => {
            for cell in cells {
                collect_generated_evidence_ranges(&cell.content, &mut offset, &mut ranges)?;
            }
        }
        _ => {}
    }
    Ok((offset, ranges))
}

fn content_candidate_output(
    raw: &str,
    input: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let mut output: serde_json::Value = serde_json::from_str(raw)
        .map_err(|error| format!("candidate is not closed JSON: {error}"))?;
    bidding::content_runtime::validate_output_schema(&output)?;
    let root = output
        .as_object()
        .ok_or_else(|| "candidate root must be an object".to_string())?;
    let mut keys = root.keys().map(String::as_str).collect::<Vec<_>>();
    keys.sort_unstable();
    if keys != ["factual_claims", "notices", "operations", "schema_version"]
        || output
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
            != Some(1)
    {
        return Err("candidate root contract is not ContentGenerationOutputV1".into());
    }
    let allowed_nodes = input
        .get("target_nodes")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "frozen target nodes missing".to_string())?;
    let mut node_limits = std::collections::HashMap::new();
    let mut node_revisions = std::collections::HashMap::new();
    for node in allowed_nodes {
        let lineage = node
            .get("node_lineage_id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "frozen node lineage missing".to_string())?;
        let block_count = node
            .get("block_count")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| "frozen node block count missing".to_string())?;
        node_limits.insert(lineage, block_count);
        let revision = node
            .get("node_revision_id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "frozen node revision missing".to_string())?;
        node_revisions.insert(revision, (lineage, node));
    }
    let anchor = input
        .get("insertion_anchor")
        .filter(|value| !value.is_null())
        .map(|anchor| {
            let node_revision = anchor
                .get("node_revision_id")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| "insertion anchor node missing".to_string())?;
            let (lineage, node) = node_revisions
                .get(node_revision)
                .ok_or_else(|| "insertion anchor node is outside target".to_string())?;
            let ordinal = if let Some(block_revision) = anchor
                .get("block_revision_id")
                .and_then(serde_json::Value::as_str)
            {
                node.get("blocks")
                    .and_then(serde_json::Value::as_array)
                    .into_iter()
                    .flatten()
                    .find(|block| {
                        block
                            .get("block_revision_id")
                            .and_then(serde_json::Value::as_str)
                            == Some(block_revision)
                    })
                    .and_then(|block| block.get("ordinal"))
                    .and_then(serde_json::Value::as_u64)
                    .ok_or_else(|| "insertion anchor block is outside target node".to_string())?
                    + 1
            } else {
                0
            };
            Ok::<_, String>((*lineage, ordinal))
        })
        .transpose()?;
    let fill_policy = input
        .get("fill_policy")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "frozen fill policy missing".to_string())?;
    let dependency = input
        .get("generation_dependency_sha256")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "frozen generation dependency missing".to_string())?
        .to_owned();
    let allowed_image_assets = input
        .get("evidence_matches")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|entry| {
            entry
                .get("items")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
        })
        .filter(|item| item.get("kind").and_then(serde_json::Value::as_str) == Some("image"))
        .filter_map(|item| {
            item.get("evidence_item_id")
                .and_then(serde_json::Value::as_str)
        })
        .collect::<std::collections::HashSet<_>>();
    let operations = output
        .get_mut("operations")
        .and_then(serde_json::Value::as_array_mut)
        .ok_or_else(|| "candidate operations missing".to_string())?;
    if operations.len() > 10_000 {
        return Err("candidate operation bound exceeded".into());
    }
    if fill_policy == "missing_requirements_only"
        && input
            .get("requirements")
            .and_then(serde_json::Value::as_array)
            .is_some_and(Vec::is_empty)
        && !operations.is_empty()
    {
        return Err("missing_requirements_only has no uncovered Need to generate".into());
    }
    let mut refs = std::collections::HashSet::new();
    let mut operation_text_lengths = std::collections::HashMap::new();
    let mut operation_marked_ranges = std::collections::HashMap::new();
    for operation in operations {
        let object = operation
            .as_object_mut()
            .ok_or_else(|| "candidate operation must be an object".to_string())?;
        let mut operation_keys = object.keys().map(String::as_str).collect::<Vec<_>>();
        operation_keys.sort_unstable();
        if operation_keys
            != [
                "block",
                "client_operation_ref",
                "kind",
                "ordinal",
                "target_node_lineage_id",
            ]
            || object.get("kind").and_then(serde_json::Value::as_str) != Some("insert_block")
        {
            return Err("only closed insert_block candidate operations are accepted".into());
        }
        let client_ref = object
            .get("client_operation_ref")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "client_operation_ref missing".to_string())?
            .to_owned();
        if client_ref.is_empty()
            || client_ref.len() > 128
            || !client_ref
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
            || !refs.insert(client_ref.clone())
        {
            return Err("client_operation_ref is invalid or duplicated".into());
        }
        let lineage = object
            .get("target_node_lineage_id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "candidate target lineage missing".to_string())?;
        let limit = node_limits
            .get(lineage)
            .ok_or_else(|| "candidate targets a node outside the frozen input".to_string())?;
        let ordinal = object
            .get("ordinal")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| "candidate block ordinal missing".to_string())?;
        if ordinal > *limit {
            return Err("candidate block ordinal exceeds the frozen node".into());
        }
        if fill_policy == "empty_only" && *limit != 0 {
            return Err("empty_only candidate targets a non-empty node".into());
        }
        if let Some((anchor_lineage, anchor_ordinal)) = anchor
            && (lineage != anchor_lineage || ordinal != anchor_ordinal)
        {
            return Err("candidate does not honor the frozen insertion anchor".into());
        }
        let mut block: bidding::content_block::ContentBlockV1 = serde_json::from_value(
            object
                .get("block")
                .ok_or_else(|| "candidate block missing".to_string())?
                .clone(),
        )
        .map_err(|error| format!("candidate block schema invalid: {error}"))?;
        block.block_revision_id = stable_candidate_uuid(&[&dependency, &client_ref, "revision"]);
        block.lineage_id = stable_candidate_uuid(&[&dependency, &client_ref, "lineage"]);
        block.revision = 1;
        block.origin = bidding::content_block::BlockOrigin::AgentCandidate;
        block.content_sha256 = block.content.sha256().map_err(|error| error.to_string())?;
        block.validate().map_err(str::to_owned)?;
        if !matches!(
            &block.content,
            bidding::content_block::BlockContent::RichText { .. }
                | bidding::content_block::BlockContent::Table { .. }
                | bidding::content_block::BlockContent::Image { .. }
        ) {
            return Err("generated Content accepts only rich_text, table, or image blocks".into());
        }
        if let bidding::content_block::BlockContent::Image {
            asset_revision_id, ..
        } = &block.content
            && !allowed_image_assets.contains(asset_revision_id.to_string().as_str())
        {
            return Err("candidate image asset is outside frozen image evidence".into());
        }
        let (visible_length, marked_ranges) = block_generated_evidence_ranges(&block.content)?;
        let block_value = serde_json::to_value(&block).map_err(|error| error.to_string())?;
        operation_text_lengths.insert(client_ref.clone(), visible_length);
        operation_marked_ranges.insert(client_ref, marked_ranges);
        object.insert("block".into(), block_value);
    }
    let allowed_evidence = input
        .get("evidence_matches")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|entry| {
            let bundle = entry
                .get("evidence_bundle_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned();
            entry
                .get("items")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(move |item| {
                    let id = item.get("evidence_item_id")?.as_str()?.to_owned();
                    let start = item.get("quote_start_offset")?.as_u64()?;
                    let end = item.get("quote_end_offset")?.as_u64()?;
                    let quote = item.get("quote_utf8")?.as_str()?;
                    let start_usize = usize::try_from(start).ok()?;
                    let end_usize = usize::try_from(end).ok()?;
                    if start >= end
                        || end_usize > quote.len()
                        || !quote.is_char_boundary(start_usize)
                        || !quote.is_char_boundary(end_usize)
                    {
                        return None;
                    }
                    Some(((bundle.clone(), id), (start, end)))
                })
        })
        .collect::<std::collections::HashMap<_, _>>();
    let claims = output
        .get("factual_claims")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "factual_claims must be an array".to_string())?;
    if claims.len() > 100_000 {
        return Err("factual claim bound exceeded".into());
    }
    let mut declared_ranges = std::collections::HashSet::new();
    for claim in claims {
        let claim = claim
            .as_object()
            .ok_or_else(|| "factual claim must be an object".to_string())?;
        let mut keys = claim.keys().map(String::as_str).collect::<Vec<_>>();
        keys.sort_unstable();
        if keys
            != [
                "client_operation_ref",
                "evidence_bundle_id",
                "evidence_item_id",
                "utf8_end",
                "utf8_start",
            ]
        {
            return Err("factual claim contract is not closed".into());
        }
        let client_ref = claim
            .get("client_operation_ref")
            .and_then(serde_json::Value::as_str)
            .ok_or("claim operation ref missing")?;
        let start = claim
            .get("utf8_start")
            .and_then(serde_json::Value::as_u64)
            .ok_or("claim start missing")? as usize;
        let end = claim
            .get("utf8_end")
            .and_then(serde_json::Value::as_u64)
            .ok_or("claim end missing")? as usize;
        if start >= end
            || end
                > *operation_text_lengths
                    .get(client_ref)
                    .ok_or("claim operation ref is not generated")?
        {
            return Err("factual claim range is outside generated text".into());
        }
        let bundle = claim
            .get("evidence_bundle_id")
            .and_then(serde_json::Value::as_str)
            .ok_or("claim bundle missing")?;
        let item = claim
            .get("evidence_item_id")
            .and_then(serde_json::Value::as_str)
            .ok_or("claim item missing")?;
        if !allowed_evidence.contains_key(&(bundle.to_owned(), item.to_owned())) {
            return Err("factual claim evidence is outside frozen selection".into());
        }
        if !declared_ranges.insert((
            client_ref.to_owned(),
            start,
            end,
            bundle.to_owned(),
            item.to_owned(),
        )) {
            return Err("factual claim is duplicated".into());
        }
    }
    let marked_ranges = operation_marked_ranges
        .into_iter()
        .flat_map(|(client_ref, ranges)| {
            ranges.into_iter().map(move |range| {
                (
                    client_ref.clone(),
                    range.start,
                    range.end,
                    range.bundle,
                    range.item,
                )
            })
        })
        .collect::<std::collections::HashSet<_>>();
    if declared_ranges != marked_ranges {
        return Err("factual claims and evidence_ref spans must correspond exactly".into());
    }
    fn validate_evidence_refs(
        value: &serde_json::Value,
        allowed: &std::collections::HashMap<(String, String), (u64, u64)>,
    ) -> Result<(), String> {
        match value {
            serde_json::Value::Object(map) => {
                if map.get("kind").and_then(serde_json::Value::as_str) == Some("evidence_ref") {
                    let bundle = map
                        .get("evidence_bundle_id")
                        .and_then(serde_json::Value::as_str)
                        .ok_or("evidence_ref bundle missing")?;
                    let item = map
                        .get("evidence_item_id")
                        .and_then(serde_json::Value::as_str)
                        .ok_or("evidence_ref item missing")?;
                    let start = map
                        .get("quote_start_offset")
                        .and_then(serde_json::Value::as_u64)
                        .ok_or("evidence_ref quote start missing")?;
                    let end = map
                        .get("quote_end_offset")
                        .and_then(serde_json::Value::as_u64)
                        .ok_or("evidence_ref quote end missing")?;
                    if allowed.get(&(bundle.to_owned(), item.to_owned())) != Some(&(start, end)) {
                        return Err("content EvidenceRef identity or quote offsets differ from frozen selection".into());
                    }
                }
                for nested in map.values() {
                    validate_evidence_refs(nested, allowed)?;
                }
            }
            serde_json::Value::Array(values) => {
                for nested in values {
                    validate_evidence_refs(nested, allowed)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    validate_evidence_refs(output.get("operations").unwrap(), &allowed_evidence)?;
    let notices = output
        .get("notices")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "notices must be an array".to_string())?;
    if notices.len() > 10_000 {
        return Err("candidate notice bound exceeded".into());
    }
    let allowed_requirements = input
        .get("requirements")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|requirement| {
            requirement
                .get("requirement_revision_id")
                .and_then(serde_json::Value::as_str)
        })
        .collect::<std::collections::HashSet<_>>();
    let notice_codes = [
        "NO_EVIDENCE",
        "WEAK_EVIDENCE",
        "UNSUPPORTED_FACT",
        "FORM_CONSTRAINT",
        "TARGET_ALREADY_HAS_CONTENT",
    ];
    for notice in notices {
        let Some(object) = notice.as_object() else {
            return Err("candidate notice must be an object".into());
        };
        let mut keys = object.keys().map(String::as_str).collect::<Vec<_>>();
        keys.sort_unstable();
        let requirement = notice
            .get("requirement_revision_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let message = notice
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if keys != ["code", "message", "requirement_revision_id", "severity"]
            || !notice_codes.contains(
                &notice
                    .get("code")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default(),
            )
            || !["info", "warning"].contains(
                &notice
                    .get("severity")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default(),
            )
            || message.is_empty()
            || message.len() > 4_096
            || Uuid::parse_str(requirement).is_err()
            || !allowed_requirements.contains(requirement)
        {
            return Err("candidate notice is outside the frozen closed contract".into());
        }
    }
    Ok(output)
}

fn content_retrieval_error(
    error: knowledge::KnowledgeRetrievalError,
) -> bidding::agent_error::AgentError {
    let (code, message) = match error {
        knowledge::KnowledgeRetrievalError::InvalidRequest(message) => {
            ("CONTENT_RETRIEVAL_INVALID_REQUEST", message)
        }
        knowledge::KnowledgeRetrievalError::Unavailable(message) => {
            ("CONTENT_RETRIEVAL_UNAVAILABLE", message)
        }
        knowledge::KnowledgeRetrievalError::PolicyRevoked(message) => {
            ("CONTENT_RETRIEVAL_POLICY_REVOKED", message)
        }
        knowledge::KnowledgeRetrievalError::DigestMismatch(message) => {
            ("CONTENT_RETRIEVAL_DIGEST_MISMATCH", message)
        }
        knowledge::KnowledgeRetrievalError::QuotaExceeded(message) => {
            ("CONTENT_RETRIEVAL_QUOTA_EXCEEDED", message)
        }
        knowledge::KnowledgeRetrievalError::InvalidHit(message) => {
            ("CONTENT_RETRIEVAL_INVALID_HIT", message)
        }
    };
    bidding::agent_error::AgentError::new(code, message)
}

async fn retrieve_content_scope_with_retry_v2(
    adapter: &knowledge::PostgresKnowledgeRetrievalAdapter,
    frozen: &knowledge::knowledge_retrieval::FrozenRetrievalPolicyIdentityV1,
    scope: knowledge::KnowledgeEvidenceScopeV2,
) -> Result<knowledge::KnowledgeEvidenceBatchV3, bidding::agent_error::AgentError> {
    let mut unavailable = None;
    for _ in 0..3 {
        match adapter
            .retrieve_frozen_evidence_v3(frozen, scope.clone())
            .await
        {
            Ok(batch) => return Ok(batch),
            Err(knowledge::KnowledgeRetrievalError::Unavailable(message)) => {
                unavailable = Some(message)
            }
            Err(error) => return Err(content_retrieval_error(error)),
        }
    }
    Err(bidding::agent_error::AgentError::new(
        "CONTENT_RETRIEVAL_UNAVAILABLE",
        unavailable.unwrap_or_else(|| "retrieval unavailable".into()),
    ))
}

async fn prepare_content_evidence_v2(
    pool: &PgPool,
    input: &serde_json::Value,
) -> Result<
    (
        knowledge::knowledge_retrieval::RetrievalPolicyIdentityV1,
        serde_json::Value,
        serde_json::Value,
    ),
    bidding::agent_error::AgentError,
> {
    let frozen_value = input.get("retrieval_identity").cloned().ok_or_else(|| {
        bidding::agent_error::AgentError::new(
            "CONTENT_RETRIEVAL_INVALID_REQUEST",
            "frozen retrieval identity missing",
        )
    })?;
    let frozen_utf8 = input
        .get("retrieval_identity_utf8")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            bidding::agent_error::AgentError::new(
                "CONTENT_RETRIEVAL_INVALID_REQUEST",
                "frozen retrieval identity bytes missing",
            )
        })?;
    let frozen: knowledge::knowledge_retrieval::FrozenRetrievalPolicyIdentityV1 =
        serde_json::from_str(frozen_utf8).map_err(|error| {
            bidding::agent_error::AgentError::new(
                "CONTENT_RETRIEVAL_INVALID_REQUEST",
                error.to_string(),
            )
        })?;
    if serde_json::to_value(&frozen).ok().as_ref() != Some(&frozen_value) {
        return Err(bidding::agent_error::AgentError::new(
            "CONTENT_RETRIEVAL_DIGEST_MISMATCH",
            "retrieval identity value differs from frozen bytes",
        ));
    }
    let (frozen_bytes, canonical_sha) = frozen.canonical_bytes_and_sha256().map_err(|error| {
        bidding::agent_error::AgentError::new("CONTENT_RETRIEVAL_INVALID_REQUEST", error)
    })?;
    if frozen_bytes.as_slice() != frozen_utf8.as_bytes() {
        return Err(bidding::agent_error::AgentError::new(
            "CONTENT_RETRIEVAL_DIGEST_MISMATCH",
            "retrieval identity bytes are not canonical",
        ));
    }
    let expected_sha = input
        .get("retrieval_identity_sha256")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            bidding::agent_error::AgentError::new(
                "CONTENT_RETRIEVAL_INVALID_REQUEST",
                "retrieval identity digest missing",
            )
        })?;
    if canonical_sha != expected_sha {
        return Err(bidding::agent_error::AgentError::new(
            "CONTENT_RETRIEVAL_DIGEST_MISMATCH",
            "retrieval identity bytes changed",
        ));
    }
    let policy = frozen.validate().map_err(|error| {
        let code = if error.starts_with("CONTENT_RETRIEVAL_DIGEST_MISMATCH:") {
            "CONTENT_RETRIEVAL_DIGEST_MISMATCH"
        } else {
            "CONTENT_RETRIEVAL_INVALID_REQUEST"
        };
        bidding::agent_error::AgentError::new(code, error)
    })?;
    let adapter = knowledge::PostgresKnowledgeRetrievalAdapter::new_complete_v2_from_environment(
        pool.clone(),
    )
    .map_err(|error| {
        bidding::agent_error::AgentError::new("CONTENT_RETRIEVAL_UNAVAILABLE", error.to_string())
    })?;
    let requirements = input
        .get("requirements")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            bidding::agent_error::AgentError::new(
                "CONTENT_RETRIEVAL_INVALID_REQUEST",
                "frozen generation requirements missing",
            )
        })?;
    let request_id = input
        .get("request_artifact_id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            bidding::agent_error::AgentError::new(
                "CONTENT_RETRIEVAL_INVALID_REQUEST",
                "frozen request identity missing",
            )
        })?
        .to_owned();
    let mut batches = Vec::with_capacity(requirements.len());
    for requirement in requirements {
        let requirement_id = requirement
            .get("requirement_revision_id")
            .and_then(serde_json::Value::as_str)
            .and_then(|value| Uuid::parse_str(value).ok())
            .ok_or_else(|| {
                bidding::agent_error::AgentError::new(
                    "CONTENT_RETRIEVAL_INVALID_REQUEST",
                    "frozen requirement identity missing",
                )
            })?;
        let requirement_text = requirement
            .get("requirement_text")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                bidding::agent_error::AgentError::new(
                    "CONTENT_RETRIEVAL_INVALID_REQUEST",
                    "frozen requirement text missing",
                )
            })?
            .to_owned();
        let requirement_identity_sha256 = requirement
            .get("requirement_identity_sha256")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                bidding::agent_error::AgentError::new(
                    "CONTENT_RETRIEVAL_INVALID_REQUEST",
                    "frozen requirement digest missing",
                )
            })?
            .to_owned();
        let product_request = knowledge::ProductEvidenceRequestV1 {
            schema_version: 1,
            requirement_identity_sha256: requirement_identity_sha256.clone(),
            requirement_text: requirement_text.clone(),
            product_version_ids: frozen.product_version_ids.clone(),
            retrieval_policy: policy.clone(),
        };
        let company_request = knowledge::CompanyEvidenceRequestV1 {
            schema_version: 1,
            requirement_identity_sha256: requirement_identity_sha256.clone(),
            requirement_text: requirement_text.clone(),
            library_version_ids: frozen.library_version_ids.clone(),
            retrieval_policy: policy.clone(),
        };
        let product_line = retrieve_content_scope_with_retry_v2(
            &adapter,
            &frozen,
            knowledge::KnowledgeEvidenceScopeV2::ProductLine(product_request),
        )
        .await?;
        let company = retrieve_content_scope_with_retry_v2(
            &adapter,
            &frozen,
            knowledge::KnowledgeEvidenceScopeV2::Company(company_request),
        )
        .await?;
        batches.push(
            knowledge::knowledge_retrieval_pg::RequirementEvidenceBatchesV2 {
                route_id: requirement_id,
                requirement_artifact_id: requirement_id,
                requirement_identity_sha256,
                requirement_text,
                product_line,
                company,
            },
        );
    }
    let canonical_scope =
        knowledge::knowledge_retrieval_pg::compile_requirement_evidence_scope_v2(&policy, &batches)
            .map_err(content_retrieval_error)?;
    let products = canonical_scope
        .get("products")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            bidding::agent_error::AgentError::new(
                "CONTENT_RETRIEVAL_INVALID_HIT",
                "attested evidence products missing",
            )
        })?;
    let hits = canonical_scope
        .get("frozen_hits")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            bidding::agent_error::AgentError::new(
                "CONTENT_RETRIEVAL_INVALID_HIT",
                "attested evidence hits missing",
            )
        })?;
    let matches=serde_json::Value::Array(batches.iter().map(|batch| {
        let requirement_id=batch.requirement_artifact_id.to_string();
        let bundle_id=stable_candidate_uuid(&[&request_id,&requirement_id,"bundle"]);
        let items=hits.iter().filter(|hit|hit.get("requirement_artifact_id").and_then(serde_json::Value::as_str)
            ==Some(requirement_id.as_str())).filter_map(|hit|{
            let product_id=hit.get("product_version_artifact_id")?.as_str()?;
            let product=products.iter().find(|product|product.get("id").and_then(serde_json::Value::as_str)==Some(product_id))?;
            let hit_id=hit.get("id")?.as_str()?;
            let evidence_item_id=stable_candidate_uuid(&[&request_id,&requirement_id,hit_id,"item"]);
            if hit.get("source_type").and_then(serde_json::Value::as_str)==Some("image_ocr") {
                let media=hit.get("media")?;
                Some(serde_json::json!({"kind":"image","evidence_item_id":evidence_item_id,
                    "document_id":hit.get("document_id")?,"source_chunk_id":hit.get("source_chunk_id")?,
                    "product_version_id":product.get("product_version_id")?,"workspace_kind":product.get("workspace_kind")?,
                    "quote_utf8":hit.get("chunk_utf8")?,"quote_sha256":hit.get("chunk_sha256")?,
                    "quote_start_offset":hit.get("quote_start_offset")?,"quote_end_offset":hit.get("quote_end_offset")?,
                    "retrieval_rank":hit.get("retrieval_rank")?,"retrieval_contract_version":hit.get("retrieval_contract_version")?,
                    "image_artifact_revision_id":media.get("image_artifact_revision_id")?,
                    "object_ref":media.get("object_ref")?,"sha256":media.get("sha256")?,
                    "media_type":media.get("media_type")?,"width":media.get("width")?,"height":media.get("height")?,
                    "frozen_document_display_name":media.get("frozen_document_display_name")?,
                    "page_ordinal":media.get("page_ordinal").cloned().unwrap_or(serde_json::Value::Null),
                    "bounding_region":media.get("bounding_region").cloned().unwrap_or(serde_json::Value::Null)}))
            }else{
                Some(serde_json::json!({"kind":"text_quote","evidence_item_id":evidence_item_id,
                    "document_id":hit.get("document_id")?,"source_chunk_id":hit.get("source_chunk_id")?,
                    "product_version_id":product.get("product_version_id")?,"workspace_kind":product.get("workspace_kind")?,
                    "frozen_document_display_name":hit.get("frozen_document_display_name")?,
                    "quote_utf8":hit.get("chunk_utf8")?,"quote_sha256":hit.get("chunk_sha256")?,
                    "quote_start_offset":hit.get("quote_start_offset")?,"quote_end_offset":hit.get("quote_end_offset")?,
                    "retrieval_rank":hit.get("retrieval_rank")?,"retrieval_contract_version":hit.get("retrieval_contract_version")?}))
            }
        }).collect::<Vec<_>>();
        serde_json::json!({"requirement_revision_id":batch.requirement_artifact_id,
            "evidence_bundle_id":bundle_id,"items":items})
    }).collect());
    Ok((policy, canonical_scope, matches))
}

async fn load_user_pick_evidence_v2(
    pool: &PgPool,
    input: &serde_json::Value,
) -> Result<
    (
        knowledge::knowledge_retrieval_pg::AttestedEvidenceScopeV2,
        serde_json::Value,
    ),
    JobErr,
> {
    let request_id = input
        .get("request_artifact_id")
        .and_then(serde_json::Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or_else(|| JobErr("frozen request identity missing".into()))?;
    let frozen_sha = input
        .get("generation_dependency_sha256")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| JobErr("frozen input digest missing".into()))?;
    let frozen: serde_json::Value =
        sqlx::query_scalar("SELECT kb_bid_v2_load_user_pick_evidence($1,1,$2::kb_sha256)")
            .bind(request_id)
            .bind(frozen_sha)
            .fetch_one(pool)
            .await
            .map_err(|error| JobErr(format!("EVIDENCE_UNAVAILABLE: {error}")))?;
    let attestation = knowledge::knowledge_retrieval_pg::AttestedEvidenceScopeV2 {
        attestation_id: frozen
            .get("attestation_id")
            .and_then(serde_json::Value::as_str)
            .and_then(|value| Uuid::parse_str(value).ok())
            .ok_or_else(|| JobErr("PickSet attestation identity missing".into()))?,
        attestation_sha256: frozen
            .get("attestation_sha256")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| JobErr("PickSet attestation digest missing".into()))?
            .to_owned(),
        canonical_scope: frozen
            .get("canonical_scope")
            .cloned()
            .ok_or_else(|| JobErr("PickSet attestation snapshot missing".into()))?,
    };
    let original_bundle = frozen
        .get("evidence_bundle_id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| JobErr("PickSet evidence bundle identity missing".into()))?;
    let request = request_id.to_string();
    let copied_bundle = stable_candidate_uuid(&[&request, original_bundle, "user-pick-bundle"]);
    let mut copied_items = frozen
        .get("items")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .ok_or_else(|| JobErr("PickSet selected evidence items missing".into()))?;
    for item in &mut copied_items {
        let old = item
            .get("evidence_item_id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| JobErr("PickSet evidence item identity missing".into()))?;
        let copied = stable_candidate_uuid(&[&request, original_bundle, old, "user-pick-item"]);
        item.as_object_mut()
            .ok_or_else(|| JobErr("PickSet evidence item invalid".into()))?
            .insert("evidence_item_id".into(), serde_json::json!(copied));
    }
    let matches = serde_json::json!([{"requirement_revision_id":frozen.get("requirement_revision_id"),
        "evidence_bundle_id":copied_bundle,"items":copied_items}]);
    Ok((attestation, matches))
}

fn content_runtime_error(
    error: bidding::content_runtime::ContentTurnError,
) -> bidding::agent_error::AgentError {
    bidding::agent_error::AgentError::new(error.code(), error.message())
}

fn retain_content_attempt_failure(
    call_ordinal: i32,
    failure: bidding::agent_error::AgentError,
) -> Result<String, bidding::agent_error::AgentError> {
    if call_ordinal == 3 {
        Err(failure)
    } else {
        Ok(failure.to_string())
    }
}

fn content_contract_error(message: String) -> bidding::agent_error::AgentError {
    let code = message.split(':').next().unwrap_or("INPUT_SCHEMA_INVALID");
    bidding::agent_error::AgentError::new(code, message.clone())
}

fn content_database_error(error: sqlx::Error) -> bidding::agent_error::AgentError {
    const CLOSED: &[&str] = &[
        "REQUEST_OBSOLETE",
        "REQUEST_ATTEMPT_SUPERSEDED",
        "FROZEN_INPUT_MISSING",
        "FROZEN_INPUT_DIGEST_MISMATCH",
        "INPUT_SCHEMA_INVALID",
        "WORKSPACE_CAS_CONFLICT",
        "AGENT_OUTPUT_INVALID",
        "AGENT_TURN_BUDGET_EXCEEDED",
        "CONTENT_RETRIEVAL_INVALID_REQUEST",
        "CONTENT_RETRIEVAL_UNAVAILABLE",
        "CONTENT_RETRIEVAL_QUOTA_EXCEEDED",
        "CONTENT_RETRIEVAL_INVALID_HIT",
        "CONTENT_RETRIEVAL_POLICY_REVOKED",
        "CONTENT_RETRIEVAL_DIGEST_MISMATCH",
        "CONTENT_MATCH_TIMEOUT",
    ];
    if let Some(database) = error.as_database_error() {
        let message = database.message();
        if let Some(code) = CLOSED.iter().find(|code| {
            message == **code
                || message
                    .strip_prefix(**code)
                    .is_some_and(|suffix| suffix.starts_with(':') || suffix.starts_with(' '))
        }) {
            return bidding::agent_error::AgentError::new(code, message);
        }
    }
    bidding::agent_error::AgentError::new("INTERNAL", error.to_string())
}

async fn run_content_agent_v1(
    pool: &PgPool,
    request: &platform::BidAuthoringRequestIdentityV2,
    owner: &bidding::bid_authoring_v2::ContentRunLease,
    input: &serde_json::Value,
    staged_input_sha256: &str,
) -> Result<serde_json::Value, bidding::agent_error::AgentError> {
    let contract = input.get("agent_contract").ok_or_else(|| {
        bidding::agent_error::AgentError::new(
            "INPUT_SCHEMA_INVALID",
            "generate Agent contract missing",
        )
    })?;
    let runtime: bidding::content_runtime::ContentAgentRuntimeContractV1 =
        serde_json::from_value(contract.get("runtime_contract").cloned().ok_or_else(|| {
            bidding::agent_error::AgentError::new(
                "INPUT_SCHEMA_INVALID",
                "Content runtime contract missing",
            )
        })?)
        .map_err(|error| {
            bidding::agent_error::AgentError::new("INPUT_SCHEMA_INVALID", error.to_string())
        })?;
    runtime.validate().map_err(content_contract_error)?;
    let prompt = contract
        .get("prompt_utf8")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            bidding::agent_error::AgentError::new(
                "INPUT_SCHEMA_INVALID",
                "Content prompt bytes missing",
            )
        })?;
    let schema = contract
        .get("output_schema_utf8")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            bidding::agent_error::AgentError::new(
                "INPUT_SCHEMA_INVALID",
                "Content output schema bytes missing",
            )
        })?;
    if prompt.as_bytes() != bidding::content_runtime::CONTENT_AGENT_SYSTEM_PROMPT.as_bytes()
        || schema.as_bytes() != bidding::content_runtime::CONTENT_OUTPUT_SCHEMA_UTF8.as_bytes()
        || platform::sha256_hex(prompt.as_bytes())
            != contract
                .get("prompt_sha256")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
        || platform::sha256_hex(schema.as_bytes())
            != contract
                .get("output_schema_sha256")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
    {
        return Err(bidding::agent_error::AgentError::new(
            "FROZEN_INPUT_DIGEST_MISMATCH",
            "Content prompt/schema bytes changed",
        ));
    }
    let runtime_value = serde_json::to_value(&runtime).map_err(|error| {
        bidding::agent_error::AgentError::new("INPUT_SCHEMA_INVALID", error.to_string())
    })?;
    let runtime_bytes = bidding::content_runtime::canonical_json_bytes(&runtime_value)
        .map_err(|error| bidding::agent_error::AgentError::new("INPUT_SCHEMA_INVALID", error))?;
    let runtime_sha = platform::sha256_hex(&runtime_bytes);
    if Some(runtime_sha.as_str())
        != contract
            .get("runtime_contract_sha256")
            .and_then(serde_json::Value::as_str)
    {
        return Err(bidding::agent_error::AgentError::new(
            "FROZEN_INPUT_DIGEST_MISMATCH",
            "Content runtime bytes changed",
        ));
    }
    if staged_input_sha256.len() != 64
        || !staged_input_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(bidding::agent_error::AgentError::new(
            "CONTENT_DIVERGENT_AGENT_INPUT_REPLAY",
            "staged Content Agent input digest invalid",
        ));
    }
    let prompt_contract_id = contract
        .get("prompt_contract_id")
        .and_then(serde_json::Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or_else(|| {
            bidding::agent_error::AgentError::new(
                "INPUT_SCHEMA_INVALID",
                "prompt contract id missing",
            )
        })?;
    let agent_contract_id = contract
        .get("agent_contract_id")
        .and_then(serde_json::Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or_else(|| {
            bidding::agent_error::AgentError::new(
                "INPUT_SCHEMA_INVALID",
                "agent contract id missing",
            )
        })?;
    let model_contract_id = contract
        .get("model_contract_id")
        .and_then(serde_json::Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or_else(|| {
            bidding::agent_error::AgentError::new(
                "INPUT_SCHEMA_INVALID",
                "model contract id missing",
            )
        })?;
    let required = |name: &str| {
        contract
            .get(name)
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                bidding::agent_error::AgentError::new(
                    "INPUT_SCHEMA_INVALID",
                    format!("{name} missing"),
                )
            })
    };
    let transport = bidding::content_runtime::ReqwestContentHttpTransport;
    let mut prior_validation_error: Option<String> = None;
    for _ in 0..3 {
        let call_ordinal = bidding::bid_authoring_v2::claim_content_boundary_attempt_v1(
            pool,
            request,
            owner,
            staged_input_sha256,
            prompt_contract_id,
            required("prompt_contract_sha256")?,
            required("prompt_sha256")?,
            required("output_schema_id")?,
            required("output_schema_sha256")?,
            agent_contract_id,
            required("agent_contract_sha256")?,
            model_contract_id,
            required("model_contract_sha256")?,
            &runtime_sha,
        )
        .await
        .map_err(content_database_error)?;
        let user_payload = serde_json::json!({
            "frozen_input": input,
            "prior_validation_error": prior_validation_error,
        });
        let failure =
            match bidding::content_runtime::turn_once_with(&transport, &runtime, &user_payload)
                .await
            {
                Ok(value) => match content_candidate_output(&value.to_string(), input) {
                    Ok(output) => return Ok(output),
                    Err(message) => {
                        bidding::agent_error::AgentError::new("AGENT_OUTPUT_INVALID", message)
                    }
                },
                Err(error) => content_runtime_error(error),
            };
        prior_validation_error = Some(retain_content_attempt_failure(call_ordinal, failure)?);
    }
    let message = prior_validation_error.unwrap_or_else(|| "Content Agent output invalid".into());
    if message.starts_with("AGENT_TURN_TIMEOUT:") {
        Err(bidding::agent_error::AgentError::new(
            "AGENT_TURN_TIMEOUT",
            message,
        ))
    } else if message.starts_with("AGENT_PROVIDER_UNAVAILABLE:") {
        Err(bidding::agent_error::AgentError::new(
            "AGENT_PROVIDER_UNAVAILABLE",
            message,
        ))
    } else {
        Err(bidding::agent_error::AgentError::new(
            "AGENT_OUTPUT_INVALID",
            message,
        ))
    }
}

async fn process_content_generation_v2(
    pool: &PgPool,
    job: &ContentGenerateJobV2,
    owner: Option<&bidding::bid_authoring_v2::ContentRunLease>,
) -> Result<(), bidding::agent_error::AgentError> {
    let input = bidding::bid_authoring_v2::load_content_generation_input_v2(
        pool,
        job.request.request_artifact_id,
        job.request.request_revision,
        &job.request.frozen_input_sha256,
    )
    .await
    .map_err(content_database_error)?;
    let input_operation = input
        .get("operation")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            bidding::agent_error::AgentError::new(
                "INPUT_SCHEMA_INVALID",
                "Content operation missing",
            )
        })?;
    let queued_operation = match job.operation {
        ContentGenerateOperationV2::Generate => "generate",
        ContentGenerateOperationV2::MatchOnly => "match_only",
    };
    if input_operation != queued_operation {
        return Err(bidding::agent_error::AgentError::new(
            "INPUT_SCHEMA_INVALID",
            "queued Content operation differs from frozen operation",
        ));
    }
    let staged = if let Some(owner) = owner {
        let staged = bidding::bid_authoring_v2::load_content_agent_input_v1(pool, &job.request)
            .await
            .map_err(content_database_error)?;
        if staged.is_none() {
            bidding::bid_authoring_v2::progress_content_agent_run_v1(
                pool,
                &job.request,
                owner,
                "retrieving",
                serde_json::json!({"phase":"retrieving"}),
            )
            .await
            .map_err(content_database_error)?;
        }
        staged
    } else {
        None
    };
    let (existing_attestation, pending_scope, matches, agent_input, staged_input_sha256) =
        if let Some(staged) = staged {
            let payload = staged.get("payload").ok_or_else(|| {
                bidding::agent_error::AgentError::new(
                    "CONTENT_DIVERGENT_AGENT_INPUT_REPLAY",
                    "stored Content Agent input payload missing",
                )
            })?;
            let matches = payload.get("matches").cloned().ok_or_else(|| {
                bidding::agent_error::AgentError::new(
                    "CONTENT_DIVERGENT_AGENT_INPUT_REPLAY",
                    "stored matches missing",
                )
            })?;
            let agent_input = payload.get("agent_input").cloned().ok_or_else(|| {
                bidding::agent_error::AgentError::new(
                    "CONTENT_DIVERGENT_AGENT_INPUT_REPLAY",
                    "stored Agent input missing",
                )
            })?;
            let pending_scope = payload
                .get("pending_scope")
                .filter(|value| !value.is_null())
                .cloned();
            let existing_attestation = payload
                .get("existing_attestation")
                .filter(|value| !value.is_null())
                .map(|value| {
                    Ok(knowledge::knowledge_retrieval_pg::AttestedEvidenceScopeV2 {
                        attestation_id: value
                            .get("attestation_id")
                            .and_then(serde_json::Value::as_str)
                            .and_then(|value| Uuid::parse_str(value).ok())
                            .ok_or_else(|| {
                                bidding::agent_error::AgentError::new(
                                    "CONTENT_DIVERGENT_AGENT_INPUT_REPLAY",
                                    "stored attestation id invalid",
                                )
                            })?,
                        attestation_sha256: value
                            .get("attestation_sha256")
                            .and_then(serde_json::Value::as_str)
                            .ok_or_else(|| {
                                bidding::agent_error::AgentError::new(
                                    "CONTENT_DIVERGENT_AGENT_INPUT_REPLAY",
                                    "stored attestation digest invalid",
                                )
                            })?
                            .to_owned(),
                        canonical_scope: value.get("canonical_scope").cloned().ok_or_else(
                            || {
                                bidding::agent_error::AgentError::new(
                                    "CONTENT_DIVERGENT_AGENT_INPUT_REPLAY",
                                    "stored attestation scope missing",
                                )
                            },
                        )?,
                    })
                })
                .transpose()?;
            let staged_input_sha256 = staged
                .get("input_sha256")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    bidding::agent_error::AgentError::new(
                        "CONTENT_DIVERGENT_AGENT_INPUT_REPLAY",
                        "stored Agent input digest missing",
                    )
                })?
                .to_owned();
            (
                existing_attestation,
                pending_scope,
                matches,
                agent_input,
                staged_input_sha256,
            )
        } else {
            let user_pick = input
                .get("evidence_selection_mode")
                .and_then(serde_json::Value::as_str)
                == Some("user_pick_set");
            let (existing_attestation, pending_scope, matches) = if user_pick {
                let (attestation, matches) = load_user_pick_evidence_v2(pool, &input)
                    .await
                    .map_err(|error| {
                        bidding::agent_error::AgentError::new(
                            "CONTENT_RETRIEVAL_INVALID_REQUEST",
                            error.0,
                        )
                    })?;
                (Some(attestation), None, matches)
            } else {
                let (_, scope, matches) = prepare_content_evidence_v2(pool, &input).await?;
                (None, Some(scope), matches)
            };
            let mut agent_input = input.clone();
            agent_input["evidence_matches"] = matches.clone();
            if let Some(owner) = owner {
                let stage_payload = serde_json::json!({
                    "matches":matches,
                    "pending_scope":pending_scope,
                    "existing_attestation":existing_attestation.as_ref().map(|value| serde_json::json!({
                        "attestation_id":value.attestation_id,
                        "attestation_sha256":value.attestation_sha256,
                        "canonical_scope":value.canonical_scope,
                    })),
                    "agent_input":agent_input,
                });
                let stored = bidding::bid_authoring_v2::store_content_agent_input_v1(
                    pool,
                    &job.request,
                    owner,
                    &stage_payload,
                )
                .await
                .map_err(content_database_error)?;
                let staged_input_sha256 = stored
                    .get("input_sha256")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| {
                        bidding::agent_error::AgentError::new(
                            "CONTENT_DIVERGENT_AGENT_INPUT_REPLAY",
                            "stored Agent input digest missing",
                        )
                    })?
                    .to_owned();
                (
                    existing_attestation,
                    pending_scope,
                    matches,
                    agent_input,
                    staged_input_sha256,
                )
            } else {
                (
                    existing_attestation,
                    pending_scope,
                    matches,
                    agent_input,
                    String::new(),
                )
            }
        };
    let (candidate_id, payload, digest, operations) = match job.operation {
        ContentGenerateOperationV2::MatchOnly => (None, None, None, serde_json::json!([])),
        ContentGenerateOperationV2::Generate => {
            let owner = owner.ok_or_else(|| {
                bidding::agent_error::AgentError::new(
                    "REQUEST_ATTEMPT_SUPERSEDED",
                    "generate owner missing",
                )
            })?;
            bidding::bid_authoring_v2::progress_content_agent_run_v1(
                pool,
                &job.request,
                owner,
                "generating",
                serde_json::json!({"phase":"generating"}),
            )
            .await
            .map_err(content_database_error)?;
            let output = run_content_agent_v1(
                pool,
                &job.request,
                owner,
                &agent_input,
                &staged_input_sha256,
            )
            .await?;
            let operations = output.get("operations").cloned().ok_or_else(|| {
                bidding::agent_error::AgentError::new(
                    "AGENT_OUTPUT_INVALID",
                    "verified operations missing",
                )
            })?;
            let bytes =
                bidding::content_runtime::canonical_json_bytes(&output).map_err(|error| {
                    bidding::agent_error::AgentError::new("AGENT_OUTPUT_INVALID", error)
                })?;
            let digest = platform::sha256_hex(&bytes);
            let request_id = job.request.request_artifact_id.to_string();
            let candidate_id =
                stable_candidate_uuid(&[&request_id, &digest, "content-candidate-v1"]);
            (Some(candidate_id), Some(bytes), Some(digest), operations)
        }
    };
    let candidate = match (candidate_id, payload.as_deref(), digest.as_deref()) {
        (Some(id), Some(bytes), Some(sha256)) => Some((id, bytes, sha256)),
        (None, None, None) => None,
        _ => {
            return Err(bidding::agent_error::AgentError::new(
                "AGENT_OUTPUT_INVALID",
                "candidate publication identity incomplete",
            ));
        }
    };
    if let Some(owner) = owner {
        bidding::bid_authoring_v2::progress_content_agent_run_v1(
            pool,
            &job.request,
            owner,
            "publishing",
            serde_json::json!({"phase":"publishing"}),
        )
        .await
        .map_err(content_database_error)?;
    }
    let mut tx = pool.begin().await.map_err(content_database_error)?;
    bidding::bid_authoring_v2::assert_content_owner_in_transaction(&mut tx, &job.request, owner)
        .await
        .map_err(content_database_error)?;
    let attestation = match (existing_attestation, pending_scope) {
        (Some(attestation), None) => attestation,
        (None, Some(scope)) => {
            knowledge::knowledge_retrieval_pg::attest_compiled_requirement_evidence_v2(
                &mut tx, &scope,
            )
            .await
            .map_err(content_retrieval_error)?
        }
        _ => {
            return Err(bidding::agent_error::AgentError::new(
                "CONTENT_RETRIEVAL_INVALID_REQUEST",
                "attestation state is not closed",
            ));
        }
    };
    bidding::bid_authoring_v2::publish_content_generation_v2_in_transaction(
        &mut tx,
        &job.request,
        owner,
        (attestation.attestation_id, &attestation.attestation_sha256),
        &matches,
        candidate,
        &operations,
    )
    .await
    .map_err(content_database_error)?;
    tx.commit().await.map_err(content_database_error)?;
    Ok(())
}

async fn cancel_and_join_task_until<T>(
    task: &mut tokio::task::JoinHandle<T>,
    cancel: &CancellationToken,
    cleanup_deadline: tokio::time::Instant,
) {
    cancel.cancel();
    let abort_deadline = cleanup_deadline
        .checked_sub(TASK_ABORT_DRAIN_RESERVE)
        .unwrap_or(cleanup_deadline);
    if tokio::time::timeout_at(abort_deadline, &mut *task)
        .await
        .is_err()
    {
        task.abort();
        let _ = tokio::time::timeout_at(cleanup_deadline, &mut *task).await;
    }
}

enum ContentOwnedCompletion {
    Pipeline(Result<(), bidding::agent_error::AgentError>),
    LeaseLost(bidding::agent_error::AgentError),
    TimedOut,
    ShuttingDown,
}

struct ContentOwnedRun {
    completion: ContentOwnedCompletion,
    cleanup_deadline: tokio::time::Instant,
}

#[allow(clippy::too_many_arguments)]
async fn await_content_owned_completion(
    pipeline: &mut tokio::task::JoinHandle<Result<(), bidding::agent_error::AgentError>>,
    heartbeat: &mut tokio::task::JoinHandle<()>,
    lease_loss_rx: &mut tokio::sync::oneshot::Receiver<bidding::agent_error::AgentError>,
    pipeline_cancel: &CancellationToken,
    heartbeat_cancel: &CancellationToken,
    shutdown: &CancellationToken,
    deadline: HandlerDeadline,
) -> ContentOwnedRun {
    let completion = tokio::select! {
        biased;
        () = shutdown.cancelled() => ContentOwnedCompletion::ShuttingDown,
        () = tokio::time::sleep_until(deadline.hard) => ContentOwnedCompletion::TimedOut,
        joined = &mut *pipeline => ContentOwnedCompletion::Pipeline(joined.unwrap_or_else(|error| Err(
            bidding::agent_error::AgentError::new("INTERNAL", error.to_string())))),
        lease_loss = &mut *lease_loss_rx => ContentOwnedCompletion::LeaseLost(
            lease_loss.unwrap_or_else(|_| bidding::agent_error::AgentError::new(
                "INTERNAL", "Content heartbeat ended without a lease result"))),
    };
    let cleanup_deadline = std::cmp::min(
        tokio::time::Instant::now() + HANDLER_CLEANUP_MARGIN,
        deadline.cleanup,
    );
    let requires_effect = match &completion {
        ContentOwnedCompletion::Pipeline(Ok(())) => false,
        ContentOwnedCompletion::Pipeline(Err(error)) | ContentOwnedCompletion::LeaseLost(error) => {
            error.disposition != bidding::agent_error::RetryDisposition::Obsolete
        }
        ContentOwnedCompletion::TimedOut | ContentOwnedCompletion::ShuttingDown => true,
    };
    let teardown_deadline = teardown_deadline_for_effect(cleanup_deadline, requires_effect);
    if !matches!(completion, ContentOwnedCompletion::Pipeline(_)) {
        cancel_and_join_task_until(pipeline, pipeline_cancel, teardown_deadline).await;
    }
    cancel_and_join_task_until(heartbeat, heartbeat_cancel, teardown_deadline).await;
    ContentOwnedRun {
        completion,
        cleanup_deadline,
    }
}

async fn yield_content_retry_until(
    pool: &PgPool,
    request: &platform::BidAuthoringRequestIdentityV2,
    owner: &bidding::bid_authoring_v2::ContentRunLease,
    message: &str,
    cleanup_deadline: tokio::time::Instant,
) -> Result<bool, JobErr> {
    use bidding::bid_authoring_v2::ContentRetryYieldCode;
    terminalize_until(cleanup_deadline, "content retry yield", async {
        if let Err(failure) = bidding::bid_authoring_v2::yield_content_agent_run_v1(
            pool,
            request,
            owner,
            ContentRetryYieldCode::Internal,
            message,
        )
        .await
        {
            let failure = failure.to_string();
            if is_obsolete_effect(&failure) {
                return Ok(true);
            }
            return Err(JobErr(failure));
        }
        let status =
            bidding::bid_authoring_v2::async_request_status_v2(pool, request.request_artifact_id)
                .await
                .map_err(|failure| JobErr(failure.to_string()))?;
        if status.as_deref() != Some("pending") {
            return Err(JobErr(
                "content retry yield did not leave the request pending".into(),
            ));
        }
        Ok(false)
    })
    .await
}

async fn process_content_generate_owned_v2(
    pool: &PgPool,
    job: &ContentGenerateJobV2,
    shutdown: CancellationToken,
    deadline: HandlerDeadline,
) -> Result<(), JobErr> {
    use bidding::agent_error::RetryDisposition;
    use bidding::bid_authoring_v2::ContentRunClaim;
    let initial_claim = tokio::select! {
        biased;
        () = shutdown.cancelled() => return Err(JobErr("WORKER_SHUTDOWN".into())),
        () = tokio::time::sleep_until(deadline.hard) => None,
        result = bidding::bid_authoring_v2::claim_content_agent_run_v1(pool, &job.request) => {
            Some(result.map_err(|error| JobErr(error.to_string()))?)
        }
    };
    let claim_timed_out = initial_claim.is_none();
    let claim = match initial_claim {
        Some(claim) => claim,
        None => {
            let persistence_deadline = deadline
                .cleanup
                .checked_sub(TERMINAL_PERSISTENCE_RESERVE)
                .unwrap_or(deadline.cleanup);
            tokio::time::timeout_at(
                persistence_deadline,
                bidding::bid_authoring_v2::claim_content_agent_run_v1(pool, &job.request),
            )
            .await
            .map_err(|_| {
                JobErr("terminal content timeout owner claim exceeded cleanup reserve".into())
            })?
            .map_err(|error| JobErr(error.to_string()))?
        }
    };
    let owner = match claim {
        ContentRunClaim::Claimed(owner) => owner,
        ContentRunClaim::LiveOwner { .. }
        | ContentRunClaim::Obsolete
        | ContentRunClaim::Exhausted => return Ok(()),
    };
    if claim_timed_out {
        return terminalize_until(
            deadline.cleanup,
            "content pre-owner timeout terminalization",
            async {
                bidding::bid_authoring_v2::mark_content_generation_failed_v2(
                    pool,
                    &job.request,
                    Some(&owner),
                    "AGENT_DEADLINE_EXCEEDED",
                    "ContentGenerate handler exceeded 45 minutes during owner claim",
                )
                .await
                .map_err(|error| JobErr(error.to_string()))?;
                require_bid_request_terminal(
                    pool,
                    job.request.request_artifact_id,
                    "content pre-owner timeout",
                )
                .await
            },
        )
        .await;
    }
    let heartbeat_pool = pool.clone();
    let heartbeat_request = job.request.clone();
    let heartbeat_owner = owner.clone();
    let (lease_loss_tx, mut lease_loss_rx) = tokio::sync::oneshot::channel();
    let heartbeat_cancel = CancellationToken::new();
    let heartbeat_stop = heartbeat_cancel.clone();
    let mut heartbeat = tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
        interval.tick().await;
        loop {
            tokio::select! {
                biased;
                () = heartbeat_stop.cancelled() => break,
                _ = interval.tick() => {
                    if let Err(error) = bidding::bid_authoring_v2::heartbeat_content_agent_run_v1(
                        &heartbeat_pool,
                        &heartbeat_request,
                        &heartbeat_owner,
                    )
                    .await
                    {
                        let _ = lease_loss_tx.send(content_database_error(error));
                        break;
                    }
                }
            }
        }
    });
    let pipeline_pool = pool.clone();
    let pipeline_job = job.clone();
    let pipeline_owner = owner.clone();
    let pipeline_cancel = CancellationToken::new();
    let pipeline_stop = pipeline_cancel.clone();
    let mut pipeline = tokio::spawn(async move {
        tokio::select! {
            biased;
            () = pipeline_stop.cancelled() => Err(bidding::agent_error::AgentError::new(
                "INTERNAL", "Content generation pipeline cancelled")),
            result = process_content_generation_v2(&pipeline_pool, &pipeline_job, Some(&pipeline_owner)) => result,
        }
    });
    let owned_run = await_content_owned_completion(
        &mut pipeline,
        &mut heartbeat,
        &mut lease_loss_rx,
        &pipeline_cancel,
        &heartbeat_cancel,
        &shutdown,
        deadline,
    )
    .await;
    let cleanup_deadline = owned_run.cleanup_deadline;
    let outcome = match owned_run.completion {
        ContentOwnedCompletion::Pipeline(result) => result,
        ContentOwnedCompletion::LeaseLost(error) => Err(error),
        ContentOwnedCompletion::TimedOut => Err(bidding::agent_error::AgentError::new(
            "AGENT_DEADLINE_EXCEEDED",
            "ContentGenerate handler exceeded 45 minutes",
        )),
        ContentOwnedCompletion::ShuttingDown => Err(bidding::agent_error::AgentError::new(
            "INTERNAL",
            "worker process is shutting down",
        )),
    };
    match outcome {
        Ok(()) => Ok(()),
        Err(error) if error.disposition == RetryDisposition::Obsolete => Ok(()),
        Err(error) if error.disposition == RetryDisposition::Transient => {
            let obsolete = yield_content_retry_until(
                pool,
                &job.request,
                &owner,
                &error.message,
                cleanup_deadline,
            )
            .await?;
            if obsolete {
                Ok(())
            } else {
                Err(JobErr(error.to_string()))
            }
        }
        Err(error) => {
            let effect = terminalize_until(
                cleanup_deadline,
                &format!("content terminal {}", error.code),
                async {
                    if bid_request_is_terminal(pool, job.request.request_artifact_id).await? {
                        return Ok(());
                    }
                    bidding::bid_authoring_v2::mark_content_generation_failed_v2(
                        pool,
                        &job.request,
                        Some(&owner),
                        &error.code,
                        &error.message,
                    )
                    .await
                    .map_err(|failure| JobErr(failure.to_string()))?;
                    require_bid_request_terminal(
                        pool,
                        job.request.request_artifact_id,
                        "content generation",
                    )
                    .await
                },
            )
            .await;
            if let Err(failure) = effect {
                if is_obsolete_effect(&failure.0) {
                    return Ok(());
                }
                return Err(JobErr(format!(
                    "content generation failed ({error}); terminal transition failed ({failure})"
                )));
            }
            Ok(())
        }
    }
}

#[async_trait]
impl oxana::Worker<ContentGenerateJobV2> for ContentGenerateV2Worker {
    type Error = JobErr;

    fn max_retries(&self, _job: &ContentGenerateJobV2) -> u32 {
        platform::BID_AUTHORING_V2_MAX_RETRIES
    }

    fn retry_delay(&self, _job: &ContentGenerateJobV2, retries: u32) -> u64 {
        platform::BidAuthoringV2OxanaPolicy::retry_delay_seconds(retries)
    }

    async fn process(
        &self,
        job: ContentGenerateJobV2,
        _ctx: &oxana::JobContext,
    ) -> Result<(), Self::Error> {
        let hard_timeout = match job.operation {
            ContentGenerateOperationV2::Generate => CONTENT_GENERATE_HANDLER_HARD_TIMEOUT,
            ContentGenerateOperationV2::MatchOnly => CONTENT_MATCH_HANDLER_HARD_TIMEOUT,
        };
        let deadline = HandlerDeadline::from_now(hard_timeout);
        let Some(pool) = &self.pool else {
            return Err(JobErr("postgres not configured".into()));
        };
        match job.operation {
            ContentGenerateOperationV2::Generate => {
                process_content_generate_owned_v2(pool, &job, self.shutdown.clone(), deadline).await
            }
            ContentGenerateOperationV2::MatchOnly => {
                let pipeline_pool = pool.clone();
                let pipeline_job = job.clone();
                let pipeline_cancel = CancellationToken::new();
                let pipeline_stop = pipeline_cancel.clone();
                let mut pipeline = tokio::spawn(async move {
                    tokio::select! {
                        biased;
                        () = pipeline_stop.cancelled() => Err(bidding::agent_error::AgentError::new(
                            "INTERNAL", "Content match pipeline cancelled")),
                        result = async {
                            if bid_request_is_terminal(
                                &pipeline_pool,
                                pipeline_job.request.request_artifact_id,
                            )
                            .await
                            .map_err(|error| bidding::agent_error::AgentError::new(
                                "INTERNAL", error.to_string()))?
                            {
                                return Ok(());
                            }
                            process_content_generation_v2(&pipeline_pool, &pipeline_job, None).await
                        } => result,
                    }
                });
                enum MatchCompletion {
                    Finished(Result<(), bidding::agent_error::AgentError>),
                    TimedOut,
                    ShuttingDown,
                }
                let completion = tokio::select! {
                    biased;
                    () = self.shutdown.cancelled() => MatchCompletion::ShuttingDown,
                    () = tokio::time::sleep_until(deadline.hard) => MatchCompletion::TimedOut,
                    result = &mut pipeline => MatchCompletion::Finished(result.unwrap_or_else(|error| Err(
                        bidding::agent_error::AgentError::new("INTERNAL", error.to_string())))),
                };
                let cleanup_deadline = deadline.early_cleanup();
                if !matches!(completion, MatchCompletion::Finished(_)) {
                    let requires_effect = matches!(completion, MatchCompletion::TimedOut);
                    let teardown_deadline =
                        teardown_deadline_for_effect(cleanup_deadline, requires_effect);
                    cancel_and_join_task_until(&mut pipeline, &pipeline_cancel, teardown_deadline)
                        .await;
                }
                match completion {
                    MatchCompletion::Finished(Ok(())) => Ok(()),
                    MatchCompletion::Finished(Err(error))
                        if error.disposition
                            == bidding::agent_error::RetryDisposition::Obsolete =>
                    {
                        Ok(())
                    }
                    MatchCompletion::Finished(Err(error))
                        if error.disposition
                            == bidding::agent_error::RetryDisposition::Transient =>
                    {
                        Err(JobErr(error.to_string()))
                    }
                    MatchCompletion::Finished(Err(error)) => {
                        terminalize_until(
                            cleanup_deadline,
                            "content match terminal failure",
                            async {
                                if bid_request_is_terminal(pool, job.request.request_artifact_id)
                                    .await?
                                {
                                    return Ok(());
                                }
                                bidding::bid_authoring_v2::mark_content_generation_failed_v2(
                                    pool,
                                    &job.request,
                                    None,
                                    &error.code,
                                    &error.message,
                                )
                                .await
                                .map_err(|failure| JobErr(failure.to_string()))?;
                                require_bid_request_terminal(
                                    pool,
                                    job.request.request_artifact_id,
                                    "content match",
                                )
                                .await
                            },
                        )
                        .await
                    }
                    MatchCompletion::ShuttingDown => Err(JobErr("WORKER_SHUTDOWN".into())),
                    MatchCompletion::TimedOut => {
                        terminalize_until(
                            cleanup_deadline,
                            "content match timeout terminalization",
                            async {
                                if bid_request_is_terminal(pool, job.request.request_artifact_id)
                                    .await?
                                {
                                    return Ok(());
                                }
                                bidding::bid_authoring_v2::mark_content_generation_failed_v2(
                                    pool,
                                    &job.request,
                                    None,
                                    "CONTENT_MATCH_TIMEOUT",
                                    "ContentGenerate(match_only) exceeded 10 minutes",
                                )
                                .await
                                .map_err(|failure| JobErr(failure.to_string()))?;
                                require_bid_request_terminal(
                                    pool,
                                    job.request.request_artifact_id,
                                    "content match timeout",
                                )
                                .await
                            },
                        )
                        .await
                    }
                }
            }
        }
    }
}

#[async_trait]
impl oxana::Worker<DocumentProcessJob> for DocumentProcessWorker {
    type Error = JobErr;

    fn max_retries(&self, _job: &DocumentProcessJob) -> u32 {
        platform::DOCUMENT_PROCESS_MAX_RETRY
    }

    async fn process(
        &self,
        job: DocumentProcessJob,
        ctx: &oxana::JobContext,
    ) -> Result<(), Self::Error> {
        let Some(pool) = &self.pool else {
            return Err(JobErr("postgres not configured".into()));
        };
        let result = match tokio::time::timeout(
            std::time::Duration::from_secs(platform::DOCUMENT_PROCESS_TIMEOUT_SECS),
            convert_document(pool, job.document_id, job.attempt, &job.passages, false),
        )
        .await
        {
            Ok(r) => r,
            Err(_) => Err("document process timeout after 2h".into()),
        };
        if let Err(e) = &result
            && ctx.meta.retries >= platform::DOCUMENT_PROCESS_MAX_RETRY
            && knowledge::document_parse_status(pool, job.document_id)
                .await
                .ok()
                .flatten()
                .as_deref()
                != Some("completed")
        {
            let _ = fail_now(pool, job.document_id, job.attempt, e).await;
        }
        result.map_err(JobErr)
    }
}

#[async_trait]
impl oxana::Worker<ManualProcessJob> for DocumentProcessWorker {
    type Error = JobErr;

    fn max_retries(&self, _job: &ManualProcessJob) -> u32 {
        platform::DOCUMENT_PROCESS_MAX_RETRY
    }

    async fn process(
        &self,
        job: ManualProcessJob,
        ctx: &oxana::JobContext,
    ) -> Result<(), Self::Error> {
        let Some(pool) = &self.pool else {
            return Err(JobErr("postgres not configured".into()));
        };
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(platform::DOCUMENT_PROCESS_TIMEOUT_SECS),
            convert_document(pool, job.document_id, job.attempt, &[], true),
        )
        .await
        .unwrap_or_else(|_| Err("manual process timeout after 2h".into()));
        if let Err(error) = &result
            && ctx.meta.retries >= platform::DOCUMENT_PROCESS_MAX_RETRY
            && knowledge::document_parse_status(pool, job.document_id)
                .await
                .ok()
                .flatten()
                .as_deref()
                != Some("completed")
        {
            let _ = fail_now(pool, job.document_id, job.attempt, error).await;
        }
        result.map_err(JobErr)
    }
}

pub async fn convert_document(
    pool: &PgPool,
    document_id: Uuid,
    attempt: i32,
    passages: &[String],
    manual: bool,
) -> Result<(), String> {
    let row = sqlx::query_as::<
        _,
        (
            String,
            String,
            String,
            bool,
            String,
            serde_json::Value,
            Option<serde_json::Value>,
            Uuid,
        ),
    >(
        "SELECT d.file_name, d.file_hash, d.parse_status,
                COALESCE((v.asr_config->>'enabled')::boolean, false),
                COALESCE(v.asr_model_id, ''),
                COALESCE(v.chunking_config, '{}'::jsonb),
                d.process_overrides,
                d.product_version_id
         FROM documents d
         JOIN product_versions v ON v.id = d.product_version_id
         WHERE d.id = $1",
    )
    .bind(document_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;
    let Some((
        file_name,
        file_hash,
        parse_status,
        mut asr_enabled,
        asr_model_id,
        chunking_cfg,
        overrides_raw,
        version_id,
    )) = row
    else {
        return Err("document missing".into());
    };
    let overrides: Option<knowledge::ProcessOverrides> = overrides_raw
        .and_then(|v| serde_json::from_value(v).ok())
        .filter(|o: &knowledge::ProcessOverrides| !o.is_empty());
    if let Some(o) = &overrides
        && let Some(v) = o.asr_config.as_ref().and_then(|a| a.enabled)
    {
        asr_enabled = v;
    }
    let ext = file_name.rsplit('.').next().unwrap_or("txt");
    let parser_engine = knowledge::parser_engine_for(&chunking_cfg, overrides.as_ref(), ext);
    if parse_status == "completed" {
        return schedule_semantic_index_v2_if_ready(pool, version_id).await;
    }
    if matches!(parse_status.as_str(), "cancelled" | "deleting") {
        return Ok(());
    }
    let flipped = knowledge::try_set_processing(pool, document_id)
        .await
        .map_err(|e| e.to_string())?;
    if !flipped {
        return match knowledge::document_parse_status(pool, document_id)
            .await
            .map_err(|error| error.to_string())?
            .as_deref()
        {
            Some("completed") => schedule_semantic_index_v2_if_ready(pool, version_id).await,
            _ => Ok(()),
        };
    }
    let _ = knowledge::open_attempt(pool, document_id, attempt).await;
    tracing::info!(
        document_id = %document_id,
        file = %file_name,
        engine = %parser_engine,
        attempt,
        "parse convert start"
    );
    if !passages.is_empty() {
        let _ = knowledge::start_span(
            pool,
            document_id,
            attempt,
            knowledge::obs::SPAN_DOCREADER,
            Some(knowledge::obs::ROOT_NAME),
            Some(serde_json::json!({"engine": "passages"})),
        )
        .await;
        let _ = knowledge::finish_span(
            pool,
            document_id,
            attempt,
            knowledge::obs::SPAN_DOCREADER,
            knowledge::obs::STATUS_DONE,
            Some(serde_json::json!({"engine": "passages"})),
        )
        .await;
        let _ = knowledge::start_span(
            pool,
            document_id,
            attempt,
            knowledge::obs::SPAN_CHUNKING,
            Some(knowledge::obs::ROOT_NAME),
            None,
        )
        .await;
        let indexed = persist_passage_index(pool, document_id, version_id, passages).await?;
        let _ = knowledge::finish_span(
            pool,
            document_id,
            attempt,
            knowledge::obs::SPAN_CHUNKING,
            knowledge::obs::STATUS_DONE,
            Some(serde_json::json!({"passages": passages.len()})),
        )
        .await;
        let _ = knowledge::start_span(
            pool,
            document_id,
            attempt,
            knowledge::obs::SPAN_EMBEDDING,
            Some(knowledge::obs::ROOT_NAME),
            None,
        )
        .await;
        let _ = knowledge::finish_span(
            pool,
            document_id,
            attempt,
            knowledge::obs::SPAN_EMBEDDING,
            knowledge::obs::STATUS_DONE,
            None,
        )
        .await;
        match indexed {
            PersistIndexResult::Aborted => {}
            PersistIndexResult::Written { text_count } => {
                after_index_fanout(pool, document_id, version_id, attempt, text_count, &[], "")
                    .await?;
            }
        }
        return Ok(());
    }
    let mut convert_image_source = String::new();
    let prior_spans = document_stage_spans(pool, document_id, attempt).await;
    let markdown = if manual {
        let bytes = platform::read_blob(&file_hash).map_err(|e| e.to_string())?;
        let md = String::from_utf8_lossy(&bytes).into_owned();
        let _ = knowledge::start_span(
            pool,
            document_id,
            attempt,
            knowledge::obs::SPAN_DOCREADER,
            Some(knowledge::obs::ROOT_NAME),
            Some(serde_json::json!({"engine": "manual", "file": file_name})),
        )
        .await;
        let _ = platform::write_blob_async(&format!("{file_hash}.md"), md.as_bytes()).await;
        let _ = knowledge::finish_span(
            pool,
            document_id,
            attempt,
            knowledge::obs::SPAN_DOCREADER,
            knowledge::obs::STATUS_DONE,
            Some(serde_json::json!({"engine": "manual", "markdown_bytes": md.len()})),
        )
        .await;
        md
    } else if let Some(md) = reused_markdown(&prior_spans, &file_hash) {
        convert_image_source = reused_image_source(&prior_spans);
        tracing::info!(
            document_id = %document_id,
            md_bytes = md.len(),
            "parse convert reuse"
        );
        md
    } else {
        let bytes = platform::read_blob(&file_hash).map_err(|e| e.to_string())?;
        let (is_url, url) = parse_stored_url(&bytes);
        let engine = docparser::resolve_engine(&parser_engine, ext, is_url);
        let _ = knowledge::start_span(
            pool,
            document_id,
            attempt,
            knowledge::obs::SPAN_DOCREADER,
            Some(knowledge::obs::ROOT_NAME),
            Some(serde_json::json!({"engine": engine, "file": file_name})),
        )
        .await;
        if engine == "docreader" && docparser::reader_addr().is_none() {
            fail_pipeline(
                pool,
                document_id,
                attempt,
                knowledge::obs::SPAN_DOCREADER,
                docparser::NOT_CONFIGURED,
            )
            .await?;
            return Ok(());
        }
        let engine_overrides = overrides
            .as_ref()
            .map(|o| o.parser_engine_overrides.clone())
            .unwrap_or_default();
        let mut result = match docparser::convert_with(docparser::ConvertInput {
            engine: &parser_engine,
            file_name: &file_name,
            file_type: ext,
            is_url,
            bytes: if is_url { Vec::new() } else { bytes },
            url: &url,
            title: &file_name,
            overrides: &engine_overrides,
        })
        .await
        {
            Ok(r) => r,
            Err(e) => {
                return fail_stage_retryable(
                    pool,
                    document_id,
                    attempt,
                    knowledge::obs::SPAN_DOCREADER,
                    &e.0,
                )
                .await;
            }
        };
        if !result.error.is_empty() {
            if result.error == docparser::ASR_NOT_CONFIGURED
                || result.error == docparser::NOT_CONFIGURED
            {
                fail_pipeline(
                    pool,
                    document_id,
                    attempt,
                    knowledge::obs::SPAN_DOCREADER,
                    &result.error,
                )
                .await?;
                return Ok(());
            }
            return fail_stage_retryable(
                pool,
                document_id,
                attempt,
                knowledge::obs::SPAN_DOCREADER,
                &result.error,
            )
            .await;
        }
        if result.is_audio {
            let cfg = docparser::AsrSettings::from_version(asr_enabled, &asr_model_id);
            result = match docparser::apply_asr(result, &file_name, &cfg).await {
                Ok(r) => r,
                Err(e) => {
                    return fail_stage_retryable(
                        pool,
                        document_id,
                        attempt,
                        knowledge::obs::SPAN_DOCREADER,
                        &e,
                    )
                    .await;
                }
            };
            if !result.error.is_empty() {
                if result.error == docparser::ASR_NOT_CONFIGURED
                    || result.error == docparser::NOT_CONFIGURED
                {
                    fail_pipeline(
                        pool,
                        document_id,
                        attempt,
                        knowledge::obs::SPAN_DOCREADER,
                        &result.error,
                    )
                    .await?;
                    return Ok(());
                }
                return fail_stage_retryable(
                    pool,
                    document_id,
                    attempt,
                    knowledge::obs::SPAN_DOCREADER,
                    &result.error,
                )
                .await;
            }
        }
        tracing::info!(
            document_id = %document_id,
            parser = result.metadata.get("parser").map(String::as_str).unwrap_or(engine),
            md_bytes = result.markdown.len(),
            images = result.images.len(),
            anydoc_fallback = result.metadata.get("anydoc_fallback").map(String::as_str).unwrap_or("-"),
            "parse convert done"
        );
        result.markdown = persist_and_rewrite_images(&result).await;
        let _ = platform::write_blob_async(&format!("{file_hash}.md"), result.markdown.as_bytes())
            .await;
        let _ = knowledge::finish_span(
            pool,
            document_id,
            attempt,
            knowledge::obs::SPAN_DOCREADER,
            knowledge::obs::STATUS_DONE,
            Some(serde_json::json!({
                "engine": engine,
                "markdown_bytes": result.markdown.len(),
                "parser": result.metadata.get("parser"),
                "anydoc_version": result.metadata.get("anydoc_version"),
                "source_format": result.metadata.get("source_format"),
                "anydoc_fallback": result.metadata.get("anydoc_fallback"),
                "image_source_type": result.metadata.get("image_source_type"),
            })),
        )
        .await;
        if let Some(src) = result.metadata.get("image_source_type") {
            convert_image_source = src.clone();
        }
        result.markdown
    };
    let mut opts = version_index_opts(pool, version_id).await;
    apply_chunking_overrides(&mut opts, overrides.as_ref());
    let prior_spans = document_stage_spans(pool, document_id, attempt).await;
    let existing_chunks = knowledge::load_document_chunks(pool, document_id)
        .await
        .unwrap_or_default();
    let chunks = if knowledge::obs::stage_satisfied(&prior_spans, knowledge::obs::SPAN_CHUNKING)
        && !existing_chunks.is_empty()
    {
        tracing::info!(
            document_id = %document_id,
            chunks = existing_chunks.len(),
            "parse chunking reuse"
        );
        existing_chunks
    } else {
        let _ = knowledge::start_span(
            pool,
            document_id,
            attempt,
            knowledge::obs::SPAN_CHUNKING,
            Some(knowledge::obs::ROOT_NAME),
            None,
        )
        .await;
        let split = knowledge::chunker::split_from_config(
            &markdown,
            version_id,
            document_id,
            knowledge::chunker::SplitterConfig {
                chunk_size: opts.size,
                chunk_overlap: opts.overlap,
                strategy: opts.strategy.clone(),
                separators: opts.separators.clone(),
                token_limit: opts.token_limit,
                languages: opts.languages.clone(),
            },
            opts.parent_child,
            opts.parent_size,
            opts.child_size,
        );
        let kept = knowledge::index::keep_nonempty_chunks(split);
        knowledge::delete_graph_for_document(pool, document_id)
            .await
            .map_err(|e| e.to_string())?;
        knowledge::replace_document_chunks(pool, document_id, &kept, &[])
            .await
            .map_err(|e| e.to_string())?;
        let _ = knowledge::finish_span(
            pool,
            document_id,
            attempt,
            knowledge::obs::SPAN_CHUNKING,
            knowledge::obs::STATUS_DONE,
            Some(serde_json::json!({"chunks": kept.len()})),
        )
        .await;
        tracing::info!(
            document_id = %document_id,
            chunks = kept.len(),
            "parse chunking done"
        );
        kept
    };
    if knowledge::obs::stage_satisfied(&prior_spans, knowledge::obs::SPAN_EMBEDDING) {
        return Ok(());
    }
    let _ = knowledge::start_span(
        pool,
        document_id,
        attempt,
        knowledge::obs::SPAN_EMBEDDING,
        Some(knowledge::obs::ROOT_NAME),
        None,
    )
    .await;
    let indexed =
        match persist_document_embeddings(pool, document_id, &chunks, opts.vector, opts.keyword)
            .await
        {
            Ok(v) => v,
            Err(e) => {
                return fail_stage_retryable(
                    pool,
                    document_id,
                    attempt,
                    knowledge::obs::SPAN_EMBEDDING,
                    &e,
                )
                .await;
            }
        };
    let _ = knowledge::finish_span(
        pool,
        document_id,
        attempt,
        knowledge::obs::SPAN_EMBEDDING,
        knowledge::obs::STATUS_DONE,
        None,
    )
    .await;
    tracing::info!(
        document_id = %document_id,
        chunks = chunks.len(),
        "parse embedding done"
    );
    let images = knowledge::enrichment::markdown_image_keys(&markdown);
    let mut mm = knowledge::version_multimodal_enabled(pool, version_id)
        .await
        .unwrap_or(false);
    if let Some(v) = overrides.as_ref().and_then(|o| o.enable_multimodel) {
        mm = v;
    }
    match indexed {
        PersistIndexResult::Aborted => {}
        PersistIndexResult::Written { text_count } => {
            let mm_images: Vec<String> = if mm { images } else { Vec::new() };
            after_index_fanout(
                pool,
                document_id,
                version_id,
                attempt,
                text_count,
                &mm_images,
                if convert_image_source.is_empty() {
                    knowledge::enrichment::image_source_type(&file_name, &markdown)
                } else {
                    convert_image_source.as_str()
                },
            )
            .await?;
        }
    }
    Ok(())
}

async fn persist_passage_index(
    pool: &PgPool,
    document_id: Uuid,
    version_id: Uuid,
    passages: &[String],
) -> Result<PersistIndexResult, String> {
    let file_hash: String =
        sqlx::query_scalar("SELECT COALESCE(file_hash,'') FROM documents WHERE id = $1")
            .bind(document_id)
            .fetch_one(pool)
            .await
            .unwrap_or_default();
    if !file_hash.is_empty() {
        let joined = passages.join("\n\n");
        let _ =
            platform::write_blob_async(format!("{file_hash}.md").as_str(), joined.as_bytes()).await;
    }
    let chunks: Vec<knowledge::Chunk> = passages
        .iter()
        .map(|text| knowledge::Chunk {
            id: Uuid::new_v4(),
            document_id,
            product_version_id: version_id,
            chunk_type: "text".into(),
            content: text.clone(),
            context_header: String::new(),
            start_at: 0,
            end_at: text.chars().count() as i32,
            parent_chunk_id: None,
            generated_questions: Vec::new(),
        })
        .collect();
    let opts = version_index_opts(pool, version_id).await;
    persist_indexed_chunks(
        pool,
        document_id,
        version_id,
        &chunks,
        opts.vector,
        opts.keyword,
    )
    .await
}

enum PersistIndexResult {
    Aborted,
    Written { text_count: usize },
}

async fn document_parse_status(pool: &PgPool, document_id: Uuid) -> Option<String> {
    sqlx::query_scalar("SELECT parse_status FROM documents WHERE id = $1")
        .bind(document_id)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
}

fn status_aborted(st: Option<&str>) -> bool {
    matches!(st, Some("cancelled") | Some("deleting"))
}

async fn after_index_fanout(
    pool: &PgPool,
    document_id: Uuid,
    version_id: Uuid,
    attempt: i32,
    text_count: usize,
    images: &[String],
    file_name: &str,
) -> Result<(), String> {
    if !images.is_empty() {
        if !platform::vlm_configured() {
            let _ = knowledge::skip_span(
                pool,
                document_id,
                attempt,
                knowledge::obs::SPAN_MULTIMODAL,
                "vlm not configured",
            )
            .await;
            let _ = knowledge::set_parse_status(
                pool,
                document_id,
                "finalizing",
                "ocr_error: vlm not configured; caption_error: vlm not configured",
            )
            .await;
            let _ = knowledge::set_index_ready(pool, document_id, false).await;
            tracing::warn!(
                document_id = %document_id,
                reason = "vlm not configured",
                images = images.len(),
                "parse multimodal hold"
            );
            if text_count > 0 {
                maybe_start_postprocess(pool, document_id, version_id, attempt).await;
            }
            return Ok(());
        }
        let _ = knowledge::start_span(
            pool,
            document_id,
            attempt,
            knowledge::obs::SPAN_MULTIMODAL,
            Some(knowledge::obs::ROOT_NAME),
            Some(serde_json::json!({"images": images.len()})),
        )
        .await;
        tracing::info!(
            document_id = %document_id,
            images = images.len(),
            "parse multimodal enqueue"
        );
        knowledge::enrichment::set_pending_count(document_id, images.len() as i32);
        let mut leftover = images.len() as i32;
        for key in images {
            match platform::enqueue_image_multimodal(
                document_id,
                key,
                file_name,
                true,
                true,
                attempt,
            )
            .await
            {
                Ok(Some(_)) => {}
                Ok(None) | Err(_) => {
                    leftover -= 1;
                    knowledge::enrichment::decr_pending_count(document_id);
                }
            }
        }
        if leftover <= 0 {
            let _ = knowledge::skip_span(
                pool,
                document_id,
                attempt,
                knowledge::obs::SPAN_MULTIMODAL,
                "enqueue failed",
            )
            .await;
            let _ = knowledge::set_parse_status(
                pool,
                document_id,
                "finalizing",
                "ocr_error: image enqueue failed; caption_error: image enqueue failed",
            )
            .await;
            let _ = knowledge::set_index_ready(pool, document_id, false).await;
            tracing::warn!(
                document_id = %document_id,
                reason = "enqueue failed",
                "parse multimodal hold"
            );
            if text_count > 0 {
                maybe_start_postprocess(pool, document_id, version_id, attempt).await;
            }
        }
        return Ok(());
    }
    let _ = knowledge::skip_span(
        pool,
        document_id,
        attempt,
        knowledge::obs::SPAN_MULTIMODAL,
        "no images",
    )
    .await;
    let _ = knowledge::set_index_ready(pool, document_id, true).await;
    tracing::info!(document_id = %document_id, index_ready = true, "parse completed");
    if text_count == 0 {
        let _ = knowledge::set_parse_status(pool, document_id, "completed", "").await;
        let _ = knowledge::skip_span(
            pool,
            document_id,
            attempt,
            knowledge::obs::SPAN_POSTPROCESS,
            "no further work",
        )
        .await;
        return schedule_semantic_index_v2_if_ready(pool, version_id).await;
    }
    maybe_start_postprocess(pool, document_id, version_id, attempt).await;
    schedule_semantic_index_v2_if_ready(pool, version_id).await
}

struct VersionIndexOpts {
    size: usize,
    overlap: usize,
    strategy: String,
    parent_child: bool,
    parent_size: usize,
    child_size: usize,
    separators: Vec<String>,
    token_limit: usize,
    languages: Vec<String>,
    vector: bool,
    keyword: bool,
}

fn apply_chunking_overrides(
    opts: &mut VersionIndexOpts,
    overrides: Option<&knowledge::ProcessOverrides>,
) {
    let Some(o) = overrides else {
        return;
    };
    let Some(c) = &o.chunking_config else {
        return;
    };
    if let Some(n) = c.chunk_size.filter(|n| *n > 0) {
        opts.size = n;
    }
    if let Some(n) = c.chunk_overlap.filter(|n| *n > 0) {
        opts.overlap = n;
    }
    if let Some(s) = c.strategy.as_ref().filter(|s| !s.is_empty()) {
        opts.strategy = s.clone();
    }
    opts.parent_child = c.enable_parent_child;
    if let Some(n) = c.parent_chunk_size.filter(|n| *n > 0) {
        opts.parent_size = n;
    }
    if let Some(n) = c.child_chunk_size.filter(|n| *n > 0) {
        opts.child_size = n;
    }
    if !c.separators.is_empty() {
        opts.separators = c.separators.clone();
    }
    if let Some(n) = c.token_limit.filter(|n| *n > 0) {
        opts.token_limit = n;
    }
    if !c.languages.is_empty() {
        opts.languages = c.languages.clone();
    }
}

async fn version_index_opts(pool: &PgPool, version_id: Uuid) -> VersionIndexOpts {
    let row: Option<(serde_json::Value, serde_json::Value)> = sqlx::query_as(
        "SELECT COALESCE(chunking_config, '{}'::jsonb), COALESCE(indexing_strategy, '{}'::jsonb)
         FROM product_versions WHERE id = $1",
    )
    .bind(version_id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let (chunking, indexing) = row.unwrap_or((serde_json::json!({}), serde_json::json!({})));
    VersionIndexOpts {
        size: chunking
            .get("chunk_size")
            .and_then(|v| v.as_u64())
            .unwrap_or(512) as usize,
        overlap: chunking
            .get("chunk_overlap")
            .and_then(|v| v.as_u64())
            .unwrap_or(80) as usize,
        strategy: chunking
            .get("strategy")
            .and_then(|v| v.as_str())
            .unwrap_or("auto")
            .to_string(),
        parent_child: chunking
            .get("enable_parent_child")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        parent_size: chunking
            .get("parent_chunk_size")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize,
        child_size: chunking
            .get("child_chunk_size")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize,
        separators: chunking
            .get("separators")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|s| s.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        token_limit: chunking
            .get("token_limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize,
        languages: chunking
            .get("languages")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|s| s.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        vector: indexing
            .get("vector")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
        keyword: indexing
            .get("keyword")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
    }
}

async fn persist_indexed_chunks(
    pool: &PgPool,
    document_id: Uuid,
    _version_id: Uuid,
    chunks: &[knowledge::Chunk],
    vector_on: bool,
    keyword_on: bool,
) -> Result<PersistIndexResult, String> {
    if status_aborted(document_parse_status(pool, document_id).await.as_deref()) {
        return Ok(PersistIndexResult::Aborted);
    }
    let kept = knowledge::index::keep_nonempty_chunks(chunks.to_vec());
    knowledge::delete_graph_for_document(pool, document_id)
        .await
        .map_err(|e| e.to_string())?;
    // Chunk rows first so an embed failure does not throw away the split.
    knowledge::replace_document_chunks(pool, document_id, &kept, &[])
        .await
        .map_err(|e| e.to_string())?;
    persist_document_embeddings(pool, document_id, &kept, vector_on, keyword_on).await
}

async fn persist_document_embeddings(
    pool: &PgPool,
    document_id: Uuid,
    chunks: &[knowledge::Chunk],
    vector_on: bool,
    keyword_on: bool,
) -> Result<PersistIndexResult, String> {
    if status_aborted(document_parse_status(pool, document_id).await.as_deref()) {
        return Ok(PersistIndexResult::Aborted);
    }
    let title: String = sqlx::query_scalar("SELECT title FROM documents WHERE id = $1")
        .bind(document_id)
        .fetch_one(pool)
        .await
        .unwrap_or_default();
    let model_id: String = sqlx::query_scalar(
        "SELECT COALESCE(pv.embedding_model_id, '')
         FROM documents d
         JOIN product_versions pv ON pv.id = d.product_version_id
         WHERE d.id = $1",
    )
    .bind(document_id)
    .fetch_one(pool)
    .await
    .unwrap_or_default();
    let embeddings =
        knowledge::index::index_chunks(chunks, &title, vector_on, keyword_on, &model_id)?;
    if status_aborted(document_parse_status(pool, document_id).await.as_deref()) {
        return Ok(PersistIndexResult::Aborted);
    }
    knowledge::replace_document_embeddings(pool, document_id, &embeddings)
        .await
        .map_err(|e| e.to_string())?;
    let st = document_parse_status(pool, document_id).await;
    if status_aborted(st.as_deref()) {
        if st.as_deref() == Some("deleting") {
            let _ = knowledge::purge_document_index(pool, document_id).await;
        }
        return Ok(PersistIndexResult::Aborted);
    }
    sqlx::query(
        "UPDATE documents SET enable_status = 'enabled', processed_at = now(),
                summary_status = 'none', updated_at = now()
         WHERE id = $1",
    )
    .bind(document_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    let text_count = chunks.iter().filter(|c| c.chunk_type == "text").count();
    Ok(PersistIndexResult::Written { text_count })
}

async fn document_stage_spans(
    pool: &PgPool,
    document_id: Uuid,
    attempt: i32,
) -> Vec<knowledge::Span> {
    knowledge::list_spans_attempt(pool, document_id, attempt)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|r| r.into_span())
        .collect()
}

fn reused_image_source(spans: &[knowledge::Span]) -> String {
    let Some(span) = spans
        .iter()
        .find(|s| s.name == knowledge::obs::SPAN_DOCREADER)
    else {
        return String::new();
    };
    image_source_from_docreader_output(span.output.as_ref())
}

fn image_source_from_docreader_output(output: Option<&serde_json::Value>) -> String {
    let Some(v) = output else {
        return String::new();
    };
    if let Some(t) = v
        .get("image_source_type")
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
    {
        return t.to_string();
    }
    if v.get("anydoc_fallback").and_then(|x| x.as_str()) == Some("scanned_pdf") {
        return "scanned_pdf".into();
    }
    String::new()
}

fn reused_markdown(spans: &[knowledge::Span], file_hash: &str) -> Option<String> {
    if !knowledge::obs::stage_satisfied(spans, knowledge::obs::SPAN_DOCREADER) {
        return None;
    }
    let bytes = platform::read_blob(&format!("{file_hash}.md")).ok()?;
    if bytes.is_empty() {
        return None;
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

fn parse_stored_url(bytes: &[u8]) -> (bool, String) {
    let text = String::from_utf8_lossy(bytes);
    let t = text.trim();
    let Some(rest) = t.strip_prefix("url:") else {
        return (false, String::new());
    };
    if rest.starts_with("http://") || rest.starts_with("https://") {
        (true, rest.to_string())
    } else {
        (false, String::new())
    }
}

async fn persist_and_rewrite_images(result: &docparser::ReadResult) -> String {
    let (md, blobs) = docparser::rewrite_images(result).await;
    if blobs.is_empty() {
        return md;
    }
    let _ = tokio::task::spawn_blocking(move || {
        for (hash, data) in blobs {
            if let Err(e) = platform::write_blob(&hash, &data) {
                tracing::warn!(hash = %hash, error = %e, "image persist failed");
            }
        }
    })
    .await;
    md
}

async fn maybe_start_postprocess(pool: &PgPool, document_id: Uuid, version_id: Uuid, attempt: i32) {
    let rows = knowledge::list_spans_attempt(pool, document_id, attempt)
        .await
        .unwrap_or_default();
    let spans: Vec<_> = rows.into_iter().map(|r| r.into_span()).collect();
    if !knowledge::obs::can_start_stage_or_legacy(knowledge::obs::SPAN_POSTPROCESS, &spans) {
        return;
    }
    let _ = knowledge::start_span(
        pool,
        document_id,
        attempt,
        knowledge::obs::SPAN_POSTPROCESS,
        Some(knowledge::obs::ROOT_NAME),
        None,
    )
    .await;
    let _ = platform::enqueue_post_process(document_id, version_id, false).await;
}

async fn fail_stage_retryable(
    pool: &PgPool,
    document_id: Uuid,
    attempt: i32,
    stage: &str,
    message: &str,
) -> Result<(), String> {
    let _ = knowledge::finish_span(
        pool,
        document_id,
        attempt,
        stage,
        knowledge::obs::STATUS_FAILED,
        Some(serde_json::json!({"error": message})),
    )
    .await;
    let _ = knowledge::cancel_dependent_stages(pool, document_id, attempt, stage).await;
    tracing::error!(document_id = %document_id, stage, error = %message, "parse stage fail");
    Err(message.into())
}

async fn fail_pipeline(
    pool: &PgPool,
    document_id: Uuid,
    attempt: i32,
    stage: &str,
    message: &str,
) -> Result<(), String> {
    let _ = knowledge::finish_span(
        pool,
        document_id,
        attempt,
        stage,
        knowledge::obs::STATUS_FAILED,
        Some(serde_json::json!({"error": message})),
    )
    .await;
    let _ = knowledge::cancel_dependent_stages(pool, document_id, attempt, stage).await;
    let _ = knowledge::finish_span(
        pool,
        document_id,
        attempt,
        knowledge::obs::ROOT_NAME,
        knowledge::obs::STATUS_FAILED,
        Some(serde_json::json!({"error": message})),
    )
    .await;
    tracing::error!(document_id = %document_id, stage, error = %message, "parse stage fail");
    fail_now(pool, document_id, attempt, message).await
}

async fn fail_now(
    pool: &PgPool,
    document_id: Uuid,
    attempt: i32,
    message: &str,
) -> Result<(), String> {
    let _ = attempt;
    knowledge::set_parse_status(pool, document_id, "failed", message)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub struct VersionCloneWorker {
    pool: Option<PgPool>,
}

impl oxana::FromContext<AppCtx> for VersionCloneWorker {
    fn from_context(ctx: &AppCtx) -> Self {
        Self {
            pool: ctx.pool.clone(),
        }
    }
}

#[async_trait]
impl oxana::Worker<VersionCloneJob> for VersionCloneWorker {
    type Error = JobErr;

    fn max_retries(&self, _job: &VersionCloneJob) -> u32 {
        3
    }

    async fn process(
        &self,
        job: VersionCloneJob,
        _ctx: &oxana::JobContext,
    ) -> Result<(), Self::Error> {
        let Some(pool) = &self.pool else {
            return Err(JobErr("postgres not configured".into()));
        };
        process_version_clone(pool, &job).await.map_err(JobErr)
    }
}

pub async fn process_version_clone(pool: &PgPool, job: &VersionCloneJob) -> Result<(), String> {
    let diffs: Vec<knowledge::clone::CloneDiff> = match &job.diffs {
        serde_json::Value::Array(a) => a
            .iter()
            .filter_map(|v| serde_json::from_value(v.clone()).ok())
            .collect(),
        _ => Vec::new(),
    };
    let follow = knowledge::clone::run_clone(
        pool,
        job.source_version_id,
        job.target_version_id,
        &diffs,
        job.make_current,
    )
    .await?;
    for f in follow {
        if f.clone_keep || f.task_type == platform::TYPE_POST_PROCESS {
            let _ =
                platform::enqueue_post_process(f.document_id, f.product_version_id, f.clone_keep)
                    .await;
        } else {
            let _ =
                platform::enqueue_document_process(f.document_id, f.product_version_id, 1).await;
        }
    }
    Ok(())
}

pub struct PostProcessWorker {
    pool: Option<PgPool>,
}

impl oxana::FromContext<AppCtx> for PostProcessWorker {
    fn from_context(ctx: &AppCtx) -> Self {
        Self {
            pool: ctx.pool.clone(),
        }
    }
}

#[async_trait]
impl oxana::Worker<PostProcessJob> for PostProcessWorker {
    type Error = JobErr;

    fn max_retries(&self, _job: &PostProcessJob) -> u32 {
        3
    }

    async fn process(
        &self,
        job: PostProcessJob,
        ctx: &oxana::JobContext,
    ) -> Result<(), Self::Error> {
        let Some(pool) = &self.pool else {
            return Err(JobErr("postgres not configured".into()));
        };
        let result = match tokio::time::timeout(
            std::time::Duration::from_secs(platform::POST_PROCESS_TIMEOUT_SECS),
            process_post_process(
                pool,
                job.document_id,
                job.product_version_id,
                job.clone_keep,
            ),
        )
        .await
        {
            Ok(r) => r,
            Err(_) => Err("post_process timeout after 30min".into()),
        };
        if let Err(e) = &result
            && ctx.meta.retries >= 3
            && knowledge::document_parse_status(pool, job.document_id)
                .await
                .ok()
                .flatten()
                .as_deref()
                != Some("completed")
        {
            let _ = fail_now(pool, job.document_id, 0, e).await;
        }
        result.map_err(JobErr)
    }
}

pub struct KnowledgeSemanticIndexV2Worker {
    pool: Option<PgPool>,
    provider: Option<Arc<dyn knowledge::knowledge_index_v2::VectorEmbeddingProviderV2>>,
    provider_configuration_error: Option<String>,
}

impl oxana::FromContext<AppCtx> for KnowledgeSemanticIndexV2Worker {
    fn from_context(ctx: &AppCtx) -> Self {
        let provider_result = knowledge::knowledge_index_v2::StrictVectorEmbeddingClientV2::new(
            Arc::new(knowledge::knowledge_index_v2::EnvironmentEmbeddingCredentialResolverV2),
        );
        let provider_configuration_error = provider_result.as_ref().err().map(ToString::to_string);
        let provider = provider_result.ok().map(|provider| {
            Arc::new(provider) as Arc<dyn knowledge::knowledge_index_v2::VectorEmbeddingProviderV2>
        });
        Self {
            pool: ctx.pool.clone(),
            provider,
            provider_configuration_error,
        }
    }
}

#[async_trait]
impl oxana::Worker<KnowledgeSemanticIndexV2Job> for KnowledgeSemanticIndexV2Worker {
    type Error = JobErr;

    fn max_retries(&self, _job: &KnowledgeSemanticIndexV2Job) -> u32 {
        platform::SEMANTIC_INDEX_V2_MAX_RETRY
    }

    fn retry_delay(&self, _job: &KnowledgeSemanticIndexV2Job, _retries: u32) -> u64 {
        platform::SEMANTIC_INDEX_V2_RETRY_DELAY_SECS
    }

    async fn process(
        &self,
        job: KnowledgeSemanticIndexV2Job,
        ctx: &oxana::JobContext,
    ) -> Result<(), Self::Error> {
        let Some(pool) = &self.pool else {
            return Err(JobErr("postgres not configured".into()));
        };
        tracing::info!(
            target_id = %job.target_id,
            target_revision = job.target_revision,
            oxana_retry = ctx.meta.retries,
            "knowledge semantic index v2 attempt"
        );
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(platform::SEMANTIC_INDEX_V2_TIMEOUT_SECS),
            process_semantic_index_intent_v2(
                pool,
                job.target_id,
                job.target_revision,
                self.provider.as_deref(),
                self.provider_configuration_error.as_deref(),
            ),
        )
        .await;
        let successor = match result {
            Ok(Ok(successor)) => successor,
            Ok(Err(error)) => {
                tracing::warn!(
                    target_id = %job.target_id,
                    target_revision = job.target_revision,
                    error = %error,
                    "knowledge semantic index v2 retryable failure"
                );
                return Err(JobErr(error));
            }
            Err(_) => {
                let detail = "semantic index v2 timeout after 2h";
                knowledge::knowledge_index_v2::record_semantic_index_error_v2(
                    pool,
                    job.target_id,
                    job.target_revision,
                    "retryable",
                    "WORKER_TIMEOUT",
                    detail,
                )
                .await
                .map_err(|error| JobErr(error.to_string()))?;
                return Err(JobErr(detail.into()));
            }
        };
        if let Some(successor) = successor {
            enqueue_semantic_index_v2_target(&successor).await?;
        }
        Ok(())
    }
}

async fn enqueue_semantic_index_v2_target(
    target: &knowledge::knowledge_index_v2::SemanticIndexIntentV2,
) -> Result<(), JobErr> {
    match platform::enqueue_semantic_index_v2(target.id, target.target_revision).await {
        Ok(Some(_)) => Ok(()),
        Ok(None) => Err(JobErr("semantic index v2 queue unavailable".into())),
        Err(error) => Err(JobErr(error)),
    }
}

fn bounded_semantic_index_error_detail(error: &str) -> String {
    let mut end = error.len().min(512);
    while !error.is_char_boundary(end) {
        end -= 1;
    }
    let detail = error[..end].trim();
    if detail.is_empty() {
        "semantic index v2 failure".into()
    } else {
        detail.into()
    }
}

pub async fn process_semantic_index_intent_v2(
    pool: &PgPool,
    target_id: Uuid,
    target_revision: i64,
    provider: Option<&dyn knowledge::knowledge_index_v2::VectorEmbeddingProviderV2>,
    provider_configuration_error: Option<&str>,
) -> Result<Option<knowledge::knowledge_index_v2::SemanticIndexIntentV2>, String> {
    use knowledge::knowledge_index_v2::{
        SemanticIndexCompletionV2, SemanticIndexPreflightV2, VectorIndexErrorV2,
    };

    let Some(intent) =
        knowledge::knowledge_index_v2::semantic_index_intent_v2(pool, target_id, target_revision)
            .await
            .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    match knowledge::knowledge_index_v2::preflight_semantic_index_intent_v2(
        pool,
        target_id,
        target_revision,
    )
    .await
    .map_err(|error| error.to_string())?
    {
        SemanticIndexPreflightV2::Current => {}
        SemanticIndexPreflightV2::PendingDerived => {
            let detail = "semantic source has pending derived work";
            let _ = knowledge::knowledge_index_v2::record_semantic_index_intent_v2(
                pool,
                &intent,
                "retryable",
                "PENDING_DERIVED",
                detail,
            )
            .await;
            return Err(detail.into());
        }
        SemanticIndexPreflightV2::Superseded => {
            return prepare_semantic_index_v2_successor(pool, intent.product_version_id).await;
        }
        SemanticIndexPreflightV2::Completed
        | SemanticIndexPreflightV2::Terminal
        | SemanticIndexPreflightV2::Duplicate => return Ok(None),
    }

    let Some(provider) = provider else {
        let detail = bounded_semantic_index_error_detail(
            provider_configuration_error
                .unwrap_or("strict V2 vector provider could not be configured"),
        );
        knowledge::knowledge_index_v2::record_semantic_index_intent_v2(
            pool,
            &intent,
            "terminal",
            "CLIENT_CONFIGURATION_INVALID",
            &detail,
        )
        .await
        .map_err(|error| error.to_string())?;
        return Ok(None);
    };

    let result = async {
        knowledge::knowledge_index_v2::rebuild_semantic_keyword_indexes_v2(pool, &intent).await?;
        let has_vector =
            knowledge::knowledge_index_v2::semantic_vector_generation_matches_intent_v2(
                pool, &intent,
            )
            .await
            .map_err(VectorIndexErrorV2::Database)?;
        if !has_vector {
            knowledge::knowledge_index_v2::rebuild_vector_indexes_for_intent_v2(
                pool, &intent, provider,
            )
            .await?;
        }
        knowledge::knowledge_index_v2::complete_semantic_index_intent_v2(pool, &intent)
            .await
            .map_err(VectorIndexErrorV2::Database)
    }
    .await;

    match result {
        Ok(SemanticIndexCompletionV2::Completed | SemanticIndexCompletionV2::Duplicate) => {
            tracing::info!(
                target_id = %intent.id,
                target_revision = intent.target_revision,
                product_version_id = %intent.product_version_id,
                source_snapshot_sha256 = %intent.source_snapshot_sha256,
                embedding_revision_sha256 = %intent.embedding_revision_sha256,
                "knowledge semantic index v2 ready"
            );
            Ok(None)
        }
        Ok(SemanticIndexCompletionV2::Superseded) => {
            prepare_semantic_index_v2_successor(pool, intent.product_version_id).await
        }
        Ok(SemanticIndexCompletionV2::Terminal) => Ok(None),
        Ok(SemanticIndexCompletionV2::PendingDerived | SemanticIndexCompletionV2::NotReady) => {
            let detail = "semantic readiness is not yet publishable";
            let _ = knowledge::knowledge_index_v2::record_semantic_index_intent_v2(
                pool,
                &intent,
                "retryable",
                "SEMANTIC_SOURCE_NOT_SETTLED",
                detail,
            )
            .await;
            Err(detail.into())
        }
        Err(VectorIndexErrorV2::SnapshotChanged(error)) => {
            let detail = bounded_semantic_index_error_detail(&error);
            knowledge::knowledge_index_v2::record_semantic_index_intent_v2(
                pool,
                &intent,
                "superseded",
                "SOURCE_GENERATION_CHANGED",
                &detail,
            )
            .await
            .map_err(|record_error| record_error.to_string())?;
            prepare_semantic_index_v2_successor(pool, intent.product_version_id).await
        }
        Err(VectorIndexErrorV2::PendingDerived(error)) => {
            let detail = bounded_semantic_index_error_detail(&error);
            let _ = knowledge::knowledge_index_v2::record_semantic_index_intent_v2(
                pool,
                &intent,
                "retryable",
                "PENDING_DERIVED",
                &detail,
            )
            .await;
            Err(detail)
        }
        Err(VectorIndexErrorV2::InvalidConfiguration(error)) => {
            let detail = bounded_semantic_index_error_detail(&error);
            knowledge::knowledge_index_v2::record_semantic_index_intent_v2(
                pool,
                &intent,
                "terminal",
                "INVALID_IMMUTABLE_CONFIGURATION",
                &detail,
            )
            .await
            .map_err(|record_error| record_error.to_string())?;
            Ok(None)
        }
        Err(error @ (VectorIndexErrorV2::Unavailable(_) | VectorIndexErrorV2::Database(_))) => {
            let detail = bounded_semantic_index_error_detail(&error.to_string());
            let error_code = match error {
                VectorIndexErrorV2::Unavailable(_) => "PROVIDER_UNAVAILABLE",
                VectorIndexErrorV2::Database(_) => "DATABASE_UNAVAILABLE",
                VectorIndexErrorV2::InvalidConfiguration(_)
                | VectorIndexErrorV2::PendingDerived(_)
                | VectorIndexErrorV2::SnapshotChanged(_) => unreachable!(),
            };
            let _ = knowledge::knowledge_index_v2::record_semantic_index_intent_v2(
                pool,
                &intent,
                "retryable",
                error_code,
                &detail,
            )
            .await;
            Err(detail)
        }
    }
}

async fn prepare_semantic_index_v2_successor(
    pool: &PgPool,
    product_version_id: Uuid,
) -> Result<Option<knowledge::knowledge_index_v2::SemanticIndexIntentV2>, String> {
    use knowledge::knowledge_index_v2::SemanticIndexPreparationV2;
    match knowledge::knowledge_index_v2::prepare_semantic_index_intent_v2(pool, product_version_id)
        .await
        .map_err(|error| error.to_string())?
    {
        SemanticIndexPreparationV2::Enqueue(successor) => Ok(Some(successor)),
        SemanticIndexPreparationV2::PendingDerived => {
            Err("semantic successor source has pending derived work".into())
        }
        SemanticIndexPreparationV2::Unbound
        | SemanticIndexPreparationV2::Ready(_)
        | SemanticIndexPreparationV2::Terminal(_)
        | SemanticIndexPreparationV2::Superseded(_) => Ok(None),
    }
}

pub async fn schedule_semantic_index_v2_if_ready(
    pool: &PgPool,
    product_version_id: Uuid,
) -> Result<(), String> {
    schedule_semantic_index_v2_if_ready_with(pool, product_version_id, |target_id, revision| {
        platform::enqueue_semantic_index_v2(target_id, revision)
    })
    .await
}

async fn schedule_semantic_index_v2_if_ready_with<F, Fut>(
    pool: &PgPool,
    product_version_id: Uuid,
    enqueue: F,
) -> Result<(), String>
where
    F: FnOnce(Uuid, i64) -> Fut,
    Fut: std::future::Future<Output = Result<Option<String>, String>>,
{
    use knowledge::knowledge_index_v2::SemanticIndexPreparationV2;
    match knowledge::knowledge_index_v2::prepare_semantic_index_intent_v2(pool, product_version_id)
        .await
        .map_err(|error| error.to_string())?
    {
        SemanticIndexPreparationV2::Enqueue(intent) => {
            match enqueue(intent.id, intent.target_revision).await {
                Ok(Some(_)) => Ok(()),
                Ok(None) => Err("semantic index v2 queue unavailable".into()),
                Err(error) => Err(error),
            }
        }
        SemanticIndexPreparationV2::Unbound
        | SemanticIndexPreparationV2::PendingDerived
        | SemanticIndexPreparationV2::Ready(_)
        | SemanticIndexPreparationV2::Terminal(_)
        | SemanticIndexPreparationV2::Superseded(_) => Ok(()),
    }
}

pub async fn process_post_process(
    pool: &PgPool,
    document_id: Uuid,
    product_version_id: Uuid,
    clone_keep: bool,
) -> Result<(), String> {
    knowledge::pipeline::run_post_process(pool, document_id, product_version_id, clone_keep).await
}

pub struct HousekeepWorker {
    pool: Option<PgPool>,
}

impl oxana::FromContext<AppCtx> for HousekeepWorker {
    fn from_context(ctx: &AppCtx) -> Self {
        Self {
            pool: ctx.pool.clone(),
        }
    }
}

#[async_trait]
impl oxana::Worker<HousekeepJob> for HousekeepWorker {
    type Error = JobErr;

    fn cron_schedule() -> Option<String> {
        Some(platform::HOUSEKEEP_CRON.into())
    }

    fn cron_queue_config() -> Option<oxana::QueueConfig> {
        Some(<LowQueue as oxana::Queue>::to_config())
    }

    async fn process(
        &self,
        _job: HousekeepJob,
        _ctx: &oxana::JobContext,
    ) -> Result<(), Self::Error> {
        if !platform::housekeep_enabled() {
            return Ok(());
        }
        let Some(pool) = &self.pool else {
            return Err(JobErr("postgres not configured".into()));
        };
        knowledge::housekeep_documents(pool, platform::HOUSEKEEP_STALE_SECS)
            .await
            .map_err(|e| JobErr(e.to_string()))?;
        // Request terminalization follows delivered work; do not scan pending rows.
        let _ = platform::HOUSEKEEP_STALE_SECS;
        Ok(())
    }
}

pub struct ImageMultimodalWorker {
    pool: Option<PgPool>,
}

impl oxana::FromContext<AppCtx> for ImageMultimodalWorker {
    fn from_context(ctx: &AppCtx) -> Self {
        Self {
            pool: ctx.pool.clone(),
        }
    }
}

#[async_trait]
impl oxana::Worker<ImageMultimodalJob> for ImageMultimodalWorker {
    type Error = JobErr;

    fn max_retries(&self, _job: &ImageMultimodalJob) -> u32 {
        3
    }

    async fn process(
        &self,
        job: ImageMultimodalJob,
        ctx: &oxana::JobContext,
    ) -> Result<(), Self::Error> {
        let Some(pool) = &self.pool else {
            return Err(JobErr("postgres not configured".into()));
        };
        let result = process_image_pg(
            pool,
            job.document_id,
            &job.image_key,
            &job.image_source_type,
            job.enable_ocr,
            job.enable_caption,
            job.attempt,
        )
        .await;
        if let Err(e) = &result
            && ctx.meta.retries >= 3
        {
            let _ = knowledge::set_parse_status(
                pool,
                job.document_id,
                "finalizing",
                &format!("ocr_error: {e}; caption_error: {e}"),
            )
            .await;
            finalize_multimodal_pg(pool, job.document_id, job.attempt).await;
            return Err(JobErr(e.clone()));
        }
        result.map_err(JobErr)
    }
}

pub async fn process_image_pg(
    pool: &PgPool,
    document_id: Uuid,
    image_key: &str,
    image_source_type: &str,
    enable_ocr: bool,
    enable_caption: bool,
    attempt: i32,
) -> Result<(), String> {
    knowledge::pipeline::run_image(
        pool,
        document_id,
        image_key,
        image_source_type,
        enable_ocr,
        enable_caption,
        attempt,
    )
    .await
}

async fn finalize_multimodal_pg(pool: &PgPool, document_id: Uuid, attempt: i32) {
    knowledge::pipeline::finalize_multimodal(pool, document_id, attempt).await
}

pub async fn process_wiki_finalize(
    pool: &PgPool,
    version_id: Uuid,
    document_id: Uuid,
) -> Result<(), String> {
    knowledge::pipeline::run_wiki_finalize(pool, version_id, document_id).await
}

pub struct WikiIngestWorker {
    pool: Option<PgPool>,
}

impl oxana::FromContext<AppCtx> for WikiIngestWorker {
    fn from_context(ctx: &AppCtx) -> Self {
        Self {
            pool: ctx.pool.clone(),
        }
    }
}

#[async_trait]
impl oxana::Worker<WikiIngestJob> for WikiIngestWorker {
    type Error = JobErr;

    fn max_retries(&self, _job: &WikiIngestJob) -> u32 {
        10
    }

    fn retry_delay(&self, _job: &WikiIngestJob, _retries: u32) -> u64 {
        platform::WIKI_LOCK_RETRY_SECS
    }

    async fn process(
        &self,
        job: WikiIngestJob,
        _ctx: &oxana::JobContext,
    ) -> Result<(), Self::Error> {
        let Some(pool) = &self.pool else {
            return Err(JobErr("postgres not configured".into()));
        };
        process_wiki_ingest(
            pool,
            job.product_version_id,
            job.document_id,
            &job.operation,
        )
        .await
        .map_err(JobErr)
    }
}

pub struct WikiFinalizeWorker {
    pool: Option<PgPool>,
}

impl oxana::FromContext<AppCtx> for WikiFinalizeWorker {
    fn from_context(ctx: &AppCtx) -> Self {
        Self {
            pool: ctx.pool.clone(),
        }
    }
}

#[async_trait]
impl oxana::Worker<WikiFinalizeJob> for WikiFinalizeWorker {
    type Error = JobErr;

    fn max_retries(&self, _job: &WikiFinalizeJob) -> u32 {
        10
    }

    async fn process(
        &self,
        job: WikiFinalizeJob,
        _ctx: &oxana::JobContext,
    ) -> Result<(), Self::Error> {
        let Some(pool) = &self.pool else {
            return Err(JobErr("postgres not configured".into()));
        };
        process_wiki_finalize(pool, job.product_version_id, job.document_id)
            .await
            .map_err(JobErr)
    }
}

pub async fn process_wiki_ingest(
    pool: &PgPool,
    version_id: Uuid,
    document_id: Uuid,
    operation: &str,
) -> Result<(), String> {
    knowledge::pipeline::run_wiki_ingest(pool, version_id, document_id, operation).await
}

async fn schedule_semantic_index_for_document_v2(
    pool: &PgPool,
    document_id: Uuid,
) -> Result<(), String> {
    let product_version_id: Option<Uuid> = sqlx::query_scalar(
        "SELECT product_version_id FROM documents WHERE id=$1 AND deleted_at IS NULL",
    )
    .bind(document_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| error.to_string())?;
    if let Some(product_version_id) = product_version_id {
        schedule_semantic_index_v2_if_ready(pool, product_version_id).await?;
    }
    Ok(())
}

pub async fn process_summary_pg(
    pool: &PgPool,
    document_id: Uuid,
    attempt: i32,
    fallback: bool,
) -> Result<(), String> {
    knowledge::pipeline::run_summary(pool, document_id, attempt, fallback).await
}

pub async fn process_questions_pg(
    pool: &PgPool,
    document_id: Uuid,
    chunk_ids: &[Uuid],
    prev_ids: &[Option<Uuid>],
    next_ids: &[Option<Uuid>],
    attempt: i32,
) -> Result<(), String> {
    knowledge::pipeline::run_questions(pool, document_id, chunk_ids, prev_ids, next_ids, attempt)
        .await
}

pub async fn process_extract_pg(
    pool: &PgPool,
    chunk_id: Uuid,
    document_id: Uuid,
    attempt: i32,
) -> Result<(), String> {
    knowledge::pipeline::run_extract(pool, chunk_id, document_id, attempt).await
}

pub async fn process_list_delete_pg(pool: &PgPool, document_id: Uuid) -> Result<(), String> {
    knowledge::pipeline::run_list_delete(pool, document_id).await
}

pub async fn process_kb_delete_pg(pool: &PgPool, product_version_id: Uuid) -> Result<(), String> {
    let _ = knowledge::cancel_active_docs_for_versions(pool, &[product_version_id]).await;
    sqlx::query(
        "UPDATE documents SET parse_status = 'deleting', updated_at = now()
         WHERE product_version_id = $1 AND deleted_at IS NULL",
    )
    .bind(product_version_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    let docs: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM documents WHERE product_version_id = $1 AND deleted_at IS NULL",
    )
    .bind(product_version_id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    for did in docs {
        process_list_delete_pg(pool, did).await?;
    }
    sqlx::query(
        "UPDATE product_versions SET status = 'archived', deleted_at = now(), updated_at = now()
         WHERE id = $1",
    )
    .bind(product_version_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    sqlx::query("UPDATE products SET current_version_id = NULL WHERE current_version_id = $1")
        .bind(product_version_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    let pid: Option<Uuid> =
        sqlx::query_scalar("SELECT product_id FROM product_versions WHERE id = $1")
            .bind(product_version_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?;
    if let Some(pid) = pid {
        let _ = knowledge::delete_empty_product(pool, pid).await;
    }
    Ok(())
}

fn require_worker_enqueue(
    result: Result<Option<String>, String>,
    task: &str,
) -> Result<(), String> {
    match result {
        Ok(Some(_)) => Ok(()),
        Ok(None) => Err(format!("Oxana Redis is not configured for {task}")),
        Err(error) => Err(format!("enqueue {task}: {error}")),
    }
}

pub async fn process_reparse_pg(
    pool: &PgPool,
    document_id: Uuid,
    attempt: i32,
) -> Result<(), String> {
    let vid: Option<Uuid> =
        sqlx::query_scalar("SELECT product_version_id FROM documents WHERE id = $1")
            .bind(document_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?;
    let current_attempt: Option<i32> =
        sqlx::query_scalar("SELECT attempt FROM documents WHERE id=$1")
            .bind(document_id)
            .fetch_optional(pool)
            .await
            .map_err(|error| error.to_string())?;
    if current_attempt != Some(attempt) {
        return Ok(());
    }
    knowledge::open_attempt(pool, document_id, attempt)
        .await
        .map_err(|error| error.to_string())?;
    if let Some(vid) = vid {
        knowledge::pipeline::run_wiki_ingest(pool, vid, document_id, knowledge::wiki::OP_RETRACT)
            .await?;
        knowledge::graph::delete_document(vid, document_id)?;
    }
    knowledge::purge_document_index(pool, document_id)
        .await
        .map_err(|e| e.to_string())?;
    require_worker_enqueue(
        platform::enqueue_index_delete(document_id).await,
        "index deletion",
    )?;
    let vid = vid.unwrap_or_default();
    let source: Option<(String, Option<serde_json::Value>)> = sqlx::query_as(
        "SELECT COALESCE(type, 'file'), source_passages FROM documents WHERE id = $1",
    )
    .bind(document_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;
    match source.as_ref() {
        Some((kind, Some(raw))) if kind == "passage" => {
            let passages: Vec<String> = serde_json::from_value(raw.clone()).unwrap_or_default();
            require_worker_enqueue(
                platform::enqueue_document_process_with(document_id, vid, attempt, passages).await,
                "passage processing",
            )?;
        }
        Some((kind, _)) if kind == "manual" => {
            require_worker_enqueue(
                platform::enqueue_manual_process(document_id, vid, attempt).await,
                "manual processing",
            )?;
        }
        _ => {
            require_worker_enqueue(
                platform::enqueue_document_process(document_id, vid, attempt).await,
                "document processing",
            )?;
        }
    }
    let file_name: Option<String> =
        sqlx::query_scalar("SELECT file_name FROM documents WHERE id = $1")
            .bind(document_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?;
    if file_name.as_deref().is_some_and(|n| {
        matches!(
            n.rsplit('.')
                .next()
                .unwrap_or("")
                .to_ascii_lowercase()
                .as_str(),
            "csv" | "xlsx" | "xls"
        )
    }) {
        require_worker_enqueue(
            platform::enqueue_datatable(document_id).await,
            "datatable processing",
        )?;
    }
    Ok(())
}

macro_rules! simple_worker {
    ($name:ident, $job:ty, $call:expr) => {
        pub struct $name {
            pool: Option<PgPool>,
        }
        impl oxana::FromContext<AppCtx> for $name {
            fn from_context(ctx: &AppCtx) -> Self {
                Self {
                    pool: ctx.pool.clone(),
                }
            }
        }
        #[async_trait]
        impl oxana::Worker<$job> for $name {
            type Error = JobErr;
            fn max_retries(&self, _job: &$job) -> u32 {
                3
            }
            async fn process(
                &self,
                job: $job,
                _ctx: &oxana::JobContext,
            ) -> Result<(), Self::Error> {
                let Some(pool) = self.pool.clone() else {
                    return Err(JobErr("postgres not configured".into()));
                };
                ($call)(pool, job).await.map_err(JobErr)
            }
        }
    };
}

pub struct SummaryWorker {
    pool: Option<PgPool>,
}

impl oxana::FromContext<AppCtx> for SummaryWorker {
    fn from_context(ctx: &AppCtx) -> Self {
        Self {
            pool: ctx.pool.clone(),
        }
    }
}

#[async_trait]
impl oxana::Worker<SummaryJob> for SummaryWorker {
    type Error = JobErr;

    fn max_retries(&self, _job: &SummaryJob) -> u32 {
        3
    }

    async fn process(&self, job: SummaryJob, ctx: &oxana::JobContext) -> Result<(), Self::Error> {
        let Some(pool) = &self.pool else {
            return Err(JobErr("postgres not configured".into()));
        };
        process_summary_pg(pool, job.document_id, job.attempt, ctx.meta.retries >= 3)
            .await
            .map_err(JobErr)
    }
}

pub struct QuestionWorker {
    pool: Option<PgPool>,
}

impl oxana::FromContext<AppCtx> for QuestionWorker {
    fn from_context(ctx: &AppCtx) -> Self {
        Self {
            pool: ctx.pool.clone(),
        }
    }
}

#[async_trait]
impl oxana::Worker<QuestionJob> for QuestionWorker {
    type Error = JobErr;

    fn max_retries(&self, _job: &QuestionJob) -> u32 {
        3
    }

    async fn process(&self, job: QuestionJob, ctx: &oxana::JobContext) -> Result<(), Self::Error> {
        let Some(pool) = &self.pool else {
            return Err(JobErr("postgres not configured".into()));
        };
        let result = process_questions_pg(
            pool,
            job.document_id,
            &job.chunk_ids,
            &job.prev_ids,
            &job.next_ids,
            job.attempt,
        )
        .await;
        if result.is_err() && ctx.meta.retries >= 3 {
            let _ = knowledge::finalize_subtask(pool, job.document_id).await;
            let _ = schedule_semantic_index_for_document_v2(pool, job.document_id).await;
        }
        result.map_err(JobErr)
    }
}

pub struct ExtractWorker {
    pool: Option<PgPool>,
}

impl oxana::FromContext<AppCtx> for ExtractWorker {
    fn from_context(ctx: &AppCtx) -> Self {
        Self {
            pool: ctx.pool.clone(),
        }
    }
}

#[async_trait]
impl oxana::Worker<ExtractJob> for ExtractWorker {
    type Error = JobErr;

    fn max_retries(&self, _job: &ExtractJob) -> u32 {
        3
    }

    async fn process(&self, job: ExtractJob, ctx: &oxana::JobContext) -> Result<(), Self::Error> {
        let Some(pool) = &self.pool else {
            return Err(JobErr("postgres not configured".into()));
        };
        let result = process_extract_pg(pool, job.chunk_id, job.document_id, job.attempt).await;
        if result.is_err() && ctx.meta.retries >= 3 {
            let _ = knowledge::finalize_subtask(pool, job.document_id).await;
            let _ = schedule_semantic_index_for_document_v2(pool, job.document_id).await;
        }
        result.map_err(JobErr)
    }
}

simple_worker!(
    DatatableWorker,
    DatatableJob,
    |pool: PgPool, job: DatatableJob| async move {
        knowledge::pipeline::run_datatable(&pool, job.document_id).await
    }
);
simple_worker!(
    ListDeleteWorker,
    ListDeleteJob,
    |pool: PgPool, job: ListDeleteJob| async move {
        process_list_delete_pg(&pool, job.document_id).await
    }
);
simple_worker!(
    KbDeleteWorker,
    KbDeleteJob,
    |pool: PgPool, job: KbDeleteJob| async move {
        process_kb_delete_pg(&pool, job.product_version_id).await
    }
);
simple_worker!(
    ListReparseWorker,
    ListReparseJob,
    |pool: PgPool, job: ListReparseJob| async move {
        process_reparse_pg(&pool, job.document_id, job.attempt).await
    }
);
simple_worker!(
    IndexDeleteWorker,
    IndexDeleteJob,
    |pool: PgPool, job: IndexDeleteJob| async move {
        knowledge::purge_document_index(&pool, job.document_id)
            .await
            .map_err(|e| e.to_string())
    }
);

async fn wait_for_worker_shutdown(mut stop: tokio::sync::watch::Receiver<bool>) {
    while !*stop.borrow() {
        if stop.changed().await.is_err() {
            return;
        }
    }
}

const TRANSPORT_RUNTIME_COUNT: usize = 7;

async fn join_transport_group(
    mut tasks: tokio::task::JoinSet<Result<(), oxana::OxanaError>>,
    group_cancel: &CancellationToken,
    stop_tx: tokio::sync::watch::Sender<bool>,
) -> Result<(), oxana::OxanaError> {
    let mut fatal = None;
    tokio::select! {
        biased;
        () = group_cancel.cancelled() => {}
        joined = tasks.join_next() => {
            fatal = Some(match joined {
                Some(Ok(Ok(()))) => oxana::OxanaError::GenericError(
                    "transport runtime exited unexpectedly".into()),
                Some(Ok(Err(error))) => error,
                Some(Err(error)) => oxana::OxanaError::TokioJoinError(error),
                None => oxana::OxanaError::GenericError("transport group lost every runtime".into()),
            });
        }
    }
    group_cancel.cancel();
    let _ = stop_tx.send(true);
    let deadline = tokio::time::Instant::now() + HANDLER_CLEANUP_MARGIN;
    let abort_deadline = deadline
        .checked_sub(TASK_ABORT_DRAIN_RESERVE)
        .unwrap_or(deadline);
    while !tasks.is_empty() {
        match tokio::time::timeout_at(abort_deadline, tasks.join_next()).await {
            Ok(Some(Ok(Ok(())))) => {}
            Ok(Some(Ok(Err(error)))) if fatal.is_none() => fatal = Some(error),
            Ok(Some(Err(error))) if fatal.is_none() => {
                fatal = Some(oxana::OxanaError::TokioJoinError(error));
            }
            Ok(Some(_)) => {}
            Ok(None) => break,
            Err(_) => {
                tasks.abort_all();
                while let Ok(Some(_)) = tokio::time::timeout_at(deadline, tasks.join_next()).await {
                }
                if fatal.is_none() {
                    fatal = Some(oxana::OxanaError::GenericError(
                        "transport cleanup exceeded 30 seconds".into(),
                    ));
                }
                break;
            }
        }
    }
    fatal.map_or(Ok(()), Err)
}

async fn run_transport_group(
    ctx: AppCtx,
    core_storage: oxana::Storage,
) -> Result<(), oxana::OxanaError> {
    let post_storage = platform::oxana_connect()?;
    let enrich_storage = platform::oxana_connect()?;
    let maintenance_storage = platform::oxana_connect()?;
    let shared_storage = platform::oxana_connect()?;
    let wiki_storage = platform::oxana_connect()?;
    let multimodal_storage = platform::oxana_connect()?;
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let shut = |stop: tokio::sync::watch::Receiver<bool>| async move {
        wait_for_worker_shutdown(stop).await;
        Ok::<(), std::io::Error>(())
    };
    let mut tasks = tokio::task::JoinSet::<Result<(), oxana::OxanaError>>::new();
    let core = core_storage
        .runtime(ctx.clone())
        .queue_with_concurrency::<DefaultQueue>(platform::runtime_concurrency("CORE", 8))
        .worker::<DocumentProcessWorker, DocumentProcessJob>()
        .worker::<DocumentProcessWorker, ManualProcessJob>()
        .queue_with_concurrency::<BidAuthoringV2Queue>(platform::runtime_concurrency(
            "BID_AUTHORING",
            platform::BID_AUTHORING_V2_CONCURRENCY,
        ))
        .worker::<TenderDocumentProcessV2Worker, TenderDocumentProcessJobV2>()
        .worker::<RequirementSetCompileV2Worker, RequirementSetCompileJobV2>()
        .worker::<DocxComposeV2Worker, DocxComposeJobV2>()
        .worker::<ContentGenerateV2Worker, ContentGenerateJobV2>()
        .worker::<SubmissionExportV2Worker, SubmissionExportJobV2>()
        .shutdown_on(shut(stop_rx.clone()))
        .shutdown_timeout(HANDLER_CLEANUP_MARGIN)
        .run();
    tasks.spawn(async move { core.await.map(|_| ()) });
    let post = post_storage
        .runtime(ctx.clone())
        .queue_with_concurrency::<PostprocessQueue>(platform::runtime_concurrency("POSTPROCESS", 2))
        .worker::<PostProcessWorker, PostProcessJob>()
        .worker::<KnowledgeSemanticIndexV2Worker, KnowledgeSemanticIndexV2Job>()
        .shutdown_on(shut(stop_rx.clone()))
        .shutdown_timeout(HANDLER_CLEANUP_MARGIN)
        .run();
    tasks.spawn(async move { post.await.map(|_| ()) });
    let enrich = enrich_storage
        .runtime(ctx.clone())
        .queue_with_concurrency::<SummaryQueue>(platform::runtime_concurrency("ENRICHMENT", 12))
        .worker::<SummaryWorker, SummaryJob>()
        .worker::<DatatableWorker, DatatableJob>()
        .shutdown_on(shut(stop_rx.clone()))
        .shutdown_timeout(HANDLER_CLEANUP_MARGIN)
        .run();
    tasks.spawn(async move { enrich.await.map(|_| ()) });
    let maintenance = maintenance_storage
        .runtime(ctx.clone())
        .queue_with_concurrency::<LowQueue>(platform::runtime_concurrency("MAINTENANCE", 4))
        .worker::<VersionCloneWorker, VersionCloneJob>()
        .worker::<ListDeleteWorker, ListDeleteJob>()
        .worker::<KbDeleteWorker, KbDeleteJob>()
        .worker::<ListReparseWorker, ListReparseJob>()
        .worker::<IndexDeleteWorker, IndexDeleteJob>()
        .shutdown_on(shut(stop_rx.clone()))
        .shutdown_timeout(HANDLER_CLEANUP_MARGIN)
        .run();
    tasks.spawn(async move { maintenance.await.map(|_| ()) });
    let shared = shared_storage
        .runtime(ctx.clone())
        .queue_with_concurrency::<SummaryQueue>(platform::runtime_concurrency("SHARED", 6))
        .worker::<SummaryWorker, SummaryJob>()
        .shutdown_on(shut(stop_rx.clone()))
        .shutdown_timeout(HANDLER_CLEANUP_MARGIN)
        .run();
    tasks.spawn(async move { shared.await.map(|_| ()) });
    let wiki = wiki_storage
        .runtime(ctx.clone())
        .queue_with_concurrency::<WikiQueue>(platform::runtime_concurrency("WIKI", 8))
        .worker::<WikiIngestWorker, WikiIngestJob>()
        .worker::<WikiFinalizeWorker, WikiFinalizeJob>()
        .shutdown_on(shut(stop_rx.clone()))
        .shutdown_timeout(HANDLER_CLEANUP_MARGIN)
        .run();
    tasks.spawn(async move { wiki.await.map(|_| ()) });
    let multimodal = multimodal_storage
        .runtime(ctx.clone())
        .queue_with_concurrency::<MultimodalQueue>(platform::runtime_concurrency("MULTIMODAL", 12))
        .worker::<ImageMultimodalWorker, ImageMultimodalJob>()
        .shutdown_on(shut(stop_rx.clone()))
        .shutdown_timeout(HANDLER_CLEANUP_MARGIN)
        .run();
    tasks.spawn(async move { multimodal.await.map(|_| ()) });
    debug_assert_eq!(tasks.len(), TRANSPORT_RUNTIME_COUNT);
    join_transport_group(tasks, &ctx.shutdown, stop_tx).await
}

pub async fn run_core(ctx: AppCtx) -> Result<(), String> {
    let pool = ctx
        .pool
        .clone()
        .ok_or_else(|| "Worker supervisor requires PostgreSQL".to_string())?;
    let storage = platform::oxana_connect().map_err(|error| error.to_string())?;
    let group_cancel = CancellationToken::new();
    let group_ctx = AppCtx {
        pool: Some(pool),
        shutdown: group_cancel.clone(),
        root_cancel: group_cancel.clone(),
    };
    let mut group = tokio::spawn(run_transport_group(group_ctx, storage));
    enum RootCompletion {
        External,
        Local,
        Group(Result<Result<(), oxana::OxanaError>, tokio::task::JoinError>),
    }
    let completion = tokio::select! {
        biased;
        () = ctx.shutdown.cancelled() => RootCompletion::External,
        () = ctx.root_cancel.cancelled() => RootCompletion::Local,
        joined = &mut group => RootCompletion::Group(joined),
    };
    let selected_group = matches!(completion, RootCompletion::Group(_));
    let mut fatal = match completion {
        RootCompletion::External | RootCompletion::Local => None,
        RootCompletion::Group(Ok(Err(error))) => Some(error.to_string()),
        RootCompletion::Group(Err(error)) => Some(format!("transport group join failed: {error}")),
        RootCompletion::Group(Ok(Ok(()))) => Some("transport group exited unexpectedly".into()),
    };
    group_cancel.cancel();
    if !selected_group {
        let deadline = tokio::time::Instant::now() + HANDLER_CLEANUP_MARGIN;
        let abort_deadline = deadline
            .checked_sub(TASK_ABORT_DRAIN_RESERVE)
            .unwrap_or(deadline);
        match tokio::time::timeout_at(abort_deadline, &mut group).await {
            Ok(Ok(Err(error))) if fatal.is_none() => fatal = Some(error.to_string()),
            Ok(Err(error)) if fatal.is_none() => fatal = Some(error.to_string()),
            Err(_) => {
                group.abort();
                let _ = tokio::time::timeout_at(deadline, &mut group).await;
                if fatal.is_none() {
                    fatal = Some("transport group cleanup exceeded 30 seconds".into());
                }
            }
            _ => {}
        }
    }
    fatal.map_or(Ok(()), Err)
}

pub async fn shutdown_signal() {
    let mut sigterm =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("sigterm");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = sigterm.recv() => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use knowledge::{create_workspace_with_library, insert_document, insert_user};

    #[tokio::test]
    async fn process_group_signal_failure_falls_back_to_direct_child_kill() {
        let mut child = tokio::process::Command::new("sleep")
            .arg("60")
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        kill_helper_group_and_reap_child(
            &mut child,
            "fallback test child",
            tokio::time::Instant::now() + std::time::Duration::from_secs(2),
        )
        .await
        .unwrap();
        assert!(child.try_wait().unwrap().is_some());
    }

    #[tokio::test]
    async fn real_postgres_non_agent_and_content_timeout_cancel_late_writes() {
        let _guard = db_lock().await;
        let Ok(pool) = connect().await else {
            eprintln!("skip: isolated PostgreSQL test database is down");
            return;
        };
        sqlx::raw_sql(
            "DROP TABLE IF EXISTS kb_worker_no_late_write_probe;
             CREATE TABLE kb_worker_no_late_write_probe (
                singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
                non_agent_written boolean NOT NULL DEFAULT false,
                content_written boolean NOT NULL DEFAULT false
             );
             INSERT INTO kb_worker_no_late_write_probe DEFAULT VALUES",
        )
        .execute(&pool)
        .await
        .unwrap();

        let mut lock_connection = pool.acquire().await.unwrap();
        let advisory_key = 7_401_002_i64;
        sqlx::query("SELECT pg_advisory_lock($1)")
            .bind(advisory_key)
            .execute(&mut *lock_connection)
            .await
            .unwrap();

        let generic_pool = pool.clone();
        let generic_cancel = CancellationToken::new();
        let now = tokio::time::Instant::now();
        let generic_run = run_owned_handler(
            async move {
                sqlx::query("SELECT pg_advisory_xact_lock($1)")
                    .bind(advisory_key)
                    .execute(&generic_pool)
                    .await
                    .map_err(|error| JobErr(error.to_string()))?;
                sqlx::query("UPDATE kb_worker_no_late_write_probe SET non_agent_written=true")
                    .execute(&generic_pool)
                    .await
                    .map_err(|error| JobErr(error.to_string()))?;
                Ok(())
            },
            HandlerDeadline {
                hard: now + std::time::Duration::from_millis(50),
                cleanup: now + std::time::Duration::from_secs(1),
            },
            CancellationToken::new(),
            generic_cancel,
            None,
            |_| false,
        )
        .await;
        assert!(matches!(
            generic_run.completion,
            OwnedHandlerCompletion::TimedOut
        ));

        let content_pool = pool.clone();
        let pipeline_cancel = CancellationToken::new();
        let pipeline_stop = pipeline_cancel.clone();
        let mut pipeline = tokio::spawn(async move {
            tokio::select! {
                biased;
                () = pipeline_stop.cancelled() => Err(
                    bidding::agent_error::AgentError::new("INTERNAL", "cancelled")),
                result = async {
                    sqlx::query("SELECT pg_advisory_xact_lock($1)")
                        .bind(advisory_key)
                        .execute(&content_pool)
                        .await
                        .map_err(|error| bidding::agent_error::AgentError::new(
                            "INTERNAL", error.to_string()))?;
                    sqlx::query(
                        "UPDATE kb_worker_no_late_write_probe SET content_written=true",
                    )
                    .execute(&content_pool)
                    .await
                    .map_err(|error| bidding::agent_error::AgentError::new(
                        "INTERNAL", error.to_string()))?;
                    Ok(())
                } => result,
            }
        });
        let heartbeat_cancel = CancellationToken::new();
        let heartbeat_stop = heartbeat_cancel.clone();
        let mut heartbeat = tokio::spawn(async move {
            heartbeat_stop.cancelled().await;
        });
        let (_lease_sender, mut lease_receiver) = tokio::sync::oneshot::channel();
        let now = tokio::time::Instant::now();
        let content_run = await_content_owned_completion(
            &mut pipeline,
            &mut heartbeat,
            &mut lease_receiver,
            &pipeline_cancel,
            &heartbeat_cancel,
            &CancellationToken::new(),
            HandlerDeadline {
                hard: now + std::time::Duration::from_millis(50),
                cleanup: now + std::time::Duration::from_secs(1),
            },
        )
        .await;
        assert!(matches!(
            content_run.completion,
            ContentOwnedCompletion::TimedOut
        ));

        sqlx::query("SELECT pg_advisory_unlock($1)")
            .bind(advisory_key)
            .execute(&mut *lock_connection)
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let flags: (bool, bool) = sqlx::query_as(
            "SELECT non_agent_written, content_written
             FROM kb_worker_no_late_write_probe",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(flags, (false, false));
        sqlx::query("DROP TABLE kb_worker_no_late_write_probe")
            .execute(&pool)
            .await
            .unwrap();
    }

    async fn single_connection_pool(
        database_url: &str,
    ) -> (PgPool, sqlx::pool::PoolConnection<sqlx::Postgres>) {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .acquire_timeout(std::time::Duration::from_secs(1))
            .connect(database_url)
            .await
            .unwrap();
        let connection = pool.acquire().await.unwrap();
        (pool, connection)
    }

    async fn assert_pending_request(pool: &PgPool, request_id: Uuid) {
        let state: (String, Option<String>) = sqlx::query_as(
            "SELECT status,error_code FROM bid_async_request_snapshot_artifacts WHERE id=$1",
        )
        .bind(request_id)
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(state, ("pending".into(), None));
    }

    #[tokio::test]
    async fn blocked_export_tender_and_content_effects_are_bounded_without_late_writes() {
        let _guard = db_lock().await;
        let Some(database_url) = std::env::var("KNOWLEDGEBRAIN_TEST_DATABASE_URL").ok() else {
            eprintln!("skip: isolated PostgreSQL test database is down");
            return;
        };
        if !database_url.contains("127.0.0.1:25433/knowledgebrain_test_") {
            panic!("blocked handler tests require 127.0.0.1:25433/knowledgebrain_test_*");
        }
        let Ok(pool) = connect().await else {
            eprintln!("skip: isolated PostgreSQL test database is down");
            return;
        };
        reset_test_schema(&pool).await;
        install_phase_fixture(&pool).await;
        let export = create_export_terminal_test_request(&pool).await;
        let tender = create_tender_terminal_test_request(&pool).await;
        let content = create_content_terminal_test_request(&pool).await;
        let content_owner =
            match bidding::bid_authoring_v2::claim_content_agent_run_v1(&pool, &content)
                .await
                .unwrap()
            {
                bidding::bid_authoring_v2::ContentRunClaim::Claimed(owner) => owner,
                other => panic!("expected claimed Content AgentRun, got {other:?}"),
            };

        let (blocked, held) = single_connection_pool(&database_url).await;
        let started = tokio::time::Instant::now();
        let export_result = terminalize_non_agent_failure_until(
            &blocked,
            &export,
            NonAgentTerminalFailure::SubmissionExport("RENDERER_FAILED"),
            started + std::time::Duration::from_millis(100),
            "blocked export failure",
        )
        .await;
        assert!(export_result.unwrap_err().0.contains("persistence reserve"));
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
        drop(held);
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert_pending_request(&pool, export.request_artifact_id).await;

        let (blocked, held) = single_connection_pool(&database_url).await;
        let started = tokio::time::Instant::now();
        let tender_result = terminalize_non_agent_failure_until(
            &blocked,
            &tender,
            NonAgentTerminalFailure::TenderDocument("AGENT_OUTPUT_INVALID"),
            started + std::time::Duration::from_millis(100),
            "blocked tender failure",
        )
        .await;
        assert!(tender_result.unwrap_err().0.contains("persistence reserve"));
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
        drop(held);
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert_pending_request(&pool, tender.request_artifact_id).await;

        let (blocked, held) = single_connection_pool(&database_url).await;
        let started = tokio::time::Instant::now();
        let content_result = yield_content_retry_until(
            &blocked,
            &content,
            &content_owner,
            "blocked transient yield",
            started + std::time::Duration::from_millis(100),
        )
        .await;
        assert!(
            content_result
                .unwrap_err()
                .0
                .contains("persistence reserve")
        );
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
        drop(held);
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let content_state: (String, Option<String>, String, Option<String>) = sqlx::query_as(
            "SELECT request_value.status,request_value.error_code,run.status,run.last_error_code
             FROM bid_async_request_snapshot_artifacts request_value
             JOIN bid_content_agent_run_artifacts run
               ON run.request_artifact_id=request_value.id AND run.attempt=$2
             WHERE request_value.id=$1",
        )
        .bind(content.request_artifact_id)
        .bind(content_owner.attempt)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            content_state,
            ("pending".into(), None, "running".into(), None)
        );
    }

    #[tokio::test]
    async fn process_group_kill_reaps_a_helper_with_hanging_grandchild() {
        let mut command = tokio::process::Command::new("sh");
        command.arg("-c").arg("sleep 60 & wait").kill_on_drop(true);
        command.process_group(0);
        let mut child = command.spawn().unwrap();
        let pid = i32::try_from(child.id().unwrap()).unwrap();
        kill_helper_group_and_reap_child(
            &mut child,
            "hanging grandchild test",
            tokio::time::Instant::now() + std::time::Duration::from_secs(2),
        )
        .await
        .unwrap();
        assert!(child.try_wait().unwrap().is_some());
        // SAFETY: signal 0 performs only an existence check for this test-owned group.
        assert_eq!(unsafe { libc::killpg(pid, 0) }, -1);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
    }
    use platform::{apply_fresh_baseline, write_blob};

    async fn install_phase_fixture(pool: &PgPool) {
        let phase = include_str!("../../bidding/tests/sql/phase1_acceptance.sql")
            .lines()
            .skip(1)
            .collect::<Vec<_>>()
            .join("\n");
        let mut connection = pool.acquire().await.unwrap();
        sqlx::Executor::execute(&mut *connection, "SET ROLE kb_app_owner")
            .await
            .unwrap();
        sqlx::raw_sql(sqlx::AssertSqlSafe(phase))
            .execute(&mut *connection)
            .await
            .unwrap();
    }

    async fn owner_connection(pool: &PgPool) -> sqlx::pool::PoolConnection<sqlx::Postgres> {
        let mut connection = pool.acquire().await.unwrap();
        sqlx::Executor::execute(&mut *connection, "SET ROLE kb_app_owner")
            .await
            .unwrap();
        connection
    }

    fn request_identity(value: &serde_json::Value) -> platform::BidAuthoringRequestIdentityV2 {
        platform::BidAuthoringRequestIdentityV2 {
            request_artifact_id: Uuid::parse_str(value["request_artifact_id"].as_str().unwrap())
                .unwrap(),
            request_revision: value["request_revision"].as_i64().unwrap(),
            frozen_input_sha256: value["frozen_input_sha256"].as_str().unwrap().into(),
        }
    }

    const TEST_ACTOR: &str = "user:10000000-0000-4000-8000-000000000001";
    const TEST_PROJECT_ID: Uuid = Uuid::from_u128(0x10000000000040008000000000000010);

    async fn test_workspace_head(pool: &PgPool) -> (Uuid, Uuid, String) {
        sqlx::query_as(
            "SELECT workspace.id,head.artifact_id,head.artifact_sha256
             FROM bid_submission_workspaces workspace
             JOIN bid_workspace_heads head ON head.scope_id=workspace.id
             WHERE workspace.project_id=$1",
        )
        .bind(TEST_PROJECT_ID)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    async fn create_tender_terminal_test_request(
        pool: &PgPool,
    ) -> platform::BidAuthoringRequestIdentityV2 {
        let document_id = Uuid::new_v4();
        let staging_id = Uuid::new_v4();
        let request_id = Uuid::new_v4();
        let object_sha = platform::sha256_hex(document_id.as_bytes());
        let object_ref = format!("objects/{object_sha}");
        let request_bytes = br#"{"reserve_test":"tender"}"#;
        let mut connection = owner_connection(pool).await;
        sqlx::query(
            "SELECT kb_object_upload_stage($1,$2::kb_object_ref,$3::kb_sha256,
             'application/pdf',1,$4::kb_actor_identity)",
        )
        .bind(staging_id)
        .bind(&object_ref)
        .bind(&object_sha)
        .bind(TEST_ACTOR)
        .execute(&mut *connection)
        .await
        .unwrap();
        let value: serde_json::Value = sqlx::query_scalar(
            "SELECT kb_bid_v2_upload_tender_document(
             $1,$2,$3,$4,'reserve.pdf','application/pdf',1,$5::kb_object_ref,
             $6::kb_sha256,$7::kb_actor_identity,$8,$9,kb_bid_v2_sha256_bytes($9))",
        )
        .bind(staging_id)
        .bind(document_id)
        .bind(request_id)
        .bind(TEST_PROJECT_ID)
        .bind(object_ref)
        .bind(object_sha)
        .bind(TEST_ACTOR)
        .bind(format!("reserve-tender-{}", Uuid::new_v4()))
        .bind(request_bytes.as_slice())
        .fetch_one(&mut *connection)
        .await
        .unwrap();
        request_identity(&value)
    }

    async fn create_export_terminal_test_request(
        pool: &PgPool,
    ) -> platform::BidAuthoringRequestIdentityV2 {
        let (workspace_id, revision_id, revision_sha) = test_workspace_head(pool).await;
        let request_bytes = r#"{"reserve_test":"export"}"#;
        let mut connection = owner_connection(pool).await;
        let value: serde_json::Value = sqlx::query_scalar(
            "SELECT kb_bid_v2_create_submission_export_request(
             $1,$2,$3::kb_sha256,'review_draft','pdf',
             jsonb_build_object('watermark','reserve test'),$4::kb_actor_identity,$5,
             convert_to($6,'UTF8'),kb_bid_v2_sha256_bytes(convert_to($6,'UTF8')))",
        )
        .bind(workspace_id)
        .bind(revision_id)
        .bind(revision_sha)
        .bind(TEST_ACTOR)
        .bind(format!("reserve-export-{}", Uuid::new_v4()))
        .bind(request_bytes)
        .fetch_one(&mut *connection)
        .await
        .unwrap();
        request_identity(&value)
    }

    async fn ensure_content_test_checkpoint(pool: &PgPool) {
        let (workspace_id, revision_id, revision_sha) = test_workspace_head(pool).await;
        let bytes = br#"{"reserve_test":"checkpoint"}"#;
        let mut connection = owner_connection(pool).await;
        sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT kb_bid_v2_create_outline_checkpoint(
             $1,$2,$3::kb_sha256,$4,$5::kb_actor_identity,$6,$7,
             kb_bid_v2_sha256_bytes($7))",
        )
        .bind(workspace_id)
        .bind(revision_id)
        .bind(revision_sha)
        .bind(Uuid::new_v4())
        .bind(TEST_ACTOR)
        .bind(format!("reserve-checkpoint-{}", Uuid::new_v4()))
        .bind(bytes.as_slice())
        .fetch_one(&mut *connection)
        .await
        .unwrap();
    }

    async fn create_content_terminal_test_request(
        pool: &PgPool,
    ) -> platform::BidAuthoringRequestIdentityV2 {
        ensure_content_test_checkpoint(pool).await;
        let (workspace_id, revision_id, revision_sha) = test_workspace_head(pool).await;
        let request_body = serde_json::json!({"reserve_test":"content"});
        let context = bidding::MutationContext::new(
            TEST_ACTOR,
            format!("reserve-content-{}", Uuid::new_v4()),
            &request_body,
        )
        .unwrap();
        let sha = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let retrieval = knowledge::knowledge_retrieval::FrozenRetrievalPolicyIdentityV1 {
            schema_version: 1,
            policy_sha256: sha.into(),
            canonical_policy_utf8: "{}".into(),
            contract_version: "knowledge-evidence-v2".into(),
            mode: "exact".into(),
            max_hits: 1,
            max_chunk_bytes: 1,
            max_total_bytes: 1,
            embedding_revision_sha256: sha.into(),
            canonical_embedding_revision_utf8: "{}".into(),
            embedding_credential_ref: "env:EMBED".into(),
            rerank_revision_sha256: sha.into(),
            canonical_rerank_revision_utf8: "{}".into(),
            rerank_credential_ref: "env:RERANK".into(),
            product_version_ids: vec![],
            library_version_ids: vec![],
            eligible_scope_sha256:
                "715d78b3301b4e5901d8dc93c9d33776a0cef3378d65de32353ee8e998541901".into(),
        };
        let runtime = bidding::content_runtime::ContentAgentRuntimeContractV1 {
            schema_version: 1,
            base_url: "http://127.0.0.1:18080".into(),
            endpoint: "http://127.0.0.1:18080/v1/chat/completions".into(),
            protocol: "openai_chat_completions_sse".into(),
            model_id: "scripted-content".into(),
            credential_ref: "env:KNOWLEDGEBRAIN_CHAT_API_KEY".into(),
            stream: true,
            max_tokens: 8192,
            timeout_ms: 180000,
            response_mode: "strict_json_schema".into(),
            transport_retries: 0,
            temperature: None,
            reasoning_effort: None,
        };
        let value = bidding::bid_authoring_v2::create_content_request_v2(
            pool,
            bidding::bid_authoring_v2::CreateContentRequestV2 {
                workspace_id,
                expected_revision_id: revision_id,
                expected_sha256: &revision_sha,
                operation: "generate",
                target_kind: "workspace",
                target_node_lineage_id: None,
                fill_policy: "append_candidate",
                insertion_anchor: None,
                evidence_selection_mode: "system_proposed",
                pick_set_artifact_id: None,
                retrieval_identity: Some(&retrieval),
                runtime_contract: Some(&runtime),
            },
            &context,
        )
        .await
        .unwrap();
        request_identity(&value)
    }

    use std::{
        collections::VecDeque,
        sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    #[tokio::test(start_paused = true)]
    async fn early_completed_error_reserves_terminal_persistence_time() {
        let started = tokio::time::Instant::now();
        let run = run_owned_handler(
            async { Err(JobErr("deterministic".into())) },
            HandlerDeadline {
                hard: started + std::time::Duration::from_secs(60),
                cleanup: started + HANDLER_CLEANUP_MARGIN,
            },
            CancellationToken::new(),
            CancellationToken::new(),
            None,
            |_| true,
        )
        .await;
        assert!(matches!(
            run.completion,
            OwnedHandlerCompletion::Completed(Err(_))
        ));
        assert_eq!(
            run.cleanup_deadline.duration_since(run.teardown_deadline),
            TERMINAL_PERSISTENCE_RESERVE
        );

        let success = run_owned_handler(
            async { Ok(()) },
            HandlerDeadline {
                hard: started + std::time::Duration::from_secs(60),
                cleanup: started + HANDLER_CLEANUP_MARGIN,
            },
            CancellationToken::new(),
            CancellationToken::new(),
            None,
            |_| true,
        )
        .await;
        assert_eq!(success.cleanup_deadline, success.teardown_deadline);
    }

    #[tokio::test(start_paused = true)]
    async fn content_heartbeat_failure_cannot_starve_the_effect_reserve() {
        let pipeline_cancel = CancellationToken::new();
        let heartbeat_cancel = CancellationToken::new();
        let mut pipeline = tokio::spawn(std::future::pending::<
            Result<(), bidding::agent_error::AgentError>,
        >());
        let mut heartbeat = tokio::spawn(std::future::pending::<()>());
        let (lease_tx, mut lease_rx) = tokio::sync::oneshot::channel();
        lease_tx
            .send(bidding::agent_error::AgentError::new(
                "INTERNAL",
                "heartbeat database unavailable",
            ))
            .unwrap();
        let started = tokio::time::Instant::now();
        let run = tokio::spawn(async move {
            await_content_owned_completion(
                &mut pipeline,
                &mut heartbeat,
                &mut lease_rx,
                &pipeline_cancel,
                &heartbeat_cancel,
                &CancellationToken::new(),
                HandlerDeadline {
                    hard: started + std::time::Duration::from_secs(60),
                    cleanup: started + HANDLER_CLEANUP_MARGIN,
                },
            )
            .await
        });
        tokio::task::yield_now().await;
        tokio::time::advance(HANDLER_CLEANUP_MARGIN - TERMINAL_PERSISTENCE_RESERVE).await;
        tokio::task::yield_now().await;
        let owned = run.await.unwrap();
        assert!(matches!(
            owned.completion,
            ContentOwnedCompletion::LeaseLost(_)
        ));
        assert!(owned.cleanup_deadline > tokio::time::Instant::now());
        assert_eq!(
            owned
                .cleanup_deadline
                .duration_since(tokio::time::Instant::now()),
            TERMINAL_PERSISTENCE_RESERVE
        );
    }

    #[tokio::test(start_paused = true)]
    async fn all_active_v2_handler_deadlines_fire_at_the_exact_boundary() {
        let deadlines = [
            ("TenderDocumentProcess", TENDER_HANDLER_HARD_TIMEOUT),
            ("RequirementSetCompile", REQUIREMENT_HANDLER_HARD_TIMEOUT),
            ("DocxCompose", DOCX_COMPOSE_HANDLER_HARD_TIMEOUT),
            (
                "ContentGenerate(generate)",
                CONTENT_GENERATE_HANDLER_HARD_TIMEOUT,
            ),
            (
                "ContentGenerate(match_only)",
                CONTENT_MATCH_HANDLER_HARD_TIMEOUT,
            ),
            ("SubmissionExport", SUBMISSION_EXPORT_HANDLER_HARD_TIMEOUT),
        ];
        for (kind, deadline) in deadlines {
            let shutdown = CancellationToken::new();
            let local = CancellationToken::new();
            let pipeline_cancel = local.clone();
            let task = tokio::spawn(run_owned_handler(
                async move {
                    pipeline_cancel.cancelled().await;
                    Ok(())
                },
                HandlerDeadline::from_now(deadline),
                shutdown,
                local,
                None,
                |_| false,
            ));
            tokio::task::yield_now().await;
            tokio::time::advance(deadline - std::time::Duration::from_millis(1)).await;
            assert!(
                !task.is_finished(),
                "{kind} stopped before its exact deadline"
            );
            tokio::time::advance(std::time::Duration::from_millis(1)).await;
            tokio::task::yield_now().await;
            assert!(matches!(
                task.await.unwrap().completion,
                OwnedHandlerCompletion::TimedOut
            ));
        }
    }

    #[tokio::test(start_paused = true)]
    async fn global_cancel_joins_handler_without_turning_it_into_a_timeout() {
        let shutdown = CancellationToken::new();
        let local = CancellationToken::new();
        let pipeline_cancel = local.clone();
        let stopped = Arc::new(AtomicBool::new(false));
        let pipeline_stopped = stopped.clone();
        let task = tokio::spawn(run_owned_handler(
            async move {
                pipeline_cancel.cancelled().await;
                pipeline_stopped.store(true, Ordering::SeqCst);
                Ok(())
            },
            HandlerDeadline::from_now(TENDER_HANDLER_HARD_TIMEOUT),
            shutdown.clone(),
            local,
            None,
            |_| false,
        ));
        tokio::task::yield_now().await;
        shutdown.cancel();
        tokio::task::yield_now().await;
        assert!(matches!(
            task.await.unwrap().completion,
            OwnedHandlerCompletion::ShuttingDown
        ));
        assert!(stopped.load(Ordering::SeqCst));
        tokio::time::advance(std::time::Duration::from_secs(60 * 60)).await;
        assert!(
            stopped.load(Ordering::SeqCst),
            "joined pipeline changed after return"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn content_generate_timeout_lease_loss_and_global_cancel_join_children() {
        async fn pending_pipeline(
            cancel: CancellationToken,
            stopped: Arc<AtomicBool>,
        ) -> Result<(), bidding::agent_error::AgentError> {
            struct StopProof(Arc<AtomicBool>);
            impl Drop for StopProof {
                fn drop(&mut self) {
                    self.0.store(true, Ordering::SeqCst);
                }
            }
            let _proof = StopProof(stopped);
            cancel.cancelled().await;
            Err(bidding::agent_error::AgentError::new(
                "INTERNAL",
                "cancelled",
            ))
        }

        let shutdown = CancellationToken::new();
        let pipeline_cancel = CancellationToken::new();
        let heartbeat_cancel = CancellationToken::new();
        let stopped = Arc::new(AtomicBool::new(false));
        let mut pipeline = tokio::spawn(pending_pipeline(pipeline_cancel.clone(), stopped.clone()));
        let heartbeat_stop = heartbeat_cancel.clone();
        let mut heartbeat = tokio::spawn(async move { heartbeat_stop.cancelled().await });
        let (_lease_tx, mut lease_rx) = tokio::sync::oneshot::channel();
        shutdown.cancel();
        assert!(matches!(
            await_content_owned_completion(
                &mut pipeline,
                &mut heartbeat,
                &mut lease_rx,
                &pipeline_cancel,
                &heartbeat_cancel,
                &shutdown,
                HandlerDeadline::from_now(CONTENT_GENERATE_HANDLER_HARD_TIMEOUT),
            )
            .await
            .completion,
            ContentOwnedCompletion::ShuttingDown
        ));
        assert!(pipeline.is_finished() && heartbeat.is_finished());
        assert!(stopped.load(Ordering::SeqCst));

        let shutdown = CancellationToken::new();
        let pipeline_cancel = CancellationToken::new();
        let heartbeat_cancel = CancellationToken::new();
        let mut pipeline = tokio::spawn(pending_pipeline(
            pipeline_cancel.clone(),
            Arc::new(AtomicBool::new(false)),
        ));
        let heartbeat_stop = heartbeat_cancel.clone();
        let mut heartbeat = tokio::spawn(async move { heartbeat_stop.cancelled().await });
        let (lease_tx, mut lease_rx) = tokio::sync::oneshot::channel();
        lease_tx
            .send(bidding::agent_error::AgentError::new(
                "REQUEST_ATTEMPT_SUPERSEDED",
                "expired owner",
            ))
            .unwrap();
        match await_content_owned_completion(
            &mut pipeline,
            &mut heartbeat,
            &mut lease_rx,
            &pipeline_cancel,
            &heartbeat_cancel,
            &shutdown,
            HandlerDeadline::from_now(CONTENT_GENERATE_HANDLER_HARD_TIMEOUT),
        )
        .await
        .completion
        {
            ContentOwnedCompletion::LeaseLost(error) => assert_eq!(
                error.disposition,
                bidding::agent_error::RetryDisposition::Obsolete
            ),
            _ => panic!("expected lease loss"),
        }
        assert!(pipeline.is_finished() && heartbeat.is_finished());

        let shutdown = CancellationToken::new();
        let pipeline_cancel = CancellationToken::new();
        let heartbeat_cancel = CancellationToken::new();
        let mut pipeline = tokio::spawn(pending_pipeline(
            pipeline_cancel.clone(),
            Arc::new(AtomicBool::new(false)),
        ));
        let heartbeat_stop = heartbeat_cancel.clone();
        let mut heartbeat = tokio::spawn(async move { heartbeat_stop.cancelled().await });
        let (_lease_tx, mut lease_rx) = tokio::sync::oneshot::channel();
        let timeout_task = tokio::spawn(async move {
            await_content_owned_completion(
                &mut pipeline,
                &mut heartbeat,
                &mut lease_rx,
                &pipeline_cancel,
                &heartbeat_cancel,
                &shutdown,
                HandlerDeadline::from_now(CONTENT_GENERATE_HANDLER_HARD_TIMEOUT),
            )
            .await
        });
        tokio::task::yield_now().await;
        tokio::time::advance(
            CONTENT_GENERATE_HANDLER_HARD_TIMEOUT - std::time::Duration::from_millis(1),
        )
        .await;
        assert!(!timeout_task.is_finished());
        tokio::time::advance(std::time::Duration::from_millis(1)).await;
        tokio::task::yield_now().await;
        assert!(matches!(
            timeout_task.await.unwrap().completion,
            ContentOwnedCompletion::TimedOut
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn transport_group_supervisor_handles_cancel_fatal_and_forced_abort() {
        assert_eq!(TRANSPORT_RUNTIME_COUNT, 7);
        let graceful_cancel = CancellationToken::new();
        let (graceful_tx, mut graceful_rx) = tokio::sync::watch::channel(false);
        let mut graceful_tasks = tokio::task::JoinSet::new();
        graceful_tasks.spawn(async move {
            while !*graceful_rx.borrow() {
                graceful_rx
                    .changed()
                    .await
                    .map_err(|error| oxana::OxanaError::GenericError(error.to_string()))?;
            }
            Ok(())
        });
        graceful_cancel.cancel();
        join_transport_group(graceful_tasks, &graceful_cancel, graceful_tx)
            .await
            .unwrap();

        let fatal_cancel = CancellationToken::new();
        let (fatal_tx, mut fatal_rx) = tokio::sync::watch::channel(false);
        let sibling_stopped = Arc::new(AtomicBool::new(false));
        let sibling_flag = sibling_stopped.clone();
        let mut fatal_tasks = tokio::task::JoinSet::new();
        fatal_tasks.spawn(async { Err(oxana::OxanaError::GenericError("fatal child".into())) });
        fatal_tasks.spawn(async move {
            while !*fatal_rx.borrow() {
                fatal_rx
                    .changed()
                    .await
                    .map_err(|error| oxana::OxanaError::GenericError(error.to_string()))?;
            }
            sibling_flag.store(true, Ordering::SeqCst);
            Ok(())
        });
        assert!(
            join_transport_group(fatal_tasks, &fatal_cancel, fatal_tx)
                .await
                .unwrap_err()
                .to_string()
                .contains("fatal child")
        );
        assert!(fatal_cancel.is_cancelled());
        assert!(sibling_stopped.load(Ordering::SeqCst));

        struct DropProof(Arc<AtomicBool>);
        impl Drop for DropProof {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let forced_cancel = CancellationToken::new();
        let (forced_tx, _forced_rx) = tokio::sync::watch::channel(false);
        let dropped = Arc::new(AtomicBool::new(false));
        let proof = DropProof(dropped.clone());
        let mut forced_tasks = tokio::task::JoinSet::new();
        forced_tasks.spawn(async move {
            let _proof = proof;
            std::future::pending::<()>().await;
            Ok(())
        });
        forced_cancel.cancel();
        let supervisor = tokio::spawn(async move {
            join_transport_group(forced_tasks, &forced_cancel, forced_tx).await
        });
        tokio::task::yield_now().await;
        tokio::time::advance(HANDLER_CLEANUP_MARGIN).await;
        assert!(
            supervisor
                .await
                .unwrap()
                .unwrap_err()
                .to_string()
                .contains("cleanup exceeded")
        );
        assert!(dropped.load(Ordering::SeqCst));
    }

    #[test]
    fn bidding_transport_retry_count_never_classifies_business_outcomes() {
        assert_eq!(platform::BID_AUTHORING_V2_MAX_RETRIES, 3);
        assert!(!include_str!("consume.rs").contains(concat!("bid_failure_", "is_final")));
    }

    #[test]
    fn frozen_asset_metadata_budget_rejects_before_reads_or_allocations() {
        let oversized = vec![serde_json::json!({
            "asset_revision_id":Uuid::new_v4(),
            "media_type":"image/png",
            "byte_length":MAX_FROZEN_ASSET_TOTAL_BYTES + 1,
            "width_px":1,
            "height_px":1
        })];
        assert!(validate_frozen_asset_metadata(&oversized).is_err());
        let pixel_overflow = vec![serde_json::json!({
            "asset_revision_id":Uuid::new_v4(),
            "media_type":"image/png",
            "byte_length":1,
            "width_px":MAX_FROZEN_ASSET_TOTAL_PIXELS,
            "height_px":2
        })];
        assert!(validate_frozen_asset_metadata(&pixel_overflow).is_err());
        let missing_length = vec![serde_json::json!({
            "asset_revision_id":Uuid::new_v4(),
            "media_type":"application/pdf",
            "page_count":1
        })];
        assert!(validate_frozen_asset_metadata(&missing_length).is_err());
    }

    #[test]
    fn export_metadata_preflight_rejects_aggregate_table_work() {
        let input = serde_json::json!({
            "assets":[],
            "workspace":{"blocks":[{"content":{"type":"table","row_count":1_000_001u64,
                "column_count":1,"cells":[]}}]},
            "form_definitions":[],
            "attachment_preparations":[]
        });
        assert!(validate_submission_export_metadata(&input).is_err());
    }

    #[tokio::test]
    async fn content_pipeline_abort_is_authoritatively_joined() {
        struct Dropped(Arc<AtomicBool>);
        impl Drop for Dropped {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let dropped = Arc::new(AtomicBool::new(false));
        let observed = dropped.clone();
        let cancel = CancellationToken::new();
        let pipeline_cancel = cancel.clone();
        let mut pipeline = tokio::spawn(async move {
            let _guard = Dropped(observed);
            pipeline_cancel.cancelled().await;
        });
        tokio::task::yield_now().await;
        cancel_and_join_task_until(
            &mut pipeline,
            &cancel,
            tokio::time::Instant::now() + HANDLER_CLEANUP_MARGIN,
        )
        .await;
        assert!(dropped.load(Ordering::SeqCst));
        assert!(pipeline.is_finished());
    }

    #[test]
    fn content_ordinal_three_returns_the_exact_reserved_call_failure() {
        for code in [
            "AGENT_TURN_TIMEOUT",
            "AGENT_PROVIDER_UNAVAILABLE",
            "AGENT_OUTPUT_INVALID",
        ] {
            let failure = bidding::agent_error::AgentError::new(code, "ordinal-three");
            let returned = retain_content_attempt_failure(3, failure).unwrap_err();
            assert_eq!(returned.code, code);
            assert_eq!(returned.message, "ordinal-three");
        }
        assert!(
            retain_content_attempt_failure(
                2,
                bidding::agent_error::AgentError::new("AGENT_PROVIDER_UNAVAILABLE", "retryable",),
            )
            .unwrap()
            .contains("AGENT_PROVIDER_UNAVAILABLE")
        );
    }

    #[test]
    fn content_candidate_verifier_rejects_unfrozen_targets_and_unknown_fields() {
        use bidding::content_block::{
            BlockContent, BlockKind, BlockOrigin, ContentBlockV1, Inline, RichNode,
        };
        let lineage = Uuid::new_v4();
        let content = BlockContent::RichText {
            nodes: vec![RichNode::Paragraph {
                content: vec![Inline::Text {
                    text: "【待人工补充】候选响应".into(),
                    marks: vec![],
                }],
            }],
        };
        let block = ContentBlockV1 {
            schema_version: 1,
            block_revision_id: Uuid::new_v4(),
            lineage_id: Uuid::new_v4(),
            revision: 1,
            kind: BlockKind::RichText,
            content_sha256: content.sha256().unwrap(),
            content,
            origin: BlockOrigin::AgentCandidate,
        };
        let requirement = Uuid::new_v4();
        let input = serde_json::json!({"target_nodes":[{"node_lineage_id":lineage,
            "node_revision_id":Uuid::new_v4(),"block_count":0,"blocks":[]}],
            "requirements":[{"requirement_revision_id":requirement}],
            "fill_policy":"append_candidate","generation_dependency_sha256":"a".repeat(64)});
        let output = serde_json::json!({"schema_version":1,"operations":[{
            "kind":"insert_block","client_operation_ref":"op-0","target_node_lineage_id":lineage,
            "ordinal":0,"block":block}],"factual_claims":[],"notices":[]});
        assert!(content_candidate_output(&serde_json::to_string(&output).unwrap(), &input).is_ok());
        let mut invalid_notice = output.clone();
        invalid_notice["notices"] = serde_json::json!([{
            "code":"NO_EVIDENCE","severity":"warning","message":"需要证据",
            "requirement_revision_id":Uuid::new_v4()
        }]);
        assert!(
            content_candidate_output(&serde_json::to_string(&invalid_notice).unwrap(), &input)
                .is_err()
        );
        let mut invalid = output;
        invalid["operations"][0]["target_node_lineage_id"] = serde_json::json!(Uuid::new_v4());
        assert!(
            content_candidate_output(&serde_json::to_string(&invalid).unwrap(), &input).is_err()
        );
    }

    #[test]
    fn content_candidate_verifier_accepts_only_frozen_image_evidence_assets() {
        use bidding::content_block::{
            BlockContent, BlockKind, BlockOrigin, ContentBlockV1, Crop, ImageAlignment,
        };
        let node = Uuid::new_v4();
        let evidence_item = Uuid::new_v4();
        let content = BlockContent::Image {
            asset_revision_id: evidence_item,
            width_mm: 120.0,
            alignment: ImageAlignment::Center,
            crop: Crop {
                left: 0.0,
                top: 0.0,
                right: 0.0,
                bottom: 0.0,
            },
            caption: Some("产品实拍图".into()),
            alt: "产品实拍图".into(),
        };
        let block = ContentBlockV1 {
            schema_version: 1,
            block_revision_id: Uuid::new_v4(),
            lineage_id: Uuid::new_v4(),
            revision: 1,
            kind: BlockKind::Image,
            content_sha256: content.sha256().unwrap(),
            content,
            origin: BlockOrigin::AgentCandidate,
        };
        let input = serde_json::json!({
            "target_nodes":[{"node_lineage_id":node,"node_revision_id":Uuid::new_v4(),
                "block_count":0,"blocks":[]}],
            "fill_policy":"append_candidate","generation_dependency_sha256":"a".repeat(64),
            "evidence_matches":[{"items":[{"kind":"image","evidence_item_id":evidence_item}]}]
        });
        let output = serde_json::json!({"schema_version":1,"operations":[{
            "kind":"insert_block","client_operation_ref":"image-0","target_node_lineage_id":node,
            "ordinal":0,"block":block}],"factual_claims":[],"notices":[]});
        assert!(content_candidate_output(&serde_json::to_string(&output).unwrap(), &input).is_ok());
        let mut invalid = output;
        invalid["operations"][0]["block"]["content"]["asset_revision_id"] =
            serde_json::json!(Uuid::new_v4());
        assert!(
            content_candidate_output(&serde_json::to_string(&invalid).unwrap(), &input).is_err()
        );
    }

    #[test]
    fn content_candidate_verifier_rejects_forbidden_content_code_links_and_offset_drift() {
        let node = Uuid::new_v4();
        let bundle = Uuid::new_v4();
        let item = Uuid::new_v4();
        let input = serde_json::json!({
            "target_nodes":[{"node_lineage_id":node,"node_revision_id":Uuid::new_v4(),
                "block_count":0,"blocks":[]}],
            "requirements":[],"fill_policy":"append_candidate",
            "generation_dependency_sha256":"a".repeat(64),
            "evidence_matches":[{"evidence_bundle_id":bundle,"items":[{
                "kind":"text_quote","evidence_item_id":item,"quote_utf8":"中文事实",
                "quote_start_offset":0,"quote_end_offset":12}]}]
        });
        let block = |content: serde_json::Value, kind: &str| {
            serde_json::json!({
                "schema_version":1,"block_revision_id":Uuid::new_v4(),"lineage_id":Uuid::new_v4(),
                "revision":1,"kind":kind,"content_sha256":"a".repeat(64),
                "content":content,"origin":"agent_candidate"
            })
        };
        let output = |block: serde_json::Value| {
            serde_json::json!({
                "schema_version":1,"operations":[{"kind":"insert_block","client_operation_ref":"op",
                    "target_node_lineage_id":node,"ordinal":0,"block":block}],
                "factual_claims":[],"notices":[]
            })
        };
        let forbidden = [
            (
                serde_json::json!({"type":"attachment_ref","asset_revision_id":Uuid::new_v4(),
                "preparation_revision_id":null,"render_mode":"file_reference","start_new_page":false}),
                "attachment_ref",
            ),
            (
                serde_json::json!({"type":"structured_form","form_definition_revision_id":Uuid::new_v4(),"field_values":[]}),
                "structured_form",
            ),
            (serde_json::json!({"type":"page_break"}), "page_break"),
            (
                serde_json::json!({"type":"signature_placeholder","signature_kind":"signature",
                "width_mm":30.0,"height_mm":20.0,"label":"签字"}),
                "signature_placeholder",
            ),
        ];
        for (content, kind) in forbidden {
            let value = output(block(content, kind));
            assert!(
                content_candidate_output(&value.to_string(), &input).is_err(),
                "{kind}"
            );
        }
        for node_value in [
            serde_json::json!({"kind":"code_block","language":"sql","text":"SELECT 1"}),
            serde_json::json!({"kind":"paragraph","content":[{"kind":"text","text":"x",
                "marks":[{"kind":"code"}]}]}),
            serde_json::json!({"kind":"paragraph","content":[{"kind":"text","text":"x",
                "marks":[{"kind":"link","href":"https://example.invalid"}]}]}),
        ] {
            let value = output(block(
                serde_json::json!({"type":"rich_text","nodes":[node_value]}),
                "rich_text",
            ));
            assert!(content_candidate_output(&value.to_string(), &input).is_err());
        }
        let mut evidenced = output(block(
            serde_json::json!({"type":"rich_text","nodes":[{
            "kind":"paragraph","content":[{"kind":"text","text":"中文事实","marks":[{
                "kind":"evidence_ref","evidence_bundle_id":bundle,"evidence_item_id":item,
                "quote_start_offset":0,"quote_end_offset":12}]}]}]}),
            "rich_text",
        ));
        evidenced["factual_claims"] = serde_json::json!([{"client_operation_ref":"op",
            "utf8_start":0,"utf8_end":12,"evidence_bundle_id":bundle,"evidence_item_id":item}]);
        assert!(content_candidate_output(&evidenced.to_string(), &input).is_ok());
        evidenced["operations"][0]["block"]["content"]["nodes"][0]["content"][0]["marks"][0]["quote_end_offset"] =
            serde_json::json!(11);
        assert!(content_candidate_output(&evidenced.to_string(), &input).is_err());
    }

    #[derive(Clone, Copy)]
    enum LifecycleProviderResult {
        Success,
        Unavailable,
    }

    struct LifecycleProvider {
        calls: AtomicUsize,
        results: std::sync::Mutex<VecDeque<LifecycleProviderResult>>,
    }

    #[async_trait]
    impl knowledge::knowledge_index_v2::VectorEmbeddingProviderV2 for LifecycleProvider {
        async fn embed_batch(
            &self,
            _revision: &knowledge::knowledge_retrieval::EmbeddingRevisionV2,
            _credential_ref: &str,
            inputs: &[knowledge::knowledge_index_v2::VectorEmbeddingInputV2],
        ) -> Result<Vec<Vec<f32>>, knowledge::knowledge_index_v2::VectorIndexErrorV2> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            match self
                .results
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(LifecycleProviderResult::Success)
            {
                LifecycleProviderResult::Success => Ok(inputs
                    .iter()
                    .enumerate()
                    .map(|(index, _)| {
                        let mut vector = vec![0.0; 1024];
                        vector[index % 1024] = 1.0;
                        vector
                    })
                    .collect()),
                LifecycleProviderResult::Unavailable => Err(
                    knowledge::knowledge_index_v2::VectorIndexErrorV2::Unavailable(
                        "injected provider timeout".into(),
                    ),
                ),
            }
        }
    }

    struct PendingAfterLifecycleProvider {
        pool: PgPool,
        document_id: Uuid,
        calls: AtomicUsize,
    }

    #[async_trait]
    impl knowledge::knowledge_index_v2::VectorEmbeddingProviderV2 for PendingAfterLifecycleProvider {
        async fn embed_batch(
            &self,
            _revision: &knowledge::knowledge_retrieval::EmbeddingRevisionV2,
            _credential_ref: &str,
            inputs: &[knowledge::knowledge_index_v2::VectorEmbeddingInputV2],
        ) -> Result<Vec<Vec<f32>>, knowledge::knowledge_index_v2::VectorIndexErrorV2> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            sqlx::query("UPDATE documents SET pending_subtasks_count=1 WHERE id=$1")
                .bind(self.document_id)
                .execute(&self.pool)
                .await
                .map_err(knowledge::knowledge_index_v2::VectorIndexErrorV2::Database)?;
            Ok(inputs
                .iter()
                .enumerate()
                .map(|(index, _)| {
                    let mut vector = vec![0.0; 1024];
                    vector[index % 1024] = 1.0;
                    vector
                })
                .collect())
        }
    }

    struct MissingLifecycleCredential {
        calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl knowledge::knowledge_index_v2::EmbeddingCredentialResolverV2 for MissingLifecycleCredential {
        async fn resolve(
            &self,
            _credential_ref: &str,
        ) -> Result<String, knowledge::knowledge_index_v2::VectorIndexErrorV2> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Err(
                knowledge::knowledge_index_v2::VectorIndexErrorV2::InvalidConfiguration(
                    "injected missing credential reference".into(),
                ),
            )
        }
    }

    #[test]
    fn semantic_index_v2_uses_the_native_three_by_ten_oxana_policy() {
        let job = KnowledgeSemanticIndexV2Job {
            target_id: Uuid::parse_str("018f3000-7d47-7a1b-9bb8-b3880f15478a").unwrap(),
            target_revision: 7,
        };
        let worker = KnowledgeSemanticIndexV2Worker {
            pool: None,
            provider: None,
            provider_configuration_error: None,
        };
        assert_eq!(
            <KnowledgeSemanticIndexV2Worker as oxana::Worker<
                KnowledgeSemanticIndexV2Job,
            >>::max_retries(&worker, &job),
            3
        );
        for retries in 0..=3 {
            assert_eq!(
                <KnowledgeSemanticIndexV2Worker as oxana::Worker<
                    KnowledgeSemanticIndexV2Job,
                >>::retry_delay(&worker, &job, retries),
                10
            );
        }
    }

    #[test]
    fn reuse_reads_scanned_pdf_from_docreader_span() {
        let tagged = serde_json::json!({"image_source_type": "scanned_pdf"});
        assert_eq!(
            image_source_from_docreader_output(Some(&tagged)),
            "scanned_pdf"
        );
        let fallback = serde_json::json!({"anydoc_fallback": "scanned_pdf"});
        assert_eq!(
            image_source_from_docreader_output(Some(&fallback)),
            "scanned_pdf"
        );
        assert_eq!(image_source_from_docreader_output(None), "");
    }
    use tokio::sync::Mutex;

    async fn db_lock() -> tokio::sync::MutexGuard<'static, ()> {
        static LOCK: Mutex<()> = Mutex::const_new(());
        LOCK.lock().await
    }

    // Destructive schema tests must opt into a dedicated isolated database. Never
    // inherit DATABASE_URL: production Compose is intentionally exposed on :15432.
    fn destructive_test_database_url() -> Result<String, sqlx::Error> {
        let database_url = std::env::var("KNOWLEDGEBRAIN_TEST_DATABASE_URL").map_err(|_| {
            sqlx::Error::Configuration(
                "KNOWLEDGEBRAIN_TEST_DATABASE_URL is required for destructive PostgreSQL tests"
                    .into(),
            )
        })?;
        if database_url.contains(":15432/") {
            return Err(sqlx::Error::Configuration(
                "destructive PostgreSQL tests refuse the live :15432 database".into(),
            ));
        }
        Ok(database_url)
    }

    // Tokio creates a separate runtime for each async unit test. A process-global
    // PgPool can retain runtime-bound connections between tests and time out.
    async fn connect() -> Result<sqlx::PgPool, sqlx::Error> {
        let database_url = destructive_test_database_url()?;
        sqlx::postgres::PgPoolOptions::new()
            .max_connections(16)
            .connect(&database_url)
            .await
    }

    async fn reset_test_schema(pool: &sqlx::PgPool) {
        sqlx::raw_sql(
            "DROP SCHEMA public CASCADE;
             CREATE SCHEMA public;
             GRANT ALL ON SCHEMA public TO CURRENT_USER;
             CREATE EXTENSION IF NOT EXISTS pgcrypto;
             CREATE EXTENSION IF NOT EXISTS vector;
             ALTER SCHEMA public OWNER TO kb_app_owner;",
        )
        .execute(pool)
        .await
        .unwrap();
        apply_fresh_baseline(pool).await.unwrap();
    }

    #[test]
    fn wiki_ingest_retry_delay_is_lock_retry() {
        let w = WikiIngestWorker { pool: None };
        let job = WikiIngestJob {
            product_version_id: Uuid::new_v4(),
            document_id: Uuid::new_v4(),
            operation: knowledge::wiki::OP_INGEST.into(),
            task_type: platform::TYPE_WIKI_INGEST.to_string(),
        };
        assert_eq!(
            oxana::Worker::<WikiIngestJob>::retry_delay(&w, &job, 0),
            platform::WIKI_LOCK_RETRY_SECS
        );
        assert_eq!(
            oxana::Worker::<WikiIngestJob>::retry_delay(&w, &job, 4),
            platform::WIKI_LOCK_RETRY_SECS
        );
        assert_eq!(
            knowledge::wiki::INGEST_DEBOUNCE_SECS,
            platform::WIKI_INGEST_DEBOUNCE_SECS
        );
        assert_eq!(
            knowledge::wiki::FINALIZE_DEBOUNCE_SECS,
            platform::WIKI_FINALIZE_DEBOUNCE_SECS
        );
        assert_eq!(
            knowledge::wiki::FOLLOW_UP_DEBOUNCE_SECS,
            platform::WIKI_FOLLOW_UP_DEBOUNCE_SECS
        );
        assert_eq!(
            knowledge::wiki::LOCK_RETRY_SECS,
            platform::WIKI_LOCK_RETRY_SECS
        );
    }

    #[tokio::test]
    async fn list_delete_skips_non_deleting_rows() {
        let _g = db_lock().await;
        let Ok(pool) = connect().await else {
            eprintln!("skip: postgres down");
            return;
        };
        reset_test_schema(&pool).await;
        let owner = Uuid::new_v4();
        insert_user(&pool, owner, &format!("{owner}@ex.com"), None)
            .await
            .unwrap();
        let seeded = create_workspace_with_library(&pool, owner, "Ld", "ld")
            .await
            .unwrap();
        let did = Uuid::new_v4();
        let hash = platform::sha256_hex(b"keep");
        write_blob(&hash, b"keep").unwrap();
        insert_document(
            &pool,
            knowledge::NewDocument {
                id: did,
                product_version_id: seeded.library_version_id,
                title: "k",
                file_name: "k.txt",
                file_size: 4,
                file_hash: &hash,
                object_ref: &format!("objects/{hash}"),
            },
        )
        .await
        .unwrap();
        process_list_delete_pg(&pool, did).await.unwrap();
        let gone: bool =
            sqlx::query_scalar("SELECT deleted_at IS NOT NULL FROM documents WHERE id = $1")
                .bind(did)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(!gone, "pending row must survive list_delete");
        sqlx::query("UPDATE documents SET parse_status = 'deleting' WHERE id = $1")
            .bind(did)
            .execute(&pool)
            .await
            .unwrap();
        process_list_delete_pg(&pool, did).await.unwrap();
        let gone: bool =
            sqlx::query_scalar("SELECT deleted_at IS NOT NULL FROM documents WHERE id = $1")
                .bind(did)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(gone);
    }

    #[tokio::test]
    async fn persist_blank_chunks_completes_without_postprocess() {
        let _g = db_lock().await;
        let Ok(pool) = connect().await else {
            eprintln!("skip: postgres down");
            return;
        };
        reset_test_schema(&pool).await;
        let owner = Uuid::new_v4();
        insert_user(&pool, owner, &format!("{owner}@ex.com"), None)
            .await
            .unwrap();
        let seeded = create_workspace_with_library(&pool, owner, "Blank", "blank")
            .await
            .unwrap();
        let did = Uuid::new_v4();
        insert_document(
            &pool,
            knowledge::NewDocument {
                id: did,
                product_version_id: seeded.library_version_id,
                title: "empty",
                file_name: "e.txt",
                file_size: 1,
                file_hash: "ff71cf74abb3ccb005b8b64371725db15edc42c1ad33413bbe561b2da3c85ef9",
                object_ref: "objects/ff71cf74abb3ccb005b8b64371725db15edc42c1ad33413bbe561b2da3c85ef9",
            },
        )
        .await
        .unwrap();
        sqlx::query("UPDATE documents SET parse_status = 'processing' WHERE id = $1")
            .bind(did)
            .execute(&pool)
            .await
            .unwrap();
        let blank = knowledge::Chunk {
            id: Uuid::new_v4(),
            document_id: did,
            product_version_id: seeded.library_version_id,
            chunk_type: "text".into(),
            content: "  \n".into(),
            context_header: String::new(),
            start_at: 0,
            end_at: 0,
            parent_chunk_id: None,
            generated_questions: Vec::new(),
        };
        let out =
            persist_indexed_chunks(&pool, did, seeded.library_version_id, &[blank], true, true)
                .await
                .unwrap();
        let PersistIndexResult::Written { text_count } = out else {
            panic!("expected written");
        };
        assert_eq!(text_count, 0);
        after_index_fanout(
            &pool,
            did,
            seeded.library_version_id,
            1,
            text_count,
            &[],
            "e.txt",
        )
        .await
        .unwrap();
        let (parse, enable, summary): (String, String, String) = sqlx::query_as(
            "SELECT parse_status, enable_status, summary_status FROM documents WHERE id = $1",
        )
        .bind(did)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(parse, "completed");
        assert_eq!(enable, "enabled");
        assert_eq!(summary, "none");
        let n: i64 = sqlx::query_scalar("SELECT count(*) FROM chunks WHERE document_id = $1")
            .bind(did)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(n, 0);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn persist_indexed_chunks_keeps_rows_when_embed_fails() {
        let _g = db_lock().await;
        let Ok(pool) = connect().await else {
            eprintln!("skip: postgres down");
            return;
        };
        reset_test_schema(&pool).await;
        let owner = Uuid::new_v4();
        insert_user(&pool, owner, &format!("{owner}@ex.com"), None)
            .await
            .unwrap();
        let seeded = create_workspace_with_library(&pool, owner, "Ef", "ef")
            .await
            .unwrap();
        let did = Uuid::new_v4();
        let hash = platform::sha256_hex(b"body");
        write_blob(&hash, b"body").unwrap();
        insert_document(
            &pool,
            knowledge::NewDocument {
                id: did,
                product_version_id: seeded.library_version_id,
                title: "t",
                file_name: "t.txt",
                file_size: 4,
                file_hash: &hash,
                object_ref: &format!("objects/{hash}"),
            },
        )
        .await
        .unwrap();
        sqlx::query("UPDATE documents SET parse_status = 'processing' WHERE id = $1")
            .bind(did)
            .execute(&pool)
            .await
            .unwrap();
        let ch = knowledge::Chunk {
            id: Uuid::new_v4(),
            document_id: did,
            product_version_id: seeded.library_version_id,
            chunk_type: "text".into(),
            content: "keep this chunk".into(),
            context_header: String::new(),
            start_at: 0,
            end_at: 15,
            parent_chunk_id: None,
            generated_questions: Vec::new(),
        };
        let prev_base = std::env::var("KNOWLEDGEBRAIN_EMBEDDING_BASE_URL").ok();
        let prev_alias = std::env::var("EMBEDDING_BASE_URL").ok();
        unsafe {
            std::env::set_var("KNOWLEDGEBRAIN_EMBEDDING_BASE_URL", "http://127.0.0.1:1");
            std::env::set_var("EMBEDDING_BASE_URL", "http://127.0.0.1:1");
        }
        let err =
            match persist_indexed_chunks(&pool, did, seeded.library_version_id, &[ch], true, true)
                .await
            {
                Ok(_) => panic!("embed must fail"),
                Err(e) => e,
            };
        unsafe {
            match prev_base {
                Some(v) => std::env::set_var("KNOWLEDGEBRAIN_EMBEDDING_BASE_URL", v),
                None => std::env::remove_var("KNOWLEDGEBRAIN_EMBEDDING_BASE_URL"),
            }
            match prev_alias {
                Some(v) => std::env::set_var("EMBEDDING_BASE_URL", v),
                None => std::env::remove_var("EMBEDDING_BASE_URL"),
            }
        }
        assert!(
            err.contains("embed") || err.contains("error") || err.contains("connect"),
            "{err}"
        );
        let n: i64 = sqlx::query_scalar("SELECT count(*) FROM chunks WHERE document_id = $1")
            .bind(did)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(n, 1, "chunk row must survive embed failure");
        let e: i64 =
            sqlx::query_scalar("SELECT count(*) FROM chunk_embeddings WHERE document_id = $1")
                .bind(did)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(e, 0);
    }

    #[tokio::test]
    async fn convert_reuses_markdown_and_chunks_after_embed_fail() {
        let _g = db_lock().await;
        let Ok(pool) = connect().await else {
            eprintln!("skip: postgres down");
            return;
        };
        reset_test_schema(&pool).await;
        let owner = Uuid::new_v4();
        insert_user(&pool, owner, &format!("{owner}@ex.com"), None)
            .await
            .unwrap();
        let seeded = create_workspace_with_library(&pool, owner, "Ru", "ru")
            .await
            .unwrap();
        let did = Uuid::new_v4();
        let hash = platform::sha256_hex(b"hello reuse");
        write_blob(&hash, b"hello reuse").unwrap();
        insert_document(
            &pool,
            knowledge::NewDocument {
                id: did,
                product_version_id: seeded.library_version_id,
                title: "r",
                file_name: "r.txt",
                file_size: 11,
                file_hash: &hash,
                object_ref: &format!("objects/{hash}"),
            },
        )
        .await
        .unwrap();
        convert_document(&pool, did, 1, &[], false).await.unwrap();
        let started: String = sqlx::query_scalar(
            "SELECT started_at::text FROM document_processing_spans
             WHERE document_id = $1 AND name = 'docreader'",
        )
        .bind(did)
        .fetch_one(&pool)
        .await
        .unwrap();
        let chunk_ids: Vec<Uuid> =
            sqlx::query_scalar("SELECT id FROM chunks WHERE document_id = $1 ORDER BY id")
                .bind(did)
                .fetch_all(&pool)
                .await
                .unwrap();
        assert!(!chunk_ids.is_empty());
        sqlx::query("DELETE FROM chunk_embeddings WHERE document_id = $1")
            .bind(did)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "UPDATE document_processing_spans SET status = 'failed', finished_at = now()
             WHERE document_id = $1 AND name = 'embedding'",
        )
        .bind(did)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("UPDATE documents SET parse_status = 'processing' WHERE id = $1")
            .bind(did)
            .execute(&pool)
            .await
            .unwrap();
        convert_document(&pool, did, 1, &[], false).await.unwrap();
        let started2: String = sqlx::query_scalar(
            "SELECT started_at::text FROM document_processing_spans
             WHERE document_id = $1 AND name = 'docreader'",
        )
        .bind(did)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(started, started2, "docreader must not rerun");
        let chunk_ids2: Vec<Uuid> =
            sqlx::query_scalar("SELECT id FROM chunks WHERE document_id = $1 ORDER BY id")
                .bind(did)
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(chunk_ids, chunk_ids2);
        let emb: i64 =
            sqlx::query_scalar("SELECT count(*) FROM chunk_embeddings WHERE document_id = $1")
                .bind(did)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(emb, chunk_ids.len() as i64);
    }

    #[tokio::test]
    async fn convert_simple_txt_sets_processing_and_span() {
        let _g = db_lock().await;
        let Ok(pool) = connect().await else {
            eprintln!("skip: postgres down");
            return;
        };
        reset_test_schema(&pool).await;
        let owner = Uuid::new_v4();
        insert_user(&pool, owner, &format!("{owner}@ex.com"), None)
            .await
            .unwrap();
        let seeded = create_workspace_with_library(&pool, owner, "W", "w")
            .await
            .unwrap();
        let did = Uuid::new_v4();
        let hash = platform::sha256_hex(b"hello worker");
        write_blob(&hash, b"hello worker").unwrap();
        insert_document(
            &pool,
            knowledge::NewDocument {
                id: did,
                product_version_id: seeded.library_version_id,
                title: "a",
                file_name: "a.txt",
                file_size: 12,
                file_hash: &hash,
                object_ref: &format!("objects/{hash}"),
            },
        )
        .await
        .unwrap();
        convert_document(&pool, did, 1, &[], false).await.unwrap();
        let status: String = sqlx::query_scalar("SELECT parse_status FROM documents WHERE id = $1")
            .bind(did)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(status, "processing");
        let span: String = sqlx::query_scalar(
            "SELECT status FROM document_processing_spans
             WHERE document_id = $1 AND name = 'docreader'",
        )
        .bind(did)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(span, "done");
        let stages: Vec<(String, String)> = sqlx::query_as(
            "SELECT name, status FROM document_processing_spans
             WHERE document_id = $1 AND kind = 'stage' ORDER BY name",
        )
        .bind(did)
        .fetch_all(&pool)
        .await
        .unwrap();
        let map: std::collections::HashMap<_, _> = stages.into_iter().collect();
        assert_eq!(map.get("docreader").map(String::as_str), Some("done"));
        assert_eq!(map.get("chunking").map(String::as_str), Some("done"));
        assert_eq!(map.get("embedding").map(String::as_str), Some("done"));
        assert_eq!(map.get("multimodal").map(String::as_str), Some("skipped"));
        assert_eq!(map.get("postprocess").map(String::as_str), Some("running"));
        assert!(platform::blob_exists(&format!("{hash}.md")));
        let enabled: String =
            sqlx::query_scalar("SELECT enable_status FROM documents WHERE id = $1")
                .bind(did)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(enabled, "enabled");
        let n: i64 = sqlx::query_scalar("SELECT count(*) FROM chunks WHERE document_id = $1")
            .bind(did)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(n >= 1, "convert must persist chunks");
        let en: i64 =
            sqlx::query_scalar("SELECT count(*) FROM chunk_embeddings WHERE document_id = $1")
                .bind(did)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(en, n);
    }

    #[tokio::test]
    async fn convert_pdf_without_reader_fails_immediately() {
        let _g = db_lock().await;
        unsafe { std::env::remove_var("DOCREADER_ADDR") };
        let Ok(pool) = connect().await else {
            eprintln!("skip: postgres down");
            return;
        };
        reset_test_schema(&pool).await;
        let owner = Uuid::new_v4();
        insert_user(&pool, owner, &format!("{owner}@ex.com"), None)
            .await
            .unwrap();
        let seeded = create_workspace_with_library(&pool, owner, "W2", "w2")
            .await
            .unwrap();
        let did = Uuid::new_v4();
        let hash = platform::sha256_hex(b"%PDF-1.4");
        write_blob(&hash, b"%PDF-1.4").unwrap();
        insert_document(
            &pool,
            knowledge::NewDocument {
                id: did,
                product_version_id: seeded.library_version_id,
                title: "p",
                file_name: "p.pdf",
                file_size: 8,
                file_hash: &hash,
                object_ref: &format!("objects/{hash}"),
            },
        )
        .await
        .unwrap();
        convert_document(&pool, did, 1, &[], false).await.unwrap();
        let (status, err): (String, String) = sqlx::query_as(
            "SELECT parse_status, COALESCE(error_message,'') FROM documents WHERE id = $1",
        )
        .bind(did)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(status, "failed");
        assert!(err.contains("DOCREADER_ADDR"), "{err}");
        let cancelled: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM document_processing_spans
             WHERE document_id = $1 AND status = 'cancelled'",
        )
        .bind(did)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(
            cancelled >= 1,
            "failed docreader must cancel dependent stages, got {cancelled}"
        );
        let post: Option<String> = sqlx::query_scalar(
            "SELECT status FROM document_processing_spans
             WHERE document_id = $1 AND name = 'postprocess'",
        )
        .bind(did)
        .fetch_optional(&pool)
        .await
        .unwrap();
        assert_ne!(post.as_deref(), Some("running"));
    }

    #[tokio::test]
    async fn convert_markdown_with_image_enqueues_multimodal() {
        let _g = db_lock().await;
        let Ok(pool) = connect().await else {
            eprintln!("skip: postgres down");
            return;
        };
        reset_test_schema(&pool).await;
        let owner = Uuid::new_v4();
        insert_user(&pool, owner, &format!("{owner}@ex.com"), None)
            .await
            .unwrap();
        let seeded = create_workspace_with_library(&pool, owner, "Mm", "mm")
            .await
            .unwrap();
        sqlx::query(
            "UPDATE product_versions SET image_processing_config = '{\"enable_multimodel\":true}'::jsonb
             WHERE id = $1",
        )
        .bind(seeded.library_version_id)
        .execute(&pool)
        .await
        .unwrap();
        let body = b"See ![p](images/p1.jpg) in the guide.";
        let did = Uuid::new_v4();
        let hash = platform::sha256_hex(body);
        write_blob(&hash, body).unwrap();
        insert_document(
            &pool,
            knowledge::NewDocument {
                id: did,
                product_version_id: seeded.library_version_id,
                title: "g",
                file_name: "g.md",
                file_size: body.len() as i64,
                file_hash: &hash,
                object_ref: &format!("objects/{hash}"),
            },
        )
        .await
        .unwrap();
        convert_document(&pool, did, 1, &[], false).await.unwrap();
        if platform::vlm_configured() {
            let Ok(storage) = platform::oxana_connect() else {
                eprintln!("skip: redis down");
                return;
            };
            let n = storage
                .enqueued_count(platform::MultimodalQueue)
                .await
                .unwrap();
            assert!(n >= 1, "image:multimodal must be enqueued, got {n}");
            assert_eq!(knowledge::enrichment::pending_count(did), Some(1));
            let _ = storage.wipe_queue(platform::MultimodalQueue).await;
        } else {
            let (status, err): (String, String) = sqlx::query_as(
                "SELECT parse_status, COALESCE(error_message,'') FROM documents WHERE id = $1",
            )
            .bind(did)
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(status, "finalizing");
            assert!(err.contains("ocr_error"), "{err}");
            let ready: bool = sqlx::query_scalar("SELECT index_ready FROM documents WHERE id = $1")
                .bind(did)
                .fetch_one(&pool)
                .await
                .unwrap();
            assert!(!ready, "images without VLM must not be searchable");
        }
    }

    #[tokio::test]
    async fn convert_audio_without_asr_fails_immediately() {
        let _g = db_lock().await;
        let Ok(pool) = connect().await else {
            eprintln!("skip: postgres down");
            return;
        };
        reset_test_schema(&pool).await;
        let owner = Uuid::new_v4();
        insert_user(&pool, owner, &format!("{owner}@ex.com"), None)
            .await
            .unwrap();
        let seeded = create_workspace_with_library(&pool, owner, "W3", "w3")
            .await
            .unwrap();
        let did = Uuid::new_v4();
        let hash = platform::sha256_hex(b"RIFF");
        write_blob(&hash, b"RIFF").unwrap();
        insert_document(
            &pool,
            knowledge::NewDocument {
                id: did,
                product_version_id: seeded.library_version_id,
                title: "a",
                file_name: "a.wav",
                file_size: 4,
                file_hash: &hash,
                object_ref: &format!("objects/{hash}"),
            },
        )
        .await
        .unwrap();
        convert_document(&pool, did, 1, &[], false).await.unwrap();
        let (status, err): (String, String) = sqlx::query_as(
            "SELECT parse_status, COALESCE(error_message,'') FROM documents WHERE id = $1",
        )
        .bind(did)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(status, "failed");
        assert!(err.contains("ASR"), "{err}");
    }

    #[tokio::test]
    async fn convert_audio_stub_writes_markdown() {
        let _g = db_lock().await;
        let Ok(pool) = connect().await else {
            eprintln!("skip: postgres down");
            return;
        };
        reset_test_schema(&pool).await;
        let owner = Uuid::new_v4();
        insert_user(&pool, owner, &format!("{owner}@ex.com"), None)
            .await
            .unwrap();
        let seeded = create_workspace_with_library(&pool, owner, "W4", "w4")
            .await
            .unwrap();
        sqlx::query(
            "UPDATE product_versions SET asr_model_id = 'stub-asr',
                asr_config = '{\"enabled\":true}'::jsonb WHERE id = $1",
        )
        .bind(seeded.library_version_id)
        .execute(&pool)
        .await
        .unwrap();
        let did = Uuid::new_v4();
        let bytes = b"RIFFWAVE";
        let hash = platform::sha256_hex(bytes);
        write_blob(&hash, bytes).unwrap();
        insert_document(
            &pool,
            knowledge::NewDocument {
                id: did,
                product_version_id: seeded.library_version_id,
                title: "a",
                file_name: "talk.wav",
                file_size: bytes.len() as i64,
                file_hash: &hash,
                object_ref: &format!("objects/{hash}"),
            },
        )
        .await
        .unwrap();
        convert_document(&pool, did, 1, &[], false).await.unwrap();
        let status: String = sqlx::query_scalar("SELECT parse_status FROM documents WHERE id = $1")
            .bind(did)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(status, "processing");
        let md = String::from_utf8(platform::read_blob(&format!("{hash}.md")).unwrap()).unwrap();
        assert_eq!(md, "[stub-asr:talk.wav:8]");
    }

    #[test]
    fn stored_url_blob_is_detected() {
        let (ok, url) = parse_stored_url(b"url:https://docs.example/a.md");
        assert!(ok);
        assert_eq!(url, "https://docs.example/a.md");
        assert!(!parse_stored_url(b"hello").0);
        assert!(!parse_stored_url(b"url:ftp://x").0);
    }

    #[tokio::test]
    async fn convert_passages_skips_reader_and_indexes_each() {
        let _g = db_lock().await;
        let Ok(pool) = connect().await else {
            eprintln!("skip: postgres down");
            return;
        };
        reset_test_schema(&pool).await;
        let owner = Uuid::new_v4();
        insert_user(&pool, owner, &format!("{owner}@ex.com"), None)
            .await
            .unwrap();
        let seeded = create_workspace_with_library(&pool, owner, "Wp", "wp")
            .await
            .unwrap();
        let did = Uuid::new_v4();
        let hash = platform::sha256_hex(b"unused");
        insert_document(
            &pool,
            knowledge::NewDocument {
                id: did,
                product_version_id: seeded.library_version_id,
                title: "p",
                file_name: "p.txt",
                file_size: 6,
                file_hash: &hash,
                object_ref: &format!("objects/{hash}"),
            },
        )
        .await
        .unwrap();
        convert_document(
            &pool,
            did,
            1,
            &["first passage".into(), "second passage".into()],
            false,
        )
        .await
        .unwrap();
        let n: i64 = sqlx::query_scalar("SELECT count(*) FROM chunks WHERE document_id = $1")
            .bind(did)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(n, 2);
        let texts: Vec<String> = sqlx::query_scalar(
            "SELECT content FROM chunks WHERE document_id = $1 ORDER BY content",
        )
        .bind(did)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(texts, vec!["first passage", "second passage"]);
        knowledge::set_document_source(
            &pool,
            did,
            "passage",
            &["first passage".into(), "second passage".into()],
        )
        .await
        .unwrap();
        let attempt = knowledge::mark_reparse_queued(&pool, did).await.unwrap();
        process_reparse_pg(&pool, did, attempt).await.unwrap();
        process_reparse_pg(&pool, did, attempt).await.unwrap();
        let replay_attempt = knowledge::mark_reparse_queued(&pool, did).await.unwrap();
        assert_eq!(replay_attempt, attempt);
        let kind: String = sqlx::query_scalar("SELECT type FROM documents WHERE id = $1")
            .bind(did)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(kind, "passage");
        let attempt: i32 = sqlx::query_scalar("SELECT attempt FROM documents WHERE id = $1")
            .bind(did)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(attempt, replay_attempt);
    }

    #[tokio::test]
    async fn convert_url_without_reader_fails_immediately() {
        let _g = db_lock().await;
        unsafe { std::env::remove_var("DOCREADER_ADDR") };
        let Ok(pool) = connect().await else {
            eprintln!("skip: postgres down");
            return;
        };
        reset_test_schema(&pool).await;
        let owner = Uuid::new_v4();
        insert_user(&pool, owner, &format!("{owner}@ex.com"), None)
            .await
            .unwrap();
        let seeded = create_workspace_with_library(&pool, owner, "Wu", "wu")
            .await
            .unwrap();
        let body = b"url:https://example.com/doc";
        let did = Uuid::new_v4();
        let hash = platform::sha256_hex(body);
        write_blob(&hash, body).unwrap();
        insert_document(
            &pool,
            knowledge::NewDocument {
                id: did,
                product_version_id: seeded.library_version_id,
                title: "u",
                file_name: "remote.md",
                file_size: body.len() as i64,
                file_hash: &hash,
                object_ref: &format!("objects/{hash}"),
            },
        )
        .await
        .unwrap();
        convert_document(&pool, did, 1, &[], false).await.unwrap();
        let (status, err): (String, String) = sqlx::query_as(
            "SELECT parse_status, COALESCE(error_message,'') FROM documents WHERE id = $1",
        )
        .bind(did)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(status, "failed");
        assert!(err.contains("DOCREADER_ADDR"), "{err}");
    }

    #[tokio::test]
    async fn wiki_ingest_job_is_direct_idempotent_and_finalizes() {
        let _g = db_lock().await;
        let Ok(pool) = connect().await else {
            eprintln!("skip: postgres down");
            return;
        };
        reset_test_schema(&pool).await;
        let owner = Uuid::new_v4();
        insert_user(&pool, owner, &format!("{owner}@ex.com"), None)
            .await
            .unwrap();
        let seeded = create_workspace_with_library(&pool, owner, "Ww", "ww")
            .await
            .unwrap();
        let vid = seeded.library_version_id;
        let did = Uuid::new_v4();
        let hash = platform::sha256_hex(b"wiki body");
        write_blob(&hash, b"wiki body").unwrap();
        insert_document(
            &pool,
            knowledge::NewDocument {
                id: did,
                product_version_id: vid,
                title: "w",
                file_name: "w.txt",
                file_size: 9,
                file_hash: &hash,
                object_ref: &format!("objects/{hash}"),
            },
        )
        .await
        .unwrap();
        sqlx::query(
            "UPDATE documents SET parse_status = 'finalizing', pending_subtasks_count = 1
             WHERE id = $1",
        )
        .bind(did)
        .execute(&pool)
        .await
        .unwrap();
        let cid = Uuid::new_v4();
        knowledge::replace_document_chunks(
            &pool,
            did,
            &[knowledge::Chunk {
                id: cid,
                document_id: did,
                product_version_id: vid,
                chunk_type: "text".into(),
                content: "wiki body about the product".into(),
                context_header: String::new(),
                start_at: 0,
                end_at: 27,
                parent_chunk_id: None,
                generated_questions: vec![],
            }],
            &[],
        )
        .await
        .unwrap();
        if let Err(error) = process_wiki_ingest(&pool, vid, did, knowledge::wiki::OP_INGEST).await {
            assert!(error.contains("Oxana Redis is not configured"), "{error}");
        }
        process_wiki_finalize(&pool, vid, did).await.unwrap();
        let postgres_queue_tables: bool = sqlx::query_scalar(
            "SELECT to_regclass('public.task_pending_ops') IS NOT NULL
                 OR to_regclass('public.task_dead_letters') IS NOT NULL",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(!postgres_queue_tables);
        let (status, pending): (String, i32) = sqlx::query_as(
            "SELECT parse_status, pending_subtasks_count FROM documents WHERE id = $1",
        )
        .bind(did)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(pending, 0);
        assert_eq!(status, "completed");
        let span: String = sqlx::query_scalar(
            "SELECT status FROM document_processing_spans
             WHERE document_id = $1 AND name = 'wiki.ingest'",
        )
        .bind(did)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(span, "done");
        let pages: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM wiki_pages WHERE product_version_id = $1 AND status = 'published'",
        )
        .bind(vid)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(pages >= 1, "wiki page persisted");
        let wiki_chunks: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM chunks WHERE document_id = $1 AND chunk_type = 'wiki_page'",
        )
        .bind(did)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(wiki_chunks >= 1, "wiki_page chunk persisted");
        let original_page: (Uuid, String, serde_json::Value) = sqlx::query_as(
            "SELECT id,content,source_refs FROM wiki_pages WHERE product_version_id=$1
               AND source_refs @> jsonb_build_array($2::text) ORDER BY slug LIMIT 1",
        )
        .bind(vid)
        .bind(did.to_string())
        .fetch_one(&pool)
        .await
        .unwrap();

        let mut concurrent_ids = Vec::new();
        for suffix in ["two", "three"] {
            let document_id = Uuid::new_v4();
            concurrent_ids.push(document_id);
            insert_document(
                &pool,
                knowledge::NewDocument {
                    id: document_id,
                    product_version_id: vid,
                    title: "w",
                    file_name: &format!("w-{suffix}.txt"),
                    file_size: 9,
                    file_hash: &hash,
                    object_ref: &format!("objects/{hash}"),
                },
            )
            .await
            .unwrap();
            sqlx::query(
                "UPDATE documents SET parse_status='finalizing', pending_subtasks_count=1 WHERE id=$1",
            )
            .bind(document_id)
            .execute(&pool)
            .await
            .unwrap();
            let chunk_id = Uuid::new_v4();
            knowledge::replace_document_chunks(
                &pool,
                document_id,
                &[knowledge::Chunk {
                    id: chunk_id,
                    document_id,
                    product_version_id: vid,
                    chunk_type: "text".into(),
                    content: "wiki body about the product".into(),
                    context_header: String::new(),
                    start_at: 0,
                    end_at: 27,
                    parent_chunk_id: None,
                    generated_questions: vec![],
                }],
                &[],
            )
            .await
            .unwrap();
        }
        let left_pool = pool.clone();
        let right_pool = pool.clone();
        let left = concurrent_ids[0];
        let right = concurrent_ids[1];
        let (left_result, right_result) = tokio::join!(
            process_wiki_ingest(&left_pool, vid, left, knowledge::wiki::OP_INGEST),
            process_wiki_ingest(&right_pool, vid, right, knowledge::wiki::OP_INGEST),
        );
        for result in [left_result, right_result] {
            if let Err(error) = result {
                assert!(error.contains("Oxana Redis is not configured"), "{error}");
            }
        }
        for document_id in [did, left, right] {
            let owned_page: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM wiki_pages WHERE product_version_id=$1
                   AND source_refs @> jsonb_build_array($2::text))",
            )
            .bind(vid)
            .bind(document_id.to_string())
            .fetch_one(&pool)
            .await
            .unwrap();
            assert!(
                owned_page,
                "each concurrent document must retain source ownership"
            );
        }
        let original_page_after: (Uuid, String, serde_json::Value) =
            sqlx::query_as("SELECT id,content,source_refs FROM wiki_pages WHERE id=$1")
                .bind(original_page.0)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            original_page, original_page_after,
            "a second document job must not rewrite an unrelated page"
        );

        process_wiki_ingest(&pool, vid, left, knowledge::wiki::OP_RETRACT)
            .await
            .unwrap_or_else(|error| {
                assert!(error.contains("Oxana Redis is not configured"), "{error}");
            });
        let retracted_source_remains: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM wiki_pages WHERE product_version_id=$1
               AND source_refs @> jsonb_build_array($2::text))",
        )
        .bind(vid)
        .bind(left.to_string())
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(!retracted_source_remains);
        let survivor_is_searchable: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM wiki_pages page
                JOIN chunks chunk ON chunk.product_version_id=page.product_version_id
                    AND chunk.chunk_type='wiki_page' AND chunk.context_header=page.slug
                WHERE page.product_version_id=$1
                  AND page.source_refs @> jsonb_build_array($2::text)
                  AND page.content<>'' AND chunk.content<>''
             )",
        )
        .bind(vid)
        .bind(right.to_string())
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(survivor_is_searchable);

        let before_failure: (i64, i64, i64) = sqlx::query_as(
            "SELECT (SELECT count(*) FROM wiki_pages WHERE product_version_id=$1),
                    (SELECT count(*) FROM wiki_folders WHERE product_version_id=$1),
                    (SELECT count(*) FROM chunks WHERE product_version_id=$1 AND chunk_type='wiki_page')",
        )
        .bind(vid)
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query("CREATE FUNCTION kb_test_reject_wiki_write() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'forced wiki write failure'; END $$")
            .execute(&pool).await.unwrap();
        sqlx::query("CREATE TRIGGER kb_test_reject_wiki_write BEFORE INSERT OR UPDATE OR DELETE ON wiki_pages FOR EACH ROW EXECUTE FUNCTION kb_test_reject_wiki_write()")
            .execute(&pool).await.unwrap();
        let mut failure_store = knowledge::Store::default();
        knowledge::hydrate_version(&pool, &mut failure_store, vid)
            .await
            .unwrap();
        let mut changed_page = failure_store
            .wiki
            .values()
            .find(|page| page.product_version_id == vid && page.source_refs.contains(&right))
            .cloned()
            .unwrap();
        changed_page.content.push_str(" forced change");
        let changed_folders = failure_store
            .wiki_folders
            .values()
            .filter(|folder| folder.product_version_id == vid)
            .cloned()
            .collect::<Vec<_>>();
        assert!(
            knowledge::persist_wiki_changes_atomic(
                &pool,
                vid,
                &[changed_page],
                &[],
                &changed_folders,
                &[],
                &[],
                &[],
                &[],
            )
            .await
            .is_err()
        );
        sqlx::query("DROP TRIGGER kb_test_reject_wiki_write ON wiki_pages")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("DROP FUNCTION kb_test_reject_wiki_write()")
            .execute(&pool)
            .await
            .unwrap();
        let after_failure: (i64, i64, i64) = sqlx::query_as(
            "SELECT (SELECT count(*) FROM wiki_pages WHERE product_version_id=$1),
                    (SELECT count(*) FROM wiki_folders WHERE product_version_id=$1),
                    (SELECT count(*) FROM chunks WHERE product_version_id=$1 AND chunk_type='wiki_page')",
        )
        .bind(vid)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            before_failure, after_failure,
            "Wiki publication must roll back atomically"
        );
    }

    #[tokio::test]
    async fn wiki_disabled_skips_without_error() {
        let _g = db_lock().await;
        let Ok(pool) = connect().await else {
            eprintln!("skip: postgres down");
            return;
        };
        reset_test_schema(&pool).await;
        let owner = Uuid::new_v4();
        insert_user(&pool, owner, &format!("{owner}@ex.com"), None)
            .await
            .unwrap();
        let seeded = create_workspace_with_library(&pool, owner, "Wn", "wn")
            .await
            .unwrap();
        sqlx::query(
            "UPDATE product_versions SET indexing_strategy = '{\"wiki\":false}'::jsonb WHERE id = $1",
        )
        .bind(seeded.library_version_id)
        .execute(&pool)
        .await
        .unwrap();
        process_wiki_ingest(
            &pool,
            seeded.library_version_id,
            Uuid::new_v4(),
            knowledge::wiki::OP_INGEST,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn version_clone_worker_copies_doc_and_sets_active() {
        let _g = db_lock().await;
        let Ok(pool) = connect().await else {
            eprintln!("skip: postgres down");
            return;
        };
        reset_test_schema(&pool).await;
        let owner = Uuid::new_v4();
        insert_user(&pool, owner, &format!("{owner}@ex.com"), None)
            .await
            .unwrap();
        let seeded = create_workspace_with_library(&pool, owner, "Wc", "wc")
            .await
            .unwrap();
        let src = seeded.library_version_id;
        let src_doc = Uuid::new_v4();
        insert_document(
            &pool,
            knowledge::NewDocument {
                id: src_doc,
                product_version_id: src,
                title: "iso",
                file_name: "iso.txt",
                file_size: 3,
                file_hash: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
                object_ref: "objects/ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            },
        )
        .await
        .unwrap();
        let dst = Uuid::new_v4();
        knowledge::insert_version_cloning(&pool, dst, seeded.library_id, "2026", src)
            .await
            .unwrap();
        process_version_clone(
            &pool,
            &VersionCloneJob {
                source_version_id: src,
                target_version_id: dst,
                diffs: serde_json::json!([]),
                make_current: false,
                task_type: platform::TYPE_VERSION_CLONE.into(),
            },
        )
        .await
        .unwrap();
        let src_n: i64 =
            sqlx::query_scalar("SELECT count(*) FROM documents WHERE product_version_id = $1")
                .bind(src)
                .fetch_one(&pool)
                .await
                .unwrap();
        let dst_n: i64 =
            sqlx::query_scalar("SELECT count(*) FROM documents WHERE product_version_id = $1")
                .bind(dst)
                .fetch_one(&pool)
                .await
                .unwrap();
        let status: String =
            sqlx::query_scalar("SELECT status FROM product_versions WHERE id = $1")
                .bind(dst)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(src_n, 1);
        assert_eq!(dst_n, 1);
        assert_eq!(status, "active");
        let dst_id: Uuid =
            sqlx::query_scalar("SELECT id FROM documents WHERE product_version_id = $1")
                .bind(dst)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_ne!(dst_id, src_doc);
    }

    #[test]
    fn run_core_registers_postprocess_queue() {
        let src = include_str!("consume.rs");
        assert!(src.contains("PostProcessWorker"));
        assert!(src.contains("PostprocessQueue"));
        assert!(src.contains(".worker::<PostProcessWorker, PostProcessJob>()"));
        assert!(src.contains(".worker::<ImageMultimodalWorker, ImageMultimodalJob>()"));
        assert!(src.contains("IndexDeleteWorker"));
        assert!(src.contains("queue_with_concurrency"));
    }

    #[tokio::test]
    async fn worker_shutdown_state_is_persistent() {
        let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
        stop_tx.send(true).unwrap();

        tokio::time::timeout(
            std::time::Duration::from_millis(100),
            wait_for_worker_shutdown(stop_rx),
        )
        .await
        .expect("a receiver created before shutdown must observe the persisted stop state");
    }

    #[tokio::test]
    async fn process_post_process_clone_keep_requires_typed_wiki_delivery() {
        let _g = db_lock().await;
        let Ok(pool) = connect().await else {
            eprintln!("skip: postgres down");
            return;
        };
        reset_test_schema(&pool).await;
        let owner = Uuid::new_v4();
        insert_user(&pool, owner, &format!("{owner}@ex.com"), None)
            .await
            .unwrap();
        let seeded = create_workspace_with_library(&pool, owner, "Pp", "pp")
            .await
            .unwrap();
        let did = Uuid::new_v4();
        insert_document(
            &pool,
            knowledge::NewDocument {
                id: did,
                product_version_id: seeded.library_version_id,
                title: "keep",
                file_name: "keep.txt",
                file_size: 8,
                file_hash: "930a443e0bc8b34f4fdba1201cf2e2a4d551d226d65270c47ef56e3256e8b3e9",
                object_ref: "objects/930a443e0bc8b34f4fdba1201cf2e2a4d551d226d65270c47ef56e3256e8b3e9",
            },
        )
        .await
        .unwrap();
        sqlx::query(
            "UPDATE documents SET parse_status = 'processing', enable_status = 'enabled'
             WHERE id = $1",
        )
        .bind(did)
        .execute(&pool)
        .await
        .unwrap();
        let cid = Uuid::new_v4();
        knowledge::replace_document_chunks(
            &pool,
            did,
            &[knowledge::Chunk {
                id: cid,
                document_id: did,
                product_version_id: seeded.library_version_id,
                chunk_type: "text".into(),
                content: "throughput keep".into(),
                context_header: String::new(),
                start_at: 0,
                end_at: 15,
                parent_chunk_id: None,
                generated_questions: vec![],
            }],
            &[knowledge::ChunkEmbedding {
                chunk_id: cid,
                product_version_id: seeded.library_version_id,
                document_id: did,
                content: "throughput keep".into(),
                vector: vec![0.1; knowledge::models::EMBEDDING_DIM],
                tsv: String::new(),
            }],
        )
        .await
        .unwrap();
        if let Err(error) = process_post_process(&pool, did, seeded.library_version_id, true).await
        {
            assert!(error.contains("Oxana Redis is not configured"), "{error}");
        }
        let status: String = sqlx::query_scalar("SELECT parse_status FROM documents WHERE id = $1")
            .bind(did)
            .fetch_one(&pool)
            .await
            .unwrap();
        let pending: i32 =
            sqlx::query_scalar("SELECT pending_subtasks_count FROM documents WHERE id = $1")
                .bind(did)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(status, "finalizing");
        assert!(
            pending >= 1,
            "typed Wiki work must remain counted until its Oxana job settles"
        );
    }

    #[tokio::test]
    async fn process_post_process_writes_summary_and_keeps_question_payload_closed() {
        let _g = db_lock().await;
        let Ok(pool) = connect().await else {
            eprintln!("skip: postgres down");
            return;
        };
        reset_test_schema(&pool).await;
        let owner = Uuid::new_v4();
        insert_user(&pool, owner, &format!("{owner}@ex.com"), None)
            .await
            .unwrap();
        let seeded = create_workspace_with_library(&pool, owner, "Sm", "sm")
            .await
            .unwrap();
        sqlx::query("UPDATE product_versions SET summary_model_id = 'stub-chat' WHERE id = $1")
            .bind(seeded.library_version_id)
            .execute(&pool)
            .await
            .unwrap();
        let did = Uuid::new_v4();
        insert_document(
            &pool,
            knowledge::NewDocument {
                id: did,
                product_version_id: seeded.library_version_id,
                title: "spec",
                file_name: "spec.txt",
                file_size: 80,
                file_hash: "bb558b4638d76b2461f5cdeca98bc8b4ba29b652cfa1ca7662c82d15fd171063",
                object_ref: "objects/bb558b4638d76b2461f5cdeca98bc8b4ba29b652cfa1ca7662c82d15fd171063",
            },
        )
        .await
        .unwrap();
        sqlx::query(
            "UPDATE documents SET parse_status = 'processing', enable_status = 'enabled'
             WHERE id = $1",
        )
        .bind(did)
        .execute(&pool)
        .await
        .unwrap();
        let body = "The product delivers forty gigabit throughput on the line card. \
                    Operators use this guide to install the switch in a rack and verify ISO9001.";
        let cid = Uuid::new_v4();
        knowledge::replace_document_chunks(
            &pool,
            did,
            &[knowledge::Chunk {
                id: cid,
                document_id: did,
                product_version_id: seeded.library_version_id,
                chunk_type: "text".into(),
                content: body.into(),
                context_header: String::new(),
                start_at: 0,
                end_at: body.len() as i32,
                parent_chunk_id: None,
                generated_questions: vec![],
            }],
            &[knowledge::ChunkEmbedding {
                chunk_id: cid,
                product_version_id: seeded.library_version_id,
                document_id: did,
                content: body.into(),
                vector: vec![0.1; knowledge::models::EMBEDDING_DIM],
                tsv: String::new(),
            }],
        )
        .await
        .unwrap();
        let image =
            process_image_pg(&pool, did, "images/p1.jpg", "scanned_pdf", true, true, 1).await;
        if platform::vlm_configured() {
            image.unwrap();
            let text_n: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM chunks WHERE document_id = $1 AND chunk_type = 'text'",
            )
            .bind(did)
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(text_n, 1, "multimodal append must keep text chunks");
            let ocr_n: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM chunks WHERE document_id = $1 AND chunk_type = 'image_ocr'",
            )
            .bind(did)
            .fetch_one(&pool)
            .await
            .unwrap();
            assert!(ocr_n >= 1, "multimodal OCR chunk persisted");
        } else {
            assert!(image.is_err(), "no VLM must not stub OCR chunks");
        }

        process_post_process(&pool, did, seeded.library_version_id, false)
            .await
            .unwrap();
        let _ = process_summary_pg(&pool, did, 1, false).await;
        let _ = process_questions_pg(&pool, did, &[cid], &[], &[], 1).await;
        let summaries: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM chunks WHERE document_id = $1 AND chunk_type = 'summary'",
        )
        .bind(did)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(summaries >= 1, "summary chunk persisted");
        let qs: serde_json::Value =
            sqlx::query_scalar("SELECT generated_questions FROM chunks WHERE id = $1")
                .bind(cid)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(
            qs.as_array().is_some(),
            "generated_questions must remain a closed array: {qs}"
        );
    }

    #[tokio::test]
    async fn semantic_index_v2_business_lifecycle_is_fenced() {
        use knowledge::knowledge_index_v2::SemanticIndexPreparationV2;
        use knowledge::knowledge_retrieval::{
            EMBEDDING_DIMENSION_V2, EMBEDDING_OUTPUT_NORMALIZATION_VERSION_V2,
            EMBEDDING_PROVIDER_PROTOCOL_VERSION_V2, EMBEDDING_REVISION_SCHEMA_V2,
            EmbeddingRevisionV2,
        };

        let _g = db_lock().await;
        let pool = match connect().await {
            Ok(pool) => pool,
            Err(error)
                if std::env::var("KNOWLEDGEBRAIN_REQUIRE_POSTGRES_TESTS").as_deref() == Ok("1") =>
            {
                panic!("required semantic-index V2 PostgreSQL test unavailable: {error}")
            }
            Err(error) => {
                eprintln!("skip: postgres down: {error}");
                return;
            }
        };
        reset_test_schema(&pool).await;
        let schema_ready: bool = sqlx::query_scalar(
            "SELECT to_regprocedure('kb_knowledge_prepare_semantic_index_intent_v2(uuid)') IS NOT NULL",
        )
        .fetch_one(&pool)
        .await
        .unwrap_or(false);
        if !schema_ready {
            if std::env::var("KNOWLEDGEBRAIN_REQUIRE_POSTGRES_TESTS").as_deref() == Ok("1") {
                panic!("required semantic-index V2 schema is unavailable");
            }
            eprintln!("skip: semantic-index V2 schema unavailable");
            return;
        }

        let workspace_id = Uuid::new_v4();
        let product_id = Uuid::new_v4();
        let version_id = Uuid::new_v4();
        let document_id = Uuid::new_v4();
        let chunk_id = Uuid::new_v4();
        let file_hash = platform::sha256_hex(document_id.as_bytes());
        let object_ref = format!("objects/{file_hash}");
        let revision = EmbeddingRevisionV2 {
            schema_version: EMBEDDING_REVISION_SCHEMA_V2,
            provider_protocol_version: EMBEDDING_PROVIDER_PROTOCOL_VERSION_V2.into(),
            provider_model_identifier: format!("lifecycle-v2-{version_id}@2025-01-15"),
            provider_model_revision_sha256: platform::sha256_hex(b"lifecycle-v2-model"),
            endpoint_config_sha256: platform::sha256_hex(b"lifecycle-v2-endpoint"),
            endpoint_identity: "https://embeddings.example.test/v1/embeddings".into(),
            dimension: EMBEDDING_DIMENSION_V2,
            request_config_sha256: EmbeddingRevisionV2::canonical_request_config_sha256(),
            output_normalization_version: EMBEDDING_OUTPUT_NORMALIZATION_VERSION_V2.into(),
        };
        let revision_sha256 = revision.sha256().unwrap();

        sqlx::query("INSERT INTO workspaces(id,name,slug,kind) VALUES($1,'semantic lifecycle',$2,'product_line')")
            .bind(workspace_id)
            .bind(format!("semantic-lifecycle-{workspace_id}"))
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO products(id,workspace_id,kind,name,slug) VALUES($1,$2,'product','semantic lifecycle',$3)")
            .bind(product_id).bind(workspace_id)
            .bind(format!("semantic-lifecycle-{product_id}"))
            .execute(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO product_versions(id,product_id,label,status) VALUES($1,$2,'v2','active')",
        )
        .bind(version_id)
        .bind(product_id)
        .execute(&pool)
        .await
        .unwrap();

        let unbound =
            knowledge::knowledge_index_v2::prepare_semantic_index_intent_v2(&pool, version_id)
                .await
                .unwrap();
        assert_eq!(unbound, SemanticIndexPreparationV2::Unbound);

        sqlx::query("INSERT INTO embedding_revisions_v2(revision_sha256,canonical_revision_payload,schema_version,provider_protocol_version,provider_model_identifier,provider_model_revision_sha256,endpoint_config_sha256,endpoint_identity,dimension,request_config_sha256,output_normalization_version,credential_ref) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,'env:KNOWLEDGEBRAIN_TEST_MISSING_SEMANTIC_V2')")
            .bind(&revision_sha256)
            .bind(revision.canonical_bytes().unwrap())
            .bind(i16::try_from(revision.schema_version).unwrap())
            .bind(&revision.provider_protocol_version)
            .bind(&revision.provider_model_identifier)
            .bind(&revision.provider_model_revision_sha256)
            .bind(&revision.endpoint_config_sha256)
            .bind(&revision.endpoint_identity)
            .bind(i32::try_from(revision.dimension).unwrap())
            .bind(&revision.request_config_sha256)
            .bind(&revision.output_normalization_version)
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO product_version_embedding_bindings_v2(product_version_id,embedding_revision_sha256) VALUES($1,$2)")
            .bind(version_id).bind(&revision_sha256).execute(&pool).await.unwrap();

        let empty_intent = match knowledge::knowledge_index_v2::prepare_semantic_index_intent_v2(
            &pool, version_id,
        )
        .await
        .unwrap()
        {
            SemanticIndexPreparationV2::Enqueue(intent) => intent,
            other => panic!("expected empty bound intent, got {other:?}"),
        };
        let empty_provider = LifecycleProvider {
            calls: AtomicUsize::new(0),
            results: std::sync::Mutex::new(VecDeque::new()),
        };
        process_semantic_index_intent_v2(
            &pool,
            empty_intent.id,
            empty_intent.target_revision,
            Some(&empty_provider),
            None,
        )
        .await
        .unwrap();
        assert_eq!(empty_provider.calls.load(Ordering::SeqCst), 0);
        let empty_completed = knowledge::knowledge_index_v2::semantic_index_intent_v2(
            &pool,
            empty_intent.id,
            empty_intent.target_revision,
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(empty_completed.status, "completed");
        for statement in [
            "UPDATE knowledge_semantic_index_intents_v2 SET source_snapshot_sha256=repeat('0',64) WHERE id=$1",
            "DELETE FROM knowledge_semantic_index_intents_v2 WHERE id=$1",
        ] {
            let immutable_error = sqlx::query(statement)
                .bind(empty_intent.id)
                .execute(&pool)
                .await
                .unwrap_err();
            assert!(
                immutable_error
                    .as_database_error()
                    .is_some_and(|error| error
                        .message()
                        .contains("KNOWLEDGE_SEMANTIC_INDEX_INTENT_V2_IMMUTABLE")),
                "intent identity/history must be immutable: {immutable_error}"
            );
        }

        sqlx::query("INSERT INTO object_registry(object_ref,digest,media_type,byte_length,state) VALUES($1,$2,'text/plain',0,'available')")
            .bind(&object_ref).bind(&file_hash).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO object_owner_references(object_ref,owner_kind,owner_id,occurrence,created_by) VALUES($1,'knowledge_document',$2,'original','system:knowledge-document-ingest')")
            .bind(&object_ref).bind(document_id).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO documents(id,product_version_id,title,parse_status,pending_subtasks_count,summary_status,enable_status,index_ready,file_name,file_size,file_hash,object_ref) VALUES($1,$2,'lifecycle','finalizing',1,'pending','enabled',true,$3,0,$4,$5)")
            .bind(document_id).bind(version_id).bind(format!("{document_id}.txt"))
            .bind(&file_hash).bind(&object_ref).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO chunks(id,product_version_id,document_id,chunk_type,content,context_header) VALUES($1,$2,$3,'text','settled source','# lifecycle')")
            .bind(chunk_id).bind(version_id).bind(document_id).execute(&pool).await.unwrap();

        assert_eq!(
            knowledge::knowledge_index_v2::prepare_semantic_index_intent_v2(&pool, version_id)
                .await
                .unwrap(),
            SemanticIndexPreparationV2::PendingDerived
        );
        sqlx::query("UPDATE documents SET parse_status='completed',pending_subtasks_count=0,summary_status='completed' WHERE id=$1")
            .bind(document_id).execute(&pool).await.unwrap();
        let source_intent = match knowledge::knowledge_index_v2::prepare_semantic_index_intent_v2(
            &pool, version_id,
        )
        .await
        .unwrap()
        {
            SemanticIndexPreparationV2::Enqueue(intent) => intent,
            other => panic!("expected settled source intent, got {other:?}"),
        };
        let scheduled_targets = Arc::new(std::sync::Mutex::new(Vec::new()));
        let first_calls = scheduled_targets.clone();
        assert!(
            schedule_semantic_index_v2_if_ready_with(&pool, version_id, move |id, revision| {
                let first_calls = first_calls.clone();
                async move {
                    first_calls.lock().unwrap().push((id, revision));
                    Ok(None)
                }
            })
            .await
            .is_err(),
            "queue unavailability must remain an Oxana-retryable parent error"
        );
        let second_calls = scheduled_targets.clone();
        schedule_semantic_index_v2_if_ready_with(&pool, version_id, move |id, revision| {
            let second_calls = second_calls.clone();
            async move {
                second_calls.lock().unwrap().push((id, revision));
                Ok(Some("accepted".into()))
            }
        })
        .await
        .unwrap();
        assert_eq!(
            scheduled_targets.lock().unwrap().as_slice(),
            &[
                (source_intent.id, source_intent.target_revision),
                (source_intent.id, source_intent.target_revision),
            ],
            "parent retry must replay the same unique business target"
        );
        assert_eq!(
            knowledge::document_parse_status(&pool, document_id)
                .await
                .unwrap()
                .as_deref(),
            Some("completed"),
            "V2 enqueue failure must not rewrite completed V1 status"
        );

        let unavailable_provider = LifecycleProvider {
            calls: AtomicUsize::new(0),
            results: std::sync::Mutex::new(VecDeque::from([
                LifecycleProviderResult::Unavailable,
                LifecycleProviderResult::Unavailable,
                LifecycleProviderResult::Unavailable,
                LifecycleProviderResult::Unavailable,
            ])),
        };
        for _ in 0..=platform::SEMANTIC_INDEX_V2_MAX_RETRY {
            assert!(
                process_semantic_index_intent_v2(
                    &pool,
                    source_intent.id,
                    source_intent.target_revision,
                    Some(&unavailable_provider),
                    None,
                )
                .await
                .is_err()
            );
        }
        let retryable = knowledge::knowledge_index_v2::semantic_index_intent_v2(
            &pool,
            source_intent.id,
            source_intent.target_revision,
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(retryable.status, "pending");
        assert_eq!(
            retryable.last_error_code.as_deref(),
            Some("PROVIDER_UNAVAILABLE")
        );

        assert_eq!(unavailable_provider.calls.load(Ordering::SeqCst), 4);
        // A fresh provider instance models native dead-job revival: the
        // business target remained pending and the same envelope can run again.
        let restarted_provider = LifecycleProvider {
            calls: AtomicUsize::new(0),
            results: std::sync::Mutex::new(VecDeque::from([LifecycleProviderResult::Success])),
        };
        process_semantic_index_intent_v2(
            &pool,
            source_intent.id,
            source_intent.target_revision,
            Some(&restarted_provider),
            None,
        )
        .await
        .unwrap();
        assert_eq!(restarted_provider.calls.load(Ordering::SeqCst), 1);
        let ready = knowledge::knowledge_index_v2::semantic_index_intent_v2(
            &pool,
            source_intent.id,
            source_intent.target_revision,
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(ready.status, "completed");
        assert_eq!(
            ready.generation_marker_sha256.as_deref(),
            Some(source_intent.source_snapshot_sha256.as_str())
        );
        let complete_generation: bool = sqlx::query_scalar(
            "SELECT EXISTS(
               SELECT 1
                 FROM product_version_keyword_index_generations_v2 keyword_generation
                 JOIN product_version_vector_index_generations_v2 vector_generation
                   ON vector_generation.product_version_id=keyword_generation.product_version_id
                  AND vector_generation.embedding_revision_sha256=keyword_generation.embedding_revision_sha256
                  AND vector_generation.source_snapshot_sha256=keyword_generation.source_snapshot_sha256
                 JOIN knowledge_semantic_index_intents_v2 intent
                   ON intent.product_version_id=keyword_generation.product_version_id
                  AND intent.embedding_revision_sha256=keyword_generation.embedding_revision_sha256
                  AND intent.source_snapshot_sha256=keyword_generation.source_snapshot_sha256
                  AND intent.status='completed'
                  AND intent.generation_marker_sha256=intent.source_snapshot_sha256
                WHERE keyword_generation.product_version_id=$1
                  AND keyword_generation.source_snapshot_sha256=$2)",
        )
        .bind(version_id)
        .bind(&source_intent.source_snapshot_sha256)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(complete_generation);
        process_semantic_index_intent_v2(
            &pool,
            source_intent.id,
            source_intent.target_revision,
            Some(&restarted_provider),
            None,
        )
        .await
        .unwrap();
        assert_eq!(
            restarted_provider.calls.load(Ordering::SeqCst),
            1,
            "duplicate delivery must noop"
        );
        let v1_ready: bool = sqlx::query_scalar("SELECT index_ready FROM documents WHERE id=$1")
            .bind(document_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(v1_ready, "V1 document readiness must remain unchanged");

        sqlx::query("UPDATE chunks SET content='aba generation b' WHERE id=$1")
            .bind(chunk_id)
            .execute(&pool)
            .await
            .unwrap();
        let aba_b = match knowledge::knowledge_index_v2::prepare_semantic_index_intent_v2(
            &pool, version_id,
        )
        .await
        .unwrap()
        {
            SemanticIndexPreparationV2::Enqueue(intent) => intent,
            other => panic!("expected ABA generation B target, got {other:?}"),
        };
        let aba_b_provider = LifecycleProvider {
            calls: AtomicUsize::new(0),
            results: std::sync::Mutex::new(VecDeque::new()),
        };
        process_semantic_index_intent_v2(
            &pool,
            aba_b.id,
            aba_b.target_revision,
            Some(&aba_b_provider),
            None,
        )
        .await
        .unwrap();
        assert_eq!(aba_b_provider.calls.load(Ordering::SeqCst), 1);

        sqlx::query("UPDATE chunks SET content='settled source' WHERE id=$1")
            .bind(chunk_id)
            .execute(&pool)
            .await
            .unwrap();
        let aba_a = match knowledge::knowledge_index_v2::prepare_semantic_index_intent_v2(
            &pool, version_id,
        )
        .await
        .unwrap()
        {
            SemanticIndexPreparationV2::Enqueue(intent) => intent,
            other => panic!("expected a new ABA generation A target, got {other:?}"),
        };
        assert_eq!(
            aba_a.source_snapshot_sha256,
            source_intent.source_snapshot_sha256
        );
        assert!(aba_a.target_revision > aba_b.target_revision);
        assert_ne!(aba_a.id, source_intent.id);

        let pending_after_provider = PendingAfterLifecycleProvider {
            pool: pool.clone(),
            document_id,
            calls: AtomicUsize::new(0),
        };
        assert!(
            process_semantic_index_intent_v2(
                &pool,
                aba_a.id,
                aba_a.target_revision,
                Some(&pending_after_provider),
                None,
            )
            .await
            .is_err(),
            "pending derived work introduced after provider I/O must fence publication"
        );
        assert_eq!(pending_after_provider.calls.load(Ordering::SeqCst), 1);
        let aba_pending = knowledge::knowledge_index_v2::semantic_index_intent_v2(
            &pool,
            aba_a.id,
            aba_a.target_revision,
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(aba_pending.status, "pending");
        sqlx::query("UPDATE documents SET pending_subtasks_count=0 WHERE id=$1")
            .bind(document_id)
            .execute(&pool)
            .await
            .unwrap();
        let aba_retry_provider = LifecycleProvider {
            calls: AtomicUsize::new(0),
            results: std::sync::Mutex::new(VecDeque::new()),
        };
        process_semantic_index_intent_v2(
            &pool,
            aba_a.id,
            aba_a.target_revision,
            Some(&aba_retry_provider),
            None,
        )
        .await
        .unwrap();
        assert_eq!(
            aba_retry_provider.calls.load(Ordering::SeqCst),
            0,
            "the already fenced vector generation is reused without duplicate provider I/O"
        );

        let prior_marker: String = sqlx::query_scalar("SELECT source_snapshot_sha256 FROM product_version_vector_index_generations_v2 WHERE product_version_id=$1")
            .bind(version_id).fetch_one(&pool).await.unwrap();
        sqlx::query("UPDATE chunks SET content='stale source generation' WHERE id=$1")
            .bind(chunk_id)
            .execute(&pool)
            .await
            .unwrap();
        let stale_intent = match knowledge::knowledge_index_v2::prepare_semantic_index_intent_v2(
            &pool, version_id,
        )
        .await
        .unwrap()
        {
            SemanticIndexPreparationV2::Enqueue(intent) => intent,
            other => panic!("expected stale source intent, got {other:?}"),
        };
        sqlx::query("UPDATE chunks SET content='new immutable source generation' WHERE id=$1")
            .bind(chunk_id)
            .execute(&pool)
            .await
            .unwrap();
        let terminal_intent = match knowledge::knowledge_index_v2::prepare_semantic_index_intent_v2(
            &pool, version_id,
        )
        .await
        .unwrap()
        {
            SemanticIndexPreparationV2::Enqueue(intent) => intent,
            other => panic!("expected newer source intent, got {other:?}"),
        };
        let stale_provider = LifecycleProvider {
            calls: AtomicUsize::new(0),
            results: std::sync::Mutex::new(VecDeque::new()),
        };
        let stale_successor = process_semantic_index_intent_v2(
            &pool,
            stale_intent.id,
            stale_intent.target_revision,
            Some(&stale_provider),
            None,
        )
        .await
        .unwrap();
        assert_eq!(
            stale_provider.calls.load(Ordering::SeqCst),
            0,
            "superseded delivery must not reach the provider"
        );
        let stale = knowledge::knowledge_index_v2::semantic_index_intent_v2(
            &pool,
            stale_intent.id,
            stale_intent.target_revision,
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(stale.status, "superseded");
        assert_eq!(
            stale_successor.as_ref().map(|successor| successor.id),
            Some(terminal_intent.id),
            "stale delivery must replay exactly the current successor target"
        );
        let credential_calls = Arc::new(AtomicUsize::new(0));
        let strict = knowledge::knowledge_index_v2::StrictVectorEmbeddingClientV2::new(Arc::new(
            MissingLifecycleCredential {
                calls: credential_calls.clone(),
            },
        ))
        .unwrap();
        process_semantic_index_intent_v2(
            &pool,
            terminal_intent.id,
            terminal_intent.target_revision,
            Some(&strict),
            None,
        )
        .await
        .unwrap();
        process_semantic_index_intent_v2(
            &pool,
            terminal_intent.id,
            terminal_intent.target_revision,
            Some(&strict),
            None,
        )
        .await
        .unwrap();
        assert_eq!(
            credential_calls.load(Ordering::SeqCst),
            1,
            "terminal intent must never be reserved twice"
        );
        let terminal = knowledge::knowledge_index_v2::semantic_index_intent_v2(
            &pool,
            terminal_intent.id,
            terminal_intent.target_revision,
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(terminal.status, "terminal");
        assert_eq!(
            terminal.last_error_code.as_deref(),
            Some("INVALID_IMMUTABLE_CONFIGURATION")
        );
        let retained_marker: String = sqlx::query_scalar("SELECT source_snapshot_sha256 FROM product_version_vector_index_generations_v2 WHERE product_version_id=$1")
            .bind(version_id).fetch_one(&pool).await.unwrap();
        assert_eq!(
            retained_marker, prior_marker,
            "failed publication must preserve the prior complete generation"
        );
        let current_snapshot: String = sqlx::query_scalar(
            "SELECT source_snapshot_sha256 FROM kb_knowledge_source_snapshot_v2($1,$2)",
        )
        .bind(version_id)
        .bind(&revision_sha256)
        .fetch_one(&pool)
        .await
        .unwrap();
        let stale_ready: bool = sqlx::query_scalar(
            "SELECT EXISTS(
               SELECT 1 FROM knowledge_semantic_index_intents_v2
                WHERE product_version_id=$1 AND embedding_revision_sha256=$2
                  AND source_snapshot_sha256=$3 AND status='completed'
                  AND generation_marker_sha256=$3)",
        )
        .bind(version_id)
        .bind(&revision_sha256)
        .bind(&current_snapshot)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(
            !stale_ready,
            "a failed new source generation must never be retrieval-ready"
        );

        sqlx::query("UPDATE chunks SET content='revision fence generation' WHERE id=$1")
            .bind(chunk_id)
            .execute(&pool)
            .await
            .unwrap();
        let revoked_intent = match knowledge::knowledge_index_v2::prepare_semantic_index_intent_v2(
            &pool, version_id,
        )
        .await
        .unwrap()
        {
            SemanticIndexPreparationV2::Enqueue(intent) => intent,
            other => panic!("expected pre-revocation intent, got {other:?}"),
        };
        sqlx::query("UPDATE embedding_revisions_v2 SET support_state='revoked',updated_at=clock_timestamp() WHERE revision_sha256=$1")
            .bind(&revision_sha256).execute(&pool).await.unwrap();
        let revoked_provider = LifecycleProvider {
            calls: AtomicUsize::new(0),
            results: std::sync::Mutex::new(VecDeque::new()),
        };
        process_semantic_index_intent_v2(
            &pool,
            revoked_intent.id,
            revoked_intent.target_revision,
            Some(&revoked_provider),
            None,
        )
        .await
        .unwrap();
        assert_eq!(
            revoked_provider.calls.load(Ordering::SeqCst),
            0,
            "revoked revision must fence before provider I/O"
        );
        let revoked = knowledge::knowledge_index_v2::semantic_index_intent_v2(
            &pool,
            revoked_intent.id,
            revoked_intent.target_revision,
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(revoked.status, "terminal");
        assert_eq!(
            knowledge::knowledge_index_v2::prepare_semantic_index_intent_v2(&pool, version_id)
                .await
                .unwrap(),
            SemanticIndexPreparationV2::Terminal(revoked.clone()),
            "terminal immutable generation must not be re-enqueued"
        );

        let mut tx = pool.begin().await.unwrap();
        sqlx::query("ALTER TABLE public.embedding_revisions_v2 DISABLE TRIGGER USER")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("DELETE FROM chunks WHERE product_version_id=$1")
            .bind(version_id)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("DELETE FROM object_owner_references WHERE object_ref=$1")
            .bind(&object_ref)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("DELETE FROM documents WHERE id=$1")
            .bind(document_id)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("DELETE FROM object_registry WHERE object_ref=$1")
            .bind(&object_ref)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("DELETE FROM product_versions WHERE id=$1")
            .bind(version_id)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("DELETE FROM embedding_revisions_v2 WHERE revision_sha256=$1")
            .bind(&revision_sha256)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("ALTER TABLE public.embedding_revisions_v2 ENABLE TRIGGER USER")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("DELETE FROM products WHERE id=$1")
            .bind(product_id)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("DELETE FROM workspaces WHERE id=$1")
            .bind(workspace_id)
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
    }
}
