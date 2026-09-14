//! SQL persistence split from persist.rs (behavior unchanged).
use super::version::parse_kind;
use crate::{PersistError, retire_workspace};
use sqlx::{PgPool, Row};
use uuid::Uuid;

pub async fn list_products_in_workspace(
    pool: &PgPool,
    workspace_id: Uuid,
) -> Result<Vec<crate::Product>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, kind, name, slug, current_version_id FROM products WHERE workspace_id = $1",
    )
    .bind(workspace_id)
    .fetch_all(pool)
    .await?;
    let mut out = Vec::new();
    for p in rows {
        let id: Uuid = p.try_get("id")?;
        let kind: String = p.try_get("kind")?;
        out.push(crate::Product {
            id,
            workspace_id,
            kind: parse_kind(&kind),
            name: p.try_get("name")?,
            slug: p.try_get("slug")?,
            current_version_id: p.try_get("current_version_id")?,
            embedding_model_id: String::new(),
        });
    }
    Ok(out)
}

pub async fn load_product(
    pool: &PgPool,
    product_id: Uuid,
) -> Result<Option<crate::Product>, sqlx::Error> {
    let p = sqlx::query(
        "SELECT id, workspace_id, kind, name, slug, current_version_id FROM products WHERE id = $1",
    )
    .bind(product_id)
    .fetch_optional(pool)
    .await?;
    let Some(p) = p else {
        return Ok(None);
    };
    let kind: String = p.try_get("kind")?;
    Ok(Some(crate::Product {
        id: p.try_get("id")?,
        workspace_id: p.try_get("workspace_id")?,
        kind: parse_kind(&kind),
        name: p.try_get("name")?,
        slug: p.try_get("slug")?,
        current_version_id: p.try_get("current_version_id")?,
        embedding_model_id: String::new(),
    }))
}

pub async fn product_slug_taken(
    pool: &PgPool,
    workspace_id: Uuid,
    slug: &str,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM products WHERE workspace_id = $1 AND slug = $2)",
    )
    .bind(workspace_id)
    .bind(slug)
    .fetch_one(pool)
    .await
}

pub async fn clear_product_current_if(
    pool: &PgPool,
    product_id: Uuid,
    version_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE products SET current_version_id = NULL WHERE id = $1 AND current_version_id = $2",
    )
    .bind(product_id)
    .bind(version_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn insert_product(
    pool: &PgPool,
    id: Uuid,
    workspace_id: Uuid,
    kind: &str,
    name: &str,
    slug: &str,
    current_version_id: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO products (id, workspace_id, kind, name, slug, current_version_id)
         VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(id)
    .bind(workspace_id)
    .bind(kind)
    .bind(name)
    .bind(slug)
    .bind(current_version_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn set_product_current(
    pool: &PgPool,
    product_id: Uuid,
    version_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE products SET current_version_id = $2 WHERE id = $1")
        .bind(product_id)
        .bind(version_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn product_workspace_id(
    pool: &PgPool,
    product_id: Uuid,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar("SELECT workspace_id FROM products WHERE id = $1")
        .bind(product_id)
        .fetch_optional(pool)
        .await
}

pub async fn delete_product(pool: &PgPool, product_id: Uuid) -> Result<(), PersistError> {
    let row = sqlx::query("SELECT kind, slug FROM products WHERE id = $1")
        .bind(product_id)
        .fetch_optional(pool)
        .await?;
    let Some(row) = row else {
        return Err(PersistError::NotFound);
    };
    let kind: String = row.try_get("kind")?;
    let slug: String = row.try_get("slug")?;
    if kind == "library" && slug == "library" {
        return Err(PersistError::DefaultLibrary);
    }
    sqlx::query("DELETE FROM products WHERE id = $1")
        .bind(product_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn update_product_name(
    pool: &PgPool,
    product_id: Uuid,
    name: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE products SET name = $2 WHERE id = $1")
        .bind(product_id)
        .bind(name)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete_empty_product(pool: &PgPool, product_id: Uuid) -> Result<bool, sqlx::Error> {
    let left: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM product_versions WHERE product_id = $1 AND deleted_at IS NULL",
    )
    .bind(product_id)
    .fetch_one(pool)
    .await?;
    if left > 0 {
        return Ok(false);
    }
    sqlx::query("UPDATE products SET current_version_id = NULL WHERE id = $1")
        .bind(product_id)
        .execute(pool)
        .await?;
    sqlx::query(
        "DELETE FROM documents WHERE product_version_id IN (
            SELECT id FROM product_versions WHERE product_id = $1 AND deleted_at IS NOT NULL
         )",
    )
    .bind(product_id)
    .execute(pool)
    .await?;
    sqlx::query("DELETE FROM product_versions WHERE product_id = $1 AND deleted_at IS NOT NULL")
        .bind(product_id)
        .execute(pool)
        .await?;
    let row = sqlx::query("SELECT kind, slug, workspace_id FROM products WHERE id = $1")
        .bind(product_id)
        .fetch_optional(pool)
        .await?;
    let Some(row) = row else {
        return Ok(false);
    };
    let kind: String = row.try_get("kind")?;
    let slug: String = row.try_get("slug")?;
    let ws: Uuid = row.try_get("workspace_id")?;
    if kind == "library" && slug == "library" {
        return Ok(false);
    }
    sqlx::query("DELETE FROM products WHERE id = $1")
        .bind(product_id)
        .execute(pool)
        .await?;
    let products: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM products WHERE workspace_id = $1")
        .bind(ws)
        .fetch_one(pool)
        .await?;
    if products == 0 {
        retire_workspace(pool, ws).await?;
    }
    Ok(true)
}

pub async fn product_name(pool: &PgPool, product_id: Uuid) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar("SELECT name FROM products WHERE id = $1")
        .bind(product_id)
        .fetch_optional(pool)
        .await
}
