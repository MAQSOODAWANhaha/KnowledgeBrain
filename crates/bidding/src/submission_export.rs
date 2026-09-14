//! Submission export job body: preflight, PDF attachment prep, layout, and publish.
//! Worker adapters call [`execute`] with helper-backed [`ExportIo`] / [`RenderIo`].

#[derive(Debug)]
pub struct ExportError(pub String);

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ExportError {}

pub const MAX_FROZEN_ASSET_COUNT: usize = 2_048;
pub const MAX_FROZEN_ASSET_TOTAL_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_FROZEN_ASSET_TOTAL_PIXELS: u64 = 300_000_000;
pub const MAX_PDF_ATTACHMENT_PAGES: usize = 1_000;
pub const MAX_EXPORT_BLOCK_OCCURRENCES: usize = 100_000;
pub const MAX_EXPORT_TABLE_ROWS: u64 = 100_000;
pub const MAX_EXPORT_TABLE_CELLS: u64 = 100_000;
pub const MAX_EXPORT_FORM_DEFINITIONS: usize = 10_000;
pub const MAX_EXPORT_ATTACHMENT_PREPARATIONS: usize = 2_048;
pub const MAX_EXPORT_REFERENCE_WORK_BYTES: u64 = 512 * 1024 * 1024;

pub fn validate_frozen_asset_metadata(values: &[serde_json::Value]) -> Result<(), ExportError> {
    if values.len() > MAX_FROZEN_ASSET_COUNT {
        return Err(ExportError(
            "frozen render asset count exceeds budget".into(),
        ));
    }
    let mut identities = std::collections::HashSet::with_capacity(values.len());
    let mut total_bytes = 0u64;
    let mut total_pixels = 0u64;
    let mut total_pdf_pages = 0u64;
    for value in values {
        let identity = value
            .get("asset_revision_id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| ExportError("frozen asset identity metadata missing".into()))?;
        if !identities.insert(identity) {
            return Err(ExportError("duplicate frozen asset identity".into()));
        }
        let media_type = value
            .get("media_type")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| ExportError("frozen asset media type metadata missing".into()))?;
        let byte_length = value
            .get("byte_length")
            .and_then(serde_json::Value::as_u64)
            .filter(|length| *length > 0)
            .ok_or_else(|| ExportError("frozen asset byte length metadata missing".into()))?;
        total_bytes = total_bytes
            .checked_add(byte_length)
            .ok_or_else(|| ExportError("frozen asset byte budget overflow".into()))?;
        if total_bytes > MAX_FROZEN_ASSET_TOTAL_BYTES {
            return Err(ExportError(
                "frozen render assets exceed aggregate byte budget".into(),
            ));
        }
        let width = value.get("width_px").and_then(serde_json::Value::as_u64);
        let height = value.get("height_px").and_then(serde_json::Value::as_u64);
        let page_count = value.get("page_count").and_then(serde_json::Value::as_u64);
        if media_type.starts_with("image/") {
            let (Some(width), Some(height), None) = (width, height, page_count) else {
                return Err(ExportError(
                    "frozen image dimension metadata incomplete".into(),
                ));
            };
            let pixels = width
                .checked_mul(height)
                .ok_or_else(|| ExportError("frozen asset pixel metadata overflow".into()))?;
            total_pixels = total_pixels
                .checked_add(pixels)
                .ok_or_else(|| ExportError("frozen asset pixel budget overflow".into()))?;
        } else if media_type == "application/pdf" {
            let (None, None, Some(pages)) = (width, height, page_count) else {
                return Err(ExportError("frozen PDF page metadata incomplete".into()));
            };
            if pages == 0 || pages > MAX_PDF_ATTACHMENT_PAGES as u64 {
                return Err(ExportError("frozen PDF page count exceeds budget".into()));
            }
            total_pdf_pages = total_pdf_pages
                .checked_add(pages)
                .ok_or_else(|| ExportError("frozen PDF page count overflow".into()))?;
        } else if width.is_some() || height.is_some() || page_count.is_some() {
            return Err(ExportError(
                "non-visual frozen asset has visual metadata".into(),
            ));
        }
        if total_pixels > MAX_FROZEN_ASSET_TOTAL_PIXELS
            || total_pdf_pages > MAX_PDF_ATTACHMENT_PAGES as u64
        {
            return Err(ExportError(
                "frozen render assets exceed aggregate page or pixel budget".into(),
            ));
        }
    }
    Ok(())
}

