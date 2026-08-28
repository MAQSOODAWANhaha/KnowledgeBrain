use oxana::{Job as _, Queue as _, Worker as _};
use platform::{
    BID_DELIVERY_V1_QUEUE, BID_DELIVERY_V1_TASK_TYPE, BidDeliveryTargetKind, BidDeliveryV1Handler,
    BidDeliveryV1Job, BidDeliveryV1Queue, BidDeliveryV1Worker,
};
use uuid::Uuid;

fn target_id() -> Uuid {
    Uuid::parse_str("018f3000-7d47-7a1b-9bb8-b3880f15478a").expect("fixed target id")
}

#[test]
fn delivery_job_uses_the_single_stable_contract() {
    let job = BidDeliveryV1Job::new(BidDeliveryTargetKind::DocumentConversion, target_id(), 7);

    assert_eq!(BidDeliveryV1Job::name(), BID_DELIVERY_V1_TASK_TYPE);
    assert_eq!(job.target_kind, BidDeliveryTargetKind::DocumentConversion);
    assert_eq!(job.target_id, target_id());
    assert_eq!(job.target_revision, 7);
    assert_eq!(
        job.unique_id().as_deref(),
        Some("document_conversion:018f3000-7d47-7a1b-9bb8-b3880f15478a:7")
    );
    assert_eq!(job.on_conflict(), oxana::JobConflictStrategy::Skip);
    assert!(BidDeliveryV1Job::should_resurrect());
    assert_eq!(
        serde_json::to_value(job).expect("serialize delivery"),
        serde_json::json!({
            "target_kind": "document_conversion",
            "target_id": "018f3000-7d47-7a1b-9bb8-b3880f15478a",
            "target_revision": 7
        })
    );
}

#[test]
fn delivery_queue_is_single_and_stable() {
    match BidDeliveryV1Queue::to_config().kind {
        oxana::QueueKind::Static { key } => assert_eq!(key, BID_DELIVERY_V1_QUEUE),
        oxana::QueueKind::Dynamic { .. } => panic!("Bid delivery queue must be static"),
    }
}

#[derive(Debug)]
struct HandlerError;

impl std::fmt::Display for HandlerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("handler error")
    }
}

impl std::error::Error for HandlerError {}

struct NoopHandler;

#[async_trait::async_trait]
impl BidDeliveryV1Handler for NoopHandler {
    type Error = HandlerError;

    async fn handle(&self, _delivery: BidDeliveryV1Job) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[test]
fn delivery_worker_uses_oxana_retry_policy() {
    let worker = BidDeliveryV1Worker::new(NoopHandler);
    let job = BidDeliveryV1Job::new(BidDeliveryTargetKind::DocumentConversion, target_id(), 7);

    assert_eq!(worker.max_retries(&job), 3);
    assert_eq!(worker.retry_delay(&job, 0), 10);
    assert_eq!(worker.retry_delay(&job, 2), 10);
}

#[test]
fn transport_source_has_no_second_queue_state_machine() {
    let source = include_str!("../src/work_transport.rs")
        .split("\n#[cfg(test)]")
        .next()
        .expect("production source");

    for forbidden in [
        "dispatch_id",
        "payload_version",
        "PhysicalLane",
        "DeliverySpec",
        "RegistryClosure",
        "canonical_payload",
        "replay_orphaned_local_jobs",
        "delivery_generation",
        "next_enqueue_at",
        "claim_token",
        ".get_job(",
        ".list_queue_jobs(",
        ".dead_count(",
        ".retries_count(",
        "oxanus:",
    ] {
        assert!(
            !source.contains(forbidden),
            "transport must not implement or inspect queue state via {forbidden}"
        );
    }

    assert_eq!(
        source.matches(".enqueue(").count(),
        1,
        "transport must call Oxana enqueue exactly once"
    );
}
