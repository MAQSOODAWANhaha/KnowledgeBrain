#!/usr/bin/env bash
set -euo pipefail

: "${DATABASE_URL:?DATABASE_URL is required}"
: "${REDIS_URL:?REDIS_URL is required}"
API_PORT="${API_PORT:-18081}"
BASE="http://127.0.0.1:${API_PORT}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
API_BIN="${API_BIN:-$ROOT/target/debug/api}"
WORKER_BIN="${WORKER_BIN:-$ROOT/target/debug/worker}"
TMP="$(mktemp -d)"
API_PID=""
WORKER_PID=""
cleanup() {
  [[ -z "$API_PID" ]] || kill -TERM "$API_PID" 2>/dev/null || true
  [[ -z "$WORKER_PID" ]] || kill -TERM "$WORKER_PID" 2>/dev/null || true
  [[ -z "$API_PID" ]] || wait "$API_PID" 2>/dev/null || true
  [[ -z "$WORKER_PID" ]] || wait "$WORKER_PID" 2>/dev/null || true
  rm -rf "$TMP"
}
trap cleanup EXIT
fail_logs() {
  echo "bid smoke failed: $*" >&2
  tail -n 160 "$TMP/api.log" >&2 || true
  tail -n 160 "$TMP/worker.log" >&2 || true
  exit 1
}
auth_json() {
  curl -sS -o "$TMP/body.json" -w '%{http_code}' "$@"
}

export API_PORT BID_EXTRACT_MODE=heuristic OBJECT_DIR="$TMP/objects"
"$API_BIN" >"$TMP/api.log" 2>&1 & API_PID=$!
"$WORKER_BIN" >"$TMP/worker.log" 2>&1 & WORKER_PID=$!

for _ in $(seq 1 150); do
  curl -fsS "$BASE/health" >/dev/null 2>&1 && grep -q 'worker ready' "$TMP/worker.log" && break
  sleep 0.1
done
curl -fsS "$BASE/health" >/dev/null || fail_logs "API did not become live"
grep -q 'worker ready' "$TMP/worker.log" || fail_logs "worker did not verify Redis and become ready"

TOKEN="$(curl -fsS -X POST "$BASE/api/v1/auth/login" \
  -H 'content-type: application/json' \
  -d '{"email":"bid-smoke@local","password":"ignored"}' | jq -r .token)"
AUTH="Authorization: Bearer $TOKEN"
ENDS="$(date -u -d '+30 days' +%Y-%m-%dT%H:%M:%SZ 2>/dev/null || date -u -v+30d +%Y-%m-%dT%H:%M:%SZ)"
CODE="$(auth_json -X POST "$BASE/api/v1/bids" -H "$AUTH" -H 'content-type: application/json' \
  -d '{"title":"legacy","owner_name":"CI"}')"
[[ "$CODE" != "200" && "$CODE" != "201" ]] || fail_logs "legacy owner_name create still accepted"

PROJECT_ID="$(curl -fsS -X POST "$BASE/api/v1/bids" -H "$AUTH" \
  -H 'content-type: application/json' -H "Idempotency-Key: $(uuidgen | tr '[:upper:]' '[:lower:]')" \
  -d "{\"title\":\"V1 tender smoke\",\"ends_at\":\"$ENDS\"}" | jq -r .id)"
[[ -n "$PROJECT_ID" && "$PROJECT_ID" != "null" ]] || fail_logs "create project failed"

CODE="$(auth_json -X POST "$BASE/api/v1/bids/$PROJECT_ID/clauses" -H "$AUTH" \
  -H 'content-type: application/json' -H "Idempotency-Key: $(uuidgen | tr '[:upper:]' '[:lower:]')" \
  -d '{"text":"x","kind":"technical","must":true,"family":"technical"}')"
[[ "$CODE" != "200" && "$CODE" != "201" ]] || fail_logs "family write still accepted"

CODE="$(auth_json "$BASE/api/v1/bids/$PROJECT_ID/export?regenerate_stale=true" -H "$AUTH")"
[[ "$CODE" == "404" || "$CODE" == "405" ]] || fail_logs "legacy export still routed ($CODE)"
CODE="$(auth_json "$BASE/api/v1/bids/$PROJECT_ID/booklet" -H "$AUTH")"
[[ "$CODE" == "404" || "$CODE" == "405" ]] || fail_logs "legacy booklet still routed ($CODE)"

CLAUSE="$(curl -fsS -X POST "$BASE/api/v1/bids/$PROJECT_ID/clauses" -H "$AUTH" \
  -H 'content-type: application/json' -H "Idempotency-Key: $(uuidgen | tr '[:upper:]' '[:lower:]')" \
  -d '{"text":"系统必须支持双千兆网络接口。","kind":"technical","must":true}')"
[[ "$(jq -r .kind <<<"$CLAUSE")" == "technical" ]] || fail_logs "kind-only create failed"
[[ "$(jq -r .family <<<"$CLAUSE")" == "technical" ]] || fail_logs "server family not derived"

curl -fsS "$BASE/api/v1/bids/$PROJECT_ID/facts" -H "$AUTH" | jq -e '.project_facts.revision >= 0' >/dev/null \
  || fail_logs "facts contract missing"
curl -fsS "$BASE/api/v1/bids/$PROJECT_ID/quote" -H "$AUTH" | jq -e 'has("exists")' >/dev/null \
  || fail_logs "quote contract missing"
curl -fsS "$BASE/api/v1/bids/$PROJECT_ID/gate-issues?format=pdf" -H "$AUTH" | jq -e '.issues | type == "array"' >/dev/null \
  || fail_logs "gate contract missing"
curl -fsS "$BASE/api/v1/bids/$PROJECT_ID/parts" -H "$AUTH" | jq -e '
  .required_part_keys as $k
  | ($k | index("1")) and ($k | index("3")) and ($k | index("6:letter")) and ($k | index("6:quote"))
' >/dev/null || fail_logs "RequiredPartSet missing ①/③/⑥"

cat >"$TMP/tender.md" <<'DOC'
# 技术要求
1. 系统必须支持双千兆网络接口。
# 商务资格
投标人须提供有效的质量管理体系认证证书。
DOC
curl -fsS -X POST "$BASE/api/v1/bids/$PROJECT_ID/documents" -H "$AUTH" \
  -H "Idempotency-Key: $(uuidgen | tr '[:upper:]' '[:lower:]')" \
  -F "file=@$TMP/tender.md;type=text/markdown" >/dev/null

echo "bid V1 smoke: PASS project=$PROJECT_ID"
echo "real: API V1 contract (kind-only, no family/export/booklet), create/facts/quote/gate/parts"
echo "not claimed: Compose first launch, Playwright browser, formal PDF hash replay"
