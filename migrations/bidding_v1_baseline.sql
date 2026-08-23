-- KnowledgeBrain final V1 fresh baseline: bidding_v1.
-- This create-only slice owns TenderPublication, ClauseLifecycle, Matching,
-- Quote, and Submission. It contains no compatibility schema or repair DDL.

CREATE FUNCTION kb_bid_family_for_kind(p_kind text)
RETURNS text
LANGUAGE sql
IMMUTABLE
STRICT
PARALLEL SAFE
SET search_path = pg_catalog
AS $$
    SELECT CASE p_kind
        WHEN 'technical' THEN 'technical'
        WHEN 'qualification' THEN 'commercial'
        WHEN 'service' THEN 'commercial'
        WHEN 'pricing' THEN NULL
        WHEN 'schedule_delivery' THEN NULL
        WHEN 'schedule_payment' THEN NULL
        WHEN 'evaluation' THEN NULL
        WHEN 'procedural' THEN NULL
        ELSE NULL
    END
$$;

CREATE TABLE bid_projects (
    id uuid PRIMARY KEY,
    title text NOT NULL CHECK (octet_length(btrim(title)) BETWEEN 1 AND 256),
    owner_user_id uuid NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    ends_at timestamptz NOT NULL CHECK (isfinite(ends_at)),
    status text NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'ended')),
    ended_at timestamptz,
    fact_revision bigint NOT NULL DEFAULT 0 CHECK (fact_revision >= 0),
    fact_sha256 kb_sha256 NOT NULL,
    budget_amount numeric(20,2),
    budget_currency text,
    ceiling_price numeric(20,2),
    ceiling_currency text,
    ceiling_basis text NOT NULL DEFAULT 'unspecified'
        CHECK (ceiling_basis IN ('tax_inclusive', 'tax_exclusive', 'unspecified')),
    ceiling_revision bigint NOT NULL DEFAULT 0 CHECK (ceiling_revision >= 0),
    ceiling_identity_sha256 kb_sha256 NOT NULL,
    expires_at timestamptz,
    bid_open_at timestamptz,
    bid_valid_until timestamptz,
    bid_valid_days integer CHECK (bid_valid_days BETWEEN 1 AND 3650),
    matching_mutation_watermark bigint NOT NULL DEFAULT 0 CHECK (matching_mutation_watermark >= 0),
    created_by kb_actor_identity NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CHECK ((status = 'open' AND ended_at IS NULL) OR (status = 'ended' AND ended_at IS NOT NULL)),
    CHECK ((budget_amount IS NULL AND budget_currency IS NULL)
        OR (budget_amount >= 0 AND budget_currency = 'CNY')),
    CHECK ((ceiling_price IS NULL AND ceiling_currency IS NULL AND ceiling_basis = 'unspecified')
        OR (ceiling_price >= 0 AND ceiling_currency = 'CNY'))
);

CREATE TABLE bid_documents (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
    file_name text NOT NULL CHECK (octet_length(file_name) BETWEEN 1 AND 512),
    media_type text NOT NULL,
    byte_length bigint NOT NULL CHECK (byte_length >= 0),
    original_object_ref kb_object_ref NOT NULL,
    original_sha256 kb_sha256 NOT NULL,
    conversion_generation integer NOT NULL DEFAULT 1 CHECK (conversion_generation > 0),
    parse_status text NOT NULL CHECK (parse_status IN ('pending', 'processing', 'completed', 'failed')),
    current_converted_source_artifact_id uuid,
    created_at timestamptz NOT NULL DEFAULT now(),
    parsed_at timestamptz,
    error_code text,
    UNIQUE (project_id, id),
    CHECK (original_object_ref = 'objects/' || original_sha256)
);

CREATE TABLE bid_document_conversion_attempts (
    document_id uuid NOT NULL REFERENCES bid_documents(id) ON DELETE RESTRICT,
    conversion_generation integer NOT NULL CHECK (conversion_generation > 0),
    attempt integer NOT NULL CHECK (attempt > 0),
    claim_token uuid NOT NULL,
    claimed_by text NOT NULL CHECK (octet_length(claimed_by) BETWEEN 1 AND 128),
    claim_lease_ms integer NOT NULL CHECK (claim_lease_ms BETWEEN 1000 AND 3600000),
    claimed_at timestamptz NOT NULL,
    heartbeat_at timestamptz NOT NULL,
    status text NOT NULL CHECK (status IN ('running', 'completed', 'failed', 'reaped')),
    error_code text,
    PRIMARY KEY (document_id, conversion_generation, attempt),
    UNIQUE (document_id, claim_token)
);

CREATE TABLE bid_converted_source_artifacts (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL,
    document_id uuid NOT NULL,
    conversion_generation integer NOT NULL CHECK (conversion_generation > 0),
    original_object_ref kb_object_ref NOT NULL,
    original_sha256 kb_sha256 NOT NULL,
    canonical_markdown_utf8 bytea NOT NULL,
    markdown_sha256 kb_sha256 NOT NULL,
    byte_length bigint NOT NULL CHECK (byte_length = octet_length(canonical_markdown_utf8)),
    converter_contract_version text NOT NULL CHECK (octet_length(converter_contract_version) BETWEEN 1 AND 128),
    image_asset_set_sha256 kb_sha256 NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (project_id, document_id, conversion_generation),
    UNIQUE (project_id, document_id, id),
    FOREIGN KEY (project_id, document_id) REFERENCES bid_documents(project_id, id) ON DELETE RESTRICT,
    CHECK (original_object_ref = 'objects/' || original_sha256),
    CHECK (markdown_sha256 = encode(digest(canonical_markdown_utf8, 'sha256'), 'hex'))
);
CREATE TRIGGER bid_converted_source_artifacts_immutable
BEFORE UPDATE OR DELETE ON bid_converted_source_artifacts
FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();
ALTER TABLE bid_documents ADD CONSTRAINT bid_documents_current_source_fk
FOREIGN KEY (project_id, id, current_converted_source_artifact_id)
REFERENCES bid_converted_source_artifacts(project_id, document_id, id) ON DELETE RESTRICT;

CREATE TABLE bid_section_artifacts (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL,
    document_id uuid NOT NULL,
    source_artifact_id uuid NOT NULL,
    conversion_generation integer NOT NULL CHECK (conversion_generation > 0),
    section_key text NOT NULL CHECK (octet_length(section_key) BETWEEN 1 AND 256),
    heading_path jsonb NOT NULL CHECK (jsonb_typeof(heading_path) = 'array'),
    parent_start_offset bigint NOT NULL CHECK (parent_start_offset >= 0),
    parent_end_offset bigint NOT NULL,
    section_sha256 kb_sha256 NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (source_artifact_id, section_key),
    UNIQUE (project_id, document_id, source_artifact_id, id),
    FOREIGN KEY (project_id, document_id, source_artifact_id)
        REFERENCES bid_converted_source_artifacts(project_id, document_id, id) ON DELETE RESTRICT,
    CHECK (parent_end_offset > parent_start_offset)
);
CREATE TRIGGER bid_section_artifacts_immutable
BEFORE UPDATE OR DELETE ON bid_section_artifacts
FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();

CREATE FUNCTION kb_bid_validate_section_artifact()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, public
AS $$
DECLARE source_value bid_converted_source_artifacts%ROWTYPE;
BEGIN
    SELECT * INTO STRICT source_value FROM bid_converted_source_artifacts
     WHERE id=NEW.source_artifact_id AND project_id=NEW.project_id
       AND document_id=NEW.document_id FOR SHARE;
    IF NEW.conversion_generation <> source_value.conversion_generation
       OR NEW.parent_end_offset > source_value.byte_length
       OR NEW.section_sha256 <> encode(digest(substring(source_value.canonical_markdown_utf8
              FROM NEW.parent_start_offset::integer + 1
              FOR (NEW.parent_end_offset-NEW.parent_start_offset)::integer), 'sha256'), 'hex') THEN
        RAISE EXCEPTION 'SECTION_ARTIFACT_SCOPE_OR_DIGEST_MISMATCH' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END
$$;
CREATE TRIGGER bid_section_artifacts_verify
BEFORE INSERT ON bid_section_artifacts
FOR EACH ROW EXECUTE FUNCTION kb_bid_validate_section_artifact();

CREATE TABLE bid_source_span_artifacts (
    id uuid PRIMARY KEY,
    schema_version smallint NOT NULL CHECK (schema_version = 2),
    project_id uuid NOT NULL,
    document_id uuid NOT NULL,
    source_artifact_id uuid NOT NULL,
    section_artifact_id uuid NOT NULL,
    conversion_generation integer NOT NULL CHECK (conversion_generation > 0),
    section_key text NOT NULL,
    parent_start_offset bigint NOT NULL CHECK (parent_start_offset >= 0),
    parent_end_offset bigint NOT NULL,
    start_offset bigint NOT NULL CHECK (start_offset >= 0),
    end_offset bigint NOT NULL,
    offset_unit text NOT NULL CHECK (offset_unit = 'utf8_byte'),
    quote text NOT NULL,
    quote_sha256 kb_sha256 NOT NULL,
    heading_path jsonb NOT NULL CHECK (jsonb_typeof(heading_path) = 'array'),
    source_span_v2 jsonb NOT NULL CHECK (jsonb_typeof(source_span_v2) = 'object'),
    canonical_payload bytea NOT NULL,
    content_sha256 kb_sha256 NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (project_id, document_id, source_artifact_id, section_artifact_id, start_offset, end_offset, quote_sha256),
    UNIQUE (project_id, id),
    FOREIGN KEY (project_id, document_id, source_artifact_id, section_artifact_id)
      REFERENCES bid_section_artifacts(project_id, document_id, source_artifact_id, id) ON DELETE RESTRICT,
    CHECK (parent_end_offset > parent_start_offset),
    CHECK (start_offset >= parent_start_offset AND end_offset > start_offset AND end_offset <= parent_end_offset),
    CHECK (quote_sha256 = encode(digest(convert_to(quote, 'UTF8'), 'sha256'), 'hex')),
    CHECK (content_sha256 = encode(digest(canonical_payload, 'sha256'), 'hex'))
);
CREATE TRIGGER bid_source_span_artifacts_immutable
BEFORE UPDATE OR DELETE ON bid_source_span_artifacts
FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();

CREATE FUNCTION kb_bid_validate_source_span_v2()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, public
AS $$
DECLARE
    section_value bid_section_artifacts%ROWTYPE;
    source_value bid_converted_source_artifacts%ROWTYPE;
    parsed jsonb;
    allowed text[] := ARRAY['schema_version','source_artifact_id','section_artifact_id','project_id',
      'document_id','conversion_generation','section_key','parent_start_offset','parent_end_offset',
      'start_offset','end_offset','offset_unit','quote','quote_sha256','heading_path'];
BEGIN
    SELECT * INTO STRICT section_value FROM bid_section_artifacts WHERE id=NEW.section_artifact_id FOR SHARE;
    SELECT * INTO STRICT source_value FROM bid_converted_source_artifacts WHERE id=NEW.source_artifact_id FOR SHARE;
    BEGIN parsed := convert_from(NEW.canonical_payload,'UTF8')::jsonb;
    EXCEPTION WHEN OTHERS THEN RAISE EXCEPTION 'SOURCE_SPAN_V2_CANONICAL_JSON_INVALID' USING ERRCODE='22023'; END;
    IF parsed <> NEW.source_span_v2
       OR (SELECT array_agg(key ORDER BY key) FROM jsonb_object_keys(NEW.source_span_v2) key)
          <> (SELECT array_agg(key ORDER BY key) FROM unnest(allowed) key)
       OR NEW.project_id <> section_value.project_id OR NEW.document_id <> section_value.document_id
       OR NEW.source_artifact_id <> section_value.source_artifact_id
       OR NEW.conversion_generation <> section_value.conversion_generation
       OR NEW.section_key <> section_value.section_key
       OR NEW.parent_start_offset <> section_value.parent_start_offset
       OR NEW.parent_end_offset <> section_value.parent_end_offset
       OR NEW.heading_path <> section_value.heading_path
       OR NEW.end_offset > source_value.byte_length
       OR convert_to(NEW.quote,'UTF8') <> substring(source_value.canonical_markdown_utf8
              FROM NEW.start_offset::integer + 1 FOR (NEW.end_offset-NEW.start_offset)::integer)
       OR NEW.source_span_v2 <> jsonb_build_object(
          'schema_version',2,'source_artifact_id',NEW.source_artifact_id,'section_artifact_id',NEW.section_artifact_id,
          'project_id',NEW.project_id,'document_id',NEW.document_id,'conversion_generation',NEW.conversion_generation,
          'section_key',NEW.section_key,'parent_start_offset',NEW.parent_start_offset,'parent_end_offset',NEW.parent_end_offset,
          'start_offset',NEW.start_offset,'end_offset',NEW.end_offset,'offset_unit','utf8_byte','quote',NEW.quote,
          'quote_sha256',NEW.quote_sha256,'heading_path',NEW.heading_path) THEN
        RAISE EXCEPTION 'SOURCE_SPAN_V2_SCOPE_QUOTE_OR_SCHEMA_MISMATCH' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END
$$;
CREATE TRIGGER bid_source_span_artifacts_verify
BEFORE INSERT ON bid_source_span_artifacts
FOR EACH ROW EXECUTE FUNCTION kb_bid_validate_source_span_v2();

CREATE TABLE bid_extraction_targets (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL,
    document_id uuid NOT NULL,
    source_artifact_id uuid NOT NULL,
    conversion_generation integer NOT NULL CHECK (conversion_generation > 0),
    extraction_generation integer NOT NULL CHECK (extraction_generation > 0),
    router_contract_version text NOT NULL,
    policy_version text NOT NULL,
    prompt_version text NOT NULL,
    output_schema_version smallint NOT NULL CHECK (output_schema_version = 1),
    expected_section_count integer NOT NULL CHECK (expected_section_count > 0),
    published_section_count integer NOT NULL DEFAULT 0 CHECK (published_section_count >= 0),
    state text NOT NULL CHECK (state IN ('pending', 'running', 'terminal', 'published', 'failed')),
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (project_id, document_id, extraction_generation),
    UNIQUE (project_id, document_id, id),
    FOREIGN KEY (project_id, document_id, source_artifact_id)
        REFERENCES bid_converted_source_artifacts(project_id, document_id, id) ON DELETE RESTRICT,
    CHECK (published_section_count <= expected_section_count)
);

CREATE TABLE bid_extraction_attempts (
    target_id uuid NOT NULL REFERENCES bid_extraction_targets(id) ON DELETE RESTRICT,
    attempt integer NOT NULL CHECK (attempt > 0),
    claim_token uuid NOT NULL,
    claimed_by text NOT NULL,
    claim_lease_ms integer NOT NULL CHECK (claim_lease_ms BETWEEN 1000 AND 3600000),
    claimed_at timestamptz NOT NULL,
    heartbeat_at timestamptz NOT NULL,
    status text NOT NULL CHECK (status IN ('running', 'completed', 'failed', 'reaped')),
    error_code text,
    PRIMARY KEY (target_id, attempt),
    UNIQUE (target_id, claim_token)
);

CREATE TABLE bid_extract_segment_candidates (
    id uuid PRIMARY KEY,
    target_id uuid NOT NULL REFERENCES bid_extraction_targets(id) ON DELETE RESTRICT,
    section_artifact_id uuid NOT NULL REFERENCES bid_section_artifacts(id) ON DELETE RESTRICT,
    source_span_artifact_id uuid NOT NULL REFERENCES bid_source_span_artifacts(id) ON DELETE RESTRICT,
    ordinal integer NOT NULL CHECK (ordinal >= 0),
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (target_id, section_artifact_id, ordinal),
    UNIQUE (target_id, source_span_artifact_id)
);
CREATE TRIGGER bid_extract_segment_candidates_immutable
BEFORE UPDATE OR DELETE ON bid_extract_segment_candidates
FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();

CREATE TABLE bid_extract_segment_dispositions (
    segment_candidate_id uuid PRIMARY KEY REFERENCES bid_extract_segment_candidates(id) ON DELETE RESTRICT,
    disposition text NOT NULL CHECK (disposition IN ('clause', 'non_requirement', 'unresolved')),
    reason_code text NOT NULL CHECK (reason_code IN (
        'CLAUSE', 'FACT_ONLY', 'DETERMINISTIC_NON_REQUIREMENT', 'AMBIGUOUS'
    )),
    created_at timestamptz NOT NULL DEFAULT now(),
    CHECK ((disposition = 'clause' AND reason_code = 'CLAUSE')
        OR (disposition = 'non_requirement' AND reason_code IN ('FACT_ONLY', 'DETERMINISTIC_NON_REQUIREMENT'))
        OR (disposition = 'unresolved' AND reason_code = 'AMBIGUOUS'))
);
CREATE TRIGGER bid_extract_segment_dispositions_immutable
BEFORE UPDATE OR DELETE ON bid_extract_segment_dispositions
FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();

