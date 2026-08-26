use oxana::Worker as _;
use oxana::{Job as _, JobConflictStrategy};
use runtime::{
    BID_DELIVERY_V1_MAX_OBSERVED_JOB_ID_BYTES, BID_DELIVERY_V1_PAYLOAD_VERSION,
    BID_DELIVERY_V1_TASK_TYPE, BidDeliveryPayloadError, BidDeliveryV1Handler, BidDeliveryV1Job,
    BidDeliveryV1WorkerAdapter, DeliverySpec, ObservedBidDeliveryV1, PrepareDeliveryError,
    RecordingResponse, RecordingTransport, TransportErrorClass, TransportOutcome, WorkTransport,
    WorkTransportReadiness, prepare_bid_delivery_v1, validate_bid_delivery_v1_registry,
    verify_bid_delivery_v1_payload,
};
use std::time::{Duration, Instant};
use uuid::Uuid;

fn fixed_dispatch_id() -> Uuid {
    Uuid::parse_str("00112233-4455-6677-8899-aabbccddeeff").expect("fixed UUID")
}

#[test]
fn bid_delivery_v1_prepare_matches_frozen_golden() {
    let dispatch_id = fixed_dispatch_id();
    let prepared = prepare_bid_delivery_v1(DeliverySpec {
        physical_lane: "bid-convert-v1".to_string(),
        task_type: BID_DELIVERY_V1_TASK_TYPE.to_string(),
        dispatch_id,
        payload_version: BID_DELIVERY_V1_PAYLOAD_VERSION,
    })
    .expect("valid delivery spec");

    assert_eq!(
        prepared.expected_job_id(),
        "bid:delivery:v1/00112233-4455-6677-8899-aabbccddeeff"
    );
    assert_eq!(
        prepared.canonical_payload_bytes(),
        &hex_literal("4b42444c000100112233445566778899aabbccddeeff0001")
    );
    assert_eq!(
        prepared.canonical_payload_sha256(),
        "0f6d6ca6c990e92ed161984e3bc85dcaf440bd51dfe7e0b010173304cefdea77"
    );
    assert!(prepared.resurrect());
    assert_eq!(prepared.on_conflict(), JobConflictStrategy::Skip);

    let job = BidDeliveryV1Job::new(dispatch_id);
    assert_eq!(BidDeliveryV1Job::name(), BID_DELIVERY_V1_TASK_TYPE);
    assert_eq!(
        job.unique_id().as_deref(),
        Some("00112233-4455-6677-8899-aabbccddeeff")
    );
    assert_eq!(job.on_conflict(), JobConflictStrategy::Skip);
    assert!(BidDeliveryV1Job::should_resurrect());
}

#[test]
fn bid_delivery_v1_prepare_rejects_contract_drift() {
    for lane in [
        "bid-convert-v1",
        "bid-extract-v1",
        "bid-matching-v1",
        "bid-render-v1",
    ] {
        let mut spec = valid_spec();
        spec.physical_lane = lane.to_string();
        prepare_bid_delivery_v1(spec).expect("published lane must be accepted");
    }

    let mut spec = valid_spec();
    spec.physical_lane = "unknown".to_string();
    assert_eq!(
        prepare_bid_delivery_v1(spec).expect_err("unknown lane must fail"),
        PrepareDeliveryError::AdapterMismatch("physical_lane")
    );

    let mut spec = valid_spec();
    spec.task_type = "bid:delivery:v2".to_string();
    assert_eq!(
        prepare_bid_delivery_v1(spec).expect_err("unknown task must fail"),
        PrepareDeliveryError::PayloadRejected("task_type")
    );

    let mut spec = valid_spec();
    spec.payload_version = 2;
    assert_eq!(
        prepare_bid_delivery_v1(spec).expect_err("unknown payload version must fail"),
        PrepareDeliveryError::PayloadRejected("payload_version")
    );
}

