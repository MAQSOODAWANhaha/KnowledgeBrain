//! Worker handlers. HTTP never calls these.
#![recursion_limit = "512"]

pub mod consume;
pub mod live_recovery;
pub mod probe;

use domain::{
    ParseStatus, Store, TYPE_BID_CONVERT, TYPE_BID_EXTRACT, TYPE_BID_MATCH_ROUTE_V1,
    TYPE_BID_RENDER_SUBMISSION_V1, TYPE_CHUNK_EXTRACT, TYPE_DATATABLE, TYPE_DOCUMENT_PROCESS,
    TYPE_IMAGE_MULTIMODAL, TYPE_KB_DELETE, TYPE_LIST_DELETE, TYPE_LIST_REPARSE,
    TYPE_MANUAL_PROCESS, TYPE_POST_PROCESS, TYPE_QUESTION, TYPE_SUMMARY, TYPE_VERSION_CLONE,
    TYPE_WIKI_FINALIZE, TYPE_WIKI_INGEST, expected_subtasks,
};
use uuid::Uuid;

pub fn drain(store: &mut Store) {
    let mut guard = 0u32;
    while !store.queue.is_empty() && guard < 20_000 {
        guard += 1;
        let Some(job) = store.pop_queue() else {
            break;
        };
        if let Err(e) = handle(store, &job) {
            if job.retries + 1 >= job.max_retry {
                let related = job
                    .payload
                    .get("document_id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| Uuid::parse_str(s).ok())
                    .unwrap_or(job.id);
                store.dead_letter(&job.task_type, related, &e);
                match job.task_type.as_str() {
                    TYPE_DOCUMENT_PROCESS | TYPE_POST_PROCESS | TYPE_MANUAL_PROCESS => {
                        store.fail_document(related, &e);
                    }
                    TYPE_IMAGE_MULTIMODAL => {
                        if enrichment::decr_pending(store, related)
                            && store.documents.contains_key(&related)
                        {
                            store.enqueue(
                                TYPE_POST_PROCESS,
                                domain::QUEUE_POSTPROCESS,
                                serde_json::json!({
                                    "document_id": related,
                                    "clone_keep": false
                                }),
                            );
                        }
                    }
                    TYPE_SUMMARY => {
                        store.finalize_subtask(related);
                        if let Some(d) = store.documents.get_mut(&related) {
                            d.summary_status = domain::SummaryStatus::Failed;
                        }
                    }
                    TYPE_QUESTION => {
                        store.finalize_subtask(related);
                    }
                    TYPE_CHUNK_EXTRACT => {
                        store.finalize_subtask(related);
                    }
                    TYPE_WIKI_INGEST => {
                        if let Some(vid) = job
                            .payload
                            .get("product_version_id")
                            .and_then(|v| v.as_str())
                            .and_then(|s| Uuid::parse_str(s).ok())
                        {
                            wiki::fail_open_pending(store, vid);
                        }
                    }
                    TYPE_LIST_DELETE => {
                        if let Some(d) = store.documents.get_mut(&related)
                            && d.parse_status == ParseStatus::Deleting
                        {
                            d.parse_status = ParseStatus::Failed;
                            d.error_message = e;
                        }
                    }
                    _ => {}
                }
            } else {
                let mut j = job;
                j.retries += 1;
                store.queue.push_back(j);
            }
        }
    }
}

pub(crate) fn handle(store: &mut Store, job: &domain::Job) -> Result<(), String> {
    match job.task_type.as_str() {
        TYPE_BID_CONVERT
        | TYPE_BID_EXTRACT
        | TYPE_BID_MATCH_ROUTE_V1
        | TYPE_BID_RENDER_SUBMISSION_V1 => Ok(()),
        TYPE_DOCUMENT_PROCESS | TYPE_MANUAL_PROCESS => document_process(store, &job.payload),
        TYPE_POST_PROCESS => post_process(store, &job.payload),
        TYPE_SUMMARY => {
            let id = req_uuid(&job.payload, "document_id")?;
            let attempt = job
                .payload
                .get("attempt")
                .and_then(|v| v.as_i64())
                .unwrap_or(0) as i32;
            enrichment::generate_summary_with(store, id, attempt, job.retries + 1 >= job.max_retry)
                .map(|_| ())
        }
        TYPE_QUESTION => {
            let id = req_uuid(&job.payload, "document_id")?;
            let attempt = job
                .payload
                .get("attempt")
                .and_then(|v| v.as_i64())
                .unwrap_or(0) as i32;
            let ids: Vec<Uuid> = job
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
                job.payload
                    .get(key)
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .map(|x| x.as_str().and_then(|s| Uuid::parse_str(s).ok()))
                            .collect::<Vec<Option<Uuid>>>()
                    })
                    .unwrap_or_default()
            };
            enrichment::generate_questions_with(
                store,
                &ids,
                &parse_opt("prev_ids"),
                &parse_opt("next_ids"),
                id,
                attempt,
            )
            .map(|_| ())
        }
        TYPE_CHUNK_EXTRACT => {
            let cid = req_uuid(&job.payload, "chunk_id")?;
            let did = req_uuid(&job.payload, "document_id")?;
            let attempt = job
                .payload
                .get("attempt")
                .and_then(|v| v.as_i64())
                .unwrap_or(0) as i32;
            graph::extract_chunk_for_attempt(store, cid, did, attempt).map(|_| ())
        }
        TYPE_WIKI_INGEST => {
            let vid = req_uuid(&job.payload, "product_version_id")?;
            wiki::process_ingest(store, vid)
        }
        TYPE_WIKI_FINALIZE => {
            let vid = req_uuid(&job.payload, "product_version_id")?;
            wiki::process_finalize(store, vid)
        }
        TYPE_IMAGE_MULTIMODAL => {
            let did = req_uuid(&job.payload, "document_id")?;
            let key = job
                .payload
                .get("image_key")
                .and_then(|v| v.as_str())
                .unwrap_or("images/x");
            let src = job
                .payload
                .get("image_source_type")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let ocr = job
                .payload
                .get("enable_ocr")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let cap = job
                .payload
                .get("enable_caption")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            enrichment::process_image_with(store, did, key, src, ocr, cap)?;
            if store.queue.iter().any(|j| j.task_type == TYPE_POST_PROCESS) {
                let attempt = store.documents.get(&did).map(|d| d.attempt).unwrap_or(1);
                obs::finish(store, did, obs::SPAN_MULTIMODAL, obs::STATUS_DONE);
                let rows = obs::timeline(store, did);
                if obs::can_start_stage(obs::SPAN_POSTPROCESS, &rows) {
                    obs::start(
                        store,
                        did,
                        attempt,
                        obs::SPAN_POSTPROCESS,
                        Some(obs::ROOT_NAME),
                    );
                }
            }
            Ok(())
        }
        TYPE_VERSION_CLONE => {
            let _ = store;
            Err("version:clone applies on Postgres via clone::run_clone, not memory drain".into())
        }
        TYPE_KB_DELETE => kb_delete(store, &job.payload),
        TYPE_LIST_DELETE => list_delete(store, &job.payload),
        TYPE_LIST_REPARSE => list_reparse(store, &job.payload),
        TYPE_DATATABLE => {
            let did = req_uuid(&job.payload, "document_id")?;
            datatable_summary(store, did)
        }
        other => Err(format!("unknown task {other}")),
    }
}

