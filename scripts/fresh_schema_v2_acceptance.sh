#!/bin/sh
set -eu

cd "$(dirname "$0")/.."

python3 - <<'PY'
import hashlib, json, pathlib, re, tomllib
fixture_path = pathlib.Path("deploy/authoring-v2/migration-manifest.toml")
fixture = tomllib.loads(fixture_path.read_text(encoding="utf-8"))
assert fixture["format_version"] == 1
expected = [
    (1, "knowledge_base_baseline", "knowledge_base_baseline.sql"),
    (2, "shared_platform_baseline", "shared_platform_baseline.sql"),
    (3, "bidding_v2_baseline", "bidding_v2_baseline.sql"),
]
assert [(x["version"], x["name"], x["filename"]) for x in fixture["migrations"]] == expected
for entry in fixture["migrations"]:
    path = pathlib.Path("migrations") / entry["filename"]
    assert entry["sha256"] == hashlib.sha256(path.read_bytes()).hexdigest(), path
bootstrap = fixture["bootstrap"]
assert bootstrap["sha256"] == hashlib.sha256(pathlib.Path(bootstrap["filename"]).read_bytes()).hexdigest()

active = tomllib.loads(pathlib.Path("deploy/first-launch/migration-manifest.toml").read_text(encoding="utf-8"))
assert active["migrations"][-1]["name"] == "bidding_v1_baseline"
assert active["migrations"][-1]["filename"] == "bidding_v1_baseline.sql"
assert all(x["name"] != "bidding_v2_baseline" for x in active["migrations"])
for entry in active["migrations"]:
    path = pathlib.Path("migrations") / entry["filename"]
    assert entry["sha256"] == hashlib.sha256(path.read_bytes()).hexdigest(), ("active", path)
active_shared = {(x["version"], x["name"], x["filename"], x["sha256"]) for x in active["migrations"][:2]}
fixture_shared = {(x["version"], x["name"], x["filename"], x["sha256"]) for x in fixture["migrations"][:2]}
assert active_shared == fixture_shared
assert active["bootstrap"] == fixture["bootstrap"]

active_queue = pathlib.Path("deploy/queue-registry.toml").read_text(encoding="utf-8")
assert "bid-authoring-v2" not in active_queue
fixture_queue = tomllib.loads(pathlib.Path("deploy/authoring-v2/queue-registry.toml").read_text(encoding="utf-8"))
assert len(fixture_queue["entries"]) == 5
assert {x["task_type"] for x in fixture_queue["entries"]} == {
    "bid:tender_document_process:v2", "bid:requirement_set_compile:v2",
    "bid:outline_generate:v2", "bid:content_generate:v2", "bid:submission_export:v2",
}
assert all(x["launch_mode"] == "declared_disabled" for x in fixture_queue["entries"])

schemas = sorted(pathlib.Path("crates/bid/schemas").glob("*.schema.json"))
assert len(schemas) == 10
for path in schemas:
    value = json.loads(path.read_text(encoding="utf-8"))
    assert value["$schema"] == "https://json-schema.org/draft/2020-12/schema"
    assert value["additionalProperties"] is False

sql = pathlib.Path("migrations/bidding_v2_baseline.sql").read_text(encoding="utf-8")
required = {
    "bid_projects", "bid_documents", "bid_document_set_artifacts",
    "bid_source_unit_revision_artifacts", "bid_source_unit_disposition_set_artifacts",
    "bid_requirement_set_artifacts", "bid_submission_workspaces", "bid_workspace_revision_artifacts",
    "bid_workspace_heads", "bid_candidate_artifacts", "bid_evidence_bundle_artifacts",
    "bid_outline_assessment_snapshot_artifacts", "bid_submission_assessment_snapshot_artifacts",
    "bid_renderer_contract_artifacts", "bid_render_document_snapshot_artifacts",
    "bid_render_snapshot_form_definition_items", "bid_render_snapshot_attachment_preparation_items",
    "bid_submission_manifest_artifacts", "bid_submission_output_artifacts", "bid_quote_snapshot_artifacts",
}
created = set(re.findall(r"CREATE TABLE\s+([a-zA-Z0-9_]+)", sql))
assert required <= created, required-created
for forbidden in (
    "bid_part_content_artifacts", "bid_current_parts", "submission_gate", "template_slot",
    "company_profile", "submission_profile", "procedural_classification", "procedural_decision",
    "delivery_attempt", "lease_expires", "retry_count", "fanout", "fanin", "dispatch_head",
):
    assert forbidden not in sql.lower(), forbidden
