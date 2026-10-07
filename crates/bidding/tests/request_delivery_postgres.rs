#[allow(dead_code)]
mod support;

use retention::{ObjectRetentionWorker, ObjectUploadExpireWorker, RetentionCtx};
use serde_json::Value;
use sqlx::{PgPool, Postgres};
use std::time::Duration;
use uuid::Uuid;

const DELIVERY_TEST_ADVISORY_LOCK: i64 = 0x4b425f44454c4956;

async fn acquire_delivery_test_lock(pool: &PgPool) -> sqlx::pool::PoolConnection<Postgres> {
    let mut connection = pool.acquire().await.expect("delivery test lock connection");
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(DELIVERY_TEST_ADVISORY_LOCK)
        .execute(&mut *connection)
        .await
        .expect("acquire delivery test advisory lock");
    connection
}

async fn release_delivery_test_lock(connection: &mut sqlx::pool::PoolConnection<Postgres>) {
    sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(DELIVERY_TEST_ADVISORY_LOCK)
        .execute(&mut **connection)
        .await
        .expect("release delivery test advisory lock");
}

fn require_local_objects() {
    let dir =
        std::env::var_os("OBJECT_DIR").expect("cleanup contract requires explicit OBJECT_DIR");
    assert!(
        std::path::Path::new(&dir).is_dir(),
        "OBJECT_DIR must already exist"
    );
    assert!(
        std::env::var("KNOWLEDGEBRAIN_S3_ENDPOINT")
            .unwrap_or_default()
            .is_empty(),
        "cleanup contract uses local objects only"
    );
}

struct RetentionConsumer {
    stop: tokio::sync::watch::Sender<bool>,
    task: tokio::task::JoinHandle<Result<(), oxana::OxanaError>>,
}

impl RetentionConsumer {
    async fn finish(mut self) {
        self.stop
            .send(true)
            .expect("retention runtime is still running");
        tokio::time::timeout(Duration::from_secs(10), &mut self.task)
            .await
            .expect("retention shutdown deadline")
            .expect("retention task join")
            .expect("retention runtime result");
    }
}

impl Drop for RetentionConsumer {
    fn drop(&mut self) {
        // Also stop the owned runtime when a test assertion panics.
        let _ = self.stop.send(true);
        self.task.abort();
    }
}

async fn start_retention_consumer() -> RetentionConsumer {
    platform::init_tracing();
    let url = std::env::var("KNOWLEDGEBRAIN_TEST_DATABASE_URL").unwrap();
    let options = url
        .parse::<sqlx::postgres::PgConnectOptions>()
        .unwrap()
        .username("kb_runtime_retention")
        .password(
            &std::env::var("KNOWLEDGEBRAIN_RETENTION_DB_PASSWORD")
                .expect("cleanup contract requires retention role password"),
        );
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .connect_with(options)
        .await
        .expect("real retention role login");
    let storage = platform::oxana_connect().expect("explicit Oxana configuration");
    let (stop, mut stopped) = tokio::sync::watch::channel(false);
    let runtime = storage
        .runtime(RetentionCtx::new(pool))
        .queue_with_concurrency::<platform::RetentionQueue>(1)
        .worker::<ObjectRetentionWorker, platform::ObjectRetentionJob>()
        .worker::<ObjectUploadExpireWorker, platform::ObjectUploadExpireJob>()
        .shutdown_on(async move {
            while !*stopped.borrow() {
                if stopped.changed().await.is_err() {
                    break;
                }
            }
            Ok(())
        })
        .shutdown_timeout(Duration::from_secs(5))
        .run();
    RetentionConsumer {
        stop,
        task: tokio::spawn(async move { runtime.await.map(|_| ()) }),
    }
}

