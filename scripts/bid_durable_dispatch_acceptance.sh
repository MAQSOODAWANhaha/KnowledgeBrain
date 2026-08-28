#!/usr/bin/env bash
set -euo pipefail

ROOT=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"

ARTIFACT_DIR=${BID_DURABLE_DISPATCH_ARTIFACT_DIR:-$ROOT/.artifacts/bid-durable-dispatch}
mkdir -p "$ARTIFACT_DIR/logs"

fail() {
  printf '%s\n' "bid durable-delivery acceptance: $*" >&2
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
  if grep -Eiq '(^|[[:space:]])skip(ped)?([[:space:]:]|$)|\.\.\. ignored$|test result:.*[1-9][0-9]* ignored' "$log"; then
    fail "$contract_id used a skip path"
  fi
}

require_nonzero_tests() {
  contract_id=$1
  log=$2
  grep -Eq 'test result: ok\..*[1-9][0-9]* passed' "$log" ||
    fail "$contract_id produced no passing tests"
}

run_logged() {
  contract_id=$1
  log=$2
  shift 2
  printf 'running required durable-delivery contract: %s\n' "$contract_id"
  "$@" 2>&1 | tee "$log"
  reject_skip_or_empty "$contract_id" "$log"
}

require_enabled KNOWLEDGEBRAIN_REQUIRE_POSTGRES_TESTS
require_enabled KNOWLEDGEBRAIN_REQUIRE_REDIS_TESTS
[[ -n "${DATABASE_URL:-}" ]] || fail "DATABASE_URL is required"
[[ -n "${REDIS_URL:-}" ]] || fail "REDIS_URL is required"

run_logged oxana-registry-source "$ARTIFACT_DIR/logs/oxana-registry-source.log" \
  scripts/verify_oxana_registry_source.sh --self-test
run_logged no-second-queue-state "$ARTIFACT_DIR/logs/no-second-queue-state.log" \
  bash scripts/bidding_v1_deletion_scan.sh
run_logged runtime-jobs "$ARTIFACT_DIR/logs/runtime-jobs.log" \
  cargo test --locked -p runtime jobs::tests -- --nocapture
run_logged work-transport "$ARTIFACT_DIR/logs/work-transport.log" \
  cargo test --locked -p runtime --test work_transport -- --nocapture
run_logged work-transport-live "$ARTIFACT_DIR/logs/work-transport-live.log" \
  cargo test --locked -p runtime --test work_transport_live -- --nocapture --test-threads=1
run_logged durable-delivery-sql "$ARTIFACT_DIR/logs/durable-delivery-sql.log" \
  cargo test --locked -p bid --test durable_delivery_sql -- --nocapture --test-threads=1
run_logged bid-queue-http "$ARTIFACT_DIR/logs/bid-queue-http.log" \
  cargo test --locked -p api --test bid_queue_contract -- --nocapture --test-threads=1
run_logged tender-publication "$ARTIFACT_DIR/logs/tender-publication.log" \
  cargo test --locked -p bid --test tender_publication -- --nocapture --test-threads=1
run_logged matching-publication "$ARTIFACT_DIR/logs/matching-publication.log" \
  cargo test --locked -p bid --test matching_publication -- --nocapture --test-threads=1
run_logged submission-sql "$ARTIFACT_DIR/logs/submission-sql.log" \
  cargo test --locked -p bid --test submission_sql -- --nocapture --test-threads=1
run_logged worker-bid-delivery "$ARTIFACT_DIR/logs/worker-bid-delivery.log" \
  cargo test --locked -p worker bid_delivery -- --nocapture
run_logged fresh-schema "$ARTIFACT_DIR/logs/fresh-schema.log" \
  scripts/fresh_schema_acceptance.sh

require_nonzero_tests runtime-jobs "$ARTIFACT_DIR/logs/runtime-jobs.log"
require_nonzero_tests work-transport "$ARTIFACT_DIR/logs/work-transport.log"
require_nonzero_tests work-transport-live "$ARTIFACT_DIR/logs/work-transport-live.log"
require_nonzero_tests durable-delivery-sql "$ARTIFACT_DIR/logs/durable-delivery-sql.log"
require_nonzero_tests bid-queue-http "$ARTIFACT_DIR/logs/bid-queue-http.log"
require_nonzero_tests tender-publication "$ARTIFACT_DIR/logs/tender-publication.log"
require_nonzero_tests matching-publication "$ARTIFACT_DIR/logs/matching-publication.log"
require_nonzero_tests submission-sql "$ARTIFACT_DIR/logs/submission-sql.log"
require_nonzero_tests worker-bid-delivery "$ARTIFACT_DIR/logs/worker-bid-delivery.log"

printf '%s\n' 'bid durable-delivery acceptance passed'
