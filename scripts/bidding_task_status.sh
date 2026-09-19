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
Sections: CURRENT (current_attempt only), ATTEMPTS (history), CURSOR (one row per completed turn).
read_bytes is the checkpoint cumulative read counter, not unique source bytes.
text_sources counts sources with committed text scan ranges, not merely delivered text.
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
 r.progress_detail->>'outline_scan_cursor' AS cursor,
 r.progress_detail->>'outline_scan_chunks' AS chunks,
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
 payload#>>'{analysis,outline,phase}' AS phase,payload->>'turn' AS turn,
 payload#>>'{outline_run,chunk_cursor}' AS cursor,
 progress_detail->>'outline_scan_chunks' AS chunks,
 (SELECT count(*) FROM jsonb_object_keys(coalesce(payload#>'{analysis,outline,requirements}','{}'))) AS reqs,
 jsonb_array_length(coalesce(payload#>'{analysis,draft_plan}','[]')) AS chapters,
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
\echo ==== CURSOR ====
WITH snapshots AS (
 SELECT batch_ordinal,created_at,convert_from(canonical_payload,'UTF8')::jsonb AS payload
 FROM bid_tender_agent_checkpoint_artifacts
 WHERE request_artifact_id=:'selected_id'::uuid AND stage_kind='analysis_checkpoint'
), turns AS (
 SELECT DISTINCT ON ((payload->>'turn')::integer)
   (payload->>'turn')::integer AS turn,created_at,payload
 FROM snapshots ORDER BY (payload->>'turn')::integer,batch_ordinal DESC,created_at DESC
)
SELECT turn,payload#>>'{outline_run,chunk_cursor}' AS cursor,
 (SELECT count(*) FROM jsonb_object_keys(coalesce(payload#>'{analysis,outline,requirements}','{}'))) AS reqs,
 (SELECT count(*) FROM jsonb_each(coalesce(payload#>'{analysis,outline,scanned,text}','{}')) e
  WHERE jsonb_array_length(e.value)>0) AS text_sources,
 payload->>'read_bytes' AS read_bytes,
 payload#>>'{analysis,outline,phase}' AS phase,
 jsonb_array_length(coalesce(payload#>'{analysis,draft_plan}','[]')) AS chapters,
 created_at AS checkpoint_at
FROM turns ORDER BY turn;
\else
\echo No matching requirement_set_compile request found.
\endif
\endif
ROLLBACK;
SQL
