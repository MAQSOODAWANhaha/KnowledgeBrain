-- Existing-database DDL for chunks.source_locator and documents.parse_status 'deleted'.
-- Fresh databases already have both from knowledge_base_baseline.sql.
-- The migrator applies only the three baselines in one transaction and does not
-- execute this file (deploy/README.md, plans/platform/runtime-foundation.md §2).
-- A database that already has platform_schema_snapshot must be rebuilt by
-- namespace reset; this script does not rewrite the schema receipt, so runtime
-- readiness stays SCHEMA_REVISION_MISMATCH until that reset.
-- documents_parse_status_check is the Postgres default name for the inline
-- column CHECK. Confirm before running:
--   SELECT conname FROM pg_constraint
--   WHERE conrelid = 'documents'::regclass
--     AND pg_get_constraintdef(oid) LIKE '%parse_status%';

BEGIN;

ALTER TABLE chunks ADD COLUMN IF NOT EXISTS source_locator jsonb;

ALTER TABLE documents DROP CONSTRAINT IF EXISTS documents_parse_status_check;
ALTER TABLE documents ADD CONSTRAINT documents_parse_status_check
  CHECK (parse_status IN (
    'pending', 'processing', 'finalizing', 'completed', 'failed', 'cancelled', 'deleting',
    'deleted'
  ));

COMMIT;
