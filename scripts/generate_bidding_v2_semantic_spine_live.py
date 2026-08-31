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
    "-- Live V11 semantic-spine contract upgrade. Safe for an existing V2 database.\nBEGIN;",
    function(SHARED, "kb_actor_identity_valid"),
    """
CREATE TABLE IF NOT EXISTS bid_outline_requirement_grouping_batch_artifacts (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  request_artifact_id uuid NOT NULL,
  frozen_input_sha256 kb_sha256 NOT NULL,
  batch_ordinal integer NOT NULL CHECK (batch_ordinal>=0),
  model_contract_sha256 kb_sha256 NOT NULL,
  agent_contract_sha256 kb_sha256 NOT NULL,
  need_occurrence_ids uuid[] NOT NULL CHECK (cardinality(need_occurrence_ids) BETWEEN 0 AND 48),
  structure_fragment_refs kb_sha256[] NOT NULL DEFAULT ARRAY[]::kb_sha256[] CHECK (cardinality(structure_fragment_refs) BETWEEN 0 AND 48),
  canonical_payload bytea NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(request_artifact_id,frozen_input_sha256,batch_ordinal,model_contract_sha256,agent_contract_sha256),
  FOREIGN KEY(request_artifact_id) REFERENCES bid_async_request_snapshot_artifacts(id),
  CHECK (content_sha256=kb_bid_v2_sha256_bytes(canonical_payload))
);
ALTER TABLE bid_outline_requirement_grouping_batch_artifacts
  ADD COLUMN IF NOT EXISTS structure_fragment_refs kb_sha256[] NOT NULL DEFAULT ARRAY[]::kb_sha256[];
ALTER TABLE bid_outline_requirement_grouping_batch_artifacts
  DROP CONSTRAINT IF EXISTS bid_outline_requirement_grouping_batc_need_occurrence_ids_check;
ALTER TABLE bid_outline_requirement_grouping_batch_artifacts
  DROP CONSTRAINT IF EXISTS bid_outline_requirement_grouping_batch_artifacts_need_occurrence_ids_check;
ALTER TABLE bid_outline_requirement_grouping_batch_artifacts
  ADD CONSTRAINT bid_outline_requirement_grouping_batch_artifacts_need_occurrence_ids_check
  CHECK (cardinality(need_occurrence_ids) BETWEEN 0 AND 48);
ALTER TABLE bid_outline_requirement_grouping_batch_artifacts
  DROP CONSTRAINT IF EXISTS bid_outline_requirement_grouping_batch_artifacts_structure_fragment_refs_check;
ALTER TABLE bid_outline_requirement_grouping_batch_artifacts
  ADD CONSTRAINT bid_outline_requirement_grouping_batch_artifacts_structure_fragment_refs_check
  CHECK (cardinality(structure_fragment_refs) BETWEEN 0 AND 48);
ALTER TABLE bid_outline_reduce_plan_artifacts
  ADD COLUMN IF NOT EXISTS grouping_evidence_set_sha256 kb_sha256;
ALTER TABLE bid_outline_reduce_plan_artifacts
  DROP CONSTRAINT IF EXISTS bid_outline_reduce_plan_artif_request_artifact_id_frozen_in_key;
ALTER TABLE bid_outline_reduce_plan_artifacts
  DROP CONSTRAINT IF EXISTS bid_outline_reduce_plan_replay_key;
ALTER TABLE bid_outline_reduce_plan_artifacts
  ADD CONSTRAINT bid_outline_reduce_plan_replay_key UNIQUE(
    request_artifact_id,frozen_input_sha256,map_evidence_set_sha256,
    grouping_evidence_set_sha256,reduce_contract_sha256);
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
    "kb_bid_v2_get_requirement_set_compile_request",
    "kb_bid_v2_load_outline_generation_input",
    "kb_bid_v2_create_outline_candidate",
    "kb_bid_v2_outline_semantics_valid",
    "kb_bid_v2_publish_outline_generation",
    "kb_bid_v2_outline_run_upsert",
    "kb_bid_v2_outline_grouping_get",
    "kb_bid_v2_outline_grouping_put",
    "kb_bid_v2_outline_semantic_grouping_put",
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
SELECT '00000000-0000-5000-8000-000000000120'::uuid,'agent',1,payload,
  kb_bid_v2_sha256_bytes(payload)
FROM (VALUES (convert_to(
  '{"kind":"outline_agent","version":20,"map_schema":4,"requirement_grouping_schema":5,"structure_placement_schema":2,"fulfillment_group_schema":1,"reduce_schema":3,"draft_patch_schema":1,"packet_schema":5,"checkpoint_schema":4,"checkpoint_resume":[3,4],"output_schema":2,"progress_control":"semantic_closure_and_atomic_patch","section_target":"explicit_frozen_section_ref","grouping_output":"semantic_delta_only","structure_placement":"model_selected_section_and_group","cross_batch_group_registry":"sequential_bounded_feedback","intra_batch_group_registry":"exact_section_title_materialization","new_group_key_scope":"batch_ordinal","response_requiredness":["mandatory","optional"],"informational_closure":"compiled_semantic_unmapped_notice","draft_closure":"mandatory_and_optional_groups","topology_closure":"every_frozen_section_has_model_authored_evidence_child","context_fragment_promotion":"forbidden_by_title_and_source","non_output_fragment_packet":"bounded_title_usage_source","patch_error_feedback":"all_invalid_identities_bounded_32","conflict_notice_severity":"high_only_if_output_relevant_frozen_fragment","repair_identity":"groups_sections_and_invalid_assignments","max_stalled_turns":2}',
  'UTF8'))) value(payload)
ON CONFLICT (id) DO NOTHING;

REVOKE ALL ON FUNCTION
  kb_bid_v2_require_project_owner(uuid,kb_actor_identity),
  kb_bid_v2_load_requirement_set_compile_input_v3(uuid,bigint,kb_sha256),
  kb_bid_v2_publish_requirement_set_v3(uuid,bigint,kb_sha256,jsonb,kb_actor_identity),
  kb_bid_v2_get_requirement_set_compile_request(uuid,uuid,kb_actor_identity),
  kb_bid_v2_load_outline_generation_input(uuid,bigint,kb_sha256),
  kb_bid_v2_outline_semantics_valid(jsonb,jsonb,jsonb,uuid),
  kb_bid_v2_outline_run_upsert(uuid,kb_sha256,integer,integer,text,jsonb),
  kb_bid_v2_outline_grouping_get(uuid,kb_sha256,integer,kb_sha256,kb_sha256),
  kb_bid_v2_outline_grouping_put(uuid,kb_sha256,integer,kb_sha256,kb_sha256,uuid[],jsonb),
  kb_bid_v2_outline_semantic_grouping_put(uuid,kb_sha256,integer,kb_sha256,kb_sha256,uuid[],kb_sha256[],jsonb),
  kb_bid_v2_outline_reduce_get(uuid,kb_sha256,kb_sha256,kb_sha256,kb_sha256),
  kb_bid_v2_outline_reduce_put(uuid,kb_sha256,kb_sha256,kb_sha256,kb_sha256,jsonb),
  kb_bid_v2_outline_synthesis_packet_append(uuid,kb_sha256,kb_sha256,kb_sha256,kb_sha256,jsonb),
  kb_bid_v2_outline_checkpoint_append(uuid,kb_sha256,integer,integer,text,jsonb),
  kb_bid_v2_outline_checkpoint_latest(uuid,kb_sha256)
FROM PUBLIC;
GRANT EXECUTE ON FUNCTION
  kb_bid_v2_load_requirement_set_compile_input_v3(uuid,bigint,kb_sha256),
  kb_bid_v2_publish_requirement_set_v3(uuid,bigint,kb_sha256,jsonb,kb_actor_identity),
  kb_bid_v2_load_outline_generation_input(uuid,bigint,kb_sha256),
  kb_bid_v2_outline_run_upsert(uuid,kb_sha256,integer,integer,text,jsonb),
  kb_bid_v2_outline_grouping_get(uuid,kb_sha256,integer,kb_sha256,kb_sha256),
  kb_bid_v2_outline_grouping_put(uuid,kb_sha256,integer,kb_sha256,kb_sha256,uuid[],jsonb),
  kb_bid_v2_outline_semantic_grouping_put(uuid,kb_sha256,integer,kb_sha256,kb_sha256,uuid[],kb_sha256[],jsonb),
  kb_bid_v2_outline_reduce_get(uuid,kb_sha256,kb_sha256,kb_sha256,kb_sha256),
  kb_bid_v2_outline_reduce_put(uuid,kb_sha256,kb_sha256,kb_sha256,kb_sha256,jsonb),
  kb_bid_v2_outline_synthesis_packet_append(uuid,kb_sha256,kb_sha256,kb_sha256,kb_sha256,jsonb),
  kb_bid_v2_outline_checkpoint_append(uuid,kb_sha256,integer,integer,text,jsonb),
  kb_bid_v2_outline_checkpoint_latest(uuid,kb_sha256)
TO kb_runtime_worker;
GRANT EXECUTE ON FUNCTION
  kb_bid_v2_get_requirement_set_compile_request(uuid,uuid,kb_actor_identity)
TO kb_runtime_api;

COMMIT;
""".strip()
)
OUTPUT.write_text("\n\n".join(parts) + "\n")
