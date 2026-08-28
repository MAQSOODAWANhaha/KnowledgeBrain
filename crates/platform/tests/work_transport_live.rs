use platform::{
    BID_DELIVERY_V1_TASK_TYPE, BidDeliveryEnqueueOutcome, BidDeliveryEnqueuer,
    BidDeliveryTargetKind, BidDeliveryV1Handler, BidDeliveryV1Job, BidDeliveryV1Queue,
    BidDeliveryV1Worker,
};
use std::process::{Child, Command, Stdio};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;
use uuid::Uuid;

fn redis_tests_required() -> bool {
    std::env::var("KNOWLEDGEBRAIN_REQUIRE_REDIS_TESTS")
        .map(|value| matches!(value.as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false)
}

async fn live_storage(test_name: &str) -> Option<oxana::Storage> {
    if std::env::var_os("REDIS_URL").is_none() && !redis_tests_required() {
        eprintln!("skip {test_name}: REDIS_URL is not configured");
        return None;
    }

    let storage = oxana::Storage::builder()
        .namespace(format!("oxanus:bid-delivery-test:{}", Uuid::new_v4()))
        .build_from_redis_url(platform::redis_url())
        .expect("configure Redis storage");
    match storage.enqueued_count(BidDeliveryV1Queue).await {
        Ok(_) => Some(storage),
        Err(error) if redis_tests_required() => {
            panic!("required live Redis test could not connect: {error}")
        }
        Err(error) => {
            eprintln!("skip {test_name}: Redis is unavailable ({error})");
            None
        }
    }
}

async fn cleanup_namespace(storage: &oxana::Storage) {
    let client = redis::Client::open(platform::redis_url()).expect("configure cleanup client");
    let mut connection = client
        .get_multiplexed_async_connection()
        .await
        .expect("connect cleanup client");
    let pattern = format!("{}:*", storage.namespace());
    let keys: Vec<String> = redis::cmd("KEYS")
        .arg(&pattern)
        .query_async(&mut connection)
        .await
        .expect("list test keys");
    if !keys.is_empty() {
        let _: i64 = redis::cmd("DEL")
            .arg(keys)
            .query_async(&mut connection)
            .await
            .expect("delete test keys");
    }
    let remaining: Vec<String> = redis::cmd("KEYS")
        .arg(pattern)
        .query_async(&mut connection)
        .await
        .expect("verify cleanup");
    assert!(remaining.is_empty(), "test namespace must be empty");
}

#[tokio::test]
async fn unreachable_redis_is_indeterminate_after_one_enqueue_call() {
    let storage = oxana::Storage::builder()
        .namespace(format!(
            "oxanus:bid-delivery-unreachable:{}",
            Uuid::new_v4()
        ))
        .build_from_redis_url("redis://127.0.0.1:1/")
        .expect("configure unreachable Redis storage");
    let enqueuer = BidDeliveryEnqueuer::new(storage);

    assert!(matches!(
        enqueuer
            .enqueue(BidDeliveryTargetKind::DocumentConversion, Uuid::new_v4(), 1)
            .await,
        BidDeliveryEnqueueOutcome::Indeterminate { .. }
    ));
}

#[tokio::test]
async fn duplicate_delivery_uses_oxana_skip_and_keeps_one_job() {
    let Some(storage) = live_storage("duplicate_delivery_uses_oxana_skip_and_keeps_one_job").await
    else {
        return;
    };
    let target_id = Uuid::new_v4();
    let enqueuer = BidDeliveryEnqueuer::new(storage.clone());

    let first = enqueuer
        .enqueue(BidDeliveryTargetKind::DocumentConversion, target_id, 4)
        .await;
    let second = enqueuer
        .enqueue(BidDeliveryTargetKind::DocumentConversion, target_id, 4)
        .await;
    assert_eq!(first, second);
    let job_id = match first {
        BidDeliveryEnqueueOutcome::Accepted { job_id } => job_id,
        BidDeliveryEnqueueOutcome::Indeterminate { error } => panic!("enqueue failed: {error}"),
    };
    assert_eq!(
        job_id,
        format!("{BID_DELIVERY_V1_TASK_TYPE}/document_conversion:{target_id}:4")
    );
    assert_eq!(
        storage
            .enqueued_count(BidDeliveryV1Queue)
            .await
            .expect("queue count"),
        1
    );
    let queued = storage
        .list_queue_jobs(
            BidDeliveryV1Queue,
            &oxana::QueueListOpts {
                count: 1,
                offset: 0,
            },
        )
        .await
        .expect("list queued jobs");
    let envelope = queued.first().expect("queued delivery");
    assert_eq!(
        envelope.job.args,
        serde_json::json!({
            "target_kind": "document_conversion",
            "target_id": target_id,
            "target_revision": 4
        })
    );
    assert!(envelope.meta.resurrect);
    assert_eq!(
        envelope.meta.on_conflict,
        Some(oxana::JobConflictStrategy::Skip)
    );
    cleanup_namespace(&storage).await;
}

#[derive(Clone)]
struct FailingContext {
    attempts: Arc<AtomicUsize>,
    observed: Arc<Mutex<Vec<BidDeliveryV1Job>>>,
}

struct FailingHandler(FailingContext);

impl oxana::FromContext<FailingContext> for FailingHandler {
    fn from_context(context: &FailingContext) -> Self {
        Self(context.clone())
    }
}

#[derive(Debug)]
struct ExpectedFailure;

impl std::fmt::Display for ExpectedFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("expected failure")
    }
}

