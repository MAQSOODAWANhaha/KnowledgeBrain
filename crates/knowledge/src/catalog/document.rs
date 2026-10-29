//! SQL persistence split from persist.rs (behavior unchanged).
use crate::{append_document_chunks, delete_chunks_by_types};
use sqlx::{PgPool, Row};
use std::collections::HashMap;
use uuid::Uuid;

pub(crate) fn document_from_row(d: sqlx::postgres::PgRow) -> Result<crate::Document, sqlx::Error> {
    let did: Uuid = d.try_get("id")?;
    let title: String = d.try_get("title")?;
    let file_name: String = d.try_get("file_name")?;
    let file_size: i64 = d.try_get("file_size")?;
    let file_hash: String = d.try_get("file_hash")?;
    let object_ref: String = d.try_get("object_ref")?;
    let vid: Uuid = d.try_get("product_version_id")?;
    let mut doc = crate::Document::new(vid, title, file_name, file_size, file_hash, object_ref);
    doc.id = did;
    doc.parse_status = d.try_get("parse_status")?;
    doc.enable_status = d.try_get("enable_status")?;
    doc.pending_subtasks_count = d.try_get("pending_subtasks_count")?;
    doc.error_message = d.try_get("error_message")?;
    doc.attempt = d.try_get("attempt").unwrap_or(1);
    doc.description = d.try_get("description").unwrap_or_default();
    doc.summary_status = d.try_get("summary_status")?;
    doc.index_ready = d.try_get("index_ready").unwrap_or(false);
    doc.doc_type = d.try_get("doc_type").unwrap_or_else(|_| "file".into());
    if let Ok(Some(raw)) = d.try_get::<Option<serde_json::Value>, _>("source_passages") {
        doc.source_passages = serde_json::from_value(raw).unwrap_or_default();
    }
    if let Ok(Some(raw)) = d.try_get::<Option<serde_json::Value>, _>("process_overrides") {
        doc.process_overrides = serde_json::from_value(raw).ok();
    }
    Ok(doc)
}

pub async fn list_documents_in_version(
    pool: &PgPool,
    version_id: Uuid,
    parse_status: Option<crate::ParseStatus>,
    keyword: Option<&str>,
    tag_id: Option<Uuid>,
) -> Result<Vec<crate::Document>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, title, file_name, byte_length AS file_size, content_sha256 AS file_hash, object_ref,
                parse_status, enable_status, pending_subtasks_count,
                COALESCE(error_message, '') AS error_message,
                process_overrides,
                COALESCE(document_kind, 'file') AS doc_type,
                COALESCE(attempt, 1) AS attempt,
                COALESCE(description, '') AS description,
                COALESCE(summary_status, 'none') AS summary_status,
                source_passages, index_ready, product_version_id
         FROM knowledge_documents
         WHERE product_version_id = $1 AND deleted_at IS NULL
           AND ($2::text IS NULL OR parse_status = $2)
           AND (
             $3::text IS NULL
             OR title ILIKE '%' || $3 || '%'
             OR file_name ILIKE '%' || $3 || '%'
             OR COALESCE(description, '') ILIKE '%' || $3 || '%'
           )
           AND (
             $4::uuid IS NULL
             OR EXISTS (
                SELECT 1 FROM knowledge_document_tags dt
                WHERE dt.document_id = knowledge_documents.id AND dt.tag_id = $4
             )
           )
         ORDER BY updated_at DESC",
    )
    .bind(version_id)
    .bind(parse_status)
    .bind(keyword)
    .bind(tag_id)
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(document_from_row).collect()
}

pub async fn load_document(
    pool: &PgPool,
    document_id: Uuid,
) -> Result<Option<crate::Document>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id, title, file_name, byte_length AS file_size, content_sha256 AS file_hash, object_ref,
                parse_status, enable_status, pending_subtasks_count,
                COALESCE(error_message, '') AS error_message,
                process_overrides,
                COALESCE(document_kind, 'file') AS doc_type,
                COALESCE(attempt, 1) AS attempt,
                COALESCE(description, '') AS description,
                COALESCE(summary_status, 'none') AS summary_status,
                source_passages, index_ready, product_version_id
         FROM knowledge_documents WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(document_id)
    .fetch_optional(pool)
    .await?;
    row.map(document_from_row).transpose()
}

