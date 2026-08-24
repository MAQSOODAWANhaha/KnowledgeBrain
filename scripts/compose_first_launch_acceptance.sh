#!/bin/sh
set -eu

cd "$(dirname "$0")/.."

if ! docker info >/dev/null 2>&1; then
  if [ "${KNOWLEDGEBRAIN_REQUIRE_DOCKER_ACCEPTANCE:-0}" = "1" ]; then
    echo "docker is required for mandatory Compose runtime/PDF acceptance" >&2
    exit 1
  fi
  echo "SKIPPED: docker unavailable; compose first-launch/restart/reclaim/render/retention not executed"
  exit 0
fi

# Static deletion/ACL denylist (no runtime claim).
sh scripts/bidding_v1_deletion_scan.sh

project="kb-acceptance-${GITHUB_RUN_ID:-local}-$$"
export COMPOSE_PROJECT_NAME="$project"
export COMPOSE_FILE="deploy/docker-compose.yml:deploy/compose.acceptance.yml"
export BID_EXTRACT_MODE=heuristic
# Keep mandatory acceptance deterministic and isolated from any provider
# endpoints inherited from the developer/CI environment. With no embedding
# endpoint configured, knowledge indexing uses the built-in hashed vector.
export KNOWLEDGEBRAIN_EMBEDDING_BASE_URL= EMBEDDING_BASE_URL=
export KNOWLEDGEBRAIN_CHAT_BASE_URL= LLM_BASE_URL=
export KNOWLEDGEBRAIN_VLM_BASE_URL=
export KNOWLEDGEBRAIN_MINERU_ENDPOINT= KNOWLEDGEBRAIN_PADDLE_ENDPOINT=
export POSTGRES_HOST_PORT=0 REDIS_HOST_PORT=0 API_HOST_PORT=0 DOCREADER_HOST_PORT=0
export MINIO_HOST_PORT=0 MINIO_CONSOLE_PORT=0 NEO4J_BOLT_PORT=0 NEO4J_HTTP_PORT=0
export KNOWLEDGEBRAIN_MIGRATOR_PASSWORD=acceptance-migrator
export KNOWLEDGEBRAIN_FIRST_LAUNCH_VERIFIER_PASSWORD=acceptance-verifier
export KNOWLEDGEBRAIN_API_DB_PASSWORD=acceptance-api
export KNOWLEDGEBRAIN_WORKER_DB_PASSWORD=acceptance-worker
export KNOWLEDGEBRAIN_RETENTION_DB_PASSWORD=acceptance-retention

ACCEPTANCE_GIT_SHA=$(git rev-parse HEAD)
ACCEPTANCE_GIT_DIFF_SHA256=$(git diff --binary HEAD | sha256sum | awk '{print $1}')
if [ -n "$(git ls-files --others --exclude-standard)" ]; then
  echo "compose acceptance requires every non-ignored source file to be tracked" >&2
  git ls-files --others --exclude-standard >&2
  exit 1
fi
if [ -n "$(git status --porcelain --untracked-files=all)" ]; then
  ACCEPTANCE_GIT_DIRTY=true
else
  ACCEPTANCE_GIT_DIRTY=false
fi
export ACCEPTANCE_GIT_SHA ACCEPTANCE_GIT_DIFF_SHA256 ACCEPTANCE_GIT_DIRTY

assert_acceptance_source_identity() {
  current_git_sha=$(git rev-parse HEAD)
  current_git_diff_sha256=$(git diff --binary HEAD | sha256sum | awk '{print $1}')
  if [ "$current_git_sha" != "$ACCEPTANCE_GIT_SHA" ] ||
    [ "$current_git_diff_sha256" != "$ACCEPTANCE_GIT_DIFF_SHA256" ]; then
    echo "checkout changed while acceptance was building or running" >&2
    return 1
  fi
  if [ -n "$(git ls-files --others --exclude-standard)" ]; then
    echo "an untracked source file appeared while acceptance was running" >&2
    git ls-files --others --exclude-standard >&2
    return 1
  fi
}

acceptance_runtime_tag="$project-runtime:local"
acceptance_docreader_tag="$project-docreader:local"
acceptance_runtime_context=""
acceptance_runtime_iid=""
acceptance_docreader_iid=""

