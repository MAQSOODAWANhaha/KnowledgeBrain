#!/bin/sh
set -eu

cd "$(dirname "$0")/.."

python3 - <<'PY'
import hashlib, pathlib, tomllib
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
PY

cargo build -q -p migrator -p first-launch-verifier -p api -p worker -p retention

container="kb-fresh-schema-${GITHUB_RUN_ID:-local}-$$"
port="${KNOWLEDGEBRAIN_SCHEMA_TEST_PORT:-55439}"
cleanup() {
  docker rm -f "$container" >/dev/null 2>&1 || true
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
  'content_' || 'objects','bid_' || 'picks','bid_' || 'booklet_parts','bid_' || 'commercial_hits'
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
 '10000000-0000-0000-0000-000000000005','text/plain','system:schema-acceptance','register-1',
 '10000000-0000-0000-0000-000000000006');
COMMIT;
" >/dev/null
role_psql kb_runtime_api api-test -Atc "SELECT count(*) FROM available_object_registry WHERE object_ref='objects/$digest'" | grep -qx '1'

# Bidding and platform internals are function/view-only for API/worker. The
# retention login has only retention functions and no business-table access.
role_psql kb_runtime_api api-test -Atc "SELECT count(*) FROM bidding_current_clauses" | grep -qx '0'
role_psql kb_runtime_worker worker-test -Atc "SELECT count(*) FROM bidding_current_matching_reports" | grep -qx '0'
expect_denied kb_runtime_api api-test "INSERT INTO bid_projects(id,title,owner_user_id,ends_at,fact_sha256,ceiling_identity_sha256,created_by) VALUES(gen_random_uuid(),'x','10000000-0000-0000-0000-000000000001',now(),repeat('0',64),repeat('0',64),'system:test')"
expect_denied kb_runtime_worker worker-test "UPDATE object_registry SET state='deleted'"
expect_denied kb_runtime_api api-test "SELECT * FROM kb_retention_claim(gen_random_uuid(),'bad',60000)"
expect_denied kb_runtime_retention retention-test "SELECT count(*) FROM bid_projects"

# Releasing the last owner reference soft-deletes the business row and queues
# retention atomically. A new owner cannot revive the digest while deleting.
role_psql kb_runtime_api api-test -Atc "SELECT kb_release_knowledge_document_object(
 '10000000-0000-0000-0000-000000000005','system:schema-acceptance','release-1',
 '10000000-0000-0000-0000-000000000007')" | grep -qx 't'
release_checks=$(admin_psql -Atc "SELECT deleted_at IS NOT NULL FROM documents WHERE id='10000000-0000-0000-0000-000000000005'; SELECT state FROM object_registry WHERE object_ref='objects/$digest'")
[ "$release_checks" = "$(printf 't\ndeleting')" ]

claim=10000000-0000-0000-0000-000000000008
role_psql kb_runtime_retention retention-test -Atc "SELECT object_ref FROM kb_retention_claim('$claim','schema-retention',60000)" | grep -qx "objects/$digest"
role_psql kb_runtime_retention retention-test -Atc "SELECT kb_retention_complete('objects/$digest','$claim')" | grep -qx 't'
retention_checks=$(admin_psql -Atc "SELECT state FROM object_registry WHERE object_ref='objects/$digest'; SELECT count(*) FROM object_retention_tombstones WHERE object_ref='objects/$digest'")
[ "$retention_checks" = "$(printf 'deleted\n1')" ]

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