impl std::error::Error for ExpectedFailure {}

#[async_trait::async_trait]
impl BidDeliveryV1Handler for FailingHandler {
    type Error = ExpectedFailure;

    async fn handle(&self, delivery: BidDeliveryV1Job) -> Result<(), Self::Error> {
        self.0.attempts.fetch_add(1, Ordering::SeqCst);
        self.0
            .observed
            .lock()
            .expect("observed delivery lock")
            .push(delivery);
        Err(ExpectedFailure)
    }
}

struct SuccessfulHandler(Arc<AtomicUsize>);

impl oxana::FromContext<Arc<AtomicUsize>> for SuccessfulHandler {
    fn from_context(context: &Arc<AtomicUsize>) -> Self {
        Self(Arc::clone(context))
    }
}

#[async_trait::async_trait]
impl BidDeliveryV1Handler for SuccessfulHandler {
    type Error = ExpectedFailure;

    async fn handle(&self, _delivery: BidDeliveryV1Job) -> Result<(), Self::Error> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[tokio::test]
async fn handler_failure_is_retried_three_times_by_oxana() {
    let Some(storage) = live_storage("handler_failure_is_retried_three_times_by_oxana").await
    else {
        return;
    };
    let context = FailingContext {
        attempts: Arc::new(AtomicUsize::new(0)),
        observed: Arc::new(Mutex::new(Vec::new())),
    };
    let target_id = Uuid::new_v4();
    let enqueuer = BidDeliveryEnqueuer::new(storage.clone());
    let accepted = enqueuer
        .enqueue(BidDeliveryTargetKind::DocumentConversion, target_id, 9)
        .await;
    let _job_id = match accepted {
        BidDeliveryEnqueueOutcome::Accepted { job_id } => job_id,
        BidDeliveryEnqueueOutcome::Indeterminate { error } => panic!("enqueue failed: {error}"),
    };

    tokio::time::timeout(
        Duration::from_secs(45),
        storage
            .clone()
            .runtime(context.clone())
            .queue::<BidDeliveryV1Queue>()
            .worker::<BidDeliveryV1Worker<FailingHandler>, platform::BidDeliveryV1Job>()
            .dequeue_timeout(Duration::from_millis(20))
            .exit_when_processed(4)
            .run(),
    )
    .await
    .expect("four executions must finish")
    .expect("Oxana runtime must stop");

    assert_eq!(context.attempts.load(Ordering::SeqCst), 4);
    {
        let observed = context.observed.lock().expect("observed delivery lock");
        assert_eq!(observed.len(), 4);
        assert!(observed.iter().all(|delivery| {
            delivery.target_kind == BidDeliveryTargetKind::DocumentConversion
                && delivery.target_id == target_id
                && delivery.target_revision == 9
        }));
    }
    assert_eq!(storage.dead_count().await.expect("dead count"), 1);
    let dead = storage
        .list_dead(&oxana::QueueListOpts {
            count: 1,
            offset: 0,
        })
        .await
        .expect("list dead jobs");
    let dead_envelope = dead.first().expect("dead delivery").clone();
    assert_eq!(dead_envelope.meta.retries, 3);

    assert_eq!(
        storage.revive_all_dead().await.expect("revive dead jobs"),
        1
    );
    assert_eq!(
        storage.dead_count().await.expect("dead count after revive"),
        0
    );
    let revived = storage
        .list_queue_jobs(
            BidDeliveryV1Queue,
            &oxana::QueueListOpts {
                count: 1,
                offset: 0,
            },
        )
        .await
        .expect("list revived jobs");
    let revived_envelope = revived.first().expect("revived delivery");
    assert_eq!(revived_envelope.id, dead_envelope.id);
    assert_eq!(revived_envelope.job.name, dead_envelope.job.name);
    assert_eq!(revived_envelope.job.args, dead_envelope.job.args);
    assert_eq!(revived_envelope.meta.retries, dead_envelope.meta.retries);
    let revived_attempts = Arc::new(AtomicUsize::new(0));
    storage
        .clone()
        .runtime(Arc::clone(&revived_attempts))
        .queue::<BidDeliveryV1Queue>()
        .worker::<BidDeliveryV1Worker<SuccessfulHandler>, BidDeliveryV1Job>()
        .dequeue_timeout(Duration::from_millis(20))
        .exit_when_processed(1)
        .run()
        .await
        .expect("revived delivery must execute");
    assert_eq!(revived_attempts.load(Ordering::SeqCst), 1);
    assert_eq!(storage.dead_count().await.expect("dead count"), 0);
    cleanup_namespace(&storage).await;
}

struct PendingHandler;

impl oxana::FromContext<()> for PendingHandler {
    fn from_context(_context: &()) -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl BidDeliveryV1Handler for PendingHandler {
    type Error = ExpectedFailure;

    async fn handle(&self, _delivery: BidDeliveryV1Job) -> Result<(), Self::Error> {
        std::future::pending::<Result<(), ExpectedFailure>>().await
    }
}

struct ChildProcessGuard(Option<Child>);

impl ChildProcessGuard {
    fn try_status(&mut self) -> Option<std::process::ExitStatus> {
        self.0
            .as_mut()
            .and_then(|child| child.try_wait().expect("inspect crash helper process"))
    }

