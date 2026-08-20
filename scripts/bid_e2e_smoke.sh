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
PROJECT_ID="$(curl -fsS -X POST "$BASE/api/v1/bids" -H "$AUTH" \
  -H 'content-type: application/json' \
  -d '{"title":"Service-backed tender smoke","owner_name":"CI","expires_at":null}' | jq -r .id)"
cat >"$TMP/tender.md" <<'DOC'
# 技术要求

1. 系统必须支持双千兆网络接口。
2. 设备应支持标准 REST API，并提供接口文档。

# 商务资格

投标人须提供有效的质量管理体系认证证书。
DOC
curl -fsS -X POST "$BASE/api/v1/bids/$PROJECT_ID/documents" -H "$AUTH" \
  -F "file=@$TMP/tender.md;type=text/markdown" >/dev/null

CLAUSES='[]'
for _ in $(seq 1 300); do
  CLAUSES="$(curl -fsS "$BASE/api/v1/bids/$PROJECT_ID/clauses" -H "$AUTH")"
  [[ "$(jq '[.[] | select(.status == "draft")] | length' <<<"$CLAUSES")" -ge 3 ]] && break
  sleep 0.1
done
[[ "$(jq '[.[] | select(.status == "draft")] | length' <<<"$CLAUSES")" -ge 3 ]] \
  || fail_logs "three deterministic draft clauses were not produced"
PROJECT="$(curl -fsS "$BASE/api/v1/bids/$PROJECT_ID" -H "$AUTH")"
[[ "$(jq -r '.latest_extract.status' <<<"$PROJECT")" == "done" ]] || fail_logs "auto extraction did not finish"
[[ "$(jq -r '.latest_extract.diagnostics.coverage.candidate_spans' <<<"$PROJECT")" -ge 3 ]] \
  || fail_logs "compact extraction diagnostics missing"

TECH_ID="$(jq -r '[.[] | select(.status == "draft" and .family == "technical")][0].id // empty' <<<"$CLAUSES")"
REJECT_ID="$(jq -r '[.[] | select(.status == "draft" and .family == "technical")][1].id // empty' <<<"$CLAUSES")"
COMM_ID="$(jq -r '[.[] | select(.status == "draft" and .family == "commercial")][0].id // empty' <<<"$CLAUSES")"
[[ -n "$TECH_ID" && -n "$REJECT_ID" && -n "$COMM_ID" ]] || fail_logs "technical/commercial draft split missing"

EDITED_TEXT='人工编辑：系统支持双千兆网络接口'
curl -fsS -X PATCH "$BASE/api/v1/bids/$PROJECT_ID/clauses/$TECH_ID" -H "$AUTH" \
  -H 'content-type: application/json' \
  -d "{\"text\":\"$EDITED_TEXT\",\"must\":false}" >/dev/null
CLAUSES="$(curl -fsS "$BASE/api/v1/bids/$PROJECT_ID/clauses" -H "$AUTH")"
[[ "$(jq -r --arg id "$TECH_ID" '.[] | select(.id == $id) | [.text,.family,.must,.status] | @tsv' <<<"$CLAUSES")" == "$EDITED_TEXT"$'\ttechnical\tfalse\tdraft' ]] \
  || fail_logs "draft text/must edit did not persist"
curl -fsS -X PATCH "$BASE/api/v1/bids/$PROJECT_ID/clauses/$TECH_ID" -H "$AUTH" \
  -H 'content-type: application/json' -d '{"family":"commercial"}' >/dev/null
[[ "$(curl -fsS "$BASE/api/v1/bids/$PROJECT_ID/clauses" -H "$AUTH" | jq -r --arg id "$TECH_ID" '.[] | select(.id == $id) | .family')" == "commercial" ]] \
  || fail_logs "meaningful family edit did not persist"
curl -fsS -X PATCH "$BASE/api/v1/bids/$PROJECT_ID/clauses/$TECH_ID" -H "$AUTH" \
  -H 'content-type: application/json' -d '{"family":"technical"}' >/dev/null
curl -fsS -X PATCH "$BASE/api/v1/bids/$PROJECT_ID/clauses/$TECH_ID" -H "$AUTH" \
  -H 'content-type: application/json' -d '{"status":"confirmed"}' >/dev/null
