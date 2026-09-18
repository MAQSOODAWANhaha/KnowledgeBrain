//! Oxana adapter runtime: AppCtx, owned-handler fence, and transport group.

#[cfg(not(unix))]
compile_error!("the Worker subprocess supervision contract requires Unix process groups");

use crate::bidding::{
    ContentGenerateV2Worker, DocxComposeV2Worker, RequirementSetCompileV2Worker,
    SubmissionExportV2Worker, TenderDocumentProcessV2Worker,
};
use crate::knowledge::{
    DatatableWorker, DocumentProcessWorker, HousekeepWorker, ImageMultimodalWorker,
    IndexDeleteWorker, KbDeleteWorker, KnowledgeSemanticIndexV2Worker, ListDeleteWorker,
    ListReparseWorker, PostProcessWorker, SummaryWorker, VersionCloneWorker, WikiFinalizeWorker,
    WikiIngestWorker,
};
use platform::{
    BidAuthoringV2Queue, ContentGenerateJobV2, DatatableJob, DefaultQueue, DocumentProcessJob,
    DocxComposeJobV2, HousekeepJob, ImageMultimodalJob, IndexDeleteJob, KbDeleteJob,
    KnowledgeSemanticIndexV2Job, ListDeleteJob, ListReparseJob, LowQueue, ManualProcessJob,
    MultimodalQueue, PostProcessJob, PostprocessQueue, RequirementSetCompileJobV2,
    SubmissionExportJobV2, SummaryJob, SummaryQueue, TenderDocumentProcessJobV2, VersionCloneJob,
    WikiFinalizeJob, WikiIngestJob, WikiQueue,
};
use sqlx::PgPool;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub(crate) const HANDLER_CLEANUP_MARGIN: std::time::Duration = std::time::Duration::from_secs(30);
pub(crate) const TERMINAL_PERSISTENCE_RESERVE: std::time::Duration =
    std::time::Duration::from_secs(5);
pub(crate) const TASK_ABORT_DRAIN_RESERVE: std::time::Duration =
    std::time::Duration::from_millis(100);
pub(crate) const TENDER_HANDLER_HARD_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(30 * 60);
pub(crate) const SUBMISSION_EXPORT_HANDLER_HARD_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(30 * 60);
pub(crate) const CONTENT_GENERATE_HANDLER_HARD_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(45 * 60);
pub(crate) const CONTENT_MATCH_HANDLER_HARD_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(10 * 60);
#[derive(Clone)]
pub struct AppCtx {
    pub pool: Option<PgPool>,
    /// External SIGINT/SIGTERM only. Fatal children must never set this token.
    pub shutdown: CancellationToken,
    /// Process-local cancellation used to stop siblings after a fatal child.
    pub root_cancel: CancellationToken,
}

#[derive(Debug)]
pub struct JobErr(pub String);

impl std::fmt::Display for JobErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for JobErr {}

pub(crate) async fn finish_knowledge_document_job(
    pool: &PgPool,
    document_id: Uuid,
    attempt: i32,
    retries: u32,
    max_retry: u32,
    result: Result<(), String>,
) -> Result<(), JobErr> {
    match result {
        Ok(()) => Ok(()),
        Err(error) if retries >= max_retry => {
            let status = knowledge::document_parse_status(pool, document_id)
                .await
                .ok()
                .flatten();
            if status.as_deref() != Some("completed") {
                knowledge::ingest::fail_now(pool, document_id, attempt, &error)
                    .await
                    .map_err(JobErr)?;
            }
            Ok(())
        }
        Err(error) => Err(JobErr(error)),
    }
}