#[tokio::test]
async fn recording_transport_observes_zero_or_one_enqueue_per_offer() {
    let prepared = prepare_bid_delivery_v1(valid_spec()).expect("valid delivery spec");

    let returned = RecordingTransport::new(RecordingResponse::ReturnExpected);
    assert_eq!(
        returned
            .offer(&prepared, Instant::now() + Duration::from_secs(1))
            .await,
        TransportOutcome::Returned {
            job_id: prepared.expected_job_id().to_string()
        }
    );
    assert_eq!(returned.enqueue_count(), 1);

    let mismatch = RecordingTransport::new(RecordingResponse::ReturnJobId(
        "bid:delivery:v1/not-the-dispatch".to_string(),
    ));
    assert_eq!(mismatch.readiness(), WorkTransportReadiness::Ready);
    assert_eq!(
        mismatch
            .offer(&prepared, Instant::now() + Duration::from_secs(1))
            .await,
        TransportOutcome::ReturnedJobIdMismatch {
            actual_job_id: runtime::BoundedJobIdObservation::new(
                "bid:delivery:v1/not-the-dispatch"
            )
        }
    );
    assert_eq!(mismatch.enqueue_count(), 1);

    let oversized_value = "x".repeat(BID_DELIVERY_V1_MAX_OBSERVED_JOB_ID_BYTES + 100);
    let oversized =
        RecordingTransport::new(RecordingResponse::ReturnJobId(oversized_value.clone()));
    let TransportOutcome::ReturnedJobIdMismatch { actual_job_id } = oversized
        .offer(&prepared, Instant::now() + Duration::from_secs(1))
        .await
    else {
        panic!("oversized returned ID must be a mismatch");
    };
    assert!(actual_job_id.truncated());
    assert_eq!(actual_job_id.original_byte_len(), oversized_value.len());
    assert!(actual_job_id.value().len() <= BID_DELIVERY_V1_MAX_OBSERVED_JOB_ID_BYTES);
    assert_eq!(actual_job_id.sha256().len(), 64);
    assert_eq!(oversized.readiness(), WorkTransportReadiness::FailedClosed);
    assert_eq!(mismatch.readiness(), WorkTransportReadiness::FailedClosed);
    assert_eq!(
        mismatch
            .offer(&prepared, Instant::now() + Duration::from_secs(1))
            .await,
        TransportOutcome::Indeterminate {
            error_class: TransportErrorClass::AdapterMismatch
        }
    );
    assert_eq!(mismatch.enqueue_count(), 1);

    let failed = RecordingTransport::new(RecordingResponse::Error(
        TransportErrorClass::RedisUnavailable,
    ));
    assert_eq!(
        failed
            .offer(&prepared, Instant::now() + Duration::from_secs(1))
            .await,
        TransportOutcome::Indeterminate {
            error_class: TransportErrorClass::RedisUnavailable
        }
    );
    assert_eq!(failed.enqueue_count(), 1);

    let expired = RecordingTransport::new(RecordingResponse::ReturnExpected);
    assert_eq!(
        expired.offer(&prepared, Instant::now()).await,
        TransportOutcome::Indeterminate {
            error_class: TransportErrorClass::DeadlineExceeded
        }
    );
    assert_eq!(expired.enqueue_count(), 0);
    assert_eq!(expired.metrics_snapshot().deadline_before_enqueue, 1);
    assert_eq!(expired.readiness(), WorkTransportReadiness::Ready);

    let cancelled = RecordingTransport::new(RecordingResponse::ReturnExpected);
    let future = cancelled.offer(&prepared, Instant::now() + Duration::from_secs(1));
    drop(future);
    assert_eq!(cancelled.enqueue_count(), 0);
    assert_eq!(cancelled.metrics_snapshot().offers_created, 1);
    assert_eq!(cancelled.metrics_snapshot().offers_started, 0);

    let timed_out = RecordingTransport::new(RecordingResponse::Pending);
    assert_eq!(
        timed_out
            .offer(&prepared, Instant::now() + Duration::from_millis(1))
            .await,
        TransportOutcome::Indeterminate {
            error_class: TransportErrorClass::DeadlineExceeded
        }
    );
    assert_eq!(timed_out.enqueue_count(), 1);
    assert_eq!(timed_out.metrics_snapshot().timeout_after_start, 1);
    assert_eq!(timed_out.readiness(), WorkTransportReadiness::Degraded);
}

