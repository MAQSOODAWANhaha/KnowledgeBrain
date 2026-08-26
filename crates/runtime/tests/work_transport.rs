use oxana::Worker as _;
use oxana::{Job as _, JobConflictStrategy};
use runtime::{
    BID_DELIVERY_V1_PAYLOAD_VERSION, BID_DELIVERY_V1_TASK_TYPE, BidDeliveryV1Handler,
    BidDeliveryV1Job, BidDeliveryV1WorkerAdapter, DeliverySpec, ObservedBidDeliveryV1,
    PrepareDeliveryError, RecordingResponse, RecordingTransport, TransportErrorClass,
    TransportOutcome, WorkTransport, WorkTransportReadiness, prepare_bid_delivery_v1,
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
            actual_job_id: "bid:delivery:v1/not-the-dispatch".to_string()
        }
    );
    assert_eq!(mismatch.enqueue_count(), 1);
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

    let cancelled = RecordingTransport::new(RecordingResponse::ReturnExpected);
    let future = cancelled.offer(&prepared, Instant::now() + Duration::from_secs(1));
    drop(future);
    assert_eq!(cancelled.enqueue_count(), 0);

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
    for forbidden in [
        "oxanus:",
        ".get_job(",
        ".list_queue_jobs(",
        ".stats(",
        ".delete_job(",
        "replay_orphaned_local_jobs",
    ] {
        assert!(
            !source.contains(forbidden),
            "WorkTransport correctness path contains forbidden token {forbidden}"
        );
    }
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
