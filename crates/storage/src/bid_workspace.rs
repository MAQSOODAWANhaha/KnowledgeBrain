//! V2 workspace publication seams. Runtime callers only execute SECURITY DEFINER functions.

use serde_json::Value;
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ProjectV2 {
    pub id: Uuid,
    pub title: String,
    pub status: String,
    pub ended_at: Option<chrono::DateTime<chrono::Utc>>,
    pub workspace_id: Uuid,
    pub owner_user_id: Uuid,
}

pub async fn list_projects_v2(
    pool: &PgPool,
    owner_user_id: Uuid,
) -> Result<Vec<ProjectV2>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT p.id, p.title, p.status, p.ended_at, p.owner_user_id, h.workspace_id
           FROM bidding_v2_projects p
           JOIN bidding_v2_workspace_heads h ON h.project_id=p.id
          WHERE p.owner_user_id=$1
          ORDER BY p.created_at DESC, p.id",
    )
    .bind(owner_user_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|row| ProjectV2 {
            id: row.get("id"),
            title: row.get("title"),
            status: row.get("status"),
            ended_at: row.get("ended_at"),
            workspace_id: row.get("workspace_id"),
            owner_user_id: row.get("owner_user_id"),
        })
        .collect())
}

pub async fn create_project_v2(
    pool: &PgPool,
    id: Uuid,
    title: &str,
    owner_user_id: Uuid,
    actor: &str,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_create_project_v2($1,$2,$3,$4::kb_actor_identity)")
        .bind(id)
        .bind(title)
        .bind(owner_user_id)
        .bind(actor)
        .fetch_one(pool)
        .await
}

pub async fn get_project_v2(pool: &PgPool, id: Uuid) -> Result<Option<ProjectV2>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT p.id, p.title, p.status, p.ended_at, p.owner_user_id, h.workspace_id
           FROM bidding_v2_projects p
           JOIN bidding_v2_workspace_heads h ON h.project_id=p.id
          WHERE p.id=$1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|row| ProjectV2 {
        id: row.get("id"),
        title: row.get("title"),
        status: row.get("status"),
        ended_at: row.get("ended_at"),
        workspace_id: row.get("workspace_id"),
        owner_user_id: row.get("owner_user_id"),
    }))
}

pub async fn load_workspace_v2(
    pool: &PgPool,
    workspace_id: Uuid,
) -> Result<Option<Value>, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_load_workspace($1)")
        .bind(workspace_id)
        .fetch_optional(pool)
        .await
}

pub async fn commit_workspace_mutation_v2(
    pool: &PgPool,
    workspace_id: Uuid,
    expected_revision_id: Uuid,
    expected_sha256: &str,
    snapshot: &Value,
    actor: &str,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT kb_bid_v2_commit_workspace_mutation($1,$2,$3::kb_sha256,$4,$5::kb_actor_identity)",
    )
    .bind(workspace_id)
    .bind(expected_revision_id)
    .bind(expected_sha256)
    .bind(snapshot)
    .bind(actor)
    .fetch_one(pool)
    .await
}

pub async fn workspace_owner(
    pool: &PgPool,
    workspace_id: Uuid,
) -> Result<Option<(Uuid, Uuid)>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT p.id project_id, p.owner_user_id
           FROM bidding_v2_workspace_heads h
           JOIN bidding_v2_projects p ON p.id=h.project_id
          WHERE h.workspace_id=$1",
    )
    .bind(workspace_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|row| (row.get("project_id"), row.get("owner_user_id"))))
}
