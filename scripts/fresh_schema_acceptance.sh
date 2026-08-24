#!/bin/sh
set -eu

cd "$(dirname "$0")/.."

python3 - <<'PY'
import hashlib, pathlib, re, tomllib
manifest_path = pathlib.Path("deploy/first-launch/migration-manifest.toml")
manifest = tomllib.loads(manifest_path.read_text(encoding="utf-8"))
expected = [
    (1, "knowledge_base_baseline", "knowledge_base_baseline.sql"),
    (2, "shared_platform_baseline", "shared_platform_baseline.sql"),
    (3, "bidding_v1_baseline", "bidding_v1_baseline.sql"),
]
assert manifest["format_version"] == 1
assert [(x["version"], x["name"], x["filename"]) for x in manifest["migrations"]] == expected
bootstrap = manifest["bootstrap"]
assert bootstrap["filename"] == "deploy/postgres-init/010-runtime-identities.sh"
assert bootstrap["sha256"] == hashlib.sha256(pathlib.Path(bootstrap["filename"]).read_bytes()).hexdigest()
for entry in manifest["migrations"]:
    path = pathlib.Path("migrations") / entry["filename"]
    assert entry["sha256"] == hashlib.sha256(path.read_bytes()).hexdigest(), path
assert sorted(path.name for path in pathlib.Path("migrations").glob("*.sql")) == sorted(x[2] for x in expected)

# Deferred constraint triggers execute when the checked SECURITY DEFINER
# mutation has returned to its runtime invoker. Their trigger functions must
# therefore carry their own fixed-search-path SECURITY DEFINER boundary.
bidding_sql = pathlib.Path("migrations/bidding_v1_baseline.sql").read_text(encoding="utf-8")
trigger_functions = re.findall(
    r"CREATE CONSTRAINT TRIGGER\s+\S+[\s\S]*?EXECUTE FUNCTION\s+([a-zA-Z0-9_]+)\s*\(",
    bidding_sql,
)
assert trigger_functions
for function_name in sorted(set(trigger_functions)):
    header = re.search(
        rf"CREATE FUNCTION {re.escape(function_name)}\([^)]*\)[\s\S]*?AS \$\$",
        bidding_sql,
    )
    assert header is not None, function_name
    assert "SECURITY DEFINER" in header.group(0), function_name
    assert "SET search_path = pg_catalog, public" in header.group(0), function_name
PY

cargo build -q -p migrator -p first-launch-verifier -p api -p worker -p retention

container="kb-fresh-schema-${GITHUB_RUN_ID:-local}-$$"
port="${KNOWLEDGEBRAIN_SCHEMA_TEST_PORT:-55439}"
cleanup() {
 docker rm -f -v "$container" >/dev/null 2>&1 || true
}
trap cleanup EXIT HUP INT TERM
cleanup

docker run -d --name "$container" -p "$port:5432" \
 -e POSTGRES_USER=knowledgebrain \
 -e POSTGRES_PASSWORD=knowledgebrain \
 -e POSTGRES_DB=knowledgebrain \
 -e KNOWLEDGEBRAIN_MIGRATOR_PASSWORD=migrator-test \
 -e KNOWLEDGEBRAIN_FIRST_LAUNCH_VERIFIER_PASSWORD=verifier-test \
 -e KNOWLEDGEBRAIN_API_DB_PASSWORD=api-test \
 -e KNOWLEDGEBRAIN_WORKER_DB_PASSWORD=worker-test \
 -e KNOWLEDGEBRAIN_RETENTION_DB_PASSWORD=retention-test \
 -v "$PWD/deploy/postgres-init:/docker-entrypoint-initdb.d:ro" \
 pgvector/pgvector:0.8.6-pg16 >/dev/null

for _ in $(seq 1 60); do
 if docker exec "$container" pg_isready -U knowledgebrain -d knowledgebrain >/dev/null 2>&1; then
  break
 fi
 sleep 1
done
docker exec "$container" pg_isready -U knowledgebrain -d knowledgebrain >/dev/null
for _ in $(seq 1 60); do
 if docker exec -e PGPASSWORD=migrator-test "$container" \
  psql -X -U kb_migrator -d knowledgebrain -Atc 'SELECT 1' >/dev/null 2>&1; then
  break
 fi
 sleep 1
