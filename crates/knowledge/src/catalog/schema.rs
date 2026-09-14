//! SQL persistence split from persist.rs (behavior unchanged).
use sqlx::{PgPool, Row};
use uuid::Uuid;

pub fn is_unique_violation(err: &sqlx::Error) -> bool {
    err.as_database_error()
        .and_then(|e| e.code())
        .is_some_and(|c| c == "23505")
}

pub async fn embedding_models_for_versions(
    pool: &PgPool,
    version_ids: &[Uuid],
) -> Result<Vec<(Uuid, String)>, sqlx::Error> {
    if version_ids.is_empty() {
        return Ok(Vec::new());
    }
    let rows = sqlx::query(
        "SELECT id, COALESCE(embedding_model_id, '') AS emb
         FROM product_versions WHERE id = ANY($1)",
    )
    .bind(version_ids)
    .fetch_all(pool)
    .await?;
    let mut out = Vec::new();
    for r in rows {
        out.push((r.try_get("id")?, r.try_get("emb")?));
    }
    Ok(out)
}

pub async fn table_names(pool: &PgPool) -> Result<Vec<String>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT tablename FROM pg_tables WHERE schemaname = 'public' ORDER BY tablename",
    )
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|r| r.try_get::<String, _>("tablename"))
        .collect()
}

pub async fn column_names(pool: &PgPool, table: &str) -> Result<Vec<String>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT column_name FROM information_schema.columns
         WHERE table_schema = 'public' AND table_name = $1",
    )
    .bind(table)
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|r| r.try_get::<String, _>("column_name"))
        .collect()
}
