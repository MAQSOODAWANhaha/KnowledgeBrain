//! Production knowledge jobs: hydrate is internal; callers pass PgPool only.

use crate::{
    ParseStatus, Store, TYPE_CHUNK_EXTRACT, TYPE_QUESTION, TYPE_SUMMARY, expected_subtasks,
};
use sqlx::PgPool;
use uuid::Uuid;

fn req_uuid(v: &serde_json::Value, key: &str) -> Result<Uuid, String> {
    let aliases: &[&str] = match key {
        "document_id" => &["document_id", "knowledge_id"],
        "product_version_id" => &["product_version_id", "knowledge_base_id"],
        other => return read_uuid(v, other),
    };
    for k in aliases {
        if let Ok(id) = read_uuid(v, k) {
            return Ok(id);
        }
    }
    let _ = v.get("tenant_id");
    Err(format!("missing {key}"))
}

fn read_uuid(v: &serde_json::Value, key: &str) -> Result<Uuid, String> {
    v.get(key)
        .and_then(|x| x.as_str())
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| format!("missing {key}"))
}

pub fn post_process(store: &mut Store, payload: &serde_json::Value) -> Result<(), String> {
    let doc_id = req_uuid(payload, "document_id")?;
    let clone_keep = payload
        .get("clone_keep")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let Some(doc) = store.documents.get(&doc_id).cloned() else {
        return Ok(());
    };
    if doc.parse_status.is_aborted() {
        return Ok(());
    }
    let rows = crate::obs::timeline(store, doc_id);
    if !crate::obs::can_start_stage_or_legacy(crate::obs::SPAN_POSTPROCESS, &rows) {
        return Err("postprocess waiting for embedding and multimodal".into());
    }
    if doc.parse_status != ParseStatus::Processing {
        return Ok(());
    }
    let Some(version) = store.effective_version(doc_id) else {
        return Ok(());
    };
    let text_count = store
        .chunks
        .values()
        .filter(|c| c.document_id == doc_id && c.chunk_type == "text")
        .count();
    let ocr_count = store
        .chunks
        .values()
        .filter(|c| {
            c.document_id == doc_id
                && matches!(c.chunk_type.as_str(), "image_ocr" | "image_caption")
        })
        .count();
    let n = expected_subtasks(
        text_count,
        ocr_count,
        version.question_enabled,
        version.needs_embedding(),
        version.wiki_enabled,
        version.graph_enabled,
        clone_keep,
    );
    if n == 0 {
        if let Some(d) = store.documents.get_mut(&doc_id) {
            d.parse_status = ParseStatus::Completed;
            d.summary_status = crate::SummaryStatus::None;
        }
        crate::obs::finish(
            store,
            doc_id,
            crate::obs::SPAN_POSTPROCESS,
            crate::obs::STATUS_DONE,
        );
        crate::obs::finish(
            store,
            doc_id,
            crate::obs::ROOT_NAME,
            crate::obs::STATUS_DONE,
        );
        return Ok(());
    }
    if !store.set_finalizing(doc_id, n as i32) {
        return Ok(());
    }
    if text_count + ocr_count > 0 && !clone_keep {
        if let Some(d) = store.documents.get_mut(&doc_id) {
            d.summary_status = crate::SummaryStatus::Pending;
        }
        store.enqueue(
            TYPE_SUMMARY,
            platform::QUEUE_SUMMARY,
            serde_json::json!({ "document_id": doc_id, "attempt": doc.attempt }),
        );
    }
    if !clone_keep && version.question_enabled && version.needs_embedding() && text_count > 0 {
        let mut ids: Vec<_> = store
            .chunks
            .values()
            .filter(|c| c.document_id == doc_id && c.chunk_type == "text")
            .cloned()
            .collect();
        ids.sort_by_key(|c| c.start_at);
        for (batch_i, batch) in ids.chunks(20).enumerate() {
            let base = batch_i * 20;
            let chunk_ids: Vec<String> = batch.iter().map(|c| c.id.to_string()).collect();
            let prev_ids: Vec<Option<String>> = (0..batch.len())
                .map(|i| {
                    if base + i == 0 {
                        None
                    } else {
                        Some(ids[base + i - 1].id.to_string())
                    }
                })
                .collect();
            let next_ids: Vec<Option<String>> = (0..batch.len())
                .map(|i| ids.get(base + i + 1).map(|c| c.id.to_string()))
                .collect();
            store.enqueue(
                TYPE_QUESTION,
                platform::QUEUE_QUESTION,
                serde_json::json!({
                    "document_id": doc_id,
                    "chunk_ids": chunk_ids,
                    "prev_ids": prev_ids,
                    "next_ids": next_ids,
                    "attempt": doc.attempt
                }),
            );
        }
    }
    if version.wiki_enabled && text_count + ocr_count > 0 {
        crate::wiki::enqueue_ingest(store, doc.product_version_id, doc_id);
    }
    if version.graph_enabled {
        let graph_ids: Vec<Uuid> = store
            .chunks
            .values()
            .filter(|c| {
                c.document_id == doc_id
                    && matches!(
                        c.chunk_type.as_str(),
                        "text" | "image_ocr" | "image_caption"
                    )
            })
            .map(|c| c.id)
            .collect();
        for cid in graph_ids {
            store.enqueue(
                TYPE_CHUNK_EXTRACT,
                platform::QUEUE_GRAPH,
                serde_json::json!({
                    "chunk_id": cid,
                    "document_id": doc_id,
                    "attempt": doc.attempt
                }),
            );
        }
    }
    crate::obs::finish(
        store,
        doc_id,
        crate::obs::SPAN_POSTPROCESS,
        crate::obs::STATUS_DONE,
    );
    Ok(())
}