done
docker exec -e PGPASSWORD=migrator-test "$container" \
 psql -X -U kb_migrator -d knowledgebrain -Atc 'SELECT 1' >/dev/null

# Runtime binaries are verification-only and must fail against an empty catalog
# without creating even the migration ledger.
if DATABASE_URL="postgres://kb_runtime_api:api-test@127.0.0.1:$port/knowledgebrain" \
 target/debug/api >/tmp/kb-empty-api.out 2>/tmp/kb-empty-api.err; then
 echo 'runtime API started against an empty catalog' >&2
 exit 1
fi
empty_ledger=$(docker exec -e PGPASSWORD=knowledgebrain "$container" \
 psql -X -U knowledgebrain -d knowledgebrain -Atc \
 "SELECT to_regclass('public.schema_migrations') IS NULL")
[ "$empty_ledger" = "t" ]

DATABASE_URL="postgres://kb_migrator:migrator-test@127.0.0.1:$port/knowledgebrain" \
 KNOWLEDGEBRAIN_BOOTSTRAP_ADMIN_DATABASE_URL="postgres://knowledgebrain:knowledgebrain@127.0.0.1:$port/knowledgebrain" \
 target/debug/migrator

DATABASE_URL="postgres://kb_first_launch_verifier:verifier-test@127.0.0.1:$port/knowledgebrain" \
 KNOWLEDGEBRAIN_APP_OWNER=kb_app_owner \
 KNOWLEDGEBRAIN_BOOTSTRAP_OWNER=knowledgebrain \
 target/debug/first-launch-verifier

admin_psql() {
 docker exec -e PGPASSWORD=knowledgebrain "$container" \
  psql -X -v ON_ERROR_STOP=1 -U knowledgebrain -d knowledgebrain "$@"
}
role_psql() {
 role=$1 password=$2
 shift 2
 docker exec -e PGPASSWORD="$password" "$container" \
  psql -X -v ON_ERROR_STOP=1 -U "$role" -d knowledgebrain "$@"
}
expect_denied() {
 role=$1 password=$2 sql=$3
 if role_psql "$role" "$password" -c "$sql" >/tmp/kb-schema-denied.out 2>/tmp/kb-schema-denied.err; then
  echo "expected denial for $role: $sql" >&2
  exit 1
 fi
}

catalog_checks=$(admin_psql -Atc "
SELECT array_agg(version ORDER BY version) = ARRAY[1,2,3] FROM schema_migrations;
SELECT count(*) = 0 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE n.nspname='public' AND c.relname = ANY(ARRAY[
  'content_' || 'objects','bid_' || 'picks','bid_' || 'booklet_parts','bid_' || 'commercial_hits',
  'bid_' || 'extract_runs','bid_' || 'sections','bid_' || 'shots','bid_' || 'section_retry_jobs',
  'bid_' || 'match_jobs','bid_' || 'booklet_generated'
 ]);
SELECT count(*) = 1 FROM pg_roles WHERE rolname='kb_runtime_retention' AND rolcanlogin;
")
[ "$catalog_checks" = "$(printf 't\nt\nt')" ]

# Seed the unchanged knowledge hierarchy. Direct document insertion must fail
# because the deferred ObjectRegistry/reference verifier sees no owner reference.
admin_psql -c "
INSERT INTO users(id,email) VALUES('10000000-0000-0000-0000-000000000001','schema@example.test');
INSERT INTO workspaces(id,name,slug) VALUES('10000000-0000-0000-0000-000000000002','Schema','schema');
INSERT INTO products(id,workspace_id,kind,name,slug)
VALUES('10000000-0000-0000-0000-000000000003','10000000-0000-0000-0000-000000000002','product','P','p');
INSERT INTO product_versions(id,product_id,label,status)
VALUES('10000000-0000-0000-0000-000000000004','10000000-0000-0000-0000-000000000003','v1','active');
"

digest=ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
expect_denied kb_runtime_api api-test "
INSERT INTO documents(id,product_version_id,title,parse_status,file_name,file_size,file_hash,object_ref)
VALUES('10000000-0000-0000-0000-000000000005','10000000-0000-0000-0000-000000000004','unsafe','pending','a',3,'$digest','objects/$digest')"

role_psql kb_runtime_api api-test -c "
BEGIN;
INSERT INTO documents(id,product_version_id,title,parse_status,file_name,file_size,file_hash,object_ref)
VALUES('10000000-0000-0000-0000-000000000005','10000000-0000-0000-0000-000000000004','safe','pending','a',3,'$digest','objects/$digest');
SELECT kb_register_knowledge_document_object(
 '10000000-0000-0000-0000-000000000005','text/plain','system:knowledge-document-ingest','register-1',
 '10000000-0000-0000-0000-000000000006');
COMMIT;
" >/dev/null
role_psql kb_runtime_api api-test -Atc "SELECT count(*) FROM available_object_registry WHERE object_ref='objects/$digest'" | grep -qx '1'

# System actors are a fixed V1 allowlist, not any bounded string. Replaying the
# same request returns the original receipt and never appends a second audit;
# changing the exact operation payload under that key fails closed.
expect_denied kb_runtime_api api-test "SELECT 'system:not-allowlisted'::kb_actor_identity"
role_psql kb_runtime_api api-test -Atc "SELECT kb_register_knowledge_document_object(
 '10000000-0000-0000-0000-000000000005','text/plain','system:knowledge-document-ingest','register-1',
 '10000000-0000-0000-0000-000000000016')" | grep -qx "objects/$digest"
