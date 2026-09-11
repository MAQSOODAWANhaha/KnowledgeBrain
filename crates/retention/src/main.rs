use axum::{Json, Router, http::StatusCode, routing::get};
use retention::{ObjectRetentionWorker, ObjectUploadExpireWorker, RetentionCtx};
use sqlx::PgPool;
use std::time::Duration;
use tokio::net::TcpListener;

const UPLOAD_STAGING_EXPIRY_INTERVAL: Duration = Duration::from_secs(5 * 60);

async fn expire_upload_staging_once(pool: &PgPool) -> Result<i32, sqlx::Error> {
    platform::enqueue_expired_object_uploads(pool).await
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
    let pool = platform::connect_runtime_verified(platform::SchemaComponentKind::Retention)
        .await
        .unwrap_or_else(|error| panic!("retention schema readiness failed: {error}"));

    let _expiry_task = spawn_upload_staging_expiry(pool.clone(), UPLOAD_STAGING_EXPIRY_INTERVAL);

    let storage = platform::oxana_connect()
        .unwrap_or_else(|error| panic!("retention Oxana configuration failed: {error}"));
    let runtime = storage
        .runtime(RetentionCtx::new(pool.clone()))
        .queue_with_concurrency::<platform::RetentionQueue>(platform::runtime_concurrency(
            "RETENTION",
            4,
        ))
        .worker::<ObjectRetentionWorker, platform::ObjectRetentionJob>()
        .worker::<ObjectUploadExpireWorker, platform::ObjectUploadExpireJob>()
        .run();

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
                    let check = platform::inspect_readiness(
                        &pool,
                        platform::SchemaComponentKind::Retention,
                    )
                    .await;
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
    let probe = axum::serve(listener, app);
    tokio::select! {
        result = runtime => { result.unwrap_or_else(|error| panic!("retention Oxana runtime failed: {error}")); },
        result = probe => result.unwrap_or_else(|error| panic!("retention probe failed: {error}")),
    }
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
                 AND to_regprocedure('public.kb_object_upload_expiry_candidates()') IS NOT NULL
                 AND to_regprocedure('public.kb_object_upload_expire_one(uuid)') IS NOT NULL
                 AND to_regprocedure('public.kb_retention_preflight(uuid,kb_object_ref,kb_sha256,bigint)') IS NOT NULL",
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
        let require_ready =
            std::env::var("KNOWLEDGEBRAIN_REQUIRE_RETENTION_READY").as_deref() == Ok("1");
        let Some(pool) = retention_role_pool().await else {
            assert!(
                !require_ready,
                "required retention readiness needs a role pool"
            );
            return;
        };
        let check =
            platform::inspect_readiness(&pool, platform::SchemaComponentKind::Retention).await;
        if require_ready {
            match check {
                platform::ReadyCheck::Ready { gate_mode, .. } => assert_eq!(gate_mode, "open"),
                check => panic!("required retention readiness was not ready: {check:?}"),
            }
            return;
        }
        match check {
            platform::ReadyCheck::Ready { gate_mode, .. }
            | platform::ReadyCheck::NotReady {
                gate_mode: Some(gate_mode),
                ..
            } => assert!(!gate_mode.is_empty()),
            check => panic!("retention readiness could not read its schema/gate inputs: {check:?}"),
        }
    }

    #[tokio::test]
    async fn retention_role_uses_only_fenced_preflight_and_completion_functions() {
        let _guard = db_lock().await;
        let Some(admin) = admin_test_pool().await else {
            return;
        };
        if !retention_test_schema_is_ready(&admin).await {
            return;
        }
        let Some(retention) = retention_role_pool().await else {
            return;
        };
        let staging_id = Uuid::new_v4();
        let bytes = format!("retention-role-e2e-{staging_id}").into_bytes();
        let digest = platform::sha256_hex(&bytes);
        let object_ref = platform::object_ref(&digest);
        platform::stage_object_upload(
            &admin,
            staging_id,
            &object_ref,
            &digest,
            "application/octet-stream",
            bytes.len() as i64,
            "system:tender-document-process-v2",
        )
        .await
        .unwrap();
        let deletion = platform::abandon_object_upload(
            &admin,
            staging_id,
            "system:tender-document-process-v2",
        )
        .await
        .unwrap()
        .expect("sole staged owner creates deletion identity");
        assert!(
            sqlx::query("SELECT object_ref FROM object_registry LIMIT 1")
                .execute(&retention)
                .await
                .is_err(),
            "retention role must not read ObjectRegistry tables directly"
        );
        platform::process_object_deletion(&retention, &deletion)
            .await
            .unwrap();
        platform::process_object_deletion(&retention, &deletion)
            .await
            .unwrap();
        let tombstone: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM object_retention_tombstones WHERE deletion_id=$1)",
        )
        .bind(deletion.deletion_id)
        .fetch_one(&admin)
        .await
        .unwrap();
        assert!(tombstone);
        let mut mismatch = deletion;
        mismatch.byte_length += 1;
        assert!(
            platform::process_object_deletion(&retention, &mismatch)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn api_style_dispatch_reaches_typed_oxana_retention_worker() {
        let _guard = db_lock().await;
        let Some(admin) = admin_test_pool().await else {
            return;
        };
        if !retention_test_schema_is_ready(&admin).await {
            return;
        }
        let Some(retention) = retention_role_pool().await else {
            return;
        };
        let staging_id = Uuid::new_v4();
        let bytes = format!("oxana retention e2e {staging_id}").into_bytes();
        let digest = platform::sha256_hex(&bytes);
        let object_ref = platform::object_ref(&digest);
        platform::stage_object_upload(
            &admin,
            staging_id,
            &object_ref,
            &digest,
            "application/octet-stream",
            bytes.len() as i64,
            "system:tender-document-process-v2",
        )
        .await
        .unwrap();
        let deletion = platform::abandon_object_upload(
            &admin,
            staging_id,
            "system:tender-document-process-v2",
        )
        .await
        .unwrap()
        .unwrap();
        let storage = platform::oxana_connect().expect("configured Oxana Redis");
        let (stop_tx, mut stop_rx) = tokio::sync::watch::channel(false);
        let runtime = storage
            .runtime(RetentionCtx::new(retention.clone()))
            .queue_with_concurrency::<platform::RetentionQueue>(1)
            .worker::<ObjectRetentionWorker, platform::ObjectRetentionJob>()
            .worker::<ObjectUploadExpireWorker, platform::ObjectUploadExpireJob>()
            .shutdown_on(async move {
                while !*stop_rx.borrow() {
                    if stop_rx.changed().await.is_err() {
                        break;
                    }
                }
                Ok(())
            })
            .shutdown_timeout(Duration::from_secs(5))
            .run();
        let runtime = tokio::spawn(runtime);
        platform::dispatch_object_deletion(&admin, deletion.clone())
            .await
            .unwrap();
        let mut completed = false;
        for _ in 0..100 {
            completed = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM object_retention_tombstones WHERE deletion_id=$1)",
            )
            .bind(deletion.deletion_id)
            .fetch_one(&admin)
            .await
            .unwrap();
            if completed {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        stop_tx.send(true).unwrap();
        runtime.await.unwrap().unwrap();
        assert!(
            completed,
            "typed Oxana retention worker must write the tombstone"
        );
    }

    #[tokio::test]
    async fn expiry_scan_only_enqueues_and_typed_job_expires_exact_staging_id() {
        let _guard = db_lock().await;
        let Some(pool) = admin_test_pool().await else {
            return;
        };
        if !retention_test_schema_is_ready(&pool).await {
            return;
        }
        let staging_id = Uuid::new_v4();
        let actor = format!("user:{}", Uuid::new_v4());
        let bytes = format!("expired retention staging {staging_id}").into_bytes();
        let digest = platform::sha256_hex(&bytes);
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

        let queued = expire_upload_staging_once(&pool).await.unwrap();
        assert!((1..=100).contains(&queued));
        let retained: i64 =
            sqlx::query_scalar("SELECT count(*) FROM object_upload_staging WHERE id=$1")
                .bind(staging_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(retained, 1, "the periodic scanner must never delete rows");
        let deletion = platform::expire_one_object_upload(&pool, staging_id)
            .await
            .unwrap();
        if let Some(deletion) = deletion {
            platform::process_object_deletion(&pool, &deletion)
                .await
                .unwrap();
        }
        let remaining: i64 =
            sqlx::query_scalar("SELECT count(*) FROM object_upload_staging WHERE id=$1")
                .bind(staging_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(remaining, 0);
    }

    #[tokio::test]
    async fn redis_enqueue_failure_leaves_expired_business_row_replayable() {
        let _guard = db_lock().await;
        let Some(pool) = admin_test_pool().await else {
            return;
        };
        if !retention_test_schema_is_ready(&pool).await {
            return;
        }
        let staging_id = Uuid::new_v4();
        let actor = format!("user:{}", Uuid::new_v4());
        let bytes = format!("redis failure {staging_id}").into_bytes();
        let digest = platform::sha256_hex(&bytes);
        platform::stage_object_upload(
            &pool,
            staging_id,
            &platform::object_ref(&digest),
            &digest,
            "application/pdf",
            bytes.len() as i64,
            &actor,
        )
        .await
        .unwrap();
        sqlx::query("UPDATE object_upload_staging SET created_at=clock_timestamp()-interval '2 seconds', expires_at=clock_timestamp()-interval '1 second' WHERE id=$1")
            .bind(staging_id).execute(&pool).await.unwrap();
        let error = platform::enqueue_expired_object_uploads_with(&pool, |_| async {
            Err("redis unavailable".to_string())
        })
        .await
        .unwrap_err();
        assert!(error.to_string().contains("redis unavailable"));
        let retained: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM object_upload_staging WHERE id=$1)")
                .bind(staging_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(retained);
        let _ = platform::expire_one_object_upload(&pool, staging_id)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn expiry_enqueue_batch_is_bounded_and_jobs_drain_exact_identities() {
        const EXPECTED_BATCH_LIMIT: usize = 100;

        let _guard = db_lock().await;
        let Some(pool) = admin_test_pool().await else {
            return;
        };
        if !retention_test_schema_is_ready(&pool).await {
            return;
        }
        let actor = format!("user:{}", Uuid::new_v4());
        let bytes = format!("bounded retention staging {actor}").into_bytes();
        let digest = platform::sha256_hex(&bytes);
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
            after_first,
            (EXPECTED_BATCH_LIMIT + 1) as i64,
            "enqueueing must not mutate business rows"
        );
        let candidates: Vec<Uuid> =
            sqlx::query_scalar("SELECT * FROM kb_object_upload_expiry_candidates()")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(candidates.len(), EXPECTED_BATCH_LIMIT);
        for staging_id in candidates {
            let _ = platform::expire_one_object_upload(&pool, staging_id)
                .await
                .unwrap();
        }

        let second_expired = expire_upload_staging_once(&pool).await.unwrap();
        let after_second: i64 =
            sqlx::query_scalar("SELECT count(*) FROM object_upload_staging WHERE created_by=$1")
                .bind(&actor)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(second_expired, 1);
        assert_eq!(after_second, 1);
        let remaining_ids: Vec<Uuid> = sqlx::query_scalar(
            "SELECT id FROM object_upload_staging WHERE created_by=$1 ORDER BY id",
        )
        .bind(&actor)
        .fetch_all(&pool)
        .await
        .unwrap();
        for staging_id in remaining_ids {
            let _ = platform::expire_one_object_upload(&pool, staging_id)
                .await
                .unwrap();
        }
    }
}
