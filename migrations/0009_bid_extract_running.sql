-- Per-document extract parallelism. Project-wide runs (document_id NULL) still
-- serialize with each other via the nil UUID.
DROP INDEX IF EXISTS bid_extract_runs_one_running_project_uidx;
CREATE UNIQUE INDEX IF NOT EXISTS bid_extract_runs_one_running_document_uidx
    ON bid_extract_runs (project_id, COALESCE(document_id, '00000000-0000-0000-0000-000000000000'))
    WHERE status = 'running';
