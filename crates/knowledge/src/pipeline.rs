//! Production knowledge jobs. Callers pass PgPool; jobs load SQL into DocJob/WikiJob.

use crate::workflow::{self, Work};
use sqlx::PgPool;
use uuid::Uuid;

pub async fn maybe_start_postprocess(
    pool: &PgPool,
    document_id: Uuid,
    version_id: Uuid,
    attempt: i32,
) {
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
    if let Err(error) = workflow::put(
        pool,
        document_id,
        version_id,
        attempt,
        Work::Postprocess { clone_keep: false },
    )
    .await
    {
        tracing::error!(%document_id,%error,"persist postprocess obligation failed");
    }
    let _ = workflow::dispatch(pool).await;
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

pub async fn schedule_semantic_index_v2_if_ready_with<F, Fut>(
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
    attempt: i32,
) -> Result<(), String> {
    if !workflow::pending(pool, document_id, attempt, "postprocess").await? {
        return Ok(());
    }
    tracing::info!(
        document_id = %document_id,
        clone_keep,
        "parse postprocess start"
    );
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
    let Some(job) = crate::DocJob::from_pool(pool, document_id)
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(());
    };
    let doc = job.document.clone();
    let version = job.version.clone();
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
    if doc.parse_status != crate::ParseStatus::Processing {
        if doc.parse_status == crate::ParseStatus::Completed {
            finish_postprocess_spans(pool, document_id, attempt).await;
            return schedule_semantic_index_v2_if_ready(pool, product_version_id).await;
        }
        finish_postprocess_spans(pool, document_id, attempt).await;
        return Ok(());
    }
    let text_count = job
        .chunks
        .values()
        .filter(|c| c.chunk_type == "text")
        .count();
    let source_count = job
        .chunks
        .values()
        .filter(|c| {
            matches!(
                c.chunk_type.as_str(),
                "text" | "image_ocr" | "image_caption"
            )
        })
        .count();
    let mut work = Vec::new();
    if source_count > 0 && !clone_keep {
        work.push(Work::Summary);
    }
    if is_table_file(&doc.file_name) {
        work.push(Work::Datatable);
    }
    if !clone_keep
        && version.question_enabled
        && version.needs_embedding()
        && text_count > 0
        && task_enabled(platform::TYPE_QUESTION)
    {
        let mut chunks: Vec<_> = job
            .chunks
            .values()
            .filter(|c| c.chunk_type == "text")
            .collect();
        chunks.sort_by_key(|c| c.start_at);
        for (batch, items) in chunks.chunks(20).enumerate() {
            let base = batch * 20;
            work.push(Work::Questions {
                chunk_ids: items.iter().map(|c| c.id).collect(),
                prev_ids: (0..items.len())
                    .map(|i| (base + i).checked_sub(1).map(|n| chunks[n].id))
                    .collect(),
                next_ids: (0..items.len())
                    .map(|i| chunks.get(base + i + 1).map(|c| c.id))
                    .collect(),
                batch: batch as u32,
            });
        }
    }
    if version.graph_enabled && task_enabled(platform::TYPE_CHUNK_EXTRACT) {
        work.extend(
            job.chunks
                .values()
                .filter(|c| {
                    matches!(
                        c.chunk_type.as_str(),
                        "text" | "image_ocr" | "image_caption"
                    )
                })
                .map(|c| Work::Extract { chunk_id: c.id }),
        );
    }
    if version.wiki_enabled && source_count > 0 {
        work.push(Work::Wiki);
    }
    workflow::begin_postprocess(pool, document_id, product_version_id, attempt, &work).await?;
    finish_postprocess_spans(pool, document_id, attempt).await;
    workflow::dispatch(pool).await?;
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
    let key = format!("image:{image_key}");
    if !workflow::pending(pool, document_id, attempt, &key).await? {
        return Ok(());
    }
    let ws: Option<Uuid> = crate::document_workspace_id(pool, document_id)
        .await
        .map_err(|e| e.to_string())?;
    let Some(_ws) = ws else {
        return Ok(());
    };
    let Some(mut job) = crate::DocJob::from_pool(pool, document_id)
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(());
    };
    job.version.embedding_model_id =
        crate::freeze_version_embedding_model(pool, job.version.id).await?;
    if job.document.parse_status == crate::ParseStatus::Pending {
        job.document.parse_status = crate::ParseStatus::Processing;
    }
    let outcome = crate::enrichment::process_image_on_job(
        &mut job,
        image_key,
        image_source_type,
        enable_ocr,
        enable_caption,
    );
    workflow::commit_chunks(
        pool,
        &job,
        attempt,
        &key,
        &["image_ocr", "image_caption", "image_ocr_partial"],
        Some(image_key),
        &[],
        outcome.is_ok(),
    )
    .await?;
    outcome?;
    let waiting:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM knowledge_job_outbox WHERE document_id=$1 AND attempt=$2 AND payload->>'kind'='image' AND state<>'complete')")
        .bind(document_id).bind(attempt).fetch_one(pool).await.map_err(|e|e.to_string())?;
    if !waiting {
        crate::finish_span(
            pool,
            document_id,
            attempt,
            crate::obs::SPAN_MULTIMODAL,
            crate::obs::STATUS_DONE,
            None,
        )
        .await
        .map_err(|e| e.to_string())?;
        maybe_start_postprocess(pool, document_id, job.document.product_version_id, attempt).await;
    }
    Ok(())
}

