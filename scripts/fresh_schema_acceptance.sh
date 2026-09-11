#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

: "${KNOWLEDGEBRAIN_TEST_DATABASE_URL:?KNOWLEDGEBRAIN_TEST_DATABASE_URL is required}"
: "${KNOWLEDGEBRAIN_MIGRATOR_PASSWORD:?KNOWLEDGEBRAIN_MIGRATOR_PASSWORD is required}"
: "${KNOWLEDGEBRAIN_API_DB_PASSWORD:?KNOWLEDGEBRAIN_API_DB_PASSWORD is required}"
: "${KNOWLEDGEBRAIN_WORKER_DB_PASSWORD:?KNOWLEDGEBRAIN_WORKER_DB_PASSWORD is required}"
: "${KNOWLEDGEBRAIN_RETENTION_DB_PASSWORD:?KNOWLEDGEBRAIN_RETENTION_DB_PASSWORD is required}"

admin_url="$KNOWLEDGEBRAIN_TEST_DATABASE_URL"
database_guard="$(python3 - "$admin_url" <<'PY'
import sys
from urllib.parse import urlparse
url = urlparse(sys.argv[1])
database = url.path.removeprefix("/")
if url.hostname != "127.0.0.1" or url.port != 25433 or not database.startswith("knowledgebrain_test_"):
    raise SystemExit(2)
print("isolated-test-database")
PY
)" || {
  echo "refusing destructive acceptance outside 127.0.0.1:25433/knowledgebrain_test_*" >&2
  exit 2
}
[[ "$database_guard" == "isolated-test-database" ]]

url_for_role() {
  python3 - "$admin_url" "$1" "$2" <<'PY'
import sys
from urllib.parse import quote, urlsplit, urlunsplit
url, role, password = sys.argv[1:]
parts = urlsplit(url)
host = parts.hostname
port = f":{parts.port}" if parts.port else ""
netloc = f"{quote(role, safe='')}:{quote(password, safe='')}@{host}{port}"
print(urlunsplit((parts.scheme, netloc, parts.path, parts.query, parts.fragment)))
PY
}

migrator_url="$(url_for_role kb_migrator "$KNOWLEDGEBRAIN_MIGRATOR_PASSWORD")"


descriptor="$(mktemp "${TMPDIR:-/tmp}/knowledgebrain-release-descriptor.XXXXXX.json")"
cleanup() { rm -f "$descriptor"; }
trap cleanup EXIT INT TERM
cp deploy/release-descriptor-v1.development.json "$descriptor"
descriptor_hash="$(python3 - "$descriptor" <<'PY'
import hashlib, json, sys
with open(sys.argv[1], "rb") as source:
    value = json.load(source)
canonical = json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()
print(hashlib.sha256(b"KB:ReleaseDescriptor:v1\0" + canonical).hexdigest())
PY
)"

export KB_RELEASE_DESCRIPTOR_PATH="$descriptor"
export KB_RELEASE_DESCRIPTOR_SHA256="$descriptor_hash"
export KB_DEPLOYMENT_NAMESPACE_ID="123e4567-e89b-12d3-a456-426614174000"

