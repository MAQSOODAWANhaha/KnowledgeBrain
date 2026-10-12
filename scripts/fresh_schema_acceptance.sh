#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

run_platform_bin() {
  local name=$1
  local bin=""
  case "$name" in
    migrator) bin=${KB_MIGRATOR_BIN:-} ;;
    *) echo "unknown platform binary: $name" >&2; return 2 ;;
  esac
  if [[ -n "$bin" ]]; then
    "$bin"
  else
    cargo run --quiet --locked -p platform --bin "$name"
  fi
}

: "${KNOWLEDGEBRAIN_TEST_DATABASE_URL:?KNOWLEDGEBRAIN_TEST_DATABASE_URL is required}"

application_url="$KNOWLEDGEBRAIN_TEST_DATABASE_URL"
database_guard="$(python3 - "$application_url" <<'PY'
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

bootstrap_state="$(psql "$application_url" -X -v ON_ERROR_STOP=1 -At <<'SQL'
SELECT CASE WHEN
  current_setting('server_version_num')::integer BETWEEN 160000 AND 169999
  AND EXISTS (SELECT 1 FROM pg_roles WHERE rolname=current_user AND rolcanlogin
    AND NOT rolsuper AND NOT rolcreatedb AND NOT rolcreaterole
    AND NOT rolreplication AND NOT rolbypassrls)
  AND NOT EXISTS (SELECT 1 FROM pg_auth_members WHERE member=(SELECT oid FROM pg_roles WHERE rolname=current_user))
  AND (SELECT pg_get_userbyid(nspowner) FROM pg_namespace WHERE nspname='public')=current_user
  AND has_database_privilege(current_user,current_database(),'CONNECT')
  AND NOT has_database_privilege(current_user,current_database(),'CREATE,TEMPORARY')
  AND has_schema_privilege(current_user,'public','CREATE')
  AND to_regclass('public.workspaces') IS NULL
THEN 'bootstrap-ready' ELSE 'bootstrap-invalid' END;
SQL
)"
[[ "$bootstrap_state" == "bootstrap-ready" ]] || {
  echo "current bootstrap role/fresh-database contract failed: $bootstrap_state" >&2
  exit 1
}

export DATABASE_URL="$application_url"
run_platform_bin migrator

schema_state="$(psql "$application_url" -X -v ON_ERROR_STOP=1 -At <<'SQL'
SELECT CASE WHEN
  to_regclass('public.workspaces') IS NOT NULL
  AND to_regclass('public.object_registry') IS NOT NULL
  AND to_regclass('public.bid_projects') IS NOT NULL
  AND to_regclass('public.bid_outline_artifacts') IS NOT NULL
  AND to_regclass('public.bid_response_sets') IS NOT NULL
  AND to_regclass('public.bid_docx_current') IS NOT NULL
THEN 'fresh-baseline-ok' ELSE 'fresh-baseline-invalid' END;
SQL
)"
[[ "$schema_state" == "fresh-baseline-ok" ]] || { echo "$schema_state" >&2; exit 1; }

# The shared application identity must not gain cluster or temporary privileges.
for denied_sql in 'CREATE TEMP TABLE forbidden_temp(id integer)' \
    'CREATE ROLE forbidden_admin' 'SET ROLE postgres'; do
  if denied_output="$(psql "$application_url" -X -v ON_ERROR_STOP=1 -v VERBOSITY=verbose -c "$denied_sql" 2>&1)"; then
    echo "application unexpectedly allowed $denied_sql" >&2; exit 1
  fi
  [[ "$denied_output" == *"42501"* ]] || {
    echo 'application failed for a reason other than insufficient privilege' >&2; exit 1
  }
done
printf '%s\n' 'fresh-schema-acceptance-ok'
