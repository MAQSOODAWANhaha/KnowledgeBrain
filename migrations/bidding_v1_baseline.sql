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
    CHECK (markdown_sha256 = encode(public.digest(canonical_markdown_utf8, 'sha256'), 'hex'))
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
       OR NEW.section_sha256 <> encode(public.digest(substring(source_value.canonical_markdown_utf8
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
    CHECK (quote_sha256 = encode(public.digest(convert_to(quote, 'UTF8'), 'sha256'), 'hex')),
    CHECK (content_sha256 = encode(public.digest(canonical_payload, 'sha256'), 'hex'))
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
    CHECK (content_sha256 = encode(public.digest(canonical_payload, 'sha256'), 'hex'))
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
 encode(public.digest(convert_to('{"family":{"evaluation":null,"pricing":null,"procedural":null,"qualification":"commercial","schedule_delivery":null,"schedule_payment":null,"service":"commercial","technical":"technical"},"schema_version":1,"version":"kind-router-v1"}', 'UTF8'), 'sha256'), 'hex'),
 '1970-01-01 UTC');
INSERT INTO kind_router_current(singleton_key, version, promotion_generation)
VALUES (true, 'kind-router-v1', 0);

CREATE TABLE procedural_router_contract_artifacts (
    version text PRIMARY KEY,
    schema_version smallint NOT NULL CHECK (schema_version=1),
    canonical_payload bytea NOT NULL,
    content_sha256 kb_sha256 NOT NULL UNIQUE,
    created_at timestamptz NOT NULL DEFAULT now(),
    CHECK (content_sha256=encode(public.digest(canonical_payload,'sha256'),'hex'))
);
CREATE TABLE procedural_router_current (
    singleton_key boolean PRIMARY KEY DEFAULT true CHECK (singleton_key),
    version text NOT NULL REFERENCES procedural_router_contract_artifacts(version) ON DELETE RESTRICT,
    promotion_generation bigint NOT NULL CHECK (promotion_generation>=0)
);
INSERT INTO procedural_router_contract_artifacts(version,schema_version,canonical_payload,content_sha256,created_at)
VALUES('procedural-router-v1',1,
  convert_to('{"schema_version":1,"version":"procedural-router-v1","overrides":{}}','UTF8'),
  encode(public.digest(convert_to('{"schema_version":1,"version":"procedural-router-v1","overrides":{}}','UTF8'),'sha256'),'hex'),
  '1970-01-01 UTC');
INSERT INTO procedural_router_current(singleton_key,version,promotion_generation)
VALUES(true,'procedural-router-v1',0);
CREATE TRIGGER kind_router_contract_artifacts_immutable
BEFORE UPDATE OR DELETE ON kind_router_contract_artifacts
FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();
CREATE TRIGGER procedural_router_contract_artifacts_immutable
BEFORE UPDATE OR DELETE ON procedural_router_contract_artifacts
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
    CHECK (content_sha256 = encode(public.digest(canonical_payload, 'sha256'), 'hex'))
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
    CHECK (payload_sha256 = encode(public.digest(canonical_items, 'sha256'), 'hex'))
);
CREATE TABLE bid_matching_staging_report_payloads (
    staging_set_id uuid PRIMARY KEY REFERENCES bid_matching_staging_sets(id) ON DELETE RESTRICT,
    canonical_payload bytea NOT NULL CHECK (octet_length(canonical_payload) <= 67108864),
    content_sha256 kb_sha256 NOT NULL,
    CHECK (content_sha256 = encode(public.digest(canonical_payload, 'sha256'), 'hex'))
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
    CHECK(chunk_sha256=encode(public.digest(chunk_utf8,'sha256'),'hex'))
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
    UNIQUE(staging_set_id,ordinal), CHECK(content_sha256=encode(public.digest(canonical_payload,'sha256'),'hex'))
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
    CHECK (chunk_sha256 = encode(public.digest(chunk_utf8, 'sha256'), 'hex'))
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
    CHECK (chunk_sha256 = encode(public.digest(chunk_utf8, 'sha256'), 'hex'))
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
    CHECK (quote_sha256 = encode(public.digest(quote_utf8, 'sha256'), 'hex'))
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
    CHECK (content_sha256 = encode(public.digest(canonical_payload, 'sha256'), 'hex'))
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
    CHECK (content_sha256 = encode(public.digest(canonical_payload, 'sha256'), 'hex'))
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

CREATE FUNCTION kb_match_report_canonical_payload_v1(p_report_id uuid)
RETURNS bytea
LANGUAGE plpgsql
STABLE
SET search_path = pg_catalog, public
AS $$
DECLARE report_value bid_matching_reports%ROWTYPE; route_value bid_matching_routes%ROWTYPE;
 decisions_json text; candidates_json text; groups_json text; sources_json text; reasons_json text;
 route_json text; payload text;
BEGIN
  SELECT * INTO STRICT report_value FROM bid_matching_reports WHERE id=p_report_id;
  SELECT * INTO STRICT route_value FROM bid_matching_routes WHERE id=report_value.route_id;

  SELECT COALESCE(string_agg(to_json(reason)::text,',' ORDER BY ordinal),'')
    INTO reasons_json
    FROM unnest(report_value.reason_codes) WITH ORDINALITY AS reason_value(reason,ordinal);

  SELECT COALESCE(string_agg(
    '{"requirement_artifact_id":'||to_json(decision.requirement_artifact_id::text)::text
    ||',"final_support":'||to_json(decision.final_support)::text
    ||',"system_decision":'||to_json(decision.system_decision)::text
    ||',"quality_status":'||to_json(decision.quality_status)::text
    ||',"reason_code":'||to_json(decision.reason_code)::text
    ||',"selected_candidate_artifact_id":'||CASE
         WHEN decision.selected_candidate_artifact_id IS NULL THEN 'null'
         ELSE to_json(decision.selected_candidate_artifact_id::text)::text END
    ||',"business_value":'||CASE
         WHEN selected.business_value_status='scored' THEN
           '{"status":"scored","value":'||to_json(to_char(selected.business_value,'FM99999999999999999990.000000'))::text
           ||',"source":"verifier"}'
         ELSE '{"status":"not_scored","reason":"NO_EVIDENCE"}' END
    ||'}',',' ORDER BY decision.ordinal),'')
    INTO decisions_json
    FROM bid_matching_requirement_decisions decision
    LEFT JOIN bid_matching_candidate_artifacts selected
      ON selected.report_id=decision.report_id AND selected.id=decision.selected_candidate_artifact_id
   WHERE decision.report_id=p_report_id;

  SELECT COALESCE(string_agg(
    '{"id":'||to_json(candidate.id::text)::text
    ||',"requirement_artifact_id":'||to_json(candidate.requirement_artifact_id::text)::text
    ||',"product_version_artifact_id":'||to_json(candidate.product_version_artifact_id::text)::text
    ||',"route_product_ordinal":'||candidate.route_product_ordinal::text
    ||',"retrieval_rank":'||candidate.retrieval_rank::text
    ||',"retrieval_raw_score":'||to_json(to_char(candidate.retrieval_raw_score,'FM99999999999999999990.000000'))::text
    ||',"candidate_identity_sha256":'||to_json(candidate.candidate_identity_sha256)::text
    ||',"evidence_v1_sha256":'||to_json(candidate.evidence_v1_sha256)::text
    ||',"evidence":'||evidence.payload
    ||',"support":'||to_json(candidate.support)::text
    ||',"business_value":'||CASE
         WHEN candidate.business_value_status='scored' THEN
           '{"status":"scored","value":'||to_json(to_char(candidate.business_value,'FM99999999999999999990.000000'))::text
           ||',"source":"verifier"}'
         ELSE '{"status":"not_scored","reason":"NO_EVIDENCE"}' END
    ||',"recommended":'||CASE WHEN candidate.recommended THEN 'true' ELSE 'false' END
    ||'}',',' ORDER BY candidate.requirement_artifact_id,candidate.route_product_ordinal,
         candidate.retrieval_rank,candidate.candidate_identity_sha256,candidate.evidence_v1_sha256,candidate.id),'')
    INTO candidates_json
    FROM bid_matching_candidate_artifacts candidate
    CROSS JOIN LATERAL (
      SELECT '{"schema_version":1,"items":['||COALESCE(string_agg(
        '{"source_chunk_artifact_id":'||to_json(item.source_chunk_artifact_id::text)::text
        ||',"document_id":'||to_json(item.document_id::text)::text
        ||',"document_display_name":'||to_json(item.document_display_name)::text
        ||',"source_chunk_id":'||to_json(item.source_chunk_id::text)::text
        ||',"source_chunk_sha256":'||to_json(item.source_chunk_sha256)::text
        ||',"quote":'||to_json(convert_from(item.quote_utf8,'UTF8'))::text
        ||',"start_offset":'||item.start_offset::text
        ||',"end_offset":'||item.end_offset::text
        ||',"offset_unit":'||to_json(item.offset_unit)::text||'}',',' ORDER BY item.ordinal),'')||']}' AS payload
        FROM bid_matching_evidence_artifacts item
       WHERE item.report_id=p_report_id AND item.candidate_artifact_id=candidate.id
    ) evidence
   WHERE candidate.report_id=p_report_id;

  SELECT COALESCE(string_agg(
    '{"requirement_artifact_id":'||to_json(group_value.requirement_artifact_id::text)::text
    ||',"support":'||to_json(group_value.support)::text
    ||',"candidate_artifact_ids":['||candidate_ids.payload||']}',',' ORDER BY group_value.ordinal),'')
    INTO groups_json
    FROM bid_matching_candidate_groups group_value
    CROSS JOIN LATERAL (
      SELECT COALESCE(string_agg(to_json(candidate.id::text)::text,',' ORDER BY candidate.id),'') AS payload
        FROM bid_matching_candidate_artifacts candidate
       WHERE candidate.report_id=p_report_id
         AND candidate.requirement_artifact_id=group_value.requirement_artifact_id
         AND candidate.support=group_value.support
    ) candidate_ids
   WHERE group_value.report_id=p_report_id;
  SELECT COALESCE(string_agg(
    '{"id":'||to_json(source_value.id::text)::text
    ||',"product_version_artifact_id":'||to_json(source_value.product_version_artifact_id::text)::text
    ||',"document_id":'||to_json(source_value.document_id::text)::text
    ||',"source_chunk_id":'||to_json(source_value.source_chunk_id::text)::text
    ||',"frozen_document_display_name":'||to_json(source_value.frozen_document_display_name)::text
    ||',"chunk_sha256":'||to_json(source_value.chunk_sha256)::text
    ||',"chunk_byte_length":'||source_value.chunk_byte_length::text
    ||',"retrieval_rank":'||source_value.retrieval_rank::text
    ||',"retrieval_raw_score":'||to_json(to_char(source_value.retrieval_raw_score,'FM99999999999999999990.000000'))::text
    ||',"retrieval_contract_version":'||to_json(source_value.retrieval_contract_version)::text||'}',',' ORDER BY source_value.id),'')
    INTO sources_json
    FROM bid_matching_source_artifacts source_value
   WHERE source_value.report_id=p_report_id;

  route_json := CASE route_value.route_kind
    WHEN 'technical' THEN '{"kind":"technical","unit_id":'||to_json(route_value.unit_id::text)::text||'}'
    ELSE '{"kind":"commercial"}' END;
  payload := '{"schema_version":1'
    ||',"report_id":'||to_json(report_value.id::text)::text
    ||',"manifest_id":'||to_json(report_value.manifest_id::text)::text
    ||',"job_id":'||to_json(report_value.job_id::text)::text
    ||',"route_id":'||to_json(report_value.route_id::text)::text
    ||',"route":'||route_json
    ||',"generation":'||report_value.generation::text
    ||',"mutation_watermark":'||report_value.mutation_watermark::text
    ||',"empty_disposition":'||CASE WHEN report_value.empty_disposition IS NULL THEN 'null' ELSE to_json(report_value.empty_disposition)::text END
    ||',"coverage":{"total":'||report_value.coverage_total::text
    ||',"eligible":'||report_value.coverage_total::text
    ||',"supported":'||report_value.coverage_supported::text
    ||',"contradicted":'||report_value.coverage_contradicted::text
    ||',"insufficient":'||report_value.coverage_insufficient::text
    ||',"unresolved":'||report_value.coverage_unresolved::text||'}'
    ||',"quality_status":'||to_json(report_value.quality_status)::text
    ||',"degraded":'||CASE WHEN report_value.degraded THEN 'true' ELSE 'false' END
    ||',"reason_codes":['||reasons_json||']'
    ||',"score":{"status":"not_scored","reason":"NO_EVIDENCE"}'
    ||',"requirement_decisions":['||decisions_json||']'
    ||',"candidates":['||candidates_json||']'
    ||',"candidate_groups":['||groups_json||']'
    ||',"source_artifacts":['||sources_json||']'
    ||',"ai_run_id":'||CASE WHEN report_value.ai_run_id IS NULL THEN 'null' ELSE to_json(report_value.ai_run_id::text)::text END
    ||',"ai_span_id":'||CASE WHEN report_value.ai_span_id IS NULL THEN 'null' ELSE to_json(report_value.ai_span_id::text)::text END
    ||'}';
  RETURN convert_to(payload,'UTF8');
END
$$;

CREATE FUNCTION kb_match_verify_report_v1()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE parsed jsonb; decision_count integer; supported_count integer;
 contradicted_count integer; insufficient_count integer; unresolved_count integer;
 expected_quality text; expected_reasons text[]; relation_reasons text[];
 candidate_count integer; source_count integer; group_count integer; evidence_count integer;
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
    SELECT count(*) INTO evidence_count FROM bid_matching_evidence_artifacts WHERE report_id=NEW.id;
    IF decision_count<>NEW.coverage_total OR supported_count<>NEW.coverage_supported
       OR contradicted_count<>NEW.coverage_contradicted OR insufficient_count<>NEW.coverage_insufficient
       OR unresolved_count<>NEW.coverage_unresolved OR expected_quality<>NEW.quality_status
       OR NEW.degraded<>(expected_quality<>'pass') OR expected_reasons<>NEW.reason_codes
       OR relation_reasons<>expected_reasons
       OR jsonb_array_length(parsed->'requirement_decisions')<>decision_count
       OR jsonb_array_length(parsed->'candidates')<>candidate_count
       OR jsonb_array_length(parsed->'candidate_groups')<>group_count
       OR jsonb_array_length(parsed->'source_artifacts')<>source_count
       OR (SELECT count(*)
             FROM jsonb_array_elements(parsed->'candidates') candidate_value
             CROSS JOIN LATERAL jsonb_array_elements(candidate_value->'evidence'->'items') item)<>evidence_count
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
      (SELECT value->>'id',value->>'requirement_artifact_id',value->>'product_version_artifact_id',
              value->>'candidate_identity_sha256',value->>'evidence_v1_sha256',value->>'support',
              value->'business_value'->>'status',value->'business_value'->>'value',
              value->>'route_product_ordinal',value->>'retrieval_rank',value->>'retrieval_raw_score',value->>'recommended'
         FROM jsonb_array_elements(parsed->'candidates') value
       EXCEPT
       SELECT id::text,requirement_artifact_id::text,product_version_artifact_id::text,
              candidate_identity_sha256,evidence_v1_sha256,support,business_value_status,
              CASE WHEN business_value IS NULL THEN NULL
                   ELSE to_char(business_value,'FM99999999999999999990.000000') END,
              route_product_ordinal::text,retrieval_rank::text,
              to_char(retrieval_raw_score,'FM99999999999999999990.000000'),recommended::text
         FROM bid_matching_candidate_artifacts WHERE report_id=NEW.id)
      UNION ALL
      (SELECT id::text,requirement_artifact_id::text,product_version_artifact_id::text,
              candidate_identity_sha256,evidence_v1_sha256,support,business_value_status,
              CASE WHEN business_value IS NULL THEN NULL
                   ELSE to_char(business_value,'FM99999999999999999990.000000') END,
              route_product_ordinal::text,retrieval_rank::text,
              to_char(retrieval_raw_score,'FM99999999999999999990.000000'),recommended::text
         FROM bid_matching_candidate_artifacts WHERE report_id=NEW.id
       EXCEPT
       SELECT value->>'id',value->>'requirement_artifact_id',value->>'product_version_artifact_id',
              value->>'candidate_identity_sha256',value->>'evidence_v1_sha256',value->>'support',
              value->'business_value'->>'status',value->'business_value'->>'value',
              value->>'route_product_ordinal',value->>'retrieval_rank',value->>'retrieval_raw_score',value->>'recommended'
         FROM jsonb_array_elements(parsed->'candidates') value)
    ) OR EXISTS(
      (SELECT value::text FROM jsonb_array_elements(parsed->'candidate_groups') value
       EXCEPT
       SELECT (convert_from(canonical_payload,'UTF8')::jsonb)::text
         FROM bid_matching_candidate_groups WHERE report_id=NEW.id)
      UNION ALL
      (SELECT (convert_from(canonical_payload,'UTF8')::jsonb)::text
         FROM bid_matching_candidate_groups WHERE report_id=NEW.id
       EXCEPT
       SELECT value::text FROM jsonb_array_elements(parsed->'candidate_groups') value)
    ) OR EXISTS(
      (SELECT value->>'id',value->>'product_version_artifact_id',value->>'document_id',value->>'source_chunk_id',
              value->>'frozen_document_display_name',value->>'chunk_sha256',value->>'chunk_byte_length',
              value->>'retrieval_rank',value->>'retrieval_raw_score',value->>'retrieval_contract_version'
         FROM jsonb_array_elements(parsed->'source_artifacts') value
       EXCEPT
       SELECT id::text,product_version_artifact_id::text,document_id::text,source_chunk_id::text,
              frozen_document_display_name,chunk_sha256,chunk_byte_length::text,retrieval_rank::text,
              to_char(retrieval_raw_score,'FM99999999999999999990.000000'),retrieval_contract_version
         FROM bid_matching_source_artifacts WHERE report_id=NEW.id)
      UNION ALL
      (SELECT id::text,product_version_artifact_id::text,document_id::text,source_chunk_id::text,
              frozen_document_display_name,chunk_sha256,chunk_byte_length::text,retrieval_rank::text,
              to_char(retrieval_raw_score,'FM99999999999999999990.000000'),retrieval_contract_version
         FROM bid_matching_source_artifacts WHERE report_id=NEW.id
       EXCEPT
       SELECT value->>'id',value->>'product_version_artifact_id',value->>'document_id',value->>'source_chunk_id',
              value->>'frozen_document_display_name',value->>'chunk_sha256',value->>'chunk_byte_length',
              value->>'retrieval_rank',value->>'retrieval_raw_score',value->>'retrieval_contract_version'
         FROM jsonb_array_elements(parsed->'source_artifacts') value)
    ) OR EXISTS(
      (SELECT candidate_value->>'id',item->>'source_chunk_artifact_id',item->>'document_id',
              item->>'document_display_name',item->>'source_chunk_id',item->>'source_chunk_sha256',
              item->>'quote',item->>'start_offset',item->>'end_offset',item->>'offset_unit',
              (item_ordinal-1)::text
         FROM jsonb_array_elements(parsed->'candidates') candidate_value
         CROSS JOIN LATERAL jsonb_array_elements(candidate_value->'evidence'->'items')
           WITH ORDINALITY AS evidence_item(item,item_ordinal)
       EXCEPT
       SELECT candidate_artifact_id::text,source_chunk_artifact_id::text,document_id::text,
              document_display_name,source_chunk_id::text,source_chunk_sha256,convert_from(quote_utf8,'UTF8'),
              start_offset::text,end_offset::text,offset_unit,ordinal::text
         FROM bid_matching_evidence_artifacts WHERE report_id=NEW.id)
      UNION ALL
      (SELECT candidate_artifact_id::text,source_chunk_artifact_id::text,document_id::text,
              document_display_name,source_chunk_id::text,source_chunk_sha256,convert_from(quote_utf8,'UTF8'),
              start_offset::text,end_offset::text,offset_unit,ordinal::text
         FROM bid_matching_evidence_artifacts WHERE report_id=NEW.id
       EXCEPT
       SELECT candidate_value->>'id',item->>'source_chunk_artifact_id',item->>'document_id',
              item->>'document_display_name',item->>'source_chunk_id',item->>'source_chunk_sha256',
              item->>'quote',item->>'start_offset',item->>'end_offset',item->>'offset_unit',
              (item_ordinal-1)::text
         FROM jsonb_array_elements(parsed->'candidates') candidate_value
         CROSS JOIN LATERAL jsonb_array_elements(candidate_value->'evidence'->'items')
           WITH ORDINALITY AS evidence_item(item,item_ordinal))
    ) THEN RAISE EXCEPTION 'MATCHING_REPORT_V1_PAYLOAD_RELATION_MISMATCH' USING ERRCODE='23514'; END IF;
    IF NEW.canonical_payload<>kb_match_report_canonical_payload_v1(NEW.id) THEN
      RAISE EXCEPTION 'MATCHING_REPORT_V1_NON_CANONICAL' USING ERRCODE='23514';
    END IF;
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
    IF NOT EXISTS(
      SELECT 1
        FROM bid_matching_jobs job
        JOIN bid_matching_manifests manifest ON manifest.id=job.manifest_id
        JOIN bid_matching_routes route ON route.id=job.route_id AND route.manifest_id=manifest.id
       WHERE job.id=NEW.job_id AND job.project_id=NEW.project_id
         AND job.manifest_id=NEW.manifest_id AND job.route_id=NEW.route_id
         AND route.project_id=NEW.project_id AND manifest.project_id=NEW.project_id
         AND manifest.generation=NEW.generation
         AND manifest.mutation_watermark=NEW.mutation_watermark
    ) OR EXISTS(
      SELECT 1
        FROM bid_matching_candidate_artifacts candidate
        JOIN bid_matching_requirement_artifacts requirement ON requirement.id=candidate.requirement_artifact_id
        LEFT JOIN bid_matching_route_memberships membership
          ON membership.route_id=NEW.route_id
         AND membership.product_version_artifact_id=candidate.product_version_artifact_id
       WHERE candidate.report_id=NEW.id
         AND (requirement.route_id<>NEW.route_id OR membership.route_id IS NULL)
    ) OR EXISTS(
      SELECT 1
        FROM bid_matching_source_artifacts source_value
        LEFT JOIN bid_matching_route_memberships membership
          ON membership.route_id=NEW.route_id
         AND membership.product_version_artifact_id=source_value.product_version_artifact_id
       WHERE source_value.report_id=NEW.id AND membership.route_id IS NULL
    ) OR EXISTS(
      SELECT 1 FROM bid_matching_requirement_decisions decision
      JOIN bid_matching_requirement_artifacts requirement ON requirement.id=decision.requirement_artifact_id
      WHERE decision.report_id=NEW.id AND requirement.route_id<>NEW.route_id
    ) OR EXISTS(
      SELECT 1 FROM bid_matching_candidate_groups group_value
      JOIN bid_matching_requirement_artifacts requirement ON requirement.id=group_value.requirement_artifact_id
      WHERE group_value.report_id=NEW.id AND requirement.route_id<>NEW.route_id
    ) THEN
      RAISE EXCEPTION 'MATCHING_REPORT_V1_SCOPE_MISMATCH' USING ERRCODE='23514';
    END IF;
    IF EXISTS(
      SELECT 1
        FROM bid_matching_candidate_artifacts candidate
        CROSS JOIN LATERAL (
          SELECT convert_to('{"schema_version":1,"items":['||COALESCE(string_agg(
            '{"source_chunk_artifact_id":'||to_json(item.source_chunk_artifact_id::text)::text
            ||',"document_id":'||to_json(item.document_id::text)::text
            ||',"document_display_name":'||to_json(item.document_display_name)::text
            ||',"source_chunk_id":'||to_json(item.source_chunk_id::text)::text
            ||',"source_chunk_sha256":'||to_json(item.source_chunk_sha256)::text
            ||',"quote":'||to_json(convert_from(item.quote_utf8,'UTF8'))::text
            ||',"start_offset":'||item.start_offset::text
            ||',"end_offset":'||item.end_offset::text
            ||',"offset_unit":'||to_json(item.offset_unit)::text||'}',',' ORDER BY item.ordinal),'')||']}','UTF8') AS payload
            FROM bid_matching_evidence_artifacts item
           WHERE item.report_id=NEW.id AND item.candidate_artifact_id=candidate.id
        ) evidence
       WHERE candidate.report_id=NEW.id
         AND candidate.evidence_v1_sha256<>encode(public.digest(evidence.payload,'sha256'),'hex')
    ) OR EXISTS(
      SELECT 1
        FROM (
          SELECT item.ordinal,row_number() OVER (
            PARTITION BY item.candidate_artifact_id
            ORDER BY item.source_chunk_artifact_id,item.start_offset,item.end_offset,item.quote_utf8)-1 AS expected_ordinal
            FROM bid_matching_evidence_artifacts item WHERE item.report_id=NEW.id
        ) ordered_item
       WHERE ordered_item.ordinal<>ordered_item.expected_ordinal
    ) THEN
      RAISE EXCEPTION 'EVIDENCE_V1_CANONICAL_MISMATCH' USING ERRCODE='23514';
    END IF;
    IF EXISTS(
      (SELECT DISTINCT candidate.requirement_artifact_id,candidate.support
         FROM bid_matching_candidate_artifacts candidate WHERE candidate.report_id=NEW.id
       EXCEPT
       SELECT group_value.requirement_artifact_id,group_value.support
         FROM bid_matching_candidate_groups group_value WHERE group_value.report_id=NEW.id)
      UNION ALL
      (SELECT group_value.requirement_artifact_id,group_value.support
         FROM bid_matching_candidate_groups group_value WHERE group_value.report_id=NEW.id
       EXCEPT
       SELECT DISTINCT candidate.requirement_artifact_id,candidate.support
         FROM bid_matching_candidate_artifacts candidate WHERE candidate.report_id=NEW.id)
    ) OR EXISTS(
      SELECT 1
        FROM bid_matching_candidate_groups group_value
        CROSS JOIN LATERAL (
          SELECT convert_to('{"requirement_artifact_id":'||to_json(group_value.requirement_artifact_id::text)::text
            ||',"support":'||to_json(group_value.support)::text
            ||',"candidate_artifact_ids":['
            ||COALESCE(string_agg(to_json(candidate.id::text)::text,',' ORDER BY candidate.id),'')||']}','UTF8') AS payload
            FROM bid_matching_candidate_artifacts candidate
           WHERE candidate.report_id=NEW.id
             AND candidate.requirement_artifact_id=group_value.requirement_artifact_id
             AND candidate.support=group_value.support
        ) expected_group
       WHERE group_value.report_id=NEW.id AND group_value.canonical_payload<>expected_group.payload
    ) OR EXISTS(
      SELECT 1 FROM (
        SELECT group_value.ordinal,row_number() OVER (
          ORDER BY group_value.requirement_artifact_id,
            CASE group_value.support WHEN 'contradicted' THEN 0 WHEN 'insufficient' THEN 1
              WHEN 'unresolved' THEN 2 ELSE 3 END)-1 AS expected_ordinal
          FROM bid_matching_candidate_groups group_value WHERE group_value.report_id=NEW.id
      ) ordered_group WHERE ordered_group.ordinal<>ordered_group.expected_ordinal
    ) OR EXISTS(
      SELECT 1 FROM (
        SELECT decision.ordinal,row_number() OVER (
          ORDER BY requirement.ordinal,requirement.id)-1 AS expected_ordinal
          FROM bid_matching_requirement_decisions decision
          JOIN bid_matching_requirement_artifacts requirement ON requirement.id=decision.requirement_artifact_id
         WHERE decision.report_id=NEW.id
      ) ordered_decision WHERE ordered_decision.ordinal<>ordered_decision.expected_ordinal
    ) THEN
      RAISE EXCEPTION 'MATCHING_REPORT_V1_COLLECTION_CANONICAL_MISMATCH' USING ERRCODE='23514';
    END IF;
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
    CHECK (content_sha256 = encode(public.digest(canonical_payload, 'sha256'), 'hex'))
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
    CHECK (content_sha256 = encode(public.digest(canonical_payload, 'sha256'), 'hex'))
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
SECURITY DEFINER
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
SECURITY DEFINER
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
    CHECK (content_sha256 = encode(public.digest(canonical_payload, 'sha256'), 'hex'))
);
CREATE TABLE bid_quote_current (
    quote_id uuid PRIMARY KEY REFERENCES bid_quotes(id) ON DELETE RESTRICT,
    project_id uuid NOT NULL UNIQUE REFERENCES bid_projects(id) ON DELETE RESTRICT,
    current_draft_revision_id uuid REFERENCES bid_quote_revisions(id) ON DELETE RESTRICT,
    active_finalized_snapshot_id uuid REFERENCES bid_quote_snapshots(id) ON DELETE RESTRICT,
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
    content_sha256 kb_sha256 NOT NULL,
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
    CHECK (content_sha256 = encode(public.digest(canonical_payload, 'sha256'), 'hex'))
);
CREATE TABLE bid_submission_profile_artifacts (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
    revision bigint NOT NULL CHECK (revision > 0),
    canonical_payload bytea NOT NULL,
    content_sha256 kb_sha256 NOT NULL,
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
    CHECK (content_sha256 = encode(public.digest(canonical_payload, 'sha256'), 'hex'))
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
    CHECK (segment_sha256 = encode(public.digest(segment_utf8, 'sha256'), 'hex'))
);
CREATE TABLE bid_procedural_classification_artifacts (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
    segment_id uuid NOT NULL REFERENCES bid_procedural_segment_artifacts(id) ON DELETE RESTRICT,
    revision integer NOT NULL CHECK (revision > 0),
    router_contract_version text NOT NULL REFERENCES procedural_router_contract_artifacts(version) ON DELETE RESTRICT,
    router_promotion_generation bigint NOT NULL CHECK (router_promotion_generation>=0),
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
    UNIQUE (project_id, id),
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
    content_sha256 kb_sha256 NOT NULL,
    media_type text NOT NULL CHECK (media_type IN (
        'application/pdf','image/png','image/jpeg','image/webp'
    )),
    byte_length bigint NOT NULL CHECK (byte_length BETWEEN 1 AND 20971520),
    pixel_width integer,
    pixel_height integer,
    validation_sha256 kb_sha256 NOT NULL,
    validation_status text NOT NULL CHECK (validation_status IN ('pending', 'valid', 'invalid')),
    status text NOT NULL CHECK (status IN ('draft', 'confirmed', 'rejected', 'superseded')),
    revision integer NOT NULL CHECK (revision > 0),
    created_by kb_actor_identity NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (project_id, id),
    CHECK ((media_type LIKE 'image/%' AND pixel_width BETWEEN 1 AND 20000 AND pixel_height BETWEEN 1 AND 20000)
        OR (media_type='application/pdf' AND pixel_width IS NULL AND pixel_height IS NULL)),
    CHECK (object_ref='objects/'||content_sha256)
);
CREATE TABLE bid_attachment_render_pages (
    attachment_id uuid NOT NULL,
    project_id uuid NOT NULL,
    page_ordinal integer NOT NULL CHECK (page_ordinal BETWEEN 0 AND 511),
    object_ref kb_object_ref NOT NULL,
    content_sha256 kb_sha256 NOT NULL,
    media_type text NOT NULL CHECK (media_type IN ('image/png','image/jpeg','image/webp')),
    byte_length bigint NOT NULL CHECK (byte_length BETWEEN 1 AND 20971520),
    pixel_width integer NOT NULL CHECK (pixel_width BETWEEN 1 AND 20000),
    pixel_height integer NOT NULL CHECK (pixel_height BETWEEN 1 AND 20000),
    PRIMARY KEY (attachment_id, page_ordinal),
    FOREIGN KEY (project_id, attachment_id)
        REFERENCES bid_procedural_attachments(project_id, id) ON DELETE RESTRICT,
    CHECK (object_ref='objects/'||content_sha256)
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
    terminal_at timestamptz,
    terminal_actor kb_actor_identity,
    UNIQUE (classification_id, revision),
    FOREIGN KEY (project_id, classification_id)
        REFERENCES bid_procedural_classification_artifacts(project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, attachment_id)
        REFERENCES bid_procedural_attachments(project_id, id) ON DELETE RESTRICT,
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
    pixel_width integer NOT NULL CHECK (pixel_width BETWEEN 1 AND 20000),
    pixel_height integer NOT NULL CHECK (pixel_height BETWEEN 1 AND 20000),
    created_by kb_actor_identity NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (project_id, id),
    CHECK (media_type IN ('image/png','image/jpeg','image/webp')),
    CHECK (byte_length BETWEEN 1 AND 20971520),
    CHECK (object_ref='objects/'||content_sha256)
);
CREATE TABLE bid_current_shot_placements (
    project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
    ordinal integer NOT NULL CHECK (ordinal >= 0),
    shot_artifact_id uuid NOT NULL,
    PRIMARY KEY (project_id, ordinal),
    UNIQUE (project_id, shot_artifact_id),
    FOREIGN KEY (project_id, shot_artifact_id)
        REFERENCES bid_shot_artifacts(project_id, id) ON DELETE RESTRICT
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
    CHECK (content_sha256 = encode(public.digest(canonical_payload, 'sha256'), 'hex'))
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
       encode(public.digest(convert_to('{"schema_version":1,"slot":"' || slot || '","version":"v1"}', 'UTF8'), 'sha256'), 'hex'),
       '1970-01-01 UTC'
FROM unnest(ARRAY['1','2:unit','2:unsectioned','3','4','5','6:letter','6:authorization','6:quote','6:implementation_plan','6:procedural']) AS slot;
INSERT INTO bid_template_contract_current(slot, version, promotion_generation)
SELECT slot, 'v1', 0 FROM bid_template_contract_artifacts;

CREATE TABLE bid_part_content_artifacts (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
    part_key text NOT NULL CHECK (
        part_key ~ '^(1|2:(unsectioned|[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})|3|4|5|6:(letter|authorization|quote|implementation_plan|procedural))$'
        AND part_key <> '2:00000000-0000-0000-0000-000000000000'
    ),
    revision bigint NOT NULL CHECK (revision > 0),
    canonical_markdown_utf8 bytea NOT NULL,
    content_sha256 kb_sha256 NOT NULL,
    created_by kb_actor_identity NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (project_id, part_key, revision),
    UNIQUE (project_id, id),
    CHECK (content_sha256 = encode(public.digest(canonical_markdown_utf8, 'sha256'), 'hex'))
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
    CHECK (content_sha256 = encode(public.digest(canonical_payload, 'sha256'), 'hex')),
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
    end_state_identity jsonb NOT NULL CHECK (jsonb_typeof(end_state_identity) = 'object'),
    canonical_payload bytea NOT NULL,
    content_sha256 kb_sha256 NOT NULL UNIQUE,
    created_by kb_actor_identity NOT NULL,
    created_at timestamptz NOT NULL,
    UNIQUE (project_id, id),
    CHECK (format = 'docx' OR gate_status = 'pass'),
    CHECK (content_sha256 = encode(public.digest(canonical_payload, 'sha256'), 'hex'))
);
CREATE TABLE bid_submission_gate_issues (
    manifest_id uuid NOT NULL REFERENCES bid_submission_manifests(id) ON DELETE RESTRICT,
    ordinal integer NOT NULL CHECK (ordinal >= 0),
    code text NOT NULL CHECK (code IN (
        'PROFILE_FIELD_MISSING', 'SIGNATURE_OR_SEAL_NOT_CONFIRMED',
        'MATCHING_REPORT_MISSING', 'MATCHING_PICK_MISSING',
        'PROCEDURAL_CLASSIFICATION_MISSING', 'PROCEDURAL_CLASSIFICATION_REVIEW',
        'PROCEDURAL_DECISION_MISSING', 'PROCEDURAL_NOT_APPLICABLE',
        'ATTACHMENT_NOT_VALID', 'PART_MISSING',
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
    template_slot text NOT NULL,
    template_version text NOT NULL,
    content_artifact_id uuid REFERENCES bid_part_content_artifacts(id) ON DELETE RESTRICT,
    dependency_artifact_id uuid REFERENCES bid_part_dependency_artifacts(id) ON DELETE RESTRICT,
    placeholder_markdown_utf8 bytea,
    placeholder_sha256 kb_sha256,
    PRIMARY KEY (manifest_id, ordinal),
    UNIQUE (manifest_id, part_key),
    FOREIGN KEY (template_slot, template_version)
        REFERENCES bid_template_contract_artifacts(slot, version) ON DELETE RESTRICT,
    CHECK (
      (content_artifact_id IS NOT NULL AND dependency_artifact_id IS NOT NULL
       AND placeholder_markdown_utf8 IS NULL AND placeholder_sha256 IS NULL)
      OR
      (content_artifact_id IS NULL AND dependency_artifact_id IS NULL
       AND placeholder_markdown_utf8 IS NOT NULL
       AND placeholder_sha256 = encode(public.digest(placeholder_markdown_utf8, 'sha256'), 'hex'))
    )
);
CREATE TABLE bid_manifest_render_assets (
    id uuid PRIMARY KEY,
    manifest_id uuid NOT NULL REFERENCES bid_submission_manifests(id) ON DELETE RESTRICT,
    source_kind text NOT NULL CHECK (source_kind IN (
        'bid_shot', 'markdown_object', 'procedural_attachment', 'procedural_attachment_page'
    )),
    source_locator jsonb NOT NULL CHECK (jsonb_typeof(source_locator) = 'object'),
    object_ref kb_object_ref NOT NULL,
    digest kb_sha256 NOT NULL,
    media_type text NOT NULL,
    byte_length bigint NOT NULL CHECK (byte_length >= 0),
    pixel_width integer,
    pixel_height integer,
    manifest_ordinal integer NOT NULL CHECK (manifest_ordinal >= 0),
    occurrence_ordinal integer NOT NULL CHECK (occurrence_ordinal >= 0),
    UNIQUE (manifest_id, manifest_ordinal),
    UNIQUE (manifest_id, source_kind, source_locator),
    CHECK ((media_type='application/pdf' AND source_kind='procedural_attachment'
            AND pixel_width IS NULL AND pixel_height IS NULL)
        OR (media_type IN ('image/png','image/jpeg','image/webp')
            AND pixel_width BETWEEN 1 AND 20000 AND pixel_height BETWEEN 1 AND 20000))
);
CREATE TABLE bid_submission_output_artifacts (
    id uuid PRIMARY KEY,
    manifest_id uuid NOT NULL,
    project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
    format text NOT NULL CHECK (format IN ('docx', 'pdf')),
    object_ref kb_object_ref NOT NULL,
    content_sha256 kb_sha256 NOT NULL,
    byte_length bigint NOT NULL CHECK (byte_length >= 0),
    rendered_at timestamptz NOT NULL,
    UNIQUE (manifest_id, format),
    UNIQUE (project_id, id),
    FOREIGN KEY (project_id, manifest_id)
        REFERENCES bid_submission_manifests(project_id, id) ON DELETE RESTRICT
);
CREATE TABLE bid_current_submission_outputs (
    project_id uuid NOT NULL,
    format text NOT NULL CHECK (format IN ('docx', 'pdf')),
    output_artifact_id uuid NOT NULL,
    PRIMARY KEY (project_id, format),
    FOREIGN KEY (project_id, output_artifact_id)
        REFERENCES bid_submission_output_artifacts(project_id, id) ON DELETE RESTRICT
);
CREATE TABLE bid_submission_render_jobs (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
    manifest_id uuid NOT NULL UNIQUE,
    expected_manifest_sha256 kb_sha256 NOT NULL,
    requested_by kb_actor_identity NOT NULL,
    idempotency_key text NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 128),
    status text NOT NULL CHECK (status IN ('pending', 'running', 'completed', 'failed')),
    attempt_count integer NOT NULL DEFAULT 0 CHECK (attempt_count BETWEEN 0 AND 4),
    max_attempts integer NOT NULL DEFAULT 4 CHECK (max_attempts = 4),
    claim_token uuid,
    claim_lease_ms integer NOT NULL DEFAULT 1800000 CHECK (claim_lease_ms = 1800000),
    heartbeat_at timestamptz,
    output_artifact_id uuid,
    error_code text,
    error_detail text,
    created_at timestamptz NOT NULL,
    started_at timestamptz,
    finished_at timestamptz,
    UNIQUE (project_id, id),
    FOREIGN KEY (project_id, manifest_id)
        REFERENCES bid_submission_manifests(project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, output_artifact_id)
        REFERENCES bid_submission_output_artifacts(project_id, id) ON DELETE RESTRICT,
    CHECK (
      (status = 'pending' AND claim_token IS NULL AND heartbeat_at IS NULL
        AND output_artifact_id IS NULL AND finished_at IS NULL)
      OR
      (status = 'running' AND claim_token IS NOT NULL AND heartbeat_at IS NOT NULL
        AND output_artifact_id IS NULL AND finished_at IS NULL)
      OR
      (status = 'completed' AND claim_token IS NULL AND heartbeat_at IS NULL
        AND output_artifact_id IS NOT NULL AND error_code IS NULL AND finished_at IS NOT NULL)
      OR
      (status = 'failed' AND claim_token IS NULL AND heartbeat_at IS NULL
        AND output_artifact_id IS NULL AND error_code IS NOT NULL AND finished_at IS NOT NULL)
    )
);
CREATE INDEX bid_submission_render_jobs_pending_idx
    ON bid_submission_render_jobs(created_at, id) WHERE status = 'pending';
CREATE INDEX bid_submission_render_jobs_running_idx
    ON bid_submission_render_jobs(heartbeat_at, id) WHERE status = 'running';

CREATE FUNCTION kb_bid_guard_procedural_classification_transition()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, public
AS $$
BEGIN
  IF TG_OP='DELETE' THEN
    RAISE EXCEPTION 'PROCEDURAL_CLASSIFICATION_APPEND_ONLY' USING ERRCODE='55000';
  END IF;
  IF OLD.lifecycle_status<>'current' OR NEW.lifecycle_status<>'superseded'
     OR (NEW.id,NEW.project_id,NEW.segment_id,NEW.revision,NEW.router_result_status,
         NEW.router_requirement_kind,NEW.review_reason,NEW.effective_requirement_kind,
         NEW.override_from,NEW.override_to,NEW.override_actor,NEW.override_reason,NEW.override_at)
        IS DISTINCT FROM
        (OLD.id,OLD.project_id,OLD.segment_id,OLD.revision,OLD.router_result_status,
         OLD.router_requirement_kind,OLD.review_reason,OLD.effective_requirement_kind,
         OLD.override_from,OLD.override_to,OLD.override_actor,OLD.override_reason,OLD.override_at)
  THEN
    RAISE EXCEPTION 'PROCEDURAL_CLASSIFICATION_APPEND_ONLY' USING ERRCODE='55000';
  END IF;
  RETURN NEW;
END
$$;

CREATE FUNCTION kb_bid_guard_procedural_decision_transition()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, public
AS $$
BEGIN
  IF TG_OP='DELETE' THEN
    RAISE EXCEPTION 'PROCEDURAL_DECISION_APPEND_ONLY' USING ERRCODE='55000';
  END IF;
  IF OLD.lifecycle_status<>'current' OR NEW.lifecycle_status<>'superseded'
     OR (NEW.id,NEW.project_id,NEW.classification_id,NEW.revision,NEW.resolution,
         NEW.attachment_id,NEW.reason,NEW.actor_identity,NEW.decided_at)
        IS DISTINCT FROM
        (OLD.id,OLD.project_id,OLD.classification_id,OLD.revision,OLD.resolution,
         OLD.attachment_id,OLD.reason,OLD.actor_identity,OLD.decided_at)
  THEN
    RAISE EXCEPTION 'PROCEDURAL_DECISION_APPEND_ONLY' USING ERRCODE='55000';
  END IF;
  RETURN NEW;
END
$$;

CREATE FUNCTION kb_bid_verify_procedural_successor()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
BEGIN
  IF NEW.successor_id IS NULL THEN RETURN NULL; END IF;
  IF TG_TABLE_NAME='bid_procedural_classification_artifacts' THEN
    IF NOT EXISTS (
      SELECT 1 FROM bid_procedural_classification_artifacts successor
       WHERE successor.id=NEW.successor_id AND successor.project_id=NEW.project_id
         AND successor.segment_id=NEW.segment_id AND successor.revision=NEW.revision+1
    ) THEN RAISE EXCEPTION 'PROCEDURAL_CLASSIFICATION_SUCCESSOR_INVALID' USING ERRCODE='23514'; END IF;
  ELSIF NOT EXISTS (
    SELECT 1 FROM bid_procedural_decision_artifacts successor
     WHERE successor.id=NEW.successor_id AND successor.project_id=NEW.project_id
       AND (
         (successor.classification_id=NEW.classification_id AND successor.revision=NEW.revision+1)
         OR (successor.revision=1 AND EXISTS (
           SELECT 1 FROM bid_procedural_classification_artifacts old_classification
            WHERE old_classification.id=NEW.classification_id
              AND old_classification.successor_id=successor.classification_id))
       )
  ) THEN RAISE EXCEPTION 'PROCEDURAL_DECISION_SUCCESSOR_INVALID' USING ERRCODE='23514'; END IF;
  RETURN NULL;
END
$$;

CREATE TRIGGER bid_company_profile_artifacts_immutable BEFORE UPDATE OR DELETE ON bid_company_profile_artifacts FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();
CREATE TRIGGER bid_submission_profile_artifacts_immutable BEFORE UPDATE OR DELETE ON bid_submission_profile_artifacts FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();
CREATE TRIGGER bid_procedural_segment_artifacts_immutable BEFORE UPDATE OR DELETE ON bid_procedural_segment_artifacts FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();
CREATE TRIGGER bid_procedural_classification_transition_guard BEFORE UPDATE OR DELETE ON bid_procedural_classification_artifacts FOR EACH ROW EXECUTE FUNCTION kb_bid_guard_procedural_classification_transition();
CREATE TRIGGER bid_procedural_decision_transition_guard BEFORE UPDATE OR DELETE ON bid_procedural_decision_artifacts FOR EACH ROW EXECUTE FUNCTION kb_bid_guard_procedural_decision_transition();
CREATE CONSTRAINT TRIGGER bid_procedural_classification_successor_verify AFTER UPDATE ON bid_procedural_classification_artifacts DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION kb_bid_verify_procedural_successor();
CREATE CONSTRAINT TRIGGER bid_procedural_decision_successor_verify AFTER UPDATE ON bid_procedural_decision_artifacts DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION kb_bid_verify_procedural_successor();
CREATE TRIGGER bid_attachment_render_pages_immutable BEFORE UPDATE OR DELETE ON bid_attachment_render_pages FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();
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

CREATE FUNCTION kb_bid_require_user_actor(p_actor kb_actor_identity)
RETURNS void
LANGUAGE plpgsql
SET search_path = pg_catalog, public
AS $$
BEGIN
    IF p_actor NOT LIKE 'user:%' THEN
        RAISE EXCEPTION 'USER_ACTOR_REQUIRED' USING ERRCODE='42501';
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
    IF p_request_sha256 <> encode(public.digest(p_request_bytes,'sha256'),'hex') THEN
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
      response_bytes=p_response_bytes,response_sha256=encode(public.digest(p_response_bytes,'sha256'),'hex'),
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

CREATE FUNCTION kb_bid_fact_canonical_bytes(p_project bid_projects)
RETURNS bytea
LANGUAGE sql
STABLE
SET search_path = pg_catalog, public
AS $$
  SELECT convert_to(
    '{"schema_version":1,"project_id":"'||p_project.id::text
    ||'","revision":'||p_project.fact_revision::text
    ||',"budget_amount":'||CASE WHEN p_project.budget_amount IS NULL THEN 'null' ELSE '"'||to_char(p_project.budget_amount,'FM99999999999999999990.00')||'"' END
    ||',"budget_currency":'||CASE WHEN p_project.budget_currency IS NULL THEN 'null' ELSE '"'||p_project.budget_currency||'"' END
    ||',"ceiling_price":'||CASE WHEN p_project.ceiling_price IS NULL THEN 'null' ELSE '"'||to_char(p_project.ceiling_price,'FM99999999999999999990.00')||'"' END
    ||',"ceiling_currency":'||CASE WHEN p_project.ceiling_currency IS NULL THEN 'null' ELSE '"'||p_project.ceiling_currency||'"' END
    ||',"expires_at":'||CASE WHEN p_project.expires_at IS NULL THEN 'null' ELSE '"'||kb_bid_utc_json_time(p_project.expires_at)||'"' END
    ||',"bid_open_at":'||CASE WHEN p_project.bid_open_at IS NULL THEN 'null' ELSE '"'||kb_bid_utc_json_time(p_project.bid_open_at)||'"' END
    ||',"bid_valid_until":'||CASE WHEN p_project.bid_valid_until IS NULL THEN 'null' ELSE '"'||kb_bid_utc_json_time(p_project.bid_valid_until)||'"' END
    ||',"bid_valid_days":'||CASE WHEN p_project.bid_valid_days IS NULL THEN 'null' ELSE p_project.bid_valid_days::text END
    ||'}','UTF8')
$$;

CREATE FUNCTION kb_bid_fact_payload(p_project bid_projects)
RETURNS jsonb
LANGUAGE sql
STABLE
SET search_path = pg_catalog, public
AS $$ SELECT convert_from(kb_bid_fact_canonical_bytes(p_project),'UTF8')::jsonb $$;

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
AS $$ SELECT encode(public.digest(convert_to(concat_ws(E'\x1f','ClauseV1',p_id,p_status,p_kind,
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
    result_value := encode(public.digest(convert_to(payload,'UTF8'),'sha256'),'hex');
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
      DELETE FROM bid_current_route_pick_sets WHERE project_id=p_project_id;
      PERFORM kb_bid_rebuild_project_pick_set(p_project_id,'system:matching-invalidation');
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
      IF set_kind='procedural' THEN
        PERFORM kb_bid_sync_project_procedural(p_project_id,'system:clause-lifecycle');
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
    SELECT encode(public.digest(kb_bid_fact_canonical_bytes(p),'sha256'),'hex'),
           encode(public.digest(convert_to(kb_bid_ceiling_payload(p)::text,'UTF8'),'sha256'),'hex')
      INTO fact_hash,ceiling_hash FROM bid_projects p WHERE id=p_id;
    UPDATE bid_projects SET fact_sha256=fact_hash,ceiling_identity_sha256=ceiling_hash WHERE id=p_id;
    FOREACH set_kind IN ARRAY ARRAY['service','pricing','schedule_payment','schedule_delivery','evaluation','procedural'] LOOP
      INSERT INTO bid_clause_set_identities(project_id,set_kind,revision,content_sha256,updated_at)
      VALUES(p_id,set_kind,0,encode(public.digest(convert_to('ClauseSetV1:'||set_kind||':','UTF8'),'sha256'),'hex'),clock_timestamp());
    END LOOP;
    INSERT INTO bid_current_profiles(project_id) VALUES(p_id);
    INSERT INTO bid_procedural_segment_sets(project_id,revision,content_sha256,updated_at)
    VALUES(p_id,0,encode(public.digest(convert_to('ProceduralSegmentSetV1:','UTF8'),'sha256'),'hex'),clock_timestamp());
    response:=jsonb_build_object('id',p_id,'fact_revision',0,'fact_sha256',fact_hash,
      'ceiling_revision',0,'ceiling_identity_sha256',ceiling_hash);
    INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
      entity_kind,entity_locator,after_revision,after_sha256)
    VALUES(gen_random_uuid(),1,'bid.project.create',p_actor,p_idempotency_key,p_request_sha256,
      encode(public.digest(convert_to(response::text,'UTF8'),'sha256'),'hex'),'bid_project',jsonb_build_object('project_id',p_id),0,fact_hash);
    PERFORM kb_bid_idempotency_complete(p_actor,'bid.project.create',p_idempotency_key,201,convert_to(response::text,'UTF8'));
    RETURN response;
END
$$;

CREATE FUNCTION kb_bid_upload_document(
 p_staging_id uuid,p_id uuid,p_project_id uuid,p_file_name text,p_media_type text,p_byte_length bigint,
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
    IF replay IS NOT NULL THEN
      PERFORM kb_object_upload_abandon(p_staging_id,p_actor);
      RETURN convert_from(replay,'UTF8')::jsonb;
    END IF;
    SELECT * INTO STRICT project_value FROM bid_projects WHERE id=p_project_id FOR UPDATE;
    IF project_value.status<>'open' THEN RAISE EXCEPTION 'PROJECT_ENDED' USING ERRCODE='55000'; END IF;
    PERFORM kb_object_upload_commit(p_staging_id,p_object_ref,p_original_sha256,p_media_type,
      p_byte_length,'bid_document',p_id,'original',p_actor);
    INSERT INTO bid_documents(id,project_id,file_name,media_type,byte_length,original_object_ref,
      original_sha256,parse_status) VALUES(p_id,p_project_id,p_file_name,p_media_type,p_byte_length,
      p_object_ref,p_original_sha256,'pending');
    response:=jsonb_build_object('id',p_id,'project_id',p_project_id,'conversion_generation',1,'parse_status','pending');
    INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
      entity_kind,entity_locator,after_revision,after_sha256)
    VALUES(gen_random_uuid(),1,'bid.document.upload',p_actor,p_idempotency_key,p_request_sha256,
      encode(public.digest(convert_to(response::text,'UTF8'),'sha256'),'hex'),'bid_document',jsonb_build_object('document_id',p_id),
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
 p_converter_contract_version text,p_image_asset_set_sha256 kb_sha256,
 p_image_assets jsonb,p_extraction_target_id uuid,p_expected_section_count integer,
 p_policy_version text,p_prompt_version text,p_actor kb_actor_identity
)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE project_key uuid; document_value bid_documents%ROWTYPE; attempt_value bid_document_conversion_attempts%ROWTYPE;
 markdown_hash kb_sha256; computed_image_asset_set_sha256 kb_sha256; image_asset jsonb;
 router_value kind_router_current%ROWTYPE; extraction_generation_value integer; response jsonb;
BEGIN
    IF p_extraction_target_id IS NULL OR p_expected_section_count<=0
       OR octet_length(p_policy_version) NOT BETWEEN 1 AND 128
       OR octet_length(p_prompt_version) NOT BETWEEN 1 AND 128 THEN
      RAISE EXCEPTION 'EXTRACTION_TARGET_INVALID' USING ERRCODE='22023';
    END IF;
    IF jsonb_typeof(p_image_assets)<>'array' OR jsonb_array_length(p_image_assets)>1024 THEN
      RAISE EXCEPTION 'CONVERTED_IMAGE_ASSETS_INVALID' USING ERRCODE='22023';
    END IF;
    IF EXISTS (
      SELECT 1 FROM jsonb_array_elements(p_image_assets) asset
       WHERE jsonb_typeof(asset)<>'object'
          OR COALESCE(asset->>'media_type','') NOT LIKE 'image/%'
          OR COALESCE(asset->>'occurrence','') !~ '^image:[0-9]+$'
          OR COALESCE(asset->>'byte_length','') !~ '^[0-9]+$'
          OR COALESCE((asset->>'byte_length')::numeric,-1)<0
    ) THEN RAISE EXCEPTION 'CONVERTED_IMAGE_ASSETS_INVALID' USING ERRCODE='22023'; END IF;
    IF EXISTS (
      SELECT asset->>'occurrence' FROM jsonb_array_elements(p_image_assets) asset
       GROUP BY asset->>'occurrence' HAVING count(*)>1
    ) THEN RAISE EXCEPTION 'CONVERTED_IMAGE_ASSET_OCCURRENCE_DUPLICATE' USING ERRCODE='22023'; END IF;
    SELECT encode(public.digest(convert_to(
             'ConvertedSourceArtifactV1:image-set:' ||
             COALESCE(string_agg(asset->>'digest','' ORDER BY asset->>'digest'),'')
           ,'UTF8'),'sha256'),'hex')
      INTO computed_image_asset_set_sha256
      FROM jsonb_array_elements(p_image_assets) asset;
    IF computed_image_asset_set_sha256<>p_image_asset_set_sha256 THEN
      RAISE EXCEPTION 'CONVERTED_IMAGE_ASSET_SET_MISMATCH' USING ERRCODE='23514';
    END IF;
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
    markdown_hash:=encode(public.digest(p_markdown,'sha256'),'hex');
    INSERT INTO bid_converted_source_artifacts(id,project_id,document_id,conversion_generation,
      original_object_ref,original_sha256,canonical_markdown_utf8,markdown_sha256,byte_length,
      converter_contract_version,image_asset_set_sha256)
    VALUES(p_source_artifact_id,project_key,p_document_id,document_value.conversion_generation,
      document_value.original_object_ref,document_value.original_sha256,p_markdown,markdown_hash,
      octet_length(p_markdown),p_converter_contract_version,p_image_asset_set_sha256);
    FOR image_asset IN
      SELECT value FROM jsonb_array_elements(p_image_assets)
       ORDER BY value->>'occurrence'
    LOOP
      PERFORM kb_object_upload_commit(
        (image_asset->>'staging_id')::uuid,
        (image_asset->>'object_ref')::kb_object_ref,
        (image_asset->>'digest')::kb_sha256,
        image_asset->>'media_type',
        (image_asset->>'byte_length')::bigint,
        'bid_converted_source_image',p_source_artifact_id,image_asset->>'occurrence',p_actor
      );
    END LOOP;
    UPDATE bid_documents SET current_converted_source_artifact_id=p_source_artifact_id,
      parse_status='completed',parsed_at=clock_timestamp(),error_code=NULL WHERE id=p_document_id;
    UPDATE bid_document_conversion_attempts SET status='completed'
      WHERE document_id=p_document_id AND claim_token=p_claim_token;
    SELECT * INTO STRICT router_value FROM kind_router_current WHERE singleton_key FOR SHARE;
    SELECT COALESCE(max(extraction_generation),0)+1 INTO extraction_generation_value
      FROM bid_extraction_targets WHERE project_id=project_key AND document_id=p_document_id;
    INSERT INTO bid_extraction_targets(id,project_id,document_id,source_artifact_id,conversion_generation,
      extraction_generation,router_contract_version,policy_version,prompt_version,output_schema_version,
      expected_section_count,state)
    VALUES(p_extraction_target_id,project_key,p_document_id,p_source_artifact_id,
      document_value.conversion_generation,extraction_generation_value,router_value.version,
      p_policy_version,p_prompt_version,1,p_expected_section_count,'pending');
    response:=jsonb_build_object('source_artifact_id',p_source_artifact_id,
      'extraction_target_id',p_extraction_target_id,'extraction_generation',extraction_generation_value,
      'router_contract_version',router_value.version);
    RETURN response;
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
      WHERE document_id=p_document_id AND claim_token=p_claim_token AND status='running'
        AND heartbeat_at+make_interval(secs=>claim_lease_ms/1000.0)>clock_timestamp();
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
      encode(public.digest(convert_to(response::text,'UTF8'),'sha256'),'hex'),'bid_extraction_target',
      jsonb_build_object('target_id',p_target_id),generation_value,
      encode(public.digest(convert_to(response::text,'UTF8'),'sha256'),'hex'));
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
 span_id uuid; span_value jsonb; span_bytes bytea; span_hash kb_sha256;
 candidate_id uuid; clause_value jsonb; clause_id uuid;
 routed_kind text; routed_reason text;
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
 section_hash:=encode(public.digest(substring(source_value.canonical_markdown_utf8
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
 graph_hash:=encode(public.digest(convert_to(p_candidate_graph::text,'UTF8'),'sha256'),'hex');
 publication_revision:=COALESCE(current_publication.revision,0)+1;
 INSERT INTO bid_section_publications(id,project_id,target_id,section_artifact_id,publication_revision,
   content_sha256,published_by,published_at) VALUES(publication_id,target_value.project_id,p_target_id,
   section_id,publication_revision,graph_hash,p_actor,clock_timestamp());
 IF current_publication.publication_id IS NOT NULL THEN
   SELECT * INTO STRICT old_publication FROM bid_section_publications
    WHERE id=current_publication.publication_id FOR SHARE;
   PERFORM 1 FROM bid_clauses clause_value
    WHERE clause_value.publication_id=old_publication.id AND clause_value.provenance='extracted'
      AND clause_value.status='draft' ORDER BY clause_value.id FOR UPDATE;
   UPDATE bid_clauses clause_value
    SET status='superseded',revision=clause_value.revision+1,updated_at=clock_timestamp()
    WHERE clause_value.publication_id=old_publication.id
      AND clause_value.provenance='extracted' AND clause_value.status='draft';
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
   prior_end:=end_value; quote_hash:=encode(public.digest(convert_to(quote_value,'UTF8'),'sha256'),'hex');
   span_value:=jsonb_build_object('schema_version',2,'source_artifact_id',target_value.source_artifact_id,
     'section_artifact_id',section_id,'project_id',target_value.project_id,'document_id',target_value.document_id,
     'conversion_generation',target_value.conversion_generation,'section_key',p_section_key,
     'parent_start_offset',p_parent_start_offset,'parent_end_offset',p_parent_end_offset,
     'start_offset',start_value,'end_offset',end_value,'offset_unit','utf8_byte','quote',quote_value,
     'quote_sha256',quote_hash,'heading_path',p_heading_path);
   span_bytes:=convert_to(span_value::text,'UTF8');
   span_hash:=encode(public.digest(span_bytes,'sha256'),'hex');
   SELECT artifact.id INTO span_id
     FROM bid_source_span_artifacts artifact
    WHERE artifact.project_id=target_value.project_id
      AND artifact.document_id=target_value.document_id
      AND artifact.source_artifact_id=target_value.source_artifact_id
      AND artifact.section_artifact_id=section_id
      AND artifact.start_offset=start_value AND artifact.end_offset=end_value
      AND artifact.quote_sha256=quote_hash
    FOR SHARE;
   IF span_id IS NULL THEN
     span_id:=gen_random_uuid();
     INSERT INTO bid_source_span_artifacts(id,schema_version,project_id,document_id,source_artifact_id,
       section_artifact_id,conversion_generation,section_key,parent_start_offset,parent_end_offset,
       start_offset,end_offset,offset_unit,quote,quote_sha256,heading_path,source_span_v2,canonical_payload,content_sha256)
     VALUES(span_id,2,target_value.project_id,target_value.document_id,target_value.source_artifact_id,section_id,
       target_value.conversion_generation,p_section_key,p_parent_start_offset,p_parent_end_offset,start_value,
       end_value,'utf8_byte',quote_value,quote_hash,p_heading_path,span_value,span_bytes,span_hash);
   ELSE
     PERFORM 1 FROM bid_source_span_artifacts artifact
      WHERE artifact.id=span_id AND artifact.schema_version=2
        AND artifact.conversion_generation=target_value.conversion_generation
        AND artifact.section_key=p_section_key
        AND artifact.parent_start_offset=p_parent_start_offset
        AND artifact.parent_end_offset=p_parent_end_offset
        AND artifact.offset_unit='utf8_byte' AND artifact.quote=quote_value
        AND artifact.heading_path=p_heading_path AND artifact.source_span_v2=span_value
        AND artifact.canonical_payload=span_bytes AND artifact.content_sha256=span_hash;
     IF NOT FOUND THEN
       RAISE EXCEPTION 'SOURCE_SPAN_ARTIFACT_IDENTITY_MISMATCH' USING ERRCODE='23505';
     END IF;
   END IF;
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
        <>ARRAY['must','text']
        AND (SELECT array_agg(k ORDER BY k) FROM jsonb_object_keys(clause_value) k)
            <>ARRAY['kind','must','router_reason_code','text'] THEN
       RAISE EXCEPTION 'CLAUSE_CANDIDATE_SCHEMA_INVALID' USING ERRCODE='22023';
     END IF;
     SELECT routed.kind, routed.reason_code INTO STRICT routed_kind, routed_reason
       FROM kb_bid_route_kind_full(clause_value->>'text', target_value.router_contract_version) routed;
     clause_id:=gen_random_uuid();
     INSERT INTO bid_extract_clause_candidates(id,target_id,segment_candidate_id,proposal_text,must,
       proposed_kind,router_reason_code) VALUES(clause_id,p_target_id,candidate_id,clause_value->>'text',
       (clause_value->>'must')::boolean,routed_kind,routed_reason);
     INSERT INTO bid_clauses(id,project_id,publication_id,origin_candidate_id,provenance,status,kind,text,must,
       current_source_span_artifact_id,extracted_origin_source_span_artifact_id,revision,created_by)
     VALUES(clause_id,target_value.project_id,publication_id,clause_id,'extracted','draft',routed_kind,
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
   encode(public.digest(convert_to(response::text,'UTF8'),'sha256'),'hex'),'bid_section_publication',
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
     new_fact_hash:=encode(public.digest(kb_bid_fact_canonical_bytes(project_value),'sha256'),'hex');
     new_ceiling_hash:=encode(public.digest(convert_to(kb_bid_ceiling_payload(project_value)::text,'UTF8'),'sha256'),'hex');
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
   encode(public.digest(convert_to(response::text,'UTF8'),'sha256'),'hex'),'bid_project_fact',
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
  encode(public.digest(convert_to(response::text,'UTF8'),'sha256'),'hex'),'bid_clause',jsonb_build_object('clause_id',p_clause_id),1,semantic_hash);
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
   IF old_status='confirmed' AND new_kind IS DISTINCT FROM old_kind THEN
     RAISE EXCEPTION 'CLAUSE_KIND_CHANGE_REQUIRES_UNCONFIRM' USING ERRCODE='55000';
   END IF;
   IF clause_value.provenance='extracted' THEN
     new_provenance:='manual_after_edit';new_current_span:=NULL;
   END IF;
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
  encode(public.digest(convert_to(response::text,'UTF8'),'sha256'),'hex'),'bid_clause',jsonb_build_object('clause_id',p_clause_id),
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
 IF p_content_sha256<>encode(public.digest(p_canonical_payload,'sha256'),'hex') THEN
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
   encode(public.digest(convert_to(response::text,'UTF8'),'sha256'),'hex'),'kind_router_contract',
   jsonb_build_object('version',p_version),1,p_content_sha256);
 PERFORM kb_bid_idempotency_complete(p_actor,'bid.kind_router.register',p_idempotency_key,201,convert_to(response::text,'UTF8'));
 RETURN response;
END
$$;

CREATE FUNCTION kb_bid_route_kind_full(p_text text,p_contract_version text)
RETURNS TABLE(kind text, reason_code text)
LANGUAGE plpgsql
STABLE
SET search_path = pg_catalog, public
AS $$
DECLARE payload jsonb; override_kind text;text_hash text;
 pricing boolean; evaluation boolean;
BEGIN
 SELECT convert_from(canonical_payload,'UTF8')::jsonb INTO STRICT payload
  FROM kind_router_contract_artifacts WHERE version=p_contract_version;
 text_hash:=encode(public.digest(convert_to(p_text,'UTF8'),'sha256'),'hex');
 override_kind:=payload->'overrides'->>text_hash;
 IF override_kind IS NOT NULL THEN
   IF override_kind NOT IN ('technical','qualification','service','pricing','schedule_delivery',
      'schedule_payment','evaluation','procedural') THEN
     RAISE EXCEPTION 'KIND_ROUTER_OVERRIDE_INVALID' USING ERRCODE='22023'; END IF;
   kind := override_kind; reason_code := 'CONTRACT_OVERRIDE'; RETURN NEXT; RETURN;
 END IF;
 IF p_text LIKE '%支付接口%' OR p_text LIKE '%支付网关%' OR p_text LIKE '%付款接口%'
    OR p_text LIKE '%支付API%' OR p_text LIKE '%支付密码%'
    OR ((p_text LIKE '%设备%' OR p_text LIKE '%系统%' OR p_text LIKE '%接口%' OR p_text LIKE '%协议%')
        AND (p_text LIKE '%性能%' OR p_text LIKE '%能力%' OR p_text LIKE '%参数%' OR p_text LIKE '%响应时间%')) THEN
   kind := 'technical'; reason_code := 'TECHNICAL_SUBJECT_PREDICATE'; RETURN NEXT; RETURN;
 END IF;
 IF p_text LIKE '%许可证%' OR p_text LIKE '%ISO%' OR p_text LIKE '%等保%' OR p_text LIKE '%资质%'
    OR p_text LIKE '%软著%' OR p_text LIKE '%业绩%' OR p_text LIKE '%合同复印件%'
    OR p_text LIKE '%合同佐证%' OR p_text LIKE '%证书%' THEN
   kind := 'qualification'; reason_code := 'QUALIFICATION_EVIDENCE'; RETURN NEXT; RETURN;
 END IF;
 IF p_text LIKE '%保证金%' OR p_text LIKE '%密封%' OR p_text LIKE '%投标函%' OR p_text LIKE '%授权委托%'
    OR p_text LIKE '%法定代表人%' OR p_text LIKE '%签章样式%' OR p_text LIKE '%递交%' THEN
   kind := 'procedural'; reason_code := 'PROCEDURAL_MATERIAL_OR_ACTION'; RETURN NEXT; RETURN;
 END IF;
 IF (p_text LIKE '%付款%' OR p_text LIKE '%结算%' OR p_text LIKE '%支付%')
    AND (p_text LIKE '%比例%' OR p_text LIKE '%金额%' OR p_text LIKE '%节点%'
         OR p_text LIKE '%账期%' OR p_text LIKE '%验收%' OR p_text LIKE '%主体%') THEN
   kind := 'schedule_payment'; reason_code := 'PAYMENT_ACTION_AND_TERM'; RETURN NEXT; RETURN;
 END IF;
 IF p_text LIKE '%到货%' OR p_text LIKE '%交货%' OR p_text LIKE '%供货%' OR p_text LIKE '%工期%'
    OR p_text LIKE '%实施周期%' OR p_text LIKE '%交付地点%' OR p_text LIKE '%供货地点%' THEN
   kind := 'schedule_delivery'; reason_code := 'DELIVERY_TERM'; RETURN NEXT; RETURN;
 END IF;
 pricing := p_text LIKE '%分项报价%' OR p_text LIKE '%计价口径%' OR p_text LIKE '%单列价格%' OR p_text LIKE '%报价明细%';
 evaluation := p_text LIKE '%评分项%' OR p_text LIKE '%权重%' OR p_text LIKE '%得分%' OR p_text LIKE '%评分标准%';
 IF pricing THEN
   kind := 'pricing';
   reason_code := CASE WHEN evaluation THEN 'PRICING_EVALUATION_CONFLICT' ELSE 'PRICING_STRUCTURE' END;
   RETURN NEXT; RETURN;
 END IF;
 IF evaluation THEN
   kind := 'evaluation'; reason_code := 'EVALUATION_SCORE'; RETURN NEXT; RETURN;
 END IF;
 IF p_text LIKE '%质保%' OR p_text LIKE '%驻场%' OR p_text LIKE '%培训%' OR p_text LIKE '%应急%'
    OR p_text LIKE '%7x24%' OR p_text LIKE '%SLA%' THEN
   kind := 'service'; reason_code := 'SERVICE_OBLIGATION'; RETURN NEXT; RETURN;
 END IF;
 kind := 'technical'; reason_code := 'BOUNDED_TECHNICAL_FALLBACK'; RETURN NEXT;
END
$$;

CREATE FUNCTION kb_bid_route_kind(p_text text,p_contract_version text)
RETURNS text
LANGUAGE sql
STABLE
SET search_path = pg_catalog, public
AS $$ SELECT kind FROM kb_bid_route_kind_full(p_text,p_contract_version) $$;

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
       encode(public.digest(convert_to(clause_response::text,'UTF8'),'sha256'),'hex'),'bid_clause',
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
  encode(public.digest(convert_to(response::text,'UTF8'),'sha256'),'hex'),'kind_router_current',
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
  WHERE target_id=p_target_id AND attempt=p_attempt AND claim_token=p_claim_token AND status='running'
    AND heartbeat_at+make_interval(secs=>claim_lease_ms/1000.0)>clock_timestamp();
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
  encode(public.digest(convert_to(response::text,'UTF8'),'sha256'),'hex'),'bid_document',jsonb_build_object('document_id',p_document_id),
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
  encode(public.digest(convert_to(response::text,'UTF8'),'sha256'),'hex'),'bid_project',jsonb_build_object('project_id',p_project_id),
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
CREATE VIEW bidding_procedural_router_current AS
SELECT current_value.version,current_value.promotion_generation,artifact.content_sha256,artifact.canonical_payload
FROM procedural_router_current current_value
JOIN procedural_router_contract_artifacts artifact ON artifact.version=current_value.version;
CREATE VIEW bidding_template_contract_current AS
SELECT current_value.slot,current_value.version,current_value.promotion_generation,
       artifact.content_sha256,artifact.canonical_payload
FROM bid_template_contract_current current_value
JOIN bid_template_contract_artifacts artifact
  ON artifact.slot=current_value.slot AND artifact.version=current_value.version;


-- Quote/Submission checked mutations, pointer integrity, and typed projections.
-- Appended to the create-only bidding_v1 baseline; not a compatibility upgrade.

ALTER TABLE bid_quote_snapshots
  ADD COLUMN title text NOT NULL CHECK (octet_length(btrim(title)) BETWEEN 1 AND 256),
  ADD COLUMN notes text CHECK (octet_length(notes) <= 4096),
  ADD COLUMN no_ceiling_review jsonb;
ALTER TABLE bid_quote_revisions
  ADD CONSTRAINT bid_quote_revisions_based_on_snapshot_fk
  FOREIGN KEY (based_on_snapshot_id) REFERENCES bid_quote_snapshots(id) ON DELETE RESTRICT;
ALTER TABLE bid_quote_current
  ADD CONSTRAINT bid_quote_current_draft_fk
  FOREIGN KEY (current_draft_revision_id) REFERENCES bid_quote_revisions(id) ON DELETE RESTRICT,
  ADD CONSTRAINT bid_quote_current_snapshot_fk
  FOREIGN KEY (active_finalized_snapshot_id) REFERENCES bid_quote_snapshots(id) ON DELETE RESTRICT;
ALTER TABLE bid_current_profiles
  ADD CONSTRAINT bid_current_profiles_company_fk
  FOREIGN KEY (project_id, company_profile_id)
  REFERENCES bid_company_profile_artifacts(project_id, id) ON DELETE RESTRICT,
  ADD CONSTRAINT bid_current_profiles_submission_fk
  FOREIGN KEY (project_id, submission_profile_id)
  REFERENCES bid_submission_profile_artifacts(project_id, id) ON DELETE RESTRICT;
ALTER TABLE bid_procedural_decision_artifacts
  ADD CONSTRAINT bid_procedural_decision_successor_fk
    FOREIGN KEY (successor_id) REFERENCES bid_procedural_decision_artifacts(id)
    DEFERRABLE INITIALLY DEFERRED,
  ADD CONSTRAINT bid_procedural_decision_terminal_reason_check CHECK (terminal_reason IN (
    'clause_deleted','clause_unconfirmed','left_procedural','text_changed','resegmented','segment_removed',
    'router_promoted'
  )),
  ADD CONSTRAINT bid_procedural_decision_successor_xor CHECK (
    (lifecycle_status='current' AND successor_id IS NULL AND terminal_reason IS NULL
      AND terminal_at IS NULL AND terminal_actor IS NULL)
    OR (lifecycle_status='superseded' AND (
      (successor_id IS NOT NULL AND terminal_reason IS NULL AND terminal_at IS NULL AND terminal_actor IS NULL)
      OR (successor_id IS NULL AND terminal_reason IS NOT NULL AND terminal_at IS NOT NULL AND terminal_actor IS NOT NULL))));
ALTER TABLE bid_procedural_classification_artifacts
  ADD CONSTRAINT bid_procedural_classification_successor_fk
    FOREIGN KEY (successor_id) REFERENCES bid_procedural_classification_artifacts(id)
    DEFERRABLE INITIALLY DEFERRED;
CREATE UNIQUE INDEX bid_procedural_classification_current_uq
  ON bid_procedural_classification_artifacts(segment_id) WHERE lifecycle_status='current';
CREATE UNIQUE INDEX bid_procedural_decision_current_uq
  ON bid_procedural_decision_artifacts(classification_id) WHERE lifecycle_status='current';

CREATE TABLE bid_shot_set_artifacts (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
    revision bigint NOT NULL CHECK (revision > 0),
    canonical_payload bytea NOT NULL,
    content_sha256 kb_sha256 NOT NULL UNIQUE,
    created_by kb_actor_identity NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (project_id, revision),
    UNIQUE (project_id, id),
    CHECK (content_sha256 = encode(public.digest(canonical_payload, 'sha256'), 'hex'))
);
CREATE TABLE bid_current_shot_sets (
    project_id uuid PRIMARY KEY REFERENCES bid_projects(id) ON DELETE RESTRICT,
    shot_set_id uuid NOT NULL,
    revision bigint NOT NULL,
    FOREIGN KEY (project_id, shot_set_id)
      REFERENCES bid_shot_set_artifacts(project_id, id) ON DELETE RESTRICT
);
CREATE TABLE bid_procedural_segment_sets (
    project_id uuid PRIMARY KEY REFERENCES bid_projects(id) ON DELETE RESTRICT,
    revision bigint NOT NULL CHECK (revision >= 0),
    content_sha256 kb_sha256 NOT NULL,
    updated_at timestamptz NOT NULL
);
CREATE TRIGGER bid_shot_set_artifacts_immutable
BEFORE UPDATE OR DELETE ON bid_shot_set_artifacts
FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();

CREATE VIEW bidding_current_routes AS
SELECT route.id AS route_id, route.project_id, route.route_kind, route.unit_id, route.ordinal,
       route.empty_policy, route.route_scope_sha256, manifest.id AS manifest_id,
       manifest.generation, manifest.mutation_watermark
FROM bid_matching_manifests manifest
JOIN bid_matching_routes route ON route.manifest_id=manifest.id
JOIN bid_projects project ON project.id=manifest.project_id
WHERE project.status='open'
  AND manifest.mutation_watermark=project.matching_mutation_watermark
  AND manifest.generation=(SELECT max(generation) FROM bid_matching_manifests m2 WHERE m2.project_id=manifest.project_id);
CREATE VIEW bidding_quote_drafts AS
SELECT revision.* FROM bid_quote_revisions revision
JOIN bid_quote_current current_value ON current_value.current_draft_revision_id=revision.id;
CREATE VIEW bidding_quote_lines AS
SELECT line.*, revision.project_id, revision.quote_id, revision.status AS revision_status
FROM bid_quote_lines line
JOIN bid_quote_revisions revision ON revision.id=line.quote_revision_id;
CREATE VIEW bidding_current_company_profiles AS
SELECT artifact.* FROM bid_company_profile_artifacts artifact
JOIN bid_current_profiles current_value ON current_value.company_profile_id=artifact.id;
CREATE VIEW bidding_current_submission_profiles AS
SELECT artifact.* FROM bid_submission_profile_artifacts artifact
JOIN bid_current_profiles current_value ON current_value.submission_profile_id=artifact.id;
CREATE VIEW bidding_current_procedural_classifications AS
SELECT classification.*,convert_from(segment.segment_utf8,'UTF8') AS segment_text
FROM bid_procedural_classification_artifacts classification
JOIN bid_procedural_segment_artifacts segment ON segment.id=classification.segment_id
WHERE classification.lifecycle_status='current';
CREATE VIEW bidding_current_procedural_decisions AS
SELECT decision.* FROM bid_procedural_decision_artifacts decision
WHERE decision.lifecycle_status='current';
CREATE VIEW bidding_current_attachments AS
SELECT * FROM bid_procedural_attachments WHERE status IN ('draft','confirmed','rejected');
CREATE VIEW bidding_current_shot_sets AS
SELECT artifact.* FROM bid_shot_set_artifacts artifact
JOIN bid_current_shot_sets current_value ON current_value.shot_set_id=artifact.id;
CREATE VIEW bidding_current_submission_outputs AS
SELECT output.* FROM bid_submission_output_artifacts output
JOIN bid_current_submission_outputs current_value ON current_value.output_artifact_id=output.id;

CREATE FUNCTION kb_bid_json_string(p_value text)
RETURNS text
LANGUAGE plpgsql
IMMUTABLE
PARALLEL SAFE
SET search_path = pg_catalog
AS $$
DECLARE result text := '"'; i int := 0; ch text; code int;
BEGIN
  IF p_value IS NULL THEN RETURN 'null'; END IF;
  LOOP
    i := i + 1;
    ch := substr(p_value, i, 1);
    EXIT WHEN ch IS NULL OR ch = '';
    IF ch = '"' THEN result := result || E'\\"';
    ELSIF ch = E'\\' THEN result := result || E'\\\\';
    ELSIF ch = E'\b' THEN result := result || E'\\b';
    ELSIF ch = E'\f' THEN result := result || E'\\f';
    ELSIF ch = E'\n' THEN result := result || E'\\n';
    ELSIF ch = E'\r' THEN result := result || E'\\r';
    ELSIF ch = E'\t' THEN result := result || E'\\t';
    ELSE
      code := ascii(ch);
      IF code < 32 THEN
        result := result || E'\\u' || lpad(to_hex(code), 4, '0');
      ELSE
        result := result || ch;
      END IF;
    END IF;
  END LOOP;
  RETURN result || '"';
END
$$;

CREATE FUNCTION kb_bid_format_amount(p_value numeric)
RETURNS text
LANGUAGE sql
IMMUTABLE
STRICT
PARALLEL SAFE
SET search_path = pg_catalog
AS $$ SELECT to_char(p_value, 'FM99999999999999999990.00') $$;

CREATE FUNCTION kb_bid_format_qty(p_value numeric)
RETURNS text
LANGUAGE sql
IMMUTABLE
STRICT
PARALLEL SAFE
SET search_path = pg_catalog
AS $$ SELECT to_char(p_value, 'FM99999999999999999990.000000') $$;

CREATE FUNCTION kb_bid_stale_parts(p_project_id uuid, p_keys text[], p_reason text)
RETURNS void
LANGUAGE plpgsql
SET search_path = pg_catalog, public
AS $$
BEGIN
  UPDATE bid_current_parts SET stale=true,
    stale_reason_codes=(SELECT ARRAY(SELECT DISTINCT x FROM unnest(stale_reason_codes||ARRAY[p_reason]) x ORDER BY 1))
  WHERE project_id=p_project_id AND part_key = ANY(p_keys);
END
$$;

CREATE FUNCTION kb_bid_required_part_keys(p_project_id uuid)
RETURNS text[]
LANGUAGE plpgsql
STABLE
SET search_path = pg_catalog, public
AS $$
DECLARE keys text[] := ARRAY['1']; unit_id uuid; has_unsectioned boolean := false;
BEGIN
  FOR unit_id IN
    SELECT DISTINCT COALESCE(
      (clause.current_source_span_v2->>'section_artifact_id')::uuid,
      '00000000-0000-0000-0000-000000000000'::uuid)
      FROM bidding_current_clauses clause
     WHERE clause.project_id=p_project_id AND clause.status='confirmed' AND clause.kind='technical'
     ORDER BY 1
  LOOP
    IF unit_id = '00000000-0000-0000-0000-000000000000'::uuid THEN
      has_unsectioned := true;
    ELSE
      keys := keys || ARRAY['2:'||unit_id::text];
    END IF;
  END LOOP;
  IF has_unsectioned THEN keys := keys || ARRAY['2:unsectioned']; END IF;
  keys := keys || ARRAY['3','4','5','6:letter','6:authorization','6:quote','6:implementation_plan','6:procedural'];
  RETURN keys;
END
$$;

CREATE FUNCTION kb_bid_template_slot(p_part_key text)
RETURNS text
LANGUAGE plpgsql
IMMUTABLE
STRICT
PARALLEL SAFE
SET search_path = pg_catalog
AS $$
DECLARE raw_unit text; unit_id uuid;
BEGIN
  IF p_part_key = '2:unsectioned' THEN RETURN '2:unsectioned'; END IF;
  IF p_part_key IN ('1','3','4','5','6:letter','6:authorization','6:quote','6:implementation_plan','6:procedural') THEN
    RETURN p_part_key;
  END IF;
  IF p_part_key !~ '^2:[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$' THEN
    RETURN NULL;
  END IF;
  raw_unit := substr(p_part_key,3);
  BEGIN unit_id := raw_unit::uuid;
  EXCEPTION WHEN invalid_text_representation THEN RETURN NULL;
  END;
  IF unit_id = '00000000-0000-0000-0000-000000000000'::uuid OR unit_id::text <> raw_unit THEN
    RETURN NULL;
  END IF;
  RETURN '2:unit';
END
$$;

CREATE FUNCTION kb_bid_parse_decimal_string(p_raw text, p_max_scale integer)
RETURNS numeric
LANGUAGE plpgsql
IMMUTABLE
SET search_path = pg_catalog
AS $$
DECLARE frac text;
BEGIN
  IF p_raw IS NULL THEN RETURN NULL; END IF;
  IF p_raw = '' OR p_raw ~ '^[+-]' OR p_raw ~ '[eE]' OR p_raw ~ '^\.' OR p_raw ~ '\.$'
     OR p_raw !~ '^[0-9]+(\.[0-9]+)?$' THEN
    RAISE EXCEPTION 'QUOTE_DECIMAL_INVALID' USING ERRCODE='22023';
  END IF;
  IF p_raw = '0' OR p_raw ~ '^0\.0+$' THEN
    -- zero is allowed; reject only signed -0
    NULL;
  END IF;
  IF position('.' in p_raw) > 0 THEN
    frac := split_part(p_raw, '.', 2);
    IF char_length(frac) > p_max_scale THEN
      RAISE EXCEPTION 'QUOTE_DECIMAL_INVALID' USING ERRCODE='22023';
    END IF;
  END IF;
  RETURN p_raw::numeric;
END
$$;

CREATE FUNCTION kb_bid_compute_quote_line(
  p_pricing_mode text, p_tax_mode text, p_quantity numeric, p_unit text,
  p_unit_price numeric, p_entered_amount numeric, p_tax_rate numeric
)
RETURNS TABLE(
  complete boolean, quantity numeric, unit text, unit_price numeric, entered_amount numeric,
  tax_rate numeric, basis_amount numeric, net_amount numeric, tax_amount numeric, gross_amount numeric
)
LANGUAGE plpgsql
IMMUTABLE
SET search_path = pg_catalog
AS $$
DECLARE basis numeric(20,2); net numeric(20,2); tax numeric(20,2); gross numeric(20,2); product numeric(30,6);
 normalized_unit text;
BEGIN
  IF p_tax_rate IS NULL OR p_tax_rate < 0 OR p_tax_rate > 1 THEN
    RAISE EXCEPTION 'QUOTE_TAX_RATE_INVALID' USING ERRCODE='22023';
  END IF;
  IF p_pricing_mode='unit_price' THEN
    IF p_entered_amount IS NOT NULL THEN RAISE EXCEPTION 'QUOTE_LINE_SHAPE_INVALID' USING ERRCODE='22023'; END IF;
    normalized_unit := NULLIF(btrim(COALESCE(p_unit,'')), '');
    IF p_quantity IS NOT NULL AND (p_quantity<=0 OR p_quantity>1000000000) THEN
      RAISE EXCEPTION 'QUOTE_LINE_INCOMPLETE' USING ERRCODE='22023';
    END IF;
    IF p_unit_price IS NOT NULL AND (p_unit_price<0 OR p_unit_price>1000000000000) THEN
      RAISE EXCEPTION 'QUOTE_LINE_INCOMPLETE' USING ERRCODE='22023';
    END IF;
    IF p_quantity IS NULL OR normalized_unit IS NULL OR p_unit_price IS NULL THEN
      RETURN QUERY SELECT false, p_quantity, normalized_unit, p_unit_price, NULL::numeric, p_tax_rate,
        NULL::numeric, NULL::numeric, NULL::numeric, NULL::numeric;
      RETURN;
    END IF;
    BEGIN
      product := p_quantity * p_unit_price;
      basis := round(product, 2);
    EXCEPTION WHEN numeric_value_out_of_range OR data_exception THEN
      RAISE EXCEPTION 'QUOTE_AMOUNT_OVERFLOW' USING ERRCODE='22003';
    END;
  ELSIF p_pricing_mode='lump_sum' THEN
    IF p_quantity IS NOT NULL OR p_unit IS NOT NULL OR p_unit_price IS NOT NULL THEN
      RAISE EXCEPTION 'QUOTE_LINE_SHAPE_INVALID' USING ERRCODE='22023';
    END IF;
    IF p_entered_amount IS NULL THEN
      RETURN QUERY SELECT false, NULL::numeric, NULL::text, NULL::numeric, NULL::numeric, p_tax_rate,
        NULL::numeric, NULL::numeric, NULL::numeric, NULL::numeric;
      RETURN;
    END IF;
    IF p_entered_amount<0 THEN RAISE EXCEPTION 'QUOTE_LINE_INCOMPLETE' USING ERRCODE='22023'; END IF;
    basis := p_entered_amount;
  ELSE
    RAISE EXCEPTION 'QUOTE_LINE_SHAPE_INVALID' USING ERRCODE='22023';
  END IF;
  BEGIN
    IF p_tax_mode='tax_exclusive' THEN
      net := basis;
      tax := round(net * p_tax_rate, 2);
      gross := net + tax;
    ELSIF p_tax_mode='tax_inclusive' THEN
      gross := basis;
      net := round(gross / (1 + p_tax_rate), 2);
      tax := gross - net;
    ELSE
      RAISE EXCEPTION 'QUOTE_TAX_MODE_INVALID' USING ERRCODE='22023';
    END IF;
  EXCEPTION WHEN numeric_value_out_of_range OR data_exception THEN
    RAISE EXCEPTION 'QUOTE_AMOUNT_OVERFLOW' USING ERRCODE='22003';
  END;
  RETURN QUERY SELECT true,
    CASE WHEN p_pricing_mode='unit_price' THEN p_quantity ELSE NULL END,
    CASE WHEN p_pricing_mode='unit_price' THEN normalized_unit ELSE NULL END,
    CASE WHEN p_pricing_mode='unit_price' THEN p_unit_price ELSE NULL END,
    CASE WHEN p_pricing_mode='lump_sum' THEN p_entered_amount ELSE NULL END,
    p_tax_rate, basis, net, tax, gross;
END
$$;

CREATE FUNCTION kb_bid_build_quote_snapshot_v1(
  p_quote_id uuid, p_project_id uuid, p_revision bigint, p_tax_mode text, p_title text, p_notes text,
  p_lines jsonb, p_net_total numeric, p_tax_total numeric, p_gross_total numeric,
  p_ceiling jsonb, p_no_ceiling_review jsonb, p_fact_revision bigint,
  p_pricing_revision bigint, p_pricing_set_sha256 kb_sha256
)
RETURNS bytea
LANGUAGE plpgsql
IMMUTABLE
SET search_path = pg_catalog, public
AS $$
DECLARE payload text; line jsonb; first boolean := true; line_json text;
BEGIN
  payload := '{"schema_version":1,"quote_id":'||kb_bid_json_string(p_quote_id::text)
    ||',"project_id":'||kb_bid_json_string(p_project_id::text)
    ||',"revision":'||p_revision::text
    ||',"currency_code":"CNY","currency_scale":2,"tax_mode":'||kb_bid_json_string(p_tax_mode)
    ||',"title":'||kb_bid_json_string(p_title)
    ||',"notes":'||CASE WHEN p_notes IS NULL THEN 'null' ELSE kb_bid_json_string(p_notes) END
    ||',"lines":[';
  FOR line IN SELECT value FROM jsonb_array_elements(p_lines) ORDER BY (value->>'ordinal')::int LOOP
    line_json := '{"id":'||kb_bid_json_string(line->>'id')
      ||',"ordinal":'||(line->>'ordinal')
      ||',"description":'||kb_bid_json_string(line->>'description')
      ||',"pricing_mode":'||kb_bid_json_string(line->>'pricing_mode')
      ||',"quantity":'||CASE WHEN line->>'quantity' IS NULL THEN 'null' ELSE kb_bid_json_string(line->>'quantity') END
      ||',"unit":'||CASE WHEN line->>'unit' IS NULL THEN 'null' ELSE kb_bid_json_string(line->>'unit') END
      ||',"unit_price":'||CASE WHEN line->>'unit_price' IS NULL THEN 'null' ELSE kb_bid_json_string(line->>'unit_price') END
      ||',"entered_amount":'||CASE WHEN line->>'entered_amount' IS NULL THEN 'null' ELSE kb_bid_json_string(line->>'entered_amount') END
      ||',"tax_rate":'||kb_bid_json_string(line->>'tax_rate')
      ||',"basis_amount":'||kb_bid_json_string(line->>'basis_amount')
      ||',"net_amount":'||kb_bid_json_string(line->>'net_amount')
      ||',"tax_amount":'||kb_bid_json_string(line->>'tax_amount')
      ||',"gross_amount":'||kb_bid_json_string(line->>'gross_amount')
      ||',"user_confirmed":'||CASE WHEN (line->>'user_confirmed')::boolean THEN 'true' ELSE 'false' END
      ||'}';
    IF first THEN payload := payload||line_json; first := false; ELSE payload := payload||','||line_json; END IF;
  END LOOP;
  payload := payload||'],"net_total":'||kb_bid_json_string(kb_bid_format_amount(p_net_total))
    ||',"tax_total":'||kb_bid_json_string(kb_bid_format_amount(p_tax_total))
    ||',"gross_total":'||kb_bid_json_string(kb_bid_format_amount(p_gross_total))
    ||',"ceiling":'||CASE WHEN p_ceiling IS NULL THEN 'null' ELSE
      '{"amount":'||kb_bid_json_string(p_ceiling->>'amount')
      ||',"currency_code":'||kb_bid_json_string(p_ceiling->>'currency_code')
      ||',"basis":'||kb_bid_json_string(p_ceiling->>'basis')
      ||',"ceiling_revision":'||(p_ceiling->>'ceiling_revision')
      ||',"ceiling_identity_sha256":'||kb_bid_json_string(p_ceiling->>'ceiling_identity_sha256')
      ||'}' END
    ||',"no_ceiling_review":'||CASE WHEN p_no_ceiling_review IS NULL THEN 'null' ELSE
      '{"reviewed":'||CASE WHEN (p_no_ceiling_review->>'reviewed')::boolean THEN 'true' ELSE 'false' END
      ||',"reason":'||kb_bid_json_string(p_no_ceiling_review->>'reason')
      ||',"actor_kind":'||kb_bid_json_string(p_no_ceiling_review->>'actor_kind')
      ||',"actor_id":'||kb_bid_json_string(p_no_ceiling_review->>'actor_id')
      ||',"at":'||kb_bid_json_string(p_no_ceiling_review->>'at')
      ||'}' END
    ||',"fact_revision":'||p_fact_revision::text
    ||',"pricing_revision":'||p_pricing_revision::text
    ||',"pricing_set_sha256":'||kb_bid_json_string(p_pricing_set_sha256)
    ||'}';
  RETURN convert_to(payload, 'UTF8');
END
$$;

CREATE FUNCTION kb_bid_lock_quote_draft(p_project_id uuid)
RETURNS bid_quote_revisions
LANGUAGE plpgsql
SET search_path = pg_catalog, public
AS $$
DECLARE project_value bid_projects%ROWTYPE; quote_value bid_quotes%ROWTYPE;
 current_value bid_quote_current%ROWTYPE; revision bid_quote_revisions%ROWTYPE;
BEGIN
  SELECT * INTO STRICT project_value FROM bid_projects WHERE id=p_project_id FOR UPDATE;
  IF project_value.status<>'open' THEN RAISE EXCEPTION 'PROJECT_ENDED' USING ERRCODE='55000'; END IF;
  SELECT * INTO quote_value FROM bid_quotes WHERE project_id=p_project_id FOR UPDATE;
  IF NOT FOUND THEN RAISE EXCEPTION 'QUOTE_DRAFT_MISSING' USING ERRCODE='P0002'; END IF;
  SELECT * INTO STRICT current_value FROM bid_quote_current WHERE quote_id=quote_value.id FOR UPDATE;
  IF current_value.current_draft_revision_id IS NULL THEN
    RAISE EXCEPTION 'QUOTE_NOT_DRAFT' USING ERRCODE='55000';
  END IF;
  SELECT * INTO STRICT revision FROM bid_quote_revisions WHERE id=current_value.current_draft_revision_id FOR UPDATE;
  IF revision.status<>'draft' THEN RAISE EXCEPTION 'QUOTE_NOT_DRAFT' USING ERRCODE='55000'; END IF;
  PERFORM 1 FROM bid_quote_lines WHERE quote_revision_id=revision.id ORDER BY ordinal FOR UPDATE;
  RETURN revision;
END
$$;

CREATE FUNCTION kb_bid_recompute_line_row(p_line bid_quote_lines, p_tax_mode text)
RETURNS bid_quote_lines
LANGUAGE plpgsql
SET search_path = pg_catalog, public
AS $$
DECLARE computed record;
BEGIN
  SELECT * INTO computed FROM kb_bid_compute_quote_line(
    p_line.pricing_mode, p_tax_mode, p_line.quantity, p_line.unit, p_line.unit_price,
    p_line.entered_amount, p_line.tax_rate);
  p_line.complete := computed.complete;
  p_line.quantity := computed.quantity;
  p_line.unit := computed.unit;
  p_line.unit_price := computed.unit_price;
  p_line.entered_amount := computed.entered_amount;
  p_line.tax_rate := computed.tax_rate;
  p_line.basis_amount := computed.basis_amount;
  p_line.net_amount := computed.net_amount;
  p_line.tax_amount := computed.tax_amount;
  p_line.gross_amount := computed.gross_amount;
  RETURN p_line;
END
$$;

CREATE FUNCTION kb_bid_quote_state_json(p_project_id uuid)
RETURNS jsonb
LANGUAGE plpgsql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE quote_value bid_quotes%ROWTYPE; current_value bid_quote_current%ROWTYPE;
 revision bid_quote_revisions%ROWTYPE; snapshot bid_quote_snapshots%ROWTYPE; lines jsonb;
BEGIN
  SELECT * INTO quote_value FROM bid_quotes WHERE project_id=p_project_id;
  IF NOT FOUND THEN RETURN jsonb_build_object('project_id',p_project_id,'exists',false); END IF;
  SELECT * INTO current_value FROM bid_quote_current WHERE quote_id=quote_value.id;
  IF current_value.current_draft_revision_id IS NOT NULL THEN
    SELECT * INTO revision FROM bid_quote_revisions WHERE id=current_value.current_draft_revision_id;
    SELECT COALESCE(jsonb_agg(jsonb_build_object(
      'id',id,'ordinal',ordinal,'description',description,'pricing_mode',pricing_mode,'complete',complete,
      'quantity',CASE WHEN quantity IS NULL THEN NULL ELSE kb_bid_format_qty(quantity) END,
      'unit',unit,
      'unit_price',CASE WHEN unit_price IS NULL THEN NULL ELSE kb_bid_format_qty(unit_price) END,
      'entered_amount',CASE WHEN entered_amount IS NULL THEN NULL ELSE kb_bid_format_amount(entered_amount) END,
      'tax_rate',kb_bid_format_qty(tax_rate),
      'basis_amount',CASE WHEN basis_amount IS NULL THEN NULL ELSE kb_bid_format_amount(basis_amount) END,
      'net_amount',CASE WHEN net_amount IS NULL THEN NULL ELSE kb_bid_format_amount(net_amount) END,
      'tax_amount',CASE WHEN tax_amount IS NULL THEN NULL ELSE kb_bid_format_amount(tax_amount) END,
      'gross_amount',CASE WHEN gross_amount IS NULL THEN NULL ELSE kb_bid_format_amount(gross_amount) END,
      'user_confirmed',user_confirmed) ORDER BY ordinal),'[]'::jsonb)
      INTO lines FROM bid_quote_lines WHERE quote_revision_id=revision.id;
    RETURN jsonb_build_object('exists',true,'quote_id',quote_value.id,'project_id',p_project_id,
      'pointer','draft','revision_id',revision.id,'revision',revision.revision,'edit_version',revision.edit_version,
      'status',revision.status,'tax_mode',revision.tax_mode,'title',revision.title,'notes',revision.notes,
      'based_on_snapshot_id',revision.based_on_snapshot_id,'lines',lines,
      'active_finalized_snapshot_id',current_value.active_finalized_snapshot_id);
  END IF;
  SELECT * INTO snapshot FROM bid_quote_snapshots WHERE id=current_value.active_finalized_snapshot_id;
  RETURN jsonb_build_object('exists',true,'quote_id',quote_value.id,'project_id',p_project_id,
    'pointer','finalized','snapshot_id',snapshot.id,'revision',revision_id_to_rev(snapshot),
    'eligibility',snapshot.eligibility,'content_sha256',snapshot.content_sha256,
    'tax_mode',snapshot.tax_mode,'title',snapshot.title,'notes',snapshot.notes,
    'net_total',kb_bid_format_amount(snapshot.net_total),'tax_total',kb_bid_format_amount(snapshot.tax_total),
    'gross_total',kb_bid_format_amount(snapshot.gross_total));
END
$$;

CREATE FUNCTION revision_id_to_rev(p_snapshot bid_quote_snapshots)
RETURNS bigint
LANGUAGE sql
STABLE
SET search_path = pg_catalog, public
AS $$ SELECT revision FROM bid_quote_revisions WHERE id=p_snapshot.revision_id $$;

CREATE FUNCTION kb_bid_create_quote_draft(
  p_project_id uuid, p_tax_mode text, p_title text, p_notes text,
  p_actor kb_actor_identity, p_idempotency_key text, p_request_bytes bytea, p_request_sha256 kb_sha256
)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE replay bytea; project_value bid_projects%ROWTYPE; quote_id uuid; revision_id uuid;
 revision_no bigint; title text; notes text; response jsonb;
BEGIN
  PERFORM kb_bid_require_human_actor(p_actor);
  replay := kb_bid_idempotency_begin(p_actor,'bid.quote.create_draft',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  IF p_tax_mode NOT IN ('tax_inclusive','tax_exclusive') THEN RAISE EXCEPTION 'QUOTE_TAX_MODE_INVALID' USING ERRCODE='22023'; END IF;
  title := btrim(p_title);
  IF octet_length(title) NOT BETWEEN 1 AND 256 THEN RAISE EXCEPTION 'QUOTE_TITLE_INVALID' USING ERRCODE='22023'; END IF;
  notes := NULLIF(p_notes, '');
  IF notes IS NOT NULL AND btrim(notes)='' THEN notes := NULL; END IF;
  IF notes IS NOT NULL AND octet_length(notes)>4096 THEN RAISE EXCEPTION 'QUOTE_NOTES_INVALID' USING ERRCODE='22023'; END IF;
  SELECT * INTO STRICT project_value FROM bid_projects WHERE id=p_project_id FOR UPDATE;
  IF project_value.status<>'open' THEN RAISE EXCEPTION 'PROJECT_ENDED' USING ERRCODE='55000'; END IF;
  IF EXISTS (SELECT 1 FROM bid_quotes WHERE project_id=p_project_id) THEN
    RAISE EXCEPTION 'QUOTE_ALREADY_EXISTS' USING ERRCODE='23505';
  END IF;
  quote_id := gen_random_uuid();
  revision_id := gen_random_uuid();
  revision_no := 1;
  INSERT INTO bid_quotes(id,project_id,next_revision) VALUES(quote_id,p_project_id,2);
  INSERT INTO bid_quote_revisions(id,quote_id,project_id,revision,status,edit_version,currency_code,currency_scale,
    tax_mode,title,notes,created_by)
  VALUES(revision_id,quote_id,p_project_id,revision_no,'draft',0,'CNY',2,p_tax_mode,title,notes,p_actor);
  INSERT INTO bid_quote_current(quote_id,project_id,current_draft_revision_id,active_finalized_snapshot_id)
  VALUES(quote_id,p_project_id,revision_id,NULL);
  PERFORM kb_bid_stale_parts(p_project_id, ARRAY['6:quote','6:letter'], 'QUOTE_DRAFT_CHANGED');
  response := jsonb_build_object('quote_id',quote_id,'revision_id',revision_id,'revision',revision_no,'edit_version',0,
    'status','draft','tax_mode',p_tax_mode,'title',title,'notes',notes);
  INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
    entity_kind,entity_locator,after_revision,after_sha256)
  VALUES(gen_random_uuid(),1,'bid.quote.create_draft',p_actor,p_idempotency_key,p_request_sha256,
    encode(public.digest(convert_to(response::text,'UTF8'),'sha256'),'hex'),'bid_quote',
    jsonb_build_object('project_id',p_project_id,'quote_id',quote_id),revision_no,
    encode(public.digest(convert_to(response::text,'UTF8'),'sha256'),'hex'));
  PERFORM kb_bid_idempotency_complete(p_actor,'bid.quote.create_draft',p_idempotency_key,201,convert_to(response::text,'UTF8'));
  RETURN response;
END
$$;

CREATE FUNCTION kb_bid_patch_quote_header(
  p_project_id uuid, p_expected_edit_version bigint, p_tax_mode text, p_title text, p_notes text,
  p_actor kb_actor_identity, p_idempotency_key text, p_request_bytes bytea, p_request_sha256 kb_sha256
)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE replay bytea; revision bid_quote_revisions%ROWTYPE; title text; notes text; response jsonb; line bid_quote_lines%ROWTYPE;
BEGIN
  PERFORM kb_bid_require_human_actor(p_actor);
  replay := kb_bid_idempotency_begin(p_actor,'bid.quote.patch_header',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  revision := kb_bid_lock_quote_draft(p_project_id);
  IF revision.edit_version<>p_expected_edit_version THEN RAISE EXCEPTION 'QUOTE_EDIT_VERSION_MISMATCH' USING ERRCODE='40001'; END IF;
  IF p_tax_mode NOT IN ('tax_inclusive','tax_exclusive') THEN RAISE EXCEPTION 'QUOTE_TAX_MODE_INVALID' USING ERRCODE='22023'; END IF;
  title := btrim(p_title);
  IF octet_length(title) NOT BETWEEN 1 AND 256 THEN RAISE EXCEPTION 'QUOTE_TITLE_INVALID' USING ERRCODE='22023'; END IF;
  notes := NULLIF(p_notes,'');
  IF notes IS NOT NULL AND btrim(notes)='' THEN notes := NULL; END IF;
  IF notes IS NOT NULL AND octet_length(notes)>4096 THEN RAISE EXCEPTION 'QUOTE_NOTES_INVALID' USING ERRCODE='22023'; END IF;
  UPDATE bid_quote_revisions SET tax_mode=p_tax_mode,title=title,notes=notes,edit_version=edit_version+1,updated_at=clock_timestamp()
   WHERE id=revision.id RETURNING * INTO revision;
  IF p_tax_mode IS DISTINCT FROM revision.tax_mode THEN
    -- tax_mode already updated; recompute lines against new mode
    NULL;
  END IF;
  FOR line IN SELECT * FROM bid_quote_lines WHERE quote_revision_id=revision.id ORDER BY ordinal LOOP
    line := kb_bid_recompute_line_row(line, p_tax_mode);
    UPDATE bid_quote_lines SET complete=line.complete,basis_amount=line.basis_amount,net_amount=line.net_amount,
      tax_amount=line.tax_amount,gross_amount=line.gross_amount WHERE id=line.id;
  END LOOP;
  PERFORM kb_bid_stale_parts(p_project_id, ARRAY['6:quote','6:letter'], 'QUOTE_DRAFT_CHANGED');
  response := jsonb_build_object('revision_id',revision.id,'edit_version',revision.edit_version,'tax_mode',p_tax_mode,
    'title',title,'notes',notes);
  INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
    entity_kind,entity_locator,after_revision,after_sha256)
  VALUES(gen_random_uuid(),1,'bid.quote.patch_header',p_actor,p_idempotency_key,p_request_sha256,
    encode(public.digest(convert_to(response::text,'UTF8'),'sha256'),'hex'),'bid_quote',
    jsonb_build_object('project_id',p_project_id,'revision_id',revision.id),revision.edit_version,
    encode(public.digest(convert_to(response::text,'UTF8'),'sha256'),'hex'));
  PERFORM kb_bid_idempotency_complete(p_actor,'bid.quote.patch_header',p_idempotency_key,200,convert_to(response::text,'UTF8'));
  RETURN response;
END
$$;

CREATE FUNCTION kb_bid_upsert_quote_line(
  p_project_id uuid, p_line_id uuid, p_expected_edit_version bigint, p_ordinal integer,
  p_description text, p_pricing_mode text, p_quantity text, p_unit text, p_unit_price text,
  p_entered_amount text, p_tax_rate text, p_user_confirmed boolean,
  p_actor kb_actor_identity, p_idempotency_key text, p_request_bytes bytea, p_request_sha256 kb_sha256
)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE replay bytea; revision bid_quote_revisions%ROWTYPE; computed record; response jsonb;
 quantity numeric; unit_price numeric; entered_amount numeric; tax_rate numeric;
BEGIN
  PERFORM kb_bid_require_human_actor(p_actor);
  replay := kb_bid_idempotency_begin(p_actor,'bid.quote.upsert_line',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  revision := kb_bid_lock_quote_draft(p_project_id);
  IF revision.edit_version<>p_expected_edit_version THEN RAISE EXCEPTION 'QUOTE_EDIT_VERSION_MISMATCH' USING ERRCODE='40001'; END IF;
  IF p_user_confirmed AND p_actor NOT LIKE 'user:%' THEN
    RAISE EXCEPTION 'QUOTE_USER_CONFIRMATION_ACTOR_REQUIRED' USING ERRCODE='42501';
  END IF;
  quantity := kb_bid_parse_decimal_string(p_quantity, 6);
  unit_price := kb_bid_parse_decimal_string(p_unit_price, 6);
  entered_amount := kb_bid_parse_decimal_string(p_entered_amount, 2);
  tax_rate := kb_bid_parse_decimal_string(p_tax_rate, 6);
  SELECT * INTO computed FROM kb_bid_compute_quote_line(
    p_pricing_mode, revision.tax_mode, quantity, p_unit, unit_price, entered_amount, tax_rate);
  INSERT INTO bid_quote_lines(id,quote_revision_id,ordinal,description,pricing_mode,complete,quantity,unit,unit_price,
    entered_amount,tax_rate,basis_amount,net_amount,tax_amount,gross_amount,user_confirmed)
  VALUES(p_line_id,revision.id,p_ordinal,p_description,p_pricing_mode,computed.complete,computed.quantity,computed.unit,
    computed.unit_price,computed.entered_amount,computed.tax_rate,computed.basis_amount,computed.net_amount,
    computed.tax_amount,computed.gross_amount,COALESCE(p_user_confirmed,false))
  ON CONFLICT (id) DO UPDATE SET ordinal=EXCLUDED.ordinal,description=EXCLUDED.description,pricing_mode=EXCLUDED.pricing_mode,
    complete=EXCLUDED.complete,quantity=EXCLUDED.quantity,unit=EXCLUDED.unit,unit_price=EXCLUDED.unit_price,
    entered_amount=EXCLUDED.entered_amount,tax_rate=EXCLUDED.tax_rate,basis_amount=EXCLUDED.basis_amount,
    net_amount=EXCLUDED.net_amount,tax_amount=EXCLUDED.tax_amount,gross_amount=EXCLUDED.gross_amount,
    user_confirmed=EXCLUDED.user_confirmed
  WHERE bid_quote_lines.quote_revision_id=revision.id;
  IF NOT FOUND AND NOT EXISTS (SELECT 1 FROM bid_quote_lines WHERE id=p_line_id AND quote_revision_id=revision.id) THEN
    RAISE EXCEPTION 'QUOTE_LINE_SCOPE_MISMATCH' USING ERRCODE='23503';
  END IF;
  UPDATE bid_quote_revisions SET edit_version=edit_version+1,updated_at=clock_timestamp() WHERE id=revision.id
    RETURNING * INTO revision;
  PERFORM kb_bid_stale_parts(p_project_id, ARRAY['6:quote','6:letter'], 'QUOTE_DRAFT_CHANGED');
  response := jsonb_build_object('line_id',p_line_id,'edit_version',revision.edit_version,'complete',computed.complete,
    'basis_amount',CASE WHEN computed.basis_amount IS NULL THEN NULL ELSE kb_bid_format_amount(computed.basis_amount) END,
    'net_amount',CASE WHEN computed.net_amount IS NULL THEN NULL ELSE kb_bid_format_amount(computed.net_amount) END,
    'tax_amount',CASE WHEN computed.tax_amount IS NULL THEN NULL ELSE kb_bid_format_amount(computed.tax_amount) END,
    'gross_amount',CASE WHEN computed.gross_amount IS NULL THEN NULL ELSE kb_bid_format_amount(computed.gross_amount) END);
  INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
    entity_kind,entity_locator,after_revision,after_sha256)
  VALUES(gen_random_uuid(),1,'bid.quote.upsert_line',p_actor,p_idempotency_key,p_request_sha256,
    encode(public.digest(convert_to(response::text,'UTF8'),'sha256'),'hex'),'bid_quote_line',
    jsonb_build_object('project_id',p_project_id,'line_id',p_line_id),revision.edit_version,
    encode(public.digest(convert_to(response::text,'UTF8'),'sha256'),'hex'));
  PERFORM kb_bid_idempotency_complete(p_actor,'bid.quote.upsert_line',p_idempotency_key,200,convert_to(response::text,'UTF8'));
  RETURN response;
END
$$;

CREATE FUNCTION kb_bid_delete_quote_line(
  p_project_id uuid, p_line_id uuid, p_expected_edit_version bigint,
  p_actor kb_actor_identity, p_idempotency_key text, p_request_bytes bytea, p_request_sha256 kb_sha256
)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE replay bytea; revision bid_quote_revisions%ROWTYPE; response jsonb;
BEGIN
  PERFORM kb_bid_require_human_actor(p_actor);
  replay := kb_bid_idempotency_begin(p_actor,'bid.quote.delete_line',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  revision := kb_bid_lock_quote_draft(p_project_id);
  IF revision.edit_version<>p_expected_edit_version THEN RAISE EXCEPTION 'QUOTE_EDIT_VERSION_MISMATCH' USING ERRCODE='40001'; END IF;
  DELETE FROM bid_quote_lines WHERE id=p_line_id AND quote_revision_id=revision.id;
  IF NOT FOUND THEN RAISE EXCEPTION 'QUOTE_LINE_MISSING' USING ERRCODE='P0002'; END IF;
  UPDATE bid_quote_revisions SET edit_version=edit_version+1,updated_at=clock_timestamp() WHERE id=revision.id
    RETURNING * INTO revision;
  PERFORM kb_bid_stale_parts(p_project_id, ARRAY['6:quote','6:letter'], 'QUOTE_DRAFT_CHANGED');
  response := jsonb_build_object('line_id',p_line_id,'edit_version',revision.edit_version,'deleted',true);
  INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
    entity_kind,entity_locator,after_revision,after_sha256)
  VALUES(gen_random_uuid(),1,'bid.quote.delete_line',p_actor,p_idempotency_key,p_request_sha256,
    encode(public.digest(convert_to(response::text,'UTF8'),'sha256'),'hex'),'bid_quote_line',
    jsonb_build_object('project_id',p_project_id,'line_id',p_line_id),revision.edit_version,
    encode(public.digest(convert_to(response::text,'UTF8'),'sha256'),'hex'));
  PERFORM kb_bid_idempotency_complete(p_actor,'bid.quote.delete_line',p_idempotency_key,200,convert_to(response::text,'UTF8'));
  RETURN response;
END
$$;

CREATE FUNCTION kb_bid_reorder_quote_lines(
  p_project_id uuid, p_expected_edit_version bigint, p_line_ids uuid[],
  p_actor kb_actor_identity, p_idempotency_key text, p_request_bytes bytea, p_request_sha256 kb_sha256
)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE replay bytea; revision bid_quote_revisions%ROWTYPE; idx int; response jsonb; existing int;
BEGIN
  PERFORM kb_bid_require_human_actor(p_actor);
  replay := kb_bid_idempotency_begin(p_actor,'bid.quote.reorder_lines',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  revision := kb_bid_lock_quote_draft(p_project_id);
  IF revision.edit_version<>p_expected_edit_version THEN RAISE EXCEPTION 'QUOTE_EDIT_VERSION_MISMATCH' USING ERRCODE='40001'; END IF;
  SELECT count(*) INTO existing FROM bid_quote_lines WHERE quote_revision_id=revision.id;
  IF existing <> COALESCE(array_length(p_line_ids,1),0) THEN RAISE EXCEPTION 'QUOTE_LINE_REORDER_MISMATCH' USING ERRCODE='22023'; END IF;
  FOR idx IN 1..COALESCE(array_length(p_line_ids,1),0) LOOP
    UPDATE bid_quote_lines SET ordinal=100000+idx WHERE id=p_line_ids[idx] AND quote_revision_id=revision.id;
    IF NOT FOUND THEN RAISE EXCEPTION 'QUOTE_LINE_REORDER_MISMATCH' USING ERRCODE='22023'; END IF;
  END LOOP;
  FOR idx IN 1..COALESCE(array_length(p_line_ids,1),0) LOOP
    UPDATE bid_quote_lines SET ordinal=idx-1 WHERE id=p_line_ids[idx];
  END LOOP;
  UPDATE bid_quote_revisions SET edit_version=edit_version+1,updated_at=clock_timestamp() WHERE id=revision.id
    RETURNING * INTO revision;
  PERFORM kb_bid_stale_parts(p_project_id, ARRAY['6:quote','6:letter'], 'QUOTE_DRAFT_CHANGED');
  response := jsonb_build_object('edit_version',revision.edit_version,'line_ids',to_jsonb(p_line_ids));
  INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
    entity_kind,entity_locator,after_revision,after_sha256)
  VALUES(gen_random_uuid(),1,'bid.quote.reorder_lines',p_actor,p_idempotency_key,p_request_sha256,
    encode(public.digest(convert_to(response::text,'UTF8'),'sha256'),'hex'),'bid_quote',
    jsonb_build_object('project_id',p_project_id),revision.edit_version,
    encode(public.digest(convert_to(response::text,'UTF8'),'sha256'),'hex'));
  PERFORM kb_bid_idempotency_complete(p_actor,'bid.quote.reorder_lines',p_idempotency_key,200,convert_to(response::text,'UTF8'));
  RETURN response;
END
$$;

CREATE FUNCTION kb_bid_preview_quote_totals(p_project_id uuid)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE revision bid_quote_revisions%ROWTYPE; net numeric(20,2):=0; tax numeric(20,2):=0; gross numeric(20,2):=0; line bid_quote_lines%ROWTYPE;
 project_value bid_projects%ROWTYPE; quote_value bid_quotes%ROWTYPE; current_value bid_quote_current%ROWTYPE;
BEGIN
  SELECT * INTO STRICT project_value FROM bid_projects WHERE id=p_project_id;
  IF project_value.status<>'open' THEN RAISE EXCEPTION 'PROJECT_ENDED' USING ERRCODE='55000'; END IF;
  SELECT * INTO quote_value FROM bid_quotes WHERE project_id=p_project_id;
  IF NOT FOUND THEN RAISE EXCEPTION 'QUOTE_DRAFT_MISSING' USING ERRCODE='P0002'; END IF;
  SELECT * INTO STRICT current_value FROM bid_quote_current WHERE quote_id=quote_value.id;
  IF current_value.current_draft_revision_id IS NULL THEN RAISE EXCEPTION 'QUOTE_NOT_DRAFT' USING ERRCODE='55000'; END IF;
  SELECT * INTO STRICT revision FROM bid_quote_revisions WHERE id=current_value.current_draft_revision_id;
  IF revision.status<>'draft' THEN RAISE EXCEPTION 'QUOTE_NOT_DRAFT' USING ERRCODE='55000'; END IF;
  FOR line IN SELECT * FROM bid_quote_lines WHERE quote_revision_id=revision.id AND complete ORDER BY ordinal LOOP
    BEGIN
      net := net + line.net_amount; tax := tax + line.tax_amount; gross := gross + line.gross_amount;
    EXCEPTION WHEN numeric_value_out_of_range OR data_exception THEN
      RAISE EXCEPTION 'QUOTE_AMOUNT_OVERFLOW' USING ERRCODE='22003';
    END;
  END LOOP;
  RETURN jsonb_build_object('edit_version',revision.edit_version,'tax_mode',revision.tax_mode,
    'net_total',kb_bid_format_amount(net),'tax_total',kb_bid_format_amount(tax),'gross_total',kb_bid_format_amount(gross));
END
$$;

CREATE FUNCTION kb_bid_finalize_quote(
  p_project_id uuid, p_expected_edit_version bigint, p_expected_fact_revision bigint,
  p_expected_ceiling_revision bigint, p_expected_ceiling_identity_sha256 kb_sha256,
  p_expected_pricing_revision bigint, p_expected_pricing_set_sha256 kb_sha256,
  p_no_ceiling_reviewed boolean, p_no_ceiling_reason text,
  p_actor kb_actor_identity, p_idempotency_key text, p_request_bytes bytea, p_request_sha256 kb_sha256
)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE replay bytea; project_value bid_projects%ROWTYPE; quote_value bid_quotes%ROWTYPE;
 current_value bid_quote_current%ROWTYPE; revision bid_quote_revisions%ROWTYPE; line bid_quote_lines%ROWTYPE;
 computed record; net numeric(20,2):=0; tax numeric(20,2):=0; gross numeric(20,2):=0;
 lines jsonb := '[]'::jsonb; line_json jsonb; snapshot_id uuid := gen_random_uuid();
 payload bytea; digest kb_sha256; ceiling jsonb; review jsonb; pricing bid_clause_set_identities%ROWTYPE;
 compare numeric(20,2); now_ts timestamptz := clock_timestamp(); response jsonb;
BEGIN
  PERFORM kb_bid_require_human_actor(p_actor);
  replay := kb_bid_idempotency_begin(p_actor,'bid.quote.finalize',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  SELECT * INTO STRICT project_value FROM bid_projects WHERE id=p_project_id FOR UPDATE;
  IF project_value.status<>'open' THEN RAISE EXCEPTION 'PROJECT_ENDED' USING ERRCODE='55000'; END IF;
  SELECT * INTO STRICT quote_value FROM bid_quotes WHERE project_id=p_project_id FOR UPDATE;
  SELECT * INTO STRICT current_value FROM bid_quote_current WHERE quote_id=quote_value.id FOR UPDATE;
  IF current_value.current_draft_revision_id IS NULL THEN RAISE EXCEPTION 'QUOTE_NOT_DRAFT' USING ERRCODE='55000'; END IF;
  SELECT * INTO STRICT revision FROM bid_quote_revisions WHERE id=current_value.current_draft_revision_id FOR UPDATE;
  IF revision.status<>'draft' OR revision.edit_version<>p_expected_edit_version THEN
    RAISE EXCEPTION 'QUOTE_EDIT_VERSION_MISMATCH' USING ERRCODE='40001';
  END IF;
  IF project_value.fact_revision<>p_expected_fact_revision THEN RAISE EXCEPTION 'FACT_REVISION_CAS_MISMATCH' USING ERRCODE='40001'; END IF;
  IF project_value.ceiling_revision<>p_expected_ceiling_revision
     OR project_value.ceiling_identity_sha256<>p_expected_ceiling_identity_sha256 THEN
    RAISE EXCEPTION 'CEILING_IDENTITY_CAS_MISMATCH' USING ERRCODE='40001';
  END IF;
  SELECT * INTO STRICT pricing FROM bid_clause_set_identities WHERE project_id=p_project_id AND set_kind='pricing' FOR UPDATE;
  IF pricing.revision<>p_expected_pricing_revision OR pricing.content_sha256<>p_expected_pricing_set_sha256 THEN
    RAISE EXCEPTION 'PRICING_IDENTITY_CAS_MISMATCH' USING ERRCODE='40001';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM bid_quote_lines WHERE quote_revision_id=revision.id) THEN
    RAISE EXCEPTION 'QUOTE_EMPTY' USING ERRCODE='22023';
  END IF;
  FOR line IN SELECT * FROM bid_quote_lines WHERE quote_revision_id=revision.id ORDER BY ordinal FOR UPDATE LOOP
    IF btrim(line.description)='' THEN RAISE EXCEPTION 'QUOTE_LINE_INCOMPLETE' USING ERRCODE='22023'; END IF;
    SELECT * INTO computed FROM kb_bid_compute_quote_line(line.pricing_mode,revision.tax_mode,line.quantity,line.unit,
      line.unit_price,line.entered_amount,line.tax_rate);
    IF NOT computed.complete THEN RAISE EXCEPTION 'QUOTE_LINE_INCOMPLETE' USING ERRCODE='22023'; END IF;
    IF NOT line.user_confirmed THEN RAISE EXCEPTION 'QUOTE_LINE_UNCONFIRMED' USING ERRCODE='22023'; END IF;
    UPDATE bid_quote_lines SET complete=true,basis_amount=computed.basis_amount,net_amount=computed.net_amount,
      tax_amount=computed.tax_amount,gross_amount=computed.gross_amount WHERE id=line.id;
    BEGIN
      net := net + computed.net_amount; tax := tax + computed.tax_amount; gross := gross + computed.gross_amount;
    EXCEPTION WHEN numeric_value_out_of_range OR data_exception THEN
      RAISE EXCEPTION 'QUOTE_AMOUNT_OVERFLOW' USING ERRCODE='22003';
    END;
    line_json := jsonb_build_object(
      'id',line.id,'ordinal',line.ordinal,'description',line.description,'pricing_mode',line.pricing_mode,
      'quantity',CASE WHEN computed.quantity IS NULL THEN NULL ELSE kb_bid_format_qty(computed.quantity) END,
      'unit',computed.unit,
      'unit_price',CASE WHEN computed.unit_price IS NULL THEN NULL ELSE kb_bid_format_qty(computed.unit_price) END,
      'entered_amount',CASE WHEN computed.entered_amount IS NULL THEN NULL ELSE kb_bid_format_amount(computed.entered_amount) END,
      'tax_rate',kb_bid_format_qty(computed.tax_rate),
      'basis_amount',kb_bid_format_amount(computed.basis_amount),
      'net_amount',kb_bid_format_amount(computed.net_amount),
      'tax_amount',kb_bid_format_amount(computed.tax_amount),
      'gross_amount',kb_bid_format_amount(computed.gross_amount),
      'user_confirmed',true);
    lines := lines || jsonb_build_array(line_json);
  END LOOP;
  IF project_value.ceiling_price IS NOT NULL THEN
    IF project_value.ceiling_basis='unspecified' THEN RAISE EXCEPTION 'CEILING_BASIS_UNSPECIFIED' USING ERRCODE='22023'; END IF;
    IF p_no_ceiling_reviewed THEN RAISE EXCEPTION 'QUOTE_CEILING_REVIEW_CONFLICT' USING ERRCODE='22023'; END IF;
    compare := CASE WHEN project_value.ceiling_basis='tax_inclusive' THEN gross ELSE net END;
    IF compare > project_value.ceiling_price THEN RAISE EXCEPTION 'QUOTE_CEILING_EXCEEDED' USING ERRCODE='22023'; END IF;
    ceiling := jsonb_build_object('amount',kb_bid_format_amount(project_value.ceiling_price),'currency_code','CNY',
      'basis',project_value.ceiling_basis,'ceiling_revision',project_value.ceiling_revision,
      'ceiling_identity_sha256',project_value.ceiling_identity_sha256);
    review := NULL;
  ELSE
    IF p_no_ceiling_reviewed IS DISTINCT FROM true OR p_no_ceiling_reason IS NULL
       OR octet_length(btrim(p_no_ceiling_reason)) NOT BETWEEN 1 AND 512 THEN
      RAISE EXCEPTION 'NO_CEILING_REVIEW_REQUIRED' USING ERRCODE='22023';
    END IF;
    IF p_actor NOT LIKE 'user:%' THEN
      RAISE EXCEPTION 'NO_CEILING_REVIEW_ACTOR_REQUIRED' USING ERRCODE='42501';
    END IF;
    ceiling := NULL;
    review := jsonb_build_object('reviewed',true,'reason',btrim(p_no_ceiling_reason),'actor_kind','user',
      'actor_id',substr(p_actor,6)::uuid,'at',kb_bid_utc_json_time(now_ts));
  END IF;
  payload := kb_bid_build_quote_snapshot_v1(quote_value.id,p_project_id,revision.revision,revision.tax_mode,revision.title,
    revision.notes,lines,net,tax,gross,ceiling,review,project_value.fact_revision,pricing.revision,pricing.content_sha256);
  digest := encode(public.digest(payload,'sha256'),'hex');
  INSERT INTO bid_quote_snapshots(id,quote_id,revision_id,project_id,schema_version,canonical_payload,content_sha256,
    currency_code,tax_mode,net_total,tax_total,gross_total,ceiling_revision,ceiling_identity_sha256,fact_revision,
    pricing_revision,pricing_set_sha256,eligibility,finalized_by,finalized_at,title,notes,no_ceiling_review)
  VALUES(snapshot_id,quote_value.id,revision.id,p_project_id,1,payload,digest,'CNY',revision.tax_mode,net,tax,gross,
    project_value.ceiling_revision,project_value.ceiling_identity_sha256,project_value.fact_revision,pricing.revision,
    pricing.content_sha256,'eligible',p_actor,now_ts,revision.title,revision.notes,review);
  UPDATE bid_quote_revisions SET status='finalized',updated_at=now_ts WHERE id=revision.id;
  UPDATE bid_quote_current SET current_draft_revision_id=NULL,active_finalized_snapshot_id=snapshot_id
   WHERE quote_id=quote_value.id;
  PERFORM kb_bid_stale_parts(p_project_id, ARRAY['6:quote','6:letter'], 'QUOTE_FINALIZED');
  response := jsonb_build_object('snapshot_id',snapshot_id,'content_sha256',digest,'revision',revision.revision,
    'eligibility','eligible','net_total',kb_bid_format_amount(net),'tax_total',kb_bid_format_amount(tax),
    'gross_total',kb_bid_format_amount(gross));
  INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
    entity_kind,entity_locator,after_revision,after_sha256)
  VALUES(gen_random_uuid(),1,'bid.quote.finalize',p_actor,p_idempotency_key,p_request_sha256,
    encode(public.digest(convert_to(response::text,'UTF8'),'sha256'),'hex'),'bid_quote_snapshot',
    jsonb_build_object('project_id',p_project_id,'snapshot_id',snapshot_id),revision.revision,digest);
  PERFORM kb_bid_idempotency_complete(p_actor,'bid.quote.finalize',p_idempotency_key,200,convert_to(response::text,'UTF8'));
  RETURN response;
END
$$;

CREATE FUNCTION kb_bid_reopen_quote(
  p_project_id uuid, p_expected_snapshot_id uuid, p_expected_fact_revision bigint,
  p_expected_pricing_revision bigint,
  p_actor kb_actor_identity, p_idempotency_key text, p_request_bytes bytea, p_request_sha256 kb_sha256
)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE replay bytea; project_value bid_projects%ROWTYPE; quote_value bid_quotes%ROWTYPE;
 current_value bid_quote_current%ROWTYPE; snapshot bid_quote_snapshots%ROWTYPE; old_rev bid_quote_revisions%ROWTYPE;
 new_id uuid := gen_random_uuid(); payload jsonb; item jsonb; response jsonb; next_rev bigint;
BEGIN
  PERFORM kb_bid_require_human_actor(p_actor);
  replay := kb_bid_idempotency_begin(p_actor,'bid.quote.reopen',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  SELECT * INTO STRICT project_value FROM bid_projects WHERE id=p_project_id FOR UPDATE;
  IF project_value.status<>'open' THEN RAISE EXCEPTION 'PROJECT_ENDED' USING ERRCODE='55000'; END IF;
  IF project_value.fact_revision<>p_expected_fact_revision THEN RAISE EXCEPTION 'FACT_REVISION_CAS_MISMATCH' USING ERRCODE='40001'; END IF;
  SELECT * INTO STRICT quote_value FROM bid_quotes WHERE project_id=p_project_id FOR UPDATE;
  SELECT * INTO STRICT current_value FROM bid_quote_current WHERE quote_id=quote_value.id FOR UPDATE;
  IF current_value.active_finalized_snapshot_id IS DISTINCT FROM p_expected_snapshot_id THEN
    RAISE EXCEPTION 'QUOTE_SNAPSHOT_CAS_MISMATCH' USING ERRCODE='40001';
  END IF;
  SELECT * INTO STRICT snapshot FROM bid_quote_snapshots WHERE id=p_expected_snapshot_id FOR UPDATE;
  IF snapshot.eligibility='superseded' THEN RAISE EXCEPTION 'QUOTE_SNAPSHOT_SUPERSEDED' USING ERRCODE='55000'; END IF;
  SELECT * INTO STRICT old_rev FROM bid_quote_revisions WHERE id=snapshot.revision_id FOR UPDATE;
  IF (SELECT revision FROM bid_clause_set_identities WHERE project_id=p_project_id AND set_kind='pricing')
     <> p_expected_pricing_revision THEN
    RAISE EXCEPTION 'PRICING_IDENTITY_CAS_MISMATCH' USING ERRCODE='40001';
  END IF;
  payload := convert_from(snapshot.canonical_payload,'UTF8')::jsonb;
  next_rev := quote_value.next_revision;
  INSERT INTO bid_quote_revisions(id,quote_id,project_id,revision,status,edit_version,currency_code,currency_scale,
    tax_mode,title,notes,based_on_snapshot_id,created_by)
  VALUES(new_id,quote_value.id,p_project_id,next_rev,'draft',0,'CNY',2,payload->>'tax_mode',payload->>'title',
    NULLIF(payload->>'notes',''),snapshot.id,p_actor);
  FOR item IN SELECT value FROM jsonb_array_elements(payload->'lines') ORDER BY (value->>'ordinal')::int LOOP
    INSERT INTO bid_quote_lines(id,quote_revision_id,ordinal,description,pricing_mode,complete,quantity,unit,unit_price,
      entered_amount,tax_rate,basis_amount,net_amount,tax_amount,gross_amount,user_confirmed)
    VALUES(gen_random_uuid(),new_id,(item->>'ordinal')::int,item->>'description',item->>'pricing_mode',true,
      CASE WHEN item->>'quantity' IS NULL THEN NULL ELSE (item->>'quantity')::numeric END,
      NULLIF(item->>'unit',''),
      CASE WHEN item->>'unit_price' IS NULL THEN NULL ELSE (item->>'unit_price')::numeric END,
      CASE WHEN item->>'entered_amount' IS NULL THEN NULL ELSE (item->>'entered_amount')::numeric END,
      (item->>'tax_rate')::numeric,(item->>'basis_amount')::numeric,(item->>'net_amount')::numeric,
      (item->>'tax_amount')::numeric,(item->>'gross_amount')::numeric,(item->>'user_confirmed')::boolean);
  END LOOP;
  UPDATE bid_quote_revisions SET status='reopened',updated_at=clock_timestamp() WHERE id=old_rev.id;
  UPDATE bid_quote_snapshots SET eligibility='superseded' WHERE id=snapshot.id;
  UPDATE bid_quotes SET next_revision=next_rev+1 WHERE id=quote_value.id;
  UPDATE bid_quote_current SET current_draft_revision_id=new_id,active_finalized_snapshot_id=NULL WHERE quote_id=quote_value.id;
  PERFORM kb_bid_stale_parts(p_project_id, ARRAY['6:quote','6:letter'], 'QUOTE_REOPENED');
  response := jsonb_build_object('revision_id',new_id,'revision',next_rev,'based_on_snapshot_id',snapshot.id,'edit_version',0);
  INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
    entity_kind,entity_locator,after_revision,after_sha256)
  VALUES(gen_random_uuid(),1,'bid.quote.reopen',p_actor,p_idempotency_key,p_request_sha256,
    encode(public.digest(convert_to(response::text,'UTF8'),'sha256'),'hex'),'bid_quote',
    jsonb_build_object('project_id',p_project_id,'revision_id',new_id),next_rev,
    encode(public.digest(convert_to(response::text,'UTF8'),'sha256'),'hex'));
  PERFORM kb_bid_idempotency_complete(p_actor,'bid.quote.reopen',p_idempotency_key,200,convert_to(response::text,'UTF8'));
  RETURN response;
END
$$;

CREATE FUNCTION kb_bid_route_procedural(p_text text,p_contract_version text)
RETURNS jsonb
LANGUAGE plpgsql
STABLE
SET search_path = pg_catalog, public
AS $$
DECLARE hits text[] := '{}'; contract_payload jsonb; overridden jsonb;
BEGIN
  SELECT convert_from(canonical_payload,'UTF8')::jsonb INTO STRICT contract_payload
    FROM procedural_router_contract_artifacts WHERE version=p_contract_version;
  overridden:=contract_payload->'overrides'->p_text;
  IF overridden IS NOT NULL THEN RETURN overridden; END IF;
  IF (p_text LIKE '%ISO%' OR p_text LIKE '%等保%' OR p_text LIKE '%资质证书%' OR p_text LIKE '%资格证书%' OR p_text LIKE '%软著%')
     AND (p_text LIKE '%上传%' OR p_text LIKE '%提交%')
     AND p_text NOT LIKE '%保证金%' AND p_text NOT LIKE '%授权委托%' AND p_text NOT LIKE '%回执%' THEN
    RETURN jsonb_build_object('status','review','reason','QUALIFICATION_NOT_PROCEDURAL');
  END IF;
  IF p_text LIKE '%保证金%' AND (p_text LIKE '%金额%' OR p_text LIKE '%万元%' OR p_text LIKE '%元%')
     AND p_text NOT LIKE '%缴纳%' AND p_text NOT LIKE '%提交%' AND p_text NOT LIKE '%上传%'
     AND p_text NOT LIKE '%凭证%' AND p_text NOT LIKE '%回执%' AND p_text NOT LIKE '%保函%' THEN
    RETURN jsonb_build_object('status','review','reason','BID_BOND_AMOUNT_WITHOUT_ACTION');
  END IF;
  IF (p_text LIKE '%保证金%' OR p_text LIKE '%保函%')
     AND (p_text LIKE '%缴纳%' OR p_text LIKE '%凭证%' OR p_text LIKE '%回执%' OR p_text LIKE '%提交%' OR p_text LIKE '%上传%') THEN
    hits := hits || ARRAY['bid_bond'];
  END IF;
  IF p_text LIKE '%口头确认%' OR p_text LIKE '%口头授权%' THEN
    IF p_text NOT LIKE '%原件%' AND p_text NOT LIKE '%复印件%' AND p_text NOT LIKE '%附件%' AND p_text NOT LIKE '%提交%' THEN
      RETURN jsonb_build_object('status','review','reason','AUTHORIZATION_WITHOUT_MATERIAL');
    END IF;
  END IF;
  IF p_text LIKE '%授权委托书%' OR p_text LIKE '%法定代表人证明%' OR p_text LIKE '%授权代理人证明%' OR p_text LIKE '%代理人证明%' THEN
    hits := hits || ARRAY['authorization_support'];
  END IF;
  IF p_text LIKE '%印章样本%' OR p_text LIKE '%签章样张%' OR p_text LIKE '%盖章截图%' OR p_text LIKE '%盖章图样%' THEN
    hits := hits || ARRAY['seal_sample'];
  ELSIF (p_text LIKE '%投标函%' AND (p_text LIKE '%签字%' OR p_text LIKE '%盖章%')) OR p_text LIKE '%骑缝章%' THEN
    hits := hits || ARRAY['confirmation'];
  END IF;
  IF p_text LIKE '%支持材料%' AND p_text NOT LIKE '%回执%' AND p_text NOT LIKE '%保函%' AND p_text NOT LIKE '%授权%' AND p_text NOT LIKE '%保证金%' THEN
    RETURN jsonb_build_object('status','review','reason','MISSING_NAMED_PROCEDURAL_ATTACHMENT');
  END IF;
  IF NOT ('bid_bond' = ANY(hits) OR 'authorization_support' = ANY(hits))
     AND (p_text LIKE '%回执%' OR p_text LIKE '%上传成功%' OR p_text LIKE '%加密回执%' OR p_text LIKE '%递交回执%'
     OR ((p_text LIKE '%支持材料%' OR p_text LIKE '%程序附件%') AND (p_text LIKE '%提交%' OR p_text LIKE '%上传%'))) THEN
    hits := hits || ARRAY['procedural_support'];
  ELSIF (p_text LIKE '%签字%' OR p_text LIKE '%盖章%' OR p_text LIKE '%密封%' OR p_text LIKE '%线下递交%' OR p_text LIKE '%现场递交%')
        AND NOT ('confirmation' = ANY(hits)) THEN
    hits := hits || ARRAY['confirmation'];
  END IF;
  IF array_length(hits,1) IS NULL THEN
    RETURN jsonb_build_object('status','review','reason','MISSING_PROCEDURAL_REQUIREMENT');
  END IF;
  IF array_length(hits,1) > 1 THEN
    RETURN jsonb_build_object('status','review','reason','MULTIPLE_PROCEDURAL_REQUIREMENTS');
  END IF;
  RETURN jsonb_build_object('status','classified','kind',hits[1]);
END
$$;

CREATE FUNCTION kb_bid_register_procedural_router_contract(
 p_version text,p_canonical_payload bytea,p_content_sha256 kb_sha256,p_actor kb_actor_identity,
 p_idempotency_key text,p_request_bytes bytea,p_request_sha256 kb_sha256
)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE replay bytea; payload jsonb; response jsonb;
BEGIN
  PERFORM 1 FROM application_maintenance_gate WHERE singleton_key AND mode='maintenance' FOR SHARE;
  IF NOT FOUND THEN RAISE EXCEPTION 'MAINTENANCE_REQUIRED' USING ERRCODE='55000'; END IF;
  replay:=kb_bid_idempotency_begin(p_actor,'bid.procedural_router.register',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  IF p_content_sha256<>encode(public.digest(p_canonical_payload,'sha256'),'hex') THEN
    RAISE EXCEPTION 'PROCEDURAL_ROUTER_HASH_MISMATCH' USING ERRCODE='22023'; END IF;
  BEGIN payload:=convert_from(p_canonical_payload,'UTF8')::jsonb;
  EXCEPTION WHEN OTHERS THEN RAISE EXCEPTION 'PROCEDURAL_ROUTER_PAYLOAD_INVALID' USING ERRCODE='22023'; END;
  IF payload->>'version'<>p_version OR payload->>'schema_version'<>'1'
     OR jsonb_typeof(payload->'overrides')<>'object'
     OR EXISTS (
       SELECT 1 FROM jsonb_each(payload->'overrides') override_value
        WHERE jsonb_typeof(override_value.value)<>'object'
          OR override_value.value->>'status' NOT IN ('classified','review')
          OR (override_value.value->>'status'='classified' AND
              override_value.value->>'kind' NOT IN ('bid_bond','authorization_support','seal_sample','procedural_support','confirmation'))
          OR (override_value.value->>'status'='review' AND COALESCE(override_value.value->>'reason','')='')
     ) THEN RAISE EXCEPTION 'PROCEDURAL_ROUTER_CONTRACT_SCHEMA_INVALID' USING ERRCODE='22023'; END IF;
  INSERT INTO procedural_router_contract_artifacts(version,schema_version,canonical_payload,content_sha256)
  VALUES(p_version,1,p_canonical_payload,p_content_sha256);
  response:=jsonb_build_object('version',p_version,'content_sha256',p_content_sha256);
  INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
    entity_kind,entity_locator,after_revision,after_sha256)
  VALUES(gen_random_uuid(),1,'bid.procedural_router.register',p_actor,p_idempotency_key,p_request_sha256,
    encode(public.digest(convert_to(response::text,'UTF8'),'sha256'),'hex'),'procedural_router_contract',
    jsonb_build_object('version',p_version),1,p_content_sha256);
  PERFORM kb_bid_idempotency_complete(p_actor,'bid.procedural_router.register',p_idempotency_key,201,convert_to(response::text,'UTF8'));
  RETURN response;
END
$$;

CREATE FUNCTION kb_bid_promote_procedural_router(
 p_target_version text,p_expected_current_version text,p_expected_promotion_generation bigint,
 p_actor kb_actor_identity,p_idempotency_key text,p_request_bytes bytea,p_request_sha256 kb_sha256
)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE gate_value application_maintenance_gate%ROWTYPE; current_value procedural_router_current%ROWTYPE;
 replay bytea; project_value record; class_value bid_procedural_classification_artifacts%ROWTYPE;
 decision_value bid_procedural_decision_artifacts%ROWTYPE; segment_value bid_procedural_segment_artifacts%ROWTYPE;
 routed jsonb; target_generation bigint; next_class_id uuid; next_decision_id uuid; next_effective text;
 preserve_decision boolean; changed_count bigint:=0; blocked_count bigint:=0; project_changed bigint; response jsonb;
BEGIN
  SELECT * INTO STRICT gate_value FROM application_maintenance_gate WHERE singleton_key FOR UPDATE;
  IF gate_value.mode<>'maintenance' THEN RAISE EXCEPTION 'MAINTENANCE_REQUIRED' USING ERRCODE='55000'; END IF;
  SELECT * INTO STRICT current_value FROM procedural_router_current WHERE singleton_key FOR UPDATE;
  replay:=kb_bid_idempotency_begin(p_actor,'bid.procedural_router.promote',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  IF current_value.version<>p_expected_current_version
     OR current_value.promotion_generation<>p_expected_promotion_generation THEN
    RAISE EXCEPTION 'PROCEDURAL_ROUTER_PROMOTION_CAS_MISMATCH' USING ERRCODE='40001'; END IF;
  PERFORM 1 FROM procedural_router_contract_artifacts WHERE version=p_target_version FOR SHARE;
  IF NOT FOUND THEN RAISE EXCEPTION 'PROCEDURAL_ROUTER_TARGET_MISSING' USING ERRCODE='23503'; END IF;
  target_generation:=current_value.promotion_generation+1;
  FOR project_value IN SELECT id FROM bid_projects WHERE status='open' ORDER BY id FOR UPDATE LOOP
    project_changed:=0;
    FOR class_value IN
      SELECT classification.* FROM bid_procedural_classification_artifacts classification
       JOIN bid_procedural_segment_artifacts segment ON segment.id=classification.segment_id
       WHERE classification.project_id=project_value.id AND classification.lifecycle_status='current'
       ORDER BY segment.stable_key FOR UPDATE OF classification
    LOOP
      SELECT * INTO STRICT segment_value FROM bid_procedural_segment_artifacts WHERE id=class_value.segment_id FOR SHARE;
      routed:=kb_bid_route_procedural(convert_from(segment_value.segment_utf8,'UTF8'),p_target_version);
      next_effective:=CASE WHEN class_value.override_actor IS NOT NULL THEN class_value.effective_requirement_kind
                           ELSE routed->>'kind' END;
      next_class_id:=gen_random_uuid();
      UPDATE bid_procedural_classification_artifacts SET lifecycle_status='superseded',successor_id=next_class_id
       WHERE id=class_value.id;
      INSERT INTO bid_procedural_classification_artifacts(
        id,project_id,segment_id,revision,router_contract_version,router_promotion_generation,
        router_result_status,router_requirement_kind,review_reason,effective_requirement_kind,
        override_from,override_to,override_actor,override_reason,override_at,lifecycle_status)
      VALUES(next_class_id,class_value.project_id,class_value.segment_id,class_value.revision+1,
        p_target_version,target_generation,routed->>'status',routed->>'kind',routed->>'reason',next_effective,
        class_value.override_from,class_value.override_to,class_value.override_actor,
        class_value.override_reason,class_value.override_at,'current');
      SELECT * INTO decision_value FROM bid_procedural_decision_artifacts
       WHERE classification_id=class_value.id AND lifecycle_status='current' FOR UPDATE;
      IF decision_value.id IS NOT NULL THEN
        preserve_decision:=next_effective IS NOT DISTINCT FROM class_value.effective_requirement_kind
          AND (decision_value.resolution<>'satisfied_by_attachment' OR EXISTS(
            SELECT 1 FROM bid_procedural_attachments attachment
             WHERE attachment.id=decision_value.attachment_id AND attachment.project_id=project_value.id
               AND attachment.status='confirmed' AND attachment.validation_status='valid'
               AND attachment.kind=next_effective));
        IF preserve_decision THEN
          next_decision_id:=gen_random_uuid();
          UPDATE bid_procedural_decision_artifacts SET lifecycle_status='superseded',successor_id=next_decision_id
           WHERE id=decision_value.id;
          INSERT INTO bid_procedural_decision_artifacts(id,project_id,classification_id,revision,resolution,
            attachment_id,reason,actor_identity,decided_at,lifecycle_status)
          VALUES(next_decision_id,decision_value.project_id,next_class_id,1,decision_value.resolution,
            decision_value.attachment_id,decision_value.reason,decision_value.actor_identity,
            decision_value.decided_at,'current');
        ELSE
          UPDATE bid_procedural_decision_artifacts SET lifecycle_status='superseded',terminal_reason='router_promoted',
            terminal_at=clock_timestamp(),terminal_actor=p_actor WHERE id=decision_value.id;
          blocked_count:=blocked_count+1;
        END IF;
      ELSIF next_effective IS NULL THEN blocked_count:=blocked_count+1;
      END IF;
      changed_count:=changed_count+1;
      project_changed:=project_changed+1;
    END LOOP;
    IF project_changed>0 THEN
      PERFORM kb_bid_stale_parts(project_value.id,ARRAY['5','6:authorization','6:procedural'],'PROCEDURAL_ROUTER_PROMOTED');
    END IF;
  END LOOP;
  UPDATE procedural_router_current SET version=p_target_version,promotion_generation=target_generation
   WHERE singleton_key AND version=p_expected_current_version
     AND promotion_generation=p_expected_promotion_generation;
  IF NOT FOUND THEN RAISE EXCEPTION 'PROCEDURAL_ROUTER_PROMOTION_CAS_MISMATCH' USING ERRCODE='40001'; END IF;
  response:=jsonb_build_object('version',p_target_version,'promotion_generation',target_generation,
    'classification_count',changed_count,'blocked_decision_count',blocked_count);
  INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
    entity_kind,entity_locator,before_revision,before_sha256,after_revision,after_sha256)
  SELECT gen_random_uuid(),1,'bid.procedural_router.promote',p_actor,p_idempotency_key,p_request_sha256,
    encode(public.digest(convert_to(response::text,'UTF8'),'sha256'),'hex'),'procedural_router_current',
    jsonb_build_object('singleton',true),p_expected_promotion_generation,old_artifact.content_sha256,
    target_generation,new_artifact.content_sha256
  FROM procedural_router_contract_artifacts old_artifact,procedural_router_contract_artifacts new_artifact
  WHERE old_artifact.version=p_expected_current_version AND new_artifact.version=p_target_version;
  PERFORM kb_bid_idempotency_complete(p_actor,'bid.procedural_router.promote',p_idempotency_key,200,convert_to(response::text,'UTF8'));
  RETURN response;
END
$$;

CREATE FUNCTION kb_bid_register_template_contract(
 p_slot text,p_version text,p_canonical_payload bytea,p_content_sha256 kb_sha256,p_actor kb_actor_identity,
 p_idempotency_key text,p_request_bytes bytea,p_request_sha256 kb_sha256
)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE replay bytea; payload jsonb; response jsonb;
BEGIN
  PERFORM 1 FROM application_maintenance_gate WHERE singleton_key AND mode='maintenance' FOR SHARE;
  IF NOT FOUND THEN RAISE EXCEPTION 'MAINTENANCE_REQUIRED' USING ERRCODE='55000'; END IF;
  replay:=kb_bid_idempotency_begin(p_actor,'bid.template_contract.register',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  IF p_content_sha256<>encode(public.digest(p_canonical_payload,'sha256'),'hex') THEN
    RAISE EXCEPTION 'TEMPLATE_CONTRACT_HASH_MISMATCH' USING ERRCODE='22023'; END IF;
  BEGIN payload:=convert_from(p_canonical_payload,'UTF8')::jsonb;
  EXCEPTION WHEN OTHERS THEN RAISE EXCEPTION 'TEMPLATE_CONTRACT_PAYLOAD_INVALID' USING ERRCODE='22023'; END;
  IF payload->>'schema_version'<>'1' OR payload->>'slot'<>p_slot OR payload->>'version'<>p_version THEN
    RAISE EXCEPTION 'TEMPLATE_CONTRACT_SCHEMA_INVALID' USING ERRCODE='22023'; END IF;
  INSERT INTO bid_template_contract_artifacts(slot,version,canonical_payload,content_sha256)
  VALUES(p_slot,p_version,p_canonical_payload,p_content_sha256);
  response:=jsonb_build_object('slot',p_slot,'version',p_version,'content_sha256',p_content_sha256);
  INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
    entity_kind,entity_locator,after_revision,after_sha256)
  VALUES(gen_random_uuid(),1,'bid.template_contract.register',p_actor,p_idempotency_key,p_request_sha256,
    encode(public.digest(convert_to(response::text,'UTF8'),'sha256'),'hex'),'bid_template_contract',
    jsonb_build_object('slot',p_slot,'version',p_version),1,p_content_sha256);
  PERFORM kb_bid_idempotency_complete(p_actor,'bid.template_contract.register',p_idempotency_key,201,convert_to(response::text,'UTF8'));
  RETURN response;
END
$$;

CREATE FUNCTION kb_bid_promote_template_contract(
 p_slot text,p_target_version text,p_expected_current_version text,p_expected_promotion_generation bigint,
 p_actor kb_actor_identity,p_idempotency_key text,p_request_bytes bytea,p_request_sha256 kb_sha256
)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE gate_value application_maintenance_gate%ROWTYPE; current_value bid_template_contract_current%ROWTYPE;
 replay bytea; project_value record; affected bigint:=0; project_affected bigint; target_generation bigint; response jsonb;
BEGIN
  SELECT * INTO STRICT gate_value FROM application_maintenance_gate WHERE singleton_key FOR UPDATE;
  IF gate_value.mode<>'maintenance' THEN RAISE EXCEPTION 'MAINTENANCE_REQUIRED' USING ERRCODE='55000'; END IF;
  SELECT * INTO STRICT current_value FROM bid_template_contract_current WHERE slot=p_slot FOR UPDATE;
  replay:=kb_bid_idempotency_begin(p_actor,'bid.template_contract.promote',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  IF current_value.version<>p_expected_current_version
     OR current_value.promotion_generation<>p_expected_promotion_generation THEN
    RAISE EXCEPTION 'TEMPLATE_CONTRACT_PROMOTION_CAS_MISMATCH' USING ERRCODE='40001'; END IF;
  PERFORM 1 FROM bid_template_contract_artifacts WHERE slot=p_slot AND version=p_target_version FOR SHARE;
  IF NOT FOUND THEN RAISE EXCEPTION 'TEMPLATE_CONTRACT_TARGET_MISSING' USING ERRCODE='23503'; END IF;
  target_generation:=current_value.promotion_generation+1;
  FOR project_value IN SELECT id FROM bid_projects WHERE status='open' ORDER BY id FOR UPDATE LOOP
    UPDATE bid_current_parts current_part SET stale=true,
      stale_reason_codes=(SELECT ARRAY(SELECT DISTINCT reason FROM unnest(
        current_part.stale_reason_codes||ARRAY['TEMPLATE_CONTRACT_PROMOTED']) reason ORDER BY reason))
      FROM bid_part_dependency_artifacts dependency
     WHERE current_part.project_id=project_value.id
       AND dependency.id=current_part.dependency_artifact_id
       AND dependency.template_slot=p_slot AND dependency.template_version=p_expected_current_version;
    GET DIAGNOSTICS project_affected=ROW_COUNT;
    affected:=affected+project_affected;
  END LOOP;
  UPDATE bid_template_contract_current SET version=p_target_version,promotion_generation=target_generation
   WHERE slot=p_slot AND version=p_expected_current_version
     AND promotion_generation=p_expected_promotion_generation;
  IF NOT FOUND THEN RAISE EXCEPTION 'TEMPLATE_CONTRACT_PROMOTION_CAS_MISMATCH' USING ERRCODE='40001'; END IF;
  response:=jsonb_build_object('slot',p_slot,'version',p_target_version,
    'promotion_generation',target_generation,'stale_part_count',affected);
  INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
    entity_kind,entity_locator,before_revision,before_sha256,after_revision,after_sha256)
  SELECT gen_random_uuid(),1,'bid.template_contract.promote',p_actor,p_idempotency_key,p_request_sha256,
    encode(public.digest(convert_to(response::text,'UTF8'),'sha256'),'hex'),'bid_template_contract_current',
    jsonb_build_object('slot',p_slot),p_expected_promotion_generation,old_artifact.content_sha256,
    target_generation,new_artifact.content_sha256
  FROM bid_template_contract_artifacts old_artifact,bid_template_contract_artifacts new_artifact
  WHERE old_artifact.slot=p_slot AND old_artifact.version=p_expected_current_version
    AND new_artifact.slot=p_slot AND new_artifact.version=p_target_version;
  PERFORM kb_bid_idempotency_complete(p_actor,'bid.template_contract.promote',p_idempotency_key,200,convert_to(response::text,'UTF8'));
  RETURN response;
END
$$;

CREATE FUNCTION kb_bid_utf8_char_at(p_bytes bytea, p_offset integer)
RETURNS text
LANGUAGE sql
IMMUTABLE
STRICT
SET search_path = pg_catalog
AS $$ SELECT substr(convert_from(substring(p_bytes from p_offset+1), 'UTF8'), 1, 1) $$;

CREATE FUNCTION kb_bid_utf8_char_len(p_bytes bytea, p_offset integer)
RETURNS integer
LANGUAGE sql
IMMUTABLE
STRICT
SET search_path = pg_catalog
AS $$ SELECT octet_length(convert_to(substr(convert_from(substring(p_bytes from p_offset+1), 'UTF8'), 1, 1), 'UTF8')) $$;

CREATE FUNCTION kb_bid_trim_utf8_ws(p_bytes bytea, p_start integer, p_end integer, OUT o_start integer, OUT o_end integer)
LANGUAGE plpgsql
IMMUTABLE
SET search_path = pg_catalog
AS $$
DECLARE ch text; ch_len integer;
BEGIN
  o_start := p_start; o_end := p_end;
  WHILE o_start < o_end LOOP
    ch := public.kb_bid_utf8_char_at(p_bytes, o_start);
    EXIT WHEN ch !~ '^[[:space:]]$';
    o_start := o_start + public.kb_bid_utf8_char_len(p_bytes, o_start);
  END LOOP;
  WHILE o_end > o_start LOOP
    ch_len := 1;
    WHILE o_end - ch_len >= o_start AND get_byte(p_bytes, o_end - ch_len) BETWEEN 128 AND 191 LOOP
      ch_len := ch_len + 1;
    END LOOP;
    ch := convert_from(substring(p_bytes from o_end - ch_len + 1 for ch_len), 'UTF8');
    EXIT WHEN ch !~ '^[[:space:]]$';
    o_end := o_end - ch_len;
  END LOOP;
END
$$;

CREATE FUNCTION kb_bid_split_procedural_segments(p_text text)
RETURNS TABLE(start_offset bigint, end_offset bigint, segment_utf8 bytea)
LANGUAGE plpgsql
IMMUTABLE
SET search_path = pg_catalog
AS $$
DECLARE bytes bytea := convert_to(p_text,'UTF8'); pos int := 0; last int := 0; ch text; ch_len int;
 trimmed record; emitted int := 0; remaining text; numbered_boundary boolean;
BEGIN
  IF p_text IS NULL OR p_text = '' THEN RETURN; END IF;
  WHILE pos < octet_length(bytes) LOOP
    ch := public.kb_bid_utf8_char_at(bytes, pos);
    ch_len := public.kb_bid_utf8_char_len(bytes, pos);
    remaining:=convert_from(substring(bytes from pos+1),'UTF8');
    numbered_boundary:=pos>last
      AND remaining~'^([0-9]{1,3}([.][[:space:]]+|[)、][[:space:]]*)|（[0-9一二三四五六七八九十]{1,4}）[[:space:]]*|[一二三四五六七八九十]{1,4}、[[:space:]]*)';
    IF numbered_boundary THEN
      SELECT * INTO trimmed FROM public.kb_bid_trim_utf8_ws(bytes,last,pos);
      IF trimmed.o_end>trimmed.o_start THEN
        emitted:=emitted+1;
        IF emitted>1024 THEN RAISE EXCEPTION 'PROCEDURAL_SEGMENT_LIMIT' USING ERRCODE='22023'; END IF;
        RETURN QUERY SELECT trimmed.o_start::bigint,trimmed.o_end::bigint,
          substring(bytes from trimmed.o_start+1 for trimmed.o_end-trimmed.o_start);
      END IF;
      last:=pos;
    END IF;
    IF ch IN ('。','；','！','？', E'\n') THEN
      SELECT * INTO trimmed FROM public.kb_bid_trim_utf8_ws(bytes, last, pos + ch_len);
      IF trimmed.o_end > trimmed.o_start THEN
        emitted := emitted + 1;
        IF emitted > 1024 THEN RAISE EXCEPTION 'PROCEDURAL_SEGMENT_LIMIT' USING ERRCODE='22023'; END IF;
        RETURN QUERY SELECT trimmed.o_start::bigint, trimmed.o_end::bigint, substring(bytes from trimmed.o_start+1 for trimmed.o_end-trimmed.o_start);
      END IF;
      last := pos + ch_len;
    END IF;
    pos := pos + ch_len;
  END LOOP;
  IF last < octet_length(bytes) THEN
    SELECT * INTO trimmed FROM public.kb_bid_trim_utf8_ws(bytes, last, octet_length(bytes));
    IF trimmed.o_end > trimmed.o_start THEN
      emitted := emitted + 1;
      IF emitted > 1024 THEN RAISE EXCEPTION 'PROCEDURAL_SEGMENT_LIMIT' USING ERRCODE='22023'; END IF;
      RETURN QUERY SELECT trimmed.o_start::bigint, trimmed.o_end::bigint, substring(bytes from trimmed.o_start+1 for trimmed.o_end-trimmed.o_start);
    END IF;
  END IF;
END
$$;

CREATE FUNCTION kb_bid_stable_segment_key(p_clause_id uuid, p_start bigint, p_end bigint, p_bytes bytea)
RETURNS kb_sha256
LANGUAGE sql
IMMUTABLE
SET search_path = pg_catalog, public
AS $$
  SELECT encode(public.digest(convert_to(
    'ProceduralSegmentV1:'||p_clause_id::text||':procedural-segment-v1:'||p_start||':'||p_end||':'||encode(public.digest(p_bytes,'sha256'),'hex'),
    'UTF8'),'sha256'),'hex')
$$;

CREATE FUNCTION kb_bid_procedural_segments_for_clause(p_clause bid_clauses)
RETURNS TABLE(start_offset bigint,end_offset bigint,segment_utf8 bytea,stable_key kb_sha256)
LANGUAGE plpgsql
STABLE
SET search_path = pg_catalog, public
AS $$
DECLARE span_value bid_source_span_artifacts%ROWTYPE; segment record;
BEGIN
  IF p_clause.provenance='extracted' THEN
    SELECT * INTO STRICT span_value FROM bid_source_span_artifacts
     WHERE id=p_clause.current_source_span_artifact_id AND project_id=p_clause.project_id;
    IF span_value.quote<>p_clause.text OR span_value.quote_sha256<>encode(public.digest(convert_to(p_clause.text,'UTF8'),'sha256'),'hex') THEN
      RAISE EXCEPTION 'PROCEDURAL_SOURCE_SPAN_NOT_CURRENT' USING ERRCODE='23514';
    END IF;
    RETURN QUERY SELECT 0::bigint,octet_length(convert_to(p_clause.text,'UTF8'))::bigint,
      convert_to(p_clause.text,'UTF8'),span_value.content_sha256;
    RETURN;
  END IF;
  FOR segment IN SELECT * FROM kb_bid_split_procedural_segments(p_clause.text) LOOP
    RETURN QUERY SELECT segment.start_offset,segment.end_offset,segment.segment_utf8,
      kb_bid_stable_segment_key(p_clause.id,segment.start_offset,segment.end_offset,segment.segment_utf8);
  END LOOP;
END
$$;

CREATE FUNCTION kb_bid_sync_procedural_clause(
  p_project_id uuid, p_clause bid_clauses, p_terminal text, p_actor kb_actor_identity
)
RETURNS void
LANGUAGE plpgsql
SET search_path = pg_catalog, public
AS $$
DECLARE seg record; routed jsonb; class_id uuid; existing bid_procedural_classification_artifacts%ROWTYPE;
 current_router procedural_router_current%ROWTYPE;
 keep text[] := '{}'; now_ts timestamptz := clock_timestamp();
BEGIN
  SELECT * INTO STRICT current_router FROM procedural_router_current WHERE singleton_key FOR SHARE;
  IF p_clause.kind<>'procedural' OR p_clause.status<>'confirmed' OR p_terminal IS NOT NULL THEN
    UPDATE bid_procedural_classification_artifacts SET lifecycle_status='superseded',
      terminal_reason=COALESCE(p_terminal,'clause_unconfirmed'), terminal_at=now_ts, terminal_actor=p_actor
     WHERE project_id=p_project_id AND lifecycle_status='current'
       AND segment_id IN (SELECT id FROM bid_procedural_segment_artifacts WHERE clause_id=p_clause.id);
    UPDATE bid_procedural_decision_artifacts SET lifecycle_status='superseded',
      terminal_reason=COALESCE(p_terminal,'clause_unconfirmed'),terminal_at=now_ts,terminal_actor=p_actor
     WHERE project_id=p_project_id AND lifecycle_status='current'
       AND classification_id IN (SELECT id FROM bid_procedural_classification_artifacts
         WHERE segment_id IN (SELECT id FROM bid_procedural_segment_artifacts WHERE clause_id=p_clause.id));
    RETURN;
  END IF;
  FOR seg IN SELECT * FROM kb_bid_procedural_segments_for_clause(p_clause) LOOP
    keep := keep || seg.stable_key;
    INSERT INTO bid_procedural_segment_artifacts(id,project_id,clause_id,stable_key,segmentation_version,start_offset,end_offset,
      segment_utf8,segment_sha256,provenance)
    VALUES(gen_random_uuid(),p_project_id,p_clause.id,
      seg.stable_key,
      'procedural-segment-v1',seg.start_offset,seg.end_offset,seg.segment_utf8,
      encode(public.digest(seg.segment_utf8,'sha256'),'hex'), p_clause.provenance)
    ON CONFLICT (project_id, stable_key) DO NOTHING;
    SELECT * INTO existing FROM bid_procedural_classification_artifacts
     WHERE segment_id=(SELECT id FROM bid_procedural_segment_artifacts
        WHERE project_id=p_project_id AND stable_key=seg.stable_key)
       AND lifecycle_status='current';
    IF existing.id IS NULL THEN
      routed := kb_bid_route_procedural(convert_from(seg.segment_utf8,'UTF8'),current_router.version);
      class_id := gen_random_uuid();
      INSERT INTO bid_procedural_classification_artifacts(id,project_id,segment_id,revision,
        router_contract_version,router_promotion_generation,router_result_status,
        router_requirement_kind,review_reason,effective_requirement_kind,lifecycle_status)
      VALUES(class_id,p_project_id,
        (SELECT id FROM bid_procedural_segment_artifacts WHERE project_id=p_project_id
          AND stable_key=seg.stable_key),
        1,current_router.version,current_router.promotion_generation,
        routed->>'status', routed->>'kind', routed->>'reason',
        routed->>'kind', 'current');
    END IF;
  END LOOP;
  UPDATE bid_procedural_classification_artifacts SET lifecycle_status='superseded',
    terminal_reason='segment_removed', terminal_at=now_ts, terminal_actor=p_actor
   WHERE project_id=p_project_id AND lifecycle_status='current'
     AND segment_id IN (SELECT id FROM bid_procedural_segment_artifacts WHERE clause_id=p_clause.id AND NOT (stable_key = ANY(keep)));
  UPDATE bid_procedural_decision_artifacts SET lifecycle_status='superseded',
    terminal_reason='segment_removed',terminal_at=now_ts,terminal_actor=p_actor
   WHERE lifecycle_status='current' AND classification_id IN (
     SELECT id FROM bid_procedural_classification_artifacts WHERE lifecycle_status='superseded' AND terminal_reason='segment_removed'
       AND segment_id IN (SELECT id FROM bid_procedural_segment_artifacts WHERE clause_id=p_clause.id));
END
$$;

CREATE FUNCTION kb_bid_sync_project_procedural(p_project_id uuid, p_actor kb_actor_identity)
RETURNS void
LANGUAGE plpgsql
SET search_path = pg_catalog, public
AS $$
DECLARE clause_row bid_clauses%ROWTYPE; payload text; digest kb_sha256;
BEGIN
  FOR clause_row IN SELECT * FROM bid_clauses WHERE project_id=p_project_id LOOP
    IF clause_row.kind='procedural' AND clause_row.status='confirmed' THEN
      PERFORM kb_bid_sync_procedural_clause(p_project_id, clause_row, NULL, p_actor);
    ELSE
      PERFORM kb_bid_sync_procedural_clause(p_project_id, clause_row,
        CASE WHEN clause_row.status='superseded' THEN 'clause_deleted'
             WHEN clause_row.kind<>'procedural' THEN 'left_procedural'
             ELSE 'clause_unconfirmed' END, p_actor);
    END IF;
  END LOOP;
  SELECT encode(public.digest(convert_to('ProceduralSegmentSetV1:'
      ||COALESCE(string_agg(stable_key, E'\n' ORDER BY stable_key),''),'UTF8'),'sha256'),'hex')
    INTO digest
    FROM (
      SELECT segment.stable_key
        FROM bid_clauses clause
        CROSS JOIN LATERAL kb_bid_procedural_segments_for_clause(clause) segment
       WHERE clause.project_id=p_project_id AND clause.status='confirmed' AND clause.kind='procedural'
    ) current_segments;
  INSERT INTO bid_procedural_segment_sets(project_id,revision,content_sha256,updated_at)
    VALUES(p_project_id,1,digest,clock_timestamp())
  ON CONFLICT (project_id) DO UPDATE SET revision=bid_procedural_segment_sets.revision+1,
    content_sha256=EXCLUDED.content_sha256, updated_at=clock_timestamp()
    WHERE bid_procedural_segment_sets.content_sha256<>EXCLUDED.content_sha256;
  IF FOUND THEN
    PERFORM kb_bid_stale_parts(p_project_id, ARRAY['5','6:authorization','6:procedural'], 'PROCEDURAL_SET_CHANGED');
  END IF;
END
$$;

CREATE FUNCTION kb_bid_update_company_profile(
  p_project_id uuid, p_expected_revision bigint, p_legal_name text, p_uscc text, p_address text,
  p_legal_rep text, p_contact_name text, p_contact_phone text, p_contact_email text,
  p_actor kb_actor_identity, p_idempotency_key text, p_request_bytes bytea, p_request_sha256 kb_sha256
)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE replay bytea; project_value bid_projects%ROWTYPE; current_rev bigint := 0; artifact_id uuid := gen_random_uuid();
 payload bytea; digest kb_sha256; response jsonb;
BEGIN
  PERFORM kb_bid_require_human_actor(p_actor);
  replay := kb_bid_idempotency_begin(p_actor,'bid.profile.company.update',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  SELECT * INTO STRICT project_value FROM bid_projects WHERE id=p_project_id FOR UPDATE;
  IF project_value.status<>'open' THEN RAISE EXCEPTION 'PROJECT_ENDED' USING ERRCODE='55000'; END IF;
  IF btrim(p_legal_name)='' OR btrim(p_uscc)='' OR btrim(p_address)='' OR btrim(p_legal_rep)=''
     OR btrim(p_contact_name)='' OR btrim(p_contact_phone)='' OR btrim(p_contact_email)='' THEN
    RAISE EXCEPTION 'PROFILE_FIELD_MISSING' USING ERRCODE='22023';
  END IF;
  INSERT INTO bid_current_profiles(project_id) VALUES(p_project_id) ON CONFLICT DO NOTHING;
  SELECT COALESCE(artifact.revision,0) INTO current_rev
    FROM bid_current_profiles cur LEFT JOIN bid_company_profile_artifacts artifact ON artifact.id=cur.company_profile_id
   WHERE cur.project_id=p_project_id FOR UPDATE OF cur;
  IF current_rev<>p_expected_revision THEN RAISE EXCEPTION 'PROFILE_REVISION_CAS_MISMATCH' USING ERRCODE='40001'; END IF;
  payload := convert_to('{"schema_version":1,"legal_name":'||kb_bid_json_string(btrim(p_legal_name))
    ||',"unified_social_credit_code":'||kb_bid_json_string(btrim(p_uscc))
    ||',"registered_address":'||kb_bid_json_string(btrim(p_address))
    ||',"legal_representative":'||kb_bid_json_string(btrim(p_legal_rep))
    ||',"contact_name":'||kb_bid_json_string(btrim(p_contact_name))
    ||',"contact_phone":'||kb_bid_json_string(btrim(p_contact_phone))
    ||',"contact_email":'||kb_bid_json_string(btrim(p_contact_email))||'}','UTF8');
  digest := encode(public.digest(payload,'sha256'),'hex');
  INSERT INTO bid_company_profile_artifacts(id,project_id,revision,canonical_payload,content_sha256,legal_name,
    unified_social_credit_code,registered_address,legal_representative,contact_name,contact_phone,contact_email,created_by,created_at)
  VALUES(artifact_id,p_project_id,current_rev+1,payload,digest,btrim(p_legal_name),btrim(p_uscc),btrim(p_address),
    btrim(p_legal_rep),btrim(p_contact_name),btrim(p_contact_phone),btrim(p_contact_email),p_actor,clock_timestamp());
  UPDATE bid_current_profiles SET company_profile_id=artifact_id WHERE project_id=p_project_id;
  PERFORM kb_bid_stale_parts(p_project_id, ARRAY['1','6:letter','6:authorization'], 'COMPANY_PROFILE_CHANGED');
  response := jsonb_build_object('id',artifact_id,'revision',current_rev+1,'content_sha256',digest);
  INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
    entity_kind,entity_locator,after_revision,after_sha256)
  VALUES(gen_random_uuid(),1,'bid.profile.company.update',p_actor,p_idempotency_key,p_request_sha256,
    encode(public.digest(convert_to(response::text,'UTF8'),'sha256'),'hex'),'bid_company_profile',
    jsonb_build_object('project_id',p_project_id),current_rev+1,digest);
  PERFORM kb_bid_idempotency_complete(p_actor,'bid.profile.company.update',p_idempotency_key,200,convert_to(response::text,'UTF8'));
  RETURN response;
END
$$;

CREATE FUNCTION kb_bid_update_submission_profile(
  p_project_id uuid, p_expected_revision bigint, p_buyer_name text, p_project_code text,
  p_authorized_representative text, p_submission_date date, p_submission_place text,
  p_seal_confirmed boolean, p_signature_confirmed boolean,
  p_actor kb_actor_identity, p_idempotency_key text, p_request_bytes bytea, p_request_sha256 kb_sha256
)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE replay bytea; project_value bid_projects%ROWTYPE; current_rev bigint := 0; artifact_id uuid := gen_random_uuid();
 payload bytea; digest kb_sha256; response jsonb; date_text text;
BEGIN
  PERFORM kb_bid_require_human_actor(p_actor);
  replay := kb_bid_idempotency_begin(p_actor,'bid.profile.submission.update',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  SELECT * INTO STRICT project_value FROM bid_projects WHERE id=p_project_id FOR UPDATE;
  IF project_value.status<>'open' THEN RAISE EXCEPTION 'PROJECT_ENDED' USING ERRCODE='55000'; END IF;
  IF btrim(p_buyer_name)='' OR btrim(p_project_code)='' OR btrim(p_authorized_representative)=''
     OR p_submission_date IS NULL OR btrim(p_submission_place)='' THEN
    RAISE EXCEPTION 'PROFILE_FIELD_MISSING' USING ERRCODE='22023';
  END IF;
  INSERT INTO bid_current_profiles(project_id) VALUES(p_project_id) ON CONFLICT DO NOTHING;
  SELECT COALESCE(artifact.revision,0) INTO current_rev
    FROM bid_current_profiles cur LEFT JOIN bid_submission_profile_artifacts artifact ON artifact.id=cur.submission_profile_id
   WHERE cur.project_id=p_project_id FOR UPDATE OF cur;
  IF current_rev<>p_expected_revision THEN RAISE EXCEPTION 'PROFILE_REVISION_CAS_MISMATCH' USING ERRCODE='40001'; END IF;
  date_text := to_char(p_submission_date,'YYYY-MM-DD');
  payload := convert_to('{"schema_version":1,"buyer_name":'||kb_bid_json_string(btrim(p_buyer_name))
    ||',"project_code":'||kb_bid_json_string(btrim(p_project_code))
    ||',"authorized_representative":'||kb_bid_json_string(btrim(p_authorized_representative))
    ||',"submission_date":'||kb_bid_json_string(date_text)
    ||',"submission_place":'||kb_bid_json_string(btrim(p_submission_place))
    ||',"seal_confirmed":'||CASE WHEN p_seal_confirmed THEN 'true' ELSE 'false' END
    ||',"signature_confirmed":'||CASE WHEN p_signature_confirmed THEN 'true' ELSE 'false' END||'}','UTF8');
  digest := encode(public.digest(payload,'sha256'),'hex');
  INSERT INTO bid_submission_profile_artifacts(id,project_id,revision,canonical_payload,content_sha256,buyer_name,project_code,
    authorized_representative,submission_date,submission_place,seal_confirmed,signature_confirmed,created_by,created_at)
  VALUES(artifact_id,p_project_id,current_rev+1,payload,digest,btrim(p_buyer_name),btrim(p_project_code),
    btrim(p_authorized_representative),p_submission_date,btrim(p_submission_place),p_seal_confirmed,p_signature_confirmed,
    p_actor,clock_timestamp());
  UPDATE bid_current_profiles SET submission_profile_id=artifact_id WHERE project_id=p_project_id;
  PERFORM kb_bid_stale_parts(p_project_id, ARRAY['1','6:letter','6:authorization'], 'SUBMISSION_PROFILE_CHANGED');
  response := jsonb_build_object('id',artifact_id,'revision',current_rev+1,'content_sha256',digest);
  INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
    entity_kind,entity_locator,after_revision,after_sha256)
  VALUES(gen_random_uuid(),1,'bid.profile.submission.update',p_actor,p_idempotency_key,p_request_sha256,
    encode(public.digest(convert_to(response::text,'UTF8'),'sha256'),'hex'),'bid_submission_profile',
    jsonb_build_object('project_id',p_project_id),current_rev+1,digest);
  PERFORM kb_bid_idempotency_complete(p_actor,'bid.profile.submission.update',p_idempotency_key,200,convert_to(response::text,'UTF8'));
  RETURN response;
END
$$;

CREATE FUNCTION kb_bid_override_procedural_classification(
  p_project_id uuid, p_classification_id uuid, p_effective_kind text, p_reason text,
  p_actor kb_actor_identity, p_idempotency_key text, p_request_bytes bytea, p_request_sha256 kb_sha256
)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE replay bytea; project_value bid_projects%ROWTYPE; current_row bid_procedural_classification_artifacts%ROWTYPE;
 new_id uuid := gen_random_uuid(); response jsonb;
BEGIN
  PERFORM kb_bid_require_human_actor(p_actor);
  replay := kb_bid_idempotency_begin(p_actor,'bid.procedural.override',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  SELECT * INTO STRICT project_value FROM bid_projects WHERE id=p_project_id FOR UPDATE;
  IF project_value.status<>'open' THEN RAISE EXCEPTION 'PROJECT_ENDED' USING ERRCODE='55000'; END IF;
  IF p_effective_kind NOT IN ('bid_bond','authorization_support','seal_sample','procedural_support','confirmation')
     OR p_reason IS NULL OR octet_length(btrim(p_reason)) NOT BETWEEN 1 AND 512 THEN
    RAISE EXCEPTION 'PROCEDURAL_OVERRIDE_INVALID' USING ERRCODE='22023';
  END IF;
  SELECT * INTO STRICT current_row FROM bid_procedural_classification_artifacts WHERE id=p_classification_id FOR UPDATE;
  IF current_row.project_id<>p_project_id OR current_row.lifecycle_status<>'current' THEN
    RAISE EXCEPTION 'PROCEDURAL_CLASSIFICATION_NOT_CURRENT' USING ERRCODE='40001';
  END IF;
  UPDATE bid_procedural_classification_artifacts SET lifecycle_status='superseded', successor_id=new_id
   WHERE id=current_row.id;
  INSERT INTO bid_procedural_classification_artifacts(id,project_id,segment_id,revision,
    router_contract_version,router_promotion_generation,router_result_status,
    router_requirement_kind,review_reason,effective_requirement_kind,override_from,override_to,override_actor,
    override_reason,override_at,lifecycle_status)
  VALUES(new_id,p_project_id,current_row.segment_id,current_row.revision+1,
    current_row.router_contract_version,current_row.router_promotion_generation,current_row.router_result_status,
    current_row.router_requirement_kind,current_row.review_reason,p_effective_kind,
    current_row.effective_requirement_kind,p_effective_kind,p_actor,btrim(p_reason),clock_timestamp(),'current');
  UPDATE bid_procedural_decision_artifacts SET lifecycle_status='superseded',
    terminal_reason='resegmented',terminal_at=clock_timestamp(),terminal_actor=p_actor
   WHERE classification_id=current_row.id AND lifecycle_status='current';
  PERFORM kb_bid_stale_parts(p_project_id, ARRAY['5','6:authorization','6:procedural'], 'PROCEDURAL_OVERRIDE');
  response := jsonb_build_object('classification_id',new_id,'revision',current_row.revision+1,'effective_requirement_kind',p_effective_kind);
  INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
    entity_kind,entity_locator,after_revision,after_sha256)
  VALUES(gen_random_uuid(),1,'bid.procedural.override',p_actor,p_idempotency_key,p_request_sha256,
    encode(public.digest(convert_to(response::text,'UTF8'),'sha256'),'hex'),'bid_procedural_classification',
    jsonb_build_object('classification_id',new_id),current_row.revision+1,
    encode(public.digest(convert_to(response::text,'UTF8'),'sha256'),'hex'));
  PERFORM kb_bid_idempotency_complete(p_actor,'bid.procedural.override',p_idempotency_key,200,convert_to(response::text,'UTF8'));
  RETURN response;
END
$$;

CREATE FUNCTION kb_bid_resolve_procedural_requirement(
  p_project_id uuid, p_classification_id uuid, p_resolution text, p_attachment_id uuid, p_reason text,
  p_actor kb_actor_identity, p_idempotency_key text, p_request_bytes bytea, p_request_sha256 kb_sha256
)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE replay bytea; project_value bid_projects%ROWTYPE; class_row bid_procedural_classification_artifacts%ROWTYPE;
 attach bid_procedural_attachments%ROWTYPE; new_id uuid := gen_random_uuid(); next_rev int := 1; response jsonb;
BEGIN
  PERFORM kb_bid_require_human_actor(p_actor);
  replay := kb_bid_idempotency_begin(p_actor,'bid.procedural.resolve',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  SELECT * INTO STRICT project_value FROM bid_projects WHERE id=p_project_id FOR UPDATE;
  IF project_value.status<>'open' THEN RAISE EXCEPTION 'PROJECT_ENDED' USING ERRCODE='55000'; END IF;
  SELECT * INTO STRICT class_row FROM bid_procedural_classification_artifacts WHERE id=p_classification_id FOR UPDATE;
  IF class_row.project_id<>p_project_id OR class_row.lifecycle_status<>'current' THEN
    RAISE EXCEPTION 'PROCEDURAL_CLASSIFICATION_NOT_CURRENT' USING ERRCODE='40001';
  END IF;
  IF class_row.effective_requirement_kind IS NULL THEN RAISE EXCEPTION 'PROCEDURAL_EFFECTIVE_KIND_NULL' USING ERRCODE='22023'; END IF;
  IF p_resolution='confirmed_by_user' THEN
    IF class_row.effective_requirement_kind<>'confirmation' OR p_attachment_id IS NOT NULL THEN
      RAISE EXCEPTION 'PROCEDURAL_RESOLUTION_INVALID' USING ERRCODE='22023';
    END IF;
  ELSIF p_resolution='satisfied_by_attachment' THEN
    SELECT * INTO STRICT attach FROM bid_procedural_attachments WHERE id=p_attachment_id FOR UPDATE;
    IF attach.project_id<>p_project_id OR attach.validation_status<>'valid' OR attach.status<>'confirmed'
       OR attach.kind<>class_row.effective_requirement_kind THEN
      RAISE EXCEPTION 'ATTACHMENT_NOT_VALID' USING ERRCODE='22023';
    END IF;
  ELSIF p_resolution='not_applicable' THEN
    IF p_attachment_id IS NOT NULL OR p_reason IS NULL OR octet_length(btrim(p_reason)) NOT BETWEEN 1 AND 512 THEN
      RAISE EXCEPTION 'PROCEDURAL_RESOLUTION_INVALID' USING ERRCODE='22023';
    END IF;
  ELSE
    RAISE EXCEPTION 'PROCEDURAL_RESOLUTION_INVALID' USING ERRCODE='22023';
  END IF;
  SELECT COALESCE(max(revision),0)+1 INTO next_rev FROM bid_procedural_decision_artifacts WHERE classification_id=p_classification_id;
  UPDATE bid_procedural_decision_artifacts SET lifecycle_status='superseded', successor_id=new_id
   WHERE classification_id=p_classification_id AND lifecycle_status='current';
  INSERT INTO bid_procedural_decision_artifacts(id,project_id,classification_id,revision,resolution,attachment_id,reason,
    actor_identity,decided_at,lifecycle_status)
  VALUES(new_id,p_project_id,p_classification_id,next_rev,p_resolution,p_attachment_id,NULLIF(btrim(COALESCE(p_reason,'')),''),
    p_actor,clock_timestamp(),'current');
  PERFORM kb_bid_stale_parts(p_project_id, ARRAY['5','6:authorization','6:procedural'], 'PROCEDURAL_DECISION_CHANGED');
  response := jsonb_build_object('decision_id',new_id,'revision',next_rev,'resolution',p_resolution);
  INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
    entity_kind,entity_locator,after_revision,after_sha256)
  VALUES(gen_random_uuid(),1,'bid.procedural.resolve',p_actor,p_idempotency_key,p_request_sha256,
    encode(public.digest(convert_to(response::text,'UTF8'),'sha256'),'hex'),'bid_procedural_decision',
    jsonb_build_object('decision_id',new_id),next_rev,
    encode(public.digest(convert_to(response::text,'UTF8'),'sha256'),'hex'));
  PERFORM kb_bid_idempotency_complete(p_actor,'bid.procedural.resolve',p_idempotency_key,200,convert_to(response::text,'UTF8'));
  RETURN response;
END
$$;

CREATE FUNCTION kb_bid_upload_attachment(
  p_staging_id uuid, p_id uuid, p_project_id uuid, p_kind text, p_object_ref kb_object_ref, p_digest kb_sha256,
  p_media_type text, p_byte_length bigint, p_pixel_width integer, p_pixel_height integer,
  p_render_pages jsonb,
  p_actor kb_actor_identity, p_idempotency_key text, p_request_bytes bytea, p_request_sha256 kb_sha256
)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE replay bytea; project_value bid_projects%ROWTYPE; response jsonb; validation_payload jsonb;
 validation_digest kb_sha256; page jsonb; expected_page integer := 0; render_page_bytes bigint := 0;
BEGIN
  PERFORM kb_bid_require_human_actor(p_actor);
  replay := kb_bid_idempotency_begin(p_actor,'bid.attachment.upload',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN
    PERFORM kb_object_upload_abandon(p_staging_id,p_actor);
    IF jsonb_typeof(p_render_pages)='array' THEN
      FOR page IN SELECT value FROM jsonb_array_elements(p_render_pages) LOOP
        BEGIN
          PERFORM kb_object_upload_abandon((page->>'staging_id')::uuid,p_actor);
        EXCEPTION WHEN OTHERS THEN NULL;
        END;
      END LOOP;
    END IF;
    RETURN convert_from(replay,'UTF8')::jsonb;
  END IF;
  SELECT * INTO STRICT project_value FROM bid_projects WHERE id=p_project_id FOR UPDATE;
  IF project_value.status<>'open' THEN RAISE EXCEPTION 'PROJECT_ENDED' USING ERRCODE='55000'; END IF;
  IF p_kind NOT IN ('bid_bond','authorization_support','seal_sample','procedural_support') THEN
    RAISE EXCEPTION 'ATTACHMENT_KIND_INVALID' USING ERRCODE='22023';
  END IF;
  IF p_media_type NOT IN ('application/pdf','image/png','image/jpeg','image/webp')
     OR p_byte_length NOT BETWEEN 1 AND 20971520
     OR ((p_media_type LIKE 'image/%')<>(p_pixel_width IS NOT NULL AND p_pixel_height IS NOT NULL))
     OR (p_pixel_width IS NOT NULL AND (p_pixel_width NOT BETWEEN 1 AND 20000 OR p_pixel_height NOT BETWEEN 1 AND 20000)) THEN
    RAISE EXCEPTION 'ATTACHMENT_VALIDATION_INVALID' USING ERRCODE='22023';
  END IF;
  IF jsonb_typeof(p_render_pages)<>'array'
     OR (p_media_type='application/pdf' AND jsonb_array_length(p_render_pages) NOT BETWEEN 1 AND 512)
     OR (p_media_type<>'application/pdf' AND jsonb_array_length(p_render_pages)<>0) THEN
    RAISE EXCEPTION 'ATTACHMENT_RENDER_PAGE_SET_INVALID' USING ERRCODE='22023';
  END IF;
  FOR page IN SELECT value FROM jsonb_array_elements(p_render_pages) LOOP
    IF (SELECT array_agg(key ORDER BY key) FROM jsonb_object_keys(page) key)
         IS DISTINCT FROM ARRAY['byte_length','digest','media_type','object_ref','page_ordinal',
           'pixel_height','pixel_width','staging_id']::text[]
       OR (page->>'page_ordinal')::integer<>expected_page
       OR page->>'object_ref'<>'objects/'||(page->>'digest')
       OR page->>'media_type' NOT IN ('image/png','image/jpeg','image/webp')
       OR (page->>'byte_length')::bigint NOT BETWEEN 1 AND 20971520
       OR (page->>'pixel_width')::integer NOT BETWEEN 1 AND 20000
       OR (page->>'pixel_height')::integer NOT BETWEEN 1 AND 20000 THEN
      RAISE EXCEPTION 'ATTACHMENT_RENDER_PAGE_SET_INVALID' USING ERRCODE='22023';
    END IF;
    render_page_bytes := render_page_bytes + (page->>'byte_length')::bigint;
    expected_page := expected_page + 1;
  END LOOP;
  IF render_page_bytes>268435456 THEN
    RAISE EXCEPTION 'ATTACHMENT_RENDER_PAGE_QUOTA_EXCEEDED' USING ERRCODE='22023';
  END IF;
  validation_payload := jsonb_build_object('schema_version',1,'object_ref',p_object_ref,
    'digest',p_digest,'media_type',p_media_type,'byte_length',p_byte_length,
    'pixel_width',p_pixel_width,'pixel_height',p_pixel_height);
  validation_digest := encode(public.digest(convert_to(validation_payload::text,'UTF8'),'sha256'),'hex');
  PERFORM kb_object_upload_commit(p_staging_id,p_object_ref,p_digest,p_media_type,p_byte_length,
    'bid_attachment',p_id,'original',p_actor);
  INSERT INTO bid_procedural_attachments(id,project_id,kind,object_ref,content_sha256,media_type,byte_length,
    pixel_width,pixel_height,validation_sha256,validation_status,status,revision,created_by)
  VALUES(p_id,p_project_id,p_kind,p_object_ref,p_digest,p_media_type,p_byte_length,p_pixel_width,p_pixel_height,
    validation_digest,'pending','draft',1,p_actor);
  FOR page IN SELECT value FROM jsonb_array_elements(p_render_pages) ORDER BY (value->>'page_ordinal')::integer LOOP
    PERFORM kb_object_upload_commit((page->>'staging_id')::uuid,page->>'object_ref',page->>'digest',
      page->>'media_type',(page->>'byte_length')::bigint,'bid_attachment_page',p_id,
      page->>'page_ordinal',p_actor);
    INSERT INTO bid_attachment_render_pages(attachment_id,project_id,page_ordinal,object_ref,content_sha256,
      media_type,byte_length,pixel_width,pixel_height)
    VALUES(p_id,p_project_id,(page->>'page_ordinal')::integer,page->>'object_ref',page->>'digest',
      page->>'media_type',(page->>'byte_length')::bigint,(page->>'pixel_width')::integer,
      (page->>'pixel_height')::integer);
  END LOOP;
  PERFORM kb_bid_stale_parts(p_project_id, ARRAY['5','6:authorization','6:procedural'], 'ATTACHMENT_CHANGED');
  response := jsonb_build_object('id',p_id,'kind',p_kind,'validation_status','pending','status','draft',
    'revision',1,'render_page_count',expected_page);
  INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
    entity_kind,entity_locator,after_revision,after_sha256)
  VALUES(gen_random_uuid(),1,'bid.attachment.upload',p_actor,p_idempotency_key,p_request_sha256,
    encode(public.digest(convert_to(response::text,'UTF8'),'sha256'),'hex'),'bid_attachment',
    jsonb_build_object('attachment_id',p_id),1,
    encode(public.digest(convert_to(response::text,'UTF8'),'sha256'),'hex'));
  PERFORM kb_bid_idempotency_complete(p_actor,'bid.attachment.upload',p_idempotency_key,201,convert_to(response::text,'UTF8'));
  RETURN response;
END
$$;

CREATE FUNCTION kb_bid_mutate_attachment(
  p_project_id uuid, p_attachment_id uuid, p_action text, p_expected_revision integer, p_reason text,
  p_actor kb_actor_identity, p_idempotency_key text, p_request_bytes bytea, p_request_sha256 kb_sha256
)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE replay bytea; project_value bid_projects%ROWTYPE; attach bid_procedural_attachments%ROWTYPE; response jsonb;
BEGIN
  PERFORM kb_bid_require_human_actor(p_actor);
  replay := kb_bid_idempotency_begin(p_actor,'bid.attachment.'||p_action,p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  SELECT * INTO STRICT project_value FROM bid_projects WHERE id=p_project_id FOR UPDATE;
  IF project_value.status<>'open' THEN RAISE EXCEPTION 'PROJECT_ENDED' USING ERRCODE='55000'; END IF;
  SELECT * INTO STRICT attach FROM bid_procedural_attachments WHERE id=p_attachment_id FOR UPDATE;
  IF attach.project_id<>p_project_id OR attach.revision<>p_expected_revision THEN
    RAISE EXCEPTION 'ATTACHMENT_REVISION_CAS_MISMATCH' USING ERRCODE='40001';
  END IF;
  IF p_action='validate' THEN
    IF NOT EXISTS (SELECT 1 FROM available_object_registry registry
      WHERE registry.object_ref=attach.object_ref AND registry.digest=attach.content_sha256
        AND registry.media_type=attach.media_type AND registry.byte_length=attach.byte_length)
       OR attach.validation_sha256<>encode(public.digest(convert_to(jsonb_build_object(
         'schema_version',1,'object_ref',attach.object_ref,'digest',attach.content_sha256,
         'media_type',attach.media_type,'byte_length',attach.byte_length,
         'pixel_width',attach.pixel_width,'pixel_height',attach.pixel_height)::text,'UTF8'),'sha256'),'hex') THEN
      RAISE EXCEPTION 'ATTACHMENT_VALIDATION_IDENTITY_MISMATCH' USING ERRCODE='23514';
    END IF;
    IF (attach.media_type='application/pdf' AND (
          NOT EXISTS (SELECT 1 FROM bid_attachment_render_pages page WHERE page.attachment_id=attach.id)
          OR EXISTS (
            SELECT 1 FROM bid_attachment_render_pages page
            LEFT JOIN available_object_registry registry ON registry.object_ref=page.object_ref
             WHERE page.attachment_id=attach.id AND (
               registry.object_ref IS NULL OR registry.digest<>page.content_sha256
               OR registry.media_type<>page.media_type OR registry.byte_length<>page.byte_length
               OR NOT EXISTS (SELECT 1 FROM object_owner_references owner_ref
                 WHERE owner_ref.object_ref=page.object_ref AND owner_ref.owner_kind='bid_attachment_page'
                   AND owner_ref.owner_id=attach.id AND owner_ref.occurrence=page.page_ordinal::text))))
       OR (attach.media_type<>'application/pdf'
           AND EXISTS (SELECT 1 FROM bid_attachment_render_pages page WHERE page.attachment_id=attach.id))) THEN
      RAISE EXCEPTION 'ATTACHMENT_RENDER_PAGE_IDENTITY_MISMATCH' USING ERRCODE='23514';
    END IF;
    UPDATE bid_procedural_attachments SET validation_status='valid', revision=revision+1, updated_at=clock_timestamp()
     WHERE id=p_attachment_id RETURNING * INTO attach;
  ELSIF p_action='invalidate' THEN
    UPDATE bid_procedural_attachments SET validation_status='invalid', revision=revision+1, updated_at=clock_timestamp()
     WHERE id=p_attachment_id RETURNING * INTO attach;
  ELSIF p_action='confirm' THEN
    IF attach.validation_status<>'valid' THEN RAISE EXCEPTION 'ATTACHMENT_NOT_VALID' USING ERRCODE='22023'; END IF;
    UPDATE bid_procedural_attachments SET status='confirmed', revision=revision+1, updated_at=clock_timestamp()
     WHERE id=p_attachment_id RETURNING * INTO attach;
  ELSIF p_action='reject' THEN
    UPDATE bid_procedural_attachments SET status='rejected', revision=revision+1, updated_at=clock_timestamp()
     WHERE id=p_attachment_id RETURNING * INTO attach;
  ELSIF p_action='delete' THEN
    PERFORM kb_object_reference_remove(attach.object_ref,'bid_attachment',attach.id,'original');
    PERFORM kb_object_reference_remove(page.object_ref,'bid_attachment_page',attach.id,page.page_ordinal::text)
      FROM bid_attachment_render_pages page WHERE page.attachment_id=attach.id;
    UPDATE bid_procedural_attachments SET status='superseded', revision=revision+1, updated_at=clock_timestamp()
     WHERE id=p_attachment_id RETURNING * INTO attach;
  ELSE
    RAISE EXCEPTION 'ATTACHMENT_ACTION_INVALID' USING ERRCODE='22023';
  END IF;
  PERFORM kb_bid_stale_parts(p_project_id, ARRAY['5','6:authorization','6:procedural'], 'ATTACHMENT_CHANGED');
  response := jsonb_build_object('id',attach.id,'validation_status',attach.validation_status,'status',attach.status,'revision',attach.revision);
  INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
    entity_kind,entity_locator,after_revision,after_sha256)
  VALUES(gen_random_uuid(),1,'bid.attachment.'||p_action,p_actor,p_idempotency_key,p_request_sha256,
    encode(public.digest(convert_to(response::text,'UTF8'),'sha256'),'hex'),'bid_attachment',
    jsonb_build_object('attachment_id',p_attachment_id),attach.revision,
    encode(public.digest(convert_to(response::text,'UTF8'),'sha256'),'hex'));
  PERFORM kb_bid_idempotency_complete(p_actor,'bid.attachment.'||p_action,p_idempotency_key,200,convert_to(response::text,'UTF8'));
  RETURN response;
END
$$;

CREATE FUNCTION kb_bid_upload_shot_artifact(
  p_staging_id uuid,p_id uuid,p_project_id uuid,p_object_ref kb_object_ref,p_digest kb_sha256,p_media_type text,
  p_byte_length bigint,p_pixel_width integer,p_pixel_height integer,
  p_actor kb_actor_identity,p_idempotency_key text,p_request_bytes bytea,p_request_sha256 kb_sha256
)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE replay bytea; project_value bid_projects%ROWTYPE; response jsonb;
BEGIN
  PERFORM kb_bid_require_human_actor(p_actor);
  replay := kb_bid_idempotency_begin(p_actor,'bid.shot.upload',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN
    PERFORM kb_object_upload_abandon(p_staging_id,p_actor);
    RETURN convert_from(replay,'UTF8')::jsonb;
  END IF;
  SELECT * INTO STRICT project_value FROM bid_projects WHERE id=p_project_id FOR UPDATE;
  IF project_value.status<>'open' THEN RAISE EXCEPTION 'PROJECT_ENDED' USING ERRCODE='55000'; END IF;
  IF p_media_type NOT IN ('image/png','image/jpeg','image/webp') OR p_byte_length NOT BETWEEN 1 AND 20971520
     OR p_pixel_width NOT BETWEEN 1 AND 20000 OR p_pixel_height NOT BETWEEN 1 AND 20000 THEN
    RAISE EXCEPTION 'SHOT_VALIDATION_INVALID' USING ERRCODE='22023';
  END IF;
  PERFORM kb_object_upload_commit(p_staging_id,p_object_ref,p_digest,p_media_type,p_byte_length,
    'bid_shot',p_id,'artifact',p_actor);
  INSERT INTO bid_shot_artifacts(id,project_id,object_ref,content_sha256,media_type,byte_length,
    pixel_width,pixel_height,created_by)
  VALUES(p_id,p_project_id,p_object_ref,p_digest,p_media_type,p_byte_length,p_pixel_width,p_pixel_height,p_actor);
  response := jsonb_build_object('shot_artifact_id',p_id,'object_ref',p_object_ref,'digest',p_digest,
    'media_type',p_media_type,'byte_length',p_byte_length,'pixel_width',p_pixel_width,'pixel_height',p_pixel_height);
  INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
    entity_kind,entity_locator,after_revision,after_sha256)
  VALUES(gen_random_uuid(),1,'bid.shot.upload',p_actor,p_idempotency_key,p_request_sha256,
    encode(public.digest(convert_to(response::text,'UTF8'),'sha256'),'hex'),'bid_shot_artifact',
    jsonb_build_object('shot_artifact_id',p_id),1,p_digest);
  PERFORM kb_bid_idempotency_complete(p_actor,'bid.shot.upload',p_idempotency_key,201,convert_to(response::text,'UTF8'));
  RETURN response;
END
$$;

CREATE FUNCTION kb_bid_replace_shot_set(
  p_project_id uuid,p_expected_revision bigint,p_shot_artifact_ids uuid[],
  p_actor kb_actor_identity, p_idempotency_key text, p_request_bytes bytea, p_request_sha256 kb_sha256
)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE replay bytea; project_value bid_projects%ROWTYPE; set_id uuid := gen_random_uuid();
 next_rev bigint; payload bytea; digest kb_sha256; response jsonb; current_revision bigint;
 items jsonb;
BEGIN
  PERFORM kb_bid_require_human_actor(p_actor);
  replay := kb_bid_idempotency_begin(p_actor,'bid.shot.replace_set',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  SELECT * INTO STRICT project_value FROM bid_projects WHERE id=p_project_id FOR UPDATE;
  IF project_value.status<>'open' THEN RAISE EXCEPTION 'PROJECT_ENDED' USING ERRCODE='55000'; END IF;
  SELECT revision INTO current_revision FROM bid_current_shot_sets WHERE project_id=p_project_id FOR UPDATE;
  IF COALESCE(current_revision,0)<>p_expected_revision THEN
    RAISE EXCEPTION 'SHOT_SET_REVISION_CAS_MISMATCH' USING ERRCODE='40001';
  END IF;
  IF p_shot_artifact_ids IS NULL OR cardinality(p_shot_artifact_ids)>2048
     OR cardinality(p_shot_artifact_ids)<>(SELECT count(DISTINCT id) FROM unnest(p_shot_artifact_ids) id)
     OR cardinality(p_shot_artifact_ids)<>(SELECT count(*) FROM bid_shot_artifacts shot
       JOIN available_object_registry registry ON registry.object_ref=shot.object_ref
       WHERE shot.project_id=p_project_id AND shot.id=ANY(p_shot_artifact_ids)
         AND registry.digest=shot.content_sha256 AND registry.media_type=shot.media_type
         AND registry.byte_length=shot.byte_length
         AND EXISTS (SELECT 1 FROM object_owner_references owner_ref
           WHERE owner_ref.object_ref=shot.object_ref AND owner_ref.owner_kind='bid_shot'
             AND owner_ref.owner_id=shot.id AND owner_ref.occurrence='artifact')) THEN
    RAISE EXCEPTION 'SHOT_SET_ARTIFACTS_INVALID' USING ERRCODE='22023';
  END IF;
  DELETE FROM bid_current_shot_placements WHERE project_id=p_project_id;
  INSERT INTO bid_current_shot_placements(project_id,ordinal,shot_artifact_id)
  SELECT p_project_id,ordinality-1,shot_id
    FROM unnest(p_shot_artifact_ids) WITH ORDINALITY AS placement(shot_id,ordinality);
  next_rev := p_expected_revision+1;
  SELECT COALESCE(jsonb_agg(jsonb_build_object('ordinal',placement.ordinal,
      'shot_artifact_id',shot.id,'object_ref',shot.object_ref,'digest',shot.content_sha256,
      'media_type',shot.media_type,'byte_length',shot.byte_length,
      'pixel_width',shot.pixel_width,'pixel_height',shot.pixel_height) ORDER BY placement.ordinal),'[]'::jsonb)
    INTO items
    FROM bid_current_shot_placements placement
    JOIN bid_shot_artifacts shot ON shot.project_id=placement.project_id AND shot.id=placement.shot_artifact_id
   WHERE placement.project_id=p_project_id;
  payload := convert_to(jsonb_build_object('schema_version',1,'project_id',p_project_id,
    'revision',next_rev,'items',items)::text,'UTF8');
  digest := encode(public.digest(payload,'sha256'),'hex');
  INSERT INTO bid_shot_set_artifacts(id,project_id,revision,canonical_payload,content_sha256,created_by)
  VALUES(set_id,p_project_id,next_rev,payload,digest,p_actor);
  INSERT INTO bid_current_shot_sets(project_id,shot_set_id,revision) VALUES(p_project_id,set_id,next_rev)
  ON CONFLICT (project_id) DO UPDATE SET shot_set_id=EXCLUDED.shot_set_id, revision=EXCLUDED.revision;
  PERFORM kb_bid_stale_parts(p_project_id, ARRAY['3'], 'SHOT_SET_CHANGED');
  response := jsonb_build_object('shot_set_id',set_id,'revision',next_rev,'content_sha256',digest);
  INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
    entity_kind,entity_locator,after_revision,after_sha256)
  VALUES(gen_random_uuid(),1,'bid.shot.replace_set',p_actor,p_idempotency_key,p_request_sha256,
    encode(public.digest(convert_to(response::text,'UTF8'),'sha256'),'hex'),'bid_shot_set',
    jsonb_build_object('project_id',p_project_id),next_rev,digest);
  PERFORM kb_bid_idempotency_complete(p_actor,'bid.shot.replace_set',p_idempotency_key,200,convert_to(response::text,'UTF8'));
  RETURN response;
END
$$;

CREATE FUNCTION kb_bid_verify_shot_set_v1()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE parsed jsonb; expected_items jsonb; expected jsonb;
BEGIN
  BEGIN
    parsed := convert_from(NEW.canonical_payload,'UTF8')::jsonb;
  EXCEPTION WHEN OTHERS THEN
    RAISE EXCEPTION 'SHOT_SET_INVALID_JSON' USING ERRCODE='23514';
  END;
  SELECT COALESCE(jsonb_agg(jsonb_build_object('ordinal',placement.ordinal,
      'shot_artifact_id',shot.id,'object_ref',shot.object_ref,'digest',shot.content_sha256,
      'media_type',shot.media_type,'byte_length',shot.byte_length,
      'pixel_width',shot.pixel_width,'pixel_height',shot.pixel_height) ORDER BY placement.ordinal),'[]'::jsonb)
    INTO expected_items
    FROM bid_current_shot_placements placement
    JOIN bid_shot_artifacts shot ON shot.project_id=placement.project_id AND shot.id=placement.shot_artifact_id
   WHERE placement.project_id=NEW.project_id;
  expected := jsonb_build_object('schema_version',1,'project_id',NEW.project_id,
    'revision',NEW.revision,'items',expected_items);
  IF parsed IS DISTINCT FROM expected OR NOT EXISTS (SELECT 1 FROM bid_current_shot_sets current_value
      WHERE current_value.project_id=NEW.project_id AND current_value.shot_set_id=NEW.id
        AND current_value.revision=NEW.revision) THEN
    RAISE EXCEPTION 'SHOT_SET_RELATION_MISMATCH' USING ERRCODE='23514';
  END IF;
  RETURN NEW;
END
$$;
CREATE CONSTRAINT TRIGGER bid_shot_set_artifacts_verify
AFTER INSERT ON bid_shot_set_artifacts DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION kb_bid_verify_shot_set_v1();

CREATE FUNCTION kb_bid_validate_part_markdown_assets(p_project_id uuid,p_markdown bytea)
RETURNS void
LANGUAGE plpgsql
STABLE
SET search_path = pg_catalog, public
AS $$
DECLARE occurrence_count integer; occurrence_bytes numeric;
BEGIN
  WITH occurrence AS (
    SELECT 'objects/'||match_value.m[1] AS object_ref
      FROM regexp_matches(convert_from(p_markdown,'UTF8'),
        '![[][^]]*[]][(]objects/([0-9a-f]{64})[)]','g') AS match_value(m)
  ), checked AS (
    SELECT occurrence.object_ref,registry.byte_length,source_image.id
      FROM occurrence
      LEFT JOIN available_object_registry registry ON registry.object_ref=occurrence.object_ref
      LEFT JOIN LATERAL (
        SELECT shot.id FROM bid_shot_artifacts shot
         WHERE shot.project_id=p_project_id AND shot.object_ref=occurrence.object_ref
           AND shot.content_sha256=registry.digest AND shot.media_type=registry.media_type
           AND shot.byte_length=registry.byte_length
           AND EXISTS (SELECT 1 FROM object_owner_references owner_ref
             WHERE owner_ref.object_ref=shot.object_ref AND owner_ref.owner_kind='bid_shot'
               AND owner_ref.owner_id=shot.id AND owner_ref.occurrence='artifact')
         ORDER BY shot.id LIMIT 1
      ) source_image ON true
  )
  SELECT count(*),COALESCE(sum(byte_length),0) INTO occurrence_count,occurrence_bytes FROM checked
   WHERE id IS NOT NULL;
  IF occurrence_count<>(SELECT count(*) FROM regexp_matches(convert_from(p_markdown,'UTF8'),
       '![[][^]]*[]][(]objects/([0-9a-f]{64})[)]','g') AS match_count(m))
     OR occurrence_count>2048 OR occurrence_bytes>268435456 THEN
    RAISE EXCEPTION 'PART_MARKDOWN_ASSET_INVALID_OR_UNAUTHORIZED' USING ERRCODE='22023';
  END IF;
END
$$;

CREATE FUNCTION kb_bid_update_part(
  p_project_id uuid, p_part_key text, p_expected_content_revision bigint, p_markdown bytea,
  p_actor kb_actor_identity, p_idempotency_key text, p_request_bytes bytea, p_request_sha256 kb_sha256
)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE replay bytea; project_value bid_projects%ROWTYPE; current_row bid_current_parts%ROWTYPE;
 content bid_part_content_artifacts%ROWTYPE; new_id uuid := gen_random_uuid(); dependency_id uuid := gen_random_uuid();
 digest kb_sha256; dependency_payload bytea; dependency_digest kb_sha256; response jsonb;
 template_slot text; template_version text; typed_identities jsonb; now_ts timestamptz := clock_timestamp();
BEGIN
  PERFORM kb_bid_require_human_actor(p_actor);
  replay := kb_bid_idempotency_begin(p_actor,'bid.part.update',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  SELECT * INTO STRICT project_value FROM bid_projects WHERE id=p_project_id FOR UPDATE;
  IF project_value.status<>'open' THEN RAISE EXCEPTION 'PROJECT_ENDED' USING ERRCODE='55000'; END IF;
  template_slot := kb_bid_template_slot(p_part_key);
  IF template_slot IS NULL THEN RAISE EXCEPTION 'PART_KEY_INVALID' USING ERRCODE='22023'; END IF;
  SELECT current_template.version INTO STRICT template_version
    FROM bid_template_contract_current current_template
   WHERE current_template.slot=template_slot FOR UPDATE;
  PERFORM kb_bid_validate_part_markdown_assets(p_project_id,p_markdown);
  SELECT * INTO current_row FROM bid_current_parts WHERE project_id=p_project_id AND part_key=p_part_key FOR UPDATE;
  IF current_row.project_id IS NOT NULL THEN
    SELECT * INTO content FROM bid_part_content_artifacts WHERE id=current_row.content_artifact_id;
    IF content.revision<>p_expected_content_revision THEN RAISE EXCEPTION 'PART_CONTENT_CAS_MISMATCH' USING ERRCODE='40001'; END IF;
  ELSIF p_expected_content_revision<>0 THEN
    RAISE EXCEPTION 'PART_CONTENT_CAS_MISMATCH' USING ERRCODE='40001';
  END IF;
  digest := encode(public.digest(p_markdown,'sha256'),'hex');
  INSERT INTO bid_part_content_artifacts(id,project_id,part_key,revision,canonical_markdown_utf8,content_sha256,created_by)
  VALUES(new_id,p_project_id,p_part_key,COALESCE(content.revision,0)+1,p_markdown,digest,p_actor);
  typed_identities := kb_bid_current_part_input_identities(p_project_id,p_part_key);
  dependency_payload := convert_to('{"schema_version":1,"project_id":'||kb_bid_json_string(p_project_id::text)
    ||',"part_key":'||kb_bid_json_string(p_part_key)
    ||',"template_slot":'||kb_bid_json_string(template_slot)
    ||',"template_version":'||kb_bid_json_string(template_version)
    ||',"input_identities":'||typed_identities::text
    ||',"part_content_revision":'||(COALESCE(content.revision,0)+1)::text
    ||',"part_content_sha256":'||kb_bid_json_string(digest)
    ||',"generated_at":'||kb_bid_json_string(kb_bid_utc_json_time(now_ts))||'}','UTF8');
  dependency_digest := encode(public.digest(dependency_payload,'sha256'),'hex');
  INSERT INTO bid_part_dependency_artifacts(id,project_id,part_key,template_slot,template_version,
    part_content_artifact_id,schema_version,typed_input_identities,canonical_payload,content_sha256,generated_at)
  VALUES(dependency_id,p_project_id,p_part_key,template_slot,template_version,new_id,1,typed_identities,
    dependency_payload,dependency_digest,now_ts);
  INSERT INTO bid_current_parts(project_id,part_key,content_artifact_id,dependency_artifact_id,stale,stale_reason_codes)
  VALUES(p_project_id,p_part_key,new_id,dependency_id,false,'{}')
  ON CONFLICT (project_id,part_key) DO UPDATE SET content_artifact_id=EXCLUDED.content_artifact_id,
    dependency_artifact_id=EXCLUDED.dependency_artifact_id,stale=false,stale_reason_codes='{}';
  response := jsonb_build_object('content_artifact_id',new_id,'dependency_artifact_id',dependency_id,
    'revision',COALESCE(content.revision,0)+1,'content_sha256',digest,'dependency_sha256',dependency_digest);
  INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
    entity_kind,entity_locator,after_revision,after_sha256)
  VALUES(gen_random_uuid(),1,'bid.part.update',p_actor,p_idempotency_key,p_request_sha256,
    encode(public.digest(convert_to(response::text,'UTF8'),'sha256'),'hex'),'bid_part',
    jsonb_build_object('project_id',p_project_id,'part_key',p_part_key),COALESCE(content.revision,0)+1,digest);
  PERFORM kb_bid_idempotency_complete(p_actor,'bid.part.update',p_idempotency_key,200,convert_to(response::text,'UTF8'));
  RETURN response;
END
$$;

CREATE FUNCTION kb_bid_part_input_identities_are_typed(p_identities jsonb)
RETURNS boolean
LANGUAGE sql
IMMUTABLE
STRICT
PARALLEL SAFE
SET search_path = pg_catalog
AS $$
  SELECT jsonb_typeof(p_identities)='array'
    AND NOT EXISTS (
      SELECT 1
        FROM jsonb_array_elements(p_identities) item
       WHERE jsonb_typeof(item)<>'object'
          OR item->>'type' NOT IN (
            'fact','clause_set','matching_report','route_pick_set','project_pick_set',
            'quote_snapshot','company_profile','submission_profile','procedural_set',
            'attachment_set','shot_set'
          )
    )
$$;

ALTER TABLE bid_part_dependency_artifacts
  ADD CONSTRAINT bid_part_dependency_typed_inputs_check
  CHECK (kb_bid_part_input_identities_are_typed(typed_input_identities));

CREATE FUNCTION kb_bid_current_part_input_identities(p_project_id uuid, p_part_key text)
RETURNS jsonb
LANGUAGE plpgsql
STABLE
SET search_path = pg_catalog, public
AS $$
DECLARE identities jsonb := '[]'::jsonb; project_value bid_projects%ROWTYPE;
  raw_unit text; unit_value uuid; subset jsonb; set_names text[] := '{}'; profile_value record;
BEGIN
  IF kb_bid_template_slot(p_part_key) IS NULL THEN
    RAISE EXCEPTION 'PART_KEY_INVALID' USING ERRCODE='22023';
  END IF;
  SELECT * INTO STRICT project_value FROM bid_projects WHERE id=p_project_id;

  IF p_part_key='1' THEN
    identities := identities || jsonb_build_array(jsonb_build_object(
      'type','fact','scope','all','revision',project_value.fact_revision,'sha256',project_value.fact_sha256));
  ELSIF p_part_key='6:letter' THEN
    subset := jsonb_build_object(
      'expires_at',project_value.expires_at,'bid_open_at',project_value.bid_open_at,
      'bid_valid_until',project_value.bid_valid_until,'bid_valid_days',project_value.bid_valid_days,
      'ceiling_revision',project_value.ceiling_revision,
      'ceiling_identity_sha256',project_value.ceiling_identity_sha256);
    identities := identities || jsonb_build_array(jsonb_build_object(
      'type','fact','scope','submission_letter','sha256',
      encode(public.digest(convert_to(subset::text,'UTF8'),'sha256'),'hex')));
  END IF;

  IF p_part_key IN ('1','6:letter','6:authorization') THEN
    SELECT company.id AS company_id,company.revision AS company_revision,company.content_sha256 AS company_sha256,
           submission.id AS submission_id,submission.revision AS submission_revision,
           submission.content_sha256 AS submission_sha256
      INTO profile_value
      FROM bid_projects project
      LEFT JOIN bid_current_profiles current_value ON current_value.project_id=project.id
      LEFT JOIN bid_company_profile_artifacts company ON company.id=current_value.company_profile_id
      LEFT JOIN bid_submission_profile_artifacts submission ON submission.id=current_value.submission_profile_id
     WHERE project.id=p_project_id;
    identities := identities || jsonb_build_array(
      jsonb_build_object('type','company_profile','artifact_id',profile_value.company_id,
        'revision',profile_value.company_revision,'sha256',profile_value.company_sha256),
      jsonb_build_object('type','submission_profile','artifact_id',profile_value.submission_id,
        'revision',profile_value.submission_revision,'sha256',profile_value.submission_sha256));
  END IF;

  set_names := CASE p_part_key
    WHEN '5' THEN ARRAY['evaluation','procedural']
    WHEN '6:letter' THEN ARRAY['pricing','schedule_payment']
    WHEN '6:quote' THEN ARRAY['pricing']
    WHEN '6:implementation_plan' THEN ARRAY['service','schedule_delivery']
    WHEN '6:authorization' THEN ARRAY['procedural']
    WHEN '6:procedural' THEN ARRAY['procedural']
    ELSE ARRAY[]::text[] END;
  IF cardinality(set_names)>0 THEN
    SELECT identities || COALESCE(jsonb_agg(jsonb_build_object(
      'type','clause_set','set_kind',set_value.set_kind,'revision',set_value.revision,
      'sha256',set_value.content_sha256) ORDER BY set_value.set_kind),'[]'::jsonb)
      INTO identities
      FROM bid_clause_set_identities set_value
     WHERE set_value.project_id=p_project_id AND set_value.set_kind=ANY(set_names);
  END IF;

  IF p_part_key LIKE '2:%' THEN
    IF p_part_key='2:unsectioned' THEN
      unit_value := '00000000-0000-0000-0000-000000000000'::uuid;
    ELSE
      raw_unit := substr(p_part_key,3);
      unit_value := raw_unit::uuid;
    END IF;
    SELECT identities || COALESCE(jsonb_agg(item ORDER BY route_id,type_order),'[]'::jsonb)
      INTO identities
      FROM (
        SELECT route.id AS route_id,0 AS type_order,jsonb_build_object(
          'type','matching_report','route_id',route.id,'artifact_id',report.id,
          'generation',report.generation,'sha256',report.content_sha256) AS item
          FROM bid_matching_routes route
          JOIN bidding_current_matching_reports report ON report.route_id=route.id
         WHERE route.project_id=p_project_id AND route.route_kind='technical' AND route.unit_id=unit_value
        UNION ALL
        SELECT route.id,1,jsonb_build_object(
          'type','route_pick_set','route_id',route.id,'artifact_id',pick.id,
          'revision',pick.revision,'sha256',pick.content_sha256)
          FROM bid_matching_routes route
          JOIN bidding_current_route_pick_sets pick ON pick.route_id=route.id
         WHERE route.project_id=p_project_id AND route.route_kind='technical' AND route.unit_id=unit_value
      ) typed;
  END IF;

  IF p_part_key IN ('4','5') THEN
    SELECT identities || COALESCE(jsonb_agg(jsonb_build_object(
      'type','matching_report','route_id',route.id,'artifact_id',report.id,
      'generation',report.generation,'sha256',report.content_sha256) ORDER BY route.id),'[]'::jsonb)
      INTO identities
     FROM bid_matching_routes route
      JOIN bidding_current_matching_reports report ON report.route_id=route.id
     WHERE route.project_id=p_project_id
       AND (p_part_key='5' OR route.route_kind='commercial');
  END IF;

  IF p_part_key IN ('3','5','6:implementation_plan') THEN
    SELECT identities || jsonb_build_array(jsonb_build_object(
      'type','project_pick_set','artifact_id',artifact.id,'revision',artifact.revision,
      'sha256',artifact.content_sha256))
      INTO identities
      FROM bid_projects project
      LEFT JOIN bid_current_project_pick_sets current_value ON current_value.project_id=project.id
      LEFT JOIN bid_project_pick_set_artifacts artifact ON artifact.id=current_value.pick_set_id
     WHERE project.id=p_project_id;
  END IF;

  IF p_part_key IN ('6:letter','6:quote') THEN
    SELECT identities || jsonb_build_array(jsonb_build_object(
      'type','quote_snapshot','artifact_id',snapshot.id,'revision_id',snapshot.revision_id,
      'sha256',snapshot.content_sha256,'eligibility',snapshot.eligibility))
      INTO identities
      FROM bid_projects project
      LEFT JOIN bid_quote_current current_value ON current_value.project_id=project.id
      LEFT JOIN bid_quote_snapshots snapshot ON snapshot.id=current_value.active_finalized_snapshot_id
     WHERE project.id=p_project_id;
  END IF;

  IF p_part_key IN ('5','6:authorization','6:procedural') THEN
    SELECT identities || jsonb_build_array(jsonb_build_object(
      'type','procedural_set','revision',segment_set.revision,'sha256',segment_set.content_sha256,
      'classifications',COALESCE((SELECT jsonb_agg(jsonb_build_object(
        'segment_id',segment.id,'classification_id',classification.id,'revision',classification.revision,
        'router_result_status',classification.router_result_status,
        'effective_requirement_kind',classification.effective_requirement_kind)
        ORDER BY segment.stable_key)
        FROM bid_procedural_segment_artifacts segment
        JOIN bid_procedural_classification_artifacts classification
          ON classification.segment_id=segment.id AND classification.lifecycle_status='current'
        WHERE segment.project_id=p_project_id),'[]'::jsonb),
      'decisions',COALESCE((SELECT jsonb_agg(jsonb_build_object(
        'classification_id',decision.classification_id,'decision_id',decision.id,
        'revision',decision.revision,'resolution',decision.resolution,
        'attachment_id',decision.attachment_id,'reason',decision.reason,
        'actor_identity',decision.actor_identity) ORDER BY decision.classification_id)
        FROM bid_procedural_decision_artifacts decision
        JOIN bid_procedural_classification_artifacts classification ON classification.id=decision.classification_id
        JOIN bid_procedural_segment_artifacts segment ON segment.id=classification.segment_id
        WHERE segment.project_id=p_project_id AND decision.lifecycle_status='current'),'[]'::jsonb)))
      INTO identities
      FROM bid_procedural_segment_sets segment_set WHERE segment_set.project_id=p_project_id;
    SELECT identities || jsonb_build_array(jsonb_build_object(
      'type','attachment_set','sha256',encode(public.digest(convert_to(COALESCE(jsonb_agg(jsonb_build_object(
        'attachment_id',attachment.id,'revision',attachment.revision,'kind',attachment.kind,
        'object_ref',attachment.object_ref,'validation_status',attachment.validation_status,
        'status',attachment.status,'digest',attachment.content_sha256,'media_type',attachment.media_type,
        'byte_length',attachment.byte_length,'pixel_width',attachment.pixel_width,
        'pixel_height',attachment.pixel_height,'validation_sha256',attachment.validation_sha256,
        'render_pages',COALESCE((SELECT jsonb_agg(jsonb_build_object(
          'page_ordinal',page.page_ordinal,'object_ref',page.object_ref,'digest',page.content_sha256,
          'media_type',page.media_type,'byte_length',page.byte_length,
          'pixel_width',page.pixel_width,'pixel_height',page.pixel_height) ORDER BY page.page_ordinal)
          FROM bid_attachment_render_pages page WHERE page.attachment_id=attachment.id),'[]'::jsonb))
        ORDER BY attachment.id),'[]'::jsonb)::text,'UTF8'),'sha256'),'hex')))
      INTO identities
      FROM bid_procedural_attachments attachment
     WHERE attachment.project_id=p_project_id AND attachment.status<>'superseded';
  END IF;

  IF p_part_key='3' THEN
    SELECT identities || jsonb_build_array(jsonb_build_object(
      'type','shot_set','artifact_id',artifact.id,'revision',artifact.revision,
      'sha256',artifact.content_sha256))
      INTO identities
      FROM bid_projects project
      LEFT JOIN bid_current_shot_sets current_value ON current_value.project_id=project.id
      LEFT JOIN bid_shot_set_artifacts artifact ON artifact.id=current_value.shot_set_id
     WHERE project.id=p_project_id;
  END IF;

  RETURN identities;
END
$$;

CREATE FUNCTION kb_bid_regenerate_part(
  p_project_id uuid, p_part_key text, p_expected_content_revision bigint, p_expected_dependency_sha256 kb_sha256,
  p_actor kb_actor_identity, p_idempotency_key text, p_request_bytes bytea, p_request_sha256 kb_sha256
)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE replay bytea; project_value bid_projects%ROWTYPE; template_slot text; template_version text; content_id uuid := gen_random_uuid();
 dep_id uuid := gen_random_uuid(); digest kb_sha256; dep_payload bytea; dep_digest kb_sha256;
 current_row bid_current_parts%ROWTYPE; content bid_part_content_artifacts%ROWTYPE; response jsonb;
 typed_identities jsonb; generated_markdown bytea; now_ts timestamptz := clock_timestamp();
BEGIN
  PERFORM kb_bid_require_human_actor(p_actor);
  PERFORM 1 FROM application_maintenance_gate WHERE singleton_key AND mode='open' FOR SHARE;
  IF NOT FOUND THEN RAISE EXCEPTION 'MAINTENANCE_PART_REGENERATION_BLOCKED' USING ERRCODE='55000'; END IF;
  replay := kb_bid_idempotency_begin(p_actor,'bid.part.regenerate',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  SELECT * INTO STRICT project_value FROM bid_projects WHERE id=p_project_id FOR UPDATE;
  IF project_value.status<>'open' THEN RAISE EXCEPTION 'PROJECT_ENDED' USING ERRCODE='55000'; END IF;
  template_slot := kb_bid_template_slot(p_part_key);
  IF template_slot IS NULL THEN RAISE EXCEPTION 'PART_KEY_INVALID' USING ERRCODE='22023'; END IF;
  SELECT current_template.version INTO STRICT template_version
    FROM bid_template_contract_current current_template
   WHERE current_template.slot=template_slot FOR UPDATE;
  SELECT * INTO current_row FROM bid_current_parts WHERE project_id=p_project_id AND part_key=p_part_key FOR UPDATE;
  IF current_row.project_id IS NOT NULL THEN
    SELECT * INTO content FROM bid_part_content_artifacts WHERE id=current_row.content_artifact_id;
    IF content.revision<>p_expected_content_revision THEN RAISE EXCEPTION 'PART_CONTENT_CAS_MISMATCH' USING ERRCODE='40001'; END IF;
    IF p_expected_dependency_sha256 IS NULL OR
       (SELECT content_sha256 FROM bid_part_dependency_artifacts WHERE id=current_row.dependency_artifact_id)
         <> p_expected_dependency_sha256 THEN
      RAISE EXCEPTION 'PART_DEPENDENCY_CAS_MISMATCH' USING ERRCODE='40001';
    END IF;
  ELSIF p_expected_content_revision<>0 OR p_expected_dependency_sha256 IS NOT NULL THEN
    RAISE EXCEPTION 'PART_CONTENT_CAS_MISMATCH' USING ERRCODE='40001';
  END IF;
  typed_identities := kb_bid_current_part_input_identities(p_project_id,p_part_key);
  generated_markdown := convert_to(kb_bid_build_part_markdown(p_project_id,p_part_key),'UTF8');
  PERFORM kb_bid_validate_part_markdown_assets(p_project_id,generated_markdown);
  digest := encode(public.digest(generated_markdown,'sha256'),'hex');
  INSERT INTO bid_part_content_artifacts(id,project_id,part_key,revision,canonical_markdown_utf8,content_sha256,created_by)
  VALUES(content_id,p_project_id,p_part_key,COALESCE(content.revision,0)+1,generated_markdown,digest,p_actor);
  dep_payload := convert_to('{"schema_version":1,"project_id":'||kb_bid_json_string(p_project_id::text)
    ||',"part_key":'||kb_bid_json_string(p_part_key)
    ||',"template_slot":'||kb_bid_json_string(template_slot)
    ||',"template_version":'||kb_bid_json_string(template_version)
    ||',"input_identities":'||typed_identities::text
    ||',"part_content_revision":'||(COALESCE(content.revision,0)+1)::text
    ||',"part_content_sha256":'||kb_bid_json_string(digest)
    ||',"generated_at":'||kb_bid_json_string(kb_bid_utc_json_time(now_ts))||'}','UTF8');
  dep_digest := encode(public.digest(dep_payload,'sha256'),'hex');
  INSERT INTO bid_part_dependency_artifacts(id,project_id,part_key,template_slot,template_version,part_content_artifact_id,
    schema_version,typed_input_identities,canonical_payload,content_sha256,generated_at)
  VALUES(dep_id,p_project_id,p_part_key,template_slot,template_version,content_id,1,typed_identities,dep_payload,dep_digest,now_ts);
  INSERT INTO bid_current_parts(project_id,part_key,content_artifact_id,dependency_artifact_id,stale,stale_reason_codes)
  VALUES(p_project_id,p_part_key,content_id,dep_id,false,'{}')
  ON CONFLICT (project_id, part_key) DO UPDATE SET content_artifact_id=EXCLUDED.content_artifact_id,
    dependency_artifact_id=EXCLUDED.dependency_artifact_id, stale=false, stale_reason_codes='{}';
  response := jsonb_build_object('content_artifact_id',content_id,'dependency_artifact_id',dep_id,
    'content_sha256',digest,'dependency_sha256',dep_digest,'revision',COALESCE(content.revision,0)+1);
  INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
    entity_kind,entity_locator,after_revision,after_sha256)
  VALUES(gen_random_uuid(),1,'bid.part.regenerate',p_actor,p_idempotency_key,p_request_sha256,
    encode(public.digest(convert_to(response::text,'UTF8'),'sha256'),'hex'),'bid_part',
    jsonb_build_object('project_id',p_project_id,'part_key',p_part_key),COALESCE(content.revision,0)+1,digest);
  PERFORM kb_bid_idempotency_complete(p_actor,'bid.part.regenerate',p_idempotency_key,200,convert_to(response::text,'UTF8'));
  RETURN response;
END
$$;

CREATE FUNCTION kb_bid_gate_issue(
  p_code text, p_part_key text, p_entity_locator jsonb,
  p_current_identity jsonb, p_expected_identity jsonb, p_remediation jsonb
)
RETURNS jsonb
LANGUAGE sql
IMMUTABLE
SET search_path = pg_catalog
AS $$
  SELECT jsonb_build_object(
    'code',p_code,'part_key',p_part_key,'entity_locator',p_entity_locator,
    'current_identity',p_current_identity,'expected_identity',p_expected_identity,
    'remediation',p_remediation)
$$;

CREATE FUNCTION kb_bid_verify_part_dependency_v1()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE parsed jsonb; expected jsonb; content bid_part_content_artifacts%ROWTYPE;
BEGIN
  BEGIN
    parsed := convert_from(NEW.canonical_payload,'UTF8')::jsonb;
  EXCEPTION WHEN OTHERS THEN
    RAISE EXCEPTION 'PART_DEPENDENCY_INVALID_JSON' USING ERRCODE='23514';
  END;
  SELECT * INTO STRICT content FROM bid_part_content_artifacts WHERE id=NEW.part_content_artifact_id;
  expected := jsonb_build_object('schema_version',1,'project_id',NEW.project_id,
    'part_key',NEW.part_key,'template_slot',NEW.template_slot,
    'template_version',NEW.template_version,'input_identities',NEW.typed_input_identities,
    'part_content_revision',content.revision,'part_content_sha256',content.content_sha256,
    'generated_at',kb_bid_utc_json_time(NEW.generated_at));
  IF parsed IS DISTINCT FROM expected OR content.project_id<>NEW.project_id
     OR content.part_key<>NEW.part_key
     OR NEW.typed_input_identities IS DISTINCT FROM kb_bid_current_part_input_identities(NEW.project_id,NEW.part_key) THEN
    RAISE EXCEPTION 'PART_DEPENDENCY_RELATION_OR_CURRENT_IDENTITY_MISMATCH' USING ERRCODE='23514';
  END IF;
  RETURN NEW;
END
$$;
CREATE CONSTRAINT TRIGGER bid_part_dependency_artifacts_verify
AFTER INSERT ON bid_part_dependency_artifacts DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION kb_bid_verify_part_dependency_v1();

CREATE FUNCTION kb_bid_list_gate_issues(p_project_id uuid, p_format text)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE issues jsonb := '[]'::jsonb; company bid_company_profile_artifacts%ROWTYPE;
 submission bid_submission_profile_artifacts%ROWTYPE; quote bid_quote_snapshots%ROWTYPE;
 project_value bid_projects%ROWTYPE; pricing bid_clause_set_identities%ROWTYPE;
 part record; seg record; pending record; key text; required text[];
 hard_issue_count integer := 0; warning_issue_count integer := 0;
BEGIN
  IF p_format NOT IN ('docx','pdf') THEN RAISE EXCEPTION 'SUBMISSION_FORMAT_INVALID' USING ERRCODE='22023'; END IF;
  SELECT * INTO STRICT project_value FROM bid_projects WHERE id=p_project_id;
  SELECT * INTO STRICT pricing FROM bid_clause_set_identities WHERE project_id=p_project_id AND set_kind='pricing';
  SELECT artifact.* INTO company FROM bid_current_profiles cur
    JOIN bid_company_profile_artifacts artifact ON artifact.id=cur.company_profile_id WHERE cur.project_id=p_project_id;
  SELECT artifact.* INTO submission FROM bid_current_profiles cur
    JOIN bid_submission_profile_artifacts artifact ON artifact.id=cur.submission_profile_id WHERE cur.project_id=p_project_id;
  IF company.id IS NULL OR btrim(COALESCE(company.legal_name,''))='' OR btrim(COALESCE(company.unified_social_credit_code,''))=''
     OR btrim(COALESCE(company.registered_address,''))='' OR btrim(COALESCE(company.legal_representative,''))=''
     OR btrim(COALESCE(company.contact_name,''))='' OR btrim(COALESCE(company.contact_phone,''))=''
     OR btrim(COALESCE(company.contact_email,''))='' THEN
    issues := issues || jsonb_build_array(kb_bid_gate_issue('PROFILE_FIELD_MISSING','6:letter',
      jsonb_build_object('profile','company_profile','profile_id',company.id),
      CASE WHEN company.id IS NULL THEN NULL ELSE jsonb_build_object(
        'legal_name',company.legal_name,'unified_social_credit_code',company.unified_social_credit_code,
        'registered_address',company.registered_address,'legal_representative',company.legal_representative,
        'contact_name',company.contact_name,'contact_phone',company.contact_phone,'contact_email',company.contact_email) END,
      jsonb_build_object('present',true,'fields_complete',true),jsonb_build_object('action','update_profile')));
    hard_issue_count := hard_issue_count + 1;
  END IF;
  IF submission.id IS NULL THEN
    issues := issues || jsonb_build_array(kb_bid_gate_issue('PROFILE_FIELD_MISSING','6:letter',
      jsonb_build_object('profile','submission_profile'),NULL,
      jsonb_build_object('present',true),jsonb_build_object('action','update_profile')));
    hard_issue_count := hard_issue_count + 1;
  ELSIF NOT submission.seal_confirmed OR NOT submission.signature_confirmed THEN
    issues := issues || jsonb_build_array(kb_bid_gate_issue('SIGNATURE_OR_SEAL_NOT_CONFIRMED','6:letter',
      jsonb_build_object('profile','submission_profile','profile_id',submission.id),
      jsonb_build_object('seal_confirmed',submission.seal_confirmed,'signature_confirmed',submission.signature_confirmed),
      jsonb_build_object('seal_confirmed',true,'signature_confirmed',true),
      jsonb_build_object('action','confirm_seal_and_signature')));
    hard_issue_count := hard_issue_count + 1;
  END IF;
  IF project_value.bid_valid_days IS NOT NULL AND project_value.bid_valid_until IS NOT NULL THEN
    issues := issues || jsonb_build_array(kb_bid_gate_issue('BID_VALIDITY_CONFLICT','6:letter',
      jsonb_build_object('project_id',p_project_id,'fact','bid_validity'),
      jsonb_build_object('bid_valid_days',project_value.bid_valid_days,'bid_valid_until',project_value.bid_valid_until),
      jsonb_build_object('single_value_only',true),jsonb_build_object('action','clear_or_choose_one_validity_fact')));
    hard_issue_count := hard_issue_count + 1;
  END IF;
  SELECT snapshot.* INTO quote FROM bid_quote_current current_value
    JOIN bid_quote_snapshots snapshot ON snapshot.id=current_value.active_finalized_snapshot_id
   WHERE current_value.project_id=p_project_id;
  IF quote.id IS NULL OR quote.eligibility<>'eligible'
     OR quote.ceiling_identity_sha256<>project_value.ceiling_identity_sha256
     OR quote.pricing_revision<>pricing.revision OR quote.pricing_set_sha256<>pricing.content_sha256 THEN
    issues := issues || jsonb_build_array(kb_bid_gate_issue('QUOTE_NOT_FINALIZED','6:quote',
      jsonb_build_object('project_id',p_project_id,'quote','active'),
      CASE WHEN quote.id IS NULL THEN NULL ELSE jsonb_build_object('snapshot_id',quote.id,
        'content_sha256',quote.content_sha256,'eligibility',quote.eligibility,
        'ceiling_identity_sha256',quote.ceiling_identity_sha256,
        'pricing_revision',quote.pricing_revision,'pricing_set_sha256',quote.pricing_set_sha256) END,
      jsonb_build_object('eligibility','eligible','ceiling_identity_sha256',project_value.ceiling_identity_sha256,
        'pricing_revision',pricing.revision,'pricing_set_sha256',pricing.content_sha256),
      jsonb_build_object('action','finalize_eligible_quote')));
    hard_issue_count := hard_issue_count + 1;
  END IF;
  FOR pending IN
    SELECT clause.id AS clause_id,clause.kind,
           COALESCE((clause.current_source_span_v2->>'section_artifact_id')::uuid,
             '00000000-0000-0000-0000-000000000000'::uuid) AS unit_id
      FROM bidding_current_clauses clause
     WHERE clause.project_id=p_project_id AND clause.status='confirmed'
       AND clause.kind IN ('technical','qualification','service')
       AND NOT EXISTS (
         SELECT 1
           FROM bidding_current_matching_reports report
           JOIN bid_matching_requirement_decisions decision ON decision.report_id=report.id
           JOIN bid_matching_requirement_artifacts requirement
             ON requirement.id=decision.requirement_artifact_id
          WHERE report.project_id=p_project_id AND requirement.clause_id=clause.id)
     ORDER BY clause.kind,clause.id
  LOOP
    key := CASE
      WHEN pending.kind='technical' AND pending.unit_id='00000000-0000-0000-0000-000000000000'::uuid
        THEN '2:unsectioned'
      WHEN pending.kind='technical' THEN '2:'||pending.unit_id::text
      ELSE '4' END;
    issues := issues || jsonb_build_array(kb_bid_gate_issue('MATCHING_REPORT_MISSING',key,
      jsonb_build_object('clause_id',pending.clause_id,'kind',pending.kind),NULL,
      jsonb_build_object('current_matching_decision',true),
      jsonb_build_object('action','run_matching')));
    hard_issue_count := hard_issue_count + 1;
  END LOOP;
  FOR pending IN
    SELECT route.id AS route_id, route.unit_id, report.id AS report_id,
           report.content_sha256 AS report_sha256,
           decision.requirement_artifact_id, requirement.clause_id
      FROM bid_matching_routes route
      JOIN bid_current_matching_reports current_report
        ON current_report.project_id=route.project_id AND current_report.route_id=route.id
      JOIN bid_matching_reports report ON report.id=current_report.report_id
      JOIN bid_matching_requirement_decisions decision
        ON decision.report_id=report.id AND decision.final_support='supported'
      JOIN bid_matching_requirement_artifacts requirement
        ON requirement.id=decision.requirement_artifact_id
     WHERE route.project_id=p_project_id AND route.route_kind='technical'
       AND NOT EXISTS (
         SELECT 1
           FROM bid_current_route_pick_sets current_pick
           JOIN bid_route_pick_set_artifacts pick_set ON pick_set.id=current_pick.pick_set_id
           JOIN bid_route_pick_set_items item ON item.pick_set_id=pick_set.id
          WHERE current_pick.project_id=p_project_id AND current_pick.route_id=route.id
            AND pick_set.source_report_artifact_id=report.id
            AND pick_set.report_sha256=report.content_sha256
            AND item.requirement_artifact_id=decision.requirement_artifact_id)
     ORDER BY route.ordinal,decision.ordinal,decision.requirement_artifact_id
  LOOP
    key := CASE
      WHEN pending.unit_id='00000000-0000-0000-0000-000000000000'::uuid THEN '2:unsectioned'
      ELSE '2:'||pending.unit_id::text END;
    issues := issues || jsonb_build_array(kb_bid_gate_issue('MATCHING_PICK_MISSING',key,
      jsonb_build_object('route_id',pending.route_id,'report_id',pending.report_id,
        'requirement_artifact_id',pending.requirement_artifact_id,'clause_id',pending.clause_id),
      jsonb_build_object('report_sha256',pending.report_sha256,'selected_candidate_count',0),
      jsonb_build_object('report_sha256',pending.report_sha256,'minimum_selected_candidates',1),
      jsonb_build_object('action','select_supported_candidate')));
    hard_issue_count := hard_issue_count + 1;
  END LOOP;
  FOR seg IN
    SELECT expected.clause_id, expected.stable_key, artifact.id AS segment_id,
           classification.id AS classification_id, classification.revision AS classification_revision,
           classification.router_result_status, classification.review_reason,
           classification.effective_requirement_kind, decision.id AS decision_id,
           decision.revision AS decision_revision, decision.resolution, decision.reason,
           decision.actor_identity, decision.attachment_id, attachment.kind AS attachment_kind,
           attachment.revision AS attachment_revision,attachment.content_sha256 AS attachment_sha256,
           attachment.validation_sha256,attachment.validation_status,attachment.status AS attachment_status
      FROM (
        SELECT clause.id AS clause_id,segment.stable_key,
               segment.start_offset
          FROM bid_clauses clause
          CROSS JOIN LATERAL kb_bid_procedural_segments_for_clause(clause) segment
         WHERE clause.project_id=p_project_id AND clause.status='confirmed' AND clause.kind='procedural'
      ) expected
      LEFT JOIN bid_procedural_segment_artifacts artifact
        ON artifact.project_id=p_project_id AND artifact.stable_key=expected.stable_key
      LEFT JOIN bid_procedural_classification_artifacts classification
        ON classification.segment_id=artifact.id AND classification.lifecycle_status='current'
      LEFT JOIN bid_procedural_decision_artifacts decision
        ON decision.classification_id=classification.id AND decision.lifecycle_status='current'
      LEFT JOIN bid_procedural_attachments attachment ON attachment.id=decision.attachment_id
     ORDER BY expected.clause_id,expected.start_offset,expected.stable_key
  LOOP
    IF seg.classification_id IS NULL THEN
      issues := issues || jsonb_build_array(kb_bid_gate_issue('PROCEDURAL_CLASSIFICATION_MISSING','6:procedural',
        jsonb_build_object('segment_id',seg.segment_id,'clause_id',seg.clause_id,'stable_key',seg.stable_key),NULL,
        jsonb_build_object('lifecycle_status','current'),jsonb_build_object('action','reclassify_segment')));
      hard_issue_count := hard_issue_count + 1;
    ELSIF seg.effective_requirement_kind IS NULL THEN
      issues := issues || jsonb_build_array(kb_bid_gate_issue('PROCEDURAL_CLASSIFICATION_REVIEW','6:procedural',
        jsonb_build_object('segment_id',seg.segment_id,'clause_id',seg.clause_id,'stable_key',seg.stable_key),
        jsonb_build_object('classification_id',seg.classification_id,'revision',seg.classification_revision,
          'router_result_status',seg.router_result_status,'review_reason',seg.review_reason),
        jsonb_build_object('effective_requirement_kind','non_null'),jsonb_build_object('action','override_or_split_segment')));
      hard_issue_count := hard_issue_count + 1;
    ELSIF seg.decision_id IS NULL THEN
      issues := issues || jsonb_build_array(kb_bid_gate_issue('PROCEDURAL_DECISION_MISSING','6:procedural',
        jsonb_build_object('segment_id',seg.segment_id,'clause_id',seg.clause_id,'stable_key',seg.stable_key),
        jsonb_build_object('classification_id',seg.classification_id,'revision',seg.classification_revision,
          'effective_requirement_kind',seg.effective_requirement_kind),
        jsonb_build_object('resolution','required'),jsonb_build_object('action','resolve_procedural_requirement')));
      hard_issue_count := hard_issue_count + 1;
    ELSIF seg.resolution='not_applicable' THEN
      issues := issues || jsonb_build_array(kb_bid_gate_issue('PROCEDURAL_NOT_APPLICABLE','6:procedural',
        jsonb_build_object('segment_id',seg.segment_id,'clause_id',seg.clause_id,'stable_key',seg.stable_key),
        jsonb_build_object('decision_id',seg.decision_id,'revision',seg.decision_revision,
          'resolution',seg.resolution,'reason',seg.reason,'actor_identity',seg.actor_identity),
        jsonb_build_object('frozen',true),jsonb_build_object('action','review_not_applicable_reason')));
      warning_issue_count := warning_issue_count + 1;
    ELSIF seg.resolution='satisfied_by_attachment' AND (
      seg.validation_status='valid' AND seg.attachment_status='confirmed'
      AND seg.attachment_kind=seg.effective_requirement_kind) IS NOT TRUE THEN
      issues := issues || jsonb_build_array(kb_bid_gate_issue('ATTACHMENT_NOT_VALID','6:authorization',
        jsonb_build_object('segment_id',seg.segment_id,'clause_id',seg.clause_id,
          'classification_id',seg.classification_id,'decision_id',seg.decision_id,'attachment_id',seg.attachment_id),
        jsonb_build_object('attachment_id',seg.attachment_id,'revision',seg.attachment_revision,
          'digest',seg.attachment_sha256,'validation_sha256',seg.validation_sha256,
          'kind',seg.attachment_kind,'validation_status',seg.validation_status,'status',seg.attachment_status),
        jsonb_build_object('attachment_id',seg.attachment_id,'kind',seg.effective_requirement_kind,
          'validation_status','valid','status','confirmed','validation_sha256','non_null'),
        jsonb_build_object('action','validate_and_confirm_attachment')));
      hard_issue_count := hard_issue_count + 1;
    END IF;
  END LOOP;
  FOR pending IN
    SELECT id,revision,confirmation_required_reason FROM bid_clauses
     WHERE project_id=p_project_id AND confirmation_required_reason IS NOT NULL AND status IN ('draft','confirmed')
     ORDER BY id
  LOOP
    issues := issues || jsonb_build_array(kb_bid_gate_issue('KIND_ROUTER_RECONFIRMATION_REQUIRED','5',
      jsonb_build_object('clause_id',pending.id),
      jsonb_build_object('revision',pending.revision,'confirmation_required_reason',pending.confirmation_required_reason),
      jsonb_build_object('confirmation_required_reason',NULL),jsonb_build_object('action','reconfirm_clause')));
    hard_issue_count := hard_issue_count + 1;
  END LOOP;
  required := kb_bid_required_part_keys(p_project_id);
  FOREACH key IN ARRAY required LOOP
    SELECT current_value.*, dependency.part_content_artifact_id AS dependency_content_artifact_id,
           dependency.content_sha256 AS dependency_sha256, dependency.template_version,
           dependency.typed_input_identities,
           template.version AS current_template_version
      INTO part
      FROM bid_current_parts current_value
      JOIN bid_part_dependency_artifacts dependency ON dependency.id=current_value.dependency_artifact_id
      JOIN bid_template_contract_current template ON template.slot=dependency.template_slot
     WHERE current_value.project_id=p_project_id AND current_value.part_key=key;
    IF part.project_id IS NULL THEN
      issues := issues || jsonb_build_array(kb_bid_gate_issue('PART_MISSING',key,
        jsonb_build_object('project_id',p_project_id,'part_key',key),NULL,
        jsonb_build_object('present',true),jsonb_build_object('action','generate_part')));
      hard_issue_count := hard_issue_count + 1;
    ELSIF part.stale THEN
      issues := issues || jsonb_build_array(kb_bid_gate_issue('PART_STALE',key,
        jsonb_build_object('project_id',p_project_id,'part_key',key),
        jsonb_build_object('content_artifact_id',part.content_artifact_id,
          'dependency_artifact_id',part.dependency_artifact_id,'stale',true,
          'stale_reason_codes',part.stale_reason_codes),
        jsonb_build_object('stale',false),jsonb_build_object('action','regenerate_part')));
      hard_issue_count := hard_issue_count + 1;
    ELSIF part.dependency_content_artifact_id<>part.content_artifact_id
       OR part.template_version<>part.current_template_version
       OR part.typed_input_identities IS DISTINCT FROM kb_bid_current_part_input_identities(p_project_id,key) THEN
      issues := issues || jsonb_build_array(kb_bid_gate_issue('DEPENDENCY_NOT_CURRENT',key,
        jsonb_build_object('project_id',p_project_id,'part_key',key),
        jsonb_build_object('dependency_artifact_id',part.dependency_artifact_id,
          'dependency_sha256',part.dependency_sha256,
          'part_content_artifact_id',part.dependency_content_artifact_id,
          'template_version',part.template_version,
          'typed_input_identities',part.typed_input_identities),
        jsonb_build_object('part_content_artifact_id',part.content_artifact_id,
          'template_version',part.current_template_version,
          'typed_input_identities',kb_bid_current_part_input_identities(p_project_id,key)),
        jsonb_build_object('action','rebuild_dependency')));
      hard_issue_count := hard_issue_count + 1;
    END IF;
  END LOOP;
  RETURN jsonb_build_object('format',p_format,'status',CASE
      WHEN hard_issue_count=0 AND (p_format='pdf' OR warning_issue_count=0) THEN 'pass'
      WHEN p_format='docx' THEN 'warning'
      ELSE 'reject' END,
    'issues',issues,'required_part_keys',to_jsonb(required));
END
$$;

CREATE FUNCTION kb_bid_submission_end_state(p_project_id uuid, p_format text)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE gate jsonb; required text[]; parts jsonb; assets_with_validity jsonb; assets jsonb;
  quote_eligible boolean; asset_count integer; asset_bytes numeric;
BEGIN
  IF p_format NOT IN ('docx','pdf') THEN
    RAISE EXCEPTION 'SUBMISSION_FORMAT_INVALID' USING ERRCODE='22023';
  END IF;
  gate := kb_bid_list_gate_issues(p_project_id,p_format);
  required := ARRAY(SELECT jsonb_array_elements_text(gate->'required_part_keys'));
  quote_eligible := NOT EXISTS (
    SELECT 1 FROM jsonb_array_elements(gate->'issues') issue WHERE issue->>'code'='QUOTE_NOT_FINALIZED');

  WITH required_part AS (
    SELECT key,ordinality-1 AS ordinal
      FROM unnest(required) WITH ORDINALITY AS required_value(key,ordinality)
  ), state AS (
    SELECT required_part.key,required_part.ordinal,current_value.content_artifact_id,
           current_value.dependency_artifact_id,content.revision AS content_revision,
           content.content_sha256,dependency.content_sha256 AS dependency_sha256,
           dependency.template_slot,dependency.template_version,dependency.typed_input_identities,
           (current_value.project_id IS NULL
             OR (p_format='docx' AND required_part.key='6:quote' AND NOT quote_eligible)) AS is_placeholder
      FROM required_part
      LEFT JOIN bid_current_parts current_value
        ON current_value.project_id=p_project_id AND current_value.part_key=required_part.key
      LEFT JOIN bid_part_content_artifacts content ON content.id=current_value.content_artifact_id
      LEFT JOIN bid_part_dependency_artifacts dependency ON dependency.id=current_value.dependency_artifact_id
  )
  SELECT COALESCE(jsonb_agg(jsonb_build_object(
      'ordinal',state.ordinal,'part_key',state.key,'is_placeholder',state.is_placeholder,
      'content_artifact_id',CASE WHEN state.is_placeholder THEN NULL ELSE state.content_artifact_id END,
      'content_revision',CASE WHEN state.is_placeholder THEN NULL ELSE state.content_revision END,
      'content_sha256',CASE WHEN state.is_placeholder THEN NULL ELSE state.content_sha256 END,
      'dependency_artifact_id',CASE WHEN state.is_placeholder THEN NULL ELSE state.dependency_artifact_id END,
      'dependency_sha256',CASE WHEN state.is_placeholder THEN NULL ELSE state.dependency_sha256 END,
      'template_slot',COALESCE(state.template_slot,kb_bid_template_slot(state.key)),
      'template_version',COALESCE(state.template_version,template.version),
      'typed_input_identities',CASE WHEN state.is_placeholder THEN NULL ELSE state.typed_input_identities END,
      'placeholder_markdown',CASE WHEN state.is_placeholder THEN CASE WHEN state.key='6:quote'
        THEN '> [报价尚未最终确认]' ELSE '> [该部分尚未生成：'||state.key||']' END ELSE NULL END,
      'placeholder_sha256',CASE WHEN state.is_placeholder THEN encode(public.digest(convert_to(
        CASE WHEN state.key='6:quote' THEN '> [报价尚未最终确认]'
             ELSE '> [该部分尚未生成：'||state.key||']' END,'UTF8'),'sha256'),'hex') ELSE NULL END
    ) ORDER BY state.ordinal),'[]'::jsonb)
    INTO parts
    FROM state
    JOIN bid_template_contract_current template
      ON template.slot=COALESCE(state.template_slot,kb_bid_template_slot(state.key));

  WITH required_part AS (
    SELECT key,ordinality-1 AS part_ordinal
      FROM unnest(required) WITH ORDINALITY AS required_value(key,ordinality)
  ), renderable_part AS (
    SELECT required_part.key,required_part.part_ordinal,content.canonical_markdown_utf8
      FROM required_part
      JOIN bid_current_parts current_value
        ON current_value.project_id=p_project_id AND current_value.part_key=required_part.key
      JOIN bid_part_content_artifacts content ON content.id=current_value.content_artifact_id
     WHERE NOT (p_format='docx' AND required_part.key='6:quote' AND NOT quote_eligible)
  ), markdown_occurrence AS (
    SELECT part.key AS part_key,part.part_ordinal,
           CASE WHEN part.key='3' THEN 1 ELSE 0 END AS source_rank,
           occurrence.ordinality-1 AS occurrence_ordinal,
           occurrence.ordinality-1 AS render_order,
           'markdown_object'::text AS source_kind,
           jsonb_build_object('part_key',part.key,'occurrence',occurrence.ordinality-1) AS source_locator,
           'objects/'||occurrence.matches[1] AS object_ref,
           registry.digest,registry.media_type,registry.byte_length,
           source_image.pixel_width,source_image.pixel_height,
           registry.object_ref IS NOT NULL
             AND registry.media_type IN ('image/png','image/jpeg','image/webp')
             AND registry.byte_length BETWEEN 1 AND 20971520
             AND source_image.id IS NOT NULL AS valid
      FROM renderable_part part
      CROSS JOIN LATERAL regexp_matches(convert_from(part.canonical_markdown_utf8,'UTF8'),
        '![[][^]]*[]][(]objects/([0-9a-f]{64})[)]','g')
        WITH ORDINALITY AS occurrence(matches,ordinality)
      LEFT JOIN available_object_registry registry
        ON registry.object_ref='objects/'||occurrence.matches[1]
      LEFT JOIN LATERAL (
        SELECT shot_owner.id,shot_owner.pixel_width,shot_owner.pixel_height
          FROM bid_shot_artifacts shot_owner
         WHERE shot_owner.project_id=p_project_id
           AND shot_owner.object_ref='objects/'||occurrence.matches[1]
           AND EXISTS (SELECT 1 FROM object_owner_references owner_ref
             WHERE owner_ref.object_ref=shot_owner.object_ref AND owner_ref.owner_kind='bid_shot'
               AND owner_ref.owner_id=shot_owner.id AND owner_ref.occurrence='artifact')
         ORDER BY shot_owner.id LIMIT 1
      ) source_image ON true
  ), shot_occurrence AS (
    SELECT required_part.key AS part_key,required_part.part_ordinal,0 AS source_rank,
           placement.ordinal AS occurrence_ordinal,placement.ordinal AS render_order,
           'bid_shot'::text AS source_kind,
           jsonb_build_object('placement_ordinal',placement.ordinal,'shot_artifact_id',shot.id) AS source_locator,
           shot.object_ref,registry.digest,registry.media_type,registry.byte_length,
           shot.pixel_width,shot.pixel_height,
           registry.object_ref IS NOT NULL AND registry.digest=shot.content_sha256
             AND registry.media_type=shot.media_type AND registry.byte_length=shot.byte_length
             AND registry.media_type IN ('image/png','image/jpeg','image/webp')
             AND registry.byte_length BETWEEN 1 AND 20971520
             AND EXISTS (SELECT 1 FROM object_owner_references owner_ref
               WHERE owner_ref.object_ref=shot.object_ref AND owner_ref.owner_kind='bid_shot'
                 AND owner_ref.owner_id=shot.id AND owner_ref.occurrence='artifact') AS valid
      FROM required_part
      JOIN bid_current_shot_placements placement ON placement.project_id=p_project_id
      JOIN bid_shot_artifacts shot
        ON shot.project_id=placement.project_id AND shot.id=placement.shot_artifact_id
      LEFT JOIN available_object_registry registry ON registry.object_ref=shot.object_ref
     WHERE required_part.key='3'
  ), selected_attachment AS (
    SELECT DISTINCT attachment.id,attachment.kind,attachment.object_ref,attachment.content_sha256,
           attachment.media_type,attachment.byte_length,attachment.pixel_width,attachment.pixel_height,
           CASE WHEN attachment.kind='authorization_support'
                THEN '6:authorization' ELSE '6:procedural' END AS part_key
      FROM bid_procedural_decision_artifacts decision
      JOIN bid_procedural_classification_artifacts classification
        ON classification.id=decision.classification_id AND classification.lifecycle_status='current'
      JOIN bid_procedural_segment_artifacts segment
        ON segment.id=classification.segment_id
      JOIN bid_procedural_attachments attachment ON attachment.id=decision.attachment_id
     WHERE segment.project_id=p_project_id AND decision.lifecycle_status='current'
       AND decision.resolution='satisfied_by_attachment'
       AND attachment.project_id=p_project_id AND attachment.status='confirmed'
       AND attachment.validation_status='valid'
       AND attachment.kind=classification.effective_requirement_kind
  ), attachment_rank AS (
    SELECT selected_attachment.*,
           row_number() OVER (ORDER BY part_key,kind,id)-1 AS attachment_ordinal
      FROM selected_attachment
  ), attachment_original AS (
    SELECT required_part.key AS part_key,required_part.part_ordinal,2 AS source_rank,
           attachment.attachment_ordinal AS occurrence_ordinal,
           attachment.attachment_ordinal*513 AS render_order,
           'procedural_attachment'::text AS source_kind,
           jsonb_build_object('part_key',attachment.part_key,
             'attachment_ordinal',attachment.attachment_ordinal,
             'attachment_id',attachment.id,'kind',attachment.kind) AS source_locator,
           attachment.object_ref,registry.digest,registry.media_type,registry.byte_length,
           attachment.pixel_width,attachment.pixel_height,
           registry.object_ref IS NOT NULL AND registry.digest=attachment.content_sha256
             AND registry.media_type=attachment.media_type AND registry.byte_length=attachment.byte_length
             AND EXISTS (SELECT 1 FROM object_owner_references owner_ref
               WHERE owner_ref.object_ref=attachment.object_ref AND owner_ref.owner_kind='bid_attachment'
                 AND owner_ref.owner_id=attachment.id AND owner_ref.occurrence='original') AS valid
      FROM required_part
      JOIN attachment_rank attachment ON attachment.part_key=required_part.key
      LEFT JOIN available_object_registry registry ON registry.object_ref=attachment.object_ref
  ), attachment_page AS (
    SELECT required_part.key AS part_key,required_part.part_ordinal,2 AS source_rank,
           page.page_ordinal AS occurrence_ordinal,
           attachment.attachment_ordinal*513+page.page_ordinal+1 AS render_order,
           'procedural_attachment_page'::text AS source_kind,
           jsonb_build_object('part_key',attachment.part_key,
             'attachment_ordinal',attachment.attachment_ordinal,
             'attachment_id',attachment.id,'page_ordinal',page.page_ordinal) AS source_locator,
           page.object_ref,registry.digest,registry.media_type,registry.byte_length,
           page.pixel_width,page.pixel_height,
           registry.object_ref IS NOT NULL AND registry.digest=page.content_sha256
             AND registry.media_type=page.media_type AND registry.byte_length=page.byte_length
             AND EXISTS (SELECT 1 FROM object_owner_references owner_ref
               WHERE owner_ref.object_ref=page.object_ref AND owner_ref.owner_kind='bid_attachment_page'
                 AND owner_ref.owner_id=attachment.id AND owner_ref.occurrence=page.page_ordinal::text) AS valid
      FROM required_part
      JOIN attachment_rank attachment ON attachment.part_key=required_part.key
      JOIN bid_attachment_render_pages page ON page.attachment_id=attachment.id
      LEFT JOIN available_object_registry registry ON registry.object_ref=page.object_ref
  ), combined AS (
    SELECT * FROM markdown_occurrence
    UNION ALL SELECT * FROM shot_occurrence
    UNION ALL SELECT * FROM attachment_original
    UNION ALL SELECT * FROM attachment_page
  ), ordered AS (
    SELECT row_number() OVER (ORDER BY part_ordinal,source_rank,render_order,source_kind)-1 AS manifest_ordinal,
           combined.* FROM combined
  )
  SELECT COALESCE(jsonb_agg(jsonb_build_object(
      'manifest_ordinal',manifest_ordinal,'source_kind',source_kind,
      'source_locator',source_locator,'object_ref',object_ref,'digest',digest,
      'media_type',media_type,'byte_length',byte_length,
      'pixel_width',pixel_width,'pixel_height',pixel_height,
      'occurrence_ordinal',occurrence_ordinal,'valid',valid)
    ORDER BY manifest_ordinal),'[]'::jsonb)
    INTO assets_with_validity FROM ordered;

  IF EXISTS (SELECT 1 FROM jsonb_array_elements(assets_with_validity) asset
              WHERE (asset->>'valid')::boolean IS NOT TRUE) THEN
    RAISE EXCEPTION 'MANIFEST_ASSET_UNAVAILABLE_OR_INVALID' USING ERRCODE='23514';
  END IF;
  SELECT count(*),COALESCE(sum((asset->>'byte_length')::bigint),0),
         COALESCE(jsonb_agg(asset-'valid' ORDER BY (asset->>'manifest_ordinal')::integer),'[]'::jsonb)
    INTO asset_count,asset_bytes,assets
    FROM jsonb_array_elements(assets_with_validity) asset;
  IF asset_count>2048 OR asset_bytes>268435456 THEN
    RAISE EXCEPTION 'MANIFEST_ASSET_QUOTA_EXCEEDED' USING ERRCODE='22023';
  END IF;
  RETURN jsonb_build_object('schema_version',1,'project_id',p_project_id,'format',p_format,
    'renderer_contract',CASE WHEN p_format='pdf' THEN jsonb_build_object(
      'version','knowledgebrain.bid.pdf.v1',
      'font_sha256','5d0df56f107605387e0de494b22dfc7fb05d8d79ffd981474e7be11dbe571882')
      ELSE jsonb_build_object('version','knowledgebrain.bid.docx.v1') END,
    'required_part_keys',to_jsonb(required),'gate',gate,'parts',parts,'render_assets',assets);
END
$$;

CREATE FUNCTION kb_bid_create_submission_manifest(
  p_id uuid, p_project_id uuid, p_format text,
  p_actor kb_actor_identity, p_idempotency_key text, p_request_bytes bytea, p_request_sha256 kb_sha256
)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE replay bytea; project_value bid_projects%ROWTYPE; gate jsonb; keys text[];
 end_state jsonb; canonical jsonb; payload bytea; digest kb_sha256; response jsonb; asset record;
 now_ts timestamptz := clock_timestamp();
BEGIN
  PERFORM kb_bid_require_human_actor(p_actor);
  PERFORM 1 FROM application_maintenance_gate WHERE singleton_key AND mode='open' FOR SHARE;
  IF NOT FOUND THEN RAISE EXCEPTION 'MAINTENANCE_SUBMISSION_BLOCKED' USING ERRCODE='55000'; END IF;
  IF p_format='pdf' THEN PERFORM kb_bid_require_user_actor(p_actor); END IF;
  replay := kb_bid_idempotency_begin(p_actor,'bid.submission.create_manifest',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  SELECT * INTO STRICT project_value FROM bid_projects WHERE id=p_project_id FOR UPDATE;
  IF project_value.status<>'open' THEN RAISE EXCEPTION 'PROJECT_ENDED' USING ERRCODE='55000'; END IF;
  IF p_format NOT IN ('docx','pdf') THEN RAISE EXCEPTION 'SUBMISSION_FORMAT_INVALID' USING ERRCODE='22023'; END IF;
  PERFORM kb_bid_sync_project_procedural(p_project_id, p_actor);
  end_state := kb_bid_submission_end_state(p_project_id,p_format);
  gate := end_state->'gate';
  IF p_format='pdf' AND gate->>'status'<>'pass' THEN RAISE EXCEPTION 'SUBMISSION_GATE_REJECTED' USING ERRCODE='22023'; END IF;
  keys := ARRAY(SELECT jsonb_array_elements_text(end_state->'required_part_keys'));
  canonical := jsonb_build_object('schema_version',1,'manifest_id',p_id,'end_state',end_state,
    'created_by',p_actor,'created_at',kb_bid_utc_json_time(now_ts));
  payload := convert_to(canonical::text,'UTF8');
  digest := encode(public.digest(payload,'sha256'),'hex');
  INSERT INTO bid_submission_manifests(id,project_id,format,schema_version,required_part_keys,gate_status,gate_issues,
    end_state_identity,canonical_payload,content_sha256,created_by,created_at)
  VALUES(p_id,p_project_id,p_format,1,keys,CASE WHEN gate->>'status'='pass' THEN 'pass' ELSE 'warning' END,
    COALESCE(gate->'issues','[]'::jsonb),end_state,payload,digest,p_actor,now_ts);
  INSERT INTO bid_submission_gate_issues(manifest_id,ordinal,code,part_key,entity_locator,current_identity,expected_identity,remediation)
  SELECT p_id, (ord-1), item->>'code', item->>'part_key', COALESCE(item->'entity_locator','{}'::jsonb),
         item->'current_identity', item->'expected_identity', COALESCE(item->'remediation','{}'::jsonb)
    FROM jsonb_array_elements(COALESCE(gate->'issues','[]'::jsonb)) WITH ORDINALITY AS t(item,ord);
  INSERT INTO bid_submission_manifest_parts(manifest_id,ordinal,part_key,template_slot,template_version,content_artifact_id,
    dependency_artifact_id,placeholder_markdown_utf8,placeholder_sha256)
  SELECT p_id,(part->>'ordinal')::integer,part->>'part_key',part->>'template_slot',part->>'template_version',
    (part->>'content_artifact_id')::uuid,(part->>'dependency_artifact_id')::uuid,
    CASE WHEN (part->>'is_placeholder')::boolean THEN convert_to(part->>'placeholder_markdown','UTF8') END,
    part->>'placeholder_sha256'
    FROM jsonb_array_elements(end_state->'parts') part;
  INSERT INTO bid_manifest_render_assets(id,manifest_id,source_kind,source_locator,object_ref,digest,
    media_type,byte_length,pixel_width,pixel_height,manifest_ordinal,occurrence_ordinal)
  SELECT gen_random_uuid(),p_id,asset_value->>'source_kind',asset_value->'source_locator',asset_value->>'object_ref',
    asset_value->>'digest',asset_value->>'media_type',(asset_value->>'byte_length')::bigint,
    (asset_value->>'pixel_width')::integer,(asset_value->>'pixel_height')::integer,
    (asset_value->>'manifest_ordinal')::integer,(asset_value->>'occurrence_ordinal')::integer
    FROM jsonb_array_elements(end_state->'render_assets') asset_value;
  FOR asset IN SELECT * FROM bid_manifest_render_assets WHERE manifest_id=p_id ORDER BY id LOOP
    PERFORM kb_object_reference_add(asset.object_ref,asset.digest,asset.media_type,asset.byte_length,
      'bid_manifest_asset',p_id,asset.id::text,p_actor);
  END LOOP;
  response := jsonb_build_object('manifest_id',p_id,'content_sha256',digest,'gate_status',gate->>'status',
    'required_part_keys',to_jsonb(keys));
  INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
    entity_kind,entity_locator,after_revision,after_sha256)
  VALUES(gen_random_uuid(),1,'bid.submission.create_manifest',p_actor,p_idempotency_key,p_request_sha256,
    encode(public.digest(convert_to(response::text,'UTF8'),'sha256'),'hex'),'bid_submission_manifest',
    jsonb_build_object('manifest_id',p_id),1,digest);
  PERFORM kb_bid_idempotency_complete(p_actor,'bid.submission.create_manifest',p_idempotency_key,201,convert_to(response::text,'UTF8'));
  RETURN response;
END
$$;


CREATE FUNCTION kb_bid_manifest_render_input(p_project_id uuid, p_manifest_id uuid)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE manifest bid_submission_manifests%ROWTYPE; parts jsonb; assets jsonb;
BEGIN
  SELECT * INTO manifest FROM bid_submission_manifests
   WHERE project_id=p_project_id AND id=p_manifest_id;
  IF manifest.id IS NULL THEN
    RAISE EXCEPTION 'SUBMISSION_MANIFEST_MISSING' USING ERRCODE='P0002';
  END IF;
  SELECT COALESCE(jsonb_agg(jsonb_build_object(
      'ordinal',mp.ordinal,'part_key',mp.part_key,
      'markdown',convert_from(COALESCE(c.canonical_markdown_utf8,mp.placeholder_markdown_utf8),'UTF8'),
      'content_sha256',COALESCE(c.content_sha256,mp.placeholder_sha256),
      'is_placeholder',mp.content_artifact_id IS NULL) ORDER BY mp.ordinal),'[]'::jsonb)
    INTO parts
    FROM bid_submission_manifest_parts mp
    LEFT JOIN bid_part_content_artifacts c ON c.id=mp.content_artifact_id
   WHERE mp.manifest_id=p_manifest_id;
  SELECT COALESCE(jsonb_agg(jsonb_build_object(
      'id',a.id,'source_kind',a.source_kind,'digest',a.digest,
      'media_type',a.media_type,'occurrence_ordinal',a.occurrence_ordinal,
      'byte_length',a.byte_length,'pixel_width',a.pixel_width,'pixel_height',a.pixel_height,
      'manifest_ordinal',a.manifest_ordinal,
      'source_locator',a.source_locator) ORDER BY a.manifest_ordinal),'[]'::jsonb)
    INTO assets FROM bid_manifest_render_assets a WHERE a.manifest_id=p_manifest_id;
  RETURN jsonb_build_object('manifest_id',manifest.id,'project_id',manifest.project_id,'format',manifest.format,
    'content_sha256',manifest.content_sha256,'gate_status',manifest.gate_status,
    'renderer_contract',manifest.end_state_identity->'renderer_contract','parts',parts,'assets',assets);
END
$$;

CREATE FUNCTION kb_bid_read_manifest_render_asset(p_project_id uuid, p_manifest_id uuid, p_asset_id uuid)
RETURNS TABLE(object_ref kb_object_ref, digest kb_sha256, media_type text, byte_length bigint,
  pixel_width integer,pixel_height integer,source_kind text,source_locator jsonb,
  manifest_ordinal integer,occurrence_ordinal integer)
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
BEGIN
  RETURN QUERY
    SELECT asset.object_ref,asset.digest,asset.media_type,asset.byte_length,
           asset.pixel_width,asset.pixel_height,asset.source_kind,
           asset.source_locator,asset.manifest_ordinal,asset.occurrence_ordinal
      FROM bid_manifest_render_assets asset
      JOIN bid_submission_manifests manifest ON manifest.id=asset.manifest_id
      JOIN available_object_registry registry ON registry.object_ref=asset.object_ref
     WHERE manifest.project_id=p_project_id AND asset.manifest_id=p_manifest_id AND asset.id=p_asset_id
       AND registry.digest=asset.digest AND registry.media_type=asset.media_type
       AND registry.byte_length=asset.byte_length
       AND EXISTS (SELECT 1 FROM object_owner_references owner_ref
         WHERE owner_ref.object_ref=asset.object_ref AND owner_ref.owner_kind='bid_manifest_asset'
           AND owner_ref.owner_id=asset.manifest_id AND owner_ref.occurrence=asset.id::text);
  IF NOT FOUND THEN RAISE EXCEPTION 'MANIFEST_ASSET_MISSING' USING ERRCODE='P0002'; END IF;
END
$$;

CREATE FUNCTION kb_bid_verify_submission_manifest_v1()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE parsed jsonb; expected jsonb; parts jsonb; assets jsonb; issues jsonb;
BEGIN
  BEGIN
    parsed := convert_from(NEW.canonical_payload,'UTF8')::jsonb;
  EXCEPTION WHEN OTHERS THEN
    RAISE EXCEPTION 'SUBMISSION_MANIFEST_INVALID_JSON' USING ERRCODE='23514';
  END;
  expected := jsonb_build_object('schema_version',1,'manifest_id',NEW.id,
    'end_state',NEW.end_state_identity,'created_by',NEW.created_by,
    'created_at',kb_bid_utc_json_time(NEW.created_at));
  IF parsed IS DISTINCT FROM expected
     OR NEW.required_part_keys IS DISTINCT FROM ARRAY(
       SELECT jsonb_array_elements_text(NEW.end_state_identity->'required_part_keys'))
     OR NEW.gate_issues IS DISTINCT FROM COALESCE(NEW.end_state_identity#>'{gate,issues}','[]'::jsonb)
     OR NEW.gate_status IS DISTINCT FROM (CASE WHEN NEW.end_state_identity#>>'{gate,status}'='pass'
          THEN 'pass' ELSE 'warning' END) THEN
    RAISE EXCEPTION 'SUBMISSION_MANIFEST_PAYLOAD_MISMATCH' USING ERRCODE='23514';
  END IF;

  SELECT COALESCE(jsonb_agg(jsonb_build_object(
      'ordinal',manifest_part.ordinal,'part_key',manifest_part.part_key,
      'is_placeholder',manifest_part.content_artifact_id IS NULL,
      'content_artifact_id',content.id,'content_revision',content.revision,
      'content_sha256',content.content_sha256,
      'dependency_artifact_id',dependency.id,'dependency_sha256',dependency.content_sha256,
      'template_slot',manifest_part.template_slot,'template_version',manifest_part.template_version,
      'typed_input_identities',dependency.typed_input_identities,
      'placeholder_markdown',CASE WHEN manifest_part.placeholder_markdown_utf8 IS NULL THEN NULL
        ELSE convert_from(manifest_part.placeholder_markdown_utf8,'UTF8') END,
      'placeholder_sha256',manifest_part.placeholder_sha256) ORDER BY manifest_part.ordinal),'[]'::jsonb)
    INTO parts
    FROM bid_submission_manifest_parts manifest_part
    LEFT JOIN bid_part_content_artifacts content ON content.id=manifest_part.content_artifact_id
    LEFT JOIN bid_part_dependency_artifacts dependency ON dependency.id=manifest_part.dependency_artifact_id
   WHERE manifest_part.manifest_id=NEW.id;
  IF parts IS DISTINCT FROM COALESCE(NEW.end_state_identity->'parts','[]'::jsonb)
     OR EXISTS (
       SELECT 1 FROM bid_submission_manifest_parts manifest_part
       JOIN bid_part_dependency_artifacts dependency ON dependency.id=manifest_part.dependency_artifact_id
       JOIN bid_part_content_artifacts content ON content.id=manifest_part.content_artifact_id
       WHERE manifest_part.manifest_id=NEW.id
         AND (dependency.project_id<>NEW.project_id OR content.project_id<>NEW.project_id
           OR dependency.part_key<>manifest_part.part_key OR content.part_key<>manifest_part.part_key
           OR dependency.part_content_artifact_id<>manifest_part.content_artifact_id
           OR dependency.template_slot<>manifest_part.template_slot
           OR dependency.template_version<>manifest_part.template_version)) THEN
    RAISE EXCEPTION 'SUBMISSION_MANIFEST_PART_RELATION_MISMATCH' USING ERRCODE='23514';
  END IF;

  SELECT COALESCE(jsonb_agg(jsonb_build_object(
      'manifest_ordinal',asset.manifest_ordinal,'source_kind',asset.source_kind,
      'source_locator',asset.source_locator,'object_ref',asset.object_ref,'digest',asset.digest,
      'media_type',asset.media_type,'byte_length',asset.byte_length,
      'pixel_width',asset.pixel_width,'pixel_height',asset.pixel_height,
      'occurrence_ordinal',asset.occurrence_ordinal) ORDER BY asset.manifest_ordinal),'[]'::jsonb)
    INTO assets FROM bid_manifest_render_assets asset WHERE asset.manifest_id=NEW.id;
  IF assets IS DISTINCT FROM COALESCE(NEW.end_state_identity->'render_assets','[]'::jsonb)
     OR EXISTS (
       SELECT 1 FROM bid_manifest_render_assets asset
       LEFT JOIN object_owner_references owner_ref
         ON owner_ref.object_ref=asset.object_ref AND owner_ref.owner_kind='bid_manifest_asset'
        AND owner_ref.owner_id=asset.manifest_id AND owner_ref.occurrence=asset.id::text
       WHERE asset.manifest_id=NEW.id AND owner_ref.object_ref IS NULL) THEN
    RAISE EXCEPTION 'SUBMISSION_MANIFEST_ASSET_RELATION_MISMATCH' USING ERRCODE='23514';
  END IF;

  SELECT COALESCE(jsonb_agg(kb_bid_gate_issue(issue.code,issue.part_key,issue.entity_locator,
      issue.current_identity,issue.expected_identity,issue.remediation) ORDER BY issue.ordinal),'[]'::jsonb)
    INTO issues FROM bid_submission_gate_issues issue WHERE issue.manifest_id=NEW.id;
  IF issues IS DISTINCT FROM NEW.gate_issues THEN
    RAISE EXCEPTION 'SUBMISSION_MANIFEST_GATE_RELATION_MISMATCH' USING ERRCODE='23514';
  END IF;
  RETURN NEW;
END
$$;
CREATE CONSTRAINT TRIGGER bid_submission_manifests_verify
AFTER INSERT ON bid_submission_manifests DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION kb_bid_verify_submission_manifest_v1();

CREATE FUNCTION kb_bid_schedule_submission_render(
  p_id uuid, p_project_id uuid, p_manifest_id uuid, p_expected_manifest_sha256 kb_sha256,
  p_actor kb_actor_identity, p_idempotency_key text, p_request_bytes bytea, p_request_sha256 kb_sha256
)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE replay bytea; manifest bid_submission_manifests%ROWTYPE;
 job bid_submission_render_jobs%ROWTYPE; response jsonb; now_ts timestamptz := clock_timestamp();
BEGIN
  PERFORM kb_bid_require_human_actor(p_actor);
  PERFORM 1 FROM application_maintenance_gate WHERE singleton_key AND mode='open' FOR SHARE;
  IF NOT FOUND THEN RAISE EXCEPTION 'MAINTENANCE_SUBMISSION_BLOCKED' USING ERRCODE='55000'; END IF;
  replay := kb_bid_idempotency_begin(p_actor,'bid.submission.schedule_render',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  SELECT * INTO manifest FROM bid_submission_manifests
   WHERE project_id=p_project_id AND id=p_manifest_id FOR SHARE;
  IF manifest.id IS NULL THEN RAISE EXCEPTION 'SUBMISSION_MANIFEST_MISSING' USING ERRCODE='P0002'; END IF;
  IF manifest.format='pdf' THEN PERFORM kb_bid_require_user_actor(p_actor); END IF;
  IF manifest.content_sha256<>p_expected_manifest_sha256 THEN
    RAISE EXCEPTION 'MANIFEST_SHA256_MISMATCH' USING ERRCODE='40001';
  END IF;
  INSERT INTO bid_submission_render_jobs(
    id,project_id,manifest_id,expected_manifest_sha256,requested_by,idempotency_key,
    status,created_at
  ) VALUES(
    p_id,p_project_id,p_manifest_id,p_expected_manifest_sha256,p_actor,p_idempotency_key,
    'pending',now_ts
  )
  ON CONFLICT (manifest_id) DO NOTHING;
  SELECT * INTO STRICT job FROM bid_submission_render_jobs WHERE manifest_id=p_manifest_id FOR UPDATE;
  IF job.project_id<>p_project_id OR job.expected_manifest_sha256<>p_expected_manifest_sha256 THEN
    RAISE EXCEPTION 'SUBMISSION_RENDER_IDENTITY_MISMATCH' USING ERRCODE='40001';
  END IF;
  response := jsonb_build_object(
    'render_job_id',job.id,'manifest_id',job.manifest_id,'status',job.status,
    'attempt_count',job.attempt_count,'created_at',kb_bid_utc_json_time(job.created_at)
  );
  INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
    entity_kind,entity_locator,after_revision,after_sha256)
  VALUES(gen_random_uuid(),1,'bid.submission.schedule_render',p_actor,p_idempotency_key,p_request_sha256,
    encode(public.digest(convert_to(response::text,'UTF8'),'sha256'),'hex'),'bid_submission_render_job',
    jsonb_build_object('render_job_id',job.id),1,job.expected_manifest_sha256);
  PERFORM kb_bid_idempotency_complete(p_actor,'bid.submission.schedule_render',p_idempotency_key,202,convert_to(response::text,'UTF8'));
  RETURN response;
END
$$;

CREATE FUNCTION kb_bid_get_submission_render_job(p_project_id uuid, p_render_job_id uuid)
RETURNS jsonb
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
  SELECT jsonb_strip_nulls(jsonb_build_object(
    'render_job_id',job.id,'manifest_id',job.manifest_id,'status',job.status,
    'attempt_count',job.attempt_count,'max_attempts',job.max_attempts,
    'output_id',job.output_artifact_id,'error_code',job.error_code,
    'created_at',kb_bid_utc_json_time(job.created_at),
    'started_at',CASE WHEN job.started_at IS NULL THEN NULL ELSE kb_bid_utc_json_time(job.started_at) END,
    'finished_at',CASE WHEN job.finished_at IS NULL THEN NULL ELSE kb_bid_utc_json_time(job.finished_at) END
  ))
  FROM bid_submission_render_jobs job
  WHERE job.project_id=p_project_id AND job.id=p_render_job_id
$$;

CREATE FUNCTION kb_bid_claim_submission_render(p_render_job_id uuid, p_claim_token uuid)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE job bid_submission_render_jobs%ROWTYPE; now_ts timestamptz := clock_timestamp();
BEGIN
  IF p_claim_token IS NULL THEN RAISE EXCEPTION 'SUBMISSION_RENDER_CLAIM_TOKEN_REQUIRED' USING ERRCODE='22023'; END IF;
  SELECT * INTO job FROM bid_submission_render_jobs WHERE id=p_render_job_id FOR UPDATE;
  IF job.id IS NULL THEN RAISE EXCEPTION 'SUBMISSION_RENDER_JOB_MISSING' USING ERRCODE='P0002'; END IF;
  IF job.status<>'pending' THEN RETURN NULL; END IF;
  UPDATE bid_submission_render_jobs
     SET status='running',attempt_count=attempt_count+1,claim_token=p_claim_token,
         heartbeat_at=now_ts,started_at=COALESCE(started_at,now_ts),error_code=NULL,error_detail=NULL
   WHERE id=p_render_job_id
   RETURNING * INTO STRICT job;
  RETURN jsonb_build_object(
    'render_job_id',job.id,'project_id',job.project_id,'manifest_id',job.manifest_id,
    'expected_manifest_sha256',job.expected_manifest_sha256,'requested_by',job.requested_by,
    'idempotency_key',job.idempotency_key,'attempt_count',job.attempt_count,
    'max_attempts',job.max_attempts,'claim_lease_ms',job.claim_lease_ms
  );
END
$$;

CREATE FUNCTION kb_bid_heartbeat_submission_render(p_render_job_id uuid, p_claim_token uuid)
RETURNS boolean
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE updated integer;
BEGIN
  UPDATE bid_submission_render_jobs SET heartbeat_at=clock_timestamp()
   WHERE id=p_render_job_id AND status='running' AND claim_token=p_claim_token
     AND heartbeat_at+make_interval(secs=>claim_lease_ms::double precision/1000.0)>clock_timestamp();
  GET DIAGNOSTICS updated = ROW_COUNT;
  RETURN updated=1;
END
$$;

CREATE FUNCTION kb_bid_fail_submission_render(
  p_render_job_id uuid, p_claim_token uuid, p_error_code text, p_error_detail text, p_retryable boolean
)
RETURNS text
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE job bid_submission_render_jobs%ROWTYPE; next_status text;
BEGIN
  SELECT * INTO job FROM bid_submission_render_jobs
   WHERE id=p_render_job_id AND status='running' AND claim_token=p_claim_token
     AND heartbeat_at+make_interval(secs=>claim_lease_ms::double precision/1000.0)>clock_timestamp()
   FOR UPDATE;
  IF job.id IS NULL THEN RETURN NULL; END IF;
  next_status := CASE WHEN p_retryable AND job.attempt_count<job.max_attempts THEN 'pending' ELSE 'failed' END;
  UPDATE bid_submission_render_jobs
     SET status=next_status,claim_token=NULL,heartbeat_at=NULL,
         error_code=left(COALESCE(NULLIF(p_error_code,''),'SUBMISSION_RENDER_FAILED'),128),
         error_detail=left(COALESCE(p_error_detail,''),4096),
         finished_at=CASE WHEN next_status='failed' THEN clock_timestamp() ELSE NULL END
   WHERE id=p_render_job_id;
  RETURN next_status;
END
$$;

CREATE FUNCTION kb_bid_reap_submission_renders()
RETURNS integer
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE updated integer;
BEGIN
  UPDATE bid_submission_render_jobs
     SET status=CASE WHEN attempt_count>=max_attempts THEN 'failed' ELSE 'pending' END,
         claim_token=NULL,heartbeat_at=NULL,error_code='CLAIM_LEASE_EXPIRED',
         error_detail='render worker claim lease expired',
         finished_at=CASE WHEN attempt_count>=max_attempts THEN clock_timestamp() ELSE NULL END
   WHERE status='running'
     AND heartbeat_at + make_interval(secs => claim_lease_ms::double precision/1000.0) <= clock_timestamp();
  GET DIAGNOSTICS updated = ROW_COUNT;
  RETURN updated;
END
$$;

CREATE FUNCTION kb_bid_pending_submission_renders()
RETURNS uuid[]
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
  SELECT COALESCE(array_agg(job.id ORDER BY job.created_at,job.id),'{}'::uuid[])
  FROM bid_submission_render_jobs job WHERE job.status='pending'
$$;

CREATE FUNCTION kb_bid_publish_submission_output(
  p_staging_id uuid, p_id uuid, p_render_job_id uuid, p_claim_token uuid,
  p_object_ref kb_object_ref, p_digest kb_sha256, p_byte_length bigint
)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE replay bytea; job bid_submission_render_jobs%ROWTYPE; manifest bid_submission_manifests%ROWTYPE;
 project_value bid_projects%ROWTYPE; current_state jsonb; response jsonb;
 request_payload jsonb; request_bytes bytea; request_sha256 kb_sha256;
BEGIN
  SELECT * INTO job FROM bid_submission_render_jobs
   WHERE id=p_render_job_id AND status='running' AND claim_token=p_claim_token
     AND heartbeat_at+make_interval(secs=>claim_lease_ms::double precision/1000.0)>clock_timestamp()
   FOR UPDATE;
  IF job.id IS NULL THEN RAISE EXCEPTION 'SUBMISSION_RENDER_CLAIM_LOST' USING ERRCODE='40001'; END IF;
  PERFORM kb_bid_require_human_actor(job.requested_by);
  request_payload := jsonb_build_object('render_job_id',job.id,'expected_manifest_sha256',job.expected_manifest_sha256);
  request_bytes := convert_to(request_payload::text,'UTF8');
  request_sha256 := encode(public.digest(request_bytes,'sha256'),'hex');
  replay := kb_bid_idempotency_begin(job.requested_by,'bid.submission.publish_output',job.idempotency_key,request_bytes,request_sha256);
  IF replay IS NOT NULL THEN
    PERFORM kb_object_upload_abandon(p_staging_id,job.requested_by);
    RETURN convert_from(replay,'UTF8')::jsonb;
  END IF;
  SELECT * INTO STRICT manifest FROM bid_submission_manifests
   WHERE project_id=job.project_id AND id=job.manifest_id FOR UPDATE;
  IF manifest.format='pdf' THEN PERFORM kb_bid_require_user_actor(job.requested_by); END IF;
  IF manifest.content_sha256<>job.expected_manifest_sha256 THEN RAISE EXCEPTION 'MANIFEST_SHA256_MISMATCH' USING ERRCODE='40001'; END IF;
  SELECT * INTO STRICT project_value FROM bid_projects WHERE id=manifest.project_id FOR UPDATE;
  IF project_value.status<>'open' THEN RAISE EXCEPTION 'PROJECT_ENDED' USING ERRCODE='55000'; END IF;
  current_state := kb_bid_submission_end_state(manifest.project_id,manifest.format);
  IF current_state IS DISTINCT FROM manifest.end_state_identity THEN
    RAISE EXCEPTION 'SUBMISSION_END_STATE_CHANGED' USING ERRCODE='40001';
  END IF;
  PERFORM kb_object_upload_commit(p_staging_id,p_object_ref,p_digest,CASE WHEN manifest.format='pdf'
      THEN 'application/pdf'
      ELSE 'application/vnd.openxmlformats-officedocument.wordprocessingml.document' END,p_byte_length,
    'bid_submission_output',p_id,'rendered',job.requested_by);
  INSERT INTO bid_submission_output_artifacts(id,manifest_id,project_id,format,object_ref,content_sha256,byte_length,rendered_at)
  VALUES(p_id,job.manifest_id,manifest.project_id,manifest.format,p_object_ref,p_digest,p_byte_length,clock_timestamp());
  INSERT INTO bid_current_submission_outputs(project_id,format,output_artifact_id)
  VALUES(manifest.project_id,manifest.format,p_id)
  ON CONFLICT (project_id, format) DO UPDATE SET output_artifact_id=EXCLUDED.output_artifact_id;
  UPDATE bid_submission_render_jobs
     SET status='completed',claim_token=NULL,heartbeat_at=NULL,output_artifact_id=p_id,
         error_code=NULL,error_detail=NULL,finished_at=clock_timestamp()
   WHERE id=job.id;
  response := jsonb_build_object('output_id',p_id,'manifest_id',job.manifest_id,'object_ref',p_object_ref,
    'content_sha256',p_digest,'format',manifest.format);
  INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
    entity_kind,entity_locator,after_revision,after_sha256)
  VALUES(gen_random_uuid(),1,'bid.submission.publish_output',job.requested_by,job.idempotency_key,request_sha256,
    encode(public.digest(convert_to(response::text,'UTF8'),'sha256'),'hex'),'bid_submission_output',
    jsonb_build_object('output_id',p_id),1,p_digest);
  PERFORM kb_bid_idempotency_complete(job.requested_by,'bid.submission.publish_output',job.idempotency_key,201,convert_to(response::text,'UTF8'));
  RETURN response;
END
$$;

CREATE FUNCTION kb_bid_housekeep_end_expired()
RETURNS integer
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE n int;
BEGIN
  UPDATE bid_projects SET status='ended', ended_at=clock_timestamp(), updated_at=clock_timestamp()
   WHERE status='open' AND ends_at <= clock_timestamp();
  GET DIAGNOSTICS n = ROW_COUNT;
  RETURN n;
END
$$;

CREATE FUNCTION kb_bid_reclaim_stale_conversions()
RETURNS uuid[]
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE ids uuid[] := '{}'; rec record;
BEGIN
  FOR rec IN
    SELECT d.id,d.conversion_generation,a.attempt,a.claim_token FROM bid_documents d
    JOIN bid_document_conversion_attempts a ON a.document_id=d.id AND a.conversion_generation=d.conversion_generation
    WHERE d.parse_status='processing' AND a.status='running'
      AND a.heartbeat_at + make_interval(secs => a.claim_lease_ms/1000.0) < clock_timestamp()
    FOR UPDATE OF d,a
  LOOP
    UPDATE bid_document_conversion_attempts SET status='reaped'
     WHERE document_id=rec.id AND conversion_generation=rec.conversion_generation
       AND attempt=rec.attempt AND claim_token=rec.claim_token AND status='running'
       AND heartbeat_at + make_interval(secs => claim_lease_ms/1000.0) < clock_timestamp();
    CONTINUE WHEN NOT FOUND;
    UPDATE bid_documents SET parse_status='pending'
     WHERE id=rec.id AND conversion_generation=rec.conversion_generation AND parse_status='processing';
    CONTINUE WHEN NOT FOUND;
    ids := ids || rec.id;
  END LOOP;
  RETURN ids;
END
$$;

CREATE FUNCTION kb_bid_pending_conversions()
RETURNS uuid[]
LANGUAGE sql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$ SELECT COALESCE(array_agg(id ORDER BY created_at),'{}') FROM bid_documents WHERE parse_status='pending' $$;

CREATE FUNCTION kb_bid_reclaim_stale_extractions()
RETURNS TABLE(target_id uuid, project_id uuid, document_id uuid)
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
BEGIN
  RETURN QUERY
    WITH expired AS (
      SELECT t.id,a.attempt,a.claim_token
        FROM bid_extraction_targets t
        JOIN bid_extraction_attempts a ON a.target_id=t.id AND a.status='running'
       WHERE t.state='running'
         AND a.heartbeat_at+make_interval(secs=>a.claim_lease_ms/1000.0)<=clock_timestamp()
       ORDER BY t.id,a.attempt
       FOR UPDATE OF t,a SKIP LOCKED
    ), reaped AS (
      UPDATE bid_extraction_attempts a SET status='reaped',error_code='CLAIM_LEASE_EXPIRED'
       FROM expired e
       WHERE a.target_id=e.id AND a.attempt=e.attempt AND a.claim_token=e.claim_token
         AND a.status='running'
       RETURNING a.target_id
    )
    UPDATE bid_extraction_targets t SET state='pending'
      FROM reaped r WHERE t.id=r.target_id AND t.state='running'
    RETURNING t.id,t.project_id,t.document_id;
END
$$;

CREATE FUNCTION kb_bid_pending_extractions()
RETURNS TABLE(target_id uuid, project_id uuid, document_id uuid)
LANGUAGE sql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$ SELECT id, project_id, document_id FROM bid_extraction_targets WHERE state='pending' $$;

CREATE FUNCTION kb_bid_dirty_match_projects()
RETURNS uuid[]
LANGUAGE sql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
  SELECT COALESCE(array_agg(p.id ORDER BY p.id),'{}')
  FROM bid_projects p
  WHERE p.status='open'
    AND p.matching_mutation_watermark > COALESCE(
      (SELECT max(m.mutation_watermark) FROM bid_matching_manifests m WHERE m.project_id=p.id), -1)
$$;

CREATE FUNCTION kb_bid_get_part(p_project_id uuid, p_part_key text)
RETURNS jsonb
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
  SELECT jsonb_build_object(
    'part_key', p.part_key,
    'stale', p.stale,
    'stale_reason_codes', to_jsonb(p.stale_reason_codes),
    'content_artifact_id', p.content_artifact_id,
    'dependency_artifact_id', p.dependency_artifact_id,
    'content_revision', c.revision,
    'content_sha256', c.content_sha256,
    'markdown', convert_from(c.canonical_markdown_utf8, 'UTF8'),
    'dependency_sha256', d.content_sha256,
    'typed_input_identities', d.typed_input_identities
  )
  FROM bid_current_parts p
  JOIN bid_part_content_artifacts c ON c.id = p.content_artifact_id
  JOIN bid_part_dependency_artifacts d ON d.id = p.dependency_artifact_id
  WHERE p.project_id = p_project_id AND p.part_key = p_part_key
$$;

CREATE FUNCTION kb_bid_get_quote_snapshot(p_project_id uuid, p_snapshot_id uuid)
RETURNS jsonb
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
  SELECT jsonb_build_object(
    'id', snapshot.id,
    'project_id', snapshot.project_id,
    'revision_id', snapshot.revision_id,
    'schema_version', snapshot.schema_version,
    'content_sha256', snapshot.content_sha256,
    'tax_mode', snapshot.tax_mode,
    'title', snapshot.title,
    'notes', snapshot.notes,
    'net_total', kb_bid_format_amount(snapshot.net_total),
    'tax_total', kb_bid_format_amount(snapshot.tax_total),
    'gross_total', kb_bid_format_amount(snapshot.gross_total),
    'eligibility', snapshot.eligibility,
    'ceiling_revision', snapshot.ceiling_revision,
    'ceiling_identity_sha256', snapshot.ceiling_identity_sha256,
    'fact_revision', snapshot.fact_revision,
    'pricing_revision', snapshot.pricing_revision,
    'pricing_set_sha256', snapshot.pricing_set_sha256,
    'no_ceiling_review', snapshot.no_ceiling_review,
    'canonical_payload', convert_from(snapshot.canonical_payload,'UTF8')::jsonb
  )
  FROM bid_quote_snapshots snapshot
  WHERE snapshot.project_id=p_project_id AND snapshot.id=p_snapshot_id
$$;

CREATE FUNCTION kb_bid_download_submission_output(p_project_id uuid, p_output_id uuid)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE output bid_submission_output_artifacts%ROWTYPE;
BEGIN
  SELECT artifact.* INTO output
    FROM bid_current_submission_outputs current_value
    JOIN bid_submission_output_artifacts artifact ON artifact.id=current_value.output_artifact_id
   WHERE current_value.project_id=p_project_id AND artifact.id=p_output_id;
  IF output.id IS NULL THEN RAISE EXCEPTION 'SUBMISSION_OUTPUT_MISSING' USING ERRCODE='P0002'; END IF;
  IF NOT EXISTS (
    SELECT 1 FROM available_object_registry registry
     WHERE registry.object_ref=output.object_ref AND registry.digest=output.content_sha256
  ) THEN
    RAISE EXCEPTION 'SUBMISSION_OUTPUT_UNAVAILABLE' USING ERRCODE='P0002';
  END IF;
  RETURN jsonb_build_object(
    'output_id',output.id,'object_ref',output.object_ref,'digest',output.content_sha256,
    'format',output.format,'byte_length',output.byte_length);
END
$$;

CREATE FUNCTION kb_bid_build_part_markdown(p_project_id uuid, p_part_key text)
RETURNS text
LANGUAGE plpgsql
STABLE
SET search_path = pg_catalog, public
AS $$
DECLARE body text := ''; project_value bid_projects%ROWTYPE; company bid_company_profile_artifacts%ROWTYPE;
 submission bid_submission_profile_artifacts%ROWTYPE; quote bid_quote_snapshots%ROWTYPE;
 part_unit_id uuid; rec record;
BEGIN
  IF kb_bid_template_slot(p_part_key) IS NULL THEN RAISE EXCEPTION 'PART_KEY_INVALID' USING ERRCODE='22023'; END IF;
  SELECT * INTO STRICT project_value FROM bid_projects WHERE id=p_project_id;
  SELECT artifact.* INTO company FROM bid_current_profiles cur
    JOIN bid_company_profile_artifacts artifact ON artifact.id=cur.company_profile_id WHERE cur.project_id=p_project_id;
  SELECT artifact.* INTO submission FROM bid_current_profiles cur
    JOIN bid_submission_profile_artifacts artifact ON artifact.id=cur.submission_profile_id WHERE cur.project_id=p_project_id;
  SELECT snapshot.* INTO quote FROM bid_quote_current current_value
    JOIN bid_quote_snapshots snapshot ON snapshot.id=current_value.active_finalized_snapshot_id
   WHERE current_value.project_id=p_project_id AND snapshot.eligibility='eligible';

  IF p_part_key='1' THEN
    body := '# 项目概况'||E'\n\n'
      ||'- 项目：'||COALESCE(project_value.title,'')||E'\n'
      ||'- 预算：'||COALESCE(to_char(project_value.budget_amount,'FM99999999999999999990.00'),'未设置')||E'\n'
      ||'- 最高限价：'||COALESCE(to_char(project_value.ceiling_price,'FM99999999999999999990.00'),'未设置')
      ||'（口径 '||project_value.ceiling_basis||'）'||E'\n'
      ||'- 开标：'||COALESCE(kb_bid_utc_json_time(project_value.bid_open_at),'未设置')||E'\n'
      ||'- 截止：'||COALESCE(kb_bid_utc_json_time(project_value.expires_at),'未设置')||E'\n'
      ||'- 有效期至：'||COALESCE(kb_bid_utc_json_time(project_value.bid_valid_until),'未设置')||E'\n'
      ||'- 有效期天数：'||COALESCE(project_value.bid_valid_days::text,'未设置')||E'\n'
      ||'- 事实修订：'||project_value.fact_revision::text||E'\n'
      ||'- 事实摘要：'||project_value.fact_sha256||E'\n';
  ELSIF p_part_key LIKE '2:%' THEN
    IF p_part_key='2:unsectioned' THEN part_unit_id := '00000000-0000-0000-0000-000000000000'::uuid;
    ELSE part_unit_id := substr(p_part_key,3)::uuid; END IF;
    body := '# 技术响应 '||p_part_key||E'\n\n';
    FOR rec IN
      SELECT clause.text, pick.product_version_id, report.content_sha256
        FROM bid_matching_routes route
        JOIN bidding_current_matching_reports report ON report.route_id=route.id
        JOIN bid_matching_requirement_artifacts requirement ON requirement.route_id=route.id
        JOIN bid_clauses clause ON clause.id=requirement.clause_id
        LEFT JOIN bidding_current_route_pick_sets current_pick ON current_pick.route_id=route.id
        LEFT JOIN bid_route_pick_set_items pick
          ON pick.pick_set_id=current_pick.id AND pick.requirement_artifact_id=requirement.id
       WHERE route.project_id=p_project_id AND route.route_kind='technical' AND route.unit_id=part_unit_id
       ORDER BY requirement.ordinal, pick.ordinal
    LOOP
      body := body||'- '||rec.text||CASE WHEN rec.product_version_id IS NULL THEN '（待选择）'
        ELSE ' → '||rec.product_version_id::text END||E'\n';
    END LOOP;
  ELSIF p_part_key='3' THEN
    body := '# 总体产品方案'||E'\n\n';
    FOR rec IN
      SELECT item.product_version_id, item.unit_id, item.source_report_artifact_id
        FROM bid_current_project_pick_sets current_value
        JOIN bid_project_pick_set_items item ON item.project_pick_set_id=current_value.pick_set_id
       WHERE current_value.project_id=p_project_id
       ORDER BY item.ordinal
    LOOP
      body := body||'- 产品版本 '||rec.product_version_id::text||' / unit '||COALESCE(rec.unit_id::text,'')||E'\n';
    END LOOP;
  ELSIF p_part_key='4' THEN
    body := '# 公司资质与服务证据'||E'\n\n';
    FOR rec IN
      SELECT clause.text, decision.system_decision, decision.reason_code
        FROM bid_matching_routes route
        JOIN bidding_current_matching_reports report ON report.route_id=route.id
        JOIN bid_matching_requirement_decisions decision ON decision.report_id=report.id
        JOIN bid_matching_requirement_artifacts requirement ON requirement.id=decision.requirement_artifact_id
        JOIN bid_clauses clause ON clause.id=requirement.clause_id
       WHERE route.project_id=p_project_id AND route.route_kind='commercial'
       ORDER BY decision.ordinal
    LOOP
      body := body||'- '||rec.text||' → '||rec.system_decision||'/'||rec.reason_code||E'\n';
    END LOOP;
  ELSIF p_part_key='5' THEN
    body := '# 偏离、未解决与缺件'||E'\n\n';
    FOR rec IN
      SELECT clause.text, decision.final_support, decision.reason_code
        FROM bid_matching_routes route
        JOIN bidding_current_matching_reports report ON report.route_id=route.id
        JOIN bid_matching_requirement_decisions decision ON decision.report_id=report.id
        JOIN bid_matching_requirement_artifacts requirement ON requirement.id=decision.requirement_artifact_id
        JOIN bid_clauses clause ON clause.id=requirement.clause_id
       WHERE route.project_id=p_project_id AND decision.final_support<>'supported'
       ORDER BY route.route_kind, decision.ordinal
    LOOP
      body := body||'- '||rec.text||' → '||rec.final_support||'/'||rec.reason_code||E'\n';
    END LOOP;
  ELSIF p_part_key='6:letter' THEN
    body := '# 投标函'||E'\n\n'
      ||COALESCE(company.legal_name,'（公司名称未填）')||' 谨此投标。'||E'\n\n'
      ||'买方：'||COALESCE(submission.buyer_name,'（未填）')||E'\n'
      ||'项目编号：'||COALESCE(submission.project_code,'（未填）')||E'\n'
      ||'授权代表：'||COALESCE(submission.authorized_representative,'（未填）')||E'\n'
      ||'报价快照：'||COALESCE(quote.id::text,'（无合格定稿）')||E'\n';
  ELSIF p_part_key='6:authorization' THEN
    body := '# 授权材料'||E'\n\n';
    FOR rec IN
      SELECT classification.effective_requirement_kind, decision.resolution
        FROM bid_procedural_segment_artifacts segment
        JOIN bid_procedural_classification_artifacts classification
          ON classification.segment_id=segment.id AND classification.lifecycle_status='current'
        LEFT JOIN bid_procedural_decision_artifacts decision
          ON decision.classification_id=classification.id AND decision.lifecycle_status='current'
       WHERE segment.project_id=p_project_id
       ORDER BY segment.stable_key
    LOOP
      body := body||'- '||COALESCE(rec.effective_requirement_kind,'review')||' → '||COALESCE(rec.resolution,'未决议')||E'\n';
    END LOOP;
  ELSIF p_part_key='6:quote' THEN
    IF quote.id IS NULL THEN
      body := '# 报价表'||E'\n\n【固定占位】当前没有合格的 QuoteSnapshotV1。过程稿可继续编辑；正式 PDF 必须先定稿报价。'||E'\n';
    ELSE
      body := '# 报价表'||E'\n\n引用快照 '||quote.id::text||' / '||quote.content_sha256||E'\n'
        ||E'\n| 序号 | 说明 | 计价方式 | 数量 | 单位 | 单价/总价 | 税率 | 未税金额 | 税额 | 含税金额 |\n'
        ||E'| ---: | --- | --- | ---: | --- | ---: | ---: | ---: | ---: | ---: |\n';
      FOR rec IN
        SELECT value AS line FROM jsonb_array_elements(convert_from(quote.canonical_payload,'UTF8')::jsonb->'lines')
         ORDER BY (value->>'ordinal')::integer
      LOOP
        body := body||'| '||((rec.line->>'ordinal')::integer+1)::text
          ||' | '||replace(replace(COALESCE(rec.line->>'description',''),E'\n',' '),'|',E'\\|')
          ||' | '||(rec.line->>'pricing_mode')
          ||' | '||COALESCE(rec.line->>'quantity','')
          ||' | '||replace(COALESCE(rec.line->>'unit',''),'|',E'\\|')
          ||' | '||COALESCE(rec.line->>'unit_price',rec.line->>'entered_amount','')
          ||' | '||COALESCE(rec.line->>'tax_rate','')
          ||' | '||COALESCE(rec.line->>'net_amount','')
          ||' | '||COALESCE(rec.line->>'tax_amount','')
          ||' | '||COALESCE(rec.line->>'gross_amount','')||E' |\n';
      END LOOP;
      body := body||E'\n'
        ||'- 未税合计：'||kb_bid_format_amount(quote.net_total)||E'\n'
        ||'- 税额合计：'||kb_bid_format_amount(quote.tax_total)||E'\n'
        ||'- 含税合计：'||kb_bid_format_amount(quote.gross_total)||E'\n';
    END IF;
  ELSIF p_part_key='6:implementation_plan' THEN
    body := '# 实施与交付计划'||E'\n\n';
    FOR rec IN
      SELECT clause.text FROM bid_clauses clause
       WHERE clause.project_id=p_project_id AND clause.status='confirmed'
         AND clause.kind IN ('service','schedule_delivery')
       ORDER BY clause.kind, clause.id
    LOOP
      body := body||'- '||rec.text||E'\n';
    END LOOP;
    FOR rec IN
      SELECT item.product_version_id FROM bid_current_project_pick_sets current_value
        JOIN bid_project_pick_set_items item ON item.project_pick_set_id=current_value.pick_set_id
       WHERE current_value.project_id=p_project_id ORDER BY item.ordinal
    LOOP
      body := body||'- 交付产品 '||rec.product_version_id::text||E'\n';
    END LOOP;
  ELSIF p_part_key='6:procedural' THEN
    body := '# 程序材料检查'||E'\n\n';
    FOR rec IN
      SELECT classification.effective_requirement_kind, classification.router_result_status, decision.resolution
        FROM bid_procedural_segment_artifacts segment
        JOIN bid_procedural_classification_artifacts classification
          ON classification.segment_id=segment.id AND classification.lifecycle_status='current'
        LEFT JOIN bid_procedural_decision_artifacts decision
          ON decision.classification_id=classification.id AND decision.lifecycle_status='current'
       WHERE segment.project_id=p_project_id
       ORDER BY segment.stable_key
    LOOP
      body := body||'- '||COALESCE(rec.effective_requirement_kind, rec.router_result_status)
        ||' → '||COALESCE(rec.resolution,'未决议')||E'\n';
    END LOOP;
  END IF;
  IF body = '' THEN body := '# '||p_part_key||E'\n\n（无冻结内容）'||E'\n'; END IF;
  RETURN body;
END
$$;

CREATE FUNCTION kb_bid_rebuild_project_pick_set(p_project_id uuid, p_actor kb_actor_identity)
RETURNS jsonb
LANGUAGE plpgsql
SET search_path = pg_catalog, public
AS $$
DECLARE revision bigint; id uuid := gen_random_uuid(); payload bytea; digest kb_sha256;
 items jsonb := '[]'::jsonb; rec record; ordinal integer := 0;
BEGIN
  PERFORM 1 FROM bid_projects project_value WHERE project_value.id=p_project_id FOR UPDATE;
  IF NOT FOUND THEN RAISE EXCEPTION 'PROJECT_NOT_FOUND' USING ERRCODE='23503'; END IF;
  SELECT COALESCE(max(artifact.revision),0)+1 INTO revision
    FROM bid_project_pick_set_artifacts artifact WHERE artifact.project_id=p_project_id;
  FOR rec IN
    SELECT current_value.pick_set_id AS route_pick_set_id, artifact.source_report_artifact_id,
           item.requirement_artifact_id, item.candidate_artifact_id, item.product_id,
           item.product_version_id, item.unit_id, artifact.route_id
      FROM bid_current_route_pick_sets current_value
      JOIN bid_route_pick_set_artifacts artifact ON artifact.id=current_value.pick_set_id
      JOIN bid_route_pick_set_items item ON item.pick_set_id=artifact.id
     WHERE current_value.project_id=p_project_id
     ORDER BY artifact.route_id, item.requirement_artifact_id, item.candidate_artifact_id
  LOOP
    items := items || jsonb_build_array(jsonb_build_object(
      'route_pick_set_id',rec.route_pick_set_id,'source_report_artifact_id',rec.source_report_artifact_id,
      'requirement_artifact_id',rec.requirement_artifact_id,'candidate_artifact_id',rec.candidate_artifact_id,
      'product_id',rec.product_id,'product_version_id',rec.product_version_id,'unit_id',rec.unit_id));
  END LOOP;
  payload := convert_to('{"schema_version":1,"project_id":"'||p_project_id::text
    ||'","revision":'||revision::text||',"items":'||items::text||'}','UTF8');
  digest := encode(public.digest(payload,'sha256'),'hex');
  INSERT INTO bid_project_pick_set_artifacts(id,project_id,revision,canonical_payload,content_sha256,created_by,created_at)
  VALUES(id,p_project_id,revision,payload,digest,p_actor,clock_timestamp());
  FOR rec IN
    SELECT current_value.pick_set_id AS route_pick_set_id, artifact.source_report_artifact_id,
           item.requirement_artifact_id, item.candidate_artifact_id, item.product_id,
           item.product_version_id, item.unit_id
      FROM bid_current_route_pick_sets current_value
      JOIN bid_route_pick_set_artifacts artifact ON artifact.id=current_value.pick_set_id
      JOIN bid_route_pick_set_items item ON item.pick_set_id=artifact.id
     WHERE current_value.project_id=p_project_id
     ORDER BY artifact.route_id, item.requirement_artifact_id, item.candidate_artifact_id
  LOOP
    INSERT INTO bid_project_pick_set_items(project_pick_set_id,ordinal,route_pick_set_id,
      source_report_artifact_id,requirement_artifact_id,candidate_artifact_id,product_id,product_version_id,unit_id)
    VALUES(id,ordinal,rec.route_pick_set_id,rec.source_report_artifact_id,rec.requirement_artifact_id,
      rec.candidate_artifact_id,rec.product_id,rec.product_version_id,rec.unit_id);
    ordinal := ordinal + 1;
  END LOOP;
  INSERT INTO bid_current_project_pick_sets(project_id,pick_set_id,revision) VALUES(p_project_id,id,revision)
  ON CONFLICT (project_id) DO UPDATE SET pick_set_id=EXCLUDED.pick_set_id, revision=EXCLUDED.revision;
  RETURN jsonb_build_object('project_pick_set_id',id,'revision',revision,'content_sha256',digest);
END
$$;

CREATE FUNCTION kb_bid_matching_schedule(
  p_project_id uuid, p_expected_watermark bigint, p_max_attempts integer, p_payload jsonb,
  p_actor kb_actor_identity, p_idempotency_key text, p_request_bytes bytea,
  p_request_sha256 kb_sha256
)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE project_value bid_projects%ROWTYPE; generation bigint; manifest_id uuid;
 item jsonb; membership jsonb; job_ids uuid[] := '{}'; job_id uuid; replay bytea;
 existing_manifest bid_matching_manifests%ROWTYPE; response jsonb;
BEGIN
  IF p_actor <> 'system:matching-publication' THEN
    PERFORM kb_bid_require_human_actor(p_actor);
  END IF;
  replay := kb_bid_idempotency_begin(
    p_actor,'bid.matching.schedule',p_idempotency_key,p_request_bytes,p_request_sha256
  );
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  IF p_max_attempts < 1 OR p_max_attempts > 32 THEN RAISE EXCEPTION 'INVALID_MATCHING_POLICY' USING ERRCODE='22023'; END IF;
  SELECT * INTO STRICT project_value FROM bid_projects WHERE id=p_project_id FOR UPDATE;
  IF project_value.status<>'open' THEN RAISE EXCEPTION 'PROJECT_ENDED' USING ERRCODE='55000'; END IF;
  IF project_value.matching_mutation_watermark<>p_expected_watermark THEN
    RAISE EXCEPTION 'MATCHING_SCHEDULE_FENCE_LOST' USING ERRCODE='40001';
  END IF;
  SELECT * INTO existing_manifest
    FROM bid_matching_manifests
   WHERE project_id=p_project_id AND mutation_watermark=p_expected_watermark
   ORDER BY generation DESC
   LIMIT 1;
  IF FOUND THEN
    SELECT COALESCE(array_agg(job.id ORDER BY route.ordinal),'{}'::uuid[])
      INTO job_ids
      FROM bid_matching_jobs job
      JOIN bid_matching_routes route ON route.id=job.route_id
     WHERE job.manifest_id=existing_manifest.id;
    response := jsonb_build_object(
      'manifest_id',existing_manifest.id,'generation',existing_manifest.generation,
      'job_ids',to_jsonb(job_ids),'scheduled',false
    );
    INSERT INTO audit_events(
      id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
      entity_kind,entity_locator,after_revision,after_sha256
    ) VALUES(
      gen_random_uuid(),1,'bid.matching.schedule',p_actor,p_idempotency_key,p_request_sha256,
      encode(public.digest(convert_to(response::text,'UTF8'),'sha256'),'hex'),'bid_matching_manifest',
      jsonb_build_object('project_id',p_project_id,'manifest_id',existing_manifest.id),
      existing_manifest.generation,existing_manifest.content_sha256
    );
    PERFORM kb_bid_idempotency_complete(
      p_actor,'bid.matching.schedule',p_idempotency_key,202,convert_to(response::text,'UTF8')
    );
    RETURN response;
  END IF;
  SELECT COALESCE(max(m.generation),0)+1 INTO generation FROM bid_matching_manifests m WHERE m.project_id=p_project_id;
  manifest_id := (p_payload->>'manifest_id')::uuid;
  INSERT INTO bid_matching_manifests(id,project_id,generation,mutation_watermark,requirement_set_sha256,eligible_scope_sha256,
    canonical_payload,content_sha256)
  VALUES(manifest_id,p_project_id,generation,p_expected_watermark,p_payload->>'requirement_set_sha256',
    p_payload->>'eligible_scope_sha256', convert_to(p_payload::text,'UTF8'),
    encode(public.digest(convert_to(p_payload::text,'UTF8'),'sha256'),'hex'));
  FOR item IN SELECT value FROM jsonb_array_elements(p_payload->'routes') LOOP
    INSERT INTO bid_matching_routes(id,manifest_id,project_id,route_kind,unit_id,ordinal,empty_policy,route_scope_sha256)
    VALUES((item->>'id')::uuid,manifest_id,p_project_id,item->>'route_kind',NULLIF(item->>'unit_id','')::uuid,
      (item->>'ordinal')::integer,item->>'empty_policy',item->>'route_scope_sha256');
  END LOOP;
  FOR item IN SELECT value FROM jsonb_array_elements(p_payload->'requirements') LOOP
    INSERT INTO bid_matching_requirement_artifacts(id,manifest_id,route_id,clause_id,ordinal,requirement_text,requirement_sha256)
    VALUES((item->>'id')::uuid,manifest_id,(item->>'route_id')::uuid,(item->>'clause_id')::uuid,
      (item->>'ordinal')::integer,item->>'text',item->>'sha256');
  END LOOP;
  FOR item IN SELECT value FROM jsonb_array_elements(COALESCE(p_payload->'products','[]'::jsonb)) LOOP
    INSERT INTO bid_matching_product_version_artifacts(id,manifest_id,product_id,product_version_id,workspace_kind,frozen_display_name,identity_sha256)
    VALUES((item->>'id')::uuid,manifest_id,(item->>'product_id')::uuid,(item->>'product_version_id')::uuid,
      item->>'workspace_kind',item->>'frozen_display_name',item->>'identity_sha256');
  END LOOP;
  FOR item IN SELECT value FROM jsonb_array_elements(COALESCE(p_payload->'memberships','[]'::jsonb)) LOOP
    INSERT INTO bid_matching_route_memberships(route_id,product_version_artifact_id,route_product_ordinal)
    VALUES((item->>'route_id')::uuid,(item->>'product_version_artifact_id')::uuid,(item->>'route_product_ordinal')::integer);
  END LOOP;
  FOR item IN SELECT value FROM jsonb_array_elements(COALESCE(p_payload->'frozen_hits','[]'::jsonb)) LOOP
    INSERT INTO bid_matching_frozen_retrieved_hits
      (id,manifest_id,route_id,requirement_artifact_id,product_version_artifact_id,document_id,source_chunk_id,
       frozen_document_display_name,chunk_utf8,chunk_sha256,chunk_byte_length,retrieval_rank,retrieval_raw_score,
       quote_start_offset,quote_end_offset,offset_unit,retrieval_contract_version)
    VALUES((item->>'id')::uuid,manifest_id,(item->>'route_id')::uuid,(item->>'requirement_artifact_id')::uuid,
      (item->>'product_version_artifact_id')::uuid,(item->>'document_id')::uuid,(item->>'source_chunk_id')::uuid,
      item->>'frozen_document_display_name',convert_to(item->>'chunk_utf8','UTF8'),item->>'chunk_sha256',
      (item->>'chunk_byte_length')::bigint,(item->>'retrieval_rank')::integer,(item->>'retrieval_raw_score')::numeric,
      (item->>'quote_start_offset')::bigint,(item->>'quote_end_offset')::bigint,item->>'offset_unit',
      item->>'retrieval_contract_version');
  END LOOP;
  FOR item IN SELECT value FROM jsonb_array_elements(p_payload->'routes') LOOP
    job_id := gen_random_uuid();
    INSERT INTO bid_matching_jobs(id,project_id,manifest_id,route_id,status,max_attempts,claim_lease_ms,lease_policy_generation,created_at)
    VALUES(job_id,p_project_id,manifest_id,(item->>'id')::uuid,'pending',p_max_attempts,300000,1,clock_timestamp());
    job_ids := job_ids || job_id;
  END LOOP;
  response := jsonb_build_object(
    'manifest_id',manifest_id,'generation',generation,'job_ids',to_jsonb(job_ids),'scheduled',true
  );
  INSERT INTO audit_events(
    id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
    entity_kind,entity_locator,after_revision,after_sha256
  ) VALUES(
    gen_random_uuid(),1,'bid.matching.schedule',p_actor,p_idempotency_key,p_request_sha256,
    encode(public.digest(convert_to(response::text,'UTF8'),'sha256'),'hex'),'bid_matching_manifest',
    jsonb_build_object('project_id',p_project_id,'manifest_id',manifest_id),generation,
    encode(public.digest(convert_to(p_payload::text,'UTF8'),'sha256'),'hex')
  );
  PERFORM kb_bid_idempotency_complete(
    p_actor,'bid.matching.schedule',p_idempotency_key,202,convert_to(response::text,'UTF8')
  );
  RETURN response;
END
$$;

CREATE FUNCTION kb_bid_matching_claim(p_job_id uuid, p_claim_token uuid)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE job bid_matching_jobs%ROWTYPE; manifest bid_matching_manifests%ROWTYPE; route bid_matching_routes%ROWTYPE;
 attempt integer; lease_ms integer; lease_gen bigint;
BEGIN
  SELECT * INTO job FROM bid_matching_jobs WHERE id=p_job_id FOR UPDATE;
  IF NOT FOUND OR job.status<>'pending' THEN RETURN NULL; END IF;
  SELECT * INTO STRICT manifest FROM bid_matching_manifests WHERE id=job.manifest_id;
  PERFORM 1 FROM bid_projects WHERE id=job.project_id AND status='open'
    AND matching_mutation_watermark=manifest.mutation_watermark FOR UPDATE;
  IF NOT FOUND THEN RETURN NULL; END IF;
  IF manifest.generation<>(SELECT max(generation) FROM bid_matching_manifests WHERE project_id=job.project_id) THEN
    RETURN NULL;
  END IF;
  SELECT * INTO STRICT route FROM bid_matching_routes WHERE id=job.route_id;
  SELECT COALESCE(max(c.attempt),0)+1 INTO attempt FROM bid_matching_job_claims c WHERE c.job_id=p_job_id;
  IF attempt > job.max_attempts THEN
    UPDATE bid_matching_jobs SET status='failed',error_code='ATTEMPTS_EXHAUSTED',finished_at=clock_timestamp() WHERE id=p_job_id;
    RETURN NULL;
  END IF;
  lease_ms := job.claim_lease_ms; lease_gen := job.lease_policy_generation;
  INSERT INTO bid_matching_job_claims(job_id,attempt,claim_token,claim_lease_ms,lease_policy_generation,claimed_at,heartbeat_at,status)
  VALUES(p_job_id,attempt,p_claim_token,lease_ms,lease_gen,clock_timestamp(),clock_timestamp(),'running');
  UPDATE bid_matching_jobs SET status='running',active_attempt=attempt,started_at=COALESCE(started_at,clock_timestamp()) WHERE id=p_job_id;
  RETURN jsonb_build_object(
    'job_id',p_job_id,'manifest_id',job.manifest_id,'project_id',job.project_id,'generation',manifest.generation,
    'mutation_watermark',manifest.mutation_watermark,'route_id',job.route_id,'route_kind',route.route_kind,
    'unit_id',route.unit_id,'empty_policy',route.empty_policy,'attempt',attempt,'claim_token',p_claim_token,
    'claim_lease_ms',lease_ms,'lease_policy_generation',lease_gen);
END
$$;

CREATE FUNCTION kb_bid_matching_heartbeat(
  p_job_id uuid, p_claim_token uuid, p_attempt integer, p_lease_ms integer, p_lease_generation bigint, p_staging_ttl_ms integer
)
RETURNS boolean
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
BEGIN
  UPDATE bid_matching_job_claims claim SET heartbeat_at=clock_timestamp()
    FROM bid_matching_jobs job
   WHERE claim.job_id=p_job_id AND claim.attempt=p_attempt AND claim.claim_token=p_claim_token
     AND claim.claim_lease_ms=p_lease_ms AND claim.lease_policy_generation=p_lease_generation
     AND claim.status='running' AND job.id=claim.job_id AND job.status='running'
     AND job.active_attempt=claim.attempt
     AND claim.heartbeat_at + make_interval(secs => claim.claim_lease_ms::double precision/1000.0) > clock_timestamp();
  IF NOT FOUND THEN RETURN false; END IF;
  UPDATE bid_matching_staging_sets SET expires_at=clock_timestamp()+make_interval(secs=>p_staging_ttl_ms::double precision/1000.0)
   WHERE job_id=p_job_id AND attempt=p_attempt AND claim_token=p_claim_token AND state='active';
  RETURN true;
END
$$;

CREATE FUNCTION kb_bid_matching_open_staging(p_payload jsonb)
RETURNS uuid
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE existing bid_matching_staging_sets%ROWTYPE; id uuid; active bigint;
BEGIN
  PERFORM 1 FROM bid_matching_jobs job
    JOIN bid_matching_job_claims claim ON claim.job_id=job.id
   WHERE job.id=(p_payload->>'job_id')::uuid AND claim.claim_token=(p_payload->>'claim_token')::uuid
     AND claim.attempt=(p_payload->>'attempt')::integer AND claim.status='running' AND job.status='running'
     AND job.active_attempt=claim.attempt
     AND claim.heartbeat_at + make_interval(secs => claim.claim_lease_ms::double precision/1000.0) > clock_timestamp()
   FOR UPDATE OF job, claim;
  IF NOT FOUND THEN RAISE EXCEPTION 'MATCHING_CLAIM_LOST' USING ERRCODE='40001'; END IF;
  SELECT * INTO existing FROM bid_matching_staging_sets
   WHERE job_id=(p_payload->>'job_id')::uuid AND claim_token=(p_payload->>'claim_token')::uuid
     AND attempt=(p_payload->>'attempt')::integer AND route_id=(p_payload->>'route_id')::uuid FOR UPDATE;
  IF existing.id IS NOT NULL THEN
    IF existing.open_payload_sha256<>p_payload->>'open_payload_sha256'
       OR existing.report_nonce<>(p_payload->>'report_nonce')::uuid THEN
      RAISE EXCEPTION 'OPEN_STAGING_PAYLOAD_MISMATCH' USING ERRCODE='23505';
    END IF;
    IF existing.state IN ('expired','failed') THEN RAISE EXCEPTION 'STAGING_TERMINAL' USING ERRCODE='55000'; END IF;
    RETURN existing.id;
  END IF;
  SELECT count(*) INTO active FROM bid_matching_staging_sets
   WHERE project_id=(p_payload->>'project_id')::uuid AND state='active';
  IF active >= 8 THEN RAISE EXCEPTION 'STAGING_ACTIVE_SET_QUOTA_EXCEEDED' USING ERRCODE='54000'; END IF;
  id := COALESCE((p_payload->>'id')::uuid, gen_random_uuid());
  INSERT INTO bid_matching_staging_sets
    (id,job_id,route_id,claim_token,attempt,manifest_id,project_id,generation,mutation_watermark,
     report_nonce,state,expires_at,open_payload_sha256,expected_batch_count,expected_item_count,expected_byte_length,
     staged_item_count,staged_byte_length)
  VALUES(id,(p_payload->>'job_id')::uuid,(p_payload->>'route_id')::uuid,(p_payload->>'claim_token')::uuid,
    (p_payload->>'attempt')::integer,(p_payload->>'manifest_id')::uuid,(p_payload->>'project_id')::uuid,
    (p_payload->>'generation')::bigint,(p_payload->>'mutation_watermark')::bigint,(p_payload->>'report_nonce')::uuid,
    'active', clock_timestamp()+make_interval(secs=>(p_payload->>'ttl_ms')::double precision/1000.0),
    p_payload->>'open_payload_sha256',(p_payload->>'expected_batch_count')::integer,
    (p_payload->>'expected_item_count')::bigint,(p_payload->>'expected_byte_length')::bigint,0,0);
  RETURN id;
END
$$;

CREATE FUNCTION kb_bid_matching_stage_batch(p_payload jsonb)
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE set_row bid_matching_staging_sets%ROWTYPE; existing_hash text; items jsonb; item jsonb; idx int := 0;
BEGIN
  SELECT * INTO STRICT set_row FROM bid_matching_staging_sets WHERE id=(p_payload->>'staging_set_id')::uuid FOR UPDATE;
  PERFORM 1 FROM bid_matching_jobs job JOIN bid_matching_job_claims claim ON claim.job_id=job.id
   WHERE job.id=set_row.job_id AND claim.claim_token=set_row.claim_token AND claim.attempt=set_row.attempt
     AND claim.status='running' AND job.status='running'
     AND claim.heartbeat_at + make_interval(secs => claim.claim_lease_ms::double precision/1000.0) > clock_timestamp()
   FOR UPDATE OF job, claim;
  IF NOT FOUND OR set_row.state<>'active' OR set_row.expires_at<=clock_timestamp() THEN
    RAISE EXCEPTION 'STAGING_NOT_ACTIVE' USING ERRCODE='40001';
  END IF;
  SELECT payload_sha256 INTO existing_hash FROM bid_matching_staged_batches
   WHERE staging_set_id=set_row.id AND batch_ordinal=(p_payload->>'batch_ordinal')::integer;
  IF existing_hash IS NOT NULL THEN
    IF existing_hash<>p_payload->>'payload_sha256' THEN RAISE EXCEPTION 'STAGING_BATCH_PAYLOAD_MISMATCH' USING ERRCODE='23505'; END IF;
    RETURN;
  END IF;
  INSERT INTO bid_matching_staged_batches(staging_set_id,batch_ordinal,collection_kind,canonical_items,payload_sha256,item_count,byte_length)
  VALUES(set_row.id,(p_payload->>'batch_ordinal')::integer,p_payload->>'collection_kind',
    decode(p_payload->>'canonical_items_b64','base64'),p_payload->>'payload_sha256',
    (p_payload->>'item_count')::integer,(p_payload->>'byte_length')::bigint);
  items := COALESCE(p_payload->'items','[]'::jsonb);
  IF p_payload->>'collection_kind'='source_artifacts' THEN
    FOR item IN SELECT value FROM jsonb_array_elements(items) LOOP
      INSERT INTO bid_matching_staged_source_artifacts
        (staging_set_id,id,batch_ordinal,item_ordinal,product_version_artifact_id,document_id,source_chunk_id,
         frozen_document_display_name,chunk_utf8,chunk_sha256,chunk_byte_length,retrieval_rank,retrieval_raw_score,retrieval_contract_version)
      VALUES(set_row.id,(item->>'id')::uuid,(p_payload->>'batch_ordinal')::integer,idx,
        (item->>'product_version_artifact_id')::uuid,(item->>'document_id')::uuid,(item->>'source_chunk_id')::uuid,
        item->>'frozen_document_display_name',convert_to(item->>'chunk_utf8','UTF8'),item->>'chunk_sha256',
        (item->>'chunk_byte_length')::bigint,(item->>'retrieval_rank')::integer,(item->>'retrieval_raw_score')::numeric,
        item->>'retrieval_contract_version');
      idx := idx + 1;
    END LOOP;
  ELSIF p_payload->>'collection_kind'='candidates' THEN
    FOR item IN SELECT value FROM jsonb_array_elements(items) LOOP
      INSERT INTO bid_matching_staged_candidates
        (staging_set_id,id,batch_ordinal,item_ordinal,requirement_artifact_id,product_version_artifact_id,
         route_product_ordinal,retrieval_rank,retrieval_raw_score,candidate_identity_sha256,evidence_v1_sha256,
         support,business_value_status,business_value,recommended)
      VALUES(set_row.id,(item->>'id')::uuid,(p_payload->>'batch_ordinal')::integer,idx,
        (item->>'requirement_artifact_id')::uuid,(item->>'product_version_artifact_id')::uuid,
        (item->>'route_product_ordinal')::integer,(item->>'retrieval_rank')::integer,(item->>'retrieval_raw_score')::numeric,
        item->>'candidate_identity_sha256',item->>'evidence_v1_sha256',item->>'support',item->>'business_value_status',
        NULLIF(item->>'business_value','')::numeric,(item->>'recommended')::boolean);
      idx := idx + 1;
    END LOOP;
  ELSIF p_payload->>'collection_kind'='evidences' THEN
    FOR item IN SELECT value FROM jsonb_array_elements(items) LOOP
      INSERT INTO bid_matching_staged_evidences
        (staging_set_id,id,batch_ordinal,item_ordinal,candidate_artifact_id,source_chunk_artifact_id,document_id,
         document_display_name,source_chunk_id,source_chunk_sha256,quote,start_offset,end_offset,offset_unit,ordinal)
      VALUES(set_row.id,(item->>'id')::uuid,(p_payload->>'batch_ordinal')::integer,idx,
        (item->>'candidate_artifact_id')::uuid,(item->>'source_chunk_artifact_id')::uuid,(item->>'document_id')::uuid,
        item->>'document_display_name',(item->>'source_chunk_id')::uuid,item->>'source_chunk_sha256',item->>'quote',
        (item->>'start_offset')::bigint,(item->>'end_offset')::bigint,item->>'offset_unit',(item->>'ordinal')::integer);
      idx := idx + 1;
    END LOOP;
  ELSIF p_payload->>'collection_kind'='requirement_decisions' THEN
    FOR item IN SELECT value FROM jsonb_array_elements(items) LOOP
      INSERT INTO bid_matching_staged_requirement_decisions
        (staging_set_id,id,batch_ordinal,item_ordinal,requirement_artifact_id,final_support,system_decision,
         quality_status,reason_code,selected_candidate_artifact_id,ordinal)
      VALUES(set_row.id,(item->>'id')::uuid,(p_payload->>'batch_ordinal')::integer,idx,
        (item->>'requirement_artifact_id')::uuid,item->>'final_support',item->>'system_decision',
        item->>'quality_status',item->>'reason_code',NULLIF(item->>'selected_candidate_artifact_id','')::uuid,
        (item->>'ordinal')::integer);
      idx := idx + 1;
    END LOOP;
  ELSIF p_payload->>'collection_kind'='candidate_groups' THEN
    FOR item IN SELECT value FROM jsonb_array_elements(items) LOOP
      INSERT INTO bid_matching_staged_candidate_groups
        (staging_set_id,id,batch_ordinal,item_ordinal,requirement_artifact_id,support,ordinal,canonical_payload,content_sha256)
      VALUES(set_row.id,(item->>'id')::uuid,(p_payload->>'batch_ordinal')::integer,idx,
        (item->>'requirement_artifact_id')::uuid,item->>'support',(item->>'ordinal')::integer,
        decode(item->>'canonical_payload_b64','base64'),item->>'content_sha256');
      idx := idx + 1;
    END LOOP;
  ELSIF p_payload->>'collection_kind'='reason_codes' THEN
    FOR item IN SELECT value FROM jsonb_array_elements(items) LOOP
      INSERT INTO bid_matching_staged_reason_codes(staging_set_id,batch_ordinal,item_ordinal,reason_code)
      VALUES(set_row.id,(p_payload->>'batch_ordinal')::integer,idx,item#>>'{}');
      idx := idx + 1;
    END LOOP;
  END IF;
  UPDATE bid_matching_staging_sets SET staged_item_count=staged_item_count+(p_payload->>'item_count')::bigint,
    staged_byte_length=staged_byte_length+(p_payload->>'byte_length')::bigint WHERE id=set_row.id;
END
$$;

CREATE FUNCTION kb_bid_matching_stage_report_payload(
  p_staging_set_id uuid, p_canonical_payload bytea, p_content_sha256 kb_sha256
)
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
BEGIN
  INSERT INTO bid_matching_staging_report_payloads(staging_set_id,canonical_payload,content_sha256)
  VALUES(p_staging_set_id,p_canonical_payload,p_content_sha256)
  ON CONFLICT (staging_set_id) DO UPDATE SET canonical_payload=EXCLUDED.canonical_payload, content_sha256=EXCLUDED.content_sha256
   WHERE bid_matching_staging_report_payloads.content_sha256=EXCLUDED.content_sha256;
  IF NOT FOUND AND NOT EXISTS (SELECT 1 FROM bid_matching_staging_report_payloads WHERE staging_set_id=p_staging_set_id AND content_sha256=p_content_sha256) THEN
    RAISE EXCEPTION 'STAGED_REPORT_PAYLOAD_MISMATCH' USING ERRCODE='23505';
  END IF;
END
$$;

CREATE FUNCTION kb_bid_matching_commit(
  p_job_id uuid, p_claim_token uuid, p_attempt integer, p_staging_set_id uuid,
  p_report_id uuid, p_report_nonce uuid, p_expected_report_sha256 kb_sha256
)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE job bid_matching_jobs%ROWTYPE; claim_value bid_matching_job_claims%ROWTYPE;
 set_row bid_matching_staging_sets%ROWTYPE; manifest_value bid_matching_manifests%ROWTYPE;
 project_value bid_projects%ROWTYPE; route_value bid_matching_routes%ROWTYPE;
 payload bytea; parsed jsonb; coverage jsonb; rec record; now_ts timestamptz;
BEGIN
  SELECT * INTO STRICT job FROM bid_matching_jobs WHERE id=p_job_id FOR UPDATE;
  IF job.status='completed' AND job.completed_report_id=p_report_id THEN
    PERFORM 1 FROM bid_matching_reports report
      JOIN bid_matching_staging_sets staging ON staging.consumed_report_id=report.id
     WHERE report.id=p_report_id AND report.job_id=p_job_id
       AND report.content_sha256=p_expected_report_sha256
       AND staging.id=p_staging_set_id AND staging.job_id=p_job_id
       AND staging.claim_token=p_claim_token AND staging.attempt=p_attempt
       AND staging.report_nonce=p_report_nonce AND staging.state='consumed';
    IF NOT FOUND THEN
      RAISE EXCEPTION 'COMPLETED_REPORT_PAYLOAD_MISMATCH' USING ERRCODE='23505';
    END IF;
    RETURN jsonb_build_object('status','replayed','report_id',p_report_id);
  END IF;
  IF job.status<>'running' OR job.active_attempt<>p_attempt THEN
    RAISE EXCEPTION 'MATCHING_CLAIM_LOST' USING ERRCODE='40001';
  END IF;
  SELECT * INTO STRICT project_value FROM bid_projects WHERE id=job.project_id FOR UPDATE;
  SELECT * INTO STRICT manifest_value FROM bid_matching_manifests WHERE id=job.manifest_id;
  IF project_value.status<>'open'
     OR project_value.matching_mutation_watermark<>manifest_value.mutation_watermark
     OR manifest_value.generation<>(SELECT max(generation) FROM bid_matching_manifests WHERE project_id=job.project_id) THEN
    RAISE EXCEPTION 'MATCHING_INPUTS_STALE' USING ERRCODE='40001';
  END IF;
  SELECT * INTO claim_value FROM bid_matching_job_claims
   WHERE job_id=p_job_id AND attempt=p_attempt AND claim_token=p_claim_token FOR UPDATE;
  SELECT * INTO STRICT route_value FROM bid_matching_routes
   WHERE id=job.route_id AND project_id=job.project_id AND manifest_id=job.manifest_id;
  SELECT * INTO STRICT set_row FROM bid_matching_staging_sets WHERE id=p_staging_set_id FOR UPDATE;
  now_ts := clock_timestamp();
  IF claim_value.job_id IS NULL OR claim_value.status<>'running'
     OR claim_value.heartbeat_at+make_interval(secs=>claim_value.claim_lease_ms::double precision/1000.0)<=now_ts THEN
    RAISE EXCEPTION 'MATCHING_CLAIM_LOST' USING ERRCODE='40001';
  END IF;
  IF set_row.state<>'active' OR set_row.expires_at<=now_ts
     OR set_row.job_id<>p_job_id OR set_row.route_id<>job.route_id
     OR set_row.claim_token<>p_claim_token OR set_row.attempt<>p_attempt
     OR set_row.manifest_id<>job.manifest_id OR set_row.project_id<>job.project_id
     OR set_row.generation<>manifest_value.generation
     OR set_row.mutation_watermark<>manifest_value.mutation_watermark
     OR set_row.report_nonce<>p_report_nonce THEN
    RAISE EXCEPTION 'STAGING_NOT_ACTIVE' USING ERRCODE='40001';
  END IF;
  IF set_row.expected_batch_count<>(SELECT count(*) FROM bid_matching_staged_batches WHERE staging_set_id=p_staging_set_id)
     OR set_row.staged_item_count<>set_row.expected_item_count
     OR set_row.staged_byte_length<>set_row.expected_byte_length
     OR 0<>(SELECT min(batch_ordinal) FROM bid_matching_staged_batches WHERE staging_set_id=p_staging_set_id)
     OR set_row.expected_batch_count-1<>(SELECT max(batch_ordinal) FROM bid_matching_staged_batches WHERE staging_set_id=p_staging_set_id) THEN
    RAISE EXCEPTION 'STAGING_COUNT_MISMATCH' USING ERRCODE='22023';
  END IF;
  SELECT canonical_payload INTO STRICT payload FROM bid_matching_staging_report_payloads WHERE staging_set_id=p_staging_set_id;
  IF encode(public.digest(payload,'sha256'),'hex')<>p_expected_report_sha256 THEN
    RAISE EXCEPTION 'REPORT_HASH_MISMATCH' USING ERRCODE='22023';
  END IF;
  parsed := convert_from(payload,'UTF8')::jsonb;
  coverage := parsed->'coverage';
  INSERT INTO bid_matching_reports(id,project_id,manifest_id,job_id,route_id,generation,mutation_watermark,empty_disposition,
    coverage_total,coverage_supported,coverage_contradicted,coverage_insufficient,coverage_unresolved,
    quality_status,degraded,reason_codes,canonical_payload,content_sha256,ai_run_id,ai_span_id,published_at)
  VALUES(p_report_id,job.project_id,job.manifest_id,job.id,job.route_id,set_row.generation,set_row.mutation_watermark,
    NULLIF(parsed->>'empty_disposition',''),(coverage->>'total')::integer,(coverage->>'supported')::integer,
    (coverage->>'contradicted')::integer,(coverage->>'insufficient')::integer,(coverage->>'unresolved')::integer,
    parsed->>'quality_status',(parsed->>'degraded')::boolean,
    COALESCE(ARRAY(SELECT jsonb_array_elements_text(parsed->'reason_codes')),'{}'),
    payload,p_expected_report_sha256,NULLIF(parsed->>'ai_run_id','')::uuid,NULLIF(parsed->>'ai_span_id','')::uuid,
    clock_timestamp());
  INSERT INTO bid_matching_source_artifacts(id,report_id,product_version_artifact_id,document_id,source_chunk_id,
    frozen_document_display_name,chunk_utf8,chunk_sha256,chunk_byte_length,retrieval_rank,retrieval_raw_score,retrieval_contract_version)
  SELECT id,p_report_id,product_version_artifact_id,document_id,source_chunk_id,frozen_document_display_name,chunk_utf8,
    chunk_sha256,chunk_byte_length,retrieval_rank,retrieval_raw_score,retrieval_contract_version
    FROM bid_matching_staged_source_artifacts WHERE staging_set_id=p_staging_set_id;
  INSERT INTO bid_matching_candidate_artifacts(id,report_id,requirement_artifact_id,product_version_artifact_id,support,
    candidate_identity_sha256,evidence_v1_sha256,business_value_status,business_value,route_product_ordinal,retrieval_rank,
    retrieval_raw_score,recommended)
  SELECT id,p_report_id,requirement_artifact_id,product_version_artifact_id,support,candidate_identity_sha256,evidence_v1_sha256,
    business_value_status,business_value,route_product_ordinal,retrieval_rank,retrieval_raw_score,recommended
    FROM bid_matching_staged_candidates WHERE staging_set_id=p_staging_set_id;
  INSERT INTO bid_matching_evidence_artifacts(id,report_id,candidate_artifact_id,source_chunk_artifact_id,document_id,
    document_display_name,source_chunk_id,source_chunk_sha256,start_offset,end_offset,offset_unit,quote_utf8,quote_sha256,ordinal)
  SELECT id,p_report_id,candidate_artifact_id,source_chunk_artifact_id,document_id,document_display_name,source_chunk_id,
    source_chunk_sha256,start_offset,end_offset,offset_unit,convert_to(quote,'UTF8'),encode(public.digest(convert_to(quote,'UTF8'),'sha256'),'hex'),ordinal
    FROM bid_matching_staged_evidences WHERE staging_set_id=p_staging_set_id;
  INSERT INTO bid_matching_requirement_decisions(id,report_id,requirement_artifact_id,final_support,system_decision,quality_status,
    reason_code,selected_candidate_artifact_id,ordinal)
  SELECT id,p_report_id,requirement_artifact_id,final_support,system_decision,quality_status,reason_code,
    selected_candidate_artifact_id,ordinal
    FROM bid_matching_staged_requirement_decisions WHERE staging_set_id=p_staging_set_id;
  INSERT INTO bid_matching_candidate_groups(id,report_id,requirement_artifact_id,support,ordinal,canonical_payload,content_sha256)
  SELECT id,p_report_id,requirement_artifact_id,support,ordinal,canonical_payload,content_sha256
    FROM bid_matching_staged_candidate_groups WHERE staging_set_id=p_staging_set_id;
  INSERT INTO bid_current_matching_reports(project_id,route_id,report_id,generation,mutation_watermark)
  VALUES(job.project_id,job.route_id,p_report_id,set_row.generation,set_row.mutation_watermark)
  ON CONFLICT (project_id,route_id) DO UPDATE SET report_id=EXCLUDED.report_id, generation=EXCLUDED.generation,
    mutation_watermark=EXCLUDED.mutation_watermark;
  DELETE FROM bid_current_route_pick_sets WHERE project_id=job.project_id AND route_id=job.route_id;
  IF route_value.route_kind='technical' THEN
    PERFORM kb_bid_rebuild_project_pick_set(job.project_id,'system:matching-publication');
  END IF;
  UPDATE bid_matching_job_claims SET status='completed' WHERE job_id=p_job_id AND attempt=p_attempt AND claim_token=p_claim_token;
  UPDATE bid_matching_jobs SET status='completed',active_attempt=NULL,completed_report_id=p_report_id,finished_at=clock_timestamp() WHERE id=p_job_id;
  UPDATE bid_matching_staging_sets SET state='consumed',consumed_report_id=p_report_id WHERE id=p_staging_set_id;
  IF route_value.route_kind='technical' THEN
    UPDATE bid_current_parts SET stale=true,
      stale_reason_codes=(SELECT ARRAY(SELECT DISTINCT x FROM unnest(stale_reason_codes||ARRAY['MATCHING_REPORT_PUBLISHED']) x ORDER BY 1))
     WHERE project_id=job.project_id AND (
       part_key=CASE
         WHEN route_value.unit_id='00000000-0000-0000-0000-000000000000'::uuid THEN '2:unsectioned'
         ELSE '2:'||route_value.unit_id::text END
       OR part_key IN ('3','6:implementation_plan'));
  ELSE
    UPDATE bid_current_parts SET stale=true,
      stale_reason_codes=(SELECT ARRAY(SELECT DISTINCT x FROM unnest(stale_reason_codes||ARRAY['MATCHING_REPORT_PUBLISHED']) x ORDER BY 1))
     WHERE project_id=job.project_id AND part_key IN ('4','5');
  END IF;
  RETURN jsonb_build_object('status','committed','report_id',p_report_id,'content_sha256',p_expected_report_sha256);
END
$$;

CREATE FUNCTION kb_bid_matching_replace_route_picks(
  p_project_id uuid, p_route_id uuid, p_source_report_id uuid, p_report_sha256 kb_sha256,
  p_expected_revision bigint, p_selections jsonb,
  p_actor kb_actor_identity, p_idempotency_key text, p_request_bytes bytea, p_request_sha256 kb_sha256
)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE replay bytea; report_value bid_matching_reports%ROWTYPE; route bid_matching_routes%ROWTYPE;
 current_rev bigint; pick_id uuid := gen_random_uuid(); revision bigint; digest kb_sha256;
 payload bytea; items jsonb := '[]'::jsonb; item jsonb; rec record; now_ts timestamptz := clock_timestamp();
 project_pick jsonb; response jsonb; ordinal integer := 0;
BEGIN
  IF p_actor LIKE 'system:%' THEN RAISE EXCEPTION 'HUMAN_ACTOR_REQUIRED' USING ERRCODE='42501'; END IF;
  replay := kb_bid_idempotency_begin(p_actor,'bid.matching.route_pick.replace',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  PERFORM 1 FROM bid_projects WHERE id=p_project_id AND status='open' FOR UPDATE;
  IF NOT FOUND THEN RAISE EXCEPTION 'PROJECT_ENDED' USING ERRCODE='55000'; END IF;
  SELECT report_row.* INTO report_value
    FROM bid_current_matching_reports current_value
    JOIN bid_matching_reports report_row ON report_row.id=current_value.report_id
   WHERE current_value.project_id=p_project_id AND current_value.route_id=p_route_id AND report_row.id=p_source_report_id
   FOR UPDATE OF current_value;
  IF report_value.id IS NULL OR report_value.content_sha256<>p_report_sha256 THEN
    RAISE EXCEPTION 'CURRENT_MATCHING_REPORT_MISMATCH' USING ERRCODE='40001';
  END IF;
  SELECT * INTO STRICT route FROM bid_matching_routes WHERE id=p_route_id;
  IF route.route_kind<>'technical' THEN RAISE EXCEPTION 'ROUTE_PICK_REQUIRES_TECHNICAL_REPORT' USING ERRCODE='22023'; END IF;
  SELECT current_pick.revision INTO current_rev
    FROM bid_current_route_pick_sets current_pick
   WHERE current_pick.project_id=p_project_id AND current_pick.route_id=p_route_id
   FOR UPDATE;
  IF COALESCE(current_rev,0)<>p_expected_revision THEN RAISE EXCEPTION 'ROUTE_PICK_REVISION_MISMATCH' USING ERRCODE='40001'; END IF;
  revision := p_expected_revision + 1;
  FOR item IN
    SELECT jsonb_build_object(
             'requirement_artifact_id', selection.requirement_artifact_id,
             'candidate_artifact_id', selection.candidate_artifact_id
           )
      FROM (
        SELECT DISTINCT
               value->>'requirement_artifact_id' AS requirement_artifact_id,
               value->>'candidate_artifact_id' AS candidate_artifact_id
          FROM jsonb_array_elements(COALESCE(p_selections,'[]'::jsonb))
      ) selection
     ORDER BY selection.requirement_artifact_id, selection.candidate_artifact_id
  LOOP
    SELECT candidate.id, product.product_id, product.product_version_id INTO rec
      FROM bid_matching_candidate_artifacts candidate
      JOIN bid_matching_product_version_artifacts product ON product.id=candidate.product_version_artifact_id
     WHERE candidate.report_id=p_source_report_id AND candidate.support='supported'
       AND candidate.id=(item->>'candidate_artifact_id')::uuid
       AND candidate.requirement_artifact_id=(item->>'requirement_artifact_id')::uuid;
    IF rec.id IS NULL THEN RAISE EXCEPTION 'PICK_ITEM_NOT_VISIBLE_SUPPORTED' USING ERRCODE='22023'; END IF;
    items := items || jsonb_build_array(jsonb_build_object(
      'requirement_artifact_id',item->>'requirement_artifact_id','candidate_artifact_id',item->>'candidate_artifact_id',
      'product_id',rec.product_id,'product_version_id',rec.product_version_id,
      'source_report_artifact_id',p_source_report_id,'unit_id',route.unit_id,'selected_by',p_actor,
      'selected_at',kb_bid_utc_json_time(now_ts)));
  END LOOP;
  payload := convert_to('{"schema_version":1,"project_id":"'||p_project_id::text||'","route_id":"'||p_route_id::text
    ||'","source_report_artifact_id":"'||p_source_report_id::text||'","report_generation":'||report_value.generation::text
    ||',"report_sha256":"'||p_report_sha256||'","route_unit_id":'||CASE WHEN route.unit_id IS NULL THEN 'null' ELSE '"'||route.unit_id::text||'"' END
    ||',"revision":'||revision::text||',"items":'||items::text||'}','UTF8');
  digest := encode(public.digest(payload,'sha256'),'hex');
  INSERT INTO bid_route_pick_set_artifacts(id,project_id,route_id,source_report_artifact_id,report_generation,
    report_sha256,route_unit_id,revision,canonical_payload,content_sha256,selected_by,selected_at)
  VALUES(pick_id,p_project_id,p_route_id,p_source_report_id,report_value.generation,p_report_sha256,route.unit_id,
    revision,payload,digest,p_actor,now_ts);
  FOR item IN SELECT value FROM jsonb_array_elements(items) LOOP
    INSERT INTO bid_route_pick_set_items(pick_set_id,ordinal,requirement_artifact_id,candidate_artifact_id,
      product_id,product_version_id,source_report_artifact_id,unit_id,selected_by,selected_at)
    VALUES(pick_id,ordinal,(item->>'requirement_artifact_id')::uuid,(item->>'candidate_artifact_id')::uuid,
      NULLIF(item->>'product_id','')::uuid,(item->>'product_version_id')::uuid,p_source_report_id,route.unit_id,p_actor,now_ts);
    ordinal := ordinal + 1;
  END LOOP;
  INSERT INTO bid_current_route_pick_sets(project_id,route_id,pick_set_id,revision)
  VALUES(p_project_id,p_route_id,pick_id,revision)
  ON CONFLICT (project_id,route_id) DO UPDATE SET pick_set_id=EXCLUDED.pick_set_id, revision=EXCLUDED.revision;
  project_pick := kb_bid_rebuild_project_pick_set(p_project_id,p_actor);
  UPDATE bid_current_parts SET stale=true,
    stale_reason_codes=(SELECT ARRAY(SELECT DISTINCT x FROM unnest(stale_reason_codes||ARRAY['MATCHING_PICK_CHANGED']) x ORDER BY 1))
   WHERE project_id=p_project_id AND (
     part_key = CASE WHEN route.unit_id IS NULL THEN '4'
                     WHEN route.unit_id='00000000-0000-0000-0000-000000000000'::uuid THEN '2:unsectioned'
                     ELSE '2:'||route.unit_id::text END
     OR part_key IN ('3','6:implementation_plan'));
  response := jsonb_build_object('route_pick_set_id',pick_id,'route_revision',revision,'route_sha256',digest,
    'project_pick_set_id',project_pick->>'project_pick_set_id','project_revision',project_pick->'revision',
    'project_sha256',project_pick->>'content_sha256');
  INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
    entity_kind,entity_locator,after_revision,after_sha256)
  VALUES(gen_random_uuid(),1,'bid.matching.route_pick.replace',p_actor,p_idempotency_key,p_request_sha256,
    encode(public.digest(convert_to(response::text,'UTF8'),'sha256'),'hex'),'bid_route_pick_set',
    jsonb_build_object('project_id',p_project_id,'route_id',p_route_id),revision,digest);
  PERFORM kb_bid_idempotency_complete(p_actor,'bid.matching.route_pick.replace',p_idempotency_key,200,convert_to(response::text,'UTF8'));
  RETURN response;
END
$$;

CREATE FUNCTION kb_bid_matching_retry_claim(p_job_id uuid, p_claim_token uuid, p_attempt integer, p_error_code text, p_error_detail text)
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE claim_value bid_matching_job_claims%ROWTYPE; job_value bid_matching_jobs%ROWTYPE;
 now_ts timestamptz;
BEGIN
  SELECT * INTO job_value FROM bid_matching_jobs WHERE id=p_job_id FOR UPDATE;
  IF NOT FOUND OR job_value.status<>'running' OR job_value.active_attempt<>p_attempt THEN
    RAISE EXCEPTION 'MATCHING_CLAIM_LOST' USING ERRCODE='40001';
  END IF;
  SELECT * INTO claim_value FROM bid_matching_job_claims
   WHERE job_id=p_job_id AND attempt=p_attempt AND claim_token=p_claim_token FOR UPDATE;
  now_ts := clock_timestamp();
  IF claim_value.job_id IS NULL OR claim_value.status<>'running'
     OR claim_value.heartbeat_at + make_interval(secs=>claim_value.claim_lease_ms::double precision/1000.0)<=now_ts THEN
    RAISE EXCEPTION 'MATCHING_CLAIM_LOST' USING ERRCODE='40001';
  END IF;
  UPDATE bid_matching_job_claims SET status='failed'
   WHERE job_id=p_job_id AND attempt=p_attempt AND claim_token=p_claim_token;
  UPDATE bid_matching_jobs SET status='pending',active_attempt=NULL,
    error_code=left(p_error_code,128),error_detail=left(p_error_detail,4096)
   WHERE id=p_job_id;
  UPDATE bid_matching_staging_sets SET state='failed' WHERE job_id=p_job_id AND attempt=p_attempt AND claim_token=p_claim_token AND state='active';
END
$$;

CREATE FUNCTION kb_bid_matching_fail_claim(p_job_id uuid, p_claim_token uuid, p_attempt integer, p_error_code text, p_error_detail text)
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE claim_value bid_matching_job_claims%ROWTYPE; job_value bid_matching_jobs%ROWTYPE;
 now_ts timestamptz;
BEGIN
  SELECT * INTO job_value FROM bid_matching_jobs WHERE id=p_job_id FOR UPDATE;
  IF NOT FOUND OR job_value.status<>'running' OR job_value.active_attempt<>p_attempt THEN
    RAISE EXCEPTION 'MATCHING_CLAIM_LOST' USING ERRCODE='40001';
  END IF;
  SELECT * INTO claim_value FROM bid_matching_job_claims
   WHERE job_id=p_job_id AND attempt=p_attempt AND claim_token=p_claim_token FOR UPDATE;
  now_ts := clock_timestamp();
  IF claim_value.job_id IS NULL OR claim_value.status<>'running'
     OR claim_value.heartbeat_at + make_interval(secs=>claim_value.claim_lease_ms::double precision/1000.0)<=now_ts THEN
    RAISE EXCEPTION 'MATCHING_CLAIM_LOST' USING ERRCODE='40001';
  END IF;
  UPDATE bid_matching_job_claims SET status='failed'
   WHERE job_id=p_job_id AND attempt=p_attempt AND claim_token=p_claim_token;
  UPDATE bid_matching_jobs SET status='failed',active_attempt=NULL,
    error_code=left(p_error_code,128),error_detail=left(p_error_detail,4096),finished_at=now_ts
   WHERE id=p_job_id;
  UPDATE bid_matching_staging_sets SET state='failed' WHERE job_id=p_job_id AND attempt=p_attempt AND claim_token=p_claim_token AND state='active';
END
$$;

CREATE FUNCTION kb_bid_matching_reap()
RETURNS integer
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE n int := 0; rec record; claim_value bid_matching_job_claims%ROWTYPE;
BEGIN
  FOR rec IN
    SELECT claim.job_id, claim.attempt, claim.claim_token, job.max_attempts
      FROM bid_matching_job_claims claim
      JOIN bid_matching_jobs job ON job.id=claim.job_id
     WHERE claim.status='running'
       AND job.status='running' AND job.active_attempt=claim.attempt
       AND claim.heartbeat_at + make_interval(secs => claim.claim_lease_ms::double precision/1000.0) <= clock_timestamp()
     FOR UPDATE OF job SKIP LOCKED
  LOOP
    SELECT * INTO claim_value FROM bid_matching_job_claims
     WHERE job_id=rec.job_id AND attempt=rec.attempt AND claim_token=rec.claim_token
       AND status='running'
       AND heartbeat_at + make_interval(secs=>claim_lease_ms::double precision/1000.0)<=clock_timestamp()
     FOR UPDATE;
    CONTINUE WHEN NOT FOUND;
    UPDATE bid_matching_job_claims SET status='reaped'
     WHERE job_id=rec.job_id AND attempt=rec.attempt AND claim_token=rec.claim_token AND status='running';
    CONTINUE WHEN NOT FOUND;
    UPDATE bid_matching_jobs SET status=CASE WHEN rec.attempt>=rec.max_attempts THEN 'failed' ELSE 'pending' END,
      active_attempt=NULL, error_code='CLAIM_LEASE_EXPIRED',
      finished_at=CASE WHEN rec.attempt>=rec.max_attempts THEN clock_timestamp() ELSE finished_at END
     WHERE id=rec.job_id AND status='running' AND active_attempt=rec.attempt;
    CONTINUE WHEN NOT FOUND;
    UPDATE bid_matching_staging_sets SET state='expired' WHERE job_id=rec.job_id AND attempt=rec.attempt AND state='active';
    n := n + 1;
  END LOOP;
  UPDATE bid_matching_staging_sets SET state='expired' WHERE state='active' AND expires_at<=clock_timestamp();
  RETURN n;
END
$$;


-- Runtime identities can read typed projections and execute checked mutations,
-- but receive no direct bidding table DML or current-pointer writes.
REVOKE ALL ON ALL TABLES IN SCHEMA public FROM PUBLIC;
REVOKE ALL ON ALL FUNCTIONS IN SCHEMA public FROM PUBLIC;
GRANT SELECT ON bidding_projects,bidding_documents,bidding_current_section_publication_state,
 bidding_current_clauses,bidding_clause_history,bidding_current_fact_suggestions,
 bidding_fact_suggestion_history,bidding_clause_set_identities,bidding_kind_router_current,
 bidding_procedural_router_current,bidding_template_contract_current,
 bidding_current_matching_reports,bidding_matching_report_history,bidding_current_technical_candidates,
 bidding_current_commercial_decisions,bidding_current_route_pick_sets,bidding_current_project_pick_sets,
 bidding_current_quote_snapshots,bidding_current_part_status,bidding_current_routes,bidding_quote_drafts,
 bidding_quote_lines,bidding_current_company_profiles,bidding_current_submission_profiles,
 bidding_current_procedural_classifications,bidding_current_procedural_decisions,
 bidding_current_attachments,bidding_current_shot_sets,bidding_current_submission_outputs
TO kb_runtime_api,kb_runtime_worker;
GRANT SELECT ON bidding_extraction_sources,bidding_extraction_targets,
 bid_matching_manifests,bid_matching_routes,bid_matching_requirement_artifacts,
 bid_matching_product_version_artifacts,bid_matching_route_memberships,
 bid_matching_frozen_retrieved_hits,bid_matching_jobs,bid_matching_job_claims,
 bid_matching_staging_sets,bid_matching_staged_batches,bid_matching_staging_report_payloads
TO kb_runtime_worker;
GRANT EXECUTE ON FUNCTION
 kb_bid_create_project(uuid,text,uuid,timestamptz,timestamptz,kb_actor_identity,text,bytea,kb_sha256),
 kb_bid_upload_document(uuid,uuid,uuid,text,text,bigint,kb_object_ref,kb_sha256,kb_actor_identity,text,bytea,kb_sha256),
 kb_bid_retry_document_conversion(uuid,uuid,integer,kb_actor_identity,text,bytea,kb_sha256),
 kb_bid_end_project(uuid,bigint,kb_actor_identity,text,bytea,kb_sha256),
 kb_bid_mutate_fact(uuid,text,uuid,text,jsonb,text,text,bigint,kb_actor_identity,text,bytea,kb_sha256),
 kb_bid_create_clause(uuid,uuid,text,text,boolean,kb_actor_identity,text,bytea,kb_sha256),
 kb_bid_mutate_clause(uuid,uuid,text,jsonb,bigint,kb_actor_identity,text,bytea,kb_sha256),
 kb_bid_register_kind_router_contract(text,bytea,kb_sha256,kb_actor_identity,text,bytea,kb_sha256),
 kb_bid_promote_kind_router(text,text,bigint,kb_actor_identity,text,bytea,kb_sha256),
 kb_bid_register_procedural_router_contract(text,bytea,kb_sha256,kb_actor_identity,text,bytea,kb_sha256),
 kb_bid_promote_procedural_router(text,text,bigint,kb_actor_identity,text,bytea,kb_sha256),
 kb_bid_register_template_contract(text,text,bytea,kb_sha256,kb_actor_identity,text,bytea,kb_sha256),
 kb_bid_promote_template_contract(text,text,text,bigint,kb_actor_identity,text,bytea,kb_sha256),
 kb_bid_create_quote_draft(uuid,text,text,text,kb_actor_identity,text,bytea,kb_sha256),
 kb_bid_patch_quote_header(uuid,bigint,text,text,text,kb_actor_identity,text,bytea,kb_sha256),
 kb_bid_upsert_quote_line(uuid,uuid,bigint,integer,text,text,text,text,text,text,text,boolean,kb_actor_identity,text,bytea,kb_sha256),
 kb_bid_delete_quote_line(uuid,uuid,bigint,kb_actor_identity,text,bytea,kb_sha256),
 kb_bid_reorder_quote_lines(uuid,bigint,uuid[],kb_actor_identity,text,bytea,kb_sha256),
 kb_bid_preview_quote_totals(uuid),
 kb_bid_finalize_quote(uuid,bigint,bigint,bigint,kb_sha256,bigint,kb_sha256,boolean,text,kb_actor_identity,text,bytea,kb_sha256),
 kb_bid_reopen_quote(uuid,uuid,bigint,bigint,kb_actor_identity,text,bytea,kb_sha256),
 kb_bid_quote_state_json(uuid),
 kb_bid_get_quote_snapshot(uuid,uuid),
 kb_bid_update_company_profile(uuid,bigint,text,text,text,text,text,text,text,kb_actor_identity,text,bytea,kb_sha256),
 kb_bid_update_submission_profile(uuid,bigint,text,text,text,date,text,boolean,boolean,kb_actor_identity,text,bytea,kb_sha256),
 kb_bid_override_procedural_classification(uuid,uuid,text,text,kb_actor_identity,text,bytea,kb_sha256),
 kb_bid_resolve_procedural_requirement(uuid,uuid,text,uuid,text,kb_actor_identity,text,bytea,kb_sha256),
 kb_bid_upload_attachment(uuid,uuid,uuid,text,kb_object_ref,kb_sha256,text,bigint,integer,integer,jsonb,kb_actor_identity,text,bytea,kb_sha256),
 kb_bid_mutate_attachment(uuid,uuid,text,integer,text,kb_actor_identity,text,bytea,kb_sha256),
 kb_bid_upload_shot_artifact(uuid,uuid,uuid,kb_object_ref,kb_sha256,text,bigint,integer,integer,kb_actor_identity,text,bytea,kb_sha256),
 kb_bid_replace_shot_set(uuid,bigint,uuid[],kb_actor_identity,text,bytea,kb_sha256),
 kb_bid_update_part(uuid,text,bigint,bytea,kb_actor_identity,text,bytea,kb_sha256),
 kb_bid_regenerate_part(uuid,text,bigint,kb_sha256,kb_actor_identity,text,bytea,kb_sha256),
 kb_bid_list_gate_issues(uuid,text),
 kb_bid_required_part_keys(uuid),
 kb_bid_get_part(uuid,text),
 kb_bid_create_submission_manifest(uuid,uuid,text,kb_actor_identity,text,bytea,kb_sha256),
 kb_bid_manifest_render_input(uuid,uuid),
 kb_bid_read_manifest_render_asset(uuid,uuid,uuid),
 kb_bid_schedule_submission_render(uuid,uuid,uuid,kb_sha256,kb_actor_identity,text,bytea,kb_sha256),
 kb_bid_get_submission_render_job(uuid,uuid),
 kb_bid_download_submission_output(uuid,uuid),
 kb_bid_matching_schedule(uuid,bigint,integer,jsonb,kb_actor_identity,text,bytea,kb_sha256),
 kb_bid_matching_replace_route_picks(uuid,uuid,uuid,kb_sha256,bigint,jsonb,kb_actor_identity,text,bytea,kb_sha256)
TO kb_runtime_api;
GRANT EXECUTE ON FUNCTION
 kb_bid_claim_document_conversion(uuid,uuid,text),kb_bid_heartbeat_document_conversion(uuid,uuid),
 kb_bid_complete_document_conversion(uuid,uuid,uuid,bytea,text,kb_sha256,jsonb,uuid,integer,text,text,kb_actor_identity),
 kb_bid_fail_document_conversion(uuid,uuid,text,boolean),
 kb_bid_schedule_extraction(uuid,uuid,integer,text,text,kb_actor_identity,text,bytea,kb_sha256),
 kb_bid_claim_extraction(uuid,uuid,text),kb_bid_heartbeat_extraction(uuid,uuid,integer),
 kb_bid_publish_extraction_section(uuid,integer,uuid,text,jsonb,bigint,bigint,uuid,jsonb,kb_actor_identity,text,bytea,kb_sha256),
 kb_bid_fail_extraction(uuid,integer,uuid,text,boolean),
 kb_bid_housekeep_end_expired(),
 kb_bid_reclaim_stale_conversions(),
 kb_bid_pending_conversions(),
 kb_bid_reclaim_stale_extractions(),
 kb_bid_pending_extractions(),
 kb_bid_dirty_match_projects(),
 kb_bid_matching_schedule(uuid,bigint,integer,jsonb,kb_actor_identity,text,bytea,kb_sha256),
 kb_bid_matching_claim(uuid,uuid),
 kb_bid_matching_heartbeat(uuid,uuid,integer,integer,bigint,integer),
 kb_bid_matching_open_staging(jsonb),
 kb_bid_matching_stage_batch(jsonb),
 kb_bid_matching_stage_report_payload(uuid,bytea,kb_sha256),
 kb_bid_matching_commit(uuid,uuid,integer,uuid,uuid,uuid,kb_sha256),
 kb_bid_matching_retry_claim(uuid,uuid,integer,text,text),
 kb_bid_matching_fail_claim(uuid,uuid,integer,text,text),
 kb_bid_matching_reap(),
 kb_bid_manifest_render_input(uuid,uuid),
 kb_bid_read_manifest_render_asset(uuid,uuid,uuid),
 kb_bid_claim_submission_render(uuid,uuid),
 kb_bid_heartbeat_submission_render(uuid,uuid),
 kb_bid_fail_submission_render(uuid,uuid,text,text,boolean),
 kb_bid_reap_submission_renders(),
 kb_bid_pending_submission_renders(),
 kb_bid_publish_submission_output(uuid,uuid,uuid,uuid,kb_object_ref,kb_sha256,bigint)
TO kb_runtime_worker;
