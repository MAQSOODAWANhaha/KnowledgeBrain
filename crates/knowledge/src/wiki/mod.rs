//! Wiki ingest / finalize. ScopeID = product_version_id.
//! Two lanes: `wiki:ingest` (ingest/retract) and `wiki:finalize` (slug/change/folder_prune).
//! FinalizeSubtask only on ingest terminal.

mod linkify;
mod taxonomy;

pub use linkify::{LinkRef, linkify_content};
pub use taxonomy::{
    ALL_PAGE_TYPES, PAGE_COMPARISON, PAGE_CONCEPT, PAGE_ENTITY, PAGE_INDEX, PAGE_LOG, PAGE_SUMMARY,
    PAGE_SYNTHESIS, junk_item_name, page_content_for, parse_extraction, typed_slug,
};

use crate::job::WikiJob;
use crate::{Chunk, TYPE_WIKI_FINALIZE, TYPE_WIKI_INGEST, WikiFolder, WikiPage, WikiPendingOp};
use chrono::{Duration, Utc};
use taxonomy::{
    Candidate, assemble_body, attach_citations, candidate_slug_prompt, category_for, cite_with_llm,
    collect_text_chunks, dedup_candidates, document_language, existing_folder_paths, fallback_path,
    first_lede, has_sufficient_text, parse_taxonomy_assignments, reduce_page,
    select_relevant_folders, split_summary_line, wiki_summary_prompt,
};
use uuid::Uuid;

pub fn summary_slug(document_id: Uuid) -> String {
    format!("{PAGE_SUMMARY}/{document_id}")
}

pub const INGEST_DEBOUNCE_SECS: u64 = 30;
pub const RETRACT_DEBOUNCE_SECS: u64 = 5;
pub const FOLLOW_UP_DEBOUNCE_SECS: u64 = 5;
pub const FINALIZE_DEBOUNCE_SECS: u64 = 20;
pub const BATCH_DOCS: usize = 5;
/// Brain `IngestReduceParallelOrDefault`.
pub const REDUCE_PARALLEL: usize = 10;
pub const ASSEMBLE_RUNE_CAP: usize = 32768;
pub const STALE_CLAIM_MIN: u64 = 90;
pub const LOCK_RETRY_SECS: u64 = 15;
pub const MAX_FAIL_RETRIES: i32 = 5;
pub const SLUG_LOCK_TTL_SECS: i64 = 5 * 60;
pub const TOMBSTONE_TTL_SECS: i64 = 60 * 60;
pub const INFLIGHT_DEFAULT: usize = 4;

pub const OP_INGEST: &str = "ingest";
pub const OP_RETRACT: &str = "retract";
pub const OP_SLUG: &str = "slug";
pub const OP_CHANGE: &str = "change";
pub const OP_FOLDER_PRUNE: &str = "folder_prune";

pub fn slug_lock_key(version_id: Uuid, slug: &str) -> String {
    format!("wiki:slug:{version_id}:{slug}")
}

pub fn inflight_key(version_id: Uuid) -> String {
    format!("wiki:inflight:{version_id}")
}

pub fn tombstone_key(version_id: Uuid, document_id: Uuid) -> String {
    format!("wiki:deleted:{version_id}:{document_id}")
}

pub fn finalize_task_id(version_id: Uuid) -> String {
    format!("wiki-finalize-{version_id}")
}

pub fn ingest_task_id(version_id: Uuid) -> String {
    format!("wiki-ingest-{version_id}")
}

/// Brain `EnqueueWikiIngest`: persist ingest-lane row, debounce trigger 30s.
pub fn enqueue_ingest_on_job(store: &mut WikiJob, version_id: Uuid, document_id: Uuid) {
    if is_tombstoned_on_job(store, version_id, document_id) {
        store.finalize_subtask(document_id);
        return;
    }
    push_op(
        store,
        TYPE_WIKI_INGEST,
        version_id,
        OP_INGEST,
        Some(document_id),
        "",
        "",
    );
    schedule_trigger(
        store,
        TYPE_WIKI_INGEST,
        version_id,
        INGEST_DEBOUNCE_SECS,
        &ingest_task_id(version_id),
    );
}

/// Seed fail_count copied from a PG `task_pending_ops` row before `process_ingest`.
pub fn set_ingest_fail_count_on_job(
    store: &mut WikiJob,
    version_id: Uuid,
    document_id: Uuid,
    fail_count: i32,
) {
    if let Some(op) = store.wiki_ops.iter_mut().rev().find(|o| {
        o.lane == TYPE_WIKI_INGEST
            && o.version_id == version_id
            && o.document_id == Some(document_id)
            && o.op == OP_INGEST
    }) {
        op.fail_count = fail_count;
    }
}

pub fn retryable_ingest_fail_count_on_job(
    store: &WikiJob,
    version_id: Uuid,
    document_id: Uuid,
) -> Option<i32> {
    store.wiki_ops.iter().find_map(|o| {
        if o.lane == TYPE_WIKI_INGEST
            && o.version_id == version_id
            && o.document_id == Some(document_id)
            && o.op == OP_INGEST
            && o.fail_count <= MAX_FAIL_RETRIES
        {
            Some(o.fail_count)
        } else {
            None
        }
    })
}

pub fn enqueue_finalize_op_on_job(
    store: &mut WikiJob,
    version_id: Uuid,
    op: &str,
    slug: &str,
    title: &str,
) {
    push_op(store, TYPE_WIKI_FINALIZE, version_id, op, None, slug, title);
}

/// Brain `EnqueueWikiRetract`: ingest-lane retract, shorter 5s trigger. No FinalizeSubtask.
pub fn enqueue_retract_on_job(
    store: &mut WikiJob,
    version_id: Uuid,
    document_id: Uuid,
    title: &str,
) {
    write_tombstone_on_job(store, version_id, document_id);
    let slug = slug_for(title, document_id);
    push_op(
        store,
        TYPE_WIKI_INGEST,
        version_id,
        OP_RETRACT,
        Some(document_id),
        &slug,
        title,
    );
    schedule_trigger(
        store,
        TYPE_WIKI_INGEST,
        version_id,
        RETRACT_DEBOUNCE_SECS,
        &ingest_task_id(version_id),
    );
}

pub fn write_tombstone_on_job(store: &mut WikiJob, version_id: Uuid, document_id: Uuid) {
    store
        .wiki_tombstones
        .insert((version_id, document_id), Utc::now());
}

pub fn is_tombstoned_on_job(store: &WikiJob, version_id: Uuid, document_id: Uuid) -> bool {
    store
        .wiki_tombstones
        .get(&(version_id, document_id))
        .is_some_and(|at| {
            Utc::now().signed_duration_since(*at) < Duration::seconds(TOMBSTONE_TTL_SECS)
        })
}

/// Compat: enqueue + process in one call (tests that still call `ingest`).
pub fn ingest_on_job(store: &mut WikiJob, product_version_id: Uuid, document_id: Option<Uuid>) {
    if let Some(id) = document_id {
        enqueue_ingest_on_job(store, product_version_id, id);
    }
    let _ = process_ingest_on_job(store, product_version_id);
}

