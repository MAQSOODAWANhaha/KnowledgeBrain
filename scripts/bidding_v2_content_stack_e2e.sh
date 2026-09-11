#!/usr/bin/env bash
set -euo pipefail

source scripts/lib/content_e2e_cleanup.sh

locked_images=$(python3 - <<'PY'
import json
import re
from pathlib import Path

expected = {
    "postgres": "pgvector/pgvector@sha256:ccc6e83d6e35e931dc7c5def2022729d5a6c370318d099181995567ff1fb4d6b",
    "redis": "redis@sha256:ff02b58f971e7d7d156a1267e283fcbbeee91773b6aa36c49dac28ecfe28eadf",
}
lock = json.loads(Path("deploy/images.lock.json").read_bytes())
rows = lock["platforms"]["linux/amd64"]["runtime_deployable"]
for lock_id in ("postgres", "redis"):
    matches = [row["image"] for row in rows if row.get("lock_id") == lock_id]
    if matches != [expected[lock_id]] or not re.fullmatch(r"[^:@]+(?:/[^:@]+)*@sha256:[0-9a-f]{64}", matches[0]):
        raise SystemExit(f"invalid immutable {lock_id} image lock")
    print(matches[0])
PY
)
mapfile -t locked_image_rows <<<"$locked_images"
if (( ${#locked_image_rows[@]} != 2 )); then
  echo "Content E2E image lock resolution failed" >&2
  exit 1
fi
postgres_image=${locked_image_rows[0]}
redis_image=${locked_image_rows[1]}

suffix="${GITHUB_RUN_ID:-local}-$$"
pg_name="kb-content-e2e-pg-$suffix"
redis_name="kb-content-e2e-redis-$suffix"
database="knowledgebrain_test_content_e2e_${suffix//-/_}"
namespace=$(python3 - <<'PY'
import uuid
print(uuid.uuid4())
PY
)
offset=$(( $$ % 1000 ))
redis_port=$((26379 + offset))
# The frozen Content runtime contract pins the deterministic gateway to 127.0.0.1:18080.
gateway_port=18080
api_port=$((30380 + offset))
object_dir=$(mktemp -d /tmp/kb-content-e2e-objects.XXXXXX)
api_log=$(mktemp /tmp/kb-content-e2e-api.XXXXXX)
worker_log=$(mktemp /tmp/kb-content-e2e-worker.XXXXXX)
gateway_log=$(mktemp /tmp/kb-content-e2e-gateway.XXXXXX)
pids=()
cleanup() {
  original_status=$?
  trap - EXIT INT TERM
  set +e
  cleanup_status=0
  if (( original_status != 0 )); then cat "$api_log" "$worker_log" "$gateway_log" >&2; fi
  for pid in "${pids[@]}"; do kill -TERM "$pid" 2>/dev/null || true; done
  for pid in "${pids[@]}"; do
    wait "$pid" 2>/dev/null
    child_status=$?
    if (( child_status != 0 && child_status != 143 )); then
      echo "Content E2E child $pid cleanup wait failed: $child_status" >&2
      cleanup_status=1
    fi
  done
  content_e2e_remove_named_containers docker "$redis_name" "$pg_name" || cleanup_status=1
  rm -rf "$object_dir" "$api_log" "$worker_log" "$gateway_log" || cleanup_status=1
  final_status=$(content_e2e_final_status "$original_status" "$cleanup_status")
  exit "$final_status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

docker run -d --name "$pg_name" --label kb.acceptance=content-e2e --label "kb.e2e.run=$suffix" -e POSTGRES_PASSWORD=postgres -e POSTGRES_DB="$database" \
  -p 127.0.0.1:25433:5432 "$postgres_image" >/dev/null
docker run -d --name "$redis_name" --label kb.acceptance=content-e2e --label "kb.e2e.run=$suffix" -p "127.0.0.1:${redis_port}:6379" "$redis_image" >/dev/null
for _ in $(seq 1 60); do docker exec "$pg_name" pg_isready -U postgres -d "$database" >/dev/null 2>&1 && break; sleep 1; done
docker exec "$pg_name" pg_isready -U postgres -d "$database" >/dev/null
for _ in $(seq 1 60); do docker exec "$redis_name" redis-cli ping 2>/dev/null | grep -q PONG && break; sleep 1; done
docker exec "$redis_name" redis-cli ping | grep -q PONG

docker exec -i \
  -e POSTGRES_USER=postgres -e POSTGRES_DB="$database" \
  -e KNOWLEDGEBRAIN_MIGRATOR_PASSWORD=migrator \
  -e KNOWLEDGEBRAIN_API_DB_PASSWORD=api \
  -e KNOWLEDGEBRAIN_WORKER_DB_PASSWORD=worker \
  -e KNOWLEDGEBRAIN_RETENTION_DB_PASSWORD=retention \
  "$pg_name" sh -s < deploy/postgres-init/010-runtime-identities.sh

common_release=(
  KB_RELEASE_DESCRIPTOR_PATH="$PWD/deploy/release-descriptor-v1.development.json"
  KB_RELEASE_DESCRIPTOR_SHA256=770ce81ba32a38ff76cdee4e26abe276e3645a07dd6a6bf9d14ebf2bee76d84c
  KB_DEPLOYMENT_NAMESPACE_ID="$namespace"
)
env "${common_release[@]}" KB_COMPONENT_KIND=migrator \
  KB_COMPONENT_IMAGE_DIGEST=sha256:1111111111111111111111111111111111111111111111111111111111111111 \
  DATABASE_URL="postgres://kb_migrator:migrator@127.0.0.1:25433/$database" \
  cargo run --locked -q -p platform --bin migrator
cargo build --locked -q -p api -p worker
target_dir=$(cargo metadata --locked --format-version=1 --no-deps | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')

GATEWAY_PORT="$gateway_port" GATEWAY_MODEL=scripted-content \
  python3 scripts/bidding_v2_deterministic_gateway.py >"$gateway_log" 2>&1 & pids+=("$!")
common_runtime=(
  REDIS_URL="redis://127.0.0.1:$redis_port"
  OBJECT_DIR="$object_dir"
  KNOWLEDGEBRAIN_CHAT_BASE_URL="http://127.0.0.1:$gateway_port"
  KNOWLEDGEBRAIN_CHAT_API_KEY=deterministic-test-key
  KNOWLEDGEBRAIN_CHAT_MODEL=scripted-content
  JWT_SECRET=content-e2e-secret
)
env "${common_release[@]}" "${common_runtime[@]}" KB_COMPONENT_KIND=api \
  KB_COMPONENT_IMAGE_DIGEST=sha256:2222222222222222222222222222222222222222222222222222222222222222 \
  DATABASE_URL="postgres://kb_runtime_api:api@127.0.0.1:25433/$database" API_PORT="$api_port" \
  "$target_dir/debug/api" >"$api_log" 2>&1 & pids+=("$!")
env "${common_release[@]}" "${common_runtime[@]}" KB_COMPONENT_KIND=worker \
  KB_COMPONENT_IMAGE_DIGEST=sha256:3333333333333333333333333333333333333333333333333333333333333333 \
  DATABASE_URL="postgres://kb_runtime_worker:worker@127.0.0.1:25433/$database" \
  KB_V2_TEST_EMBEDDING_KEY=unused-exact-only KB_V2_TEST_RERANK_KEY=unused-exact-only \
  "$target_dir/debug/worker" >"$worker_log" 2>&1 & pids+=("$!")

for _ in $(seq 1 120); do
  curl -fsS "http://127.0.0.1:$gateway_port/healthz" >/dev/null 2>&1 \
    && curl -fsS "http://127.0.0.1:$api_port/health" >/dev/null 2>&1 && break
  if ! kill -0 "${pids[1]}" 2>/dev/null || ! kill -0 "${pids[2]}" 2>/dev/null; then
    cat "$api_log" "$worker_log" >&2; exit 1
  fi
  sleep 1
done
curl -fsS "http://127.0.0.1:$api_port/health" >/dev/null

# Runtime schema identity is verified before test-only fixture DML changes frozen seed tables.
{ echo 'SET ROLE kb_app_owner;'; cat crates/bidding/tests/sql/phase0_acceptance.sql; } \
  | docker exec -i "$pg_name" psql -U postgres -d "$database" -v ON_ERROR_STOP=1 >/dev/null

jwt=$(python3 - <<'PY'
import base64,hashlib,hmac,json,time
enc=lambda v: base64.urlsafe_b64encode(json.dumps(v,separators=(',',':')).encode()).rstrip(b'=').decode()
h=enc({'alg':'HS256','typ':'JWT'}); p=enc({'sub':'00000000-0000-4000-8000-000000000001','exp':int(time.time())+3600})
s=base64.urlsafe_b64encode(hmac.new(b'content-e2e-secret',f'{h}.{p}'.encode(),hashlib.sha256).digest()).rstrip(b'=').decode()
print(f'{h}.{p}.{s}')
PY
)
if ! API_URL="http://127.0.0.1:$api_port" BID_V2_JWT="$jwt" BID_V2_USE_FIXTURE_EVIDENCE=1 BID_V2_WAIT_SECONDS=45 \
  BID_V2_E2E_KEY_PREFIX="content-e2e-$suffix" \
  python3 scripts/bidding_v2_evidence_api_worker_e2e.py; then
  docker exec "$pg_name" psql -U postgres -d "$database" -c \
    "SELECT run.request_artifact_id,run.attempt,run.status,run.last_error_code,run.last_error_message,identity.runtime_contract_sha256 FROM bid_content_agent_run_artifacts run JOIN bid_content_generation_request_identities identity USING(request_artifact_id) ORDER BY run.started_at DESC LIMIT 4; SELECT 'stage' kind,request_artifact_id,NULL::integer ordinal,input_sha256 FROM bid_content_agent_input_artifacts UNION ALL SELECT 'call',request_artifact_id,call_ordinal,input_sha256 FROM bid_content_agent_boundary_attempts" >&2
  exit 1
fi

# Shutdown is authoritative: terminate, join, require the Worker supervisor to
# exit cleanly, and prove terminal business/object state cannot change later.
before_shutdown=$(docker exec "$pg_name" psql -U postgres -d "$database" -At -v ON_ERROR_STOP=1 -c \
  "SELECT count(*)||':'||coalesce(string_agg(id::text||':'||status||':'||coalesce(error_code,''),',' ORDER BY id),'') FROM bid_async_request_snapshot_artifacts; SELECT count(*) FROM object_upload_staging;")
for pid in "${pids[@]}"; do kill -TERM "$pid"; done
for index in "${!pids[@]}"; do
  pid=${pids[$index]}
  set +e
  wait "$pid"
  exit_code=$?
  set -e
  if (( index == 2 )); then
    if (( exit_code != 0 )); then
      echo "Content E2E Worker did not shut down gracefully: $exit_code" >&2
      exit 1
    fi
  elif (( exit_code != 0 && exit_code != 143 )); then
    echo "Content E2E process $pid exited unexpectedly: $exit_code" >&2
    exit 1
  fi
done
pids=()
sleep 1
after_shutdown=$(docker exec "$pg_name" psql -U postgres -d "$database" -At -v ON_ERROR_STOP=1 -c \
  "SELECT count(*)||':'||coalesce(string_agg(id::text||':'||status||':'||coalesce(error_code,''),',' ORDER BY id),'') FROM bid_async_request_snapshot_artifacts; SELECT count(*) FROM object_upload_staging;")
if [[ "$before_shutdown" != "$after_shutdown" ]] || [[ "${after_shutdown##*$'\n'}" != "0" ]]; then
  echo "Content E2E observed late business writes or staged-object residue" >&2
  printf 'before=%s\nafter=%s\n' "$before_shutdown" "$after_shutdown" >&2
  exit 1
fi
printf '%s\n' 'bidding-v2-content-stack-e2e-ok'