completion_registry=deploy/first-launch/runtime-completion.toml
completion_backup=$(mktemp)
cp "$completion_registry" "$completion_backup"
cleanup() {
  status=$?
  if [ "$status" -ne 0 ]; then
    echo "compose acceptance failed; final service state:" >&2
    docker compose --profile first-launch --profile runtime ps -a >&2 || true
    docker compose --profile first-launch --profile runtime logs --tail=160 postgres api worker retention minio >&2 || true
  fi
  if [ -f "$completion_backup" ]; then
    cp "$completion_backup" "$completion_registry"
    rm -f "$completion_backup"
  fi
  docker compose --profile first-launch --profile runtime down --volumes --remove-orphans >/dev/null 2>&1 || true
  docker image rm "$acceptance_runtime_tag" >/dev/null 2>&1 || true
  docker image rm "$acceptance_docreader_tag" >/dev/null 2>&1 || true
  if [ -n "$acceptance_runtime_context" ] && [ -d "$acceptance_runtime_context" ]; then
    rm -rf -- "$acceptance_runtime_context"
  fi
  [ -z "$acceptance_runtime_iid" ] || rm -f "$acceptance_runtime_iid"
  [ -z "$acceptance_docreader_iid" ] || rm -f "$acceptance_docreader_iid"
  return "$status"
}
trap cleanup EXIT HUP INT TERM
cleanup_docker() {
  docker compose --profile first-launch --profile runtime down --volumes --remove-orphans >/dev/null 2>&1 || true
}
cleanup_docker

# Build every accepted application byte before freezing it into one immutable
# image. Compose never mounts the mutable checkout or target directory.
cargo build -p migrator -p first-launch-verifier -p api -p worker -p retention
npm --prefix web run build
assert_acceptance_source_identity
KB_ACCEPT_BINARY_SHA256=$(jq -n \
  --arg api "$(sha256sum target/debug/api | awk '{print $1}')" \
  --arg worker "$(sha256sum target/debug/worker | awk '{print $1}')" \
  --arg retention "$(sha256sum target/debug/retention | awk '{print $1}')" \
  --arg migrator "$(sha256sum target/debug/migrator | awk '{print $1}')" \
  --arg verifier "$(sha256sum target/debug/first-launch-verifier | awk '{print $1}')" \
  '{api:$api,worker:$worker,retention:$retention,migrator:$migrator,
    first_launch_verifier:$verifier}')
export KB_ACCEPT_BINARY_SHA256

if ! docker image inspect ubuntu:24.04 >/dev/null 2>&1; then
  docker pull ubuntu:24.04 >/dev/null
fi
KB_ACCEPT_RUNTIME_BASE_REF=$(docker image inspect --format '{{index .RepoDigests 0}}' ubuntu:24.04)
if [ -z "$KB_ACCEPT_RUNTIME_BASE_REF" ] || [ "$KB_ACCEPT_RUNTIME_BASE_REF" = "<no value>" ]; then
  docker pull ubuntu:24.04 >/dev/null
  KB_ACCEPT_RUNTIME_BASE_REF=$(docker image inspect --format '{{index .RepoDigests 0}}' ubuntu:24.04)
fi

acceptance_runtime_context=$(mktemp -d)
cp target/debug/api target/debug/worker target/debug/retention target/debug/migrator \
  target/debug/first-launch-verifier "$acceptance_runtime_context/"
cp -R web/dist "$acceptance_runtime_context/web"
acceptance_runtime_iid=$(mktemp)
docker build --iidfile "$acceptance_runtime_iid" -t "$acceptance_runtime_tag" \
  --build-arg "RUNTIME_IMAGE=$KB_ACCEPT_RUNTIME_BASE_REF" \
  -f deploy/Dockerfile.acceptance-runtime "$acceptance_runtime_context" >/dev/null
KB_ACCEPT_RUNTIME_ID=$(cat "$acceptance_runtime_iid")

acceptance_docreader_iid=$(mktemp)
docker build --iidfile "$acceptance_docreader_iid" -t "$acceptance_docreader_tag" \
  -f deploy/Dockerfile.docreader . >/dev/null