/// Last-retry DECR so a dead image cannot pin `multimodal:pending`.
pub async fn finalize_multimodal(pool: &PgPool, document_id: Uuid, attempt: i32) {
    let _ = workflow::update_status(
        pool,
        document_id,
        attempt,
        "failed",
        "required image processing exhausted retries; incomplete evidence",
    )
    .await;
}

/// Execute one closed Wiki ingest/retract job. Oxana owns retries and dead jobs;
/// PostgreSQL stores only the resulting Wiki business artifacts.
pub async fn run_wiki_ingest(
    pool: &PgPool,
    version_id: Uuid,
    document_id: Uuid,
    operation: &str,
    attempt: i32,
) -> Result<(), String> {
    if operation == crate::wiki::OP_INGEST
        && !workflow::pending(pool, document_id, attempt, "wiki").await?
    {
        return Ok(());
    }
    let mut lock = pool.acquire().await.map_err(|error| error.to_string())?;
    sqlx::query("SELECT pg_catalog.pg_advisory_lock(pg_catalog.hashtextextended($1,0))")
        .bind(format!("knowledge-wiki:{version_id}"))
        .execute(&mut *lock)
        .await
        .map_err(|error| error.to_string())?;
    let result = run_wiki_ingest_locked(pool, version_id, document_id, operation, attempt).await;
    let unlock =
        sqlx::query("SELECT pg_catalog.pg_advisory_unlock(pg_catalog.hashtextextended($1,0))")
            .bind(format!("knowledge-wiki:{version_id}"))
            .execute(&mut *lock)
            .await
            .map_err(|error| error.to_string());
    match (result, unlock) {
        (Err(error), _) => Err(error),
        (Ok(()), Err(error)) => Err(error),
        (Ok(()), Ok(_)) => Ok(()),
    }
}

async fn run_wiki_ingest_locked(
    pool: &PgPool,
    version_id: Uuid,
    document_id: Uuid,
    operation: &str,
    attempt: i32,
) -> Result<(), String> {
    if !crate::version_wiki_enabled(pool, version_id)
        .await
        .map_err(|error| error.to_string())?
    {
        tracing::info!(%version_id, %document_id, "wiki ingest skipped: not enabled");
        if operation == crate::wiki::OP_INGEST {
            workflow::complete(pool, document_id, attempt, "wiki").await?;
        }
        return schedule_semantic_index_v2_if_ready(pool, version_id).await;
    }
    if !matches!(operation, crate::wiki::OP_INGEST | crate::wiki::OP_RETRACT) {
        return Err("invalid closed Wiki ingest operation".into());
    }
    let frozen = crate::freeze_version_embedding_model(pool, version_id).await?;
    let mut job = crate::WikiJob::from_pool(pool, version_id)
        .await
        .map_err(|error| error.to_string())?;
    job.versions.entry(version_id).or_insert_with(|| {
        let mut version = crate::ProductVersion::new(Uuid::nil(), "v".into());
        version.id = version_id;
        version.wiki_enabled = true;
        version
    });
    if let Some(version) = job.versions.get_mut(&version_id) {
        version.embedding_model_id = frozen;
    }
    job.documents.entry(document_id).or_insert_with(|| {
        let mut document = crate::Document::new(
            version_id,
            document_id.to_string(),
            "doc.txt".into(),
            0,
            String::new(),
            String::new(),
        );
        document.id = document_id;
        document
    });
    let before_pages = job.wiki.clone();
    let before_folders = job.wiki_folders.clone();
    if operation == crate::wiki::OP_INGEST && attempt > 1 {
        crate::wiki::enqueue_retract_on_job(&mut job, version_id, document_id, "");
        crate::wiki::process_ingest_on_job(&mut job, version_id)?;
        crate::wiki::process_finalize_on_job(&mut job, version_id)?;
        job.wiki_tombstones.remove(&(version_id, document_id));
    }
    if operation == crate::wiki::OP_RETRACT {
        crate::wiki::enqueue_retract_on_job(&mut job, version_id, document_id, "");
    } else {
        crate::wiki::enqueue_ingest_on_job(&mut job, version_id, document_id);
    }
    crate::wiki::process_ingest_on_job(&mut job, version_id)?;
    crate::wiki::process_finalize_on_job(&mut job, version_id)?;
    if !job.wiki_ops.is_empty() {
        return Err("Wiki reducer has unfinished work; retry required".into());
    }
    persist_wiki_job(
        pool,
        &job,
        version_id,
        document_id,
        attempt,
        if operation == crate::wiki::OP_INGEST {
            "wiki"
        } else {
            "wiki-retract"
        },
        &before_pages,
        &before_folders,
    )
    .await?;
    schedule_semantic_index_v2_if_ready(pool, version_id).await
}

