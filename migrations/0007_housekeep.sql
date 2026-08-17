-- Housekeeping needs a document-row heartbeat (brain knowledge.updated_at).
ALTER TABLE documents
    ADD COLUMN IF NOT EXISTS updated_at timestamptz NOT NULL DEFAULT now();