fn persist_inline_images(store: &mut Store, result: docparser::ReadResult) -> String {
    let (md, blobs) = docparser::rewrite_inline(&result);
    for (hash, data) in blobs {
        let key = storage::object_ref(&hash);
        store.objects.insert(key, data.clone());
        let _ = storage::write_blob_off_runtime(&hash, &data);
    }
    md
}

fn persist_index_snapshot(store: &Store, document_id: Uuid) {
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        return;
    };
    let chunks: Vec<_> = store
        .chunks
        .values()
        .filter(|c| c.document_id == document_id)
        .cloned()
        .collect();
    let embeddings: Vec<_> = store
        .embeddings
        .values()
        .filter(|e| e.document_id == document_id)
        .cloned()
        .collect();
    handle.spawn(async move {
        if let Ok(pool) = storage::connect().await {
            let n = storage::document_chunk_count(&pool, document_id)
                .await
                .unwrap_or(1);
            if n == 0 {
                let _ = storage::insert_document_chunks(&pool, &chunks, &embeddings).await;
            }
        }
    });
}

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

fn document_process(store: &mut Store, payload: &serde_json::Value) -> Result<(), String> {
    let doc_id = req_uuid(payload, "document_id")?;
    let Some(doc) = store.documents.get(&doc_id).cloned() else {
        return Err("document missing".into());
    };
    if doc.parse_status.worker_should_exit() {
        return Ok(());
    }
    if store.mark_processing(doc_id).is_err() {
        return Ok(());
    }
    if store
        .documents
        .get(&doc_id)
        .is_some_and(|d| d.parse_status.is_aborted())
    {
        return Ok(());
    }
    obs::start(store, doc_id, doc.attempt, obs::ROOT_NAME, None);
    obs::start(
        store,
        doc_id,
        doc.attempt,
        obs::SPAN_DOCREADER,
        Some(obs::ROOT_NAME),
    );
    let is_manual = payload
        .get("manual")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let mut convert_image_source = String::new();
    let markdown = if let Some(passages) = payload.get("passages").and_then(|v| v.as_array()) {
        passages
            .iter()
            .filter_map(|p| p.as_str())
            .collect::<Vec<_>>()
            .join("\n\n")
    } else if is_manual {
        store
            .objects
            .get(&doc.object_ref)
            .cloned()
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default()
    } else {
        let bytes = store
            .objects
            .get(&doc.object_ref)
            .cloned()
            .unwrap_or_default();
        let ext = doc
            .file_name
            .rsplit('.')
            .next()
            .unwrap_or("txt")
            .to_string();
        // In-memory drain is HTTP/unit-test only. Office/PDF convert lives in consume.rs.
        if !domain::is_simple_format(&ext)
            && !domain::is_image_type(&doc.file_name)
            && !domain::is_audio_type(&doc.file_name)
        {
            obs::finish(store, doc_id, obs::SPAN_DOCREADER, obs::STATUS_FAILED);
            obs::cascade_cancel(store, doc_id, doc.attempt, obs::SPAN_DOCREADER);
            store.fail_document(
                doc_id,
                "in-memory drain does not convert this type; oxana worker is the parse path",
            );
            return Ok(());
        }
        let r = docparser::convert_simple(&doc.file_name, &bytes);
        if !r.error.is_empty() {
            obs::finish(store, doc_id, obs::SPAN_DOCREADER, obs::STATUS_FAILED);
            obs::cascade_cancel(store, doc_id, doc.attempt, obs::SPAN_DOCREADER);
            return Err(r.error);
        }
        convert_image_source = r
            .metadata
            .get("image_source_type")
            .cloned()
            .unwrap_or_default();
        if r.is_audio {
            let version = store.effective_version(doc_id).ok_or("version missing")?;
            if !version.asr_enabled || version.asr_model_id.is_empty() {
                obs::finish(store, doc_id, obs::SPAN_DOCREADER, obs::STATUS_FAILED);
                obs::cascade_cancel(store, doc_id, doc.attempt, obs::SPAN_DOCREADER);
                store.fail_document(doc_id, docparser::ASR_NOT_CONFIGURED);
                return Ok(());
            }
            if version.asr_model_id == "stub-asr" {
                docparser::apply_asr_stub(r, &doc.file_name).markdown
            } else {
                obs::finish(store, doc_id, obs::SPAN_DOCREADER, obs::STATUS_FAILED);
                obs::cascade_cancel(store, doc_id, doc.attempt, obs::SPAN_DOCREADER);
                store.fail_document(doc_id, docparser::ASR_NOT_CONFIGURED);
                return Ok(());
            }
        } else {
            persist_inline_images(store, r)
        }
    };
    obs::finish(store, doc_id, obs::SPAN_DOCREADER, obs::STATUS_DONE);
    if let Some(d) = store.documents.get_mut(&doc_id) {
        d.markdown = markdown.clone();
    }
    obs::start(
        store,
        doc_id,
        doc.attempt,
        obs::SPAN_CHUNKING,
        Some(obs::ROOT_NAME),
    );
    let version = store.effective_version(doc_id).ok_or("version missing")?;
    let chunks = if payload.get("passages").and_then(|v| v.as_array()).is_some() {
        payload
            .get("passages")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|p| p.as_str())
            .map(|text| domain::Chunk {
                id: Uuid::new_v4(),
                document_id: doc_id,
                product_version_id: doc.product_version_id,
                chunk_type: "text".into(),
                content: text.to_string(),
                context_header: String::new(),
                start_at: 0,
                end_at: text.chars().count() as i32,
                parent_chunk_id: None,
                generated_questions: Vec::new(),
            })
            .collect()
    } else {
        chunker::split_from_config(
            &markdown,
            doc.product_version_id,
            doc_id,
            chunker::SplitterConfig {
                chunk_size: version.chunk_size,
                chunk_overlap: version.chunk_overlap,
                strategy: version.chunk_strategy.clone(),
                separators: version.chunk_separators.clone(),
                token_limit: version.chunk_token_limit,
                languages: version.chunk_languages.clone(),
            },
            version.enable_parent_child,
            version.parent_chunk_size,
            version.child_chunk_size,
        )
    };
    obs::finish(store, doc_id, obs::SPAN_CHUNKING, obs::STATUS_DONE);
    let images = enrichment::markdown_image_keys(&markdown);
    let has_mm = version.enable_multimodel && !images.is_empty();
    obs::start(
        store,
        doc_id,
        doc.attempt,
        obs::SPAN_EMBEDDING,
        Some(obs::ROOT_NAME),
    );
    index::process_chunks(store, doc_id, chunks, has_mm)?;
    persist_index_snapshot(store, doc_id);
    obs::finish(store, doc_id, obs::SPAN_EMBEDDING, obs::STATUS_DONE);
    if store
        .documents
        .get(&doc_id)
        .is_some_and(|d| d.parse_status == ParseStatus::Completed)
    {
        obs::skip(
            store,
            doc_id,
            doc.attempt,
            obs::SPAN_MULTIMODAL,
            "no images",
        );
        obs::skip(
            store,
            doc_id,
            doc.attempt,
            obs::SPAN_POSTPROCESS,
            "no further work",
        );
        obs::finish(store, doc_id, obs::ROOT_NAME, obs::STATUS_DONE);
        return Ok(());
    }
    if has_mm && !domain::vlm_configured() {
        if let Some(d) = store.documents.get_mut(&doc_id) {
            d.index_ready = false;
            d.parse_status = ParseStatus::Finalizing;
            d.error_message =
                "ocr_error: vlm not configured; caption_error: vlm not configured".into();
        }
        obs::skip(
            store,
            doc_id,
            doc.attempt,
            obs::SPAN_MULTIMODAL,
            "vlm not configured",
        );
        let rows = obs::timeline(store, doc_id);
        if obs::can_start_stage(obs::SPAN_POSTPROCESS, &rows) {
            obs::start(
                store,
                doc_id,
                doc.attempt,
                obs::SPAN_POSTPROCESS,
                Some(obs::ROOT_NAME),
            );
            store.enqueue(
                TYPE_POST_PROCESS,
                domain::QUEUE_POSTPROCESS,
                serde_json::json!({
                    "document_id": doc_id,
                    "product_version_id": doc.product_version_id,
                    "clone_keep": false
                }),
            );
        }
    } else if has_mm {
        obs::start(
            store,
            doc_id,
            doc.attempt,
            obs::SPAN_MULTIMODAL,
            Some(obs::ROOT_NAME),
        );
        enrichment::set_pending(store, doc_id, images.len() as i32);
        let source = if convert_image_source.is_empty() {
            enrichment::image_source_type(&doc.file_name, &markdown).to_string()
        } else {
            convert_image_source.clone()
        };
        for key in images {
            store.enqueue(
                TYPE_IMAGE_MULTIMODAL,
                domain::QUEUE_MULTIMODAL,
                serde_json::json!({
                    "document_id": doc_id,
                    "image_key": key,
                    "image_source_type": source,
                    "enable_ocr": true,
                    "enable_caption": true
                }),
            );
        }
    } else {
        obs::skip(
            store,
            doc_id,
            doc.attempt,
            obs::SPAN_MULTIMODAL,
            "no images",
        );
        let rows = obs::timeline(store, doc_id);
        if obs::can_start_stage(obs::SPAN_POSTPROCESS, &rows) {
            obs::start(
                store,
                doc_id,
                doc.attempt,
                obs::SPAN_POSTPROCESS,
                Some(obs::ROOT_NAME),
            );
            store.enqueue(
                TYPE_POST_PROCESS,
                domain::QUEUE_POSTPROCESS,
                serde_json::json!({
                    "document_id": doc_id,
                    "product_version_id": doc.product_version_id,
                    "clone_keep": false
                }),
            );
        }
    }
    Ok(())
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
    let rows = obs::timeline(store, doc_id);
    if !obs::can_start_stage_or_legacy(obs::SPAN_POSTPROCESS, &rows) {
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
            d.summary_status = domain::SummaryStatus::None;
        }
        obs::finish(store, doc_id, obs::SPAN_POSTPROCESS, obs::STATUS_DONE);
        obs::finish(store, doc_id, obs::ROOT_NAME, obs::STATUS_DONE);
        return Ok(());
    }
    if !store.set_finalizing(doc_id, n as i32) {
        return Ok(());
    }
    if text_count + ocr_count > 0 && !clone_keep {
        if let Some(d) = store.documents.get_mut(&doc_id) {
            d.summary_status = domain::SummaryStatus::Pending;
        }
        store.enqueue(
            TYPE_SUMMARY,
            domain::QUEUE_SUMMARY,
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
                domain::QUEUE_QUESTION,
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
        wiki::enqueue_ingest(store, doc.product_version_id, doc_id);
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
                domain::QUEUE_GRAPH,
                serde_json::json!({
                    "chunk_id": cid,
                    "document_id": doc_id,
                    "attempt": doc.attempt
                }),
            );
        }
    }
    obs::finish(store, doc_id, obs::SPAN_POSTPROCESS, obs::STATUS_DONE);
    Ok(())
}

