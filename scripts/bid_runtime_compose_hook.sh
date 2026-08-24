#!/usr/bin/env bash
set -euo pipefail

: "${ACCEPTANCE_BASE_URL:?ACCEPTANCE_BASE_URL is required}"
: "${ACCEPTANCE_PROJECT_ID:?ACCEPTANCE_PROJECT_ID is required}"
: "${ACCEPTANCE_AUTH_TOKEN:?ACCEPTANCE_AUTH_TOKEN is required}"
: "${ACCEPTANCE_ATTACHMENT_ID:?ACCEPTANCE_ATTACHMENT_ID is required}"
: "${ACCEPTANCE_ATTACHMENT_REVISION:?ACCEPTANCE_ATTACHMENT_REVISION is required}"
: "${ACCEPTANCE_ATTACHMENT_OBJECT_REF:?ACCEPTANCE_ATTACHMENT_OBJECT_REF is required}"
: "${ACCEPTANCE_ATTACHMENT_DIGEST:?ACCEPTANCE_ATTACHMENT_DIGEST is required}"
: "${ACCEPTANCE_ATTACHMENT_CLASSIFICATION_ID:?ACCEPTANCE_ATTACHMENT_CLASSIFICATION_ID is required}"
: "${ACCEPTANCE_EVALUATION_CLAUSE_ID:?ACCEPTANCE_EVALUATION_CLAUSE_ID is required}"
: "${ACCEPTANCE_PDF_MANIFEST_ID:?ACCEPTANCE_PDF_MANIFEST_ID is required}"
: "${ACCEPTANCE_PDF_OUTPUT_ID:?ACCEPTANCE_PDF_OUTPUT_ID is required}"
: "${ACCEPTANCE_PDF_SHA256:?ACCEPTANCE_PDF_SHA256 is required}"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
EVIDENCE_DIR="${EVIDENCE_DIR:-$ROOT/web/e2e/artifacts/runtime}"
HOOK_EVIDENCE_FILE="${ACCEPTANCE_HOOK_EVIDENCE_FILE:-$EVIDENCE_DIR/compose-hook.json}"
TMP="$(mktemp -d)"
AUTH="Authorization: Bearer $ACCEPTANCE_AUTH_TOKEN"
gate_in_maintenance=0

admin_psql() {
  docker compose exec -T postgres sh -c \
    'PGPASSWORD="$POSTGRES_PASSWORD" psql -X -v ON_ERROR_STOP=1 -U "$POSTGRES_USER" -d "$POSTGRES_DB" "$@"' sh "$@"
}

role_psql() {
  local role=$1 password=$2
  shift 2
  docker compose exec -T -e PGPASSWORD="$password" postgres \
    psql -X -v ON_ERROR_STOP=1 -U "$role" -d knowledgebrain "$@"
}

transition_gate() {
  local from=$1 to=$2 reason=$3 generation
  generation="$(printf '%s\n' \
    "WITH changed AS (" \
    "  UPDATE application_maintenance_gate" \
    "     SET mode='$to',generation=generation+1,updated_by='system:first-launch',updated_at=clock_timestamp()" \
    "   WHERE singleton_key AND mode='$from'" \
    "   RETURNING generation" \
    ")" \
    "INSERT INTO maintenance_gate_audit(id,from_mode,to_mode,generation,actor_identity,reason)" \
    "SELECT gen_random_uuid(),'$from','$to',generation,'system:first-launch','$reason' FROM changed" \
    "RETURNING generation;" | admin_psql -At)"
  [ -n "$generation" ] || fail "maintenance transition $from -> $to did not change the gate"
}

cleanup() {
  local status=$?
  if [ "$gate_in_maintenance" -eq 1 ]; then
    transition_gate maintenance open "recover failed isolated runtime acceptance hook" >/dev/null 2>&1 || true
  fi
  rm -rf "$TMP"
  exit "$status"
}
trap cleanup EXIT HUP INT TERM

fail() {
  echo "bidding Compose runtime hook failed: $*" >&2
  exit 1
}

new_id() {
  tr '[:upper:]' '[:lower:]' </proc/sys/kernel/random/uuid
}

get_json() {
  local path=$1 output=$2
  curl -fsS "$ACCEPTANCE_BASE_URL$path" -H "$AUTH" -o "$output" || fail "GET $path"
}