KB_ACCEPT_DOCREADER_ID=$(cat "$acceptance_docreader_iid")
export KB_ACCEPT_RUNTIME_ID KB_ACCEPT_DOCREADER_ID
assert_acceptance_source_identity

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
if [ "$forged_status" -ne 66 ] ||
  ! grep -Eq 'does not match|identity mismatch' /tmp/kb-forged-completion.log; then
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
timeout 20 docker compose --profile runtime run --rm --no-deps -T api \
  </dev/null >/tmp/kb-runtime-before-verifier.log 2>&1
before_status=$?
set -e
if [ "$before_status" -eq 0 ] || [ "$before_status" -eq 124 ]; then
  echo "runtime did not fail closed before verifier (status=$before_status)" >&2
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

# The disabled verifier login makes replay fail. Capture the expected panic so
# it cannot be mistaken for the result of the successful first invocation.
verifier_replay_log=$(mktemp)
if docker compose --profile first-launch run --rm --no-deps first-launch-verifier \
  >"$verifier_replay_log" 2>&1; then
  echo "first-launch verifier replay unexpectedly succeeded" >&2
  rm -f "$verifier_replay_log"
  exit 1
fi
if ! grep -Eq 'password authentication failed|is not permitted to log in' "$verifier_replay_log"; then
  echo "first-launch verifier replay failed for an unexpected reason" >&2
  cat "$verifier_replay_log" >&2
  rm -f "$verifier_replay_log"
  exit 1
fi
rm -f "$verifier_replay_log"

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

# The verified fresh database deliberately remains in pretraffic maintenance.
# Open only this disposable acceptance stack, with its append-only transition
# envelope, before exercising normal human mutations. This is not a production
# completion or deployment claim.
printf '%s\n' \
  "BEGIN;" \
  "UPDATE application_maintenance_gate SET mode='open',generation=generation+1,updated_by='system:first-launch',updated_at=clock_timestamp() WHERE singleton_key AND mode='maintenance' AND generation=0;" \
  "INSERT INTO maintenance_gate_audit(id,from_mode,to_mode,generation,actor_identity,reason) VALUES(gen_random_uuid(),'maintenance','open',1,'system:first-launch','isolated Compose bidding V1 runtime acceptance');" \
  "COMMIT;" |
  docker compose exec -T postgres sh -c \
    'PGPASSWORD="$POSTGRES_PASSWORD" psql -X -v ON_ERROR_STOP=1 -U "$POSTGRES_USER" -d "$POSTGRES_DB"' \
    >/dev/null

# Reclaim / render / retention probes against the live acceptance stack.
# These prove role grants and process liveness; they do not invent PDF hashes.
role_psql() {
  role=$1 password=$2
  shift 2
  docker compose exec -T -e PGPASSWORD="$password" postgres \
    psql -X -v ON_ERROR_STOP=1 -U "$role" -d knowledgebrain "$@"
}
expect_denied() {
  role=$1 password=$2 sql=$3
  if role_psql "$role" "$password" -c "$sql" >/dev/null 2>&1; then
    echo "expected denial for $role: $sql" >&2
    exit 1
  fi
}
role_psql kb_runtime_worker acceptance-worker -Atc "SELECT kb_bid_reclaim_stale_conversions()" >/dev/null
role_psql kb_runtime_worker acceptance-worker -Atc "SELECT kb_bid_housekeep_end_expired()" >/dev/null
expect_denied kb_runtime_api acceptance-api "INSERT INTO bid_projects(id,title,owner_user_id,ends_at,fact_sha256,ceiling_identity_sha256,created_by) VALUES(gen_random_uuid(),'x',gen_random_uuid(),now(),repeat('0',64),repeat('0',64),'system:test')"
expect_denied kb_runtime_retention acceptance-retention "SELECT count(*) FROM bid_projects"
api_addr=$(docker compose --profile runtime port api 8080)
base_url="http://127.0.0.1:${api_addr##*:}"
curl -fsS "$base_url/health" | grep -q ok
[ "$(docker compose --profile runtime ps --status running --services retention)" = "retention" ]