pub fn validate_submission_export_metadata(input: &serde_json::Value) -> Result<(), ExportError> {
    let assets = input
        .get("assets")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| ExportError("frozen export assets missing".into()))?;
    validate_frozen_asset_metadata(assets)?;
    let mut asset_metadata = std::collections::HashMap::with_capacity(assets.len());
    for asset in assets {
        let id = asset
            .get("asset_revision_id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| ExportError("frozen asset identity metadata missing".into()))?;
        let bytes = asset
            .get("byte_length")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| ExportError("frozen asset byte length metadata missing".into()))?;
        let pixels = match (
            asset.get("width_px").and_then(serde_json::Value::as_u64),
            asset.get("height_px").and_then(serde_json::Value::as_u64),
        ) {
            (Some(width), Some(height)) => width
                .checked_mul(height)
                .ok_or_else(|| ExportError("frozen asset occurrence pixels overflow".into()))?,
            _ => 0,
        };
        asset_metadata.insert(id, (bytes, pixels));
    }

    let blocks = input
        .get("workspace")
        .and_then(|workspace| workspace.get("blocks"))
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| ExportError("frozen export blocks missing".into()))?;
    if blocks.len() > MAX_EXPORT_BLOCK_OCCURRENCES {
        return Err(ExportError(
            "frozen export block count exceeds budget".into(),
        ));
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
            let (bytes, pixels) = asset_metadata.get(asset_id).ok_or_else(|| {
                ExportError("frozen block references missing asset metadata".into())
            })?;
            repeated_bytes = repeated_bytes.checked_add(*bytes).ok_or_else(|| {
                ExportError("frozen asset reference byte estimate overflow".into())
            })?;
            occurrence_pixels = occurrence_pixels.checked_add(*pixels).ok_or_else(|| {
                ExportError("frozen asset occurrence pixel estimate overflow".into())
            })?;
        }
        if content.get("type").and_then(serde_json::Value::as_str) == Some("table") {
            let rows = content
                .get("row_count")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| ExportError("frozen table row metadata missing".into()))?;
            let columns = content
                .get("column_count")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| ExportError("frozen table column metadata missing".into()))?;
            let declared_cells = rows
                .checked_mul(columns)
                .ok_or_else(|| ExportError("frozen table cell count overflow".into()))?;
            let cells = content
                .get("cells")
                .and_then(serde_json::Value::as_array)
                .ok_or_else(|| ExportError("frozen table cells missing".into()))?;
            if u64::try_from(cells.len())
                .map_err(|_| ExportError("table cell count overflow".into()))?
                > declared_cells
            {
                return Err(ExportError(
                    "frozen table cells exceed declared grid".into(),
                ));
            }
            for cell in cells {
                let row = cell
                    .get("row")
                    .and_then(serde_json::Value::as_u64)
                    .ok_or_else(|| ExportError("table row missing".into()))?;
                let column = cell
                    .get("column")
                    .and_then(serde_json::Value::as_u64)
                    .ok_or_else(|| ExportError("table column missing".into()))?;
                let rowspan = cell
                    .get("rowspan")
                    .and_then(serde_json::Value::as_u64)
                    .ok_or_else(|| ExportError("table rowspan missing".into()))?;
                let colspan = cell
                    .get("colspan")
                    .and_then(serde_json::Value::as_u64)
                    .ok_or_else(|| ExportError("table colspan missing".into()))?;
                if rowspan == 0
                    || colspan == 0
                    || row.checked_add(rowspan).is_none_or(|end| end > rows)
                    || column.checked_add(colspan).is_none_or(|end| end > columns)
                    || rowspan.checked_mul(colspan).is_none()
                {
                    return Err(ExportError("frozen table span metadata invalid".into()));
                }
            }
            table_rows = table_rows
                .checked_add(rows)
                .ok_or_else(|| ExportError("aggregate table row count overflow".into()))?;
            table_cells = table_cells
                .checked_add(declared_cells)
                .ok_or_else(|| ExportError("aggregate table cell count overflow".into()))?;
        }
    }
    let forms = input
        .get("form_definitions")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| ExportError("frozen form definitions missing".into()))?;
    let preparations = input
        .get("attachment_preparations")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| ExportError("frozen attachment preparations missing".into()))?;
    if forms.len() > MAX_EXPORT_FORM_DEFINITIONS
        || preparations.len() > MAX_EXPORT_ATTACHMENT_PREPARATIONS
        || table_rows > MAX_EXPORT_TABLE_ROWS
        || table_cells > MAX_EXPORT_TABLE_CELLS
        || occurrence_pixels > MAX_FROZEN_ASSET_TOTAL_PIXELS
    {
        return Err(ExportError(
            "frozen export aggregate work exceeds budget".into(),
        ));
    }
    let mut prepared_pages = 0u64;
    let mut prepared_pixels = 0u64;
    for preparation in preparations {
        let pages = preparation
            .get("page_assets")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| ExportError("attachment preparation page metadata missing".into()))?;
        prepared_pages = prepared_pages
            .checked_add(
                u64::try_from(pages.len())
                    .map_err(|_| ExportError("prepared page count overflow".into()))?,
            )
            .ok_or_else(|| ExportError("prepared page count overflow".into()))?;
        for page in pages {
            let geometry = page
                .get("geometry")
                .ok_or_else(|| ExportError("prepared page geometry missing".into()))?;
            let width = geometry
                .get("width_px")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| ExportError("prepared page width missing".into()))?;
            let height = geometry
                .get("height_px")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| ExportError("prepared page height missing".into()))?;
            prepared_pixels = prepared_pixels
                .checked_add(
                    width
                        .checked_mul(height)
                        .ok_or_else(|| ExportError("prepared page pixels overflow".into()))?,
                )
                .ok_or_else(|| ExportError("prepared page pixels overflow".into()))?;
        }
    }
    let estimated_work = repeated_bytes
        .checked_add(
            prepared_pages
                .checked_mul(256 * 1024)
                .ok_or_else(|| ExportError("prepared page output estimate overflow".into()))?,
        )
        .and_then(|value| value.checked_add(table_cells.checked_mul(256)?))
        .ok_or_else(|| ExportError("final export work estimate overflow".into()))?;
    if prepared_pages > MAX_PDF_ATTACHMENT_PAGES as u64
        || prepared_pixels > MAX_FROZEN_ASSET_TOTAL_PIXELS
        || estimated_work > MAX_EXPORT_REFERENCE_WORK_BYTES
    {
        return Err(ExportError(
            "frozen export aggregate work exceeds budget".into(),
        ));
    }
    Ok(())
}

