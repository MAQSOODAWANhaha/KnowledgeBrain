//! Knowledge-domain Oxana adapters.

use crate::runtime::{
    AppCtx, HandlerDeadline, JobErr, OwnedHandlerCompletion, finish_knowledge_document_job,
    run_owned_handler,
};
use async_trait::async_trait;
use platform::{
    DatatableJob, DocumentProcessJob, HousekeepJob, ImageMultimodalJob, IndexDeleteJob,
    KbDeleteJob, KnowledgeSemanticIndexV2Job, ListDeleteJob, ListReparseJob, LowQueue,
    ManualProcessJob, PostProcessJob, SummaryJob, VersionCloneJob, WikiFinalizeJob, WikiIngestJob,
};
use sqlx::PgPool;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub struct DocumentProcessWorker {
    pool: Option<PgPool>,
    shutdown: CancellationToken,
}

impl oxana::FromContext<AppCtx> for DocumentProcessWorker {
    fn from_context(ctx: &AppCtx) -> Self {
        Self {
            pool: ctx.pool.clone(),
            shutdown: ctx.shutdown.clone(),
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
        let Some(pool) = self.pool.clone() else {
            return Err(JobErr("postgres not configured".into()));
        };
        let deadline = HandlerDeadline::from_now(std::time::Duration::from_secs(
            platform::DOCUMENT_PROCESS_TIMEOUT_SECS,
        ));
        let local_cancel = CancellationToken::new();
        let pipeline_cancel = local_cancel.clone();
        let retries = ctx.meta.retries;
        let run = run_owned_handler(
            async move {
                knowledge::ingest::run_convert(
                    &pool,
                    job.document_id,
                    job.attempt,
                    &job.passages,
                    false,
                    &pipeline_cancel,
                )
                .await
                .map_err(JobErr)
            },
            deadline,
            self.shutdown.clone(),
            local_cancel,
            None,
            |_| true,
        )
        .await;
        match run.completion {
            OwnedHandlerCompletion::ShuttingDown => Err(JobErr("WORKER_SHUTDOWN".into())),
            OwnedHandlerCompletion::TimedOut => {
                finish_knowledge_document_job(
                    self.pool.as_ref().unwrap(),
                    job.document_id,
                    job.attempt,
                    retries,
                    platform::DOCUMENT_PROCESS_MAX_RETRY,
                    Err("document process timeout after 2h".into()),
                )
                .await
            }
            OwnedHandlerCompletion::Completed(Ok(())) => Ok(()),
            OwnedHandlerCompletion::Completed(Err(error)) => {
                finish_knowledge_document_job(
                    self.pool.as_ref().unwrap(),
                    job.document_id,
                    job.attempt,
                    retries,
                    platform::DOCUMENT_PROCESS_MAX_RETRY,
                    Err(error.0),
                )
                .await
            }
        }
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
        let Some(pool) = self.pool.clone() else {
            return Err(JobErr("postgres not configured".into()));
        };
        let deadline = HandlerDeadline::from_now(std::time::Duration::from_secs(
            platform::DOCUMENT_PROCESS_TIMEOUT_SECS,
        ));
        let local_cancel = CancellationToken::new();
        let pipeline_cancel = local_cancel.clone();
        let retries = ctx.meta.retries;
        let run = run_owned_handler(
            async move {
                knowledge::ingest::run_convert(
                    &pool,
                    job.document_id,
                    job.attempt,
                    &[],
                    true,
                    &pipeline_cancel,
                )
                .await
                .map_err(JobErr)
            },
            deadline,
            self.shutdown.clone(),
            local_cancel,
            None,
            |_| true,
        )
        .await;
        match run.completion {
            OwnedHandlerCompletion::ShuttingDown => Err(JobErr("WORKER_SHUTDOWN".into())),
            OwnedHandlerCompletion::TimedOut => {
                finish_knowledge_document_job(
                    self.pool.as_ref().unwrap(),
                    job.document_id,
                    job.attempt,
                    retries,
                    platform::DOCUMENT_PROCESS_MAX_RETRY,
                    Err("manual process timeout after 2h".into()),
                )
                .await
            }
            OwnedHandlerCompletion::Completed(Ok(())) => Ok(()),
            OwnedHandlerCompletion::Completed(Err(error)) => {
                finish_knowledge_document_job(
                    self.pool.as_ref().unwrap(),
                    job.document_id,
                    job.attempt,
                    retries,
                    platform::DOCUMENT_PROCESS_MAX_RETRY,
                    Err(error.0),
                )
                .await
            }
        }
    }
}

pub async fn convert_document(
    pool: &PgPool,
    document_id: Uuid,
    attempt: i32,
    passages: &[String],
    manual: bool,
) -> Result<(), String> {
    knowledge::ingest::run_convert(
        pool,
        document_id,
        attempt,
        passages,
        manual,
        &CancellationToken::new(),
    )
    .await
}

pub struct VersionCloneWorker {
    pool: Option<PgPool>,
    shutdown: CancellationToken,
}

impl oxana::FromContext<AppCtx> for VersionCloneWorker {
    fn from_context(ctx: &AppCtx) -> Self {
        Self {
            pool: ctx.pool.clone(),
            shutdown: ctx.shutdown.clone(),
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
        let Some(pool) = self.pool.clone() else {
            return Err(JobErr("postgres not configured".into()));
        };
        let deadline = HandlerDeadline::from_now(std::time::Duration::from_secs(
            platform::POST_PROCESS_TIMEOUT_SECS,
        ));
        let local_cancel = CancellationToken::new();
        let run = run_owned_handler(
            async move { process_version_clone(&pool, &job).await.map_err(JobErr) },
            deadline,
            self.shutdown.clone(),
            local_cancel,
            None,
            |_| true,
        )
        .await;
        match run.completion {
            OwnedHandlerCompletion::ShuttingDown => Err(JobErr("WORKER_SHUTDOWN".into())),
            OwnedHandlerCompletion::TimedOut => Err(JobErr("handler timeout".into())),
            OwnedHandlerCompletion::Completed(result) => result,
        }
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
    shutdown: CancellationToken,
}

impl oxana::FromContext<AppCtx> for PostProcessWorker {
    fn from_context(ctx: &AppCtx) -> Self {
        Self {
            pool: ctx.pool.clone(),
            shutdown: ctx.shutdown.clone(),
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
        let Some(pool) = self.pool.clone() else {
            return Err(JobErr("postgres not configured".into()));
        };
        let deadline = HandlerDeadline::from_now(std::time::Duration::from_secs(
            platform::POST_PROCESS_TIMEOUT_SECS,
        ));
        let local_cancel = CancellationToken::new();
        let retries = ctx.meta.retries;
        let run = run_owned_handler(
            async move {
                process_post_process(
                    &pool,
                    job.document_id,
                    job.product_version_id,
                    job.clone_keep,
                )
                .await
                .map_err(JobErr)
            },
            deadline,
            self.shutdown.clone(),
            local_cancel,
            None,
            |_| true,
        )
        .await;
        match run.completion {
            OwnedHandlerCompletion::ShuttingDown => Err(JobErr("WORKER_SHUTDOWN".into())),
            OwnedHandlerCompletion::TimedOut => {
                finish_knowledge_document_job(
                    self.pool.as_ref().unwrap(),
                    job.document_id,
                    0,
                    retries,
                    3,
                    Err("post_process timeout after 30min".into()),
                )
                .await
            }
            OwnedHandlerCompletion::Completed(Ok(())) => Ok(()),
            OwnedHandlerCompletion::Completed(Err(error)) => {
                finish_knowledge_document_job(
                    self.pool.as_ref().unwrap(),
                    job.document_id,
                    0,
                    retries,
                    3,
                    Err(error.0),
                )
                .await
            }
        }
    }
}

pub struct KnowledgeSemanticIndexV2Worker {
    pub(crate) pool: Option<PgPool>,
    pub(crate) shutdown: CancellationToken,
    pub(crate) provider: Option<Arc<dyn knowledge::knowledge_index_v2::VectorEmbeddingProviderV2>>,
    pub(crate) provider_configuration_error: Option<String>,
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
            shutdown: ctx.shutdown.clone(),
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
        let Some(pool) = self.pool.clone() else {
            return Err(JobErr("postgres not configured".into()));
        };
        tracing::info!(
            target_id = %job.target_id,
            target_revision = job.target_revision,
            oxana_retry = ctx.meta.retries,
            "knowledge semantic index v2 attempt"
        );
        let deadline = HandlerDeadline::from_now(std::time::Duration::from_secs(
            platform::SEMANTIC_INDEX_V2_TIMEOUT_SECS,
        ));
        let local_cancel = CancellationToken::new();
        let provider = self.provider.clone();
        let provider_configuration_error = self.provider_configuration_error.clone();
        let target_id = job.target_id;
        let target_revision = job.target_revision;
        let successor_slot = Arc::new(std::sync::Mutex::new(None));
        let successor_out = successor_slot.clone();
        let run = run_owned_handler(
            async move {
                let successor = process_semantic_index_intent_v2(
                    &pool,
                    target_id,
                    target_revision,
                    provider.as_deref(),
                    provider_configuration_error.as_deref(),
                )
                .await
                .map_err(JobErr)?;
                *successor_out.lock().expect("semantic successor slot") = successor;
                Ok(())
            },
            deadline,
            self.shutdown.clone(),
            local_cancel,
            None,
            |_| true,
        )
        .await;
        let successor = match run.completion {
            OwnedHandlerCompletion::ShuttingDown => {
                return Err(JobErr("WORKER_SHUTDOWN".into()));
            }
            OwnedHandlerCompletion::TimedOut => {
                let detail = "semantic index v2 timeout after 2h";
                if let Some(pool) = &self.pool {
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
                }
                return Err(JobErr(detail.into()));
            }
            OwnedHandlerCompletion::Completed(Ok(())) => successor_slot
                .lock()
                .expect("semantic successor slot")
                .take(),
            OwnedHandlerCompletion::Completed(Err(error)) => {
                tracing::warn!(
                    target_id = %job.target_id,
                    target_revision = job.target_revision,
                    error = %error.0,
                    "knowledge semantic index v2 retryable failure"
                );
                return Err(error);
            }
        };
        if let Some(successor) = successor {
            enqueue_semantic_index_v2_target(&successor).await?;
        }
        Ok(())
    }
}

pub(crate) async fn enqueue_semantic_index_v2_target(
    target: &knowledge::knowledge_index_v2::SemanticIndexIntentV2,
) -> Result<(), JobErr> {
    match platform::enqueue_semantic_index_v2(target.id, target.target_revision).await {
        Ok(Some(_)) => Ok(()),
        Ok(None) => Err(JobErr("semantic index v2 queue unavailable".into())),
        Err(error) => Err(JobErr(error)),
    }
}

pub async fn process_semantic_index_intent_v2(
    pool: &PgPool,
    target_id: Uuid,
    target_revision: i64,
    provider: Option<&dyn knowledge::knowledge_index_v2::VectorEmbeddingProviderV2>,
    provider_configuration_error: Option<&str>,
) -> Result<Option<knowledge::knowledge_index_v2::SemanticIndexIntentV2>, String> {
    knowledge::knowledge_index_v2::run_semantic_index_job(
        pool,
        target_id,
        target_revision,
        provider,
        provider_configuration_error,
    )
    .await
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
    shutdown: CancellationToken,
}

impl oxana::FromContext<AppCtx> for HousekeepWorker {
    fn from_context(ctx: &AppCtx) -> Self {
        Self {
            pool: ctx.pool.clone(),
            shutdown: ctx.shutdown.clone(),
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
        let Some(pool) = self.pool.clone() else {
            return Err(JobErr("postgres not configured".into()));
        };
        let deadline = HandlerDeadline::from_now(std::time::Duration::from_secs(
            platform::POST_PROCESS_TIMEOUT_SECS,
        ));
        let local_cancel = CancellationToken::new();
        let run = run_owned_handler(
            async move {
                knowledge::housekeep_documents(&pool, platform::HOUSEKEEP_STALE_SECS)
                    .await
                    .map_err(|error| JobErr(error.to_string()))?;
                let _ = platform::HOUSEKEEP_STALE_SECS;
                Ok(())
            },
            deadline,
            self.shutdown.clone(),
            local_cancel,
            None,
            |_| true,
        )
        .await;
        match run.completion {
            OwnedHandlerCompletion::ShuttingDown => Err(JobErr("WORKER_SHUTDOWN".into())),
            OwnedHandlerCompletion::TimedOut => Err(JobErr("handler timeout".into())),
            OwnedHandlerCompletion::Completed(result) => result,
        }
    }
}

pub struct ImageMultimodalWorker {
    pool: Option<PgPool>,
    shutdown: CancellationToken,
}

impl oxana::FromContext<AppCtx> for ImageMultimodalWorker {
    fn from_context(ctx: &AppCtx) -> Self {
        Self {
            pool: ctx.pool.clone(),
            shutdown: ctx.shutdown.clone(),
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
        let Some(pool) = self.pool.clone() else {
            return Err(JobErr("postgres not configured".into()));
        };
        let deadline = HandlerDeadline::from_now(std::time::Duration::from_secs(
            platform::POST_PROCESS_TIMEOUT_SECS,
        ));
        let local_cancel = CancellationToken::new();
        let retries = ctx.meta.retries;
        let document_id = job.document_id;
        let attempt = job.attempt;
        let run = run_owned_handler(
            async move {
                process_image_pg(
                    &pool,
                    job.document_id,
                    &job.image_key,
                    &job.image_source_type,
                    job.enable_ocr,
                    job.enable_caption,
                    job.attempt,
                )
                .await
                .map_err(JobErr)
            },
            deadline,
            self.shutdown.clone(),
            local_cancel,
            None,
            |_| true,
        )
        .await;
        let result = match run.completion {
            OwnedHandlerCompletion::ShuttingDown => {
                return Err(JobErr("WORKER_SHUTDOWN".into()));
            }
            OwnedHandlerCompletion::TimedOut => Err("image multimodal timeout".to_string()),
            OwnedHandlerCompletion::Completed(Ok(())) => return Ok(()),
            OwnedHandlerCompletion::Completed(Err(error)) => Err(error.0),
        };
        if retries >= 3
            && let Some(pool) = &self.pool
        {
            let error = result
                .as_ref()
                .err()
                .map(String::as_str)
                .unwrap_or("timeout");
            let _ = knowledge::set_parse_status(
                pool,
                document_id,
                "finalizing",
                &format!("ocr_error: {error}; caption_error: {error}"),
            )
            .await;
            finalize_multimodal_pg(pool, document_id, attempt).await;
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

pub(crate) async fn finalize_multimodal_pg(pool: &PgPool, document_id: Uuid, attempt: i32) {
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
    pub(crate) pool: Option<PgPool>,
    pub(crate) shutdown: CancellationToken,
}

impl oxana::FromContext<AppCtx> for WikiIngestWorker {
    fn from_context(ctx: &AppCtx) -> Self {
        Self {
            pool: ctx.pool.clone(),
            shutdown: ctx.shutdown.clone(),
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
        let Some(pool) = self.pool.clone() else {
            return Err(JobErr("postgres not configured".into()));
        };
        let deadline = HandlerDeadline::from_now(std::time::Duration::from_secs(
            platform::POST_PROCESS_TIMEOUT_SECS,
        ));
        let local_cancel = CancellationToken::new();
        let run = run_owned_handler(
            async move {
                process_wiki_ingest(
                    &pool,
                    job.product_version_id,
                    job.document_id,
                    &job.operation,
                )
                .await
                .map_err(JobErr)
            },
            deadline,
            self.shutdown.clone(),
            local_cancel,
            None,
            |_| true,
        )
        .await;
        match run.completion {
            OwnedHandlerCompletion::ShuttingDown => Err(JobErr("WORKER_SHUTDOWN".into())),
            OwnedHandlerCompletion::TimedOut => Err(JobErr("handler timeout".into())),
            OwnedHandlerCompletion::Completed(result) => result,
        }
    }
}

pub struct WikiFinalizeWorker {
    pool: Option<PgPool>,
    shutdown: CancellationToken,
}

impl oxana::FromContext<AppCtx> for WikiFinalizeWorker {
    fn from_context(ctx: &AppCtx) -> Self {
        Self {
            pool: ctx.pool.clone(),
            shutdown: ctx.shutdown.clone(),
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
        let Some(pool) = self.pool.clone() else {
            return Err(JobErr("postgres not configured".into()));
        };
        let deadline = HandlerDeadline::from_now(std::time::Duration::from_secs(
            platform::POST_PROCESS_TIMEOUT_SECS,
        ));
        let local_cancel = CancellationToken::new();
        let run = run_owned_handler(
            async move {
                process_wiki_finalize(&pool, job.product_version_id, job.document_id)
                    .await
                    .map_err(JobErr)
            },
            deadline,
            self.shutdown.clone(),
            local_cancel,
            None,
            |_| true,
        )
        .await;
        match run.completion {
            OwnedHandlerCompletion::ShuttingDown => Err(JobErr("WORKER_SHUTDOWN".into())),
            OwnedHandlerCompletion::TimedOut => Err(JobErr("handler timeout".into())),
            OwnedHandlerCompletion::Completed(result) => result,
        }
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

pub async fn process_summary_pg(
    pool: &PgPool,
    document_id: Uuid,
    attempt: i32,
    fallback: bool,
) -> Result<(), String> {
    knowledge::pipeline::run_summary(pool, document_id, attempt, fallback).await
}

pub async fn process_list_delete_pg(pool: &PgPool, document_id: Uuid) -> Result<(), String> {
    knowledge::pipeline::run_list_delete(pool, document_id).await
}

pub async fn process_kb_delete_pg(pool: &PgPool, product_version_id: Uuid) -> Result<(), String> {
    knowledge::ingest::run_kb_delete(pool, product_version_id).await
}

pub async fn process_reparse_pg(
    pool: &PgPool,
    document_id: Uuid,
    attempt: i32,
) -> Result<(), String> {
    knowledge::ingest::run_reparse(pool, document_id, attempt).await
}

/// Timeout + `run_owned_handler` adapter for identical knowledge jobs.
/// Expand only when a caller needs distinct retry/cancel/`fail_now` behavior.
macro_rules! simple_worker {
    ($name:ident, $job:ty, $timeout_secs:expr, $call:expr) => {
        pub struct $name {
            pool: Option<PgPool>,
            shutdown: CancellationToken,
        }
        impl oxana::FromContext<AppCtx> for $name {
            fn from_context(ctx: &AppCtx) -> Self {
                Self {
                    pool: ctx.pool.clone(),
                    shutdown: ctx.shutdown.clone(),
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
                let deadline =
                    HandlerDeadline::from_now(std::time::Duration::from_secs($timeout_secs));
                let local_cancel = CancellationToken::new();
                let run = run_owned_handler(
                    async move { ($call)(pool, job).await.map_err(JobErr) },
                    deadline,
                    self.shutdown.clone(),
                    local_cancel,
                    None,
                    |_| true,
                )
                .await;
                match run.completion {
                    OwnedHandlerCompletion::ShuttingDown => Err(JobErr("WORKER_SHUTDOWN".into())),
                    OwnedHandlerCompletion::TimedOut => Err(JobErr("handler timeout".into())),
                    OwnedHandlerCompletion::Completed(result) => result,
                }
            }
        }
    };
}

pub struct SummaryWorker {
    pool: Option<PgPool>,
    shutdown: CancellationToken,
}

impl oxana::FromContext<AppCtx> for SummaryWorker {
    fn from_context(ctx: &AppCtx) -> Self {
        Self {
            pool: ctx.pool.clone(),
            shutdown: ctx.shutdown.clone(),
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
        let Some(pool) = self.pool.clone() else {
            return Err(JobErr("postgres not configured".into()));
        };
        let fallback = ctx.meta.retries >= 3;
        let deadline = HandlerDeadline::from_now(std::time::Duration::from_secs(
            platform::POST_PROCESS_TIMEOUT_SECS,
        ));
        let local_cancel = CancellationToken::new();
        let run = run_owned_handler(
            async move {
                process_summary_pg(&pool, job.document_id, job.attempt, fallback)
                    .await
                    .map_err(JobErr)
            },
            deadline,
            self.shutdown.clone(),
            local_cancel,
            None,
            |_| true,
        )
        .await;
        match run.completion {
            OwnedHandlerCompletion::ShuttingDown => Err(JobErr("WORKER_SHUTDOWN".into())),
            OwnedHandlerCompletion::TimedOut => Err(JobErr("handler timeout".into())),
            OwnedHandlerCompletion::Completed(result) => result,
        }
    }
}

simple_worker!(
    DatatableWorker,
    DatatableJob,
    platform::POST_PROCESS_TIMEOUT_SECS,
    |pool: PgPool, job: DatatableJob| async move {
        knowledge::pipeline::run_datatable(&pool, job.document_id).await
    }
);
simple_worker!(
    ListDeleteWorker,
    ListDeleteJob,
    platform::POST_PROCESS_TIMEOUT_SECS,
    |pool: PgPool, job: ListDeleteJob| async move {
        process_list_delete_pg(&pool, job.document_id).await
    }
);
simple_worker!(
    KbDeleteWorker,
    KbDeleteJob,
    platform::POST_PROCESS_TIMEOUT_SECS,
    |pool: PgPool, job: KbDeleteJob| async move {
        process_kb_delete_pg(&pool, job.product_version_id).await
    }
);
simple_worker!(
    ListReparseWorker,
    ListReparseJob,
    platform::DOCUMENT_PROCESS_TIMEOUT_SECS,
    |pool: PgPool, job: ListReparseJob| async move {
        process_reparse_pg(&pool, job.document_id, job.attempt).await
    }
);
simple_worker!(
    IndexDeleteWorker,
    IndexDeleteJob,
    platform::POST_PROCESS_TIMEOUT_SECS,
    |pool: PgPool, job: IndexDeleteJob| async move {
        knowledge::purge_document_index(&pool, job.document_id)
            .await
            .map_err(|e| e.to_string())
    }
);