/// Load one document plus its version/product/workspace and that document's chunks.
pub async fn document_workspace_id(
    pool: &PgPool,
    document_id: Uuid,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT p.workspace_id FROM knowledge_documents d
         JOIN knowledge_product_versions pv ON pv.id = d.product_version_id
         JOIN knowledge_products p ON p.id = pv.product_id
         WHERE d.id = $1",
    )
    .bind(document_id)
    .fetch_optional(pool)
    .await
}

pub struct NewDocument<'a> {
    pub id: Uuid,
    pub product_version_id: Uuid,
    pub title: &'a str,
    pub file_name: &'a str,
    pub file_size: i64,
    pub file_hash: &'a str,
    pub object_ref: &'a str,
}

pub struct NewIngestDocument<'a> {
    pub document: NewDocument<'a>,
    pub doc_type: &'a str,
    pub source_passages: &'a [String],
    pub process_overrides: Option<&'a crate::ProcessOverrides>,
    pub tag_ids: &'a [Uuid],
}

pub async fn find_duplicate_document(
    pool: &PgPool,
    version_id: Uuid,
    file_name: &str,
    file_size: i64,
    file_hash: &str,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT id FROM knowledge_documents
         WHERE product_version_id = $1 AND file_name = $2 AND byte_length = $3
           AND content_sha256 = $4 AND deleted_at IS NULL",
    )
    .bind(version_id)
    .bind(file_name)
    .bind(file_size)
    .bind(file_hash)
    .fetch_optional(pool)
    .await
}

pub async fn insert_ingest_document(
    pool: &PgPool,
    input: NewIngestDocument<'_>,
) -> Result<(), sqlx::Error> {
    let NewIngestDocument {
        document: doc,
        doc_type,
        source_passages,
        process_overrides,
        tag_ids,
    } = input;
    let source_passages = if source_passages.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::json!(source_passages)
    };
    let process_overrides = process_overrides
        .map(serde_json::to_value)
        .transpose()
        .map_err(|error| sqlx::Error::Encode(Box::new(error)))?;
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO knowledge_documents (
            id, product_version_id, title, parse_status, enable_status,
            file_name, byte_length, content_sha256, object_ref, document_kind,
            source_passages, process_overrides
         ) VALUES ($1,$2,$3,'pending','disabled',$4,$5,$6,$7,$8,$9,$10)",
    )
    .bind(doc.id)
    .bind(doc.product_version_id)
    .bind(doc.title)
    .bind(doc.file_name)
    .bind(doc.file_size)
    .bind(doc.file_hash)
    .bind(doc.object_ref)
    .bind(doc_type)
    .bind(source_passages)
    .bind(process_overrides)
    .execute(&mut *tx)
    .await?;
    platform::register_knowledge_document_object_in_tx(
        &mut tx,
        doc.id,
        "application/octet-stream",
        &format!("knowledge-document:{}", doc.id),
    )
    .await?;
    for tag_id in tag_ids {
        sqlx::query(
            "INSERT INTO knowledge_document_tags (document_id,tag_id) VALUES ($1,$2)
             ON CONFLICT DO NOTHING",
        )
        .bind(doc.id)
        .bind(tag_id)
        .execute(&mut *tx)
        .await?;
    }
    crate::workflow::record_document_change(&mut tx, doc.id, None).await?;
    tx.commit().await
}

pub async fn insert_document(pool: &PgPool, doc: NewDocument<'_>) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO knowledge_documents (
            id, product_version_id, title, parse_status, enable_status,
            file_name, byte_length, content_sha256, object_ref, document_kind
         ) VALUES ($1, $2, $3, 'pending', 'disabled', $4, $5, $6, $7, 'file')",
    )
    .bind(doc.id)
    .bind(doc.product_version_id)
    .bind(doc.title)
    .bind(doc.file_name)
    .bind(doc.file_size)
    .bind(doc.file_hash)
    .bind(doc.object_ref)
    .execute(&mut *tx)
    .await?;
    platform::register_knowledge_document_object_in_tx(
        &mut tx,
        doc.id,
        "application/octet-stream",
        &format!("knowledge-document:{}", doc.id),
    )
    .await?;
    crate::workflow::record_document_change(&mut tx, doc.id, None).await?;
    tx.commit().await
}

