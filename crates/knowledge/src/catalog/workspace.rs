//! SQL persistence split from persist.rs (behavior unchanged).
use sqlx::{PgPool, Row};
use uuid::Uuid;

pub struct SeededWorkspace {
    pub workspace_id: Uuid,
    pub library_id: Uuid,
    pub library_version_id: Uuid,
}

pub async fn create_workspace_with_library(
    pool: &PgPool,
    owner_id: Uuid,
    name: &str,
    slug: &str,
) -> Result<SeededWorkspace, sqlx::Error> {
    let workspace_id = Uuid::new_v4();
    let library_id = Uuid::new_v4();
    let library_version_id = Uuid::new_v4();
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO knowledge_workspaces (id, name, slug, kind, owner_user_id) VALUES ($1, $2, $3, 'product_line', $4)",
    )
    .bind(workspace_id)
    .bind(name)
    .bind(slug)
    .bind(owner_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO knowledge_products (id, workspace_id, kind, name, slug, current_version_id)
         VALUES ($1, $2, 'library', '公司资料', 'library', NULL)",
    )
    .bind(library_id)
    .bind(workspace_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO knowledge_product_versions (id, product_id, label, status)
         VALUES ($1, $2, 'current', 'active')",
    )
    .bind(library_version_id)
    .bind(library_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE knowledge_products SET current_version_id=$2 WHERE id=$1")
        .bind(library_id)
        .bind(library_version_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(SeededWorkspace {
        workspace_id,
        library_id,
        library_version_id,
    })
}

pub async fn insert_workspace(
    pool: &PgPool,
    id: Uuid,
    owner_user_id: Uuid,
    name: &str,
    slug: &str,
) -> Result<(), sqlx::Error> {
    insert_workspace_kind(pool, id, owner_user_id, name, slug, "product_line").await
}

pub async fn insert_workspace_kind(
    pool: &PgPool,
    id: Uuid,
    owner_user_id: Uuid,
    name: &str,
    slug: &str,
    kind: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO knowledge_workspaces (id, name, slug, kind, owner_user_id) VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(id)
    .bind(name)
    .bind(slug)
    .bind(kind)
    .bind(owner_user_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn ensure_company_workspace(
    pool: &PgPool,
    owner_user_id: Uuid,
) -> Result<Uuid, sqlx::Error> {
    if let Some(id) = company_workspace_id(pool, owner_user_id).await? {
        return Ok(id);
    }
    let id = Uuid::new_v4();
    sqlx::query_scalar(
        "INSERT INTO knowledge_workspaces (id, name, slug, kind, owner_user_id)
         VALUES ($1, '公司资料', $3, 'company', $2)
         ON CONFLICT (owner_user_id) WHERE kind = 'company' AND deleted_at IS NULL
         DO UPDATE SET owner_user_id = EXCLUDED.owner_user_id RETURNING id",
    )
    .bind(id)
    .bind(owner_user_id)
    .bind(format!("company-{id}"))
    .fetch_one(pool)
    .await
}

pub async fn list_workspace_ids(pool: &PgPool) -> Result<Vec<Uuid>, sqlx::Error> {
    sqlx::query_scalar("SELECT id FROM knowledge_workspaces ORDER BY created_at")
        .fetch_all(pool)
        .await
}

pub(crate) fn workspace_from_row(
    r: sqlx::postgres::PgRow,
) -> Result<crate::Workspace, sqlx::Error> {
    let id: Uuid = r.try_get("id")?;
    let retrieval: crate::RetrievalConfig = r
        .try_get::<serde_json::Value, _>("retrieval_config")
        .ok()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    let kind_s: String = r.try_get("kind").unwrap_or_else(|_| "product_line".into());
    Ok(crate::Workspace {
        id,
        owner_user_id: r.try_get("owner_user_id")?,
        name: r.try_get("name")?,
        slug: r.try_get("slug")?,
        kind: crate::WorkspaceKind::parse(&kind_s),
        retrieval,
    })
}

pub async fn list_workspaces(
    pool: &PgPool,
    owner_user_id: Uuid,
) -> Result<Vec<crate::Workspace>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, owner_user_id, name, slug, retrieval_config,
                COALESCE(kind, 'product_line') AS kind
         FROM knowledge_workspaces WHERE owner_user_id = $1 AND deleted_at IS NULL ORDER BY created_at",
    )
    .bind(owner_user_id)
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(workspace_from_row).collect()
}

pub async fn load_workspace(
    pool: &PgPool,
    workspace_id: Uuid,
) -> Result<Option<crate::Workspace>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id, owner_user_id, name, slug, retrieval_config,
                COALESCE(kind, 'product_line') AS kind
         FROM knowledge_workspaces WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(workspace_id)
    .fetch_optional(pool)
    .await?;
    row.map(workspace_from_row).transpose()
}