curl -fsS -X PATCH "$BASE/api/v1/bids/$PROJECT_ID/clauses/$REJECT_ID" -H "$AUTH" \
  -H 'content-type: application/json' -d '{"status":"rejected"}' >/dev/null
curl -fsS -X PATCH "$BASE/api/v1/bids/$PROJECT_ID/clauses/$COMM_ID" -H "$AUTH" \
  -H 'content-type: application/json' -d '{"family":"commercial","must":true,"status":"confirmed"}' >/dev/null
CLAUSES="$(curl -fsS "$BASE/api/v1/bids/$PROJECT_ID/clauses" -H "$AUTH")"
[[ "$(jq -r --arg id "$TECH_ID" '.[] | select(.id == $id) | [.text,.status,.family,.must] | @tsv' <<<"$CLAUSES")" == "$EDITED_TEXT"$'\tconfirmed\ttechnical\tfalse' ]] \
  || fail_logs "separate draft edit/family restore/confirmation did not persist"
[[ "$(jq -r --arg id "$REJECT_ID" '.[] | select(.id == $id) | .status' <<<"$CLAUSES")" == "rejected" ]] \
  || fail_logs "rejected state did not persist"

MATCH_TERMINAL=false
for _ in $(seq 1 900); do
  PROJECT="$(curl -fsS "$BASE/api/v1/bids/$PROJECT_ID" -H "$AUTH")"
  if [[ "$(jq -r '.derived.match_running' <<<"$PROJECT")" == "false" ]]; then MATCH_TERMINAL=true; break; fi
  sleep 0.1
done
[[ "$MATCH_TERMINAL" == true ]] || fail_logs "durable match did not reach a terminal state"
PROJECT="$(curl -fsS "$BASE/api/v1/bids/$PROJECT_ID" -H "$AUTH")"
jq -e '[.match_jobs[] | select(.job_kind == "technical")] | length >= 1 and all(.[]; .status == "done" and .tech_status == "done" and (.tech_candidates | length == 0))' <<<"$PROJECT" >/dev/null \
  || fail_logs "technical match job did not succeed with deterministic empty candidates"
jq -e '[.match_jobs[] | select(.job_kind == "commercial")] | length == 1 and all(.[]; .status == "done" and .commercial_status == "done")' <<<"$PROJECT" >/dev/null \
  || fail_logs "commercial match job did not succeed"
CLAUSES="$(curl -fsS "$BASE/api/v1/bids/$PROJECT_ID/clauses" -H "$AUTH")"
[[ "$(jq -r --arg id "$COMM_ID" '.[] | select(.id == $id) | .hit_outcome' <<<"$CLAUSES")" == "miss" ]] \
  || fail_logs "commercial deterministic miss was not published"

UNITS="$(curl -fsS "$BASE/api/v1/bids/$PROJECT_ID/units" -H "$AUTH")"
SECTION_ID="$(jq -r '[.units[] | select(.kind == "technical")][0].id // empty' <<<"$UNITS")"
[[ -n "$SECTION_ID" ]] || fail_logs "technical Section/unit missing"
curl -fsS -X POST "$BASE/api/v1/bids/$PROJECT_ID/sections/$SECTION_ID/retry" -H "$AUTH" >/dev/null
RETRY_TERMINAL=false
for _ in $(seq 1 900); do
  UNITS="$(curl -fsS "$BASE/api/v1/bids/$PROJECT_ID/units" -H "$AUTH")"
  RETRY_STATUS="$(jq -r --arg id "$SECTION_ID" '.units[] | select(.id == $id) | .retry_status // ""' <<<"$UNITS")"
  if [[ "$RETRY_STATUS" == "done" ]]; then RETRY_TERMINAL=true; break; fi
  [[ "$RETRY_STATUS" != "failed" ]] || fail_logs "Section retry failed"
  sleep 0.1
done
[[ "$RETRY_TERMINAL" == true ]] || fail_logs "Section retry did not reach done"

curl -fsS -X PUT "$BASE/api/v1/bids/$PROJECT_ID/booklet/1" -H "$AUTH" \
  -H 'content-type: application/json' -d '{"markdown":"# 人工成稿\n\nservice-backed smoke"}' >/dev/null
