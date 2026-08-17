-- KnowledgeBrain 0003: API keys. scope_type = workspace | product.

CREATE TABLE IF NOT EXISTS api_keys (
    id uuid PRIMARY KEY,
    name text NOT NULL,
    key_hash text NOT NULL UNIQUE,
    prefix text NOT NULL,
    scope_type text NOT NULL CHECK (scope_type IN ('workspace', 'product')),
    scope_id uuid NOT NULL,
    scopes text[] NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS api_keys_scope_idx ON api_keys (scope_type, scope_id);