pub async fn workspace_embedding_conflict(
    pool: &PgPool,
    workspace_id: Uuid,
    incoming: &str,
) -> Result<Option<String>, sqlx::Error> {
    if incoming.is_empty() {
        return Ok(None);
    }
    let existing: Option<String> = sqlx::query_scalar(
        "SELECT pv.embedding_model_id
         FROM knowledge_products p
         JOIN knowledge_product_versions pv ON pv.id = p.current_version_id
         WHERE p.workspace_id = $1 AND p.kind = 'product'
           AND COALESCE(pv.embedding_model_id, '') <> ''
         LIMIT 1",
    )
    .bind(workspace_id)
    .fetch_optional(pool)
    .await?;
    Ok(existing.filter(|have| have != incoming).map(|have| {
        format!("workspace products must share embedding_model_id (have {have}, got {incoming})")
    }))
}

pub async fn company_workspace_id(
    pool: &PgPool,
    owner_user_id: Uuid,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar("SELECT id FROM knowledge_workspaces WHERE kind = 'company' AND owner_user_id = $1 AND deleted_at IS NULL LIMIT 1")
        .bind(owner_user_id)
        .fetch_optional(pool)
        .await
}

pub async fn is_frozen_default_library(
    pool: &PgPool,
    product_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let row = sqlx::query(
        "SELECT p.kind, p.slug, w.kind AS ws_kind
         FROM knowledge_products p JOIN knowledge_workspaces w ON w.id = p.workspace_id
         WHERE p.id = $1",
    )
    .bind(product_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.is_some_and(|r| {
        r.get::<String, _>("kind") == "library"
            && r.get::<String, _>("slug") == "library"
            && r.get::<String, _>("ws_kind") == "product_line"
    }))
}

pub async fn workspaces_for_user(pool: &PgPool, user_id: Uuid) -> Result<Vec<Uuid>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT id FROM knowledge_workspaces WHERE owner_user_id = $1 AND deleted_at IS NULL",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
}

pub async fn update_workspace_name(
    pool: &PgPool,
    workspace_id: Uuid,
    name: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE knowledge_workspaces SET name = $2, updated_at = now() WHERE id = $1")
        .bind(workspace_id)
        .bind(name)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn retire_workspace(pool: &PgPool, workspace_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE knowledge_workspaces SET slug = $2, name = $3, deleted_at = now(), updated_at = now() WHERE id = $1")
        .bind(workspace_id)
        .bind(format!("__deleted_{workspace_id}"))
        .bind(format!("deleted-{workspace_id}"))
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn workspace_thresholds_for_product(
    pool: &PgPool,
    product_id: Uuid,
) -> Result<(f64, f64), sqlx::Error> {
    let cfg: Option<serde_json::Value> = sqlx::query_scalar(
        "SELECT w.retrieval_config FROM knowledge_workspaces w
         JOIN knowledge_products p ON p.workspace_id = w.id WHERE p.id = $1",
    )
    .bind(product_id)
    .fetch_optional(pool)
    .await?;
    Ok(thresholds_from_cfg(cfg.as_ref()))
}

pub async fn workspace_thresholds_for_version(
    pool: &PgPool,
    version_id: Uuid,
) -> Result<(f64, f64), sqlx::Error> {
    let cfg: Option<serde_json::Value> = sqlx::query_scalar(
        "SELECT w.retrieval_config FROM knowledge_workspaces w
         JOIN knowledge_products p ON p.workspace_id = w.id
         JOIN knowledge_product_versions pv ON pv.product_id = p.id
         WHERE pv.id = $1",
    )
    .bind(version_id)
    .fetch_optional(pool)
    .await?;
    Ok(thresholds_from_cfg(cfg.as_ref()))
}

pub async fn set_retrieval_config(
    pool: &PgPool,
    workspace_id: Uuid,
    vector_threshold: f64,
    keyword_threshold: f64,
    embedding_top_k: i32,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE knowledge_workspaces SET retrieval_config = $2::jsonb WHERE id = $1")
        .bind(workspace_id)
        .bind(serde_json::json!({
            "vector_threshold": vector_threshold,
            "keyword_threshold": keyword_threshold,
            "embedding_top_k": embedding_top_k,
        }))
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) fn thresholds_from_cfg(cfg: Option<&serde_json::Value>) -> (f64, f64) {
    let vth = cfg
        .and_then(|c| c.get("vector_threshold"))
        .and_then(|x| x.as_f64())
        .unwrap_or(0.15);
    let kth = cfg
        .and_then(|c| c.get("keyword_threshold"))
        .and_then(|x| x.as_f64())
        .unwrap_or(0.3);
    (vth, kth)
}

pub async fn workspace_top_k_for_version(
    pool: &PgPool,
    version_id: Uuid,
) -> Result<usize, sqlx::Error> {
    let cfg: Option<serde_json::Value> = sqlx::query_scalar(
        "SELECT w.retrieval_config FROM knowledge_workspaces w
         JOIN knowledge_products p ON p.workspace_id = w.id
         JOIN knowledge_product_versions pv ON pv.product_id = p.id
         WHERE pv.id = $1",
    )
    .bind(version_id)
    .fetch_optional(pool)
    .await?;
    let n = cfg
        .as_ref()
        .and_then(|c| c.get("embedding_top_k"))
        .and_then(|x| x.as_u64())
        .unwrap_or(50) as usize;
    Ok(n.max(1))
}
