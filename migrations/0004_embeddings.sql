-- KnowledgeBrain 0004: chunks + pgvector embeddings. D=32 matches stub-emb.

CREATE EXTENSION IF NOT EXISTS vector;

CREATE TABLE IF NOT EXISTS chunks (
    id uuid PRIMARY KEY,
    product_version_id uuid NOT NULL REFERENCES product_versions (id),
    document_id uuid NOT NULL REFERENCES documents (id) ON DELETE CASCADE,
    chunk_type text NOT NULL,
    content text NOT NULL,
    context_header text NOT NULL DEFAULT '',
    start_at integer NOT NULL DEFAULT 0,
    end_at integer NOT NULL DEFAULT 0,
    parent_chunk_id uuid,
    generated_questions jsonb NOT NULL DEFAULT '[]'::jsonb
);

CREATE INDEX IF NOT EXISTS chunks_document_idx ON chunks (document_id);
CREATE INDEX IF NOT EXISTS chunks_version_idx ON chunks (product_version_id);

CREATE TABLE IF NOT EXISTS chunk_embeddings (
    chunk_id uuid PRIMARY KEY REFERENCES chunks (id) ON DELETE CASCADE,
    product_version_id uuid NOT NULL,
    document_id uuid NOT NULL,
    embedding vector(32),
    tsv tsvector,
    content text NOT NULL
);

CREATE INDEX IF NOT EXISTS chunk_embeddings_hnsw
    ON chunk_embeddings USING hnsw (embedding vector_cosine_ops);
CREATE INDEX IF NOT EXISTS chunk_embeddings_tsv
    ON chunk_embeddings USING gin (tsv);
CREATE INDEX IF NOT EXISTS chunk_embeddings_version_idx
    ON chunk_embeddings (product_version_id);