bootstrap_state="$(psql "$admin_url" -X -v ON_ERROR_STOP=1 -At <<'SQL'
WITH expected(role_name,can_login,inherit_value,has_password) AS (
  VALUES ('kb_app_owner',false,false,false),('kb_migrator',true,false,true),
         ('kb_runtime_api',true,true,true),('kb_runtime_worker',true,true,true),
         ('kb_runtime_retention',true,true,true)
), exact_roles AS (
  SELECT count(*)=5 AS ok FROM expected
  JOIN pg_catalog.pg_authid role_value ON role_value.rolname=expected.role_name
  WHERE role_value.rolcanlogin=expected.can_login
    AND role_value.rolinherit=expected.inherit_value
    AND (role_value.rolpassword IS NOT NULL)=expected.has_password
    AND role_value.rolvaliduntil IS NULL AND role_value.rolconnlimit=-1
    AND NOT role_value.rolsuper AND NOT role_value.rolcreatedb
    AND NOT role_value.rolcreaterole AND NOT role_value.rolreplication
    AND NOT role_value.rolbypassrls
), exact_membership AS (
  SELECT count(*)=1 AND bool_and(role_value.rolname='kb_app_owner' AND member_value.rolname='kb_migrator' AND grantor_value.rolname=current_user AND NOT membership.admin_option) AS ok
  FROM pg_catalog.pg_auth_members membership
  JOIN pg_catalog.pg_roles role_value ON role_value.oid=membership.roleid
  JOIN pg_catalog.pg_roles member_value ON member_value.oid=membership.member
  JOIN pg_catalog.pg_roles grantor_value ON grantor_value.oid=membership.grantor
  WHERE role_value.rolname IN ('kb_app_owner','kb_migrator','kb_runtime_api','kb_runtime_worker','kb_runtime_retention')
     OR member_value.rolname IN ('kb_app_owner','kb_migrator','kb_runtime_api','kb_runtime_worker','kb_runtime_retention')
)
SELECT CASE WHEN
  current_setting('server_version_num')::integer BETWEEN 160000 AND 169999
  AND (SELECT ok FROM exact_roles)
  AND (SELECT ok FROM exact_membership)
  AND (SELECT owner_role.rolname FROM pg_catalog.pg_namespace namespace_value JOIN pg_catalog.pg_roles owner_role ON owner_role.oid=namespace_value.nspowner WHERE namespace_value.nspname='public')='kb_app_owner'
  AND has_database_privilege('kb_migrator',current_database(),'CONNECT')
  AND has_database_privilege('kb_runtime_api',current_database(),'CONNECT')
  AND has_database_privilege('kb_runtime_worker',current_database(),'CONNECT')
  AND has_database_privilege('kb_runtime_retention',current_database(),'CONNECT')
  AND NOT has_database_privilege('kb_migrator',current_database(),'CREATE')
  AND NOT has_schema_privilege('kb_migrator','public','CREATE')
  AND NOT has_database_privilege('kb_runtime_api',current_database(),'TEMPORARY')
  AND NOT has_database_privilege('kb_runtime_worker',current_database(),'TEMPORARY')
  AND NOT has_database_privilege('kb_runtime_retention',current_database(),'TEMPORARY')
  AND to_regclass('public.platform_schema_snapshot') IS NULL
THEN 'bootstrap-ready' ELSE 'bootstrap-invalid' END;
SQL
)"
[[ "$bootstrap_state" == "bootstrap-ready" ]] || {
  echo "current bootstrap role/fresh-database contract failed: $bootstrap_state" >&2
  exit 1
}

export DATABASE_URL="$migrator_url"
export KB_COMPONENT_KIND=migrator
export KB_COMPONENT_IMAGE_DIGEST="sha256:1111111111111111111111111111111111111111111111111111111111111111"
cargo run --quiet --locked -p platform --bin migrator

schema_state="$(psql "$admin_url" -X -v ON_ERROR_STOP=1 -At <<'SQL'
SELECT CASE WHEN
  to_regclass('public.platform_schema_snapshot') IS NOT NULL
  AND (SELECT count(*) FROM public.platform_schema_snapshot WHERE singleton)=1
  AND to_regclass('public.workspaces') IS NOT NULL
  AND to_regclass('public.object_registry') IS NOT NULL
  AND to_regclass('public.bid_projects') IS NOT NULL
  AND to_regclass('public.bid_submission_workspaces') IS NOT NULL
  AND jsonb_array_length((SELECT extensions FROM public.platform_schema_snapshot WHERE singleton)) > 0
  AND (SELECT postgres_server_version_num FROM public.platform_schema_snapshot WHERE singleton)=current_setting('server_version_num')::integer
THEN 'fresh-receipt-ok' ELSE 'fresh-receipt-invalid' END;
SQL
)"
[[ "$schema_state" == "fresh-receipt-ok" ]] || { echo "$schema_state" >&2; exit 1; }

before_replay="$(psql "$admin_url" -X -v ON_ERROR_STOP=1 -At <<'SQL'
SELECT created_at::text||'|'||catalog_manifest_sha256::text||'|'||
  (SELECT count(*) FROM pg_catalog.pg_class relation JOIN pg_catalog.pg_roles owner_role ON owner_role.oid=relation.relowner WHERE owner_role.rolname='kb_app_owner')||'|'||
  (SELECT count(*) FROM pg_catalog.pg_proc procedure_value JOIN pg_catalog.pg_roles owner_role ON owner_role.oid=procedure_value.proowner WHERE owner_role.rolname='kb_app_owner')
FROM public.platform_schema_snapshot WHERE singleton;
SQL
)"
cargo run --quiet --locked -p platform --bin migrator
after_replay="$(psql "$admin_url" -X -v ON_ERROR_STOP=1 -At <<'SQL'
SELECT created_at::text||'|'||catalog_manifest_sha256::text||'|'||
  (SELECT count(*) FROM pg_catalog.pg_class relation JOIN pg_catalog.pg_roles owner_role ON owner_role.oid=relation.relowner WHERE owner_role.rolname='kb_app_owner')||'|'||
  (SELECT count(*) FROM pg_catalog.pg_proc procedure_value JOIN pg_catalog.pg_roles owner_role ON owner_role.oid=procedure_value.proowner WHERE owner_role.rolname='kb_app_owner')