CREATE TABLE bid_extract_clause_candidates (
    id uuid PRIMARY KEY,
    target_id uuid NOT NULL REFERENCES bid_extraction_targets(id) ON DELETE RESTRICT,
    segment_candidate_id uuid NOT NULL UNIQUE REFERENCES bid_extract_segment_candidates(id) ON DELETE RESTRICT,
    proposal_text text NOT NULL CHECK (octet_length(btrim(proposal_text)) BETWEEN 1 AND 32768),
    must boolean NOT NULL,
    proposed_kind text NOT NULL CHECK (proposed_kind IN (
        'technical', 'qualification', 'service', 'pricing', 'schedule_delivery',
        'schedule_payment', 'evaluation', 'procedural'
    )),
    router_reason_code text NOT NULL CHECK (octet_length(router_reason_code) BETWEEN 1 AND 128),
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TRIGGER bid_extract_clause_candidates_immutable
BEFORE UPDATE OR DELETE ON bid_extract_clause_candidates
FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();

CREATE TABLE bid_extract_fact_candidates (
    id uuid PRIMARY KEY,
    target_id uuid NOT NULL REFERENCES bid_extraction_targets(id) ON DELETE RESTRICT,
    segment_candidate_id uuid NOT NULL REFERENCES bid_extract_segment_candidates(id) ON DELETE RESTRICT,
    field text NOT NULL CHECK (field IN (
        'budget_amount', 'ceiling_price', 'expires_at', 'bid_open_at',
        'bid_valid_until', 'bid_valid_days'
    )),
    typed_value jsonb NOT NULL,
    raw_quote text NOT NULL,
    confidence numeric(5,4) NOT NULL CHECK (confidence BETWEEN 0 AND 1),
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (segment_candidate_id, field, typed_value)
);
CREATE TRIGGER bid_extract_fact_candidates_immutable
BEFORE UPDATE OR DELETE ON bid_extract_fact_candidates
FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();

CREATE TABLE bid_section_publications (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
    target_id uuid NOT NULL REFERENCES bid_extraction_targets(id) ON DELETE RESTRICT,
    section_artifact_id uuid NOT NULL REFERENCES bid_section_artifacts(id) ON DELETE RESTRICT,
    publication_revision bigint NOT NULL CHECK (publication_revision > 0),
    content_sha256 kb_sha256 NOT NULL,
    published_by kb_actor_identity NOT NULL,
    published_at timestamptz NOT NULL,
    UNIQUE (target_id, section_artifact_id),
    UNIQUE (project_id, id)
);
CREATE TRIGGER bid_section_publications_immutable
BEFORE UPDATE OR DELETE ON bid_section_publications
FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();
CREATE TABLE bid_current_section_publications (
    project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
    document_id uuid NOT NULL,
    section_key text NOT NULL,
    publication_id uuid NOT NULL,
    revision bigint NOT NULL CHECK (revision > 0),
    PRIMARY KEY (project_id, document_id, section_key),
    FOREIGN KEY (project_id, publication_id)
        REFERENCES bid_section_publications(project_id, id) ON DELETE RESTRICT
);

CREATE TABLE bid_fact_suggestion_decisions (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
    candidate_id uuid NOT NULL REFERENCES bid_extract_fact_candidates(id) ON DELETE RESTRICT,
    revision integer NOT NULL CHECK (revision > 0),
    status text NOT NULL CHECK (status IN ('pending', 'accepted', 'rejected', 'superseded')),
    reason text,
    decided_by kb_actor_identity NOT NULL,
    decided_at timestamptz NOT NULL,
    previous_decision_id uuid,
    UNIQUE (candidate_id, revision),
    UNIQUE (candidate_id, previous_decision_id),
    FOREIGN KEY (previous_decision_id) REFERENCES bid_fact_suggestion_decisions(id) DEFERRABLE INITIALLY DEFERRED,
    CHECK (status <> 'rejected' OR octet_length(btrim(reason)) BETWEEN 1 AND 512),
    CHECK (revision = 1 OR previous_decision_id IS NOT NULL)
);
CREATE TRIGGER bid_fact_suggestion_decisions_immutable
BEFORE UPDATE OR DELETE ON bid_fact_suggestion_decisions
FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();

CREATE TABLE bid_clauses (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
    publication_id uuid REFERENCES bid_section_publications(id) ON DELETE RESTRICT,
    origin_candidate_id uuid REFERENCES bid_extract_clause_candidates(id) ON DELETE RESTRICT,
    provenance text NOT NULL CHECK (provenance IN ('extracted', 'manual', 'manual_after_edit')),
    status text NOT NULL CHECK (status IN ('draft', 'confirmed', 'rejected', 'superseded')),
    kind text NOT NULL CHECK (kind IN (
        'technical', 'qualification', 'service', 'pricing', 'schedule_delivery',
        'schedule_payment', 'evaluation', 'procedural'
    )),
    family text GENERATED ALWAYS AS (kb_bid_family_for_kind(kind)) STORED,
    text text NOT NULL CHECK (octet_length(btrim(text)) BETWEEN 1 AND 32768),
    must boolean NOT NULL,
    current_source_span_artifact_id uuid REFERENCES bid_source_span_artifacts(id) ON DELETE RESTRICT,
    extracted_origin_source_span_artifact_id uuid REFERENCES bid_source_span_artifacts(id) ON DELETE RESTRICT,
    revision bigint NOT NULL CHECK (revision > 0),
    confirmation_required_reason text,
    confirmation_required_router_generation bigint,
    created_by kb_actor_identity NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (project_id, id),
    FOREIGN KEY (project_id, current_source_span_artifact_id)
      REFERENCES bid_source_span_artifacts(project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, extracted_origin_source_span_artifact_id)
      REFERENCES bid_source_span_artifacts(project_id, id) ON DELETE RESTRICT,
    CHECK ((provenance = 'manual' AND current_source_span_artifact_id IS NULL
            AND extracted_origin_source_span_artifact_id IS NULL)
        OR (provenance = 'extracted' AND current_source_span_artifact_id IS NOT NULL
            AND extracted_origin_source_span_artifact_id IS NOT NULL)
        OR (provenance = 'manual_after_edit' AND current_source_span_artifact_id IS NULL
            AND extracted_origin_source_span_artifact_id IS NOT NULL)),
    CHECK ((confirmation_required_reason IS NULL)
        = (confirmation_required_router_generation IS NULL)),
    CHECK (confirmation_required_reason IS NULL OR status = 'draft')
);
CREATE INDEX bid_clauses_matching_idx ON bid_clauses(project_id, family, id)
    WHERE status = 'confirmed' AND family IS NOT NULL;

CREATE TABLE bid_clause_set_identities (
    project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
    set_kind text NOT NULL CHECK (set_kind IN (
        'service', 'pricing', 'schedule_payment', 'schedule_delivery', 'evaluation', 'procedural'
    )),
    revision bigint NOT NULL CHECK (revision >= 0),
    content_sha256 kb_sha256 NOT NULL,
    updated_at timestamptz NOT NULL,
    PRIMARY KEY (project_id, set_kind)
);

CREATE TABLE kind_router_contract_artifacts (
    version text PRIMARY KEY,
    schema_version smallint NOT NULL CHECK (schema_version = 1),
    canonical_payload bytea NOT NULL,
    content_sha256 kb_sha256 NOT NULL UNIQUE,
    created_at timestamptz NOT NULL DEFAULT now(),
    CHECK (content_sha256 = encode(digest(canonical_payload, 'sha256'), 'hex'))
);
CREATE TABLE kind_router_current (
    singleton_key boolean PRIMARY KEY DEFAULT true CHECK (singleton_key),
    version text NOT NULL REFERENCES kind_router_contract_artifacts(version) ON DELETE RESTRICT,
    promotion_generation bigint NOT NULL CHECK (promotion_generation >= 0)
);
INSERT INTO kind_router_contract_artifacts(
    version, schema_version, canonical_payload, content_sha256, created_at
)
VALUES ('kind-router-v1', 1,
 convert_to('{"family":{"evaluation":null,"pricing":null,"procedural":null,"qualification":"commercial","schedule_delivery":null,"schedule_payment":null,"service":"commercial","technical":"technical"},"schema_version":1,"version":"kind-router-v1"}', 'UTF8'),
 encode(digest(convert_to('{"family":{"evaluation":null,"pricing":null,"procedural":null,"qualification":"commercial","schedule_delivery":null,"schedule_payment":null,"service":"commercial","technical":"technical"},"schema_version":1,"version":"kind-router-v1"}', 'UTF8'), 'sha256'), 'hex'),
 '1970-01-01 UTC');
INSERT INTO kind_router_current(singleton_key, version, promotion_generation)
VALUES (true, 'kind-router-v1', 0);
CREATE TRIGGER kind_router_contract_artifacts_immutable
BEFORE UPDATE OR DELETE ON kind_router_contract_artifacts
FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();

-- MatchingPublication: frozen manifest, routes, scope, staging, immutable report,
-- and distinct route/project pick-set artifacts.
CREATE TABLE bid_matching_manifests (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
    generation bigint NOT NULL CHECK (generation > 0),
    mutation_watermark bigint NOT NULL CHECK (mutation_watermark >= 0),
    requirement_set_sha256 kb_sha256 NOT NULL,
    eligible_scope_sha256 kb_sha256 NOT NULL,
    canonical_payload bytea NOT NULL,
    content_sha256 kb_sha256 NOT NULL UNIQUE,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (project_id, generation),
    UNIQUE (project_id, id),
    CHECK (content_sha256 = encode(digest(canonical_payload, 'sha256'), 'hex'))
);
CREATE TRIGGER bid_matching_manifests_immutable
BEFORE UPDATE OR DELETE ON bid_matching_manifests
FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();

CREATE TABLE bid_matching_routes (
    id uuid PRIMARY KEY,
    manifest_id uuid NOT NULL REFERENCES bid_matching_manifests(id) ON DELETE RESTRICT,
    project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
    route_kind text NOT NULL CHECK (route_kind IN ('technical', 'commercial')),
    unit_id uuid,
    ordinal integer NOT NULL CHECK (ordinal >= 0),
    empty_policy text NOT NULL CHECK (empty_policy IN ('clear_route', 'skip_unit')),
    route_scope_sha256 kb_sha256 NOT NULL,
    UNIQUE (manifest_id, route_kind, unit_id),
    UNIQUE (manifest_id, ordinal),
    CHECK ((route_kind = 'commercial' AND unit_id IS NULL)
        OR (route_kind = 'technical' AND unit_id IS NOT NULL))
);
CREATE TRIGGER bid_matching_routes_immutable
BEFORE UPDATE OR DELETE ON bid_matching_routes
FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();

CREATE TABLE bid_matching_requirement_artifacts (
    id uuid PRIMARY KEY,
    manifest_id uuid NOT NULL REFERENCES bid_matching_manifests(id) ON DELETE RESTRICT,
    route_id uuid NOT NULL REFERENCES bid_matching_routes(id) ON DELETE RESTRICT,
    clause_id uuid NOT NULL REFERENCES bid_clauses(id) ON DELETE RESTRICT,
    ordinal integer NOT NULL CHECK (ordinal >= 0),
    requirement_text text NOT NULL,
    requirement_sha256 kb_sha256 NOT NULL,
    UNIQUE (route_id, ordinal),
    UNIQUE (route_id, id)
);
CREATE TRIGGER bid_matching_requirement_artifacts_immutable
BEFORE UPDATE OR DELETE ON bid_matching_requirement_artifacts
FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();

CREATE TABLE bid_matching_product_version_artifacts (
    id uuid PRIMARY KEY,
    manifest_id uuid NOT NULL REFERENCES bid_matching_manifests(id) ON DELETE RESTRICT,
    product_id uuid,
    product_version_id uuid NOT NULL,
    workspace_kind text NOT NULL CHECK (workspace_kind IN ('product_line', 'company')),
    frozen_display_name text NOT NULL,
    identity_sha256 kb_sha256 NOT NULL,
    UNIQUE (manifest_id, product_version_id)
);
CREATE TRIGGER bid_matching_product_version_artifacts_immutable
BEFORE UPDATE OR DELETE ON bid_matching_product_version_artifacts
FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();

CREATE TABLE bid_matching_route_memberships (
    route_id uuid NOT NULL REFERENCES bid_matching_routes(id) ON DELETE RESTRICT,
    product_version_artifact_id uuid NOT NULL REFERENCES bid_matching_product_version_artifacts(id) ON DELETE RESTRICT,
    route_product_ordinal integer NOT NULL CHECK (route_product_ordinal >= 0),
    PRIMARY KEY (route_id, product_version_artifact_id),
    UNIQUE (route_id, route_product_ordinal)
);
CREATE TRIGGER bid_matching_route_memberships_immutable
BEFORE UPDATE OR DELETE ON bid_matching_route_memberships
FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();

CREATE TABLE bid_matching_jobs (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
    manifest_id uuid NOT NULL REFERENCES bid_matching_manifests(id) ON DELETE RESTRICT,
    route_id uuid NOT NULL UNIQUE REFERENCES bid_matching_routes(id) ON DELETE RESTRICT,
    status text NOT NULL CHECK (status IN ('pending', 'running', 'completed', 'failed', 'superseded')),
    max_attempts integer NOT NULL CHECK (max_attempts BETWEEN 1 AND 32),
    claim_lease_ms integer NOT NULL CHECK (claim_lease_ms BETWEEN 1000 AND 3600000),
    lease_policy_generation bigint NOT NULL CHECK (lease_policy_generation >= 0),
    active_attempt integer,
    completed_report_id uuid,
    error_code text,
    error_detail text CHECK (error_detail IS NULL OR octet_length(error_detail) <= 4096),
    started_at timestamptz,
    finished_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    CHECK ((status = 'running') = (active_attempt IS NOT NULL)),
    CHECK ((status = 'completed') = (completed_report_id IS NOT NULL))
);
CREATE TABLE bid_matching_job_claims (
    job_id uuid NOT NULL REFERENCES bid_matching_jobs(id) ON DELETE RESTRICT,
    attempt integer NOT NULL CHECK (attempt > 0),
    claim_token uuid NOT NULL,
    claim_lease_ms integer NOT NULL CHECK (claim_lease_ms BETWEEN 1000 AND 3600000),
    lease_policy_generation bigint NOT NULL CHECK (lease_policy_generation >= 0),
    claimed_at timestamptz NOT NULL,
    heartbeat_at timestamptz NOT NULL,
    status text NOT NULL CHECK (status IN ('running', 'completed', 'failed', 'reaped')),
    PRIMARY KEY (job_id, attempt),
    UNIQUE (job_id, claim_token)
);

CREATE TABLE bid_matching_staging_sets (
    id uuid PRIMARY KEY,
    job_id uuid NOT NULL REFERENCES bid_matching_jobs(id) ON DELETE RESTRICT,
    route_id uuid NOT NULL REFERENCES bid_matching_routes(id) ON DELETE RESTRICT,
    claim_token uuid NOT NULL,
    attempt integer NOT NULL CHECK (attempt > 0),
    manifest_id uuid NOT NULL REFERENCES bid_matching_manifests(id) ON DELETE RESTRICT,
    project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
    generation bigint NOT NULL,
    mutation_watermark bigint NOT NULL,
    report_nonce uuid NOT NULL,
    state text NOT NULL CHECK (state IN ('active', 'consumed', 'expired', 'failed')),
    expires_at timestamptz NOT NULL,
    open_payload_sha256 kb_sha256 NOT NULL,
    expected_batch_count integer NOT NULL CHECK (expected_batch_count BETWEEN 6 AND 100000),
    expected_item_count bigint NOT NULL CHECK (expected_item_count BETWEEN 0 AND 100000),
    expected_byte_length bigint NOT NULL CHECK (expected_byte_length BETWEEN 0 AND 67108864),
    staged_item_count bigint NOT NULL DEFAULT 0 CHECK (staged_item_count BETWEEN 0 AND 100000),
    staged_byte_length bigint NOT NULL DEFAULT 0 CHECK (staged_byte_length BETWEEN 0 AND 67108864),
    consumed_report_id uuid,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (job_id, claim_token, attempt, route_id),
    UNIQUE (id, report_nonce),
    CHECK ((state = 'consumed') = (consumed_report_id IS NOT NULL))
);
CREATE TABLE bid_matching_staged_batches (
    staging_set_id uuid NOT NULL REFERENCES bid_matching_staging_sets(id) ON DELETE RESTRICT,
    batch_ordinal integer NOT NULL CHECK (batch_ordinal >= 0),
    collection_kind text NOT NULL CHECK (collection_kind IN (
        'source_artifacts', 'candidates', 'evidences', 'requirement_decisions',
        'candidate_groups', 'reason_codes'
    )),
    canonical_items bytea NOT NULL CHECK (octet_length(canonical_items) <= 1048576),
    payload_sha256 kb_sha256 NOT NULL,
    item_count integer NOT NULL CHECK (item_count BETWEEN 0 AND 10000),
    byte_length bigint NOT NULL CHECK (byte_length = octet_length(canonical_items)),
    PRIMARY KEY (staging_set_id, batch_ordinal),
    CHECK (payload_sha256 = encode(digest(canonical_items, 'sha256'), 'hex'))
);
CREATE TABLE bid_matching_staging_report_payloads (
    staging_set_id uuid PRIMARY KEY REFERENCES bid_matching_staging_sets(id) ON DELETE RESTRICT,
    canonical_payload bytea NOT NULL CHECK (octet_length(canonical_payload) <= 67108864),
    content_sha256 kb_sha256 NOT NULL,
    CHECK (content_sha256 = encode(digest(canonical_payload, 'sha256'), 'hex'))
);
CREATE TABLE bid_matching_staged_source_artifacts (
    staging_set_id uuid NOT NULL REFERENCES bid_matching_staging_sets(id) ON DELETE CASCADE,
    id uuid NOT NULL, batch_ordinal integer NOT NULL, item_ordinal integer NOT NULL,
    product_version_artifact_id uuid NOT NULL, document_id uuid NOT NULL, source_chunk_id uuid NOT NULL,
    frozen_document_display_name text NOT NULL, chunk_utf8 bytea NOT NULL,
    chunk_sha256 kb_sha256 NOT NULL, chunk_byte_length bigint NOT NULL,
    retrieval_rank integer NOT NULL, retrieval_raw_score numeric(20,10) NOT NULL,
    retrieval_contract_version text NOT NULL,
    PRIMARY KEY(staging_set_id,id), UNIQUE(staging_set_id,batch_ordinal,item_ordinal),
    CHECK(chunk_byte_length=octet_length(chunk_utf8)),
    CHECK(chunk_sha256=encode(digest(chunk_utf8,'sha256'),'hex'))
);
CREATE TABLE bid_matching_staged_candidates (
    staging_set_id uuid NOT NULL REFERENCES bid_matching_staging_sets(id) ON DELETE CASCADE,
    id uuid NOT NULL, batch_ordinal integer NOT NULL, item_ordinal integer NOT NULL,
    requirement_artifact_id uuid NOT NULL, product_version_artifact_id uuid NOT NULL,
    route_product_ordinal integer NOT NULL, retrieval_rank integer NOT NULL,
    retrieval_raw_score numeric(20,10) NOT NULL, candidate_identity_sha256 kb_sha256 NOT NULL,
    evidence_v1_sha256 kb_sha256 NOT NULL,
    support text NOT NULL CHECK(support IN ('supported','unresolved','insufficient','contradicted')),
    business_value_status text NOT NULL CHECK(business_value_status IN ('scored','not_scored')),
    business_value numeric(20,6), recommended boolean NOT NULL,
    PRIMARY KEY(staging_set_id,id), UNIQUE(staging_set_id,batch_ordinal,item_ordinal),
    CHECK((business_value_status='scored')=(business_value IS NOT NULL))
);
CREATE TABLE bid_matching_staged_evidences (
    staging_set_id uuid NOT NULL REFERENCES bid_matching_staging_sets(id) ON DELETE CASCADE,
    id uuid NOT NULL, batch_ordinal integer NOT NULL, item_ordinal integer NOT NULL,
    candidate_artifact_id uuid NOT NULL, source_chunk_artifact_id uuid NOT NULL,
    document_id uuid NOT NULL, document_display_name text NOT NULL, source_chunk_id uuid NOT NULL,
    source_chunk_sha256 kb_sha256 NOT NULL, quote text NOT NULL,
    start_offset bigint NOT NULL, end_offset bigint NOT NULL,
    offset_unit text NOT NULL CHECK(offset_unit='utf8_byte'), ordinal integer NOT NULL,
    PRIMARY KEY(staging_set_id,id), UNIQUE(staging_set_id,batch_ordinal,item_ordinal),
    UNIQUE(staging_set_id,candidate_artifact_id,ordinal), CHECK(end_offset>start_offset)
);
CREATE TABLE bid_matching_staged_requirement_decisions (
    staging_set_id uuid NOT NULL REFERENCES bid_matching_staging_sets(id) ON DELETE CASCADE,
    id uuid NOT NULL, batch_ordinal integer NOT NULL, item_ordinal integer NOT NULL,
    requirement_artifact_id uuid NOT NULL, final_support text NOT NULL,
    system_decision text NOT NULL, quality_status text NOT NULL, reason_code text NOT NULL,
    selected_candidate_artifact_id uuid, ordinal integer NOT NULL,
    PRIMARY KEY(staging_set_id,id), UNIQUE(staging_set_id,batch_ordinal,item_ordinal),
    UNIQUE(staging_set_id,requirement_artifact_id), UNIQUE(staging_set_id,ordinal),
    CHECK(final_support IN ('supported','unresolved','insufficient','contradicted')),
    CHECK(system_decision IN ('select','review','reject')), CHECK(quality_status IN ('pass','review','block'))
);
CREATE TABLE bid_matching_staged_candidate_groups (
    staging_set_id uuid NOT NULL REFERENCES bid_matching_staging_sets(id) ON DELETE CASCADE,
    id uuid NOT NULL, batch_ordinal integer NOT NULL, item_ordinal integer NOT NULL,
    requirement_artifact_id uuid NOT NULL, support text NOT NULL, ordinal integer NOT NULL,
    canonical_payload bytea NOT NULL, content_sha256 kb_sha256 NOT NULL,
    PRIMARY KEY(staging_set_id,id), UNIQUE(staging_set_id,batch_ordinal,item_ordinal),
    UNIQUE(staging_set_id,ordinal), CHECK(content_sha256=encode(digest(canonical_payload,'sha256'),'hex'))
);
CREATE TABLE bid_matching_staged_reason_codes (
    staging_set_id uuid NOT NULL REFERENCES bid_matching_staging_sets(id) ON DELETE CASCADE,
    batch_ordinal integer NOT NULL, item_ordinal integer NOT NULL, reason_code text NOT NULL,
    PRIMARY KEY(staging_set_id,batch_ordinal,item_ordinal), UNIQUE(staging_set_id,reason_code)
);

-- Immediate bidding-owned freeze of every KnowledgeRetrievalPort hit. These
-- scalar source identities intentionally have no FK to live knowledge rows.
CREATE TABLE bid_matching_frozen_retrieved_hits (
    id uuid PRIMARY KEY,
    manifest_id uuid NOT NULL REFERENCES bid_matching_manifests(id) ON DELETE RESTRICT,
    route_id uuid NOT NULL REFERENCES bid_matching_routes(id) ON DELETE RESTRICT,
    requirement_artifact_id uuid NOT NULL REFERENCES bid_matching_requirement_artifacts(id) ON DELETE RESTRICT,
    product_version_artifact_id uuid NOT NULL REFERENCES bid_matching_product_version_artifacts(id) ON DELETE RESTRICT,
    document_id uuid NOT NULL,
    source_chunk_id uuid NOT NULL,
    frozen_document_display_name text NOT NULL,
    chunk_utf8 bytea NOT NULL CHECK (octet_length(chunk_utf8) <= 1048576),
    chunk_sha256 kb_sha256 NOT NULL,
    chunk_byte_length bigint NOT NULL CHECK (chunk_byte_length = octet_length(chunk_utf8)),
    retrieval_rank integer NOT NULL CHECK (retrieval_rank > 0),
    retrieval_raw_score numeric(20,10) NOT NULL,
    quote_start_offset bigint NOT NULL CHECK (quote_start_offset >= 0),
    quote_end_offset bigint NOT NULL,
    offset_unit text NOT NULL CHECK (offset_unit = 'utf8_byte'),
    retrieval_contract_version text NOT NULL,
    UNIQUE (route_id, requirement_artifact_id, product_version_artifact_id, document_id,
            source_chunk_id, quote_start_offset, quote_end_offset),
    CHECK (quote_end_offset > quote_start_offset AND quote_end_offset <= chunk_byte_length),
    CHECK (chunk_sha256 = encode(digest(chunk_utf8, 'sha256'), 'hex'))
);
CREATE TRIGGER bid_matching_frozen_retrieved_hits_immutable
BEFORE UPDATE OR DELETE ON bid_matching_frozen_retrieved_hits
FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();

CREATE TABLE bid_matching_source_artifacts (
    id uuid PRIMARY KEY,
    report_id uuid NOT NULL,
    product_version_artifact_id uuid NOT NULL REFERENCES bid_matching_product_version_artifacts(id) ON DELETE RESTRICT,
    document_id uuid NOT NULL,
    source_chunk_id uuid NOT NULL,
    frozen_document_display_name text NOT NULL,
    chunk_utf8 bytea NOT NULL,
    chunk_sha256 kb_sha256 NOT NULL,
    chunk_byte_length bigint NOT NULL CHECK (chunk_byte_length = octet_length(chunk_utf8)),
    retrieval_rank integer NOT NULL CHECK (retrieval_rank > 0),
    retrieval_raw_score numeric(20,10),
    retrieval_contract_version text NOT NULL,
    UNIQUE (report_id, id),
    CHECK (chunk_sha256 = encode(digest(chunk_utf8, 'sha256'), 'hex'))
);

CREATE TABLE bid_matching_candidate_artifacts (
    id uuid PRIMARY KEY,
    report_id uuid NOT NULL,
    requirement_artifact_id uuid NOT NULL REFERENCES bid_matching_requirement_artifacts(id) ON DELETE RESTRICT,
    product_version_artifact_id uuid REFERENCES bid_matching_product_version_artifacts(id) ON DELETE RESTRICT,
    support text NOT NULL CHECK (support IN ('supported', 'unresolved', 'insufficient', 'contradicted')),
    candidate_identity_sha256 kb_sha256 NOT NULL,
    evidence_v1_sha256 kb_sha256 NOT NULL,
    business_value_status text NOT NULL CHECK (business_value_status IN ('scored', 'not_scored')),
    business_value numeric(20,6),
    route_product_ordinal integer NOT NULL CHECK (route_product_ordinal >= 0),
    retrieval_rank integer NOT NULL CHECK (retrieval_rank > 0),
    retrieval_raw_score numeric(20,10) NOT NULL,
    recommended boolean NOT NULL,
    UNIQUE (report_id, id),
    UNIQUE (report_id, requirement_artifact_id, candidate_identity_sha256, evidence_v1_sha256),
    CHECK ((business_value_status = 'scored') = (business_value IS NOT NULL))
);

CREATE TABLE bid_matching_evidence_artifacts (
    id uuid PRIMARY KEY,
    report_id uuid NOT NULL,
    candidate_artifact_id uuid NOT NULL REFERENCES bid_matching_candidate_artifacts(id) ON DELETE RESTRICT,
    source_chunk_artifact_id uuid NOT NULL REFERENCES bid_matching_source_artifacts(id) ON DELETE RESTRICT,
    document_id uuid NOT NULL,
    document_display_name text NOT NULL,
    source_chunk_id uuid NOT NULL,
    source_chunk_sha256 kb_sha256 NOT NULL,
    start_offset bigint NOT NULL CHECK (start_offset >= 0),
    end_offset bigint NOT NULL,
    offset_unit text NOT NULL CHECK (offset_unit = 'utf8_byte'),
    quote_utf8 bytea NOT NULL,
    quote_sha256 kb_sha256 NOT NULL,
    ordinal integer NOT NULL CHECK (ordinal >= 0),
    UNIQUE (candidate_artifact_id, ordinal),
    CHECK (end_offset > start_offset),
    CHECK (quote_sha256 = encode(digest(quote_utf8, 'sha256'), 'hex'))
);

CREATE TABLE bid_matching_requirement_decisions (
    id uuid PRIMARY KEY,
    report_id uuid NOT NULL,
    requirement_artifact_id uuid NOT NULL REFERENCES bid_matching_requirement_artifacts(id) ON DELETE RESTRICT,
    final_support text NOT NULL CHECK (final_support IN ('supported', 'unresolved', 'insufficient', 'contradicted')),
    system_decision text NOT NULL CHECK (system_decision IN ('select', 'review', 'reject')),
    quality_status text NOT NULL CHECK (quality_status IN ('pass', 'review', 'block')),
    reason_code text NOT NULL CHECK (reason_code IN (
        'SUPPORTED', 'UNRESOLVED', 'INSUFFICIENT', 'CONTRADICTED', 'NO_EVIDENCE'
    )),
    selected_candidate_artifact_id uuid REFERENCES bid_matching_candidate_artifacts(id) ON DELETE RESTRICT,
    ordinal integer NOT NULL CHECK (ordinal >= 0),
    UNIQUE (report_id, requirement_artifact_id),
    UNIQUE (report_id, ordinal),
    CHECK ((final_support = 'supported') = (selected_candidate_artifact_id IS NOT NULL))
);

CREATE TABLE bid_matching_candidate_groups (
    id uuid PRIMARY KEY,
    report_id uuid NOT NULL,
    requirement_artifact_id uuid NOT NULL REFERENCES bid_matching_requirement_artifacts(id) ON DELETE RESTRICT,
    support text NOT NULL CHECK (support IN ('supported', 'unresolved', 'insufficient', 'contradicted')),
    ordinal integer NOT NULL CHECK (ordinal >= 0),
    canonical_payload bytea NOT NULL,
    content_sha256 kb_sha256 NOT NULL,
    UNIQUE (report_id, requirement_artifact_id, support),
    CHECK (content_sha256 = encode(digest(canonical_payload, 'sha256'), 'hex'))
);

CREATE TABLE bid_matching_reports (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
    manifest_id uuid NOT NULL REFERENCES bid_matching_manifests(id) ON DELETE RESTRICT,
    job_id uuid NOT NULL REFERENCES bid_matching_jobs(id) ON DELETE RESTRICT,
    route_id uuid NOT NULL REFERENCES bid_matching_routes(id) ON DELETE RESTRICT,
    generation bigint NOT NULL CHECK (generation > 0),
    mutation_watermark bigint NOT NULL CHECK (mutation_watermark >= 0),
    empty_disposition text CHECK (empty_disposition IN ('clear_route', 'skip_unit')),
    coverage_total integer NOT NULL CHECK (coverage_total >= 0),
    coverage_supported integer NOT NULL CHECK (coverage_supported >= 0),
    coverage_contradicted integer NOT NULL CHECK (coverage_contradicted >= 0),
    coverage_insufficient integer NOT NULL CHECK (coverage_insufficient >= 0),
    coverage_unresolved integer NOT NULL CHECK (coverage_unresolved >= 0),
    quality_status text NOT NULL CHECK (quality_status IN ('pass', 'review', 'block')),
    degraded boolean NOT NULL,
    reason_codes text[] NOT NULL,
    canonical_payload bytea NOT NULL,
    content_sha256 kb_sha256 NOT NULL UNIQUE,
    ai_run_id uuid,
    ai_span_id uuid,
    published_at timestamptz NOT NULL,
    UNIQUE (route_id, generation),
    UNIQUE (project_id, id),
    CHECK (coverage_total = coverage_supported + coverage_contradicted
        + coverage_insufficient + coverage_unresolved),
    CHECK (degraded = (quality_status <> 'pass')),
    CHECK (content_sha256 = encode(digest(canonical_payload, 'sha256'), 'hex'))
);
CREATE TRIGGER bid_matching_reports_immutable
BEFORE UPDATE OR DELETE ON bid_matching_reports
FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();

-- Add report ownership only after the report relation exists. These are final
-- graph constraints, not upgrade/repair steps.
ALTER TABLE bid_matching_source_artifacts
    ADD CONSTRAINT bid_matching_source_artifacts_report_fk
    FOREIGN KEY (report_id) REFERENCES bid_matching_reports(id) ON DELETE RESTRICT;
ALTER TABLE bid_matching_candidate_artifacts
    ADD CONSTRAINT bid_matching_candidate_artifacts_report_fk
    FOREIGN KEY (report_id) REFERENCES bid_matching_reports(id) ON DELETE RESTRICT;
ALTER TABLE bid_matching_evidence_artifacts
    ADD CONSTRAINT bid_matching_evidence_artifacts_report_fk
    FOREIGN KEY (report_id) REFERENCES bid_matching_reports(id) ON DELETE RESTRICT;
ALTER TABLE bid_matching_evidence_artifacts
    ADD CONSTRAINT bid_matching_evidence_candidate_scope_fk
    FOREIGN KEY (report_id,candidate_artifact_id)
    REFERENCES bid_matching_candidate_artifacts(report_id,id) ON DELETE RESTRICT;
ALTER TABLE bid_matching_evidence_artifacts
    ADD CONSTRAINT bid_matching_evidence_source_scope_fk
    FOREIGN KEY (report_id,source_chunk_artifact_id)
    REFERENCES bid_matching_source_artifacts(report_id,id) ON DELETE RESTRICT;
ALTER TABLE bid_matching_requirement_decisions
    ADD CONSTRAINT bid_matching_requirement_decisions_report_fk
    FOREIGN KEY (report_id) REFERENCES bid_matching_reports(id) ON DELETE RESTRICT;
ALTER TABLE bid_matching_requirement_decisions
    ADD CONSTRAINT bid_matching_decision_selected_scope_fk
    FOREIGN KEY (report_id,selected_candidate_artifact_id)
    REFERENCES bid_matching_candidate_artifacts(report_id,id) ON DELETE RESTRICT;
ALTER TABLE bid_matching_candidate_groups
    ADD CONSTRAINT bid_matching_candidate_groups_report_fk
    FOREIGN KEY (report_id) REFERENCES bid_matching_reports(id) ON DELETE RESTRICT;
ALTER TABLE bid_matching_jobs
    ADD CONSTRAINT bid_matching_jobs_completed_report_fk
    FOREIGN KEY (completed_report_id) REFERENCES bid_matching_reports(id) ON DELETE RESTRICT;
ALTER TABLE bid_matching_staging_sets
    ADD CONSTRAINT bid_matching_staging_consumed_report_fk
    FOREIGN KEY (consumed_report_id) REFERENCES bid_matching_reports(id) ON DELETE RESTRICT;

CREATE FUNCTION kb_match_verify_evidence_v1()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, public
AS $$
DECLARE source_value bid_matching_source_artifacts%ROWTYPE;
BEGIN
    SELECT * INTO STRICT source_value FROM bid_matching_source_artifacts
     WHERE id=NEW.source_chunk_artifact_id AND report_id=NEW.report_id FOR SHARE;
    IF NEW.candidate_artifact_id IS NULL
       OR NEW.document_id<>source_value.document_id
       OR NEW.document_display_name<>source_value.frozen_document_display_name
       OR NEW.source_chunk_id<>source_value.source_chunk_id
       OR NEW.source_chunk_sha256<>source_value.chunk_sha256
       OR NEW.end_offset>source_value.chunk_byte_length
       OR NEW.quote_utf8<>substring(source_value.chunk_utf8
              FROM NEW.start_offset::integer+1
              FOR (NEW.end_offset-NEW.start_offset)::integer)
    THEN
        RAISE EXCEPTION 'EVIDENCE_V1_BYTE_SLICE_MISMATCH' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END
$$;
CREATE TRIGGER bid_matching_evidence_verify
BEFORE INSERT ON bid_matching_evidence_artifacts
FOR EACH ROW EXECUTE FUNCTION kb_match_verify_evidence_v1();

CREATE FUNCTION kb_match_verify_report_v1()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, public
AS $$
DECLARE parsed jsonb; decision_count integer; supported_count integer;
 contradicted_count integer; insufficient_count integer; unresolved_count integer;
 expected_quality text; expected_reasons text[]; relation_reasons text[];
 candidate_count integer; source_count integer; group_count integer;
BEGIN
    BEGIN parsed:=convert_from(NEW.canonical_payload,'UTF8')::jsonb;
    EXCEPTION WHEN OTHERS THEN
      RAISE EXCEPTION 'MATCHING_REPORT_V1_INVALID_JSON' USING ERRCODE='23514';
    END;
    IF parsed->>'schema_version'<>'1' OR parsed->>'report_id'<>NEW.id::text
       OR parsed->>'manifest_id'<>NEW.manifest_id::text OR parsed->>'job_id'<>NEW.job_id::text
       OR parsed->>'route_id'<>NEW.route_id::text
       OR (parsed->>'generation')::bigint<>NEW.generation
       OR (parsed->>'mutation_watermark')::bigint<>NEW.mutation_watermark
       OR (SELECT count(*) FROM jsonb_object_keys(parsed))<>20 THEN
      RAISE EXCEPTION 'MATCHING_REPORT_V1_HEADER_MISMATCH' USING ERRCODE='23514';
    END IF;
    SELECT count(*),count(*) FILTER(WHERE final_support='supported'),
      count(*) FILTER(WHERE final_support='contradicted'),
      count(*) FILTER(WHERE final_support='insufficient'),
      count(*) FILTER(WHERE final_support='unresolved')
      INTO decision_count,supported_count,contradicted_count,insufficient_count,unresolved_count
      FROM bid_matching_requirement_decisions WHERE report_id=NEW.id;
    IF decision_count=0 THEN expected_quality:='review';
    ELSIF EXISTS(SELECT 1 FROM bid_matching_requirement_decisions WHERE report_id=NEW.id AND quality_status='block') THEN expected_quality:='block';
    ELSIF EXISTS(SELECT 1 FROM bid_matching_requirement_decisions WHERE report_id=NEW.id AND quality_status='review') THEN expected_quality:='review';
    ELSE expected_quality:='pass'; END IF;
    SELECT ARRAY(SELECT DISTINCT code FROM (
      SELECT 'FROZEN_SCOPE'::text code
      UNION ALL SELECT reason_code FROM bid_matching_requirement_decisions WHERE report_id=NEW.id
      UNION ALL SELECT 'EMPTY_ROUTE' WHERE decision_count=0
      UNION ALL SELECT 'SKIP_UNIT' WHERE decision_count=0 AND NEW.empty_disposition='skip_unit'
    ) reason_values ORDER BY code) INTO expected_reasons;
    SELECT ARRAY(SELECT jsonb_array_elements_text(parsed->'reason_codes') ORDER BY 1) INTO relation_reasons;
    SELECT count(*) INTO candidate_count FROM bid_matching_candidate_artifacts WHERE report_id=NEW.id;
    SELECT count(*) INTO source_count FROM bid_matching_source_artifacts WHERE report_id=NEW.id;
    SELECT count(*) INTO group_count FROM bid_matching_candidate_groups WHERE report_id=NEW.id;
    IF decision_count<>NEW.coverage_total OR supported_count<>NEW.coverage_supported
       OR contradicted_count<>NEW.coverage_contradicted OR insufficient_count<>NEW.coverage_insufficient
       OR unresolved_count<>NEW.coverage_unresolved OR expected_quality<>NEW.quality_status
       OR NEW.degraded<>(expected_quality<>'pass') OR expected_reasons<>NEW.reason_codes
       OR relation_reasons<>expected_reasons
       OR jsonb_array_length(parsed->'requirement_decisions')<>decision_count
       OR jsonb_array_length(parsed->'candidates')<>candidate_count
       OR jsonb_array_length(parsed->'candidate_groups')<>group_count
       OR jsonb_array_length(parsed->'source_artifacts')<>source_count
    THEN
      RAISE EXCEPTION 'MATCHING_REPORT_V1_RELATION_MISMATCH' USING ERRCODE='23514';
    END IF;
    IF EXISTS(
      (SELECT value->>'requirement_artifact_id',value->>'final_support',value->>'system_decision',
              value->>'quality_status',value->>'reason_code',value->>'selected_candidate_artifact_id'
         FROM jsonb_array_elements(parsed->'requirement_decisions') value
       EXCEPT
       SELECT requirement_artifact_id::text,final_support,system_decision,quality_status,reason_code,
              selected_candidate_artifact_id::text
         FROM bid_matching_requirement_decisions WHERE report_id=NEW.id)
      UNION ALL
      (SELECT requirement_artifact_id::text,final_support,system_decision,quality_status,reason_code,
              selected_candidate_artifact_id::text
         FROM bid_matching_requirement_decisions WHERE report_id=NEW.id
       EXCEPT
       SELECT value->>'requirement_artifact_id',value->>'final_support',value->>'system_decision',
              value->>'quality_status',value->>'reason_code',value->>'selected_candidate_artifact_id'
         FROM jsonb_array_elements(parsed->'requirement_decisions') value)
    ) OR EXISTS(
      SELECT value->>'id',value->>'requirement_artifact_id',value->>'product_version_artifact_id',
             value->>'candidate_identity_sha256',value->>'evidence_v1_sha256',value->>'support',
             value->>'route_product_ordinal',value->>'retrieval_rank',value->>'retrieval_raw_score',value->>'recommended'
        FROM jsonb_array_elements(parsed->'candidates') value
      EXCEPT
      SELECT id::text,requirement_artifact_id::text,product_version_artifact_id::text,
             candidate_identity_sha256,evidence_v1_sha256,support,route_product_ordinal::text,
             retrieval_rank::text,to_char(retrieval_raw_score,'FM99999999999999999990.000000'),recommended::text
        FROM bid_matching_candidate_artifacts WHERE report_id=NEW.id
    ) OR EXISTS(
      SELECT value::text FROM jsonb_array_elements(parsed->'candidate_groups') value
      EXCEPT
      SELECT (convert_from(canonical_payload,'UTF8')::jsonb)::text
        FROM bid_matching_candidate_groups WHERE report_id=NEW.id
    ) OR EXISTS(
      SELECT value->>'id',value->>'product_version_artifact_id',value->>'document_id',value->>'source_chunk_id',
             value->>'frozen_document_display_name',value->>'chunk_sha256',value->>'chunk_byte_length',
             value->>'retrieval_rank',value->>'retrieval_raw_score',value->>'retrieval_contract_version'
        FROM jsonb_array_elements(parsed->'source_artifacts') value
      EXCEPT
      SELECT id::text,product_version_artifact_id::text,document_id::text,source_chunk_id::text,
             frozen_document_display_name,chunk_sha256,chunk_byte_length::text,retrieval_rank::text,
             to_char(retrieval_raw_score,'FM99999999999999999990.000000'),retrieval_contract_version
        FROM bid_matching_source_artifacts WHERE report_id=NEW.id
    ) OR EXISTS(
      SELECT candidate_value->>'id',item->>'source_chunk_artifact_id',item->>'document_id',
             item->>'document_display_name',item->>'source_chunk_id',item->>'source_chunk_sha256',
             item->>'quote',item->>'start_offset',item->>'end_offset',item->>'offset_unit'
        FROM jsonb_array_elements(parsed->'candidates') candidate_value
        CROSS JOIN LATERAL jsonb_array_elements(candidate_value->'evidence'->'items') item
      EXCEPT
      SELECT candidate_artifact_id::text,source_chunk_artifact_id::text,document_id::text,
             document_display_name,source_chunk_id::text,source_chunk_sha256,convert_from(quote_utf8,'UTF8'),
             start_offset::text,end_offset::text,offset_unit
        FROM bid_matching_evidence_artifacts WHERE report_id=NEW.id
    ) THEN RAISE EXCEPTION 'MATCHING_REPORT_V1_PAYLOAD_RELATION_MISMATCH' USING ERRCODE='23514'; END IF;
    IF EXISTS(
      SELECT 1 FROM bid_matching_requirement_decisions d
      LEFT JOIN LATERAL (
        SELECT c.id FROM bid_matching_candidate_artifacts c
        WHERE c.report_id=d.report_id AND c.requirement_artifact_id=d.requirement_artifact_id
          AND c.support='supported'
        ORDER BY c.route_product_ordinal,c.retrieval_rank,c.candidate_identity_sha256,c.evidence_v1_sha256
        LIMIT 1
      ) expected ON true
      CROSS JOIN LATERAL (
        SELECT CASE
          WHEN bool_or(c.support='supported') THEN 'supported'
          WHEN bool_or(c.support='unresolved') THEN 'unresolved'
          WHEN bool_or(c.support='insufficient') THEN 'insufficient'
          WHEN count(c.id)>0 THEN 'contradicted'
          ELSE 'insufficient' END AS final_support,
          count(c.id)=0 AS no_evidence
        FROM bid_matching_candidate_artifacts c
        WHERE c.report_id=d.report_id AND c.requirement_artifact_id=d.requirement_artifact_id
      ) aggregate_value
      WHERE d.report_id=NEW.id AND (
        d.final_support<>aggregate_value.final_support
        OR (d.final_support='supported' AND (d.system_decision<>'select' OR d.quality_status<>'pass'
          OR d.reason_code<>'SUPPORTED' OR d.selected_candidate_artifact_id IS DISTINCT FROM expected.id))
        OR (d.final_support='unresolved' AND (d.system_decision<>'review' OR d.quality_status<>'review' OR d.reason_code<>'UNRESOLVED'))
        OR (d.final_support='insufficient' AND (d.system_decision<>'review' OR d.quality_status<>'review'
          OR d.reason_code<>CASE WHEN aggregate_value.no_evidence THEN 'NO_EVIDENCE' ELSE 'INSUFFICIENT' END))
        OR (d.final_support='contradicted' AND (d.system_decision<>'reject' OR d.quality_status<>'block' OR d.reason_code<>'CONTRADICTED'))
      )
    ) OR EXISTS(
      SELECT 1 FROM bid_matching_candidate_artifacts candidate
      LEFT JOIN bid_matching_route_memberships membership
        ON membership.route_id=NEW.route_id
       AND membership.product_version_artifact_id=candidate.product_version_artifact_id
      WHERE candidate.report_id=NEW.id AND (membership.product_version_artifact_id IS NULL
        OR membership.route_product_ordinal<>candidate.route_product_ordinal
        OR candidate.recommended<>(candidate.id=(SELECT decision.selected_candidate_artifact_id
             FROM bid_matching_requirement_decisions decision
             WHERE decision.report_id=NEW.id AND decision.requirement_artifact_id=candidate.requirement_artifact_id)))
    ) THEN RAISE EXCEPTION 'REQUIREMENT_DECISION_V1_AGGREGATION_MISMATCH' USING ERRCODE='23514'; END IF;
    RETURN NEW;
END
$$;
CREATE CONSTRAINT TRIGGER bid_matching_reports_verify
AFTER INSERT ON bid_matching_reports DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION kb_match_verify_report_v1();

CREATE TRIGGER bid_matching_source_artifacts_immutable
BEFORE UPDATE OR DELETE ON bid_matching_source_artifacts
FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();
CREATE TRIGGER bid_matching_candidate_artifacts_immutable
BEFORE UPDATE OR DELETE ON bid_matching_candidate_artifacts
FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();
CREATE TRIGGER bid_matching_evidence_artifacts_immutable
BEFORE UPDATE OR DELETE ON bid_matching_evidence_artifacts
FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();
CREATE TRIGGER bid_matching_requirement_decisions_immutable
BEFORE UPDATE OR DELETE ON bid_matching_requirement_decisions
FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();
CREATE TRIGGER bid_matching_candidate_groups_immutable
BEFORE UPDATE OR DELETE ON bid_matching_candidate_groups
FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();

CREATE TABLE bid_current_matching_reports (
    project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
    route_id uuid NOT NULL REFERENCES bid_matching_routes(id) ON DELETE RESTRICT,
    report_id uuid NOT NULL,
    generation bigint NOT NULL,
    mutation_watermark bigint NOT NULL,
    PRIMARY KEY (project_id, route_id),
    FOREIGN KEY (project_id, report_id) REFERENCES bid_matching_reports(project_id, id) ON DELETE RESTRICT
);

CREATE TABLE bid_route_pick_set_artifacts (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
    route_id uuid NOT NULL REFERENCES bid_matching_routes(id) ON DELETE RESTRICT,
    source_report_artifact_id uuid NOT NULL REFERENCES bid_matching_reports(id) ON DELETE RESTRICT,
    report_generation bigint NOT NULL,
    report_sha256 kb_sha256 NOT NULL,
    route_unit_id uuid,
    revision bigint NOT NULL CHECK (revision > 0),
    canonical_payload bytea NOT NULL,
    content_sha256 kb_sha256 NOT NULL UNIQUE,
    selected_by kb_actor_identity NOT NULL,
    selected_at timestamptz NOT NULL,
    UNIQUE (project_id, route_id, revision),
    UNIQUE (project_id, id),
    CHECK (content_sha256 = encode(digest(canonical_payload, 'sha256'), 'hex'))
);
CREATE TABLE bid_route_pick_set_items (
    pick_set_id uuid NOT NULL REFERENCES bid_route_pick_set_artifacts(id) ON DELETE RESTRICT,
    ordinal integer NOT NULL CHECK (ordinal >= 0),
    requirement_artifact_id uuid NOT NULL REFERENCES bid_matching_requirement_artifacts(id) ON DELETE RESTRICT,
    candidate_artifact_id uuid NOT NULL REFERENCES bid_matching_candidate_artifacts(id) ON DELETE RESTRICT,
    product_id uuid,
    product_version_id uuid NOT NULL,
    source_report_artifact_id uuid NOT NULL REFERENCES bid_matching_reports(id) ON DELETE RESTRICT,
    unit_id uuid,
    selected_by kb_actor_identity NOT NULL,
    selected_at timestamptz NOT NULL,
    PRIMARY KEY (pick_set_id, ordinal),
    UNIQUE (pick_set_id, requirement_artifact_id, candidate_artifact_id)
);
CREATE TABLE bid_current_route_pick_sets (
    project_id uuid NOT NULL,
    route_id uuid NOT NULL,
    pick_set_id uuid NOT NULL,
    revision bigint NOT NULL,
    PRIMARY KEY (project_id, route_id),
    FOREIGN KEY (project_id, pick_set_id)
        REFERENCES bid_route_pick_set_artifacts(project_id, id) ON DELETE RESTRICT
);

CREATE TABLE bid_project_pick_set_artifacts (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
    revision bigint NOT NULL CHECK (revision > 0),
    canonical_payload bytea NOT NULL,
    content_sha256 kb_sha256 NOT NULL UNIQUE,
    created_by kb_actor_identity NOT NULL,
    created_at timestamptz NOT NULL,
    UNIQUE (project_id, revision),
    UNIQUE (project_id, id),
    CHECK (content_sha256 = encode(digest(canonical_payload, 'sha256'), 'hex'))
);
CREATE TABLE bid_project_pick_set_items (
    project_pick_set_id uuid NOT NULL REFERENCES bid_project_pick_set_artifacts(id) ON DELETE RESTRICT,
    ordinal integer NOT NULL CHECK (ordinal >= 0),
    route_pick_set_id uuid NOT NULL REFERENCES bid_route_pick_set_artifacts(id) ON DELETE RESTRICT,
    source_report_artifact_id uuid NOT NULL REFERENCES bid_matching_reports(id) ON DELETE RESTRICT,
    requirement_artifact_id uuid NOT NULL,
    candidate_artifact_id uuid NOT NULL,
    product_id uuid,
    product_version_id uuid NOT NULL,
    unit_id uuid,
    PRIMARY KEY (project_pick_set_id, ordinal),
    UNIQUE (project_pick_set_id, route_pick_set_id, requirement_artifact_id, candidate_artifact_id)
);
CREATE TABLE bid_current_project_pick_sets (
    project_id uuid PRIMARY KEY REFERENCES bid_projects(id) ON DELETE RESTRICT,
    pick_set_id uuid NOT NULL,
    revision bigint NOT NULL,
    FOREIGN KEY (project_id, pick_set_id)
        REFERENCES bid_project_pick_set_artifacts(project_id, id) ON DELETE RESTRICT
);
CREATE TRIGGER bid_route_pick_set_artifacts_immutable
BEFORE UPDATE OR DELETE ON bid_route_pick_set_artifacts
FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();
CREATE TRIGGER bid_route_pick_set_items_immutable
BEFORE UPDATE OR DELETE ON bid_route_pick_set_items
FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();
CREATE TRIGGER bid_project_pick_set_artifacts_immutable
BEFORE UPDATE OR DELETE ON bid_project_pick_set_artifacts
FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();
CREATE TRIGGER bid_project_pick_set_items_immutable
BEFORE UPDATE OR DELETE ON bid_project_pick_set_items
FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();

CREATE FUNCTION kb_match_verify_route_pick_set_v1()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, public
AS $$
DECLARE parsed jsonb; report_value bid_matching_reports%ROWTYPE; route_value bid_matching_routes%ROWTYPE;
BEGIN
  SELECT * INTO STRICT report_value FROM bid_matching_reports WHERE id=NEW.source_report_artifact_id FOR SHARE;
  SELECT * INTO STRICT route_value FROM bid_matching_routes WHERE id=NEW.route_id FOR SHARE;
  BEGIN parsed:=convert_from(NEW.canonical_payload,'UTF8')::jsonb;
  EXCEPTION WHEN OTHERS THEN RAISE EXCEPTION 'ROUTE_PICK_SET_V1_INVALID_JSON' USING ERRCODE='23514'; END;
  IF report_value.project_id<>NEW.project_id OR report_value.route_id<>NEW.route_id
     OR route_value.route_kind<>'technical' OR route_value.unit_id IS DISTINCT FROM NEW.route_unit_id
     OR report_value.generation<>NEW.report_generation OR report_value.content_sha256<>NEW.report_sha256
     OR parsed->>'schema_version'<>'1' OR parsed->>'project_id'<>NEW.project_id::text
     OR parsed->>'route_id'<>NEW.route_id::text OR parsed->>'source_report_artifact_id'<>NEW.source_report_artifact_id::text
     OR jsonb_array_length(parsed->'items')<>(SELECT count(*) FROM bid_route_pick_set_items WHERE pick_set_id=NEW.id)
     OR EXISTS(
       SELECT 1 FROM bid_route_pick_set_items item
       LEFT JOIN bid_matching_candidate_artifacts candidate ON candidate.id=item.candidate_artifact_id
       WHERE item.pick_set_id=NEW.id AND (item.source_report_artifact_id<>NEW.source_report_artifact_id
         OR item.unit_id IS DISTINCT FROM NEW.route_unit_id OR candidate.report_id<>NEW.source_report_artifact_id
         OR candidate.requirement_artifact_id<>item.requirement_artifact_id OR candidate.support<>'supported')
     )
  THEN RAISE EXCEPTION 'ROUTE_PICK_SET_V1_RELATION_MISMATCH' USING ERRCODE='23514'; END IF;
  RETURN NEW;
END
$$;
CREATE CONSTRAINT TRIGGER bid_route_pick_set_artifacts_verify
AFTER INSERT ON bid_route_pick_set_artifacts DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION kb_match_verify_route_pick_set_v1();

CREATE FUNCTION kb_match_verify_project_pick_set_v1()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, public
AS $$
DECLARE parsed jsonb; unsectioned_report_id uuid;
BEGIN
  BEGIN parsed:=convert_from(NEW.canonical_payload,'UTF8')::jsonb;
  EXCEPTION WHEN OTHERS THEN RAISE EXCEPTION 'PROJECT_PICK_SET_V1_INVALID_JSON' USING ERRCODE='23514'; END;
  SELECT report.id INTO unsectioned_report_id
   FROM bid_current_matching_reports current_value
   JOIN bid_matching_reports report ON report.id=current_value.report_id
   JOIN bid_matching_routes route ON route.id=report.route_id
   WHERE current_value.project_id=NEW.project_id AND route.route_kind='technical'
     AND route.unit_id='00000000-0000-0000-0000-000000000000'::uuid;
  IF parsed->>'schema_version'<>'1' OR parsed->>'project_id'<>NEW.project_id::text
     OR jsonb_array_length(parsed->'items')<>(SELECT count(*) FROM bid_project_pick_set_items WHERE project_pick_set_id=NEW.id)
     OR EXISTS(
       (SELECT route_item.route_pick_set_id,route_item.source_report_artifact_id,
               route_item.requirement_artifact_id,route_item.candidate_artifact_id,
               route_item.product_id,route_item.product_version_id,route_item.unit_id
          FROM bid_current_route_pick_sets current_value
          JOIN LATERAL (
            SELECT item.pick_set_id AS route_pick_set_id,item.source_report_artifact_id,
                   item.requirement_artifact_id,item.candidate_artifact_id,
                   item.product_id,item.product_version_id,item.unit_id
              FROM bid_route_pick_set_items item WHERE item.pick_set_id=current_value.pick_set_id
          ) route_item ON true WHERE current_value.project_id=NEW.project_id
        EXCEPT
        SELECT item.route_pick_set_id,item.source_report_artifact_id,item.requirement_artifact_id,
               item.candidate_artifact_id,item.product_id,item.product_version_id,item.unit_id
          FROM bid_project_pick_set_items item WHERE item.project_pick_set_id=NEW.id)
       UNION ALL
       (SELECT item.route_pick_set_id,item.source_report_artifact_id,item.requirement_artifact_id,
               item.candidate_artifact_id,item.product_id,item.product_version_id,item.unit_id
          FROM bid_project_pick_set_items item WHERE item.project_pick_set_id=NEW.id
        EXCEPT
        SELECT route_item.pick_set_id,route_item.source_report_artifact_id,
               route_item.requirement_artifact_id,route_item.candidate_artifact_id,
               route_item.product_id,route_item.product_version_id,route_item.unit_id
          FROM bid_current_route_pick_sets current_value
          JOIN bid_route_pick_set_items route_item ON route_item.pick_set_id=current_value.pick_set_id
         WHERE current_value.project_id=NEW.project_id)
     )
     OR EXISTS(SELECT 1 FROM bid_project_pick_set_items item WHERE item.project_pick_set_id=NEW.id
          AND ((item.source_report_artifact_id IS NOT DISTINCT FROM unsectioned_report_id AND item.unit_id<>'00000000-0000-0000-0000-000000000000'::uuid)
            OR (item.source_report_artifact_id IS DISTINCT FROM unsectioned_report_id AND item.unit_id='00000000-0000-0000-0000-000000000000'::uuid)))
     OR EXISTS(
       (SELECT item.requirement_artifact_id,item.candidate_artifact_id
          FROM bid_project_pick_set_items item WHERE item.project_pick_set_id=NEW.id
            AND item.source_report_artifact_id=unsectioned_report_id
        EXCEPT
        SELECT item.requirement_artifact_id,item.candidate_artifact_id
          FROM bid_current_route_pick_sets current_value
          JOIN bid_route_pick_set_artifacts artifact ON artifact.id=current_value.pick_set_id
          JOIN bid_route_pick_set_items item ON item.pick_set_id=artifact.id
         WHERE current_value.project_id=NEW.project_id AND artifact.source_report_artifact_id=unsectioned_report_id)
     )
  THEN RAISE EXCEPTION 'PROJECT_PICK_SET_V1_RELATION_OR_UNSECTIONED_SUBSET_MISMATCH' USING ERRCODE='23514'; END IF;
  RETURN NEW;
END
$$;
CREATE CONSTRAINT TRIGGER bid_project_pick_set_artifacts_verify
AFTER INSERT ON bid_project_pick_set_artifacts DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION kb_match_verify_project_pick_set_v1();

-- Quote: CNY decimal drafts and immutable QuoteSnapshotV1.
CREATE TABLE bid_quotes (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL UNIQUE REFERENCES bid_projects(id) ON DELETE RESTRICT,
    next_revision bigint NOT NULL DEFAULT 1 CHECK (next_revision > 0),
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE bid_quote_revisions (
    id uuid PRIMARY KEY,
    quote_id uuid NOT NULL REFERENCES bid_quotes(id) ON DELETE RESTRICT,
    project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
    revision bigint NOT NULL CHECK (revision > 0),
    status text NOT NULL CHECK (status IN ('draft', 'finalized', 'reopened')),
    edit_version bigint NOT NULL CHECK (edit_version >= 0),
    currency_code text NOT NULL CHECK (currency_code = 'CNY'),
    currency_scale smallint NOT NULL CHECK (currency_scale = 2),
    tax_mode text NOT NULL CHECK (tax_mode IN ('tax_inclusive', 'tax_exclusive')),
    title text NOT NULL CHECK (octet_length(btrim(title)) BETWEEN 1 AND 256),
    notes text CHECK (octet_length(notes) <= 4096),
    based_on_snapshot_id uuid,
    created_by kb_actor_identity NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (quote_id, revision),
    UNIQUE (project_id, id)
);
CREATE TABLE bid_quote_lines (
    id uuid PRIMARY KEY,
    quote_revision_id uuid NOT NULL REFERENCES bid_quote_revisions(id) ON DELETE RESTRICT,
    ordinal integer NOT NULL CHECK (ordinal >= 0),
    description text NOT NULL,
    pricing_mode text NOT NULL CHECK (pricing_mode IN ('unit_price', 'lump_sum')),
    complete boolean NOT NULL,
    quantity numeric(30,6),
    unit text,
    unit_price numeric(30,6),
    entered_amount numeric(20,2),
    tax_rate numeric(7,6) NOT NULL CHECK (tax_rate BETWEEN 0 AND 1),
    basis_amount numeric(20,2),
    net_amount numeric(20,2),
    tax_amount numeric(20,2),
    gross_amount numeric(20,2),
    user_confirmed boolean NOT NULL DEFAULT false,
    UNIQUE (quote_revision_id, ordinal),
    CHECK (quantity IS NULL OR quantity > 0),
    CHECK (unit_price IS NULL OR unit_price >= 0),
    CHECK (entered_amount IS NULL OR entered_amount >= 0),
    CHECK (basis_amount IS NULL OR basis_amount >= 0),
    CHECK (net_amount IS NULL OR net_amount >= 0),
    CHECK (tax_amount IS NULL OR tax_amount >= 0),
    CHECK (gross_amount IS NULL OR gross_amount >= 0),
    CHECK ((pricing_mode = 'unit_price' AND entered_amount IS NULL)
        OR (pricing_mode = 'lump_sum' AND quantity IS NULL AND unit IS NULL AND unit_price IS NULL))
);
CREATE TABLE bid_quote_snapshots (
    id uuid PRIMARY KEY,
    quote_id uuid NOT NULL REFERENCES bid_quotes(id) ON DELETE RESTRICT,
    revision_id uuid NOT NULL REFERENCES bid_quote_revisions(id) ON DELETE RESTRICT,
    project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
    schema_version smallint NOT NULL CHECK (schema_version = 1),
    canonical_payload bytea NOT NULL,
    content_sha256 kb_sha256 NOT NULL UNIQUE,
    currency_code text NOT NULL CHECK (currency_code = 'CNY'),
    tax_mode text NOT NULL CHECK (tax_mode IN ('tax_inclusive', 'tax_exclusive')),
    net_total numeric(20,2) NOT NULL CHECK (net_total >= 0),
    tax_total numeric(20,2) NOT NULL CHECK (tax_total >= 0),
    gross_total numeric(20,2) NOT NULL CHECK (gross_total >= 0),
    ceiling_revision bigint NOT NULL,
    ceiling_identity_sha256 kb_sha256 NOT NULL,
    fact_revision bigint NOT NULL,
    pricing_revision bigint NOT NULL,
    pricing_set_sha256 kb_sha256 NOT NULL,
    eligibility text NOT NULL CHECK (eligibility IN (
        'eligible', 'ineligible_ceiling_changed', 'ineligible_pricing_changed',
        'ineligible_multiple_inputs_changed', 'superseded'
    )),
    finalized_by kb_actor_identity NOT NULL,
    finalized_at timestamptz NOT NULL,
    UNIQUE (quote_id, revision_id),
    UNIQUE (project_id, id),
    CHECK (content_sha256 = encode(digest(canonical_payload, 'sha256'), 'hex'))
);
CREATE TABLE bid_quote_current (
    quote_id uuid PRIMARY KEY REFERENCES bid_quotes(id) ON DELETE RESTRICT,
    project_id uuid NOT NULL UNIQUE REFERENCES bid_projects(id) ON DELETE RESTRICT,
    current_draft_revision_id uuid,
    active_finalized_snapshot_id uuid,
    CHECK ((current_draft_revision_id IS NULL) <> (active_finalized_snapshot_id IS NULL))
);
CREATE FUNCTION kb_bid_guard_quote_snapshot_transition()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, public
AS $$
BEGIN
  IF TG_OP='DELETE' OR (to_jsonb(OLD)-'eligibility')<>(to_jsonb(NEW)-'eligibility')
    OR NOT ((OLD.eligibility='eligible' AND NEW.eligibility IN (
       'ineligible_ceiling_changed','ineligible_pricing_changed','ineligible_multiple_inputs_changed','superseded'))
      OR (OLD.eligibility IN ('ineligible_ceiling_changed','ineligible_pricing_changed')
          AND NEW.eligibility IN ('ineligible_multiple_inputs_changed','superseded'))
      OR (OLD.eligibility='ineligible_multiple_inputs_changed' AND NEW.eligibility='superseded')) THEN
    RAISE EXCEPTION 'QUOTE_SNAPSHOT_IMMUTABLE_OR_ELIGIBILITY_TRANSITION_INVALID' USING ERRCODE='42501';
  END IF;
  RETURN NEW;
END
$$;
CREATE TRIGGER bid_quote_snapshots_immutable
BEFORE UPDATE OR DELETE ON bid_quote_snapshots
FOR EACH ROW EXECUTE FUNCTION kb_bid_guard_quote_snapshot_transition();

-- Submission profiles, procedural lifecycle, immutable assets, parts, and
-- manifest/render relations.
CREATE TABLE bid_company_profile_artifacts (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
    revision bigint NOT NULL CHECK (revision > 0),
    canonical_payload bytea NOT NULL,
    content_sha256 kb_sha256 NOT NULL UNIQUE,
    legal_name text NOT NULL,
    unified_social_credit_code text NOT NULL,
    registered_address text NOT NULL,
    legal_representative text NOT NULL,
    contact_name text NOT NULL,
    contact_phone text NOT NULL,
    contact_email text NOT NULL,
    created_by kb_actor_identity NOT NULL,
    created_at timestamptz NOT NULL,
    UNIQUE (project_id, revision),
    UNIQUE (project_id, id),
    CHECK (content_sha256 = encode(digest(canonical_payload, 'sha256'), 'hex'))
);
CREATE TABLE bid_submission_profile_artifacts (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
    revision bigint NOT NULL CHECK (revision > 0),
    canonical_payload bytea NOT NULL,
    content_sha256 kb_sha256 NOT NULL UNIQUE,
    buyer_name text NOT NULL,
    project_code text NOT NULL,
    authorized_representative text NOT NULL,
    submission_date date NOT NULL,
    submission_place text NOT NULL,
    seal_confirmed boolean NOT NULL,
    signature_confirmed boolean NOT NULL,
    created_by kb_actor_identity NOT NULL,
    created_at timestamptz NOT NULL,
    UNIQUE (project_id, revision),
    UNIQUE (project_id, id),
    CHECK (content_sha256 = encode(digest(canonical_payload, 'sha256'), 'hex'))
);
CREATE TABLE bid_current_profiles (
    project_id uuid PRIMARY KEY REFERENCES bid_projects(id) ON DELETE RESTRICT,
    company_profile_id uuid,
    submission_profile_id uuid
);

CREATE TABLE bid_procedural_segment_artifacts (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
    clause_id uuid NOT NULL REFERENCES bid_clauses(id) ON DELETE RESTRICT,
    stable_key kb_sha256 NOT NULL,
    segmentation_version text NOT NULL,
    start_offset bigint NOT NULL CHECK (start_offset >= 0),
    end_offset bigint NOT NULL,
    segment_utf8 bytea NOT NULL,
    segment_sha256 kb_sha256 NOT NULL,
    provenance text NOT NULL CHECK (provenance IN ('extracted', 'manual', 'manual_after_edit')),
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (project_id, stable_key),
    CHECK (end_offset > start_offset),
    CHECK (segment_sha256 = encode(digest(segment_utf8, 'sha256'), 'hex'))
);
CREATE TABLE bid_procedural_classification_artifacts (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
    segment_id uuid NOT NULL REFERENCES bid_procedural_segment_artifacts(id) ON DELETE RESTRICT,
    revision integer NOT NULL CHECK (revision > 0),
    router_result_status text NOT NULL CHECK (router_result_status IN ('classified', 'review')),
    router_requirement_kind text CHECK (router_requirement_kind IN (
        'bid_bond', 'authorization_support', 'seal_sample', 'procedural_support', 'confirmation'
    )),
    review_reason text,
    effective_requirement_kind text CHECK (effective_requirement_kind IN (
        'bid_bond', 'authorization_support', 'seal_sample', 'procedural_support', 'confirmation'
    )),
    override_from text,
    override_to text,
    override_actor kb_actor_identity,
    override_reason text,
    override_at timestamptz,
    lifecycle_status text NOT NULL CHECK (lifecycle_status IN ('current', 'superseded')),
    successor_id uuid,
    terminal_reason text CHECK (terminal_reason IN (
        'clause_deleted', 'clause_unconfirmed', 'left_procedural',
        'text_changed', 'resegmented', 'segment_removed'
    )),
    terminal_at timestamptz,
    terminal_actor kb_actor_identity,
    UNIQUE (segment_id, revision),
    CHECK ((router_result_status = 'classified' AND router_requirement_kind IS NOT NULL AND review_reason IS NULL)
        OR (router_result_status = 'review' AND router_requirement_kind IS NULL AND review_reason IS NOT NULL)),
    CHECK ((lifecycle_status = 'current' AND successor_id IS NULL AND terminal_reason IS NULL
            AND terminal_at IS NULL AND terminal_actor IS NULL)
        OR (lifecycle_status = 'superseded'
            AND ((successor_id IS NOT NULL AND terminal_reason IS NULL AND terminal_at IS NULL AND terminal_actor IS NULL)
              OR (successor_id IS NULL AND terminal_reason IS NOT NULL AND terminal_at IS NOT NULL AND terminal_actor IS NOT NULL))))
);
CREATE TABLE bid_procedural_attachments (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
    kind text NOT NULL CHECK (kind IN (
        'bid_bond', 'authorization_support', 'seal_sample', 'procedural_support'
    )),
    object_ref kb_object_ref NOT NULL,
    validation_status text NOT NULL CHECK (validation_status IN ('pending', 'valid', 'invalid')),
    status text NOT NULL CHECK (status IN ('draft', 'confirmed', 'rejected', 'superseded')),
    revision integer NOT NULL CHECK (revision > 0),
    created_by kb_actor_identity NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (project_id, id)
);
CREATE TABLE bid_procedural_decision_artifacts (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
    classification_id uuid NOT NULL REFERENCES bid_procedural_classification_artifacts(id) ON DELETE RESTRICT,
    revision integer NOT NULL CHECK (revision > 0),
    resolution text NOT NULL CHECK (resolution IN (
        'confirmed_by_user', 'satisfied_by_attachment', 'not_applicable'
    )),
    attachment_id uuid REFERENCES bid_procedural_attachments(id) ON DELETE RESTRICT,
    reason text,
    actor_identity kb_actor_identity NOT NULL,
    decided_at timestamptz NOT NULL,
    lifecycle_status text NOT NULL CHECK (lifecycle_status IN ('current', 'superseded')),
    successor_id uuid,
    terminal_reason text,
    UNIQUE (classification_id, revision),
    CHECK ((resolution = 'satisfied_by_attachment') = (attachment_id IS NOT NULL)),
    CHECK ((resolution = 'not_applicable') = (reason IS NOT NULL))
);

CREATE TABLE bid_shot_artifacts (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
    object_ref kb_object_ref NOT NULL,
    content_sha256 kb_sha256 NOT NULL,
    media_type text NOT NULL,
    byte_length bigint NOT NULL CHECK (byte_length >= 0),
    created_by kb_actor_identity NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (project_id, id)
);
CREATE TABLE bid_current_shot_placements (
    project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
    ordinal integer NOT NULL CHECK (ordinal >= 0),
    shot_artifact_id uuid NOT NULL REFERENCES bid_shot_artifacts(id) ON DELETE RESTRICT,
    PRIMARY KEY (project_id, ordinal),
    UNIQUE (project_id, shot_artifact_id)
);

CREATE TABLE bid_template_contract_artifacts (
    slot text NOT NULL CHECK (slot IN (
        '1', '2:unit', '2:unsectioned', '3', '4', '5', '6:letter',
        '6:authorization', '6:quote', '6:implementation_plan', '6:procedural'
    )),
    version text NOT NULL,
    canonical_payload bytea NOT NULL,
    content_sha256 kb_sha256 NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (slot, version),
    UNIQUE (content_sha256),
    CHECK (content_sha256 = encode(digest(canonical_payload, 'sha256'), 'hex'))
);
CREATE TABLE bid_template_contract_current (
    slot text PRIMARY KEY,
    version text NOT NULL,
    promotion_generation bigint NOT NULL CHECK (promotion_generation >= 0),
    FOREIGN KEY (slot, version) REFERENCES bid_template_contract_artifacts(slot, version) ON DELETE RESTRICT
);
INSERT INTO bid_template_contract_artifacts(
    slot, version, canonical_payload, content_sha256, created_at
)
SELECT slot, 'v1', convert_to('{"schema_version":1,"slot":"' || slot || '","version":"v1"}', 'UTF8'),
       encode(digest(convert_to('{"schema_version":1,"slot":"' || slot || '","version":"v1"}', 'UTF8'), 'sha256'), 'hex'),
       '1970-01-01 UTC'
FROM unnest(ARRAY['1','2:unit','2:unsectioned','3','4','5','6:letter','6:authorization','6:quote','6:implementation_plan','6:procedural']) AS slot;
INSERT INTO bid_template_contract_current(slot, version, promotion_generation)
SELECT slot, 'v1', 0 FROM bid_template_contract_artifacts;

CREATE TABLE bid_part_content_artifacts (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
    part_key text NOT NULL CHECK (part_key ~ '^(1|2:(unsectioned|[0-9a-f-]{36})|3|4|5|6:(letter|authorization|quote|implementation_plan|procedural))$'),
    revision bigint NOT NULL CHECK (revision > 0),
    canonical_markdown_utf8 bytea NOT NULL,
    content_sha256 kb_sha256 NOT NULL,
    created_by kb_actor_identity NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (project_id, part_key, revision),
    UNIQUE (project_id, id),
    CHECK (content_sha256 = encode(digest(canonical_markdown_utf8, 'sha256'), 'hex'))
);
CREATE TABLE bid_part_dependency_artifacts (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
    part_key text NOT NULL,
    template_slot text NOT NULL,
    template_version text NOT NULL,
    part_content_artifact_id uuid NOT NULL REFERENCES bid_part_content_artifacts(id) ON DELETE RESTRICT,
    schema_version smallint NOT NULL CHECK (schema_version = 1),
    typed_input_identities jsonb NOT NULL CHECK (jsonb_typeof(typed_input_identities) = 'array'),
    canonical_payload bytea NOT NULL,
    content_sha256 kb_sha256 NOT NULL,
    generated_at timestamptz NOT NULL,
    UNIQUE (project_id, part_key, id),
    CHECK (content_sha256 = encode(digest(canonical_payload, 'sha256'), 'hex')),
    FOREIGN KEY (template_slot, template_version)
        REFERENCES bid_template_contract_artifacts(slot, version) ON DELETE RESTRICT
);
CREATE TABLE bid_current_parts (
    project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
    part_key text NOT NULL,
    content_artifact_id uuid NOT NULL,
    dependency_artifact_id uuid NOT NULL,
    stale boolean NOT NULL,
    stale_reason_codes text[] NOT NULL DEFAULT '{}',
    PRIMARY KEY (project_id, part_key),
    FOREIGN KEY (project_id, content_artifact_id)
        REFERENCES bid_part_content_artifacts(project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, part_key, dependency_artifact_id)
        REFERENCES bid_part_dependency_artifacts(project_id, part_key, id) ON DELETE RESTRICT
);

CREATE TABLE bid_submission_manifests (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
    format text NOT NULL CHECK (format IN ('docx', 'pdf')),
    schema_version smallint NOT NULL CHECK (schema_version = 1),
    required_part_keys text[] NOT NULL,
    gate_status text NOT NULL CHECK (gate_status IN ('pass', 'warning')),
    gate_issues jsonb NOT NULL CHECK (jsonb_typeof(gate_issues) = 'array'),
    canonical_payload bytea NOT NULL,
    content_sha256 kb_sha256 NOT NULL UNIQUE,
    created_by kb_actor_identity NOT NULL,
    created_at timestamptz NOT NULL,
    UNIQUE (project_id, id),
    CHECK (format = 'docx' OR gate_status = 'pass'),
    CHECK (content_sha256 = encode(digest(canonical_payload, 'sha256'), 'hex'))
);
CREATE TABLE bid_submission_gate_issues (
    manifest_id uuid NOT NULL REFERENCES bid_submission_manifests(id) ON DELETE RESTRICT,
    ordinal integer NOT NULL CHECK (ordinal >= 0),
    code text NOT NULL CHECK (code IN (
        'PROFILE_FIELD_MISSING', 'SIGNATURE_OR_SEAL_NOT_CONFIRMED',
        'PROCEDURAL_CLASSIFICATION_MISSING', 'PROCEDURAL_CLASSIFICATION_REVIEW',
        'PROCEDURAL_DECISION_MISSING', 'ATTACHMENT_NOT_VALID', 'PART_MISSING',
        'PART_STALE', 'QUOTE_NOT_FINALIZED', 'BID_VALIDITY_CONFLICT',
        'KIND_ROUTER_RECONFIRMATION_REQUIRED', 'DEPENDENCY_NOT_CURRENT'
    )),
    part_key text,
    entity_locator jsonb NOT NULL CHECK (jsonb_typeof(entity_locator) = 'object'),
    current_identity jsonb,
    expected_identity jsonb,
    remediation jsonb NOT NULL CHECK (jsonb_typeof(remediation) = 'object'),
    PRIMARY KEY (manifest_id, ordinal)
);
CREATE TABLE bid_submission_manifest_parts (
    manifest_id uuid NOT NULL REFERENCES bid_submission_manifests(id) ON DELETE RESTRICT,
    ordinal integer NOT NULL CHECK (ordinal >= 0),
    part_key text NOT NULL,
    content_artifact_id uuid NOT NULL REFERENCES bid_part_content_artifacts(id) ON DELETE RESTRICT,
    dependency_artifact_id uuid NOT NULL REFERENCES bid_part_dependency_artifacts(id) ON DELETE RESTRICT,
    PRIMARY KEY (manifest_id, ordinal),
    UNIQUE (manifest_id, part_key)
);
CREATE TABLE bid_manifest_render_assets (
    id uuid PRIMARY KEY,
    manifest_id uuid NOT NULL REFERENCES bid_submission_manifests(id) ON DELETE RESTRICT,
    source_kind text NOT NULL CHECK (source_kind IN ('bid_shot', 'markdown_object')),
    source_locator jsonb NOT NULL CHECK (jsonb_typeof(source_locator) = 'object'),
    object_ref kb_object_ref NOT NULL,
    digest kb_sha256 NOT NULL,
    media_type text NOT NULL,
    byte_length bigint NOT NULL CHECK (byte_length >= 0),
    occurrence_ordinal integer NOT NULL CHECK (occurrence_ordinal >= 0),
    UNIQUE (manifest_id, source_kind, occurrence_ordinal)
);
CREATE TABLE bid_submission_output_artifacts (
    id uuid PRIMARY KEY,
    manifest_id uuid NOT NULL REFERENCES bid_submission_manifests(id) ON DELETE RESTRICT,
    project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
    format text NOT NULL CHECK (format IN ('docx', 'pdf')),
    object_ref kb_object_ref NOT NULL,
    content_sha256 kb_sha256 NOT NULL,
    byte_length bigint NOT NULL CHECK (byte_length >= 0),
    rendered_at timestamptz NOT NULL,
    UNIQUE (manifest_id, format),
    UNIQUE (project_id, id)
);
CREATE TABLE bid_current_submission_outputs (
    project_id uuid NOT NULL,
    format text NOT NULL CHECK (format IN ('docx', 'pdf')),
    output_artifact_id uuid NOT NULL,
    PRIMARY KEY (project_id, format),
    FOREIGN KEY (project_id, output_artifact_id)
        REFERENCES bid_submission_output_artifacts(project_id, id) ON DELETE RESTRICT
);

CREATE TRIGGER bid_company_profile_artifacts_immutable BEFORE UPDATE OR DELETE ON bid_company_profile_artifacts FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();
CREATE TRIGGER bid_submission_profile_artifacts_immutable BEFORE UPDATE OR DELETE ON bid_submission_profile_artifacts FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();
CREATE TRIGGER bid_procedural_segment_artifacts_immutable BEFORE UPDATE OR DELETE ON bid_procedural_segment_artifacts FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();
CREATE TRIGGER bid_procedural_classification_artifacts_immutable BEFORE UPDATE OR DELETE ON bid_procedural_classification_artifacts FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();
CREATE TRIGGER bid_procedural_decision_artifacts_immutable BEFORE UPDATE OR DELETE ON bid_procedural_decision_artifacts FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();
CREATE TRIGGER bid_shot_artifacts_immutable BEFORE UPDATE OR DELETE ON bid_shot_artifacts FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();
CREATE TRIGGER bid_template_contract_artifacts_immutable BEFORE UPDATE OR DELETE ON bid_template_contract_artifacts FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();
CREATE TRIGGER bid_part_content_artifacts_immutable BEFORE UPDATE OR DELETE ON bid_part_content_artifacts FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();
CREATE TRIGGER bid_part_dependency_artifacts_immutable BEFORE UPDATE OR DELETE ON bid_part_dependency_artifacts FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();
CREATE TRIGGER bid_submission_manifests_immutable BEFORE UPDATE OR DELETE ON bid_submission_manifests FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();
CREATE TRIGGER bid_submission_gate_issues_immutable BEFORE UPDATE OR DELETE ON bid_submission_gate_issues FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();
CREATE TRIGGER bid_submission_manifest_parts_immutable BEFORE UPDATE OR DELETE ON bid_submission_manifest_parts FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();
CREATE TRIGGER bid_manifest_render_assets_immutable BEFORE UPDATE OR DELETE ON bid_manifest_render_assets FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();
CREATE TRIGGER bid_submission_output_artifacts_immutable BEFORE UPDATE OR DELETE ON bid_submission_output_artifacts FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();

-- TenderPublication and ClauseLifecycle checked mutation boundary.

CREATE FUNCTION kb_bid_require_human_actor(p_actor kb_actor_identity)
RETURNS void
LANGUAGE plpgsql
SET search_path = pg_catalog, public
AS $$
BEGIN
    IF p_actor LIKE 'system:%' THEN
        RAISE EXCEPTION 'HUMAN_ACTOR_REQUIRED' USING ERRCODE='42501';
    END IF;
END
$$;

CREATE FUNCTION kb_bid_idempotency_begin(
    p_actor kb_actor_identity, p_operation text, p_key text,
    p_request_bytes bytea, p_request_sha256 kb_sha256
)
RETURNS bytea
LANGUAGE plpgsql
SET search_path = pg_catalog, public
AS $$
DECLARE saved idempotency_requests%ROWTYPE;
BEGIN
    IF p_request_sha256 <> encode(digest(p_request_bytes,'sha256'),'hex') THEN
        RAISE EXCEPTION 'REQUEST_PAYLOAD_HASH_MISMATCH' USING ERRCODE='22023';
    END IF;
    INSERT INTO idempotency_requests(actor_identity,operation,idempotency_key,schema_version,
      request_bytes,request_sha256,state)
    VALUES(p_actor,p_operation,p_key,1,p_request_bytes,p_request_sha256,'intent')
    ON CONFLICT DO NOTHING;
    SELECT * INTO STRICT saved FROM idempotency_requests
     WHERE actor_identity=p_actor AND operation=p_operation AND idempotency_key=p_key FOR UPDATE;
    IF saved.request_sha256 <> p_request_sha256 OR saved.request_bytes <> p_request_bytes THEN
        RAISE EXCEPTION 'IDEMPOTENCY_PAYLOAD_MISMATCH' USING ERRCODE='23505';
    END IF;
    IF saved.state='completed' THEN RETURN saved.response_bytes; END IF;
    RETURN NULL;
END
$$;

CREATE FUNCTION kb_bid_idempotency_complete(
    p_actor kb_actor_identity, p_operation text, p_key text,
    p_response_status integer, p_response_bytes bytea
)
RETURNS void
LANGUAGE plpgsql
SET search_path = pg_catalog, public
AS $$
BEGIN
    UPDATE idempotency_requests SET state='completed',response_status=p_response_status,
      response_bytes=p_response_bytes,response_sha256=encode(digest(p_response_bytes,'sha256'),'hex'),
      completed_at=clock_timestamp()
    WHERE actor_identity=p_actor AND operation=p_operation AND idempotency_key=p_key AND state='intent';
    IF NOT FOUND THEN RAISE EXCEPTION 'IDEMPOTENCY_INTENT_MISSING' USING ERRCODE='40001'; END IF;
END
$$;

CREATE FUNCTION kb_bid_utc_json_time(p_value timestamptz)
RETURNS text
LANGUAGE sql
IMMUTABLE
PARALLEL SAFE
SET search_path = pg_catalog
AS $$ SELECT CASE WHEN p_value IS NULL THEN NULL ELSE
  to_char(p_value AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"') END $$;

CREATE FUNCTION kb_bid_fact_payload(p_project bid_projects)
RETURNS jsonb
LANGUAGE sql
STABLE
SET search_path = pg_catalog, public
AS $$ SELECT jsonb_build_object(
 'schema_version',1,'project_id',p_project.id,'revision',p_project.fact_revision,
 'budget_amount',CASE WHEN p_project.budget_amount IS NULL THEN NULL ELSE to_char(p_project.budget_amount,'FM99999999999999999990.00') END,
 'budget_currency',p_project.budget_currency,
 'ceiling_price',CASE WHEN p_project.ceiling_price IS NULL THEN NULL ELSE to_char(p_project.ceiling_price,'FM99999999999999999990.00') END,
 'ceiling_currency',p_project.ceiling_currency,'ceiling_basis',p_project.ceiling_basis,
 'expires_at',kb_bid_utc_json_time(p_project.expires_at),'bid_open_at',kb_bid_utc_json_time(p_project.bid_open_at),
 'bid_valid_until',kb_bid_utc_json_time(p_project.bid_valid_until),'bid_valid_days',p_project.bid_valid_days) $$;

CREATE FUNCTION kb_bid_ceiling_payload(p_project bid_projects)
RETURNS jsonb
LANGUAGE sql
STABLE
SET search_path = pg_catalog, public
AS $$ SELECT jsonb_build_object(
 'schema_version',1,'project_id',p_project.id,'revision',p_project.ceiling_revision,
 'amount',CASE WHEN p_project.ceiling_price IS NULL THEN NULL ELSE to_char(p_project.ceiling_price,'FM99999999999999999990.00') END,
 'currency_code',p_project.ceiling_currency,'basis',p_project.ceiling_basis) $$;

CREATE FUNCTION kb_bid_clause_semantic_sha256(
 p_id uuid,p_status text,p_kind text,p_text text,p_must boolean,p_revision bigint
)
RETURNS kb_sha256
LANGUAGE sql
IMMUTABLE
PARALLEL SAFE
SET search_path = pg_catalog, public
AS $$ SELECT encode(digest(convert_to(concat_ws(E'\x1f','ClauseV1',p_id,p_status,p_kind,
 octet_length(convert_to(p_text,'UTF8')),p_text,p_must,p_revision),'UTF8'),'sha256'),'hex') $$;

CREATE FUNCTION kb_bid_refresh_clause_set(p_project_id uuid,p_set_kind text)
RETURNS kb_sha256
LANGUAGE plpgsql
SET search_path = pg_catalog, public
AS $$
DECLARE payload text; result_value kb_sha256;
BEGIN
    IF p_set_kind NOT IN ('service','pricing','schedule_payment','schedule_delivery','evaluation','procedural') THEN
        RAISE EXCEPTION 'INVALID_CLAUSE_SET_KIND' USING ERRCODE='22023';
    END IF;
    SELECT 'ClauseSetV1:'||p_set_kind||':'||COALESCE(string_agg(
      id::text||':'||octet_length(convert_to(text,'UTF8'))||':'||text||':'||must::text||':'||kind,
      E'\x1e' ORDER BY uuid_send(id)), '') INTO payload
    FROM bid_clauses WHERE project_id=p_project_id AND status='confirmed' AND kind=p_set_kind;
    result_value := encode(digest(convert_to(payload,'UTF8'),'sha256'),'hex');
    UPDATE bid_clause_set_identities SET revision=revision+1,content_sha256=result_value,
      updated_at=clock_timestamp() WHERE project_id=p_project_id AND set_kind=p_set_kind;
    IF NOT FOUND THEN RAISE EXCEPTION 'CLAUSE_SET_IDENTITY_MISSING' USING ERRCODE='23503'; END IF;
    RETURN result_value;
END
$$;

CREATE FUNCTION kb_bid_stale_for_clause_change(
 p_project_id uuid,p_matching boolean,p_set_kinds text[]
)
RETURNS void
LANGUAGE plpgsql
SET search_path = pg_catalog, public
AS $$
DECLARE set_kind text;
BEGIN
    IF p_matching THEN
      UPDATE bid_projects SET matching_mutation_watermark=matching_mutation_watermark+1,
        updated_at=clock_timestamp() WHERE id=p_project_id;
      DELETE FROM bid_current_matching_reports WHERE project_id=p_project_id;
      UPDATE bid_current_parts SET stale=true,
        stale_reason_codes=(SELECT ARRAY(SELECT DISTINCT x FROM unnest(stale_reason_codes||ARRAY['CLAUSE_MATCHING_CHANGED']) x ORDER BY x))
       WHERE project_id=p_project_id AND (part_key LIKE '2:%' OR part_key IN ('3','4','5','6:implementation_plan'));
    END IF;
    FOREACH set_kind IN ARRAY COALESCE(p_set_kinds,ARRAY[]::text[]) LOOP
      PERFORM kb_bid_refresh_clause_set(p_project_id,set_kind);
      UPDATE bid_current_parts SET stale=true,
        stale_reason_codes=(SELECT ARRAY(SELECT DISTINCT x FROM unnest(stale_reason_codes||ARRAY['CLAUSE_SET_CHANGED']) x ORDER BY x))
       WHERE project_id=p_project_id AND (
         (set_kind='service' AND part_key='6:implementation_plan') OR
         (set_kind='pricing' AND part_key IN ('6:quote','6:letter')) OR
         (set_kind='schedule_payment' AND part_key='6:letter') OR
         (set_kind='schedule_delivery' AND part_key='6:implementation_plan') OR
         (set_kind='evaluation' AND part_key='5') OR
         (set_kind='procedural' AND part_key IN ('5','6:authorization','6:procedural')));
      IF set_kind='pricing' THEN
        UPDATE bid_quote_snapshots SET eligibility=CASE
          WHEN eligibility='eligible' THEN 'ineligible_pricing_changed'
          WHEN eligibility='ineligible_ceiling_changed' THEN 'ineligible_multiple_inputs_changed'
          ELSE eligibility END
         WHERE project_id=p_project_id AND eligibility IN ('eligible','ineligible_ceiling_changed');
      END IF;
    END LOOP;
END
$$;

CREATE FUNCTION kb_bid_create_project(
 p_id uuid,p_title text,p_owner_user_id uuid,p_ends_at timestamptz,p_expires_at timestamptz,
 p_actor kb_actor_identity,p_idempotency_key text,p_request_bytes bytea,p_request_sha256 kb_sha256
)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE replay bytea; response jsonb; fact_hash kb_sha256; ceiling_hash kb_sha256; set_kind text;
BEGIN
    PERFORM kb_bid_require_human_actor(p_actor);
    IF p_actor <> 'user:'||p_owner_user_id::text THEN RAISE EXCEPTION 'PROJECT_OWNER_ACTOR_MISMATCH' USING ERRCODE='42501'; END IF;
    replay := kb_bid_idempotency_begin(p_actor,'bid.project.create',p_idempotency_key,p_request_bytes,p_request_sha256);
    IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
    IF p_ends_at <= clock_timestamp() THEN RAISE EXCEPTION 'PROJECT_END_MUST_BE_FUTURE' USING ERRCODE='22023'; END IF;
    INSERT INTO bid_projects(id,title,owner_user_id,ends_at,expires_at,fact_sha256,
      ceiling_identity_sha256,created_by)
    VALUES(p_id,p_title,p_owner_user_id,p_ends_at,p_expires_at,repeat('0',64),repeat('0',64),p_actor);
    SELECT encode(digest(convert_to(kb_bid_fact_payload(p)::text,'UTF8'),'sha256'),'hex'),
           encode(digest(convert_to(kb_bid_ceiling_payload(p)::text,'UTF8'),'sha256'),'hex')
      INTO fact_hash,ceiling_hash FROM bid_projects p WHERE id=p_id;
    UPDATE bid_projects SET fact_sha256=fact_hash,ceiling_identity_sha256=ceiling_hash WHERE id=p_id;
    FOREACH set_kind IN ARRAY ARRAY['service','pricing','schedule_payment','schedule_delivery','evaluation','procedural'] LOOP
      INSERT INTO bid_clause_set_identities(project_id,set_kind,revision,content_sha256,updated_at)
      VALUES(p_id,set_kind,0,encode(digest(convert_to('ClauseSetV1:'||set_kind||':','UTF8'),'sha256'),'hex'),clock_timestamp());
    END LOOP;
    response:=jsonb_build_object('id',p_id,'fact_revision',0,'fact_sha256',fact_hash,
      'ceiling_revision',0,'ceiling_identity_sha256',ceiling_hash);
    INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
      entity_kind,entity_locator,after_revision,after_sha256)
    VALUES(gen_random_uuid(),1,'bid.project.create',p_actor,p_idempotency_key,p_request_sha256,
      encode(digest(convert_to(response::text,'UTF8'),'sha256'),'hex'),'bid_project',jsonb_build_object('project_id',p_id),0,fact_hash);
    PERFORM kb_bid_idempotency_complete(p_actor,'bid.project.create',p_idempotency_key,201,convert_to(response::text,'UTF8'));
    RETURN response;
END
$$;

CREATE FUNCTION kb_bid_upload_document(
 p_id uuid,p_project_id uuid,p_file_name text,p_media_type text,p_byte_length bigint,
 p_object_ref kb_object_ref,p_original_sha256 kb_sha256,p_actor kb_actor_identity,
 p_idempotency_key text,p_request_bytes bytea,p_request_sha256 kb_sha256
)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE replay bytea; response jsonb; project_value bid_projects%ROWTYPE;
BEGIN
    PERFORM kb_bid_require_human_actor(p_actor);
    replay:=kb_bid_idempotency_begin(p_actor,'bid.document.upload',p_idempotency_key,p_request_bytes,p_request_sha256);
    IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
    SELECT * INTO STRICT project_value FROM bid_projects WHERE id=p_project_id FOR UPDATE;
    IF project_value.status<>'open' THEN RAISE EXCEPTION 'PROJECT_ENDED' USING ERRCODE='55000'; END IF;
    PERFORM kb_object_reference_add(p_object_ref,p_original_sha256,p_media_type,p_byte_length,
      'bid_document',p_id,'original',p_actor);
    INSERT INTO bid_documents(id,project_id,file_name,media_type,byte_length,original_object_ref,
      original_sha256,parse_status) VALUES(p_id,p_project_id,p_file_name,p_media_type,p_byte_length,
      p_object_ref,p_original_sha256,'pending');
    response:=jsonb_build_object('id',p_id,'project_id',p_project_id,'conversion_generation',1,'parse_status','pending');
    INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
      entity_kind,entity_locator,after_revision,after_sha256)
    VALUES(gen_random_uuid(),1,'bid.document.upload',p_actor,p_idempotency_key,p_request_sha256,
      encode(digest(convert_to(response::text,'UTF8'),'sha256'),'hex'),'bid_document',jsonb_build_object('document_id',p_id),
      1,p_original_sha256);
    PERFORM kb_bid_idempotency_complete(p_actor,'bid.document.upload',p_idempotency_key,201,convert_to(response::text,'UTF8'));
    RETURN response;
END
$$;

CREATE FUNCTION kb_bid_claim_document_conversion(p_document_id uuid,p_claim_token uuid,p_claimed_by text)
RETURNS TABLE(project_id uuid,file_name text,object_ref kb_object_ref,conversion_generation integer,claim_lease_ms integer)
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE project_key uuid; document_value bid_documents%ROWTYPE; next_attempt integer; lease_value integer:=300000;
BEGIN
    SELECT d.project_id INTO project_key FROM bid_documents d WHERE d.id=p_document_id;
    IF project_key IS NULL THEN RETURN; END IF;
    PERFORM 1 FROM bid_projects WHERE id=project_key AND status='open' FOR UPDATE;
    IF NOT FOUND THEN RETURN; END IF;
    SELECT * INTO document_value FROM bid_documents WHERE id=p_document_id FOR UPDATE;
    IF document_value.parse_status<>'pending' THEN RETURN; END IF;
    SELECT COALESCE(max(a.attempt),0)+1 INTO next_attempt FROM bid_document_conversion_attempts a
      WHERE a.document_id=p_document_id AND a.conversion_generation=document_value.conversion_generation;
    INSERT INTO bid_document_conversion_attempts(document_id,conversion_generation,attempt,claim_token,
      claimed_by,claim_lease_ms,claimed_at,heartbeat_at,status)
    VALUES(p_document_id,document_value.conversion_generation,next_attempt,p_claim_token,p_claimed_by,
      lease_value,clock_timestamp(),clock_timestamp(),'running');
    UPDATE bid_documents SET parse_status='processing',error_code=NULL WHERE id=p_document_id;
    RETURN QUERY SELECT document_value.project_id,document_value.file_name,document_value.original_object_ref,
      document_value.conversion_generation,lease_value;
END
$$;

CREATE FUNCTION kb_bid_heartbeat_document_conversion(p_document_id uuid,p_claim_token uuid)
RETURNS boolean
LANGUAGE sql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$ WITH changed AS (
 UPDATE bid_document_conversion_attempts a SET heartbeat_at=clock_timestamp()
 WHERE a.document_id=p_document_id AND a.claim_token=p_claim_token AND a.status='running'
   AND a.heartbeat_at + make_interval(secs=>a.claim_lease_ms/1000.0)>clock_timestamp()
 RETURNING 1) SELECT EXISTS(SELECT 1 FROM changed) $$;

CREATE FUNCTION kb_bid_complete_document_conversion(
 p_document_id uuid,p_claim_token uuid,p_source_artifact_id uuid,p_markdown bytea,
 p_converter_contract_version text,p_image_asset_set_sha256 kb_sha256
)
RETURNS uuid
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE project_key uuid; document_value bid_documents%ROWTYPE; attempt_value bid_document_conversion_attempts%ROWTYPE;
 markdown_hash kb_sha256;
BEGIN
    SELECT project_id INTO STRICT project_key FROM bid_documents WHERE id=p_document_id;
    PERFORM 1 FROM bid_projects WHERE id=project_key AND status='open' FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'PROJECT_ENDED' USING ERRCODE='55000'; END IF;
    SELECT * INTO STRICT document_value FROM bid_documents WHERE id=p_document_id FOR UPDATE;
    SELECT * INTO STRICT attempt_value FROM bid_document_conversion_attempts
      WHERE document_id=p_document_id AND claim_token=p_claim_token FOR UPDATE;
    IF document_value.parse_status<>'processing' OR attempt_value.status<>'running'
       OR attempt_value.conversion_generation<>document_value.conversion_generation
       OR attempt_value.heartbeat_at+make_interval(secs=>attempt_value.claim_lease_ms/1000.0)<=clock_timestamp() THEN
      RAISE EXCEPTION 'CONVERSION_LEASE_LOST' USING ERRCODE='40001';
    END IF;
    markdown_hash:=encode(digest(p_markdown,'sha256'),'hex');
    INSERT INTO bid_converted_source_artifacts(id,project_id,document_id,conversion_generation,
      original_object_ref,original_sha256,canonical_markdown_utf8,markdown_sha256,byte_length,
      converter_contract_version,image_asset_set_sha256)
    VALUES(p_source_artifact_id,project_key,p_document_id,document_value.conversion_generation,
      document_value.original_object_ref,document_value.original_sha256,p_markdown,markdown_hash,
      octet_length(p_markdown),p_converter_contract_version,p_image_asset_set_sha256);
    UPDATE bid_documents SET current_converted_source_artifact_id=p_source_artifact_id,
      parse_status='completed',parsed_at=clock_timestamp(),error_code=NULL WHERE id=p_document_id;
    UPDATE bid_document_conversion_attempts SET status='completed'
      WHERE document_id=p_document_id AND claim_token=p_claim_token;
    RETURN p_source_artifact_id;
END
$$;

CREATE FUNCTION kb_bid_fail_document_conversion(p_document_id uuid,p_claim_token uuid,p_error_code text,p_retry boolean)
RETURNS boolean
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE project_key uuid;
BEGIN
    SELECT project_id INTO project_key FROM bid_documents WHERE id=p_document_id;
    IF project_key IS NULL THEN RETURN false; END IF;
    PERFORM 1 FROM bid_projects WHERE id=project_key FOR UPDATE;
    PERFORM 1 FROM bid_documents WHERE id=p_document_id FOR UPDATE;
    UPDATE bid_document_conversion_attempts SET status='failed',error_code=left(p_error_code,128)
      WHERE document_id=p_document_id AND claim_token=p_claim_token AND status='running';
    IF NOT FOUND THEN RETURN false; END IF;
    UPDATE bid_documents SET parse_status=CASE WHEN p_retry THEN 'pending' ELSE 'failed' END,
      error_code=left(p_error_code,128) WHERE id=p_document_id;
    RETURN true;
END
$$;

CREATE FUNCTION kb_bid_schedule_extraction(
 p_target_id uuid,p_document_id uuid,p_expected_section_count integer,p_policy_version text,
 p_prompt_version text,p_actor kb_actor_identity,p_idempotency_key text,p_request_bytes bytea,
 p_request_sha256 kb_sha256
)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE replay bytea; document_value bid_documents%ROWTYPE; project_value bid_projects%ROWTYPE;
 router_value kind_router_current%ROWTYPE; generation_value integer; response jsonb;
BEGIN
    replay:=kb_bid_idempotency_begin(p_actor,'bid.extraction.schedule',p_idempotency_key,p_request_bytes,p_request_sha256);
    IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
    SELECT * INTO STRICT document_value FROM bid_documents WHERE id=p_document_id;
    SELECT * INTO STRICT project_value FROM bid_projects WHERE id=document_value.project_id FOR UPDATE;
    IF project_value.status<>'open' OR document_value.parse_status<>'completed'
       OR document_value.current_converted_source_artifact_id IS NULL THEN
      RAISE EXCEPTION 'EXTRACTION_SOURCE_NOT_CURRENT' USING ERRCODE='55000';
    END IF;
    SELECT * INTO STRICT document_value FROM bid_documents WHERE id=p_document_id FOR UPDATE;
    SELECT * INTO STRICT router_value FROM kind_router_current WHERE singleton_key FOR SHARE;
    SELECT COALESCE(max(extraction_generation),0)+1 INTO generation_value
      FROM bid_extraction_targets WHERE project_id=document_value.project_id AND document_id=p_document_id;
    INSERT INTO bid_extraction_targets(id,project_id,document_id,source_artifact_id,conversion_generation,
      extraction_generation,router_contract_version,policy_version,prompt_version,output_schema_version,
      expected_section_count,state)
    VALUES(p_target_id,document_value.project_id,p_document_id,document_value.current_converted_source_artifact_id,
      document_value.conversion_generation,generation_value,router_value.version,p_policy_version,p_prompt_version,
      1,p_expected_section_count,'pending');
    response:=jsonb_build_object('target_id',p_target_id,'project_id',document_value.project_id,
      'document_id',p_document_id,'extraction_generation',generation_value,
      'source_artifact_id',document_value.current_converted_source_artifact_id,
      'router_contract_version',router_value.version);
    INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
      entity_kind,entity_locator,after_revision,after_sha256)
    VALUES(gen_random_uuid(),1,'bid.extraction.schedule',p_actor,p_idempotency_key,p_request_sha256,
      encode(digest(convert_to(response::text,'UTF8'),'sha256'),'hex'),'bid_extraction_target',
      jsonb_build_object('target_id',p_target_id),generation_value,
      encode(digest(convert_to(response::text,'UTF8'),'sha256'),'hex'));
    PERFORM kb_bid_idempotency_complete(p_actor,'bid.extraction.schedule',p_idempotency_key,202,convert_to(response::text,'UTF8'));
    RETURN response;