pub async fn set_index_ready(
    pool: &PgPool,
    document_id: Uuid,
    ready: bool,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    let before = crate::workflow::capture_document_state(&mut tx, document_id).await?;
    sqlx::query(
        "UPDATE knowledge_documents SET index_ready = $2, updated_at = now() WHERE id = $1",
    )
    .bind(document_id)
    .bind(ready)
    .execute(&mut *tx)
    .await?;
    crate::workflow::record_document_change(&mut tx, document_id, before).await?;
    tx.commit().await?;
    Ok(())
}

pub async fn set_document_source(
    pool: &PgPool,
    document_id: Uuid,
    doc_type: &str,
    passages: &[String],
) -> Result<(), sqlx::Error> {
    let raw = if passages.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::json!(passages)
    };
    sqlx::query(
        "UPDATE knowledge_documents SET document_kind = $2, source_passages = $3, updated_at = now() WHERE id = $1",
    )
    .bind(document_id)
    .bind(doc_type)
    .bind(raw)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn persist_summary_maps(
    pool: &PgPool,
    chunks: &HashMap<Uuid, crate::Chunk>,
    embeddings: &HashMap<Uuid, crate::ChunkEmbedding>,
    document_id: Uuid,
) -> Result<(), sqlx::Error> {
    delete_chunks_by_types(pool, document_id, &["summary"]).await?;
    let chunks: Vec<_> = chunks
        .values()
        .filter(|c| c.document_id == document_id && c.chunk_type == "summary")
        .cloned()
        .collect();
    let ids: std::collections::HashSet<_> = chunks.iter().map(|c| c.id).collect();
    let embeddings: Vec<_> = embeddings
        .values()
        .filter(|e| ids.contains(&e.chunk_id))
        .cloned()
        .collect();
    append_document_chunks(pool, &chunks, &embeddings).await
}

pub async fn persist_question_maps(
    pool: &PgPool,
    chunks: &std::collections::HashMap<Uuid, crate::Chunk>,
    embeddings: &std::collections::HashMap<Uuid, crate::ChunkEmbedding>,
    document_id: Uuid,
    parent_ids: &[Uuid],
) -> Result<(), sqlx::Error> {
    if !parent_ids.is_empty() {
        sqlx::query(
            "DELETE FROM knowledge_chunks
             WHERE document_id = $1
               AND (
                    chunk_type = 'question'
                    OR (chunk_type = 'text'
                        AND parent_chunk_id = ANY($2)
                        AND COALESCE(jsonb_array_length(generated_questions), 0) = 0)
               )
               AND parent_chunk_id = ANY($2)",
        )
        .bind(document_id)
        .bind(parent_ids)
        .execute(pool)
        .await?;
    }
    for ch in chunks.values().filter(|c| parent_ids.contains(&c.id)) {
        sqlx::query("UPDATE knowledge_chunks SET generated_questions = $2 WHERE id = $1")
            .bind(ch.id)
            .bind(serde_json::json!(ch.generated_questions))
            .execute(pool)
            .await?;
    }
    let parents: std::collections::HashSet<_> = parent_ids.iter().copied().collect();
    let kids: Vec<_> = chunks
        .values()
        .filter(|c| {
            c.document_id == document_id
                && c.chunk_type == "question"
                && c.parent_chunk_id.is_some_and(|p| parents.contains(&p))
        })
        .cloned()
        .collect();
    let ids: std::collections::HashSet<_> = kids.iter().map(|c| c.id).collect();
    let embeddings: Vec<_> = embeddings
        .values()
        .filter(|e| ids.contains(&e.chunk_id))
        .cloned()
        .collect();
    append_document_chunks(pool, &kids, &embeddings).await
}

pub async fn set_process_overrides(
    pool: &PgPool,
    document_id: Uuid,
    overrides: &crate::ProcessOverrides,
) -> Result<(), sqlx::Error> {
    let raw = serde_json::to_value(overrides).unwrap_or(serde_json::Value::Null);
    sqlx::query(
        "UPDATE knowledge_documents SET process_overrides = $2, updated_at = now() WHERE id = $1",
    )
    .bind(document_id)
    .bind(raw)
    .execute(pool)
    .await?;
    Ok(())
}

