//! SQL persistence split from persist.rs (behavior unchanged).
use sqlx::{PgPool, Row};
use uuid::Uuid;

pub async fn insert_user(
    pool: &PgPool,
    id: Uuid,
    email: &str,
    password_hash: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO platform_users (id, email, password_hash) VALUES ($1, $2, $3)
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(id)
    .bind(email)
    .bind(password_hash)
    .execute(pool)
    .await?;
    Ok(())
}

pub struct NewApiKey<'a> {
    pub id: Uuid,
    pub name: &'a str,
    pub key_hash: &'a str,
    pub prefix: &'a str,
    pub owner_user_id: Uuid,
}

pub async fn insert_api_key(pool: &PgPool, key: NewApiKey<'_>) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO platform_api_keys (id, name, key_hash, prefix, owner_user_id)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(key.id)
    .bind(key.name)
    .bind(key.key_hash)
    .bind(key.prefix)
    .bind(key.owner_user_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete_api_key(pool: &PgPool, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM platform_api_keys WHERE id = $1")
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
        "SELECT id, name, key_hash, prefix, owner_user_id FROM platform_api_keys WHERE key_hash = $1",
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
        owner_user_id: r.try_get("owner_user_id")?,
    }))
}

pub async fn find_user_by_id(
    pool: &PgPool,
    id: Uuid,
) -> Result<Option<(Uuid, String)>, sqlx::Error> {
    let row = sqlx::query("SELECT id, email FROM platform_users WHERE id = $1")
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
        "SELECT id, email, COALESCE(password_hash, '') AS password_hash FROM platform_users WHERE email = $1",
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