END
$$;

CREATE FUNCTION kb_bid_claim_extraction(p_target_id uuid,p_claim_token uuid,p_claimed_by text)
RETURNS TABLE(project_id uuid,document_id uuid,source_artifact_id uuid,conversion_generation integer,
 extraction_generation integer,attempt integer,claim_lease_ms integer)
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE target_value bid_extraction_targets%ROWTYPE; next_attempt integer; lease_value integer:=300000;
BEGIN
    SELECT * INTO target_value FROM bid_extraction_targets WHERE id=p_target_id;
    IF NOT FOUND THEN RETURN; END IF;
    PERFORM 1 FROM bid_projects WHERE id=target_value.project_id AND status='open' FOR UPDATE;
    IF NOT FOUND THEN RETURN; END IF;
    SELECT * INTO target_value FROM bid_extraction_targets WHERE id=p_target_id FOR UPDATE;
    IF target_value.state<>'pending' THEN RETURN; END IF;
    SELECT COALESCE(max(a.attempt),0)+1 INTO next_attempt FROM bid_extraction_attempts a WHERE a.target_id=p_target_id;
    INSERT INTO bid_extraction_attempts(target_id,attempt,claim_token,claimed_by,claim_lease_ms,
      claimed_at,heartbeat_at,status) VALUES(p_target_id,next_attempt,p_claim_token,p_claimed_by,
      lease_value,clock_timestamp(),clock_timestamp(),'running');
    UPDATE bid_extraction_targets SET state='running' WHERE id=p_target_id;
    RETURN QUERY SELECT target_value.project_id,target_value.document_id,target_value.source_artifact_id,
      target_value.conversion_generation,target_value.extraction_generation,next_attempt,lease_value;
