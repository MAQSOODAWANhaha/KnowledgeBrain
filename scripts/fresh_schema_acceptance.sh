#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
: "${KNOWLEDGEBRAIN_TEST_DATABASE_URL:?KNOWLEDGEBRAIN_TEST_DATABASE_URL is required}"
export DATABASE_URL="$KNOWLEDGEBRAIN_TEST_DATABASE_URL"
if [[ "$DATABASE_URL" == *":15432/"* ]]; then
  echo "refusing destructive acceptance against live PostgreSQL :15432" >&2
  exit 2
fi

psql "$DATABASE_URL" -X -v ON_ERROR_STOP=1 <<'SQL'
DROP SCHEMA public CASCADE;
CREATE SCHEMA public;
GRANT ALL ON SCHEMA public TO CURRENT_USER;
SQL
cargo run --locked -p platform --bin migrator
psql "$DATABASE_URL" -X -v ON_ERROR_STOP=1 -f scripts/bidding_v2_phase0_live.sql
psql "$DATABASE_URL" -X -v ON_ERROR_STOP=1 -f scripts/bidding_v2_phase1_live.sql
psql "$DATABASE_URL" -X -v ON_ERROR_STOP=1 -f scripts/bidding_v2_phase1_supersession_live.sql
psql "$DATABASE_URL" -X -v ON_ERROR_STOP=1 -f scripts/bidding_v2_phase3_live.sql
psql "$DATABASE_URL" -X -v ON_ERROR_STOP=1 -f scripts/bidding_v2_phase6_live.sql
schema_state="$(psql "$DATABASE_URL" -X -v ON_ERROR_STOP=1 -At <<'SQL'
SELECT CASE WHEN
  to_regclass('public.workspaces') IS NOT NULL AND
  to_regclass('public.object_registry') IS NOT NULL AND
  to_regclass('public.bid_projects') IS NOT NULL AND
  to_regclass('public.bid_submission_workspaces') IS NOT NULL AND
  to_regclass('public.bid_render_document_snapshot_artifacts') IS NOT NULL AND
  to_regclass('public.bid_submission_manifest_artifacts') IS NOT NULL AND
  to_regclass('public.bid_submission_assessment_snapshot_evidence_items') IS NOT NULL AND
  to_regclass('public.bid_quote_snapshot_artifacts') IS NOT NULL AND
  to_regclass('public.bid_quote_snapshot_object_identities') IS NOT NULL AND
  to_regclass('public.bid_pdf_attachment_preparation_attestations') IS NOT NULL AND
  NOT has_database_privilege('kb_runtime_api',current_database(),'TEMPORARY') AND
  NOT has_database_privilege('kb_runtime_worker',current_database(),'TEMPORARY') AND
  NOT has_database_privilege('kb_runtime_retention',current_database(),'TEMPORARY') AND
  has_function_privilege('kb_runtime_api','vector_in(cstring,oid,integer)','EXECUTE') AND
  has_function_privilege('kb_runtime_worker','vector_in(cstring,oid,integer)','EXECUTE') AND
  has_function_privilege('kb_runtime_api','cosine_distance(vector,vector)','EXECUTE') AND
  has_function_privilege('kb_runtime_worker','cosine_distance(vector,vector)','EXECUTE') AND
  has_function_privilege('kb_runtime_worker','kb_knowledge_has_pending_derived_v2(uuid)','EXECUTE') AND
  EXISTS (SELECT 1 FROM information_schema.columns
    WHERE table_schema='public' AND table_name='bid_workspace_revision_artifacts'
      AND column_name='quote_snapshot_sha256') AND
  to_regprocedure('public.kb_bid_v2_mark_requirement_set_compile_failed(uuid,bigint,kb_sha256,text)') IS NOT NULL AND
  to_regprocedure('public.kb_bid_v2_publish_pdf_attachment_preparation(uuid,bigint,kb_sha256,uuid,uuid,uuid[],uuid[],kb_object_ref[],kb_sha256[],text[],bigint[],integer[],integer[],kb_actor_identity)') IS NOT NULL AND
  to_regprocedure('public.kb_bid_v2_publish_quote_snapshot(uuid,uuid,bigint,uuid,kb_object_ref,kb_sha256,bigint,bytea,kb_actor_identity,text,bytea,kb_sha256)') IS NOT NULL AND
  to_regprocedure('public.kb_bid_v2_get_requirement_set_compile_request(uuid,uuid,kb_actor_identity)') IS NOT NULL AND
  to_regprocedure('public.kb_bid_v2_refresh_requirement_projection(uuid,uuid,kb_sha256,uuid,kb_sha256,kb_actor_identity,text,bytea,kb_sha256)') IS NOT NULL AND
  to_regprocedure('public.kb_bid_v2_apply_quote_snapshot(uuid,uuid,kb_sha256,uuid,kb_sha256,kb_actor_identity,text,bytea,kb_sha256)') IS NOT NULL AND
  to_regprocedure('public.kb_bid_v2_load_user_pick_evidence(uuid,bigint,kb_sha256)') IS NOT NULL AND
  to_regprocedure('public.kb_bid_v2_retry_tender_document(uuid,uuid,uuid,bigint,kb_actor_identity,text,bytea,kb_sha256)') IS NOT NULL AND
  to_regprocedure('public.kb_bid_v2_prepare_workspace_attachment(uuid,uuid,uuid,uuid[],uuid[],integer[],integer[],kb_actor_identity,text,bytea,kb_sha256)') IS NOT NULL AND
  to_regprocedure('public.kb_bid_v2_prepare_submission_export(uuid,bigint,kb_sha256,uuid,kb_object_ref,kb_sha256,text,uuid,uuid,kb_actor_identity)') IS NOT NULL AND
  to_regprocedure('public.kb_bid_v2_load_submission_manifest_render_input(uuid,kb_sha256)') IS NOT NULL AND
  to_regprocedure('public.kb_bid_v2_publish_submission_export(uuid,bigint,kb_sha256,uuid,kb_object_ref,kb_sha256,text,uuid,uuid,uuid,uuid,kb_object_ref,kb_sha256,text,bigint,kb_actor_identity)') IS NOT NULL AND
  to_regclass('public.bid_part_content_artifacts') IS NULL AND
  to_regclass('public.production_launch_state') IS NULL AND
  to_regclass('public.production_first_launch_catalog_verifications') IS NULL
