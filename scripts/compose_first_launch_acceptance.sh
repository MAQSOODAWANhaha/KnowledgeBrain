#!/bin/sh
set -eu

cd "$(dirname "$0")/.."

project="kb-acceptance-${GITHUB_RUN_ID:-local}-$$"
export COMPOSE_PROJECT_NAME="$project"
export COMPOSE_FILE="deploy/docker-compose.yml:deploy/compose.acceptance.yml"
export POSTGRES_HOST_PORT=0 REDIS_HOST_PORT=0 API_HOST_PORT=0 DOCREADER_HOST_PORT=0
export MINIO_HOST_PORT=0 MINIO_CONSOLE_PORT=0 NEO4J_BOLT_PORT=0 NEO4J_HTTP_PORT=0
export KNOWLEDGEBRAIN_MIGRATOR_PASSWORD=acceptance-migrator
export KNOWLEDGEBRAIN_FIRST_LAUNCH_VERIFIER_PASSWORD=acceptance-verifier
export KNOWLEDGEBRAIN_API_DB_PASSWORD=acceptance-api
export KNOWLEDGEBRAIN_WORKER_DB_PASSWORD=acceptance-worker
export KNOWLEDGEBRAIN_RETENTION_DB_PASSWORD=acceptance-retention

completion_registry=deploy/first-launch/runtime-completion.toml
completion_backup=$(mktemp)
cp "$completion_registry" "$completion_backup"
cleanup() {
  if [ -f "$completion_backup" ]; then
    cp "$completion_backup" "$completion_registry"
    rm -f "$completion_backup"
  fi
  docker compose --profile first-launch --profile runtime down --volumes --remove-orphans >/dev/null 2>&1 || true
}
trap cleanup EXIT HUP INT TERM
cleanup_docker() {
  docker compose --profile first-launch --profile runtime down --volumes --remove-orphans >/dev/null 2>&1 || true
}
cleanup_docker

# Missing/malformed/duplicate/unknown completion registries all fail before Docker.
for mutation in duplicate unknown missing; do
  cp "$completion_backup" "$completion_registry"
  case "$mutation" in
    duplicate) printf '\nformat_version = 1\n' >>"$completion_registry" ;;
    unknown) printf '\nunknown_field = "rejected"\n' >>"$completion_registry" ;;
    missing) grep -v '^topology_sha256' "$completion_backup" >"$completion_registry" ;;
  esac
  set +e
  KNOWLEDGEBRAIN_FIRST_LAUNCH_FRESH=required deploy/compose-first-launch.sh \
    >/tmp/kb-malformed-first-launch.log 2>&1
  malformed_status=$?
  set -e
  if [ "$malformed_status" -ne 66 ]; then
    echo "production orchestrator accepted $mutation completion registry" >&2
    cat /tmp/kb-malformed-first-launch.log >&2
    exit 1
  fi
done
cp "$completion_backup" "$completion_registry"
forged_digest=$(printf 'a%.0s' $(seq 1 64))
sed -e 's/phase_1d_runtime_complete = false/phase_1d_runtime_complete = true/' \
    -e "s/= \"\"/= \"$forged_digest\"/" \
    "$completion_backup" >"$completion_registry"
set +e
KNOWLEDGEBRAIN_FIRST_LAUNCH_FRESH=required deploy/compose-first-launch.sh \
  >/tmp/kb-forged-completion.log 2>&1
forged_status=$?
set -e
if [ "$forged_status" -ne 66 ] || ! grep -q 'does not match' /tmp/kb-forged-completion.log; then
  echo "production orchestrator accepted forged completion hashes" >&2
  cat /tmp/kb-forged-completion.log >&2
  exit 1
fi
cp "$completion_backup" "$completion_registry"

# The checked-in runtime closure registry must stop the production orchestrator
# before its first Docker action until real runtime acceptance is recorded.
for bypass in absent attempted; do
  set +e
  if [ "$bypass" = attempted ]; then
    KNOWLEDGEBRAIN_PHASE_1D_RUNTIME_COMPLETE=true \
      KNOWLEDGEBRAIN_REGISTRY_SHA256=$(printf 'a%.0s' $(seq 1 64)) \
      KNOWLEDGEBRAIN_FIRST_LAUNCH_FRESH=required deploy/compose-first-launch.sh \
      >/tmp/kb-incomplete-first-launch.log 2>&1
  else
    KNOWLEDGEBRAIN_FIRST_LAUNCH_FRESH=required deploy/compose-first-launch.sh \
      >/tmp/kb-incomplete-first-launch.log 2>&1
  fi
  production_gate_status=$?
  set -e
  if [ "$production_gate_status" -ne 66 ] ||
     ! grep -q 'Phase 1D runtime is not complete' /tmp/kb-incomplete-first-launch.log; then
    echo "production orchestrator did not reject incomplete runtime state ($bypass)" >&2
    cat /tmp/kb-incomplete-first-launch.log >&2
    exit 1
  fi
done

docker compose up -d --wait postgres redis

docker compose --profile first-launch run --rm --no-deps migrate

# A real runtime service must fail rather than bind before verifier finalization.
set +e
timeout 20 docker compose --profile runtime run --rm --no-deps api >/tmp/kb-runtime-before-verifier.log 2>&1
before_status=$?
set -e
if [ "$before_status" -eq 0 ] || [ "$before_status" -eq 124 ]; then
  echo "runtime did not fail closed before verifier" >&2
  cat /tmp/kb-runtime-before-verifier.log >&2
  exit 1
fi

docker compose --profile first-launch run --rm --no-deps first-launch-verifier

docker compose --profile runtime up -d api worker retention docreader
sleep 3
for service in api worker retention docreader; do
  [ "$(docker compose --profile runtime ps --status running --services "$service")" = "$service" ] || {
    echo "$service did not start after verification" >&2
    docker compose --profile runtime logs "$service" >&2
    exit 1
  }
done

# The disabled verifier login makes replay fail.
if docker compose --profile first-launch run --rm --no-deps first-launch-verifier; then
  echo "first-launch verifier replay unexpectedly succeeded" >&2
  exit 1
fi

# The checked-in postlaunch command starts only the runtime profile and reads
# the durable marker; migrate and verifier are never dependencies.
deploy/compose-runtime-restart.sh
sleep 10
for service in api worker retention docreader; do
  [ "$(docker compose --profile runtime ps --status running --services "$service")" = "$service" ] || {
    echo "$service did not survive marker-only runtime restart" >&2
    exit 1
  }
done
