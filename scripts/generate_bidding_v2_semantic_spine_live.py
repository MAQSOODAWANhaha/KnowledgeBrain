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
    "-- Live semantic-spine contract upgrade. Safe for an existing V2 database.\nBEGIN;",
    function(SHARED, "kb_actor_identity_valid"),
]
for function_name in [
    "kb_bid_v2_load_requirement_set_compile_input_v3",
    "kb_bid_v2_publish_requirement_set_v3",
    "kb_bid_v2_create_outline_candidate",
    "kb_bid_v2_outline_semantics_valid",
    "kb_bid_v2_publish_outline_generation",
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
SELECT '00000000-0000-5000-8000-000000000107'::uuid,'agent',1,payload,
  kb_bid_v2_sha256_bytes(payload)
FROM (VALUES (convert_to(
  '{"kind":"outline_agent","version":7,"map_schema":3,"reduce_schema":2,"output_schema":2,"progress_control":"completion_and_stall","max_stalled_turns":2}',
  'UTF8'))) value(payload)
ON CONFLICT (id) DO NOTHING;

REVOKE ALL ON FUNCTION
  kb_bid_v2_load_requirement_set_compile_input_v3(uuid,bigint,kb_sha256),
  kb_bid_v2_publish_requirement_set_v3(uuid,bigint,kb_sha256,jsonb,kb_actor_identity),
  kb_bid_v2_outline_semantics_valid(jsonb,jsonb,jsonb,uuid)
FROM PUBLIC;
GRANT EXECUTE ON FUNCTION
  kb_bid_v2_load_requirement_set_compile_input_v3(uuid,bigint,kb_sha256),
  kb_bid_v2_publish_requirement_set_v3(uuid,bigint,kb_sha256,jsonb,kb_actor_identity)
TO kb_runtime_worker;

COMMIT;
""".strip()
)
OUTPUT.write_text("\n\n".join(parts) + "\n")