async fn wait_for_deleted(
    pool: &PgPool,
    staging_id: Uuid,
    digest: &str,
    length: i64,
) -> Result<(), tokio::time::error::Elapsed> {
    tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            let completed: bool = sqlx::query_scalar(
                "SELECT NOT EXISTS(SELECT 1 FROM object_upload_staging WHERE id=$1)
                  AND EXISTS(SELECT 1 FROM object_registry r
                    JOIN object_retention_tombstones t ON t.object_ref=r.object_ref
                    JOIN object_deletion_artifacts d ON d.id=t.deletion_id
                    WHERE t.deletion_id=$1 AND r.digest=$2 AND t.digest=$2
                      AND d.digest=$2 AND d.byte_length=$3 AND t.byte_length=$3
                      AND r.byte_length=$3 AND r.state='deleted'
                      AND r.deleted_at IS NOT NULL AND t.deleted_at IS NOT NULL
                      AND t.deleted_by IS NULL)",
            )
            .bind(staging_id)
            .bind(digest)
            .bind(length)
            .fetch_one(pool)
            .await
            .unwrap();
            if completed
                && !platform::blob_path(digest)
                    .expect("valid configured test object path")
                    .exists()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
}

#[tokio::test]
async fn cleanup_native_duplicates_protect_references_and_retry_failed_blob_deletion() {
    let Some(pool) = support::connect_postgres_contract("typed cleanup recovery").await else {
        return;
    };
    let mut test_lock = acquire_delivery_test_lock(&pool).await;
    require_local_objects();
    let first = Uuid::new_v4();
    let remaining_owner = Uuid::new_v4();
    let bytes = format!("cleanup reference and retry {first}").into_bytes();
    let digest = platform::sha256_hex(&bytes);
    let object_ref = platform::object_ref(&digest);
    for id in [first, remaining_owner] {
        platform::stage_object_upload(
            &pool,
            id,
            &object_ref,
            &digest,
            "application/octet-stream",
            bytes.len() as i64,
            None,
        )
        .await
        .unwrap();
    }
    let blob = platform::write_blob_async(&digest, &bytes).await.unwrap();
    let storage = platform::oxana_connect().unwrap();
    let first_job = storage
        .enqueue(
            platform::RetentionQueue,
            platform::ObjectUploadExpireJob { staging_id: first },
        )
        .await
        .unwrap();
    let duplicate_job = storage
        .enqueue(
            platform::RetentionQueue,
            platform::ObjectUploadExpireJob { staging_id: first },
        )
        .await
        .unwrap();
    assert_ne!(
        first_job, duplicate_job,
        "each handoff must acknowledge a new native envelope, not Skip"
    );
    for id in [&first_job, &duplicate_job] {
        let envelope = storage.get_job(id).await.unwrap().unwrap();
        assert_eq!(envelope.job.args["staging_id"], first.to_string());
        assert!(!envelope.meta.unique);
    }
    let consumer = start_retention_consumer().await;
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let released: bool = sqlx::query_scalar(
                "SELECT NOT EXISTS(SELECT 1 FROM object_upload_staging WHERE id=$1)",
            )
            .bind(first)
            .fetch_one(&pool)
            .await
            .unwrap();
            if released
                && storage.get_job(&first_job).await.unwrap().is_none()
                && storage.get_job(&duplicate_job).await.unwrap().is_none()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("both real duplicate upload consumers finish");
    let protected: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM object_upload_staging WHERE id=$1)
          AND EXISTS(SELECT 1 FROM object_registry WHERE object_ref=$2 AND state='available')
          AND NOT EXISTS(SELECT 1 FROM object_deletion_artifacts WHERE object_ref=$2)
          AND NOT EXISTS(SELECT 1 FROM object_retention_tombstones WHERE object_ref=$2)",
    )
    .bind(remaining_owner)
    .bind(&object_ref)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        protected,
        "the remaining reference must refuse physical deletion"
    );
    assert_eq!(std::fs::read(&blob).unwrap(), bytes);

    // A directory at the exact blob path makes the real filesystem deletion fail.
    // Preserve the actual bytes, then repair only this owned fixture after observing native retry.
    let saved = blob.with_extension("retry-bytes");
    std::fs::rename(&blob, &saved).unwrap();
    std::fs::create_dir(&blob).unwrap();
    let retry_job = storage
        .enqueue(
            platform::RetentionQueue,
            platform::ObjectUploadExpireJob {
                staging_id: remaining_owner,
            },
        )
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let envelope = storage
                .get_job(&retry_job)
                .await
                .unwrap()
                .expect("failed job must survive");
            if envelope.meta.retries > 0 {
                assert!(
                    envelope.meta.error.as_deref().is_some_and(|error| {
                        error.contains("Is a directory")
                            || error.contains("regular, singly linked file")
                    }),
                    "retry must be caused by the injected filesystem failure: {:?}",
                    envelope.meta.error
                );
                eprintln!(
                    "native retry observed job={} staging={} retries={}",
                    retry_job, remaining_owner, envelope.meta.retries
                );
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("real deletion error must enter Oxana native retry");
    let recoverable: bool = sqlx::query_scalar(
        "SELECT NOT EXISTS(SELECT 1 FROM object_upload_staging WHERE id=$1)
          AND EXISTS(SELECT 1 FROM object_deletion_artifacts WHERE id=$1 AND object_ref=$2 AND digest=$3)
          AND EXISTS(SELECT 1 FROM object_registry WHERE object_ref=$2 AND state='deleting')
          AND NOT EXISTS(SELECT 1 FROM object_retention_tombstones WHERE deletion_id=$1)")
        .bind(remaining_owner).bind(&object_ref).bind(&digest).fetch_one(&pool).await.unwrap();
    assert!(
        recoverable,
        "failed physical deletion preserves the immutable recovery identity, not a receipt"
    );
    assert_eq!(std::fs::read(&saved).unwrap(), bytes);
    std::fs::remove_dir(&blob).unwrap();
    std::fs::rename(&saved, &blob).unwrap();
    // No re-enqueue, SQL expiry/deletion call, or custom retry budget: same native job resumes.
    wait_for_deleted(&pool, remaining_owner, &digest, bytes.len() as i64)
        .await
        .expect("native retry must finish deletion with the original identity");

    let deletion = platform::ObjectRetentionJob {
        deletion_id: remaining_owner,
        object_ref,
        digest: digest.clone(),
        byte_length: bytes.len() as i64,
    };
    let one = storage
        .enqueue(platform::RetentionQueue, deletion.clone())
        .await
        .unwrap();
    let two = storage
        .enqueue(platform::RetentionQueue, deletion)
        .await
        .unwrap();
    assert_ne!(one, two);
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            if storage.get_job(&one).await.unwrap().is_none()
                && storage.get_job(&two).await.unwrap().is_none()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("both real retention duplicates finish");

    let live_staging = Uuid::new_v4();
    let owner = Uuid::new_v4();
    let live_bytes = format!("persistent owner {owner}").into_bytes();
    let live_digest = platform::sha256_hex(&live_bytes);
    let live_ref = platform::object_ref(&live_digest);
    platform::stage_object_upload(
        &pool,
        live_staging,
        &live_ref,
        &live_digest,
        "application/octet-stream",
        live_bytes.len() as i64,
        None,
    )
    .await
    .unwrap();
    let live_blob = platform::write_blob_async(&live_digest, &live_bytes)
        .await
        .unwrap();
    sqlx::query("SELECT kb_object_reference_add($1::kb_object_ref,$2::kb_sha256,'application/octet-stream',$3,
        'cleanup_test_owner',$4,'payload',NULL)")
        .bind(&live_ref).bind(&live_digest).bind(live_bytes.len() as i64).bind(owner)
        .execute(&pool).await.unwrap();
    let live_job = storage
        .enqueue(
            platform::RetentionQueue,
            platform::ObjectUploadExpireJob {
                staging_id: live_staging,
            },
        )
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let released: bool = sqlx::query_scalar(
                "SELECT NOT EXISTS(SELECT 1 FROM object_upload_staging WHERE id=$1)",
            )
            .bind(live_staging)
            .fetch_one(&pool)
            .await
            .unwrap();
            if released && storage.get_job(&live_job).await.unwrap().is_none() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("real consumer releases staging with a live business owner");
    let live_protected: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM object_owner_references WHERE object_ref=$1 AND owner_id=$2 AND owner_kind='cleanup_test_owner')
          AND EXISTS(SELECT 1 FROM object_registry WHERE object_ref=$1 AND state='available')
          AND NOT EXISTS(SELECT 1 FROM object_deletion_artifacts WHERE object_ref=$1)
          AND NOT EXISTS(SELECT 1 FROM object_retention_tombstones WHERE object_ref=$1)")
        .bind(&live_ref).bind(owner).fetch_one(&pool).await.unwrap();
    assert!(live_protected);
    assert_eq!(std::fs::read(&live_blob).unwrap(), live_bytes);
    // Release the fixture's business ownership, never emulate the consumer's physical deletion.
    let released: Value = sqlx::query_scalar(
        "SELECT kb_object_reference_remove($1::kb_object_ref,'cleanup_test_owner',$2,'payload',$3)",
    )
    .bind(&live_ref)
    .bind(owner)
    .bind(live_staging)
    .fetch_one(&pool)
    .await
    .unwrap();
    let deletion: platform::ObjectDeletionIdentity = serde_json::from_value(released).unwrap();
    platform::dispatch_object_deletion(&pool, deletion)
        .await
        .unwrap();
    wait_for_deleted(&pool, live_staging, &live_digest, live_bytes.len() as i64)
        .await
        .expect("released live owner must reach the real retention consumer and receipt");
    consumer.finish().await;
    let receipts: i64 =
        sqlx::query_scalar("SELECT count(*) FROM object_retention_tombstones WHERE deletion_id=$1")
            .bind(remaining_owner)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(receipts, 1);
    assert!(!blob.exists());
    release_delivery_test_lock(&mut test_lock).await;
}