#[tokio::test]
async fn transport_metrics_drive_degraded_recovery_without_overriding_fatal() {
    let prepared = prepare_bid_delivery_v1(valid_spec()).expect("valid delivery spec");
    let transport = RecordingTransport::new(RecordingResponse::Error(
        TransportErrorClass::RedisUnavailable,
    ));
    assert_eq!(transport.readiness(), WorkTransportReadiness::Ready);
    transport
        .offer(&prepared, Instant::now() + Duration::from_secs(1))
        .await;
    assert_eq!(transport.readiness(), WorkTransportReadiness::Degraded);
    let degraded = transport.metrics_snapshot();
    assert_eq!(degraded.offers_created, 1);
    assert_eq!(degraded.enqueue_attempts, 1);
    assert_eq!(degraded.indeterminate, 1);
    assert_eq!(degraded.redis_unavailable, 1);
    assert!(degraded.total_latency_micros >= degraded.last_latency_micros);

    transport.set_response(RecordingResponse::ReturnExpected);
    transport
        .offer(&prepared, Instant::now() + Duration::from_secs(1))
        .await;
    assert_eq!(transport.readiness(), WorkTransportReadiness::Ready);
    assert_eq!(transport.metrics_snapshot().returned, 1);

    transport.set_response(RecordingResponse::ReturnJobId("wrong".to_string()));
    transport
        .offer(&prepared, Instant::now() + Duration::from_secs(1))
        .await;
    assert_eq!(transport.readiness(), WorkTransportReadiness::FailedClosed);
    transport.set_response(RecordingResponse::ReturnExpected);
    transport
        .offer(&prepared, Instant::now() + Duration::from_secs(1))
        .await;
    assert_eq!(transport.readiness(), WorkTransportReadiness::FailedClosed);
    assert_eq!(transport.enqueue_count(), 3);

    let fatal = RecordingTransport::new(RecordingResponse::Error(
        TransportErrorClass::AdapterMismatch,
    ));
    fatal
        .offer(&prepared, Instant::now() + Duration::from_secs(1))
        .await;
    assert_eq!(fatal.readiness(), WorkTransportReadiness::FailedClosed);
}

#[test]
fn bid_delivery_v1_payload_verifier_rejects_each_frozen_field() {
    let prepared = prepare_bid_delivery_v1(valid_spec()).expect("valid delivery spec");
    let bytes = prepared.canonical_payload_bytes();
    verify_bid_delivery_v1_payload(bytes, fixed_dispatch_id()).expect("valid KBDL");

    assert_eq!(
        verify_bid_delivery_v1_payload(&bytes[..bytes.len() - 1], fixed_dispatch_id()),
        Err(BidDeliveryPayloadError::Length)
    );
    let mut tampered = bytes.to_vec();
    tampered[0] ^= 1;
    assert_eq!(
        verify_bid_delivery_v1_payload(&tampered, fixed_dispatch_id()),
        Err(BidDeliveryPayloadError::Magic)
    );
    let mut tampered = bytes.to_vec();
    tampered[5] = 2;
    assert_eq!(
        verify_bid_delivery_v1_payload(&tampered, fixed_dispatch_id()),
        Err(BidDeliveryPayloadError::SchemaVersion)
    );
    let mut tampered = bytes.to_vec();
    tampered[6] ^= 1;
    assert_eq!(
        verify_bid_delivery_v1_payload(&tampered, fixed_dispatch_id()),
        Err(BidDeliveryPayloadError::DispatchId)
    );
    let mut tampered = bytes.to_vec();
    tampered[23] = 2;
    assert_eq!(
        verify_bid_delivery_v1_payload(&tampered, fixed_dispatch_id()),
        Err(BidDeliveryPayloadError::PayloadVersion)
    );
}

#[test]
fn published_registry_closure_is_valid() {
    validate_bid_delivery_v1_registry().expect("published registry closure");
}

