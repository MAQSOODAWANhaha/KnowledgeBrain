//! oxana `default` consumer: convert only (ticket 09).

use async_trait::async_trait;
use runtime::{
    BidConvertJob, BidConvertV1Queue, BidExtractJob, BidExtractV1Queue, BidMatchRouteV1Job,
    BidMatchingV1Queue, BidSectionRetryJob, BidSectionRetryV1Queue, DatatableJob, DefaultQueue,
    DocumentProcessJob, ExtractJob, HousekeepJob, ImageMultimodalJob, IndexDeleteJob, KbDeleteJob,
    ListDeleteJob, ListReparseJob, LowQueue, PostProcessJob, PostprocessQueue, QuestionJob,
    SummaryJob, SummaryQueue, VersionCloneJob, WikiFinalizeJob, WikiIngestJob, WikiQueue,
};
use sqlx::PgPool;
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
    if matches!(
        parse_status.as_str(),
        "cancelled" | "deleting" | "completed"
    ) {
        return Ok(());
    }
    let flipped = storage::try_set_processing(pool, document_id)
        .await
        .map_err(|e| e.to_string())?;
    if !flipped {
        return Ok(());
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
                    .await;
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
            .await;
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
) {
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
            return;
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
        return;
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
    let _ = bid::maybe_rematch_company_doc(pool, document_id).await;
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
        return;
    }
    maybe_start_postprocess(pool, document_id, version_id, attempt).await;
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
        {
            let _ = fail_now(pool, job.document_id, 0, e).await;
        }
        result.map_err(JobErr)
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
        return Ok(());
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
    Ok(())
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
        let _ = storage::bid::end_expired_projects(pool).await;
        if let Ok(ids) =
            storage::bid::reclaim_stale_converts(pool, runtime::HOUSEKEEP_STALE_SECS).await
        {
            for id in ids {
                tracing::warn!(document_id = %id, "bid convert reclaim");
                let _ = runtime::enqueue_bid_convert(id).await;
            }
        }
        if let Ok(ids) = storage::bid::pending_converts(pool).await {
            for id in ids {
                let _ = runtime::enqueue_bid_convert(id).await;
            }
        }
        if let Ok(stale) =
            storage::bid::reclaim_stale_extracts(pool, runtime::HOUSEKEEP_EXTRACT_STALE_SECS).await
        {
            for (rid, pid, did) in stale {
                tracing::warn!(
                    run_id = %rid,
                    project_id = %pid,
                    document_id = ?did,
                    "bid extract reclaim"
                );
                let _ = runtime::enqueue_bid_extract(rid, pid, did).await;
            }
        }
        if let Ok(rows) = sqlx::query(
            "SELECT r.id, r.project_id, r.document_id
             FROM bid_extract_runs r
             JOIN bid_projects p ON p.id = r.project_id
             WHERE r.status = 'pending' AND p.status = 'open'",
        )
        .fetch_all(pool)
        .await
        {
            use sqlx::Row;
            for r in rows {
                let rid: Uuid = r.get("id");
                let pid: Uuid = r.get("project_id");
                let did: Option<Uuid> = r.get("document_id");
                let _ = runtime::enqueue_bid_extract(rid, pid, did).await;
            }
        }
        let _ = storage::bid::reclaim_stale_section_retry_jobs(pool, runtime::HOUSEKEEP_STALE_SECS)
            .await;
        if let Ok(jobs) = storage::bid::pending_section_retries(pool).await {
            for (job_id, project_id, section_id) in jobs {
                let _ = runtime::enqueue_bid_section_retry(job_id, project_id, section_id).await;
            }
        }
        if let Ok(projects) = storage::bid::dirty_match_projects(pool).await {
            for project_id in projects {
                let _ = bid::schedule_dirty_and_enqueue(pool, project_id).await;
            }
        }
        // Matching reaping uses each claim's frozen lease policy and DB time;
        // housekeeping cannot inject an arbitrary stale threshold.
        let _ = storage::bid_matching::reap_expired_claims(pool).await;
        let _ = bid::enqueue_pending_route_jobs(pool).await;
        Ok(())
    }
}

pub struct BidConvertWorker {
    pool: Option<PgPool>,
}

impl oxana::FromContext<AppCtx> for BidConvertWorker {
    fn from_context(ctx: &AppCtx) -> Self {
        Self {
            pool: ctx.pool.clone(),
        }
    }
}

