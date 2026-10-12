use sqlx::{PgConnection, PgPool};
use thiserror::Error;

pub const KNOWLEDGE_BASE_BASELINE: &str =
    include_str!("../../../migrations/knowledge_base_baseline.sql");
pub const SHARED_PLATFORM_BASELINE: &str =
    include_str!("../../../migrations/shared_platform_baseline.sql");
pub const BIDDING_BASELINE: &str = include_str!("../../../migrations/bidding_v2_baseline.sql");

const BOOTSTRAP_LOCK_ID: i64 = 0x4b_42_53_43_48_45_4d_41;
#[derive(Debug, Error)]
pub enum SchemaError {
    #[error("postgres operation failed")]
    Sql(#[from] sqlx::Error),
    #[error(
        "application database identity must be a non-superuser login without administrative, inherited role, CREATE or TEMPORARY privileges"
    )]
    UnsafeApplicationRole,
    #[error("fresh initialization refused a partially populated application schema")]
    PartialSchema,
}

#[derive(Debug, Error)]
#[error("{0}")]
pub struct DatabaseConfigError(&'static str);

/// Parse one source of database configuration without leaking credentials.
pub fn database_url() -> Result<String, DatabaseConfigError> {
    database_url_from(|name| std::env::var(name).ok())
}

fn database_url_from(get: impl Fn(&str) -> Option<String>) -> Result<String, DatabaseConfigError> {
    use sqlx::ConnectOptions;
    use std::str::FromStr;
    let supplied = |name: &str| get(name).filter(|v| !v.trim().is_empty());
    let user = supplied("POSTGRES_USER");
    let password = supplied("POSTGRES_PASSWORD");
    let host = supplied("POSTGRES_HOST");
    let port = supplied("POSTGRES_PORT")
        .map(|v| {
            v.trim()
                .parse::<u16>()
                .ok()
                .filter(|v| *v > 0)
                .ok_or(DatabaseConfigError("POSTGRES_PORT must be a positive port"))
        })
        .transpose()?;
    let database = supplied("POSTGRES_DB");
    if let Some(url) = supplied("DATABASE_URL") {
        let parsed = sqlx::postgres::PgConnectOptions::from_str(&url)
            .map_err(|_| DatabaseConfigError("DATABASE_URL is invalid"))?;
        if user
            .as_ref()
            .is_some_and(|v| v.trim() != parsed.get_username())
            || password.as_ref().is_some_and(|v| {
                sqlx::postgres::PgConnectOptions::new()
                    .username("application")
                    .password(v)
                    .to_url_lossy()
                    .password()
                    != parsed
                        .clone()
                        .username("application")
                        .to_url_lossy()
                        .password()
            })
            || host.as_ref().is_some_and(|v| v.trim() != parsed.get_host())
            || port.is_some_and(|v| v != parsed.get_port())
            || database
                .as_ref()
                .is_some_and(|v| Some(v.trim()) != parsed.get_database())
        {
            return Err(DatabaseConfigError(
                "DATABASE_URL conflicts with POSTGRES_* configuration",
            ));
        }
        return Ok(url);
    }
    let user = user.ok_or(DatabaseConfigError(
        "POSTGRES_USER is required without DATABASE_URL",
    ))?;
    let password = password.ok_or(DatabaseConfigError(
        "POSTGRES_PASSWORD is required without DATABASE_URL",
    ))?;
    // SQLx's URL builder interpolates usernames before escaping; assemble with
    // URL setters instead, preserving arbitrary identifier/password bytes.
    let encode = |value: &str| -> String {
        use std::fmt::Write;
        let mut encoded = String::new();
        for byte in value.bytes() {
            if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
                encoded.push(char::from(byte));
            } else {
                write!(&mut encoded, "%{byte:02X}").expect("writing to String");
            }
        }
        encoded
    };
    let mut url = reqwest::Url::parse("postgresql://localhost").expect("static URL");
    url.set_host(Some(host.as_deref().map(str::trim).unwrap_or("127.0.0.1")))
        .map_err(|_| DatabaseConfigError("POSTGRES_HOST is invalid"))?;
    url.set_port(Some(port.unwrap_or(5432)))
        .map_err(|_| DatabaseConfigError("POSTGRES_PORT is invalid"))?;
    url.set_username(&encode(user.trim()))
        .map_err(|_| DatabaseConfigError("POSTGRES_USER is invalid"))?;
    url.set_password(Some(&encode(&password)))
        .map_err(|_| DatabaseConfigError("POSTGRES_PASSWORD is invalid"))?;
    url.set_path(&encode(
        database
            .as_deref()
            .map(str::trim)
            .unwrap_or("knowledgebrain"),
    ));
    Ok(url.to_string())
}