pub fn process_ingest_on_job(store: &mut WikiJob, version_id: Uuid) -> Result<(), String> {
    let Some(version) = store.versions.get(&version_id).cloned() else {
        clear_lane(store, TYPE_WIKI_INGEST, version_id);
        return Ok(());
    };
    if !version.wiki_enabled {
        clear_lane(store, TYPE_WIKI_INGEST, version_id);
        return Ok(());
    }
    if !reserve_inflight(store, version_id) {
        schedule_trigger(
            store,
            TYPE_WIKI_INGEST,
            version_id,
            LOCK_RETRY_SECS,
            &ingest_task_id(version_id),
        );
        return Ok(());
    }
    let claimed = claim_lane(store, TYPE_WIKI_INGEST, version_id, BATCH_DOCS);
    if claimed.is_empty() {
        release_inflight(store, version_id);
        return Ok(());
    }
    let mut slugs: Vec<(String, String)> = Vec::new();
    let mut done_ids: Vec<i64> = Vec::new();
    let mut grouped: std::collections::HashMap<String, Vec<SlugUpdate>> =
        std::collections::HashMap::new();
    let mut mapped: Vec<(i64, Uuid)> = Vec::new();
    for op in &claimed {
        match op.op.as_str() {
            OP_RETRACT => {
                retract_one(store, version_id, op);
                done_ids.push(op.id);
            }
            _ => {
                let Some(did) = op.document_id else {
                    done_ids.push(op.id);
                    continue;
                };
                if is_tombstoned_on_job(store, version_id, did) {
                    store.finalize_subtask(did);
                    done_ids.push(op.id);
                    continue;
                }
                match map_document(store, version_id, did) {
                    Ok(updates) if updates.is_empty() => {
                        store.finalize_subtask(did);
                        done_ids.push(op.id);
                    }
                    Ok(updates) => {
                        for u in updates {
                            grouped.entry(u.slug.clone()).or_default().push(u);
                        }
                        mapped.push((op.id, did));
                    }
                    Err(_) => {
                        if let Some(row) = store.wiki_ops.iter_mut().find(|r| r.id == op.id) {
                            row.fail_count += 1;
                            row.claimed_at = None;
                            if row.fail_count > MAX_FAIL_RETRIES {
                                store.dead_letter(TYPE_WIKI_INGEST, did, "wiki map failed");
                                store.finalize_subtask(did);
                                done_ids.push(op.id);
                            }
                        }
                    }
                }
            }
        }
    }
    let model = version.wiki_chat_model().to_string();
    let (written, failed_docs) = reduce_grouped(store, version_id, &model, grouped);
    slugs.extend(written);
    for (op_id, did) in mapped {
        if failed_docs.contains(&did) {
            if let Some(row) = store.wiki_ops.iter_mut().find(|r| r.id == op_id) {
                row.fail_count += 1;
                row.claimed_at = None;
                if row.fail_count > MAX_FAIL_RETRIES {
                    store.dead_letter(TYPE_WIKI_INGEST, did, "wiki reduce failed");
                    store.finalize_subtask(did);
                    done_ids.push(op_id);
                }
            }
            continue;
        }
        store.finalize_subtask(did);
        done_ids.push(op_id);
    }
    trim_ops(store, &done_ids);
    if !slugs.is_empty() {
        plan_and_apply_taxonomy(store, version_id, &slugs);
    }
    let wrote = !slugs.is_empty();
    for (slug, title) in slugs {
        push_op(
            store,
            TYPE_WIKI_FINALIZE,
            version_id,
            OP_SLUG,
            None,
            &slug,
            &title,
        );
        push_op(
            store,
            TYPE_WIKI_FINALIZE,
            version_id,
            OP_CHANGE,
            None,
            "",
            &title,
        );
    }
    if wrote {
        push_op(
            store,
            TYPE_WIKI_FINALIZE,
            version_id,
            OP_FOLDER_PRUNE,
            None,
            "",
            "",
        );
    }
    if store
        .wiki_ops
        .iter()
        .any(|o| o.lane == TYPE_WIKI_FINALIZE && o.version_id == version_id)
    {
        schedule_trigger(
            store,
            TYPE_WIKI_FINALIZE,
            version_id,
            FINALIZE_DEBOUNCE_SECS,
            &finalize_task_id(version_id),
        );
    }
    if store
        .wiki_ops
        .iter()
        .any(|o| o.lane == TYPE_WIKI_INGEST && o.version_id == version_id)
    {
        schedule_trigger(
            store,
            TYPE_WIKI_INGEST,
            version_id,
            INGEST_DEBOUNCE_SECS,
            &ingest_task_id(version_id),
        );
    }
    release_inflight(store, version_id);
    Ok(())
}

pub fn process_finalize_on_job(store: &mut WikiJob, version_id: Uuid) -> Result<(), String> {
    let rows = claim_lane(store, TYPE_WIKI_FINALIZE, version_id, 5000);
    if rows.is_empty() {
        return Ok(());
    }
    let mut ids = Vec::new();
    let mut prune_ids = Vec::new();
    let mut rebuild_index = false;
    for op in &rows {
        if op.op == OP_FOLDER_PRUNE {
            prune_ids.push(op.id);
            continue;
        }
        if op.op == OP_SLUG && !op.slug.is_empty() && try_slug_lock(store, version_id, &op.slug) {
            let _ = reduce_slug(store, version_id, op);
            release_slug_lock(store, version_id, &op.slug);
            rebuild_index = true;
        }
        if op.op == OP_CHANGE {
            rebuild_index = true;
        }
        ids.push(op.id);
    }
    if rebuild_index {
        write_index_page(store, version_id);
        write_log_page(store, version_id);
        write_synthesis_pages(store, version_id);
        linkify_version(store, version_id);
    }
    trim_ops(store, &ids);
    if !prune_ids.is_empty() {
        let ingest_live = store
            .wiki_ops
            .iter()
            .any(|o| o.lane == TYPE_WIKI_INGEST && o.version_id == version_id);
        if ingest_live {
            for op in store.wiki_ops.iter_mut() {
                if prune_ids.contains(&op.id) {
                    op.claimed_at = None;
                }
            }
            schedule_trigger(
                store,
                TYPE_WIKI_FINALIZE,
                version_id,
                LOCK_RETRY_SECS,
                &finalize_task_id(version_id),
            );
        } else {
            prune_empty_folders(store, version_id);
            trim_ops(store, &prune_ids);
        }
    }
    Ok(())
}

pub fn finalize_on_job(store: &mut WikiJob, product_version_id: Uuid) {
    let _ = process_finalize_on_job(store, product_version_id);
}

/// Last-attempt fail-open: drain ingest-lane slots so parent is not stuck.
pub fn fail_open_pending_on_job(store: &mut WikiJob, version_id: Uuid) {
    let ids: Vec<Uuid> = store
        .wiki_ops
        .iter()
        .filter(|o| o.lane == TYPE_WIKI_INGEST && o.version_id == version_id && o.op == OP_INGEST)
        .filter_map(|o| o.document_id)
        .collect();
    for did in ids {
        store.finalize_subtask(did);
    }
    store
        .wiki_ops
        .retain(|o| !(o.lane == TYPE_WIKI_INGEST && o.version_id == version_id));
}

#[derive(Debug, Clone)]
struct SlugUpdate {
    slug: String,
    update_type: String,
    document_id: Uuid,
    doc_title: String,
    language: String,
    title: String,
    about: String,
    details: String,
    aliases: Vec<String>,
    chunk_ids: Vec<Uuid>,
    summary_line: String,
    summary_body: String,
    retract_content: String,
    doc_summary: String,
}

fn slugs_for_document(store: &WikiJob, version_id: Uuid, document_id: Uuid) -> Vec<String> {
    store
        .wiki
        .values()
        .filter(|p| p.product_version_id == version_id && p.source_refs.contains(&document_id))
        .map(|p| p.slug.clone())
        .collect()
}