/// Execute one idempotent Wiki finalize job derived from its immutable document identity.
pub async fn run_wiki_finalize(
    pool: &PgPool,
    version_id: Uuid,
    document_id: Uuid,
    attempt: i32,
) -> Result<(), String> {
    let mut lock = pool.acquire().await.map_err(|error| error.to_string())?;
    sqlx::query("SELECT pg_catalog.pg_advisory_lock(pg_catalog.hashtextextended($1,0))")
        .bind(format!("knowledge-wiki:{version_id}"))
        .execute(&mut *lock)
        .await
        .map_err(|error| error.to_string())?;
    let result = run_wiki_finalize_locked(pool, version_id, document_id, attempt).await;
    let unlock =
        sqlx::query("SELECT pg_catalog.pg_advisory_unlock(pg_catalog.hashtextextended($1,0))")
            .bind(format!("knowledge-wiki:{version_id}"))
            .execute(&mut *lock)
            .await
            .map_err(|error| error.to_string());
    match (result, unlock) {
        (Err(error), _) => Err(error),
        (Ok(()), Err(error)) => Err(error),
        (Ok(()), Ok(_)) => Ok(()),
    }
}

async fn run_wiki_finalize_locked(
    pool: &PgPool,
    version_id: Uuid,
    document_id: Uuid,
    attempt: i32,
) -> Result<(), String> {
    let frozen = crate::freeze_version_embedding_model(pool, version_id).await?;
    let mut job = crate::WikiJob::from_pool(pool, version_id)
        .await
        .map_err(|error| error.to_string())?;
    job.versions.entry(version_id).or_insert_with(|| {
        let mut version = crate::ProductVersion::new(Uuid::nil(), "v".into());
        version.id = version_id;
        version.wiki_enabled = true;
        version
    });
    if let Some(version) = job.versions.get_mut(&version_id) {
        version.embedding_model_id = frozen;
    }
    let before_pages = job.wiki.clone();
    let before_folders = job.wiki_folders.clone();
    crate::wiki::enqueue_finalize_op_on_job(
        &mut job,
        version_id,
        crate::wiki::OP_SLUG,
        &document_id.to_string(),
        "",
    );
    crate::wiki::enqueue_finalize_op_on_job(&mut job, version_id, crate::wiki::OP_CHANGE, "", "");
    crate::wiki::enqueue_finalize_op_on_job(
        &mut job,
        version_id,
        crate::wiki::OP_FOLDER_PRUNE,
        "",
        "",
    );
    crate::wiki::process_finalize_on_job(&mut job, version_id)?;
    if job.wiki_ops.iter().any(|operation| {
        operation.lane == platform::TYPE_WIKI_FINALIZE && operation.version_id == version_id
    }) {
        return Err("Wiki finalize business lock is busy".into());
    }
    persist_wiki_job(
        pool,
        &job,
        version_id,
        document_id,
        attempt,
        "wiki-finalize",
        &before_pages,
        &before_folders,
    )
    .await?;
    schedule_semantic_index_v2_if_ready(pool, version_id).await
}