assert "scope_kind text NOT NULL DEFAULT 'project_wide' CHECK (scope_kind='project_wide')" in sql
assert "kb_bid_v2_advance_workspace_head" in sql
assert "kb_bid_v2_publish_requirement_set(\n  p_artifact_id uuid,p_artifact_sha256 kb_sha256" in sql
assert "p_expected_artifact_id" not in sql[sql.index("CREATE FUNCTION kb_bid_v2_publish_requirement_set"):sql.index("CREATE VIEW bidding_v2_projects")]
assert "kb_bid_v2_validate_fulfillment_binding_target" in sql
assert "mode_options ?& ARRAY['watermark','include_assessment_notices','include_knowledge_sources']" in sql
assert "mode_options - ARRAY['watermark','include_assessment_notices','include_knowledge_sources']::text[] = '{}'::jsonb" in sql
assert "jsonb_typeof(mode_options->'include_assessment_notices') IS NOT DISTINCT FROM 'boolean'" in sql
assert "output_mode='review_draft'" in sql
assert "mode_options @> '{\"watermark\":null,\"include_assessment_notices\":false,\"include_knowledge_sources\":false}'::jsonb" in sql
assert "kb_bid_v2_guard_async_request_initial_state" in sql
assert "async request initial status must be pending" in sql
assert "kb_bid_v2_guard_candidate_initial_state" in sql
assert "candidate initial state must be proposed and undecided" in sql
assert "UNIQUE(project_id,workspace_id,id,scope_revision_id,requirement_projection_id,document_settings_revision_id)" in sql
assert "UNIQUE(project_id,workspace_id,id,requirement_projection_id,requirement_projection_sha256)" in sql
assert "FOREIGN KEY(project_id,workspace_id,workspace_revision_id,requirement_projection_id,requirement_projection_sha256)" in sql
assert "REFERENCES bid_workspace_revision_artifacts(project_id,workspace_id,id,requirement_projection_id,requirement_projection_sha256)" in sql
assert "FOREIGN KEY(project_id,workspace_id,workspace_revision_id,scope_revision_id,requirement_projection_id,document_settings_revision_id)" in sql
assert "submission_assessment_snapshot_sha256 kb_sha256 NOT NULL" in sql
assert "FOREIGN KEY(project_id,workspace_id,submission_assessment_snapshot_id,workspace_revision_id,scope_revision_id,requirement_projection_id,document_settings_revision_id,submission_assessment_snapshot_sha256)" in sql
assert "preparation_status text NOT NULL DEFAULT 'ready' CHECK (preparation_status='ready')" in sql
assert "FOREIGN KEY(project_id,workspace_id,attachment_preparation_revision_id,preparation_status,canonical_sha256)" in sql
assert "FOREIGN KEY(project_id,workspace_id,render_snapshot_id,output_mode,format,mode_options)" in sql
assert "FOREIGN KEY(project_id,workspace_id,manifest_id,format)" in sql
assert "REFERENCES knowledge_matching_scope_attestations_v2(id,content_sha256)" in sql
assert "REFERENCES object_registry(object_ref,digest,media_type,state)" in sql
assert "REFERENCES knowledge_image_artifact_revisions(id,object_ref,content_sha256,media_type,object_state)" in sql
assert "item_payload->>'evidence_item_id')::uuid=id" in sql
assert "item_payload->>'kind' IS NOT DISTINCT FROM item_kind" in sql
assert "UNION ALL SELECT 'outline_checkpoint',outline_checkpoint_id,outline_checkpoint_sha256 FROM manifest" in sql
knowledge_sql = pathlib.Path("migrations/knowledge_base_baseline.sql").read_text(encoding="utf-8")
for fragment in (
    "CREATE TABLE knowledge_image_artifact_revisions",
    "CREATE TABLE knowledge_image_ocr_chunk_artifact_mappings",
    "REFERENCES chunks(id,product_version_id,document_id)",
    "KNOWLEDGE_IMAGE_OCR_MAPPING_SOURCE_INVALID",
):
    assert fragment in knowledge_sql
