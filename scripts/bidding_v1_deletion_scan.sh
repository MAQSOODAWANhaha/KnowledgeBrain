#!/bin/sh
set -eu

cd "$(dirname "$0")/.."

fail=0
scan() {
  pattern=$1
  shift
  if rg -n --glob '!target/**' --glob '!web/dist/**' --glob '!web/node_modules/**' \
      --glob '!web/playwright-report/**' --glob '!web/test-results/**' \
      --glob '!deploy/first-launch/catalog-row-allowlist.toml' \
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

if rg -n --glob '!target/**' -e 'value === "booklet"' web/src/hash.ts; then
  echo "deletion-scan hit: booklet alias in hash.ts" >&2
  fail=1
fi

if [ -e crates/storage/src/bid.rs ] || [ -e crates/storage/src/bid_extract_publication.rs ] \
   || [ -e crates/bid/src/booklet.rs ] || [ -e crates/bid/src/export.rs ]; then
  echo "deleted modules still present" >&2
  fail=1
fi

if [ "$fail" -ne 0 ]; then
  echo "bidding V1 deletion scan failed" >&2
  exit 1
fi
echo "bidding V1 deletion scan passed"