/// Brain `SetFinalizing`: only from `processing`. Returns whether the row flipped.
pub async fn set_finalizing(
    pool: &PgPool,
    document_id: Uuid,
    pending_subtasks_count: i32,
) -> Result<bool, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let before = crate::workflow::capture_document_state(&mut tx, document_id).await?;
    let n = sqlx::query(
        "UPDATE knowledge_documents SET parse_status = 'finalizing', pending_subtasks_count = $2,
                updated_at = now()
         WHERE id = $1 AND parse_status = 'processing'",
    )
    .bind(document_id)
    .bind(pending_subtasks_count)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    crate::workflow::record_document_change(&mut tx, document_id, before).await?;
    tx.commit().await?;
    Ok(n == 1)
}

pub async fn set_document_progress(
    pool: &PgPool,
    document_id: Uuid,
    status: crate::ParseStatus,
    pending_subtasks_count: i32,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    let before = crate::workflow::capture_document_state(&mut tx, document_id).await?;
    sqlx::query(
        "UPDATE knowledge_documents SET parse_status = $2, pending_subtasks_count = $3,
                updated_at = now() WHERE id = $1",
    )
    .bind(document_id)
    .bind(status)
    .bind(pending_subtasks_count)
    .execute(&mut *tx)
    .await?;
    crate::workflow::record_document_change(&mut tx, document_id, before).await?;
    tx.commit().await?;
    Ok(())
}

pub async fn set_summary_status(
    pool: &PgPool,
    document_id: Uuid,
    status: crate::SummaryStatus,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    let before = crate::workflow::capture_document_state(&mut tx, document_id).await?;
    sqlx::query(
        "UPDATE knowledge_documents SET summary_status = $2, updated_at = now() WHERE id = $1",
    )
    .bind(document_id)
    .bind(status)
    .execute(&mut *tx)
    .await?;
    crate::workflow::record_document_change(&mut tx, document_id, before).await?;
    tx.commit().await?;
    Ok(())
}

pub async fn set_parse_status(
    pool: &PgPool,
    document_id: Uuid,
    status: crate::ParseStatus,
    error_message: &str,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    let before = crate::workflow::capture_document_state(&mut tx, document_id).await?;
    sqlx::query(
        "UPDATE knowledge_documents SET parse_status = $2, error_message = $3, updated_at = now() WHERE id = $1",
    )
        .bind(document_id)
        .bind(status)
        .bind(error_message)
        .execute(&mut *tx)
        .await?;
    crate::workflow::record_document_change(&mut tx, document_id, before).await?;
    tx.commit().await?;
    Ok(())
}

/// Spec 3.2: flip to `processing` only from an explicit source-state whitelist.
/// D6: the previous `NOT IN (...)` form also matched rows already in `processing`,
/// so two workers racing on the same attempt could both "win".
/// Crash recovery for stuck `processing`/`finalizing` rows is owned by
/// `housekeep_documents` (resets them to `failed`), hence `failed` — not
/// `finalizing` — is the legal re-entry state here.
pub async fn try_set_processing(pool: &PgPool, document_id: Uuid) -> Result<bool, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let before = crate::workflow::capture_document_state(&mut tx, document_id).await?;
    let n = sqlx::query(
        "UPDATE knowledge_documents SET parse_status = 'processing', error_message = '', updated_at = now()
         WHERE id = $1
           AND parse_status IN ('pending', 'failed')",
    )
    .bind(document_id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    crate::workflow::record_document_change(&mut tx, document_id, before).await?;
    tx.commit().await?;
    Ok(n > 0)
}

pub async fn document_parse_status(
    pool: &PgPool,
    document_id: Uuid,
) -> Result<Option<crate::ParseStatus>, sqlx::Error> {
    sqlx::query_scalar("SELECT parse_status FROM knowledge_documents WHERE id = $1")
        .bind(document_id)
        .fetch_optional(pool)
        .await
}

pub async fn mark_reparse_queued(pool: &PgPool, document_id: Uuid) -> Result<i32, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let before = crate::workflow::capture_document_state(&mut tx, document_id).await?;
    let attempt = sqlx::query_scalar(
        "UPDATE knowledge_documents SET
            attempt = CASE
                WHEN parse_status='pending' AND enable_status='disabled' THEN attempt
                ELSE attempt+1
            END,
            parse_status = 'pending',
            enable_status = 'disabled',
            index_ready = false,
            pending_subtasks_count = 0,
            error_message = '',
            updated_at = now()
         WHERE id = $1 AND deleted_at IS NULL AND parse_status NOT IN ('deleting','deleted')
         RETURNING attempt",
    )
    .bind(document_id)
    .fetch_one(&mut *tx)
    .await?;
    crate::workflow::record_document_change(&mut tx, document_id, before).await?;
    tx.commit().await?;
    Ok(attempt)
}

