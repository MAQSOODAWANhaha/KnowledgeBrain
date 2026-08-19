-- Brain WikiPage.ChunkRefs: cited chunk IDs, separate from document source_refs.

ALTER TABLE wiki_pages
    ADD COLUMN IF NOT EXISTS chunk_refs jsonb NOT NULL DEFAULT '[]'::jsonb;
