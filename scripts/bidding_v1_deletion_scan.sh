#!/bin/sh
set -eu

cd "$(dirname "$0")/.."

fail=0
scan() {
  pattern=$1
  shift
  if rg -n --glob '!target/**' --glob '!web/dist/**' --glob '!web/node_modules/**' \
      --glob '!web/playwright-report/**' --glob '!web/test-results/**' \
      --glob '!migrations/bidding_v1_extra_functions.sql' \
      -e "$pattern" "$@"; then
    echo "deletion-scan hit: $pattern" >&2
    fail=1
  fi
}

# Old persist / export / family write seams must not remain in production surfaces.
scan 'persist_extraction_report' crates/storage/src crates/api/src crates/bid/src crates/worker/src
scan 'persist_section_retry' crates/storage/src crates/api/src crates/bid/src crates/worker/src
scan 'ExtractionPublicationStore' crates
scan 'pub async fn commit_route\(' crates/storage/src
scan 'CommitRouteV1' crates
scan 'regenerate_stale' crates/api/src crates/bid/src web/src
scan 'bid_extract_runs' crates/api/src crates/bid/src crates/worker/src
scan 'bid[-_:]section[-_:]retry|BidSectionRetry|BID_SECTION_RETRY' crates deploy/queue-registry.toml deploy/first-launch/intended-state.toml
scan 'owner_name' web/src/api.ts web/src/App.tsx web/src/bid
scan 'downloadExport' web/src
scan 'content_objects' crates/storage/src/persist.rs crates/storage/src/object_registry.rs
scan 'bump_object_ref' crates/storage/src
scan 'release_object_ref' crates/storage/src
scan 'drop_blob' crates/storage/src
scan 'bid_booklet_parts' crates/api/src crates/bid/src crates/worker/src web/src
scan 'ClauseView|pub fn clause_from_row\(|decorate_clauses|coverage_for(_commercial)?\(' crates/bid/src
scan 'visible_(pick|commercial|technical_candidates)_json|section_merge_map|meet_blocked_by_suggestion' crates/bid/src
scan 'regenerate_stale' crates/api/src/bid_routes.rs
scan '/api/v1/bids/.*/export' crates/api/src
scan 'step=booklet' web/src
scan '/booklet/' web/src

# Oxana owns enqueue, retry, delay, membership, heartbeat, resurrection and the
# dead queue. Bid keeps only business revision/current CAS and immutable staging.
scan 'replay_orphaned_local_jobs|LiveRecoveryV1|TYPE_LIVE_RECOVERY|QUEUE_LIVE_RECOVERY|enqueue_live_recovery|bid_recovery' \
  crates/domain/src crates/runtime/src crates/storage/src crates/worker/src
scan 'oxanus:' \
  crates/runtime/src/work_transport.rs crates/worker/src/consume.rs crates/worker/src/main.rs \
  crates/storage/src/bid_matching.rs \
  crates/storage/src/bid_submission.rs crates/storage/src/bidding.rs
scan 'bid_(delivery_attempts|delivery_settlements|delivery_successors|queue_memberships|dispatch_heads|dispatch_intents|repair_obligations|rejected_deliveries)' \
  migrations/bidding_v1_baseline.sql crates/api/src crates/bid/src crates/domain/src \
  crates/runtime/src crates/storage/src crates/worker/src
scan 'attempt_count|max_attempts|retry_backoff|retry_schedule|dead_queue|queue_membership' \
  migrations/bidding_v1_baseline.sql crates/api/src/bid_routes.rs crates/bid/src \
  crates/domain/src/intended_state.rs crates/domain/src/queue_registry.rs crates/domain/src/status.rs \
  crates/runtime/src/work_transport.rs \
  crates/storage/src/bid_matching.rs crates/storage/src/bid_submission.rs \
  crates/storage/src/bidding.rs crates/worker/src/consume.rs
scan 'delivery_generation([[:space:]]*[:.]|[[:space:]]+bigint)|next_enqueue_at([[:space:]]*[:.=]|[[:space:]]+timestamptz)|fn (run_bid_delivery_reconciler|reconcile_bid_deliveries_once)|run_bid_delivery_reconciler\(|reconcile_bid_deliveries_once\(|reserve_due_deliveries\(|reap_expired_deliveries\(' \
  migrations/bidding_v1_baseline.sql crates/api/src/bid_routes.rs crates/bid/src \
  crates/runtime/src/work_transport.rs \
  crates/storage/src/bid_matching.rs crates/storage/src/bid_submission.rs \
  crates/storage/src/bidding.rs crates/worker/src/consume.rs
scan 'bid_(document_conversion_attempts|extraction_attempts|matching_job_claims)' \
  migrations/bidding_v1_baseline.sql
scan 'kb_bid_(reserve_due_deliveries|reclaim_stale_conversions|reclaim_stale_extractions|matching_reap|reap_attachment_preparations|reap_submission_renders|heartbeat_document_conversion|heartbeat_extraction|matching_heartbeat|heartbeat_attachment_preparation|heartbeat_submission_render)\(' \
  migrations/bidding_v1_baseline.sql crates/storage/src crates/bid/src crates/worker/src
scan 'run_with_heartbeat|LeaseRun' crates/bid/src crates/worker/src

if rg -n --glob '!target/**' -e 'value === "booklet"' web/src/hash.ts; then
  echo "deletion-scan hit: booklet alias in hash.ts" >&2
  fail=1
fi

if [ -e crates/storage/src/bid.rs ] || [ -e crates/storage/src/bid_extract_publication.rs ] \
   || [ -e crates/bid/src/booklet.rs ] || [ -e crates/bid/src/export.rs ] \
   || [ -e crates/runtime/src/lease.rs ]; then
  echo "deleted modules still present" >&2
  fail=1
fi

if [ "$fail" -ne 0 ]; then
  echo "bidding V1 deletion scan failed" >&2
  exit 1
fi
echo "bidding V1 deletion scan passed"