pub async fn cancel_active_docs_for_versions(
    pool: &PgPool,
    version_ids: &[Uuid],
) -> Result<u64, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM knowledge_documents WHERE product_version_id=ANY($1)
         AND parse_status IN ('pending','processing','finalizing') AND deleted_at IS NULL
         ORDER BY id FOR UPDATE",
    )
    .bind(version_ids)
    .fetch_all(&mut *tx)
    .await?;
    for id in &ids {
        let before = crate::workflow::capture_document_state(&mut tx, *id).await?;
        sqlx::query(
            "UPDATE knowledge_documents SET parse_status='cancelled',updated_at=now() WHERE id=$1",
        )
        .bind(id)
        .execute(&mut *tx)
        .await?;
        crate::workflow::record_document_change(&mut tx, *id, before).await?;
    }
    tx.commit().await?;
    Ok(ids.len() as u64)
}

/// Fail pending/processing/finalizing rows whose last heartbeat
/// (span finished/started, else `updated_at`) is older than `stale_secs`.
pub async fn housekeep_documents(pool: &PgPool, stale_secs: i64) -> Result<u64, sqlx::Error> {
    let msg = format!("task stuck in processing > {stale_secs}s, recovered by housekeeping");
    let mut tx = pool.begin().await?;
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT d.id FROM knowledge_documents d WHERE d.parse_status IN ('processing', 'finalizing')
           AND NOT EXISTS(SELECT 1 FROM knowledge_job_outbox o WHERE o.document_id=d.id AND o.attempt=d.attempt AND o.state IN ('pending','published'))
           AND COALESCE(
                 (SELECT MAX(COALESCE(finished_at, started_at))
                    FROM knowledge_document_processing_spans s
                   WHERE s.document_id = d.id),
                 d.updated_at
               ) < now() - make_interval(secs => $1::double precision)
         ORDER BY d.id FOR UPDATE",
    )
    .bind(stale_secs)
    .fetch_all(&mut *tx)
    .await?;
    for id in &ids {
        let before = crate::workflow::capture_document_state(&mut tx, *id).await?;
        sqlx::query("UPDATE knowledge_documents SET parse_status='failed',error_message=$2,pending_subtasks_count=0,updated_at=now() WHERE id=$1")
            .bind(id).bind(&msg).execute(&mut *tx).await?;
        crate::workflow::record_document_change(&mut tx, *id, before).await?;
    }
    if !ids.is_empty() {
        sqlx::query(
            "UPDATE knowledge_document_processing_spans
             SET status = 'failed', finished_at = COALESCE(finished_at, now()),
                 duration_ms = COALESCE(duration_ms, 0)
             WHERE document_id = ANY($1)
               AND attempt=(SELECT attempt FROM knowledge_documents WHERE id=knowledge_document_processing_spans.document_id)
               AND status IN ('running', 'pending')",
        )
        .bind(&ids)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(ids.len() as u64)
}

pub async fn set_document_description(
    pool: &PgPool,
    document_id: Uuid,
    description: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE knowledge_documents SET description = $2, updated_at = now() WHERE id = $1",
    )
    .bind(document_id)
    .bind(description)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn bump_document_attempt(pool: &PgPool, document_id: Uuid) -> Result<i32, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let before = crate::workflow::capture_document_state(&mut tx, document_id).await?;
    let n: i32 = sqlx::query_scalar(
        "UPDATE knowledge_documents SET
            attempt = attempt + 1,
            parse_status = 'pending',
            enable_status = 'disabled',
            index_ready = false,
            pending_subtasks_count = 0,
            error_message = '',
            updated_at = now()
         WHERE id = $1 AND deleted_at IS NULL AND parse_status NOT IN ('deleting','deleted')
         RETURNING attempt",
    )
    .bind(document_id)
    .fetch_one(&mut *tx)
    .await?;
    crate::workflow::record_document_change(&mut tx, document_id, before).await?;
    tx.commit().await?;
    Ok(n)
}