#[allow(clippy::too_many_arguments)] // Version reducer plus generation receipt are one commit.
async fn persist_wiki_job(
    pool: &PgPool,
    job: &crate::WikiJob,
    version_id: Uuid,
    document_id: Uuid,
    attempt: i32,
    receipt: &str,
    before_pages: &std::collections::HashMap<(Uuid, String), crate::WikiPage>,
    before_folders: &std::collections::HashMap<Uuid, crate::WikiFolder>,
) -> Result<(), String> {
    let changed_pages = job
        .wiki
        .iter()
        .filter(|(key, page)| {
            key.0 == version_id
                && before_pages.get(*key).is_none_or(|before| {
                    serde_json::to_value(before).ok() != serde_json::to_value(page).ok()
                })
        })
        .map(|(_, page)| page.clone())
        .collect::<Vec<_>>();
    let removed_page_ids = before_pages
        .iter()
        .filter(|((candidate_version, slug), _)| {
            *candidate_version == version_id
                && !job.wiki.contains_key(&(*candidate_version, slug.clone()))
        })
        .map(|(_, page)| page.id)
        .collect::<Vec<_>>();
    let changed_folders = job
        .wiki_folders
        .iter()
        .filter(|(id, folder)| {
            folder.product_version_id == version_id
                && before_folders.get(id).is_none_or(|before| {
                    serde_json::to_value(before).ok() != serde_json::to_value(folder).ok()
                })
        })
        .map(|(_, folder)| folder.clone())
        .collect::<Vec<_>>();
    let removed_folder_ids = before_folders
        .iter()
        .filter(|(id, folder)| {
            folder.product_version_id == version_id && !job.wiki_folders.contains_key(id)
        })
        .map(|(id, _)| *id)
        .collect::<Vec<_>>();
    let mut changed_slugs = changed_pages
        .iter()
        .map(|page| page.slug.clone())
        .collect::<std::collections::HashSet<_>>();
    changed_slugs.extend(
        before_pages
            .values()
            .filter(|page| removed_page_ids.contains(&page.id))
            .map(|page| page.slug.clone()),
    );
    let mut wiki_chunks = job
        .chunks
        .values()
        .filter(|chunk| {
            chunk.product_version_id == version_id
                && chunk.chunk_type == "wiki_page"
                && changed_slugs.contains(&chunk.context_header)
        })
        .cloned()
        .collect::<Vec<_>>();
    for chunk in &mut wiki_chunks {
        if chunk.document_id.is_nil() {
            // D5: version-level wiki chunks have no owning document; document_id is
            // only a storage host to satisfy the NOT NULL FK. Their lifecycle is
            // owned by the wiki retract path (retracted pages are removed from
            // job.wiki, their slugs flow into changed_slugs, and
            // persist_wiki_changes_atomic deletes the matching wiki_page chunks),
            // and purge_document_index explicitly spares chunk_type = 'wiki_page'.
            chunk.document_id = document_id;
        }
    }
    let wiki_embeddings = wiki_chunks
        .iter()
        .filter_map(|chunk| {
            let mut embedding = job.embeddings.get(&chunk.id).cloned()?;
            if embedding.document_id.is_nil() {
                embedding.document_id = chunk.document_id;
            }
            Some(embedding)
        })
        .collect::<Vec<_>>();
    crate::persist_wiki_changes_atomic(
        pool,
        version_id,
        &changed_pages,
        &removed_page_ids,
        &changed_folders,
        &removed_folder_ids,
        &changed_slugs.into_iter().collect::<Vec<_>>(),
        &wiki_chunks,
        &wiki_embeddings,
        &job.documents
            .values()
            .map(|document| (document.id, document.attempt))
            .collect::<Vec<_>>(),
        Some((document_id, attempt, receipt)),
    )
    .await
    .map_err(|error| error.to_string())
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
    if !workflow::pending(pool, document_id, attempt, "summary").await? {
        return Ok(());
    }
    if crate::document_parse_status(pool, document_id)
        .await
        .map_err(|error| error.to_string())?
        .as_deref()
        == Some("completed")
    {
        return schedule_semantic_index_for_document_v2(pool, document_id).await;
    }
    let Some(mut job) = crate::DocJob::from_pool(pool, document_id)
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(());
    };
    job.version.embedding_model_id =
        crate::freeze_version_embedding_model(pool, job.version.id).await?;
    let outcome = crate::enrichment::generate_summary_on_job(&mut job, attempt, fallback)?;
    if matches!(outcome, crate::enrichment::SummaryOutcome::Superseded) {
        return Ok(());
    }
    workflow::commit_chunks(
        pool,
        &job,
        attempt,
        "summary",
        &["summary"],
        None,
        &[],
        true,
    )
    .await?;
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
    if !workflow::pending(
        pool,
        document_id,
        attempt,
        &question_job_key(pool, document_id, attempt, chunk_ids).await?,
    )
    .await?
    {
        return Ok(());
    }
    if crate::document_parse_status(pool, document_id)
        .await
        .map_err(|error| error.to_string())?
        .as_deref()
        == Some("completed")
    {
        return schedule_semantic_index_for_document_v2(pool, document_id).await;
    }
    let Some(mut job) = crate::DocJob::from_pool(pool, document_id)
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(());
    };
    job.version.embedding_model_id =
        crate::freeze_version_embedding_model(pool, job.version.id).await?;
    let outcome = crate::enrichment::generate_questions_on_job(
        &mut job, chunk_ids, prev_ids, next_ids, attempt,
    )?;
    if matches!(outcome, crate::enrichment::QuestionOutcome::Superseded) {
        return Ok(());
    }
    let key = question_job_key(pool, document_id, attempt, chunk_ids).await?;
    workflow::commit_chunks(
        pool,
        &job,
        attempt,
        &key,
        &["question"],
        None,
        chunk_ids,
        true,
    )
    .await?;
    schedule_semantic_index_for_document_v2(pool, document_id).await
}