// Own the fault thread before any child spawn/assertion can unwind. Stop is observed
// even before accept/read; Drop must never panic over the original test failure.
struct HandoffFaultServer {
    address: std::net::SocketAddr,
    request: std::sync::mpsc::Receiver<()>,
    stop: std::sync::mpsc::Sender<()>,
    task: Option<std::thread::JoinHandle<std::io::Result<()>>>,
}

impl HandoffFaultServer {
    fn start() -> std::io::Result<Self> {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        listener.set_nonblocking(true)?;
        let (request_tx, request) = std::sync::mpsc::channel();
        let (stop, stop_rx) = std::sync::mpsc::channel();
        let task = std::thread::Builder::new()
            .name("handoff-fault-server".into())
            .spawn(move || {
                use std::io::Read;
                let deadline = std::time::Instant::now() + Duration::from_secs(10);
                let mut stream = None;
                let mut received = false;
                loop {
                    match stop_rx.recv_timeout(Duration::from_millis(10)) {
                        Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                            return Ok(());
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    }
                    if std::time::Instant::now() >= deadline {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::TimedOut,
                            "fault fixture stop deadline",
                        ));
                    }
                    if stream.is_none() {
                        match listener.accept() {
                            Ok((connection, _)) => {
                                connection.set_nonblocking(true)?;
                                stream = Some(connection);
                            }
                            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                            Err(error) => return Err(error),
                        }
                    }
                    if let Some(connection) = stream.as_mut().filter(|_| !received) {
                        match connection.read(&mut [0]) {
                            Ok(0) => {
                                return Err(std::io::Error::new(
                                    std::io::ErrorKind::UnexpectedEof,
                                    "fault client closed before request",
                                ));
                            }
                            Ok(_) => {
                                received = true;
                                request_tx.send(()).map_err(std::io::Error::other)?;
                            }
                            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                            Err(error) => return Err(error),
                        }
                    }
                    // Hold the real connection without replying until stop, not child EOF.
                }
            })?;
        Ok(Self {
            address,
            request,
            stop,
            task: Some(task),
        })
    }

    fn stop_and_join(&mut self) -> std::io::Result<()> {
        let Some(task) = self.task.take() else {
            return Ok(());
        };
        let _ = self.stop.send(());
        let result = task
            .join()
            .map_err(|_| std::io::Error::other("fault server panicked"))
            .and_then(|result| result);
        use std::io::Write;
        let _ = writeln!(
            std::io::stderr(),
            "fault server stopped/joined address={} result={result:?}",
            self.address
        );
        result
    }

    fn finish(mut self) {
        self.stop_and_join()
            .expect("fault server stopped and joined");
    }
}