json_request_with_key() {
  local method=$1 path=$2 body=$3 key=$4 output=$5 status
  status="$(curl -sS -X "$method" "$ACCEPTANCE_BASE_URL$path" \
    -H "$AUTH" -H 'content-type: application/json' -H "Idempotency-Key: $key" \
    --data "$body" -o "$output" -w '%{http_code}')" || fail "$method $path"
  case "$status" in 200 | 201 | 202 | 204) ;; *) fail "$method $path returned HTTP $status" ;; esac
}

json_request() {
  json_request_with_key "$1" "$2" "$3" "$(new_id)" "$4"
}

regenerate_parts() {
  get_json "/api/v1/bids/$ACCEPTANCE_PROJECT_ID/parts" "$TMP/parts.json"
  while IFS= read -r part_key; do
    local encoded revision dependency body file_key
    encoded="$(jq -rn --arg value "$part_key" '$value|@uri')"
    file_key="$(printf '%s' "$part_key" | tr ':/' '__')"
    get_json "/api/v1/bids/$ACCEPTANCE_PROJECT_ID/parts/$encoded" "$TMP/current-part-$file_key.json"
    revision="$(jq -er .content_revision "$TMP/current-part-$file_key.json")"
    dependency="$(jq -er .dependency_sha256 "$TMP/current-part-$file_key.json")"
    if [ -n "$dependency" ]; then
      body="$(jq -cn --argjson revision "$revision" --arg dependency "$dependency" \
        '{expected_content_revision:$revision,expected_dependency_sha256:$dependency}')"
    else
      body="$(jq -cn --argjson revision "$revision" '{expected_content_revision:$revision}')"
    fi
    json_request POST "/api/v1/bids/$ACCEPTANCE_PROJECT_ID/parts/$encoded/regenerate" \
      "$body" "$TMP/part-$file_key.json"
  done < <(jq -r '.required_part_keys[]' "$TMP/parts.json")
}

mkdir -p "$EVIDENCE_DIR"

# Promote a contract override for one extracted evaluation clause. The clause
# must leave confirmed membership, carry the reconfirm marker, and block PDF
# until a human confirms the new server-owned kind.
router_identity="$(printf '%s\n' \
  "SELECT version||E'\\t'||promotion_generation FROM kind_router_current WHERE singleton_key;" |
  admin_psql -At)"
current_router="${router_identity%%$'\t'*}"
current_generation="${router_identity##*$'\t'}"
evaluation_identity="$(printf '%s\n' \
  "SELECT id||E'\\t'||revision||E'\\t'||encode(public.digest(convert_to(text,'UTF8'),'sha256'),'hex')" \
  "  FROM bid_clauses" \
  " WHERE project_id='$ACCEPTANCE_PROJECT_ID'::uuid" \
  "   AND id='$ACCEPTANCE_EVALUATION_CLAUSE_ID'::uuid AND status='confirmed'" \
  "   AND kind='evaluation' AND provenance='extracted'" \
  " LIMIT 1;" | admin_psql -At)"
[ -n "$evaluation_identity" ] || fail "no confirmed extracted evaluation clause for marker acceptance"
evaluation_clause_id="${evaluation_identity%%$'\t'*}"
evaluation_tail="${evaluation_identity#*$'\t'}"
evaluation_revision="${evaluation_tail%%$'\t'*}"
evaluation_sha256="${evaluation_tail##*$'\t'}"
target_router="acceptance-router-${evaluation_sha256%${evaluation_sha256#????????????}}"
override_payload="$(jq -cn --arg digest "$evaluation_sha256" '{overrides:{($digest):"schedule_delivery"}}')"
register_body="$(jq -cn --arg version "$target_router" --arg payload "$override_payload" \
  '{version:$version,canonical_payload:$payload}')"
transition_gate open maintenance "exercise isolated KindRouter promotion marker"
gate_in_maintenance=1
json_request POST /api/v1/maintenance/kind-router/register "$register_body" "$TMP/router-register.json"

promote_body="$(jq -cn --arg target "$target_router" --arg current "$current_router" \
  --argjson generation "$current_generation" \
  '{target_version:$target,expected_current_version:$current,expected_promotion_generation:$generation}')"
json_request POST /api/v1/maintenance/kind-router/promote "$promote_body" "$TMP/router-promote.json"
jq -e '.reconfirmation_marker_count >= 1 and .changed_clause_count >= 1' \
  "$TMP/router-promote.json" >/dev/null || fail "KindRouter promotion did not create a reconfirm marker"
transition_gate maintenance open "complete isolated KindRouter promotion marker acceptance"
gate_in_maintenance=0