pub async fn run_extract(
    pool: &PgPool,
    chunk_id: Uuid,
    document_id: Uuid,
    attempt: i32,
) -> Result<(), String> {
    if !workflow::pending(pool, document_id, attempt, &format!("extract:{chunk_id}")).await? {
        return Ok(());
    }
    if crate::document_parse_status(pool, document_id)
        .await
        .map_err(|error| error.to_string())?
        .as_deref()
        == Some("completed")
    {
        return schedule_semantic_index_for_document_v2(pool, document_id).await;
    }
    let Some(mut job) = crate::DocJob::from_pool(pool, document_id)
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(());
    };
    let outcome = crate::graph::extract_chunk_on_job(&mut job, chunk_id, attempt)?;
    if matches!(outcome, crate::graph::ExtractOutcome::Superseded) {
        return Ok(());
    }
    workflow::commit_graph(pool, &job, attempt, &format!("extract:{chunk_id}")).await?;
    schedule_semantic_index_for_document_v2(pool, document_id).await
}

pub async fn run_list_delete(pool: &PgPool, document_id: Uuid) -> Result<(), String> {
    let attempt: i32 = sqlx::query_scalar("SELECT attempt FROM documents WHERE id=$1")
        .bind(document_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?
        .unwrap_or(0);
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
        run_wiki_ingest(pool, vid, document_id, crate::wiki::OP_RETRACT, attempt).await?;
        crate::graph::delete_document(vid, document_id)?;
    }
    crate::purge_document_index(pool, document_id)
        .await
        .map_err(|error| error.to_string())?;
    let deletion = platform::release_knowledge_document_object(
        pool,
        document_id,
        &format!("knowledge-document-delete:{document_id}"),
    )
    .await
    .map_err(|e| e.to_string())?;
    if let Some(deletion) = deletion {
        platform::dispatch_object_deletion(pool, deletion).await?;
    }
    // D4: mark the document row deleted only after all cleanup succeeded.
    // documents.parse_status allows 'deleted' in knowledge_base_baseline.sql.
    sqlx::query(
        "UPDATE documents SET deleted_at = now(), parse_status = 'deleted', updated_at = now()
         WHERE id = $1 AND parse_status = 'deleting'",
    )
    .bind(document_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    let _ = ws;
    workflow::dispatch(pool).await?;
    Ok(())
}

pub async fn run_datatable(pool: &PgPool, document_id: Uuid, attempt: i32) -> Result<(), String> {
    if !workflow::current(pool, document_id, attempt)
        .await
        .map_err(|e| e.to_string())?
    {
        return Ok(());
    }
    let Some(doc) = crate::load_document(pool, document_id)
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(());
    };
    let Some(mut version) = crate::load_version(pool, doc.product_version_id)
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(());
    };
    crate::resolve_process_config(&version, doc.process_overrides.as_ref()).apply_to(&mut version);
    version.embedding_model_id = crate::freeze_version_embedding_model(pool, version.id).await?;
    let (table, embeds) = datatable_chunks(&doc, &version)?;
    let Some(mut job) = crate::DocJob::from_pool(pool, document_id)
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(());
    };
    job.chunks = table.into_iter().map(|c| (c.id, c)).collect();
    job.embeddings = embeds.into_iter().map(|e| (e.chunk_id, e)).collect();
    workflow::commit_chunks(
        pool,
        &job,
        attempt,
        "datatable",
        &["table_summary", "table_column"],
        None,
        &[],
        true,
    )
    .await?;
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

