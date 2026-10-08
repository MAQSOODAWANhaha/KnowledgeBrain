#!/usr/bin/env bash
# Start or stop the PostgreSQL and Redis containers a CI job needs.
# Images come from deploy/images.lock.json. Ports and credentials come from .github/ci.env.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
cmd=${1:?start or stop}
kind=${2:-both}
state_dir=${CI_SERVICE_STATE_DIR:-${RUNNER_TEMP:-/tmp}/kb-ci-services}
mkdir -p "$state_dir"
names_file=$state_dir/names

image_for() {
  python3 - "$1" <<'PY'
import json
import sys
from pathlib import Path
lock_id = sys.argv[1]
lock = json.loads(Path("deploy/images.lock.json").read_text())
rows = lock["platforms"]["linux/amd64"]["runtime_deployable"]
matches = [row["image"] for row in rows if row.get("lock_id") == lock_id]
if len(matches) != 1:
    raise SystemExit(f"missing lock image {lock_id}")
print(matches[0])
PY
}

stop_recorded() {
  if [[ ! -f "$names_file" ]]; then
    return 0
  fi
  while IFS= read -r name; do
    [[ -n "$name" ]] || continue
    docker rm -f "$name" >/dev/null 2>&1 || true
  done < "$names_file"
  rm -f "$names_file"
}

if [[ "$cmd" == "stop" ]]; then
  stop_recorded
  exit 0
fi
if [[ "$cmd" != "start" ]]; then
  echo "usage: scripts/ci_services.sh start|stop [postgres|redis|both]" >&2
  exit 2
fi

attempts=${CI_SERVICE_READY_ATTEMPTS:?}
interval=${CI_SERVICE_READY_INTERVAL_SECONDS:?}
suffix="${GITHUB_RUN_ID:-local}-${GITHUB_JOB:-local}"
: > "$names_file"

wait_ready() {
  local name=$1
  shift
  local attempt
  for ((attempt = 1; attempt <= attempts; attempt++)); do
    if "$@"; then
      return 0
    fi
    sleep "$interval"
  done
  echo "$name did not become ready" >&2
  docker logs "$name" >&2 || true
  exit 1
}

redis_is_ready() {
  docker exec "$1" redis-cli ping 2>/dev/null | grep -q PONG
}

if [[ "$kind" == "postgres" || "$kind" == "both" ]]; then
  : "${CI_POSTGRES_USER:?}"
  : "${CI_POSTGRES_PASSWORD:?}"
  : "${CI_POSTGRES_DB:?}"
  : "${CI_POSTGRES_PORT:?}"
  pg_name="kb-ci-pg-${suffix}"
  docker run -d --name "$pg_name" \
    -e POSTGRES_USER="$CI_POSTGRES_USER" \
    -e POSTGRES_PASSWORD="$CI_POSTGRES_PASSWORD" \
    -e POSTGRES_DB="$CI_POSTGRES_DB" \
    -p "127.0.0.1:${CI_POSTGRES_PORT}:5432" \
    "$(image_for postgres)" >/dev/null
  printf '%s\n' "$pg_name" >> "$names_file"
  wait_ready "$pg_name" docker exec "$pg_name" pg_isready -U "$CI_POSTGRES_USER" -d "$CI_POSTGRES_DB"
fi

if [[ "$kind" == "redis" || "$kind" == "both" ]]; then
  : "${CI_REDIS_PORT:?}"
  redis_name="kb-ci-redis-${suffix}"
  docker run -d --name "$redis_name" \
    -p "127.0.0.1:${CI_REDIS_PORT}:6379" \
    "$(image_for redis)" >/dev/null
  printf '%s\n' "$redis_name" >> "$names_file"
  wait_ready "$redis_name" redis_is_ready "$redis_name"
fi
