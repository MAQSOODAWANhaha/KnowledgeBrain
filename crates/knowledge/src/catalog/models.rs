//! SQL persistence split from persist.rs (behavior unchanged).
use sqlx::{PgPool, Row};

pub async fn list_models(pool: &PgPool) -> Result<Vec<(String, String, i32)>, sqlx::Error> {
    let rows = sqlx::query("SELECT id, kind, COALESCE(dimension, 0) FROM models ORDER BY id")
        .fetch_all(pool)
        .await?;
    let mut out = Vec::new();
    for r in rows {
        out.push((r.try_get(0)?, r.try_get(1)?, r.try_get(2)?));
    }
    Ok(out)
}

pub async fn upsert_model(
    pool: &PgPool,
    id: &str,
    kind: &str,
    dimension: i32,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO models (id, kind, dimension) VALUES ($1,$2,$3)
         ON CONFLICT (id) DO UPDATE SET kind = EXCLUDED.kind, dimension = EXCLUDED.dimension",
    )
    .bind(id)
    .bind(kind)
    .bind(dimension)
    .execute(pool)
    .await?;
    Ok(())
}