admin_psql -Atc "SELECT count(*) FROM audit_events
 WHERE operation='knowledge.document.object.register'
   AND actor_identity='system:knowledge-document-ingest' AND idempotency_key='register-1'" | grep -qx '1'
expect_denied kb_runtime_api api-test "SELECT kb_register_knowledge_document_object(
 '10000000-0000-0000-0000-000000000005','application/pdf','system:knowledge-document-ingest','register-1',
 '10000000-0000-0000-0000-000000000017')"

# A second owner reference protects the shared digest. Releasing only the first
# owner must not queue deletion; releasing the last owner must do so atomically.
role_psql kb_runtime_api api-test -c "
BEGIN;
INSERT INTO documents(id,product_version_id,title,parse_status,file_name,file_size,file_hash,object_ref)
VALUES('10000000-0000-0000-0000-000000000015','10000000-0000-0000-0000-000000000004','safe-2','pending','b',3,'$digest','objects/$digest');
SELECT kb_register_knowledge_document_object(
 '10000000-0000-0000-0000-000000000015','text/plain','system:knowledge-document-ingest','register-2',
 '10000000-0000-0000-0000-000000000018');
COMMIT;
" >/dev/null

# Bidding and platform internals are function/view-only for API/worker. The
# retention login has only retention functions and no business-table access.
role_psql kb_runtime_api api-test -Atc "SELECT count(*) FROM bidding_current_clauses" | grep -qx '0'
role_psql kb_runtime_api api-test -Atc "SELECT has_function_privilege('kb_runtime_api','kb_bid_read_manifest_render_asset(uuid,uuid,uuid)','EXECUTE')" | grep -qx 't'
role_psql kb_runtime_worker worker-test -Atc "SELECT count(*) FROM bidding_current_matching_reports" | grep -qx '0'
role_psql kb_runtime_worker worker-test -Atc "SELECT count(*) FROM bid_matching_jobs job LEFT JOIN bidding_matching_report_history report ON report.id=job.completed_report_id" | grep -qx '0'
role_psql kb_runtime_worker worker-test -Atc "SELECT has_function_privilege('kb_runtime_worker','kb_bid_manifest_render_input(uuid,uuid)','EXECUTE')" | grep -qx 't'
role_psql kb_runtime_worker worker-test -Atc "SELECT has_function_privilege('kb_runtime_worker','kb_bid_complete_document_conversion(uuid,uuid,uuid,bytea,text,kb_sha256,jsonb,kb_actor_identity)','EXECUTE')" | grep -qx 't'
role_psql kb_runtime_worker worker-test -Atc "SELECT has_function_privilege('kb_runtime_worker','kb_bid_sync_project_procedural(uuid,kb_actor_identity)','EXECUTE')" | grep -qx 'f'

