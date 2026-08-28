use sqlx::PgPool;

pub const KNOWLEDGE_BASE_BASELINE: &str =
    include_str!("../../../migrations/knowledge_base_baseline.sql");
pub const SHARED_PLATFORM_BASELINE: &str =
    include_str!("../../../migrations/shared_platform_baseline.sql");
pub const BIDDING_BASELINE: &str = include_str!("../../../migrations/bidding_v2_baseline.sql");

const DEFAULT_DATABASE_URL: &str =
    "postgres://knowledgebrain:knowledgebrain@127.0.0.1:15432/knowledgebrain";
const BOOTSTRAP_LOCK_ID: i64 = 0x4b_42_53_43_48_45_4d_41;

fn database_url_from_env_value(
    value: Result<String, std::env::VarError>,
) -> Result<String, std::env::VarError> {
    match value {
        Ok(value) => Ok(value),
        Err(std::env::VarError::NotPresent) => Ok(DEFAULT_DATABASE_URL.into()),
        Err(error) => Err(error),
    }
}

pub fn database_url() -> Result<String, std::env::VarError> {
    database_url_from_env_value(std::env::var("DATABASE_URL"))
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

/// Process-wide runtime pool. Runtime startup only connects; schema bootstrap is
/// an explicit deployment/test action and is never a readiness or launch gate.
pub async fn connect() -> Result<PgPool, sqlx::Error> {
    POOL.get_or_try_init(open_pool).await.cloned()
}

async fn schema_slice_state(pool: &PgPool) -> Result<(bool, bool, bool), sqlx::Error> {
    sqlx::query_as(
        "SELECT
           to_regclass('public.workspaces') IS NOT NULL,
           to_regclass('public.object_registry') IS NOT NULL,
           to_regclass('public.bid_projects') IS NOT NULL",
    )
    .fetch_one(pool)
    .await
}

/// Apply the single fresh schema directly. This helper is checksum-, manifest-,
/// marker-, and compatibility-free. An already-complete schema is accepted;
/// a partial schema must be reset rather than silently repaired.
pub async fn apply_fresh_baseline(pool: &PgPool) -> Result<(), sqlx::Error> {
    let mut lock_connection = pool.acquire().await?;
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(BOOTSTRAP_LOCK_ID)
        .execute(&mut *lock_connection)
        .await?;

    let result = async {
        match schema_slice_state(pool).await? {
            (true, true, true) => return Ok(()),
            (false, false, false) => {}
            state => {
                return Err(sqlx::Error::Protocol(format!(
                    "fresh schema is partial (knowledge={}, shared={}, bidding={}); reset the database",
                    state.0, state.1, state.2
                )));
            }
        }

        let mut transaction = pool.begin().await?;
        sqlx::raw_sql(KNOWLEDGE_BASE_BASELINE).execute(&mut *transaction).await?;
        sqlx::raw_sql(SHARED_PLATFORM_BASELINE).execute(&mut *transaction).await?;
        sqlx::raw_sql(BIDDING_BASELINE).execute(&mut *transaction).await?;
        transaction.commit().await
    }
    .await;

    let unlock = sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(BOOTSTRAP_LOCK_ID)
        .execute(&mut *lock_connection)
        .await;
    result?;
    unlock?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_database_url_only_when_absent() {
        assert_eq!(
            database_url_from_env_value(Err(std::env::VarError::NotPresent)).unwrap(),
            DEFAULT_DATABASE_URL
        );
        assert!(database_url_from_env_value(Err(std::env::VarError::NotUnicode(
            std::ffi::OsString::from("bad")
        ))).is_err());
    }

    #[test]
    fn fresh_schema_contains_only_target_bidding_contract() {
        assert!(BIDDING_BASELINE.contains("CREATE TABLE bid_submission_workspaces"));
        assert!(!BIDDING_BASELINE.contains("SubmissionGateV1"));
        assert!(!BIDDING_BASELINE.contains("required_part_keys"));
        assert!(!BIDDING_BASELINE.contains("template_slot_for_part_key"));
    }

    #[test]
    fn runtime_connect_never_bootstraps_schema() {
        let source = include_str!("db.rs");
        let connect_body = source
            .split("pub async fn connect()")
            .nth(1)
            .expect("connect body")
            .split("async fn schema_slice_state")
            .next()
            .expect("connect end");
        assert!(!connect_body.contains("apply_fresh_baseline"));
        assert!(!connect_body.contains("raw_sql"));
    }
}
