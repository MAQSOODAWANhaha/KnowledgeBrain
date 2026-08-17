-- Live-file identity is unique only while the row is not soft-deleted.
-- source_passages keeps passage ingest so reparse does not Split.

ALTER TABLE documents ADD COLUMN IF NOT EXISTS source_passages jsonb;

ALTER TABLE documents DROP CONSTRAINT IF EXISTS documents_product_version_id_file_name_file_size_file_hash_key;

CREATE UNIQUE INDEX IF NOT EXISTS documents_live_file_uidx
    ON documents (product_version_id, file_name, file_size, file_hash)
    WHERE deleted_at IS NULL;
