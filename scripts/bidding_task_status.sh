#!/usr/bin/env bash
# Read-only tender outline status. No argument selects the latest request.
set -euo pipefail
if [[ ${1:-} == --help || ${1:-} == -h ]]; then
  cat <<'HELP'
Usage: scripts/bidding_task_status.sh [--list | request_artifact_id]
--list lists all requirement_set_compile requests, newest first.
Copy a request_artifact_id from the list to inspect its detailed progress.
Omit the ID to inspect the latest requirement_set_compile request.
Environment: KB_POSTGRES_CONTAINER (default knowledgebrain-postgres).
POSTGRES_USER and POSTGRES_DB are read inside the container (default knowledgebrain).
Sections: CURRENT (current_attempt only), ATTEMPTS (history), PHASE (one row per completed turn).
Phase is discover, outline (organize slots, then check), check, or complete.
read_bytes is the checkpoint cumulative read counter.
Elapsed time is wall time, including time waiting for manual continuation.
HELP
  exit 0
fi
if [[ $# -gt 1 || ( $# -eq 1 && $1 != --list && ! $1 =~ ^[[:xdigit:]]{8}-[[:xdigit:]]{4}-[[:xdigit:]]{4}-[[:xdigit:]]{4}-[[:xdigit:]]{12}$ ) ]]; then
  echo 'Expected --list or an optional UUID; use --help for usage.' >&2
  exit 2
fi
docker exec -i "${KB_POSTGRES_CONTAINER:-knowledgebrain-postgres}" sh -c '
  exec psql -X -v ON_ERROR_STOP=1 -P pager=off -U "${POSTGRES_USER:-knowledgebrain}" -d "${POSTGRES_DB:-knowledgebrain}" -v requested_id="$1"
' sh "${1:-}" <<'SQL'
BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY;
SET LOCAL statement_timeout = '30s';
SELECT :'requested_id' = '--list' AS list_requests \gset
\if :list_requests
\echo ==== OUTLINE REQUESTS (newest first; current attempt only) ====
SELECT q.id AS request_artifact_id,q.project_id,q.status AS request_status,
 q.current_attempt,r.status AS run_status,
 r.progress_detail->>'outline_phase' AS phase,
 r.progress_detail->>'outline_pack_committed' AS packs_committed,
 r.progress_detail->>'outline_pack_total' AS packs_total,
 r.progress_detail->>'outline_slots_submitted' AS slots,
 r.progress_detail->>'outline_finished' AS finished,
 r.progress_detail->>'outline_requirements' AS reqs,
 r.progress_detail->>'outline_chapters' AS chapters,
 r.turn_count,q.created_at,q.finished_at,
 coalesce(q.error_code,r.last_error_code) AS last_error_code
FROM bid_async_request_snapshot_artifacts q
LEFT JOIN bid_tender_agent_run_artifacts r
 ON r.request_artifact_id=q.id AND r.attempt=q.current_attempt
WHERE q.request_kind='requirement_set_compile'
ORDER BY q.created_at DESC,q.id DESC;
\echo Details: ./scripts/bidding_task_status.sh <request_artifact_id>
\else
SELECT coalesce((SELECT id::text FROM bid_async_request_snapshot_artifacts
 WHERE request_kind='requirement_set_compile'
   AND (:'requested_id'='' OR id::text=lower(:'requested_id'))
 ORDER BY created_at DESC,id DESC LIMIT 1),'') AS selected_id \gset
SELECT :'selected_id' <> '' AS found \gset
\if :found
\echo ==== CURRENT (request status + current_attempt) ====
WITH current AS (
 SELECT q.id,q.status AS request_status,q.current_attempt,q.error_code,q.created_at,q.finished_at,
        r.status AS run_status,r.turn_count,r.text_bytes_read,r.started_at,r.updated_at,
        r.hard_deadline_at,r.last_error_at,r.last_error_code,r.last_error_message,
        r.progress_detail, s.payload
 FROM bid_async_request_snapshot_artifacts q
 LEFT JOIN bid_tender_agent_run_artifacts r
   ON r.request_artifact_id=q.id AND r.attempt=q.current_attempt
 LEFT JOIN LATERAL (
   SELECT convert_from(canonical_payload,'UTF8')::jsonb AS payload
   FROM bid_tender_agent_checkpoint_artifacts
   WHERE request_artifact_id=q.id AND stage_kind='analysis_checkpoint'
   ORDER BY batch_ordinal DESC,created_at DESC LIMIT 1
 ) s ON true
 WHERE q.id=:'selected_id'::uuid
)
SELECT id AS request_artifact_id,request_status AS status,current_attempt,run_status,
 coalesce(progress_detail->>'outline_phase', payload#>>'{outline_run,phase}') AS phase,
 payload->>'turn' AS turn,
 progress_detail->>'outline_pack_committed' AS packs_committed,
 progress_detail->>'outline_pack_total' AS packs_total,
 progress_detail->>'outline_slots_submitted' AS slots,
 progress_detail->>'outline_finished' AS finished,
 coalesce(progress_detail->>'outline_requirements',
   (SELECT count(*)::text FROM jsonb_object_keys(coalesce(payload#>'{analysis,outline,requirements}','{}')))) AS reqs,
 coalesce(progress_detail->>'outline_chapters',
   jsonb_array_length(coalesce(payload#>'{outline_run,tool_draft,chapters}', payload#>'{analysis,draft_plan}', '[]'::jsonb))::text) AS chapters,
 turn_count,text_bytes_read,
 coalesce(finished_at,now())-coalesce(started_at,created_at) AS elapsed,
 coalesce(error_code,last_error_code) AS last_error_code
FROM current;
\echo ==== EXECUTION / PUBLICATION ====
SELECT q.status AS request_status,q.finished_at,r.progress_detail->>'phase' AS execution_phase,
 r.hard_deadline_at,
 CASE WHEN r.status='retry_yielded' AND r.last_error_code='AGENT_TRANSPORT_INTERRUPTED'
   THEN greatest(r.hard_deadline_at-r.last_error_at,interval '0 seconds')
   WHEN r.status='running' THEN greatest(r.hard_deadline_at-now(),interval '0 seconds')
 END AS remaining_execution_budget,
 r.last_error_message,q.result_identity
FROM bid_async_request_snapshot_artifacts q
LEFT JOIN bid_tender_agent_run_artifacts r ON r.request_artifact_id=q.id AND r.attempt=q.current_attempt
WHERE q.id=:'selected_id'::uuid;
\echo ==== ATTEMPTS (historical interruptions are not current failures) ====
SELECT attempt,status,turn_count,started_at,lease_acquired_at,updated_at,last_error_code
FROM bid_tender_agent_run_artifacts WHERE request_artifact_id=:'selected_id'::uuid ORDER BY attempt;
\echo ==== PHASE ====
WITH snapshots AS (
 SELECT batch_ordinal,created_at,convert_from(canonical_payload,'UTF8')::jsonb AS payload
 FROM bid_tender_agent_checkpoint_artifacts
 WHERE request_artifact_id=:'selected_id'::uuid AND stage_kind='analysis_checkpoint'
), turns AS (
 SELECT DISTINCT ON ((payload->>'turn')::integer)
   (payload->>'turn')::integer AS turn,created_at,payload
 FROM snapshots ORDER BY (payload->>'turn')::integer,batch_ordinal DESC,created_at DESC
)
SELECT turn,payload#>>'{outline_run,phase}' AS phase,
 payload#>>'{outline_run,tool_draft,slots_submitted}' AS slots_submitted,
 payload#>>'{outline_run,tool_draft,finished}' AS finished,
 jsonb_array_length(coalesce(payload#>'{outline_run,tool_draft,chapters}','[]'::jsonb)) AS chapters,
 (SELECT count(*) FROM jsonb_object_keys(coalesce(payload#>'{analysis,outline,requirements}','{}'))) AS reqs,
 payload->>'read_bytes' AS read_bytes,
 created_at AS checkpoint_at
FROM turns ORDER BY turn;
\else
\echo No matching requirement_set_compile request found.
\endif
\endif
ROLLBACK;
SQL