THEN 'fresh-v2-ok' ELSE 'fresh-v2-invalid' END;
SQL
)"
if [[ "$schema_state" != "fresh-v2-ok" ]]; then
  printf 'fresh schema completeness check failed: %s\n' "$schema_state" >&2
  exit 1
fi
printf '%s\n' "$schema_state"

expect_stale_schema_rejected() {
  local label="$1"
  local output
  if output="$(cargo run --locked -p platform --bin migrator 2>&1)"; then
    printf 'migrator accepted stale %s schema\n' "$label" >&2
    return 1
  fi
  if [[ "$output" != *"fresh schema is partial or stale"* ]]; then
    printf 'migrator rejected stale %s schema for the wrong reason:\n%s\n' \
      "$label" "$output" >&2
    return 1
  fi
}

psql "$DATABASE_URL" -X -v ON_ERROR_STOP=1 <<'SQL'
ALTER TABLE bid_async_request_snapshot_artifacts
  DROP CONSTRAINT bid_async_request_snapshot_artifacts_status_check,
  ADD CONSTRAINT bid_async_request_snapshot_artifacts_status_check
    CHECK (status IN ('pending','succeeded','failed','cancelled'));
SQL
expect_stale_schema_rejected "async-request status"
psql "$DATABASE_URL" -X -v ON_ERROR_STOP=1 <<'SQL'
ALTER TABLE bid_async_request_snapshot_artifacts
  DROP CONSTRAINT bid_async_request_snapshot_artifacts_status_check,
  ADD CONSTRAINT bid_async_request_snapshot_artifacts_status_check
    CHECK (status IN ('pending','succeeded','failed'));
ALTER TABLE bid_candidate_artifacts
  DROP CONSTRAINT bid_candidate_artifacts_state_check,
  ADD CONSTRAINT bid_candidate_artifacts_state_check
    CHECK (state IN ('proposed','accepted','rejected','superseded'));
SQL
expect_stale_schema_rejected "candidate state"
psql "$DATABASE_URL" -X -v ON_ERROR_STOP=1 <<'SQL'
ALTER TABLE bid_candidate_artifacts
  DROP CONSTRAINT bid_candidate_artifacts_state_check,
  ADD CONSTRAINT bid_candidate_artifacts_state_check
    CHECK (state IN ('proposed','accepted','rejected'));
ALTER TABLE bid_async_request_snapshot_artifacts
  DROP CONSTRAINT bid_async_request_snapshot_artifacts_status_check,
  ADD CONSTRAINT bid_async_request_snapshot_artifacts_status_check
    CHECK (status IN ('pending','succeeded','failed')) NOT VALID;
SQL
expect_stale_schema_rejected "unvalidated async-request status constraint"
psql "$DATABASE_URL" -X -v ON_ERROR_STOP=1 <<'SQL'
ALTER TABLE bid_async_request_snapshot_artifacts
  VALIDATE CONSTRAINT bid_async_request_snapshot_artifacts_status_check;
ALTER TABLE bid_candidate_artifacts
  DROP CONSTRAINT bid_candidate_artifacts_state_check,
  ADD CONSTRAINT bid_candidate_artifacts_state_check
    CHECK (state IN ('proposed','accepted','rejected')) NOT VALID;
SQL
expect_stale_schema_rejected "unvalidated candidate state constraint"
psql "$DATABASE_URL" -X -v ON_ERROR_STOP=1 <<'SQL'
ALTER TABLE bid_candidate_artifacts
  VALIDATE CONSTRAINT bid_candidate_artifacts_state_check;
DROP FUNCTION kb_bid_v2_get_requirement_set_compile_request(uuid,uuid,kb_actor_identity);
SQL
expect_stale_schema_rejected "typed RequirementSetCompile status function"
psql "$DATABASE_URL" -X -v ON_ERROR_STOP=1 \
  -f migrations/bidding_v2_semantic_spine_live.sql >/dev/null
cargo run --locked -p platform --bin migrator