static POOL: tokio::sync::OnceCell<PgPool> = tokio::sync::OnceCell::const_new();

async fn open_pool() -> Result<PgPool, sqlx::Error> {
    let database_url =
        database_url().map_err(|error| sqlx::Error::Configuration(Box::new(error)))?;
    sqlx::postgres::PgPoolOptions::new()
        .max_connections(16)
        .acquire_timeout(std::time::Duration::from_secs(10))
        .connect(&database_url)
        .await
}

/// Shared application pool; does not inspect or alter application schema.
pub async fn connect() -> Result<PgPool, sqlx::Error> {
    POOL.get_or_try_init(open_pool).await.cloned()
}

pub async fn verify_application_role(pool: &PgPool) -> Result<(), SchemaError> {
    let safe: bool = sqlx::query_scalar(
        "SELECT NOT rolsuper AND NOT rolcreatedb AND NOT rolcreaterole AND NOT rolreplication
           AND NOT rolbypassrls AND rolcanlogin AND current_user=session_user
           AND NOT has_database_privilege(current_user,current_database(),'CREATE,TEMPORARY')
           AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_auth_members WHERE member=pg_roles.oid)
         FROM pg_catalog.pg_roles WHERE rolname=current_user",
    )
    .fetch_one(pool)
    .await?;
    if !safe {
        return Err(SchemaError::UnsafeApplicationRole);
    }
    Ok(())
}

async fn application_object_count(connection: &mut PgConnection) -> Result<i64, sqlx::Error> {
    // Reject a partially populated application schema regardless of the owner.
    // Extension members include dependent array/row types and indexes, not just
    // objects with a direct extension-membership dependency.
    sqlx::query_scalar(
        "WITH RECURSIVE extension_objects(classid,objid) AS (
           SELECT classid,objid FROM pg_catalog.pg_depend WHERE deptype='e'
           UNION
           SELECT d.classid,d.objid FROM pg_catalog.pg_depend d
           JOIN extension_objects e ON e.classid=d.refclassid AND e.objid=d.refobjid
           WHERE d.deptype IN ('i','a')
         ), application_objects AS (
           SELECT 'pg_catalog.pg_class'::regclass AS classid,c.oid AS objid
           FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace
           WHERE n.nspname='public'
           UNION ALL
           SELECT 'pg_catalog.pg_proc'::regclass,p.oid
           FROM pg_catalog.pg_proc p JOIN pg_catalog.pg_namespace n ON n.oid=p.pronamespace
           WHERE n.nspname='public'
           UNION ALL
           SELECT 'pg_catalog.pg_type'::regclass,t.oid
           FROM pg_catalog.pg_type t JOIN pg_catalog.pg_namespace n ON n.oid=t.typnamespace
           WHERE n.nspname='public'
         ) SELECT count(*) FROM application_objects a
         WHERE NOT EXISTS (SELECT 1 FROM extension_objects e WHERE e.classid=a.classid AND e.objid=a.objid)",
    ).fetch_one(connection).await
}