/// Tombstone-only cleanup. No queue envelope may turn this into a live
/// document-wide purge; new attempts are protected by the locked status gate.
pub async fn purge_document_index(pool: &PgPool, document_id: Uuid) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    let deleted:Option<Uuid>=sqlx::query_scalar("SELECT id FROM knowledge_documents WHERE id=$1 AND parse_status IN ('deleting','deleted') FOR UPDATE").bind(document_id).fetch_optional(&mut *tx).await?;
    if deleted.is_none() {
        return Ok(());
    }
    sqlx::query("DELETE FROM knowledge_graph_relations WHERE document_id=$1")
        .bind(document_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM knowledge_graph_nodes WHERE document_id=$1")
        .bind(document_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM knowledge_chunks WHERE document_id=$1 AND chunk_type<>'wiki_page'")
        .bind(document_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "DELETE FROM knowledge_wiki_pages WHERE source_refs @> jsonb_build_array($1::text)",
    )
    .bind(document_id.to_string())
    .execute(&mut *tx)
    .await?;
    tx.commit().await
}

pub async fn copy_document_index(
    pool: &PgPool,
    source_document_id: Uuid,
    target_document_id: Uuid,
    target_version_id: Uuid,
) -> Result<usize, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let copied = copy_document_index_tx(
        &mut tx,
        source_document_id,
        target_document_id,
        target_version_id,
    )
    .await?;
    tx.commit().await?;
    Ok(copied)
}

