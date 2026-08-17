-- KnowledgeBrain 0002: model catalog.

CREATE TABLE IF NOT EXISTS models (
    id text PRIMARY KEY,
    kind text NOT NULL CHECK (kind IN ('embedding', 'chat', 'vlm', 'asr')),
    endpoint text,
    api_key_enc text,
    dimension integer,
    extra jsonb NOT NULL DEFAULT '{}'::jsonb
);