pub(crate) fn non_agent_sql_error(error: sqlx::Error) -> JobErr {
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

pub(crate) async fn bid_request_is_terminal(
    pool: &PgPool,
    request_artifact_id: Uuid,
) -> Result<bool, JobErr> {
    let status = bidding::bid_authoring_v2::async_request_status_v2(pool, request_artifact_id)
        .await
        .map_err(non_agent_sql_error)?;
    Ok(matches!(status.as_deref(), Some("succeeded" | "failed")))
}

pub(crate) async fn require_bid_request_terminal(
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

pub(crate) async fn terminalize_tender_document_failure_until(
    pool: &PgPool,
    request: &platform::BidAuthoringRequestIdentityV2,
    code: &str,
    cleanup_deadline: tokio::time::Instant,
    label: &str,
) -> Result<bool, JobErr> {
    terminalize_until(cleanup_deadline, label, async {
        if bid_request_is_terminal(pool, request.request_artifact_id).await? {
            return Ok(true);
        }
        let result = bidding::bid_authoring_v2::mark_tender_document_failed_v2(
            pool,
            request.request_artifact_id,
            request.request_revision,
            &request.frozen_input_sha256,
            code,
        )
        .await;
        result.map_err(|error| JobErr(format!("{label}: terminal transition failed: {error}")))?;
        require_bid_request_terminal(pool, request.request_artifact_id, label).await?;
        Ok(false)
    })
    .await
}

pub(crate) struct HandlerDeadline {
    pub(crate) hard: tokio::time::Instant,
    pub(crate) cleanup: tokio::time::Instant,
}

impl HandlerDeadline {
    pub(crate) fn from_now(hard_timeout: std::time::Duration) -> Self {
        let hard = tokio::time::Instant::now() + hard_timeout;
        Self {
            hard,
            cleanup: hard + HANDLER_CLEANUP_MARGIN,
        }
    }

    pub(crate) fn early_cleanup(self) -> tokio::time::Instant {
        std::cmp::min(
            tokio::time::Instant::now() + HANDLER_CLEANUP_MARGIN,
            self.cleanup,
        )
    }
}

#[derive(Debug)]
pub(crate) enum OwnedHandlerCompletion {
    Completed(Result<(), JobErr>),
    TimedOut,
    ShuttingDown,
}

#[derive(Debug)]
pub(crate) struct OwnedHandlerRun {
    pub(crate) completion: OwnedHandlerCompletion,
    pub(crate) cleanup_error: Option<String>,
    pub(crate) cleanup_deadline: tokio::time::Instant,
    #[cfg(test)]
    pub(crate) teardown_deadline: tokio::time::Instant,
}

pub(crate) fn teardown_deadline_for_effect(
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

pub(crate) async fn terminalize_until<F, T>(
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

pub(crate) async fn cleanup_tracker_until(
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

pub(crate) async fn join_cancelled_handler(
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

pub(crate) async fn run_owned_handler<F>(
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

pub(crate) async fn wait_for_worker_shutdown(mut stop: tokio::sync::watch::Receiver<bool>) {
    while !*stop.borrow() {
        if stop.changed().await.is_err() {
            return;
        }
    }
}

/// Tasks this process consumes. Must match `run_transport_group` and
/// `QueueRegistry` required_enabled (except retention) plus housekeep.
pub const WORKER_REGISTERED_TASKS: &[&str] = &[
    platform::TYPE_DOCUMENT_PROCESS,
    platform::TYPE_MANUAL_PROCESS,
    platform::BID_TENDER_DOCUMENT_PROCESS_V2_TASK,
    platform::BID_REQUIREMENT_SET_COMPILE_V2_TASK,
    platform::BID_DOCX_COMPOSE_V2_TASK,
    platform::BID_CONTENT_GENERATE_V2_TASK,
    platform::BID_SUBMISSION_EXPORT_V2_TASK,
    platform::TYPE_POST_PROCESS,
    platform::TYPE_SEMANTIC_INDEX_V2,
    platform::TYPE_SUMMARY,
    platform::TYPE_DATATABLE,
    platform::TYPE_IMAGE_MULTIMODAL,
    platform::TYPE_WIKI_INGEST,
    platform::TYPE_WIKI_FINALIZE,
    platform::TYPE_VERSION_CLONE,
    platform::TYPE_LIST_DELETE,
    platform::TYPE_KB_DELETE,
    platform::TYPE_LIST_REPARSE,
    platform::TYPE_INDEX_DELETE,
    platform::TYPE_MAINTENANCE_HOUSEKEEP,
];

pub(crate) const TRANSPORT_RUNTIME_COUNT: usize = 6;

pub(crate) async fn join_transport_group(
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

pub(crate) async fn run_transport_group(
    ctx: AppCtx,
    core_storage: oxana::Storage,
) -> Result<(), oxana::OxanaError> {
    let post_storage = platform::oxana_connect()?;
    let enrich_storage = platform::oxana_connect()?;
    let maintenance_storage = platform::oxana_connect()?;
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
        .worker::<HousekeepWorker, HousekeepJob>()
        .shutdown_on(shut(stop_rx.clone()))
        .shutdown_timeout(HANDLER_CLEANUP_MARGIN)
        .run();
    tasks.spawn(async move { maintenance.await.map(|_| ()) });
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