get_json "/api/v1/bids/$ACCEPTANCE_PROJECT_ID/clauses?include_history=false" "$TMP/clauses-after-promotion.json"
marked_revision="$(jq -er --arg id "$evaluation_clause_id" '
  .clauses[] | select(.id==$id and .status=="draft"
    and .kind=="schedule_delivery"
    and .confirmation_required_reason=="KIND_ROUTER_PROMOTION_RECONFIRM") | .revision
' "$TMP/clauses-after-promotion.json")" || fail "promoted clause marker/current kind is invalid"
get_json "/api/v1/bids/$ACCEPTANCE_PROJECT_ID/gate-issues?format=pdf" "$TMP/marker-gate.json"
jq -e '.status == "reject" and any(.issues[]; .code == "KIND_ROUTER_RECONFIRMATION_REQUIRED")' \
  "$TMP/marker-gate.json" >/dev/null || fail "PDF gate did not reject the KindRouter marker"
json_request PATCH "/api/v1/bids/$ACCEPTANCE_PROJECT_ID/clauses/$evaluation_clause_id" \
  "$(jq -cn --argjson revision "$marked_revision" '{action:"confirm",expected_revision:$revision,patch:{}}')" \
  "$TMP/reconfirm-marker.json"
regenerate_parts
get_json "/api/v1/bids/$ACCEPTANCE_PROJECT_ID/gate-issues?format=pdf" "$TMP/gate-after-marker.json"
jq -e '.status == "pass"' "$TMP/gate-after-marker.json" >/dev/null || fail "PDF gate did not recover after marker confirmation"

# A current attachment owns its independent object until the business reference
# is released. Separately, the immutable manifest must own every frozen render
# asset occurrence even while retention is running.
protected_identity="$(printf '%s\n' \
  "SELECT registry.state||E'\\t'||count(reference_value.*)" \
  "  FROM object_registry registry" \
  "  JOIN object_owner_references reference_value ON reference_value.object_ref=registry.object_ref" \
  "   AND reference_value.owner_kind='bid_attachment'" \
  "   AND reference_value.owner_id='$ACCEPTANCE_ATTACHMENT_ID'::uuid" \
  "   AND reference_value.occurrence='original'" \
  " WHERE registry.object_ref='$ACCEPTANCE_ATTACHMENT_OBJECT_REF'::kb_object_ref" \
  "   AND registry.digest='$ACCEPTANCE_ATTACHMENT_DIGEST'::kb_sha256" \
  " GROUP BY registry.state;" | admin_psql -At)"
[ "${protected_identity%%$'\t'*}" = "available" ] || fail "attachment object is not available"
[ "${protected_identity##*$'\t'}" = "1" ] || fail "attachment object has no exact business owner reference"

manifest_asset_identity="$(printf '%s\n' \
  "SELECT count(asset.*)||E'\\t'||count(owner_ref.*)||E'\\t'||count(registry.*)" \
  "  FROM bid_manifest_render_assets asset" \
  "  LEFT JOIN object_owner_references owner_ref" \
  "    ON owner_ref.object_ref=asset.object_ref" \
  "   AND owner_ref.owner_kind='bid_manifest_asset'" \
  "   AND owner_ref.owner_id=asset.manifest_id" \
  "   AND owner_ref.occurrence=asset.id::text" \
  "  LEFT JOIN object_registry registry ON registry.object_ref=asset.object_ref" \
  "   AND registry.digest=asset.digest AND registry.state='available'" \
  " WHERE asset.manifest_id='$ACCEPTANCE_PDF_MANIFEST_ID'::uuid;" | admin_psql -At)"
IFS=$'\t' read -r manifest_asset_count manifest_owner_count manifest_available_count \
  <<<"$manifest_asset_identity"
[ "$manifest_asset_count" -ge 1 ] || fail "frozen manifest has no real render asset"
[ "$manifest_owner_count" = "$manifest_asset_count" ] || fail "frozen manifest render asset owner mismatch"
[ "$manifest_available_count" = "$manifest_asset_count" ] || fail "frozen manifest render asset is unavailable"
manifest_asset_sample="$(printf '%s\n' \
  "SELECT object_ref||E'\\t'||digest FROM bid_manifest_render_assets" \
  " WHERE manifest_id='$ACCEPTANCE_PDF_MANIFEST_ID'::uuid" \
  " ORDER BY manifest_ordinal LIMIT 1;" | admin_psql -At)"