/// Share the clone initialization transaction so its index and follow-up
/// obligation become visible together. Lock both generation selectors until
/// every chunk, embedding and graph row has been copied.
pub(crate) async fn copy_document_index_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    source_document_id: Uuid,
    target_document_id: Uuid,
    target_version_id: Uuid,
) -> Result<usize, sqlx::Error> {
    let source: Option<(Uuid, i32)> = sqlx::query_as(
        "SELECT product_version_id, active_generation FROM knowledge_documents
         WHERE id = $1 AND deleted_at IS NULL
           AND parse_status NOT IN ('cancelled','deleting','deleted') FOR SHARE",
    )
    .bind(source_document_id)
    .fetch_optional(&mut **tx)
    .await?;
    let Some((source_version_id, source_generation)) = source else {
        return Ok(0);
    };
    let target_generation = sqlx::query_scalar::<_, i32>(
        "SELECT attempt FROM knowledge_documents WHERE id = $1 AND product_version_id = $2
         AND deleted_at IS NULL AND parse_status IN ('pending','processing') FOR UPDATE",
    )
    .bind(target_document_id)
    .bind(target_version_id)
    .fetch_one(&mut **tx)
    .await?;
    let rows = sqlx::query(
        "SELECT id, chunk_type, content, context_header, start_offset AS start_at, end_offset AS end_at,
                parent_chunk_id, generated_questions, source_locator
         FROM knowledge_chunks WHERE document_id = $1 AND generation = $2
           AND chunk_type <> 'wiki_page'",
    )
    .bind(source_document_id)
    .bind(source_generation)
    .fetch_all(&mut **tx)
    .await?;

    struct ChunkCopy {
        new_id: Uuid,
        chunk_type: String,
        content: String,
        context_header: String,
        start_at: i32,
        end_at: i32,
        parent_chunk_id: Option<Uuid>,
        generated_questions: serde_json::Value,
        source_locator: Option<serde_json::Value>,
    }
    let mut id_map: std::collections::HashMap<Uuid, Uuid> =
        std::collections::HashMap::with_capacity(rows.len());
    let mut copies: Vec<ChunkCopy> = Vec::with_capacity(rows.len());
    for r in &rows {
        let old: Uuid = r.try_get("id")?;
        let new_id = Uuid::new_v4();
        id_map.insert(old, new_id);
        copies.push(ChunkCopy {
            new_id,
            chunk_type: r.try_get("chunk_type")?,
            content: r.try_get("content")?,
            context_header: r.try_get("context_header")?,
            start_at: r.try_get("start_at")?,
            end_at: r.try_get("end_at")?,
            parent_chunk_id: None,
            generated_questions: r.try_get("generated_questions")?,
            source_locator: r.try_get("source_locator")?,
        });
    }
    for (r, c) in rows.iter().zip(copies.iter_mut()) {
        let parent: Option<Uuid> = r.try_get("parent_chunk_id")?;
        c.parent_chunk_id = parent.and_then(|p| id_map.get(&p).copied());
    }
    for batch in copies.chunks(1000) {
        let mut qb = sqlx::QueryBuilder::new(
            "INSERT INTO knowledge_chunks (id, product_version_id, document_id, chunk_type, content,
               context_header, start_offset, end_offset, parent_chunk_id, generated_questions,
               source_locator, generation) ",
        );
        qb.push_values(batch, |mut b, c| {
            b.push_bind(c.new_id)
                .push_bind(target_version_id)
                .push_bind(target_document_id)
                .push_bind(&c.chunk_type)
                .push_bind(&c.content)
                .push_bind(&c.context_header)
                .push_bind(c.start_at)
                .push_bind(c.end_at)
                .push_bind(c.parent_chunk_id)
                .push_bind(&c.generated_questions)
                .push_bind(&c.source_locator)
                .push_bind(target_generation);
        });
        qb.build().execute(&mut **tx).await?;
    }
    if !id_map.is_empty() {
        // Set-based embedding copy through the chunk id map: one statement.
        let old_ids: Vec<Uuid> = id_map.keys().copied().collect();
        let new_ids: Vec<Uuid> = old_ids.iter().map(|o| id_map[o]).collect();
        sqlx::query(
            "INSERT INTO knowledge_chunk_embeddings
                (chunk_id, product_version_id, document_id, embedding, tsv, content)
             SELECT m.new_id, $1, $2, e.embedding, e.tsv, e.content
             FROM knowledge_chunk_embeddings e
             JOIN (SELECT * FROM unnest($3::uuid[], $4::uuid[])) AS m(old_id, new_id)
               ON m.old_id = e.chunk_id",
        )
        .bind(target_version_id)
        .bind(target_document_id)
        .bind(&old_ids)
        .bind(&new_ids)
        .execute(&mut **tx)
        .await?;
        // OCR evidence belongs to a document/version-specific immutable media
        // artifact. Preserve that mapping and its object owner with fresh target
        // identities; copied chunks must not lose their retrievable evidence.
        let media = sqlx::query(
            "SELECT mapping.chunk_id, artifact.id, artifact.canonical_payload,
                    artifact.object_ref, artifact.content_sha256, artifact.media_type,
                    registry.byte_length
             FROM knowledge_image_chunk_links mapping
             JOIN knowledge_image_artifact_revisions artifact
               ON artifact.id = mapping.image_artifact_revision_id
             JOIN platform_available_objects registry ON registry.object_ref = artifact.object_ref
             WHERE mapping.chunk_id = ANY($1) AND EXISTS(SELECT 1 FROM knowledge_chunks WHERE id=mapping.chunk_id AND chunk_type='image_ocr')",
        )
        .bind(&old_ids)
        .fetch_all(&mut **tx)
        .await?;
        for artifact in media {
            let artifact_id = Uuid::new_v4();
            let old_artifact_id: Uuid = artifact.try_get("id")?;
            let old_chunk_id: Uuid = artifact.try_get("chunk_id")?;
            let payload: Vec<u8> = artifact.try_get("canonical_payload")?;
            let mut payload: serde_json::Value = serde_json::from_slice(&payload)
                .map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
            payload["image_artifact_revision_id"] = serde_json::json!(artifact_id);
            payload["document_id"] = serde_json::json!(target_document_id);
            payload["product_version_id"] = serde_json::json!(target_version_id);
            let payload =
                serde_json::to_vec(&payload).map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
            let artifact_sha = platform::sha256_hex(&payload);
            platform::register_knowledge_image_object_in_tx(
                tx,
                artifact_id,
                &artifact.try_get::<String, _>("object_ref")?,
                &artifact.try_get::<String, _>("content_sha256")?,
                &artifact.try_get::<String, _>("media_type")?,
                artifact.try_get::<i64, _>("byte_length")?,
            )
            .await?;
            sqlx::query(
                "INSERT INTO knowledge_image_artifact_revisions(
                    id,product_version_id,document_id,revision,object_ref,content_sha256,
                    media_type,width,height,page_ordinal,bounding_region,source_image_key,
                    canonical_payload,artifact_sha256)
                 SELECT $1,$2,$3,revision,object_ref,content_sha256,media_type,width,height,
                        page_ordinal,bounding_region,source_image_key,$4,$6
                 FROM knowledge_image_artifact_revisions WHERE id=$5",
            )
            .bind(artifact_id)
            .bind(target_version_id)
            .bind(target_document_id)
            .bind(payload)
            .bind(old_artifact_id)
            .bind(artifact_sha)
            .execute(&mut **tx)
            .await?;
            sqlx::query(
                "INSERT INTO knowledge_image_chunk_links(
                    chunk_id,product_version_id,document_id,image_artifact_revision_id,
                    object_ref,content_sha256,media_type)
                 SELECT $1,$2,$3,$4,object_ref,content_sha256,media_type
                 FROM knowledge_image_chunk_links WHERE chunk_id=$5",
            )
            .bind(id_map[&old_chunk_id])
            .bind(target_version_id)
            .bind(target_document_id)
            .bind(artifact_id)
            .bind(old_chunk_id)
            .execute(&mut **tx)
            .await?;
        }
    }
    // D7: deep-copy the per-document graph, remapping chunk_ids to the new ids.
    let graph_nodes = sqlx::query(
        "SELECT name, chunk_ids FROM knowledge_graph_nodes
         WHERE product_version_id = $1 AND document_id = $2 AND generation = $3",
    )
    .bind(source_version_id)
    .bind(source_document_id)
    .bind(source_generation)
    .fetch_all(&mut **tx)
    .await?;
    let mut node_copies: Vec<(String, Vec<Uuid>)> = Vec::with_capacity(graph_nodes.len());
    for n in &graph_nodes {
        let name: String = n.try_get("name")?;
        let chunk_ids: Vec<Uuid> = n.try_get("chunk_ids")?;
        let remapped: Vec<Uuid> = chunk_ids
            .into_iter()
            .filter_map(|c| id_map.get(&c).copied())
            .collect();
        if !remapped.is_empty() {
            node_copies.push((name, remapped));
        }
    }
    for batch in node_copies.chunks(1000) {
        let mut qb = sqlx::QueryBuilder::new(
            "INSERT INTO knowledge_graph_nodes (product_version_id, document_id, name, chunk_ids, generation) ",
        );
        qb.push_values(batch, |mut b, (name, chunk_ids)| {
            b.push_bind(target_version_id)
                .push_bind(target_document_id)
                .push_bind(name)
                .push_bind(chunk_ids)
                .push_bind(target_generation);
        });
        qb.push(" ON CONFLICT DO NOTHING");
        qb.build().execute(&mut **tx).await?;
    }
    sqlx::query(
        "INSERT INTO knowledge_graph_relations
            (product_version_id, document_id, source_node_name, target_node_name, relation_type, generation)
         SELECT $1, $2, source_node_name AS node1, target_node_name AS node2, relation_type AS rel_type, (SELECT attempt FROM knowledge_documents WHERE id=$2) FROM knowledge_graph_relations
         WHERE product_version_id = $3 AND document_id = $4 AND generation = $5
           AND source_node_name = ANY($6::text[]) AND target_node_name = ANY($6::text[])
         ON CONFLICT DO NOTHING",
    )
    .bind(target_version_id)
    .bind(target_document_id)
    .bind(source_version_id)
    .bind(source_document_id)
    .bind(source_generation)
    .bind(node_copies.iter().map(|(name, _)| name).collect::<Vec<_>>())
    .execute(&mut **tx)
    .await?;
    Ok(rows.len())
}

pub async fn document_chunk_count(pool: &PgPool, document_id: Uuid) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT count(*) FROM knowledge_chunks WHERE document_id = $1")
        .bind(document_id)
        .fetch_one(pool)
        .await
}

pub async fn document_file_meta(
    pool: &PgPool,
    document_id: Uuid,
) -> Result<Option<(String, String)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT COALESCE(file_name, ''), COALESCE(object_ref, '')
         FROM knowledge_documents WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(document_id)
    .fetch_optional(pool)
    .await
}

pub async fn document_image_object_refs(
    pool: &PgPool,
    document_id: Uuid,
) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT DISTINCT context_header FROM knowledge_chunks
         WHERE document_id = $1
           AND chunk_type IN ('image_ocr', 'image_caption')
           AND context_header IS NOT NULL AND context_header <> ''",
    )
    .bind(document_id)
    .fetch_all(pool)
    .await
}
