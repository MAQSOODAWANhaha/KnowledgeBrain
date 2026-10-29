use sqlx::PgPool;
use thiserror::Error;

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

#[cfg(test)]
const KNOWLEDGE_BASE_BASELINE: &str =
    include_str!("../../../migrations/20261028000002_knowledge_base.sql");
#[cfg(test)]
const SHARED_PLATFORM_BASELINE: &str =
    include_str!("../../../migrations/20261028000001_shared_platform.sql");
#[cfg(test)]
const TENDER_BASELINE: &str = include_str!("../../../migrations/20261028000003_tender.sql");

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

/// Apply pending SQLx migrations and validate previously applied checksums.
/// Only the deployment migrator invokes this; application startup never runs DDL.
pub async fn migrate_database(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    let mut connection = pool.acquire().await?;
    // SQLx holds a session advisory lock and may return before unlocking on an
    // error. Never return this connection to the pool, including on cancellation.
    connection.close_on_drop();
    let result = MIGRATOR.run(&mut *connection).await;
    let closed = connection.close().await;
    result?;
    closed?;
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
    fn baselines_have_no_database_authorization_layer() {
        for ddl in [
            SHARED_PLATFORM_BASELINE,
            KNOWLEDGE_BASE_BASELINE,
            TENDER_BASELINE,
        ] {
            assert!(!ddl.contains("SECURITY DEFINER"));
            assert!(!ddl.contains("CREATE ROLE"));
            assert!(!ddl.contains("CREATE POLICY"));
            assert!(!ddl.contains("CREATE FUNCTION"));
            assert!(!ddl.contains("CREATE TRIGGER"));
        }
    }

    #[test]
    fn knowledge_baseline_stores_chunk_source_locator_and_deleted_parse_status() {
        let documents = KNOWLEDGE_BASE_BASELINE
            .split_once("CREATE TABLE knowledge_documents (")
            .unwrap()
            .1
            .split_once("\n);")
            .unwrap()
            .0;
        assert!(documents.contains(
            "'pending', 'processing', 'finalizing', 'completed', 'failed', 'cancelled', 'deleting',\n        'deleted'"
        ));
        let chunks = KNOWLEDGE_BASE_BASELINE
            .split_once("CREATE TABLE knowledge_chunks (")
            .unwrap()
            .1
            .split_once("\n);")
            .unwrap()
            .0;
        assert!(chunks.contains("source_locator jsonb,"));
        assert!(
            !chunks.contains("source_locator jsonb NOT NULL"),
            "existing chunks stay readable when no locator was stored"
        );
    }
}