fn datatable_chunks(
    doc: &crate::Document,
    version: &crate::ProductVersion,
) -> Result<(Vec<crate::Chunk>, Vec<crate::ChunkEmbedding>), String> {
    if !is_table_file(&doc.file_name) {
        return Ok((Vec::new(), Vec::new()));
    }
    let ext = doc
        .file_name
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let bytes = platform::read_blob(&doc.file_hash).unwrap_or_default();
    let markdown = converted_markdown(doc);
    let (headers, rows) = if ext == "csv" {
        parse_csv_sample(&bytes)
    } else {
        sample_from_converted_markdown(&markdown)
    };
    if headers.is_empty() {
        if matches!(ext.as_str(), "xlsx" | "xls") && markdown.trim().is_empty() {
            return Err("datatable waiting for docreader convert markdown".into());
        }
        return Ok((Vec::new(), Vec::new()));
    }
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
    // D3: table chunks carry a Spreadsheet source locator (sheet = this file).
    let table_locator = serde_json::json!([{
        "key": format!("datatable:{table_name}"),
        "ordinal": 0u32,
        "kind": "spreadsheet",
        "locator": {
            "locator_kind": "spreadsheet",
            "sheet_ordinal": 0u32,
            "sheet_name": table_name,
            "region": {
                "a1_range": "",
                "start_row": 0u32,
                "start_column": 0u32,
                "end_row": rows.len() as u32,
                "end_column": headers.len() as u32
            },
            "cells": [],
            "merged_ranges": [],
            "defined_tables": []
        },
        "grid": null
    }]);
    let summary = crate::Chunk {
        id: Uuid::new_v4(),
        document_id: doc.id,
        product_version_id: doc.product_version_id,
        chunk_type: "table_summary".into(),
        content: table_content.clone(),
        context_header: String::new(),
        start_at: 0,
        end_at: table_content.chars().count() as i32,
        parent_chunk_id: None,
        generated_questions: Vec::new(),
        source_locator: Some(table_locator.clone()),
    };
    let column = crate::Chunk {
        id: Uuid::new_v4(),
        document_id: doc.id,
        product_version_id: doc.product_version_id,
        chunk_type: "table_column".into(),
        content: col_content.clone(),
        context_header: String::new(),
        start_at: 0,
        end_at: col_content.chars().count() as i32,
        parent_chunk_id: Some(summary.id),
        generated_questions: Vec::new(),
        source_locator: Some(table_locator),
    };
    let mut embeddings = std::collections::HashMap::new();
    for ch in [&summary, &column] {
        crate::index::index_one_in(
            &mut embeddings,
            ch,
            &doc.title,
            version.vector_enabled,
            version.keyword_enabled,
        )?;
    }
    let embeds = embeddings.into_values().collect();
    Ok((vec![summary, column], embeds))
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

fn converted_markdown(doc: &crate::Document) -> String {
    // E1: Document.markdown was write-never (always empty); the converted
    // markdown now always comes from the {file_hash}.md blob.
    platform::read_blob(&format!("{}.{}.{}.md", doc.file_hash, doc.id, doc.attempt))
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

async fn question_job_key(
    pool: &PgPool,
    document_id: Uuid,
    attempt: i32,
    chunk_ids: &[Uuid],
) -> Result<String, String> {
    let key:Option<String>=sqlx::query_scalar("SELECT job_key FROM knowledge_job_outbox WHERE document_id=$1 AND attempt=$2 AND payload->'chunk_ids'=$3")
        .bind(document_id).bind(attempt).bind(serde_json::json!(chunk_ids)).fetch_optional(pool).await.map_err(|e|e.to_string())?;
    Ok(key.unwrap_or_else(|| {
        format!(
            "questions:{}",
            chunk_ids
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",")
        )
    }))
}

fn task_enabled(task_type: &str) -> bool {
    !matches!(
        platform::launch_mode(task_type),
        Ok(Some(platform::LaunchMode::DeclaredDisabled))
    )
}