fn truncate_key(key: &str) -> &str {
    let t = key.trim_start_matches("objects/").trim_start_matches('/');
    match t.char_indices().nth(16) {
        Some((i, _)) => &t[..i],
        None => t,
    }
}

async fn maybe_start_postprocess(pool: &PgPool, document_id: Uuid, version_id: Uuid, attempt: i32) {
    let rows = crate::list_spans_attempt(pool, document_id, attempt)
        .await
        .unwrap_or_default();
    let spans: Vec<_> = rows.into_iter().map(|r| r.into_span()).collect();
    if !crate::obs::can_start_stage_or_legacy(crate::obs::SPAN_POSTPROCESS, &spans) {
        return;
    }
    let _ = crate::start_span(
        pool,
        document_id,
        attempt,
        crate::obs::SPAN_POSTPROCESS,
        Some(crate::obs::ROOT_NAME),
        None,
    )
    .await;
    let _ = platform::enqueue_post_process(document_id, version_id, false).await;
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
    use crate::knowledge_index_v2::SemanticIndexPreparationV2;
    match crate::knowledge_index_v2::prepare_semantic_index_intent_v2(pool, product_version_id)
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

/// Enqueue already returns `Ok(None)` for `DeclaredDisabled` lanes.
/// Running generators here would re-enable those lanes in-process.
fn allow_inline_fallback(task_type: &str) -> bool {
    !matches!(
        crate::launch_mode(task_type),
        Ok(Some(crate::LaunchMode::DeclaredDisabled))
    )
}

/// Invoke `inline` only when the registry does not declare the lane disabled.

/// Invoke `inline` only when the registry does not declare the lane disabled.
fn run_inline_if_allowed<T>(task_type: &str, inline: impl FnOnce() -> T) -> Option<T> {
    allow_inline_fallback(task_type).then(inline)
}

#[tracing::instrument(
    name = "parse.postprocess",
    skip_all,
    fields(document_id = %document_id, clone_keep)
)]