fn map_document(
    store: &mut WikiJob,
    version_id: Uuid,
    document_id: Uuid,
) -> Result<Vec<SlugUpdate>, String> {
    let Some(doc) = store.documents.get(&document_id).cloned() else {
        return Ok(Vec::new());
    };
    let Some(version) = store.versions.get(&version_id).cloned() else {
        return Ok(Vec::new());
    };
    if !version.wiki_enabled {
        return Ok(Vec::new());
    }
    let chunks = collect_text_chunks(store, document_id);
    let body = assemble_body(&chunks);
    if body.is_empty() || !has_sufficient_text(&body) {
        return Ok(Vec::new());
    }
    let title = doc.title.clone();
    let language = document_language(&body).to_string();
    let mut candidates = extract_candidates(
        store,
        version_id,
        document_id,
        &title,
        &body,
        &chunks,
        &language,
    );
    candidates = dedup_candidates(candidates);
    let listing: String = candidates
        .iter()
        .filter(|c| c.page_type != PAGE_SUMMARY)
        .map(|c| format!("- [[{}]] = {}", c.slug, c.title))
        .collect::<Vec<_>>()
        .join("\n");
    let (sum_sys, sum_user) = wiki_summary_prompt(&body, &listing, &language);
    let synthesized = if crate::enrichment::chat_http_configured() {
        crate::enrichment::chat_complete_wiki(&sum_sys, &sum_user, version.wiki_chat_model())?
    } else {
        String::new()
    };
    let (summary_lede, summary_body) = split_summary_line(&synthesized);
    let summary_body = if summary_body.is_empty() {
        synthesized.clone()
    } else {
        summary_body
    };
    let doc_summary = if !summary_body.is_empty() {
        summary_body.clone()
    } else {
        summary_lede.clone()
    };
    let sum_slug = summary_slug(document_id);
    let mut updates = vec![SlugUpdate {
        slug: sum_slug.clone(),
        update_type: PAGE_SUMMARY.into(),
        document_id,
        doc_title: title.clone(),
        language: language.clone(),
        title: title.clone(),
        about: summary_lede.clone(),
        details: String::new(),
        aliases: Vec::new(),
        chunk_ids: Vec::new(),
        summary_line: summary_lede.clone(),
        summary_body,
        retract_content: String::new(),
        doc_summary: doc_summary.clone(),
    }];
    let mut new_slugs: std::collections::HashSet<String> = std::collections::HashSet::new();
    new_slugs.insert(sum_slug);
    for cand in candidates {
        if cand.page_type == PAGE_SUMMARY {
            continue;
        }
        new_slugs.insert(cand.slug.clone());
        updates.push(SlugUpdate {
            slug: cand.slug,
            update_type: cand.page_type,
            document_id,
            doc_title: title.clone(),
            language: language.clone(),
            title: cand.title,
            about: cand.about,
            details: cand.details,
            aliases: cand.aliases,
            chunk_ids: cand.source_refs,
            summary_line: String::new(),
            summary_body: String::new(),
            retract_content: String::new(),
            doc_summary: doc_summary.clone(),
        });
    }
    let prior = store
        .wiki
        .get(&(version_id, summary_slug(document_id)))
        .map(|p| p.content.clone())
        .unwrap_or_else(|| doc_summary.clone());
    for old in slugs_for_document(store, version_id, document_id) {
        if new_slugs.contains(&old) {
            if old.starts_with("summary/") {
                continue;
            }
            updates.push(SlugUpdate {
                slug: old,
                update_type: "retract".into(),
                document_id,
                doc_title: title.clone(),
                language: language.clone(),
                title: String::new(),
                about: String::new(),
                details: String::new(),
                aliases: Vec::new(),
                chunk_ids: Vec::new(),
                summary_line: String::new(),
                summary_body: String::new(),
                retract_content: prior.clone(),
                doc_summary: doc_summary.clone(),
            });
        } else {
            updates.push(SlugUpdate {
                slug: old,
                update_type: "retractStale".into(),
                document_id,
                doc_title: title.clone(),
                language: language.clone(),
                title: String::new(),
                about: String::new(),
                details: String::new(),
                aliases: Vec::new(),
                chunk_ids: Vec::new(),
                summary_line: String::new(),
                summary_body: String::new(),
                retract_content: body.clone(),
                doc_summary: doc_summary.clone(),
            });
        }
    }
    Ok(updates)
}

struct ReduceJob {
    slug: String,
    updates: Vec<SlugUpdate>,
    existing: Option<WikiPage>,
    chunks: Vec<Chunk>,
    remaining: String,
    valid_links: String,
}

struct ReducePatch {
    slug: String,
    title: String,
    page_type: String,
    content: String,
    summary: String,
    aliases: Vec<String>,
    source_refs: Vec<Uuid>,
    chunk_refs: Vec<Uuid>,
    owner_doc: Uuid,
}

fn reduce_grouped(
    store: &mut WikiJob,
    version_id: Uuid,
    model: &str,
    grouped: std::collections::HashMap<String, Vec<SlugUpdate>>,
) -> (Vec<(String, String)>, std::collections::HashSet<Uuid>) {
    let mut failed = std::collections::HashSet::new();
    let mut jobs = Vec::new();
    let valid_links: String = grouped
        .keys()
        .map(|s| format!("- [[{s}]]"))
        .collect::<Vec<_>>()
        .join("\n");
    for (slug, updates) in grouped {
        if !try_slug_lock(store, version_id, &slug) {
            for u in &updates {
                failed.insert(u.document_id);
            }
            continue;
        }
        let existing = store.wiki.get(&(version_id, slug.clone())).cloned();
        let mut chunk_ids = Vec::new();
        for u in &updates {
            chunk_ids.extend(u.chunk_ids.iter().copied());
        }
        chunk_ids.sort();
        chunk_ids.dedup();
        let chunks: Vec<Chunk> = chunk_ids
            .iter()
            .filter_map(|id| store.chunks.get(id).cloned())
            .collect();
        let retract_ids: std::collections::HashSet<Uuid> = updates
            .iter()
            .filter(|u| u.update_type == "retract" || u.update_type == "retractStale")
            .map(|u| u.document_id)
            .collect();
        let remaining = if retract_ids.is_empty() {
            String::new()
        } else {
            let keep: Vec<Uuid> = existing
                .as_ref()
                .map(|p| {
                    p.source_refs
                        .iter()
                        .copied()
                        .filter(|id| !retract_ids.contains(id))
                        .collect()
                })
                .unwrap_or_default();
            keep.iter()
                .filter_map(|did| {
                    store.wiki.values().find(|p| {
                        p.product_version_id == version_id
                            && p.page_type == PAGE_SUMMARY
                            && p.source_refs.contains(did)
                    })
                })
                .map(|p| {
                    format!(
                        "<document>\n<title>{}</title>\n<content>\n{}\n</content>\n</document>\n",
                        p.title, p.content
                    )
                })
                .collect()
        };
        jobs.push(ReduceJob {
            slug,
            updates,
            existing,
            chunks,
            remaining,
            valid_links: valid_links.clone(),
        });
    }
    type BatchOut = Option<ReducePatch>;
    let collected = std::sync::Mutex::new(Vec::<BatchOut>::new());
    let next = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|scope| {
        let workers = REDUCE_PARALLEL.min(jobs.len()).max(1);
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    if i >= jobs.len() {
                        break;
                    }
                    let out = run_reduce_job(model, &jobs[i]);
                    collected.lock().expect("reduce mutex").push(out);
                }
            });
        }
    });
    let mut written = Vec::new();
    for patch in collected
        .into_inner()
        .expect("reduce mutex")
        .into_iter()
        .flatten()
    {
        let slug = patch.slug.clone();
        let title = patch.title.clone();
        apply_patch(store, version_id, patch);
        release_slug_lock(store, version_id, &slug);
        written.push((slug, title));
    }
    for job in &jobs {
        if written.iter().all(|(s, _)| s != &job.slug) {
            release_slug_lock(store, version_id, &job.slug);
            for u in &job.updates {
                failed.insert(u.document_id);
            }
        }
    }
    (written, failed)
}

