use super::{ObjectDeletionIdentity, add_object_reference, release_object_reference};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

pub async fn register_knowledge_document_object(
    pool: &PgPool,
    document_id: Uuid,
    media_type: &str,
    idempotency_key: &str,
) -> Result<String, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let reference =
        register_knowledge_document_object_in_tx(&mut tx, document_id, media_type, idempotency_key)
            .await?;
    tx.commit().await?;
    Ok(reference)
}

/// Register the original in the same transaction as document creation/cloning.
pub async fn register_knowledge_document_object_in_tx(
    conn: &mut PgConnection,
    document_id: Uuid,
    media_type: &str,
    idempotency_key: &str,
) -> Result<String, sqlx::Error> {
    let (reference, digest, length, deleted): (String, String, i64, bool) = sqlx::query_as(
        "SELECT object_ref::text,content_sha256::text AS file_hash,byte_length AS file_size,deleted_at IS NOT NULL
         FROM knowledge_documents WHERE id=$1 FOR UPDATE",
    )
    .bind(document_id)
    .fetch_one(&mut *conn)
    .await?;
    if deleted {
        return Err(sqlx::Error::Protocol(
            "deleted knowledge document cannot acquire an object".into(),
        ));
    }
    let registered: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM platform_object_references
         WHERE owner_kind='knowledge_document' AND owner_id=$1 AND occurrence='original')",
    )
    .bind(document_id)
    .fetch_one(&mut *conn)
    .await?;
    add_object_reference(
        conn,
        &reference,
        &digest,
        media_type,
        length,
        "knowledge_document",
        document_id,
        "original",
    )
    .await?;
    if !registered {
        sqlx::query(
            "INSERT INTO platform_audit_events(id,schema_version,operation,idempotency_key,
             request_sha256,response_sha256,entity_kind,entity_locator,after_revision,after_sha256)
             VALUES($1,1,'knowledge.document.object.register',$2,
             encode(digest(convert_to(jsonb_build_object('schema_version',1,'document_id',$3::uuid,
                 'object_ref',$4::text,'digest',$5::text,'media_type',$6::text,'byte_length',$7::bigint)::text,'UTF8'),'sha256'),'hex'),
             encode(digest(convert_to($4::text,'UTF8'),'sha256'),'hex'),
             'knowledge_document',jsonb_build_object('document_id',$3::uuid),1,$5)",
        ).bind(Uuid::new_v4()).bind(idempotency_key).bind(document_id)
            .bind(&reference).bind(&digest).bind(media_type).bind(length)
            .execute(conn).await?;
    }
    Ok(reference)
}

pub async fn register_knowledge_image_object_in_tx(
    conn: &mut PgConnection,
    image_artifact_revision_id: Uuid,
    object_ref: &str,
    digest: &str,
    media_type: &str,
    byte_length: i64,
) -> Result<(), sqlx::Error> {
    if !matches!(media_type, "image/png" | "image/jpeg" | "image/webp") || byte_length <= 0 {
        return Err(sqlx::Error::Protocol(
            "knowledge image object metadata invalid".into(),
        ));
    }
    add_object_reference(
        conn,
        object_ref,
        digest,
        media_type,
        byte_length,
        "knowledge_image_artifact",
        image_artifact_revision_id,
        "source-media",
    )
    .await
}

pub async fn release_knowledge_document_object(
    pool: &PgPool,
    document_id: Uuid,
    idempotency_key: &str,
) -> Result<Option<ObjectDeletionIdentity>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let result =
        release_knowledge_document_object_in_tx(&mut tx, document_id, idempotency_key).await?;
    tx.commit().await?;
    Ok(result)
}

pub async fn release_knowledge_document_object_in_tx(
    conn: &mut PgConnection,
    document_id: Uuid,
    idempotency_key: &str,
) -> Result<Option<ObjectDeletionIdentity>, sqlx::Error> {
    let (reference, digest, deleted): (String, String, bool) = sqlx::query_as(
        "SELECT object_ref::text,content_sha256::text AS file_hash,deleted_at IS NOT NULL
         FROM knowledge_documents WHERE id=$1 FOR UPDATE",
    )
    .bind(document_id)
    .fetch_one(&mut *conn)
    .await?;
    let owned: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM platform_object_references
         WHERE owner_kind='knowledge_document' AND owner_id=$1 AND occurrence='original')",
    )
    .bind(document_id)
    .fetch_one(&mut *conn)
    .await?;
    if deleted && !owned {
        return Ok(None);
    }
    let audit_id = Uuid::new_v4();
    let deletion = release_object_reference(
        conn,
        &reference,
        "knowledge_document",
        document_id,
        "original",
        audit_id,
    )
    .await?;
    sqlx::query("UPDATE knowledge_documents SET deleted_at=COALESCE(deleted_at,clock_timestamp()),updated_at=clock_timestamp() WHERE id=$1")
        .bind(document_id).execute(&mut *conn).await?;
    let response =
        serde_json::to_value(&deletion).map_err(|error| sqlx::Error::Encode(Box::new(error)))?;
    sqlx::query(
        "INSERT INTO platform_audit_events(id,schema_version,operation,idempotency_key,
         request_sha256,response_sha256,entity_kind,entity_locator,before_revision,before_sha256)
         VALUES($1,1,'knowledge.document.object.release',$2,
         encode(digest(convert_to(jsonb_build_object('schema_version',1,'document_id',$3::uuid,'object_ref',$4::text)::text,'UTF8'),'sha256'),'hex'),
         encode(digest(convert_to($5::jsonb::text,'UTF8'),'sha256'),'hex'),
         'knowledge_document',jsonb_build_object('document_id',$3::uuid),1,$6)",
    ).bind(audit_id).bind(idempotency_key).bind(document_id).bind(&reference)
        .bind(response).bind(digest).execute(&mut *conn).await?;
    Ok(deletion)
}