pub async fn run_post_process(
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
    let rows = crate::list_spans_attempt(pool, document_id, attempt)
        .await
        .unwrap_or_default();
    let spans: Vec<_> = rows.into_iter().map(|r| r.into_span()).collect();
    if !crate::obs::can_start_stage_or_legacy(crate::obs::SPAN_POSTPROCESS, &spans) {
        return Err("postprocess waiting for embedding and multimodal".into());
    }
    let _ = crate::start_span(
        pool,
        document_id,
        attempt,
        crate::obs::SPAN_POSTPROCESS,
        Some(crate::obs::ROOT_NAME),
        None,
    )
    .await;
    let mut store = crate::Store::default();
    crate::hydrate_workspace(pool, &mut store, ws)
        .await
        .map_err(|e| e.to_string())?;
    post_process(
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
        let _ = crate::skip_span(
            pool,
            document_id,
            attempt,
            crate::obs::SPAN_POSTPROCESS,
            "aborted",
        )
        .await;
        return Ok(());
    }
    if doc.parse_status == crate::ParseStatus::Completed {
        crate::set_document_progress(pool, document_id, "completed", 0)
            .await
            .map_err(|e| e.to_string())?;
        let _ = crate::set_summary_status(pool, document_id, "none").await;
        finish_postprocess_spans(pool, document_id, attempt).await;
        return schedule_semantic_index_v2_if_ready(pool, product_version_id).await;
    }
    if doc.parse_status != crate::ParseStatus::Finalizing {
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
            .any(|j| j.task_type == platform::TYPE_WIKI_INGEST);
    if wiki_trigger {
        crate::enqueue_pending_op(
            pool,
            platform::TYPE_WIKI_INGEST,
            product_version_id,
            crate::wiki::OP_INGEST,
            Some(&document_id.to_string()),
            serde_json::json!({"document_id": document_id}),
        )
        .await
        .map_err(|e| e.to_string())?;
    }
    if !crate::set_finalizing(pool, document_id, doc.pending_subtasks_count)
        .await
        .map_err(|e| e.to_string())?
    {
        finish_postprocess_spans(pool, document_id, attempt).await;
        return Ok(());
    }
    if matches!(doc.summary_status, crate::SummaryStatus::Pending) {
        let _ = crate::set_summary_status(pool, document_id, "pending").await;
    }
    if store
        .queue
        .iter()
        .any(|j| j.task_type == platform::TYPE_SUMMARY)
    {
        match platform::enqueue_summary(document_id, attempt).await {
            Ok(Some(_)) => {}
            Ok(None) => {
                crate::enrichment::generate_summary(&mut store, document_id);
                let _ = crate::persist_summary_chunks(pool, &store, document_id).await;
                if let Some(d) = store.documents.get(&document_id) {
                    let st = match d.summary_status {
                        crate::SummaryStatus::Completed => "completed",
                        crate::SummaryStatus::Failed => "failed",
                        crate::SummaryStatus::Pending => "pending",
                        crate::SummaryStatus::Processing => "processing",
                        crate::SummaryStatus::None => "none",
                    };
                    let _ =
                        crate::set_document_description(pool, document_id, &d.description).await;
                    let _ = crate::set_summary_status(pool, document_id, st).await;
                }
                let _ = crate::finalize_subtask(pool, document_id).await;
            }
            Err(_) => {
                let _ = crate::finalize_subtask(pool, document_id).await;
            }
        }
    }
    let question_jobs: Vec<crate::Job> = store
        .queue
        .iter()
        .filter(|j| j.task_type == platform::TYPE_QUESTION)
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
        match platform::enqueue_question_neighbors(
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
                if run_inline_if_allowed(platform::TYPE_QUESTION, || {
                    crate::enrichment::generate_questions_with(
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
                    let _ = crate::persist_question_updates(pool, &store, document_id, &ids).await;
                }
                let _ = crate::finalize_subtask(pool, document_id).await;
            }
            Err(_) => {
                let _ = crate::finalize_subtask(pool, document_id).await;
            }
        }
    }
    let extracts: Vec<(Uuid, Uuid)> = store
        .queue
        .iter()
        .filter(|j| j.task_type == platform::TYPE_CHUNK_EXTRACT)
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
        match platform::enqueue_extract(*cid, *did, attempt).await {
            Ok(Some(_)) => {}
            Ok(None) => {
                if let Some(outcome) = run_inline_if_allowed(platform::TYPE_CHUNK_EXTRACT, || {
                    crate::graph::extract_chunk(&mut store, *cid, *did)
                }) {
                    outcome?;
                    let _ = crate::persist_graph_for_document(pool, &store, document_id).await;
                    let _ = crate::graph::sync_document(&store, document_id);
                }
                let _ = crate::finalize_subtask(pool, document_id).await;
            }
            Err(_) => {
                let _ = crate::finalize_subtask(pool, document_id).await;
            }
        }
    }
    if wiki_trigger {
        match platform::enqueue_wiki_ingest(product_version_id).await {
            Ok(Some(_)) => {}
            Ok(None) => {
                let _ = run_wiki_ingest(pool, product_version_id).await;
            }
            Err(_) => {
                let _ = crate::finalize_subtask(pool, document_id).await;
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
    let _ = crate::finish_span(
        pool,
        document_id,
        attempt,
        crate::obs::SPAN_POSTPROCESS,
        crate::obs::STATUS_DONE,
        None,
    )
    .await;
    let _ = crate::finish_span(
        pool,
        document_id,
        attempt,
        crate::obs::ROOT_NAME,
        crate::obs::STATUS_DONE,
        None,
    )
    .await;
}

pub async fn run_image(
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
    let ws: Option<Uuid> = crate::document_workspace_id(pool, document_id)
        .await
        .map_err(|e| e.to_string())?;
    let Some(ws) = ws else {
        return Ok(());
    };
    let mut store = crate::Store::default();
    crate::hydrate_workspace(pool, &mut store, ws)
        .await
        .map_err(|e| e.to_string())?;
    if let Some(d) = store.documents.get_mut(&document_id)
        && d.parse_status == crate::ParseStatus::Pending
    {
        d.parse_status = crate::ParseStatus::Processing;
    }
    if let Err(error) = crate::enrichment::process_image_without_decr(
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
    crate::delete_image_chunks(pool, document_id, image_key)
        .await
        .map_err(|e| e.to_string())?;
    crate::insert_document_chunks(pool, &image_chunks, &embeddings)
        .await
        .map_err(|e| e.to_string())?;
    if crate::enrichment::decr_pending(&mut store, document_id) {
        let _ = crate::set_index_ready(pool, document_id, true).await;
        let vid = store
            .documents
            .get(&document_id)
            .map(|d| d.product_version_id)
            .unwrap_or_default();
        let tracked = crate::list_spans_attempt(pool, document_id, attempt)
            .await
            .unwrap_or_default()
            .iter()
            .any(|r| r.name == crate::obs::SPAN_MULTIMODAL);
        if tracked {
            let _ = crate::finish_span(
                pool,
                document_id,
                attempt,
                crate::obs::SPAN_MULTIMODAL,
                crate::obs::STATUS_DONE,
                None,
            )
            .await;
            maybe_start_postprocess(pool, document_id, vid, attempt).await;
        } else {
            let _ = platform::enqueue_post_process(document_id, vid, false).await;
        }
    }
    Ok(())
}

/// Last-retry DECR so a dead image cannot pin `multimodal:pending`.

/// Last-retry DECR so a dead image cannot pin `multimodal:pending`.
pub async fn finalize_multimodal(pool: &PgPool, document_id: Uuid, attempt: i32) {
    let current: Option<i32> = sqlx::query_scalar("SELECT attempt FROM documents WHERE id = $1")
        .bind(document_id)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();
    if current.is_some_and(|n| n != attempt) {
        return;
    }
    let mut tmp = crate::Store::default();
    if !crate::enrichment::decr_pending(&mut tmp, document_id) {
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

/// Brain ProcessWikiIngest on PG `task_pending_ops` ingest lane only.
pub async fn run_wiki_ingest(pool: &PgPool, version_id: Uuid) -> Result<(), String> {
    if !crate::version_wiki_enabled(pool, version_id)
        .await
        .map_err(|e| e.to_string())?
    {
        tracing::info!(%version_id, "wiki ingest skipped: not enabled");
        let _ = crate::drop_pending_ops(pool, platform::TYPE_WIKI_INGEST, version_id).await;
        return schedule_semantic_index_v2_if_ready(pool, version_id).await;
    }
    let claimed = crate::claim_pending_batch(
        pool,
        platform::TYPE_WIKI_INGEST,
        version_id,
        crate::wiki::BATCH_DOCS as i64,
        crate::wiki::STALE_CLAIM_MIN as i64,
    )
    .await
    .map_err(|e| e.to_string())?;
    if claimed.is_empty() {
        return schedule_semantic_index_v2_if_ready(pool, version_id).await;
    }
    let mut store = crate::Store::default();
    if let Ok(Some(ws)) = crate::document_workspace_id(
        pool,
        claimed
            .iter()
            .find_map(|o| o.dedup_key.as_deref().and_then(|s| Uuid::parse_str(s).ok()))
            .unwrap_or(version_id),
    )
    .await
    {
        let _ = crate::hydrate_workspace(pool, &mut store, ws).await;
    }
    store.versions.entry(version_id).or_insert_with(|| {
        let mut v = crate::ProductVersion::new(Uuid::nil(), "v".into());
        v.id = version_id;
        v.wiki_enabled = true;
        v
    });
    let mut done = Vec::new();
    let mut slugs = Vec::new();
    let mut ingest_ops = Vec::new();
    for op in &claimed {
        if op.op == crate::wiki::OP_RETRACT {
            if let Some(did) = op
                .dedup_key
                .as_deref()
                .and_then(|s| Uuid::parse_str(s).ok())
            {
                crate::wiki::enqueue_retract(&mut store, version_id, did, "");
                let _ = crate::delete_wiki_for_document(pool, version_id, did).await;
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
            let mut doc = crate::Document::new(
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
        crate::wiki::enqueue_ingest(&mut store, version_id, did);
        crate::wiki::set_ingest_fail_count(&mut store, version_id, did, op.fail_count);
        ingest_ops.push((op.id, did));
    }
    if !ingest_ops.is_empty() {
        if let Err(e) = crate::wiki::process_ingest(&mut store, version_id) {
            for (_, did) in &ingest_ops {
                let _ = crate::upsert_span(
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
                if let Some(n) = crate::wiki::retryable_ingest_fail_count(&store, version_id, *did)
                {
                    crate::retry_pending_op(pool, *op_id, n)
                        .await
                        .map_err(|e| e.to_string())?;
                } else {
                    crate::finalize_subtask(pool, *did)
                        .await
                        .map_err(|e| e.to_string())?;
                    done.push(*op_id);
                }
            }
        } else {
            persist_wiki_store(pool, &store, version_id, None).await?;
            for (op_id, did) in &ingest_ops {
                if let Some(n) = crate::wiki::retryable_ingest_fail_count(&store, version_id, *did)
                {
                    crate::retry_pending_op(pool, *op_id, n)
                        .await
                        .map_err(|e| e.to_string())?;
                    continue;
                }
                crate::finalize_subtask(pool, *did)
                    .await
                    .map_err(|e| e.to_string())?;
                let _ = crate::upsert_span(
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
    crate::delete_pending_ids(pool, &done)
        .await
        .map_err(|e| e.to_string())?;
    for did in slugs {
        crate::enqueue_pending_op(
            pool,
            platform::TYPE_WIKI_FINALIZE,
            version_id,
            crate::wiki::OP_SLUG,
            Some(&did.to_string()),
            serde_json::json!({"document_id": did}),
        )
        .await
        .map_err(|e| e.to_string())?;
        let _ = crate::enqueue_pending_op(
            pool,
            platform::TYPE_WIKI_FINALIZE,
            version_id,
            crate::wiki::OP_CHANGE,
            None,
            serde_json::json!({"document_id": did}),
        )
        .await;
        let _ = crate::enqueue_pending_op(
            pool,
            platform::TYPE_WIKI_FINALIZE,
            version_id,
            crate::wiki::OP_FOLDER_PRUNE,
            None,
            serde_json::json!({}),
        )
        .await;
    }
    if crate::count_pending(pool, platform::TYPE_WIKI_FINALIZE, version_id)
        .await
        .map_err(|e| e.to_string())?
        > 0
    {
        let _ = platform::enqueue_wiki_finalize(version_id).await;
    }
    if crate::count_pending(pool, platform::TYPE_WIKI_INGEST, version_id)
        .await
        .map_err(|e| e.to_string())?
        > 0
    {
        let delay = if done.len() < claimed.len() {
            crate::wiki::LOCK_RETRY_SECS
        } else {
            crate::wiki::FOLLOW_UP_DEBOUNCE_SECS
        };
        let _ = platform::enqueue_wiki_ingest_in(version_id, delay).await;
    }
    schedule_semantic_index_v2_if_ready(pool, version_id).await
}

/// Brain ProcessWikiFinalize — finalize lane only, never ingest.

/// Brain ProcessWikiFinalize — finalize lane only, never ingest.
pub async fn run_wiki_finalize(pool: &PgPool, version_id: Uuid) -> Result<(), String> {
    let claimed = crate::claim_pending_batch(
        pool,
        platform::TYPE_WIKI_FINALIZE,
        version_id,
        5000,
        crate::wiki::STALE_CLAIM_MIN as i64,
    )
    .await
    .map_err(|e| e.to_string())?;
    if claimed.is_empty() {
        return schedule_semantic_index_v2_if_ready(pool, version_id).await;
    }
    let mut store = crate::Store::default();
    if let Ok(Some(ws)) = crate::version_workspace_id(pool, version_id).await {
        let _ = crate::hydrate_workspace(pool, &mut store, ws).await;
    }
    store.versions.entry(version_id).or_insert_with(|| {
        let mut v = crate::ProductVersion::new(Uuid::nil(), "v".into());
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
        crate::wiki::enqueue_finalize_op(&mut store, version_id, &op.op, &slug, &title);
    }
    crate::wiki::process_finalize(&mut store, version_id)?;
    persist_wiki_store(pool, &store, version_id, None).await?;
    let deferred: Vec<Uuid> = claimed
        .iter()
        .filter(|op| {
            store.wiki_ops.iter().any(|o| {
                o.lane == platform::TYPE_WIKI_FINALIZE
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
    crate::delete_pending_ids(pool, &done)
        .await
        .map_err(|e| e.to_string())?;
    crate::unclaim_pending_ids(pool, &deferred)
        .await
        .map_err(|e| e.to_string())?;
    if !deferred.is_empty() {
        let _ = platform::enqueue_wiki_finalize_in(version_id, crate::wiki::LOCK_RETRY_SECS).await;
    }
    schedule_semantic_index_v2_if_ready(pool, version_id).await
}

async fn persist_wiki_store(
    pool: &PgPool,
    store: &crate::Store,
    version_id: Uuid,
    document_id: Option<Uuid>,
) -> Result<(), String> {
    for page in store
        .wiki
        .values()
        .filter(|p| p.product_version_id == version_id)
    {
        let _ = crate::upsert_wiki_page(pool, page, document_id).await;
    }
    for folder in store
        .wiki_folders
        .values()
        .filter(|f| f.product_version_id == version_id)
    {
        let _ = crate::upsert_wiki_folder(pool, folder).await;
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
    crate::replace_wiki_page_chunks(pool, version_id, &slugs, &wiki_chunks, &wiki_emb)
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

pub async fn run_summary(
    pool: &PgPool,
    document_id: Uuid,
    attempt: i32,
    fallback: bool,
) -> Result<(), String> {
    if crate::document_parse_status(pool, document_id)
        .await
        .map_err(|error| error.to_string())?
        .as_deref()
        == Some("completed")
    {
        return schedule_semantic_index_for_document_v2(pool, document_id).await;
    }
    let ws = crate::document_workspace_id(pool, document_id)
        .await
        .map_err(|e| e.to_string())?;
    let Some(ws) = ws else {
        return Ok(());
    };
    let mut store = crate::Store::default();
    crate::hydrate_workspace(pool, &mut store, ws)
        .await
        .map_err(|e| e.to_string())?;
    let outcome =
        crate::enrichment::generate_summary_with(&mut store, document_id, attempt, fallback)?;
    if matches!(outcome, crate::enrichment::SummaryOutcome::Superseded) {
        return Ok(());
    }
    crate::persist_summary_chunks(pool, &store, document_id)
        .await
        .map_err(|e| e.to_string())?;
    if let Some(d) = store.documents.get(&document_id) {
        let _ = crate::set_document_description(pool, document_id, &d.description).await;
        let st = match d.summary_status {
            crate::SummaryStatus::Completed => "completed",
            crate::SummaryStatus::Failed => "failed",
            crate::SummaryStatus::Pending => "pending",
            crate::SummaryStatus::Processing => "processing",
            crate::SummaryStatus::None => "none",
        };
        let _ = crate::set_summary_status(pool, document_id, st).await;
    }
    crate::finalize_subtask(pool, document_id)
        .await
        .map_err(|e| e.to_string())?;
    schedule_semantic_index_for_document_v2(pool, document_id).await
}

pub async fn run_questions(
    pool: &PgPool,
    document_id: Uuid,
    chunk_ids: &[Uuid],
    prev_ids: &[Option<Uuid>],
    next_ids: &[Option<Uuid>],
    attempt: i32,
) -> Result<(), String> {
    if crate::document_parse_status(pool, document_id)
        .await
        .map_err(|error| error.to_string())?
        .as_deref()
        == Some("completed")
    {
        return schedule_semantic_index_for_document_v2(pool, document_id).await;
    }
    let ws = crate::document_workspace_id(pool, document_id)
        .await
        .map_err(|e| e.to_string())?;
    let Some(ws) = ws else {
        return Ok(());
    };
    let mut store = crate::Store::default();
    crate::hydrate_workspace(pool, &mut store, ws)
        .await
        .map_err(|e| e.to_string())?;
    let outcome = crate::enrichment::generate_questions_with(
        &mut store,
        chunk_ids,
        prev_ids,
        next_ids,
        document_id,
        attempt,
    )?;
    if matches!(outcome, crate::enrichment::QuestionOutcome::Superseded) {
        return Ok(());
    }
    crate::persist_question_updates(pool, &store, document_id, chunk_ids)
        .await
        .map_err(|e| e.to_string())?;
    crate::finalize_subtask(pool, document_id)
        .await
        .map_err(|e| e.to_string())?;
    schedule_semantic_index_for_document_v2(pool, document_id).await
}

pub async fn run_extract(
    pool: &PgPool,
    chunk_id: Uuid,
    document_id: Uuid,
    attempt: i32,
) -> Result<(), String> {
    if crate::document_parse_status(pool, document_id)
        .await
        .map_err(|error| error.to_string())?
        .as_deref()
        == Some("completed")
    {
        return schedule_semantic_index_for_document_v2(pool, document_id).await;
    }
    let ws = crate::document_workspace_id(pool, document_id)
        .await
        .map_err(|e| e.to_string())?;
    let Some(ws) = ws else {
        return Ok(());
    };
    let mut store = crate::Store::default();
    crate::hydrate_workspace(pool, &mut store, ws)
        .await
        .map_err(|e| e.to_string())?;
    let outcome =
        crate::graph::extract_chunk_for_attempt(&mut store, chunk_id, document_id, attempt)?;
    if matches!(outcome, crate::graph::ExtractOutcome::Superseded) {
        return Ok(());
    }
    crate::persist_graph_for_document(pool, &store, document_id)
        .await
        .map_err(|e| e.to_string())?;
    let _ = crate::graph::sync_document(&store, document_id);
    crate::finalize_subtask(pool, document_id)
        .await
        .map_err(|e| e.to_string())?;
    schedule_semantic_index_for_document_v2(pool, document_id).await
}

pub async fn run_list_delete(pool: &PgPool, document_id: Uuid) -> Result<(), String> {
    let status = crate::document_parse_status(pool, document_id)
        .await
        .map_err(|e| e.to_string())?;
    if status.as_deref() != Some("deleting") {
        return Ok(());
    }
    let ws = crate::document_workspace_id(pool, document_id)
        .await
        .map_err(|e| e.to_string())?;
    let vid: Option<Uuid> =
        sqlx::query_scalar("SELECT product_version_id FROM documents WHERE id = $1")
            .bind(document_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?;
    if let Some(vid) = vid {
        let _ = crate::delete_wiki_for_document(pool, vid, document_id).await;
        let _ = crate::graph::delete_document(vid, document_id);
    }
    let _ = crate::purge_document_index(pool, document_id).await;
    let _ = platform::enqueue_index_delete(document_id).await;
    platform::release_knowledge_document_object(
        pool,
        document_id,
        "system:knowledge-document-delete",
        &format!("knowledge-document-delete:{document_id}"),
    )
    .await
    .map_err(|e| e.to_string())?;
    let _ = ws;
    Ok(())
}

pub async fn run_datatable(pool: &PgPool, document_id: Uuid) -> Result<(), String> {
    let ws = crate::document_workspace_id(pool, document_id)
        .await
        .map_err(|e| e.to_string())?;
    let Some(ws) = ws else {
        return Ok(());
    };
    let mut store = Store::default();
    crate::hydrate_workspace(pool, &mut store, ws)
        .await
        .map_err(|e| e.to_string())?;
    datatable_summary(&mut store, document_id)?;
    let table: Vec<_> = store
        .chunks
        .values()
        .filter(|c| {
            c.document_id == document_id
                && matches!(c.chunk_type.as_str(), "table_summary" | "table_column")
        })
        .cloned()
        .collect();
    let embeds: Vec<_> = table
        .iter()
        .filter_map(|c| store.embeddings.get(&c.id).cloned())
        .collect();
    crate::delete_chunks_by_types(pool, document_id, &["table_summary", "table_column"])
        .await
        .map_err(|e| e.to_string())?;
    crate::append_document_chunks(pool, &table, &embeds)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn is_table_file(name: &str) -> bool {
    matches!(
        name.rsplit('.')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str(),
        "csv" | "xlsx" | "xls"
    )
}

pub fn datatable_summary(store: &mut Store, document_id: Uuid) -> Result<(), String> {
    let Some(doc) = store.documents.get(&document_id).cloned() else {
        return Ok(());
    };
    let Some(version) = store.effective_version(document_id) else {
        return Ok(());
    };
    if !is_table_file(&doc.file_name) {
        return Ok(());
    }
    let ext = doc
        .file_name
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let bytes = store
        .objects
        .get(&doc.object_ref)
        .cloned()
        .unwrap_or_default();
    let markdown = converted_markdown(&doc);
    let (headers, rows) = if ext == "csv" {
        parse_csv_sample(&bytes)
    } else {
        sample_from_converted_markdown(&markdown)
    };
    if headers.is_empty() {
        if matches!(ext.as_str(), "xlsx" | "xls") && markdown.trim().is_empty() {
            return Err("datatable waiting for docreader convert markdown".into());
        }
        return Ok(());
    }
    drop_prior_table_chunks(store, document_id);
    let schema = headers
        .iter()
        .enumerate()
        .map(|(i, h)| format!("- col{i}: {h}"))
        .collect::<Vec<_>>()
        .join("\n");
    let sample = sample_rows_json(&headers, &rows);
    let table_name = doc
        .file_name
        .rsplit('/')
        .next()
        .unwrap_or(doc.file_name.as_str());
    let table_prompt = crate::enrichment::append_custom_instructions(
        &crate::enrichment::render_table_prompt(
            crate::enrichment::TABLE_DESCRIPTION_PROMPT,
            table_name,
            &schema,
            &sample,
        ),
        &version.table_metadata_instructions,
        "table_metadata",
    );
    let col_prompt = crate::enrichment::append_custom_instructions(
        &crate::enrichment::render_table_prompt(
            crate::enrichment::COLUMN_DESCRIPTIONS_PROMPT,
            table_name,
            &schema,
            &sample,
        ),
        &version.table_metadata_instructions,
        "table_metadata",
    );
    let table_raw = chat_table(&table_prompt, &sample, &version.summary_model_id, || {
        format!(
            "tabular file {table_name} with columns: {}",
            headers.join(", ")
        )
    })?;
    let table_content = format!("# Table Summary\n\nTable name: {table_name}\n\n{table_raw}");
    let col_raw = chat_table(&col_prompt, &sample, &version.summary_model_id, || {
        headers.join(", ")
    })?;
    let col_content =
        format!("# Table Column Information\n\nTable name: {table_name}\n\n{col_raw}");
    let summary = crate::Chunk {
        id: Uuid::new_v4(),
        document_id,
        product_version_id: doc.product_version_id,
        chunk_type: "table_summary".into(),
        content: table_content.clone(),
        context_header: String::new(),
        start_at: 0,
        end_at: table_content.chars().count() as i32,
        parent_chunk_id: None,
        generated_questions: Vec::new(),
    };
    let column = crate::Chunk {
        id: Uuid::new_v4(),
        document_id,
        product_version_id: doc.product_version_id,
        chunk_type: "table_column".into(),
        content: col_content.clone(),
        context_header: String::new(),
        start_at: 0,
        end_at: col_content.chars().count() as i32,
        parent_chunk_id: Some(summary.id),
        generated_questions: Vec::new(),
    };
    for ch in [&summary, &column] {
        crate::index::index_one(
            store,
            ch,
            &doc.title,
            version.vector_enabled,
            version.keyword_enabled,
        )?;
    }
    store.chunks.insert(summary.id, summary);
    store.chunks.insert(column.id, column);
    Ok(())
}

fn chat_table(
    prompt: &str,
    user: &str,
    model_id: &str,
    fallback: impl FnOnce() -> String,
) -> Result<String, String> {
    match crate::enrichment::chat_complete(prompt, user, model_id) {
        Ok(s) if !s.trim().is_empty() => Ok(s),
        other => {
            if crate::enrichment::chat_http_configured() && model_id != "stub-chat" {
                return Err(other.err().unwrap_or_else(|| "chat empty".into()));
            }
            Ok(fallback())
        }
    }
}

fn sample_rows_json(headers: &[String], rows: &[Vec<String>]) -> String {
    let mut out = format!("Sample data (first {} rows):\n", rows.len().min(10));
    for row in rows.iter().take(10) {
        let mut obj = serde_json::Map::new();
        for (i, h) in headers.iter().enumerate() {
            obj.insert(
                h.clone(),
                serde_json::Value::String(row.get(i).cloned().unwrap_or_default()),
            );
        }
        if let Ok(s) = serde_json::to_string(&obj) {
            out.push_str(&s);
            out.push('\n');
        }
    }
    out
}

fn drop_prior_table_chunks(store: &mut Store, document_id: Uuid) {
    let drop: Vec<Uuid> = store
        .chunks
        .values()
        .filter(|c| {
            c.document_id == document_id
                && matches!(c.chunk_type.as_str(), "table_summary" | "table_column")
        })
        .map(|c| c.id)
        .collect();
    for id in drop {
        store.chunks.remove(&id);
        store.embeddings.remove(&id);
    }
}

fn converted_markdown(doc: &crate::Document) -> String {
    if !doc.markdown.trim().is_empty() {
        return doc.markdown.clone();
    }
    platform::read_blob(&format!("{}.md", doc.file_hash))
        .ok()
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default()
}

fn sample_from_converted_markdown(markdown: &str) -> (Vec<String>, Vec<Vec<String>>) {
    if let Some(t) = parse_markdown_table(markdown) {
        return t;
    }
    parse_excel_kv_rows(markdown)
}

/// DocReader `ExcelParser` emits `col: val,col: val` rows, not a Markdown table.
fn parse_excel_kv_rows(text: &str) -> (Vec<String>, Vec<Vec<String>>) {
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    let Some(first) = lines.next() else {
        return (Vec::new(), Vec::new());
    };
    let headers = kv_keys(first);
    if headers.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let mut rows = vec![kv_values(first, &headers)];
    for line in lines.take(9) {
        rows.push(kv_values(line, &headers));
    }
    (headers, rows)
}

fn kv_keys(line: &str) -> Vec<String> {
    line.split(',')
        .filter_map(|part| part.split_once(':').map(|(k, _)| k.trim().to_string()))
        .filter(|k| !k.is_empty())
        .collect()
}

fn kv_values(line: &str, headers: &[String]) -> Vec<String> {
    let mut map = std::collections::HashMap::new();
    for part in line.split(',') {
        if let Some((k, v)) = part.split_once(':') {
            map.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    headers
        .iter()
        .map(|h| map.get(h).cloned().unwrap_or_default())
        .collect()
}

fn parse_csv_sample(bytes: &[u8]) -> (Vec<String>, Vec<Vec<String>>) {
    let text = String::from_utf8_lossy(bytes);
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let Some(header) = lines.next() else {
        return (Vec::new(), Vec::new());
    };
    let headers = split_csv_line(header);
    if headers.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let rows: Vec<Vec<String>> = lines.take(10).map(split_csv_line).collect();
    (headers, rows)
}

fn split_csv_line(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '"' {
            if quoted && chars.peek() == Some(&'"') {
                cur.push('"');
                chars.next();
            } else {
                quoted = !quoted;
            }
        } else if c == ',' && !quoted {
            out.push(cur.trim().to_string());
            cur.clear();
        } else {
            cur.push(c);
        }
    }
    out.push(cur.trim().to_string());
    out
}

fn parse_markdown_table(md: &str) -> Option<(Vec<String>, Vec<Vec<String>>)> {
    let mut lines = md.lines().filter(|l| l.trim().starts_with('|'));
    let header = lines.next()?;
    let _sep = lines.next();
    let headers: Vec<String> = header
        .trim()
        .trim_matches('|')
        .split('|')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if headers.is_empty() {
        return None;
    }
    let rows: Vec<Vec<String>> = lines
        .take(10)
        .map(|l| {
            l.trim()
                .trim_matches('|')
                .split('|')
                .map(|s| s.trim().to_string())
                .collect()
        })
        .collect();
    Some((headers, rows))
}