fn run_reduce_job(model: &str, job: &ReduceJob) -> Option<ReducePatch> {
    let mut summary_u = None;
    let mut additions = Vec::new();
    let mut retracts = Vec::new();
    for u in &job.updates {
        match u.update_type.as_str() {
            t if t == PAGE_SUMMARY => summary_u = Some(u),
            "retract" | "retractStale" => retracts.push(u),
            _ => additions.push(u),
        }
    }
    if let Some(sum) = summary_u {
        let content = if !sum.summary_body.is_empty() {
            sum.summary_body.clone()
        } else {
            format!("# {}\n", sum.title)
        };
        let lede = if !sum.summary_line.is_empty() {
            sum.summary_line.clone()
        } else {
            first_lede(&content)
        };
        let mut source_refs = job
            .existing
            .as_ref()
            .map(|p| p.source_refs.clone())
            .unwrap_or_default();
        if !source_refs.contains(&sum.document_id) {
            source_refs.push(sum.document_id);
        }
        return Some(ReducePatch {
            slug: job.slug.clone(),
            title: sum.title.clone(),
            page_type: PAGE_SUMMARY.into(),
            content,
            summary: lede,
            aliases: Vec::new(),
            source_refs,
            chunk_refs: Vec::new(),
            owner_doc: sum.document_id,
        });
    }
    if additions.is_empty() && retracts.is_empty() {
        return None;
    }
    let first = additions
        .first()
        .copied()
        .or_else(|| retracts.first().copied())?;
    let mut aliases = job
        .existing
        .as_ref()
        .map(|p| p.aliases.clone())
        .unwrap_or_default();
    let mut source_refs = job
        .existing
        .as_ref()
        .map(|p| p.source_refs.clone())
        .unwrap_or_default();
    let mut chunk_refs = job
        .existing
        .as_ref()
        .map(|p| p.chunk_refs.clone())
        .unwrap_or_default();
    let retract_ids: std::collections::HashSet<Uuid> =
        retracts.iter().map(|u| u.document_id).collect();
    source_refs.retain(|id| !retract_ids.contains(id));
    let mut about = String::new();
    let mut details = String::new();
    let mut title = job
        .existing
        .as_ref()
        .map(|p| p.title.clone())
        .unwrap_or_default();
    let mut page_type = job
        .existing
        .as_ref()
        .map(|p| p.page_type.clone())
        .unwrap_or_default();
    let mut cited: Vec<&Chunk> = Vec::new();
    let mut deleted = String::new();
    for u in &retracts {
        if !u.retract_content.is_empty() {
            deleted.push_str(&format!(
                "<document>\n<title>{}</title>\n<content>\n{}\n</content>\n</document>\n",
                u.doc_title, u.retract_content
            ));
        }
    }
    for u in &additions {
        if title.is_empty() {
            title = u.title.clone();
        }
        if page_type.is_empty() {
            page_type = u.update_type.clone();
        }
        if about.len() < u.about.len() {
            about = u.about.clone();
        }
        if details.len() < u.details.len() {
            details = u.details.clone();
        }
        for a in &u.aliases {
            if !aliases.iter().any(|x| x == a) {
                aliases.push(a.clone());
            }
        }
        if !source_refs.contains(&u.document_id) {
            source_refs.push(u.document_id);
        }
        for id in &u.chunk_ids {
            if !chunk_refs.contains(id) {
                chunk_refs.push(*id);
            }
            if let Some(ch) = job.chunks.iter().find(|c| c.id == *id) {
                cited.push(ch);
            }
        }
    }
    if title.is_empty() {
        title = job.slug.clone();
    }
    if page_type.is_empty() {
        page_type = PAGE_ENTITY.into();
    }
    let existing = job
        .existing
        .as_ref()
        .map(|p| p.content.as_str())
        .unwrap_or("");
    let (content, lede) = reduce_page(taxonomy::ReduceInput {
        model,
        slug: &job.slug,
        title: &title,
        page_type: &page_type,
        existing,
        about: &about,
        details: &details,
        cited: &cited,
        doc_title: &first.doc_title,
        doc_summary: &first.doc_summary,
        valid_links: &job.valid_links,
        language: &first.language,
        deleted_content: &deleted,
        remaining_sources: &job.remaining,
    });
    Some(ReducePatch {
        slug: job.slug.clone(),
        title,
        page_type,
        content,
        summary: lede,
        aliases,
        source_refs,
        chunk_refs,
        owner_doc: first.document_id,
    })
}

fn apply_patch(store: &mut WikiJob, version_id: Uuid, patch: ReducePatch) {
    let existing = store.wiki.get(&(version_id, patch.slug.clone())).cloned();
    let (id, category_path, folder_id) = if let Some(old) = existing {
        (
            old.id,
            if old.category_path.is_empty() {
                category_for(&patch.page_type, &patch.title)
            } else {
                old.category_path
            },
            old.folder_id,
        )
    } else {
        (
            Uuid::new_v4(),
            category_for(&patch.page_type, &patch.title),
            None,
        )
    };
    let page = WikiPage {
        id,
        product_version_id: version_id,
        slug: patch.slug.clone(),
        title: patch.title.clone(),
        content: patch.content.clone(),
        page_type: patch.page_type,
        status: "published".into(),
        summary: patch.summary,
        aliases: patch.aliases,
        source_refs: patch.source_refs,
        chunk_refs: patch.chunk_refs,
        category_path,
        folder_id,
    };
    store.wiki.insert((version_id, patch.slug.clone()), page);
    index_wiki_page(
        store,
        version_id,
        patch.owner_doc,
        &patch.slug,
        &patch.title,
        &patch.content,
    );
}

fn extract_candidates(
    store: &WikiJob,
    version_id: Uuid,
    document_id: Uuid,
    title: &str,
    body: &str,
    chunks: &[Chunk],
    language: &str,
) -> Vec<Candidate> {
    let model = store
        .versions
        .get(&version_id)
        .map(|v| v.wiki_chat_model().to_string())
        .unwrap_or_else(|| "stub-chat".into());
    let previous: String = store
        .wiki
        .values()
        .filter(|p| {
            p.product_version_id == version_id
                && (p.page_type == PAGE_ENTITY || p.page_type == PAGE_CONCEPT)
        })
        .map(|p| format!("- {} = {}", p.slug, p.title))
        .collect::<Vec<_>>()
        .join("\n");
    let (sys, user) = candidate_slug_prompt(body, &previous, title, language);
    let raw = if crate::enrichment::chat_http_configured() {
        crate::enrichment::chat_complete_wiki(&sys, &user, &model).unwrap_or_default()
    } else {
        String::new()
    };
    let items = parse_extraction(&raw);
    let candidates = if items.is_empty() {
        let nodes: Vec<_> = store
            .graph
            .values()
            .filter(|n| n.version_id == version_id && n.document_id == document_id)
            .collect();
        taxonomy::candidates_from_graph(&nodes, chunks)
    } else {
        items
            .into_iter()
            .map(|it| {
                let page_type = if ALL_PAGE_TYPES.contains(&it.page_type.as_str()) {
                    it.page_type
                } else {
                    PAGE_ENTITY.into()
                };
                let slug = if it.slug.is_empty() {
                    typed_slug(&page_type, &it.name)
                } else if it.slug.contains('/') {
                    it.slug
                } else {
                    format!("{page_type}/{}", it.slug)
                };
                Candidate {
                    title: it.name,
                    slug,
                    page_type,
                    aliases: it.aliases,
                    about: it.description,
                    details: it.details,
                    source_refs: Vec::new(),
                }
            })
            .collect()
    };
    let (citations, news) = cite_with_llm(&model, &candidates, chunks, language);
    attach_citations(candidates, citations, news, chunks)
}

