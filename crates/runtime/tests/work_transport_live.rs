use runtime::{
    BID_DELIVERY_V1_PAYLOAD_VERSION, BID_DELIVERY_V1_TASK_TYPE, BidConvertV1Queue,
    BidDeliveryV1Handler, BidDeliveryV1Job, BidDeliveryV1WorkerAdapter, DeliverySpec,
    ObservedBidDeliveryV1, OxanaStableAdapter, TransportErrorClass, TransportOutcome,
    WorkTransport, WorkTransportReadiness, prepare_bid_delivery_v1,
};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
    time::Instant,
};
use tokio::sync::Notify;
use uuid::Uuid;

fn redis_tests_required() -> bool {
    std::env::var("KNOWLEDGEBRAIN_REQUIRE_REDIS_TESTS")
        .map(|value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

async fn live_storage(test_name: &str) -> Option<oxana::Storage> {
    if std::env::var_os("REDIS_URL").is_none() && !redis_tests_required() {
        eprintln!("skip {test_name}: REDIS_URL is not configured");
        return None;
    }

    let namespace = format!("oxanus:work-transport-live:{}", Uuid::new_v4());
    let storage = oxana::Storage::builder()
        .namespace(namespace)
        .build_from_redis_url(runtime::redis_url())
        .unwrap_or_else(|error| {
            panic!("required live work transport test could not configure Redis: {error}")
        });
    match storage.enqueued_count(BidConvertV1Queue).await {
        Ok(_) => Some(storage),
        Err(error) if redis_tests_required() => {
            panic!("required live work transport test could not reach Redis: {error}")
        }
        Err(error) => {
            eprintln!("skip {test_name}: Redis is unavailable ({error})");
            None
        }
    }
}

fn prepared_delivery(dispatch_id: Uuid) -> runtime::PreparedDelivery {
    prepare_bid_delivery_v1(DeliverySpec {
        physical_lane: "bid-convert-v1".to_string(),
        task_type: BID_DELIVERY_V1_TASK_TYPE.to_string(),
        dispatch_id,
        payload_version: BID_DELIVERY_V1_PAYLOAD_VERSION,
    })
    .expect("valid delivery spec")
}

async fn cleanup_namespace(storage: &oxana::Storage) {
    let client = redis::Client::open(runtime::redis_url()).expect("configure cleanup client");
    let mut connection = client
        .get_multiplexed_async_connection()
        .await
        .expect("connect cleanup client");
    let pattern = format!("{}:*", storage.namespace());
    let keys: Vec<String> = redis::cmd("KEYS")
        .arg(&pattern)
        .query_async(&mut connection)
        .await
        .expect("list test namespace keys");
    if !keys.is_empty() {
        let _: i64 = redis::cmd("DEL")
            .arg(keys)
            .query_async(&mut connection)
            .await
            .expect("delete test namespace keys");
    }
    let remaining: Vec<String> = redis::cmd("KEYS")
        .arg(pattern)
        .query_async(&mut connection)
        .await
        .expect("verify test namespace cleanup");
    assert!(remaining.is_empty(), "test namespace must be empty");
}

#[tokio::test]
async fn stable_adapter_classifies_unreachable_redis_as_indeterminate_once() {
    let storage = oxana::Storage::builder()
        .namespace(format!(
            "oxanus:work-transport-unreachable:{}",
            Uuid::new_v4()
        ))
        .build_from_redis_url("redis://127.0.0.1:1/")
        .expect("configure unreachable Redis storage");
    let transport = OxanaStableAdapter::new(storage).expect("registry closure");
    let prepared = prepared_delivery(Uuid::new_v4());
    let deadline = Instant::now() + Duration::from_secs(2);

    assert_eq!(
        transport.offer(&prepared, deadline).await,
        TransportOutcome::Indeterminate {
            error_class: TransportErrorClass::RedisUnavailable
        }
    );
    assert!(
        Instant::now() < deadline,
        "connection refusal must beat the hard deadline"
    );
    assert_eq!(transport.readiness(), WorkTransportReadiness::Degraded);
    let metrics = transport.metrics_snapshot();
    assert_eq!(metrics.offers_created, 1);
    assert_eq!(metrics.offers_started, 1);
    assert_eq!(metrics.enqueue_attempts, 1);
    assert_eq!(metrics.indeterminate, 1);
    assert_eq!(metrics.redis_unavailable, 1);
    assert_eq!(metrics.timeout_after_start, 0);
}

#[tokio::test]
async fn stable_adapter_uses_storage_enqueue_unique_skip_and_resurrect_metadata() {
    let Some(storage) =
        live_storage("stable_adapter_uses_storage_enqueue_unique_skip_and_resurrect_metadata")
            .await
    else {
        return;
    };
    let dispatch_id = Uuid::new_v4();
    let prepared = prepared_delivery(dispatch_id);
    let expected_job_id = prepared.expected_job_id().to_string();
    let transport = OxanaStableAdapter::new(storage.clone()).expect("registry closure");

    assert_eq!(
        transport
            .offer(&prepared, Instant::now() + Duration::from_secs(5))
            .await,
        TransportOutcome::Returned {
            job_id: expected_job_id.clone()
        }
    );
    assert_eq!(
        storage
            .enqueued_count(BidConvertV1Queue)
            .await
            .expect("count queue after first enqueue"),
        1
    );
    let queued = storage
        .list_queue_jobs(
            BidConvertV1Queue,
            &oxana::QueueListOpts {
                count: 1,
                offset: 0,
            },
        )
        .await
        .expect("list queue after enqueue");
    let envelope = queued.first().expect("enqueued delivery envelope");
    assert_eq!(envelope.id, expected_job_id);
    assert!(envelope.meta.resurrect);
    assert_eq!(envelope.job.name, BID_DELIVERY_V1_TASK_TYPE);
    assert_eq!(
        envelope.job.args,
        serde_json::json!({
            "dispatch_id": dispatch_id,
            "payload_version": BID_DELIVERY_V1_PAYLOAD_VERSION
        })
    );

    assert_eq!(
        transport
            .offer(&prepared, Instant::now() + Duration::from_secs(5))
            .await,
        TransportOutcome::Returned {
            job_id: expected_job_id
        }
    );
    assert_eq!(
        storage
            .enqueued_count(BidConvertV1Queue)
            .await
            .expect("count queue after Skip"),
        1
    );
    let diagnostics = transport
        .diagnostics_snapshot()
        .await
        .expect("public monitoring diagnostics");
    assert_eq!(diagnostics.convert_depth, 1);
    assert_eq!(diagnostics.dead_count, 0);
    assert_eq!(
        transport.readiness(),
        runtime::WorkTransportReadiness::Ready
    );
    cleanup_namespace(&storage).await;
}

#[tokio::test]
async fn oxana_native_resurrection_restores_dead_processing_membership() {
    let Some(storage) =
        live_storage("oxana_native_resurrection_restores_dead_processing_membership").await
    else {
        return;
    };
    let dispatch_id = Uuid::new_v4();
    let prepared = prepared_delivery(dispatch_id);
    let expected_job_id = prepared.expected_job_id().to_string();
    let transport = OxanaStableAdapter::new(storage.clone()).expect("registry closure");
    assert!(matches!(
        transport
            .offer(&prepared, Instant::now() + Duration::from_secs(5))
            .await,
        TransportOutcome::Returned { .. }
    ));

    let client = redis::Client::open(runtime::redis_url()).expect("configure raw Redis client");
    let mut connection = client
        .get_multiplexed_async_connection()
        .await
        .expect("connect raw Redis client");
    let queue_key = format!("{}:queue:bid-convert-v1", storage.namespace());
    let processing_key = format!(
        "{}:processing:dead-fixture-{dispatch_id}",
        storage.namespace()
    );
    let removed: i64 = redis::cmd("LREM")
        .arg(&queue_key)
        .arg(0)
        .arg(&expected_job_id)
        .query_async(&mut connection)
        .await
        .expect("move delivery out of queue");
    assert_eq!(removed, 1);
    let _: i64 = redis::cmd("LPUSH")
        .arg(&processing_key)
        .arg(&expected_job_id)
        .query_async(&mut connection)
        .await
        .expect("seed dead processing membership");

    let stop = Arc::new(Notify::new());
    let stop_runtime = stop.clone();
    let mut runtime_task = tokio::spawn(
        storage
            .clone()
            .runtime(())
            .resurrect_scan_interval(Duration::from_millis(20))
            .shutdown_on(async move {
                stop_runtime.notified().await;
                Ok(())
            })
            .run(),
    );
    let resurrection_result = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let position: Option<usize> = redis::cmd("LPOS")
                .arg(&queue_key)
                .arg(&expected_job_id)
                .query_async(&mut connection)
                .await
                .expect("observe native resurrection");
            if position.is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;

    stop.notify_waiters();
    let runtime_result = match tokio::time::timeout(Duration::from_secs(2), &mut runtime_task).await
    {
        Ok(Ok(Ok(_))) => Ok(()),
        Ok(Ok(Err(error))) => Err(format!("Oxana runtime failed: {error}")),
        Ok(Err(error)) => Err(format!("joining Oxana runtime failed: {error}")),
        Err(_) => {
            runtime_task.abort();
            let _ = runtime_task.await;
            Err("Oxana runtime did not stop within the deadline".to_string())
        }
    };
    let processing_members: Vec<String> = redis::cmd("LRANGE")
        .arg(&processing_key)
        .arg(0)
        .arg(-1)
        .query_async(&mut connection)
        .await
        .expect("verify dead processing list drained");
    let processing_drained = processing_members.is_empty();
    let diagnostics = transport
        .diagnostics_snapshot()
        .await
        .expect("public monitoring diagnostics");
    cleanup_namespace(&storage).await;
    resurrection_result.expect("Oxana native resurrection must restore membership");
    runtime_result.expect("Oxana runtime must stop cleanly");
    assert!(
        processing_drained,
        "dead processing membership must be drained"
    );
    assert!(diagnostics.resurrection_enabled);
}

#[derive(Clone)]
struct FailingWorkerContext {
    attempts: Arc<AtomicUsize>,
    observed_job_ids: Arc<Mutex<Vec<String>>>,
}

struct FailingHandler {
    attempts: Arc<AtomicUsize>,
    observed_job_ids: Arc<Mutex<Vec<String>>>,
}

impl oxana::FromContext<FailingWorkerContext> for FailingHandler {
    fn from_context(context: &FailingWorkerContext) -> Self {
        Self {
            attempts: context.attempts.clone(),
            observed_job_ids: context.observed_job_ids.clone(),
        }
    }
}

#[derive(Debug)]
struct FailingHandlerError;

impl std::fmt::Display for FailingHandlerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("expected live worker failure")
    }
}

