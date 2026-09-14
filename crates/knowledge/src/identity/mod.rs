//! SQL persistence split from persist.rs (behavior unchanged).
use sqlx::{PgPool, Row};
use uuid::Uuid;

pub async fn list_members_for_workspace(
    pool: &PgPool,
    workspace_id: Uuid,
) -> Result<Vec<(Uuid, String)>, sqlx::Error> {
    sqlx::query_as("SELECT user_id, role FROM workspace_members WHERE workspace_id = $1")
        .bind(workspace_id)
        .fetch_all(pool)
        .await
}

pub async fn insert_member(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
    role: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, $3)
         ON CONFLICT DO NOTHING",
    )
    .bind(workspace_id)
    .bind(user_id)
    .bind(role)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn insert_user(
    pool: &PgPool,
    id: Uuid,
    email: &str,
    password_hash: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO users (id, email, password_hash) VALUES ($1, $2, $3)
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(id)
    .bind(email)
    .bind(password_hash)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn upsert_member(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
    role: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, $3)
         ON CONFLICT (workspace_id, user_id) DO UPDATE SET role = EXCLUDED.role",
    )
    .bind(workspace_id)
    .bind(user_id)
    .bind(role)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete_member(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM workspace_members WHERE workspace_id = $1 AND user_id = $2")
        .bind(workspace_id)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn update_user_email(
    pool: &PgPool,
    user_id: Uuid,
    email: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE users SET email = $2, updated_at = now() WHERE id = $1")
        .bind(user_id)
        .bind(email)
        .execute(pool)
        .await?;
    Ok(())
}

pub struct NewApiKey<'a> {
    pub id: Uuid,
    pub name: &'a str,
    pub key_hash: &'a str,
    pub prefix: &'a str,
    pub scope_type: &'a str,
    pub scope_id: Uuid,
    pub scopes: &'a [String],
}

pub async fn insert_api_key(pool: &PgPool, key: NewApiKey<'_>) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO api_keys (id, name, key_hash, prefix, scope_type, scope_id, scopes)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(key.id)
    .bind(key.name)
    .bind(key.key_hash)
    .bind(key.prefix)
    .bind(key.scope_type)
    .bind(key.scope_id)
    .bind(key.scopes)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete_api_key(pool: &PgPool, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM api_keys WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn find_api_key_by_hash(
    pool: &PgPool,
    key_hash: &str,
) -> Result<Option<crate::ApiKey>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id, name, key_hash, prefix, scope_type, scope_id, scopes FROM api_keys WHERE key_hash = $1",
    )
    .bind(key_hash)
    .fetch_optional(pool)
    .await?;
    let Some(r) = row else {
        return Ok(None);
    };
    Ok(Some(crate::ApiKey {
        id: r.try_get("id")?,
        name: r.try_get("name")?,
        key_hash: r.try_get("key_hash")?,
        prefix: r.try_get("prefix")?,
        scope_type: r.try_get("scope_type")?,
        scope_id: r.try_get("scope_id")?,
        scopes: r.try_get("scopes")?,
    }))
}

pub async fn find_user_by_id(
    pool: &PgPool,
    id: Uuid,
) -> Result<Option<(Uuid, String)>, sqlx::Error> {
    let row = sqlx::query("SELECT id, email FROM users WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    let Some(r) = row else {
        return Ok(None);
    };
    Ok(Some((r.try_get("id")?, r.try_get("email")?)))
}

pub async fn find_user_by_email(
    pool: &PgPool,
    email: &str,
) -> Result<Option<(Uuid, String, String)>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id, email, COALESCE(password_hash, '') AS password_hash FROM users WHERE email = $1",
    )
    .bind(email)
    .fetch_optional(pool)
    .await?;
    let Some(r) = row else {
        return Ok(None);
    };
    Ok(Some((
        r.try_get("id")?,
        r.try_get("email")?,
        r.try_get("password_hash")?,
    )))
}