manifest_asset_object_ref="${manifest_asset_sample%%$'\t'*}"
manifest_asset_digest="${manifest_asset_sample##*$'\t'}"

json_request POST "/api/v1/bids/$ACCEPTANCE_PROJECT_ID/procedural-requirements/$ACCEPTANCE_ATTACHMENT_CLASSIFICATION_ID/resolve" \
  '{"resolution":"not_applicable","reason":"运行验收在冻结 manifest 后释放业务附件引用"}' \
  "$TMP/release-procedural-decision.json"
json_request POST "/api/v1/bids/$ACCEPTANCE_PROJECT_ID/attachments/$ACCEPTANCE_ATTACHMENT_ID/delete" \
  "$(jq -cn --argjson revision "$ACCEPTANCE_ATTACHMENT_REVISION" \
    '{expected_revision:$revision,reason:"运行验收释放业务附件引用"}')" \
  "$TMP/delete-attachment.json"
attachment_owner_count="$(printf '%s\n' \
  "SELECT count(*) FROM object_owner_references" \
  " WHERE object_ref='$ACCEPTANCE_ATTACHMENT_OBJECT_REF'::kb_object_ref" \
  "   AND owner_kind='bid_attachment' AND owner_id='$ACCEPTANCE_ATTACHMENT_ID'::uuid" \
  "   AND occurrence='original';" | admin_psql -At)"
[ "$attachment_owner_count" = "0" ] || fail "deleted attachment retained its business owner reference"
for _ in $(seq 1 60); do
  attachment_object_state="$(printf '%s\n' \
    "SELECT state FROM object_registry WHERE object_ref='$ACCEPTANCE_ATTACHMENT_OBJECT_REF'::kb_object_ref;" |
    admin_psql -At)"
  [ "$attachment_object_state" = "deleted" ] && break
  sleep 1
done
[ "$attachment_object_state" = "deleted" ] || fail "retention did not delete the released attachment object"
curl -fsS "$ACCEPTANCE_BASE_URL/api/v1/bids/$ACCEPTANCE_PROJECT_ID/submission/artifacts/$ACCEPTANCE_PDF_OUTPUT_ID" \
  -H "$AUTH" -o "$TMP/historical-after-attachment-delete.pdf" || fail "download historical PDF after attachment delete"
historical_pdf_sha="$(sha256sum "$TMP/historical-after-attachment-delete.pdf" | awk '{print $1}')"
[ "$historical_pdf_sha" = "$ACCEPTANCE_PDF_SHA256" ] || fail "historical PDF changed after attachment release"

# Stop both asynchronous consumers before creating the recovery fixtures. The
# original Redis render message remains durable while the role-scoped reapers
# move the exact records back into states the restarted consumers can finish.
docker compose stop worker retention >/dev/null
json_request POST "/api/v1/bids/$ACCEPTANCE_PROJECT_ID/submission/manifests" \
  '{"format":"docx"}' "$TMP/recovery-manifest.json"
recovery_manifest_id="$(jq -er .manifest_id "$TMP/recovery-manifest.json")"
recovery_manifest_sha="$(jq -er .content_sha256 "$TMP/recovery-manifest.json")"
recovery_render_key="$(new_id)"
json_request_with_key POST "/api/v1/bids/$ACCEPTANCE_PROJECT_ID/submission/manifests/$recovery_manifest_id/render" \
  "$(jq -cn --arg digest "$recovery_manifest_sha" '{expected_manifest_sha256:$digest}')" \
  "$recovery_render_key" "$TMP/recovery-render.json"
recovery_render_job_id="$(jq -er .render_job_id "$TMP/recovery-render.json")"
recovery_claim_token="$(new_id)"
claimed="$(role_psql kb_runtime_worker acceptance-worker -Atc \
  "SELECT kb_bid_claim_submission_render('$recovery_render_job_id'::uuid,'$recovery_claim_token'::uuid);")"
printf '%s' "$claimed" | jq -e --arg id "$recovery_render_job_id" '.render_job_id==$id' >/dev/null ||
  fail "worker role could not create the active render recovery fixture"
recovery_claim_lease_ms="$(printf '%s' "$claimed" | jq -er .claim_lease_ms)"
recovery_attempt_count_before="$(printf '%s' "$claimed" | jq -er .attempt_count)"
[ "$recovery_claim_lease_ms" -ge 1000 ] || fail "recovery render claim lease is invalid"
[ "$recovery_attempt_count_before" = "1" ] || fail "recovery render did not start at attempt 1"
concurrent_claim_fenced="$(role_psql kb_runtime_worker acceptance-worker -Atc \
  "SELECT kb_bid_claim_submission_render('$recovery_render_job_id'::uuid,'$(new_id)'::uuid) IS NULL;")"