    fn terminate(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for ChildProcessGuard {
    fn drop(&mut self) {
        self.terminate();
    }
}

#[tokio::test]
async fn crash_worker_process_helper() {
    let Ok(namespace) = std::env::var("KNOWLEDGEBRAIN_OXANA_CRASH_HELPER_NAMESPACE") else {
        return;
    };
    let storage = oxana::Storage::builder()
        .namespace(namespace)
        .build_from_redis_url(platform::redis_url())
        .expect("configure crash helper Redis storage");
    storage
        .runtime(())
        .queue::<BidDeliveryV1Queue>()
        .worker::<BidDeliveryV1Worker<PendingHandler>, BidDeliveryV1Job>()
        .heartbeat_interval(Duration::from_millis(20))
        .dead_process_threshold(Duration::from_millis(100))
        .resurrect_scan_interval(Duration::from_millis(20))
        .dequeue_timeout(Duration::from_millis(20))
        .run()
        .await
        .expect("crash helper runtime must run until killed");
}

async fn wait_until_processing(
    storage: &oxana::Storage,
    job_id: &str,
    crash_helper: &mut ChildProcessGuard,
) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            assert!(
                crash_helper.try_status().is_none(),
                "crash helper exited before claiming the delivery"
            );
            let stats = storage.stats().await.expect("read Oxana stats");
            if stats
                .processing
                .iter()
                .any(|processing| processing.job_envelope.id == job_id)
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("crash helper must claim the delivery");
}

fn spawn_crash_helper(namespace: &str) -> ChildProcessGuard {
    let child = Command::new(std::env::current_exe().expect("resolve current test executable"))
        .arg("crash_worker_process_helper")
        .arg("--exact")
        .arg("--nocapture")
        .arg("--test-threads=1")
        .env("KNOWLEDGEBRAIN_OXANA_CRASH_HELPER_NAMESPACE", namespace)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn crash helper process");
    ChildProcessGuard(Some(child))
}

#[tokio::test]
async fn in_flight_delivery_is_resurrected_after_worker_crash() {
    let Some(storage) = live_storage("in_flight_delivery_is_resurrected_after_worker_crash").await
    else {
        return;
    };
    let enqueuer = BidDeliveryEnqueuer::new(storage.clone());
    let target_id = Uuid::new_v4();
    let job_id = match enqueuer
        .enqueue(BidDeliveryTargetKind::DocumentConversion, target_id, 11)
        .await
    {
        BidDeliveryEnqueueOutcome::Accepted { job_id } => job_id,
        BidDeliveryEnqueueOutcome::Indeterminate { error } => panic!("enqueue failed: {error}"),
    };

    let mut crash_helper = spawn_crash_helper(storage.namespace());
    wait_until_processing(&storage, &job_id, &mut crash_helper).await;
    crash_helper.terminate();

    let attempts = Arc::new(AtomicUsize::new(0));
    let second_runtime = storage
        .clone()
        .runtime(Arc::clone(&attempts))
        .queue::<BidDeliveryV1Queue>()
        .worker::<BidDeliveryV1Worker<SuccessfulHandler>, BidDeliveryV1Job>()
        .heartbeat_interval(Duration::from_millis(20))
        .dead_process_threshold(Duration::from_millis(100))
        .resurrect_scan_interval(Duration::from_millis(20))
        .dequeue_timeout(Duration::from_millis(20))
        .exit_when_processed(1);
    tokio::time::timeout(Duration::from_secs(5), second_runtime.run())
        .await
        .expect("resurrected delivery must finish")
        .expect("second Oxana runtime must stop");

    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    assert_eq!(storage.dead_count().await.expect("dead count"), 0);
    assert_eq!(
        storage
            .enqueued_count(BidDeliveryV1Queue)
            .await
            .expect("queue count"),
        0
    );
    cleanup_namespace(&storage).await;
}