fn kb_delete(store: &mut Store, payload: &serde_json::Value) -> Result<(), String> {
    let vid = payload
        .get("product_version_id")
        .or_else(|| payload.get("knowledge_base_id"))
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or("missing product_version_id")?;
    let docs: Vec<_> = store
        .documents
        .values()
        .filter(|d| d.product_version_id == vid)
        .map(|d| d.id)
        .collect();
    for id in docs {
        delete_document(store, id);
    }
    if let Some(v) = store.versions.get_mut(&vid) {
        v.status = domain::VersionStatus::Archived;
    }
    Ok(())
}

fn list_delete(store: &mut Store, payload: &serde_json::Value) -> Result<(), String> {
    let ids: Vec<Uuid> = payload
        .get("document_ids")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().and_then(|s| Uuid::parse_str(s).ok()))
                .collect()
        })
        .unwrap_or_default();
    for id in ids {
        if store
            .documents
            .get(&id)
            .is_some_and(|d| d.parse_status == ParseStatus::Deleting)
        {
            delete_document(store, id);
        }
    }
    Ok(())
}

fn list_reparse(store: &mut Store, payload: &serde_json::Value) -> Result<(), String> {
    let ids: Vec<Uuid> = payload
        .get("document_ids")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().and_then(|s| Uuid::parse_str(s).ok()))
                .collect()
        })
        .unwrap_or_default();
    for id in ids {
        reparse_document(store, id);
    }
    Ok(())
}