#[async_trait]
impl oxana::Worker<BidConvertJob> for BidConvertWorker {
    type Error = JobErr;

    fn max_retries(&self, _job: &BidConvertJob) -> u32 {
        3
    }

    async fn process(
        &self,
        job: BidConvertJob,
        _ctx: &oxana::JobContext,
    ) -> Result<(), Self::Error> {
        let Some(pool) = &self.pool else {
            return Err(JobErr("postgres not configured".into()));
        };
        let target_id = bid::tender::convert_and_schedule_document(pool, job.document_id)
            .await
            .map_err(JobErr)?;
        let Some(target_id) = target_id else {
            return Ok(());
        };
        let document = storage::bidding::get_document(pool, job.document_id)
            .await
            .map_err(|error| JobErr(error.to_string()))?
            .ok_or_else(|| JobErr("converted bid document disappeared".into()))?;
        tracing::info!(
            document_id = %job.document_id,
            target_id = %target_id,
            project_id = %document.project_id,
            "bid_convert queued frozen extraction target"
        );
        let _ = runtime::enqueue_bid_extract(target_id, document.project_id, Some(job.document_id))
            .await;
        Ok(())
    }
}

pub struct BidSectionRetryWorker {
    pool: Option<PgPool>,
}

impl oxana::FromContext<AppCtx> for BidSectionRetryWorker {
    fn from_context(ctx: &AppCtx) -> Self {
        Self {
            pool: ctx.pool.clone(),
        }
    }
}

#[async_trait]
impl oxana::Worker<BidSectionRetryJob> for BidSectionRetryWorker {
    type Error = JobErr;

    fn max_retries(&self, _job: &BidSectionRetryJob) -> u32 {
        3
    }

    async fn process(
        &self,
        job: BidSectionRetryJob,
        ctx: &oxana::JobContext,
    ) -> Result<(), Self::Error> {
        let Some(pool) = &self.pool else {
            return Err(JobErr("postgres not configured".into()));
        };
        let Some(token) =
            storage::bid::claim_section_retry_job(pool, job.job_id, job.project_id, job.section_id)
                .await
                .map_err(|error| JobErr(error.to_string()))?
        else {
            return Ok(());
        };
        let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel();
        let heartbeat_pool = pool.clone();
        let heartbeat_job_id = job.job_id;
        let heartbeat = tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(15));
            loop {
                tokio::select! {
                    _ = &mut stop_rx => break,
                    _ = interval.tick() => {
                        match storage::bid::heartbeat_section_retry_job(&heartbeat_pool, heartbeat_job_id, token).await {
                            Ok(true) => {}
                            _ => break,
                        }
                    }
                }
            }
        });
        let result = bid::retry_section_claimed(pool, job.project_id, job.section_id, token).await;
        let _ = stop_tx.send(());
        let _ = heartbeat.await;
        match result {
            Ok(()) => {
                let finished = storage::bid::finish_section_retry_job(
                    pool,
                    job.job_id,
                    job.project_id,
                    job.section_id,
                    token,
                    "done",
                    "",
                )
                .await
                .map_err(|error| JobErr(error.to_string()))?;
                if !finished {
                    return Err(JobErr("section retry job lease lost".into()));
                }
                Ok(())
            }
            Err(error) => {
                let terminal = ctx.meta.retries >= 3;
                let status = if terminal { "failed" } else { "pending" };
                let finished = storage::bid::finish_section_retry_job(
                    pool,
                    job.job_id,
                    job.project_id,
                    job.section_id,
                    token,
                    status,
                    &error,
                )
                .await
                .map_err(|finish_error| JobErr(finish_error.to_string()))?;
                if !finished {
                    return Err(JobErr("section retry job lease lost".into()));
                }
                Err(JobErr(error))
            }
        }
    }
}

pub struct BidExtractWorker {
    pool: Option<PgPool>,
}

impl oxana::FromContext<AppCtx> for BidExtractWorker {
    fn from_context(ctx: &AppCtx) -> Self {
        Self {
            pool: ctx.pool.clone(),
        }
    }
}

#[async_trait]
impl oxana::Worker<BidExtractJob> for BidExtractWorker {
    type Error = JobErr;

    async fn process(
        &self,
        job: BidExtractJob,
        _ctx: &oxana::JobContext,
    ) -> Result<(), Self::Error> {
        let Some(pool) = &self.pool else {
            return Err(JobErr("postgres not configured".into()));
        };
        bid::tender::run_extraction_target(pool, job.run_id, job.project_id, job.document_id)
            .await
            .map_err(JobErr)
    }
}

