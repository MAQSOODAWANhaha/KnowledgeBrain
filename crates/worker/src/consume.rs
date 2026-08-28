//! oxana `default` consumer: convert only (ticket 09).

use async_trait::async_trait;
use platform::{
    BidAuthoringJobPayloadV2, BidAuthoringV2Queue, DatatableJob, DefaultQueue, DocumentProcessJob,
    ExtractJob, HousekeepJob,
    ImageMultimodalJob, IndexDeleteJob, KbDeleteJob, KnowledgeSemanticIndexV2Job, ListDeleteJob,
    ListReparseJob, LowQueue, PostProcessJob, PostprocessQueue, QuestionJob,
    RequirementSetCompileJobV2, SummaryJob, SummaryQueue, TenderDocumentProcessJobV2,
    VersionCloneJob, WikiFinalizeJob, WikiIngestJob, WikiQueue,
};
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone)]
pub struct AppCtx {
    pub pool: Option<PgPool>,
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

pub struct TenderDocumentProcessV2Worker {
    pool: Option<PgPool>,
}

impl oxana::FromContext<AppCtx> for TenderDocumentProcessV2Worker {
    fn from_context(ctx: &AppCtx) -> Self {
        Self {
            pool: ctx.pool.clone(),
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
        ctx: &oxana::JobContext,
    ) -> Result<(), Self::Error> {
        let Some(pool) = &self.pool else {
            return Err(JobErr("postgres not configured".into()));
        };
        let request_artifact_id = job.request.request_artifact_id;
        let payload = BidAuthoringJobPayloadV2::TenderDocumentProcess {
            request: job.request,
            project_id: job.project_id,
            document_revision_id: job.document_revision_id,
        };
        let service = bidding::tender_process::TenderDocumentProcessService::new(
            bidding::tender_process::PgTenderDocumentProcessRepository::new(pool.clone()),
            bidding::tender_process::DocParserTenderSourceConverter,
            bidding::tender_process::ExistingTenderVisionEnricher,
            bidding::tender_process::InactiveTenderProcessTransport,
        );
        match service.process(&payload).await {
            Ok(_) => Ok(()),
            Err(error) => {
                if ctx.meta.retries >= platform::BID_AUTHORING_V2_MAX_RETRIES {
                    let _ = bidding::bid_authoring_v2::mark_tender_document_failed_v2(
                        pool,
                        request_artifact_id,
                        "AGENT_OUTPUT_INVALID",
                    )
                    .await;
                }
                Err(JobErr(error.to_string()))
            }
        }
    }
}

pub struct RequirementSetCompileV2Worker {
    pool: Option<PgPool>,
}

impl oxana::FromContext<AppCtx> for RequirementSetCompileV2Worker {
    fn from_context(ctx: &AppCtx) -> Self {
        Self {
            pool: ctx.pool.clone(),
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
        bidding::bid_authoring_v2::compile_requirement_set_v2(
            pool,
            job.request.request_artifact_id,
            job.request.request_revision,
            &job.request.frozen_input_sha256,
        )
        .await
        .map(|_| ())
        .map_err(|error| JobErr(error.to_string()))
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
            convert_document(
                pool,
                job.document_id,
                job.attempt,
                &job.passages,
                job.manual,
            ),
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
    let _ =
        knowledge::insert_dead_letter(pool, platform::TYPE_DOCUMENT_PROCESS, document_id, message)
            .await;
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

pub async fn process_wiki_finalize(pool: &PgPool, version_id: Uuid) -> Result<(), String> {
    knowledge::pipeline::run_wiki_finalize(pool, version_id).await
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
        process_wiki_ingest(pool, job.product_version_id)
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
        process_wiki_finalize(pool, job.product_version_id)
            .await
            .map_err(JobErr)
    }
}

pub async fn process_wiki_ingest(pool: &PgPool, version_id: Uuid) -> Result<(), String> {
    knowledge::pipeline::run_wiki_ingest(pool, version_id).await
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

pub async fn process_reparse_pg(pool: &PgPool, document_id: Uuid) -> Result<(), String> {
    let vid: Option<Uuid> =
        sqlx::query_scalar("SELECT product_version_id FROM documents WHERE id = $1")
            .bind(document_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?;
    knowledge::purge_document_index(pool, document_id)
        .await
        .map_err(|e| e.to_string())?;
    let _ = platform::enqueue_index_delete(document_id).await;
    if let Some(vid) = vid {
        let _ = knowledge::delete_wiki_for_document(pool, vid, document_id).await;
        let _ = knowledge::graph::delete_document(vid, document_id);
        sqlx::query("DELETE FROM task_pending_ops WHERE dedup_key = $1")
            .bind(document_id.to_string())
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
    }
    let attempt = knowledge::bump_document_attempt(pool, document_id)
        .await
        .map_err(|e| e.to_string())?;
    let _ = knowledge::open_attempt(pool, document_id, attempt).await;
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
            let _ =
                platform::enqueue_document_process_with(document_id, vid, attempt, passages).await;
        }
        Some((kind, _)) if kind == "manual" => {
            let _ = platform::enqueue_manual_process(document_id, vid, attempt).await;
        }
        _ => {
            let _ = platform::enqueue_document_process(document_id, vid, attempt).await;
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
        let _ = platform::enqueue_datatable(document_id).await;
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
    |pool: PgPool, job: ListReparseJob| async move { process_reparse_pg(&pool, job.document_id).await }
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

pub async fn run_core(ctx: AppCtx) -> Result<(), String> {
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let signal_stop = stop_tx.clone();
    let signal_task = tokio::spawn(async move {
        shutdown_signal().await;
        let _ = signal_stop.send(true);
    });
    let shut = |stop: tokio::sync::watch::Receiver<bool>| async move {
        wait_for_worker_shutdown(stop).await;
        Ok::<(), std::io::Error>(())
    };
    let timeout = std::time::Duration::from_secs(2);
    let core = {
        let storage = platform::oxana_connect().map_err(|e| e.to_string())?;
        storage
            .runtime(ctx.clone())
            .queue_with_concurrency::<DefaultQueue>(platform::runtime_concurrency("CORE", 8))
            .worker::<DocumentProcessWorker, DocumentProcessJob>()
            .queue_with_concurrency::<BidAuthoringV2Queue>(platform::runtime_concurrency(
                "BID_AUTHORING",
                platform::BID_AUTHORING_V2_CONCURRENCY,
            ))
            .worker::<TenderDocumentProcessV2Worker, TenderDocumentProcessJobV2>()
            .worker::<RequirementSetCompileV2Worker, RequirementSetCompileJobV2>()
            .shutdown_on(shut(stop_rx.clone()))
            .shutdown_timeout(timeout)
            .run()
    };
    let post = {
        let storage = platform::oxana_connect().map_err(|e| e.to_string())?;
        storage
            .runtime(ctx.clone())
            .queue_with_concurrency::<PostprocessQueue>(platform::runtime_concurrency(
                "POSTPROCESS",
                2,
            ))
            .worker::<PostProcessWorker, PostProcessJob>()
            .worker::<KnowledgeSemanticIndexV2Worker, KnowledgeSemanticIndexV2Job>()
            .shutdown_on(shut(stop_rx.clone()))
            .shutdown_timeout(timeout)
            .run()
    };
    let enrich_n = platform::runtime_concurrency("ENRICHMENT", 12);
    let enrich = {
        let storage = platform::oxana_connect().map_err(|e| e.to_string())?;
        storage
            .runtime(ctx.clone())
            .queue_with_concurrency::<SummaryQueue>(enrich_n)
            .worker::<SummaryWorker, SummaryJob>()
            .worker::<DatatableWorker, DatatableJob>()
            .shutdown_on(shut(stop_rx.clone()))
            .shutdown_timeout(timeout)
            .run()
    };
    let maint_n = platform::runtime_concurrency("MAINTENANCE", 4);
    let maint = {
        let storage = platform::oxana_connect().map_err(|e| e.to_string())?;
        storage
            .runtime(ctx.clone())
            .queue_with_concurrency::<LowQueue>(maint_n)
            .worker::<VersionCloneWorker, VersionCloneJob>()
            .worker::<ListDeleteWorker, ListDeleteJob>()
            .worker::<KbDeleteWorker, KbDeleteJob>()
            .worker::<ListReparseWorker, ListReparseJob>()
            .worker::<IndexDeleteWorker, IndexDeleteJob>()
            .shutdown_on(shut(stop_rx.clone()))
            .shutdown_timeout(timeout)
            .run()
    };
    let shared_n = platform::runtime_concurrency("SHARED", 6);
    let shared = {
        let storage = platform::oxana_connect().map_err(|e| e.to_string())?;
        storage
            .runtime(ctx.clone())
            .queue_with_concurrency::<SummaryQueue>(shared_n)
            .worker::<SummaryWorker, SummaryJob>()
            .shutdown_on(shut(stop_rx.clone()))
            .shutdown_timeout(timeout)
            .run()
    };
    let wiki_rt = {
        let storage = platform::oxana_connect().map_err(|e| e.to_string())?;
        storage
            .runtime(ctx)
            .queue_with_concurrency::<WikiQueue>(platform::runtime_concurrency("WIKI", 8))
            .worker::<WikiIngestWorker, WikiIngestJob>()
            .worker::<WikiFinalizeWorker, WikiFinalizeJob>()
            .shutdown_on(shut(stop_rx.clone()))
            .shutdown_timeout(timeout)
            .run()
    };
    let result = tokio::try_join!(core, post, enrich, maint, shared, wiki_rt)
        .map(|_| ())
        .map_err(|e| e.to_string());
    let _ = stop_tx.send(true);
    signal_task.abort();
    result
}

async fn shutdown_signal() {
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
    use platform::{apply_fresh_baseline, connect, write_blob};
    use std::{
        collections::VecDeque,
        sync::atomic::{AtomicUsize, Ordering},
    };

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

    #[test]
    fn wiki_ingest_retry_delay_is_lock_retry() {
        let w = WikiIngestWorker { pool: None };
        let job = WikiIngestJob {
            product_version_id: Uuid::new_v4(),
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
        let _ = sqlx::query(
            "DROP TABLE IF EXISTS
                wiki_log_entries, wiki_folders, wiki_pages,
                graph_relations, graph_nodes, chunk_embeddings, chunks,
                api_keys, models,
                task_dead_letters, task_pending_ops, document_processing_spans,
                document_tags, tags, documents,
                product_versions, products, workspace_members, users, workspaces
             CASCADE",
        )
        .execute(&pool)
        .await;
        apply_fresh_baseline(&pool).await.unwrap();
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
        let _ = sqlx::query(
            "DROP TABLE IF EXISTS
                wiki_log_entries, wiki_folders, wiki_pages,
                graph_relations, graph_nodes, chunk_embeddings, chunks,
                api_keys, models,
                task_dead_letters, task_pending_ops, document_processing_spans,
                document_tags, tags, documents,
                product_versions, products, workspace_members, users, workspaces
             CASCADE",
        )
        .execute(&pool)
        .await;
        apply_fresh_baseline(&pool).await.unwrap();
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
                file_hash: "blank1",
                object_ref: "objects/blank1",
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
        let _ = sqlx::query(
            "DROP TABLE IF EXISTS
                wiki_log_entries, wiki_folders, wiki_pages,
                graph_relations, graph_nodes, chunk_embeddings, chunks,
                api_keys, models,
                task_dead_letters, task_pending_ops, document_processing_spans,
                document_tags, tags, documents,
                product_versions, products, workspace_members, users, workspaces
             CASCADE",
        )
        .execute(&pool)
        .await;
        apply_fresh_baseline(&pool).await.unwrap();
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
        let _ = sqlx::query(
            "DROP TABLE IF EXISTS
                wiki_log_entries, wiki_folders, wiki_pages,
                graph_relations, graph_nodes, chunk_embeddings, chunks,
                api_keys, models,
                task_dead_letters, task_pending_ops, document_processing_spans,
                document_tags, tags, documents,
                product_versions, products, workspace_members, users, workspaces
             CASCADE",
        )
        .execute(&pool)
        .await;
        apply_fresh_baseline(&pool).await.unwrap();
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
        let _ = sqlx::query(
            "DROP TABLE IF EXISTS
                wiki_log_entries, wiki_folders, wiki_pages,
                graph_relations, graph_nodes, chunk_embeddings, chunks,
                api_keys, models,
                task_dead_letters, task_pending_ops, document_processing_spans,
                document_tags, tags, documents,
                product_versions, products, workspace_members, users, workspaces
             CASCADE",
        )
        .execute(&pool)
        .await;
        apply_fresh_baseline(&pool).await.unwrap();
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
        let _ = sqlx::query(
            "DROP TABLE IF EXISTS
                wiki_log_entries, wiki_folders, wiki_pages,
                graph_relations, graph_nodes, chunk_embeddings, chunks,
                api_keys, models,
                task_dead_letters, task_pending_ops, document_processing_spans,
                document_tags, tags, documents,
                product_versions, products, workspace_members, users, workspaces
             CASCADE",
        )
        .execute(&pool)
        .await;
        apply_fresh_baseline(&pool).await.unwrap();
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
        let _ = sqlx::query(
            "DROP TABLE IF EXISTS
                wiki_log_entries, wiki_folders, wiki_pages,
                graph_relations, graph_nodes, chunk_embeddings, chunks,
                api_keys, models,
                task_dead_letters, task_pending_ops, document_processing_spans,
                document_tags, tags, documents,
                product_versions, products, workspace_members, users, workspaces
             CASCADE",
        )
        .execute(&pool)
        .await;
        apply_fresh_baseline(&pool).await.unwrap();
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
        let _ = sqlx::query(
            "DROP TABLE IF EXISTS
                wiki_log_entries, wiki_folders, wiki_pages,
                graph_relations, graph_nodes, chunk_embeddings, chunks,
                api_keys, models,
                task_dead_letters, task_pending_ops, document_processing_spans,
                document_tags, tags, documents,
                product_versions, products, workspace_members, users, workspaces
             CASCADE",
        )
        .execute(&pool)
        .await;
        apply_fresh_baseline(&pool).await.unwrap();
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
        let _ = sqlx::query(
            "DROP TABLE IF EXISTS
                wiki_log_entries, wiki_folders, wiki_pages,
                graph_relations, graph_nodes, chunk_embeddings, chunks,
                api_keys, models,
                task_dead_letters, task_pending_ops, document_processing_spans,
                document_tags, tags, documents,
                product_versions, products, workspace_members, users, workspaces
             CASCADE",
        )
        .execute(&pool)
        .await;
        apply_fresh_baseline(&pool).await.unwrap();
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
        let _ = sqlx::query(
            "DROP TABLE IF EXISTS
                wiki_log_entries, wiki_folders, wiki_pages,
                graph_relations, graph_nodes, chunk_embeddings, chunks,
                api_keys, models,
                task_dead_letters, task_pending_ops, document_processing_spans,
                document_tags, tags, documents,
                product_versions, products, workspace_members, users, workspaces
             CASCADE",
        )
        .execute(&pool)
        .await;
        apply_fresh_baseline(&pool).await.unwrap();
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
        process_reparse_pg(&pool, did).await.unwrap();
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
        assert!(attempt >= 2);
    }

    #[tokio::test]
    async fn convert_url_without_reader_fails_immediately() {
        let _g = db_lock().await;
        unsafe { std::env::remove_var("DOCREADER_ADDR") };
        let Ok(pool) = connect().await else {
            eprintln!("skip: postgres down");
            return;
        };
        let _ = sqlx::query(
            "DROP TABLE IF EXISTS
                wiki_log_entries, wiki_folders, wiki_pages,
                graph_relations, graph_nodes, chunk_embeddings, chunks,
                api_keys, models,
                task_dead_letters, task_pending_ops, document_processing_spans,
                document_tags, tags, documents,
                product_versions, products, workspace_members, users, workspaces
             CASCADE",
        )
        .execute(&pool)
        .await;
        apply_fresh_baseline(&pool).await.unwrap();
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
    async fn wiki_ingest_claims_ingest_lane_only_and_finalizes() {
        let _g = db_lock().await;
        let Ok(pool) = connect().await else {
            eprintln!("skip: postgres down");
            return;
        };
        let _ = sqlx::query(
            "DROP TABLE IF EXISTS
                wiki_log_entries, wiki_folders, wiki_pages,
                graph_relations, graph_nodes, chunk_embeddings, chunks,
                api_keys, models,
                task_dead_letters, task_pending_ops, document_processing_spans,
                document_tags, tags, documents,
                product_versions, products, workspace_members, users, workspaces
             CASCADE",
        )
        .execute(&pool)
        .await;
        apply_fresh_baseline(&pool).await.unwrap();
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
        knowledge::enqueue_pending_op(
            &pool,
            platform::TYPE_WIKI_INGEST,
            vid,
            knowledge::wiki::OP_INGEST,
            Some(&did.to_string()),
            serde_json::json!({"document_id": did}),
        )
        .await
        .unwrap();
        knowledge::enqueue_pending_op(
            &pool,
            platform::TYPE_WIKI_FINALIZE,
            vid,
            knowledge::wiki::OP_SLUG,
            Some("preexisting"),
            serde_json::json!({}),
        )
        .await
        .unwrap();
        process_wiki_ingest(&pool, vid).await.unwrap();
        let ingest_left = knowledge::count_pending(&pool, platform::TYPE_WIKI_INGEST, vid)
            .await
            .unwrap();
        let finalize_left = knowledge::count_pending(&pool, platform::TYPE_WIKI_FINALIZE, vid)
            .await
            .unwrap();
        assert_eq!(ingest_left, 0);
        assert!(
            finalize_left >= 1,
            "finalize lane must survive ingest claim, got {finalize_left}"
        );
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

        process_wiki_finalize(&pool, vid).await.unwrap();
        let finalize_after = knowledge::count_pending(&pool, platform::TYPE_WIKI_FINALIZE, vid)
            .await
            .unwrap();
        assert_eq!(finalize_after, 0);
        let ingest_after = knowledge::count_pending(&pool, platform::TYPE_WIKI_INGEST, vid)
            .await
            .unwrap();
        assert_eq!(ingest_after, 0);
    }

    #[tokio::test]
    async fn wiki_disabled_is_retryable() {
        let _g = db_lock().await;
        let Ok(pool) = connect().await else {
            eprintln!("skip: postgres down");
            return;
        };
        let _ = sqlx::query(
            "DROP TABLE IF EXISTS
                wiki_log_entries, wiki_folders, wiki_pages,
                graph_relations, graph_nodes, chunk_embeddings, chunks,
                api_keys, models,
                task_dead_letters, task_pending_ops, document_processing_spans,
                document_tags, tags, documents,
                product_versions, products, workspace_members, users, workspaces
             CASCADE",
        )
        .execute(&pool)
        .await;
        apply_fresh_baseline(&pool).await.unwrap();
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
        let err = process_wiki_ingest(&pool, seeded.library_version_id)
            .await
            .unwrap_err();
        assert!(err.contains("not wiki enabled"), "{err}");
    }

    #[tokio::test]
    async fn version_clone_worker_copies_doc_and_sets_active() {
        let _g = db_lock().await;
        let Ok(pool) = connect().await else {
            eprintln!("skip: postgres down");
            return;
        };
        let _ = sqlx::query(
            "DROP TABLE IF EXISTS
                wiki_log_entries, wiki_folders, wiki_pages,
                graph_relations, graph_nodes, chunk_embeddings, chunks,
                api_keys, models,
                task_dead_letters, task_pending_ops, document_processing_spans,
                document_tags, tags, documents,
                product_versions, products, workspace_members, users, workspaces
             CASCADE",
        )
        .execute(&pool)
        .await;
        apply_fresh_baseline(&pool).await.unwrap();
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
                file_hash: "abc",
                object_ref: "objects/abc",
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
    async fn process_post_process_clone_keep_sets_progress_and_wiki_pending() {
        let _g = db_lock().await;
        let Ok(pool) = connect().await else {
            eprintln!("skip: postgres down");
            return;
        };
        let _ = sqlx::query(
            "DROP TABLE IF EXISTS
                wiki_log_entries, wiki_folders, wiki_pages,
                graph_relations, graph_nodes, chunk_embeddings, chunks,
                api_keys, models,
                task_dead_letters, task_pending_ops, document_processing_spans,
                document_tags, tags, documents,
                product_versions, products, workspace_members, users, workspaces
             CASCADE",
        )
        .execute(&pool)
        .await;
        apply_fresh_baseline(&pool).await.unwrap();
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
                file_hash: "pp1",
                object_ref: "objects/pp1",
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
        process_post_process(&pool, did, seeded.library_version_id, true)
            .await
            .unwrap();
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
        assert!(pending >= 1, "clone_keep should count wiki/graph");
        let wiki_n =
            knowledge::count_pending(&pool, platform::TYPE_WIKI_INGEST, seeded.library_version_id)
                .await
                .unwrap();
        assert!(
            wiki_n >= 1,
            "wiki ingest pending after clone_keep post_process"
        );
    }

    #[tokio::test]
    async fn process_post_process_writes_summary_and_questions() {
        let _g = db_lock().await;
        let Ok(pool) = connect().await else {
            eprintln!("skip: postgres down");
            return;
        };
        let _ = sqlx::query(
            "DROP TABLE IF EXISTS
                wiki_log_entries, wiki_folders, wiki_pages,
                graph_relations, graph_nodes, chunk_embeddings, chunks,
                api_keys, models,
                task_dead_letters, task_pending_ops, document_processing_spans,
                document_tags, tags, documents,
                product_versions, products, workspace_members, users, workspaces
             CASCADE",
        )
        .execute(&pool)
        .await;
        apply_fresh_baseline(&pool).await.unwrap();
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
                file_hash: "sum1",
                object_ref: "objects/sum1",
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
            qs.as_array().is_some_and(|a| !a.is_empty()),
            "questions written back: {qs}"
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
        let database_url = std::env::var("DATABASE_URL").unwrap_or_default();
        let pool = match sqlx::postgres::PgPoolOptions::new()
            .max_connections(8)
            .connect(&database_url)
            .await
        {
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