FROM public.platform_schema_snapshot WHERE singleton;
SQL
)"
[[ "$before_replay" == "$after_replay" ]] || { echo "matching replay changed receipt or catalog object counts" >&2; exit 1; }

# Exercise the shared verifier with each real runtime login, not admin SET ROLE.
for component in api worker retention; do
  case "$component" in
    api) password="$KNOWLEDGEBRAIN_API_DB_PASSWORD"; digest_digit=2 ;;
    worker) password="$KNOWLEDGEBRAIN_WORKER_DB_PASSWORD"; digest_digit=3 ;;
    retention) password="$KNOWLEDGEBRAIN_RETENTION_DB_PASSWORD"; digest_digit=4 ;;
  esac
  runtime_url="$(url_for_role "kb_runtime_$component" "$password")"
  export DATABASE_URL="$runtime_url" KB_COMPONENT_KIND="$component"
  KB_COMPONENT_IMAGE_DIGEST="sha256:$(printf '%064d' 0 | tr 0 "$digest_digit")"
  export KB_COMPONENT_IMAGE_DIGEST
  cargo run --quiet --locked -p platform --bin schema-verifier
  for denied_sql in 'CREATE TEMP TABLE forbidden_temp(id integer)' \
      'CREATE TABLE public.forbidden_runtime_ddl(id integer)' 'SET ROLE kb_app_owner'; do
    if denied_output="$(psql "$runtime_url" -X -v ON_ERROR_STOP=1 -v VERBOSITY=verbose -c "$denied_sql" 2>&1)"; then
      echo "runtime $component unexpectedly allowed $denied_sql" >&2; exit 1
    fi
    [[ "$denied_output" == *"42501"* ]] || {
      echo "runtime $component failed for a reason other than insufficient privilege" >&2; exit 1
    }
  done
  echo "runtime-$component-readonly-ok"
done

original_revision="$(psql "$admin_url" -X -v ON_ERROR_STOP=1 -Atc 'SELECT schema_revision FROM public.platform_schema_snapshot WHERE singleton')"
psql "$admin_url" -X -v ON_ERROR_STOP=1 -c "UPDATE public.platform_schema_snapshot SET schema_revision=schema_revision||'-drift' WHERE singleton" >/dev/null
export DATABASE_URL="$migrator_url"
export KB_COMPONENT_KIND=migrator
export KB_COMPONENT_IMAGE_DIGEST="sha256:1111111111111111111111111111111111111111111111111111111111111111"
if mismatch_output="$(cargo run --quiet --locked -p platform --bin migrator 2>&1)"; then
  echo "migrator accepted a mismatched receipt" >&2; exit 1
fi
[[ "$mismatch_output" == *"SCHEMA_REVISION_MISMATCH"* && "$mismatch_output" == *"reset required"* ]] || {
  echo "migrator returned the wrong mismatch contract" >&2; exit 1
}
psql "$admin_url" -X -v ON_ERROR_STOP=1 -v revision="$original_revision" >/dev/null <<'SQL'
UPDATE public.platform_schema_snapshot SET schema_revision=:'revision' WHERE singleton;
SQL
cargo run --quiet --locked -p platform --bin migrator

# Catalog drift must also be refused by the migrator, without repairing it.
psql "$admin_url" -X -v ON_ERROR_STOP=1 -c 'ALTER FUNCTION kb_actor_identity_valid(text) VOLATILE' >/dev/null
if mismatch_output="$(cargo run --quiet --locked -p platform --bin migrator 2>&1)"; then
  echo "migrator accepted catalog drift" >&2; exit 1
fi
[[ "$mismatch_output" == *"SCHEMA_REVISION_MISMATCH"* ]] || {
  echo "migrator returned the wrong catalog mismatch contract" >&2; exit 1
}
[[ "$(psql "$admin_url" -X -v ON_ERROR_STOP=1 -Atc "SELECT provolatile FROM pg_proc WHERE oid='kb_actor_identity_valid(text)'::regprocedure")" == v ]] || {
  echo "migrator repaired catalog drift" >&2; exit 1
}
psql "$admin_url" -X -v ON_ERROR_STOP=1 -c 'ALTER FUNCTION kb_actor_identity_valid(text) IMMUTABLE' >/dev/null
cargo run --quiet --locked -p platform --bin migrator

printf '%s\n' 'fresh-schema-acceptance-ok'