fn index_wiki_page(
    store: &mut WikiJob,
    version_id: Uuid,
    document_id: Uuid,
    slug: &str,
    title: &str,
    content: &str,
) {
    let version = store.versions.get(&version_id).cloned();
    let vector_on = version.as_ref().is_none_or(|v| v.vector_enabled);
    let keyword_on = version.as_ref().is_none_or(|v| v.keyword_enabled);
    let ch = Chunk {
        id: Uuid::new_v4(),
        document_id,
        product_version_id: version_id,
        chunk_type: "wiki_page".into(),
        content: content.to_string(),
        context_header: slug.to_string(),
        start_at: 0,
        end_at: content.chars().count() as i32,
        parent_chunk_id: None,
        generated_questions: Vec::new(),
    };
    let model = store
        .versions
        .get(&version_id)
        .map(|v| v.embedding_model_id.clone())
        .unwrap_or_default();
    let _ = crate::index::index_one_in(
        &mut store.embeddings,
        &ch,
        title,
        &model,
        vector_on,
        keyword_on,
    );
    store.chunks.insert(ch.id, ch);
}

fn linkify_version(store: &mut WikiJob, version_id: Uuid) {
    let refs: Vec<LinkRef> = store
        .wiki
        .values()
        .filter(|p| p.product_version_id == version_id && p.status == "published")
        .flat_map(|p| {
            let mut r = vec![LinkRef {
                slug: p.slug.clone(),
                match_text: p.title.clone(),
            }];
            r.extend(p.aliases.iter().map(|a| LinkRef {
                slug: p.slug.clone(),
                match_text: a.clone(),
            }));
            r
        })
        .collect();
    if refs.is_empty() {
        return;
    }
    let slugs: Vec<String> = store
        .wiki
        .values()
        .filter(|p| p.product_version_id == version_id)
        .map(|p| p.slug.clone())
        .collect();
    for slug in slugs {
        let Some(page) = store.wiki.get(&(version_id, slug.clone())).cloned() else {
            continue;
        };
        let (next, changed) = linkify_content(&page.content, &refs, &page.slug);
        if changed && let Some(p) = store.wiki.get_mut(&(version_id, slug)) {
            p.content = next;
        }
    }
}

fn write_index_page(store: &mut WikiJob, version_id: Uuid) {
    let mut lines = vec!["# Wiki index".to_string(), String::new()];
    let mut pages: Vec<_> = store
        .wiki
        .values()
        .filter(|p| p.product_version_id == version_id && p.page_type != PAGE_INDEX)
        .cloned()
        .collect();
    pages.sort_by(|a, b| a.page_type.cmp(&b.page_type).then(a.title.cmp(&b.title)));
    for p in pages {
        lines.push(format!("- [[{}|{}]] ({})", p.slug, p.title, p.page_type));
    }
    let content = lines.join("\n");
    let slug = PAGE_INDEX.to_string();
    let page = WikiPage::published(
        store
            .wiki
            .get(&(version_id, slug.clone()))
            .map(|p| p.id)
            .unwrap_or_else(Uuid::new_v4),
        version_id,
        slug.clone(),
        "Index".into(),
        content.clone(),
        PAGE_INDEX.into(),
        "Wiki index".into(),
    );
    store.wiki.insert((version_id, slug.clone()), page);
    store
        .chunks
        .retain(|_, c| !(c.product_version_id == version_id && c.context_header == PAGE_INDEX));
    index_wiki_page(
        store,
        version_id,
        Uuid::nil(),
        PAGE_INDEX,
        "Index",
        &content,
    );
}

fn plan_and_apply_taxonomy(store: &mut WikiJob, version_id: Uuid, slugs: &[(String, String)]) {
    let pool = existing_folder_paths(store, version_id);
    let mut items = Vec::new();
    for (slug, title) in slugs {
        let Some(page) = store.wiki.get(&(version_id, slug.clone())) else {
            continue;
        };
        if page.page_type != PAGE_ENTITY && page.page_type != PAGE_CONCEPT {
            continue;
        }
        items.push((
            slug.clone(),
            title.clone(),
            page.page_type.clone(),
            page.summary.clone(),
        ));
    }
    if items.is_empty() {
        return;
    }
    let model = store
        .versions
        .get(&version_id)
        .map(|v| v.wiki_chat_model().to_string())
        .unwrap_or_else(|| "stub-chat".into());
    let item_texts: Vec<(String, String)> = items
        .iter()
        .map(|(_, title, _, about)| (title.clone(), about.clone()))
        .collect();
    let selected = select_relevant_folders(&pool, &item_texts);
    let mut tree = String::new();
    for p in &selected {
        tree.push_str(&format!("- {}\n", p.join(" / ")));
    }
    if tree.is_empty() {
        tree.push_str("(empty — invent a shallow tree)\n");
    }
    let mut block = String::new();
    for (slug, title, ty, about) in &items {
        block.push_str(&format!(
            "- slug: {slug} | title: {title} | type: {ty} | about: {about}\n"
        ));
    }
    let raw = crate::enrichment::chat_complete(
        r#"Assign each wiki item a directory path. JSON only:
{"assignments":[{"slug":"entity/x","path":["Entities","Hardware"]}]}
Reuse existing folders when they fit. Max 3 levels. Do not invent facts."#,
        &format!("Existing folders:\n{tree}\nItems:\n{block}"),
        &model,
    )
    .unwrap_or_default();
    let mut planned = parse_taxonomy_assignments(&raw);
    for (slug, title, ty, _) in items {
        let path = planned
            .remove(&slug)
            .filter(|p| !p.is_empty())
            .unwrap_or_else(|| fallback_path(&ty, &title, &selected));
        let fid = ensure_folder_path(store, version_id, &path);
        if let Some(p) = store.wiki.get_mut(&(version_id, slug))
            && (p.category_path.is_empty() || p.folder_id.is_none())
        {
            p.category_path = path;
            p.folder_id = fid;
        }
    }
}

fn ensure_folder_path(store: &mut WikiJob, version_id: Uuid, path: &[String]) -> Option<Uuid> {
    if path.is_empty() {
        return None;
    }
    let mut parent = None;
    let mut last = None;
    for (i, name) in path.iter().enumerate() {
        let joined = path[..=i].join("/");
        if let Some(existing) = store
            .wiki_folders
            .values()
            .find(|f| f.product_version_id == version_id && f.path == joined)
        {
            parent = Some(existing.id);
            last = Some(existing.id);
            continue;
        }
        let id = Uuid::new_v4();
        store.wiki_folders.insert(
            id,
            WikiFolder {
                id,
                product_version_id: version_id,
                parent_id: parent,
                name: name.clone(),
                path: joined,
                depth: (i as i32) + 1,
                sort_order: i as i32,
            },
        );
        parent = Some(id);
        last = Some(id);
    }
    last
}