impl Drop for HandoffFaultServer {
    fn drop(&mut self) {
        if let Err(error) = self.stop_and_join() {
            use std::io::Write;
            let _ = writeln!(std::io::stderr(), "fault server cleanup error: {error}");
        }
    }
}

#[test]
fn handoff_fault_server_reclaims_socket_after_child_failure() {
    const CHILD: &str = "KB_HANDOFF_FAULT_FAILURE_CHILD";
    if let Ok(address) = std::env::var(CHILD) {
        if address != "before-connect" {
            use std::io::Write;
            let mut stream = std::net::TcpStream::connect(address).unwrap();
            stream.write_all(b"*").unwrap();
        }
        panic!("intentional handoff child failure");
    }
    for mode in ["spawn-error", "before-connect", "after-connect"] {
        let server = HandoffFaultServer::start().unwrap();
        let address = server.address;
        let observed_failure = std::cell::Cell::new(false);
        let observed = &observed_failure;
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            // Move ownership into the unwinding scope, as in the async parent test.
            let server = server;
            let executable = if mode == "spawn-error" {
                std::env::current_exe().unwrap().join("nonexistent-child")
            } else {
                std::env::current_exe().unwrap()
            };
            let output = std::process::Command::new(executable)
                .env_clear()
                .env(
                    CHILD,
                    if mode == "after-connect" {
                        address.to_string()
                    } else {
                        "before-connect".into()
                    },
                )
                .args([
                    "--exact",
                    "handoff_fault_server_reclaims_socket_after_child_failure",
                    "--nocapture",
                ])
                .output();
            eprintln!(
                "failure probe mode={mode} spawn={:?}",
                output.as_ref().map(|out| out.status)
            );
            if mode == "spawn-error" {
                assert!(
                    output
                        .as_ref()
                        .is_err_and(|error| error.kind() == std::io::ErrorKind::NotADirectory)
                );
                observed.set(true);
            }
            let output = output.expect("intentional child startup failure");
            eprintln!("{}", String::from_utf8_lossy(&output.stdout));
            eprintln!("{}", String::from_utf8_lossy(&output.stderr));
            if mode == "after-connect" {
                server.request.recv_timeout(Duration::from_secs(1)).unwrap();
            }
            assert_eq!(output.status.code(), Some(101));
            assert!(
                String::from_utf8_lossy(&output.stderr)
                    .contains("intentional handoff child failure")
            );
            observed.set(true);
            assert!(
                output.status.success(),
                "intentional parent assertion on child failure"
            );
        }));
        assert!(failure.is_err(), "probe must really unwind");
        assert!(
            observed_failure.get(),
            "only the intended failure may satisfy the probe"
        );
        let rebound = std::net::TcpListener::bind(address).expect("Drop released fault socket");
        eprintln!(
            "failure probe mode={mode} unwind observed; socket rebound={}",
            rebound.local_addr().unwrap()
        );
    }
}

