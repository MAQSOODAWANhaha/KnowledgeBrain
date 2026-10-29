SET LOCAL search_path = public, pg_catalog;

-- KnowledgeBrain final V1 fresh baseline: knowledge-base-owned catalog.
-- Knowledge catalogs belong to their creator. Service code checks ownership.
-- Object lifecycle, index publication, outbox and generation transitions are
-- owned by Rust transactions. The database contains no application functions/triggers.
-- It is create-only: no compatibility objects, backfill, repair, or upgrade DDL.

CREATE EXTENSION IF NOT EXISTS vector;
CREATE EXTENSION IF NOT EXISTS pgcrypto;

CREATE TABLE knowledge_workspaces (
    id uuid PRIMARY KEY,
    owner_user_id uuid NOT NULL REFERENCES platform_users (id),
    name text NOT NULL,
    slug text NOT NULL,
    kind text NOT NULL DEFAULT 'product_line' CHECK (kind IN ('product_line', 'company')),
    retrieval_config jsonb NOT NULL DEFAULT '{"vector_threshold":0.15,"keyword_threshold":0.3,"embedding_top_k":50}'::jsonb,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    deleted_at timestamptz,
    UNIQUE (owner_user_id, slug)
);
CREATE UNIQUE INDEX knowledge_workspaces_company_uidx ON knowledge_workspaces (owner_user_id)
    WHERE kind = 'company' AND deleted_at IS NULL;

CREATE TABLE knowledge_products (
    id uuid PRIMARY KEY,
    workspace_id uuid NOT NULL REFERENCES knowledge_workspaces (id),
    kind text NOT NULL CHECK (kind IN ('product', 'library')),
    name text NOT NULL,
    slug text NOT NULL,
    current_version_id uuid,
    UNIQUE (workspace_id, slug)
);

CREATE TABLE knowledge_product_versions (
    id uuid PRIMARY KEY,
    product_id uuid NOT NULL REFERENCES knowledge_products (id),
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
    deleted_at timestamptz,
    UNIQUE (id, product_id),
    CONSTRAINT knowledge_versions_clone_source_fk
        FOREIGN KEY (cloned_from_version_id, product_id)
        REFERENCES knowledge_product_versions (id, product_id)
        DEFERRABLE INITIALLY DEFERRED
);
ALTER TABLE knowledge_products ADD CONSTRAINT knowledge_products_current_version_fk
    FOREIGN KEY (current_version_id, id)
    REFERENCES knowledge_product_versions (id, product_id)
    DEFERRABLE INITIALLY DEFERRED;
CREATE INDEX knowledge_product_versions_product_idx ON knowledge_product_versions (product_id);

CREATE UNIQUE INDEX knowledge_product_versions_live_label_uidx
    ON knowledge_product_versions (product_id, label) WHERE deleted_at IS NULL;