#[derive(Debug)]
struct HandlerError;

impl std::fmt::Display for HandlerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("handler error")
    }
}

impl std::error::Error for HandlerError {}

struct ContractHandler;

#[async_trait::async_trait]
impl BidDeliveryV1Handler for ContractHandler {
    type Error = HandlerError;

    async fn handle(&self, _delivery: ObservedBidDeliveryV1) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[test]
fn bid_delivery_v1_worker_contract_disables_oxana_retries() {
    let job = BidDeliveryV1Job::new(fixed_dispatch_id());
    let worker = BidDeliveryV1WorkerAdapter::new(ContractHandler);
    assert_eq!(worker.max_retries(&job), 0);
}

#[test]
fn work_transport_correctness_path_has_no_redis_inspection_or_private_keys() {
    let source = include_str!("../src/work_transport.rs");
    let correctness_start = source
        .find("impl WorkTransport for OxanaStableAdapter")
        .expect("production WorkTransport implementation");
    let correctness_end = source[correctness_start..]
        .find("pub enum RecordingResponse")
        .map(|offset| correctness_start + offset)
        .expect("recording adapter follows production adapter");
    let correctness_path = &source[correctness_start..correctness_end];
    for forbidden in [
        "oxanus:",
        ".get_job(",
        ".list_queue_jobs(",
        ".enqueued_count(",
        ".dead_count(",
        ".stats(",
        ".delete_job(",
        "replay_orphaned_local_jobs",
    ] {
        assert!(
            !correctness_path.contains(forbidden),
            "WorkTransport correctness path contains forbidden token {forbidden}"
        );
    }

    let repository = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("workspace root");
    for crate_name in ["api", "bid", "worker", "storage", "domain"] {
        let root = repository.join("crates").join(crate_name).join("src");
        for path in rust_sources(&root) {
            let contents = std::fs::read_to_string(&path).expect("read production source");
            for forbidden in [
                "bid:delivery:v1",
                "BidDeliveryV1",
                "oxanus:",
                ".get_job(",
                ".list_queue_jobs(",
                ".delete_job(",
            ] {
                assert!(
                    !contents.contains(forbidden),
                    "unapproved Bid delivery/private Redis call-site {forbidden} in {}",
                    path.display()
                );
            }
        }
    }

    let runtime_root = repository.join("crates/runtime/src");
    for path in rust_sources(&runtime_root) {
        let file_name = path.file_name().and_then(|name| name.to_str());
        let contents = std::fs::read_to_string(&path).expect("read runtime source");
        if file_name == Some("jobs.rs") {
            let production = contents
                .split("#[cfg(test)]\nmod tests")
                .next()
                .expect("runtime jobs production prefix");
            assert!(!production.contains("bid:delivery:v1"));
            assert!(!production.contains("BidDeliveryV1"));
            continue;
        }
        if contents.contains("bid:delivery:v1") || contents.contains("BidDeliveryV1") {
            assert!(
                file_name == Some("work_transport.rs"),
                "Bid delivery marker escaped approved runtime files: {}",
                path.display()
            );
        }
    }
    let legacy_jobs = include_str!("../src/jobs.rs");
    assert!(legacy_jobs.contains("pub async fn replay_orphaned_local_jobs"));
    assert!(legacy_jobs.contains("oxanus:processing:"));
}

fn rust_sources(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut sources = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory).expect("read source directory") {
            let path = entry.expect("source entry").path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                sources.push(path);
            }
        }
    }
    sources
}

fn valid_spec() -> DeliverySpec {
    DeliverySpec {
        physical_lane: "bid-convert-v1".to_string(),
        task_type: BID_DELIVERY_V1_TASK_TYPE.to_string(),
        dispatch_id: fixed_dispatch_id(),
        payload_version: BID_DELIVERY_V1_PAYLOAD_VERSION,
    }
}

fn hex_literal(value: &str) -> Vec<u8> {
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            u8::from_str_radix(std::str::from_utf8(pair).expect("ASCII hex"), 16)
                .expect("valid hex")
        })
        .collect()
}