END
$$;

CREATE FUNCTION kb_bid_heartbeat_extraction(p_target_id uuid,p_claim_token uuid,p_attempt integer)
RETURNS boolean
LANGUAGE sql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$ WITH changed AS (
 UPDATE bid_extraction_attempts a SET heartbeat_at=clock_timestamp()
 WHERE a.target_id=p_target_id AND a.claim_token=p_claim_token AND a.attempt=p_attempt AND a.status='running'
   AND a.heartbeat_at+make_interval(secs=>a.claim_lease_ms/1000.0)>clock_timestamp()
 RETURNING 1) SELECT EXISTS(SELECT 1 FROM changed) $$;

CREATE FUNCTION kb_bid_validate_fact_value(p_field text,p_value jsonb)
RETURNS boolean
LANGUAGE plpgsql
IMMUTABLE
SET search_path = pg_catalog
AS $$
DECLARE amount_value numeric; time_value timestamptz; days_value integer;
BEGIN
  IF p_field='budget_amount' THEN
    IF jsonb_typeof(p_value)<>'object' OR p_value->>'currency_code'<>'CNY'
       OR (SELECT array_agg(k ORDER BY k) FROM jsonb_object_keys(p_value) k)<>ARRAY['amount','currency_code']
       OR (p_value->>'amount') !~ '^(0|[1-9][0-9]{0,17})\.[0-9]{2}$' THEN RETURN false; END IF;
    amount_value:=(p_value->>'amount')::numeric; RETURN amount_value BETWEEN 0 AND 999999999999999999.99;
  ELSIF p_field='ceiling_price' THEN
    IF jsonb_typeof(p_value)<>'object' OR p_value->>'currency_code'<>'CNY' OR p_value->>'basis' NOT IN ('tax_inclusive','tax_exclusive','unspecified')
       OR (SELECT array_agg(k ORDER BY k) FROM jsonb_object_keys(p_value) k)<>ARRAY['amount','basis','currency_code']
       OR (p_value->>'amount') !~ '^(0|[1-9][0-9]{0,17})\.[0-9]{2}$' THEN RETURN false; END IF;
    amount_value:=(p_value->>'amount')::numeric; RETURN amount_value BETWEEN 0 AND 999999999999999999.99;
  ELSIF p_field IN ('expires_at','bid_open_at','bid_valid_until') THEN
    IF jsonb_typeof(p_value)<>'string' OR (p_value#>>'{}') !~ '(Z|[+-][0-9]{2}:[0-9]{2})$' THEN RETURN false; END IF;
    time_value:=(p_value#>>'{}')::timestamptz; RETURN isfinite(time_value);
  ELSIF p_field='bid_valid_days' THEN
    IF jsonb_typeof(p_value)<>'number' OR p_value::text !~ '^[0-9]+$' THEN RETURN false; END IF;
    days_value:=(p_value::text)::integer; RETURN days_value BETWEEN 1 AND 3650;
  END IF;
  RETURN false;
EXCEPTION WHEN OTHERS THEN RETURN false;
END
$$;

CREATE FUNCTION kb_bid_publish_extraction_section(
 p_target_id uuid,p_attempt integer,p_claim_token uuid,p_section_key text,p_heading_path jsonb,
 p_parent_start_offset bigint,p_parent_end_offset bigint,p_expected_current_publication_id uuid,
 p_candidate_graph jsonb,p_actor kb_actor_identity,p_idempotency_key text,
 p_request_bytes bytea,p_request_sha256 kb_sha256
)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE
 target_value bid_extraction_targets%ROWTYPE; attempt_value bid_extraction_attempts%ROWTYPE;
 project_value bid_projects%ROWTYPE; source_value bid_converted_source_artifacts%ROWTYPE;
 current_publication bid_current_section_publications%ROWTYPE; old_publication bid_section_publications%ROWTYPE;
 section_id uuid; section_hash kb_sha256; publication_id uuid:=gen_random_uuid(); publication_revision bigint;
 replay bytea; response jsonb; graph_hash kb_sha256; segment jsonb; segment_ordinal bigint;
 prior_end bigint:=NULL; start_value bigint; end_value bigint; quote_value text; quote_hash kb_sha256;
 span_id uuid; span_value jsonb; span_bytes bytea; candidate_id uuid; clause_value jsonb; clause_id uuid;
 fact_value jsonb; fact_id uuid; decision_value record; previous_decision uuid; previous_revision integer;
 response_clauses jsonb:='[]'::jsonb; response_facts jsonb:='[]'::jsonb; allowed_keys text[];
BEGIN
 SELECT * INTO STRICT target_value FROM bid_extraction_targets WHERE id=p_target_id;
 SELECT * INTO STRICT project_value FROM bid_projects WHERE id=target_value.project_id FOR UPDATE;
 replay:=kb_bid_idempotency_begin(p_actor,'bid.extraction.publish_section',p_idempotency_key,p_request_bytes,p_request_sha256);
 IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
 IF project_value.status<>'open' THEN RAISE EXCEPTION 'PROJECT_ENDED' USING ERRCODE='55000'; END IF;
 SELECT * INTO STRICT target_value FROM bid_extraction_targets WHERE id=p_target_id FOR UPDATE;
 SELECT * INTO STRICT attempt_value FROM bid_extraction_attempts
  WHERE target_id=p_target_id AND attempt=p_attempt FOR UPDATE;
 IF target_value.state<>'running' OR attempt_value.status<>'running'
   OR attempt_value.claim_token<>p_claim_token
   OR attempt_value.heartbeat_at+make_interval(secs=>attempt_value.claim_lease_ms/1000.0)<=clock_timestamp() THEN
   RAISE EXCEPTION 'EXTRACTION_LEASE_LOST' USING ERRCODE='40001';
 END IF;
 PERFORM 1 FROM bid_documents d WHERE d.id=target_value.document_id AND d.project_id=target_value.project_id
   AND d.parse_status='completed' AND d.conversion_generation=target_value.conversion_generation
   AND d.current_converted_source_artifact_id=target_value.source_artifact_id FOR UPDATE;
 IF NOT FOUND OR target_value.extraction_generation<>(SELECT max(t.extraction_generation) FROM bid_extraction_targets t
    WHERE t.project_id=target_value.project_id AND t.document_id=target_value.document_id) THEN
   RAISE EXCEPTION 'EXTRACTION_GENERATION_STALE' USING ERRCODE='40001';
 END IF;
 SELECT * INTO STRICT source_value FROM bid_converted_source_artifacts
  WHERE id=target_value.source_artifact_id FOR SHARE;
 IF jsonb_typeof(p_heading_path)<>'array' OR jsonb_array_length(p_heading_path)>16
   OR p_parent_start_offset<0 OR p_parent_end_offset<=p_parent_start_offset
   OR p_parent_end_offset>source_value.byte_length OR jsonb_typeof(p_candidate_graph)<>'array'
   OR jsonb_array_length(p_candidate_graph)>1024 THEN
   RAISE EXCEPTION 'SECTION_OR_GRAPH_BOUNDS_INVALID' USING ERRCODE='22023';
 END IF;
 section_hash:=encode(digest(substring(source_value.canonical_markdown_utf8
   FROM p_parent_start_offset::integer+1 FOR (p_parent_end_offset-p_parent_start_offset)::integer),'sha256'),'hex');
 SELECT id INTO section_id FROM bid_section_artifacts
  WHERE source_artifact_id=target_value.source_artifact_id AND section_key=p_section_key FOR SHARE;
 IF section_id IS NULL THEN
   section_id:=gen_random_uuid();
   INSERT INTO bid_section_artifacts(id,project_id,document_id,source_artifact_id,conversion_generation,
    section_key,heading_path,parent_start_offset,parent_end_offset,section_sha256)
   VALUES(section_id,target_value.project_id,target_value.document_id,target_value.source_artifact_id,
    target_value.conversion_generation,p_section_key,p_heading_path,p_parent_start_offset,p_parent_end_offset,section_hash);
 ELSE
   PERFORM 1 FROM bid_section_artifacts WHERE id=section_id AND heading_path=p_heading_path
    AND parent_start_offset=p_parent_start_offset AND parent_end_offset=p_parent_end_offset
    AND section_sha256=section_hash;
   IF NOT FOUND THEN RAISE EXCEPTION 'SECTION_ARTIFACT_IDENTITY_MISMATCH' USING ERRCODE='23505'; END IF;
 END IF;
 SELECT * INTO current_publication FROM bid_current_section_publications
   WHERE project_id=target_value.project_id AND document_id=target_value.document_id
     AND section_key=p_section_key FOR UPDATE;
 IF current_publication.publication_id IS DISTINCT FROM p_expected_current_publication_id THEN
   RAISE EXCEPTION 'SECTION_PUBLICATION_CAS_MISMATCH' USING ERRCODE='40001';
 END IF;
 graph_hash:=encode(digest(convert_to(p_candidate_graph::text,'UTF8'),'sha256'),'hex');
 publication_revision:=COALESCE(current_publication.revision,0)+1;
 INSERT INTO bid_section_publications(id,project_id,target_id,section_artifact_id,publication_revision,
   content_sha256,published_by,published_at) VALUES(publication_id,target_value.project_id,p_target_id,
   section_id,publication_revision,graph_hash,p_actor,clock_timestamp());
 IF current_publication.publication_id IS NOT NULL THEN
   SELECT * INTO STRICT old_publication FROM bid_section_publications
    WHERE id=current_publication.publication_id FOR SHARE;
   PERFORM 1 FROM bid_clauses WHERE publication_id=old_publication.id AND provenance='extracted'
     AND status='draft' ORDER BY id FOR UPDATE;
   UPDATE bid_clauses SET status='superseded',revision=revision+1,updated_at=clock_timestamp()
    WHERE publication_id=old_publication.id AND provenance='extracted' AND status='draft';
   FOR decision_value IN
     SELECT latest.id,latest.candidate_id,latest.revision FROM (
       SELECT DISTINCT ON (d.candidate_id) d.id,d.candidate_id,d.revision,d.status
       FROM bid_fact_suggestion_decisions d
       JOIN bid_extract_fact_candidates f ON f.id=d.candidate_id
       JOIN bid_extract_segment_candidates s ON s.id=f.segment_candidate_id
       WHERE s.section_artifact_id=old_publication.section_artifact_id
       ORDER BY d.candidate_id,d.revision DESC) latest
     WHERE latest.status='pending' ORDER BY latest.candidate_id
   LOOP
     INSERT INTO bid_fact_suggestion_decisions(id,project_id,candidate_id,revision,status,reason,
       decided_by,decided_at,previous_decision_id)
     VALUES(gen_random_uuid(),target_value.project_id,decision_value.candidate_id,
       decision_value.revision+1,'superseded','SOURCE_PUBLICATION_REPLACED',p_actor,clock_timestamp(),decision_value.id);
   END LOOP;
 END IF;

 FOR segment,segment_ordinal IN SELECT value,ordinality-1 FROM jsonb_array_elements(p_candidate_graph) WITH ORDINALITY LOOP
   allowed_keys:=ARRAY['clause','disposition','end_offset','facts','quote','reason_code','start_offset'];
   IF jsonb_typeof(segment)<>'object'
      OR (SELECT array_agg(k ORDER BY k) FROM jsonb_object_keys(segment) k)<>allowed_keys
      OR jsonb_typeof(segment->'facts')<>'array' OR jsonb_array_length(segment->'facts')>16 THEN
     RAISE EXCEPTION 'SEGMENT_SCHEMA_INVALID' USING ERRCODE='22023';
   END IF;
   BEGIN start_value:=(segment->>'start_offset')::bigint; end_value:=(segment->>'end_offset')::bigint;
   EXCEPTION WHEN OTHERS THEN RAISE EXCEPTION 'SEGMENT_OFFSET_INVALID' USING ERRCODE='22023'; END;
   quote_value:=segment->>'quote';
   IF quote_value IS NULL OR start_value<p_parent_start_offset OR end_value<=start_value
      OR end_value>p_parent_end_offset OR (prior_end IS NOT NULL AND start_value<prior_end)
      OR convert_to(quote_value,'UTF8')<>substring(source_value.canonical_markdown_utf8
         FROM start_value::integer+1 FOR (end_value-start_value)::integer) THEN
     RAISE EXCEPTION 'SEGMENT_QUOTE_OR_BOUNDS_INVALID' USING ERRCODE='23514';
   END IF;
   prior_end:=end_value; quote_hash:=encode(digest(convert_to(quote_value,'UTF8'),'sha256'),'hex');
   span_id:=gen_random_uuid();
   span_value:=jsonb_build_object('schema_version',2,'source_artifact_id',target_value.source_artifact_id,
     'section_artifact_id',section_id,'project_id',target_value.project_id,'document_id',target_value.document_id,
     'conversion_generation',target_value.conversion_generation,'section_key',p_section_key,
     'parent_start_offset',p_parent_start_offset,'parent_end_offset',p_parent_end_offset,
     'start_offset',start_value,'end_offset',end_value,'offset_unit','utf8_byte','quote',quote_value,
     'quote_sha256',quote_hash,'heading_path',p_heading_path);
   span_bytes:=convert_to(span_value::text,'UTF8');
   INSERT INTO bid_source_span_artifacts(id,schema_version,project_id,document_id,source_artifact_id,
     section_artifact_id,conversion_generation,section_key,parent_start_offset,parent_end_offset,
     start_offset,end_offset,offset_unit,quote,quote_sha256,heading_path,source_span_v2,canonical_payload,content_sha256)
   VALUES(span_id,2,target_value.project_id,target_value.document_id,target_value.source_artifact_id,section_id,
     target_value.conversion_generation,p_section_key,p_parent_start_offset,p_parent_end_offset,start_value,
     end_value,'utf8_byte',quote_value,quote_hash,p_heading_path,span_value,span_bytes,
     encode(digest(span_bytes,'sha256'),'hex'));
   candidate_id:=gen_random_uuid();
   INSERT INTO bid_extract_segment_candidates(id,target_id,section_artifact_id,source_span_artifact_id,ordinal)
     VALUES(candidate_id,p_target_id,section_id,span_id,segment_ordinal);
   INSERT INTO bid_extract_segment_dispositions(segment_candidate_id,disposition,reason_code)
     VALUES(candidate_id,segment->>'disposition',segment->>'reason_code');
   clause_value:=segment->'clause';
   IF (segment->>'disposition'='clause') IS DISTINCT FROM (jsonb_typeof(clause_value)='object')
      OR (segment->>'disposition'<>'clause' AND clause_value<>'null'::jsonb) THEN
     RAISE EXCEPTION 'SEGMENT_CLAUSE_CARDINALITY_INVALID' USING ERRCODE='23514';
   END IF;
   IF segment->>'disposition'='clause' THEN
     IF (SELECT array_agg(k ORDER BY k) FROM jsonb_object_keys(clause_value) k)
        <>ARRAY['kind','must','router_reason_code','text'] THEN
       RAISE EXCEPTION 'CLAUSE_CANDIDATE_SCHEMA_INVALID' USING ERRCODE='22023';
     END IF;
     clause_id:=gen_random_uuid();
     INSERT INTO bid_extract_clause_candidates(id,target_id,segment_candidate_id,proposal_text,must,
       proposed_kind,router_reason_code) VALUES(clause_id,p_target_id,candidate_id,clause_value->>'text',
       (clause_value->>'must')::boolean,clause_value->>'kind',clause_value->>'router_reason_code');
     INSERT INTO bid_clauses(id,project_id,publication_id,origin_candidate_id,provenance,status,kind,text,must,
       current_source_span_artifact_id,extracted_origin_source_span_artifact_id,revision,created_by)
     VALUES(clause_id,target_value.project_id,publication_id,clause_id,'extracted','draft',clause_value->>'kind',
       clause_value->>'text',(clause_value->>'must')::boolean,span_id,span_id,1,p_actor);
     response_clauses:=response_clauses||jsonb_build_array(jsonb_build_object('id',clause_id,'revision',1));
   END IF;
   FOR fact_value IN SELECT value FROM jsonb_array_elements(segment->'facts') LOOP
     IF jsonb_typeof(fact_value)<>'object'
       OR (SELECT array_agg(k ORDER BY k) FROM jsonb_object_keys(fact_value) k)
          <>ARRAY['confidence','field','typed_value']
       OR NOT kb_bid_validate_fact_value(fact_value->>'field',fact_value->'typed_value') THEN
       RAISE EXCEPTION 'FACT_CANDIDATE_SCHEMA_INVALID' USING ERRCODE='22023';
     END IF;
     fact_id:=gen_random_uuid();
     INSERT INTO bid_extract_fact_candidates(id,target_id,segment_candidate_id,field,typed_value,raw_quote,confidence)
      VALUES(fact_id,p_target_id,candidate_id,fact_value->>'field',fact_value->'typed_value',quote_value,
       (fact_value->>'confidence')::numeric);
     INSERT INTO bid_fact_suggestion_decisions(id,project_id,candidate_id,revision,status,decided_by,decided_at)
      VALUES(gen_random_uuid(),target_value.project_id,fact_id,1,'pending',p_actor,clock_timestamp());
     response_facts:=response_facts||jsonb_build_array(jsonb_build_object('id',fact_id,'field',fact_value->>'field'));
   END LOOP;
 END LOOP;

 INSERT INTO bid_current_section_publications(project_id,document_id,section_key,publication_id,revision)
 VALUES(target_value.project_id,target_value.document_id,p_section_key,publication_id,publication_revision)
 ON CONFLICT(project_id,document_id,section_key) DO UPDATE
   SET publication_id=EXCLUDED.publication_id,revision=EXCLUDED.revision;
 UPDATE bid_extraction_targets SET published_section_count=published_section_count+1,
   state=CASE WHEN published_section_count+1=expected_section_count THEN 'published' ELSE state END
 WHERE id=p_target_id AND published_section_count<expected_section_count;
 IF NOT FOUND THEN RAISE EXCEPTION 'EXTRACTION_PUBLICATION_COUNT_CAS_LOST' USING ERRCODE='40001'; END IF;
 IF (SELECT state FROM bid_extraction_targets WHERE id=p_target_id)='published' THEN
   UPDATE bid_extraction_attempts SET status='completed' WHERE target_id=p_target_id AND attempt=p_attempt;
 END IF;
 response:=jsonb_build_object('publication_id',publication_id,'section_artifact_id',section_id,
   'publication_revision',publication_revision,'content_sha256',graph_hash,'clauses',response_clauses,'facts',response_facts);
 INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
   entity_kind,entity_locator,before_revision,before_sha256,after_revision,after_sha256)
 VALUES(gen_random_uuid(),1,'bid.extraction.publish_section',p_actor,p_idempotency_key,p_request_sha256,
   encode(digest(convert_to(response::text,'UTF8'),'sha256'),'hex'),'bid_section_publication',
   jsonb_build_object('project_id',target_value.project_id,'document_id',target_value.document_id,'section_key',p_section_key),
   CASE WHEN current_publication.revision IS NULL THEN NULL ELSE current_publication.revision END,
   CASE WHEN old_publication.content_sha256 IS NULL THEN NULL ELSE old_publication.content_sha256 END,
   publication_revision,graph_hash);
 PERFORM kb_bid_idempotency_complete(p_actor,'bid.extraction.publish_section',p_idempotency_key,200,convert_to(response::text,'UTF8'));
 RETURN response;
END
$$;

CREATE FUNCTION kb_bid_current_fact_value(p_project bid_projects,p_field text)
RETURNS jsonb
LANGUAGE plpgsql
STABLE
SET search_path = pg_catalog, public
AS $$
BEGIN
 CASE p_field
  WHEN 'budget_amount' THEN RETURN CASE WHEN p_project.budget_amount IS NULL THEN NULL ELSE
    jsonb_build_object('amount',to_char(p_project.budget_amount,'FM99999999999999999990.00'),'currency_code','CNY') END;
  WHEN 'ceiling_price' THEN RETURN CASE WHEN p_project.ceiling_price IS NULL THEN NULL ELSE
    jsonb_build_object('amount',to_char(p_project.ceiling_price,'FM99999999999999999990.00'),
      'currency_code','CNY','basis',p_project.ceiling_basis) END;
  WHEN 'expires_at' THEN RETURN CASE WHEN p_project.expires_at IS NULL THEN NULL ELSE to_jsonb(kb_bid_utc_json_time(p_project.expires_at)) END;
  WHEN 'bid_open_at' THEN RETURN CASE WHEN p_project.bid_open_at IS NULL THEN NULL ELSE to_jsonb(kb_bid_utc_json_time(p_project.bid_open_at)) END;
  WHEN 'bid_valid_until' THEN RETURN CASE WHEN p_project.bid_valid_until IS NULL THEN NULL ELSE to_jsonb(kb_bid_utc_json_time(p_project.bid_valid_until)) END;
  WHEN 'bid_valid_days' THEN RETURN to_jsonb(p_project.bid_valid_days);
  ELSE RAISE EXCEPTION 'INVALID_FACT_FIELD' USING ERRCODE='22023';
 END CASE;
END
$$;

CREATE FUNCTION kb_bid_mutate_fact(
 p_project_id uuid,p_action text,p_candidate_id uuid,p_field text,p_typed_value jsonb,
 p_reason text,p_override_reason text,p_expected_fact_revision bigint,p_actor kb_actor_identity,
 p_idempotency_key text,p_request_bytes bytea,p_request_sha256 kb_sha256
)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE replay bytea; project_value bid_projects%ROWTYPE; old_project bid_projects%ROWTYPE;
 candidate_value bid_extract_fact_candidates%ROWTYPE; latest_decision bid_fact_suggestion_decisions%ROWTYPE;
 other_value record; effective_field text; effective_value jsonb; old_value jsonb; changed boolean:=false;
 ceiling_changed boolean:=false; new_fact_hash kb_sha256; new_ceiling_hash kb_sha256; response jsonb;
 before_revision bigint; before_hash kb_sha256; new_decision_id uuid;
BEGIN
 PERFORM kb_bid_require_human_actor(p_actor);
 IF p_action NOT IN ('accept','reject','set','clear') THEN RAISE EXCEPTION 'INVALID_FACT_ACTION' USING ERRCODE='22023'; END IF;
 replay:=kb_bid_idempotency_begin(p_actor,'bid.fact.'||p_action,p_idempotency_key,p_request_bytes,p_request_sha256);
 IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
 SELECT * INTO STRICT project_value FROM bid_projects WHERE id=p_project_id FOR UPDATE;
 old_project:=project_value; before_revision:=project_value.fact_revision; before_hash:=project_value.fact_sha256;
 IF project_value.status<>'open' THEN RAISE EXCEPTION 'PROJECT_ENDED' USING ERRCODE='55000'; END IF;
 IF p_action IN ('accept','reject') THEN
   SELECT * INTO STRICT candidate_value FROM bid_extract_fact_candidates WHERE id=p_candidate_id;
   IF candidate_value.target_id NOT IN (SELECT publication.target_id FROM bid_current_section_publications current_value
       JOIN bid_section_publications publication ON publication.id=current_value.publication_id
       JOIN bid_extract_segment_candidates segment_value ON segment_value.section_artifact_id=publication.section_artifact_id
       WHERE current_value.project_id=p_project_id AND segment_value.id=candidate_value.segment_candidate_id) THEN
     RAISE EXCEPTION 'FACT_SUGGESTION_NOT_CURRENT' USING ERRCODE='40001';
   END IF;
   SELECT * INTO STRICT latest_decision FROM bid_fact_suggestion_decisions
    WHERE candidate_id=p_candidate_id ORDER BY revision DESC LIMIT 1 FOR UPDATE;
   IF latest_decision.status<>'pending' THEN RAISE EXCEPTION 'FACT_SUGGESTION_NOT_PENDING' USING ERRCODE='40001'; END IF;
   effective_field:=candidate_value.field; effective_value:=candidate_value.typed_value;
 ELSE
   effective_field:=p_field; effective_value:=p_typed_value;
 END IF;
 IF p_action<>'reject' AND project_value.fact_revision<>p_expected_fact_revision THEN
   RAISE EXCEPTION 'FACT_REVISION_CAS_MISMATCH' USING ERRCODE='40001';
 END IF;
 IF p_action='reject' THEN
   IF p_reason IS NULL OR octet_length(btrim(p_reason)) NOT BETWEEN 1 AND 512 THEN
     RAISE EXCEPTION 'FACT_REJECTION_REASON_REQUIRED' USING ERRCODE='22023';
   END IF;
   INSERT INTO bid_fact_suggestion_decisions(id,project_id,candidate_id,revision,status,reason,
     decided_by,decided_at,previous_decision_id)
   VALUES(gen_random_uuid(),p_project_id,p_candidate_id,latest_decision.revision+1,'rejected',btrim(p_reason),
     p_actor,clock_timestamp(),latest_decision.id);
 ELSE
   IF p_action='clear' THEN effective_value:=NULL;
   ELSIF NOT kb_bid_validate_fact_value(effective_field,effective_value) THEN
     RAISE EXCEPTION 'FACT_VALUE_INVALID' USING ERRCODE='22023';
   END IF;
   old_value:=kb_bid_current_fact_value(project_value,effective_field);
   changed:=old_value IS DISTINCT FROM effective_value;
   IF p_action='accept' AND old_value IS NOT NULL AND changed
      AND (p_override_reason IS NULL OR octet_length(btrim(p_override_reason)) NOT BETWEEN 1 AND 512) THEN
     RAISE EXCEPTION 'FACT_OVERRIDE_REASON_REQUIRED' USING ERRCODE='22023';
   END IF;
   IF changed THEN
     CASE effective_field
      WHEN 'budget_amount' THEN UPDATE bid_projects SET budget_amount=CASE WHEN effective_value IS NULL THEN NULL ELSE (effective_value->>'amount')::numeric END,
        budget_currency=CASE WHEN effective_value IS NULL THEN NULL ELSE 'CNY' END WHERE id=p_project_id;
      WHEN 'ceiling_price' THEN
        UPDATE bid_projects SET ceiling_price=CASE WHEN effective_value IS NULL THEN NULL ELSE (effective_value->>'amount')::numeric END,
          ceiling_currency=CASE WHEN effective_value IS NULL THEN NULL ELSE 'CNY' END,
          ceiling_basis=CASE WHEN effective_value IS NULL THEN 'unspecified' ELSE effective_value->>'basis' END WHERE id=p_project_id;
        ceiling_changed:=true;
      WHEN 'expires_at' THEN UPDATE bid_projects SET expires_at=CASE WHEN effective_value IS NULL THEN NULL ELSE (effective_value#>>'{}')::timestamptz END WHERE id=p_project_id;
      WHEN 'bid_open_at' THEN UPDATE bid_projects SET bid_open_at=CASE WHEN effective_value IS NULL THEN NULL ELSE (effective_value#>>'{}')::timestamptz END WHERE id=p_project_id;
      WHEN 'bid_valid_until' THEN UPDATE bid_projects SET bid_valid_until=CASE WHEN effective_value IS NULL THEN NULL ELSE (effective_value#>>'{}')::timestamptz END WHERE id=p_project_id;
      WHEN 'bid_valid_days' THEN UPDATE bid_projects SET bid_valid_days=CASE WHEN effective_value IS NULL THEN NULL ELSE (effective_value::text)::integer END WHERE id=p_project_id;
      ELSE RAISE EXCEPTION 'INVALID_FACT_FIELD' USING ERRCODE='22023';
     END CASE;
     UPDATE bid_projects SET fact_revision=fact_revision+1,updated_at=clock_timestamp(),
       ceiling_revision=ceiling_revision+CASE WHEN ceiling_changed THEN 1 ELSE 0 END WHERE id=p_project_id;
     SELECT * INTO project_value FROM bid_projects WHERE id=p_project_id;
     new_fact_hash:=encode(digest(convert_to(kb_bid_fact_payload(project_value)::text,'UTF8'),'sha256'),'hex');
     new_ceiling_hash:=encode(digest(convert_to(kb_bid_ceiling_payload(project_value)::text,'UTF8'),'sha256'),'hex');
     UPDATE bid_projects SET fact_sha256=new_fact_hash,
       ceiling_identity_sha256=CASE WHEN ceiling_changed THEN new_ceiling_hash ELSE ceiling_identity_sha256 END
       WHERE id=p_project_id RETURNING * INTO project_value;
     UPDATE bid_current_parts SET stale=true,
       stale_reason_codes=(SELECT ARRAY(SELECT DISTINCT x FROM unnest(stale_reason_codes||ARRAY['PROJECT_FACT_CHANGED']) x ORDER BY x))
      WHERE project_id=p_project_id AND (part_key='1' OR (effective_field IN ('expires_at','bid_open_at','bid_valid_until','bid_valid_days','schedule_payment','ceiling_price') AND part_key='6:letter'));
     IF ceiling_changed THEN
       UPDATE bid_quote_snapshots SET eligibility=CASE
         WHEN eligibility='eligible' THEN 'ineligible_ceiling_changed'
         WHEN eligibility='ineligible_pricing_changed' THEN 'ineligible_multiple_inputs_changed'
         ELSE eligibility END
       WHERE project_id=p_project_id AND eligibility IN ('eligible','ineligible_pricing_changed');
       UPDATE bid_current_parts SET stale=true,
         stale_reason_codes=(SELECT ARRAY(SELECT DISTINCT x FROM unnest(stale_reason_codes||ARRAY['CEILING_CHANGED']) x ORDER BY x))
        WHERE project_id=p_project_id AND part_key IN ('6:quote','6:letter');
     END IF;
   ELSE
     new_fact_hash:=project_value.fact_sha256; new_ceiling_hash:=project_value.ceiling_identity_sha256;
   END IF;
   IF p_action='accept' THEN
     new_decision_id:=gen_random_uuid();
     INSERT INTO bid_fact_suggestion_decisions(id,project_id,candidate_id,revision,status,reason,
       decided_by,decided_at,previous_decision_id)
     VALUES(new_decision_id,p_project_id,p_candidate_id,latest_decision.revision+1,'accepted',
       NULLIF(btrim(COALESCE(p_override_reason,'')),''),p_actor,clock_timestamp(),latest_decision.id);
     FOR other_value IN
       SELECT latest.id,latest.candidate_id,latest.revision FROM (
        SELECT DISTINCT ON (d.candidate_id) d.id,d.candidate_id,d.revision,d.status,f.field
        FROM bid_fact_suggestion_decisions d JOIN bid_extract_fact_candidates f ON f.id=d.candidate_id
        WHERE d.project_id=p_project_id AND f.field=effective_field
        ORDER BY d.candidate_id,d.revision DESC) latest
       WHERE latest.status='pending' AND latest.candidate_id<>p_candidate_id ORDER BY latest.candidate_id
     LOOP
       INSERT INTO bid_fact_suggestion_decisions(id,project_id,candidate_id,revision,status,reason,
         decided_by,decided_at,previous_decision_id)
       VALUES(gen_random_uuid(),p_project_id,other_value.candidate_id,other_value.revision+1,'superseded',
         'SAME_FIELD_ACCEPTED',p_actor,clock_timestamp(),other_value.id);
     END LOOP;
   END IF;
 END IF;
 SELECT * INTO project_value FROM bid_projects WHERE id=p_project_id;
 response:=jsonb_build_object('project_id',p_project_id,'action',p_action,'field',effective_field,
   'fact_revision',project_value.fact_revision,'fact_sha256',project_value.fact_sha256,
   'ceiling_revision',project_value.ceiling_revision,'ceiling_identity_sha256',project_value.ceiling_identity_sha256,
   'changed',changed);
 INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
   entity_kind,entity_locator,before_revision,before_sha256,after_revision,after_sha256)
 VALUES(gen_random_uuid(),1,'bid.fact.'||p_action,p_actor,p_idempotency_key,p_request_sha256,
   encode(digest(convert_to(response::text,'UTF8'),'sha256'),'hex'),'bid_project_fact',
   jsonb_build_object('project_id',p_project_id,'field',effective_field,'candidate_id',p_candidate_id),
   before_revision,before_hash,project_value.fact_revision,project_value.fact_sha256);
 PERFORM kb_bid_idempotency_complete(p_actor,'bid.fact.'||p_action,p_idempotency_key,200,convert_to(response::text,'UTF8'));
 RETURN response;
END
$$;

CREATE FUNCTION kb_bid_create_clause(
 p_clause_id uuid,p_project_id uuid,p_text text,p_kind text,p_must boolean,p_actor kb_actor_identity,
 p_idempotency_key text,p_request_bytes bytea,p_request_sha256 kb_sha256
)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE replay bytea; project_value bid_projects%ROWTYPE; response jsonb; semantic_hash kb_sha256;
BEGIN
 PERFORM kb_bid_require_human_actor(p_actor);
 replay:=kb_bid_idempotency_begin(p_actor,'bid.clause.create',p_idempotency_key,p_request_bytes,p_request_sha256);
 IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
 SELECT * INTO STRICT project_value FROM bid_projects WHERE id=p_project_id FOR UPDATE;
 IF project_value.status<>'open' THEN RAISE EXCEPTION 'PROJECT_ENDED' USING ERRCODE='55000'; END IF;
 INSERT INTO bid_clauses(id,project_id,provenance,status,kind,text,must,revision,created_by)
 VALUES(p_clause_id,p_project_id,'manual','draft',p_kind,p_text,p_must,1,p_actor);
 semantic_hash:=kb_bid_clause_semantic_sha256(p_clause_id,'draft',p_kind,p_text,p_must,1);
 response:=jsonb_build_object('id',p_clause_id,'revision',1,'status','draft','kind',p_kind,
   'family',kb_bid_family_for_kind(p_kind),'provenance','manual','semantic_sha256',semantic_hash);
 INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
   entity_kind,entity_locator,after_revision,after_sha256)
 VALUES(gen_random_uuid(),1,'bid.clause.create',p_actor,p_idempotency_key,p_request_sha256,
  encode(digest(convert_to(response::text,'UTF8'),'sha256'),'hex'),'bid_clause',jsonb_build_object('clause_id',p_clause_id),1,semantic_hash);
 PERFORM kb_bid_idempotency_complete(p_actor,'bid.clause.create',p_idempotency_key,201,convert_to(response::text,'UTF8'));
 RETURN response;
END
$$;

CREATE FUNCTION kb_bid_mutate_clause(
 p_project_id uuid,p_clause_id uuid,p_action text,p_patch jsonb,p_expected_revision bigint,
 p_actor kb_actor_identity,p_idempotency_key text,p_request_bytes bytea,p_request_sha256 kb_sha256
)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE replay bytea; project_value bid_projects%ROWTYPE; clause_value bid_clauses%ROWTYPE;
 old_status text;old_kind text;old_text text;old_must boolean;old_family text;old_revision bigint;
 new_status text;new_kind text;new_text text;new_must boolean;new_family text;new_provenance text;
 new_current_span uuid;new_reason text;new_generation bigint;current_router_generation bigint;
 matching_changed boolean:=false; set_kinds text[]:=ARRAY[]::text[]; response jsonb;
 before_hash kb_sha256;after_hash kb_sha256;allowed_patch text[]:=ARRAY['kind','must','text'];
BEGIN
 PERFORM kb_bid_require_human_actor(p_actor);
 IF p_action NOT IN ('patch','confirm','unconfirm','reject','delete') THEN
   RAISE EXCEPTION 'INVALID_CLAUSE_ACTION' USING ERRCODE='22023'; END IF;
 replay:=kb_bid_idempotency_begin(p_actor,'bid.clause.'||p_action,p_idempotency_key,p_request_bytes,p_request_sha256);
 IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
 SELECT * INTO STRICT project_value FROM bid_projects WHERE id=p_project_id FOR UPDATE;
 IF project_value.status<>'open' THEN RAISE EXCEPTION 'PROJECT_ENDED' USING ERRCODE='55000'; END IF;
 SELECT * INTO STRICT clause_value FROM bid_clauses WHERE id=p_clause_id AND project_id=p_project_id FOR UPDATE;
 IF clause_value.revision<>p_expected_revision THEN RAISE EXCEPTION 'CLAUSE_REVISION_CAS_MISMATCH' USING ERRCODE='40001'; END IF;
 old_status:=clause_value.status;old_kind:=clause_value.kind;old_text:=clause_value.text;
 old_must:=clause_value.must;old_family:=clause_value.family;old_revision:=clause_value.revision;
 before_hash:=kb_bid_clause_semantic_sha256(p_clause_id,old_status,old_kind,old_text,old_must,old_revision);
 new_status:=old_status;new_kind:=old_kind;new_text:=old_text;new_must:=old_must;
 new_provenance:=clause_value.provenance;new_current_span:=clause_value.current_source_span_artifact_id;
 new_reason:=clause_value.confirmation_required_reason;new_generation:=clause_value.confirmation_required_router_generation;
 IF p_action='patch' THEN
   IF jsonb_typeof(p_patch)<>'object' OR p_patch='{}'::jsonb
     OR EXISTS(SELECT 1 FROM jsonb_object_keys(p_patch) k WHERE NOT k=ANY(allowed_patch)) THEN
     RAISE EXCEPTION 'CLAUSE_PATCH_SCHEMA_INVALID' USING ERRCODE='22023';
   END IF;
   IF p_patch?'text' THEN new_text:=p_patch->>'text'; END IF;
   IF p_patch?'kind' THEN new_kind:=p_patch->>'kind'; END IF;
   IF p_patch?'must' THEN
     IF jsonb_typeof(p_patch->'must')<>'boolean' THEN RAISE EXCEPTION 'CLAUSE_MUST_INVALID' USING ERRCODE='22023'; END IF;
     new_must:=(p_patch->>'must')::boolean;
   END IF;
   IF new_text IS NOT DISTINCT FROM old_text AND new_kind IS NOT DISTINCT FROM old_kind
      AND new_must IS NOT DISTINCT FROM old_must THEN RAISE EXCEPTION 'CLAUSE_PATCH_NO_CHANGES' USING ERRCODE='22023'; END IF;
   IF clause_value.provenance='extracted' THEN new_provenance:='manual_after_edit';new_current_span:=NULL; END IF;
 ELSIF p_action='confirm' THEN
   PERFORM 1 FROM application_maintenance_gate WHERE singleton_key AND mode='open' FOR SHARE;
   IF NOT FOUND THEN RAISE EXCEPTION 'MAINTENANCE_CONFIRM_BLOCKED' USING ERRCODE='55000'; END IF;
   IF old_status<>'draft' THEN RAISE EXCEPTION 'CLAUSE_NOT_DRAFT' USING ERRCODE='55000'; END IF;
   SELECT promotion_generation INTO current_router_generation FROM kind_router_current WHERE singleton_key FOR SHARE;
   IF clause_value.confirmation_required_router_generation IS NOT NULL
      AND clause_value.confirmation_required_router_generation<>current_router_generation THEN
     RAISE EXCEPTION 'KIND_ROUTER_MARKER_STALE' USING ERRCODE='40001';
   END IF;
   new_status:='confirmed';new_reason:=NULL;new_generation:=NULL;
 ELSIF p_action='unconfirm' THEN
   IF old_status<>'confirmed' THEN RAISE EXCEPTION 'CLAUSE_NOT_CONFIRMED' USING ERRCODE='55000'; END IF;
   new_status:='draft';
 ELSIF p_action='reject' THEN
   IF old_status NOT IN ('draft','confirmed') THEN RAISE EXCEPTION 'CLAUSE_NOT_CURRENT' USING ERRCODE='55000'; END IF;
   new_status:='rejected';new_reason:=NULL;new_generation:=NULL;
 ELSIF p_action='delete' THEN
   IF old_status='superseded' THEN RAISE EXCEPTION 'CLAUSE_NOT_CURRENT' USING ERRCODE='55000'; END IF;
   new_status:='superseded';new_reason:=NULL;new_generation:=NULL;
 END IF;
 new_family:=kb_bid_family_for_kind(new_kind);
 matching_changed:=((old_status='confirmed' AND old_family IS NOT NULL)
    IS DISTINCT FROM (new_status='confirmed' AND new_family IS NOT NULL))
   OR (old_status='confirmed' AND new_status='confirmed' AND old_family IS NOT NULL
       AND (old_kind,old_text,old_must) IS DISTINCT FROM (new_kind,new_text,new_must));
 IF old_status='confirmed' AND old_kind=ANY(ARRAY['service','pricing','schedule_payment','schedule_delivery','evaluation','procedural'])
    AND (new_status<>'confirmed' OR (old_kind,old_text,old_must) IS DISTINCT FROM (new_kind,new_text,new_must)) THEN
   set_kinds:=array_append(set_kinds,old_kind);
 END IF;
 IF new_status='confirmed' AND new_kind=ANY(ARRAY['service','pricing','schedule_payment','schedule_delivery','evaluation','procedural'])
    AND (old_status<>'confirmed' OR (old_kind,old_text,old_must) IS DISTINCT FROM (new_kind,new_text,new_must)) THEN
   set_kinds:=array_append(set_kinds,new_kind);
 END IF;
 SELECT COALESCE(array_agg(DISTINCT x ORDER BY x),ARRAY[]::text[]) INTO set_kinds FROM unnest(set_kinds) x;
 UPDATE bid_clauses SET status=new_status,kind=new_kind,text=new_text,must=new_must,
   provenance=new_provenance,current_source_span_artifact_id=new_current_span,revision=revision+1,
   confirmation_required_reason=new_reason,confirmation_required_router_generation=new_generation,
   updated_at=clock_timestamp() WHERE id=p_clause_id RETURNING * INTO clause_value;
 PERFORM kb_bid_stale_for_clause_change(p_project_id,matching_changed,set_kinds);
 after_hash:=kb_bid_clause_semantic_sha256(p_clause_id,clause_value.status,clause_value.kind,
   clause_value.text,clause_value.must,clause_value.revision);
 response:=jsonb_build_object('id',p_clause_id,'revision',clause_value.revision,'status',clause_value.status,
   'kind',clause_value.kind,'family',clause_value.family,'provenance',clause_value.provenance,
   'confirmation_required_reason',clause_value.confirmation_required_reason,
   'confirmation_required_router_generation',clause_value.confirmation_required_router_generation,
   'semantic_sha256',after_hash);
 INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
  entity_kind,entity_locator,before_revision,before_sha256,after_revision,after_sha256)
 VALUES(gen_random_uuid(),1,'bid.clause.'||p_action,p_actor,p_idempotency_key,p_request_sha256,
  encode(digest(convert_to(response::text,'UTF8'),'sha256'),'hex'),'bid_clause',jsonb_build_object('clause_id',p_clause_id),
  old_revision,before_hash,clause_value.revision,after_hash);
 PERFORM kb_bid_idempotency_complete(p_actor,'bid.clause.'||p_action,p_idempotency_key,200,convert_to(response::text,'UTF8'));
 RETURN response;
END
$$;

CREATE FUNCTION kb_bid_register_kind_router_contract(
 p_version text,p_canonical_payload bytea,p_content_sha256 kb_sha256,p_actor kb_actor_identity,
 p_idempotency_key text,p_request_bytes bytea,p_request_sha256 kb_sha256
)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE replay bytea; payload jsonb;response jsonb;
BEGIN
 PERFORM 1 FROM application_maintenance_gate WHERE singleton_key AND mode='maintenance' FOR SHARE;
 IF NOT FOUND THEN RAISE EXCEPTION 'MAINTENANCE_REQUIRED' USING ERRCODE='55000'; END IF;
 replay:=kb_bid_idempotency_begin(p_actor,'bid.kind_router.register',p_idempotency_key,p_request_bytes,p_request_sha256);
 IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
 IF p_content_sha256<>encode(digest(p_canonical_payload,'sha256'),'hex') THEN
  RAISE EXCEPTION 'KIND_ROUTER_HASH_MISMATCH' USING ERRCODE='22023'; END IF;
 BEGIN payload:=convert_from(p_canonical_payload,'UTF8')::jsonb;
 EXCEPTION WHEN OTHERS THEN RAISE EXCEPTION 'KIND_ROUTER_PAYLOAD_INVALID' USING ERRCODE='22023'; END;
 IF payload->>'version'<>p_version OR payload->>'schema_version'<>'1'
   OR payload->'family'<>jsonb_build_object('technical','technical','qualification','commercial','service','commercial',
      'pricing',NULL,'schedule_delivery',NULL,'schedule_payment',NULL,'evaluation',NULL,'procedural',NULL)
   OR (payload?'overrides' AND jsonb_typeof(payload->'overrides')<>'object') THEN
  RAISE EXCEPTION 'KIND_ROUTER_CONTRACT_SCHEMA_INVALID' USING ERRCODE='22023'; END IF;
 INSERT INTO kind_router_contract_artifacts(version,schema_version,canonical_payload,content_sha256)
 VALUES(p_version,1,p_canonical_payload,p_content_sha256);
 response:=jsonb_build_object('version',p_version,'content_sha256',p_content_sha256);
 INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
  entity_kind,entity_locator,after_revision,after_sha256)
 VALUES(gen_random_uuid(),1,'bid.kind_router.register',p_actor,p_idempotency_key,p_request_sha256,
   encode(digest(convert_to(response::text,'UTF8'),'sha256'),'hex'),'kind_router_contract',
   jsonb_build_object('version',p_version),1,p_content_sha256);
 PERFORM kb_bid_idempotency_complete(p_actor,'bid.kind_router.register',p_idempotency_key,201,convert_to(response::text,'UTF8'));
 RETURN response;
