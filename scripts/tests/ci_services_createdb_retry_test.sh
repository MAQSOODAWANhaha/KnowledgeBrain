#!/usr/bin/env bash
# createdb retries use CI_SERVICE_READY_* and treat pg_database existence as success.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
script=$root/scripts/ci_services.sh
workflow=$root/.github/workflows/ci.yml
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
bin=$tmp/bin
mkdir -p "$bin"

cat > "$bin/createdb" <<'EOF'
#!/bin/sh
count=0
if [ -f "$CREATEDB_COUNT" ]; then
  count=$(cat "$CREATEDB_COUNT")
fi
count=$((count + 1))
printf '%s\n' "$count" > "$CREATEDB_COUNT"
printf '%s\n' "$*" >> "$CREATEDB_LOG"
if [ "$count" -le "${CREATEDB_FAILS:-0}" ]; then
  echo "createdb failed" >&2
  exit 1
fi
exit 0
EOF

cat > "$bin/psql" <<'EOF'
#!/bin/sh
count=0
if [ -f "$PSQL_COUNT" ]; then
  count=$(cat "$PSQL_COUNT")
fi
count=$((count + 1))
printf '%s\n' "$count" > "$PSQL_COUNT"
printf '%s\n' "$*" >> "$PSQL_LOG"
if [ "${CREATEDB_EXISTS:-0}" = 1 ]; then
  echo 1
  exit 0
fi
echo "psql failed" >&2
exit 2
EOF
chmod +x "$bin/createdb" "$bin/psql"

run_createdb() {
  env \
    PATH="$bin:$PATH" \
    CI_SERVICE_STATE_DIR="$tmp/state" \
    CI_SERVICE_READY_ATTEMPTS="${CI_SERVICE_READY_ATTEMPTS:-3}" \
    CI_SERVICE_READY_INTERVAL_SECONDS="${CI_SERVICE_READY_INTERVAL_SECONDS:-0}" \
    CI_SCHEMA_POSTGRES_DB="${CI_SCHEMA_POSTGRES_DB:-postgres}" \
    CREATEDB_COUNT="$tmp/createdb.count" \
    CREATEDB_LOG="$tmp/createdb.log" \
    CREATEDB_FAILS="${CREATEDB_FAILS:-0}" \
    CREATEDB_EXISTS="${CREATEDB_EXISTS:-0}" \
    PSQL_COUNT="$tmp/psql.count" \
    PSQL_LOG="$tmp/psql.log" \
    bash "$script" createdb "$@"
}

reset_logs() {
  rm -f "$tmp/createdb.count" "$tmp/createdb.log" "$tmp/psql.count" "$tmp/psql.log"
  mkdir -p "$tmp/state"
}

grep -Fq 'bash scripts/ci_services.sh createdb -T template0 "$database"' "$workflow"

reset_logs
CREATEDB_FAILS=1
run_createdb -T template0 lifecycle
[[ $(cat "$tmp/createdb.count") == 2 ]]
[[ $(cat "$tmp/createdb.log") == $'-T template0 lifecycle\n-T template0 lifecycle' ]]
[[ $(cat "$tmp/psql.count") == 1 ]]
grep -Fq -- '-d postgres' "$tmp/psql.log"
grep -Fq -- 'name=lifecycle' "$tmp/psql.log"

reset_logs
CREATEDB_FAILS=5
CREATEDB_EXISTS=1
run_createdb -T template0 lifecycle
[[ $(cat "$tmp/createdb.count") == 1 ]]
[[ $(cat "$tmp/psql.count") == 1 ]]

reset_logs
CREATEDB_FAILS=5
CREATEDB_EXISTS=0
CI_SERVICE_READY_ATTEMPTS=3
if run_createdb lifecycle; then
  echo "createdb retry succeeded after the budget was exhausted" >&2
  exit 1
fi
[[ $(cat "$tmp/createdb.count") == 3 ]]
[[ $(cat "$tmp/psql.count") == 3 ]]

reset_logs
if env -u CI_SERVICE_READY_ATTEMPTS \
  PATH="$bin:$PATH" \
  CI_SERVICE_STATE_DIR="$tmp/state" \
  CI_SERVICE_READY_INTERVAL_SECONDS=0 \
  CI_SCHEMA_POSTGRES_DB=postgres \
  CREATEDB_COUNT="$tmp/createdb.count" \
  CREATEDB_LOG="$tmp/createdb.log" \
  CREATEDB_FAILS=0 \
  CREATEDB_EXISTS=0 \
  PSQL_COUNT="$tmp/psql.count" \
  PSQL_LOG="$tmp/psql.log" \
  bash "$script" createdb lifecycle; then
  echo "createdb retry ran without CI_SERVICE_READY_ATTEMPTS" >&2
  exit 1
fi
[[ ! -f "$tmp/createdb.count" ]]
