//! oxana `default` consumer: convert only (ticket 09).

use async_trait::async_trait;
use runtime::{
    BidDeliveryTargetKind, BidDeliveryV1Job, BidDeliveryV1Queue, BidDeliveryV1Worker, DatatableJob,
    DefaultQueue, DocumentProcessJob, ExtractJob, HousekeepJob, ImageMultimodalJob, IndexDeleteJob,
    KbDeleteJob, KnowledgeSemanticIndexV2Job, ListDeleteJob, ListReparseJob, LowQueue,
    PostProcessJob, PostprocessQueue, QuestionJob, SummaryJob, SummaryQueue, VersionCloneJob,
    WikiFinalizeJob, WikiIngestJob, WikiQueue,
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

fn truncate_key(key: &str) -> &str {
    let t = key.trim_start_matches("objects/").trim_start_matches('/');
    match t.char_indices().nth(16) {
        Some((i, _)) => &t[..i],
        None => t,
    }
}

#[async_trait]
impl oxana::Worker<DocumentProcessJob> for DocumentProcessWorker {
    type Error = JobErr;

    fn max_retries(&self, _job: &DocumentProcessJob) -> u32 {
        runtime::DOCUMENT_PROCESS_MAX_RETRY
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
            std::time::Duration::from_secs(runtime::DOCUMENT_PROCESS_TIMEOUT_SECS),
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
            && ctx.meta.retries >= runtime::DOCUMENT_PROCESS_MAX_RETRY
            && storage::document_parse_status(pool, job.document_id)
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
    let overrides: Option<domain::ProcessOverrides> = overrides_raw
        .and_then(|v| serde_json::from_value(v).ok())
        .filter(|o: &domain::ProcessOverrides| !o.is_empty());
    if let Some(o) = &overrides
        && let Some(v) = o.asr_config.as_ref().and_then(|a| a.enabled)
    {
        asr_enabled = v;
    }
    let ext = file_name.rsplit('.').next().unwrap_or("txt");
    let parser_engine = domain::parser_engine_for(&chunking_cfg, overrides.as_ref(), ext);
    if parse_status == "completed" {
        return schedule_semantic_index_v2_if_ready(pool, version_id).await;
    }
    if matches!(parse_status.as_str(), "cancelled" | "deleting") {
        return Ok(());
    }
    let flipped = storage::try_set_processing(pool, document_id)
        .await
        .map_err(|e| e.to_string())?;
    if !flipped {
        return match storage::document_parse_status(pool, document_id)
            .await
            .map_err(|error| error.to_string())?
            .as_deref()
        {
            Some("completed") => schedule_semantic_index_v2_if_ready(pool, version_id).await,
            _ => Ok(()),
        };
    }
    let _ = storage::open_attempt(pool, document_id, attempt).await;
    tracing::info!(
        document_id = %document_id,
        file = %file_name,
        engine = %parser_engine,
        attempt,
        "parse convert start"
    );
    if !passages.is_empty() {
        let _ = storage::start_span(
            pool,
            document_id,
            attempt,
            obs::SPAN_DOCREADER,
            Some(obs::ROOT_NAME),
            Some(serde_json::json!({"engine": "passages"})),
        )
        .await;
        let _ = storage::finish_span(
            pool,
            document_id,
            attempt,
            obs::SPAN_DOCREADER,
            obs::STATUS_DONE,
            Some(serde_json::json!({"engine": "passages"})),
        )
        .await;
        let _ = storage::start_span(
            pool,
            document_id,
            attempt,
            obs::SPAN_CHUNKING,
            Some(obs::ROOT_NAME),
            None,
        )
        .await;
        let indexed = persist_passage_index(pool, document_id, version_id, passages).await?;
        let _ = storage::finish_span(
            pool,
            document_id,
            attempt,
            obs::SPAN_CHUNKING,
            obs::STATUS_DONE,
            Some(serde_json::json!({"passages": passages.len()})),
        )
        .await;
        let _ = storage::start_span(
            pool,
            document_id,
            attempt,
            obs::SPAN_EMBEDDING,
            Some(obs::ROOT_NAME),
            None,
        )
        .await;
        let _ = storage::finish_span(
            pool,
            document_id,
            attempt,
            obs::SPAN_EMBEDDING,
            obs::STATUS_DONE,
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
        let bytes = storage::read_blob(&file_hash).map_err(|e| e.to_string())?;
        let md = String::from_utf8_lossy(&bytes).into_owned();
        let _ = storage::start_span(
            pool,
            document_id,
            attempt,
            obs::SPAN_DOCREADER,
            Some(obs::ROOT_NAME),
            Some(serde_json::json!({"engine": "manual", "file": file_name})),
        )
        .await;
        let _ = storage::write_blob_async(&format!("{file_hash}.md"), md.as_bytes()).await;
        let _ = storage::finish_span(
            pool,
            document_id,
            attempt,
            obs::SPAN_DOCREADER,
            obs::STATUS_DONE,
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
        let bytes = storage::read_blob(&file_hash).map_err(|e| e.to_string())?;
        let (is_url, url) = parse_stored_url(&bytes);
        let engine = docparser::resolve_engine(&parser_engine, ext, is_url);
        let _ = storage::start_span(
            pool,
            document_id,
            attempt,
            obs::SPAN_DOCREADER,
            Some(obs::ROOT_NAME),
            Some(serde_json::json!({"engine": engine, "file": file_name})),
        )
        .await;
        if engine == "docreader" && docparser::reader_addr().is_none() {
            fail_pipeline(
                pool,
                document_id,
                attempt,
                obs::SPAN_DOCREADER,
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
                return fail_stage_retryable(pool, document_id, attempt, obs::SPAN_DOCREADER, &e.0)
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
                    obs::SPAN_DOCREADER,
                    &result.error,
                )
                .await?;
                return Ok(());
            }
            return fail_stage_retryable(
                pool,
                document_id,
                attempt,
                obs::SPAN_DOCREADER,
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
                        obs::SPAN_DOCREADER,
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
                        obs::SPAN_DOCREADER,
                        &result.error,
                    )
                    .await?;
                    return Ok(());
                }
                return fail_stage_retryable(
                    pool,
                    document_id,
                    attempt,
                    obs::SPAN_DOCREADER,
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
        let _ =
            storage::write_blob_async(&format!("{file_hash}.md"), result.markdown.as_bytes()).await;
        let _ = storage::finish_span(
            pool,
            document_id,
            attempt,
            obs::SPAN_DOCREADER,
            obs::STATUS_DONE,
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
    let existing_chunks = storage::load_document_chunks(pool, document_id)
        .await
        .unwrap_or_default();
    let chunks =
        if obs::stage_satisfied(&prior_spans, obs::SPAN_CHUNKING) && !existing_chunks.is_empty() {
            tracing::info!(
                document_id = %document_id,
                chunks = existing_chunks.len(),
                "parse chunking reuse"
            );
            existing_chunks
        } else {
            let _ = storage::start_span(
                pool,
                document_id,
                attempt,
                obs::SPAN_CHUNKING,
                Some(obs::ROOT_NAME),
                None,
            )
            .await;
            let split = chunker::split_from_config(
                &markdown,
                version_id,
                document_id,
                chunker::SplitterConfig {
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
            let kept = index::keep_nonempty_chunks(split);
            storage::delete_graph_for_document(pool, document_id)
                .await
                .map_err(|e| e.to_string())?;
            storage::replace_document_chunks(pool, document_id, &kept, &[])
                .await
                .map_err(|e| e.to_string())?;
            let _ = storage::finish_span(
                pool,
                document_id,
                attempt,
                obs::SPAN_CHUNKING,
                obs::STATUS_DONE,
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
    if obs::stage_satisfied(&prior_spans, obs::SPAN_EMBEDDING) {
        return Ok(());
    }
    let _ = storage::start_span(
        pool,
        document_id,
        attempt,
        obs::SPAN_EMBEDDING,
        Some(obs::ROOT_NAME),
        None,
    )
    .await;
    let indexed =
        match persist_document_embeddings(pool, document_id, &chunks, opts.vector, opts.keyword)
            .await
        {
            Ok(v) => v,
            Err(e) => {
                return fail_stage_retryable(pool, document_id, attempt, obs::SPAN_EMBEDDING, &e)
                    .await;
            }
        };
    let _ = storage::finish_span(
        pool,
        document_id,
        attempt,
        obs::SPAN_EMBEDDING,
        obs::STATUS_DONE,
        None,
    )
    .await;
    tracing::info!(
        document_id = %document_id,
        chunks = chunks.len(),
        "parse embedding done"
    );
    let images = enrichment::markdown_image_keys(&markdown);
    let mut mm = storage::version_multimodal_enabled(pool, version_id)
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
                    enrichment::image_source_type(&file_name, &markdown)
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
            storage::write_blob_async(format!("{file_hash}.md").as_str(), joined.as_bytes()).await;
    }
    let chunks: Vec<domain::Chunk> = passages
        .iter()
        .map(|text| domain::Chunk {
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
        if !domain::vlm_configured() {
            let _ = storage::skip_span(
                pool,
                document_id,
                attempt,
                obs::SPAN_MULTIMODAL,
                "vlm not configured",
            )
            .await;
            let _ = storage::set_parse_status(
                pool,
                document_id,
                "finalizing",
                "ocr_error: vlm not configured; caption_error: vlm not configured",
            )
            .await;
            let _ = storage::set_index_ready(pool, document_id, false).await;
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
        let _ = storage::start_span(
            pool,
            document_id,
            attempt,
            obs::SPAN_MULTIMODAL,
            Some(obs::ROOT_NAME),
            Some(serde_json::json!({"images": images.len()})),
        )
        .await;
        tracing::info!(
            document_id = %document_id,
            images = images.len(),
            "parse multimodal enqueue"
        );
        let mut pending = domain::Store::default();
        enrichment::set_pending(&mut pending, document_id, images.len() as i32);
        let mut leftover = images.len() as i32;
        for key in images {
            match runtime::enqueue_image_multimodal(
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
                    enrichment::decr_pending(&mut pending, document_id);
                }
            }
        }
        if leftover <= 0 {
            let _ = storage::skip_span(
                pool,
                document_id,
                attempt,
                obs::SPAN_MULTIMODAL,
                "enqueue failed",
            )
            .await;
            let _ = storage::set_parse_status(
                pool,
                document_id,
                "finalizing",
                "ocr_error: image enqueue failed; caption_error: image enqueue failed",
            )
            .await;
            let _ = storage::set_index_ready(pool, document_id, false).await;
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
    let _ = storage::skip_span(
        pool,
        document_id,
        attempt,
        obs::SPAN_MULTIMODAL,
        "no images",
    )
    .await;
    let _ = storage::set_index_ready(pool, document_id, true).await;
    tracing::info!(document_id = %document_id, index_ready = true, "parse completed");
    if text_count == 0 {
        let _ = storage::set_parse_status(pool, document_id, "completed", "").await;
        let _ = storage::skip_span(
            pool,
            document_id,
            attempt,
            obs::SPAN_POSTPROCESS,
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
    overrides: Option<&domain::ProcessOverrides>,
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
    chunks: &[domain::Chunk],
    vector_on: bool,
    keyword_on: bool,
) -> Result<PersistIndexResult, String> {
    if status_aborted(document_parse_status(pool, document_id).await.as_deref()) {
        return Ok(PersistIndexResult::Aborted);
    }
    let kept = index::keep_nonempty_chunks(chunks.to_vec());
    storage::delete_graph_for_document(pool, document_id)
        .await
        .map_err(|e| e.to_string())?;
    // Chunk rows first so an embed failure does not throw away the split.
    storage::replace_document_chunks(pool, document_id, &kept, &[])
        .await
        .map_err(|e| e.to_string())?;
    persist_document_embeddings(pool, document_id, &kept, vector_on, keyword_on).await
}

async fn persist_document_embeddings(
    pool: &PgPool,
    document_id: Uuid,
    chunks: &[domain::Chunk],
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
    let embeddings = index::index_chunks(chunks, &title, vector_on, keyword_on, &model_id)?;
    if status_aborted(document_parse_status(pool, document_id).await.as_deref()) {
        return Ok(PersistIndexResult::Aborted);
    }
    storage::replace_document_embeddings(pool, document_id, &embeddings)
        .await
        .map_err(|e| e.to_string())?;
    let st = document_parse_status(pool, document_id).await;
    if status_aborted(st.as_deref()) {
        if st.as_deref() == Some("deleting") {
            let _ = storage::purge_document_index(pool, document_id).await;
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

async fn document_stage_spans(pool: &PgPool, document_id: Uuid, attempt: i32) -> Vec<domain::Span> {
    storage::list_spans_attempt(pool, document_id, attempt)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|r| r.into_span())
        .collect()
}

fn reused_image_source(spans: &[domain::Span]) -> String {
    let Some(span) = spans.iter().find(|s| s.name == obs::SPAN_DOCREADER) else {
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

fn reused_markdown(spans: &[domain::Span], file_hash: &str) -> Option<String> {
    if !obs::stage_satisfied(spans, obs::SPAN_DOCREADER) {
        return None;
    }
    let bytes = storage::read_blob(&format!("{file_hash}.md")).ok()?;
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
            if let Err(e) = storage::write_blob(&hash, &data) {
                tracing::warn!(hash = %hash, error = %e, "image persist failed");
            }
        }
    })
    .await;
    md
}

async fn maybe_start_postprocess(pool: &PgPool, document_id: Uuid, version_id: Uuid, attempt: i32) {
    let rows = storage::list_spans_attempt(pool, document_id, attempt)
        .await
        .unwrap_or_default();
    let spans: Vec<_> = rows.into_iter().map(|r| r.into_span()).collect();
    if !obs::can_start_stage_or_legacy(obs::SPAN_POSTPROCESS, &spans) {
        return;
    }
    let _ = storage::start_span(
        pool,
        document_id,
        attempt,
        obs::SPAN_POSTPROCESS,
        Some(obs::ROOT_NAME),
        None,
    )
    .await;
    let _ = runtime::enqueue_post_process(document_id, version_id, false).await;
}

async fn fail_stage_retryable(
    pool: &PgPool,
    document_id: Uuid,
    attempt: i32,
    stage: &str,
    message: &str,
) -> Result<(), String> {
    let _ = storage::finish_span(
        pool,
        document_id,
        attempt,
        stage,
        obs::STATUS_FAILED,
        Some(serde_json::json!({"error": message})),
    )
    .await;
    let _ = storage::cancel_dependent_stages(pool, document_id, attempt, stage).await;
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
    let _ = storage::finish_span(
        pool,
        document_id,
        attempt,
        stage,
        obs::STATUS_FAILED,
        Some(serde_json::json!({"error": message})),
    )
    .await;
    let _ = storage::cancel_dependent_stages(pool, document_id, attempt, stage).await;
    let _ = storage::finish_span(
        pool,
        document_id,
        attempt,
        obs::ROOT_NAME,
        obs::STATUS_FAILED,
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
    storage::set_parse_status(pool, document_id, "failed", message)
        .await
        .map_err(|e| e.to_string())?;
    let _ = storage::insert_dead_letter(pool, domain::TYPE_DOCUMENT_PROCESS, document_id, message)
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
    let diffs: Vec<clone::CloneDiff> = match &job.diffs {
        serde_json::Value::Array(a) => a
            .iter()
            .filter_map(|v| serde_json::from_value(v.clone()).ok())
            .collect(),
        _ => Vec::new(),
    };
    let follow = clone::run_clone(
        pool,
        job.source_version_id,
        job.target_version_id,
        &diffs,
        job.make_current,
    )
    .await?;
    for f in follow {
        if f.clone_keep || f.task_type == domain::TYPE_POST_PROCESS {
            let _ =
                runtime::enqueue_post_process(f.document_id, f.product_version_id, f.clone_keep)
                    .await;
        } else {
            let _ = runtime::enqueue_document_process(f.document_id, f.product_version_id, 1).await;
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
            std::time::Duration::from_secs(runtime::POST_PROCESS_TIMEOUT_SECS),
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
            && storage::document_parse_status(pool, job.document_id)
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
    provider: Option<Arc<dyn storage::knowledge_index_v2::VectorEmbeddingProviderV2>>,
    provider_configuration_error: Option<String>,
}

impl oxana::FromContext<AppCtx> for KnowledgeSemanticIndexV2Worker {
    fn from_context(ctx: &AppCtx) -> Self {
        let provider_result = storage::knowledge_index_v2::StrictVectorEmbeddingClientV2::new(
            Arc::new(storage::knowledge_index_v2::EnvironmentEmbeddingCredentialResolverV2),
        );
        let provider_configuration_error = provider_result.as_ref().err().map(ToString::to_string);
        let provider = provider_result.ok().map(|provider| {
            Arc::new(provider) as Arc<dyn storage::knowledge_index_v2::VectorEmbeddingProviderV2>
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
        runtime::SEMANTIC_INDEX_V2_MAX_RETRY
    }

    fn retry_delay(&self, _job: &KnowledgeSemanticIndexV2Job, _retries: u32) -> u64 {
        runtime::SEMANTIC_INDEX_V2_RETRY_DELAY_SECS
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
            std::time::Duration::from_secs(runtime::SEMANTIC_INDEX_V2_TIMEOUT_SECS),
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
                storage::knowledge_index_v2::record_semantic_index_error_v2(
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
    target: &storage::knowledge_index_v2::SemanticIndexIntentV2,
) -> Result<(), JobErr> {
    match runtime::enqueue_semantic_index_v2(target.id, target.target_revision).await {
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
    provider: Option<&dyn storage::knowledge_index_v2::VectorEmbeddingProviderV2>,
    provider_configuration_error: Option<&str>,
) -> Result<Option<storage::knowledge_index_v2::SemanticIndexIntentV2>, String> {
    use storage::knowledge_index_v2::{
        SemanticIndexCompletionV2, SemanticIndexPreflightV2, VectorIndexErrorV2,
    };

    let Some(intent) =
        storage::knowledge_index_v2::semantic_index_intent_v2(pool, target_id, target_revision)
            .await
            .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    match storage::knowledge_index_v2::preflight_semantic_index_intent_v2(
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
            let _ = storage::knowledge_index_v2::record_semantic_index_intent_v2(
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
        storage::knowledge_index_v2::record_semantic_index_intent_v2(
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
        storage::knowledge_index_v2::rebuild_semantic_keyword_indexes_v2(pool, &intent).await?;
        let has_vector = storage::knowledge_index_v2::semantic_vector_generation_matches_intent_v2(
            pool, &intent,
        )
        .await
        .map_err(VectorIndexErrorV2::Database)?;
        if !has_vector {
            storage::knowledge_index_v2::rebuild_vector_indexes_for_intent_v2(
                pool, &intent, provider,
            )
            .await?;
        }
        storage::knowledge_index_v2::complete_semantic_index_intent_v2(pool, &intent)
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
            let _ = storage::knowledge_index_v2::record_semantic_index_intent_v2(
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
            storage::knowledge_index_v2::record_semantic_index_intent_v2(
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
            let _ = storage::knowledge_index_v2::record_semantic_index_intent_v2(
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
            storage::knowledge_index_v2::record_semantic_index_intent_v2(
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
            let _ = storage::knowledge_index_v2::record_semantic_index_intent_v2(
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
) -> Result<Option<storage::knowledge_index_v2::SemanticIndexIntentV2>, String> {
    use storage::knowledge_index_v2::SemanticIndexPreparationV2;
    match storage::knowledge_index_v2::prepare_semantic_index_intent_v2(pool, product_version_id)
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
        runtime::enqueue_semantic_index_v2(target_id, revision)
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
    use storage::knowledge_index_v2::SemanticIndexPreparationV2;
    match storage::knowledge_index_v2::prepare_semantic_index_intent_v2(pool, product_version_id)
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

/// Enqueue already returns `Ok(None)` for `DeclaredDisabled` lanes.
/// Running generators here would re-enable those lanes in-process.
fn allow_inline_fallback(task_type: &str) -> bool {
    !matches!(
        domain::launch_mode(task_type),
        Ok(Some(domain::LaunchMode::DeclaredDisabled))
    )
}

/// Invoke `inline` only when the registry does not declare the lane disabled.
fn run_inline_if_allowed<T>(task_type: &str, inline: impl FnOnce() -> T) -> Option<T> {
    allow_inline_fallback(task_type).then(inline)
}

#[tracing::instrument(
    name = "parse.postprocess",
    skip_all,
    fields(document_id = %document_id, clone_keep)
)]
pub async fn process_post_process(
    pool: &PgPool,
    document_id: Uuid,
    product_version_id: Uuid,
    clone_keep: bool,
) -> Result<(), String> {
    let ws: Option<Uuid> = sqlx::query_scalar(
        "SELECT p.workspace_id FROM documents d
         JOIN product_versions pv ON pv.id = d.product_version_id
         JOIN products p ON p.id = pv.product_id
         WHERE d.id = $1",
    )
    .bind(document_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;
    let Some(ws) = ws else {
        return Ok(());
    };
    tracing::info!(
        document_id = %document_id,
        clone_keep,
        "parse postprocess start"
    );
    let attempt: i32 =
        sqlx::query_scalar("SELECT COALESCE(attempt, 1) FROM documents WHERE id = $1")
            .bind(document_id)
            .fetch_optional(pool)
            .await
            .ok()
            .flatten()
            .unwrap_or(1);
    let rows = storage::list_spans_attempt(pool, document_id, attempt)
        .await
        .unwrap_or_default();
    let spans: Vec<_> = rows.into_iter().map(|r| r.into_span()).collect();
    if !obs::can_start_stage_or_legacy(obs::SPAN_POSTPROCESS, &spans) {
        return Err("postprocess waiting for embedding and multimodal".into());
    }
    let _ = storage::start_span(
        pool,
        document_id,
        attempt,
        obs::SPAN_POSTPROCESS,
        Some(obs::ROOT_NAME),
        None,
    )
    .await;
    let mut store = domain::Store::default();
    storage::hydrate_workspace(pool, &mut store, ws)
        .await
        .map_err(|e| e.to_string())?;
    crate::post_process(
        &mut store,
        &serde_json::json!({
            "document_id": document_id,
            "clone_keep": clone_keep
        }),
    )?;
    let Some(doc) = store.documents.get(&document_id).cloned() else {
        return Ok(());
    };
    if doc.parse_status.is_aborted() {
        let _ =
            storage::skip_span(pool, document_id, attempt, obs::SPAN_POSTPROCESS, "aborted").await;
        return Ok(());
    }
    if doc.parse_status == domain::ParseStatus::Completed {
        storage::set_document_progress(pool, document_id, "completed", 0)
            .await
            .map_err(|e| e.to_string())?;
        let _ = storage::set_summary_status(pool, document_id, "none").await;
        finish_postprocess_spans(pool, document_id, attempt).await;
        return schedule_semantic_index_v2_if_ready(pool, product_version_id).await;
    }
    if doc.parse_status != domain::ParseStatus::Finalizing {
        finish_postprocess_spans(pool, document_id, attempt).await;
        return Ok(());
    }
    let wiki_on = store
        .versions
        .get(&product_version_id)
        .map(|v| v.wiki_enabled)
        .unwrap_or(false);
    let wiki_trigger = wiki_on
        && store
            .queue
            .iter()
            .any(|j| j.task_type == domain::TYPE_WIKI_INGEST);
    if wiki_trigger {
        storage::enqueue_pending_op(
            pool,
            domain::TYPE_WIKI_INGEST,
            product_version_id,
            wiki::OP_INGEST,
            Some(&document_id.to_string()),
            serde_json::json!({"document_id": document_id}),
        )
        .await
        .map_err(|e| e.to_string())?;
    }
    if !storage::set_finalizing(pool, document_id, doc.pending_subtasks_count)
        .await
        .map_err(|e| e.to_string())?
    {
        finish_postprocess_spans(pool, document_id, attempt).await;
        return Ok(());
    }
    if matches!(doc.summary_status, domain::SummaryStatus::Pending) {
        let _ = storage::set_summary_status(pool, document_id, "pending").await;
    }
    if store
        .queue
        .iter()
        .any(|j| j.task_type == domain::TYPE_SUMMARY)
    {
        match runtime::enqueue_summary(document_id, attempt).await {
            Ok(Some(_)) => {}
            Ok(None) => {
                enrichment::generate_summary(&mut store, document_id);
                let _ = storage::persist_summary_chunks(pool, &store, document_id).await;
                if let Some(d) = store.documents.get(&document_id) {
                    let st = match d.summary_status {
                        domain::SummaryStatus::Completed => "completed",
                        domain::SummaryStatus::Failed => "failed",
                        domain::SummaryStatus::Pending => "pending",
                        domain::SummaryStatus::Processing => "processing",
                        domain::SummaryStatus::None => "none",
                    };
                    let _ =
                        storage::set_document_description(pool, document_id, &d.description).await;
                    let _ = storage::set_summary_status(pool, document_id, st).await;
                }
                let _ = storage::finalize_subtask(pool, document_id).await;
            }
            Err(_) => {
                let _ = storage::finalize_subtask(pool, document_id).await;
            }
        }
    }
    let question_jobs: Vec<domain::Job> = store
        .queue
        .iter()
        .filter(|j| j.task_type == domain::TYPE_QUESTION)
        .cloned()
        .collect();
    for (batch, j) in question_jobs.into_iter().enumerate() {
        let ids: Vec<Uuid> = j
            .payload
            .get("chunk_ids")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().and_then(|s| Uuid::parse_str(s).ok()))
                    .collect()
            })
            .unwrap_or_default();
        let parse_opt = |key: &str| {
            j.payload
                .get(key)
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .map(|x| x.as_str().and_then(|s| Uuid::parse_str(s).ok()))
                        .collect::<Vec<Option<Uuid>>>()
                })
                .unwrap_or_default()
        };
        let prev_ids = parse_opt("prev_ids");
        let next_ids = parse_opt("next_ids");
        match runtime::enqueue_question_neighbors(
            document_id,
            ids.clone(),
            prev_ids.clone(),
            next_ids.clone(),
            attempt,
            batch as u32,
        )
        .await
        {
            Ok(Some(_)) => {}
            Ok(None) => {
                if run_inline_if_allowed(domain::TYPE_QUESTION, || {
                    enrichment::generate_questions_with(
                        &mut store,
                        &ids,
                        &prev_ids,
                        &next_ids,
                        document_id,
                        attempt,
                    )
                })
                .is_some()
                {
                    let _ =
                        storage::persist_question_updates(pool, &store, document_id, &ids).await;
                }
                let _ = storage::finalize_subtask(pool, document_id).await;
            }
            Err(_) => {
                let _ = storage::finalize_subtask(pool, document_id).await;
            }
        }
    }
    let extracts: Vec<(Uuid, Uuid)> = store
        .queue
        .iter()
        .filter(|j| j.task_type == domain::TYPE_CHUNK_EXTRACT)
        .filter_map(|j| {
            let cid = j
                .payload
                .get("chunk_id")
                .and_then(|v| v.as_str())
                .and_then(|s| Uuid::parse_str(s).ok())?;
            let did = j
                .payload
                .get("document_id")
                .and_then(|v| v.as_str())
                .and_then(|s| Uuid::parse_str(s).ok())?;
            Some((cid, did))
        })
        .collect();
    for (cid, did) in &extracts {
        match runtime::enqueue_extract(*cid, *did, attempt).await {
            Ok(Some(_)) => {}
            Ok(None) => {
                if let Some(outcome) = run_inline_if_allowed(domain::TYPE_CHUNK_EXTRACT, || {
                    graph::extract_chunk(&mut store, *cid, *did)
                }) {
                    outcome?;
                    let _ = storage::persist_graph_for_document(pool, &store, document_id).await;
                    let _ = graph::sync_document(&store, document_id);
                }
                let _ = storage::finalize_subtask(pool, document_id).await;
            }
            Err(_) => {
                let _ = storage::finalize_subtask(pool, document_id).await;
            }
        }
    }
    if wiki_trigger {
        match runtime::enqueue_wiki_ingest(product_version_id).await {
            Ok(Some(_)) => {}
            Ok(None) => {
                let _ = process_wiki_ingest(pool, product_version_id).await;
            }
            Err(_) => {
                let _ = storage::finalize_subtask(pool, document_id).await;
            }
        }
    }
    finish_postprocess_spans(pool, document_id, attempt).await;
    tracing::info!(
        document_id = %document_id,
        clone_keep,
        "parse postprocess done"
    );
    schedule_semantic_index_v2_if_ready(pool, product_version_id).await
}

async fn finish_postprocess_spans(pool: &PgPool, document_id: Uuid, attempt: i32) {
    let _ = storage::finish_span(
        pool,
        document_id,
        attempt,
        obs::SPAN_POSTPROCESS,
        obs::STATUS_DONE,
        None,
    )
    .await;
    let _ = storage::finish_span(
        pool,
        document_id,
        attempt,
        obs::ROOT_NAME,
        obs::STATUS_DONE,
        None,
    )
    .await;
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
        Some(runtime::HOUSEKEEP_CRON.into())
    }

    fn cron_queue_config() -> Option<oxana::QueueConfig> {
        Some(<LowQueue as oxana::Queue>::to_config())
    }

    async fn process(
        &self,
        _job: HousekeepJob,
        _ctx: &oxana::JobContext,
    ) -> Result<(), Self::Error> {
        if !runtime::housekeep_enabled() {
            return Ok(());
        }
        let Some(pool) = &self.pool else {
            return Err(JobErr("postgres not configured".into()));
        };
        storage::housekeep_documents(pool, runtime::HOUSEKEEP_STALE_SECS)
            .await
            .map_err(|e| JobErr(e.to_string()))?;
        Ok(())
    }
}

pub struct BidDeliveryHandler {
    pool: Option<PgPool>,
}

impl oxana::FromContext<AppCtx> for BidDeliveryHandler {
    fn from_context(ctx: &AppCtx) -> Self {
        Self {
            pool: ctx.pool.clone(),
        }
    }
}

#[async_trait]
impl runtime::BidDeliveryV1Handler for BidDeliveryHandler {
    type Error = JobErr;

    async fn handle(&self, delivery: BidDeliveryV1Job) -> Result<(), Self::Error> {
        let Some(pool) = &self.pool else {
            return Err(JobErr("postgres not configured".into()));
        };
        match delivery.target_kind {
            BidDeliveryTargetKind::DocumentConversion => {
                let successor = bid::tender::convert_and_schedule_document(
                    pool,
                    delivery.target_id,
                    delivery.target_revision,
                )
                .await
                .map_err(JobErr)?;
                if let Some((target_id, target_revision)) = successor {
                    enqueue_bid_targets([(
                        BidDeliveryTargetKind::ExtractionTarget,
                        target_id,
                        target_revision,
                    )])
                    .await
                    .map_err(JobErr)?;
                }
                Ok(())
            }
            BidDeliveryTargetKind::ExtractionTarget => bid::tender::run_extraction_target(
                pool,
                delivery.target_id,
                delivery.target_revision,
            )
            .await
            .map_err(JobErr),
            BidDeliveryTargetKind::MatchingSchedule => {
                process_matching_schedule(pool, delivery.target_id, delivery.target_revision)
                    .await
                    .map_err(JobErr)
            }
            BidDeliveryTargetKind::MatchingJob => bid::matching::run_match_route_v1(
                pool,
                delivery.target_id,
                delivery.target_revision,
            )
            .await
            .map_err(JobErr),
            BidDeliveryTargetKind::AttachmentPreparation => {
                process_attachment_preparation(pool, delivery.target_id, delivery.target_revision)
                    .await
            }
            BidDeliveryTargetKind::SubmissionRender => {
                process_submission_render(pool, delivery.target_id, delivery.target_revision).await
            }
        }
    }
}

const ATTACHMENT_PREPARATION_ACTOR: &str = "system:bid-attachment-preparation";

async fn process_attachment_preparation(
    pool: &PgPool,
    preparation_job_id: Uuid,
    target_revision: i64,
) -> Result<(), JobErr> {
    let attachment_revision = i32::try_from(target_revision)
        .map_err(|_| JobErr("attachment preparation target revision is invalid".into()))?;
    let Some(target) = storage::bid_submission::load_attachment_preparation(
        pool,
        preparation_job_id,
        attachment_revision,
    )
    .await
    .map_err(|error| JobErr(error.to_string()))?
    else {
        return Ok(());
    };
    let result = prepare_attachment_pdf_pages(pool, &target, attachment_revision).await;
    match result {
        Ok(_) => Ok(()),
        Err(detail) => {
            let error_code = attachment_preparation_error_code(&detail);
            let retryable = attachment_preparation_error_retryable(error_code);
            let status = storage::bid_submission::fail_attachment_preparation(
                pool,
                target.preparation_job_id,
                attachment_revision,
                error_code,
                &detail,
                retryable,
            )
            .await
            .map_err(|error| JobErr(error.to_string()))?;
            match status.as_deref() {
                Some("pending") => Err(JobErr(detail)),
                Some("failed") => {
                    tracing::error!(
                        preparation_job_id=%target.preparation_job_id,
                        attachment_id=%target.attachment_id,
                        %error_code,
                        "attachment preparation reached durable failed state"
                    );
                    Ok(())
                }
                Some("cancelled") | None => Ok(()),
                Some(other) => Err(JobErr(format!(
                    "attachment preparation returned invalid status {other}"
                ))),
            }
        }
    }
}

async fn prepare_attachment_pdf_pages(
    pool: &PgPool,
    target: &storage::bid_submission::AttachmentPreparationTarget,
    attachment_revision: i32,
) -> Result<serde_json::Value, String> {
    let digest = target.content_sha256.clone();
    let expected_bytes = target.byte_length;
    let expected_ref = target.object_ref.clone();
    let bytes = tokio::task::spawn_blocking(move || {
        let bytes = storage::read_blob(&digest)
            .map_err(|error| format!("ATTACHMENT_SOURCE_BYTES_MISSING: {error}"))?;
        let metadata = storage::bid_submission::validate_upload_bytes(&bytes, true)
            .map_err(|error| format!("ATTACHMENT_SOURCE_INVALID: {error}"))?;
        if domain::sha256_hex(&bytes) != digest
            || storage::object_ref(&digest) != expected_ref
            || metadata.media_type != "application/pdf"
            || metadata.byte_length != expected_bytes
        {
            return Err("ATTACHMENT_SOURCE_IDENTITY_MISMATCH".to_string());
        }
        Ok::<_, String>(bytes)
    })
    .await
    .map_err(|error| format!("ATTACHMENT_SOURCE_READ_TASK_FAILED: {error}"))??;
    let overrides = std::collections::HashMap::from([("pdf_force_scanned".into(), "true".into())]);
    let converted = docparser::convert_with(docparser::ConvertInput {
        engine: "builtin",
        file_name: "attachment.pdf",
        file_type: "pdf",
        is_url: false,
        bytes,
        url: "",
        title: "attachment.pdf",
        overrides: &overrides,
    })
    .await
    .map_err(|error| format!("ATTACHMENT_PDF_RENDER_FAILED: {error}"))?;
    if !converted.error.is_empty() {
        return Err(format!("ATTACHMENT_PDF_RENDER_FAILED: {}", converted.error));
    }
    let page_count = converted
        .metadata
        .get("page_count")
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| "ATTACHMENT_RENDER_PAGE_SET_INVALID: page count missing".to_string())?;
    if !(1..=512).contains(&page_count) || converted.images.len() != page_count {
        return Err("ATTACHMENT_RENDER_PAGE_SET_INVALID: page count mismatch".into());
    }
    let mut staging = AttachmentPageStagingGuard::new(pool.clone());
    let mut pages = Vec::with_capacity(page_count);
    let mut total_bytes = 0usize;
    for (page_ordinal, image) in converted.images.into_iter().enumerate() {
        let page_bytes = image.data;
        let (page_bytes, metadata) = tokio::task::spawn_blocking(move || {
            let metadata = storage::bid_submission::validate_upload_bytes(&page_bytes, false)
                .map_err(|error| format!("ATTACHMENT_RENDER_PAGE_INVALID: {error}"))?;
            Ok::<_, String>((page_bytes, metadata))
        })
        .await
        .map_err(|error| format!("ATTACHMENT_RENDER_PAGE_TASK_FAILED: {error}"))??;
        total_bytes = total_bytes
            .checked_add(page_bytes.len())
            .ok_or_else(|| "ATTACHMENT_RENDER_PAGE_QUOTA_EXCEEDED".to_string())?;
        if total_bytes > 256 * 1024 * 1024 {
            return Err("ATTACHMENT_RENDER_PAGE_QUOTA_EXCEEDED".into());
        }
        let page_digest = domain::sha256_hex(&page_bytes);
        let page_object_ref = storage::object_ref(&page_digest);
        let staging_id = Uuid::new_v4();
        // Register the identity before the cancellable database await. The
        // server may commit staging even when this future is dropped before it
        // receives the response; the guard must still know what to abandon.
        staging.push(staging_id);
        storage::stage_object_upload(
            pool,
            staging_id,
            &page_object_ref,
            &page_digest,
            metadata.media_type,
            metadata.byte_length,
            ATTACHMENT_PREPARATION_ACTOR,
        )
        .await
        .map_err(|error| format!("ATTACHMENT_RENDER_PAGE_STAGE_FAILED: {error}"))?;
        storage::write_blob_async(&page_digest, &page_bytes)
            .await
            .map_err(|error| format!("ATTACHMENT_RENDER_PAGE_WRITE_FAILED: {error}"))?;
        pages.push(serde_json::json!({
            "staging_id":staging_id,
            "page_ordinal":page_ordinal,
            "object_ref":page_object_ref,
            "digest":page_digest,
            "media_type":metadata.media_type,
            "byte_length":metadata.byte_length,
            "pixel_width":metadata.pixel_width,
            "pixel_height":metadata.pixel_height,
        }));
    }
    let published = storage::bid_submission::publish_attachment_preparation(
        pool,
        target.preparation_job_id,
        attachment_revision,
        &serde_json::Value::Array(pages),
        ATTACHMENT_PREPARATION_ACTOR,
    )
    .await
    .map_err(|error| format!("ATTACHMENT_PREPARATION_PUBLISH_FAILED: {error}"))?;
    staging.disarm();
    Ok(published)
}

fn attachment_preparation_error_code(detail: &str) -> &'static str {
    for code in [
        "ATTACHMENT_SOURCE_BYTES_MISSING",
        "ATTACHMENT_SOURCE_READ_TASK_FAILED",
        "ATTACHMENT_SOURCE_INVALID",
        "ATTACHMENT_SOURCE_IDENTITY_MISMATCH",
        "ATTACHMENT_PDF_RENDER_FAILED",
        "ATTACHMENT_RENDER_PAGE_SET_INVALID",
        "ATTACHMENT_RENDER_PAGE_INVALID",
        "ATTACHMENT_RENDER_PAGE_TASK_FAILED",
        "ATTACHMENT_RENDER_PAGE_QUOTA_EXCEEDED",
        "ATTACHMENT_RENDER_PAGE_STAGE_FAILED",
        "ATTACHMENT_RENDER_PAGE_WRITE_FAILED",
        "ATTACHMENT_PREPARATION_CLAIM_LOST",
        "ATTACHMENT_PREPARATION_PUBLISH_FAILED",
    ] {
        if detail.contains(code) {
            return code;
        }
    }
    "ATTACHMENT_PREPARATION_FAILED"
}

fn attachment_preparation_error_retryable(code: &str) -> bool {
    matches!(
        code,
        "ATTACHMENT_SOURCE_BYTES_MISSING"
            | "ATTACHMENT_SOURCE_READ_TASK_FAILED"
            | "ATTACHMENT_RENDER_PAGE_TASK_FAILED"
            | "ATTACHMENT_RENDER_PAGE_STAGE_FAILED"
            | "ATTACHMENT_RENDER_PAGE_WRITE_FAILED"
            | "ATTACHMENT_PREPARATION_PUBLISH_FAILED"
            | "ATTACHMENT_PREPARATION_FAILED"
    )
}

struct AttachmentPageStagingGuard {
    pool: PgPool,
    staging_ids: Vec<Uuid>,
}

const STAGING_ABANDON_ATTEMPTS: usize = 10;
const STAGING_ABANDON_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(50);

async fn abandon_staging_with_retry(pool: &PgPool, staging_id: Uuid, actor: &str) {
    let mut last_error = None;
    for attempt in 1..=STAGING_ABANDON_ATTEMPTS {
        match storage::abandon_object_upload(pool, staging_id, actor).await {
            Ok(true) => return,
            Ok(false) => last_error = None,
            Err(error) => last_error = Some(error.to_string()),
        }
        if attempt < STAGING_ABANDON_ATTEMPTS {
            tokio::time::sleep(STAGING_ABANDON_RETRY_DELAY).await;
        }
    }
    tracing::warn!(%staging_id, %actor, ?last_error,
        "staging abandon retries exhausted; retention expiry remains the backstop");
}

impl AttachmentPageStagingGuard {
    fn new(pool: PgPool) -> Self {
        Self {
            pool,
            staging_ids: Vec::new(),
        }
    }

    fn push(&mut self, staging_id: Uuid) {
        self.staging_ids.push(staging_id);
    }

    fn disarm(&mut self) {
        self.staging_ids.clear();
    }
}

impl Drop for AttachmentPageStagingGuard {
    fn drop(&mut self) {
        if self.staging_ids.is_empty() {
            return;
        }
        let pool = self.pool.clone();
        let staging_ids = std::mem::take(&mut self.staging_ids);
        tokio::spawn(async move {
            for staging_id in staging_ids {
                abandon_staging_with_retry(&pool, staging_id, ATTACHMENT_PREPARATION_ACTOR).await;
            }
        });
    }
}

async fn enqueue_bid_targets(
    targets: impl IntoIterator<Item = (BidDeliveryTargetKind, Uuid, i64)>,
) -> Result<(), String> {
    let enqueuer =
        runtime::BidDeliveryEnqueuer::new(runtime::connect().map_err(|error| error.to_string())?);
    enqueue_bid_targets_with(targets, move |target_kind, target_id, target_revision| {
        let enqueuer = enqueuer.clone();
        async move {
            enqueuer
                .enqueue(target_kind, target_id, target_revision)
                .await
        }
    })
    .await
}

async fn enqueue_bid_targets_with<F, Fut>(
    targets: impl IntoIterator<Item = (BidDeliveryTargetKind, Uuid, i64)>,
    mut enqueue: F,
) -> Result<(), String>
where
    F: FnMut(BidDeliveryTargetKind, Uuid, i64) -> Fut,
    Fut: std::future::Future<Output = runtime::BidDeliveryEnqueueOutcome>,
{
    for (target_kind, target_id, target_revision) in targets {
        match enqueue(target_kind, target_id, target_revision).await {
            runtime::BidDeliveryEnqueueOutcome::Accepted { .. } => {}
            runtime::BidDeliveryEnqueueOutcome::Indeterminate { error } => {
                return Err(format!(
                    "{} target {target_id} revision {target_revision} enqueue failed: {error}",
                    target_kind.as_str()
                ));
            }
        }
    }
    Ok(())
}

async fn process_matching_schedule(
    pool: &PgPool,
    schedule_intent_id: Uuid,
    target_revision: i64,
) -> Result<(), String> {
    let Some(receipt) = storage::bid_matching::execute_schedule(
        pool,
        schedule_intent_id,
        target_revision,
        bid::matching_schedule_environment(),
        &storage::bid_matching::ScheduleMutationContext::system(),
    )
    .await
    .map_err(|error| error.to_string())?
    else {
        return Ok(());
    };
    enqueue_bid_targets(
        receipt
            .jobs
            .into_iter()
            .map(|job| (BidDeliveryTargetKind::MatchingJob, job.id, job.generation)),
    )
    .await
}

async fn process_submission_render(
    pool: &PgPool,
    render_job_id: Uuid,
    target_revision: i64,
) -> Result<(), JobErr> {
    let Some(target) =
        storage::bid_submission::load_submission_render(pool, render_job_id, target_revision)
            .await
            .map_err(|error| JobErr(error.to_string()))?
    else {
        return Ok(());
    };
    let result = render_submission_manifest(pool, &target, target_revision).await;
    match result {
        Ok(_) => Ok(()),
        Err(detail) => {
            let error_code = submission_render_error_code(&detail);
            let retryable = submission_render_error_retryable(error_code, &detail);
            let status = storage::bid_submission::fail_submission_render(
                pool,
                target.render_job_id,
                target_revision,
                error_code,
                &detail,
                retryable,
            )
            .await
            .map_err(|error| JobErr(error.to_string()))?;
            match status.as_deref() {
                Some("pending") => Err(JobErr(detail)),
                Some("failed") => {
                    tracing::error!(
                        render_job_id = %target.render_job_id,
                        %error_code,
                        "submission render reached durable failed state"
                    );
                    Ok(())
                }
                _ => {
                    tracing::warn!(
                        render_job_id = %target.render_job_id,
                        "submission render target was already settled"
                    );
                    Ok(())
                }
            }
        }
    }
}

fn submission_render_error_code(detail: &str) -> &'static str {
    for code in [
        "MANIFEST_SHA256_MISMATCH",
        "RENDERER_CONTRACT_MISMATCH",
        "SUBMISSION_END_STATE_CHANGED",
        "SUBMISSION_MANIFEST_MISSING",
        "MANIFEST_ASSET_MISSING",
        "MANIFEST_ASSET_IDENTITY_MISMATCH",
        "MANIFEST_ASSET_BYTES_MISSING",
        "PROJECT_ENDED",
        "SUBMISSION_RENDER_CLAIM_LOST",
    ] {
        if detail.contains(code) {
            return code;
        }
    }
    "SUBMISSION_RENDER_FAILED"
}

fn submission_render_error_retryable(error_code: &str, detail: &str) -> bool {
    if error_code != "SUBMISSION_RENDER_FAILED" {
        return false;
    }
    let detail = detail.to_ascii_lowercase();
    if [
        "permission denied",
        "authentication failed",
        "unauthorized",
        "forbidden",
        "not configured",
        "configuration",
        "renderer",
        "manifest",
        "bid shot",
        "markdown",
    ]
    .iter()
    .any(|fragment| detail.contains(fragment))
    {
        return false;
    }
    [
        "temporary",
        "temporarily",
        "timeout",
        "timed out",
        "connection refused",
        "connection reset",
        "connection closed",
        "broken pipe",
        "network is unreachable",
        "dns error",
        "error trying to connect",
        "service unavailable",
        "too many requests",
        "too many connections",
        "deadlock detected",
        "could not serialize access",
        "no space left on device",
        "-> 408",
        "-> 429",
        "-> 500",
        "-> 502",
        "-> 503",
        "-> 504",
    ]
    .iter()
    .any(|fragment| detail.contains(fragment))
}

async fn render_submission_manifest(
    pool: &PgPool,
    target: &storage::bid_submission::SubmissionRenderTarget,
    target_revision: i64,
) -> Result<serde_json::Value, String> {
    let input =
        storage::bid_submission::manifest_render_input(pool, target.project_id, target.manifest_id)
            .await
            .map_err(|error| error.to_string())?;
    if input
        .get("content_sha256")
        .and_then(serde_json::Value::as_str)
        != Some(target.expected_manifest_sha256.as_str())
    {
        return Err("MANIFEST_SHA256_MISMATCH".into());
    }
    let format = match input.get("format").and_then(serde_json::Value::as_str) {
        Some("pdf") => bid::submission::GateFormat::Pdf,
        Some("docx") => bid::submission::GateFormat::Docx,
        _ => return Err("invalid manifest format".into()),
    };
    if input.get("renderer_contract") != Some(&bid::renderer_contract_identity(format)) {
        return Err("RENDERER_CONTRACT_MISMATCH".into());
    }
    let parts = input
        .get("parts")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .map(|row| {
            (
                row.get("part_key")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                row.get("markdown")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            )
        })
        .collect::<Vec<_>>();
    let asset_rows = input
        .get("assets")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "manifest assets missing".to_string())?;
    let mut assets = Vec::with_capacity(asset_rows.len());
    for row in asset_rows {
        let asset_id = row
            .get("id")
            .and_then(serde_json::Value::as_str)
            .and_then(|value| Uuid::parse_str(value).ok())
            .ok_or_else(|| "manifest asset id invalid".to_string())?;
        let stored = storage::bid_submission::read_manifest_render_asset(
            pool,
            target.project_id,
            target.manifest_id,
            asset_id,
        )
        .await
        .map_err(|error| error.to_string())?;
        let manifest_ordinal = u32::try_from(stored.manifest_ordinal)
            .map_err(|_| "manifest asset ordinal invalid".to_string())?;
        let occurrence_ordinal = u32::try_from(stored.occurrence_ordinal)
            .map_err(|_| "manifest occurrence ordinal invalid".to_string())?;
        let locator = match stored.source_kind.as_str() {
            "bid_shot" => bid::ManifestRenderAssetLocator::BidShot {
                placement_ordinal: stored
                    .source_locator
                    .get("placement_ordinal")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|value| u32::try_from(value).ok())
                    .ok_or_else(|| "bid shot placement locator invalid".to_string())?,
                shot_artifact_id: stored
                    .source_locator
                    .get("shot_artifact_id")
                    .and_then(serde_json::Value::as_str)
                    .and_then(|value| Uuid::parse_str(value).ok())
                    .ok_or_else(|| "bid shot artifact locator invalid".to_string())?,
            },
            "markdown_object" => bid::ManifestRenderAssetLocator::MarkdownObject {
                part_key: stored
                    .source_locator
                    .get("part_key")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| "markdown part locator invalid".to_string())?
                    .to_string(),
                occurrence_ordinal,
            },
            "procedural_attachment" => {
                bid::ManifestRenderAssetLocator::ProceduralAttachmentOriginal {
                    part_key: stored
                        .source_locator
                        .get("part_key")
                        .and_then(serde_json::Value::as_str)
                        .ok_or_else(|| "procedural attachment part locator invalid".to_string())?
                        .to_string(),
                    attachment_ordinal: stored
                        .source_locator
                        .get("attachment_ordinal")
                        .and_then(serde_json::Value::as_u64)
                        .and_then(|value| u32::try_from(value).ok())
                        .ok_or_else(|| {
                            "procedural attachment ordinal locator invalid".to_string()
                        })?,
                    attachment_id: stored
                        .source_locator
                        .get("attachment_id")
                        .and_then(serde_json::Value::as_str)
                        .and_then(|value| Uuid::parse_str(value).ok())
                        .ok_or_else(|| "procedural attachment id locator invalid".to_string())?,
                    kind: stored
                        .source_locator
                        .get("kind")
                        .and_then(serde_json::Value::as_str)
                        .ok_or_else(|| "procedural attachment kind locator invalid".to_string())?
                        .to_string(),
                }
            }
            "procedural_attachment_page" => {
                bid::ManifestRenderAssetLocator::ProceduralAttachmentPage {
                    part_key: stored
                        .source_locator
                        .get("part_key")
                        .and_then(serde_json::Value::as_str)
                        .ok_or_else(|| {
                            "procedural attachment page part locator invalid".to_string()
                        })?
                        .to_string(),
                    attachment_ordinal: stored
                        .source_locator
                        .get("attachment_ordinal")
                        .and_then(serde_json::Value::as_u64)
                        .and_then(|value| u32::try_from(value).ok())
                        .ok_or_else(|| {
                            "procedural attachment page ordinal locator invalid".to_string()
                        })?,
                    attachment_id: stored
                        .source_locator
                        .get("attachment_id")
                        .and_then(serde_json::Value::as_str)
                        .and_then(|value| Uuid::parse_str(value).ok())
                        .ok_or_else(|| {
                            "procedural attachment page id locator invalid".to_string()
                        })?,
                    page_ordinal: stored
                        .source_locator
                        .get("page_ordinal")
                        .and_then(serde_json::Value::as_u64)
                        .and_then(|value| u32::try_from(value).ok())
                        .ok_or_else(|| "procedural attachment page locator invalid".to_string())?,
                }
            }
            _ => return Err("manifest asset source kind invalid".into()),
        };
        assets.push(bid::ManifestRenderAsset {
            manifest_ordinal,
            locator,
            object_ref: stored.object_ref,
            digest: stored.digest,
            media_type: stored.media_type,
            byte_length: u64::try_from(stored.byte_length)
                .map_err(|_| "manifest asset byte length invalid".to_string())?,
            bytes: stored.bytes,
        });
    }
    let bytes = bid::render_manifest_document(format, "投标文件", &parts, &assets)?;
    let digest = domain::sha256_hex(&bytes);
    let object_ref = storage::object_ref(&digest);
    let media_type = match format {
        bid::submission::GateFormat::Pdf => "application/pdf",
        bid::submission::GateFormat::Docx => {
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        }
    };
    let staging_id = Uuid::new_v4();
    let mut staging = SubmissionStagingGuard::stage(
        pool.clone(),
        staging_id,
        &object_ref,
        &digest,
        media_type,
        bytes.len() as i64,
        &target.requested_by,
    )
    .await
    .map_err(|error| error.to_string())?;
    storage::write_blob_async(&digest, &bytes)
        .await
        .map_err(|error| error.to_string())?;
    let published = storage::bid_submission::publish_submission_output(
        pool,
        storage::bid_submission::PublishSubmissionOutput {
            staging_id,
            id: Uuid::new_v4(),
            render_job_id: target.render_job_id,
            target_revision,
            object_ref: &object_ref,
            digest: &digest,
            byte_length: bytes.len() as i64,
        },
    )
    .await
    .map_err(|error| error.to_string())?;
    staging.disarm();
    Ok(published)
}

struct SubmissionStagingGuard {
    pool: PgPool,
    staging_id: Option<Uuid>,
    actor: String,
}

impl SubmissionStagingGuard {
    async fn stage(
        pool: PgPool,
        staging_id: Uuid,
        object_ref: &str,
        digest: &str,
        media_type: &str,
        byte_length: i64,
        actor: &str,
    ) -> Result<Self, sqlx::Error> {
        // Hold the cleanup identity before the cancellable database await. The
        // server can commit staging before this future receives its response.
        let guard = Self::new(pool.clone(), staging_id, actor);
        storage::stage_object_upload(
            &pool,
            staging_id,
            object_ref,
            digest,
            media_type,
            byte_length,
            actor,
        )
        .await?;
        Ok(guard)
    }

    fn new(pool: PgPool, staging_id: Uuid, actor: &str) -> Self {
        Self {
            pool,
            staging_id: Some(staging_id),
            actor: actor.to_string(),
        }
    }

    fn disarm(&mut self) {
        self.staging_id = None;
    }
}

impl Drop for SubmissionStagingGuard {
    fn drop(&mut self) {
        let Some(staging_id) = self.staging_id.take() else {
            return;
        };
        let pool = self.pool.clone();
        let actor = self.actor.clone();
        tokio::spawn(async move {
            abandon_staging_with_retry(&pool, staging_id, &actor).await;
        });
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
            let _ = storage::set_parse_status(
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

#[tracing::instrument(
    name = "parse.image",
    skip_all,
    fields(document_id = %document_id, attempt)
)]
pub async fn process_image_pg(
    pool: &PgPool,
    document_id: Uuid,
    image_key: &str,
    image_source_type: &str,
    enable_ocr: bool,
    enable_caption: bool,
    attempt: i32,
) -> Result<(), String> {
    let current: Option<i32> = sqlx::query_scalar("SELECT attempt FROM documents WHERE id = $1")
        .bind(document_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?;
    if current.is_some_and(|n| n != attempt) {
        return Ok(());
    }
    let ws: Option<Uuid> = storage::document_workspace_id(pool, document_id)
        .await
        .map_err(|e| e.to_string())?;
    let Some(ws) = ws else {
        return Ok(());
    };
    let mut store = domain::Store::default();
    storage::hydrate_workspace(pool, &mut store, ws)
        .await
        .map_err(|e| e.to_string())?;
    if let Some(d) = store.documents.get_mut(&document_id)
        && d.parse_status == domain::ParseStatus::Pending
    {
        d.parse_status = domain::ParseStatus::Processing;
    }
    if let Err(error) = enrichment::process_image_without_decr(
        &mut store,
        document_id,
        image_key,
        image_source_type,
        enable_ocr,
        enable_caption,
    ) {
        tracing::warn!(
            document_id = %document_id,
            image_key = truncate_key(image_key),
            error = %error,
            "parse image fail"
        );
        return Err(error);
    }
    tracing::info!(
        document_id = %document_id,
        image_key = truncate_key(image_key),
        ocr = enable_ocr,
        caption = enable_caption,
        "parse image done"
    );
    let image_chunks: Vec<_> = store
        .chunks
        .values()
        .filter(|c| {
            c.document_id == document_id
                && c.context_header == image_key
                && matches!(c.chunk_type.as_str(), "image_ocr" | "image_caption")
        })
        .cloned()
        .collect();
    let ids: std::collections::HashSet<_> = image_chunks.iter().map(|c| c.id).collect();
    let embeddings: Vec<_> = store
        .embeddings
        .values()
        .filter(|e| ids.contains(&e.chunk_id))
        .cloned()
        .collect();
    storage::delete_image_chunks(pool, document_id, image_key)
        .await
        .map_err(|e| e.to_string())?;
    storage::insert_document_chunks(pool, &image_chunks, &embeddings)
        .await
        .map_err(|e| e.to_string())?;
    if enrichment::decr_pending(&mut store, document_id) {
        let _ = storage::set_index_ready(pool, document_id, true).await;
        let vid = store
            .documents
            .get(&document_id)
            .map(|d| d.product_version_id)
            .unwrap_or_default();
        let tracked = storage::list_spans_attempt(pool, document_id, attempt)
            .await
            .unwrap_or_default()
            .iter()
            .any(|r| r.name == obs::SPAN_MULTIMODAL);
        if tracked {
            let _ = storage::finish_span(
                pool,
                document_id,
                attempt,
                obs::SPAN_MULTIMODAL,
                obs::STATUS_DONE,
                None,
            )
            .await;
            maybe_start_postprocess(pool, document_id, vid, attempt).await;
        } else {
            let _ = runtime::enqueue_post_process(document_id, vid, false).await;
        }
    }
    Ok(())
}

/// Last-retry DECR so a dead image cannot pin `multimodal:pending`.
async fn finalize_multimodal_pg(pool: &PgPool, document_id: Uuid, attempt: i32) {
    let current: Option<i32> = sqlx::query_scalar("SELECT attempt FROM documents WHERE id = $1")
        .bind(document_id)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();
    if current.is_some_and(|n| n != attempt) {
        return;
    }
    let mut tmp = domain::Store::default();
    if !enrichment::decr_pending(&mut tmp, document_id) {
        return;
    }
    let vid: Option<Uuid> =
        sqlx::query_scalar("SELECT product_version_id FROM documents WHERE id = $1")
            .bind(document_id)
            .fetch_optional(pool)
            .await
            .ok()
            .flatten();
    let Some(vid) = vid else {
        return;
    };
    maybe_start_postprocess(pool, document_id, vid, attempt).await;
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
        runtime::WIKI_LOCK_RETRY_SECS
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

/// Brain ProcessWikiIngest on PG `task_pending_ops` ingest lane only.
pub async fn process_wiki_ingest(pool: &PgPool, version_id: Uuid) -> Result<(), String> {
    if !storage::version_wiki_enabled(pool, version_id)
        .await
        .map_err(|e| e.to_string())?
    {
        tracing::info!(%version_id, "wiki ingest skipped: not enabled");
        let _ = storage::drop_pending_ops(pool, domain::TYPE_WIKI_INGEST, version_id).await;
        return schedule_semantic_index_v2_if_ready(pool, version_id).await;
    }
    let claimed = storage::claim_pending_batch(
        pool,
        domain::TYPE_WIKI_INGEST,
        version_id,
        wiki::BATCH_DOCS as i64,
        wiki::STALE_CLAIM_MIN as i64,
    )
    .await
    .map_err(|e| e.to_string())?;
    if claimed.is_empty() {
        return schedule_semantic_index_v2_if_ready(pool, version_id).await;
    }
    let mut store = domain::Store::default();
    if let Ok(Some(ws)) = storage::document_workspace_id(
        pool,
        claimed
            .iter()
            .find_map(|o| o.dedup_key.as_deref().and_then(|s| Uuid::parse_str(s).ok()))
            .unwrap_or(version_id),
    )
    .await
    {
        let _ = storage::hydrate_workspace(pool, &mut store, ws).await;
    }
    store.versions.entry(version_id).or_insert_with(|| {
        let mut v = domain::ProductVersion::new(Uuid::nil(), "v".into());
        v.id = version_id;
        v.wiki_enabled = true;
        v
    });
    let mut done = Vec::new();
    let mut slugs = Vec::new();
    let mut ingest_ops = Vec::new();
    for op in &claimed {
        if op.op == wiki::OP_RETRACT {
            if let Some(did) = op
                .dedup_key
                .as_deref()
                .and_then(|s| Uuid::parse_str(s).ok())
            {
                wiki::enqueue_retract(&mut store, version_id, did, "");
                let _ = storage::delete_wiki_for_document(pool, version_id, did).await;
            }
            done.push(op.id);
            continue;
        }
        let Some(did) = op
            .dedup_key
            .as_deref()
            .and_then(|s| Uuid::parse_str(s).ok())
        else {
            done.push(op.id);
            continue;
        };
        store.documents.entry(did).or_insert_with(|| {
            let mut doc = domain::Document::new(
                version_id,
                did.to_string(),
                "doc.txt".into(),
                0,
                String::new(),
                String::new(),
            );
            doc.id = did;
            doc
        });
        wiki::enqueue_ingest(&mut store, version_id, did);
        wiki::set_ingest_fail_count(&mut store, version_id, did, op.fail_count);
        ingest_ops.push((op.id, did));
    }
    if !ingest_ops.is_empty() {
        if let Err(e) = wiki::process_ingest(&mut store, version_id) {
            for (_, did) in &ingest_ops {
                let _ = storage::upsert_span(
                    pool,
                    *did,
                    1,
                    "wiki.ingest",
                    "failed",
                    Some(serde_json::json!({"error": e})),
                )
                .await;
            }
            for (op_id, did) in &ingest_ops {
                if let Some(n) = wiki::retryable_ingest_fail_count(&store, version_id, *did) {
                    storage::retry_pending_op(pool, *op_id, n)
                        .await
                        .map_err(|e| e.to_string())?;
                } else {
                    storage::finalize_subtask(pool, *did)
                        .await
                        .map_err(|e| e.to_string())?;
                    done.push(*op_id);
                }
            }
        } else {
            persist_wiki_store(pool, &store, version_id, None).await?;
            for (op_id, did) in &ingest_ops {
                if let Some(n) = wiki::retryable_ingest_fail_count(&store, version_id, *did) {
                    storage::retry_pending_op(pool, *op_id, n)
                        .await
                        .map_err(|e| e.to_string())?;
                    continue;
                }
                storage::finalize_subtask(pool, *did)
                    .await
                    .map_err(|e| e.to_string())?;
                let _ = storage::upsert_span(
                    pool,
                    *did,
                    1,
                    "wiki.ingest",
                    "done",
                    Some(serde_json::json!({"version_id": version_id})),
                )
                .await;
                slugs.push(*did);
                done.push(*op_id);
            }
        }
    }
    storage::delete_pending_ids(pool, &done)
        .await
        .map_err(|e| e.to_string())?;
    for did in slugs {
        storage::enqueue_pending_op(
            pool,
            domain::TYPE_WIKI_FINALIZE,
            version_id,
            wiki::OP_SLUG,
            Some(&did.to_string()),
            serde_json::json!({"document_id": did}),
        )
        .await
        .map_err(|e| e.to_string())?;
        let _ = storage::enqueue_pending_op(
            pool,
            domain::TYPE_WIKI_FINALIZE,
            version_id,
            wiki::OP_CHANGE,
            None,
            serde_json::json!({"document_id": did}),
        )
        .await;
        let _ = storage::enqueue_pending_op(
            pool,
            domain::TYPE_WIKI_FINALIZE,
            version_id,
            wiki::OP_FOLDER_PRUNE,
            None,
            serde_json::json!({}),
        )
        .await;
    }
    if storage::count_pending(pool, domain::TYPE_WIKI_FINALIZE, version_id)
        .await
        .map_err(|e| e.to_string())?
        > 0
    {
        let _ = runtime::enqueue_wiki_finalize(version_id).await;
    }
    if storage::count_pending(pool, domain::TYPE_WIKI_INGEST, version_id)
        .await
        .map_err(|e| e.to_string())?
        > 0
    {
        let delay = if done.len() < claimed.len() {
            wiki::LOCK_RETRY_SECS
        } else {
            wiki::FOLLOW_UP_DEBOUNCE_SECS
        };
        let _ = runtime::enqueue_wiki_ingest_in(version_id, delay).await;
    }
    schedule_semantic_index_v2_if_ready(pool, version_id).await
}

/// Brain ProcessWikiFinalize — finalize lane only, never ingest.
pub async fn process_wiki_finalize(pool: &PgPool, version_id: Uuid) -> Result<(), String> {
    let claimed = storage::claim_pending_batch(
        pool,
        domain::TYPE_WIKI_FINALIZE,
        version_id,
        5000,
        wiki::STALE_CLAIM_MIN as i64,
    )
    .await
    .map_err(|e| e.to_string())?;
    if claimed.is_empty() {
        return schedule_semantic_index_v2_if_ready(pool, version_id).await;
    }
    let mut store = domain::Store::default();
    if let Ok(Some(ws)) = storage::version_workspace_id(pool, version_id).await {
        let _ = storage::hydrate_workspace(pool, &mut store, ws).await;
    }
    store.versions.entry(version_id).or_insert_with(|| {
        let mut v = domain::ProductVersion::new(Uuid::nil(), "v".into());
        v.id = version_id;
        v.wiki_enabled = true;
        v
    });
    for op in &claimed {
        let slug = op.dedup_key.clone().unwrap_or_default();
        let title = op
            .payload
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        wiki::enqueue_finalize_op(&mut store, version_id, &op.op, &slug, &title);
    }
    wiki::process_finalize(&mut store, version_id)?;
    persist_wiki_store(pool, &store, version_id, None).await?;
    let deferred: Vec<Uuid> = claimed
        .iter()
        .filter(|op| {
            store.wiki_ops.iter().any(|o| {
                o.lane == domain::TYPE_WIKI_FINALIZE
                    && o.version_id == version_id
                    && o.op == op.op
                    && (op.dedup_key.as_deref().unwrap_or("").is_empty()
                        || o.slug == op.dedup_key.as_deref().unwrap_or_default())
            })
        })
        .map(|op| op.id)
        .collect();
    let done: Vec<Uuid> = claimed
        .iter()
        .map(|op| op.id)
        .filter(|id| !deferred.contains(id))
        .collect();
    storage::delete_pending_ids(pool, &done)
        .await
        .map_err(|e| e.to_string())?;
    storage::unclaim_pending_ids(pool, &deferred)
        .await
        .map_err(|e| e.to_string())?;
    if !deferred.is_empty() {
        let _ = runtime::enqueue_wiki_finalize_in(version_id, wiki::LOCK_RETRY_SECS).await;
    }
    schedule_semantic_index_v2_if_ready(pool, version_id).await
}

async fn persist_wiki_store(
    pool: &PgPool,
    store: &domain::Store,
    version_id: Uuid,
    document_id: Option<Uuid>,
) -> Result<(), String> {
    for page in store
        .wiki
        .values()
        .filter(|p| p.product_version_id == version_id)
    {
        let _ = storage::upsert_wiki_page(pool, page, document_id).await;
    }
    for folder in store
        .wiki_folders
        .values()
        .filter(|f| f.product_version_id == version_id)
    {
        let _ = storage::upsert_wiki_folder(pool, folder).await;
    }
    let owner = match document_id {
        Some(id) => Some(id),
        None => store
            .documents
            .values()
            .find(|d| d.product_version_id == version_id)
            .map(|d| d.id),
    };
    let mut wiki_chunks: Vec<_> = store
        .chunks
        .values()
        .filter(|c| {
            c.product_version_id == version_id
                && c.chunk_type == "wiki_page"
                && document_id.is_none_or(|did| c.document_id == did || c.document_id.is_nil())
        })
        .cloned()
        .collect();
    if let Some(oid) = owner {
        for c in &mut wiki_chunks {
            if c.document_id.is_nil() {
                c.document_id = oid;
            }
        }
    }
    wiki_chunks.retain(|c| !c.document_id.is_nil());
    if wiki_chunks.is_empty() {
        return Ok(());
    }
    let slugs: Vec<String> = wiki_chunks
        .iter()
        .map(|c| c.context_header.clone())
        .collect();
    let owner_ids: std::collections::HashSet<_> =
        wiki_chunks.iter().map(|c| c.document_id).collect();
    let wiki_emb: Vec<_> = wiki_chunks
        .iter()
        .filter_map(|c| {
            let mut e = store.embeddings.get(&c.id).cloned()?;
            if e.document_id.is_nil() && owner_ids.contains(&c.document_id) {
                e.document_id = c.document_id;
            }
            Some(e)
        })
        .collect();
    storage::replace_wiki_page_chunks(pool, version_id, &slugs, &wiki_chunks, &wiki_emb)
        .await
        .map_err(|e| e.to_string())
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
    if storage::document_parse_status(pool, document_id)
        .await
        .map_err(|error| error.to_string())?
        .as_deref()
        == Some("completed")
    {
        return schedule_semantic_index_for_document_v2(pool, document_id).await;
    }
    let ws = storage::document_workspace_id(pool, document_id)
        .await
        .map_err(|e| e.to_string())?;
    let Some(ws) = ws else {
        return Ok(());
    };
    let mut store = domain::Store::default();
    storage::hydrate_workspace(pool, &mut store, ws)
        .await
        .map_err(|e| e.to_string())?;
    let outcome = enrichment::generate_summary_with(&mut store, document_id, attempt, fallback)?;
    if matches!(outcome, enrichment::SummaryOutcome::Superseded) {
        return Ok(());
    }
    storage::persist_summary_chunks(pool, &store, document_id)
        .await
        .map_err(|e| e.to_string())?;
    if let Some(d) = store.documents.get(&document_id) {
        let _ = storage::set_document_description(pool, document_id, &d.description).await;
        let st = match d.summary_status {
            domain::SummaryStatus::Completed => "completed",
            domain::SummaryStatus::Failed => "failed",
            domain::SummaryStatus::Pending => "pending",
            domain::SummaryStatus::Processing => "processing",
            domain::SummaryStatus::None => "none",
        };
        let _ = storage::set_summary_status(pool, document_id, st).await;
    }
    storage::finalize_subtask(pool, document_id)
        .await
        .map_err(|e| e.to_string())?;
    schedule_semantic_index_for_document_v2(pool, document_id).await
}

pub async fn process_questions_pg(
    pool: &PgPool,
    document_id: Uuid,
    chunk_ids: &[Uuid],
    prev_ids: &[Option<Uuid>],
    next_ids: &[Option<Uuid>],
    attempt: i32,
) -> Result<(), String> {
    if storage::document_parse_status(pool, document_id)
        .await
        .map_err(|error| error.to_string())?
        .as_deref()
        == Some("completed")
    {
        return schedule_semantic_index_for_document_v2(pool, document_id).await;
    }
    let ws = storage::document_workspace_id(pool, document_id)
        .await
        .map_err(|e| e.to_string())?;
    let Some(ws) = ws else {
        return Ok(());
    };
    let mut store = domain::Store::default();
    storage::hydrate_workspace(pool, &mut store, ws)
        .await
        .map_err(|e| e.to_string())?;
    let outcome = enrichment::generate_questions_with(
        &mut store,
        chunk_ids,
        prev_ids,
        next_ids,
        document_id,
        attempt,
    )?;
    if matches!(outcome, enrichment::QuestionOutcome::Superseded) {
        return Ok(());
    }
    storage::persist_question_updates(pool, &store, document_id, chunk_ids)
        .await
        .map_err(|e| e.to_string())?;
    storage::finalize_subtask(pool, document_id)
        .await
        .map_err(|e| e.to_string())?;
    schedule_semantic_index_for_document_v2(pool, document_id).await
}

pub async fn process_extract_pg(
    pool: &PgPool,
    chunk_id: Uuid,
    document_id: Uuid,
    attempt: i32,
) -> Result<(), String> {
    if storage::document_parse_status(pool, document_id)
        .await
        .map_err(|error| error.to_string())?
        .as_deref()
        == Some("completed")
    {
        return schedule_semantic_index_for_document_v2(pool, document_id).await;
    }
    let ws = storage::document_workspace_id(pool, document_id)
        .await
        .map_err(|e| e.to_string())?;
    let Some(ws) = ws else {
        return Ok(());
    };
    let mut store = domain::Store::default();
    storage::hydrate_workspace(pool, &mut store, ws)
        .await
        .map_err(|e| e.to_string())?;
    let outcome = graph::extract_chunk_for_attempt(&mut store, chunk_id, document_id, attempt)?;
    if matches!(outcome, graph::ExtractOutcome::Superseded) {
        return Ok(());
    }
    storage::persist_graph_for_document(pool, &store, document_id)
        .await
        .map_err(|e| e.to_string())?;
    let _ = graph::sync_document(&store, document_id);
    storage::finalize_subtask(pool, document_id)
        .await
        .map_err(|e| e.to_string())?;
    schedule_semantic_index_for_document_v2(pool, document_id).await
}

pub async fn process_list_delete_pg(pool: &PgPool, document_id: Uuid) -> Result<(), String> {
    let status = storage::document_parse_status(pool, document_id)
        .await
        .map_err(|e| e.to_string())?;
    if status.as_deref() != Some("deleting") {
        return Ok(());
    }
    let ws = storage::document_workspace_id(pool, document_id)
        .await
        .map_err(|e| e.to_string())?;
    let vid: Option<Uuid> =
        sqlx::query_scalar("SELECT product_version_id FROM documents WHERE id = $1")
            .bind(document_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?;
    if let Some(vid) = vid {
        let _ = storage::delete_wiki_for_document(pool, vid, document_id).await;
        let _ = graph::delete_document(vid, document_id);
    }
    let _ = storage::purge_document_index(pool, document_id).await;
    let _ = runtime::enqueue_index_delete(document_id).await;
    storage::release_knowledge_document_object(
        pool,
        document_id,
        "system:knowledge-document-delete",
        &format!("knowledge-document-delete:{document_id}"),
    )
    .await
    .map_err(|e| e.to_string())?;
    if let Some(ws) = ws {
        let mut store = domain::Store::default();
        let _ = storage::hydrate_workspace(pool, &mut store, ws).await;
        crate::delete_document(&mut store, document_id);
    }
    Ok(())
}

pub async fn process_kb_delete_pg(pool: &PgPool, product_version_id: Uuid) -> Result<(), String> {
    let _ = storage::cancel_active_docs_for_versions(pool, &[product_version_id]).await;
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
        let _ = storage::delete_empty_product(pool, pid).await;
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
    storage::purge_document_index(pool, document_id)
        .await
        .map_err(|e| e.to_string())?;
    let _ = runtime::enqueue_index_delete(document_id).await;
    if let Some(vid) = vid {
        let _ = storage::delete_wiki_for_document(pool, vid, document_id).await;
        let _ = graph::delete_document(vid, document_id);
        sqlx::query("DELETE FROM task_pending_ops WHERE dedup_key = $1")
            .bind(document_id.to_string())
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
    }
    let attempt = storage::bump_document_attempt(pool, document_id)
        .await
        .map_err(|e| e.to_string())?;
    let _ = storage::open_attempt(pool, document_id, attempt).await;
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
                runtime::enqueue_document_process_with(document_id, vid, attempt, passages).await;
        }
        Some((kind, _)) if kind == "manual" => {
            let _ = runtime::enqueue_manual_process(document_id, vid, attempt).await;
        }
        _ => {
            let _ = runtime::enqueue_document_process(document_id, vid, attempt).await;
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
        let _ = runtime::enqueue_datatable(document_id).await;
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
            let _ = storage::finalize_subtask(pool, job.document_id).await;
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
            let _ = storage::finalize_subtask(pool, job.document_id).await;
            let _ = schedule_semantic_index_for_document_v2(pool, job.document_id).await;
        }
        result.map_err(JobErr)
    }
}

simple_worker!(
    DatatableWorker,
    DatatableJob,
    |pool: PgPool, job: DatatableJob| async move {
        let ws = storage::document_workspace_id(&pool, job.document_id)
            .await
            .map_err(|e| e.to_string())?;
        let Some(ws) = ws else {
            return Ok(());
        };
        let mut store = domain::Store::default();
        storage::hydrate_workspace(&pool, &mut store, ws)
            .await
            .map_err(|e| e.to_string())?;
        crate::handle(
            &mut store,
            &domain::Job {
                id: Uuid::new_v4(),
                task_type: domain::TYPE_DATATABLE.into(),
                queue: domain::QUEUE_SUMMARY.into(),
                payload: serde_json::json!({"document_id": job.document_id}),
                retries: 0,
                max_retry: 3,
            },
        )?;
        let table: Vec<_> = store
            .chunks
            .values()
            .filter(|c| {
                c.document_id == job.document_id
                    && matches!(c.chunk_type.as_str(), "table_summary" | "table_column")
            })
            .cloned()
            .collect();
        let embeds: Vec<_> = table
            .iter()
            .filter_map(|c| store.embeddings.get(&c.id).cloned())
            .collect();
        storage::delete_chunks_by_types(&pool, job.document_id, &["table_summary", "table_column"])
            .await
            .map_err(|e| e.to_string())?;
        storage::append_document_chunks(&pool, &table, &embeds)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
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
        storage::purge_document_index(&pool, job.document_id)
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
        let storage = runtime::connect().map_err(|e| e.to_string())?;
        storage
            .runtime(ctx.clone())
            .queue_with_concurrency::<DefaultQueue>(runtime::runtime_concurrency("CORE", 8))
            .worker::<DocumentProcessWorker, DocumentProcessJob>()
            .queue_with_concurrency::<BidDeliveryV1Queue>(runtime::runtime_concurrency(
                "BID_DELIVERY",
                4,
            ))
            .worker::<BidDeliveryV1Worker<BidDeliveryHandler>, BidDeliveryV1Job>()
            .shutdown_on(shut(stop_rx.clone()))
            .shutdown_timeout(timeout)
            .run()
    };
    let post = {
        let storage = runtime::connect().map_err(|e| e.to_string())?;
        storage
            .runtime(ctx.clone())
            .queue_with_concurrency::<PostprocessQueue>(runtime::runtime_concurrency(
                "POSTPROCESS",
                2,
            ))
            .worker::<PostProcessWorker, PostProcessJob>()
            .worker::<KnowledgeSemanticIndexV2Worker, KnowledgeSemanticIndexV2Job>()
            .shutdown_on(shut(stop_rx.clone()))
            .shutdown_timeout(timeout)
            .run()
    };
    let enrich_n = runtime::runtime_concurrency("ENRICHMENT", 12);
    let enrich = {
        let storage = runtime::connect().map_err(|e| e.to_string())?;
        storage
            .runtime(ctx.clone())
            .queue_with_concurrency::<SummaryQueue>(enrich_n)
            .worker::<SummaryWorker, SummaryJob>()
            .worker::<DatatableWorker, DatatableJob>()
            .shutdown_on(shut(stop_rx.clone()))
            .shutdown_timeout(timeout)
            .run()
    };
    let maint_n = runtime::runtime_concurrency("MAINTENANCE", 4);
    let maint = {
        let storage = runtime::connect().map_err(|e| e.to_string())?;
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
    let shared_n = runtime::runtime_concurrency("SHARED", 6);
    let shared = {
        let storage = runtime::connect().map_err(|e| e.to_string())?;
        storage
            .runtime(ctx.clone())
            .queue_with_concurrency::<SummaryQueue>(shared_n)
            .worker::<SummaryWorker, SummaryJob>()
            .shutdown_on(shut(stop_rx.clone()))
            .shutdown_timeout(timeout)
            .run()
    };
    let wiki_rt = {
        let storage = runtime::connect().map_err(|e| e.to_string())?;
        storage
            .runtime(ctx)
            .queue_with_concurrency::<WikiQueue>(runtime::runtime_concurrency("WIKI", 8))
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
    use std::{
        collections::VecDeque,
        sync::atomic::{AtomicUsize, Ordering},
    };
    use storage::{
        apply_fresh_baseline, connect, create_workspace_with_library, insert_document, insert_user,
        write_blob,
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
    impl storage::knowledge_index_v2::VectorEmbeddingProviderV2 for LifecycleProvider {
        async fn embed_batch(
            &self,
            _revision: &domain::knowledge_retrieval::EmbeddingRevisionV2,
            _credential_ref: &str,
            inputs: &[storage::knowledge_index_v2::VectorEmbeddingInputV2],
        ) -> Result<Vec<Vec<f32>>, storage::knowledge_index_v2::VectorIndexErrorV2> {
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
                    storage::knowledge_index_v2::VectorIndexErrorV2::Unavailable(
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
    impl storage::knowledge_index_v2::VectorEmbeddingProviderV2 for PendingAfterLifecycleProvider {
        async fn embed_batch(
            &self,
            _revision: &domain::knowledge_retrieval::EmbeddingRevisionV2,
            _credential_ref: &str,
            inputs: &[storage::knowledge_index_v2::VectorEmbeddingInputV2],
        ) -> Result<Vec<Vec<f32>>, storage::knowledge_index_v2::VectorIndexErrorV2> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            sqlx::query("UPDATE documents SET pending_subtasks_count=1 WHERE id=$1")
                .bind(self.document_id)
                .execute(&self.pool)
                .await
                .map_err(storage::knowledge_index_v2::VectorIndexErrorV2::Database)?;
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
    impl storage::knowledge_index_v2::EmbeddingCredentialResolverV2 for MissingLifecycleCredential {
        async fn resolve(
            &self,
            _credential_ref: &str,
        ) -> Result<String, storage::knowledge_index_v2::VectorIndexErrorV2> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Err(
                storage::knowledge_index_v2::VectorIndexErrorV2::InvalidConfiguration(
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

    #[tokio::test]
    async fn bid_delivery_partial_enqueue_replays_identical_children_on_second_attempt() {
        let targets = vec![
            (BidDeliveryTargetKind::MatchingJob, Uuid::new_v4(), 7),
            (BidDeliveryTargetKind::MatchingJob, Uuid::new_v4(), 7),
        ];
        let mut first_calls = Vec::new();
        let mut call_ordinal = 0_usize;
        let first = enqueue_bid_targets_with(targets.clone(), |kind, id, revision| {
            first_calls.push((kind, id, revision));
            call_ordinal += 1;
            std::future::ready(if call_ordinal == 1 {
                runtime::BidDeliveryEnqueueOutcome::Accepted {
                    job_id: format!("{}:{id}:{revision}", kind.as_str()),
                }
            } else {
                runtime::BidDeliveryEnqueueOutcome::Indeterminate {
                    error: "injected Redis failure".into(),
                }
            })
        })
        .await;
        assert!(first.is_err(), "partial enqueue must retry the parent job");
        assert_eq!(first_calls, targets);

        let mut retry_calls = Vec::new();
        enqueue_bid_targets_with(targets.clone(), |kind, id, revision| {
            retry_calls.push((kind, id, revision));
            std::future::ready(runtime::BidDeliveryEnqueueOutcome::Accepted {
                job_id: format!("{}:{id}:{revision}", kind.as_str()),
            })
        })
        .await
        .expect("the parent retry replays both deterministic child identities");
        assert_eq!(retry_calls, targets);
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

    #[test]
    fn submission_render_failure_classification_is_deterministic() {
        let cases = [
            (
                "database: SUBMISSION_END_STATE_CHANGED",
                "SUBMISSION_END_STATE_CHANGED",
                false,
            ),
            ("invalid manifest format", "SUBMISSION_RENDER_FAILED", false),
            (
                "manifest asset MIME is not a supported image: image/tiff",
                "SUBMISSION_RENDER_FAILED",
                false,
            ),
            (
                "permission denied for function kb_bid_manifest_render_input",
                "SUBMISSION_RENDER_FAILED",
                false,
            ),
            (
                "submission renderer is not configured",
                "SUBMISSION_RENDER_FAILED",
                false,
            ),
            (
                "invalid manifest timeout configuration",
                "SUBMISSION_RENDER_FAILED",
                false,
            ),
            (
                "permission denied for object store: service unavailable",
                "SUBMISSION_RENDER_FAILED",
                false,
            ),
            (
                "manifest asset temporary signature mismatch",
                "SUBMISSION_RENDER_FAILED",
                false,
            ),
            (
                "temporary object store timeout",
                "SUBMISSION_RENDER_FAILED",
                true,
            ),
            (
                "reqwest request failed: connection refused",
                "SUBMISSION_RENDER_FAILED",
                true,
            ),
            (
                "s3 PUT /bucket/output -> 503",
                "SUBMISSION_RENDER_FAILED",
                true,
            ),
            (
                "s3 PUT /bucket/output -> 403",
                "SUBMISSION_RENDER_FAILED",
                false,
            ),
            (
                "database error: deadlock detected",
                "SUBMISSION_RENDER_FAILED",
                true,
            ),
        ];

        for (detail, expected_code, expected_retryable) in cases {
            let error_code = submission_render_error_code(detail);
            assert_eq!(error_code, expected_code, "detail: {detail}");
            assert_eq!(
                submission_render_error_retryable(error_code, detail),
                expected_retryable,
                "detail: {detail}"
            );
        }
    }

    #[test]
    fn attachment_preparation_failure_classification_is_deterministic() {
        let cases = [
            (
                "ATTACHMENT_SOURCE_BYTES_MISSING: object store unavailable",
                "ATTACHMENT_SOURCE_BYTES_MISSING",
                true,
            ),
            (
                "ATTACHMENT_SOURCE_READ_TASK_FAILED: cancelled",
                "ATTACHMENT_SOURCE_READ_TASK_FAILED",
                true,
            ),
            (
                "ATTACHMENT_SOURCE_INVALID: PDF_STRUCTURE_INVALID",
                "ATTACHMENT_SOURCE_INVALID",
                false,
            ),
            (
                "ATTACHMENT_SOURCE_IDENTITY_MISMATCH",
                "ATTACHMENT_SOURCE_IDENTITY_MISMATCH",
                false,
            ),
            (
                "ATTACHMENT_PDF_RENDER_FAILED: invalid xref",
                "ATTACHMENT_PDF_RENDER_FAILED",
                false,
            ),
            (
                "ATTACHMENT_PREPARATION_PUBLISH_FAILED: ATTACHMENT_RENDER_PAGE_SET_INVALID",
                "ATTACHMENT_RENDER_PAGE_SET_INVALID",
                false,
            ),
            (
                "ATTACHMENT_RENDER_PAGE_INVALID: image/tiff",
                "ATTACHMENT_RENDER_PAGE_INVALID",
                false,
            ),
            (
                "ATTACHMENT_RENDER_PAGE_TASK_FAILED: cancelled",
                "ATTACHMENT_RENDER_PAGE_TASK_FAILED",
                true,
            ),
            (
                "ATTACHMENT_RENDER_PAGE_QUOTA_EXCEEDED",
                "ATTACHMENT_RENDER_PAGE_QUOTA_EXCEEDED",
                false,
            ),
            (
                "ATTACHMENT_RENDER_PAGE_STAGE_FAILED: database unavailable",
                "ATTACHMENT_RENDER_PAGE_STAGE_FAILED",
                true,
            ),
            (
                "ATTACHMENT_RENDER_PAGE_WRITE_FAILED: object store unavailable",
                "ATTACHMENT_RENDER_PAGE_WRITE_FAILED",
                true,
            ),
            (
                "ATTACHMENT_PREPARATION_PUBLISH_FAILED: ATTACHMENT_PREPARATION_CLAIM_LOST",
                "ATTACHMENT_PREPARATION_CLAIM_LOST",
                false,
            ),
            (
                "ATTACHMENT_PREPARATION_PUBLISH_FAILED: deadlock detected",
                "ATTACHMENT_PREPARATION_PUBLISH_FAILED",
                true,
            ),
            (
                "unexpected attachment preparation failure",
                "ATTACHMENT_PREPARATION_FAILED",
                true,
            ),
        ];

        for (detail, expected_code, expected_retryable) in cases {
            let error_code = attachment_preparation_error_code(detail);
            assert_eq!(error_code, expected_code, "detail: {detail}");
            assert_eq!(
                attachment_preparation_error_retryable(error_code),
                expected_retryable,
                "detail: {detail}"
            );
        }
    }

    #[test]
    fn attachment_preparation_worker_matches_durable_contract() {
        let worker = BidDeliveryV1Worker::new(BidDeliveryHandler { pool: None });
        let job = BidDeliveryV1Job::new(
            BidDeliveryTargetKind::AttachmentPreparation,
            Uuid::from_u128(1),
            1,
        );

        assert_eq!(
            ATTACHMENT_PREPARATION_ACTOR,
            "system:bid-attachment-preparation"
        );
        assert_eq!(
            oxana::Worker::<BidDeliveryV1Job>::max_retries(&worker, &job),
            3,
            "one initial attempt plus three queue retries must match four durable attempts"
        );
    }

    async fn db_lock() -> tokio::sync::MutexGuard<'static, ()> {
        static LOCK: Mutex<()> = Mutex::const_new(());
        LOCK.lock().await
    }

    #[test]
    fn wiki_ingest_retry_delay_is_lock_retry() {
        let w = WikiIngestWorker { pool: None };
        let job = WikiIngestJob {
            product_version_id: Uuid::new_v4(),
            task_type: domain::TYPE_WIKI_INGEST.to_string(),
        };
        assert_eq!(
            oxana::Worker::<WikiIngestJob>::retry_delay(&w, &job, 0),
            runtime::WIKI_LOCK_RETRY_SECS
        );
        assert_eq!(
            oxana::Worker::<WikiIngestJob>::retry_delay(&w, &job, 4),
            runtime::WIKI_LOCK_RETRY_SECS
        );
        assert_eq!(
            wiki::INGEST_DEBOUNCE_SECS,
            runtime::WIKI_INGEST_DEBOUNCE_SECS
        );
        assert_eq!(
            wiki::FINALIZE_DEBOUNCE_SECS,
            runtime::WIKI_FINALIZE_DEBOUNCE_SECS
        );
        assert_eq!(
            wiki::FOLLOW_UP_DEBOUNCE_SECS,
            runtime::WIKI_FOLLOW_UP_DEBOUNCE_SECS
        );
        assert_eq!(wiki::LOCK_RETRY_SECS, runtime::WIKI_LOCK_RETRY_SECS);
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
        let hash = domain::sha256_hex(b"keep");
        write_blob(&hash, b"keep").unwrap();
        insert_document(
            &pool,
            storage::NewDocument {
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
            storage::NewDocument {
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
        let blank = domain::Chunk {
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
        let hash = domain::sha256_hex(b"body");
        write_blob(&hash, b"body").unwrap();
        insert_document(
            &pool,
            storage::NewDocument {
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
        let ch = domain::Chunk {
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
        let hash = domain::sha256_hex(b"hello reuse");
        write_blob(&hash, b"hello reuse").unwrap();
        insert_document(
            &pool,
            storage::NewDocument {
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
        let hash = domain::sha256_hex(b"hello worker");
        write_blob(&hash, b"hello worker").unwrap();
        insert_document(
            &pool,
            storage::NewDocument {
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
        assert!(storage::blob_exists(&format!("{hash}.md")));
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
        let hash = domain::sha256_hex(b"%PDF-1.4");
        write_blob(&hash, b"%PDF-1.4").unwrap();
        insert_document(
            &pool,
            storage::NewDocument {
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
        let hash = domain::sha256_hex(body);
        write_blob(&hash, body).unwrap();
        insert_document(
            &pool,
            storage::NewDocument {
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
        if domain::vlm_configured() {
            let Ok(storage) = runtime::connect() else {
                eprintln!("skip: redis down");
                return;
            };
            let n = storage
                .enqueued_count(runtime::MultimodalQueue)
                .await
                .unwrap();
            assert!(n >= 1, "image:multimodal must be enqueued, got {n}");
            assert_eq!(enrichment::pending_count(did), Some(1));
            let _ = storage.wipe_queue(runtime::MultimodalQueue).await;
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
        let hash = domain::sha256_hex(b"RIFF");
        write_blob(&hash, b"RIFF").unwrap();
        insert_document(
            &pool,
            storage::NewDocument {
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
        let hash = domain::sha256_hex(bytes);
        write_blob(&hash, bytes).unwrap();
        insert_document(
            &pool,
            storage::NewDocument {
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
        let md = String::from_utf8(storage::read_blob(&format!("{hash}.md")).unwrap()).unwrap();
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
        let hash = domain::sha256_hex(b"unused");
        insert_document(
            &pool,
            storage::NewDocument {
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
        storage::set_document_source(
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
        let hash = domain::sha256_hex(body);
        write_blob(&hash, body).unwrap();
        insert_document(
            &pool,
            storage::NewDocument {
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
        let hash = domain::sha256_hex(b"wiki body");
        write_blob(&hash, b"wiki body").unwrap();
        insert_document(
            &pool,
            storage::NewDocument {
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
        storage::replace_document_chunks(
            &pool,
            did,
            &[domain::Chunk {
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
        storage::enqueue_pending_op(
            &pool,
            domain::TYPE_WIKI_INGEST,
            vid,
            wiki::OP_INGEST,
            Some(&did.to_string()),
            serde_json::json!({"document_id": did}),
        )
        .await
        .unwrap();
        storage::enqueue_pending_op(
            &pool,
            domain::TYPE_WIKI_FINALIZE,
            vid,
            wiki::OP_SLUG,
            Some("preexisting"),
            serde_json::json!({}),
        )
        .await
        .unwrap();
        process_wiki_ingest(&pool, vid).await.unwrap();
        let ingest_left = storage::count_pending(&pool, domain::TYPE_WIKI_INGEST, vid)
            .await
            .unwrap();
        let finalize_left = storage::count_pending(&pool, domain::TYPE_WIKI_FINALIZE, vid)
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
        let finalize_after = storage::count_pending(&pool, domain::TYPE_WIKI_FINALIZE, vid)
            .await
            .unwrap();
        assert_eq!(finalize_after, 0);
        let ingest_after = storage::count_pending(&pool, domain::TYPE_WIKI_INGEST, vid)
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
            storage::NewDocument {
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
        storage::insert_version_cloning(&pool, dst, seeded.library_id, "2026", src)
            .await
            .unwrap();
        process_version_clone(
            &pool,
            &VersionCloneJob {
                source_version_id: src,
                target_version_id: dst,
                diffs: serde_json::json!([]),
                make_current: false,
                task_type: domain::TYPE_VERSION_CLONE.into(),
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

    #[test]
    fn run_core_registers_the_single_bid_delivery_worker() {
        let src = include_str!("consume.rs")
            .split("\n#[cfg(test)]")
            .next()
            .expect("production worker source");
        assert!(
            src.contains(".worker::<BidDeliveryV1Worker<BidDeliveryHandler>, BidDeliveryV1Job>()")
        );
        assert!(!src.contains("run_bid_delivery_reconciler"));
        assert!(!src.contains("reserve_due_deliveries"));
        assert!(!src.contains("reap_expired_deliveries"));
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

    #[test]
    fn target_kind_mapping_covers_all_six_business_targets() {
        for (value, expected) in [
            (
                "document_conversion",
                BidDeliveryTargetKind::DocumentConversion,
            ),
            ("extraction_target", BidDeliveryTargetKind::ExtractionTarget),
            ("matching_schedule", BidDeliveryTargetKind::MatchingSchedule),
            ("matching_job", BidDeliveryTargetKind::MatchingJob),
            (
                "attachment_preparation",
                BidDeliveryTargetKind::AttachmentPreparation,
            ),
            ("submission_render", BidDeliveryTargetKind::SubmissionRender),
        ] {
            assert_eq!(value.parse::<BidDeliveryTargetKind>().unwrap(), expected);
        }
        assert!("unknown".parse::<BidDeliveryTargetKind>().is_err());
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
            storage::NewDocument {
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
        storage::replace_document_chunks(
            &pool,
            did,
            &[domain::Chunk {
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
            &[domain::ChunkEmbedding {
                chunk_id: cid,
                product_version_id: seeded.library_version_id,
                document_id: did,
                content: "throughput keep".into(),
                vector: vec![0.1; models::EMBEDDING_DIM],
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
            storage::count_pending(&pool, domain::TYPE_WIKI_INGEST, seeded.library_version_id)
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
            storage::NewDocument {
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
        storage::replace_document_chunks(
            &pool,
            did,
            &[domain::Chunk {
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
            &[domain::ChunkEmbedding {
                chunk_id: cid,
                product_version_id: seeded.library_version_id,
                document_id: did,
                content: body.into(),
                vector: vec![0.1; models::EMBEDDING_DIM],
                tsv: String::new(),
            }],
        )
        .await
        .unwrap();
        let image =
            process_image_pg(&pool, did, "images/p1.jpg", "scanned_pdf", true, true, 1).await;
        if domain::vlm_configured() {
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
        use domain::knowledge_retrieval::{
            EMBEDDING_DIMENSION_V2, EMBEDDING_OUTPUT_NORMALIZATION_VERSION_V2,
            EMBEDDING_PROVIDER_PROTOCOL_VERSION_V2, EMBEDDING_REVISION_SCHEMA_V2,
            EmbeddingRevisionV2,
        };
        use storage::knowledge_index_v2::SemanticIndexPreparationV2;

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
        let file_hash = domain::sha256_hex(document_id.as_bytes());
        let object_ref = format!("objects/{file_hash}");
        let revision = EmbeddingRevisionV2 {
            schema_version: EMBEDDING_REVISION_SCHEMA_V2,
            provider_protocol_version: EMBEDDING_PROVIDER_PROTOCOL_VERSION_V2.into(),
            provider_model_identifier: format!("lifecycle-v2-{version_id}@2025-01-15"),
            provider_model_revision_sha256: domain::sha256_hex(b"lifecycle-v2-model"),
            endpoint_config_sha256: domain::sha256_hex(b"lifecycle-v2-endpoint"),
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
            storage::knowledge_index_v2::prepare_semantic_index_intent_v2(&pool, version_id)
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

        let empty_intent =
            match storage::knowledge_index_v2::prepare_semantic_index_intent_v2(&pool, version_id)
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
        let empty_completed = storage::knowledge_index_v2::semantic_index_intent_v2(
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
            storage::knowledge_index_v2::prepare_semantic_index_intent_v2(&pool, version_id)
                .await
                .unwrap(),
            SemanticIndexPreparationV2::PendingDerived
        );
        sqlx::query("UPDATE documents SET parse_status='completed',pending_subtasks_count=0,summary_status='completed' WHERE id=$1")
            .bind(document_id).execute(&pool).await.unwrap();
        let source_intent =
            match storage::knowledge_index_v2::prepare_semantic_index_intent_v2(&pool, version_id)
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
            storage::document_parse_status(&pool, document_id)
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
        for _ in 0..=runtime::SEMANTIC_INDEX_V2_MAX_RETRY {
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
        let retryable = storage::knowledge_index_v2::semantic_index_intent_v2(
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
        let ready = storage::knowledge_index_v2::semantic_index_intent_v2(
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
        let aba_b =
            match storage::knowledge_index_v2::prepare_semantic_index_intent_v2(&pool, version_id)
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
        let aba_a =
            match storage::knowledge_index_v2::prepare_semantic_index_intent_v2(&pool, version_id)
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
        let aba_pending = storage::knowledge_index_v2::semantic_index_intent_v2(
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
        let stale_intent =
            match storage::knowledge_index_v2::prepare_semantic_index_intent_v2(&pool, version_id)
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
        let terminal_intent =
            match storage::knowledge_index_v2::prepare_semantic_index_intent_v2(&pool, version_id)
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
        let stale = storage::knowledge_index_v2::semantic_index_intent_v2(
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
        let strict = storage::knowledge_index_v2::StrictVectorEmbeddingClientV2::new(Arc::new(
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
        let terminal = storage::knowledge_index_v2::semantic_index_intent_v2(
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
        let revoked_intent =
            match storage::knowledge_index_v2::prepare_semantic_index_intent_v2(&pool, version_id)
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
        let revoked = storage::knowledge_index_v2::semantic_index_intent_v2(
            &pool,
            revoked_intent.id,
            revoked_intent.target_revision,
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(revoked.status, "terminal");
        assert_eq!(
            storage::knowledge_index_v2::prepare_semantic_index_intent_v2(&pool, version_id)
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

    #[tokio::test]
    async fn submission_staging_is_abandoned_when_the_render_future_is_cancelled() {
        let _g = db_lock().await;
        let pool = match connect().await {
            Ok(pool) => pool,
            Err(error)
                if std::env::var("KNOWLEDGEBRAIN_REQUIRE_POSTGRES_TESTS").as_deref() == Ok("1") =>
            {
                panic!("required PostgreSQL staging cleanup test unavailable: {error}")
            }
            Err(error) => {
                eprintln!("skip: postgres down: {error}");
                return;
            }
        };
        apply_fresh_baseline(&pool).await.unwrap();
        let staging_id = Uuid::new_v4();
        let actor = format!("user:{}", Uuid::new_v4());
        let bytes = b"cancelled submission output";
        let digest = domain::sha256_hex(bytes);
        let object_ref = storage::object_ref(&digest);
        let staging = SubmissionStagingGuard::new(pool.clone(), staging_id, &actor);
        let delayed_pool = pool.clone();
        let delayed_actor = actor.clone();
        let delayed_object_ref = object_ref.clone();
        let delayed_digest = digest.clone();
        let delayed_stage = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            storage::stage_object_upload(
                &delayed_pool,
                staging_id,
                &delayed_object_ref,
                &delayed_digest,
                "application/pdf",
                bytes.len() as i64,
                &delayed_actor,
            )
            .await
        });

        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let render = tokio::spawn(async move {
            let _staging = staging;
            let _ = started_tx.send(());
            std::future::pending::<()>().await;
        });
        started_rx.await.unwrap();
        render.abort();
        let _ = render.await;
        delayed_stage.await.unwrap().unwrap();

        let mut remaining = 1_i64;
        for _ in 0..100 {
            remaining =
                sqlx::query_scalar("SELECT count(*) FROM object_upload_staging WHERE id=$1")
                    .bind(staging_id)
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            if remaining == 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert_eq!(
            remaining, 0,
            "retry must abandon staging committed after lease cancellation"
        );
    }
}