fn prune_empty_folders(store: &mut WikiJob, version_id: Uuid) {
    let used: Vec<String> = store
        .wiki
        .values()
        .filter(|p| p.product_version_id == version_id)
        .flat_map(|p| (1..=p.category_path.len()).map(|n| p.category_path[..n].join("/")))
        .collect();
    store
        .wiki_folders
        .retain(|_, f| f.product_version_id != version_id || used.iter().any(|p| p == &f.path));
}

fn write_log_page(store: &mut WikiJob, version_id: Uuid) {
    let mut lines = vec!["# Wiki log".to_string(), String::new()];
    let mut pages: Vec<_> = store
        .wiki
        .values()
        .filter(|p| p.product_version_id == version_id && p.page_type != PAGE_LOG)
        .cloned()
        .collect();
    pages.sort_by(|a, b| a.title.cmp(&b.title));
    for p in pages {
        lines.push(format!(
            "- published [[{}|{}]] ({})",
            p.slug, p.title, p.page_type
        ));
    }
    upsert_system_page(store, version_id, PAGE_LOG, "Log", &lines.join("\n"));
}

fn write_synthesis_pages(store: &mut WikiJob, version_id: Uuid) {
    let entities: Vec<_> = store
        .wiki
        .values()
        .filter(|p| p.product_version_id == version_id && p.page_type == PAGE_ENTITY)
        .cloned()
        .collect();
    if entities.len() < 2 {
        return;
    }
    let names: Vec<_> = entities.iter().map(|e| e.title.as_str()).collect();
    let synth = format!(
        "# Synthesis\n\nCovers {}.\n\n{}",
        names.join(", "),
        entities
            .iter()
            .map(|e| format!("## {}\n\n{}", e.title, e.summary))
            .collect::<Vec<_>>()
            .join("\n\n")
    );
    upsert_system_page(store, version_id, PAGE_SYNTHESIS, "Synthesis", &synth);
    if entities.len() >= 2 {
        let cmp = format!(
            "# Comparison\n\n| Item | Summary |\n|---|---|\n{}",
            entities
                .iter()
                .map(|e| format!(
                    "| [[{}|{}]] | {} |",
                    e.slug,
                    e.title,
                    e.summary.replace('|', "/")
                ))
                .collect::<Vec<_>>()
                .join("\n")
        );
        upsert_system_page(store, version_id, PAGE_COMPARISON, "Comparison", &cmp);
    }
}

fn upsert_system_page(
    store: &mut WikiJob,
    version_id: Uuid,
    slug: &str,
    title: &str,
    content: &str,
) {
    let id = store
        .wiki
        .get(&(version_id, slug.to_string()))
        .map(|p| p.id)
        .unwrap_or_else(Uuid::new_v4);
    store.wiki.insert(
        (version_id, slug.to_string()),
        WikiPage::published(
            id,
            version_id,
            slug.to_string(),
            title.into(),
            content.to_string(),
            slug.to_string(),
            content.chars().take(240).collect(),
        ),
    );
    store
        .chunks
        .retain(|_, c| !(c.product_version_id == version_id && c.context_header == slug));
    index_wiki_page(store, version_id, Uuid::nil(), slug, title, content);
}

fn retract_one(store: &mut WikiJob, version_id: Uuid, op: &WikiPendingOp) {
    if !op.slug.is_empty() {
        store.wiki.remove(&(version_id, op.slug.clone()));
    }
    if let Some(did) = op.document_id {
        store
            .chunks
            .retain(|_, c| !(c.document_id == did && c.chunk_type == "wiki_page"));
    }
}

fn reduce_slug(store: &mut WikiJob, version_id: Uuid, op: &WikiPendingOp) -> Result<(), String> {
    let Some(page) = store.wiki.get(&(version_id, op.slug.clone())).cloned() else {
        return Ok(());
    };
    if page.status != "published" {
        return Ok(());
    }
    if let Some(p) = store.wiki.get_mut(&(version_id, op.slug.clone()))
        && p.page_type.is_empty()
    {
        p.page_type = "summary".into();
    }
    let version = store.versions.get(&version_id).cloned();
    let vector_on = version.as_ref().is_none_or(|v| v.vector_enabled);
    let keyword_on = version.as_ref().is_none_or(|v| v.keyword_enabled);
    let existing = store
        .chunks
        .values()
        .find(|c| {
            c.product_version_id == version_id
                && c.chunk_type == "wiki_page"
                && (c.content == page.content || c.context_header == page.slug)
        })
        .cloned();
    if let Some(mut existing) = existing {
        existing.content = page.content.clone();
        existing.end_at = page.content.chars().count() as i32;
        let model = store
            .versions
            .get(&version_id)
            .map(|v| v.embedding_model_id.clone())
            .unwrap_or_default();
        crate::index::index_one_in(
            &mut store.embeddings,
            &existing,
            &page.title,
            &model,
            vector_on,
            keyword_on,
        )?;
        store.chunks.insert(existing.id, existing);
        return Ok(());
    }
    let ch = Chunk {
        id: Uuid::new_v4(),
        document_id: Uuid::nil(),
        product_version_id: version_id,
        chunk_type: "wiki_page".into(),
        content: page.content.clone(),
        context_header: page.slug.clone(),
        start_at: 0,
        end_at: page.content.chars().count() as i32,
        parent_chunk_id: None,
        generated_questions: Vec::new(),
    };
    let model = store
        .versions
        .get(&version_id)
        .map(|v| v.embedding_model_id.clone())
        .unwrap_or_default();
    crate::index::index_one_in(
        &mut store.embeddings,
        &ch,
        &page.title,
        &model,
        vector_on,
        keyword_on,
    )?;
    store.chunks.insert(ch.id, ch);
    Ok(())
}

fn push_op(
    store: &mut WikiJob,
    lane: &str,
    version_id: Uuid,
    op: &str,
    document_id: Option<Uuid>,
    slug: &str,
    title: &str,
) {
    if lane == TYPE_WIKI_INGEST
        && let Some(did) = document_id
    {
        store.wiki_ops.retain(|o| {
            !(o.lane == TYPE_WIKI_INGEST
                && o.version_id == version_id
                && o.document_id == Some(did)
                && o.op == op
                && o.claimed_at.is_none())
        });
    }
    store.wiki_op_seq += 1;
    store.wiki_ops.push(WikiPendingOp {
        id: store.wiki_op_seq,
        lane: lane.into(),
        version_id,
        op: op.into(),
        document_id,
        slug: slug.into(),
        title: title.into(),
        claimed_at: None,
        fail_count: 0,
    });
}

fn claim_lane(
    store: &mut WikiJob,
    lane: &str,
    version_id: Uuid,
    limit: usize,
) -> Vec<WikiPendingOp> {
    let stale_before = Utc::now() - Duration::minutes(STALE_CLAIM_MIN as i64);
    let mut out = Vec::new();
    let now = Utc::now();
    for op in &mut store.wiki_ops {
        if op.lane != lane || op.version_id != version_id {
            continue;
        }
        let reclaimable = op.claimed_at.is_none_or(|at| at <= stale_before);
        if !reclaimable {
            continue;
        }
        op.claimed_at = Some(now);
        out.push(op.clone());
        if out.len() >= limit {
            break;
        }
    }
    out
}

fn trim_ops(store: &mut WikiJob, ids: &[i64]) {
    store.wiki_ops.retain(|o| {
        if !ids.contains(&o.id) {
            return true;
        }
        o.fail_count > 0 && o.fail_count <= MAX_FAIL_RETRIES && o.claimed_at.is_none()
    });
}

fn clear_lane(store: &mut WikiJob, lane: &str, version_id: Uuid) {
    store
        .wiki_ops
        .retain(|o| !(o.lane == lane && o.version_id == version_id));
}

