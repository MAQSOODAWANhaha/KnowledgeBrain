//! Real Redis Oxana native fault coverage: unique Skip, crash resurrection, dead revive.
//! Local jobs only; does not start the production DocumentProcess worker.

use platform::{HousekeepJob, LowQueue};
use serde::{Deserialize, Serialize};
use std::future;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};
use uuid::Uuid;

const REQUIRE_ENV: &str = "KNOWLEDGEBRAIN_REQUIRE_REDIS_TESTS";
const CHILD_ENV: &str = "OXANA_NATIVE_FAULTS_CHILD";
const NAMESPACE_ENV: &str = "OXANA_NATIVE_FAULTS_NAMESPACE";
const REDIS_ENV: &str = "REDIS_URL";

fn redis_tests_required() -> bool {
    std::env::var(REQUIRE_ENV)
        .map(|value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

fn redis_url_from_env() -> Option<String> {
    match std::env::var(REDIS_ENV) {
        Ok(url) if !url.trim().is_empty() => Some(url),
        _ if redis_tests_required() => panic!("{REQUIRE_ENV}=1 requires {REDIS_ENV}"),
        _ => {
            eprintln!("skip oxana native faults: {REDIS_ENV} is not configured");
            None
        }
    }
}

fn child_mode() -> Option<String> {
    std::env::var(CHILD_ENV)
        .ok()
        .filter(|value| !value.is_empty())
}

fn storage_from(url: &str, namespace: &str) -> oxana::Storage {
    platform::oxana_storage_in_namespace(url, namespace.parse().expect("test deployment UUID"))
        .unwrap_or_else(|error| panic!("oxana storage from redis url: {error}"))
}

async fn connect_storage(test_name: &str) -> Option<(oxana::Storage, String, String)> {
    if child_mode().is_some() {
        let url = std::env::var(REDIS_ENV).expect("child needs REDIS_URL");
        let namespace = std::env::var(NAMESPACE_ENV).expect("child needs namespace");
        return Some((storage_from(&url, &namespace), url, namespace));
    }
    let url = redis_url_from_env()?;
    let namespace = Uuid::new_v4().to_string();
    let storage = storage_from(&url, &namespace);
    match storage.enqueued_count(FaultQueue).await {
        Ok(_) => Some((storage, url, namespace)),
        Err(error) if redis_tests_required() => {
            panic!("required runtime test {test_name} redis is down: {error}");
        }
        Err(error) => {
            eprintln!("skip runtime test {test_name}: redis not up ({error})");
            None
        }
    }
}

#[derive(Debug, thiserror::Error)]
enum FaultError {
    #[error("{0}")]
    Message(&'static str),
}

#[derive(Debug, Clone, Serialize, Deserialize, oxana::Job)]
#[oxana(unique_id = "fault:skip:{key}", on_conflict = Skip)]
struct SkipJob {
    key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, oxana::Job)]
#[oxana(
    unique_id = "fault:resurrect:{key}",
    on_conflict = Skip,
    resurrect = true
)]
struct ResurrectJob {
    key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, oxana::Job)]
#[oxana(unique_id = "fault:dead:{key}", on_conflict = Skip)]
struct DeadJob {
    key: String,
}

#[derive(oxana::Queue)]
#[oxana(key = "oxana-native-faults", concurrency = 1)]
struct FaultQueue;

#[derive(oxana::Queue)]
#[oxana(key = "oxana-native-faults-scanner", concurrency = 1)]
struct ScannerQueue;

struct BlockResurrectWorker;