use async_trait::async_trait;
use platform::SubmissionExportJobV2;
use sqlx::PgPool;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub struct ExportLimits {
    pub max_input_bytes: usize,
    pub max_render_output_bytes: usize,
    pub max_pdf_attachment_bytes: usize,
    pub max_raster_total_bytes: usize,
}

#[async_trait]
pub trait ExportIo: Send + Sync {
    async fn read_blob(
        &self,
        sha256: &str,
        max_bytes: usize,
        cancel: &CancellationToken,
    ) -> Result<Vec<u8>, ExportError>;
    async fn stage_object(
        &self,
        pool: &PgPool,
        staging_id: Uuid,
        digest: &str,
        media_type: &str,
        bytes: &[u8],
        actor: &str,
    ) -> Result<String, ExportError>;
}

#[async_trait]
pub trait RenderIo: Send + Sync {
    async fn render(
        &self,
        layout: crate::render_v2::LayoutDocumentV2,
        format: &str,
        cancel: &CancellationToken,
    ) -> Result<(Vec<u8>, &'static str), ExportError>;
    async fn raster_pdf(
        &self,
        bytes: &[u8],
        geometry: &[(u32, u32)],
        cancel: &CancellationToken,
    ) -> Result<Vec<Vec<u8>>, ExportError>;
}

fn sql_error(error: sqlx::Error) -> ExportError {
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
        ExportError(error.to_string())
    } else {
        ExportError(format!("TRANSIENT_HANDLER:{error}"))
    }
}

