//! Bid-authoring-v2 Oxana adapters.

use crate::helpers::{
    HelperCompositionObjects, HelperExportIo, MAX_EXPORT_INPUT_BYTES, MAX_RENDER_OUTPUT_BYTES,
    MAX_TENDER_DOCUMENT_BYTES, read_blob_in_helper,
};
use crate::runtime::{
    AppCtx, CONTENT_GENERATE_HANDLER_HARD_TIMEOUT, CONTENT_MATCH_HANDLER_HARD_TIMEOUT,
    HANDLER_CLEANUP_MARGIN, HandlerDeadline, JobErr, OwnedHandlerCompletion,
    SUBMISSION_EXPORT_HANDLER_HARD_TIMEOUT, TASK_ABORT_DRAIN_RESERVE, TENDER_HANDLER_HARD_TIMEOUT,
    TERMINAL_PERSISTENCE_RESERVE, bid_request_is_terminal, cleanup_tracker_until,
    require_bid_request_terminal, run_owned_handler, teardown_deadline_for_effect,
    terminalize_tender_document_failure_until, terminalize_until,
};
use async_trait::async_trait;
use platform::{
    BidAuthoringJobPayloadV2, ContentGenerateJobV2, ContentGenerateOperationV2, DocxComposeJobV2,
    RequirementSetCompileJobV2, SubmissionExportJobV2, TenderDocumentProcessJobV2,
};
use sqlx::PgPool;
use tokio_util::sync::CancellationToken;

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

pub(crate) async fn process_submission_export_v2(
    pool: &PgPool,
    job: &SubmissionExportJobV2,
    cancel: CancellationToken,
    cleanup_tracker: platform::StagedObjectCleanupTracker,
) -> Result<(), JobErr> {
    let io = HelperExportIo;
    bidding::submission_export::execute(
        pool,
        job,
        cancel,
        cleanup_tracker,
        &io,
        bidding::submission_export::ExportLimits {
            max_input_bytes: MAX_EXPORT_INPUT_BYTES,
            max_render_output_bytes: MAX_RENDER_OUTPUT_BYTES,
        },
    )
    .await
    .or_else(|error| {
        use bidding::agent_error::RequestQueueEffect;
        match error.request_queue_effect() {
            RequestQueueEffect::AckObsolete => Ok(()),
            _ => Err(JobErr(format!("TRANSIENT_HANDLER:{error}"))),
        }
    })
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
        let cleanup_error = run.cleanup_error;
        let result = match run.completion {
            OwnedHandlerCompletion::ShuttingDown => {
                return Err(JobErr(cleanup_error.map_or_else(
                    || "WORKER_SHUTDOWN".into(),
                    |error| format!("WORKER_SHUTDOWN; cleanup failed: {error}"),
                )));
            }
            OwnedHandlerCompletion::TimedOut => {
                return Err(JobErr(cleanup_error.map_or_else(
                    || "submission export deadline reached; resume the same frozen request".into(),
                    |error| format!("submission export deadline reached; cleanup failed: {error}"),
                )));
            }
            OwnedHandlerCompletion::Completed(result) => result,
        };
        match (result, cleanup_error) {
            (Ok(()), Some(error)) => Err(JobErr(format!(
                "submission export completed but cleanup failed: {error}"
            ))),
            (result, _) => result,
        }
    }
}

pub(crate) struct HelperTenderObjectReader;

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
                terminalize_tender_document_failure_until(
                    pool,
                    &job.request,
                    "TENDER_DOCUMENT_PROCESS_TIMEOUT",
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
                let already_terminal = terminalize_tender_document_failure_until(
                    pool,
                    &job.request,
                    "AGENT_OUTPUT_INVALID",
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

pub(crate) async fn tender_handler_deadline(
    pool: &PgPool,
    request_id: uuid::Uuid,
) -> Result<HandlerDeadline, JobErr> {
    let deadline_unix: Option<i64> = sqlx::query_scalar(
        "SELECT floor(extract(epoch from coalesce(kb_bid_v2_tender_agent_frozen_deadline($1), clock_timestamp()+make_interval(secs => (kb_bid_v2_tender_agent_runtime($1)#>>'{budget,total_timeout_secs}')::double precision))))::bigint",
    )
    .bind(request_id)
    .fetch_one(pool)
    .await
    .map_err(|error| JobErr(error.to_string()))?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| JobErr(error.to_string()))?
        .as_secs() as i64;
    let secs = bidding::tender_analysis::draft::handler_budget_secs(deadline_unix, now, 0);
    Ok(HandlerDeadline::from_now(std::time::Duration::from_secs(
        secs,
    )))
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
        let deadline = tender_handler_deadline(&pool, job.request.request_artifact_id).await?;
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
            deadline,
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
        let deadline = tender_handler_deadline(&pool, job.request.request_artifact_id).await?;
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
            deadline,
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

pub(crate) fn is_obsolete_effect(error: &str) -> bool {
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

pub(crate) async fn cancel_and_join_task_until<T>(
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

pub(crate) enum ContentOwnedCompletion {
    Pipeline(Result<(), bidding::agent_error::AgentError>),
    LeaseLost(bidding::agent_error::AgentError),
    TimedOut,
    ShuttingDown,
}

pub(crate) struct ContentOwnedRun {
    pub(crate) completion: ContentOwnedCompletion,
    pub(crate) cleanup_deadline: tokio::time::Instant,
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn await_content_owned_completion(
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

pub(crate) async fn yield_content_retry_until(
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

pub(crate) async fn process_content_generate_owned_v2(
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
                        let _ = lease_loss_tx.send(bidding::content_generate::content_database_error(error));
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
            result = bidding::content_generate::execute(&pipeline_pool, &pipeline_job, Some(&pipeline_owner)) => result,
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
                            bidding::content_generate::execute(&pipeline_pool, &pipeline_job, None).await
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
