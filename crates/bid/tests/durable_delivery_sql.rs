const BIDDING_SQL: &str = include_str!("../../../migrations/bidding_v1_baseline.sql");

fn table_definition(table: &str) -> &str {
    let start = BIDDING_SQL
        .find(&format!("CREATE TABLE {table} ("))
        .unwrap_or_else(|| panic!("missing target table {table}"));
    BIDDING_SQL[start..]
        .split_once("\n);")
        .unwrap_or_else(|| panic!("unterminated target table {table}"))
        .0
}

#[test]
fn six_business_targets_have_no_transport_delivery_state() {
    for table in [
        "bid_documents",
        "bid_extraction_targets",
        "bid_matching_schedule_intents",
        "bid_matching_jobs",
        "bid_attachment_preparation_jobs",
        "bid_submission_render_jobs",
    ] {
        let definition = table_definition(table);
        for forbidden in [
            "delivery_generation",
            "next_enqueue_at",
            "claim_token",
            "claim_lease_ms",
            "heartbeat_at",
            "active_attempt",
        ] {
            assert!(
                !definition.contains(forbidden),
                "{table} must not mirror Oxana transport state via {forbidden}"
            );
        }
    }
}

#[test]
fn baseline_has_no_second_bid_queue_scheduler_or_lease_machine() {
    for forbidden in [
        "bid_document_conversion_attempts",
        "bid_extraction_attempts",
        "bid_matching_job_claims",
        "kb_bid_reserve_due_deliveries",
        "kb_bid_reclaim_stale_conversions",
        "kb_bid_reclaim_stale_extractions",
        "kb_bid_matching_reap",
        "kb_bid_reap_attachment_preparations",
        "kb_bid_reap_submission_renders",
        "kb_bid_heartbeat_document_conversion",
        "kb_bid_heartbeat_extraction",
        "kb_bid_matching_heartbeat",
        "kb_bid_heartbeat_attachment_preparation",
        "kb_bid_heartbeat_submission_render",
    ] {
        assert!(
            !BIDDING_SQL.contains(forbidden),
            "Oxana owns retry/resurrection; baseline still contains {forbidden}"
        );
    }
}

#[test]
fn business_targets_keep_atomic_publish_boundaries() {
    for required in [
        "conversion_generation integer",
        "extraction_generation integer",
        "matching_mutation_watermark bigint",
        "CREATE FUNCTION kb_bid_complete_document_conversion(",
        "CREATE FUNCTION kb_bid_publish_extraction_section(",
        "CREATE FUNCTION kb_bid_matching_commit(",
        "CREATE FUNCTION kb_bid_publish_attachment_preparation(",
        "CREATE FUNCTION kb_bid_publish_submission_output(",
    ] {
        assert!(
            BIDDING_SQL.contains(required),
            "business revision or atomic publish boundary is missing: {required}"
        );
    }
}