END
$$;

CREATE FUNCTION kb_bid_route_kind(p_text text,p_contract_version text)
RETURNS text
LANGUAGE plpgsql
STABLE
SET search_path = pg_catalog, public
AS $$
DECLARE payload jsonb; override_kind text;text_hash text;
BEGIN
 SELECT convert_from(canonical_payload,'UTF8')::jsonb INTO STRICT payload
  FROM kind_router_contract_artifacts WHERE version=p_contract_version;
 text_hash:=encode(digest(convert_to(p_text,'UTF8'),'sha256'),'hex');
 override_kind:=payload->'overrides'->>text_hash;
 IF override_kind IS NOT NULL THEN
   IF override_kind NOT IN ('technical','qualification','service','pricing','schedule_delivery',
      'schedule_payment','evaluation','procedural') THEN
     RAISE EXCEPTION 'KIND_ROUTER_OVERRIDE_INVALID' USING ERRCODE='22023'; END IF;
   RETURN override_kind;
 END IF;
 IF p_text ~* '(支付|付款).*(接口|网关|API|密码|协议)' OR p_text ~* '(接口|网关|API|密码|协议).*(支付|付款)'
    OR p_text ~ '(设备|系统|接口|协议).*(性能|能力|参数|响应时间)' THEN RETURN 'technical'; END IF;
 IF p_text ~ '(许可证|ISO|等保|资质|软著|业绩|合同复印件|合同佐证|证书)' THEN RETURN 'qualification'; END IF;
 IF p_text ~ '(保证金|密封|投标函|授权委托|法定代表人|签章样式|递交)' THEN RETURN 'procedural'; END IF;
 IF p_text ~ '(付款|结算|支付).*(比例|金额|节点|账期|验收|主体)' THEN RETURN 'schedule_payment'; END IF;
 IF p_text ~ '(到货|交货|供货|工期|实施周期|交付地点|供货地点)' THEN RETURN 'schedule_delivery'; END IF;
 IF p_text ~ '(分项报价|计价口径|单列价格|报价明细)' THEN RETURN 'pricing'; END IF;
 IF p_text ~ '(评分项|权重|得分|评分标准)' THEN RETURN 'evaluation'; END IF;
 IF p_text ~ '(质保|驻场|培训|应急|7x24|SLA)' THEN RETURN 'service'; END IF;
 RETURN 'technical';