pub struct BidMatchRouteV1Handler {
    pool: Option<PgPool>,
}

impl oxana::FromContext<AppCtx> for BidMatchRouteV1Handler {
    fn from_context(ctx: &AppCtx) -> Self {
        Self {
            pool: ctx.pool.clone(),
        }
    }
}

#[async_trait]
impl oxana::Worker<BidMatchRouteV1Job> for BidMatchRouteV1Handler {
    type Error = JobErr;

    fn max_retries(&self, _job: &BidMatchRouteV1Job) -> u32 {
        0
    }

    async fn process(
        &self,
        job: BidMatchRouteV1Job,
        _ctx: &oxana::JobContext,
    ) -> Result<(), Self::Error> {
        let Some(pool) = &self.pool else {
            return Err(JobErr("postgres not configured".into()));
        };
        bid::matching::run_match_route_v1(pool, job)
            .await
            .map_err(JobErr)
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
        let _ = bid::maybe_rematch_company_doc(pool, document_id).await;
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
        return Ok(());
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
        return Ok(());
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
    Ok(())
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
        return Ok(());
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
    Ok(())
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

pub async fn process_summary_pg(
    pool: &PgPool,
    document_id: Uuid,
    attempt: i32,
    fallback: bool,
) -> Result<(), String> {
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
    Ok(())
}

pub async fn process_questions_pg(
    pool: &PgPool,
    document_id: Uuid,
    chunk_ids: &[Uuid],
    prev_ids: &[Option<Uuid>],
    next_ids: &[Option<Uuid>],
    attempt: i32,
) -> Result<(), String> {
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
    Ok(())
}

pub async fn process_extract_pg(
    pool: &PgPool,
    chunk_id: Uuid,
    document_id: Uuid,
    attempt: i32,
) -> Result<(), String> {
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
    Ok(())
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

pub async fn run_core(ctx: AppCtx) -> Result<(), String> {
    let stop = std::sync::Arc::new(tokio::sync::Notify::new());
    let stopper = stop.clone();
    let signal_task = tokio::spawn(async move {
        shutdown_signal().await;
        stopper.notify_waiters();
    });
    let shut = |n: std::sync::Arc<tokio::sync::Notify>| async move {
        n.notified().await;
        Ok::<(), std::io::Error>(())
    };
    let timeout = std::time::Duration::from_secs(2);
    let core = {
        let storage = runtime::connect().map_err(|e| e.to_string())?;
        storage
            .runtime(ctx.clone())
            .queue_with_concurrency::<DefaultQueue>(runtime::runtime_concurrency("CORE", 8))
            .worker::<DocumentProcessWorker, DocumentProcessJob>()
            .queue_with_concurrency::<BidConvertV1Queue>(runtime::runtime_concurrency(
                "BID_CONVERT",
                4,
            ))
            .worker::<BidConvertWorker, BidConvertJob>()
            .queue_with_concurrency::<BidExtractV1Queue>(runtime::runtime_concurrency(
                "BID_EXTRACT",
                4,
            ))
            .worker::<BidExtractWorker, BidExtractJob>()
            .queue_with_concurrency::<BidSectionRetryV1Queue>(runtime::runtime_concurrency(
                "BID_SECTION_RETRY",
                4,
            ))
            .worker::<BidSectionRetryWorker, BidSectionRetryJob>()
            .queue_with_concurrency::<BidMatchingV1Queue>(runtime::runtime_concurrency(
                "BID_MATCHING",
                4,
            ))
            .worker::<BidMatchRouteV1Handler, BidMatchRouteV1Job>()
            .shutdown_on(shut(stop.clone()))
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
            .shutdown_on(shut(stop.clone()))
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
            .shutdown_on(shut(stop.clone()))
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
            .shutdown_on(shut(stop.clone()))
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
            .shutdown_on(shut(stop.clone()))
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
            .shutdown_on(shut(stop))
            .shutdown_timeout(timeout)
            .run()
    };
    let result = tokio::try_join!(core, post, enrich, maint, shared, wiki_rt)
        .map(|_| ())
        .map_err(|e| e.to_string());
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
    use storage::{
        apply_fresh_baseline, connect, create_workspace_with_library, insert_document, insert_user,
        write_blob,
    };

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
        .await;
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
}