curl -fsS "$BASE/api/v1/bids/$PROJECT_ID/booklet" -H "$AUTH" \
  | jq -e '.parts[] | select(.key == "1" and (.markdown | contains("service-backed smoke")))' >/dev/null \
  || fail_logs "booklet save/read failed"
curl -fsS "$BASE/api/v1/bids/$PROJECT_ID/preview" -H "$AUTH" \
  | jq -e --arg id "$PROJECT_ID" '.project_id == $id' >/dev/null || fail_logs "preview failed"
curl -fsS "$BASE/api/v1/bids/$PROJECT_ID/export?format=docx" -H "$AUTH" -o "$TMP/out.docx"
[[ "$(od -An -tx1 -N2 "$TMP/out.docx" | tr -d ' \n')" == "504b" ]] || fail_logs "DOCX signature missing"
curl -fsS "$BASE/api/v1/bids/$PROJECT_ID/export?format=pdf" -H "$AUTH" -o "$TMP/out.pdf"
[[ "$(head -c 4 "$TMP/out.pdf")" == "%PDF" ]] || fail_logs "PDF signature missing"

OLD_RUN_ID="$(jq -r '.latest_extract.id' <<<"$PROJECT")"
curl -fsS -X POST "$BASE/api/v1/bids/$PROJECT_ID/extract" -H "$AUTH" >/dev/null
REEXTRACT_TERMINAL=false
for _ in $(seq 1 900); do
  PROJECT="$(curl -fsS "$BASE/api/v1/bids/$PROJECT_ID" -H "$AUTH")"
  if [[ "$(jq -r '.derived.extract_running' <<<"$PROJECT")" == "false" && "$(jq -r '.latest_extract.status' <<<"$PROJECT")" == "done" && "$(jq -r '.latest_extract.id' <<<"$PROJECT")" != "$OLD_RUN_ID" ]]; then REEXTRACT_TERMINAL=true; break; fi
  sleep 0.1
done
[[ "$REEXTRACT_TERMINAL" == true ]] || fail_logs "manual re-extraction did not finish done with a new run"
[[ "$(jq -r '.latest_extract.extractor_mode' <<<"$PROJECT")" == "heuristic" ]] || fail_logs "manual extraction mode missing"
[[ "$(jq -r '.latest_extract.diagnostics.coverage.candidate_spans' <<<"$PROJECT")" -ge 3 ]] || fail_logs "manual extraction diagnostics missing"
ALL_CLAUSES="$(curl -fsS "$BASE/api/v1/bids/$PROJECT_ID/clauses?include_superseded=true" -H "$AUTH")"
[[ "$(jq -r --arg id "$TECH_ID" '.[] | select(.id == $id) | .status' <<<"$ALL_CLAUSES")" == "confirmed" ]] \
  || fail_logs "confirmed clause changed during re-extraction"
[[ "$(jq -r --arg id "$REJECT_ID" '.[] | select(.id == $id) | .status' <<<"$ALL_CLAUSES")" == "rejected" ]] \
  || fail_logs "rejected clause changed during re-extraction"
[[ "$(jq '[.[] | select(.status == "superseded")] | length' <<<"$ALL_CLAUSES")" -ge 1 ]] \
  || fail_logs "old drafts were not superseded"

curl -fsS -X POST "$BASE/api/v1/bids/$PROJECT_ID" -H "$AUTH" >/dev/null
CODE="$(curl -sS -o /dev/null -w '%{http_code}' -X PATCH "$BASE/api/v1/bids/$PROJECT_ID/clauses/$REJECT_ID" -H "$AUTH" \
  -H 'content-type: application/json' -d '{"status":"draft"}')"
[[ "$CODE" == "409" ]] || fail_logs "ended project accepted a mutation (HTTP $CODE)"

echo "bid service-backed smoke: PASS project=$PROJECT_ID"
echo "real: PostgreSQL migrations/state, Redis queue, API+worker, local blob, heuristic convert/extract, edits/reject/confirm, match terminal, Section retry, booklet/preview/export/reextract/end"
echo "stubbed/not covered: local-open auth, deterministic embedding boundary, no seeded pick/shot asset, no strict Agent/VLM/DocReader provider, no dependency restart injection"