END
$$;

CREATE FUNCTION kb_bid_promote_kind_router(
 p_target_version text,p_expected_current_version text,p_expected_promotion_generation bigint,
 p_actor kb_actor_identity,p_idempotency_key text,p_request_bytes bytea,p_request_sha256 kb_sha256
)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE gate_value application_maintenance_gate%ROWTYPE; current_value kind_router_current%ROWTYPE;
 replay bytea;project_value record;clause_value bid_clauses%ROWTYPE;target_generation bigint;
 new_kind text;before_kind text;eligible boolean;router_recomputed boolean;matching_changed boolean;set_kinds text[];
 before_hash kb_sha256;after_hash kb_sha256;clause_response jsonb;response jsonb;
 changed_count bigint:=0;marker_count bigint:=0;
BEGIN
 SELECT * INTO STRICT gate_value FROM application_maintenance_gate WHERE singleton_key FOR UPDATE;
 IF gate_value.mode<>'maintenance' THEN RAISE EXCEPTION 'MAINTENANCE_REQUIRED' USING ERRCODE='55000'; END IF;
 SELECT * INTO STRICT current_value FROM kind_router_current WHERE singleton_key FOR UPDATE;
 replay:=kb_bid_idempotency_begin(p_actor,'bid.kind_router.promote',p_idempotency_key,p_request_bytes,p_request_sha256);
 IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
 IF current_value.version<>p_expected_current_version
   OR current_value.promotion_generation<>p_expected_promotion_generation THEN
  RAISE EXCEPTION 'KIND_ROUTER_PROMOTION_CAS_MISMATCH' USING ERRCODE='40001'; END IF;
 PERFORM 1 FROM kind_router_contract_artifacts WHERE version=p_target_version FOR SHARE;
 IF NOT FOUND THEN RAISE EXCEPTION 'KIND_ROUTER_TARGET_MISSING' USING ERRCODE='23503'; END IF;
 target_generation:=current_value.promotion_generation+1;
 FOR project_value IN SELECT id FROM bid_projects WHERE status='open' ORDER BY id FOR UPDATE LOOP
   matching_changed:=false;set_kinds:=ARRAY[]::text[];
   FOR clause_value IN SELECT * FROM bid_clauses WHERE project_id=project_value.id
      AND status IN ('draft','confirmed') ORDER BY id FOR UPDATE LOOP
     eligible:=clause_value.provenance='extracted' AND clause_value.current_source_span_artifact_id IS NOT NULL;
     router_recomputed:=false;new_kind:=clause_value.kind;before_kind:=clause_value.kind;
     before_hash:=kb_bid_clause_semantic_sha256(clause_value.id,clause_value.status,clause_value.kind,
       clause_value.text,clause_value.must,clause_value.revision);
     IF clause_value.confirmation_required_router_generation IS NOT NULL THEN
       IF eligible THEN new_kind:=kb_bid_route_kind(clause_value.text,p_target_version);router_recomputed:=true; END IF;
       UPDATE bid_clauses SET kind=new_kind,revision=revision+1,
         confirmation_required_reason='KIND_ROUTER_PROMOTION_RECONFIRM',
         confirmation_required_router_generation=target_generation,updated_at=clock_timestamp()
        WHERE id=clause_value.id RETURNING * INTO clause_value;
       marker_count:=marker_count+1;changed_count:=changed_count+1;
     ELSIF eligible AND clause_value.status='confirmed' THEN
       new_kind:=kb_bid_route_kind(clause_value.text,p_target_version);router_recomputed:=true;
       IF new_kind<>clause_value.kind THEN
         IF clause_value.family IS NOT NULL THEN matching_changed:=true; END IF;
         IF clause_value.kind=ANY(ARRAY['service','pricing','schedule_payment','schedule_delivery','evaluation','procedural']) THEN
           set_kinds:=array_append(set_kinds,clause_value.kind); END IF;
         UPDATE bid_clauses SET status='draft',kind=new_kind,revision=revision+1,
           confirmation_required_reason='KIND_ROUTER_PROMOTION_RECONFIRM',
           confirmation_required_router_generation=target_generation,updated_at=clock_timestamp()
          WHERE id=clause_value.id RETURNING * INTO clause_value;
         marker_count:=marker_count+1;changed_count:=changed_count+1;
       ELSE CONTINUE;
       END IF;
     ELSIF eligible AND clause_value.status='draft' THEN
       new_kind:=kb_bid_route_kind(clause_value.text,p_target_version);router_recomputed:=true;
       IF new_kind<>clause_value.kind THEN
         UPDATE bid_clauses SET kind=new_kind,revision=revision+1,updated_at=clock_timestamp()
          WHERE id=clause_value.id RETURNING * INTO clause_value;
         changed_count:=changed_count+1;
       ELSE CONTINUE;
       END IF;
     ELSE CONTINUE;
     END IF;
     after_hash:=kb_bid_clause_semantic_sha256(clause_value.id,clause_value.status,clause_value.kind,
       clause_value.text,clause_value.must,clause_value.revision);
     clause_response:=jsonb_build_object('clause_id',clause_value.id,'before_kind',before_kind,
       'after_kind',clause_value.kind,
       'target_version',p_target_version,'target_generation',target_generation,
       'router_recomputed',router_recomputed,'revision',clause_value.revision);
     INSERT INTO audit_events(id,schema_version,operation,actor_identity,request_sha256,response_sha256,
       entity_kind,entity_locator,before_revision,before_sha256,after_revision,after_sha256)
     VALUES(gen_random_uuid(),1,'bid.kind_router.promotion_clause','system:kind-router-promotion',p_request_sha256,
       encode(digest(convert_to(clause_response::text,'UTF8'),'sha256'),'hex'),'bid_clause',
       jsonb_build_object('clause_id',clause_value.id,'target_version',p_target_version,
         'target_generation',target_generation,'router_recomputed',router_recomputed),
       clause_value.revision-1,before_hash,clause_value.revision,after_hash);
   END LOOP;
   SELECT COALESCE(array_agg(DISTINCT x ORDER BY x),ARRAY[]::text[]) INTO set_kinds FROM unnest(set_kinds) x;
   PERFORM kb_bid_stale_for_clause_change(project_value.id,matching_changed,set_kinds);
   IF EXISTS(SELECT 1 FROM bid_clauses WHERE project_id=project_value.id
      AND confirmation_required_router_generation=target_generation) THEN
     UPDATE bid_current_parts SET stale=true,
       stale_reason_codes=(SELECT ARRAY(SELECT DISTINCT x FROM unnest(stale_reason_codes||ARRAY['KIND_ROUTER_RECONFIRMATION_REQUIRED']) x ORDER BY x))
      WHERE project_id=project_value.id;
   END IF;
 END LOOP;
 UPDATE kind_router_current SET version=p_target_version,promotion_generation=target_generation
  WHERE singleton_key AND version=p_expected_current_version
    AND promotion_generation=p_expected_promotion_generation;
 IF NOT FOUND THEN RAISE EXCEPTION 'KIND_ROUTER_PROMOTION_CAS_MISMATCH' USING ERRCODE='40001'; END IF;
 response:=jsonb_build_object('version',p_target_version,'promotion_generation',target_generation,
   'changed_clause_count',changed_count,'reconfirmation_marker_count',marker_count);
 INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
  entity_kind,entity_locator,before_revision,before_sha256,after_revision,after_sha256)
 SELECT gen_random_uuid(),1,'bid.kind_router.promote',p_actor,p_idempotency_key,p_request_sha256,
  encode(digest(convert_to(response::text,'UTF8'),'sha256'),'hex'),'kind_router_current',
  jsonb_build_object('singleton',true),p_expected_promotion_generation,old_artifact.content_sha256,
  target_generation,new_artifact.content_sha256
 FROM kind_router_contract_artifacts old_artifact,kind_router_contract_artifacts new_artifact
 WHERE old_artifact.version=p_expected_current_version AND new_artifact.version=p_target_version;
 PERFORM kb_bid_idempotency_complete(p_actor,'bid.kind_router.promote',p_idempotency_key,200,convert_to(response::text,'UTF8'));
 RETURN response;