/// Initialize a new application database atomically. Existing users means the
/// database is initialized; this is deliberately not a compatibility check or migration.
pub async fn apply_fresh_baseline(pool: &PgPool) -> Result<(), SchemaError> {
    verify_application_role(pool).await?;
    let mut transaction = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(BOOTSTRAP_LOCK_ID)
        .execute(&mut *transaction)
        .await?;
    let initialized: bool = sqlx::query_scalar("SELECT to_regclass('public.users') IS NOT NULL")
        .fetch_one(&mut *transaction)
        .await?;
    if initialized {
        transaction.commit().await?;
        return Ok(());
    }
    if application_object_count(&mut transaction).await? != 0 {
        return Err(SchemaError::PartialSchema);
    }
    sqlx::query("SET LOCAL search_path = public, pg_catalog")
        .execute(&mut *transaction)
        .await?;
    sqlx::raw_sql(SHARED_PLATFORM_BASELINE)
        .execute(&mut *transaction)
        .await?;
    sqlx::raw_sql(KNOWLEDGE_BASE_BASELINE)
        .execute(&mut *transaction)
        .await?;
    sqlx::raw_sql(BIDDING_BASELINE)
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_url_has_no_checked_in_fallback() {
        let source = include_str!("db.rs");
        assert!(!source.contains(&["postgres", "://"].concat()));
        assert!(!source.contains(&["154", "32"].concat()));
        assert!(database_url_from(|_| None).is_err());
    }

    #[test]
    fn database_parts_escape_credentials_and_detect_conflicts() {
        use sqlx::ConnectOptions;
        use std::str::FromStr;
        let entries = [
            ("POSTGRES_USER", "app@/:%user"),
            ("POSTGRES_PASSWORD", " p@ss:/?#% "),
        ];
        let url = database_url_from(|key| {
            entries
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| (*v).into())
        })
        .unwrap();
        let parsed = sqlx::postgres::PgConnectOptions::from_str(&url).unwrap();
        assert_eq!(parsed.get_username(), "app@/:%user");
        assert_eq!(parsed.get_database(), Some("knowledgebrain"));
        assert_eq!(
            parsed
                .clone()
                .username("application")
                .to_url_lossy()
                .password(),
            sqlx::postgres::PgConnectOptions::new()
                .password(" p@ss:/?#% ")
                .to_url_lossy()
                .password()
        );
        assert!(
            database_url_from(|key| {
                if key == "DATABASE_URL" {
                    Some(url.clone())
                } else {
                    entries
                        .iter()
                        .find(|(k, _)| *k == key)
                        .map(|(_, v)| (*v).into())
                }
            })
            .is_ok()
        );
        assert!(
            database_url_from(|key| match key {
                "POSTGRES_HOST" => Some("invalid host".into()),
                _ => entries
                    .iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, v)| (*v).into()),
            })
            .is_err()
        );
        assert!(
            database_url_from(|key| match key {
                "DATABASE_URL" => Some(url.clone()),
                "POSTGRES_USER" => Some("different".into()),
                _ => None,
            })
            .is_err()
        );
        for value in ["0", "65536", "abc"] {
            assert!(
                database_url_from(|key| if key == "POSTGRES_PORT" {
                    Some(value.into())
                } else {
                    None
                })
                .is_err()
            );
        }
    }

    #[test]
    fn actor_identity_is_only_a_user_or_api_key() {
        let allowlist = SHARED_PLATFORM_BASELINE
            .split_once("CREATE FUNCTION kb_actor_identity_valid")
            .unwrap()
            .1
            .split_once("$$;")
            .unwrap()
            .0;
        assert!(allowlist.contains("^(user|api_key):"));
        assert!(!allowlist.contains("system:"));
        assert!(!SHARED_PLATFORM_BASELINE.contains("SYSTEM_ACTOR_REQUIRED"));
        assert!(!BIDDING_BASELINE.contains("SYSTEM_ACTOR_REQUIRED"));
        assert!(!KNOWLEDGE_BASE_BASELINE.contains("system:"));
    }

    #[test]
    fn knowledge_baseline_stores_chunk_source_locator_and_deleted_parse_status() {
        let documents = KNOWLEDGE_BASE_BASELINE
            .split_once("CREATE TABLE documents (")
            .unwrap()
            .1
            .split_once("CREATE UNIQUE INDEX documents_live_file_uidx")
            .unwrap()
            .0;
        assert!(documents.contains(
            "'pending', 'processing', 'finalizing', 'completed', 'failed', 'cancelled', 'deleting',\n        'deleted'"
        ));
        let chunks = KNOWLEDGE_BASE_BASELINE
            .split_once("CREATE TABLE chunks (")
            .unwrap()
            .1
            .split_once("CREATE INDEX chunks_document_idx")
            .unwrap()
            .0;
        assert!(chunks.contains("source_locator jsonb,"));
        assert!(
            !chunks.contains("source_locator jsonb NOT NULL"),
            "existing chunks stay readable when no locator was stored"
        );
    }
}