#[tokio::test]
async fn cleanup_unconfirmed_handoff_keeps_staging_blob_and_tracker() {
    let Some(pool) = support::connect_postgres_contract("unconfirmed cleanup handoff").await else {
        return;
    };
    require_local_objects();
    if let Ok(mode) = std::env::var("KB_CLEANUP_HANDOFF_CHILD") {
        let ids: Vec<Uuid> = std::env::var("KB_CLEANUP_STAGING_IDS")
            .unwrap()
            .split(',')
            .map(|id| Uuid::parse_str(id).unwrap())
            .collect();
        assert_eq!(ids.len(), 2);
        let tracker = platform::StagedObjectCleanupTracker::new(&pool);
        for id in &ids {
            let bytes = format!("unconfirmed cleanup {id}").into_bytes();
            let digest = platform::sha256_hex(&bytes);
            platform::stage_object_upload(
                &pool,
                *id,
                &platform::object_ref(&digest),
                &digest,
                "application/octet-stream",
                bytes.len() as i64,
                None,
            )
            .await
            .unwrap();
            platform::write_blob_async(&digest, &bytes).await.unwrap();
            tracker.register(*id);
        }
        let deadline = tokio::time::Instant::now() + Duration::from_millis(200);
        let result = tokio::time::timeout_at(deadline, tracker.cleanup_pending()).await;
        match mode.as_str() {
            "configuration-error" => {
                let error = result.unwrap().unwrap_err();
                assert!(
                    error.contains(&ids[0].to_string()),
                    "first identity must fail first: {error}"
                );
            }
            "unresponsive-redis" => assert!(
                result.is_err(),
                "unconfirmed handoff must exhaust only its absolute deadline"
            ),
            _ => panic!("unknown handoff fixture mode"),
        }
        assert_eq!(
            tracker.pending_staging_ids(),
            ids,
            "failure/cancellation must retain both identities in order"
        );
        for id in &ids {
            let retained: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM object_upload_staging WHERE id=$1)",
            )
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
            assert!(retained);
            let bytes = format!("unconfirmed cleanup {id}").into_bytes();
            assert_eq!(
                std::fs::read(
                    platform::blob_path(&platform::sha256_hex(&bytes))
                        .expect("valid configured test object path")
                )
                .unwrap(),
                bytes
            );
        }
        eprintln!("unconfirmed mode={mode} retained staging={ids:?} tracker=2 blobs=true");
        return;
    }
    let mut test_lock = acquire_delivery_test_lock(&pool).await;
    // Each subprocess owns its environment, so the normal suite's real Redis is never changed.
    let server = HandoffFaultServer::start().unwrap();
    let unresponsive = format!("redis://{}/", server.address);
    let mut staged = Vec::new();
    for (mode, redis) in [
        ("configuration-error", "not-a-redis-url"),
        ("unresponsive-redis", unresponsive.as_str()),
    ] {
        let ids = [Uuid::new_v4(), Uuid::new_v4()];
        staged.extend(ids);
        let mut child = std::process::Command::new(std::env::current_exe().unwrap());
        child
            .env_clear()
            .args([
                "--exact",
                "cleanup_unconfirmed_handoff_keeps_staging_blob_and_tracker",
                "--nocapture",
            ])
            .env("KB_CLEANUP_STAGING_IDS", format!("{},{}", ids[0], ids[1]))
            .env("KB_CLEANUP_HANDOFF_CHILD", mode)
            .env("REDIS_URL", redis);
        for key in [
            "KNOWLEDGEBRAIN_TEST_DATABASE_URL",
            "KNOWLEDGEBRAIN_REQUIRE_POSTGRES_TESTS",
            "OBJECT_DIR",
            "KB_DEPLOYMENT_NAMESPACE_ID",
        ] {
            child.env(
                key,
                std::env::var_os(key).expect("explicit isolated child configuration"),
            );
        }
        let output = tokio::task::spawn_blocking(move || child.output())
            .await
            .unwrap()
            .unwrap();
        eprintln!("{}", String::from_utf8_lossy(&output.stdout));
        eprintln!("{}", String::from_utf8_lossy(&output.stderr));
        assert!(
            output.status.success(),
            "unconfirmed handoff child {mode} failed: {}",
            output.status
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed; 0 ignored;"));
    }
    server
        .request
        .recv_timeout(Duration::from_secs(1))
        .expect("Redis request actually arrived before cancellation");
    server.finish();
    let tracker = platform::StagedObjectCleanupTracker::new(&pool);
    for id in &staged {
        tracker.register(*id);
    }
    tokio::time::timeout(Duration::from_secs(5), tracker.cleanup_pending())
        .await
        .unwrap()
        .unwrap();
    assert!(!tracker.has_pending());
    let consumer = start_retention_consumer().await;
    for id in staged {
        let bytes = format!("unconfirmed cleanup {id}").into_bytes();
        wait_for_deleted(&pool, id, &platform::sha256_hex(&bytes), bytes.len() as i64)
            .await
            .expect("explicit replay must recover the retained staging identity");
    }
    consumer.finish().await;
    release_delivery_test_lock(&mut test_lock).await;
}