pub fn delete_document(store: &mut Store, id: Uuid) {
    if let Some(d) = store.documents.get(&id).cloned() {
        wiki::enqueue_retract(store, d.product_version_id, id, &d.title);
        let _ = graph::delete_document(d.product_version_id, id);
        store.clear_document_index(id);
        forget_in_memory_object(store, &d.file_hash);
        store.documents.remove(&id);
    }
}

fn forget_in_memory_object(store: &mut Store, hash: &str) {
    store.objects.remove(&format!("objects/{hash}"));
}

pub fn reparse_document(store: &mut Store, id: Uuid) {
    let Some(doc) = store.documents.get(&id).cloned() else {
        return;
    };
    store.clear_document_index(id);
    if let Some(d) = store.documents.get_mut(&id) {
        d.parse_status = ParseStatus::Pending;
        d.enable_status = "disabled".into();
        d.pending_subtasks_count = 0;
        d.attempt += 1;
        d.error_message.clear();
    }
    store.enqueue(
        TYPE_DOCUMENT_PROCESS,
        domain::QUEUE_DEFAULT,
        serde_json::json!({
            "document_id": id,
            "product_version_id": doc.product_version_id,
            "attempt": doc.attempt + 1
        }),
    );
    if is_table_file(&doc.file_name) {
        store.enqueue(
            TYPE_DATATABLE,
            domain::QUEUE_SUMMARY,
            serde_json::json!({ "document_id": id }),
        );
    }
}

