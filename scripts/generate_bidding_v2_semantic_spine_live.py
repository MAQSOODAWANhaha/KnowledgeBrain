#!/usr/bin/env python3
"""Regenerate the atomic live upgrade from the checked-in fresh baselines."""

from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SHARED = (ROOT / "migrations/shared_platform_baseline.sql").read_text()
BIDDING = (ROOT / "migrations/bidding_v2_baseline.sql").read_text()
OUTPUT = ROOT / "migrations/bidding_v2_semantic_spine_live.sql"


def function(sql: str, name: str) -> str:
    marker = f"CREATE FUNCTION {name}"
    start = sql.index(marker)
    body = sql.index("AS $$", start)
    end = sql.index("$$;", body + 5) + 3
    return sql[start:end].replace("CREATE FUNCTION", "CREATE OR REPLACE FUNCTION", 1)


parts = [
    "-- Live V8 semantic-spine contract upgrade. Safe for an existing V2 database.\nBEGIN;",
    function(SHARED, "kb_actor_identity_valid"),
    """
CREATE TABLE IF NOT EXISTS bid_outline_requirement_grouping_batch_artifacts (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  request_artifact_id uuid NOT NULL,
  frozen_input_sha256 kb_sha256 NOT NULL,
  batch_ordinal integer NOT NULL CHECK (batch_ordinal>=0),
  model_contract_sha256 kb_sha256 NOT NULL,
  agent_contract_sha256 kb_sha256 NOT NULL,
  need_occurrence_ids uuid[] NOT NULL CHECK (cardinality(need_occurrence_ids) BETWEEN 1 AND 48),
  canonical_payload bytea NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(request_artifact_id,frozen_input_sha256,batch_ordinal,model_contract_sha256,agent_contract_sha256),
  FOREIGN KEY(request_artifact_id) REFERENCES bid_async_request_snapshot_artifacts(id),
  CHECK (content_sha256=kb_bid_v2_sha256_bytes(canonical_payload))
);
ALTER TABLE bid_outline_reduce_plan_artifacts
  ADD COLUMN IF NOT EXISTS grouping_evidence_set_sha256 kb_sha256;
ALTER TABLE bid_outline_synthesis_packet_artifacts
  ADD COLUMN IF NOT EXISTS grouping_evidence_set_sha256 kb_sha256;
ALTER TABLE bid_outline_agent_run_artifacts
  DROP CONSTRAINT IF EXISTS bid_outline_agent_run_artifacts_progress_phase_check;
ALTER TABLE bid_outline_agent_run_artifacts
  ADD CONSTRAINT bid_outline_agent_run_artifacts_progress_phase_check
  CHECK (progress_phase IN ('analyzing','mapping','grouping','reducing','collecting','drafting','routing','verifying','repairing','publishing','retrying','succeeded','failed','cancelled'));
""".strip(),
]
for function_name in [
    "kb_bid_v2_require_project_owner",
    "kb_bid_v2_load_requirement_set_compile_input_v3",
    "kb_bid_v2_publish_requirement_set_v3",
    "kb_bid_v2_create_outline_candidate",
    "kb_bid_v2_outline_semantics_valid",
    "kb_bid_v2_publish_outline_generation",
    "kb_bid_v2_outline_run_upsert",
    "kb_bid_v2_outline_grouping_get",
    "kb_bid_v2_outline_grouping_put",
    "kb_bid_v2_outline_reduce_get",
    "kb_bid_v2_outline_reduce_put",
    "kb_bid_v2_outline_synthesis_packet_append",
    "kb_bid_v2_outline_checkpoint_append",
    "kb_bid_v2_outline_checkpoint_latest",
]:
    parts.append(function(BIDDING, function_name))

parts.append(
    """
INSERT INTO bid_authoring_contract_artifacts(
  id,contract_kind,schema_version,canonical_payload,content_sha256)
SELECT '00000000-0000-5000-8000-000000000108'::uuid,'agent',1,payload,
  kb_bid_v2_sha256_bytes(payload)
FROM (VALUES (convert_to(
  '{"kind":"outline_agent","version":8,"map_schema":4,"requirement_grouping_schema":1,"fulfillment_group_schema":1,"reduce_schema":3,"draft_patch_schema":1,"packet_schema":3,"checkpoint_schema":3,"output_schema":2,"progress_control":"semantic_closure_and_atomic_patch","max_stalled_turns":2}',
  'UTF8'))) value(payload)
ON CONFLICT (id) DO NOTHING;

REVOKE ALL ON FUNCTION
  kb_bid_v2_require_project_owner(uuid,kb_actor_identity),
  kb_bid_v2_load_requirement_set_compile_input_v3(uuid,bigint,kb_sha256),
  kb_bid_v2_publish_requirement_set_v3(uuid,bigint,kb_sha256,jsonb,kb_actor_identity),
  kb_bid_v2_outline_semantics_valid(jsonb,jsonb,jsonb,uuid),
  kb_bid_v2_outline_run_upsert(uuid,kb_sha256,integer,integer,text,jsonb),
  kb_bid_v2_outline_grouping_get(uuid,kb_sha256,integer,kb_sha256,kb_sha256),
  kb_bid_v2_outline_grouping_put(uuid,kb_sha256,integer,kb_sha256,kb_sha256,uuid[],jsonb),
  kb_bid_v2_outline_reduce_get(uuid,kb_sha256,kb_sha256,kb_sha256,kb_sha256),
  kb_bid_v2_outline_reduce_put(uuid,kb_sha256,kb_sha256,kb_sha256,kb_sha256,jsonb),
  kb_bid_v2_outline_synthesis_packet_append(uuid,kb_sha256,kb_sha256,kb_sha256,kb_sha256,jsonb),
  kb_bid_v2_outline_checkpoint_append(uuid,kb_sha256,integer,integer,text,jsonb),
  kb_bid_v2_outline_checkpoint_latest(uuid,kb_sha256)
FROM PUBLIC;
GRANT EXECUTE ON FUNCTION
  kb_bid_v2_load_requirement_set_compile_input_v3(uuid,bigint,kb_sha256),
  kb_bid_v2_publish_requirement_set_v3(uuid,bigint,kb_sha256,jsonb,kb_actor_identity),
  kb_bid_v2_outline_run_upsert(uuid,kb_sha256,integer,integer,text,jsonb),
  kb_bid_v2_outline_grouping_get(uuid,kb_sha256,integer,kb_sha256,kb_sha256),
  kb_bid_v2_outline_grouping_put(uuid,kb_sha256,integer,kb_sha256,kb_sha256,uuid[],jsonb),
  kb_bid_v2_outline_reduce_get(uuid,kb_sha256,kb_sha256,kb_sha256,kb_sha256),
  kb_bid_v2_outline_reduce_put(uuid,kb_sha256,kb_sha256,kb_sha256,kb_sha256,jsonb),
  kb_bid_v2_outline_synthesis_packet_append(uuid,kb_sha256,kb_sha256,kb_sha256,kb_sha256,jsonb),
  kb_bid_v2_outline_checkpoint_append(uuid,kb_sha256,integer,integer,text,jsonb),
  kb_bid_v2_outline_checkpoint_latest(uuid,kb_sha256)
TO kb_runtime_worker;

COMMIT;
""".strip()
)
OUTPUT.write_text("\n\n".join(parts) + "\n")