# Execute the V1 chain against real API/worker/retention binaries, the real
# DocReader gRPC service, and the verified empty-volume database. Extraction
# and evidence verification remain deterministic V1 policy implementations;
# the tender inputs themselves are real PDF and DOCX files.
evidence_dir="$PWD/web/e2e/artifacts/runtime"
if ! BASE_URL="$base_url" EVIDENCE_DIR="$evidence_dir" \
  ACCEPTANCE_RUNTIME_MODE=compose-live-api-worker-retention-docreader \
  ACCEPTANCE_EXTRACT_MODE=heuristic \
  ACCEPTANCE_INPUT_MODE=docreader-pdf-docx \
  ACCEPTANCE_DOCREADER_MODE=real-grpc \
  ACCEPTANCE_BEFORE_END_HOOK="$PWD/scripts/bid_runtime_compose_hook.sh" \
  scripts/bid_runtime_acceptance.sh; then
  docker compose --profile runtime logs --tail=240 api worker docreader retention >&2 || true
  exit 1
fi
compose_config_sha256=$(docker compose config | sha256sum | awk '{print $1}')
api_image_id=$(docker inspect --format '{{.Image}}' "$(docker compose ps -q api)")
worker_image_id=$(docker inspect --format '{{.Image}}' "$(docker compose ps -q worker)")
retention_image_id=$(docker inspect --format '{{.Image}}' "$(docker compose ps -q retention)")
docreader_image_id=$(docker inspect --format '{{.Image}}' "$(docker compose ps -q docreader)")
[ "$api_image_id" = "$KB_ACCEPT_RUNTIME_ID" ] &&
  [ "$worker_image_id" = "$KB_ACCEPT_RUNTIME_ID" ] &&
  [ "$retention_image_id" = "$KB_ACCEPT_RUNTIME_ID" ] &&
  [ "$docreader_image_id" = "$KB_ACCEPT_DOCREADER_ID" ] || {
  echo "acceptance runtime did not use the frozen image identities" >&2
  exit 1
}
tmp_runtime_identity=$(mktemp)
jq \
  --arg compose_config_sha256 "$compose_config_sha256" \
  --arg application_base_ref "$KB_ACCEPT_RUNTIME_BASE_REF" \
  --arg application_image_id "$KB_ACCEPT_RUNTIME_ID" \
  --argjson binary_sha256 "$KB_ACCEPT_BINARY_SHA256" \
  --arg api_image_id "$api_image_id" \
  --arg worker_image_id "$worker_image_id" \
  --arg retention_image_id "$retention_image_id" \
  --arg docreader_image_id "$docreader_image_id" \
  '.runtime_identity={compose_config_sha256:$compose_config_sha256,
    application_base_ref:$application_base_ref,application_image_id:$application_image_id,
    binary_sha256:$binary_sha256,
    image_ids:{api:$api_image_id,worker:$worker_image_id,retention:$retention_image_id,
      docreader:$docreader_image_id}}' \
  "$evidence_dir/evidence.json" >"$tmp_runtime_identity"
mv "$tmp_runtime_identity" "$evidence_dir/evidence.json"
if [ "${KNOWLEDGEBRAIN_REQUIRE_BROWSER_ACCEPTANCE:-0}" = "1" ]; then
  KB_LIVE_API_URL="$base_url" npm --prefix web run test:e2e:live
  browser_evidence="$PWD/web/e2e/artifacts/live/evidence.json"
  jq -e '
    .mode == "playwright-live-ui"
    and (.project_id | type == "string" and length > 0)
    and (.docx_sha256 | test("^[0-9a-f]{64}$"))
    and (.pdf_sha256 | test("^[0-9a-f]{64}$"))
  ' "$browser_evidence" >/dev/null || {
    echo "live browser evidence is missing or invalid" >&2
    exit 1
  }
  tmp_browser_mode=$(mktemp)
  jq '.execution.playwright="live-ui"' "$evidence_dir/evidence.json" >"$tmp_browser_mode"
  mv "$tmp_browser_mode" "$evidence_dir/evidence.json"
fi
project_id=$(jq -er .project_id "$evidence_dir/evidence.json")
audit_count=$(printf "SELECT count(*) FROM audit_events WHERE entity_locator->>'project_id' = '%s';\n" "$project_id" |
  docker compose exec -T postgres sh -c \
    'PGPASSWORD="$POSTGRES_PASSWORD" psql -X -U "$POSTGRES_USER" -d "$POSTGRES_DB" -At')