async fn load_frozen_layout_assets(
    input: &serde_json::Value,
    cancel: &CancellationToken,
    objects: &dyn ExportIo,
) -> Result<Vec<crate::render_v2::FrozenLayoutAssetV2>, ExportError> {
    use sha2::{Digest, Sha256};
    let values = input
        .get("assets")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| ExportError("frozen export assets missing".into()))?;
    validate_frozen_asset_metadata(values)?;
    let mut assets = Vec::with_capacity(values.len());
    let mut decoded_pixels = 0u64;
    for value in values {
        let asset_revision_id = value
            .get("asset_revision_id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| ExportError("frozen asset revision identity missing".into()))?
            .to_owned();
        let sha256 = value
            .get("sha256")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| ExportError("frozen asset digest missing".into()))?
            .to_owned();
        let media_type = value
            .get("media_type")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| ExportError("frozen asset media type missing".into()))?
            .to_owned();
        let file_name = value
            .get("file_name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("资产")
            .to_owned();
        let bytes = if media_type.starts_with("image/") {
            objects
                .read_blob(&sha256, crate::render_v2::MAX_FROZEN_IMAGE_BYTES, cancel)
                .await
                .map_err(|error| {
                    ExportError(format!("read frozen asset {asset_revision_id}: {error}"))
                })?
        } else {
            Vec::new()
        };
        if media_type.starts_with("image/") {
            if bytes.is_empty()
                || bytes.len() > crate::render_v2::MAX_FROZEN_IMAGE_BYTES
                || hex::encode(Sha256::digest(&bytes)) != sha256
            {
                return Err(ExportError(format!(
                    "frozen image asset missing, oversized, or digest-mismatched: {asset_revision_id}"
                )));
            }
            let (width, height) =
                crate::render_v2::frozen_image_dimensions(&bytes).map_err(ExportError)?;
            decoded_pixels = decoded_pixels
                .checked_add(u64::from(width) * u64::from(height))
                .ok_or_else(|| ExportError("decoded image pixel budget overflow".into()))?;
            if decoded_pixels > MAX_FROZEN_ASSET_TOTAL_PIXELS {
                return Err(ExportError(
                    "decoded images exceed aggregate pixel budget".into(),
                ));
            }
        }
        assets.push(crate::render_v2::FrozenLayoutAssetV2 {
            asset_revision_id,
            sha256,
            media_type,
            file_name,
            bytes: Arc::new(bytes),
        });
    }
    Ok(assets)
}