fn schedule_trigger(
    store: &mut WikiJob,
    task_type: &str,
    version_id: Uuid,
    delay_secs: u64,
    task_id: &str,
) {
    let already = store.queue.iter().any(|j| {
        j.task_type == task_type
            && j.payload.get("task_id").and_then(|v| v.as_str()) == Some(task_id)
    });
    if already {
        return;
    }
    store.enqueue(
        task_type,
        crate::QUEUE_WIKI,
        serde_json::json!({
            "product_version_id": version_id,
            "delay_secs": delay_secs,
            "task_id": task_id,
        }),
    );
}

fn try_slug_lock(store: &mut WikiJob, version_id: Uuid, slug: &str) -> bool {
    let key = slug_lock_key(version_id, slug);
    let now = Utc::now();
    if let Some(held) = store.wiki_slug_locks.get(&key)
        && now.signed_duration_since(*held) < Duration::seconds(SLUG_LOCK_TTL_SECS)
    {
        return false;
    }
    store.wiki_slug_locks.insert(key, now);
    true
}

fn release_slug_lock(store: &mut WikiJob, version_id: Uuid, slug: &str) {
    store
        .wiki_slug_locks
        .remove(&slug_lock_key(version_id, slug));
}

fn reserve_inflight(store: &mut WikiJob, version_id: Uuid) -> bool {
    let live = store
        .wiki_inflight
        .get(&version_id)
        .is_some_and(|at| Utc::now().signed_duration_since(*at) < Duration::seconds(90));
    if live {
        return false;
    }
    store.wiki_inflight.insert(version_id, Utc::now());
    true
}

fn release_inflight(store: &mut WikiJob, version_id: Uuid) {
    store.wiki_inflight.remove(&version_id);
}