pub fn cancel_document(store: &mut Store, id: Uuid) {
    if let Some(d) = store.documents.get_mut(&id) {
        d.parse_status = ParseStatus::Cancelled;
    }
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

fn datatable_summary(store: &mut Store, document_id: Uuid) -> Result<(), String> {
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
    let table_prompt = enrichment::append_custom_instructions(
        &enrichment::render_table_prompt(
            enrichment::TABLE_DESCRIPTION_PROMPT,
            table_name,
            &schema,
            &sample,
        ),
        &version.table_metadata_instructions,
        "table_metadata",
    );
    let col_prompt = enrichment::append_custom_instructions(
        &enrichment::render_table_prompt(
            enrichment::COLUMN_DESCRIPTIONS_PROMPT,
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
    let summary = domain::Chunk {
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
    let column = domain::Chunk {
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
        index::index_one(
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
    match enrichment::chat_complete(prompt, user, model_id) {
        Ok(s) if !s.trim().is_empty() => Ok(s),
        other => {
            if enrichment::chat_http_configured() && model_id != "stub-chat" {
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

fn converted_markdown(doc: &domain::Document) -> String {
    if !doc.markdown.trim().is_empty() {
        return doc.markdown.clone();
    }
    storage::read_blob(&format!("{}.md", doc.file_hash))
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

#[cfg(test)]
mod tests {
    use super::*;
    use domain::{Document, Product, ProductKind, ProductVersion, Workspace};

    fn seed() -> (Store, Uuid, Uuid) {
        let mut s = Store::default();
        let ws = Workspace {
            id: Uuid::new_v4(),
            name: "ws".into(),
            slug: "ws".into(),
            kind: Default::default(),
            retrieval: Default::default(),
        };
        let mut p = Product {
            id: Uuid::new_v4(),
            workspace_id: ws.id,
            kind: ProductKind::Product,
            name: "sw".into(),
            slug: "sw".into(),
            current_version_id: None,
            embedding_model_id: "stub-emb".into(),
        };
        let v = ProductVersion::new(p.id, "v1".into());
        p.current_version_id = Some(v.id);
        let vid = v.id;
        s.workspaces.insert(ws.id, ws);
        s.products.insert(p.id, p);
        s.versions.insert(v.id, v);
        (s, vid, Uuid::new_v4())
    }

    #[test]
    fn passage_skips_split_manual_splits() {
        let (mut s, vid, _) = seed();
        {
            let v = s.versions.get_mut(&vid).unwrap();
            v.chunk_size = 80;
            v.chunk_overlap = 10;
            v.chunk_strategy = "heading".into();
            v.wiki_enabled = false;
            v.graph_enabled = false;
            v.question_enabled = false;
        }
        let long = "# One\nfirst section about switching.\n\n# Two\nsecond section about routing.\n\n# Three\nthird section about fabric.\n";
        let (hash, key) = {
            let h = domain::sha256_hex(long.as_bytes());
            let k = format!("objects/{h}");
            s.objects.insert(k.clone(), long.as_bytes().to_vec());
            (h, k)
        };
        let passage_doc = Document::new(
            vid,
            "notes".into(),
            "notes.txt".into(),
            long.len() as i64,
            hash.clone(),
            key.clone(),
        );
        let pid = passage_doc.id;
        s.documents.insert(pid, passage_doc);
        s.enqueue(
            TYPE_DOCUMENT_PROCESS,
            domain::QUEUE_DEFAULT,
            serde_json::json!({
                "document_id": pid,
                "product_version_id": vid,
                "attempt": 1,
                "passages": [long]
            }),
        );
        drain(&mut s);
        let passage_n = s
            .chunks
            .values()
            .filter(|c| c.document_id == pid && c.chunk_type == "text")
            .count();
        assert_eq!(passage_n, 1, "passage must not split");

        let manual_doc = Document::new(
            vid,
            "guide".into(),
            "guide.md".into(),
            long.len() as i64,
            hash,
            key,
        );
        let mid = manual_doc.id;
        s.documents.insert(mid, manual_doc);
        s.enqueue(
            TYPE_MANUAL_PROCESS,
            domain::QUEUE_DEFAULT,
            serde_json::json!({
                "document_id": mid,
                "product_version_id": vid,
                "attempt": 1,
                "manual": true
            }),
        );
        drain(&mut s);
        let manual_n = s
            .chunks
            .values()
            .filter(|c| c.document_id == mid && c.chunk_type == "text")
            .count();
        assert!(
            manual_n > 1,
            "manual markdown must be split, got {manual_n}"
        );
    }

    #[test]
    fn txt_pipeline_completes_and_is_searchable() {
        let (mut s, vid, _) = seed();
        let body = b"The Alpha switch provides 40Gbps throughput and ISO9001 factory test.";
        let (hash, key) = {
            let h = domain::sha256_hex(body);
            let k = format!("objects/{h}");
            s.objects.insert(k.clone(), body.to_vec());
            (h, k)
        };
        let doc = Document::new(
            vid,
            "spec".into(),
            "spec.txt".into(),
            body.len() as i64,
            hash,
            key,
        );
        let did = doc.id;
        s.documents.insert(did, doc);
        s.enqueue(
            TYPE_DOCUMENT_PROCESS,
            domain::QUEUE_DEFAULT,
            serde_json::json!({"document_id": did, "product_version_id": vid, "attempt": 1}),
        );
        drain(&mut s);
        let d = &s.documents[&did];
        assert_eq!(
            d.parse_status,
            ParseStatus::Completed,
            "status {:?}",
            d.parse_status
        );
        assert_eq!(d.enable_status, "enabled");
        assert!(s.chunks.values().any(|c| c.document_id == did));
        assert!(!s.wiki.is_empty() || s.chunks.values().any(|c| c.chunk_type == "wiki_page"));
        assert!(!s.graph.is_empty());
    }

    #[test]
    fn expected_zero_completes_without_finalizing_from_pending() {
        let (mut s, vid, _) = seed();
        let mut v = s.versions.get(&vid).cloned().unwrap();
        v.wiki_enabled = false;
        v.graph_enabled = false;
        v.question_enabled = false;
        s.versions.insert(vid, v);
        let mut doc = Document::new(vid, "e".into(), "e.txt".into(), 1, "h".into(), "h".into());
        doc.parse_status = ParseStatus::Processing;
        let did = doc.id;
        s.documents.insert(did, doc);
        s.enqueue(
            TYPE_POST_PROCESS,
            domain::QUEUE_POSTPROCESS,
            serde_json::json!({"document_id": did, "clone_keep": false}),
        );
        drain(&mut s);
        assert_eq!(s.documents[&did].parse_status, ParseStatus::Completed);
    }

    #[test]
    fn process_overrides_disable_graph() {
        let (mut s, vid, _) = seed();
        let body = b"Alpha switch throughput and factory ISO9001 notes for campus core.";
        let (hash, key) = {
            let h = domain::sha256_hex(body);
            let k = format!("objects/{h}");
            s.objects.insert(k.clone(), body.to_vec());
            (h, k)
        };
        let mut doc = Document::new(
            vid,
            "spec".into(),
            "spec.txt".into(),
            body.len() as i64,
            hash,
            key,
        );
        doc.process_overrides = Some(domain::ProcessOverrides {
            graph_enabled: Some(false),
            ..Default::default()
        });
        let did = doc.id;
        s.documents.insert(did, doc);
        s.enqueue(
            TYPE_DOCUMENT_PROCESS,
            domain::QUEUE_DEFAULT,
            serde_json::json!({"document_id": did, "attempt": 1}),
        );
        drain(&mut s);
        assert!(!s.graph.keys().any(|k| k.1 == did));
        assert_eq!(s.documents[&did].parse_status, ParseStatus::Completed);
    }

    #[test]
    fn datatable_csv_writes_summary_and_column_chunks() {
        let (mut s, vid, _) = seed();
        let csv = b"name,speed\nAlpha,40\nBeta,10\n";
        let (hash, key) = {
            let h = domain::sha256_hex(csv);
            let k = format!("objects/{h}");
            s.objects.insert(k.clone(), csv.to_vec());
            (h, k)
        };
        let doc = Document::new(
            vid,
            "ports".into(),
            "ports.csv".into(),
            csv.len() as i64,
            hash,
            key,
        );
        let did = doc.id;
        s.documents.insert(did, doc);
        s.enqueue(
            TYPE_DATATABLE,
            domain::QUEUE_SUMMARY,
            serde_json::json!({"document_id": did}),
        );
        drain(&mut s);
        assert!(
            s.chunks
                .values()
                .any(|c| c.document_id == did && c.chunk_type == "table_summary")
        );
        assert!(
            s.chunks
                .values()
                .any(|c| c.document_id == did && c.chunk_type == "table_column")
        );
        let summary = s
            .chunks
            .values()
            .find(|c| c.chunk_type == "table_summary")
            .unwrap();
        assert!(summary.content.contains("Table Summary"));
        assert!(summary.content.contains("name") || summary.content.contains("speed"));
    }

    #[test]
    fn datatable_xlsx_uses_docreader_markdown_not_raw_bytes() {
        let (mut s, vid, _) = seed();
        let mut doc = Document::new(
            vid,
            "ports".into(),
            "ports.xlsx".into(),
            8,
            "h".into(),
            "objects/h".into(),
        );
        doc.markdown = "name: Alpha,speed: 40\nname: Beta,speed: 10\n".into();
        s.objects
            .insert("objects/h".into(), b"PK\x03\x04not-a-sheet".to_vec());
        let did = doc.id;
        s.documents.insert(did, doc);
        s.enqueue(
            TYPE_DATATABLE,
            domain::QUEUE_SUMMARY,
            serde_json::json!({"document_id": did}),
        );
        drain(&mut s);
        let summary = s
            .chunks
            .values()
            .find(|c| c.document_id == did && c.chunk_type == "table_summary")
            .expect("xlsx summary from convert markdown");
        assert!(summary.content.contains("name") || summary.content.contains("Alpha"));
    }

    #[test]
    fn datatable_xlsx_without_convert_markdown_retries() {
        let (mut s, vid, _) = seed();
        let doc = Document::new(
            vid,
            "ports".into(),
            "ports.xlsx".into(),
            1,
            "h".into(),
            "objects/h".into(),
        );
        s.objects.insert("objects/h".into(), b"PK\x03\x04".to_vec());
        let did = doc.id;
        s.documents.insert(did, doc);
        let err = datatable_summary(&mut s, did).unwrap_err();
        assert!(err.contains("docreader"));
        assert!(s.chunks.is_empty());
    }

    #[test]
    fn datatable_retry_replaces_prior_table_chunks() {
        let (mut s, vid, _) = seed();
        let csv = b"name,speed\nAlpha,40\nBeta,10\n";
        let (hash, key) = {
            let h = domain::sha256_hex(csv);
            let k = format!("objects/{h}");
            s.objects.insert(k.clone(), csv.to_vec());
            (h, k)
        };
        let doc = Document::new(
            vid,
            "ports".into(),
            "ports.csv".into(),
            csv.len() as i64,
            hash,
            key,
        );
        let did = doc.id;
        s.documents.insert(did, doc);
        datatable_summary(&mut s, did).unwrap();
        datatable_summary(&mut s, did).unwrap();
        let n = s
            .chunks
            .values()
            .filter(|c| c.document_id == did && c.chunk_type == "table_summary")
            .count();
        assert_eq!(n, 1);
    }

    #[test]
    fn reparse_enqueues_datatable_for_csv() {
        let (mut s, vid, _) = seed();
        let csv = b"name,speed\nAlpha,40\n";
        let (hash, key) = {
            let h = domain::sha256_hex(csv);
            let k = format!("objects/{h}");
            s.objects.insert(k.clone(), csv.to_vec());
            (h, k)
        };
        let doc = Document::new(
            vid,
            "ports".into(),
            "ports.csv".into(),
            csv.len() as i64,
            hash,
            key,
        );
        let did = doc.id;
        s.documents.insert(did, doc);
        reparse_document(&mut s, did);
        assert!(s.queue.iter().any(|j| j.task_type == TYPE_DATATABLE));
    }

    #[test]
    fn question_payload_carries_prev_next() {
        let (mut s, vid, _) = seed();
        let mut v = s.versions.get(&vid).cloned().unwrap();
        v.wiki_enabled = false;
        v.graph_enabled = false;
        s.versions.insert(vid, v);
        let mut doc = Document::new(vid, "q".into(), "q.txt".into(), 1, "h".into(), "h".into());
        doc.parse_status = ParseStatus::Processing;
        let did = doc.id;
        s.documents.insert(did, doc);
        for i in 0..3 {
            let content = format!("chunk body {i} about switching fabric");
            let ch = domain::Chunk {
                id: Uuid::new_v4(),
                document_id: did,
                product_version_id: vid,
                chunk_type: "text".into(),
                content: content.clone(),
                context_header: String::new(),
                start_at: i * 10,
                end_at: i * 10 + content.chars().count() as i32,
                parent_chunk_id: None,
                generated_questions: Vec::new(),
            };
            s.chunks.insert(ch.id, ch);
        }
        post_process(&mut s, &serde_json::json!({"document_id": did})).unwrap();
        let q = s
            .queue
            .iter()
            .find(|j| j.task_type == TYPE_QUESTION)
            .expect("question job");
        let prev = q.payload["prev_ids"].as_array().expect("prev_ids");
        let next = q.payload["next_ids"].as_array().expect("next_ids");
        assert_eq!(prev.len(), 3);
        assert!(prev[0].is_null());
        assert!(prev[1].as_str().is_some());
        assert!(next[2].is_null());
        assert!(next[0].as_str().is_some());
    }

    #[test]
    fn question_handle_uses_prev_next_and_finalizes() {
        let (mut s, vid, _) = seed();
        let mut doc = Document::new(vid, "q".into(), "q.txt".into(), 1, "h".into(), "h".into());
        doc.parse_status = ParseStatus::Finalizing;
        doc.pending_subtasks_count = 1;
        let did = doc.id;
        s.documents.insert(did, doc);
        let mut ids = Vec::new();
        for i in 0..2 {
            let content = format!("chunk body {i} about switching fabric and rack install");
            let ch = domain::Chunk {
                id: Uuid::new_v4(),
                document_id: did,
                product_version_id: vid,
                chunk_type: "text".into(),
                content: content.clone(),
                context_header: String::new(),
                start_at: i * 40,
                end_at: i * 40 + content.chars().count() as i32,
                parent_chunk_id: None,
                generated_questions: Vec::new(),
            };
            ids.push(ch.id);
            s.chunks.insert(ch.id, ch);
        }
        s.enqueue(
            TYPE_QUESTION,
            domain::QUEUE_QUESTION,
            serde_json::json!({
                "document_id": did,
                "chunk_ids": ids,
                "prev_ids": [null, ids[0]],
                "next_ids": [ids[1], null],
                "attempt": 1
            }),
        );
        drain(&mut s);
        assert_eq!(s.documents[&did].pending_subtasks_count, 0);
        assert!(!s.chunks[&ids[0]].generated_questions.is_empty());
        assert!(!s.chunks[&ids[1]].generated_questions.is_empty());
    }

    #[test]
    fn bid_convert_is_not_routed_via_default_and_unknown_task_is_not_default() {
        assert_ne!(runtime::queue_for(TYPE_BID_CONVERT), domain::QUEUE_DEFAULT);
        assert_eq!(
            runtime::queue_for(TYPE_BID_CONVERT),
            domain::QUEUE_BID_CONVERT_V1
        );
        assert_ne!(runtime::queue_for("not-a-real-task"), domain::QUEUE_DEFAULT);
        assert_eq!(runtime::queue_for("not-a-real-task"), "rejected:unknown");
        let err = handle(
            &mut Store::default(),
            &domain::Job {
                id: Uuid::new_v4(),
                task_type: "not-a-real-task".into(),
                queue: runtime::queue_for("not-a-real-task").into(),
                payload: serde_json::json!({}),
                retries: 0,
                max_retry: 1,
            },
        )
        .unwrap_err();
        assert!(err.contains("unknown task"), "{err}");
    }

    #[test]
    fn payload_accepts_brain_aliases() {
        let (mut s, vid, _) = seed();
        s.versions.get_mut(&vid).unwrap().wiki_enabled = false;
        s.versions.get_mut(&vid).unwrap().graph_enabled = false;
        let mut doc = Document::new(vid, "a".into(), "a.txt".into(), 1, "h".into(), "h".into());
        doc.parse_status = ParseStatus::Processing;
        let did = doc.id;
        s.documents.insert(did, doc);
        post_process(
            &mut s,
            &serde_json::json!({
                "knowledge_id": did,
                "knowledge_base_id": vid,
                "tenant_id": "ignored"
            }),
        )
        .unwrap();
        assert_eq!(s.documents[&did].parse_status, ParseStatus::Completed);
    }
}