impl oxana::FromContext<()> for BlockResurrectWorker {
    fn from_context(_ctx: &()) -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl oxana::Worker<ResurrectJob> for BlockResurrectWorker {
    type Error = FaultError;

    async fn process(
        &self,
        _job: ResurrectJob,
        _ctx: &oxana::JobContext,
    ) -> Result<(), Self::Error> {
        future::pending::<()>().await;
        Ok(())
    }

    fn max_retries(&self, _job: &ResurrectJob) -> u32 {
        0
    }

    fn retry_delay(&self, _job: &ResurrectJob, _retries: u32) -> u64 {
        0
    }
}

struct BlockHousekeepWorker;

impl oxana::FromContext<()> for BlockHousekeepWorker {
    fn from_context(_ctx: &()) -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl oxana::Worker<HousekeepJob> for BlockHousekeepWorker {
    type Error = FaultError;

    async fn process(
        &self,
        _job: HousekeepJob,
        _ctx: &oxana::JobContext,
    ) -> Result<(), Self::Error> {
        future::pending::<()>().await;
        Ok(())
    }

    fn max_retries(&self, _job: &HousekeepJob) -> u32 {
        0
    }

    fn retry_delay(&self, _job: &HousekeepJob, _retries: u32) -> u64 {
        0
    }
}

#[derive(Clone)]
struct CountCtx {
    hits: Arc<AtomicU32>,
}

struct CountWorker {
    hits: Arc<AtomicU32>,
}

impl oxana::FromContext<CountCtx> for CountWorker {
    fn from_context(ctx: &CountCtx) -> Self {
        Self {
            hits: ctx.hits.clone(),
        }
    }
}

#[async_trait::async_trait]
impl oxana::Worker<DeadJob> for CountWorker {
    type Error = FaultError;

    async fn process(&self, _job: DeadJob, _ctx: &oxana::JobContext) -> Result<(), Self::Error> {
        let n = self.hits.fetch_add(1, Ordering::SeqCst) + 1;
        if n == 1 {
            Err(FaultError::Message("first consumption fails"))
        } else {
            Ok(())
        }
    }

    fn max_retries(&self, _job: &DeadJob) -> u32 {
        0
    }