tmp_evidence=$(mktemp)
jq --argjson audit_count "$audit_count" '.audit_count=$audit_count' \
  "$evidence_dir/evidence.json" >"$tmp_evidence"
mv "$tmp_evidence" "$evidence_dir/evidence.json"
[ "$audit_count" -gt 0 ] || {
  echo "runtime chain produced no project audit envelope" >&2
  exit 1
}
jq -e \
  --arg git_sha "$ACCEPTANCE_GIT_SHA" \
  --arg git_diff_sha256 "$ACCEPTANCE_GIT_DIFF_SHA256" \
  --argjson git_dirty "$ACCEPTANCE_GIT_DIRTY" \
  --arg application_image_id "$KB_ACCEPT_RUNTIME_ID" '
  .schema_version == 3
  and .git_sha == $git_sha
  and .git_diff_sha256 == $git_diff_sha256
  and .git_dirty == $git_dirty
  and (.git_sha | test("^[0-9a-f]{40}$"))
  and (.git_diff_sha256 | test("^[0-9a-f]{64}$"))
  and (.git_dirty | type == "boolean")
  and .execution.runtime == "compose-live-api-worker-retention-docreader"
  and .execution.extract == "heuristic"
  and .execution.input == "docreader-pdf-docx"
  and .execution.docreader == "real-grpc"
  and (.execution.playwright == "not-used" or .execution.playwright == "live-ui")
  and (.tender_inputs.pdf.sha256 | test("^[0-9a-f]{64}$"))
  and (.tender_inputs.docx.sha256 | test("^[0-9a-f]{64}$"))
  and (.matching.technical_picks | length > 0)
  and .matching.replayed_after_live_document_delete
  and .quote.reopen_refinalize_verified
  and .compose_before_end_hook.configured
  and .compose_before_end_hook.status == "passed"
  and .compose_before_end_hook.evidence.kind_router.marker_rejected_pdf
  and .compose_before_end_hook.evidence.restart_recovery.completed_after_restart
  and .compose_before_end_hook.evidence.object_lifecycle.attachment_business_owner_released
  and .compose_before_end_hook.evidence.object_lifecycle.attachment_manifest_owner_retained
  and .compose_before_end_hook.evidence.object_lifecycle.attachment_object_available_after_business_release
  and .compose_before_end_hook.evidence.object_lifecycle.attachment_manifest_owner_count > 0
  and .compose_before_end_hook.evidence.object_lifecycle.manifest_asset_count > 0
  and .compose_before_end_hook.evidence.object_lifecycle.manifest_owner_present_and_available
  and .compose_before_end_hook.evidence.object_lifecycle.released_staging_deleted
  and (.ended.publication_rejections | length == 2)
  and (.ended.export_rejections | length == 2)
  and .ended.historical_pdf_replayed
  and (.runtime_identity.compose_config_sha256 | test("^[0-9a-f]{64}$"))
  and .runtime_identity.application_image_id == $application_image_id
  and (.runtime_identity.application_image_id | test("^sha256:[0-9a-f]{64}$"))
  and all(.runtime_identity.binary_sha256[]; test("^[0-9a-f]{64}$"))
  and all(.runtime_identity.image_ids[]; test("^sha256:[0-9a-f]{64}$"))
  and (.docx.sha256 | test("^[0-9a-f]{64}$"))
  and (.pdf.sha256 | test("^[0-9a-f]{64}$"))
  and .audit_count > 0
' "$evidence_dir/evidence.json" >/dev/null || {
  echo "runtime/PDF evidence is incomplete or overclaims its execution mode" >&2
  exit 1
}
if [ "${KNOWLEDGEBRAIN_REQUIRE_BROWSER_ACCEPTANCE:-0}" = "1" ]; then
  jq -e '.execution.playwright == "live-ui"' "$evidence_dir/evidence.json" >/dev/null || {
    echo "mandatory live browser acceptance was not recorded" >&2
    exit 1
  }
fi

assert_acceptance_source_identity
echo "compose first-launch and bidding V1 real DocReader/runtime/PDF acceptance passed"