// Keep the export I/O, cancellation and frozen limits explicit at this boundary.
#[allow(clippy::too_many_arguments)]
async fn prepare_pdf_attachments(
    pool: &PgPool,
    job: &SubmissionExportJobV2,
    input: &serde_json::Value,
    cancel: &CancellationToken,
    cleanup_tracker: &platform::StagedObjectCleanupTracker,
    objects: &dyn ExportIo,
    render: &dyn RenderIo,
    limits: &ExportLimits,
) -> Result<(), ExportError> {
    use sha2::{Digest, Sha256};
    const ACTOR: &str = "system:submission-export-v2";
    let blocks = input
        .get("workspace")
        .and_then(|value| value.get("blocks"))
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| ExportError("frozen export blocks missing".into()))?;
    let assets = input
        .get("assets")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| ExportError("frozen export assets missing".into()))?;
    let preparations = input
        .get("attachment_preparations")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| ExportError("frozen attachment preparations missing".into()))?;
    validate_frozen_asset_metadata(assets)?;
    let mut handled = std::collections::HashSet::new();
    let mut raster_total_pages = 0u64;
    let mut raster_total_pixels = 0u64;
    let mut raster_total_bytes = 0usize;
    for preparation in preparations {
        let page_assets = preparation
            .get("page_assets")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| ExportError("attachment preparation page metadata missing".into()))?;
        raster_total_pages = raster_total_pages
            .checked_add(
                u64::try_from(page_assets.len())
                    .map_err(|_| ExportError("prepared page count overflow".into()))?,
            )
            .ok_or_else(|| ExportError("prepared page count overflow".into()))?;
        for page in page_assets {
            let geometry = page
                .get("geometry")
                .ok_or_else(|| ExportError("prepared page geometry missing".into()))?;
            let width = geometry
                .get("width_px")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| ExportError("prepared page width missing".into()))?;
            let height = geometry
                .get("height_px")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| ExportError("prepared page height missing".into()))?;
            raster_total_pixels = raster_total_pixels
                .checked_add(
                    width
                        .checked_mul(height)
                        .ok_or_else(|| ExportError("prepared page pixel overflow".into()))?,
                )
                .ok_or_else(|| ExportError("prepared page pixel overflow".into()))?;
        }
    }
    if raster_total_pages > MAX_PDF_ATTACHMENT_PAGES as u64
        || raster_total_pixels > MAX_FROZEN_ASSET_TOTAL_PIXELS
    {
        return Err(ExportError(
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
            .ok_or_else(|| ExportError("frozen PDF attachment identity missing".into()))?;
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
            .ok_or_else(|| {
                ExportError(format!("frozen PDF attachment asset {source_id} missing"))
            })?;
        if asset.get("media_type").and_then(serde_json::Value::as_str) != Some("application/pdf") {
            continue;
        }
        let source_uuid = Uuid::parse_str(source_id)
            .map_err(|_| ExportError("frozen PDF attachment UUID invalid".into()))?;
        let source_sha = asset
            .get("sha256")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| ExportError("frozen PDF attachment digest missing".into()))?
            .to_owned();
        let source_length = asset
            .get("byte_length")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| ExportError("frozen PDF byte length metadata missing".into()))?;
        if source_length > limits.max_pdf_attachment_bytes as u64 {
            return Err(ExportError(
                "frozen PDF attachment exceeds the source byte budget".into(),
            ));
        }
        let read_sha = source_sha.clone();
        let source_bytes = objects
            .read_blob(&read_sha, limits.max_pdf_attachment_bytes, cancel)
            .await
            .map_err(|error| {
                error.0.strip_prefix("TRANSIENT_HANDLER:").map_or_else(
                    || ExportError(format!("read frozen PDF attachment: {error}")),
                    |message| ExportError(format!("TRANSIENT_HANDLER:{message}")),
                )
            })?;
        if u64::try_from(source_bytes.len()).ok() != Some(source_length)
            || hex::encode(Sha256::digest(&source_bytes)) != source_sha
        {
            return Err(ExportError(
                "frozen PDF attachment length or digest mismatch".into(),
            ));
        }
        let geometry =
            crate::render_v2::frozen_pdf_raster_geometry(&source_bytes).map_err(ExportError)?;
        let declared_pages = asset
            .get("page_count")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| ExportError("frozen PDF page count metadata missing".into()))?;
        if u64::try_from(geometry.len()).ok() != Some(declared_pages) {
            return Err(ExportError(
                "frozen PDF page count metadata mismatch".into(),
            ));
        }
        raster_total_pages = raster_total_pages
            .checked_add(declared_pages)
            .ok_or_else(|| ExportError("rasterized PDF page count overflow".into()))?;
        for (width, height) in &geometry {
            raster_total_pixels = raster_total_pixels
                .checked_add(u64::from(*width) * u64::from(*height))
                .ok_or_else(|| ExportError("rasterized PDF pixel budget overflow".into()))?;
        }
        if raster_total_pages > MAX_PDF_ATTACHMENT_PAGES as u64
            || raster_total_pixels > MAX_FROZEN_ASSET_TOTAL_PIXELS
        {
            return Err(ExportError(
                "rasterized PDF geometry exceeds aggregate budget".into(),
            ));
        }
        let pages = render.raster_pdf(&source_bytes, &geometry, cancel).await?;
        let mut total_pixels = 0u64;
        for page in &pages {
            raster_total_bytes = raster_total_bytes
                .checked_add(page.len())
                .ok_or_else(|| ExportError("rasterized PDF aggregate bytes overflow".into()))?;
            if raster_total_bytes > limits.max_raster_total_bytes {
                return Err(ExportError(
                    "rasterized PDF pages exceed aggregate byte budget".into(),
                ));
            }
            let (width, height) =
                crate::render_v2::frozen_image_dimensions(page).map_err(ExportError)?;
            total_pixels =
                total_pixels
                    .checked_add(u64::from(width).checked_mul(u64::from(height)).ok_or_else(
                        || ExportError("rasterized PDF pixel budget overflow".into()),
                    )?)
                    .ok_or_else(|| ExportError("rasterized PDF pixel budget overflow".into()))?;
            if total_pixels > MAX_FROZEN_ASSET_TOTAL_PIXELS {
                return Err(ExportError(
                    "rasterized PDF pages exceed aggregate pixel budget".into(),
                ));
            }
            i64::try_from(page.len())
                .map_err(|_| ExportError("rasterized PDF page exceeds size limit".into()))?;
            i32::try_from(width)
                .map_err(|_| ExportError("rasterized PDF page width exceeds limit".into()))?;
            i32::try_from(height)
                .map_err(|_| ExportError("rasterized PDF page height exceeds limit".into()))?;
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
            let (width, height) = crate::render_v2::frozen_image_dimensions(&page)
                .expect("raster page metadata was preflighted");
            let digest = hex::encode(Sha256::digest(&page));
            let staging_id = Uuid::new_v4();
            cleanup.register(staging_id);
            match objects
                .stage_object(pool, staging_id, &digest, "image/png", &page, ACTOR)
                .await
            {
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
        let result = crate::bid_authoring_v2::publish_pdf_attachment_preparation_v2(
            pool,
            crate::bid_authoring_v2::PublishPdfAttachmentPreparationV2 {
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
                return Err(sql_error(error));
            }
        }
    }
    Ok(())
}

