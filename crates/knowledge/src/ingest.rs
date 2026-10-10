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
    let current:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM documents WHERE id=$1 AND attempt=$2 AND deleted_at IS NULL AND parse_status IN ('pending','processing','finalizing','failed')) AND EXISTS(SELECT 1 FROM knowledge_job_outbox WHERE document_id=$1 AND attempt=$2 AND job_key='convert' AND state IN ('pending','published')) AND NOT EXISTS(SELECT 1 FROM knowledge_job_receipts WHERE document_id=$1 AND attempt=$2 AND job_key='convert')").bind(document_id).bind(attempt).fetch_one(pool).await.map_err(|e|e.to_string())?;
    if !current {
        return Ok(());
    }
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
    let flipped = sqlx::query("UPDATE documents SET parse_status='processing',error_message='',updated_at=now() WHERE id=$1 AND attempt=$2 AND deleted_at IS NULL AND parse_status IN ('pending','failed')")
        .bind(document_id).bind(attempt).execute(pool).await.map_err(|e|e.to_string())?.rows_affected()==1;
    if !flipped {
        return match crate::document_parse_status(pool, document_id)
            .await
            .map_err(|error| error.to_string())?
            .as_deref()
        {
            Some("completed") => {
                crate::pipeline::schedule_semantic_index_v2_if_ready(pool, version_id).await
            }
            // A duplicate delivery does not own the running generation.
            // Durable conversion obligations are reclaimed with a new attempt
            // after the bounded processing lease, never by failing this worker.
            Some("processing") => Ok(()),
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
        let indexed =
            persist_passage_index(pool, document_id, version_id, attempt, passages).await?;
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
    // D3: structured source units from docparser (empty when markdown is reused/manual).
    let mut source_units: Vec<docparser::StructuredSourceUnit> = Vec::new();
    let mut source_contract: Option<docparser::SourceContract> = None;
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
        platform::write_blob_async(
            &format!("{file_hash}.{document_id}.{attempt}.md"),
            md.as_bytes(),
        )
        .await
        .map_err(|e| format!("persist manual markdown: {e}"))?;
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
    } else if let Some(md) = reused_markdown(&prior_spans, &file_hash, document_id, attempt) {
        convert_image_source = reused_image_source(&prior_spans);
        let manifest = platform::read_blob(&format!(
            "{file_hash}.{document_id}.{attempt}.manifest.json"
        ))
        .map_err(|e| format!("read persisted parse manifest: {e}"))?;
        let parsed: serde_json::Value =
            serde_json::from_slice(&manifest).map_err(|e| e.to_string())?;
        source_units =
            serde_json::from_value(parsed["source_units"].clone()).map_err(|e| e.to_string())?;
        source_contract =
            serde_json::from_value(parsed["source_contract"].clone()).map_err(|e| e.to_string())?;
        if let Some(contract) = &source_contract {
            let persisted = docparser::ReadResult {
                markdown: md.clone(),
                structured_source_units: source_units.clone(),
                ..Default::default()
            };
            contract.validate(&persisted).map_err(|e| e.0)?;
        }
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
        // D3: keep structured units before markdown is rewritten/consumed.
        let (rewritten, contract) = persist_and_rewrite_images(&result).await?;
        source_contract = contract;
        source_units = std::mem::take(&mut result.structured_source_units);
        result.markdown = rewritten;
        let manifest = serde_json::to_vec(
            &serde_json::json!({"source_units":source_units,"source_contract":source_contract}),
        )
        .map_err(|e| e.to_string())?;
        platform::write_blob_async(
            &format!("{file_hash}.{document_id}.{attempt}.manifest.json"),
            &manifest,
        )
        .await
        .map_err(|e| format!("persist parse manifest: {e}"))?;
        platform::write_blob_async(
            &format!("{file_hash}.{document_id}.{attempt}.md"),
            result.markdown.as_bytes(),
        )
        .await
        .map_err(|e| format!("persist converted markdown: {e}"))?;
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
        let mut split = crate::chunker::split_from_config(
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
        // D3: attach structured source locators (page/bbox/table grid) to chunks.
        crate::chunker::annotate_source_locators(
            &mut split,
            &markdown,
            &source_units,
            source_contract.as_ref(),
        );
        let kept = crate::index::keep_nonempty_chunks(split);
        if !crate::workflow::stage_index(pool, document_id, attempt, &kept, &[], true).await? {
            return Ok(());
        }
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
    let _ = crate::start_span(
        pool,
        document_id,
        attempt,
        crate::obs::SPAN_EMBEDDING,
        Some(crate::obs::ROOT_NAME),
        None,
    )
    .await;
    let indexed = match persist_document_embeddings(
        pool,
        document_id,
        attempt,
        &chunks,
        opts.vector,
        opts.keyword,
    )
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
    attempt: i32,
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
        let _ = platform::write_blob_async(
            format!("{file_hash}.{document_id}.{attempt}.md").as_str(),
            joined.as_bytes(),
        )
        .await;
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
            source_locator: None,
        })
        .collect();
    let opts = version_index_opts(pool, version_id).await;
    persist_indexed_chunks(
        pool,
        document_id,
        version_id,
        attempt,
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
    if !crate::workflow::current(pool, document_id, attempt)
        .await
        .map_err(|e| e.to_string())?
    {
        return Ok(());
    }
    if !images.is_empty() {
        crate::start_span(
            pool,
            document_id,
            attempt,
            crate::obs::SPAN_MULTIMODAL,
            Some(crate::obs::ROOT_NAME),
            Some(serde_json::json!({"images":images.len()})),
        )
        .await
        .map_err(|e| e.to_string())?;
        let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
        if !crate::workflow::lock_current(&mut tx, document_id, attempt)
            .await
            .map_err(|e| e.to_string())?
        {
            return Ok(());
        }
        for key in images {
            crate::workflow::put_tx(
                &mut tx,
                document_id,
                version_id,
                attempt,
                &crate::workflow::Work::Image {
                    image_key: key.clone(),
                    image_source_type: file_name.to_owned(),
                },
            )
            .await
            .map_err(|e| e.to_string())?;
        }
        crate::workflow::complete_tx(&mut tx, document_id, attempt, "convert")
            .await
            .map_err(|e| e.to_string())?;
        tx.commit().await.map_err(|e| e.to_string())?;
        return crate::workflow::dispatch(pool).await;
    }
    crate::skip_span(
        pool,
        document_id,
        attempt,
        crate::obs::SPAN_MULTIMODAL,
        "no images",
    )
    .await
    .map_err(|e| e.to_string())?;
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    if !crate::workflow::lock_current(&mut tx, document_id, attempt)
        .await
        .map_err(|e| e.to_string())?
    {
        return Ok(());
    }
    if text_count > 0 {
        crate::workflow::put_tx(
            &mut tx,
            document_id,
            version_id,
            attempt,
            &crate::workflow::Work::Postprocess { clone_keep: false },
        )
        .await
        .map_err(|e| e.to_string())?;
    }
    if text_count == 0 {
        sqlx::query("UPDATE documents SET parse_status='completed',active_generation=$2,index_ready=true,enable_status='enabled',updated_at=now() WHERE id=$1 AND attempt=$2")
            .bind(document_id).bind(attempt).execute(&mut *tx).await.map_err(|e|e.to_string())?;
    }
    crate::workflow::complete_tx(&mut tx, document_id, attempt, "convert")
        .await
        .map_err(|e| e.to_string())?;
    tx.commit().await.map_err(|e| e.to_string())?;
    crate::workflow::dispatch(pool).await?;
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
    attempt: i32,
    chunks: &[crate::Chunk],
    vector_on: bool,
    keyword_on: bool,
) -> Result<PersistIndexResult, String> {
    if status_aborted(document_parse_status(pool, document_id).await.as_deref()) {
        return Ok(PersistIndexResult::Aborted);
    }
    let kept = crate::index::keep_nonempty_chunks(chunks.to_vec());
    if !crate::workflow::stage_index(pool, document_id, attempt, &kept, &[], true).await? {
        return Ok(PersistIndexResult::Aborted);
    }
    persist_document_embeddings(pool, document_id, attempt, &kept, vector_on, keyword_on).await
}

async fn persist_document_embeddings(
    pool: &PgPool,
    document_id: Uuid,
    attempt: i32,
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
    if !crate::workflow::stage_index(pool, document_id, attempt, chunks, &embeddings, false).await?
    {
        return Ok(PersistIndexResult::Aborted);
    }
    sqlx::query("UPDATE documents SET processed_at=now(),updated_at=now() WHERE id=$1 AND attempt=$2 AND parse_status='processing'")
        .bind(document_id).bind(attempt).execute(pool).await.map_err(|e|e.to_string())?;
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

fn reused_markdown(
    spans: &[crate::Span],
    file_hash: &str,
    document_id: Uuid,
    attempt: i32,
) -> Option<String> {
    if !crate::obs::stage_satisfied(spans, crate::obs::SPAN_DOCREADER) {
        return None;
    }
    let bytes = platform::read_blob(&format!("{file_hash}.{document_id}.{attempt}.md")).ok()?;
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

async fn persist_and_rewrite_images(
    result: &docparser::ReadResult,
) -> Result<(String, Option<docparser::SourceContract>), String> {
    let (md, blobs, contract) = docparser::rewrite_images_with_contract(result)
        .await
        .map_err(|e| e.0)?;
    let rewritten = md.clone();
    persist_image_work(move || -> Result<(), String> {
        for (hash, data) in blobs {
            platform::write_blob(&hash, &data)
                .map_err(|e| format!("persist required image {hash}: {e}"))?;
        }
        // Verify all owned Markdown targets, including references which the
        // parser already expressed as object keys, before publishing Markdown.
        for key in crate::enrichment::markdown_image_keys(&md) {
            if let Some(hash) = key.strip_prefix("objects/") {
                let bytes = platform::read_blob(hash)
                    .map_err(|e| format!("required image {hash} unavailable: {e}"))?;
                if platform::sha256_hex(&bytes) != hash {
                    return Err(format!("required image {hash} digest mismatch"));
                }
            }
        }
        Ok(())
    })
    .await?;
    Ok((rewritten, contract))
}

async fn persist_image_work(
    work: impl FnOnce() -> Result<(), String> + Send + 'static,
) -> Result<(), String> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| format!("join required image persistence: {e}"))?
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
    crate::workflow::update_status(pool, document_id, attempt, "failed", message).await?;
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
    crate::workflow::update_status(pool, document_id, attempt, "failed", message).await?;
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

/// Reparse publication is owned by the fresh attempt's durable conversion
/// obligation. A delayed reparse envelope must never retract newly built data.
pub async fn run_reparse(pool: &PgPool, document_id: Uuid, attempt: i32) -> Result<(), String> {
    let row:Option<(Uuid,String,Option<serde_json::Value>)>=sqlx::query_as("SELECT product_version_id,type,source_passages FROM documents WHERE id=$1 AND attempt=$2 AND parse_status='pending' AND deleted_at IS NULL")
        .bind(document_id).bind(attempt).fetch_optional(pool).await.map_err(|e|e.to_string())?;
    if let Some((version_id, kind, source)) = row {
        let passages = source
            .filter(|v| !v.is_null())
            .map(serde_json::from_value)
            .transpose()
            .map_err(|e| e.to_string())?
            .unwrap_or_default();
        crate::workflow::put(
            pool,
            document_id,
            version_id,
            attempt,
            crate::workflow::Work::Convert {
                passages,
                manual: kind == "manual",
            },
        )
        .await?;
        crate::workflow::dispatch(pool).await?;
    }
    Ok(())
}

#[cfg(test)]
mod persistence_fault_tests {
    #[tokio::test]
    async fn required_image_write_and_join_failures_are_not_acknowledged() {
        let write = super::persist_image_work(|| Err("injected object write failure".into())).await;
        assert_eq!(write.unwrap_err(), "injected object write failure");
        let join = super::persist_image_work(|| panic!("injected object worker panic")).await;
        assert!(
            join.unwrap_err()
                .contains("join required image persistence")
        );
    }
}