END
$$;

CREATE FUNCTION kb_bid_fail_extraction(p_target_id uuid,p_attempt integer,p_claim_token uuid,p_error_code text,p_retry boolean)
RETURNS boolean
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE target_value bid_extraction_targets%ROWTYPE;
BEGIN
 SELECT * INTO target_value FROM bid_extraction_targets WHERE id=p_target_id;
 IF NOT FOUND THEN RETURN false; END IF;
 PERFORM 1 FROM bid_projects WHERE id=target_value.project_id FOR UPDATE;
 PERFORM 1 FROM bid_extraction_targets WHERE id=p_target_id FOR UPDATE;
 UPDATE bid_extraction_attempts SET status='failed',error_code=left(p_error_code,128)
  WHERE target_id=p_target_id AND attempt=p_attempt AND claim_token=p_claim_token AND status='running';
 IF NOT FOUND THEN RETURN false; END IF;
 UPDATE bid_extraction_targets SET state=CASE WHEN p_retry THEN 'pending' ELSE 'failed' END WHERE id=p_target_id;
 RETURN true;
END
$$;

CREATE FUNCTION kb_bid_retry_document_conversion(
 p_project_id uuid,p_document_id uuid,p_expected_generation integer,p_actor kb_actor_identity,
 p_idempotency_key text,p_request_bytes bytea,p_request_sha256 kb_sha256
)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE replay bytea;project_value bid_projects%ROWTYPE;document_value bid_documents%ROWTYPE;response jsonb;
BEGIN
 PERFORM kb_bid_require_human_actor(p_actor);
 replay:=kb_bid_idempotency_begin(p_actor,'bid.document.retry_conversion',p_idempotency_key,p_request_bytes,p_request_sha256);
 IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
 SELECT * INTO STRICT project_value FROM bid_projects WHERE id=p_project_id FOR UPDATE;
 SELECT * INTO STRICT document_value FROM bid_documents WHERE id=p_document_id AND project_id=p_project_id FOR UPDATE;
 IF project_value.status<>'open' OR document_value.parse_status='processing'
   OR document_value.conversion_generation<>p_expected_generation THEN
  RAISE EXCEPTION 'DOCUMENT_CONVERSION_CAS_MISMATCH' USING ERRCODE='40001'; END IF;
 UPDATE bid_documents SET conversion_generation=conversion_generation+1,parse_status='pending',
  current_converted_source_artifact_id=NULL,parsed_at=NULL,error_code=NULL WHERE id=p_document_id RETURNING * INTO document_value;
 response:=jsonb_build_object('document_id',p_document_id,'conversion_generation',document_value.conversion_generation,
  'parse_status','pending');
 INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
  entity_kind,entity_locator,before_revision,before_sha256,after_revision,after_sha256)
 VALUES(gen_random_uuid(),1,'bid.document.retry_conversion',p_actor,p_idempotency_key,p_request_sha256,
  encode(digest(convert_to(response::text,'UTF8'),'sha256'),'hex'),'bid_document',jsonb_build_object('document_id',p_document_id),
  p_expected_generation,document_value.original_sha256,document_value.conversion_generation,document_value.original_sha256);
 PERFORM kb_bid_idempotency_complete(p_actor,'bid.document.retry_conversion',p_idempotency_key,202,convert_to(response::text,'UTF8'));
 RETURN response;