impl std::error::Error for FailingHandlerError {}

#[async_trait::async_trait]
impl BidDeliveryV1Handler for FailingHandler {
    type Error = FailingHandlerError;

    async fn handle(&self, delivery: ObservedBidDeliveryV1) -> Result<(), Self::Error> {
        self.attempts.fetch_add(1, Ordering::SeqCst);
        self.observed_job_ids
            .lock()
            .expect("observed job ID lock poisoned")
            .push(delivery.observed_job_id);
        Err(FailingHandlerError)
    }
}

#[tokio::test]
async fn bid_delivery_worker_failure_is_attempted_once_and_moved_dead() {
    let Some(storage) =
        live_storage("bid_delivery_worker_failure_is_attempted_once_and_moved_dead").await
    else {
        return;
    };
    let attempts = Arc::new(AtomicUsize::new(0));
    let observed_job_ids = Arc::new(Mutex::new(Vec::new()));
    let context = FailingWorkerContext {
        attempts: attempts.clone(),
        observed_job_ids: observed_job_ids.clone(),
    };
    let prepared = prepared_delivery(Uuid::new_v4());
    let expected_job_id = prepared.expected_job_id().to_string();
    let transport = OxanaStableAdapter::new(storage.clone()).expect("registry closure");
    assert!(matches!(
        transport
            .offer(&prepared, Instant::now() + Duration::from_secs(5))
            .await,
        TransportOutcome::Returned { .. }
    ));

    tokio::time::timeout(
        Duration::from_secs(3),
        storage
            .clone()
            .runtime(context.clone())
            .queue::<BidConvertV1Queue>()
            .worker::<BidDeliveryV1WorkerAdapter<FailingHandler>, BidDeliveryV1Job>()
            .exit_when_processed(1)
            .run(),
    )
    .await
    .expect("worker must process the delivery")
    .expect("worker runtime must stop");
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    assert_eq!(
        observed_job_ids
            .lock()
            .expect("observed job ID lock poisoned")
            .as_slice(),
        [expected_job_id.as_str()],
        "worker must forward JobContext.meta.id without rewriting it"
    );
    assert_eq!(storage.dead_count().await.expect("dead count"), 1);
    assert_eq!(
        storage
            .enqueued_count(BidConvertV1Queue)
            .await
            .expect("queue count"),
        0
    );

    storage
        .clone()
        .runtime(context)
        .queue::<BidConvertV1Queue>()
        .worker::<BidDeliveryV1WorkerAdapter<FailingHandler>, BidDeliveryV1Job>()
        .shutdown_on(async {
            tokio::time::sleep(Duration::from_millis(200)).await;
            Ok(())
        })
        .run()
        .await
        .expect("second runtime must stop");
    let diagnostics = transport
        .diagnostics_snapshot()
        .await
        .expect("public monitoring diagnostics");
    let final_attempts = attempts.load(Ordering::SeqCst);
    cleanup_namespace(&storage).await;
    assert_eq!(final_attempts, 1, "dead job must not be processed twice");
    assert_eq!(diagnostics.dead_count, 1);
}