CREATE TABLE knowledge_documents (
    id uuid PRIMARY KEY,
    product_version_id uuid NOT NULL REFERENCES knowledge_product_versions (id),
    document_kind text NOT NULL DEFAULT 'file' CHECK (document_kind IN ('file', 'url', 'passage', 'manual')),
    title text NOT NULL,
    parse_status text NOT NULL CHECK (parse_status IN (
        'pending', 'processing', 'finalizing', 'completed', 'failed', 'cancelled', 'deleting',
        'deleted'
    )),
    pending_subtasks_count integer NOT NULL DEFAULT 0,
    summary_status text NOT NULL DEFAULT 'none'
        CHECK (summary_status IN ('none', 'pending', 'processing', 'completed', 'failed')),
    enable_status text NOT NULL DEFAULT 'disabled' CHECK (enable_status IN ('disabled', 'enabled')),
    index_ready boolean NOT NULL DEFAULT false,
    description text NOT NULL DEFAULT '',
    attempt integer NOT NULL DEFAULT 1,
    active_generation integer NOT NULL DEFAULT 1,
    file_name text NOT NULL,
    byte_length bigint NOT NULL CHECK (byte_length >= 0),
    content_sha256 text NOT NULL CHECK (content_sha256 ~ '^[0-9a-f]{64}$'),
    object_ref text NOT NULL CHECK (object_ref = 'objects/' || content_sha256),
    source_passages jsonb,
    process_overrides jsonb,
    error_message text,
    processed_at timestamptz,
    updated_at timestamptz NOT NULL DEFAULT now(),
    deleted_at timestamptz,
    derived_status text GENERATED ALWAYS AS (
        CASE WHEN summary_status='failed' OR (parse_status='failed' AND pending_subtasks_count>0) THEN 'failed'
             WHEN pending_subtasks_count>0 OR summary_status IN ('pending','processing') THEN 'pending'
             WHEN parse_status='completed' THEN 'ready' ELSE 'none' END) STORED,
    index_status text GENERATED ALWAYS AS (
        CASE WHEN parse_status IN ('deleting','deleted','cancelled') THEN 'unavailable'
             WHEN parse_status='failed' THEN 'failed'
             WHEN index_ready AND active_generation=attempt THEN 'ready' ELSE 'building' END) STORED,
    CONSTRAINT knowledge_documents_object_fk FOREIGN KEY (object_ref)
        REFERENCES platform_objects (object_ref) DEFERRABLE INITIALLY DEFERRED,
    UNIQUE (id,product_version_id)
);
CREATE UNIQUE INDEX knowledge_documents_live_file_uidx
    ON knowledge_documents (product_version_id, file_name, byte_length, content_sha256) WHERE deleted_at IS NULL;

CREATE TABLE knowledge_tags (
    id uuid PRIMARY KEY,
    workspace_id uuid NOT NULL REFERENCES knowledge_workspaces (id),
    name text NOT NULL,
    slug text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, slug)
);
CREATE TABLE knowledge_document_tags (
    document_id uuid NOT NULL REFERENCES knowledge_documents (id) ON DELETE CASCADE,
    tag_id uuid NOT NULL REFERENCES knowledge_tags (id) ON DELETE CASCADE,
    PRIMARY KEY (document_id, tag_id)
);

