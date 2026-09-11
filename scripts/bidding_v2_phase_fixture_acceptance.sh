#!/usr/bin/env bash
set -euo pipefail

: "${KNOWLEDGEBRAIN_TEST_DATABASE_URL:?KNOWLEDGEBRAIN_TEST_DATABASE_URL is required}"
readarray -t parsed < <(python3 - "$KNOWLEDGEBRAIN_TEST_DATABASE_URL" <<'PY'
import sys, urllib.parse
u=urllib.parse.urlsplit(sys.argv[1])
if u.hostname != "127.0.0.1" or u.port != 25433 or not u.path.removeprefix("/").startswith("knowledgebrain_test_"):
    raise SystemExit("phase fixture acceptance requires 127.0.0.1:25433/knowledgebrain_test_*")
print(urllib.parse.urlunsplit((u.scheme,u.netloc,"/postgres",u.query,u.fragment)))
print(u.path.removeprefix("/"))
PY
)
admin_url=${parsed[0]}
base_name=${parsed[1]}
prefix="${base_name}_fixtures_$$"
databases=("${prefix}_phase0_6" "${prefix}_phase1_3")
cleanup() {
  for database in "${databases[@]}"; do
    psql "$admin_url" -v ON_ERROR_STOP=1 -c "DROP DATABASE IF EXISTS \"$database\" WITH (FORCE)" >/dev/null || true
  done
}
trap cleanup EXIT

create_baseline() {
  local database=$1
  local url=${KNOWLEDGEBRAIN_TEST_DATABASE_URL%/*}/$database
  psql "$admin_url" -v ON_ERROR_STOP=1 \
    -c "CREATE DATABASE \"$database\"" \
    -c "REVOKE TEMPORARY,CREATE ON DATABASE \"$database\" FROM PUBLIC" >/dev/null
  psql "$url" -v ON_ERROR_STOP=1 <<'SQL' >/dev/null
CREATE EXTENSION pgcrypto;
CREATE EXTENSION vector;
ALTER SCHEMA public OWNER TO kb_app_owner;
REVOKE CREATE ON SCHEMA public FROM PUBLIC;
SQL
  { printf '%s\n' 'BEGIN; SET LOCAL ROLE kb_app_owner;';
    cat migrations/shared_platform_baseline.sql migrations/knowledge_base_baseline.sql migrations/bidding_v2_baseline.sql;
    printf '%s\n' 'COMMIT;';
  } | psql "$url" -v ON_ERROR_STOP=1 >/dev/null
}

create_baseline "${databases[0]}"
url=${KNOWLEDGEBRAIN_TEST_DATABASE_URL%/*}/${databases[0]}
{ printf '%s\n' 'SET ROLE kb_app_owner;';
  cat crates/bidding/tests/sql/phase0_acceptance.sql crates/bidding/tests/sql/phase6_acceptance.sql;
} | psql "$url" -v ON_ERROR_STOP=1 >/dev/null

create_baseline "${databases[1]}"
url=${KNOWLEDGEBRAIN_TEST_DATABASE_URL%/*}/${databases[1]}
{ printf '%s\n' 'SET ROLE kb_app_owner;';
  cat crates/bidding/tests/sql/phase1_acceptance.sql \
      crates/bidding/tests/sql/phase1_supersession_acceptance.sql \
      crates/bidding/tests/sql/phase3_acceptance.sql;
} | psql "$url" -v ON_ERROR_STOP=1 >/dev/null

printf '%s\n' 'bidding-v2-phase-fixture-acceptance-ok'