profile_project=10000000-0000-0000-0000-000000000037
profile_actor=user:10000000-0000-0000-0000-000000000001
empty_sha=e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
role_psql kb_runtime_api api-test -Atc "SELECT (kb_bid_create_project(
 '$profile_project','Profile contract','10000000-0000-0000-0000-000000000001',
 clock_timestamp()+interval '30 days',NULL,'$profile_actor','profile-project','\\x','$empty_sha'))->>'id'" |
 grep -qx "$profile_project"

# ProjectPickSet revisions are a project-scoped sequence. Keep the first
# transaction open after rebuilding so the second call deterministically races
# with its uncommitted artifact; both calls must commit with consecutive
# revisions and the current pointer must select the latter artifact.
pick_revision_before=$(admin_psql -Atc "SELECT COALESCE(max(revision),0)
 FROM bid_project_pick_set_artifacts WHERE project_id='$profile_project'")
docker exec -e PGPASSWORD=knowledgebrain "$container" \
 psql -X -v ON_ERROR_STOP=1 -U knowledgebrain -d knowledgebrain -c "
BEGIN;
SELECT kb_bid_rebuild_project_pick_set('$profile_project','$profile_actor');
SELECT pg_sleep(2);
COMMIT;
" >/dev/null &
first_pick_pid=$!
sleep 1
docker exec -e PGPASSWORD=knowledgebrain "$container" \
 psql -X -v ON_ERROR_STOP=1 -U knowledgebrain -d knowledgebrain -c "
SELECT kb_bid_rebuild_project_pick_set('$profile_project','$profile_actor');
" >/dev/null &
second_pick_pid=$!
wait "$first_pick_pid"
wait "$second_pick_pid"
pick_concurrency_checks=$(admin_psql -Atc "
SELECT count(*)=2 FROM bid_project_pick_set_artifacts
 WHERE project_id='$profile_project' AND revision>'$pick_revision_before';
SELECT min(revision)='$pick_revision_before'::bigint+1
   AND max(revision)='$pick_revision_before'::bigint+2
  FROM bid_project_pick_set_artifacts
 WHERE project_id='$profile_project' AND revision>'$pick_revision_before';
SELECT current_value.revision='$pick_revision_before'::bigint+2
   AND artifact.revision=current_value.revision
  FROM bid_current_project_pick_sets current_value
  JOIN bid_project_pick_set_artifacts artifact ON artifact.id=current_value.pick_set_id
 WHERE current_value.project_id='$profile_project';
")
[ "$pick_concurrency_checks" = "$(printf 't\nt\nt')" ]

role_psql kb_runtime_api api-test -Atc "SELECT (kb_bid_update_company_profile(
 '$profile_project',0,'示例公司','91310000MA00000001','示例地址','张三','李四','13800000000','bid@example.test',
 '$profile_actor','profile-company','\\x','$empty_sha'))->>'revision'" | grep -qx '1'
role_psql kb_runtime_api api-test -Atc "SELECT (kb_bid_update_submission_profile(
 '$profile_project',0,'示例采购人','KB-PROFILE','李四',current_date+30,'上海',true,true,
 '$profile_actor','profile-submission','\\x','$empty_sha'))->>'revision'" | grep -qx '1'
# Profile content hashes protect payload integrity, but identical company and
# submission data is valid in separate projects and must not collide globally.
profile_project_two=10000000-0000-0000-0000-000000000040
role_psql kb_runtime_api api-test -Atc "SELECT (kb_bid_create_project(
 '$profile_project_two','Second profile contract','10000000-0000-0000-0000-000000000001',
 clock_timestamp()+interval '30 days',NULL,'$profile_actor','profile-project-two','\\x','$empty_sha'))->>'id'" |
 grep -qx "$profile_project_two"
role_psql kb_runtime_api api-test -Atc "SELECT (kb_bid_update_company_profile(
 '$profile_project_two',0,'示例公司','91310000MA00000001','示例地址','张三','李四','13800000000','bid@example.test',
 '$profile_actor','profile-company-two','\\x','$empty_sha'))->>'revision'" | grep -qx '1'
role_psql kb_runtime_api api-test -Atc "SELECT (kb_bid_update_submission_profile(
 '$profile_project_two',0,'示例采购人','KB-PROFILE','李四',current_date+30,'上海',true,true,
 '$profile_actor','profile-submission-two','\\x','$empty_sha'))->>'revision'" | grep -qx '1'
