//! Registry required_enabled (minus retention) plus housekeep must match
//! `WORKER_REGISTERED_TASKS` and the source registrations in `run_transport_group`.

use std::collections::BTreeSet;

use platform::{LaunchMode, QueueRegistry};

const WORKER_SOURCE: &str = include_str!("../src/runtime.rs");

#[test]
fn worker_runtime_matches_queue_registry() {
    let registry = QueueRegistry::load().expect("queue registry");
    let expected: BTreeSet<&str> = registry
        .entries()
        .iter()
        .filter(|entry| {
            entry.physical_queue != "retention" && entry.launch_mode != LaunchMode::DeclaredDisabled
        })
        .map(|entry| entry.task_type.as_str())
        .collect();
    let registered: BTreeSet<&str> = worker::runtime::WORKER_REGISTERED_TASKS
        .iter()
        .copied()
        .collect();
    assert_eq!(
        expected, registered,
        "queue-registry tasks for this process must equal WORKER_REGISTERED_TASKS"
    );

    let production = WORKER_SOURCE
        .split("\n#[cfg(test)]")
        .next()
        .expect("production worker source");
    assert_eq!(
        production
            .matches("queue_with_concurrency::<SummaryQueue>")
            .count(),
        1,
        "SummaryQueue must have one runtime"
    );
    assert!(
        !production.contains("runtime_concurrency(\"SHARED\""),
        "SHARED SummaryQueue runtime must be gone"
    );
    assert!(!production.contains("QuestionWorker"));
    assert!(!production.contains("ExtractWorker"));
    assert!(production.contains(".worker::<HousekeepWorker, HousekeepJob>()"));
    assert!(production.contains(".worker::<DocumentProcessWorker, DocumentProcessJob>()"));
    assert!(production.contains(".worker::<DocumentProcessWorker, ManualProcessJob>()"));
    assert!(production.contains("queue_with_concurrency::<BidAuthoringV2Queue>"));
    assert!(production.contains("TenderDocumentProcessV2Worker"));
    assert!(production.contains("RequirementSetCompileV2Worker"));
    assert!(production.contains("DocxComposeV2Worker"));
    assert!(production.contains("ContentGenerateV2Worker"));
    assert!(production.contains("SubmissionExportV2Worker"));
    assert!(production.contains(".worker::<PostProcessWorker, PostProcessJob>()"));
    assert!(
        production
            .contains(".worker::<KnowledgeSemanticIndexV2Worker, KnowledgeSemanticIndexV2Job>()")
    );
    assert!(production.contains(".worker::<SummaryWorker, SummaryJob>()"));
    assert!(production.contains("DatatableWorker"));
    assert!(production.contains(".worker::<ImageMultimodalWorker, ImageMultimodalJob>()"));
    assert!(production.contains("WikiIngestWorker"));
    assert!(production.contains("WikiFinalizeWorker"));
    assert!(production.contains("VersionCloneWorker"));
    assert!(production.contains("ListDeleteWorker"));
    assert!(production.contains("KbDeleteWorker"));
    assert!(production.contains("ListReparseWorker"));
    assert!(production.contains("IndexDeleteWorker"));
    assert!(!production.contains(concat!("bid_failure_", "is_final")));
    assert!(!production.contains("ctx.meta.retries >= platform::BID_AUTHORING_V2_MAX_RETRIES"));
    assert!(!production.contains("oxanus:"));
    for forbidden in [
        "run_bid_request_delivery_reconciler",
        "claim_stale_request_deliveries_v2",
        "DELIVERY_RECONCILE_MIN_SECONDS",
        "automatic_reconcile",
    ] {
        assert!(
            !production.contains(forbidden),
            "worker contains PG queue recovery: {forbidden}"
        );
    }
}

#[test]
fn housekeep_queue_maps_to_low() {
    assert_eq!(
        platform::queue_for(platform::TYPE_MAINTENANCE_HOUSEKEEP),
        platform::QUEUE_LOW
    );
}

#[test]
fn declared_disabled_lanes_stay_rejected() {
    assert!(platform::queue_for(platform::TYPE_QUESTION).starts_with("rejected:"));
    assert!(platform::queue_for(platform::TYPE_CHUNK_EXTRACT).starts_with("rejected:"));
}