assert "KnowledgeEvidenceHitV3" not in knowledge_sql
assert "canonical_payload-'snapshot_sha256'" in sql
assert "canonical_payload-'bundle_sha256'" in sql
assert "kb_bid_v2_verify_render_snapshot_projection" in sql
assert "kb_bid_v2_verify_evidence_bundle_projection" in sql
assert "kb_bid_v2_manifest_expected_dependencies" in sql
assert "kb_bid_v2_verify_manifest_dependency_set" in sql
for fragment in (
    "kb_bid_v2_rfc3339_datetime_matches",
    "EvidenceAsset knowledge media qualified identity mismatch",
    "ADD UNIQUE NULLS NOT DISTINCT(id,object_ref,content_sha256,media_type,object_state,width,height,page_ordinal,bounding_region)",
    "REFERENCES bid_workspace_asset_artifacts(project_id,workspace_id,id)",
    "canonical_payload-'preparation_sha256'",
    "kb_bid_v2_verify_attachment_preparation_projection",
    "REFERENCES object_registry(object_ref,digest,media_type,byte_length,state)",
    "REFERENCES object_owner_references(object_ref,owner_kind,owner_id,occurrence)",
):
    assert fragment in sql, fragment
live_fixture = pathlib.Path("scripts/bidding_v2_phase0_live.sql").read_text(encoding="utf-8")
assert "\\if false" not in live_fixture
assert "kb_knowledge_attest_matching_scope_v2(scope)" in live_fixture
assert "INSERT INTO knowledge_matching_scope_attestations_v2" not in live_fixture
assert "EXCEPTION WHEN check_violation OR unique_violation" not in live_fixture
for fragment in (
    "phase0_expected_manifest_dependencies",
    "SELECT pg_temp.assert_evidence_media_rejected('MIME'",
    "2026-01-01T00:00:00+24:00",
    "reordered preparation pages accepted",
    "missing output owner accepted",
):
    assert fragment in live_fixture, fragment
manifest_fixture = live_fixture[live_fixture.index("-- Formal manifest"):live_fixture.index("-- Output bytes")]
assert "FROM kb_bid_v2_manifest_expected_dependencies" not in manifest_fixture
print("V2 static manifest/schema/queue/SQL contract: PASS")
PY
python3 scripts/bid_authoring_schema_validation.py

if [ "${RUN_LIVE_V2_SCHEMA:-0}" != "1" ]; then
  echo "V2 live fresh-schema apply: SKIP (set RUN_LIVE_V2_SCHEMA=1; static contract passed)"
  exit 0
fi

command -v docker >/dev/null 2>&1 || {
  echo "docker unavailable" >&2
  exit 1
}
container="kb-v2-schema-${GITHUB_RUN_ID:-local}-$$"
cleanup() { docker rm -f -v "$container" >/dev/null 2>&1 || true; }
trap cleanup EXIT HUP INT TERM
cleanup

docker run -d --rm --name "$container" \
  --tmpfs /var/lib/postgresql/data:rw,noexec,nosuid,size=1g \
  -e POSTGRES_USER=knowledgebrain -e POSTGRES_PASSWORD=knowledgebrain \
  -e POSTGRES_DB=knowledgebrain \
  -e KNOWLEDGEBRAIN_MIGRATOR_PASSWORD=migrator-test \
  -e KNOWLEDGEBRAIN_FIRST_LAUNCH_VERIFIER_PASSWORD=verifier-test \
  -e KNOWLEDGEBRAIN_API_DB_PASSWORD=api-test \
  -e KNOWLEDGEBRAIN_WORKER_DB_PASSWORD=worker-test \
  -e KNOWLEDGEBRAIN_RETENTION_DB_PASSWORD=retention-test \
  -v "$PWD/deploy/postgres-init:/docker-entrypoint-initdb.d:ro" \
  -v "$PWD/migrations:/workspace/migrations:ro" \
  pgvector/pgvector:0.8.6-pg16 >/dev/null

# The role is created early by the init script, so it is not a readiness fence.
# Wait for the entrypoint's completed-init marker and the final postmaster, then
# verify every bootstrap role before applying the isolated V2 manifest.
ready=0
for _ in $(seq 1 180); do
  if docker logs "$container" 2>&1 | grep -q 'PostgreSQL init process complete; ready for start up' \
    && docker exec "$container" pg_isready -U knowledgebrain -d knowledgebrain >/dev/null 2>&1 \
    && [ "$(docker exec -e PGPASSWORD=knowledgebrain "$container" \
      psql -X -U knowledgebrain -d knowledgebrain -Atc \
      "SELECT count(*)=5 FROM pg_roles WHERE rolname IN ('kb_migrator','kb_first_launch_verifier','kb_runtime_api','kb_runtime_worker','kb_runtime_retention')" 2>/dev/null)" = "t" ]; then
    ready=1
    break
  fi
  sleep 1