pub async fn execute(
    pool: &PgPool,
    job: &SubmissionExportJobV2,
    cancel: CancellationToken,
    cleanup_tracker: platform::StagedObjectCleanupTracker,
    objects: &dyn ExportIo,
    render: &dyn RenderIo,
    limits: ExportLimits,
) -> Result<(), ExportError> {
    use sha2::{Digest, Sha256};
    const ACTOR: &str = "system:submission-export-v2";
    let preflight_input = crate::bid_authoring_v2::load_submission_export_input_v2(
        pool,
        job.request.request_artifact_id,
        job.request.request_revision,
        &job.request.frozen_input_sha256,
    )
    .await
    .map_err(sql_error)?;
    if serde_json::to_vec(&preflight_input)
        .map_err(|error| ExportError(format!("serialize frozen export input: {error}")))?
        .len()
        > limits.max_input_bytes
    {
        return Err(ExportError(
            "frozen export input exceeds the byte budget".into(),
        ));
    }
    validate_submission_export_metadata(&preflight_input)?;
    prepare_pdf_attachments(
        pool,
        job,
        &preflight_input,
        &cancel,
        &cleanup_tracker,
        objects,
        render,
        &limits,
    )
    .await
    .map_err(|error| {
        error.0.strip_prefix("TRANSIENT_HANDLER:").map_or_else(
            || ExportError(format!("ATTACHMENT_PREPARATION_FAILED: {}", error.0)),
            |message| ExportError(format!("TRANSIENT_HANDLER:{message}")),
        )
    })?;
    let font_digest = hex::encode(Sha256::digest(crate::render_v2::PDF_FONT_BYTES));
    let font_staging_id = Uuid::new_v4();
    let mut font_cleanup = cleanup_tracker.guard();
    font_cleanup.register(font_staging_id);
    let font_ref = objects
        .stage_object(
            pool,
            font_staging_id,
            &font_digest,
            "font/otf",
            crate::render_v2::PDF_FONT_BYTES,
            ACTOR,
        )
        .await?;
    let prepared = match crate::bid_authoring_v2::prepare_submission_export_v2(
        pool,
        &job.request,
        crate::bid_authoring_v2::SubmissionExportFontV2 {
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
            return Err(sql_error(error));
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
        .ok_or_else(|| ExportError("prepared manifest identity missing".into()))?;
    let manifest_sha = prepared
        .get("sha256")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| ExportError("prepared manifest digest missing".into()))?;
    let snapshot_id = prepared
        .get("render_snapshot_id")
        .and_then(serde_json::Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or_else(|| ExportError("prepared render snapshot identity missing".into()))?;
    let input = crate::bid_authoring_v2::load_submission_manifest_render_input_v2(
        pool,
        manifest_id,
        manifest_sha,
    )
    .await
    .map_err(sql_error)?;
    if serde_json::to_vec(&input)
        .map_err(|error| ExportError(format!("serialize frozen export input: {error}")))?
        .len()
        > limits.max_input_bytes
    {
        return Err(ExportError(
            "frozen export input exceeds the byte budget".into(),
        ));
    }
    validate_submission_export_metadata(&input)?;
    let request = input
        .get("request")
        .ok_or_else(|| ExportError("export request identity missing".into()))?;
    let workspace = input
        .get("workspace")
        .ok_or_else(|| ExportError("frozen export workspace missing".into()))?;
    let title = input
        .get("project_title")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("投标文件");
    let watermark = request
        .get("mode_options")
        .and_then(|value| value.get("watermark"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let assets = load_frozen_layout_assets(&input, &cancel, objects).await?;
    let forms = input
        .get("form_definitions")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| ExportError("frozen form definitions missing".into()))?;
    let preparations = input
        .get("attachment_preparations")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| ExportError("frozen attachment preparations missing".into()))?;
    let layout = crate::render_v2::layout_from_workspace_with_resources(
        title,
        workspace,
        &assets,
        forms,
        preparations,
        watermark,
    )
    .map_err(ExportError)?;
    let format = request
        .get("format")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| ExportError("frozen export format invalid".into()))?
        .to_owned();
    let rendered = render.render(layout, &format, &cancel).await?;
    let (bytes, media_type) = rendered;
    if bytes.is_empty() || bytes.len() > limits.max_render_output_bytes {
        return Err(ExportError(
            "rendered output exceeds the byte budget".into(),
        ));
    }
    let output_digest = hex::encode(Sha256::digest(&bytes));
    let output_staging_id = Uuid::new_v4();
    let mut output_cleanup = cleanup_tracker.guard();
    output_cleanup.register(output_staging_id);
    let output_ref = objects
        .stage_object(
            pool,
            output_staging_id,
            &output_digest,
            media_type,
            &bytes,
            ACTOR,
        )
        .await?;
    let output_id = Uuid::new_v4();
    let result = crate::bid_authoring_v2::publish_submission_export_v2(
        pool,
        &job.request,
        crate::bid_authoring_v2::SubmissionExportFontV2 {
            staging_id: font_staging_id,
            object_ref: &font_ref,
            sha256: &font_digest,
            media_type: "font/otf",
        },
        snapshot_id,
        manifest_id,
        crate::bid_authoring_v2::SubmissionExportOutputV2 {
            staging_id: output_staging_id,
            artifact_id: output_id,
            object_ref: &output_ref,
            sha256: &output_digest,
            media_type,
            byte_length: i64::try_from(bytes.len())
                .map_err(|_| ExportError("rendered object too large".into()))?,
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
            Err(sql_error(error))
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

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn frozen_asset_metadata_budget_rejects_before_reads_or_allocations() {
        let oversized = vec![serde_json::json!({
            "asset_revision_id": Uuid::new_v4(),
            "media_type": "image/png",
            "byte_length": MAX_FROZEN_ASSET_TOTAL_BYTES + 1,
            "width_px": 1,
            "height_px": 1
        })];
        assert!(validate_frozen_asset_metadata(&oversized).is_err());
        let pixel_overflow = vec![serde_json::json!({
            "asset_revision_id": Uuid::new_v4(),
            "media_type": "image/png",
            "byte_length": 1,
            "width_px": MAX_FROZEN_ASSET_TOTAL_PIXELS,
            "height_px": 2
        })];
        assert!(validate_frozen_asset_metadata(&pixel_overflow).is_err());
        let missing_length = vec![serde_json::json!({
            "asset_revision_id": Uuid::new_v4(),
            "media_type": "application/pdf",
            "page_count": 1
        })];
        assert!(validate_frozen_asset_metadata(&missing_length).is_err());
    }

    #[test]
    fn export_metadata_preflight_rejects_aggregate_table_work() {
        let input = serde_json::json!({
            "assets": [],
            "workspace": {"blocks": [{"content": {"type": "table", "row_count": 1_000_001u64,
                "column_count": 1, "cells": []}}]},
            "form_definitions": [],
            "attachment_preparations": []
        });
        assert!(validate_submission_export_metadata(&input).is_err());
    }
}
