#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

scope=(
  crates/bidding/src crates/api/src crates/worker/src crates/platform/src web/src/bid migrations
  ':(glob)deploy/**/*.sh' ':(glob)deploy/**/*.yml' ':(glob)deploy/**/*.yaml'
  ':(glob)deploy/**/*.toml' ':(glob)deploy/**/Dockerfile*' deploy/queue-registry.toml
)
patterns=(
  'BidDeliveryV1|bid-delivery-v1|bid:delivery:v1'
  'SubmissionGateV1|submission_gate'
  'bidding_v1_baseline|/api/v1/bids?'
  'first[-_ ]launch|first_launch|intended[-_]state|migration-manifest|catalog-row-allowlist'
  'bid_part_content|bid_current_parts|required_part_keys|template_slot_for_part_key'
  'EvidenceMatchJob|evidence_match_job'
)
failed=0
for pattern in "${patterns[@]}"; do
  if git grep -nEi "$pattern" -- "${scope[@]}"; then
    echo "forbidden clean-slate residue: $pattern" >&2
    failed=1
  fi
done
exit "$failed"
