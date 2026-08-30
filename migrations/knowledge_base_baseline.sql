-- KnowledgeBrain final V1 fresh baseline: knowledge-base-owned catalog.
-- This slice preserves Workspace/Product/ProductVersion/Document semantics.
-- It is create-only: no compatibility objects, backfill, repair, or upgrade DDL.

CREATE EXTENSION IF NOT EXISTS vector;
CREATE EXTENSION IF NOT EXISTS pgcrypto;

CREATE TABLE workspaces (
    id uuid PRIMARY KEY,
    name text NOT NULL,
    slug text NOT NULL UNIQUE,
    kind text NOT NULL DEFAULT 'product_line' CHECK (kind IN ('product_line', 'company')),
    retrieval_config jsonb NOT NULL DEFAULT '{"vector_threshold":0.15,"keyword_threshold":0.3,"embedding_top_k":50}'::jsonb,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX workspaces_one_company ON workspaces (kind) WHERE kind = 'company';

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
    ON product_versions (product_id, label) WHERE deleted_at IS NULL;

CREATE TABLE documents (
    id uuid PRIMARY KEY,
    product_version_id uuid NOT NULL REFERENCES product_versions (id),
    type text NOT NULL DEFAULT 'file' CHECK (type IN ('file', 'url', 'passage', 'manual')),
    title text NOT NULL,
    parse_status text NOT NULL CHECK (parse_status IN (
        'pending', 'processing', 'finalizing', 'completed', 'failed', 'cancelled', 'deleting'
    )),
    pending_subtasks_count integer NOT NULL DEFAULT 0,
    summary_status text NOT NULL DEFAULT 'none'
        CHECK (summary_status IN ('none', 'pending', 'processing', 'completed', 'failed')),
    enable_status text NOT NULL DEFAULT 'disabled' CHECK (enable_status IN ('disabled', 'enabled')),
    index_ready boolean NOT NULL DEFAULT false,
    description text NOT NULL DEFAULT '',
    attempt integer NOT NULL DEFAULT 1,
    file_name text NOT NULL,
    file_size bigint NOT NULL CHECK (file_size >= 0),
    file_hash text NOT NULL CHECK (file_hash ~ '^[0-9a-f]{64}$'),
    object_ref text NOT NULL CHECK (object_ref = 'objects/' || file_hash),
    source_passages jsonb,
    process_overrides jsonb,
    error_message text,
    processed_at timestamptz,
    updated_at timestamptz NOT NULL DEFAULT now(),
    deleted_at timestamptz,
    UNIQUE (id,product_version_id)
);
CREATE UNIQUE INDEX documents_live_file_uidx
    ON documents (product_version_id, file_name, file_size, file_hash) WHERE deleted_at IS NULL;

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

CREATE TABLE models (
    id text PRIMARY KEY,
    kind text NOT NULL CHECK (kind IN ('embedding', 'chat', 'vlm', 'asr')),
    endpoint text,
    api_key_enc text,
    dimension integer,
    extra jsonb NOT NULL DEFAULT '{}'::jsonb
);
CREATE TABLE api_keys (
    id uuid PRIMARY KEY,
    name text NOT NULL,
    key_hash text NOT NULL UNIQUE,
    prefix text NOT NULL,
    scope_type text NOT NULL CHECK (scope_type IN ('workspace', 'product')),
    scope_id uuid NOT NULL,
    scopes text[] NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX api_keys_scope_idx ON api_keys (scope_type, scope_id);

CREATE TABLE chunks (
    id uuid PRIMARY KEY,
    product_version_id uuid NOT NULL REFERENCES product_versions (id),
    document_id uuid NOT NULL REFERENCES documents (id) ON DELETE CASCADE,
    chunk_type text NOT NULL,
    content text NOT NULL,
    context_header text NOT NULL DEFAULT '',
    start_at integer NOT NULL DEFAULT 0,
    end_at integer NOT NULL DEFAULT 0,
    parent_chunk_id uuid,
    generated_questions jsonb NOT NULL DEFAULT '[]'::jsonb,
    UNIQUE (id,product_version_id),
    UNIQUE (id,product_version_id,document_id)
);
CREATE INDEX chunks_document_idx ON chunks (document_id);
CREATE INDEX chunks_version_idx ON chunks (product_version_id);

-- Phase 0 freezes only the V3 media storage identity needed by downstream
-- immutable foreign keys. Phase 4 owns publication and retrieval behavior.
CREATE TABLE knowledge_image_artifact_revisions (
    id uuid PRIMARY KEY,
    product_version_id uuid NOT NULL REFERENCES product_versions(id),
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
    FOREIGN KEY(document_id,product_version_id) REFERENCES documents(id,product_version_id),
    CHECK (object_ref='objects/'||content_sha256),
    CHECK (artifact_sha256=encode(public.digest(canonical_payload,'sha256'),'hex')),
    CHECK (bounding_region IS NULL OR jsonb_typeof(bounding_region)='object')
);

CREATE TABLE knowledge_image_ocr_chunk_artifact_mappings (
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
      REFERENCES chunks(id,product_version_id,document_id),
    FOREIGN KEY(image_artifact_revision_id,product_version_id,document_id)
      REFERENCES knowledge_image_artifact_revisions(id,product_version_id,document_id),
    FOREIGN KEY(image_artifact_revision_id,object_ref,content_sha256,media_type,object_state)
      REFERENCES knowledge_image_artifact_revisions(id,object_ref,content_sha256,media_type,object_state)
);

CREATE FUNCTION kb_knowledge_validate_image_ocr_mapping()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM chunks
        WHERE id=NEW.chunk_id AND product_version_id=NEW.product_version_id
          AND document_id=NEW.document_id AND chunk_type='image_ocr'
    ) THEN
        RAISE EXCEPTION 'KNOWLEDGE_IMAGE_OCR_MAPPING_SOURCE_INVALID' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END
$$;
CREATE TRIGGER knowledge_image_ocr_mapping_source_valid
BEFORE INSERT ON knowledge_image_ocr_chunk_artifact_mappings
FOR EACH ROW EXECUTE FUNCTION kb_knowledge_validate_image_ocr_mapping();
CREATE FUNCTION kb_knowledge_reject_image_media_mutation()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
BEGIN
    RAISE EXCEPTION 'KNOWLEDGE_IMAGE_MEDIA_IMMUTABLE' USING ERRCODE='42501';
    RETURN NULL;
END
$$;
CREATE TRIGGER knowledge_image_artifact_revisions_immutable
BEFORE UPDATE OR DELETE ON knowledge_image_artifact_revisions
FOR EACH ROW EXECUTE FUNCTION kb_knowledge_reject_image_media_mutation();
CREATE TRIGGER knowledge_image_artifact_revisions_truncate_guard
BEFORE TRUNCATE ON knowledge_image_artifact_revisions
FOR EACH STATEMENT EXECUTE FUNCTION kb_knowledge_reject_image_media_mutation();
CREATE TRIGGER knowledge_image_ocr_mappings_immutable
BEFORE UPDATE OR DELETE ON knowledge_image_ocr_chunk_artifact_mappings
FOR EACH ROW EXECUTE FUNCTION kb_knowledge_reject_image_media_mutation();
CREATE TRIGGER knowledge_image_ocr_mappings_truncate_guard
BEFORE TRUNCATE ON knowledge_image_ocr_chunk_artifact_mappings
FOR EACH STATEMENT EXECUTE FUNCTION kb_knowledge_reject_image_media_mutation();

CREATE TABLE chunk_embeddings (
    chunk_id uuid PRIMARY KEY REFERENCES chunks (id) ON DELETE CASCADE,
    product_version_id uuid NOT NULL,
    document_id uuid NOT NULL,
    embedding vector(1024),
    tsv tsvector,
    content text NOT NULL
);
CREATE INDEX chunk_embeddings_hnsw ON chunk_embeddings USING hnsw (embedding vector_cosine_ops);
CREATE INDEX chunk_embeddings_tsv ON chunk_embeddings USING gin (tsv);
CREATE INDEX chunk_embeddings_version_idx ON chunk_embeddings (product_version_id);

-- Knowledge-base-owned persistence attestation for the frozen DTO returned by
-- KnowledgeRetrievalPort. Bidding passes only the port snapshot; this function
-- alone may compare that snapshot with live knowledge-owned relations.
CREATE TABLE knowledge_matching_scope_attestations (
    id uuid PRIMARY KEY,
    schema_version smallint NOT NULL CHECK (schema_version=1),
    canonical_payload bytea NOT NULL,
    content_sha256 text NOT NULL CHECK (content_sha256 ~ '^[0-9a-f]{64}$'),
    created_at timestamptz NOT NULL DEFAULT now(),
    CHECK (content_sha256=encode(public.digest(canonical_payload,'sha256'),'hex'))
);

CREATE FUNCTION kb_knowledge_attest_matching_scope_v1(p_scope jsonb)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE
    p_products jsonb := p_scope->'products';
    p_hits jsonb := p_scope->'frozen_hits';
    p_version_selections jsonb := p_scope->'version_selections';
    p_workspace_kinds text[] := ARRAY(
        SELECT jsonb_array_elements_text(p_scope->'workspace_kinds') ORDER BY 1);
    p_product_line_versions uuid[];
    p_company_versions uuid[];
    attestation_id uuid := gen_random_uuid();
    canonical_payload bytea;
    content_sha256 text;
BEGIN
    IF (SELECT array_agg(key ORDER BY key) FROM jsonb_object_keys(p_scope) key)
         IS DISTINCT FROM ARRAY[
             'frozen_hits','products','schema_version','version_selections','workspace_kinds']::text[]
       OR p_scope->>'schema_version'<>'1'
       OR jsonb_typeof(p_products) IS DISTINCT FROM 'array'
       OR jsonb_typeof(p_hits) IS DISTINCT FROM 'array'
       OR jsonb_typeof(p_version_selections) IS DISTINCT FROM 'object'
       OR (SELECT array_agg(key ORDER BY key) FROM jsonb_object_keys(p_version_selections) key)
          IS DISTINCT FROM ARRAY['company','product_line']::text[]
       OR jsonb_typeof(p_version_selections->'product_line') IS DISTINCT FROM 'array'
       OR jsonb_typeof(p_version_selections->'company') IS DISTINCT FROM 'array'
       OR jsonb_typeof(p_scope->'workspace_kinds') IS DISTINCT FROM 'array'
       OR EXISTS (
           SELECT 1 FROM unnest(p_workspace_kinds) kind
            WHERE kind NOT IN ('product_line', 'company'))
       OR cardinality(p_workspace_kinds)
          <> (SELECT count(DISTINCT kind) FROM unnest(p_workspace_kinds) kind) THEN
        RAISE EXCEPTION 'KNOWLEDGE_MATCHING_SCOPE_V1_INVALID' USING ERRCODE='23514';
    END IF;

    IF EXISTS (
           SELECT 1 FROM jsonb_array_elements(p_version_selections->'product_line') selection
            WHERE jsonb_typeof(selection) IS DISTINCT FROM 'string'
               OR selection#>>'{}' !~* '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$')
       OR EXISTS (
           SELECT 1 FROM jsonb_array_elements(p_version_selections->'company') selection
            WHERE jsonb_typeof(selection) IS DISTINCT FROM 'string'
               OR selection#>>'{}' !~* '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$') THEN
        RAISE EXCEPTION 'KNOWLEDGE_MATCHING_SCOPE_V1_INVALID' USING ERRCODE='23514';
    END IF;
    SELECT COALESCE(array_agg(selection::uuid ORDER BY ordinal),'{}'::uuid[])
      INTO p_product_line_versions
      FROM jsonb_array_elements_text(p_version_selections->'product_line')
           WITH ORDINALITY selected(selection,ordinal);
    SELECT COALESCE(array_agg(selection::uuid ORDER BY ordinal),'{}'::uuid[])
      INTO p_company_versions
      FROM jsonb_array_elements_text(p_version_selections->'company')
           WITH ORDINALITY selected(selection,ordinal);
    IF p_product_line_versions IS DISTINCT FROM ARRAY(
           SELECT DISTINCT version_id FROM unnest(p_product_line_versions) version_id ORDER BY version_id)
       OR p_company_versions IS DISTINCT FROM ARRAY(
           SELECT DISTINCT version_id FROM unnest(p_company_versions) version_id ORDER BY version_id)
       OR (NOT ('product_line'=ANY(p_workspace_kinds)) AND cardinality(p_product_line_versions)>0)
       OR (NOT ('company'=ANY(p_workspace_kinds)) AND cardinality(p_company_versions)>0) THEN
        RAISE EXCEPTION 'KNOWLEDGE_MATCHING_SCOPE_V1_INVALID' USING ERRCODE='23514';
    END IF;

    IF EXISTS (
        SELECT 1
          FROM (
              SELECT 'product_line'::text AS kind, version_id
                FROM unnest(p_product_line_versions) version_id
              UNION ALL
              SELECT 'company'::text AS kind, version_id
                FROM unnest(p_company_versions) version_id
          ) selection
          LEFT JOIN product_versions version_value ON version_value.id=selection.version_id
          LEFT JOIN products product ON product.id=version_value.product_id
          LEFT JOIN workspaces workspace_value ON workspace_value.id=product.workspace_id
         WHERE product.id IS NULL
            OR workspace_value.kind IS DISTINCT FROM selection.kind
            OR (selection.kind='product_line' AND product.kind IS DISTINCT FROM 'product')
            OR (selection.kind='company' AND product.kind IS DISTINCT FROM 'library')
            OR version_value.status IS DISTINCT FROM 'active'
            OR version_value.deleted_at IS NOT NULL
            OR product.current_version_id IS DISTINCT FROM version_value.id) THEN
        RAISE EXCEPTION 'KNOWLEDGE_MATCHING_SCOPE_V1_MISMATCH' USING ERRCODE='23514';
    END IF;

    IF EXISTS (
        SELECT 1
          FROM jsonb_array_elements(p_products) artifact
          LEFT JOIN product_versions version_value
            ON version_value.id=(artifact->>'product_version_id')::uuid
          LEFT JOIN products product ON product.id=version_value.product_id
          LEFT JOIN workspaces workspace_value ON workspace_value.id=product.workspace_id
         WHERE product.id IS NULL
            OR (artifact->>'product_id')::uuid IS DISTINCT FROM product.id
            OR artifact->>'workspace_kind' IS DISTINCT FROM workspace_value.kind
            OR NOT (workspace_value.kind=ANY(p_workspace_kinds))
            OR (workspace_value.kind='product_line' AND product.kind IS DISTINCT FROM 'product')
            OR (workspace_value.kind='company' AND product.kind IS DISTINCT FROM 'library')
            OR (workspace_value.kind='product_line' AND cardinality(p_product_line_versions)>0
                AND NOT (version_value.id=ANY(p_product_line_versions)))
            OR (workspace_value.kind='company' AND cardinality(p_company_versions)>0
                AND NOT (version_value.id=ANY(p_company_versions)))
            OR version_value.status IS DISTINCT FROM 'active'
            OR version_value.deleted_at IS NOT NULL
            OR product.current_version_id IS DISTINCT FROM version_value.id
            OR artifact->>'frozen_display_name' IS DISTINCT FROM version_value.id::text
            OR artifact->>'identity_sha256' IS DISTINCT FROM encode(public.digest(convert_to(
                'ProductVersionEvidenceV1:'||product.id::text||':'||version_value.id::text||':'
                ||workspace_value.kind,'UTF8'),'sha256'),'hex'))
       OR EXISTS (
          SELECT 1 FROM jsonb_array_elements(p_products) artifact
           GROUP BY artifact->>'product_version_id' HAVING count(*)<>1)
       OR EXISTS (
        SELECT 1
          FROM workspaces workspace_value
          JOIN products product ON product.workspace_id=workspace_value.id
          JOIN product_versions version_value
            ON version_value.product_id=product.id
           AND product.current_version_id=version_value.id
         WHERE workspace_value.kind=ANY(p_workspace_kinds)
           AND version_value.status='active'
           AND version_value.deleted_at IS NULL
           AND ((workspace_value.kind='product_line' AND product.kind='product')
             OR (workspace_value.kind='company' AND product.kind='library'))
           AND ((workspace_value.kind='product_line'
                 AND (cardinality(p_product_line_versions)=0
                      OR version_value.id=ANY(p_product_line_versions)))
             OR (workspace_value.kind='company'
                 AND (cardinality(p_company_versions)=0
                      OR version_value.id=ANY(p_company_versions))))
           AND NOT EXISTS (
               SELECT 1 FROM jsonb_array_elements(p_products) artifact
                WHERE (artifact->>'product_id')::uuid=product.id
                  AND (artifact->>'product_version_id')::uuid=version_value.id
                  AND artifact->>'workspace_kind'=workspace_value.kind)) THEN
        RAISE EXCEPTION 'KNOWLEDGE_MATCHING_SCOPE_V1_MISMATCH' USING ERRCODE='23514';
    END IF;

    IF EXISTS (
        SELECT 1
          FROM jsonb_array_elements(p_hits) hit
          LEFT JOIN LATERAL (
              SELECT artifact
                FROM jsonb_array_elements(p_products) artifact
               WHERE artifact->>'id'=hit->>'product_version_artifact_id'
          ) artifact_value ON true
          LEFT JOIN documents document_value ON document_value.id=(hit->>'document_id')::uuid
          LEFT JOIN chunks chunk_value ON chunk_value.id=(hit->>'source_chunk_id')::uuid
         WHERE artifact_value.artifact IS NULL
            OR document_value.product_version_id IS DISTINCT FROM
               (artifact_value.artifact->>'product_version_id')::uuid
            OR chunk_value.product_version_id IS DISTINCT FROM
               (artifact_value.artifact->>'product_version_id')::uuid
            OR chunk_value.document_id IS DISTINCT FROM document_value.id
            OR document_value.deleted_at IS NOT NULL
            OR document_value.enable_status IS DISTINCT FROM 'enabled'
            OR NOT document_value.index_ready
            OR hit->>'frozen_document_display_name' IS DISTINCT FROM document_value.file_name
            OR hit->>'chunk_utf8' IS DISTINCT FROM chunk_value.content
            OR (hit->>'chunk_byte_length')::bigint IS DISTINCT FROM
               octet_length(convert_to(chunk_value.content,'UTF8'))
            OR hit->>'chunk_sha256' IS DISTINCT FROM encode(public.digest(
               convert_to(chunk_value.content,'UTF8'),'sha256'),'hex')
            OR hit->>'retrieval_contract_version' IS DISTINCT FROM 'knowledge-evidence-v1') THEN
        RAISE EXCEPTION 'KNOWLEDGE_MATCHING_HIT_V1_MISMATCH' USING ERRCODE='23514';
    END IF;
    canonical_payload := convert_to(p_scope::text,'UTF8');
    content_sha256 := encode(public.digest(canonical_payload,'sha256'),'hex');
    INSERT INTO knowledge_matching_scope_attestations(
        id,schema_version,canonical_payload,content_sha256)
    VALUES(attestation_id,1,canonical_payload,content_sha256);
    RETURN jsonb_build_object('id',attestation_id,'content_sha256',content_sha256);
END
$$;

CREATE FUNCTION kb_knowledge_verify_matching_scope_v1(
    p_attestation_id uuid,
    p_content_sha256 text,
    p_scope jsonb
)
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
BEGIN
    PERFORM 1
      FROM knowledge_matching_scope_attestations attestation
     WHERE attestation.id=p_attestation_id
       AND attestation.content_sha256=p_content_sha256
       AND attestation.canonical_payload=convert_to(p_scope::text,'UTF8');
    IF NOT FOUND THEN
        RAISE EXCEPTION 'KNOWLEDGE_MATCHING_ATTESTATION_V1_MISMATCH' USING ERRCODE='23514';
    END IF;
END
$$;
REVOKE ALL ON FUNCTION kb_knowledge_attest_matching_scope_v1(jsonb) FROM PUBLIC;
REVOKE ALL ON FUNCTION kb_knowledge_verify_matching_scope_v1(uuid,text,jsonb) FROM PUBLIC;

CREATE TABLE knowledge_matching_scope_attestations_v2 (
    id uuid PRIMARY KEY,
    schema_version smallint NOT NULL CHECK (schema_version=2),
    canonical_payload bytea NOT NULL,
    content_sha256 text NOT NULL CHECK (content_sha256 ~ '^[0-9a-f]{64}$'),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CHECK (content_sha256=encode(public.digest(canonical_payload,'sha256'),'hex'))
);
REVOKE ALL ON TABLE knowledge_matching_scope_attestations_v2 FROM PUBLIC;

-- Embedding behavior is an immutable canonical artifact. Credentials are
-- operational registry metadata and are intentionally outside the digest.
CREATE FUNCTION kb_knowledge_valid_provider_model_identifier_v2(value text)
RETURNS boolean
LANGUAGE plpgsql
IMMUTABLE
STRICT
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE
    revision text;
    revision_date date;
BEGIN
    IF octet_length(value)>256
       OR value !~ '^[A-Za-z0-9._/-]+@([0-9]{4}-[0-9]{2}-[0-9]{2}|sha256:[0-9a-f]{64})$' THEN
        RETURN false;
    END IF;
    revision := split_part(value,'@',2);
    IF revision LIKE 'sha256:%' THEN
        RETURN true;
    END IF;
    BEGIN
        revision_date := revision::date;
    EXCEPTION WHEN others THEN
        RETURN false;
    END;
    RETURN to_char(revision_date,'YYYY-MM-DD')=revision;
END
$$;
REVOKE ALL ON FUNCTION kb_knowledge_valid_provider_model_identifier_v2(text) FROM PUBLIC;

CREATE FUNCTION kb_knowledge_valid_endpoint_identity_v2(value text)
RETURNS boolean
LANGUAGE plpgsql
IMMUTABLE
STRICT
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE
    scheme text;
    remainder text;
    authority text;
    host text;
    path text;
    port_text text;
    port_number integer;
BEGIN
    IF value ~ '[[:cntrl:][:space:]]' THEN
        RETURN false;
    END IF;
    IF value LIKE 'https://%' THEN
        scheme := 'https';
        remainder := substr(value,9);
    ELSE
        RETURN false;
    END IF;
    authority := split_part(remainder,'/',1);
    path := CASE WHEN strpos(remainder,'/')=0 THEN ''
                 ELSE substr(remainder,strpos(remainder,'/')+1) END;
    IF authority !~ '^[^:]+(:[^:]*)?$' THEN
        RETURN false;
    END IF;
    host := split_part(authority,':',1);
    IF host !~ '^[a-z0-9]([a-z0-9-]*[a-z0-9])?(\.[a-z0-9]([a-z0-9-]*[a-z0-9])?)*$' THEN
        RETURN false;
    END IF;
    IF strpos(remainder,'/')>0
       AND (path !~ '^[A-Za-z0-9._~-]+(/[A-Za-z0-9._~-]+)*$'
            OR path ~ '(^|/)\.{1,2}(/|$)') THEN
        RETURN false;
    END IF;
    IF strpos(authority,':')=0 THEN
        RETURN true;
    END IF;
    port_text := split_part(authority,':',2);
    IF port_text !~ '^[1-9][0-9]*$' OR length(port_text)>5 THEN
        RETURN false;
    END IF;
    port_number := port_text::integer;
    RETURN port_number<=65535
       AND NOT (scheme='https' AND port_number=443);
END
$$;
REVOKE ALL ON FUNCTION kb_knowledge_valid_endpoint_identity_v2(text) FROM PUBLIC;

CREATE TABLE embedding_revisions_v2 (
    revision_sha256 text PRIMARY KEY CHECK (revision_sha256 ~ '^[0-9a-f]{64}$'),
    canonical_revision_payload bytea NOT NULL,
    schema_version smallint NOT NULL CHECK (schema_version=2),
    provider_protocol_version text NOT NULL
        CHECK (provider_protocol_version='openai-compatible-embeddings-json-v1'),
    provider_model_identifier text NOT NULL
        CHECK (kb_knowledge_valid_provider_model_identifier_v2(provider_model_identifier)),
    provider_model_revision_sha256 text NOT NULL
        CHECK (provider_model_revision_sha256 ~ '^[0-9a-f]{64}$'),
    endpoint_config_sha256 text NOT NULL
        CHECK (endpoint_config_sha256 ~ '^[0-9a-f]{64}$'),
    endpoint_identity text NOT NULL
        CHECK (kb_knowledge_valid_endpoint_identity_v2(endpoint_identity)),
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
    CONSTRAINT embedding_revisions_v2_payload_matches_columns CHECK (
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
REVOKE ALL ON TABLE embedding_revisions_v2 FROM PUBLIC;

CREATE FUNCTION kb_knowledge_guard_embedding_revision_v2_update()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
BEGIN
    IF NEW.revision_sha256 IS DISTINCT FROM OLD.revision_sha256
       OR NEW.canonical_revision_payload IS DISTINCT FROM OLD.canonical_revision_payload
       OR NEW.schema_version IS DISTINCT FROM OLD.schema_version
       OR NEW.provider_protocol_version IS DISTINCT FROM OLD.provider_protocol_version
       OR NEW.provider_model_identifier IS DISTINCT FROM OLD.provider_model_identifier
       OR NEW.provider_model_revision_sha256 IS DISTINCT FROM OLD.provider_model_revision_sha256
       OR NEW.endpoint_config_sha256 IS DISTINCT FROM OLD.endpoint_config_sha256
       OR NEW.endpoint_identity IS DISTINCT FROM OLD.endpoint_identity
       OR NEW.dimension IS DISTINCT FROM OLD.dimension
       OR NEW.request_config_sha256 IS DISTINCT FROM OLD.request_config_sha256
       OR NEW.output_normalization_version IS DISTINCT FROM OLD.output_normalization_version
       OR NEW.credential_ref IS DISTINCT FROM OLD.credential_ref
       OR NEW.created_at IS DISTINCT FROM OLD.created_at
       OR (NEW.support_state IS NOT DISTINCT FROM OLD.support_state
           AND NEW.updated_at IS DISTINCT FROM OLD.updated_at)
       OR NOT (
           NEW.support_state IS NOT DISTINCT FROM OLD.support_state
           OR (OLD.support_state='supported' AND NEW.support_state='revoked')) THEN
        RAISE EXCEPTION 'EMBEDDING_REVISION_V2_IMMUTABLE' USING ERRCODE='23514';
    END IF;
    IF NEW.support_state IS DISTINCT FROM OLD.support_state THEN
        NEW.updated_at := clock_timestamp();
    ELSE
        NEW.updated_at := OLD.updated_at;
    END IF;
    RETURN NEW;
END
$$;
REVOKE ALL ON FUNCTION kb_knowledge_guard_embedding_revision_v2_update() FROM PUBLIC;
CREATE TRIGGER embedding_revisions_v2_guard_update
BEFORE UPDATE ON embedding_revisions_v2
FOR EACH ROW EXECUTE FUNCTION kb_knowledge_guard_embedding_revision_v2_update();

CREATE FUNCTION kb_knowledge_guard_embedding_revision_v2_removal()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
BEGIN
    RAISE EXCEPTION 'EMBEDDING_REVISION_V2_IMMUTABLE' USING ERRCODE='23514';
    RETURN NULL;
END
$$;
REVOKE ALL ON FUNCTION kb_knowledge_guard_embedding_revision_v2_removal() FROM PUBLIC;
CREATE TRIGGER embedding_revisions_v2_guard_removal
BEFORE DELETE OR TRUNCATE ON embedding_revisions_v2
FOR EACH STATEMENT EXECUTE FUNCTION kb_knowledge_guard_embedding_revision_v2_removal();

CREATE TABLE rerank_revisions_v2 (
    revision_sha256 text PRIMARY KEY CHECK (revision_sha256 ~ '^[0-9a-f]{64}$'),
    canonical_revision_payload bytea NOT NULL,
    schema_version smallint NOT NULL CHECK (schema_version=2),
    provider_protocol_version text NOT NULL CHECK (provider_protocol_version='indexed-json-v1'),
    provider_model_identifier text NOT NULL
        CHECK (kb_knowledge_valid_provider_model_identifier_v2(provider_model_identifier)),
    provider_model_revision_sha256 text NOT NULL
        CHECK (provider_model_revision_sha256 ~ '^[0-9a-f]{64}$'),
    config_revision_sha256 text NOT NULL CHECK (config_revision_sha256 ~ '^[0-9a-f]{64}$'),
    endpoint_identity text NOT NULL CHECK (kb_knowledge_valid_endpoint_identity_v2(endpoint_identity)),
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
    CONSTRAINT rerank_revisions_v2_payload_matches_columns CHECK (
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
REVOKE ALL ON TABLE rerank_revisions_v2 FROM PUBLIC;

CREATE FUNCTION kb_knowledge_guard_rerank_revision_v2_update()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public, pg_temp
AS $$
BEGIN
    IF NEW.revision_sha256 IS DISTINCT FROM OLD.revision_sha256
       OR NEW.canonical_revision_payload IS DISTINCT FROM OLD.canonical_revision_payload
       OR NEW.schema_version IS DISTINCT FROM OLD.schema_version
       OR NEW.provider_protocol_version IS DISTINCT FROM OLD.provider_protocol_version
       OR NEW.provider_model_identifier IS DISTINCT FROM OLD.provider_model_identifier
       OR NEW.provider_model_revision_sha256 IS DISTINCT FROM OLD.provider_model_revision_sha256
       OR NEW.config_revision_sha256 IS DISTINCT FROM OLD.config_revision_sha256
       OR NEW.endpoint_identity IS DISTINCT FROM OLD.endpoint_identity
       OR NEW.request_config_sha256 IS DISTINCT FROM OLD.request_config_sha256
       OR NEW.score_normalization_version IS DISTINCT FROM OLD.score_normalization_version
       OR NEW.credential_ref IS DISTINCT FROM OLD.credential_ref
       OR NEW.created_at IS DISTINCT FROM OLD.created_at
       OR (NEW.support_state IS NOT DISTINCT FROM OLD.support_state
           AND NEW.updated_at IS DISTINCT FROM OLD.updated_at)
       OR NOT (NEW.support_state IS NOT DISTINCT FROM OLD.support_state
            OR (OLD.support_state='supported' AND NEW.support_state='revoked')) THEN
        RAISE EXCEPTION 'RERANK_REVISION_V2_IMMUTABLE' USING ERRCODE='23514';
    END IF;
    IF NEW.support_state IS DISTINCT FROM OLD.support_state THEN
        NEW.updated_at := clock_timestamp();
    ELSE
        NEW.updated_at := OLD.updated_at;
    END IF;
    RETURN NEW;
END
$$;
REVOKE ALL ON FUNCTION kb_knowledge_guard_rerank_revision_v2_update() FROM PUBLIC;
CREATE TRIGGER rerank_revisions_v2_guard_update
BEFORE UPDATE ON rerank_revisions_v2
FOR EACH ROW EXECUTE FUNCTION kb_knowledge_guard_rerank_revision_v2_update();

CREATE FUNCTION kb_knowledge_guard_rerank_revision_v2_removal()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public, pg_temp
AS $$
BEGIN
    RAISE EXCEPTION 'RERANK_REVISION_V2_IMMUTABLE' USING ERRCODE='23514';
    RETURN NULL;
END
$$;
REVOKE ALL ON FUNCTION kb_knowledge_guard_rerank_revision_v2_removal() FROM PUBLIC;
CREATE TRIGGER rerank_revisions_v2_guard_removal
BEFORE DELETE OR TRUNCATE ON rerank_revisions_v2
FOR EACH STATEMENT EXECUTE FUNCTION kb_knowledge_guard_rerank_revision_v2_removal();

CREATE FUNCTION kb_knowledge_lock_rerank_revision_v2(p_revision_sha256 text)
RETURNS TABLE(canonical_revision_payload bytea,credential_ref text)
LANGUAGE sql
SECURITY DEFINER
SET search_path = pg_catalog, public, pg_temp
AS $$
 SELECT revision.canonical_revision_payload,revision.credential_ref
   FROM public.rerank_revisions_v2 revision
  WHERE revision.revision_sha256=p_revision_sha256
    AND revision.support_state='supported'
  FOR SHARE OF revision
$$;
REVOKE ALL ON FUNCTION kb_knowledge_lock_rerank_revision_v2(text) FROM PUBLIC;

-- Knowledge owns the v2 policy artifact and support decision. Request DTOs may
-- reference a supported identity, but cannot define their own trusted quotas.
CREATE FUNCTION kb_knowledge_valid_retrieval_policy_v2(
    canonical_policy_payload bytea,
    embedding_revision_sha256 text,
    contract_version text,
    max_hits bigint,
    max_chunk_bytes bigint,
    max_total_bytes bigint)
RETURNS boolean
LANGUAGE plpgsql
IMMUTABLE
STRICT
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE
    payload jsonb;
    canonical_text text;
    keyword_top_k text;
    keyword_threshold text;
    embedding_top_k text;
    embedding_threshold text;
    rrf_k text;
    keyword_weight text;
    vector_weight text;
    rerank_model_sha256 text;
    rerank_config_sha256 text;
    rerank_top_k text;
    rerank_timeout_ms text;
BEGIN
    payload := convert_from(canonical_policy_payload,'UTF8')::jsonb;
    IF jsonb_typeof(payload) IS DISTINCT FROM 'object' THEN
        RETURN false;
    END IF;
    IF (SELECT array_agg(key ORDER BY key) FROM jsonb_object_keys(payload) key)
          IS DISTINCT FROM ARRAY[
              'contract_version','embedding','keyword','normalization_version','ranking',
              'request_quotas','rerank','rrf','schema_version','trusted_source_types']::text[]
       OR payload->'schema_version' IS DISTINCT FROM '2'::jsonb
       OR payload->>'contract_version' IS DISTINCT FROM contract_version
       OR contract_version IS DISTINCT FROM 'knowledge-evidence-v2'
       OR payload->>'normalization_version' IS DISTINCT FROM
            'unicode-whitespace-lowercase-v1'
       OR payload->'trusted_source_types' IS DISTINCT FROM
            '["text","parent_text","image_ocr"]'::jsonb
       OR embedding_revision_sha256 !~ '^[0-9a-f]{64}$'
       OR max_hits NOT BETWEEN 1 AND 1000000
       OR max_chunk_bytes NOT BETWEEN 1 AND 1073741824
       OR max_total_bytes NOT BETWEEN 1 AND 1099511627776 THEN
        RETURN false;
    END IF;
    IF jsonb_typeof(payload->'ranking') IS DISTINCT FROM 'object'
       OR jsonb_typeof(payload->'keyword') IS DISTINCT FROM 'object'
       OR jsonb_typeof(payload->'embedding') IS DISTINCT FROM 'object'
       OR jsonb_typeof(payload->'rrf') IS DISTINCT FROM 'object'
       OR jsonb_typeof(payload->'rerank') IS DISTINCT FROM 'object'
       OR jsonb_typeof(payload->'request_quotas') IS DISTINCT FROM 'object' THEN
        RETURN false;
    END IF;

    IF (SELECT array_agg(key ORDER BY key) FROM jsonb_object_keys(payload->'ranking') key)
          IS DISTINCT FROM ARRAY[
              'a_primary_comparator','a_version_comparator','b_exact_comparator',
              'channel_rank_comparator','channel_score_quantization_version',
              'c_semantic_comparator','pre_rerank_rrf_comparator',
              'quota_semantics_version','source_folding_version']::text[]
       OR payload->'ranking'->'a_primary_comparator' IS DISTINCT FROM
            '["chunk_byte_length ASC","document_id ASC","source_chunk_id ASC"]'::jsonb
       OR payload->'ranking'->'a_version_comparator' IS DISTINCT FROM
            '["product_id ASC","product_version_id ASC"]'::jsonb
       OR payload->'ranking'->'b_exact_comparator' IS DISTINCT FROM
            '["product_id ASC","product_version_id ASC","chunk_byte_length ASC","document_id ASC","source_chunk_id ASC"]'::jsonb
       OR payload->'ranking'->'c_semantic_comparator' IS DISTINCT FROM
            '["normalized_rerank_score DESC","pre_rerank_rrf_rank ASC","complete_source_identity ASC"]'::jsonb
       OR payload->'ranking'->>'source_folding_version' IS DISTINCT FROM
            'unique-live-trusted-source-v1'
       OR payload->'ranking'->>'channel_score_quantization_version' IS DISTINCT FROM
            'floor-unit-interval-millionths-v1'
       OR payload->'ranking'->'channel_rank_comparator' IS DISTINCT FROM
            '["score_millionths DESC","complete_signal_identity ASC"]'::jsonb
       OR payload->'ranking'->'pre_rerank_rrf_comparator' IS DISTINCT FROM
            '["exact_rrf_score DESC","vector_rank ASC NULLS LAST","keyword_rank ASC NULLS LAST","product_id ASC","product_version_id ASC","document_id ASC","source_chunk_id ASC"]'::jsonb
       OR payload->'ranking'->>'quota_semantics_version' IS DISTINCT FROM
            'fair-exact-prefix-fail-closed-v1' THEN
        RETURN false;
    END IF;

    IF (SELECT array_agg(key ORDER BY key) FROM jsonb_object_keys(payload->'keyword') key)
          IS DISTINCT FROM ARRAY[
              'score_version','threshold_millionths','tokenizer','tokenizer_version','top_k']::text[]
       OR payload->'keyword'->>'tokenizer' IS DISTINCT FROM 'latin-numeric-cjk-bigram'
       OR payload->'keyword'->>'tokenizer_version' IS DISTINCT FROM 'v1'
       OR payload->'keyword'->>'score_version' IS DISTINCT FROM
            'postgres-ts-rank-cd-normalization32-millionths-v1'
       OR jsonb_typeof(payload->'keyword'->'top_k') IS DISTINCT FROM 'number'
       OR jsonb_typeof(payload->'keyword'->'threshold_millionths') IS DISTINCT FROM 'number'
       OR payload->'keyword'->>'top_k' !~ '^[1-9][0-9]*$'
       OR length(payload->'keyword'->>'top_k')>7
       OR payload->'keyword'->>'threshold_millionths' !~ '^(0|[1-9][0-9]*)$'
       OR length(payload->'keyword'->>'threshold_millionths')>7 THEN
        RETURN false;
    END IF;
    keyword_top_k := payload->'keyword'->>'top_k';
    keyword_threshold := payload->'keyword'->>'threshold_millionths';
    IF keyword_top_k::bigint>1000000 OR keyword_threshold::bigint>1000000 THEN
        RETURN false;
    END IF;

    IF (SELECT array_agg(key ORDER BY key) FROM jsonb_object_keys(payload->'embedding') key)
          IS DISTINCT FROM ARRAY[
              'model_revision_sha256','policy','policy_version','similarity_version',
              'threshold_millionths','top_k']::text[]
       OR payload->'embedding'->>'policy' IS DISTINCT FROM 'declared-version-model'
       OR payload->'embedding'->>'policy_version' IS DISTINCT FROM 'v1'
       OR payload->'embedding'->>'similarity_version' IS DISTINCT FROM
            'pgvector-cosine-clamp-zero-one-millionths-v1'
       OR payload->'embedding'->>'model_revision_sha256' IS DISTINCT FROM
            embedding_revision_sha256
       OR payload->'embedding'->>'model_revision_sha256' !~ '^[0-9a-f]{64}$'
       OR jsonb_typeof(payload->'embedding'->'top_k') IS DISTINCT FROM 'number'
       OR jsonb_typeof(payload->'embedding'->'threshold_millionths') IS DISTINCT FROM 'number'
       OR payload->'embedding'->>'top_k' !~ '^[1-9][0-9]*$'
       OR length(payload->'embedding'->>'top_k')>7
       OR payload->'embedding'->>'threshold_millionths' !~ '^(0|[1-9][0-9]*)$'
       OR length(payload->'embedding'->>'threshold_millionths')>7 THEN
        RETURN false;
    END IF;
    embedding_top_k := payload->'embedding'->>'top_k';
    embedding_threshold := payload->'embedding'->>'threshold_millionths';
    IF embedding_top_k::bigint>1000000 OR embedding_threshold::bigint>1000000 THEN
        RETURN false;
    END IF;

    IF (SELECT array_agg(key ORDER BY key) FROM jsonb_object_keys(payload->'rrf') key)
          IS DISTINCT FROM ARRAY[
              'k','keyword_weight_millionths','score_representation_version',
              'vector_weight_millionths']::text[]
       OR payload->'rrf'->>'score_representation_version' IS DISTINCT FROM
            'reduced-u128-rational-v1'
       OR jsonb_typeof(payload->'rrf'->'k') IS DISTINCT FROM 'number'
       OR jsonb_typeof(payload->'rrf'->'keyword_weight_millionths') IS DISTINCT FROM 'number'
       OR jsonb_typeof(payload->'rrf'->'vector_weight_millionths') IS DISTINCT FROM 'number'
       OR payload->'rrf'->>'k' !~ '^[1-9][0-9]*$'
       OR length(payload->'rrf'->>'k')>7
       OR payload->'rrf'->>'keyword_weight_millionths' !~ '^[1-9][0-9]*$'
       OR length(payload->'rrf'->>'keyword_weight_millionths')>10
       OR payload->'rrf'->>'vector_weight_millionths' !~ '^[1-9][0-9]*$'
       OR length(payload->'rrf'->>'vector_weight_millionths')>10 THEN
        RETURN false;
    END IF;
    rrf_k := payload->'rrf'->>'k';
    keyword_weight := payload->'rrf'->>'keyword_weight_millionths';
    vector_weight := payload->'rrf'->>'vector_weight_millionths';
    IF rrf_k::bigint>1000000
       OR keyword_weight::bigint>1000000000
       OR vector_weight::bigint>1000000000 THEN
        RETURN false;
    END IF;

    IF (SELECT array_agg(key ORDER BY key) FROM jsonb_object_keys(payload->'rerank') key)
          IS DISTINCT FROM ARRAY[
              'config_revision_sha256','model_revision_sha256','provider_protocol_version',
              'revision_sha256','score_normalization_version','timeout_ms','top_k']::text[]
       OR payload->'rerank'->>'provider_protocol_version' IS DISTINCT FROM 'indexed-json-v1'
       OR payload->'rerank'->>'score_normalization_version' IS DISTINCT FROM
            'unit-interval-millionths-v1'
       OR jsonb_typeof(payload->'rerank'->'revision_sha256') IS DISTINCT FROM 'string'
       OR payload->'rerank'->>'revision_sha256' !~ '^[0-9a-f]{64}$'
       OR jsonb_typeof(payload->'rerank'->'model_revision_sha256') IS DISTINCT FROM 'string'
       OR payload->'rerank'->>'model_revision_sha256' !~ '^[0-9a-f]{64}$'
       OR jsonb_typeof(payload->'rerank'->'config_revision_sha256') IS DISTINCT FROM 'string'
       OR payload->'rerank'->>'config_revision_sha256' !~ '^[0-9a-f]{64}$'
       OR jsonb_typeof(payload->'rerank'->'top_k') IS DISTINCT FROM 'number'
       OR jsonb_typeof(payload->'rerank'->'timeout_ms') IS DISTINCT FROM 'number'
       OR payload->'rerank'->>'top_k' !~ '^[1-9][0-9]*$'
       OR length(payload->'rerank'->>'top_k')>7
       OR payload->'rerank'->>'timeout_ms' !~ '^[1-9][0-9]*$'
       OR length(payload->'rerank'->>'timeout_ms')>7 THEN
        RETURN false;
    END IF;
    rerank_model_sha256 := payload->'rerank'->>'model_revision_sha256';
    rerank_config_sha256 := payload->'rerank'->>'config_revision_sha256';
    rerank_top_k := payload->'rerank'->>'top_k';
    rerank_timeout_ms := payload->'rerank'->>'timeout_ms';
    IF rerank_top_k::bigint>1000000 OR rerank_timeout_ms::bigint>3600000 THEN
        RETURN false;
    END IF;

    IF (SELECT array_agg(key ORDER BY key) FROM jsonb_object_keys(payload->'request_quotas') key)
          IS DISTINCT FROM ARRAY['max_chunk_bytes','max_hits','max_total_bytes']::text[]
       OR jsonb_typeof(payload->'request_quotas'->'max_hits') IS DISTINCT FROM 'number'
       OR jsonb_typeof(payload->'request_quotas'->'max_chunk_bytes') IS DISTINCT FROM 'number'
       OR jsonb_typeof(payload->'request_quotas'->'max_total_bytes') IS DISTINCT FROM 'number'
       OR payload->'request_quotas'->>'max_hits' !~ '^[1-9][0-9]*$'
       OR payload->'request_quotas'->>'max_chunk_bytes' !~ '^[1-9][0-9]*$'
       OR payload->'request_quotas'->>'max_total_bytes' !~ '^[1-9][0-9]*$'
       OR payload->'request_quotas'->>'max_hits' IS DISTINCT FROM max_hits::text
       OR payload->'request_quotas'->>'max_chunk_bytes' IS DISTINCT FROM max_chunk_bytes::text
       OR payload->'request_quotas'->>'max_total_bytes' IS DISTINCT FROM max_total_bytes::text THEN
        RETURN false;
    END IF;

    canonical_text :=
        '{"schema_version":2,"contract_version":'||to_json(contract_version)::text||
        ',"normalization_version":"unicode-whitespace-lowercase-v1"'||
        ',"trusted_source_types":["text","parent_text","image_ocr"]'||
        ',"ranking":{"a_primary_comparator":["chunk_byte_length ASC","document_id ASC","source_chunk_id ASC"]'||
        ',"a_version_comparator":["product_id ASC","product_version_id ASC"]'||
        ',"b_exact_comparator":["product_id ASC","product_version_id ASC","chunk_byte_length ASC","document_id ASC","source_chunk_id ASC"]'||
        ',"c_semantic_comparator":["normalized_rerank_score DESC","pre_rerank_rrf_rank ASC","complete_source_identity ASC"]'||
        ',"source_folding_version":"unique-live-trusted-source-v1"'||
        ',"channel_score_quantization_version":"floor-unit-interval-millionths-v1"'||
        ',"channel_rank_comparator":["score_millionths DESC","complete_signal_identity ASC"]'||
        ',"pre_rerank_rrf_comparator":["exact_rrf_score DESC","vector_rank ASC NULLS LAST","keyword_rank ASC NULLS LAST","product_id ASC","product_version_id ASC","document_id ASC","source_chunk_id ASC"]'||
        ',"quota_semantics_version":"fair-exact-prefix-fail-closed-v1"}'||
        ',"keyword":{"tokenizer":"latin-numeric-cjk-bigram","tokenizer_version":"v1"'||
        ',"score_version":"postgres-ts-rank-cd-normalization32-millionths-v1"'||
        ',"top_k":'||keyword_top_k||',"threshold_millionths":'||keyword_threshold||'}'||
        ',"embedding":{"policy":"declared-version-model","policy_version":"v1"'||
        ',"similarity_version":"pgvector-cosine-clamp-zero-one-millionths-v1"'||
        ',"model_revision_sha256":'||to_json(embedding_revision_sha256)::text||
        ',"top_k":'||embedding_top_k||',"threshold_millionths":'||embedding_threshold||'}'||
        ',"rrf":{"k":'||rrf_k||',"keyword_weight_millionths":'||keyword_weight||
        ',"vector_weight_millionths":'||vector_weight||
        ',"score_representation_version":"reduced-u128-rational-v1"}'||
        ',"rerank":{"provider_protocol_version":"indexed-json-v1"'||
        ',"revision_sha256":'||to_json(payload->'rerank'->>'revision_sha256')::text||
        ',"model_revision_sha256":'||to_json(rerank_model_sha256)::text||
        ',"config_revision_sha256":'||to_json(rerank_config_sha256)::text||
        ',"top_k":'||rerank_top_k||',"timeout_ms":'||rerank_timeout_ms||
        ',"score_normalization_version":"unit-interval-millionths-v1"}'||
        ',"request_quotas":{"max_hits":'||max_hits::text||
        ',"max_chunk_bytes":'||max_chunk_bytes::text||
        ',"max_total_bytes":'||max_total_bytes::text||'}}';
    RETURN canonical_policy_payload=convert_to(canonical_text,'UTF8');
END
$$;
REVOKE ALL ON FUNCTION kb_knowledge_valid_retrieval_policy_v2(bytea,text,text,bigint,bigint,bigint) FROM PUBLIC;

CREATE TABLE knowledge_retrieval_policies_v2 (
    policy_sha256 text PRIMARY KEY CHECK (policy_sha256 ~ '^[0-9a-f]{64}$'),
    canonical_policy_payload bytea NOT NULL,
    embedding_revision_sha256 text NOT NULL REFERENCES embedding_revisions_v2(revision_sha256),
    rerank_revision_sha256 text NOT NULL REFERENCES rerank_revisions_v2(revision_sha256),
    contract_version text NOT NULL CHECK (contract_version='knowledge-evidence-v2'),
    max_hits bigint NOT NULL CHECK (max_hits BETWEEN 1 AND 1000000),
    max_chunk_bytes bigint NOT NULL CHECK (max_chunk_bytes BETWEEN 1 AND 1073741824),
    max_total_bytes bigint NOT NULL CHECK (max_total_bytes BETWEEN 1 AND 1099511627776),
    support_state text NOT NULL DEFAULT 'supported'
        CHECK (support_state IN ('supported','revoked')),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CHECK (policy_sha256=encode(public.digest(canonical_policy_payload,'sha256'),'hex')),
    CONSTRAINT knowledge_retrieval_policies_v2_payload_matches_columns CHECK (
        kb_knowledge_valid_retrieval_policy_v2(
            canonical_policy_payload,embedding_revision_sha256,contract_version,
            max_hits,max_chunk_bytes,max_total_bytes)
    )
);
REVOKE ALL ON TABLE knowledge_retrieval_policies_v2 FROM PUBLIC;

CREATE FUNCTION kb_knowledge_require_supported_embedding_revision_v2()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public, pg_temp
AS $$
BEGIN
    IF TG_TABLE_NAME='knowledge_retrieval_policies_v2' THEN
        IF NEW.rerank_revision_sha256 IS NOT NULL
           AND NEW.rerank_revision_sha256 IS DISTINCT FROM
               convert_from(NEW.canonical_policy_payload,'UTF8')::jsonb->'rerank'->>'revision_sha256' THEN
            RAISE EXCEPTION 'RERANK_REVISION_V2_POLICY_MISMATCH' USING ERRCODE='23514';
        END IF;
        NEW.rerank_revision_sha256 :=
            convert_from(NEW.canonical_policy_payload,'UTF8')::jsonb->'rerank'->>'revision_sha256';
    END IF;
    PERFORM 1
      FROM public.embedding_revisions_v2 revision
     WHERE revision.revision_sha256=NEW.embedding_revision_sha256
       AND revision.support_state='supported'
     FOR SHARE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'EMBEDDING_REVISION_V2_NOT_SUPPORTED' USING ERRCODE='23514';
    END IF;
    IF TG_TABLE_NAME='knowledge_retrieval_policies_v2' THEN
        PERFORM 1
          FROM public.rerank_revisions_v2 revision
         WHERE revision.revision_sha256=NEW.rerank_revision_sha256
           AND revision.support_state='supported'
           AND revision.provider_model_revision_sha256=
               convert_from(NEW.canonical_policy_payload,'UTF8')::jsonb->'rerank'->>'model_revision_sha256'
           AND revision.config_revision_sha256=
               convert_from(NEW.canonical_policy_payload,'UTF8')::jsonb->'rerank'->>'config_revision_sha256'
         FOR SHARE;
        IF NOT FOUND THEN
            RAISE EXCEPTION 'RERANK_REVISION_V2_NOT_SUPPORTED' USING ERRCODE='23514';
        END IF;
    END IF;
    RETURN NEW;
END
$$;
REVOKE ALL ON FUNCTION kb_knowledge_require_supported_embedding_revision_v2() FROM PUBLIC;
CREATE TRIGGER knowledge_retrieval_policies_v2_require_supported_embedding
BEFORE INSERT ON knowledge_retrieval_policies_v2
FOR EACH ROW EXECUTE FUNCTION kb_knowledge_require_supported_embedding_revision_v2();

CREATE FUNCTION kb_knowledge_guard_retrieval_policy_v2_update()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public, pg_temp
AS $$
BEGIN
    IF NEW.policy_sha256 IS DISTINCT FROM OLD.policy_sha256
       OR NEW.canonical_policy_payload IS DISTINCT FROM OLD.canonical_policy_payload
       OR NEW.embedding_revision_sha256 IS DISTINCT FROM OLD.embedding_revision_sha256
       OR NEW.rerank_revision_sha256 IS DISTINCT FROM OLD.rerank_revision_sha256
       OR NEW.contract_version IS DISTINCT FROM OLD.contract_version
       OR NEW.max_hits IS DISTINCT FROM OLD.max_hits
       OR NEW.max_chunk_bytes IS DISTINCT FROM OLD.max_chunk_bytes
       OR NEW.max_total_bytes IS DISTINCT FROM OLD.max_total_bytes
       OR NEW.created_at IS DISTINCT FROM OLD.created_at
       OR NOT (
           NEW.support_state IS NOT DISTINCT FROM OLD.support_state
           OR (OLD.support_state='supported' AND NEW.support_state='revoked')) THEN
        RAISE EXCEPTION 'KNOWLEDGE_RETRIEVAL_POLICY_V2_IMMUTABLE' USING ERRCODE='23514';
    END IF;
    IF NEW.support_state IS DISTINCT FROM OLD.support_state THEN
        NEW.updated_at := clock_timestamp();
    ELSE
        NEW.updated_at := OLD.updated_at;
    END IF;
    RETURN NEW;
END
$$;
REVOKE ALL ON FUNCTION kb_knowledge_guard_retrieval_policy_v2_update() FROM PUBLIC;
CREATE TRIGGER knowledge_retrieval_policies_v2_guard_update
BEFORE UPDATE ON knowledge_retrieval_policies_v2
FOR EACH ROW EXECUTE FUNCTION kb_knowledge_guard_retrieval_policy_v2_update();

CREATE FUNCTION kb_knowledge_guard_retrieval_policy_v2_removal()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public, pg_temp
AS $$
BEGIN
    RAISE EXCEPTION 'KNOWLEDGE_RETRIEVAL_POLICY_V2_IMMUTABLE' USING ERRCODE='23514';
    RETURN NULL;
END
$$;
REVOKE ALL ON FUNCTION kb_knowledge_guard_retrieval_policy_v2_removal() FROM PUBLIC;
CREATE TRIGGER knowledge_retrieval_policies_v2_guard_removal
BEFORE DELETE OR TRUNCATE ON knowledge_retrieval_policies_v2
FOR EACH STATEMENT EXECUTE FUNCTION kb_knowledge_guard_retrieval_policy_v2_removal();

-- Runtime semantic recall locks the immutable policy and embedding revision for
-- the lifetime of its transaction. SECURITY DEFINER permits runtime roles with
-- SELECT-only registry ACLs to acquire the row locks without registry DML rights.
CREATE FUNCTION kb_knowledge_lock_semantic_policy_v2(p_policy_sha256 text)
RETURNS TABLE(
    canonical_policy_payload bytea,
    canonical_revision_payload bytea,
    embedding_revision_sha256 text,
    credential_ref text
)
LANGUAGE sql
SECURITY DEFINER
SET search_path = pg_catalog, public, pg_temp
AS $$
SELECT policy.canonical_policy_payload,
       revision.canonical_revision_payload,
       policy.embedding_revision_sha256,
       revision.credential_ref
  FROM public.knowledge_retrieval_policies_v2 policy
  JOIN public.embedding_revisions_v2 revision
    ON revision.revision_sha256=policy.embedding_revision_sha256
  JOIN public.rerank_revisions_v2 reranker
    ON reranker.revision_sha256=policy.rerank_revision_sha256
 WHERE policy.policy_sha256=p_policy_sha256
   AND policy.support_state='supported'
   AND revision.support_state='supported'
   AND reranker.support_state='supported'
 FOR SHARE OF policy,revision,reranker
$$;
REVOKE ALL ON FUNCTION kb_knowledge_lock_semantic_policy_v2(text) FROM PUBLIC;

CREATE TABLE product_version_embedding_bindings_v2 (
    product_version_id uuid PRIMARY KEY REFERENCES product_versions(id) ON DELETE CASCADE,
    embedding_revision_sha256 text NOT NULL REFERENCES embedding_revisions_v2(revision_sha256),
    created_at timestamptz NOT NULL DEFAULT now()
);
REVOKE ALL ON TABLE product_version_embedding_bindings_v2 FROM PUBLIC;
CREATE TRIGGER product_version_embedding_v2_require_supported
BEFORE INSERT ON product_version_embedding_bindings_v2
FOR EACH ROW EXECUTE FUNCTION kb_knowledge_require_supported_embedding_revision_v2();

CREATE FUNCTION kb_knowledge_guard_embedding_binding_v2_mutation()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public, pg_temp
AS $$
BEGIN
    -- The parent row is already absent when its AFTER DELETE FK action reaches
    -- this trigger. Direct child deletion still sees the parent and is rejected.
    IF TG_OP='DELETE' AND NOT EXISTS(
        SELECT 1 FROM public.product_versions version
         WHERE version.id=OLD.product_version_id
    ) THEN
        RETURN OLD;
    END IF;
    RAISE EXCEPTION 'PRODUCT_VERSION_EMBEDDING_BINDING_V2_IMMUTABLE' USING ERRCODE='23514';
    RETURN NULL;
END
$$;
REVOKE ALL ON FUNCTION kb_knowledge_guard_embedding_binding_v2_mutation() FROM PUBLIC;
CREATE TRIGGER product_version_embedding_bindings_v2_guard_row_mutation
BEFORE UPDATE OR DELETE ON product_version_embedding_bindings_v2
FOR EACH ROW EXECUTE FUNCTION kb_knowledge_guard_embedding_binding_v2_mutation();
CREATE TRIGGER product_version_embedding_bindings_v2_guard_truncate
BEFORE TRUNCATE ON product_version_embedding_bindings_v2
FOR EACH STATEMENT EXECUTE FUNCTION kb_knowledge_guard_embedding_binding_v2_mutation();

-- This code-point set is shared exactly with index::keyword_tokens_v2. Unicode
-- 15.1 CJK Unified Ideographs Extension I is U+2EBF0..U+2EE5F; all other
-- pre-existing accepted ranges are retained.
CREATE FUNCTION kb_knowledge_keyword_token_stream_v2(value text)
RETURNS text
LANGUAGE sql
IMMUTABLE
STRICT
SECURITY DEFINER
SET search_path = pg_catalog, public, pg_temp
AS $$
WITH classified AS (
    SELECT character_value, ordinal,
           CASE
             WHEN ascii(character_value) BETWEEN 48 AND 57
               OR ascii(character_value) BETWEEN 65 AND 90
               OR ascii(character_value) BETWEEN 97 AND 122 THEN 'ascii'
             WHEN ascii(character_value) BETWEEN 13312 AND 19903
               OR ascii(character_value) BETWEEN 19968 AND 40959
               OR ascii(character_value) BETWEEN 63744 AND 64255
               OR ascii(character_value) BETWEEN 131072 AND 191471
               OR ascii(character_value) BETWEEN 191472 AND 192095
               OR ascii(character_value) BETWEEN 194560 AND 195103
               OR ascii(character_value) BETWEEN 196608 AND 205743 THEN 'cjk'
             ELSE 'separator'
           END AS character_kind
      FROM unnest(string_to_array(value,NULL)) WITH ORDINALITY
           AS characters(character_value,ordinal)
), marked AS (
    SELECT character_value, ordinal, character_kind,
           CASE WHEN character_kind='separator'
                  OR character_kind IS DISTINCT FROM
                     lag(character_kind) OVER (ORDER BY ordinal)
                THEN 1 ELSE 0 END AS starts_run
      FROM classified
), grouped AS (
    SELECT character_value, ordinal, character_kind,
           sum(starts_run) OVER (ORDER BY ordinal) AS run_ordinal
      FROM marked
), runs AS (
    SELECT run_ordinal, character_kind,
           string_agg(character_value,'' ORDER BY ordinal) AS run_value
      FROM grouped
     WHERE character_kind<>'separator'
     GROUP BY run_ordinal,character_kind
), tokens AS (
    SELECT run_ordinal, token_ordinal,
           CASE WHEN character_kind='ascii'
                THEN translate(run_value,'ABCDEFGHIJKLMNOPQRSTUVWXYZ','abcdefghijklmnopqrstuvwxyz')
                ELSE substr(run_value,token_ordinal,2)
           END AS token
      FROM runs
      CROSS JOIN LATERAL generate_series(
          1,
          CASE WHEN character_kind='ascii' OR char_length(run_value)=1
               THEN 1 ELSE char_length(run_value)-1 END
      ) AS token_ordinals(token_ordinal)
)
SELECT COALESCE(string_agg(token,' ' ORDER BY run_ordinal,token_ordinal),'')
  FROM tokens
$$;
REVOKE ALL ON FUNCTION kb_knowledge_keyword_token_stream_v2(text) FROM PUBLIC;

CREATE TABLE chunk_keyword_indexes_v2 (
    chunk_id uuid NOT NULL REFERENCES chunks(id) ON DELETE CASCADE,
    tokenizer text NOT NULL CHECK (tokenizer='latin-numeric-cjk-bigram'),
    tokenizer_version text NOT NULL CHECK (tokenizer_version='v1'),
    indexed_content text NOT NULL,
    indexed_content_sha256 text NOT NULL CHECK (indexed_content_sha256 ~ '^[0-9a-f]{64}$'),
    tsv tsvector NOT NULL,
    PRIMARY KEY (chunk_id,tokenizer,tokenizer_version),
    CHECK (indexed_content_sha256=encode(public.digest(convert_to(indexed_content,'UTF8'),'sha256'),'hex')),
    CHECK (tsv=to_tsvector('simple',kb_knowledge_keyword_token_stream_v2(indexed_content)))
);
CREATE INDEX chunk_keyword_indexes_v2_tsv_idx
    ON chunk_keyword_indexes_v2 USING gin(tsv);
CREATE INDEX chunk_keyword_indexes_v2_tokenizer_idx
    ON chunk_keyword_indexes_v2(tokenizer,tokenizer_version);
REVOKE ALL ON TABLE chunk_keyword_indexes_v2 FROM PUBLIC;

-- FOR UPDATE on product_versions conflicts with the key-share lock taken by the
-- chunks FK check, so inserts for this version wait until a rebuild commits.
-- Existing chunks are then locked in UUID order before the atomic replacement.
CREATE FUNCTION kb_knowledge_rebuild_keyword_indexes_v2(p_product_version_id uuid)
RETURNS bigint
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public, pg_temp
AS $$
DECLARE
    inserted_count bigint;
BEGIN
    PERFORM 1
      FROM public.product_versions
     WHERE id=p_product_version_id
     FOR UPDATE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'KNOWLEDGE_PRODUCT_VERSION_V2_NOT_FOUND: %',p_product_version_id
            USING ERRCODE='23503';
    END IF;

    PERFORM chunk.id
      FROM public.chunks chunk
     WHERE chunk.product_version_id=p_product_version_id
     ORDER BY chunk.id
     FOR UPDATE;

    DELETE FROM public.chunk_keyword_indexes_v2 keyword_index
     USING public.chunks chunk
     WHERE keyword_index.chunk_id=chunk.id
       AND chunk.product_version_id=p_product_version_id
       AND keyword_index.tokenizer='latin-numeric-cjk-bigram'
       AND keyword_index.tokenizer_version='v1';

    INSERT INTO public.chunk_keyword_indexes_v2(
        chunk_id,tokenizer,tokenizer_version,indexed_content,
        indexed_content_sha256,tsv)
    SELECT chunk.id,'latin-numeric-cjk-bigram','v1',chunk.content,
           encode(public.digest(convert_to(chunk.content,'UTF8'),'sha256'),'hex'),
           to_tsvector('simple',public.kb_knowledge_keyword_token_stream_v2(chunk.content))
      FROM public.chunks chunk
     WHERE chunk.product_version_id=p_product_version_id
     ORDER BY chunk.id;
    GET DIAGNOSTICS inserted_count=ROW_COUNT;
    RETURN inserted_count;
END
$$;
REVOKE ALL ON FUNCTION kb_knowledge_rebuild_keyword_indexes_v2(uuid) FROM PUBLIC;

CREATE TABLE product_version_vector_index_generations_v2 (
    product_version_id uuid PRIMARY KEY REFERENCES product_versions(id) ON DELETE CASCADE,
    embedding_revision_sha256 text NOT NULL REFERENCES embedding_revisions_v2(revision_sha256),
    source_snapshot_sha256 text NOT NULL CHECK (source_snapshot_sha256 ~ '^[0-9a-f]{64}$'),
    chunk_count bigint NOT NULL CHECK (chunk_count BETWEEN 0 AND 16384),
    completed_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
REVOKE ALL ON TABLE product_version_vector_index_generations_v2 FROM PUBLIC;

CREATE TABLE product_version_keyword_index_generations_v2 (
    product_version_id uuid PRIMARY KEY REFERENCES product_versions(id) ON DELETE CASCADE,
    embedding_revision_sha256 text NOT NULL REFERENCES embedding_revisions_v2(revision_sha256),
    source_snapshot_sha256 text NOT NULL CHECK (source_snapshot_sha256 ~ '^[0-9a-f]{64}$'),
    chunk_count bigint NOT NULL CHECK (chunk_count BETWEEN 0 AND 16384),
    completed_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
REVOKE ALL ON TABLE product_version_keyword_index_generations_v2 FROM PUBLIC;

CREATE TABLE knowledge_semantic_index_intents_v2 (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    product_version_id uuid NOT NULL REFERENCES product_versions(id) ON DELETE CASCADE,
    embedding_revision_sha256 text NOT NULL REFERENCES embedding_revisions_v2(revision_sha256),
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
CREATE UNIQUE INDEX knowledge_semantic_index_intents_v2_one_pending
    ON knowledge_semantic_index_intents_v2(product_version_id) WHERE status='pending';
CREATE INDEX knowledge_semantic_index_intents_v2_status_idx
    ON knowledge_semantic_index_intents_v2(status,created_at,id);
REVOKE ALL ON TABLE knowledge_semantic_index_intents_v2 FROM PUBLIC;

CREATE FUNCTION kb_knowledge_guard_semantic_index_intent_v2_mutation()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public, pg_temp
AS $$
BEGIN
    IF TG_OP='UPDATE'
       AND NEW.id IS NOT DISTINCT FROM OLD.id
       AND NEW.product_version_id IS NOT DISTINCT FROM OLD.product_version_id
       AND NEW.embedding_revision_sha256 IS NOT DISTINCT FROM OLD.embedding_revision_sha256
       AND NEW.source_snapshot_sha256 IS NOT DISTINCT FROM OLD.source_snapshot_sha256
       AND NEW.target_revision IS NOT DISTINCT FROM OLD.target_revision
       AND NEW.created_at IS NOT DISTINCT FROM OLD.created_at THEN
        RETURN NEW;
    END IF;
    -- Preserve direct-delete immutability while allowing the declared
    -- product-version parent cascade after that parent row is gone.
    IF TG_OP='DELETE' AND NOT EXISTS(
        SELECT 1 FROM public.product_versions version
         WHERE version.id=OLD.product_version_id
    ) THEN
        RETURN OLD;
    END IF;
    RAISE EXCEPTION 'KNOWLEDGE_SEMANTIC_INDEX_INTENT_V2_IMMUTABLE' USING ERRCODE='23514';
    RETURN NULL;
END
$$;
REVOKE ALL ON FUNCTION kb_knowledge_guard_semantic_index_intent_v2_mutation() FROM PUBLIC;
CREATE TRIGGER knowledge_semantic_index_intents_v2_guard_row_mutation
BEFORE UPDATE OR DELETE ON knowledge_semantic_index_intents_v2
FOR EACH ROW EXECUTE FUNCTION kb_knowledge_guard_semantic_index_intent_v2_mutation();
CREATE TRIGGER knowledge_semantic_index_intents_v2_guard_truncate
BEFORE TRUNCATE ON knowledge_semantic_index_intents_v2
FOR EACH STATEMENT EXECUTE FUNCTION kb_knowledge_guard_semantic_index_intent_v2_mutation();

CREATE TABLE chunk_vector_indexes_v2 (
    chunk_id uuid NOT NULL,
    product_version_id uuid NOT NULL,
    embedding_revision_sha256 text NOT NULL REFERENCES embedding_revisions_v2(revision_sha256),
    source_snapshot_sha256 text NOT NULL CHECK (source_snapshot_sha256 ~ '^[0-9a-f]{64}$'),
    indexed_content_sha256 text NOT NULL CHECK (indexed_content_sha256 ~ '^[0-9a-f]{64}$'),
    embedding vector(1024) NOT NULL,
    PRIMARY KEY (chunk_id,embedding_revision_sha256),
    FOREIGN KEY (chunk_id,product_version_id)
        REFERENCES chunks(id,product_version_id) ON DELETE CASCADE
);
CREATE INDEX chunk_vector_indexes_v2_hnsw_idx
    ON chunk_vector_indexes_v2 USING hnsw(embedding vector_cosine_ops);
CREATE INDEX chunk_vector_indexes_v2_revision_idx
    ON chunk_vector_indexes_v2(embedding_revision_sha256);
CREATE INDEX chunk_vector_indexes_v2_owner_revision_idx
    ON chunk_vector_indexes_v2(product_version_id,embedding_revision_sha256);
REVOKE ALL ON TABLE chunk_vector_indexes_v2 FROM PUBLIC;

-- Reconciles one complete, immutable-revision vector generation. The runtime
-- worker can execute this verifier but has no direct sidecar or marker DML.
-- Product/document/chunk and binding/revision locks make the post-network
-- source revalidation and replacement one atomic statement.
CREATE FUNCTION kb_knowledge_reconcile_vector_indexes_v2(
    p_product_version_id uuid,
    p_embedding_revision_sha256 text,
    p_source_snapshot_sha256 text,
    p_vectors jsonb)
RETURNS bigint
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public, pg_temp
AS $$
DECLARE
    actual_snapshot_sha256 text;
    actual_count bigint;
    payload_count bigint;
BEGIN
    IF p_embedding_revision_sha256 !~ '^[0-9a-f]{64}$'
       OR p_source_snapshot_sha256 !~ '^[0-9a-f]{64}$'
       OR jsonb_typeof(p_vectors) IS DISTINCT FROM 'array'
       OR jsonb_array_length(p_vectors)>16384 THEN
        RAISE EXCEPTION 'KNOWLEDGE_VECTOR_INDEX_V2_INVALID' USING ERRCODE='23514';
    END IF;

    PERFORM 1 FROM public.product_versions version
     WHERE version.id=p_product_version_id AND version.deleted_at IS NULL
     FOR UPDATE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'KNOWLEDGE_VECTOR_INDEX_V2_NOT_SUPPORTED' USING ERRCODE='23514';
    END IF;
    PERFORM 1
      FROM public.product_version_embedding_bindings_v2 binding
      JOIN public.embedding_revisions_v2 revision
        ON revision.revision_sha256=binding.embedding_revision_sha256
       AND revision.support_state='supported'
     WHERE binding.product_version_id=p_product_version_id
       AND binding.embedding_revision_sha256=p_embedding_revision_sha256
     FOR SHARE OF binding,revision;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'KNOWLEDGE_VECTOR_INDEX_V2_NOT_SUPPORTED' USING ERRCODE='23514';
    END IF;

    PERFORM document.id FROM public.documents document
     WHERE document.product_version_id=p_product_version_id
     ORDER BY document.id FOR UPDATE;
    PERFORM chunk.id FROM public.chunks chunk
     WHERE chunk.product_version_id=p_product_version_id
     ORDER BY chunk.id FOR UPDATE;

    SELECT count(*),
           encode(public.digest(convert_to(
             p_embedding_revision_sha256||E'\n'||COALESCE(string_agg(
               chunk.id::text||':'||encode(public.digest(convert_to(
                 chunk.context_header||E'\n'||chunk.content,'UTF8'),'sha256'),'hex')||E'\n',
               '' ORDER BY chunk.id),''),'UTF8'),'sha256'),'hex')
      INTO actual_count,actual_snapshot_sha256
      FROM public.chunks chunk
      JOIN public.documents document ON document.id=chunk.document_id
       AND document.product_version_id=chunk.product_version_id
     WHERE chunk.product_version_id=p_product_version_id
       AND document.deleted_at IS NULL
       AND document.enable_status='enabled'
       AND document.index_ready
       AND chunk.chunk_type=ANY(ARRAY[
         'text','parent_text','image_ocr','question','summary','image_caption','graph_node','wiki_page']);

    IF actual_snapshot_sha256 IS DISTINCT FROM p_source_snapshot_sha256
       OR actual_count<>jsonb_array_length(p_vectors) THEN
        RAISE EXCEPTION 'KNOWLEDGE_VECTOR_INDEX_V2_SNAPSHOT_CHANGED' USING ERRCODE='40001';
    END IF;

    IF EXISTS (
        SELECT 1
          FROM jsonb_array_elements(p_vectors) WITH ORDINALITY payload(value,ordinal)
         WHERE jsonb_typeof(value) IS DISTINCT FROM 'object'
            OR (SELECT array_agg(key ORDER BY key) FROM jsonb_object_keys(value) key)
                 IS DISTINCT FROM ARRAY['chunk_id','embedding','indexed_content_sha256']::text[]
            OR jsonb_typeof(value->'chunk_id') IS DISTINCT FROM 'string'
            OR value->>'chunk_id' !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
            OR jsonb_typeof(value->'indexed_content_sha256') IS DISTINCT FROM 'string'
            OR value->>'indexed_content_sha256' !~ '^[0-9a-f]{64}$'
            OR jsonb_typeof(value->'embedding') IS DISTINCT FROM 'array'
            OR jsonb_array_length(value->'embedding')<>1024
            OR vector_dims((value->'embedding')::text::vector)<>1024
            OR vector_norm((value->'embedding')::text::vector)=0
    ) THEN
        RAISE EXCEPTION 'KNOWLEDGE_VECTOR_INDEX_V2_INVALID' USING ERRCODE='23514';
    END IF;

    WITH payload AS (
        SELECT (value->>'chunk_id')::uuid AS chunk_id,
               value->>'indexed_content_sha256' AS indexed_content_sha256,
               ordinal
          FROM jsonb_array_elements(p_vectors) WITH ORDINALITY values(value,ordinal)
    ), expected AS (
        SELECT chunk.id AS chunk_id,
               encode(public.digest(convert_to(
                 chunk.context_header||E'\n'||chunk.content,'UTF8'),'sha256'),'hex')
                 AS indexed_content_sha256,
               row_number() OVER (ORDER BY chunk.id) AS ordinal
          FROM public.chunks chunk
          JOIN public.documents document ON document.id=chunk.document_id
           AND document.product_version_id=chunk.product_version_id
         WHERE chunk.product_version_id=p_product_version_id
           AND document.deleted_at IS NULL
           AND document.enable_status='enabled'
           AND document.index_ready
           AND chunk.chunk_type=ANY(ARRAY[
             'text','parent_text','image_ocr','question','summary','image_caption','graph_node','wiki_page'])
    )
    SELECT count(*) INTO payload_count
      FROM payload FULL JOIN expected USING(chunk_id,indexed_content_sha256,ordinal)
     WHERE payload.chunk_id IS NULL OR expected.chunk_id IS NULL;
    IF payload_count<>0 THEN
        RAISE EXCEPTION 'KNOWLEDGE_VECTOR_INDEX_V2_SNAPSHOT_CHANGED' USING ERRCODE='40001';
    END IF;

    DELETE FROM public.chunk_vector_indexes_v2
     WHERE product_version_id=p_product_version_id;

    INSERT INTO public.chunk_vector_indexes_v2(
        chunk_id,product_version_id,embedding_revision_sha256,
        source_snapshot_sha256,indexed_content_sha256,embedding)
    SELECT (value->>'chunk_id')::uuid,p_product_version_id,
           p_embedding_revision_sha256,p_source_snapshot_sha256,
           value->>'indexed_content_sha256',
           (value->'embedding')::text::vector
      FROM jsonb_array_elements(p_vectors) WITH ORDINALITY values(value,ordinal)
     ORDER BY ordinal;

    INSERT INTO public.product_version_vector_index_generations_v2(
        product_version_id,embedding_revision_sha256,source_snapshot_sha256,
        chunk_count,completed_at)
    VALUES(p_product_version_id,p_embedding_revision_sha256,
           p_source_snapshot_sha256,actual_count,clock_timestamp())
    ON CONFLICT(product_version_id) DO UPDATE SET
        embedding_revision_sha256=EXCLUDED.embedding_revision_sha256,
        source_snapshot_sha256=EXCLUDED.source_snapshot_sha256,
        chunk_count=EXCLUDED.chunk_count,
        completed_at=EXCLUDED.completed_at;
    RETURN actual_count;
END
$$;
REVOKE ALL ON FUNCTION kb_knowledge_reconcile_vector_indexes_v2(uuid,text,text,jsonb) FROM PUBLIC;

CREATE FUNCTION kb_knowledge_source_snapshot_v2(
    p_product_version_id uuid,
    p_embedding_revision_sha256 text)
RETURNS TABLE(source_snapshot_sha256 text,chunk_count bigint)
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog, public, pg_temp
AS $$
  SELECT encode(public.digest(convert_to(
           p_embedding_revision_sha256||E'\n'||COALESCE(string_agg(
             chunk.id::text||':'||encode(public.digest(convert_to(
               chunk.context_header||E'\n'||chunk.content,'UTF8'),'sha256'),'hex')||E'\n',
             '' ORDER BY chunk.id),''),'UTF8'),'sha256'),'hex'),
         count(*)
    FROM public.chunks chunk
    JOIN public.documents document ON document.id=chunk.document_id
     AND document.product_version_id=chunk.product_version_id
   WHERE chunk.product_version_id=p_product_version_id
     AND document.deleted_at IS NULL
     AND document.enable_status='enabled'
     AND document.index_ready
     AND chunk.chunk_type=ANY(ARRAY[
       'text','parent_text','image_ocr','question','summary','image_caption','graph_node','wiki_page'])
     AND (chunk.chunk_type<>'image_ocr' OR EXISTS (
       SELECT 1 FROM public.knowledge_image_ocr_chunk_artifact_mappings mapping
        WHERE mapping.chunk_id=chunk.id))
$$;
REVOKE ALL ON FUNCTION kb_knowledge_source_snapshot_v2(uuid,text) FROM PUBLIC;

CREATE FUNCTION kb_knowledge_has_pending_derived_v2(p_product_version_id uuid)
RETURNS boolean
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog, public, pg_temp
AS $$
  SELECT EXISTS(
           SELECT 1 FROM public.documents document
            WHERE document.product_version_id=p_product_version_id
              AND document.deleted_at IS NULL
              AND (document.parse_status IN ('pending','processing','finalizing')
                   OR document.pending_subtasks_count<>0
                   OR document.summary_status IN ('pending','processing')))
      OR EXISTS(
           SELECT 1 FROM public.task_pending_ops pending
            WHERE pending.scope='product_version'
              AND pending.scope_id=p_product_version_id)
$$;
REVOKE ALL ON FUNCTION kb_knowledge_has_pending_derived_v2(uuid) FROM PUBLIC;

CREATE FUNCTION kb_knowledge_prepare_semantic_index_intent_v2(p_product_version_id uuid)
RETURNS TABLE(state text,intent_id uuid,target_revision bigint)
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public, pg_temp
AS $$
DECLARE
    revision_sha256_value text;
    support_state_value text;
    snapshot_value text;
    intent_value public.knowledge_semantic_index_intents_v2%ROWTYPE;
    next_target_revision bigint;
BEGIN
    PERFORM 1 FROM public.product_versions version
     WHERE version.id=p_product_version_id AND version.deleted_at IS NULL
     FOR UPDATE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'KNOWLEDGE_SEMANTIC_INDEX_V2_NOT_FOUND' USING ERRCODE='23503';
    END IF;

    SELECT binding.embedding_revision_sha256,revision.support_state
      INTO revision_sha256_value,support_state_value
      FROM public.product_version_embedding_bindings_v2 binding
      JOIN public.embedding_revisions_v2 revision
        ON revision.revision_sha256=binding.embedding_revision_sha256
     WHERE binding.product_version_id=p_product_version_id
     FOR SHARE OF binding,revision;
    IF NOT FOUND THEN
        RETURN QUERY SELECT 'unbound'::text,NULL::uuid,NULL::bigint;
        RETURN;
    END IF;

    PERFORM document.id FROM public.documents document
     WHERE document.product_version_id=p_product_version_id
     ORDER BY document.id FOR UPDATE;
    PERFORM chunk.id FROM public.chunks chunk
     WHERE chunk.product_version_id=p_product_version_id
     ORDER BY chunk.id FOR UPDATE;
    PERFORM pending.id FROM public.task_pending_ops pending
     WHERE pending.scope='product_version' AND pending.scope_id=p_product_version_id
     ORDER BY pending.id FOR UPDATE;

    IF public.kb_knowledge_has_pending_derived_v2(p_product_version_id) THEN
        RETURN QUERY SELECT 'pending_derived'::text,NULL::uuid,NULL::bigint;
        RETURN;
    END IF;

    SELECT source_snapshot_sha256 INTO snapshot_value
      FROM public.kb_knowledge_source_snapshot_v2(
        p_product_version_id,revision_sha256_value);

    UPDATE public.knowledge_semantic_index_intents_v2
       SET status='superseded',last_error_code='SOURCE_GENERATION_SUPERSEDED',
           last_error_detail='a newer settled source or binding generation exists'
     WHERE product_version_id=p_product_version_id AND status='pending'
       AND (embedding_revision_sha256<>revision_sha256_value
            OR source_snapshot_sha256<>snapshot_value
            OR support_state_value<>'supported');

    SELECT * INTO intent_value
      FROM public.knowledge_semantic_index_intents_v2 intent
     WHERE intent.product_version_id=p_product_version_id
       AND intent.embedding_revision_sha256=revision_sha256_value
       AND intent.source_snapshot_sha256=snapshot_value
       AND intent.status='pending'
     ORDER BY intent.target_revision DESC LIMIT 1;
    IF FOUND THEN
        RETURN QUERY SELECT 'enqueue'::text,intent_value.id,intent_value.target_revision;
        RETURN;
    END IF;

    IF support_state_value='supported' THEN
        SELECT * INTO intent_value
          FROM public.knowledge_semantic_index_intents_v2 intent
         WHERE intent.product_version_id=p_product_version_id
           AND intent.embedding_revision_sha256=revision_sha256_value
           AND intent.source_snapshot_sha256=snapshot_value
           AND intent.status='completed'
           AND EXISTS(
             SELECT 1 FROM public.product_version_keyword_index_generations_v2 keyword_generation
              JOIN public.product_version_vector_index_generations_v2 vector_generation
                ON vector_generation.product_version_id=keyword_generation.product_version_id
               AND vector_generation.embedding_revision_sha256=keyword_generation.embedding_revision_sha256
               AND vector_generation.source_snapshot_sha256=keyword_generation.source_snapshot_sha256
               AND vector_generation.chunk_count=keyword_generation.chunk_count
             WHERE keyword_generation.product_version_id=p_product_version_id
               AND keyword_generation.embedding_revision_sha256=revision_sha256_value
               AND keyword_generation.source_snapshot_sha256=snapshot_value)
         ORDER BY intent.target_revision DESC LIMIT 1;
        IF FOUND THEN
            RETURN QUERY SELECT 'ready'::text,intent_value.id,intent_value.target_revision;
            RETURN;
        END IF;
    ELSE
        SELECT * INTO intent_value
          FROM public.knowledge_semantic_index_intents_v2 intent
         WHERE intent.product_version_id=p_product_version_id
           AND intent.embedding_revision_sha256=revision_sha256_value
           AND intent.source_snapshot_sha256=snapshot_value
           AND intent.status='terminal'
         ORDER BY intent.target_revision DESC LIMIT 1;
        IF FOUND THEN
            RETURN QUERY SELECT 'terminal'::text,intent_value.id,intent_value.target_revision;
            RETURN;
        END IF;
    END IF;

    SELECT COALESCE(max(intent.target_revision),0)+1 INTO next_target_revision
      FROM public.knowledge_semantic_index_intents_v2 intent
     WHERE intent.product_version_id=p_product_version_id;
    INSERT INTO public.knowledge_semantic_index_intents_v2(
        product_version_id,embedding_revision_sha256,source_snapshot_sha256,
        target_revision,status,last_error_code,last_error_detail)
    VALUES(p_product_version_id,revision_sha256_value,snapshot_value,next_target_revision,
           CASE WHEN support_state_value='supported' THEN 'pending' ELSE 'terminal' END,
           CASE WHEN support_state_value='supported' THEN NULL ELSE 'EMBEDDING_REVISION_REVOKED' END,
           CASE WHEN support_state_value='supported' THEN NULL ELSE 'immutable embedding revision is revoked' END)
    RETURNING * INTO intent_value;

    RETURN QUERY SELECT
      CASE intent_value.status WHEN 'pending' THEN 'enqueue' ELSE 'terminal' END,
      intent_value.id,intent_value.target_revision;
END
$$;
REVOKE ALL ON FUNCTION kb_knowledge_prepare_semantic_index_intent_v2(uuid) FROM PUBLIC;

CREATE FUNCTION kb_knowledge_preflight_semantic_index_intent_v2(
    p_intent_id uuid,p_target_revision bigint)
RETURNS text
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public, pg_temp
AS $$
DECLARE
    intent_value public.knowledge_semantic_index_intents_v2%ROWTYPE;
    actual_snapshot text;
BEGIN
    SELECT * INTO intent_value
      FROM public.knowledge_semantic_index_intents_v2 intent
     WHERE intent.id=p_intent_id AND intent.target_revision=p_target_revision
     FOR UPDATE;
    IF NOT FOUND THEN
        RETURN 'duplicate';
    END IF;
    IF intent_value.status<>'pending' THEN
        RETURN intent_value.status;
    END IF;

    PERFORM 1 FROM public.product_versions version
     WHERE version.id=intent_value.product_version_id AND version.deleted_at IS NULL
     FOR UPDATE;
    IF NOT FOUND THEN
        UPDATE public.knowledge_semantic_index_intents_v2
           SET status='superseded',last_error_code='SOURCE_GENERATION_SUPERSEDED',
               last_error_detail='product version is no longer live'
         WHERE id=p_intent_id;
        RETURN 'superseded';
    END IF;
    PERFORM 1 FROM public.product_version_embedding_bindings_v2 binding
      JOIN public.embedding_revisions_v2 revision
        ON revision.revision_sha256=binding.embedding_revision_sha256
       AND revision.support_state='supported'
     WHERE binding.product_version_id=intent_value.product_version_id
       AND binding.embedding_revision_sha256=intent_value.embedding_revision_sha256
     FOR SHARE OF binding,revision;
    IF NOT FOUND THEN
        UPDATE public.knowledge_semantic_index_intents_v2
           SET status='terminal',last_error_code='EMBEDDING_REVISION_UNSUPPORTED',
               last_error_detail='immutable embedding binding or revision is unsupported'
         WHERE id=p_intent_id;
        RETURN 'terminal';
    END IF;

    PERFORM document.id FROM public.documents document
     WHERE document.product_version_id=intent_value.product_version_id
     ORDER BY document.id FOR UPDATE;
    PERFORM chunk.id FROM public.chunks chunk
     WHERE chunk.product_version_id=intent_value.product_version_id
     ORDER BY chunk.id FOR UPDATE;
    PERFORM pending.id FROM public.task_pending_ops pending
     WHERE pending.scope='product_version'
       AND pending.scope_id=intent_value.product_version_id
     ORDER BY pending.id FOR UPDATE;
    IF public.kb_knowledge_has_pending_derived_v2(intent_value.product_version_id) THEN
        RETURN 'pending_derived';
    END IF;

    SELECT source_snapshot_sha256 INTO actual_snapshot
      FROM public.kb_knowledge_source_snapshot_v2(
        intent_value.product_version_id,intent_value.embedding_revision_sha256);
    IF actual_snapshot IS DISTINCT FROM intent_value.source_snapshot_sha256 THEN
        UPDATE public.knowledge_semantic_index_intents_v2
           SET status='superseded',last_error_code='SOURCE_GENERATION_SUPERSEDED',
               last_error_detail='source snapshot changed before provider access'
         WHERE id=p_intent_id;
        RETURN 'superseded';
    END IF;
    RETURN 'current';
END
$$;
REVOKE ALL ON FUNCTION kb_knowledge_preflight_semantic_index_intent_v2(uuid,bigint) FROM PUBLIC;

CREATE FUNCTION kb_knowledge_rebuild_semantic_keyword_indexes_v2(
    p_product_version_id uuid,
    p_embedding_revision_sha256 text,
    p_source_snapshot_sha256 text)
RETURNS bigint
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public, pg_temp
AS $$
DECLARE
    actual_snapshot text;
    actual_count bigint;
BEGIN
    PERFORM 1 FROM public.product_versions version
     WHERE version.id=p_product_version_id AND version.deleted_at IS NULL
     FOR UPDATE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'KNOWLEDGE_SEMANTIC_INDEX_V2_NOT_SUPPORTED' USING ERRCODE='23514';
    END IF;
    PERFORM 1 FROM public.product_version_embedding_bindings_v2 binding
      JOIN public.embedding_revisions_v2 revision
        ON revision.revision_sha256=binding.embedding_revision_sha256
       AND revision.support_state='supported'
     WHERE binding.product_version_id=p_product_version_id
       AND binding.embedding_revision_sha256=p_embedding_revision_sha256
     FOR SHARE OF binding,revision;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'KNOWLEDGE_SEMANTIC_INDEX_V2_NOT_SUPPORTED' USING ERRCODE='23514';
    END IF;
    PERFORM document.id FROM public.documents document
     WHERE document.product_version_id=p_product_version_id
     ORDER BY document.id FOR UPDATE;
    PERFORM chunk.id FROM public.chunks chunk
     WHERE chunk.product_version_id=p_product_version_id
     ORDER BY chunk.id FOR UPDATE;
    PERFORM pending.id FROM public.task_pending_ops pending
     WHERE pending.scope='product_version' AND pending.scope_id=p_product_version_id
     ORDER BY pending.id FOR UPDATE;
    IF public.kb_knowledge_has_pending_derived_v2(p_product_version_id) THEN
        RAISE EXCEPTION 'KNOWLEDGE_SEMANTIC_INDEX_V2_PENDING_DERIVED' USING ERRCODE='40001';
    END IF;
    SELECT source_snapshot_sha256,chunk_count INTO actual_snapshot,actual_count
      FROM public.kb_knowledge_source_snapshot_v2(
        p_product_version_id,p_embedding_revision_sha256);
    IF actual_snapshot IS DISTINCT FROM p_source_snapshot_sha256 THEN
        RAISE EXCEPTION 'KNOWLEDGE_SEMANTIC_INDEX_V2_SNAPSHOT_CHANGED' USING ERRCODE='40001';
    END IF;

    PERFORM public.kb_knowledge_rebuild_keyword_indexes_v2(p_product_version_id);
    IF EXISTS(
        SELECT 1 FROM public.chunks chunk
        JOIN public.documents document ON document.id=chunk.document_id
         AND document.product_version_id=chunk.product_version_id
        LEFT JOIN public.chunk_keyword_indexes_v2 keyword_index
          ON keyword_index.chunk_id=chunk.id
         AND keyword_index.tokenizer='latin-numeric-cjk-bigram'
         AND keyword_index.tokenizer_version='v1'
       WHERE chunk.product_version_id=p_product_version_id
         AND document.deleted_at IS NULL
         AND document.enable_status='enabled'
         AND document.index_ready
         AND chunk.chunk_type=ANY(ARRAY[
           'text','parent_text','image_ocr','question','summary','image_caption','graph_node','wiki_page'])
         AND (keyword_index.chunk_id IS NULL
              OR keyword_index.indexed_content IS DISTINCT FROM chunk.content
              OR keyword_index.indexed_content_sha256 IS DISTINCT FROM
                 encode(public.digest(convert_to(chunk.content,'UTF8'),'sha256'),'hex')))
    THEN
        RAISE EXCEPTION 'KNOWLEDGE_SEMANTIC_INDEX_V2_KEYWORD_INCOMPLETE' USING ERRCODE='23514';
    END IF;

    INSERT INTO public.product_version_keyword_index_generations_v2(
        product_version_id,embedding_revision_sha256,source_snapshot_sha256,
        chunk_count,completed_at)
    VALUES(p_product_version_id,p_embedding_revision_sha256,
           p_source_snapshot_sha256,actual_count,clock_timestamp())
    ON CONFLICT(product_version_id) DO UPDATE SET
        embedding_revision_sha256=EXCLUDED.embedding_revision_sha256,
        source_snapshot_sha256=EXCLUDED.source_snapshot_sha256,
        chunk_count=EXCLUDED.chunk_count,
        completed_at=EXCLUDED.completed_at;
    RETURN actual_count;
END
$$;
REVOKE ALL ON FUNCTION kb_knowledge_rebuild_semantic_keyword_indexes_v2(uuid,text,text) FROM PUBLIC;

CREATE FUNCTION kb_knowledge_complete_semantic_index_intent_v2(
    p_intent_id uuid,p_target_revision bigint)
RETURNS text
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public, pg_temp
AS $$
DECLARE
    intent_value public.knowledge_semantic_index_intents_v2%ROWTYPE;
    actual_snapshot text;
    actual_count bigint;
BEGIN
    SELECT * INTO intent_value
      FROM public.knowledge_semantic_index_intents_v2 intent
     WHERE intent.id=p_intent_id AND intent.target_revision=p_target_revision
     FOR UPDATE;
    IF NOT FOUND THEN
        RETURN 'duplicate';
    END IF;
    IF intent_value.status='completed' THEN
        RETURN 'duplicate';
    END IF;
    IF intent_value.status<>'pending' THEN
        RETURN intent_value.status;
    END IF;

    PERFORM 1 FROM public.product_versions version
     WHERE version.id=intent_value.product_version_id AND version.deleted_at IS NULL
     FOR UPDATE;
    IF NOT FOUND THEN
        UPDATE public.knowledge_semantic_index_intents_v2
           SET status='superseded',last_error_code='SOURCE_GENERATION_SUPERSEDED',
               last_error_detail='product version is no longer live'
         WHERE id=p_intent_id;
        RETURN 'superseded';
    END IF;
    PERFORM 1 FROM public.product_version_embedding_bindings_v2 binding
      JOIN public.embedding_revisions_v2 revision
        ON revision.revision_sha256=binding.embedding_revision_sha256
       AND revision.support_state='supported'
     WHERE binding.product_version_id=intent_value.product_version_id
       AND binding.embedding_revision_sha256=intent_value.embedding_revision_sha256
     FOR SHARE OF binding,revision;
    IF NOT FOUND THEN
        UPDATE public.knowledge_semantic_index_intents_v2
           SET status='terminal',last_error_code='EMBEDDING_REVISION_UNSUPPORTED',
               last_error_detail='immutable embedding binding or revision is unsupported'
         WHERE id=p_intent_id;
        RETURN 'terminal';
    END IF;
    PERFORM document.id FROM public.documents document
     WHERE document.product_version_id=intent_value.product_version_id
     ORDER BY document.id FOR UPDATE;
    PERFORM chunk.id FROM public.chunks chunk
     WHERE chunk.product_version_id=intent_value.product_version_id
     ORDER BY chunk.id FOR UPDATE;
    PERFORM pending.id FROM public.task_pending_ops pending
     WHERE pending.scope='product_version'
       AND pending.scope_id=intent_value.product_version_id
     ORDER BY pending.id FOR UPDATE;
    IF public.kb_knowledge_has_pending_derived_v2(intent_value.product_version_id) THEN
        RETURN 'pending_derived';
    END IF;
    SELECT source_snapshot_sha256,chunk_count INTO actual_snapshot,actual_count
      FROM public.kb_knowledge_source_snapshot_v2(
        intent_value.product_version_id,intent_value.embedding_revision_sha256);
    IF actual_snapshot IS DISTINCT FROM intent_value.source_snapshot_sha256 THEN
        UPDATE public.knowledge_semantic_index_intents_v2
           SET status='superseded',last_error_code='SOURCE_GENERATION_SUPERSEDED',
               last_error_detail='source snapshot changed before readiness publication'
         WHERE id=p_intent_id;
        RETURN 'superseded';
    END IF;
    IF NOT EXISTS(
        SELECT 1 FROM public.product_version_keyword_index_generations_v2 keyword_generation
         WHERE keyword_generation.product_version_id=intent_value.product_version_id
           AND keyword_generation.embedding_revision_sha256=intent_value.embedding_revision_sha256
           AND keyword_generation.source_snapshot_sha256=intent_value.source_snapshot_sha256
           AND keyword_generation.chunk_count=actual_count)
       OR NOT EXISTS(
        SELECT 1 FROM public.product_version_vector_index_generations_v2 vector_generation
         WHERE vector_generation.product_version_id=intent_value.product_version_id
           AND vector_generation.embedding_revision_sha256=intent_value.embedding_revision_sha256
           AND vector_generation.source_snapshot_sha256=intent_value.source_snapshot_sha256
           AND vector_generation.chunk_count=actual_count)
    THEN
        RETURN 'not_ready';
    END IF;

    UPDATE public.knowledge_semantic_index_intents_v2
       SET status='completed',generation_marker_sha256=source_snapshot_sha256,
           last_error_code=NULL,last_error_detail=NULL,completed_at=clock_timestamp()
     WHERE id=p_intent_id AND status='pending';
    RETURN CASE WHEN FOUND THEN 'completed' ELSE 'duplicate' END;
END
$$;
REVOKE ALL ON FUNCTION kb_knowledge_complete_semantic_index_intent_v2(uuid,bigint) FROM PUBLIC;

CREATE FUNCTION kb_knowledge_record_semantic_index_intent_v2(
    p_intent_id uuid,p_target_revision bigint,p_disposition text,
    p_error_code text,p_error_detail text)
RETURNS boolean
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public, pg_temp
AS $$
BEGIN
    IF p_disposition NOT IN ('retryable','terminal','superseded')
       OR p_error_code IS NULL OR octet_length(p_error_code) NOT BETWEEN 1 AND 96
       OR p_error_detail IS NULL OR octet_length(p_error_detail) NOT BETWEEN 1 AND 512 THEN
        RAISE EXCEPTION 'KNOWLEDGE_SEMANTIC_INDEX_V2_INVALID_STATUS' USING ERRCODE='23514';
    END IF;
    UPDATE public.knowledge_semantic_index_intents_v2
       SET status=CASE p_disposition
                    WHEN 'terminal' THEN 'terminal'
                    WHEN 'superseded' THEN 'superseded'
                    ELSE status
                  END,
           last_error_code=p_error_code,last_error_detail=p_error_detail
     WHERE id=p_intent_id AND target_revision=p_target_revision AND status='pending';
    RETURN FOUND;
END
$$;
REVOKE ALL ON FUNCTION kb_knowledge_record_semantic_index_intent_v2(uuid,bigint,text,text,text) FROM PUBLIC;

CREATE FUNCTION kb_knowledge_normalize_matching_text_v2(value text)
RETURNS text
LANGUAGE sql
IMMUTABLE
STRICT
SECURITY DEFINER
SET search_path = pg_catalog, public, pg_temp
AS $$
    SELECT lower(regexp_replace(value,'[[:space:]]','','g'))
$$;
REVOKE ALL ON FUNCTION kb_knowledge_normalize_matching_text_v2(text) FROM PUBLIC;

-- V2 is an independent, not-yet-routed contract. It replays the complete V1
-- eligible product scope while additionally binding trusted source hits to the
-- canonical retrieval policy and its quotas.
CREATE FUNCTION kb_knowledge_attest_matching_scope_v2(p_scope jsonb)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public, pg_temp
AS $$
DECLARE
    p_products jsonb;
    p_hits jsonb;
    p_requirements jsonb;
    p_version_selections jsonb;
    p_policy jsonb;
    p_workspace_kinds text[];
    p_product_line_versions uuid[];
    p_company_versions uuid[];
    v_max_hits bigint;
    v_max_chunk_bytes bigint;
    v_max_total_bytes bigint;
    attestation_id uuid := gen_random_uuid();
    canonical_payload bytea;
    content_sha256 text;
BEGIN
    -- Table locks make each validation statement observe one post-lock state
    -- only at READ COMMITTED. A pre-established REPEATABLE READ or SERIALIZABLE
    -- snapshot would remain stale after the locks are acquired, so fail closed.
    IF current_setting('transaction_isolation') IS DISTINCT FROM 'read committed' THEN
        RAISE EXCEPTION 'KNOWLEDGE_MATCHING_SCOPE_V2_INVALID' USING ERRCODE='23514';
    END IF;
    IF p_scope IS NULL OR jsonb_typeof(p_scope) IS DISTINCT FROM 'object' THEN
        RAISE EXCEPTION 'KNOWLEDGE_MATCHING_SCOPE_V2_INVALID' USING ERRCODE='23514';
    END IF;
    IF (SELECT array_agg(key ORDER BY key) FROM jsonb_object_keys(p_scope) key)
         IS DISTINCT FROM ARRAY[
             'frozen_hits','products','retrieval_policy','retrieval_requirements',
             'schema_version','version_selections','workspace_kinds']::text[]
       OR p_scope->'schema_version' IS DISTINCT FROM '2'::jsonb
       OR jsonb_typeof(p_scope->'products') IS DISTINCT FROM 'array'
       OR jsonb_typeof(p_scope->'frozen_hits') IS DISTINCT FROM 'array'
       OR jsonb_typeof(p_scope->'retrieval_requirements') IS DISTINCT FROM 'array'
       OR jsonb_typeof(p_scope->'version_selections') IS DISTINCT FROM 'object'
       OR jsonb_typeof(p_scope->'workspace_kinds') IS DISTINCT FROM 'array'
       OR jsonb_typeof(p_scope->'retrieval_policy') IS DISTINCT FROM 'object' THEN
        RAISE EXCEPTION 'KNOWLEDGE_MATCHING_SCOPE_V2_INVALID' USING ERRCODE='23514';
    END IF;

    p_products := p_scope->'products';
    p_hits := p_scope->'frozen_hits';
    p_requirements := p_scope->'retrieval_requirements';
    p_version_selections := p_scope->'version_selections';
    p_policy := p_scope->'retrieval_policy';

    IF (SELECT array_agg(key ORDER BY key) FROM jsonb_object_keys(p_policy) key)
          IS DISTINCT FROM ARRAY[
              'contract_version','max_chunk_bytes','max_hits','max_total_bytes',
              'policy_sha256']::text[]
       OR p_policy->>'contract_version' IS DISTINCT FROM 'knowledge-evidence-v2'
       OR jsonb_typeof(p_policy->'contract_version') IS DISTINCT FROM 'string'
       OR jsonb_typeof(p_policy->'policy_sha256') IS DISTINCT FROM 'string'
       OR p_policy->>'policy_sha256' !~ '^[0-9a-f]{64}$'
       OR jsonb_typeof(p_policy->'max_hits') IS DISTINCT FROM 'number'
       OR jsonb_typeof(p_policy->'max_chunk_bytes') IS DISTINCT FROM 'number'
       OR jsonb_typeof(p_policy->'max_total_bytes') IS DISTINCT FROM 'number'
       OR p_policy->>'max_hits' !~ '^[1-9][0-9]*$'
       OR length(p_policy->>'max_hits')>7
       OR p_policy->>'max_chunk_bytes' !~ '^[1-9][0-9]*$'
       OR length(p_policy->>'max_chunk_bytes')>10
       OR p_policy->>'max_total_bytes' !~ '^[1-9][0-9]*$'
       OR length(p_policy->>'max_total_bytes')>13 THEN
        RAISE EXCEPTION 'KNOWLEDGE_MATCHING_RETRIEVAL_POLICY_V2_INVALID' USING ERRCODE='23514';
    END IF;
    v_max_hits := (p_policy->>'max_hits')::bigint;
    v_max_chunk_bytes := (p_policy->>'max_chunk_bytes')::bigint;
    v_max_total_bytes := (p_policy->>'max_total_bytes')::bigint;
    IF v_max_hits > 1000000
       OR v_max_chunk_bytes > 1073741824
       OR v_max_total_bytes > 1099511627776 THEN
        RAISE EXCEPTION 'KNOWLEDGE_MATCHING_RETRIEVAL_POLICY_V2_INVALID' USING ERRCODE='23514';
    END IF;
    PERFORM 1
      FROM knowledge_retrieval_policies_v2 policy
      JOIN public.embedding_revisions_v2 revision
        ON revision.revision_sha256=policy.embedding_revision_sha256
       AND revision.support_state='supported'
      JOIN public.rerank_revisions_v2 reranker
        ON reranker.revision_sha256=policy.rerank_revision_sha256
       AND reranker.support_state='supported'
     WHERE policy.policy_sha256=p_policy->>'policy_sha256'
       AND policy.support_state='supported'
       AND policy.contract_version='knowledge-evidence-v2'
       AND policy.contract_version=p_policy->>'contract_version'
       AND kb_knowledge_valid_retrieval_policy_v2(
           policy.canonical_policy_payload,policy.embedding_revision_sha256,
           policy.contract_version,policy.max_hits,policy.max_chunk_bytes,
           policy.max_total_bytes)
       AND revision.revision_sha256=policy.embedding_revision_sha256
       AND reranker.revision_sha256=policy.rerank_revision_sha256
       AND policy.max_hits=v_max_hits
       AND policy.max_chunk_bytes=v_max_chunk_bytes
       AND policy.max_total_bytes=v_max_total_bytes
     FOR SHARE OF policy,revision,reranker;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'KNOWLEDGE_MATCHING_RETRIEVAL_POLICY_V2_INVALID' USING ERRCODE='23514';
    END IF;

    -- A single deterministic lock order prevents a READ COMMITTED caller from
    -- attesting product scope from one state and document/chunk bytes from a
    -- later state. Attestation is schedule-time and intentionally favors a
    -- mutation-free proof over write concurrency.
    LOCK TABLE public.workspaces,public.products,public.product_versions,
               public.documents,public.chunks IN SHARE MODE;

    IF EXISTS (
        SELECT 1 FROM jsonb_array_elements(p_requirements) requirement_value
         WHERE jsonb_typeof(requirement_value) IS DISTINCT FROM 'object'
            OR (SELECT array_agg(key ORDER BY key)
                  FROM jsonb_object_keys(CASE WHEN jsonb_typeof(requirement_value)='object'
                                              THEN requirement_value ELSE '{}'::jsonb END) key)
               IS DISTINCT FROM ARRAY[
                   'exact_prefix_hit_count','requirement_artifact_id',
                   'requirement_identity_sha256','requirement_text','route_id']::text[]
            OR jsonb_typeof(requirement_value->'route_id') IS DISTINCT FROM 'string'
            OR requirement_value->>'route_id' !~* '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
            OR jsonb_typeof(requirement_value->'requirement_artifact_id') IS DISTINCT FROM 'string'
            OR requirement_value->>'requirement_artifact_id' !~* '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
            OR jsonb_typeof(requirement_value->'requirement_identity_sha256') IS DISTINCT FROM 'string'
            OR requirement_value->>'requirement_identity_sha256' !~ '^[0-9a-f]{64}$'
            OR jsonb_typeof(requirement_value->'requirement_text') IS DISTINCT FROM 'string'
            OR kb_knowledge_normalize_matching_text_v2(requirement_value->>'requirement_text')=''
            OR requirement_value->>'requirement_identity_sha256' IS DISTINCT FROM
               encode(public.digest(convert_to(requirement_value->>'requirement_text','UTF8'),'sha256'),'hex')
            OR jsonb_typeof(requirement_value->'exact_prefix_hit_count') IS DISTINCT FROM 'number'
            OR requirement_value->>'exact_prefix_hit_count' !~ '^(0|[1-9][0-9]*)$'
            OR length(requirement_value->>'exact_prefix_hit_count')>7)
       OR EXISTS (
        SELECT 1 FROM jsonb_array_elements(p_requirements) requirement_value
         GROUP BY requirement_value->>'route_id',requirement_value->>'requirement_artifact_id'
        HAVING count(*)<>1) THEN
        RAISE EXCEPTION 'KNOWLEDGE_MATCHING_REQUIREMENT_V2_INVALID' USING ERRCODE='23514';
    END IF;

    IF (SELECT array_agg(key ORDER BY key) FROM jsonb_object_keys(p_version_selections) key)
          IS DISTINCT FROM ARRAY['company','product_line']::text[]
       OR jsonb_typeof(p_version_selections->'product_line') IS DISTINCT FROM 'array'
       OR jsonb_typeof(p_version_selections->'company') IS DISTINCT FROM 'array'
       OR EXISTS (
           SELECT 1 FROM jsonb_array_elements(p_scope->'workspace_kinds') kind
            WHERE jsonb_typeof(kind) IS DISTINCT FROM 'string'
               OR kind#>>'{}' NOT IN ('product_line', 'company')) THEN
        RAISE EXCEPTION 'KNOWLEDGE_MATCHING_SCOPE_V2_INVALID' USING ERRCODE='23514';
    END IF;
    p_workspace_kinds := ARRAY(
        SELECT jsonb_array_elements_text(p_scope->'workspace_kinds') ORDER BY 1);
    IF cardinality(p_workspace_kinds)
          <> (SELECT count(DISTINCT kind) FROM unnest(p_workspace_kinds) kind)
       OR EXISTS (
           SELECT 1 FROM jsonb_array_elements(p_version_selections->'product_line') selection
            WHERE jsonb_typeof(selection) IS DISTINCT FROM 'string'
               OR selection#>>'{}' !~* '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$')
       OR EXISTS (
           SELECT 1 FROM jsonb_array_elements(p_version_selections->'company') selection
            WHERE jsonb_typeof(selection) IS DISTINCT FROM 'string'
               OR selection#>>'{}' !~* '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$') THEN
        RAISE EXCEPTION 'KNOWLEDGE_MATCHING_SCOPE_V2_INVALID' USING ERRCODE='23514';
    END IF;
    SELECT COALESCE(array_agg(selection::uuid ORDER BY ordinal),'{}'::uuid[])
      INTO p_product_line_versions
      FROM jsonb_array_elements_text(p_version_selections->'product_line')
           WITH ORDINALITY selected(selection,ordinal);
    SELECT COALESCE(array_agg(selection::uuid ORDER BY ordinal),'{}'::uuid[])
      INTO p_company_versions
      FROM jsonb_array_elements_text(p_version_selections->'company')
           WITH ORDINALITY selected(selection,ordinal);
    IF p_product_line_versions IS DISTINCT FROM ARRAY(
           SELECT DISTINCT version_id FROM unnest(p_product_line_versions) version_id ORDER BY version_id)
       OR p_company_versions IS DISTINCT FROM ARRAY(
           SELECT DISTINCT version_id FROM unnest(p_company_versions) version_id ORDER BY version_id)
       OR (NOT ('product_line'=ANY(p_workspace_kinds)) AND cardinality(p_product_line_versions)>0)
       OR (NOT ('company'=ANY(p_workspace_kinds)) AND cardinality(p_company_versions)>0) THEN
        RAISE EXCEPTION 'KNOWLEDGE_MATCHING_SCOPE_V2_INVALID' USING ERRCODE='23514';
    END IF;

    IF EXISTS (
        SELECT 1 FROM jsonb_array_elements(p_products) artifact
         WHERE jsonb_typeof(artifact) IS DISTINCT FROM 'object'
            OR (SELECT array_agg(key ORDER BY key)
                  FROM jsonb_object_keys(CASE WHEN jsonb_typeof(artifact)='object'
                                              THEN artifact ELSE '{}'::jsonb END) key)
               IS DISTINCT FROM ARRAY[
                   'frozen_display_name','id','identity_sha256','product_id',
                   'product_version_id','workspace_kind']::text[]
            OR jsonb_typeof(artifact->'id') IS DISTINCT FROM 'string'
            OR artifact->>'id' !~* '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
            OR jsonb_typeof(artifact->'product_id') IS DISTINCT FROM 'string'
            OR artifact->>'product_id' !~* '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
            OR jsonb_typeof(artifact->'product_version_id') IS DISTINCT FROM 'string'
            OR artifact->>'product_version_id' !~* '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
            OR jsonb_typeof(artifact->'workspace_kind') IS DISTINCT FROM 'string'
            OR artifact->>'workspace_kind' NOT IN ('product_line','company')
            OR jsonb_typeof(artifact->'frozen_display_name') IS DISTINCT FROM 'string'
            OR jsonb_typeof(artifact->'identity_sha256') IS DISTINCT FROM 'string'
            OR artifact->>'identity_sha256' !~ '^[0-9a-f]{64}$') THEN
        RAISE EXCEPTION 'KNOWLEDGE_MATCHING_SCOPE_V2_INVALID' USING ERRCODE='23514';
    END IF;

    IF EXISTS (
        SELECT 1
          FROM (
              SELECT 'product_line'::text AS kind, version_id
                FROM unnest(p_product_line_versions) version_id
              UNION ALL
              SELECT 'company'::text AS kind, version_id
                FROM unnest(p_company_versions) version_id
          ) selection
          LEFT JOIN product_versions version_value ON version_value.id=selection.version_id
          LEFT JOIN products product ON product.id=version_value.product_id
          LEFT JOIN workspaces workspace_value ON workspace_value.id=product.workspace_id
         WHERE product.id IS NULL
            OR workspace_value.kind IS DISTINCT FROM selection.kind
            OR (selection.kind='product_line' AND product.kind IS DISTINCT FROM 'product')
            OR (selection.kind='company' AND product.kind IS DISTINCT FROM 'library')
            OR version_value.status IS DISTINCT FROM 'active'
            OR version_value.deleted_at IS NOT NULL
            OR product.current_version_id IS DISTINCT FROM version_value.id) THEN
        RAISE EXCEPTION 'KNOWLEDGE_MATCHING_SCOPE_V2_MISMATCH' USING ERRCODE='23514';
    END IF;

    IF EXISTS (
        SELECT 1
          FROM jsonb_array_elements(p_products) artifact
          LEFT JOIN product_versions version_value
            ON version_value.id=(artifact->>'product_version_id')::uuid
          LEFT JOIN products product ON product.id=version_value.product_id
          LEFT JOIN workspaces workspace_value ON workspace_value.id=product.workspace_id
         WHERE product.id IS NULL
            OR (artifact->>'product_id')::uuid IS DISTINCT FROM product.id
            OR artifact->>'workspace_kind' IS DISTINCT FROM workspace_value.kind
            OR NOT (workspace_value.kind=ANY(p_workspace_kinds))
            OR (workspace_value.kind='product_line' AND product.kind IS DISTINCT FROM 'product')
            OR (workspace_value.kind='company' AND product.kind IS DISTINCT FROM 'library')
            OR (workspace_value.kind='product_line' AND cardinality(p_product_line_versions)>0
                AND NOT (version_value.id=ANY(p_product_line_versions)))
            OR (workspace_value.kind='company' AND cardinality(p_company_versions)>0
                AND NOT (version_value.id=ANY(p_company_versions)))
            OR version_value.status IS DISTINCT FROM 'active'
            OR version_value.deleted_at IS NOT NULL
            OR product.current_version_id IS DISTINCT FROM version_value.id
            OR artifact->>'frozen_display_name' IS DISTINCT FROM version_value.id::text
            OR artifact->>'identity_sha256' IS DISTINCT FROM encode(public.digest(convert_to(
                'ProductVersionEvidenceV1:'||product.id::text||':'||version_value.id::text||':'
                ||workspace_value.kind,'UTF8'),'sha256'),'hex'))
       OR EXISTS (
          SELECT 1 FROM jsonb_array_elements(p_products) artifact
           GROUP BY artifact->>'id' HAVING count(*)<>1)
       OR EXISTS (
          SELECT 1 FROM jsonb_array_elements(p_products) artifact
           GROUP BY artifact->>'product_version_id' HAVING count(*)<>1)
       OR EXISTS (
        SELECT 1
          FROM workspaces workspace_value
          JOIN products product ON product.workspace_id=workspace_value.id
          JOIN product_versions version_value
            ON version_value.product_id=product.id
           AND product.current_version_id=version_value.id
         WHERE workspace_value.kind=ANY(p_workspace_kinds)
           AND version_value.status='active'
           AND version_value.deleted_at IS NULL
           AND ((workspace_value.kind='product_line' AND product.kind='product')
             OR (workspace_value.kind='company' AND product.kind='library'))
           AND ((workspace_value.kind='product_line'
                 AND (cardinality(p_product_line_versions)=0
                      OR version_value.id=ANY(p_product_line_versions)))
             OR (workspace_value.kind='company'
                 AND (cardinality(p_company_versions)=0
                      OR version_value.id=ANY(p_company_versions))))
           AND NOT EXISTS (
               SELECT 1 FROM jsonb_array_elements(p_products) artifact
                WHERE (artifact->>'product_id')::uuid=product.id
                  AND (artifact->>'product_version_id')::uuid=version_value.id
                  AND artifact->>'workspace_kind'=workspace_value.kind)) THEN
        RAISE EXCEPTION 'KNOWLEDGE_MATCHING_SCOPE_V2_MISMATCH' USING ERRCODE='23514';
    END IF;

    IF EXISTS (
        SELECT 1 FROM jsonb_array_elements(p_hits) hit
         WHERE jsonb_typeof(hit) IS DISTINCT FROM 'object'
            OR (SELECT array_agg(key ORDER BY key)
                  FROM jsonb_object_keys(CASE WHEN jsonb_typeof(hit)='object'
                                              THEN hit ELSE '{}'::jsonb END) key)
               IS DISTINCT FROM ARRAY[
                   'chunk_byte_length','chunk_sha256','chunk_utf8','document_id',
                   'frozen_document_display_name','id','media','offset_unit','pre_rerank_rrf_rank',
                   'product_version_artifact_id','quote_end_offset','quote_start_offset',
                   'requirement_artifact_id','retrieval_contract_version','retrieval_rank',
                   'retrieval_raw_score','route_id','source_chunk_id','source_type']::text[]
            OR jsonb_typeof(hit->'id') IS DISTINCT FROM 'string'
            OR hit->>'id' !~* '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
            OR jsonb_typeof(hit->'route_id') IS DISTINCT FROM 'string'
            OR hit->>'route_id' !~* '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
            OR jsonb_typeof(hit->'requirement_artifact_id') IS DISTINCT FROM 'string'
            OR hit->>'requirement_artifact_id' !~* '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
            OR jsonb_typeof(hit->'product_version_artifact_id') IS DISTINCT FROM 'string'
            OR hit->>'product_version_artifact_id' !~* '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
            OR jsonb_typeof(hit->'document_id') IS DISTINCT FROM 'string'
            OR hit->>'document_id' !~* '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
            OR jsonb_typeof(hit->'source_chunk_id') IS DISTINCT FROM 'string'
            OR hit->>'source_chunk_id' !~* '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
            OR jsonb_typeof(hit->'frozen_document_display_name') IS DISTINCT FROM 'string'
            OR jsonb_typeof(hit->'chunk_utf8') IS DISTINCT FROM 'string'
            OR jsonb_typeof(hit->'chunk_sha256') IS DISTINCT FROM 'string'
            OR hit->>'chunk_sha256' !~ '^[0-9a-f]{64}$'
            OR jsonb_typeof(hit->'source_type') IS DISTINCT FROM 'string'
            OR hit->>'source_type' NOT IN ('text','parent_text','image_ocr')
            OR (hit->>'source_type'='image_ocr' AND (
              jsonb_typeof(hit->'media') IS DISTINCT FROM 'object'
              OR (SELECT array_agg(key ORDER BY key) FROM jsonb_object_keys(hit->'media') key)
                IS DISTINCT FROM ARRAY['bounding_region','frozen_document_display_name','height','image_artifact_revision_id','media_type','object_ref','page_ordinal','sha256','width']::text[]
              OR (hit#>>'{media,image_artifact_revision_id}') !~* '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
              OR hit#>>'{media,object_ref}' IS DISTINCT FROM 'objects/'||(hit#>>'{media,sha256}')
              OR (hit#>>'{media,sha256}') !~ '^[0-9a-f]{64}$'
              OR hit#>>'{media,media_type}' NOT IN ('image/png','image/jpeg','image/webp')
              OR jsonb_typeof(hit#>'{media,width}') IS DISTINCT FROM 'number' OR (hit#>>'{media,width}') !~ '^[1-9][0-9]*$'
              OR jsonb_typeof(hit#>'{media,height}') IS DISTINCT FROM 'number' OR (hit#>>'{media,height}') !~ '^[1-9][0-9]*$'
              OR jsonb_typeof(hit#>'{media,page_ordinal}') NOT IN ('null','number')
              OR (jsonb_typeof(hit#>'{media,page_ordinal}')='number' AND (hit#>>'{media,page_ordinal}') !~ '^(0|[1-9][0-9]*)$')
              OR jsonb_typeof(hit#>'{media,bounding_region}') NOT IN ('null','object')
              OR hit#>>'{media,frozen_document_display_name}' IS DISTINCT FROM hit->>'frozen_document_display_name'))
            OR (hit->>'source_type'<>'image_ocr' AND jsonb_typeof(hit->'media') IS DISTINCT FROM 'null')
            OR jsonb_typeof(hit->'retrieval_contract_version') IS DISTINCT FROM 'string'
            OR hit->>'retrieval_contract_version' IS DISTINCT FROM 'knowledge-evidence-v2'
            OR jsonb_typeof(hit->'offset_unit') IS DISTINCT FROM 'string'
            OR hit->>'offset_unit' IS DISTINCT FROM 'utf8_byte'
            OR jsonb_typeof(hit->'chunk_byte_length') IS DISTINCT FROM 'number'
            OR hit->>'chunk_byte_length' !~ '^(0|[1-9][0-9]*)$'
            OR length(hit->>'chunk_byte_length')>10
            OR jsonb_typeof(hit->'quote_start_offset') IS DISTINCT FROM 'number'
            OR hit->>'quote_start_offset' IS DISTINCT FROM '0'
            OR jsonb_typeof(hit->'quote_end_offset') IS DISTINCT FROM 'number'
            OR hit->>'quote_end_offset' !~ '^(0|[1-9][0-9]*)$'
            OR length(hit->>'quote_end_offset')>10
            OR jsonb_typeof(hit->'retrieval_rank') IS DISTINCT FROM 'number'
            OR hit->>'retrieval_rank' !~ '^[1-9][0-9]*$'
            OR length(hit->>'retrieval_rank')>10
            OR jsonb_typeof(hit->'retrieval_raw_score') IS DISTINCT FROM 'string'
            OR hit->>'retrieval_raw_score' !~ '^(0\.[0-9]{6}|1\.000000)$'
            OR jsonb_typeof(hit->'pre_rerank_rrf_rank') NOT IN ('null','number')
            OR (jsonb_typeof(hit->'pre_rerank_rrf_rank')='number'
                AND (hit->>'pre_rerank_rrf_rank' !~ '^[1-9][0-9]*$'
                     OR length(hit->>'pre_rerank_rrf_rank')>10))) THEN
        RAISE EXCEPTION 'KNOWLEDGE_MATCHING_HIT_V2_INVALID' USING ERRCODE='23514';
    END IF;

    IF EXISTS (
        SELECT 1
          FROM jsonb_array_elements(p_hits) hit
          LEFT JOIN LATERAL (
              SELECT artifact
                FROM jsonb_array_elements(p_products) artifact
               WHERE artifact->>'id'=hit->>'product_version_artifact_id'
          ) artifact_value ON true
          LEFT JOIN documents document_value ON document_value.id=(hit->>'document_id')::uuid
          LEFT JOIN chunks chunk_value ON chunk_value.id=(hit->>'source_chunk_id')::uuid
          LEFT JOIN knowledge_image_ocr_chunk_artifact_mappings media_mapping ON media_mapping.chunk_id=chunk_value.id
          LEFT JOIN knowledge_image_artifact_revisions media_value ON media_value.id=media_mapping.image_artifact_revision_id
         WHERE artifact_value.artifact IS NULL
            OR document_value.product_version_id IS DISTINCT FROM
               (artifact_value.artifact->>'product_version_id')::uuid
            OR chunk_value.product_version_id IS DISTINCT FROM
               (artifact_value.artifact->>'product_version_id')::uuid
            OR chunk_value.document_id IS DISTINCT FROM document_value.id
            OR document_value.deleted_at IS NOT NULL
            OR document_value.enable_status IS DISTINCT FROM 'enabled'
            OR NOT document_value.index_ready
            OR hit->>'frozen_document_display_name' IS DISTINCT FROM document_value.file_name
            OR hit->>'source_type' IS DISTINCT FROM chunk_value.chunk_type
            OR chunk_value.chunk_type NOT IN ('text','parent_text','image_ocr')
            OR hit->>'chunk_utf8' IS DISTINCT FROM chunk_value.content
            OR (hit->>'chunk_byte_length')::bigint IS DISTINCT FROM
               octet_length(convert_to(chunk_value.content,'UTF8'))
            OR hit->>'chunk_sha256' IS DISTINCT FROM encode(public.digest(
               convert_to(chunk_value.content,'UTF8'),'sha256'),'hex')
            OR (hit->>'quote_end_offset')::bigint IS DISTINCT FROM
               octet_length(convert_to(chunk_value.content,'UTF8'))
            OR (hit->>'source_type'='image_ocr' AND (media_value.id IS NULL
               OR (hit#>>'{media,image_artifact_revision_id}')::uuid IS DISTINCT FROM media_value.id
               OR hit#>>'{media,object_ref}' IS DISTINCT FROM media_value.object_ref
               OR hit#>>'{media,sha256}' IS DISTINCT FROM media_value.content_sha256
               OR hit#>>'{media,media_type}' IS DISTINCT FROM media_value.media_type
               OR (hit#>>'{media,width}')::integer IS DISTINCT FROM media_value.width
               OR (hit#>>'{media,height}')::integer IS DISTINCT FROM media_value.height
               OR CASE WHEN jsonb_typeof(hit#>'{media,page_ordinal}')='null' THEN NULL
                  ELSE (hit#>>'{media,page_ordinal}')::integer END IS DISTINCT FROM media_value.page_ordinal
               OR hit#>'{media,bounding_region}' IS DISTINCT FROM coalesce(media_value.bounding_region,'null'::jsonb)))) THEN
        RAISE EXCEPTION 'KNOWLEDGE_MATCHING_HIT_V2_MISMATCH' USING ERRCODE='23514';
    END IF;

    -- Exact/C membership is explicit per route+requirement and never inferred
    -- from the model score. A legitimate C score of 1.000000 remains C.
    IF EXISTS (
        SELECT 1
          FROM jsonb_array_elements(p_hits) hit
          LEFT JOIN LATERAL (
              SELECT requirement_value
                FROM jsonb_array_elements(p_requirements) requirement_value
               WHERE requirement_value->>'route_id'=hit->>'route_id'
                 AND requirement_value->>'requirement_artifact_id'=
                     hit->>'requirement_artifact_id'
          ) requirement_scope ON true
         WHERE requirement_scope.requirement_value IS NULL
            OR ((hit->>'retrieval_rank')::bigint <=
                (requirement_scope.requirement_value->>'exact_prefix_hit_count')::bigint
                AND (hit->>'retrieval_raw_score'<>'1.000000'
                     OR jsonb_typeof(hit->'pre_rerank_rrf_rank')<>'null'
                     OR position(
                          kb_knowledge_normalize_matching_text_v2(
                              requirement_scope.requirement_value->>'requirement_text')
                          IN kb_knowledge_normalize_matching_text_v2(hit->>'chunk_utf8'))=0))
            OR ((hit->>'retrieval_rank')::bigint >
                (requirement_scope.requirement_value->>'exact_prefix_hit_count')::bigint
                AND (jsonb_typeof(hit->'pre_rerank_rrf_rank')<>'number'
                     OR position(
                          kb_knowledge_normalize_matching_text_v2(
                              requirement_scope.requirement_value->>'requirement_text')
                          IN kb_knowledge_normalize_matching_text_v2(hit->>'chunk_utf8'))>0)))
       OR EXISTS (
        SELECT 1
          FROM jsonb_array_elements(p_requirements) requirement_value
          LEFT JOIN LATERAL (
              SELECT count(*) AS hit_count
                FROM jsonb_array_elements(p_hits) hit
               WHERE hit->>'route_id'=requirement_value->>'route_id'
                 AND hit->>'requirement_artifact_id'=
                     requirement_value->>'requirement_artifact_id'
          ) counts ON true
         WHERE (requirement_value->>'exact_prefix_hit_count')::bigint>counts.hit_count)
       OR EXISTS (
        SELECT 1
          FROM (
              SELECT hit->>'route_id' AS route_id,
                     hit->>'requirement_artifact_id' AS requirement_artifact_id,
                     hit->>'retrieval_raw_score' AS score,
                     (hit->>'pre_rerank_rrf_rank')::bigint AS rrf_rank,
                     lag(hit->>'retrieval_raw_score') OVER (
                         PARTITION BY hit->>'route_id',hit->>'requirement_artifact_id'
                         ORDER BY (hit->>'retrieval_rank')::bigint) AS previous_score,
                     lag((hit->>'pre_rerank_rrf_rank')::bigint) OVER (
                         PARTITION BY hit->>'route_id',hit->>'requirement_artifact_id'
                         ORDER BY (hit->>'retrieval_rank')::bigint) AS previous_rrf_rank
                FROM jsonb_array_elements(p_hits) hit
                JOIN LATERAL (
                    SELECT requirement_value
                      FROM jsonb_array_elements(p_requirements) requirement_value
                     WHERE requirement_value->>'route_id'=hit->>'route_id'
                       AND requirement_value->>'requirement_artifact_id'=
                           hit->>'requirement_artifact_id'
                ) requirement_scope ON true
               WHERE (hit->>'retrieval_rank')::bigint>
                     (requirement_scope.requirement_value->>'exact_prefix_hit_count')::bigint
          ) ordered_suffix
         WHERE previous_score<score
            OR (previous_score=score AND previous_rrf_rank>rrf_rank))
       OR EXISTS (
        SELECT 1 FROM jsonb_array_elements(p_hits) hit
        JOIN LATERAL (
            SELECT requirement_value
              FROM jsonb_array_elements(p_requirements) requirement_value
             WHERE requirement_value->>'route_id'=hit->>'route_id'
               AND requirement_value->>'requirement_artifact_id'=
                   hit->>'requirement_artifact_id'
        ) requirement_scope ON true
         WHERE (hit->>'retrieval_rank')::bigint>
               (requirement_scope.requirement_value->>'exact_prefix_hit_count')::bigint
         GROUP BY hit->>'route_id',hit->>'requirement_artifact_id',
                  hit->>'pre_rerank_rrf_rank'
        HAVING count(*)<>1) THEN
        RAISE EXCEPTION 'KNOWLEDGE_MATCHING_HIT_V2_PROVENANCE_INVALID' USING ERRCODE='23514';
    END IF;

    IF EXISTS (
        SELECT 1 FROM jsonb_array_elements(p_hits) hit
         GROUP BY hit->>'id' HAVING count(*)<>1)
       OR EXISTS (
        SELECT 1 FROM jsonb_array_elements(p_hits) hit
         GROUP BY hit->>'requirement_artifact_id',hit->>'product_version_artifact_id',
                  hit->>'document_id',hit->>'source_chunk_id'
        HAVING count(*)<>1)
       OR EXISTS (
        SELECT 1
          FROM (
              SELECT (hit->>'retrieval_rank')::bigint AS retrieval_rank,
                     row_number() OVER (
                         PARTITION BY hit->>'route_id',hit->>'requirement_artifact_id'
                         ORDER BY (hit->>'retrieval_rank')::bigint,(hit->>'id')::uuid
                     ) AS expected_rank
                FROM jsonb_array_elements(p_hits) hit
          ) ranked
         WHERE ranked.retrieval_rank<>ranked.expected_rank) THEN
        RAISE EXCEPTION 'KNOWLEDGE_MATCHING_HIT_V2_INVALID' USING ERRCODE='23514';
    END IF;

    IF EXISTS (
        SELECT 1
          FROM jsonb_array_elements(p_hits) hit
         GROUP BY hit->>'route_id',hit->>'requirement_artifact_id'
        HAVING count(*)>v_max_hits
            OR max((hit->>'chunk_byte_length')::bigint)>v_max_chunk_bytes
            OR sum((hit->>'chunk_byte_length')::bigint)>v_max_total_bytes) THEN
        RAISE EXCEPTION 'KNOWLEDGE_MATCHING_HIT_V2_QUOTA_EXCEEDED' USING ERRCODE='23514';
    END IF;

    canonical_payload := convert_to(p_scope::text,'UTF8');
    content_sha256 := encode(public.digest(canonical_payload,'sha256'),'hex');
    INSERT INTO knowledge_matching_scope_attestations_v2(
        id,schema_version,canonical_payload,content_sha256)
    VALUES(attestation_id,2,canonical_payload,content_sha256);
    RETURN jsonb_build_object('id',attestation_id,'content_sha256',content_sha256);
END
$$;

CREATE FUNCTION kb_knowledge_verify_matching_scope_v2(
    p_attestation_id uuid,
    p_content_sha256 text,
    p_scope jsonb
)
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public, pg_temp
AS $$
BEGIN
    PERFORM 1
      FROM knowledge_matching_scope_attestations_v2 attestation
     WHERE attestation.schema_version=2
       AND attestation.id=p_attestation_id
       AND attestation.content_sha256=p_content_sha256
       AND p_content_sha256 ~ '^[0-9a-f]{64}$'
       AND attestation.canonical_payload=convert_to(p_scope::text,'UTF8');
    IF NOT FOUND THEN
        RAISE EXCEPTION 'KNOWLEDGE_MATCHING_ATTESTATION_V2_MISMATCH' USING ERRCODE='23514';
    END IF;
END
$$;
REVOKE ALL ON FUNCTION kb_knowledge_attest_matching_scope_v2(jsonb) FROM PUBLIC;
REVOKE ALL ON FUNCTION kb_knowledge_verify_matching_scope_v2(uuid,text,jsonb) FROM PUBLIC;

CREATE FUNCTION kb_knowledge_require_matching_attestation_v2(
  p_attestation_id uuid,p_attestation_sha256 text
) RETURNS void LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
BEGIN
  PERFORM 1 FROM knowledge_matching_scope_attestations_v2
    WHERE id=p_attestation_id AND content_sha256=p_attestation_sha256;
  IF NOT FOUND THEN RAISE EXCEPTION 'KNOWLEDGE_MATCHING_ATTESTATION_V2_MISMATCH' USING ERRCODE='23514'; END IF;
END $$;
REVOKE ALL ON FUNCTION kb_knowledge_require_matching_attestation_v2(uuid,text) FROM PUBLIC;

CREATE FUNCTION kb_knowledge_load_matching_attestation_v2(
  p_attestation_id uuid,p_attestation_sha256 text
) RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE result_value jsonb;
BEGIN
  SELECT convert_from(canonical_payload,'UTF8')::jsonb INTO result_value
    FROM knowledge_matching_scope_attestations_v2
    WHERE id=p_attestation_id AND content_sha256=p_attestation_sha256;
  IF result_value IS NULL THEN RAISE EXCEPTION 'KNOWLEDGE_MATCHING_ATTESTATION_V2_MISMATCH' USING ERRCODE='23514'; END IF;
  RETURN result_value;
END $$;
REVOKE ALL ON FUNCTION kb_knowledge_load_matching_attestation_v2(uuid,text) FROM PUBLIC;

CREATE FUNCTION kb_knowledge_verify_attested_text_hit_v2(
  p_attestation_id uuid,p_attestation_sha256 text,p_requirement_artifact_id uuid,p_item jsonb
) RETURNS void LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
BEGIN
  PERFORM 1 FROM knowledge_matching_scope_attestations_v2 attestation
  CROSS JOIN LATERAL jsonb_array_elements(convert_from(attestation.canonical_payload,'UTF8')::jsonb->'frozen_hits') hit
  CROSS JOIN LATERAL jsonb_array_elements(convert_from(attestation.canonical_payload,'UTF8')::jsonb->'products') product
  WHERE attestation.id=p_attestation_id AND attestation.content_sha256=p_attestation_sha256
    AND (hit->>'requirement_artifact_id')::uuid=p_requirement_artifact_id
    AND hit->>'product_version_artifact_id'=product->>'id'
    AND hit->>'document_id'=p_item->>'document_id' AND hit->>'source_chunk_id'=p_item->>'source_chunk_id'
    AND product->>'product_version_id'=p_item->>'product_version_id'
    AND product->>'workspace_kind'=p_item->>'workspace_kind'
    AND hit->>'frozen_document_display_name'=p_item->>'frozen_document_display_name'
    AND hit->>'chunk_utf8'=p_item->>'quote_utf8' AND hit->>'chunk_sha256'=p_item->>'quote_sha256'
    AND hit->>'quote_start_offset'=p_item->>'quote_start_offset'
    AND hit->>'quote_end_offset'=p_item->>'quote_end_offset'
    AND hit->>'retrieval_rank'=p_item->>'retrieval_rank'
    AND hit->>'retrieval_contract_version'=p_item->>'retrieval_contract_version';
  IF NOT FOUND THEN RAISE EXCEPTION 'KNOWLEDGE_ATTESTED_TEXT_HIT_V2_MISMATCH' USING ERRCODE='23514'; END IF;
END $$;
REVOKE ALL ON FUNCTION kb_knowledge_verify_attested_text_hit_v2(uuid,text,uuid,jsonb) FROM PUBLIC;

CREATE FUNCTION kb_knowledge_verify_attested_image_hit_v3(
  p_attestation_id uuid,p_attestation_sha256 text,p_requirement_artifact_id uuid,p_item jsonb
) RETURNS void LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
BEGIN
  PERFORM 1 FROM knowledge_matching_scope_attestations_v2 attestation
  CROSS JOIN LATERAL jsonb_array_elements(convert_from(attestation.canonical_payload,'UTF8')::jsonb->'frozen_hits') hit
  CROSS JOIN LATERAL jsonb_array_elements(convert_from(attestation.canonical_payload,'UTF8')::jsonb->'products') product
  WHERE attestation.id=p_attestation_id AND attestation.content_sha256=p_attestation_sha256
    AND (hit->>'requirement_artifact_id')::uuid=p_requirement_artifact_id
    AND hit->>'source_type'='image_ocr' AND hit->>'product_version_artifact_id'=product->>'id'
    AND hit->>'document_id'=p_item->>'document_id' AND hit->>'source_chunk_id'=p_item->>'source_chunk_id'
    AND product->>'product_version_id'=p_item->>'product_version_id' AND product->>'workspace_kind'=p_item->>'workspace_kind'
    AND hit->>'chunk_utf8'=p_item->>'quote_utf8' AND hit->>'chunk_sha256'=p_item->>'quote_sha256'
    AND hit->>'quote_start_offset'=p_item->>'quote_start_offset' AND hit->>'quote_end_offset'=p_item->>'quote_end_offset'
    AND hit->>'retrieval_rank'=p_item->>'retrieval_rank' AND hit->>'retrieval_contract_version'=p_item->>'retrieval_contract_version'
    AND hit#>>'{media,image_artifact_revision_id}'=p_item->>'image_artifact_revision_id'
    AND hit#>>'{media,object_ref}'=p_item->>'object_ref' AND hit#>>'{media,sha256}'=p_item->>'sha256'
    AND hit#>>'{media,media_type}'=p_item->>'media_type' AND hit#>>'{media,width}'=p_item->>'width'
    AND hit#>>'{media,height}'=p_item->>'height'
    AND hit#>'{media,page_ordinal}' IS NOT DISTINCT FROM coalesce(p_item->'page_ordinal','null'::jsonb)
    AND hit#>'{media,bounding_region}' IS NOT DISTINCT FROM coalesce(p_item->'bounding_region','null'::jsonb)
    AND hit#>>'{media,frozen_document_display_name}'=p_item->>'frozen_document_display_name';
  IF NOT FOUND THEN RAISE EXCEPTION 'KNOWLEDGE_ATTESTED_IMAGE_HIT_V3_MISMATCH' USING ERRCODE='23514'; END IF;
END $$;
REVOKE ALL ON FUNCTION kb_knowledge_verify_attested_image_hit_v3(uuid,text,uuid,jsonb) FROM PUBLIC;

CREATE TABLE graph_nodes (
    product_version_id uuid NOT NULL REFERENCES product_versions (id),
    document_id uuid NOT NULL REFERENCES documents (id) ON DELETE CASCADE,
    name text NOT NULL,
    chunk_ids uuid[] NOT NULL DEFAULT '{}',
    PRIMARY KEY (product_version_id, document_id, name)
);
CREATE TABLE graph_relations (
    product_version_id uuid NOT NULL REFERENCES product_versions (id),
    document_id uuid NOT NULL REFERENCES documents (id) ON DELETE CASCADE,
    node1 text NOT NULL,
    node2 text NOT NULL,
    rel_type text NOT NULL,
    PRIMARY KEY (product_version_id, document_id, node1, node2, rel_type)
);

CREATE TABLE wiki_pages (
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
CREATE TABLE wiki_folders (
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
CREATE TABLE wiki_log_entries (
    id uuid PRIMARY KEY,
    product_version_id uuid NOT NULL REFERENCES product_versions (id),
    document_id uuid,
    level text NOT NULL DEFAULT 'info',
    message text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);

-- Knowledge-base runtime DML is intentionally unchanged. Platform-owned object,
-- idempotency, audit, and retention tables are introduced by the next slice and
-- are never granted here.
REVOKE ALL ON ALL TABLES IN SCHEMA public FROM PUBLIC;
GRANT SELECT, INSERT, UPDATE, DELETE ON
    workspaces, users, workspace_members, products, product_versions, documents,
    tags, document_tags, document_processing_spans, task_pending_ops, task_dead_letters,
    models, api_keys, chunks, chunk_embeddings, graph_nodes, graph_relations,
    wiki_pages, wiki_folders, wiki_log_entries
TO kb_runtime_api, kb_runtime_worker;
GRANT SELECT ON
    embedding_revisions_v2, rerank_revisions_v2, knowledge_retrieval_policies_v2,
    product_version_embedding_bindings_v2, knowledge_image_artifact_revisions,
    knowledge_image_ocr_chunk_artifact_mappings
TO kb_runtime_api, kb_runtime_worker;
GRANT INSERT ON knowledge_image_artifact_revisions,knowledge_image_ocr_chunk_artifact_mappings
TO kb_runtime_worker;
GRANT SELECT ON chunk_keyword_indexes_v2, chunk_vector_indexes_v2,
    product_version_vector_index_generations_v2,
    product_version_keyword_index_generations_v2,
    knowledge_semantic_index_intents_v2 TO kb_runtime_api;
GRANT SELECT ON chunk_keyword_indexes_v2, chunk_vector_indexes_v2,
    product_version_vector_index_generations_v2,
    product_version_keyword_index_generations_v2,
    knowledge_semantic_index_intents_v2 TO kb_runtime_worker;
GRANT EXECUTE ON FUNCTION kb_knowledge_rebuild_keyword_indexes_v2(uuid),
    kb_knowledge_reconcile_vector_indexes_v2(uuid,text,text,jsonb),
    kb_knowledge_prepare_semantic_index_intent_v2(uuid),
    kb_knowledge_preflight_semantic_index_intent_v2(uuid,bigint),
    kb_knowledge_rebuild_semantic_keyword_indexes_v2(uuid,text,text),
    kb_knowledge_complete_semantic_index_intent_v2(uuid,bigint),
    kb_knowledge_record_semantic_index_intent_v2(uuid,bigint,text,text,text)
TO kb_runtime_worker;
GRANT EXECUTE ON FUNCTION kb_knowledge_keyword_token_stream_v2(text),
    kb_knowledge_lock_semantic_policy_v2(text),
    kb_knowledge_lock_rerank_revision_v2(text)
TO kb_runtime_api, kb_runtime_worker;
GRANT EXECUTE ON FUNCTION kb_knowledge_attest_matching_scope_v2(jsonb),
    kb_knowledge_verify_matching_scope_v2(uuid,text,jsonb),
    kb_knowledge_require_matching_attestation_v2(uuid,text),
    kb_knowledge_load_matching_attestation_v2(uuid,text),
    kb_knowledge_verify_attested_text_hit_v2(uuid,text,uuid,jsonb),
    kb_knowledge_verify_attested_image_hit_v3(uuid,text,uuid,jsonb)
TO kb_runtime_worker;