    fn retry_delay(&self, _job: &DeadJob, _retries: u32) -> u64 {
        0
    }
}

async fn wait_until<F, Fut>(deadline: Instant, mut check: F, what: &str)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    while Instant::now() <= deadline {
        if check().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting for {what}");
}

fn spawn_child(
    mode: &str,
    test_name: &str,
    redis_url: &str,
    namespace: &str,
) -> std::process::Child {
    let exe = std::env::current_exe().expect("current test executable");
    Command::new(&exe)
        .env(CHILD_ENV, mode)
        .env(REDIS_ENV, redis_url)
        .env(NAMESPACE_ENV, namespace)
        .env("KB_DEPLOYMENT_NAMESPACE_ID", namespace)
        .env("RUST_TEST_THREADS", "1")
        .args(["--exact", test_name, "--nocapture"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap_or_else(|error| panic!("spawn child {mode} from {}: {error}", exe.display()))
}

fn kill_child(mut child: std::process::Child) {
    let _ = child.kill();
    let reap_by = Instant::now() + Duration::from_secs(2);
    while Instant::now() <= reap_by {
        if child.try_wait().expect("reap fault child").is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

async fn run_scanner(
    storage: oxana::Storage,
) -> (
    tokio::sync::oneshot::Sender<()>,
    tokio::task::JoinHandle<Result<oxana::RunStats, oxana::OxanaError>>,
) {
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let handle = tokio::spawn(async move {
        storage
            .runtime(())
            .queue::<ScannerQueue>()
            .heartbeat_interval(Duration::from_millis(100))
            .dead_process_threshold(Duration::from_millis(400))
            .resurrect_scan_interval(Duration::from_millis(200))
            .shutdown_on(async move {
                let _ = stop_rx.await;
                Ok(())
            })
            .run()
            .await
    });
    (stop_tx, handle)
}

async fn stop_scanner(
    stop_tx: tokio::sync::oneshot::Sender<()>,
    handle: tokio::task::JoinHandle<Result<oxana::RunStats, oxana::OxanaError>>,
) {
    let _ = stop_tx.send(());
    match tokio::time::timeout(Duration::from_secs(5), handle).await {
        Ok(_) => {}
        Err(_) => panic!("scanner runtime did not stop"),
    }
}

async fn run_blocking_resurrect_child(storage: oxana::Storage) {
    storage
        .runtime(())
        .queue::<FaultQueue>()
        .worker::<BlockResurrectWorker, ResurrectJob>()
        .heartbeat_interval(Duration::from_millis(100))
        .dead_process_threshold(Duration::from_secs(5))
        .dequeue_timeout(Duration::from_millis(200))
        .run()
        .await
        .expect("blocking resurrect child runtime");
}

async fn run_blocking_housekeep_child(storage: oxana::Storage) {
    storage
        .runtime(())
        .queue::<LowQueue>()
        .worker::<BlockHousekeepWorker, HousekeepJob>()
        .heartbeat_interval(Duration::from_millis(100))
        .dead_process_threshold(Duration::from_secs(5))
        .dequeue_timeout(Duration::from_millis(200))
        .run()
        .await
        .expect("blocking housekeep child runtime");
}

#[tokio::test(flavor = "multi_thread")]
async fn unique_id_skip_duplicate_returns_same_job_id() {
    if child_mode().is_some() {
        return;
    }
    let Some((storage, _, namespace)) = connect_storage("unique skip").await else {
        return;
    };
    assert!(!namespace.is_empty());
    let job = SkipJob {
        key: format!("skip-{namespace}"),
    };
    let first = storage
        .enqueue(FaultQueue, job.clone())
        .await
        .expect("first enqueue");
    let second = storage
        .enqueue(FaultQueue, job)
        .await
        .expect("duplicate enqueue");
    assert_eq!(second, first);
    assert_eq!(storage.enqueued_count(FaultQueue).await.expect("count"), 1);
    assert!(storage.get_job(&first).await.expect("get_job").is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn crash_resurrection_reenqueues_job_and_keeps_get_job() {
    if child_mode().as_deref() == Some("resurrect") {
        let (storage, _, _) = connect_storage("crash resurrection child")
            .await
            .expect("child storage");
        run_blocking_resurrect_child(storage).await;
        return;
    }
    let Some((storage, url, namespace)) = connect_storage("crash resurrection").await else {
        return;
    };
    let job_id = storage
        .enqueue(
            FaultQueue,
            ResurrectJob {
                key: format!("resurrect-{namespace}"),
            },
        )
        .await
        .expect("enqueue resurrect job");
    assert_eq!(storage.enqueued_count(FaultQueue).await.expect("count"), 1);
    let child = spawn_child(
        "resurrect",
        "crash_resurrection_reenqueues_job_and_keeps_get_job",
        &url,
        &namespace,
    );
    wait_until(
        Instant::now() + Duration::from_secs(8),
        || {
            let storage = storage.clone();
            let job_id = job_id.clone();
            async move {
                storage.enqueued_count(FaultQueue).await.ok() == Some(0)
                    && storage.get_job(&job_id).await.ok().flatten().is_some()
            }
        },
        "child to take resurrect job",
    )
    .await;
    kill_child(child);
    let (stop_tx, handle) = run_scanner(storage.clone()).await;
    wait_until(
        Instant::now() + Duration::from_secs(8),
        || {
            let storage = storage.clone();
            let job_id = job_id.clone();
            async move {
                storage.enqueued_count(FaultQueue).await.ok() == Some(1)
                    && storage.get_job(&job_id).await.ok().flatten().is_some()
            }
        },
        "resurrected job to rejoin consumable queue",
    )
    .await;
    assert!(storage.get_job(&job_id).await.expect("get_job").is_some());
    assert_eq!(storage.enqueued_count(FaultQueue).await.expect("count"), 1);
    let _ = storage.processes().await.expect("processes");
    stop_scanner(stop_tx, handle).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn housekeep_job_is_not_resurrected_after_dead_process() {
    if child_mode().as_deref() == Some("housekeep") {
        let (storage, _, _) = connect_storage("housekeep child")
            .await
            .expect("child storage");
        run_blocking_housekeep_child(storage).await;
        return;
    }
    let Some((storage, url, namespace)) = connect_storage("housekeep no resurrect").await else {
        return;
    };
    let job_id = storage
        .enqueue(LowQueue, HousekeepJob {})
        .await
        .expect("enqueue housekeep job");
    assert_eq!(storage.enqueued_count(LowQueue).await.expect("count"), 1);
    assert!(!<HousekeepJob as oxana::Job>::should_resurrect());
    let child = spawn_child(
        "housekeep",
        "housekeep_job_is_not_resurrected_after_dead_process",
        &url,
        &namespace,
    );
    wait_until(
        Instant::now() + Duration::from_secs(8),
        || {
            let storage = storage.clone();
            let job_id = job_id.clone();
            async move {
                storage.enqueued_count(LowQueue).await.ok() == Some(0)
                    && storage.get_job(&job_id).await.ok().flatten().is_some()
            }
        },
        "child to take housekeep job",
    )
    .await;
    kill_child(child);
    let (stop_tx, handle) = run_scanner(storage.clone()).await;
    wait_until(
        Instant::now() + Duration::from_secs(8),
        || {
            let storage = storage.clone();
            let job_id = job_id.clone();
            async move {
                storage.enqueued_count(LowQueue).await.ok() == Some(0)
                    && storage.get_job(&job_id).await.ok().flatten().is_none()
            }
        },
        "housekeep job to be dropped instead of requeued",
    )
    .await;
    assert!(storage.get_job(&job_id).await.expect("get_job").is_none());
    assert_eq!(storage.enqueued_count(LowQueue).await.expect("count"), 0);
    stop_scanner(stop_tx, handle).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn dead_revive_allows_reconsume_without_duplicate_agent() {
    if child_mode().is_some() {
        return;
    }
    let Some((storage, _, namespace)) = connect_storage("dead revive").await else {
        return;
    };
    let hits = Arc::new(AtomicU32::new(0));
    let job = DeadJob {
        key: format!("dead-{namespace}"),
    };
    let first = storage
        .enqueue(FaultQueue, job.clone())
        .await
        .expect("enqueue dead job");
    storage
        .runtime(CountCtx { hits: hits.clone() })
        .queue::<FaultQueue>()
        .worker::<CountWorker, DeadJob>()
        .exit_when_processed(1)
        .dequeue_timeout(Duration::from_millis(200))
        .run()
        .await
        .expect("first failing runtime");
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    assert_eq!(storage.dead_count().await.expect("dead"), 1);
    let queued_after_dead = storage.enqueued_count(FaultQueue).await.expect("count");
    let job_after_dead = storage.get_job(&first).await.expect("get_job");
    eprintln!(
        "dead_revive after kill queued={queued_after_dead} get_job={}",
        job_after_dead.is_some()
    );
    let dead = storage
        .list_dead(&oxana::QueueListOpts {
            count: 10,
            offset: 0,
        })
        .await
        .expect("list_dead");
    assert_eq!(dead.len(), 1);
    assert_eq!(dead[0].id, first);
    let duplicate = storage
        .enqueue(FaultQueue, job)
        .await
        .expect("duplicate enqueue while dead");
    assert_eq!(duplicate, first);
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    assert_eq!(storage.dead_count().await.expect("dead after skip"), 1);
    let queued_after_skip = storage.enqueued_count(FaultQueue).await.expect("count");
    eprintln!("dead_revive after duplicate skip queued={queued_after_skip}");
    let revived = storage.revive_all_dead().await.expect("revive_all_dead");
    assert_eq!(revived, 1);
    assert_eq!(storage.dead_count().await.expect("dead after revive"), 0);
    let queued_after_revive = storage.enqueued_count(FaultQueue).await.expect("count");
    eprintln!("dead_revive after revive queued={queued_after_revive}");
    assert_eq!(
        queued_after_revive, 1,
        "revive_all_dead must not create two consumable copies"
    );
    storage
        .runtime(CountCtx { hits: hits.clone() })
        .queue::<FaultQueue>()
        .worker::<CountWorker, DeadJob>()
        .exit_when_processed(1)
        .dequeue_timeout(Duration::from_millis(200))
        .run()
        .await
        .expect("second succeeding runtime");
    assert_eq!(hits.load(Ordering::SeqCst), 2);
    assert_eq!(storage.dead_count().await.expect("final dead"), 0);
}