done
[ "$ready" = "1" ] || {
  docker logs "$container" >&2
  echo "final PostgreSQL bootstrap readiness timed out" >&2
  exit 1
}

for file in knowledge_base_baseline.sql shared_platform_baseline.sql bidding_v2_baseline.sql; do
  docker exec -e PGPASSWORD=knowledgebrain "$container" \
    psql -X -v ON_ERROR_STOP=1 -U knowledgebrain -d knowledgebrain \
    -f "/workspace/migrations/$file" >/dev/null
done
checks=$(docker exec -e PGPASSWORD=knowledgebrain "$container" \
  psql -X -U knowledgebrain -d knowledgebrain -Atc "
SELECT to_regclass('public.bid_submission_workspaces') IS NOT NULL;
SELECT to_regclass('public.bid_workspace_heads') IS NOT NULL;
SELECT to_regclass('public.bid_submission_gate_issues') IS NULL;
SELECT to_regclass('public.bid_part_content_artifacts') IS NULL;
SELECT EXISTS (SELECT 1 FROM pg_index WHERE indrelid='bid_submission_workspaces'::regclass
 AND indisunique AND pg_get_indexdef(indexrelid) LIKE '%(project_id)%');
SELECT has_function_privilege('kb_runtime_api','kb_bid_v2_advance_workspace_head(uuid,uuid,kb_sha256,uuid,kb_sha256)','EXECUTE');
SELECT has_function_privilege('kb_runtime_worker','kb_bid_v2_publish_requirement_set(uuid,kb_sha256)','EXECUTE');
")
[ "$checks" = "$(printf 't\nt\nt\nt\nt\nt\nt')" ]

docker exec -i -e PGPASSWORD=knowledgebrain "$container" \
  psql -X -v ON_ERROR_STOP=1 -U knowledgebrain -d knowledgebrain \
  < scripts/bidding_v2_phase0_live.sql >/dev/null

docker exec -i -e PGPASSWORD=knowledgebrain "$container" \
  psql -X -v ON_ERROR_STOP=1 -U knowledgebrain -d knowledgebrain \
  < scripts/bidding_v2_phase1_live.sql >/dev/null

render_payload=$(docker exec -e PGPASSWORD=knowledgebrain "$container" \
  psql -X -U knowledgebrain -d knowledgebrain -Atc \
  "SELECT canonical_payload::text FROM bid_render_document_snapshot_artifacts WHERE id='00000000-0000-4000-8000-000000000127'")
RENDER_PAYLOAD="$render_payload" python3 - <<'PY'
import json
import os
from pathlib import Path
from jsonschema import Draft202012Validator, FormatChecker
schema = json.loads(Path("crates/bid/schemas/render-document-snapshot-v2.schema.json").read_text())
payload = json.loads(os.environ["RENDER_PAYLOAD"])
Draft202012Validator(schema, format_checker=FormatChecker()).validate(payload)
assert payload["font_artifact_identities"] and payload["ordered_nodes"]
assert payload["ordered_nodes"][0]["block_occurrences"]
PY

evidence_payload=$(docker exec -e PGPASSWORD=knowledgebrain "$container" \
  psql -X -U knowledgebrain -d knowledgebrain -Atc \
  "SELECT canonical_payload::text FROM bid_evidence_bundle_artifacts WHERE id='00000000-0000-4000-8000-000000000143'")
EVIDENCE_PAYLOAD="$evidence_payload" python3 - <<'PY'
import json
import os
from pathlib import Path
from jsonschema import Draft202012Validator, FormatChecker
schema = json.loads(Path("crates/bid/schemas/evidence-bundle-v1.schema.json").read_text())
payload = json.loads(os.environ["EVIDENCE_PAYLOAD"])
Draft202012Validator(schema, format_checker=FormatChecker()).validate(payload)
assert payload["items"] and payload["items"][0]["kind"] == "image"
PY

if rg -n '\\\\if false' scripts/bidding_v2_phase0_live.sql; then
  echo "disabled V2 live regression block remains" >&2
  exit 1
fi

echo "V2 live fresh-schema: Phase 0 contracts plus Phase 1 owner/idempotency, role/relation, DocumentSet and RequirementProjection: PASS"
