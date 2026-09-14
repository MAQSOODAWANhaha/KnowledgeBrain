//! SQL persistence split from persist.rs (behavior unchanged).
use sqlx::PgPool;
use uuid::Uuid;

pub async fn insert_tag(
    pool: &PgPool,
    id: Uuid,
    workspace_id: Uuid,
    name: &str,
    slug: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO tags (id, workspace_id, name, slug) VALUES ($1,$2,$3,$4)
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(id)
    .bind(workspace_id)
    .bind(name)
    .bind(slug)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn insert_document_tags(
    pool: &PgPool,
    document_id: Uuid,
    tag_ids: &[Uuid],
) -> Result<(), sqlx::Error> {
    for tag_id in tag_ids {
        sqlx::query(
            "INSERT INTO document_tags (document_id, tag_id) VALUES ($1,$2)
             ON CONFLICT DO NOTHING",
        )
        .bind(document_id)
        .bind(tag_id)
        .execute(pool)
        .await?;
    }
    Ok(())
}

pub async fn replace_document_tags(
    pool: &PgPool,
    document_id: Uuid,
    tag_ids: &[Uuid],
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM document_tags WHERE document_id = $1")
        .bind(document_id)
        .execute(pool)
        .await?;
    insert_document_tags(pool, document_id, tag_ids).await
}

pub async fn document_tag_ids(pool: &PgPool, document_id: Uuid) -> Result<Vec<Uuid>, sqlx::Error> {
    sqlx::query_scalar("SELECT tag_id FROM document_tags WHERE document_id=$1 ORDER BY tag_id")
        .bind(document_id)
        .fetch_all(pool)
        .await
}

pub async fn delete_tag(pool: &PgPool, tag_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM document_tags WHERE tag_id = $1")
        .bind(tag_id)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM tags WHERE id = $1")
        .bind(tag_id)
        .execute(pool)
        .await?;
    Ok(())
}
