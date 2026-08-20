-- KnowledgeBrain 0006: wiki pages / folders / log. Scope is product_version.

CREATE TABLE IF NOT EXISTS wiki_pages (
    id uuid PRIMARY KEY,
    product_version_id uuid NOT NULL REFERENCES product_versions (id),
    slug text NOT NULL,
    title text NOT NULL,
    page_type text NOT NULL DEFAULT 'summary',
    status text NOT NULL CHECK (status IN ('draft', 'published', 'archived')),
    content text NOT NULL DEFAULT '',
    summary text NOT NULL DEFAULT '',
    aliases jsonb NOT NULL DEFAULT '[]'::jsonb,
    parent_slug text,
    folder_id uuid,
    category_path jsonb NOT NULL DEFAULT '[]'::jsonb,
    source_refs jsonb NOT NULL DEFAULT '[]'::jsonb,
    chunk_refs jsonb NOT NULL DEFAULT '[]'::jsonb,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    deleted_at timestamptz,
    UNIQUE (product_version_id, slug)
);

CREATE TABLE IF NOT EXISTS wiki_folders (
    id uuid PRIMARY KEY,
    product_version_id uuid NOT NULL REFERENCES product_versions (id),
    parent_id uuid,
    name text NOT NULL,
    path text NOT NULL,
    depth integer NOT NULL DEFAULT 0,
    sort_order integer NOT NULL DEFAULT 0,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    deleted_at timestamptz
);

CREATE TABLE IF NOT EXISTS wiki_log_entries (
    id uuid PRIMARY KEY,
    product_version_id uuid NOT NULL REFERENCES product_versions (id),
    document_id uuid,
    level text NOT NULL DEFAULT 'info',
    message text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);
