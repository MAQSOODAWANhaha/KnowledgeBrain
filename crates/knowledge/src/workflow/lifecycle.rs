use sqlx::{PgConnection, Row};
use uuid::Uuid;

pub(crate) struct DocumentState {
    attempt: i32,
    parse_status: crate::ParseStatus,
    source_state: serde_json::Value,
}

/// Hold the document lock until its state change and durable obligations commit.
pub(crate) async fn capture_document_state(
    conn: &mut PgConnection,
    document_id: Uuid,
) -> Result<Option<DocumentState>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT attempt,parse_status,jsonb_build_array(deleted_at,enable_status,index_ready,
            parse_status,summary_status,active_generation) AS source_state
         FROM knowledge_documents WHERE id=$1 FOR UPDATE",
    )
    .bind(document_id)
    .fetch_optional(conn)
    .await?;
    row.map(|row| {
        Ok(DocumentState {
            attempt: row.get("attempt"),
            parse_status: row.try_get("parse_status")?,
            source_state: row.get("source_state"),
        })
    })
    .transpose()
}

/// Business state changes own their queue obligations and derived-data cleanup.
/// `None` is the initial insert; callers must call this before committing.
pub(crate) async fn record_document_change(
    conn: &mut PgConnection,
    document_id: Uuid,
    before: Option<DocumentState>,
) -> Result<(), sqlx::Error> {
    let Some(after) = capture_document_state(conn, document_id).await? else {
        return Ok(());
    };
    let attempt_changed = before
        .as_ref()
        .is_some_and(|old| old.attempt != after.attempt);
    let terminal = matches!(
        after.parse_status,
        crate::ParseStatus::Cancelled | crate::ParseStatus::Deleting | crate::ParseStatus::Deleted
    );
    if before
        .as_ref()
        .is_some_and(|old| old.source_state != after.source_state)
    {
        sqlx::query(
            "INSERT INTO knowledge_job_outbox(document_id,product_version_id,attempt,job_key,payload)
             SELECT id,product_version_id,attempt,'reconcile','{\"kind\":\"reconcile\"}'::jsonb FROM knowledge_documents WHERE id=$1
             ON CONFLICT(document_id,attempt,job_key) DO UPDATE SET state='pending',deliveries=0,
                revision=knowledge_job_outbox.revision+1,next_delivery_at=now(),last_error=NULL",
        ).bind(document_id).execute(&mut *conn).await?;
    }
    if attempt_changed || terminal {
        sqlx::query(
            "UPDATE knowledge_job_outbox SET state='complete' WHERE document_id=$1
             AND job_key<>'reconcile' AND (attempt<>$2 OR $3)",
        )
        .bind(document_id)
        .bind(after.attempt)
        .bind(terminal)
        .execute(&mut *conn)
        .await?;
    }
    if after.parse_status == crate::ParseStatus::Pending && (before.is_none() || attempt_changed) {
        sqlx::query(
            "INSERT INTO knowledge_job_outbox(document_id,product_version_id,attempt,job_key,payload)
             SELECT id,product_version_id,attempt,'convert',jsonb_build_object('kind','convert',
                'manual',document_kind='manual','passages',CASE WHEN jsonb_typeof(source_passages)='array'
                THEN source_passages ELSE '[]'::jsonb END) FROM knowledge_documents WHERE id=$1
             ON CONFLICT DO NOTHING",
        ).bind(document_id).execute(&mut *conn).await?;
    }
    if attempt_changed
        || terminal
            && before
                .as_ref()
                .is_some_and(|old| old.parse_status != after.parse_status)
    {
        sqlx::query(
            "DELETE FROM knowledge_chunks chunk USING knowledge_wiki_pages page,knowledge_documents doc
             WHERE doc.id=$1 AND chunk.product_version_id=doc.product_version_id AND chunk.chunk_type='wiki_page'
             AND page.product_version_id=chunk.product_version_id AND page.slug=chunk.context_header
             AND (page.source_refs @> jsonb_build_array(doc.id::text) OR page.source_refs='[]'::jsonb)",
        ).bind(document_id).execute(&mut *conn).await?;
        sqlx::query(
            "UPDATE knowledge_wiki_pages page SET status='draft',updated_at=now() FROM knowledge_documents doc
             WHERE doc.id=$1 AND page.product_version_id=doc.product_version_id
             AND (page.source_refs @> jsonb_build_array(doc.id::text) OR page.source_refs='[]'::jsonb)",
        ).bind(document_id).execute(conn).await?;
    }
    Ok(())
}