part_unit=10000000-0000-0000-0000-000000000039
admin_psql -Atc "SELECT position(
 '# 技术响应 2:$part_unit' IN kb_bid_build_part_markdown('$profile_project','2:$part_unit')
)=1" | grep -qx 't'
role_psql kb_runtime_api api-test -Atc "SELECT (kb_bid_create_quote_draft(
 '$profile_project','tax_exclusive','示例报价','',
 '$profile_actor','profile-quote','\\x','$empty_sha'))->>'edit_version'" | grep -qx '0'
role_psql kb_runtime_api api-test -Atc "SELECT (kb_bid_upsert_quote_line(
 '$profile_project','10000000-0000-0000-0000-000000000038',0,0,
 '示例报价项','unit_price','2','项','50',NULL,'0.13',true,
 '$profile_actor','profile-quote-line','\\x','$empty_sha'))->>'edit_version'" | grep -qx '1'
role_psql kb_runtime_api api-test -Atc "SELECT (kb_bid_quote_state_json('$profile_project'))->>'pointer'" |
 grep -qx 'draft'

expect_denied kb_runtime_api api-test "INSERT INTO bid_projects(id,title,owner_user_id,ends_at,fact_sha256,ceiling_identity_sha256,created_by) VALUES(gen_random_uuid(),'x','10000000-0000-0000-0000-000000000001',now(),repeat('0',64),repeat('0',64),'system:test')"
expect_denied kb_runtime_worker worker-test "UPDATE object_registry SET state='deleted'"
expect_denied kb_runtime_api api-test "SELECT * FROM kb_retention_claim(gen_random_uuid(),'bad',60000)"
expect_denied kb_runtime_retention retention-test "SELECT count(*) FROM bid_projects"

# Releasing the last owner reference soft-deletes the business row and queues
# retention atomically. A new owner cannot revive the digest while deleting.
role_psql kb_runtime_api api-test -Atc "SELECT kb_release_knowledge_document_object(
 '10000000-0000-0000-0000-000000000005','system:knowledge-document-delete','release-1',
 '10000000-0000-0000-0000-000000000007')" | grep -qx 'f'
first_release_checks=$(admin_psql -Atc "SELECT deleted_at IS NOT NULL FROM documents WHERE id='10000000-0000-0000-0000-000000000005'; SELECT state FROM object_registry WHERE object_ref='objects/$digest'; SELECT count(*) FROM object_owner_references WHERE object_ref='objects/$digest'")
[ "$first_release_checks" = "$(printf 't\navailable\n1')" ]
role_psql kb_runtime_api api-test -Atc "SELECT kb_release_knowledge_document_object(
 '10000000-0000-0000-0000-000000000015','system:knowledge-document-delete','release-2',
 '10000000-0000-0000-0000-000000000019')" | grep -qx 't'
release_checks=$(admin_psql -Atc "SELECT deleted_at IS NOT NULL FROM documents WHERE id='10000000-0000-0000-0000-000000000015'; SELECT state FROM object_registry WHERE object_ref='objects/$digest'")
[ "$release_checks" = "$(printf 't\ndeleting')" ]

# A deleting object cannot be revived through a new business owner.
expect_denied kb_runtime_api api-test "BEGIN;
INSERT INTO documents(id,product_version_id,title,parse_status,file_name,file_size,file_hash,object_ref)
VALUES('10000000-0000-0000-0000-000000000025','10000000-0000-0000-0000-000000000004','unsafe-revive','pending','c',3,'$digest','objects/$digest');
SELECT kb_register_knowledge_document_object(
 '10000000-0000-0000-0000-000000000025','text/plain','system:knowledge-document-ingest','register-revive',
 '10000000-0000-0000-0000-000000000026');
COMMIT;"

claim=10000000-0000-0000-0000-000000000008
role_psql kb_runtime_retention retention-test -Atc "SELECT object_ref FROM kb_retention_claim('$claim','schema-retention',60000)" | grep -qx "objects/$digest"
# A lost claim response is replayed with the same token and does not increment
# the attempt. Heartbeat extends only the live token lease.
role_psql kb_runtime_retention retention-test -Atc "SELECT object_ref FROM kb_retention_claim('$claim','schema-retention',60000)" | grep -qx "objects/$digest"
role_psql kb_runtime_retention retention-test -Atc "SELECT kb_retention_heartbeat('objects/$digest','$claim',60000)" | grep -qx 't'
role_psql kb_runtime_retention retention-test -Atc "SELECT attempt FROM kb_retention_claim('$claim','schema-retention',60000)" | grep -qx '1'