fn slug_for(title: &str, id: Uuid) -> String {
    let s: String = title
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    format!("{s}-{}", &id.to_string()[..8])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Chunk, Document, ProductVersion};

    fn seed() -> (WikiJob, Uuid, Uuid) {
        let mut s = WikiJob::default();
        let v = ProductVersion::new(Uuid::new_v4(), "v1".into());
        let vid = v.id;
        s.version_id = vid;
        s.versions.insert(vid, v);
        let doc = Document::new(
            vid,
            "Alpha".into(),
            "a.txt".into(),
            1,
            "h".into(),
            "h".into(),
        );
        let did = doc.id;
        s.documents.insert(did, doc);
        let c = Chunk {
            id: Uuid::new_v4(),
            document_id: did,
            product_version_id: vid,
            chunk_type: "text".into(),
            content: "body of alpha".into(),
            context_header: String::new(),
            start_at: 0,
            end_at: 13,
            parent_chunk_id: None,
            generated_questions: Vec::new(),
        };
        s.chunks.insert(c.id, c);
        (s, vid, did)
    }

    #[test]
    fn constants_match_brain() {
        assert_eq!(INGEST_DEBOUNCE_SECS, 30);
        assert_eq!(RETRACT_DEBOUNCE_SECS, 5);
        assert_eq!(FOLLOW_UP_DEBOUNCE_SECS, 5);
        assert_eq!(FINALIZE_DEBOUNCE_SECS, 20);
        assert_eq!(BATCH_DOCS, 5);
        assert_eq!(REDUCE_PARALLEL, 10);
        assert_eq!(LOCK_RETRY_SECS, 15);
        assert_eq!(STALE_CLAIM_MIN, 90);
        assert_eq!(ASSEMBLE_RUNE_CAP, 32768);
        assert_eq!(INFLIGHT_DEFAULT, 4);
    }

    #[test]
    fn enqueue_coalesces_ingest_trigger_and_keeps_two_ops() {
        let (mut s, vid, did) = seed();
        let doc2 = Document::new(
            vid,
            "Beta".into(),
            "b.txt".into(),
            1,
            "h".into(),
            "h".into(),
        );
        let did2 = doc2.id;
        s.documents.insert(did2, doc2);
        enqueue_ingest_on_job(&mut s, vid, did);
        enqueue_ingest_on_job(&mut s, vid, did2);
        assert_eq!(
            s.wiki_ops
                .iter()
                .filter(|o| o.lane == TYPE_WIKI_INGEST)
                .count(),
            2
        );
        let triggers: Vec<_> = s
            .queue
            .iter()
            .filter(|j| j.task_type == TYPE_WIKI_INGEST)
            .collect();
        assert_eq!(triggers.len(), 1);
        assert_eq!(triggers[0].payload["delay_secs"], 30);
        assert_eq!(
            triggers[0].payload["task_id"].as_str().unwrap(),
            ingest_task_id(vid)
        );
    }

    #[test]
    fn lanes_do_not_mix() {
        let (mut s, vid, did) = seed();
        enqueue_ingest_on_job(&mut s, vid, did);
        process_ingest_on_job(&mut s, vid).unwrap();
        assert!(
            s.wiki_ops.iter().all(|o| o.lane == TYPE_WIKI_FINALIZE),
            "ingest lane drained; leftover is finalize"
        );
        assert!(
            s.queue
                .iter()
                .any(|j| { j.task_type == TYPE_WIKI_FINALIZE && j.payload["delay_secs"] == 20 })
        );
        let finalize_n = s
            .wiki_ops
            .iter()
            .filter(|o| o.lane == TYPE_WIKI_FINALIZE)
            .count();
        process_finalize_on_job(&mut s, vid).unwrap();
        assert!(
            s.wiki_ops
                .iter()
                .filter(|o| o.lane == TYPE_WIKI_FINALIZE)
                .count()
                < finalize_n
                || finalize_n == 0
        );
        assert!(!s.wiki.is_empty());
    }

    #[test]
    fn tombstone_skips_ingest_and_still_finalizes() {
        let (mut s, vid, did) = seed();
        if let Some(d) = s.documents.get_mut(&did) {
            d.parse_status = crate::ParseStatus::Finalizing;
            d.pending_subtasks_count = 1;
        }
        write_tombstone_on_job(&mut s, vid, did);
        enqueue_ingest_on_job(&mut s, vid, did);
        assert_eq!(s.documents[&did].pending_subtasks_count, 0);
        assert!(s.wiki.is_empty());
    }

    #[test]
    fn stale_claim_is_reclaimable() {
        let (mut s, vid, did) = seed();
        enqueue_ingest_on_job(&mut s, vid, did);
        s.wiki_ops[0].claimed_at = Some(Utc::now() - Duration::minutes(91));
        let claimed = claim_lane(&mut s, TYPE_WIKI_INGEST, vid, 5);
        assert_eq!(claimed.len(), 1);
    }

    #[test]
    fn batch_caps_at_five_and_schedules_follow_up() {
        let (mut s, vid, _) = seed();
        for i in 0..6 {
            let doc = Document::new(
                vid,
                format!("D{i}"),
                format!("d{i}.txt"),
                1,
                "h".into(),
                "h".into(),
            );
            let did = doc.id;
            s.documents.insert(did, doc);
            let c = Chunk {
                id: Uuid::new_v4(),
                document_id: did,
                product_version_id: vid,
                chunk_type: "text".into(),
                content: format!("body {i}"),
                context_header: String::new(),
                start_at: 0,
                end_at: 6,
                parent_chunk_id: None,
                generated_questions: Vec::new(),
            };
            s.chunks.insert(c.id, c);
            enqueue_ingest_on_job(&mut s, vid, did);
        }
        s.queue.clear();
        process_ingest_on_job(&mut s, vid).unwrap();
        let remaining = s
            .wiki_ops
            .iter()
            .filter(|o| o.lane == TYPE_WIKI_INGEST)
            .count();
        assert_eq!(remaining, 1);
        assert!(s.queue.iter().any(|j| j.task_type == TYPE_WIKI_INGEST));
    }

    #[test]
    fn wiki_disabled_skips_without_error() {
        let (mut s, vid, did) = seed();
        s.versions.get_mut(&vid).unwrap().wiki_enabled = false;
        enqueue_ingest_on_job(&mut s, vid, did);
        process_ingest_on_job(&mut s, vid).unwrap();
        let remaining = s
            .wiki_ops
            .iter()
            .filter(|o| o.lane == TYPE_WIKI_INGEST)
            .count();
        assert_eq!(remaining, 0);
    }

    #[test]
    fn map_lock_conflict_keeps_op_and_skips_finalize() {
        let (mut s, vid, did) = seed();
        s.documents.get_mut(&did).unwrap().pending_subtasks_count = 1;
        let slug = summary_slug(did);
        s.wiki_slug_locks
            .insert(slug_lock_key(vid, &slug), Utc::now());
        enqueue_ingest_on_job(&mut s, vid, did);
        s.queue.clear();
        process_ingest_on_job(&mut s, vid).unwrap();
        assert_eq!(s.documents[&did].pending_subtasks_count, 1);
        let left = s
            .wiki_ops
            .iter()
            .filter(|o| o.lane == TYPE_WIKI_INGEST && o.document_id == Some(did))
            .count();
        assert_eq!(left, 1);
        assert_eq!(
            s.wiki_ops
                .iter()
                .find(|o| o.document_id == Some(did))
                .unwrap()
                .fail_count,
            1
        );
    }

    #[test]
    fn slug_lock_key_format() {
        let vid = Uuid::nil();
        assert_eq!(
            slug_lock_key(vid, "alpha"),
            format!("wiki:slug:{vid}:alpha")
        );
        assert_eq!(tombstone_key(vid, vid), format!("wiki:deleted:{vid}:{vid}"));
        assert!(finalize_task_id(vid).starts_with("wiki-finalize-"));
    }

    #[test]
    fn map_writes_summary_entity_and_index_and_linkifies() {
        let (mut s, vid, did) = seed();
        s.graph.insert(
            (vid, did, "Alpha".into()),
            crate::GraphNode {
                version_id: vid,
                document_id: did,
                name: "Alpha".into(),
                chunk_ids: vec![],
            },
        );
        s.graph.insert(
            (vid, did, "Beta".into()),
            crate::GraphNode {
                version_id: vid,
                document_id: did,
                name: "Beta".into(),
                chunk_ids: vec![],
            },
        );
        if let Some(c) = s.chunks.values_mut().find(|c| c.document_id == did) {
            c.content = "body of Alpha and Beta".into();
            c.end_at = c.content.chars().count() as i32;
        }
        enqueue_ingest_on_job(&mut s, vid, did);
        process_ingest_on_job(&mut s, vid).unwrap();
        process_finalize_on_job(&mut s, vid).unwrap();
        assert!(
            s.wiki
                .values()
                .any(|p| p.page_type == PAGE_SUMMARY && p.product_version_id == vid)
        );
        assert!(s.wiki.contains_key(&(vid, summary_slug(did))));
        assert!(
            s.wiki
                .values()
                .any(|p| p.page_type == PAGE_ENTITY && p.title == "Alpha")
        );
        let index = s.wiki.get(&(vid, PAGE_INDEX.to_string())).expect("index");
        assert_eq!(index.page_type, PAGE_INDEX);
        assert!(index.content.contains("[["));
        let entity = s
            .wiki
            .values()
            .find(|p| p.page_type == PAGE_ENTITY)
            .unwrap();
        assert!(entity.source_refs.contains(&did));
        assert!(
            s.wiki_folders
                .values()
                .any(|f| f.product_version_id == vid && f.path.starts_with("Entities"))
        );
        assert!(entity.folder_id.is_some());
        assert!(s.wiki.contains_key(&(vid, PAGE_SYNTHESIS.to_string())));
        assert!(s.wiki.contains_key(&(vid, PAGE_COMPARISON.to_string())));
        assert!(s.wiki.contains_key(&(vid, PAGE_LOG.to_string())));
        assert!(
            s.wiki_ops.iter().any(|o| o.op == OP_FOLDER_PRUNE)
                || s.wiki_folders.values().any(|f| f.product_version_id == vid)
        );
    }

    #[test]
    fn wiki_chat_model_prefers_synthesis() {
        let mut v = ProductVersion::new(Uuid::new_v4(), "v1".into());
        assert_eq!(v.wiki_chat_model(), "stub-chat");
        v.wiki_synthesis_model_id = "wiki-chat".into();
        assert_eq!(v.wiki_chat_model(), "wiki-chat");
    }

    #[test]
    fn second_doc_unions_source_refs_on_same_slug() {
        let (mut s, vid, did) = seed();
        s.graph.insert(
            (vid, did, "Alpha".into()),
            crate::GraphNode {
                version_id: vid,
                document_id: did,
                name: "Alpha".into(),
                chunk_ids: vec![],
            },
        );
        enqueue_ingest_on_job(&mut s, vid, did);
        process_ingest_on_job(&mut s, vid).unwrap();
        let first_refs = s
            .wiki
            .values()
            .find(|p| p.page_type == PAGE_ENTITY && p.title == "Alpha")
            .map(|p| p.source_refs.clone())
            .unwrap_or_default();
        let doc2 = Document::new(
            vid,
            "Other".into(),
            "b.txt".into(),
            1,
            "h".into(),
            "h".into(),
        );
        let did2 = doc2.id;
        s.documents.insert(did2, doc2);
        let c2 = Chunk {
            id: Uuid::new_v4(),
            document_id: did2,
            product_version_id: vid,
            chunk_type: "text".into(),
            content: "Alpha also appears here".into(),
            context_header: String::new(),
            start_at: 0,
            end_at: 22,
            parent_chunk_id: None,
            generated_questions: Vec::new(),
        };
        s.chunks.insert(c2.id, c2.clone());
        s.graph.insert(
            (vid, did2, "Alpha".into()),
            crate::GraphNode {
                version_id: vid,
                document_id: did2,
                name: "Alpha".into(),
                chunk_ids: vec![c2.id],
            },
        );
        enqueue_ingest_on_job(&mut s, vid, did2);
        process_ingest_on_job(&mut s, vid).unwrap();
        let entity = s
            .wiki
            .values()
            .find(|p| p.page_type == PAGE_ENTITY && p.title == "Alpha")
            .unwrap();
        assert!(entity.source_refs.len() >= first_refs.len());
        assert!(entity.source_refs.contains(&did2) || entity.source_refs.len() > 1);
    }

    #[test]
    fn folder_prune_waits_while_ingest_pending() {
        let (mut s, vid, did) = seed();
        enqueue_ingest_on_job(&mut s, vid, did);
        process_ingest_on_job(&mut s, vid).unwrap();
        let extra = Document::new(
            vid,
            "More".into(),
            "c.txt".into(),
            1,
            "h".into(),
            "h".into(),
        );
        let did2 = extra.id;
        s.documents.insert(did2, extra);
        enqueue_ingest_on_job(&mut s, vid, did2);
        process_finalize_on_job(&mut s, vid).unwrap();
        assert!(
            s.wiki_ops
                .iter()
                .any(|o| o.op == OP_FOLDER_PRUNE && o.claimed_at.is_none()),
            "prune stays until ingest lane drains"
        );
    }
}