END
$$;

CREATE FUNCTION kb_bid_end_project(
 p_project_id uuid,p_expected_fact_revision bigint,p_actor kb_actor_identity,p_idempotency_key text,
 p_request_bytes bytea,p_request_sha256 kb_sha256
)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE replay bytea;project_value bid_projects%ROWTYPE;response jsonb;
BEGIN
 PERFORM kb_bid_require_human_actor(p_actor);
 replay:=kb_bid_idempotency_begin(p_actor,'bid.project.end',p_idempotency_key,p_request_bytes,p_request_sha256);
 IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
 SELECT * INTO STRICT project_value FROM bid_projects WHERE id=p_project_id FOR UPDATE;
 IF project_value.status<>'open' OR project_value.fact_revision<>p_expected_fact_revision THEN
  RAISE EXCEPTION 'PROJECT_END_CAS_MISMATCH' USING ERRCODE='40001'; END IF;
 UPDATE bid_documents SET parse_status='failed',error_code='PROJECT_ENDED'
  WHERE project_id=p_project_id AND parse_status IN ('pending','processing');
 UPDATE bid_extraction_targets SET state='failed' WHERE project_id=p_project_id AND state IN ('pending','running');
 UPDATE bid_extraction_attempts SET status='failed',error_code='PROJECT_ENDED'
  WHERE target_id IN (SELECT id FROM bid_extraction_targets WHERE project_id=p_project_id) AND status='running';
 UPDATE bid_projects SET status='ended',ended_at=clock_timestamp(),updated_at=clock_timestamp() WHERE id=p_project_id;
 response:=jsonb_build_object('project_id',p_project_id,'status','ended','fact_revision',project_value.fact_revision);
 INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
  entity_kind,entity_locator,before_revision,before_sha256,after_revision,after_sha256)
 VALUES(gen_random_uuid(),1,'bid.project.end',p_actor,p_idempotency_key,p_request_sha256,
  encode(digest(convert_to(response::text,'UTF8'),'sha256'),'hex'),'bid_project',jsonb_build_object('project_id',p_project_id),
  project_value.fact_revision,project_value.fact_sha256,project_value.fact_revision,project_value.fact_sha256);
 PERFORM kb_bid_idempotency_complete(p_actor,'bid.project.end',p_idempotency_key,200,convert_to(response::text,'UTF8'));
 RETURN response;
END
$$;

CREATE VIEW bidding_projects AS
SELECT id,title,owner_user_id,ends_at,status,ended_at,fact_revision,fact_sha256,
 budget_amount,budget_currency,ceiling_price,ceiling_currency,ceiling_basis,
 ceiling_revision,ceiling_identity_sha256,expires_at,bid_open_at,bid_valid_until,
 bid_valid_days,matching_mutation_watermark,created_at,updated_at
FROM bid_projects;
CREATE VIEW bidding_documents AS
SELECT id,project_id,file_name,media_type,byte_length,original_object_ref,original_sha256,
 conversion_generation,parse_status,current_converted_source_artifact_id,created_at,parsed_at,error_code
FROM bid_documents;
CREATE VIEW bidding_extraction_sources AS
SELECT d.id AS document_id,d.project_id,d.file_name,d.conversion_generation,
 s.id AS source_artifact_id,s.canonical_markdown_utf8,s.markdown_sha256,s.byte_length
FROM bid_documents d JOIN bid_converted_source_artifacts s
 ON s.id=d.current_converted_source_artifact_id WHERE d.parse_status='completed';
CREATE VIEW bidding_extraction_targets AS
SELECT id,project_id,document_id,source_artifact_id,conversion_generation,extraction_generation,
 router_contract_version,policy_version,prompt_version,expected_section_count,published_section_count,state,created_at
FROM bid_extraction_targets;
CREATE VIEW bidding_current_section_publication_state AS
SELECT c.project_id,c.document_id,c.section_key,c.publication_id,c.revision,p.target_id,
 p.section_artifact_id,p.content_sha256,p.published_at
FROM bid_current_section_publications c JOIN bid_section_publications p ON p.id=c.publication_id;
CREATE VIEW bidding_current_clauses AS
SELECT c.id,c.project_id,c.publication_id,c.origin_candidate_id,c.provenance,c.status,c.kind,c.family,
 c.text,c.must,c.revision,c.confirmation_required_reason,c.confirmation_required_router_generation,
 c.current_source_span_artifact_id,current_span.source_span_v2 AS current_source_span_v2,
 c.extracted_origin_source_span_artifact_id,origin_span.source_span_v2 AS extracted_origin_source_span_v2,
 c.created_by,c.created_at,c.updated_at
FROM bid_clauses c
LEFT JOIN bid_source_span_artifacts current_span ON current_span.id=c.current_source_span_artifact_id
LEFT JOIN bid_source_span_artifacts origin_span ON origin_span.id=c.extracted_origin_source_span_artifact_id
WHERE c.status IN ('draft','confirmed');
CREATE VIEW bidding_clause_history AS
SELECT c.*,current_span.source_span_v2 AS current_source_span_v2,
 origin_span.source_span_v2 AS extracted_origin_source_span_v2
FROM bid_clauses c
LEFT JOIN bid_source_span_artifacts current_span ON current_span.id=c.current_source_span_artifact_id
LEFT JOIN bid_source_span_artifacts origin_span ON origin_span.id=c.extracted_origin_source_span_artifact_id;
CREATE VIEW bidding_current_fact_suggestions AS
SELECT f.id,f.target_id,d.project_id,f.segment_candidate_id,f.field,f.typed_value,f.raw_quote,
 f.confidence,d.revision AS decision_revision,s.source_span_v2
FROM bid_extract_fact_candidates f
JOIN bid_extract_segment_candidates segment_value ON segment_value.id=f.segment_candidate_id
JOIN bid_source_span_artifacts s ON s.id=segment_value.source_span_artifact_id
JOIN LATERAL (SELECT decision_value.* FROM bid_fact_suggestion_decisions decision_value
 WHERE decision_value.candidate_id=f.id ORDER BY decision_value.revision DESC LIMIT 1) d ON d.status='pending'
JOIN bid_current_section_publications current_value ON current_value.project_id=d.project_id
 AND current_value.publication_id=(SELECT p.id FROM bid_section_publications p
   WHERE p.target_id=f.target_id AND p.section_artifact_id=segment_value.section_artifact_id);
CREATE VIEW bidding_fact_suggestion_history AS
SELECT d.id,d.project_id,d.candidate_id,d.revision,d.status,d.reason,d.decided_by,d.decided_at,
 d.previous_decision_id,f.field,f.typed_value,f.raw_quote,f.confidence,s.source_span_v2
FROM bid_fact_suggestion_decisions d
JOIN bid_extract_fact_candidates f ON f.id=d.candidate_id
JOIN bid_extract_segment_candidates segment_value ON segment_value.id=f.segment_candidate_id
JOIN bid_source_span_artifacts s ON s.id=segment_value.source_span_artifact_id;
CREATE VIEW bidding_current_matching_reports AS
SELECT report.* FROM bid_matching_reports report
JOIN bid_current_matching_reports current_value ON current_value.report_id = report.id
JOIN bid_projects project ON project.id=report.project_id
WHERE project.status='open'
  AND report.generation=(SELECT max(manifest.generation) FROM bid_matching_manifests manifest WHERE manifest.project_id=report.project_id)
  AND report.mutation_watermark=project.matching_mutation_watermark;
CREATE VIEW bidding_matching_report_history AS
SELECT report.* FROM bid_matching_reports report;
CREATE VIEW bidding_current_technical_candidates AS
SELECT report.project_id,report.route_id,route.unit_id,candidate.requirement_artifact_id,
 candidate.id AS candidate_artifact_id,product.product_id,product.product_version_id,
 candidate.support,candidate.recommended,candidate.route_product_ordinal,candidate.retrieval_rank,
 candidate.retrieval_raw_score,candidate.candidate_identity_sha256,candidate.evidence_v1_sha256
FROM bidding_current_matching_reports report
JOIN bid_matching_routes route ON route.id=report.route_id AND route.route_kind='technical'
JOIN bid_matching_candidate_artifacts candidate ON candidate.report_id=report.id AND candidate.support='supported'
JOIN bid_matching_product_version_artifacts product ON product.id=candidate.product_version_artifact_id;
CREATE VIEW bidding_current_commercial_decisions AS
SELECT report.project_id,report.route_id,decision.*,requirement.clause_id,
 source.frozen_document_display_name
FROM bidding_current_matching_reports report
JOIN bid_matching_routes route ON route.id=report.route_id AND route.route_kind='commercial'
JOIN bid_matching_requirement_decisions decision ON decision.report_id=report.id
JOIN bid_matching_requirement_artifacts requirement ON requirement.id=decision.requirement_artifact_id
LEFT JOIN bid_matching_candidate_artifacts candidate ON candidate.id=decision.selected_candidate_artifact_id
LEFT JOIN bid_matching_evidence_artifacts evidence ON evidence.candidate_artifact_id=candidate.id AND evidence.ordinal=0
LEFT JOIN bid_matching_source_artifacts source ON source.id=evidence.source_chunk_artifact_id;
CREATE VIEW bidding_current_route_pick_sets AS
SELECT artifact.*,current_value.revision AS current_revision
FROM bid_current_route_pick_sets current_value
JOIN bid_route_pick_set_artifacts artifact ON artifact.id=current_value.pick_set_id
JOIN bid_current_matching_reports report_current ON report_current.project_id=current_value.project_id
 AND report_current.route_id=current_value.route_id AND report_current.report_id=artifact.source_report_artifact_id;
CREATE VIEW bidding_current_project_pick_sets AS
SELECT artifact.* FROM bid_current_project_pick_sets current_value
JOIN bid_project_pick_set_artifacts artifact ON artifact.id=current_value.pick_set_id;
CREATE VIEW bidding_current_quote_snapshots AS
SELECT snapshot.* FROM bid_quote_snapshots snapshot
JOIN bid_quote_current current_value ON current_value.active_finalized_snapshot_id = snapshot.id;
CREATE VIEW bidding_current_part_status AS
SELECT project_id, part_key, content_artifact_id, dependency_artifact_id, stale, stale_reason_codes
FROM bid_current_parts;
CREATE VIEW bidding_clause_set_identities AS
SELECT project_id,set_kind,revision,content_sha256,updated_at FROM bid_clause_set_identities;
CREATE VIEW bidding_kind_router_current AS
SELECT current_value.version,current_value.promotion_generation,artifact.content_sha256,artifact.canonical_payload
FROM kind_router_current current_value JOIN kind_router_contract_artifacts artifact ON artifact.version=current_value.version;

-- Runtime identities can read typed projections and execute checked mutations,
-- but receive no direct bidding table DML or current-pointer writes.
REVOKE ALL ON ALL TABLES IN SCHEMA public FROM PUBLIC;
REVOKE ALL ON ALL FUNCTIONS IN SCHEMA public FROM PUBLIC;
GRANT SELECT ON bidding_projects,bidding_documents,bidding_current_section_publication_state,
 bidding_current_clauses,bidding_clause_history,bidding_current_fact_suggestions,
 bidding_fact_suggestion_history,bidding_clause_set_identities,bidding_kind_router_current,
 bidding_current_matching_reports,bidding_matching_report_history,bidding_current_technical_candidates,
 bidding_current_commercial_decisions,bidding_current_route_pick_sets,bidding_current_project_pick_sets,
 bidding_current_quote_snapshots,bidding_current_part_status
TO kb_runtime_api,kb_runtime_worker;
GRANT SELECT ON bidding_extraction_sources,bidding_extraction_targets,
 bid_matching_manifests,bid_matching_routes,bid_matching_requirement_artifacts,
 bid_matching_product_version_artifacts,bid_matching_route_memberships,
 bid_matching_frozen_retrieved_hits,bid_matching_jobs,bid_matching_job_claims,
 bid_matching_staging_sets,bid_matching_staged_batches,bid_matching_staging_report_payloads
TO kb_runtime_worker;
GRANT EXECUTE ON FUNCTION
 kb_bid_create_project(uuid,text,uuid,timestamptz,timestamptz,kb_actor_identity,text,bytea,kb_sha256),
 kb_bid_upload_document(uuid,uuid,text,text,bigint,kb_object_ref,kb_sha256,kb_actor_identity,text,bytea,kb_sha256),
 kb_bid_retry_document_conversion(uuid,uuid,integer,kb_actor_identity,text,bytea,kb_sha256),
 kb_bid_end_project(uuid,bigint,kb_actor_identity,text,bytea,kb_sha256),
 kb_bid_mutate_fact(uuid,text,uuid,text,jsonb,text,text,bigint,kb_actor_identity,text,bytea,kb_sha256),
 kb_bid_create_clause(uuid,uuid,text,text,boolean,kb_actor_identity,text,bytea,kb_sha256),
 kb_bid_mutate_clause(uuid,uuid,text,jsonb,bigint,kb_actor_identity,text,bytea,kb_sha256),
 kb_bid_register_kind_router_contract(text,bytea,kb_sha256,kb_actor_identity,text,bytea,kb_sha256),
 kb_bid_promote_kind_router(text,text,bigint,kb_actor_identity,text,bytea,kb_sha256)
TO kb_runtime_api;
GRANT EXECUTE ON FUNCTION
 kb_bid_claim_document_conversion(uuid,uuid,text),kb_bid_heartbeat_document_conversion(uuid,uuid),
 kb_bid_complete_document_conversion(uuid,uuid,uuid,bytea,text,kb_sha256),
 kb_bid_fail_document_conversion(uuid,uuid,text,boolean),
 kb_bid_schedule_extraction(uuid,uuid,integer,text,text,kb_actor_identity,text,bytea,kb_sha256),
 kb_bid_claim_extraction(uuid,uuid,text),kb_bid_heartbeat_extraction(uuid,uuid,integer),
 kb_bid_publish_extraction_section(uuid,integer,uuid,text,jsonb,bigint,bigint,uuid,jsonb,kb_actor_identity,text,bytea,kb_sha256),
 kb_bid_fail_extraction(uuid,integer,uuid,text,boolean)
TO kb_runtime_worker;
