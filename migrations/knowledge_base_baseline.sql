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
    deleted_at timestamptz
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
    generated_questions jsonb NOT NULL DEFAULT '[]'::jsonb
);
CREATE INDEX chunks_document_idx ON chunks (document_id);
CREATE INDEX chunks_version_idx ON chunks (product_version_id);
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

-- Knowledge owns the v2 policy artifact and support decision. Request DTOs may
-- reference a supported identity, but cannot define their own trusted quotas.
CREATE TABLE knowledge_retrieval_policies_v2 (
    policy_sha256 text PRIMARY KEY CHECK (policy_sha256 ~ '^[0-9a-f]{64}$'),
    canonical_policy_payload bytea NOT NULL,
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
        jsonb_typeof(convert_from(canonical_policy_payload,'UTF8')::jsonb)
            IS NOT DISTINCT FROM 'object'
        AND jsonb_typeof((convert_from(canonical_policy_payload,'UTF8')::jsonb)
            ->'contract_version') IS NOT DISTINCT FROM 'string'
        AND (convert_from(canonical_policy_payload,'UTF8')::jsonb)
            ->>'contract_version' IS NOT DISTINCT FROM contract_version
        AND jsonb_typeof((convert_from(canonical_policy_payload,'UTF8')::jsonb)
            ->'request_quotas') IS NOT DISTINCT FROM 'object'
        AND jsonb_typeof((convert_from(canonical_policy_payload,'UTF8')::jsonb)
            ->'request_quotas'->'max_hits') IS NOT DISTINCT FROM 'number'
        AND (convert_from(canonical_policy_payload,'UTF8')::jsonb)
            ->'request_quotas'->'max_hits' IS NOT DISTINCT FROM to_jsonb(max_hits)
        AND jsonb_typeof((convert_from(canonical_policy_payload,'UTF8')::jsonb)
            ->'request_quotas'->'max_chunk_bytes') IS NOT DISTINCT FROM 'number'
        AND (convert_from(canonical_policy_payload,'UTF8')::jsonb)
            ->'request_quotas'->'max_chunk_bytes' IS NOT DISTINCT FROM to_jsonb(max_chunk_bytes)
        AND jsonb_typeof((convert_from(canonical_policy_payload,'UTF8')::jsonb)
            ->'request_quotas'->'max_total_bytes') IS NOT DISTINCT FROM 'number'
        AND (convert_from(canonical_policy_payload,'UTF8')::jsonb)
            ->'request_quotas'->'max_total_bytes' IS NOT DISTINCT FROM to_jsonb(max_total_bytes)
    )
);
REVOKE ALL ON TABLE knowledge_retrieval_policies_v2 FROM PUBLIC;

CREATE FUNCTION kb_knowledge_guard_retrieval_policy_v2_update()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
BEGIN
    IF NEW.policy_sha256 IS DISTINCT FROM OLD.policy_sha256
       OR NEW.canonical_policy_payload IS DISTINCT FROM OLD.canonical_policy_payload
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
SET search_path = pg_catalog, public
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

-- V2 is an independent, not-yet-routed contract. It replays the complete V1
-- eligible product scope while additionally binding trusted source hits to the
-- canonical retrieval policy and its quotas.
CREATE FUNCTION kb_knowledge_attest_matching_scope_v2(p_scope jsonb)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE
    p_products jsonb;
    p_hits jsonb;
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
    IF p_scope IS NULL OR jsonb_typeof(p_scope) IS DISTINCT FROM 'object' THEN
        RAISE EXCEPTION 'KNOWLEDGE_MATCHING_SCOPE_V2_INVALID' USING ERRCODE='23514';
    END IF;
    IF (SELECT array_agg(key ORDER BY key) FROM jsonb_object_keys(p_scope) key)
         IS DISTINCT FROM ARRAY[
             'frozen_hits','products','retrieval_policy','schema_version',
             'version_selections','workspace_kinds']::text[]
       OR p_scope->'schema_version' IS DISTINCT FROM '2'::jsonb
       OR jsonb_typeof(p_scope->'products') IS DISTINCT FROM 'array'
       OR jsonb_typeof(p_scope->'frozen_hits') IS DISTINCT FROM 'array'
       OR jsonb_typeof(p_scope->'version_selections') IS DISTINCT FROM 'object'
       OR jsonb_typeof(p_scope->'workspace_kinds') IS DISTINCT FROM 'array'
       OR jsonb_typeof(p_scope->'retrieval_policy') IS DISTINCT FROM 'object' THEN
        RAISE EXCEPTION 'KNOWLEDGE_MATCHING_SCOPE_V2_INVALID' USING ERRCODE='23514';
    END IF;

    p_products := p_scope->'products';
    p_hits := p_scope->'frozen_hits';
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
     WHERE policy.policy_sha256=p_policy->>'policy_sha256'
       AND policy.support_state='supported'
       AND policy.contract_version='knowledge-evidence-v2'
       AND policy.contract_version=p_policy->>'contract_version'
       AND policy.max_hits=v_max_hits
       AND policy.max_chunk_bytes=v_max_chunk_bytes
       AND policy.max_total_bytes=v_max_total_bytes
     FOR SHARE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'KNOWLEDGE_MATCHING_RETRIEVAL_POLICY_V2_INVALID' USING ERRCODE='23514';
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
                   'frozen_document_display_name','id','offset_unit',
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
            OR hit->>'retrieval_raw_score' !~ '^(0|[1-9][0-9]*)\.[0-9]{6}$') THEN
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
               octet_length(convert_to(chunk_value.content,'UTF8'))) THEN
        RAISE EXCEPTION 'KNOWLEDGE_MATCHING_HIT_V2_MISMATCH' USING ERRCODE='23514';
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
                         PARTITION BY hit->>'requirement_artifact_id'
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
         GROUP BY hit->>'requirement_artifact_id'
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
SET search_path = pg_catalog, public
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