# A retry receipt is idempotent after response loss. The next claim uses a new
# token; once that lease expires, a reclaim gets a new token and stale CAS calls
# fail closed. The completion receipt is likewise replayable.
role_psql kb_runtime_retention retention-test -Atc "SELECT kb_retention_fail('objects/$digest','$claim','OBJECT_DELETE_TIMEOUT')" | grep -qx 't'
role_psql kb_runtime_retention retention-test -Atc "SELECT kb_retention_fail('objects/$digest','$claim','OBJECT_DELETE_TIMEOUT')" | grep -qx 't'
admin_psql -c "UPDATE object_retention_outbox SET next_attempt_at=clock_timestamp() WHERE object_ref='objects/$digest'" >/dev/null
claim2=10000000-0000-0000-0000-000000000009
role_psql kb_runtime_retention retention-test -Atc "SELECT attempt FROM kb_retention_claim('$claim2','schema-retention',60000)" | grep -qx '2'
admin_psql -c "UPDATE object_retention_outbox SET lease_until=clock_timestamp()-interval '1 second' WHERE object_ref='objects/$digest'" >/dev/null
claim3=10000000-0000-0000-0000-000000000010
# Claim-response replay is valid only while its lease is live. An expired token
# cannot renew itself before another worker reclaims the item.
expect_denied kb_runtime_retention retention-test "SELECT * FROM kb_retention_claim('$claim2','schema-retention',60000)"
role_psql kb_runtime_retention retention-test -Atc "SELECT attempt FROM kb_retention_claim('$claim3','schema-retention',60000)" | grep -qx '3'
admin_psql -Atc "SELECT count(*) FROM object_retention_attempt_receipts
 WHERE claim_token='$claim2' AND attempt=2 AND outcome='retry' AND error_code='LEASE_EXPIRED'" | grep -qx '1'
role_psql kb_runtime_retention retention-test -Atc "SELECT kb_retention_heartbeat('objects/$digest','$claim2',60000)" | grep -qx 'f'
expect_denied kb_runtime_retention retention-test "SELECT * FROM kb_retention_claim('$claim2','schema-retention',60000)"
expect_denied kb_runtime_retention retention-test "SELECT kb_retention_complete('objects/$digest','$claim2')"
role_psql kb_runtime_retention retention-test -Atc "SELECT kb_retention_complete('objects/$digest','$claim3')" | grep -qx 't'
role_psql kb_runtime_retention retention-test -Atc "SELECT kb_retention_complete('objects/$digest','$claim3')" | grep -qx 't'
retention_checks=$(admin_psql -Atc "SELECT state FROM object_registry WHERE object_ref='objects/$digest'; SELECT count(*) FROM object_retention_tombstones WHERE object_ref='objects/$digest'")
[ "$retention_checks" = "$(printf 'deleted\n1')" ]

# Tombstones make deletion permanent for a digest in V1.
expect_denied kb_runtime_api api-test "BEGIN;
INSERT INTO documents(id,product_version_id,title,parse_status,file_name,file_size,file_hash,object_ref)
VALUES('10000000-0000-0000-0000-000000000035','10000000-0000-0000-0000-000000000004','unsafe-deleted','pending','d',3,'$digest','objects/$digest');
SELECT kb_register_knowledge_document_object(
 '10000000-0000-0000-0000-000000000035','text/plain','system:knowledge-document-ingest','register-deleted',
 '10000000-0000-0000-0000-000000000036');
COMMIT;"

# One-shot identities are disabled after verification.
if role_psql kb_migrator migrator-test -c 'SELECT 1' >/dev/null 2>&1; then
 echo 'migrator login remained enabled' >&2
 exit 1
fi
if role_psql kb_first_launch_verifier verifier-test -c 'SELECT 1' >/dev/null 2>&1; then
 echo 'verifier login remained enabled' >&2
 exit 1
fi

printf '%s\n' 'fresh schema, catalog, seed, actor/idempotency/object, and ACL acceptance passed'