CREATE TABLE knowledge_document_processing_spans (
    document_id uuid NOT NULL REFERENCES knowledge_documents (id) ON DELETE CASCADE,
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

CREATE TABLE knowledge_chunks (
    id uuid PRIMARY KEY,
    product_version_id uuid NOT NULL,
    document_id uuid NOT NULL,
    generation integer NOT NULL DEFAULT 1,
    chunk_type text NOT NULL,
    content text NOT NULL,
    context_header text NOT NULL DEFAULT '',
    start_offset integer NOT NULL DEFAULT 0,
    end_offset integer NOT NULL DEFAULT 0,
    parent_chunk_id uuid,
    generated_questions jsonb NOT NULL DEFAULT '[]'::jsonb,
    source_locator jsonb,
    UNIQUE (id,product_version_id),
    UNIQUE (id,product_version_id,document_id),
    CONSTRAINT knowledge_chunks_source_fk FOREIGN KEY (document_id, product_version_id)
        REFERENCES knowledge_documents (id, product_version_id) ON DELETE CASCADE,
    CONSTRAINT knowledge_chunks_parent_fk
        FOREIGN KEY (parent_chunk_id, product_version_id, document_id)
        REFERENCES knowledge_chunks (id, product_version_id, document_id)
        ON DELETE SET NULL (parent_chunk_id) DEFERRABLE INITIALLY DEFERRED
);
CREATE INDEX knowledge_chunks_version_idx ON knowledge_chunks (product_version_id);

-- Image metadata and OCR chunks share a document/version identity. Foreign keys
-- retain that relation across retries and cloning; the service owns publication.
CREATE TABLE knowledge_image_artifact_revisions (
    id uuid PRIMARY KEY,
    product_version_id uuid NOT NULL REFERENCES knowledge_product_versions(id),
    document_id uuid NOT NULL,
    revision integer NOT NULL CHECK (revision>0),
    object_ref text NOT NULL CHECK (object_ref ~ '^objects/[0-9a-f]{64}$'),
    content_sha256 text NOT NULL CHECK (content_sha256 ~ '^[0-9a-f]{64}$'),
    media_type text NOT NULL CHECK (media_type IN ('image/png','image/jpeg','image/webp')),
    object_state text NOT NULL DEFAULT 'available' CHECK (object_state='available'),
    width integer NOT NULL CHECK (width>0),
    height integer NOT NULL CHECK (height>0),
    page_ordinal integer CHECK (page_ordinal>=0),
    bounding_region jsonb,
    source_image_key text NOT NULL CHECK (octet_length(source_image_key) BETWEEN 1 AND 1024),
    canonical_payload bytea NOT NULL,
    artifact_sha256 text NOT NULL CHECK (artifact_sha256 ~ '^[0-9a-f]{64}$'),
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE(id,product_version_id,document_id),
    UNIQUE(id,object_ref,content_sha256,media_type,object_state),
    FOREIGN KEY(document_id,product_version_id) REFERENCES knowledge_documents(id,product_version_id),
    FOREIGN KEY(object_ref,content_sha256,media_type,object_state)
      REFERENCES platform_objects(object_ref,content_sha256,media_type,state),
    CHECK (object_ref='objects/'||content_sha256),
    CHECK (artifact_sha256=encode(public.digest(canonical_payload,'sha256'),'hex')),
    CHECK (bounding_region IS NULL OR jsonb_typeof(bounding_region)='object')
);

CREATE TABLE knowledge_image_chunk_links (
    chunk_id uuid PRIMARY KEY,
    product_version_id uuid NOT NULL,
    document_id uuid NOT NULL,
    image_artifact_revision_id uuid NOT NULL,
    object_ref text NOT NULL CHECK (object_ref ~ '^objects/[0-9a-f]{64}$'),
    content_sha256 text NOT NULL CHECK (content_sha256 ~ '^[0-9a-f]{64}$'),
    media_type text NOT NULL,
    object_state text NOT NULL DEFAULT 'available' CHECK (object_state='available'),
    created_at timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY(chunk_id,product_version_id,document_id)
      REFERENCES knowledge_chunks(id,product_version_id,document_id)
      ON DELETE CASCADE,
    FOREIGN KEY(image_artifact_revision_id,product_version_id,document_id)
      REFERENCES knowledge_image_artifact_revisions(id,product_version_id,document_id),
    FOREIGN KEY(image_artifact_revision_id,object_ref,content_sha256,media_type,object_state)
      REFERENCES knowledge_image_artifact_revisions(id,object_ref,content_sha256,media_type,object_state)
);

CREATE TABLE knowledge_chunk_embeddings (
    chunk_id uuid PRIMARY KEY,
    product_version_id uuid NOT NULL,
    document_id uuid NOT NULL,
    embedding vector(1024),
    tsv tsvector,
    content text NOT NULL,
    CONSTRAINT knowledge_chunk_embeddings_source_fk
        FOREIGN KEY (chunk_id, product_version_id, document_id)
        REFERENCES knowledge_chunks (id, product_version_id, document_id) ON DELETE CASCADE
);
CREATE INDEX knowledge_chunk_embeddings_hnsw ON knowledge_chunk_embeddings USING hnsw (embedding vector_cosine_ops);
CREATE INDEX knowledge_chunk_embeddings_tsv ON knowledge_chunk_embeddings USING gin (tsv);
CREATE INDEX knowledge_chunk_embeddings_version_idx ON knowledge_chunk_embeddings (product_version_id);

-- Embedding behavior is an immutable canonical artifact. Credentials are
-- operational registry metadata and are intentionally outside the digest.

CREATE TABLE knowledge_embedding_revisions (
    revision_sha256 text PRIMARY KEY CHECK (revision_sha256 ~ '^[0-9a-f]{64}$'),
    canonical_revision_payload bytea NOT NULL,
    schema_version smallint NOT NULL CHECK (schema_version=2),
    provider_protocol_version text NOT NULL
        CHECK (provider_protocol_version='openai-compatible-embeddings-json-v1'),
    provider_model_identifier text NOT NULL CHECK (octet_length(provider_model_identifier) BETWEEN 1 AND 256),
    provider_model_revision_sha256 text NOT NULL
        CHECK (provider_model_revision_sha256 ~ '^[0-9a-f]{64}$'),
    endpoint_config_sha256 text NOT NULL
        CHECK (endpoint_config_sha256 ~ '^[0-9a-f]{64}$'),
    endpoint_identity text NOT NULL CHECK (endpoint_identity<>''),
    dimension integer NOT NULL CHECK (dimension=1024),
    request_config_sha256 text NOT NULL CHECK (
        request_config_sha256='a2ccbf02dc959b101e69f85df1b494ae0852065383e1e88e2a1c5a4bd09f40cb'),
    output_normalization_version text NOT NULL
        CHECK (output_normalization_version='finite-vector-no-client-normalization-v1'),
    credential_ref text NOT NULL CHECK (credential_ref<>''),
    support_state text NOT NULL DEFAULT 'supported'
        CHECK (support_state IN ('supported','revoked')),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CHECK (revision_sha256=encode(public.digest(canonical_revision_payload,'sha256'),'hex')),
    CONSTRAINT knowledge_embedding_revisions_payload_matches_columns CHECK (
        canonical_revision_payload = convert_to(
            '{"schema_version":'||to_json(schema_version)::text||
            ',"provider_protocol_version":'||to_json(provider_protocol_version)::text||
            ',"provider_model_identifier":'||to_json(provider_model_identifier)::text||
            ',"provider_model_revision_sha256":'||to_json(provider_model_revision_sha256)::text||
            ',"endpoint_config_sha256":'||to_json(endpoint_config_sha256)::text||
            ',"endpoint_identity":'||to_json(endpoint_identity)::text||
            ',"dimension":'||to_json(dimension)::text||
            ',"request_config_sha256":'||to_json(request_config_sha256)::text||
            ',"output_normalization_version":'||to_json(output_normalization_version)::text||'}',
            'UTF8')
    )
);

CREATE TABLE knowledge_rerank_revisions (
    revision_sha256 text PRIMARY KEY CHECK (revision_sha256 ~ '^[0-9a-f]{64}$'),
    canonical_revision_payload bytea NOT NULL,
    schema_version smallint NOT NULL CHECK (schema_version=2),
    provider_protocol_version text NOT NULL CHECK (provider_protocol_version='indexed-json-v1'),
    provider_model_identifier text NOT NULL CHECK (octet_length(provider_model_identifier) BETWEEN 1 AND 256),
    provider_model_revision_sha256 text NOT NULL
        CHECK (provider_model_revision_sha256 ~ '^[0-9a-f]{64}$'),
    config_revision_sha256 text NOT NULL CHECK (config_revision_sha256 ~ '^[0-9a-f]{64}$'),
    endpoint_identity text NOT NULL CHECK (endpoint_identity<>''),
    request_config_sha256 text NOT NULL CHECK (
        request_config_sha256='21c0ee51fa4df1a5e436fab5e5df6ab851c2f6ebfcf115c86d77b40f40bf02f1'),
    score_normalization_version text NOT NULL
        CHECK (score_normalization_version='unit-interval-millionths-v1'),
    credential_ref text NOT NULL CHECK (credential_ref<>''),
    support_state text NOT NULL DEFAULT 'supported'
        CHECK (support_state IN ('supported','revoked')),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE(provider_model_revision_sha256,config_revision_sha256),
    CHECK (revision_sha256=encode(public.digest(canonical_revision_payload,'sha256'),'hex')),
    CONSTRAINT knowledge_rerank_revisions_payload_matches_columns CHECK (
        canonical_revision_payload=convert_to(
            '{"schema_version":'||to_json(schema_version)::text||
            ',"provider_protocol_version":'||to_json(provider_protocol_version)::text||
            ',"provider_model_identifier":'||to_json(provider_model_identifier)::text||
            ',"provider_model_revision_sha256":'||to_json(provider_model_revision_sha256)::text||
            ',"config_revision_sha256":'||to_json(config_revision_sha256)::text||
            ',"endpoint_identity":'||to_json(endpoint_identity)::text||
            ',"request_config_sha256":'||to_json(request_config_sha256)::text||
            ',"score_normalization_version":'||to_json(score_normalization_version)::text||'}',
            'UTF8'))
);

-- Knowledge owns the v2 policy artifact and support decision. Request DTOs may
-- reference a supported identity, but cannot define their own trusted quotas.

CREATE TABLE knowledge_retrieval_policies (
    policy_sha256 text PRIMARY KEY CHECK (policy_sha256 ~ '^[0-9a-f]{64}$'),
    canonical_policy_payload bytea NOT NULL,
    embedding_revision_sha256 text NOT NULL REFERENCES knowledge_embedding_revisions(revision_sha256),
    rerank_revision_sha256 text NOT NULL REFERENCES knowledge_rerank_revisions(revision_sha256),
    contract_version text NOT NULL CHECK (contract_version='knowledge-evidence-v2'),
    max_hits bigint NOT NULL CHECK (max_hits BETWEEN 1 AND 1000000),
    max_chunk_bytes bigint NOT NULL CHECK (max_chunk_bytes BETWEEN 1 AND 1073741824),
    max_total_bytes bigint NOT NULL CHECK (max_total_bytes BETWEEN 1 AND 1099511627776),
    support_state text NOT NULL DEFAULT 'supported'
        CHECK (support_state IN ('supported','revoked')),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CHECK (policy_sha256=encode(public.digest(canonical_policy_payload,'sha256'),'hex'))
);

CREATE TABLE knowledge_product_version_embedding_bindings (
    product_version_id uuid PRIMARY KEY REFERENCES knowledge_product_versions(id) ON DELETE CASCADE,
    embedding_revision_sha256 text NOT NULL REFERENCES knowledge_embedding_revisions(revision_sha256),
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE knowledge_chunk_keyword_indexes (
    chunk_id uuid NOT NULL REFERENCES knowledge_chunks(id) ON DELETE CASCADE,
    tokenizer text NOT NULL CHECK (tokenizer='latin-numeric-cjk-bigram'),
    tokenizer_version text NOT NULL CHECK (tokenizer_version='v1'),
    indexed_content text NOT NULL,
    indexed_content_sha256 text NOT NULL CHECK (indexed_content_sha256 ~ '^[0-9a-f]{64}$'),
    tsv tsvector NOT NULL,
    PRIMARY KEY (chunk_id,tokenizer,tokenizer_version),
    CHECK (indexed_content_sha256=encode(public.digest(convert_to(indexed_content,'UTF8'),'sha256'),'hex'))
);
CREATE INDEX knowledge_chunk_keyword_indexes_tsv_idx
    ON knowledge_chunk_keyword_indexes USING gin(tsv);
CREATE INDEX knowledge_chunk_keyword_indexes_tokenizer_idx
    ON knowledge_chunk_keyword_indexes(tokenizer,tokenizer_version);

CREATE TABLE knowledge_product_version_vector_generations (
    product_version_id uuid PRIMARY KEY REFERENCES knowledge_product_versions(id) ON DELETE CASCADE,
    embedding_revision_sha256 text NOT NULL REFERENCES knowledge_embedding_revisions(revision_sha256),
    source_snapshot_sha256 text NOT NULL CHECK (source_snapshot_sha256 ~ '^[0-9a-f]{64}$'),
    chunk_count bigint NOT NULL CHECK (chunk_count BETWEEN 0 AND 16384),
    completed_at timestamptz NOT NULL DEFAULT clock_timestamp()
);

CREATE TABLE knowledge_product_version_keyword_generations (
    product_version_id uuid PRIMARY KEY REFERENCES knowledge_product_versions(id) ON DELETE CASCADE,
    embedding_revision_sha256 text NOT NULL REFERENCES knowledge_embedding_revisions(revision_sha256),
    source_snapshot_sha256 text NOT NULL CHECK (source_snapshot_sha256 ~ '^[0-9a-f]{64}$'),
    chunk_count bigint NOT NULL CHECK (chunk_count BETWEEN 0 AND 16384),
    completed_at timestamptz NOT NULL DEFAULT clock_timestamp()
);

CREATE TABLE knowledge_semantic_index_intents (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    product_version_id uuid NOT NULL REFERENCES knowledge_product_versions(id) ON DELETE CASCADE,
    embedding_revision_sha256 text NOT NULL REFERENCES knowledge_embedding_revisions(revision_sha256),
    source_snapshot_sha256 text NOT NULL CHECK (source_snapshot_sha256 ~ '^[0-9a-f]{64}$'),
    target_revision bigint NOT NULL CHECK (target_revision>0),
    status text NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending','completed','terminal','superseded')),
    generation_marker_sha256 text CHECK (generation_marker_sha256 ~ '^[0-9a-f]{64}$'),
    last_error_code text CHECK (last_error_code IS NULL OR octet_length(last_error_code) BETWEEN 1 AND 96),
    last_error_detail text CHECK (last_error_detail IS NULL OR octet_length(last_error_detail) BETWEEN 1 AND 512),
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    completed_at timestamptz,
    UNIQUE(product_version_id,target_revision),
    CHECK ((status='completed')=(generation_marker_sha256 IS NOT NULL AND completed_at IS NOT NULL)),
    CHECK (status<>'terminal' OR last_error_code IS NOT NULL)
);
CREATE UNIQUE INDEX knowledge_semantic_index_intents_pending_uidx
    ON knowledge_semantic_index_intents(product_version_id) WHERE status='pending';
CREATE INDEX knowledge_semantic_index_intents_status_idx
    ON knowledge_semantic_index_intents(status,created_at,id);

CREATE TABLE knowledge_chunk_vector_indexes (
    chunk_id uuid NOT NULL,
    product_version_id uuid NOT NULL,
    embedding_revision_sha256 text NOT NULL REFERENCES knowledge_embedding_revisions(revision_sha256),
    source_snapshot_sha256 text NOT NULL CHECK (source_snapshot_sha256 ~ '^[0-9a-f]{64}$'),
    indexed_content_sha256 text NOT NULL CHECK (indexed_content_sha256 ~ '^[0-9a-f]{64}$'),
    embedding vector(1024) NOT NULL,
    PRIMARY KEY (chunk_id,embedding_revision_sha256),
    FOREIGN KEY (chunk_id,product_version_id)
        REFERENCES knowledge_chunks(id,product_version_id) ON DELETE CASCADE
);
CREATE INDEX knowledge_chunk_vector_indexes_hnsw_idx
    ON knowledge_chunk_vector_indexes USING hnsw(embedding vector_cosine_ops);
CREATE INDEX knowledge_chunk_vector_indexes_revision_idx
    ON knowledge_chunk_vector_indexes(embedding_revision_sha256);
CREATE INDEX knowledge_chunk_vector_indexes_owner_revision_idx
    ON knowledge_chunk_vector_indexes(product_version_id,embedding_revision_sha256);

CREATE TABLE knowledge_graph_nodes (
    product_version_id uuid NOT NULL,
    document_id uuid NOT NULL,
    generation integer NOT NULL DEFAULT 1,
    name text NOT NULL,
    chunk_ids uuid[] NOT NULL DEFAULT '{}',
    PRIMARY KEY (product_version_id, document_id, name, generation),
    CONSTRAINT knowledge_graph_nodes_source_fk FOREIGN KEY (document_id, product_version_id)
        REFERENCES knowledge_documents (id, product_version_id) ON DELETE CASCADE
);
CREATE TABLE knowledge_graph_relations (
    product_version_id uuid NOT NULL,
    document_id uuid NOT NULL,
    generation integer NOT NULL DEFAULT 1,
    source_node_name text NOT NULL,
    target_node_name text NOT NULL,
    relation_type text NOT NULL,
    PRIMARY KEY (product_version_id, document_id, source_node_name, target_node_name, relation_type, generation),
    CONSTRAINT knowledge_graph_relations_source_fk FOREIGN KEY (document_id, product_version_id)
        REFERENCES knowledge_documents (id, product_version_id) ON DELETE CASCADE
);

CREATE TABLE knowledge_wiki_folders (
    id uuid PRIMARY KEY,
    product_version_id uuid NOT NULL REFERENCES knowledge_product_versions (id),
    parent_folder_id uuid,
    name text NOT NULL,
    path text NOT NULL,
    depth integer NOT NULL DEFAULT 0,
    sort_order integer NOT NULL DEFAULT 0,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    deleted_at timestamptz,
    UNIQUE (id, product_version_id),
    CONSTRAINT knowledge_wiki_folders_parent_fk
        FOREIGN KEY (parent_folder_id, product_version_id)
        REFERENCES knowledge_wiki_folders (id, product_version_id)
        ON DELETE SET NULL (parent_folder_id) DEFERRABLE INITIALLY DEFERRED
);

CREATE TABLE knowledge_wiki_pages (
    id uuid PRIMARY KEY,
    product_version_id uuid NOT NULL REFERENCES knowledge_product_versions (id),
    slug text NOT NULL,
    title text NOT NULL,
    page_type text NOT NULL DEFAULT 'summary',
    status text NOT NULL CHECK (status IN ('draft', 'published', 'archived')),
    content text NOT NULL DEFAULT '',
    summary text NOT NULL DEFAULT '',
    aliases jsonb NOT NULL DEFAULT '[]'::jsonb,
    folder_id uuid,
    category_path jsonb NOT NULL DEFAULT '[]'::jsonb,
    source_refs jsonb NOT NULL DEFAULT '[]'::jsonb,
    chunk_refs jsonb NOT NULL DEFAULT '[]'::jsonb,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    deleted_at timestamptz,
    UNIQUE (product_version_id, slug),
    CONSTRAINT knowledge_wiki_pages_folder_fk FOREIGN KEY (folder_id, product_version_id)
        REFERENCES knowledge_wiki_folders (id, product_version_id)
        ON DELETE SET NULL (folder_id) DEFERRABLE INITIALLY DEFERRED
);

-- Document attempts are isolated index generations. Readers select only the
-- published generation; workers publish with an expected-attempt row lock.
CREATE INDEX knowledge_chunks_document_generation_idx ON knowledge_chunks(document_id,generation);

CREATE TABLE knowledge_job_outbox (
  document_id uuid NOT NULL,
  product_version_id uuid NOT NULL,
  attempt integer NOT NULL,
  job_key text NOT NULL,
  payload jsonb NOT NULL,
  is_derived boolean NOT NULL DEFAULT false,
  state text NOT NULL DEFAULT 'pending' CHECK(state IN ('pending','published','complete','failed')),
  deliveries integer NOT NULL DEFAULT 0,
  revision bigint NOT NULL DEFAULT 1,
  next_delivery_at timestamptz NOT NULL DEFAULT now(),
  last_error text,
  PRIMARY KEY(document_id,attempt,job_key),
    CONSTRAINT knowledge_job_outbox_source_fk FOREIGN KEY (document_id, product_version_id)
        REFERENCES knowledge_documents (id, product_version_id)
);
CREATE INDEX knowledge_job_outbox_delivery_idx ON knowledge_job_outbox(next_delivery_at)
WHERE state IN ('pending','published');
CREATE TABLE knowledge_job_receipts (
  document_id uuid NOT NULL REFERENCES knowledge_documents(id),
  attempt integer NOT NULL,
  job_key text NOT NULL,
  completed_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY(document_id,attempt,job_key)
);

-- Reverse lookups used by source cleanup, tags and folder navigation.
CREATE INDEX knowledge_documents_version_idx ON knowledge_documents (product_version_id);
CREATE INDEX knowledge_document_tags_tag_idx ON knowledge_document_tags (tag_id, document_id);
CREATE INDEX knowledge_chunk_embeddings_document_idx ON knowledge_chunk_embeddings (document_id);
CREATE INDEX knowledge_graph_nodes_document_idx ON knowledge_graph_nodes (document_id, generation);
CREATE INDEX knowledge_graph_relations_document_idx ON knowledge_graph_relations (document_id, generation);
CREATE INDEX knowledge_image_chunk_links_artifact_idx ON knowledge_image_chunk_links (image_artifact_revision_id);
CREATE INDEX knowledge_image_artifact_revisions_source_idx ON knowledge_image_artifact_revisions (document_id, product_version_id);
CREATE INDEX knowledge_image_artifact_revisions_object_idx ON knowledge_image_artifact_revisions (object_ref);
CREATE INDEX knowledge_wiki_folders_version_parent_idx ON knowledge_wiki_folders (product_version_id, parent_folder_id);
CREATE INDEX knowledge_wiki_pages_folder_idx ON knowledge_wiki_pages (folder_id, product_version_id);
