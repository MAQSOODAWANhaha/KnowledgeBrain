#!/usr/bin/env bash
set -euo pipefail

ROOT=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"

ARTIFACT_DIR=${BID_DURABLE_DISPATCH_ARTIFACT_DIR:-$ROOT/.artifacts/bid-durable-dispatch}
mkdir -p "$ARTIFACT_DIR/logs"
export BID_DURABLE_DISPATCH_CLEANUP_RECEIPT_DIR=${BID_DURABLE_DISPATCH_CLEANUP_RECEIPT_DIR:-$ARTIFACT_DIR/cleanup}

fail() {
  printf '%s\n' "bid durable-dispatch PR8A acceptance: $*" >&2
  exit 1
}

require_enabled() {
  name=$1
  value=${!name:-}
  [[ "$value" == 1 ]] || fail "$name must be exactly 1"
}

reject_skip_or_empty() {
  contract_id=$1
  log=$2
  if grep -Eiq '^skip .*redis|skip runtime test|skipped live|skipped redis|skipped required|\.\.\. ignored$|test result:.*[1-9][0-9]* ignored' "$log"; then
    fail "$contract_id used a skip path"
  fi
}

require_nonzero_tests() {
  contract_id=$1
  log=$2
  grep -Eq 'test result: ok\..*[1-9][0-9]* passed' "$log" \
    || fail "$contract_id produced no passing tests"
}

require_contract_id() {
  contract_id=$1
  test_name=$2
  log=$3
  grep -Fxq "test $test_name ... ok" "$log" \
    || fail "required test $test_name did not finish with an exact ok result for $contract_id"
  printf 'verified required contract ID: %s\n' "$contract_id" | tee -a "$log"
}

run_logged() {
  contract_id=$1
  log=$2
  shift 2
  printf 'running required PR8A contract: %s\n' "$contract_id"
  "$@" 2>&1 | tee "$log"
  reject_skip_or_empty "$contract_id" "$log"
}

require_enabled KNOWLEDGEBRAIN_REQUIRE_POSTGRES_TESTS
require_enabled KNOWLEDGEBRAIN_REQUIRE_REDIS_TESTS
[[ -n "${DATABASE_URL:-}" ]] || fail "DATABASE_URL is required"
[[ -n "${REDIS_URL:-}" ]] || fail "REDIS_URL is required"

# Exercise process-local trap cleanup before any later contract can fail. The
# workflow repeats cleanup independently with `if: always()`.
scripts/bid_durable_dispatch_cleanup.sh exercise

run_logged oxana-registry-source "$ARTIFACT_DIR/logs/oxana-registry-source.log" \
  scripts/verify_oxana_registry_source.sh --self-test
run_logged work-transport-pure "$ARTIFACT_DIR/logs/work-transport.log" \
  cargo test -p runtime --test work_transport -- --nocapture
run_logged work-transport-registry-negative "$ARTIFACT_DIR/logs/work-transport-registry-negative.log" \
  cargo test -p runtime work_transport::registry_tests::tampered_registry_closure_fails_closed -- --exact --nocapture
run_logged runtime-pure-and-legacy "$ARTIFACT_DIR/logs/runtime-jobs.log" \
  cargo test -p runtime jobs::tests -- --nocapture
run_logged work-transport-live "$ARTIFACT_DIR/logs/work-transport-live.log" \
  cargo test -p runtime --test work_transport_live -- --nocapture --test-threads=1

require_contract_id \
  runtime.work_transport.bid_delivery_v1.prepare_golden \
  bid_delivery_v1_prepare_matches_frozen_golden \
  "$ARTIFACT_DIR/logs/work-transport.log"
require_contract_id \
  runtime.work_transport.bid_delivery_v1.prepare_rejects_drift \
  bid_delivery_v1_prepare_rejects_contract_drift \
  "$ARTIFACT_DIR/logs/work-transport.log"
require_contract_id \
  runtime.work_transport.bid_delivery_v1.payload_verifier_negative \
  bid_delivery_v1_payload_verifier_rejects_each_frozen_field \
  "$ARTIFACT_DIR/logs/work-transport.log"
require_contract_id \
  runtime.work_transport.bid_delivery_v1.registry_positive \
  published_registry_closure_is_valid \
  "$ARTIFACT_DIR/logs/work-transport.log"
require_contract_id \
  runtime.work_transport.bid_delivery_v1.registry_negative \
  work_transport::registry_tests::tampered_registry_closure_fails_closed \
  "$ARTIFACT_DIR/logs/work-transport-registry-negative.log"
require_contract_id \
  runtime.work_transport.bid_delivery_v1.recording_once \
  recording_transport_observes_zero_or_one_enqueue_per_offer \
  "$ARTIFACT_DIR/logs/work-transport.log"
require_contract_id \
  runtime.work_transport.bid_delivery_v1.metrics_readiness \
  transport_metrics_drive_degraded_recovery_without_overriding_fatal \
  "$ARTIFACT_DIR/logs/work-transport.log"
require_contract_id \
  runtime.work_transport.bid_delivery_v1.worker_max_retries_zero \
  bid_delivery_v1_worker_contract_disables_oxana_retries \
  "$ARTIFACT_DIR/logs/work-transport.log"
require_contract_id \
  runtime.work_transport.bid_delivery_v1.correctness_denylist \
  work_transport_correctness_path_has_no_redis_inspection_or_private_keys \
  "$ARTIFACT_DIR/logs/work-transport.log"
require_contract_id \
  runtime.work_transport.bid_delivery_v1.live_oxana_2_1_3 \
  stable_adapter_uses_storage_enqueue_unique_skip_and_resurrect_metadata \
  "$ARTIFACT_DIR/logs/work-transport-live.log"
require_contract_id \
  runtime.work_transport.bid_delivery_v1.live_redis_unreachable \
  stable_adapter_classifies_unreachable_redis_as_indeterminate_once \
  "$ARTIFACT_DIR/logs/work-transport-live.log"
require_contract_id \
  runtime.work_transport.bid_delivery_v1.live_native_resurrection \
  oxana_native_resurrection_restores_dead_processing_membership \
  "$ARTIFACT_DIR/logs/work-transport-live.log"
require_contract_id \
  runtime.work_transport.bid_delivery_v1.live_worker_dead_once \
  bid_delivery_worker_failure_is_attempted_once_and_moved_dead \
  "$ARTIFACT_DIR/logs/work-transport-live.log"
require_contract_id \
  runtime.jobs.legacy_replay.mixed_membership \
  jobs::tests::legacy_replay_mixed_list_can_move_bid_delivery_membership \
  "$ARTIFACT_DIR/logs/runtime-jobs.log"
require_nonzero_tests work-transport-pure "$ARTIFACT_DIR/logs/work-transport.log"
require_nonzero_tests work-transport-registry-negative "$ARTIFACT_DIR/logs/work-transport-registry-negative.log"
require_nonzero_tests runtime-pure-and-legacy "$ARTIFACT_DIR/logs/runtime-jobs.log"
require_nonzero_tests work-transport-live "$ARTIFACT_DIR/logs/work-transport-live.log"

printf '%s\n' 'PR8A bid durable-dispatch acceptance passed'
