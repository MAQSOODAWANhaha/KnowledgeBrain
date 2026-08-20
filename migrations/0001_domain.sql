-- KnowledgeBrain 0001: domain + members + spans + pending + DL.
-- No quota, tenant, TOKEN, embeddings, graph, wiki, models, or api_keys.

CREATE TABLE workspaces (
    id uuid PRIMARY KEY,
    name text NOT NULL,
    slug text NOT NULL UNIQUE,
    kind text NOT NULL DEFAULT 'product_line' CHECK (kind IN ('product_line', 'company')),
    retrieval_config jsonb NOT NULL DEFAULT '{"vector_threshold":0.15,"keyword_threshold":0.3,"embedding_top_k":50}'::jsonb,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX workspaces_one_company
    ON workspaces (kind) WHERE kind = 'company';

CREATE TABLE users (
    id uuid PRIMARY KEY,
    email text NOT NULL UNIQUE,
    password_hash text,
    ldap_dn text,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE workspace_members (
    workspace_id uuid NOT NULL REFERENCES workspaces (id),
    user_id uuid NOT NULL REFERENCES users (id),
    role text NOT NULL CHECK (role IN ('owner', 'admin', 'contributor', 'viewer')),
    PRIMARY KEY (workspace_id, user_id)
);

CREATE TABLE products (
    id uuid PRIMARY KEY,
    workspace_id uuid NOT NULL REFERENCES workspaces (id),
    kind text NOT NULL CHECK (kind IN ('product', 'library')),
    name text NOT NULL,
    slug text NOT NULL,
    current_version_id uuid,
    UNIQUE (workspace_id, slug)
);

CREATE TABLE product_versions (
    id uuid PRIMARY KEY,
    product_id uuid NOT NULL REFERENCES products (id),
    label text NOT NULL,
    status text NOT NULL CHECK (status IN ('cloning', 'active', 'archived', 'failed')),
    cloned_from_version_id uuid,
    chunking_config jsonb NOT NULL DEFAULT '{}'::jsonb,
    indexing_strategy jsonb NOT NULL DEFAULT '{"vector":true,"keyword":true,"wiki":true,"graph":true}'::jsonb,
    image_processing_config jsonb NOT NULL DEFAULT '{"enable_multimodel":true}'::jsonb,
    embedding_model_id text,
    summary_model_id text,
    vlm_model_id text,
    asr_model_id text,
    vlm_config jsonb NOT NULL DEFAULT '{}'::jsonb,
    asr_config jsonb NOT NULL DEFAULT '{}'::jsonb,
    extract_config jsonb NOT NULL DEFAULT '{}'::jsonb,
    wiki_config jsonb NOT NULL DEFAULT '{}'::jsonb,
    question_generation_config jsonb NOT NULL DEFAULT '{}'::jsonb,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    deleted_at timestamptz
);

CREATE UNIQUE INDEX product_versions_live_label_uidx
    ON product_versions (product_id, label)
    WHERE deleted_at IS NULL;

CREATE TABLE documents (
    id uuid PRIMARY KEY,
    product_version_id uuid NOT NULL REFERENCES product_versions (id),
    type text NOT NULL DEFAULT 'file' CHECK (type IN ('file', 'url', 'passage', 'manual')),
    title text NOT NULL,
    parse_status text NOT NULL CHECK (
        parse_status IN (
            'pending',
            'processing',
            'finalizing',
            'completed',
            'failed',
            'cancelled',
            'deleting'
        )
    ),
    pending_subtasks_count integer NOT NULL DEFAULT 0,
    summary_status text NOT NULL DEFAULT 'none' CHECK (
        summary_status IN ('none', 'pending', 'processing', 'completed', 'failed')
    ),
    enable_status text NOT NULL DEFAULT 'disabled' CHECK (enable_status IN ('disabled', 'enabled')),
    index_ready boolean NOT NULL DEFAULT false,
    description text NOT NULL DEFAULT '',
    attempt integer NOT NULL DEFAULT 1,
    file_name text NOT NULL,
    file_size bigint NOT NULL,
    file_hash text NOT NULL,
    object_key text NOT NULL,
    source_passages jsonb,
    process_overrides jsonb,
    error_message text,
    processed_at timestamptz,
    updated_at timestamptz NOT NULL DEFAULT now(),
    deleted_at timestamptz
);

CREATE UNIQUE INDEX documents_live_file_uidx
    ON documents (product_version_id, file_name, file_size, file_hash)
    WHERE deleted_at IS NULL;

CREATE TABLE tags (
    id uuid PRIMARY KEY,
    workspace_id uuid NOT NULL REFERENCES workspaces (id),
    name text NOT NULL,
    slug text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, slug)
);

CREATE TABLE document_tags (
    document_id uuid NOT NULL REFERENCES documents (id) ON DELETE CASCADE,
    tag_id uuid NOT NULL REFERENCES tags (id) ON DELETE CASCADE,
    PRIMARY KEY (document_id, tag_id)
);

CREATE TABLE content_objects (
    hash text PRIMARY KEY,
    size bigint NOT NULL,
    refcount integer NOT NULL DEFAULT 0
);

CREATE TABLE document_processing_spans (
    document_id uuid NOT NULL REFERENCES documents (id) ON DELETE CASCADE,
    attempt integer NOT NULL,
    name text NOT NULL,
    span_id uuid,
    parent_span_id uuid,
    kind text,
    status text,
    input jsonb,
    output jsonb,
    metadata jsonb,
    error_code text,
    error_message text,
    started_at timestamptz,
    finished_at timestamptz,
    duration_ms bigint,
    PRIMARY KEY (document_id, attempt, name)
);

CREATE TABLE task_pending_ops (
    id uuid PRIMARY KEY,
    task_type text NOT NULL,
    scope text NOT NULL,
    scope_id uuid NOT NULL,
    op text NOT NULL,
    dedup_key text,
    payload jsonb,
    fail_count integer NOT NULL DEFAULT 0,
    enqueued_at timestamptz NOT NULL DEFAULT now(),
    claimed_at timestamptz
);

CREATE TABLE task_dead_letters (
    id uuid PRIMARY KEY,
    task_type text NOT NULL,
    scope text NOT NULL,
    scope_id uuid,
    related_id uuid,
    payload jsonb,
    last_error text,
    fail_count integer NOT NULL DEFAULT 0,
    failed_at timestamptz NOT NULL DEFAULT now()
);