[ "$concurrent_claim_fenced" = "t" ] || fail "fresh active render claim was not fenced"

staging_id="$(new_id)"
staging_digest="$(printf '%s' "$staging_id" | sha256sum | awk '{print $1}')"
staging_ref="objects/$staging_digest"
project_actor="$(printf '%s\n' \
  "SELECT 'user:'||owner_user_id FROM bid_projects WHERE id='$ACCEPTANCE_PROJECT_ID'::uuid;" |
  admin_psql -At)"
role_psql kb_runtime_worker acceptance-worker -Atc \
  "SELECT kb_object_upload_stage('$staging_id'::uuid,'$staging_ref'::kb_object_ref,'$staging_digest'::kb_sha256,'application/octet-stream',0,'$project_actor'::kb_actor_identity);" \
  >/dev/null
sleep 1
aged_claim="$(printf '%s\n' \
  "WITH updated AS (" \
  "  UPDATE bid_submission_render_jobs" \
  "     SET heartbeat_at=clock_timestamp()-make_interval(secs=>claim_lease_ms::double precision/1000.0)-interval '1 second'" \
  "   WHERE id='$recovery_render_job_id'::uuid AND status='running'" \
  "     AND claim_token='$recovery_claim_token'::uuid" \
  "   RETURNING 1" \
  ") SELECT count(*) FROM updated;" | admin_psql -At)"
[ "$aged_claim" = "1" ] || fail "could not expire the isolated active render fixture"
reaped="$(role_psql kb_runtime_worker acceptance-worker -Atc 'SELECT kb_bid_reap_submission_renders();')"
[ "$reaped" -ge 1 ] || fail "expired active render claim was not reaped"
reaped_target="$(printf '%s\n' \
  "SELECT status||E'\\t'||attempt_count||E'\\t'||(claim_token IS NULL)||E'\\t'||" \
  "       (heartbeat_at IS NULL)||E'\\t'||COALESCE(error_code,'')" \
  "  FROM bid_submission_render_jobs WHERE id='$recovery_render_job_id'::uuid;" |
  admin_psql -At)"
IFS=$'\t' read -r reaped_status reaped_attempt reaped_claim_cleared reaped_heartbeat_cleared reaped_error \
  <<<"$reaped_target"
[ "$reaped_status" = "pending" ] || fail "reaped render did not return to pending"
[ "$reaped_attempt" = "1" ] || fail "reaping changed the render attempt count"
[ "$reaped_claim_cleared" = "true" ] || fail "reaping retained the old render claim token"
[ "$reaped_heartbeat_cleared" = "true" ] || fail "reaping retained the old render heartbeat"
[ "$reaped_error" = "CLAIM_LEASE_EXPIRED" ] || fail "reaped render lacks the lease-expired reason"
old_token_heartbeat="$(role_psql kb_runtime_worker acceptance-worker -Atc \
  "SELECT kb_bid_heartbeat_submission_render('$recovery_render_job_id'::uuid,'$recovery_claim_token'::uuid);")"
[ "$old_token_heartbeat" = "f" ] || fail "expired render claim token was not fenced"

expired_staging_row="$(printf '%s\n' \
  "WITH updated AS (" \
  "  UPDATE object_upload_staging SET expires_at=created_at+interval '1 millisecond'" \
  "   WHERE id='$staging_id'::uuid" \
  "   RETURNING 1" \
  ") SELECT count(*) FROM updated;" | admin_psql -At)"
[ "$expired_staging_row" = "1" ] || fail "could not expire the isolated upload staging fixture"
expired_staging="$(role_psql kb_runtime_worker acceptance-worker -Atc 'SELECT kb_object_upload_expire();')"
[ "$expired_staging" -ge 1 ] || fail "expired upload staging reference was not recovered"
staging_after_expire="$(printf '%s\n' \
  "SELECT registry.state||E'\\t'||" \
  "       (NOT EXISTS (SELECT 1 FROM object_upload_staging staging WHERE staging.id='$staging_id'::uuid))||E'\\t'||" \
  "       COALESCE((SELECT outbox.state FROM object_retention_outbox outbox" \
  "                  WHERE outbox.object_ref=registry.object_ref),'missing')" \
  "  FROM object_registry registry WHERE registry.object_ref='$staging_ref'::kb_object_ref;" |
  admin_psql -At)"
