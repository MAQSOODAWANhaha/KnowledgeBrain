-- KnowledgeBrain 0008: widen embeddings from stub 32 to production 1024.
-- Existing 32-d stub vectors cannot be cast; recreate the column when needed.

DO $$
DECLARE
    typ text;
BEGIN
    IF to_regclass('public.chunk_embeddings') IS NULL THEN
        RETURN;
    END IF;
    SELECT format_type(atttypid, atttypmod) INTO typ
    FROM pg_attribute
    WHERE attrelid = 'public.chunk_embeddings'::regclass
      AND attname = 'embedding'
      AND NOT attisdropped;
    IF typ IS NULL OR typ <> 'vector(1024)' THEN
        DROP INDEX IF EXISTS chunk_embeddings_hnsw;
        ALTER TABLE chunk_embeddings DROP COLUMN IF EXISTS embedding;
        ALTER TABLE chunk_embeddings ADD COLUMN embedding vector(1024);
        CREATE INDEX chunk_embeddings_hnsw
            ON chunk_embeddings USING hnsw (embedding vector_cosine_ops);
    END IF;
END $$;
