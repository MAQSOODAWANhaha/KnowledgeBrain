use axum::{Json, Router, http::StatusCode, routing::get};
use sqlx::PgPool;
use std::time::Duration;
use tokio::net::TcpListener;

const UPLOAD_STAGING_EXPIRY_INTERVAL: Duration = Duration::from_secs(5 * 60);

async fn expire_upload_staging_once(pool: &PgPool) -> Result<i32, sqlx::Error> {
    platform::expire_object_uploads(pool).await
}

fn spawn_upload_staging_expiry(pool: PgPool, interval: Duration) -> tokio::task::JoinHandle<()> {
    tokio::spawn(run_upload_staging_expiry(pool, interval))
}

async fn run_upload_staging_expiry(pool: PgPool, expiry_interval: Duration) {
    let mut interval = tokio::time::interval(expiry_interval);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        interval.tick().await;
        match expire_upload_staging_once(&pool).await {
            Ok(expired) if expired > 0 => {
                tracing::info!(expired, "expired upload staging references")
            }
            Ok(_) => {}
            Err(error) => tracing::error!(%error, "upload staging expiry iteration failed"),
        }
    }
}

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();
    platform::init_tracing();
    let pool = platform::connect()
        .await
        .unwrap_or_else(|error| panic!("retention postgres connection failed: {error}"));

    let _expiry_task = spawn_upload_staging_expiry(pool.clone(), UPLOAD_STAGING_EXPIRY_INTERVAL);

    let consumer_pool = pool.clone();
    tokio::spawn(async move {
        loop {
            match platform::process_one_retention_item(&consumer_pool, "retention-v1").await {
                Ok(true) => continue,
                Ok(false) => tokio::time::sleep(std::time::Duration::from_secs(2)).await,
                Err(error) => {
                    tracing::error!(%error, "retention iteration failed");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
            }
        }
    });

    let probe_pool = pool.clone();
    let app = Router::new()
        .route(
            "/live",
            get(|| async { Json(platform::live_body("retention")) }),
        )
        .route(
            "/ready",
            get(move || {
                let pool = probe_pool.clone();
                async move {
                    let check = platform::inspect_readiness(&pool).await;
                    let status = if check.is_ready() {
                        StatusCode::OK
                    } else {
                        StatusCode::SERVICE_UNAVAILABLE
                    };
                    (status, Json(platform::ready_body("retention", &check)))
                }
            }),
        );
    let address =
        std::env::var("RETENTION_PROBE_ADDR").unwrap_or_else(|_| "0.0.0.0:8082".to_owned());
    let listener = TcpListener::bind(&address)
        .await
        .unwrap_or_else(|error| panic!("retention probe bind {address}: {error}"));
    tracing::info!(%address, "retention consumer ready");
    axum::serve(listener, app)
        .await
        .unwrap_or_else(|error| panic!("retention probe failed: {error}"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;
    use tokio::sync::Mutex;
    use uuid::Uuid;

    async fn db_lock() -> tokio::sync::MutexGuard<'static, ()> {
        static LOCK: Mutex<()> = Mutex::const_new(());
        LOCK.lock().await
    }

    fn postgres_contract_tests_required() -> bool {
        std::env::var("KNOWLEDGEBRAIN_REQUIRE_POSTGRES_TESTS").as_deref() == Ok("1")
            || std::env::var_os("KNOWLEDGEBRAIN_TEST_DATABASE_URL").is_some()
    }

    fn isolated_test_database_url(label: &str) -> Option<String> {
        let database_url = match std::env::var("KNOWLEDGEBRAIN_TEST_DATABASE_URL") {
            Ok(database_url) if !database_url.trim().is_empty() => database_url,
            Ok(_) | Err(_) if postgres_contract_tests_required() => {
                panic!("required {label} needs explicit KNOWLEDGEBRAIN_TEST_DATABASE_URL")
            }
            Ok(_) | Err(_) => {
                eprintln!("skip: {label} needs explicit KNOWLEDGEBRAIN_TEST_DATABASE_URL");
                return None;
            }
        };
        let normalized = database_url.to_ascii_lowercase();
        assert!(
            !normalized.contains(":15432/") && !normalized.ends_with(":15432"),
            "refusing retention tests against live PostgreSQL port 15432"
        );
        Some(database_url)
    }

    async fn admin_test_pool() -> Option<PgPool> {
        let database_url = isolated_test_database_url("retention PostgreSQL contract")?;
        match PgPoolOptions::new()
            .max_connections(4)
            .connect(&database_url)
            .await
        {
            Ok(pool) => Some(pool),
            Err(error) if postgres_contract_tests_required() => {
                panic!("required PostgreSQL retention test unavailable: {error}")
            }
            Err(error) => {
                eprintln!("skip: retention PostgreSQL unavailable: {error}");
                None
            }
        }
    }

    async fn retention_test_schema_is_ready(pool: &PgPool) -> bool {
        let ready = sqlx::query_scalar(
            "SELECT to_regclass('public.object_upload_staging') IS NOT NULL
                 AND to_regprocedure('public.kb_object_upload_stage(uuid,kb_object_ref,kb_sha256,text,bigint,kb_actor_identity)') IS NOT NULL
                 AND to_regprocedure('public.kb_object_upload_expire()') IS NOT NULL",
        )
        .fetch_one(pool)
        .await;
        match ready {
            Ok(true) => true,
            Ok(false) if postgres_contract_tests_required() => {
                panic!("required migrated retention test schema is unavailable")
            }
            Err(error) if postgres_contract_tests_required() => {
                panic!("inspect required retention test schema: {error}")
            }
            Ok(false) => {
                eprintln!("skip: migrated retention test schema is unavailable");
                false
            }
            Err(error) => {
                eprintln!("skip: inspect retention test schema: {error}");
                false
            }
        }
    }

    async fn retention_role_pool() -> Option<PgPool> {
        let database_url = isolated_test_database_url("retention role contract")?;
        let password = match std::env::var("KNOWLEDGEBRAIN_RETENTION_DB_PASSWORD") {
            Ok(password) => password,
            Err(error) if postgres_contract_tests_required() => {
                panic!("required retention role password unavailable: {error}")
            }
            Err(error) => {
                eprintln!("skip: retention role password unavailable: {error}");
                return None;
            }
        };
        let options = PgConnectOptions::from_str(&database_url)
            .expect("parse retention test database URL")
            .username("kb_runtime_retention")
            .password(&password);
        match PgPoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
        {
            Ok(pool) => Some(pool),
            Err(error) if postgres_contract_tests_required() => {
                panic!("required retention role connection unavailable: {error}")
            }
            Err(error) => {
                eprintln!("skip: retention role connection unavailable: {error}");
                None
            }
        }
    }

    #[tokio::test]
    async fn retention_role_can_inspect_readiness_gate() {
        let _guard = db_lock().await;
        let Some(pool) = retention_role_pool().await else {
            return;
        };
        match platform::inspect_readiness(&pool).await {
            platform::ReadyCheck::Ready { gate_mode, .. }
            | platform::ReadyCheck::NotReady {
                gate_mode: Some(gate_mode),
                ..
            } => assert!(!gate_mode.is_empty()),
            check => panic!("retention readiness could not read its schema/gate inputs: {check:?}"),
        }
    }

    #[tokio::test]
    async fn expired_upload_staging_is_removed_by_running_retention_expiry_loop() {
        let _guard = db_lock().await;
        let Some(pool) = admin_test_pool().await else {
            return;
        };
        if !retention_test_schema_is_ready(&pool).await {
            return;
        }
        let staging_id = Uuid::new_v4();
        let actor = format!("user:{}", Uuid::new_v4());
        let bytes = b"expired retention staging";
        let digest = platform::sha256_hex(bytes);
        let object_ref = platform::object_ref(&digest);
        platform::stage_object_upload(
            &pool,
            staging_id,
            &object_ref,
            &digest,
            "application/pdf",
            bytes.len() as i64,
            &actor,
        )
        .await
        .unwrap();
        let expiry_task = spawn_upload_staging_expiry(pool.clone(), Duration::from_millis(10));
        tokio::time::sleep(Duration::from_millis(50)).await;
        sqlx::query(
            "UPDATE object_upload_staging
                SET created_at=clock_timestamp()-interval '2 seconds',
                    expires_at=clock_timestamp()-interval '1 second'
              WHERE id=$1",
        )
        .bind(staging_id)
        .execute(&pool)
        .await
        .unwrap();

        let mut remaining = 1_i64;
        for _ in 0..100 {
            remaining =
                sqlx::query_scalar("SELECT count(*) FROM object_upload_staging WHERE id=$1")
                    .bind(staging_id)
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            if remaining == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        expiry_task.abort();
        let _ = expiry_task.await;
        assert_eq!(remaining, 0);
    }

    #[tokio::test]
    async fn one_expiry_iteration_is_bounded_and_the_next_drains_remaining_backlog() {
        const EXPECTED_BATCH_LIMIT: usize = 100;

        let _guard = db_lock().await;
        let Some(pool) = admin_test_pool().await else {
            return;
        };
        if !retention_test_schema_is_ready(&pool).await {
            return;
        }
        let actor = format!("user:{}", Uuid::new_v4());
        let bytes = b"bounded retention staging";
        let digest = platform::sha256_hex(bytes);
        let object_ref = platform::object_ref(&digest);
        for _ in 0..=EXPECTED_BATCH_LIMIT {
            platform::stage_object_upload(
                &pool,
                Uuid::new_v4(),
                &object_ref,
                &digest,
                "application/pdf",
                bytes.len() as i64,
                &actor,
            )
            .await
            .unwrap();
        }
        sqlx::query(
            "UPDATE object_upload_staging
                SET created_at=clock_timestamp()-interval '2 seconds',
                    expires_at=clock_timestamp()-interval '1 second'
              WHERE created_by=$1",
        )
        .bind(&actor)
        .execute(&pool)
        .await
        .unwrap();

        let first_expired = expire_upload_staging_once(&pool).await.unwrap();
        let after_first: i64 =
            sqlx::query_scalar("SELECT count(*) FROM object_upload_staging WHERE created_by=$1")
                .bind(&actor)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(first_expired, EXPECTED_BATCH_LIMIT as i32);
        assert_eq!(
            after_first, 1,
            "one backlog item must remain for the next tick"
        );

        let second_expired = expire_upload_staging_once(&pool).await.unwrap();
        let after_second: i64 =
            sqlx::query_scalar("SELECT count(*) FROM object_upload_staging WHERE created_by=$1")
                .bind(&actor)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(second_expired, 1);
        assert_eq!(after_second, 0);
    }
}