IFS=$'\t' read -r staging_reaped_state staging_row_removed staging_outbox_state \
  <<<"$staging_after_expire"
[ "$staging_reaped_state" = "deleting" ] || fail "expired staging object did not enter deleting"
[ "$staging_row_removed" = "true" ] || fail "expired staging row was not removed"
[ "$staging_outbox_state" = "queued" ] || fail "expired staging object was not queued for retention"

docker compose up -d --no-deps worker retention >/dev/null
for _ in $(seq 1 120); do
  get_json "/api/v1/bids/$ACCEPTANCE_PROJECT_ID/submission/render-jobs/$recovery_render_job_id" "$TMP/recovery-status.json"
  recovery_status="$(jq -r .status "$TMP/recovery-status.json")"
  [ "$recovery_status" != "failed" ] || fail "recovered render entered failed"
  [ "$recovery_status" != "completed" ] || break
  sleep 1
done
[ "$(jq -r .status "$TMP/recovery-status.json")" = "completed" ] || fail "recovered render did not complete"
recovery_attempt_count_after="$(jq -er .attempt_count "$TMP/recovery-status.json")"
[ "$recovery_attempt_count_after" = "2" ] || fail "recovered render did not complete on attempt 2"
for _ in $(seq 1 60); do
  staging_state="$(printf '%s\n' \
    "SELECT COALESCE((SELECT state FROM object_registry WHERE object_ref='$staging_ref'::kb_object_ref),'missing');" |
    admin_psql -At)"
  [ "$staging_state" = "deleted" ] && break
  sleep 1
done
[ "$staging_state" = "deleted" ] || fail "retention did not delete the released staging object"

jq -n \
  --arg target_router "$target_router" \
  --arg evaluation_clause_id "$evaluation_clause_id" \
  --argjson promotion_generation "$((current_generation + 1))" \
  --arg attachment_object_ref "$ACCEPTANCE_ATTACHMENT_OBJECT_REF" \
  --arg attachment_digest "$ACCEPTANCE_ATTACHMENT_DIGEST" \
  --arg manifest_asset_object_ref "$manifest_asset_object_ref" \
  --arg manifest_asset_digest "$manifest_asset_digest" \
  --argjson manifest_asset_count "$manifest_asset_count" \
  --arg historical_pdf_sha256 "$historical_pdf_sha" \
  --arg recovery_render_job_id "$recovery_render_job_id" \
  --argjson recovery_claim_lease_ms "$recovery_claim_lease_ms" \
  --argjson recovery_attempt_count_before "$recovery_attempt_count_before" \
  --argjson recovery_attempt_count_after "$recovery_attempt_count_after" \
  --arg reaped_status "$reaped_status" \
  --arg recovery_staging_object_ref "$staging_ref" \
  '{schema_version:1,status:"passed",kind_router:{target_version:$target_router,promotion_generation:$promotion_generation,
      evaluation_clause_id:$evaluation_clause_id,marker_rejected_pdf:true,reconfirmed:true},
    object_lifecycle:{attachment_object_ref:$attachment_object_ref,attachment_digest:$attachment_digest,
      attachment_released_deleted:true,manifest_asset_object_ref:$manifest_asset_object_ref,
      manifest_asset_digest:$manifest_asset_digest,manifest_asset_count:$manifest_asset_count,
      manifest_owner_present_and_available:true,historical_pdf_sha256:$historical_pdf_sha256,
      recovered_staging_object_ref:$recovery_staging_object_ref,staging_state_after_expire:"deleting",
      staging_outbox_queued:true,released_staging_deleted:true},
    restart_recovery:{render_job_id:$recovery_render_job_id,claim_lease_ms:$recovery_claim_lease_ms,
      attempt_count_before_recovery:$recovery_attempt_count_before,
      fresh_claim_fenced:true,target_state_after_reap:$reaped_status,old_token_fenced:true,
      active_claim_reaped:true,attempt_count_after_recovery:$recovery_attempt_count_after,
      completed_after_restart:true}}' \
  >"$HOOK_EVIDENCE_FILE"
if [ "$HOOK_EVIDENCE_FILE" != "$EVIDENCE_DIR/compose-hook.json" ]; then
  cp "$HOOK_EVIDENCE_FILE" "$EVIDENCE_DIR/compose-hook.json"
fi

echo "bidding Compose runtime hook passed"
