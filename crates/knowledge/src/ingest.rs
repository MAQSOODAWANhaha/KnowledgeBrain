//! Knowledge ingest: convert, fanout, kb delete, reparse.
//!
//! Read: documents ⇔ product_versions, parse_status, attempt, source_passages,
//! file_name, spans.
//! Write: try_set_processing / open_attempt / set_parse_status / set_index_ready /
//! span start-finish-skip / persist_passage_index / deleting / archived /
//! current_version_id.
//! Enqueue: enqueue_image_multimodal, enqueue_post_process, enqueue_semantic_index_v2,
//! reparse process/manual/index_delete/datatable, wiki retract.

use sqlx::PgPool;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub async fn run_convert(
    pool: &PgPool,
    document_id: Uuid,
    attempt: i32,
    passages: &[String],
    manual: bool,
    cancel: &CancellationToken,
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
    let overrides: Option<crate::ProcessOverrides> = overrides_raw
        .and_then(|v| serde_json::from_value(v).ok())
        .filter(|o: &crate::ProcessOverrides| !o.is_empty());
    if let Some(o) = &overrides
        && let Some(v) = o.asr_config.as_ref().and_then(|a| a.enabled)
    {
        asr_enabled = v;
    }
    let ext = file_name.rsplit('.').next().unwrap_or("txt");
    let parser_engine = crate::parser_engine_for(&chunking_cfg, overrides.as_ref(), ext);
    if parse_status == "completed" {
        return crate::pipeline::schedule_semantic_index_v2_if_ready(pool, version_id).await;
    }
    if matches!(parse_status.as_str(), "cancelled" | "deleting") {
        return Ok(());
    }
    let flipped = crate::try_set_processing(pool, document_id)
        .await
        .map_err(|e| e.to_string())?;
    if !flipped {
        return match crate::document_parse_status(pool, document_id)
            .await
            .map_err(|error| error.to_string())?
            .as_deref()
        {
            Some("completed") => {
                crate::pipeline::schedule_semantic_index_v2_if_ready(pool, version_id).await
            }
            _ => Ok(()),
        };
    }
    let _ = crate::open_attempt(pool, document_id, attempt).await;
    tracing::info!(
        document_id = %document_id,
        file = %file_name,
        engine = %parser_engine,
        attempt,
        "parse convert start"
    );
    if !passages.is_empty() {
        let _ = crate::start_span(
            pool,
            document_id,
            attempt,
            crate::obs::SPAN_DOCREADER,
            Some(crate::obs::ROOT_NAME),
            Some(serde_json::json!({"engine": "passages"})),
        )
        .await;
        let _ = crate::finish_span(
            pool,
            document_id,
            attempt,
            crate::obs::SPAN_DOCREADER,
            crate::obs::STATUS_DONE,
            Some(serde_json::json!({"engine": "passages"})),
        )
        .await;
        let _ = crate::start_span(
            pool,
            document_id,
            attempt,
            crate::obs::SPAN_CHUNKING,
            Some(crate::obs::ROOT_NAME),
            None,
        )
        .await;
        let indexed = persist_passage_index(pool, document_id, version_id, passages).await?;
        let _ = crate::finish_span(
            pool,
            document_id,
            attempt,
            crate::obs::SPAN_CHUNKING,
            crate::obs::STATUS_DONE,
            Some(serde_json::json!({"passages": passages.len()})),
        )
        .await;
        let _ = crate::start_span(
            pool,
            document_id,
            attempt,
            crate::obs::SPAN_EMBEDDING,
            Some(crate::obs::ROOT_NAME),
            None,
        )
        .await;
        let _ = crate::finish_span(
            pool,
            document_id,
            attempt,
            crate::obs::SPAN_EMBEDDING,
            crate::obs::STATUS_DONE,
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
        let _ = crate::start_span(
            pool,
            document_id,
            attempt,
            crate::obs::SPAN_DOCREADER,
            Some(crate::obs::ROOT_NAME),
            Some(serde_json::json!({"engine": "manual", "file": file_name})),
        )
        .await;
        let _ = platform::write_blob_async(&format!("{file_hash}.md"), md.as_bytes()).await;
        let _ = crate::finish_span(
            pool,
            document_id,
            attempt,
            crate::obs::SPAN_DOCREADER,
            crate::obs::STATUS_DONE,
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
        let _ = crate::start_span(
            pool,
            document_id,
            attempt,
            crate::obs::SPAN_DOCREADER,
            Some(crate::obs::ROOT_NAME),
            Some(serde_json::json!({"engine": engine, "file": file_name})),
        )
        .await;
        if engine == "docreader" && docparser::reader_addr().is_none() {
            fail_pipeline(
                pool,
                document_id,
                attempt,
                crate::obs::SPAN_DOCREADER,
                docparser::NOT_CONFIGURED,
            )
            .await?;
            return Ok(());
        }
        let engine_overrides = overrides
            .as_ref()
            .map(|o| o.parser_engine_overrides.clone())
            .unwrap_or_default();
        let mut result = match docparser::convert_with_cancel(
            docparser::ConvertInput {
                engine: &parser_engine,
                file_name: &file_name,
                file_type: ext,
                is_url,
                bytes: if is_url { Vec::new() } else { bytes },
                url: &url,
                title: &file_name,
                overrides: &engine_overrides,
            },
            cancel,
        )
        .await
        {
            Ok(r) => r,
            Err(e) => {
                return fail_stage_retryable(
                    pool,
                    document_id,
                    attempt,
                    crate::obs::SPAN_DOCREADER,
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
                    crate::obs::SPAN_DOCREADER,
                    &result.error,
                )
                .await?;
                return Ok(());
            }
            return fail_stage_retryable(
                pool,
                document_id,
                attempt,
                crate::obs::SPAN_DOCREADER,
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
                        crate::obs::SPAN_DOCREADER,
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
                        crate::obs::SPAN_DOCREADER,
                        &result.error,
                    )
                    .await?;
                    return Ok(());
                }
                return fail_stage_retryable(
                    pool,
                    document_id,
                    attempt,
                    crate::obs::SPAN_DOCREADER,
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
        let _ = crate::finish_span(
            pool,
            document_id,
            attempt,
            crate::obs::SPAN_DOCREADER,
            crate::obs::STATUS_DONE,
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
    let existing_chunks = crate::load_document_chunks(pool, document_id)
        .await
        .unwrap_or_default();
    let chunks = if crate::obs::stage_satisfied(&prior_spans, crate::obs::SPAN_CHUNKING)
        && !existing_chunks.is_empty()
    {
        tracing::info!(
            document_id = %document_id,
            chunks = existing_chunks.len(),
            "parse chunking reuse"
        );
        existing_chunks
    } else {
        let _ = crate::start_span(
            pool,
            document_id,
            attempt,
            crate::obs::SPAN_CHUNKING,
            Some(crate::obs::ROOT_NAME),
            None,
        )
        .await;
        let split = crate::chunker::split_from_config(
            &markdown,
            version_id,
            document_id,
            crate::chunker::SplitterConfig {
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
        let kept = crate::index::keep_nonempty_chunks(split);
        crate::delete_graph_for_document(pool, document_id)
            .await
            .map_err(|e| e.to_string())?;
        crate::replace_document_chunks(pool, document_id, &kept, &[])
            .await
            .map_err(|e| e.to_string())?;
        let _ = crate::finish_span(
            pool,
            document_id,
            attempt,
            crate::obs::SPAN_CHUNKING,
            crate::obs::STATUS_DONE,
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
    if crate::obs::stage_satisfied(&prior_spans, crate::obs::SPAN_EMBEDDING) {
        return Ok(());
    }
    let _ = crate::start_span(
        pool,
        document_id,
        attempt,
        crate::obs::SPAN_EMBEDDING,
        Some(crate::obs::ROOT_NAME),
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
                    crate::obs::SPAN_EMBEDDING,
                    &e,
                )
                .await;
            }
        };
    let _ = crate::finish_span(
        pool,
        document_id,
        attempt,
        crate::obs::SPAN_EMBEDDING,
        crate::obs::STATUS_DONE,
        None,
    )
    .await;
    tracing::info!(
        document_id = %document_id,
        chunks = chunks.len(),
        "parse embedding done"
    );
    let images = crate::enrichment::markdown_image_keys(&markdown);
    let mut mm = crate::version_multimodal_enabled(pool, version_id)
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
                    crate::enrichment::image_source_type(&file_name, &markdown)
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
    let chunks: Vec<crate::Chunk> = passages
        .iter()
        .map(|text| crate::Chunk {
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

pub enum PersistIndexResult {
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

pub async fn after_index_fanout(
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
            let _ = crate::skip_span(
                pool,
                document_id,
                attempt,
                crate::obs::SPAN_MULTIMODAL,
                "vlm not configured",
            )
            .await;
            let _ = crate::set_parse_status(
                pool,
                document_id,
                "finalizing",
                "ocr_error: vlm not configured; caption_error: vlm not configured",
            )
            .await;
            let _ = crate::set_index_ready(pool, document_id, false).await;
            tracing::warn!(
                document_id = %document_id,
                reason = "vlm not configured",
                images = images.len(),
                "parse multimodal hold"
            );
            if text_count > 0 {
                crate::pipeline::maybe_start_postprocess(pool, document_id, version_id, attempt)
                    .await;
            }
            return Ok(());
        }
        let _ = crate::start_span(
            pool,
            document_id,
            attempt,
            crate::obs::SPAN_MULTIMODAL,
            Some(crate::obs::ROOT_NAME),
            Some(serde_json::json!({"images": images.len()})),
        )
        .await;
        tracing::info!(
            document_id = %document_id,
            images = images.len(),
            "parse multimodal enqueue"
        );
        crate::enrichment::set_pending_count(document_id, images.len() as i32)?;
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
                    crate::enrichment::decr_pending_count(document_id)?;
                }
            }
        }
        if leftover <= 0 {
            let _ = crate::skip_span(
                pool,
                document_id,
                attempt,
                crate::obs::SPAN_MULTIMODAL,
                "enqueue failed",
            )
            .await;
            let _ = crate::set_parse_status(
                pool,
                document_id,
                "finalizing",
                "ocr_error: image enqueue failed; caption_error: image enqueue failed",
            )
            .await;
            let _ = crate::set_index_ready(pool, document_id, false).await;
            tracing::warn!(
                document_id = %document_id,
                reason = "enqueue failed",
                "parse multimodal hold"
            );
            if text_count > 0 {
                crate::pipeline::maybe_start_postprocess(pool, document_id, version_id, attempt)
                    .await;
            }
        }
        return Ok(());
    }
    let _ = crate::skip_span(
        pool,
        document_id,
        attempt,
        crate::obs::SPAN_MULTIMODAL,
        "no images",
    )
    .await;
    let _ = crate::set_index_ready(pool, document_id, true).await;
    tracing::info!(document_id = %document_id, index_ready = true, "parse completed");
    if text_count == 0 {
        let _ = crate::set_parse_status(pool, document_id, "completed", "").await;
        let _ = crate::skip_span(
            pool,
            document_id,
            attempt,
            crate::obs::SPAN_POSTPROCESS,
            "no further work",
        )
        .await;
        return crate::pipeline::schedule_semantic_index_v2_if_ready(pool, version_id).await;
    }
    crate::pipeline::maybe_start_postprocess(pool, document_id, version_id, attempt).await;
    crate::pipeline::schedule_semantic_index_v2_if_ready(pool, version_id).await
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
    overrides: Option<&crate::ProcessOverrides>,
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

pub async fn persist_indexed_chunks(
    pool: &PgPool,
    document_id: Uuid,
    _version_id: Uuid,
    chunks: &[crate::Chunk],
    vector_on: bool,
    keyword_on: bool,
) -> Result<PersistIndexResult, String> {
    if status_aborted(document_parse_status(pool, document_id).await.as_deref()) {
        return Ok(PersistIndexResult::Aborted);
    }
    let kept = crate::index::keep_nonempty_chunks(chunks.to_vec());
    crate::delete_graph_for_document(pool, document_id)
        .await
        .map_err(|e| e.to_string())?;
    // Chunk rows first so an embed failure does not throw away the split.
    crate::replace_document_chunks(pool, document_id, &kept, &[])
        .await
        .map_err(|e| e.to_string())?;
    persist_document_embeddings(pool, document_id, &kept, vector_on, keyword_on).await
}

async fn persist_document_embeddings(
    pool: &PgPool,
    document_id: Uuid,
    chunks: &[crate::Chunk],
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
    if vector_on {
        let version_id: Uuid =
            sqlx::query_scalar("SELECT product_version_id FROM documents WHERE id = $1")
                .bind(document_id)
                .fetch_one(pool)
                .await
                .map_err(|e| e.to_string())?;
        crate::freeze_version_embedding_model(pool, version_id).await?;
    }
    let embeddings = crate::index::index_chunks(chunks, &title, vector_on, keyword_on)?;
    if status_aborted(document_parse_status(pool, document_id).await.as_deref()) {
        return Ok(PersistIndexResult::Aborted);
    }
    crate::replace_document_embeddings(pool, document_id, &embeddings)
        .await
        .map_err(|e| e.to_string())?;
    let st = document_parse_status(pool, document_id).await;
    if status_aborted(st.as_deref()) {
        if st.as_deref() == Some("deleting") {
            let _ = crate::purge_document_index(pool, document_id).await;
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

async fn document_stage_spans(pool: &PgPool, document_id: Uuid, attempt: i32) -> Vec<crate::Span> {
    crate::list_spans_attempt(pool, document_id, attempt)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|r| r.into_span())
        .collect()
}

fn reused_image_source(spans: &[crate::Span]) -> String {
    let Some(span) = spans.iter().find(|s| s.name == crate::obs::SPAN_DOCREADER) else {
        return String::new();
    };
    image_source_from_docreader_output(span.output.as_ref())
}

pub fn image_source_from_docreader_output(output: Option<&serde_json::Value>) -> String {
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

fn reused_markdown(spans: &[crate::Span], file_hash: &str) -> Option<String> {
    if !crate::obs::stage_satisfied(spans, crate::obs::SPAN_DOCREADER) {
        return None;
    }
    let bytes = platform::read_blob(&format!("{file_hash}.md")).ok()?;
    if bytes.is_empty() {
        return None;
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

pub fn parse_stored_url(bytes: &[u8]) -> (bool, String) {
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

async fn fail_stage_retryable(
    pool: &PgPool,
    document_id: Uuid,
    attempt: i32,
    stage: &str,
    message: &str,
) -> Result<(), String> {
    let _ = crate::finish_span(
        pool,
        document_id,
        attempt,
        stage,
        crate::obs::STATUS_FAILED,
        Some(serde_json::json!({"error": message})),
    )
    .await;
    let _ = crate::cancel_dependent_stages(pool, document_id, attempt, stage).await;
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
    let _ = crate::finish_span(
        pool,
        document_id,
        attempt,
        stage,
        crate::obs::STATUS_FAILED,
        Some(serde_json::json!({"error": message})),
    )
    .await;
    let _ = crate::cancel_dependent_stages(pool, document_id, attempt, stage).await;
    let _ = crate::finish_span(
        pool,
        document_id,
        attempt,
        crate::obs::ROOT_NAME,
        crate::obs::STATUS_FAILED,
        Some(serde_json::json!({"error": message})),
    )
    .await;
    tracing::error!(document_id = %document_id, stage, error = %message, "parse stage fail");
    fail_now(pool, document_id, attempt, message).await
}

pub async fn fail_now(
    pool: &PgPool,
    document_id: Uuid,
    attempt: i32,
    message: &str,
) -> Result<(), String> {
    let _ = attempt;
    crate::set_parse_status(pool, document_id, "failed", message)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn run_kb_delete(pool: &PgPool, product_version_id: Uuid) -> Result<(), String> {
    let _ = crate::cancel_active_docs_for_versions(pool, &[product_version_id]).await;
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
        crate::pipeline::run_list_delete(pool, did).await?;
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
        let _ = crate::delete_empty_product(pool, pid).await;
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

pub async fn run_reparse(pool: &PgPool, document_id: Uuid, attempt: i32) -> Result<(), String> {
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
    crate::open_attempt(pool, document_id, attempt)
        .await
        .map_err(|error| error.to_string())?;
    if let Some(vid) = vid {
        crate::pipeline::run_wiki_ingest(pool, vid, document_id, crate::wiki::OP_RETRACT).await?;
        crate::graph::delete_document(vid, document_id)?;
    }
    crate::purge_document_index(pool, document_id)
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
