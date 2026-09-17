-- KnowledgeBrain Target V2 fresh bidding foundation.
-- This create-only baseline is an inactive Phase 0 fixture until the Phase 7
-- fresh cutover. It contains no fixed parts, business gate, profile/procedural
-- specialization, compatibility views, scheduler, lease, retry, or fan-out state.

CREATE FUNCTION kb_bid_v2_sha256_bytes(p_bytes bytea)
RETURNS kb_sha256
LANGUAGE sql
IMMUTABLE
STRICT
PARALLEL SAFE
SET search_path = pg_catalog, public
AS $$ SELECT encode(public.digest(p_bytes, 'sha256'), 'hex')::kb_sha256 $$;

CREATE FUNCTION kb_bid_v2_json_keys_exact(value jsonb, expected text[])
RETURNS boolean LANGUAGE sql IMMUTABLE PARALLEL SAFE
SET search_path=pg_catalog,public AS $$
  SELECT COALESCE(
    jsonb_typeof(value)='object'
    AND ARRAY(SELECT key FROM jsonb_object_keys(value) key ORDER BY key)=
        ARRAY(SELECT key FROM unnest(expected) key ORDER BY key),
    false)
$$;

CREATE FUNCTION kb_bid_v2_applicability_valid(value jsonb)
RETURNS boolean LANGUAGE sql IMMUTABLE PARALLEL SAFE
SET search_path=pg_catalog,public AS $$
  SELECT COALESCE(
    value='{}'::jsonb OR (
      kb_bid_v2_json_keys_exact(value,ARRAY['fragments'])
      AND jsonb_typeof(value->'fragments')='array'
      AND jsonb_array_length(value->'fragments') BETWEEN 1 AND 1024
      AND NOT EXISTS (SELECT 1 FROM jsonb_array_elements(value->'fragments') fragment
        WHERE jsonb_typeof(fragment)<>'string' OR octet_length(fragment#>>'{}') NOT BETWEEN 1 AND 256)
      AND jsonb_array_length(value->'fragments')=(SELECT count(DISTINCT fragment#>>'{}')
        FROM jsonb_array_elements(value->'fragments') fragment)), false)
$$;

CREATE FUNCTION kb_bid_v2_applicability_fragments(value jsonb)
RETURNS text[] LANGUAGE sql IMMUTABLE PARALLEL SAFE
SET search_path=pg_catalog,public AS $$
  SELECT CASE WHEN NOT kb_bid_v2_applicability_valid(value) THEN NULL
    WHEN value='{}'::jsonb THEN ARRAY['*']::text[]
    ELSE ARRAY(SELECT fragment#>>'{}' FROM jsonb_array_elements(value->'fragments') fragment ORDER BY fragment#>>'{}') END
$$;

CREATE FUNCTION kb_bid_v2_applicability_from_fragments(fragments text[])
RETURNS jsonb LANGUAGE sql IMMUTABLE PARALLEL SAFE
SET search_path=pg_catalog,public AS $$
  SELECT CASE WHEN fragments=ARRAY['*']::text[] THEN '{}'::jsonb
    ELSE jsonb_build_object('fragments',to_jsonb(ARRAY(SELECT DISTINCT fragment FROM unnest(fragments) fragment ORDER BY fragment))) END
$$;

CREATE FUNCTION kb_bid_v2_fulfillment_need_ids(value jsonb)
RETURNS uuid[] LANGUAGE plpgsql IMMUTABLE PARALLEL SAFE
SET search_path=pg_catalog,public AS $$
DECLARE kind text; child jsonb; ids uuid[]:='{}'::uuid[]; child_ids uuid[];
BEGIN
  IF jsonb_typeof(value)<>'object' OR jsonb_typeof(value->'kind')<>'string' THEN RETURN NULL; END IF;
  kind:=value->>'kind';
  -- A root with no submission obligations is not a fabricated response need.
  IF value='{"kind":"all_of","children":[]}'::jsonb THEN RETURN ids; END IF;
  IF kind='need' THEN
    IF NOT kb_bid_v2_json_keys_exact(value,ARRAY['kind','need_occurrence_id','channel'])
       OR NOT kb_bid_v2_uuid_text(value->>'need_occurrence_id')
       OR value->>'channel' NOT IN ('narrative_content','response_table','deviation_statement','structured_form','evidence_attachment','quotation')
    THEN RETURN NULL; END IF;
    RETURN ARRAY[(value->>'need_occurrence_id')::uuid];
  END IF;
  IF kind NOT IN ('all_of','any_of','at_least')
     OR (kind IN ('all_of','any_of') AND NOT kb_bid_v2_json_keys_exact(value,ARRAY['kind','children']))
     OR (kind='at_least' AND NOT kb_bid_v2_json_keys_exact(value,ARRAY['kind','min_count','children']))
     OR jsonb_typeof(value->'children')<>'array' OR jsonb_array_length(value->'children')=0
     OR (kind='at_least' AND (jsonb_typeof(value->'min_count')<>'number'
       OR coalesce(value->>'min_count','')!~'^[1-9][0-9]*$'
       OR (value->>'min_count')::integer>jsonb_array_length(value->'children')))
  THEN RETURN NULL; END IF;
  FOR child IN SELECT item FROM jsonb_array_elements(value->'children') item LOOP
    child_ids:=kb_bid_v2_fulfillment_need_ids(child);
    IF child_ids IS NULL OR cardinality(child_ids)=0 THEN RETURN NULL; END IF;
    ids:=ids||child_ids;
  END LOOP;
  RETURN ids;
END $$;

CREATE FUNCTION kb_bid_v2_fulfillment_needs(value jsonb)
RETURNS jsonb LANGUAGE plpgsql IMMUTABLE PARALLEL SAFE
SET search_path=pg_catalog,public AS $$
DECLARE kind text; child jsonb; result_value jsonb:='[]'::jsonb; child_value jsonb;
BEGIN
  IF jsonb_typeof(value)<>'object' OR jsonb_typeof(value->'kind')<>'string' THEN RETURN NULL; END IF;
  kind:=value->>'kind';
  IF kind='need' THEN
    IF NOT kb_bid_v2_json_keys_exact(value,ARRAY['kind','need_occurrence_id','channel'])
      OR NOT kb_bid_v2_uuid_text(value->>'need_occurrence_id')
      OR value->>'channel' NOT IN ('narrative_content','response_table','deviation_statement','structured_form','evidence_attachment','quotation')
    THEN RETURN NULL; END IF;
    RETURN jsonb_build_array(jsonb_build_object(
      'need_occurrence_id',value->>'need_occurrence_id','channel',value->>'channel'));
  END IF;
  IF kind NOT IN ('all_of','any_of','at_least') OR jsonb_typeof(value->'children')<>'array' THEN RETURN NULL; END IF;
  FOR child IN SELECT item FROM jsonb_array_elements(value->'children') item LOOP
    child_value:=kb_bid_v2_fulfillment_needs(child); IF child_value IS NULL THEN RETURN NULL; END IF;
    result_value:=result_value||child_value;
  END LOOP;
  RETURN result_value;
END $$;

CREATE FUNCTION kb_bid_v2_fulfillment_expr_valid(value jsonb)
RETURNS boolean LANGUAGE sql IMMUTABLE PARALLEL SAFE SET search_path=pg_catalog,public AS $$
  SELECT ids IS NOT NULL AND (cardinality(ids)>0 OR value='{"kind":"all_of","children":[]}'::jsonb)
    AND cardinality(ids)=(SELECT count(DISTINCT item) FROM unnest(ids) item)
  FROM (SELECT kb_bid_v2_fulfillment_need_ids(value) ids) validated
$$;

CREATE FUNCTION kb_bid_v2_deterministic_uuid(value text)
RETURNS uuid LANGUAGE sql IMMUTABLE PARALLEL SAFE SET search_path=pg_catalog,public AS $$
  SELECT (substr(hash,1,12)||'5'||substr(hash,14,3)||'8'||substr(hash,18,15))::uuid
  FROM (SELECT md5(value) hash) identity_value
$$;

CREATE FUNCTION kb_bid_v2_uuid_text(value text)
RETURNS boolean LANGUAGE sql IMMUTABLE PARALLEL SAFE
SET search_path=pg_catalog,public AS $$
  SELECT COALESCE(value ~ '^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$',false)
$$;

CREATE FUNCTION kb_bid_v2_sha256_text(value text)
RETURNS boolean LANGUAGE sql IMMUTABLE PARALLEL SAFE
SET search_path=pg_catalog,public AS $$ SELECT COALESCE(value ~ '^[0-9a-f]{64}$',false) $$;

-- Exact parity with Rust `validate_reason`: NFC, scalar/byte bounds, no C0,
-- and no leading/trailing code point accepted by `char::is_whitespace`.

-- Draft 2020-12 `format: date-time` is RFC3339, not PostgreSQL's wider
-- timestamptz input language. This helper is deliberately lexical first, then
-- parses only the closed form and compares the exact instant to the row value.
CREATE FUNCTION kb_bid_v2_rfc3339_datetime_matches(value text, expected timestamptz)
RETURNS boolean LANGUAGE plpgsql IMMUTABLE PARALLEL SAFE
SET search_path=pg_catalog,public AS $$
DECLARE parsed timestamptz;
BEGIN
  IF value IS NULL OR expected IS NULL OR value !~
    '^[0-9]{4}-(0[1-9]|1[0-2])-(0[1-9]|[12][0-9]|3[01])T([01][0-9]|2[0-3]):[0-5][0-9]:[0-5][0-9](\.[0-9]+)?(Z|[+-]([01][0-9]|2[0-3]):[0-5][0-9])$'
  THEN RETURN false; END IF;
  parsed:=value::timestamptz;
  RETURN isfinite(parsed) AND parsed=expected;
EXCEPTION WHEN datetime_field_overflow OR invalid_datetime_format THEN
  RETURN false;
END $$;

CREATE FUNCTION kb_bid_v2_guard_current_pointer()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
BEGIN
  IF TG_OP = 'DELETE' THEN
    RAISE EXCEPTION 'current pointer cannot be deleted' USING ERRCODE='42501';
  END IF;
  IF OLD.scope_id IS DISTINCT FROM NEW.scope_id
     OR OLD.created_at IS DISTINCT FROM NEW.created_at
     OR NEW.generation <> OLD.generation + 1 THEN
    RAISE EXCEPTION 'invalid current pointer transition' USING ERRCODE='40001';
  END IF;
  RETURN NEW;
END
$$;

CREATE TABLE bid_projects (
  id uuid PRIMARY KEY,
  owner_user_id uuid NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
  title text NOT NULL CHECK (octet_length(title) BETWEEN 1 AND 1024),
  status text NOT NULL CHECK (status IN ('open','ended')),
  created_at timestamptz NOT NULL DEFAULT now(),
  ended_at timestamptz,
  CHECK ((status='open' AND ended_at IS NULL) OR (status='ended' AND ended_at IS NOT NULL))
);

CREATE TABLE bid_documents (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
  file_name text NOT NULL CHECK (octet_length(file_name) BETWEEN 1 AND 1024),
  media_type text NOT NULL CHECK (media_type IN (
    'application/pdf',
    'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
    'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet',
    'application/vnd.ms-excel.sheet.macroEnabled.12',
    'application/msword',
    'application/vnd.ms-excel',
    'image/png','image/jpeg','image/webp'
  )),
  byte_length bigint NOT NULL CHECK (byte_length > 0),
  original_object_ref kb_object_ref NOT NULL,
  original_sha256 kb_sha256 NOT NULL,
  parse_status text NOT NULL CHECK (parse_status IN ('pending','processing','ready','failed')),
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(project_id,id),
  CHECK (original_object_ref='objects/'||original_sha256)
);

CREATE TABLE bid_document_role_revision_artifacts (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
  document_id uuid NOT NULL,
  revision bigint NOT NULL CHECK (revision > 0),
  role text NOT NULL CHECK (role IN (
    'primary_tender','bid_format','technical_specification','commercial_requirement',
    'bill_of_quantities','contract','drawing','clarification','amendment','other_attachment'
  )),
  provenance text NOT NULL CHECK (provenance IN ('system_suggested','human_confirmed','human_modified')),
  canonical_payload bytea NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  actor kb_actor_identity NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(project_id,document_id,revision),
  UNIQUE(project_id,id),
  FOREIGN KEY(project_id,document_id) REFERENCES bid_documents(project_id,id) ON DELETE RESTRICT,
  CHECK (content_sha256=kb_bid_v2_sha256_bytes(canonical_payload))
);

CREATE TABLE bid_document_role_current (
  scope_id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  artifact_id uuid NOT NULL,
  generation bigint NOT NULL CHECK (generation > 0),
  created_at timestamptz NOT NULL,
  FOREIGN KEY(project_id,scope_id,generation) REFERENCES bid_document_role_revision_artifacts(project_id,document_id,revision),
  FOREIGN KEY(project_id,artifact_id) REFERENCES bid_document_role_revision_artifacts(project_id,id)
);

CREATE TABLE bid_document_relation_revision_artifacts (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
  relation_lineage_id uuid NOT NULL,
  revision bigint NOT NULL CHECK (revision > 0),
  from_document_id uuid NOT NULL,
  to_document_id uuid NOT NULL,
  relation_kind text NOT NULL CHECK (relation_kind IN ('complements','clarifies','partially_amends','replaces','withdraws')),
  applicability jsonb NOT NULL CHECK (jsonb_typeof(applicability)='object'),
  tombstone boolean NOT NULL DEFAULT false,
  canonical_payload bytea NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  actor kb_actor_identity NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(project_id,relation_lineage_id,revision),
  UNIQUE(project_id,id),
  FOREIGN KEY(project_id,from_document_id) REFERENCES bid_documents(project_id,id),
  FOREIGN KEY(project_id,to_document_id) REFERENCES bid_documents(project_id,id),
  CHECK (from_document_id<>to_document_id),
  CHECK (content_sha256=kb_bid_v2_sha256_bytes(canonical_payload))
);

CREATE TABLE bid_document_relation_current (
  scope_id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  artifact_id uuid NOT NULL,
  generation bigint NOT NULL CHECK (generation > 0),
  created_at timestamptz NOT NULL,
  FOREIGN KEY(project_id,scope_id,generation) REFERENCES bid_document_relation_revision_artifacts(project_id,relation_lineage_id,revision),
  FOREIGN KEY(project_id,artifact_id) REFERENCES bid_document_relation_revision_artifacts(project_id,id)
);

CREATE TABLE bid_converted_source_artifacts (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  document_id uuid NOT NULL,
  revision bigint NOT NULL CHECK (revision > 0),
  source_object_ref kb_object_ref NOT NULL,
  source_sha256 kb_sha256 NOT NULL,
  converter_contract_id uuid NOT NULL,
  converter_contract_sha256 kb_sha256 NOT NULL,
  image_asset_set_sha256 kb_sha256 NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(project_id,document_id,revision),
  UNIQUE(project_id,id),
  FOREIGN KEY(project_id,document_id) REFERENCES bid_documents(project_id,id),
  CHECK (source_object_ref='objects/'||source_sha256)
);

CREATE TABLE bid_document_set_artifacts (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
  revision bigint NOT NULL CHECK (revision > 0),
  canonical_payload bytea NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  actor kb_actor_identity NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(project_id,revision),
  UNIQUE(project_id,id),
  CHECK (content_sha256=kb_bid_v2_sha256_bytes(canonical_payload))
);

CREATE TABLE bid_document_set_items (
  document_set_id uuid NOT NULL,
  project_id uuid NOT NULL,
  document_id uuid NOT NULL,
  ordinal integer NOT NULL CHECK (ordinal >= 0),
  role_revision_id uuid NOT NULL,
  source_revision_id uuid,
  disposition text NOT NULL CHECK (disposition IN ('ready','pending','failed','unresolved')),
  PRIMARY KEY(document_set_id,document_id),
  UNIQUE(document_set_id,ordinal),
  FOREIGN KEY(project_id,document_set_id) REFERENCES bid_document_set_artifacts(project_id,id),
  FOREIGN KEY(project_id,document_id) REFERENCES bid_documents(project_id,id),
  FOREIGN KEY(project_id,role_revision_id) REFERENCES bid_document_role_revision_artifacts(project_id,id),
  FOREIGN KEY(project_id,source_revision_id) REFERENCES bid_converted_source_artifacts(project_id,id)
);

CREATE TABLE bid_document_set_current (
  scope_id uuid PRIMARY KEY,
  artifact_id uuid NOT NULL,
  artifact_sha256 kb_sha256 NOT NULL,
  generation bigint NOT NULL CHECK (generation > 0),
  created_at timestamptz NOT NULL,
  FOREIGN KEY(scope_id,generation) REFERENCES bid_document_set_artifacts(project_id,revision),
  FOREIGN KEY(scope_id,artifact_id) REFERENCES bid_document_set_artifacts(project_id,id)
);

CREATE TABLE bid_source_unit_lineages (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
  document_id uuid NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(project_id,id),
  FOREIGN KEY(project_id,document_id) REFERENCES bid_documents(project_id,id)
);

CREATE TABLE bid_source_unit_revision_artifacts (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  lineage_id uuid NOT NULL,
  revision bigint NOT NULL CHECK (revision > 0),
  document_id uuid NOT NULL,
  source_revision_id uuid NOT NULL,
  unit_kind text NOT NULL CHECK (unit_kind IN ('section','table_row','form_region','attachment_region','image_ocr_region')),
  ordinal integer NOT NULL CHECK (ordinal >= 0),
  source_locator jsonb NOT NULL CHECK (
    jsonb_typeof(source_locator)='object'
    AND source_locator->>'schema_version'='2'
    AND source_locator->>'project_id'=project_id::text
    AND source_locator->>'document_id'=document_id::text
    AND source_locator->>'converted_source_revision_id'=source_revision_id::text
    AND source_locator->>'source_purpose'='tender_requirements_and_structure_only'
    AND length(source_locator->>'parser_unit_key')>0
    AND (source_locator->>'parser_ordinal')::integer=ordinal
    AND jsonb_typeof(source_locator->'locator')='object'
    AND source_locator ?& ARRAY['schema_version','project_id','document_id','converted_source_revision_id','parser_unit_key','parser_ordinal','source_purpose','locator']
    AND (source_locator-'schema_version'-'project_id'-'document_id'-'converted_source_revision_id'-'parser_unit_key'-'parser_ordinal'-'source_purpose'-'locator'-'tender_image_artifact_revision_id')='{}'::jsonb
  ),
  source_span_sha256 kb_sha256 NOT NULL,
  text_utf8 bytea NOT NULL,
  text_sha256 kb_sha256 NOT NULL,
  canonical_payload bytea NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(project_id,lineage_id,revision),
  UNIQUE(project_id,id),
  FOREIGN KEY(project_id,lineage_id) REFERENCES bid_source_unit_lineages(project_id,id),
  FOREIGN KEY(project_id,document_id) REFERENCES bid_documents(project_id,id),
  FOREIGN KEY(project_id,source_revision_id) REFERENCES bid_converted_source_artifacts(project_id,id),
  CHECK (text_sha256=kb_bid_v2_sha256_bytes(text_utf8)),
  CHECK (content_sha256=kb_bid_v2_sha256_bytes(canonical_payload))
);

-- OCR/VLM output is a frozen tender-source artifact, never knowledge evidence.
-- Both the original image bytes and exact OCR UTF-8 bytes are qualified by
-- shared ObjectRegistry identities and transferred by the publication procedure.
CREATE TABLE bid_tender_source_image_revision_artifacts (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  document_id uuid NOT NULL,
  source_revision_id uuid NOT NULL,
  ordinal integer NOT NULL CHECK (ordinal >= 0),
  source_purpose text NOT NULL CHECK (source_purpose='tender_requirements_and_structure_only'),
  source_locator jsonb NOT NULL CHECK (jsonb_typeof(source_locator)='object'),
  original_object_ref kb_object_ref NOT NULL,
  original_sha256 kb_sha256 NOT NULL,
  original_media_type text NOT NULL CHECK (original_media_type IN ('image/png','image/jpeg','image/webp')),
  original_byte_length bigint NOT NULL CHECK (original_byte_length>0),
  original_object_state text NOT NULL DEFAULT 'available' CHECK (original_object_state='available'),
  ocr_object_ref kb_object_ref NOT NULL,
  ocr_sha256 kb_sha256 NOT NULL,
  ocr_media_type text NOT NULL CHECK (ocr_media_type='text/plain'),
  ocr_byte_length bigint NOT NULL CHECK (ocr_byte_length>0),
  ocr_object_state text NOT NULL DEFAULT 'available' CHECK (ocr_object_state='available'),
  model_contract_id uuid NOT NULL,
  model_contract_sha256 kb_sha256 NOT NULL,
  operation_contract_id uuid NOT NULL,
  operation_contract_sha256 kb_sha256 NOT NULL,
  canonical_payload bytea NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(project_id,document_id,source_revision_id,ordinal),
  UNIQUE(project_id,id),
  UNIQUE(project_id,id,content_sha256),
  CHECK (original_object_ref='objects/'||original_sha256),
  CHECK (ocr_object_ref='objects/'||ocr_sha256),
  CHECK (content_sha256=kb_bid_v2_sha256_bytes(canonical_payload))
);

CREATE TABLE bid_source_unit_disposition_set_artifacts (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  document_set_id uuid NOT NULL,
  document_set_sequence bigint NOT NULL CHECK (document_set_sequence > 0),
  revision bigint NOT NULL CHECK (revision > 0),
  canonical_payload bytea NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  actor kb_actor_identity NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(project_id,document_set_id,revision),
  UNIQUE(project_id,id),
  UNIQUE(project_id,id,document_set_id),
  FOREIGN KEY(project_id,document_set_id) REFERENCES bid_document_set_artifacts(project_id,id),
  CHECK (content_sha256=kb_bid_v2_sha256_bytes(canonical_payload))
);

CREATE TABLE bid_source_unit_disposition_set_items (
  disposition_set_id uuid NOT NULL,
  project_id uuid NOT NULL,
  source_unit_revision_id uuid NOT NULL,
  disposition text NOT NULL CHECK (disposition IN ('requirement','non_requirement','unresolved')),
  reason text CHECK (reason IS NULL OR octet_length(reason) BETWEEN 1 AND 4096),
  PRIMARY KEY(disposition_set_id,source_unit_revision_id),
  FOREIGN KEY(project_id,disposition_set_id) REFERENCES bid_source_unit_disposition_set_artifacts(project_id,id),
  FOREIGN KEY(project_id,source_unit_revision_id) REFERENCES bid_source_unit_revision_artifacts(project_id,id)
);

CREATE TABLE bid_source_unit_disposition_set_current (
  scope_id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  document_set_id uuid NOT NULL,
  artifact_id uuid NOT NULL,
  artifact_sha256 kb_sha256 NOT NULL,
  generation bigint NOT NULL CHECK (generation > 0),
  created_at timestamptz NOT NULL,
  CHECK (scope_id=project_id),
  FOREIGN KEY(project_id,artifact_id) REFERENCES bid_source_unit_disposition_set_artifacts(project_id,id)
);

CREATE TABLE bid_tender_structured_form_definition_artifacts (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  source_unit_revision_id uuid NOT NULL,
  schema_version smallint NOT NULL CHECK (schema_version IN (1, 2, 3)),
  canonical_payload bytea NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(project_id,id),
  UNIQUE(project_id,id,content_sha256),
  FOREIGN KEY(project_id,source_unit_revision_id) REFERENCES bid_source_unit_revision_artifacts(project_id,id),
  CHECK (content_sha256=kb_bid_v2_sha256_bytes(canonical_payload))
);

CREATE TABLE bid_requirement_set_artifacts (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
  document_set_id uuid NOT NULL,
  document_set_sequence bigint NOT NULL CHECK (document_set_sequence > 0),
  disposition_set_id uuid NOT NULL,
  disposition_set_sequence bigint NOT NULL CHECK (disposition_set_sequence > 0),
  revision bigint NOT NULL CHECK (revision > 0),
  canonical_payload bytea NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(project_id,revision),
  UNIQUE(project_id,id),
  FOREIGN KEY(project_id,document_set_id) REFERENCES bid_document_set_artifacts(project_id,id),
  FOREIGN KEY(project_id,disposition_set_id,document_set_id)
    REFERENCES bid_source_unit_disposition_set_artifacts(project_id,id,document_set_id),
  CHECK (content_sha256=kb_bid_v2_sha256_bytes(canonical_payload))
);

CREATE TABLE bid_requirement_revision_artifacts (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
  lineage_id uuid NOT NULL,
  revision bigint NOT NULL CHECK (revision > 0),
  requirement_kind text NOT NULL CHECK (requirement_kind IN ('qualification','technical','commercial','pricing','delivery','evaluation','format','attachment','other','personnel','rejection')),
  requiredness text NOT NULL CHECK (requiredness IN ('mandatory','optional','informational','unknown')),
  compliance_policy text NOT NULL CHECK (compliance_policy IN ('must_comply','explicit_response','deviation_allowed','scored','unknown')),
  lifecycle text NOT NULL CHECK (lifecycle IN ('current','superseded','withdrawn','unresolved')),
  text_utf8 bytea NOT NULL,
  text_sha256 kb_sha256 NOT NULL,
  fulfillment_expr jsonb NOT NULL CHECK (kb_bid_v2_fulfillment_expr_valid(fulfillment_expr)),
  applicability jsonb NOT NULL CHECK (kb_bid_v2_applicability_valid(applicability)),
  tombstone boolean NOT NULL DEFAULT false,
  canonical_payload bytea NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  actor kb_actor_identity NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(project_id,lineage_id,revision),
  UNIQUE(project_id,id),
  CHECK (text_sha256=kb_bid_v2_sha256_bytes(text_utf8)),
  CHECK (content_sha256=kb_bid_v2_sha256_bytes(canonical_payload))
);

CREATE TABLE bid_requirement_set_items (
  requirement_set_id uuid NOT NULL,
  project_id uuid NOT NULL,
  requirement_revision_id uuid NOT NULL,
  effective_applicability jsonb NOT NULL DEFAULT '{}'::jsonb CHECK (kb_bid_v2_applicability_valid(effective_applicability)),
  ordinal integer NOT NULL CHECK (ordinal >= 0),
  PRIMARY KEY(requirement_set_id,requirement_revision_id),
  UNIQUE(requirement_set_id,ordinal),
  FOREIGN KEY(project_id,requirement_set_id) REFERENCES bid_requirement_set_artifacts(project_id,id),
  FOREIGN KEY(project_id,requirement_revision_id) REFERENCES bid_requirement_revision_artifacts(project_id,id)
);

CREATE TABLE bid_requirement_source_revision_artifacts (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  requirement_revision_id uuid NOT NULL,
  source_unit_revision_id uuid NOT NULL,
  quote_start_offset bigint NOT NULL CHECK (quote_start_offset >= 0),
  quote_end_offset bigint NOT NULL CHECK (quote_end_offset > quote_start_offset),
  quote_sha256 kb_sha256 NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(project_id,requirement_revision_id,source_unit_revision_id,quote_start_offset,quote_end_offset),
  FOREIGN KEY(project_id,requirement_revision_id) REFERENCES bid_requirement_revision_artifacts(project_id,id),
  FOREIGN KEY(project_id,source_unit_revision_id) REFERENCES bid_source_unit_revision_artifacts(project_id,id)
);

CREATE FUNCTION kb_bid_v2_guard_published_requirement_source_insert()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
BEGIN
  IF EXISTS (SELECT 1 FROM bid_requirement_set_items item
      WHERE item.requirement_revision_id=NEW.requirement_revision_id) THEN
    RAISE EXCEPTION 'published RequirementSource identity is immutable' USING ERRCODE='42501';
  END IF;
  RETURN NEW;
END $$;
CREATE TRIGGER bid_requirement_source_published_insert_guard
BEFORE INSERT ON bid_requirement_source_revision_artifacts
FOR EACH ROW EXECUTE FUNCTION kb_bid_v2_guard_published_requirement_source_insert();

CREATE TABLE bid_requirement_supersession_revision_artifacts (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  lineage_id uuid NOT NULL,
  revision bigint NOT NULL CHECK (revision > 0),
  old_requirement_revision_id uuid NOT NULL,
  new_requirement_revision_id uuid NOT NULL,
  old_source_unit_revision_ids uuid[] NOT NULL DEFAULT '{}'::uuid[],
  new_source_unit_revision_ids uuid[] NOT NULL DEFAULT '{}'::uuid[],
  amendment_document_relation_revision_id uuid,
  amendment_document_relation_sha256 kb_sha256,
  applicability jsonb NOT NULL CHECK (kb_bid_v2_applicability_valid(applicability)),
  tombstone boolean NOT NULL DEFAULT false,
  canonical_payload bytea NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  actor kb_actor_identity NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(project_id,lineage_id,revision),
  UNIQUE(project_id,id),
  FOREIGN KEY(project_id,old_requirement_revision_id) REFERENCES bid_requirement_revision_artifacts(project_id,id),
  FOREIGN KEY(project_id,new_requirement_revision_id) REFERENCES bid_requirement_revision_artifacts(project_id,id),
  CHECK (old_requirement_revision_id<>new_requirement_revision_id),
  CHECK ((amendment_document_relation_revision_id IS NULL)=(amendment_document_relation_sha256 IS NULL)),
  CHECK (content_sha256=kb_bid_v2_sha256_bytes(canonical_payload))
);

CREATE TABLE bid_requirement_supersession_current (
  scope_id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  artifact_id uuid NOT NULL,
  artifact_sha256 kb_sha256 NOT NULL,
  generation bigint NOT NULL CHECK (generation > 0),
  created_at timestamptz NOT NULL,
  FOREIGN KEY(project_id,artifact_id) REFERENCES bid_requirement_supersession_revision_artifacts(project_id,id)
);

CREATE TABLE bid_requirement_set_current (
  scope_id uuid PRIMARY KEY,
  artifact_id uuid NOT NULL,
  artifact_sha256 kb_sha256 NOT NULL,
  generation bigint NOT NULL CHECK (generation > 0),
  document_set_sequence bigint NOT NULL CHECK (document_set_sequence > 0),
  disposition_set_sequence bigint NOT NULL CHECK (disposition_set_sequence > 0),
  created_at timestamptz NOT NULL,
  FOREIGN KEY(scope_id,artifact_id) REFERENCES bid_requirement_set_artifacts(project_id,id)
);

CREATE TABLE bid_submission_workspaces (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL UNIQUE REFERENCES bid_projects(id) ON DELETE RESTRICT,
  scope_kind text NOT NULL DEFAULT 'project_wide' CHECK (scope_kind='project_wide'),
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(project_id,id)
);

CREATE TABLE bid_workspace_scope_revision_artifacts (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  revision bigint NOT NULL CHECK (revision > 0),
  scope_kind text NOT NULL CHECK (scope_kind='project_wide'),
  canonical_payload bytea NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(workspace_id,revision),
  UNIQUE(project_id,id),
  UNIQUE(project_id,workspace_id,id,content_sha256),
  FOREIGN KEY(project_id,workspace_id) REFERENCES bid_submission_workspaces(project_id,id),
  CHECK (content_sha256=kb_bid_v2_sha256_bytes(canonical_payload))
);

CREATE TABLE bid_workspace_requirement_projection_artifacts (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  requirement_set_id uuid NOT NULL,
  revision bigint NOT NULL CHECK (revision > 0),
  canonical_payload bytea NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(workspace_id,revision),
  UNIQUE(project_id,id),
  UNIQUE(project_id,workspace_id,id,content_sha256),
  FOREIGN KEY(project_id,workspace_id) REFERENCES bid_submission_workspaces(project_id,id),
  FOREIGN KEY(project_id,requirement_set_id) REFERENCES bid_requirement_set_artifacts(project_id,id),
  CHECK (content_sha256=kb_bid_v2_sha256_bytes(canonical_payload))
);

CREATE TABLE bid_workspace_requirement_projection_items (
  projection_id uuid NOT NULL,
  project_id uuid NOT NULL,
  requirement_revision_id uuid NOT NULL,
  effective_applicability jsonb NOT NULL DEFAULT '{}'::jsonb CHECK (kb_bid_v2_applicability_valid(effective_applicability)),
  ordinal integer NOT NULL CHECK (ordinal >= 0),
  PRIMARY KEY(projection_id,requirement_revision_id),
  UNIQUE(projection_id,ordinal),
  FOREIGN KEY(project_id,projection_id) REFERENCES bid_workspace_requirement_projection_artifacts(project_id,id),
  FOREIGN KEY(project_id,requirement_revision_id) REFERENCES bid_requirement_revision_artifacts(project_id,id)
);

CREATE TABLE bid_workspace_requirement_projection_current (
  scope_id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  artifact_id uuid NOT NULL,
  artifact_sha256 kb_sha256 NOT NULL,
  generation bigint NOT NULL CHECK (generation > 0),
  created_at timestamptz NOT NULL,
  FOREIGN KEY(project_id,scope_id) REFERENCES bid_submission_workspaces(project_id,id),
  FOREIGN KEY(project_id,artifact_id) REFERENCES bid_workspace_requirement_projection_artifacts(project_id,id)
);

CREATE TABLE bid_document_settings_revision_artifacts (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  revision bigint NOT NULL CHECK (revision > 0),
  schema_version smallint NOT NULL CHECK (schema_version=1),
  settings jsonb NOT NULL CHECK (jsonb_typeof(settings)='object' AND settings->>'page_size'='A4'),
  canonical_payload bytea NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  actor kb_actor_identity NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(workspace_id,revision),
  UNIQUE(project_id,id),
  UNIQUE(project_id,workspace_id,id,content_sha256),
  FOREIGN KEY(project_id,workspace_id) REFERENCES bid_submission_workspaces(project_id,id),
  CHECK (content_sha256=kb_bid_v2_sha256_bytes(canonical_payload))
);

CREATE TABLE bid_outline_node_lineages (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(project_id,id),
  FOREIGN KEY(project_id,workspace_id) REFERENCES bid_submission_workspaces(project_id,id)
);

CREATE TABLE bid_outline_node_revision_artifacts (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  lineage_id uuid NOT NULL,
  revision bigint NOT NULL CHECK (revision > 0),
  title text NOT NULL CHECK (octet_length(title) BETWEEN 1 AND 1024),
  semantic_role text NOT NULL CHECK (semantic_role IN ('cover','toc','qualification','technical','commercial','quotation','deviation','implementation','evidence_index','attachment','other')),
  render_role text NOT NULL CHECK (render_role IN ('section','front_matter','toc','appendix','hidden')),
  origin text NOT NULL CHECK (origin IN ('human','agent_candidate','deterministic')),
  tombstone boolean NOT NULL DEFAULT false,
  canonical_payload bytea NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(project_id,lineage_id,revision),
  UNIQUE(project_id,id),
  UNIQUE(project_id,workspace_id,id,lineage_id),
  FOREIGN KEY(project_id,workspace_id) REFERENCES bid_submission_workspaces(project_id,id),
  FOREIGN KEY(project_id,lineage_id) REFERENCES bid_outline_node_lineages(project_id,id),
  CHECK (content_sha256=kb_bid_v2_sha256_bytes(canonical_payload))
);

CREATE TABLE bid_content_block_lineages (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(project_id,id),
  FOREIGN KEY(project_id,workspace_id) REFERENCES bid_submission_workspaces(project_id,id)
);

CREATE TABLE bid_content_block_revision_artifacts (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  lineage_id uuid NOT NULL,
  revision bigint NOT NULL CHECK (revision > 0),
  schema_version smallint NOT NULL CHECK (schema_version=1),
  block_kind text NOT NULL CHECK (block_kind IN ('rich_text','table','image','attachment_ref','structured_form','page_break','signature_placeholder')),
  block_payload jsonb NOT NULL CHECK (jsonb_typeof(block_payload)='object'),
  origin text NOT NULL CHECK (origin IN ('human','agent_candidate','deterministic')),
  tombstone boolean NOT NULL DEFAULT false,
  canonical_payload bytea NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(project_id,lineage_id,revision),
  UNIQUE(project_id,id),
  UNIQUE(project_id,id,content_sha256),
  FOREIGN KEY(project_id,workspace_id) REFERENCES bid_submission_workspaces(project_id,id),
  FOREIGN KEY(project_id,lineage_id) REFERENCES bid_content_block_lineages(project_id,id),
  CHECK (content_sha256=kb_bid_v2_sha256_bytes(canonical_payload))
);

CREATE TABLE bid_outline_fulfillment_binding_lineages (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(project_id,id),
  FOREIGN KEY(project_id,workspace_id) REFERENCES bid_submission_workspaces(project_id,id)
);

CREATE TABLE bid_outline_fulfillment_binding_revision_artifacts (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  lineage_id uuid NOT NULL,
  revision bigint NOT NULL CHECK (revision > 0),
  need_occurrence_id uuid NOT NULL,
  requirement_projection_id uuid NOT NULL,
  channel text NOT NULL CHECK (channel IN ('narrative_content','response_table','deviation_statement','structured_form','evidence_attachment','quotation')),
  target_kind text NOT NULL CHECK (target_kind IN ('outline_node','content_block','response_table','structured_form','quote')),
  target_id uuid NOT NULL,
  target_node_id uuid,
  candidate_id uuid,
  state text NOT NULL CHECK (state IN ('bound','unbound','superseded')),
  CHECK ((target_kind='outline_node' AND target_node_id=target_id)
      OR (target_kind='content_block' AND target_node_id IS NOT NULL AND candidate_id IS NOT NULL)
      OR (target_kind NOT IN ('outline_node','content_block') AND target_node_id IS NULL AND candidate_id IS NULL)),
  reason text NOT NULL CHECK (octet_length(reason) BETWEEN 1 AND 4096),
  actor kb_actor_identity NOT NULL,
  canonical_payload bytea NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(project_id,lineage_id,revision),
  UNIQUE(project_id,id),
  FOREIGN KEY(project_id,workspace_id) REFERENCES bid_submission_workspaces(project_id,id),
  FOREIGN KEY(project_id,lineage_id) REFERENCES bid_outline_fulfillment_binding_lineages(project_id,id),
  FOREIGN KEY(project_id,requirement_projection_id) REFERENCES bid_workspace_requirement_projection_artifacts(project_id,id),
  CHECK (content_sha256=kb_bid_v2_sha256_bytes(canonical_payload))
);

CREATE TABLE bid_workspace_revision_artifacts (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  revision bigint NOT NULL CHECK (revision > 0),
  parent_revision_id uuid,
  parent_sha256 kb_sha256,
  scope_revision_id uuid NOT NULL,
  requirement_projection_id uuid NOT NULL,
  requirement_projection_sha256 kb_sha256 NOT NULL,
  document_settings_revision_id uuid NOT NULL,
  quote_snapshot_id uuid,
  quote_snapshot_sha256 kb_sha256,
  canonical_payload bytea NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  actor kb_actor_identity NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(workspace_id,revision),
  UNIQUE(project_id,id),
  UNIQUE(project_id,id,content_sha256),
  UNIQUE(project_id,workspace_id,id,content_sha256),
  UNIQUE(project_id,workspace_id,id,requirement_projection_id,requirement_projection_sha256),
  UNIQUE(project_id,workspace_id,id,scope_revision_id,requirement_projection_id,document_settings_revision_id),
  UNIQUE(project_id,workspace_id,id,scope_revision_id,requirement_projection_id,document_settings_revision_id,quote_snapshot_id,quote_snapshot_sha256),
  FOREIGN KEY(project_id,workspace_id) REFERENCES bid_submission_workspaces(project_id,id),
  FOREIGN KEY(project_id,parent_revision_id,parent_sha256) REFERENCES bid_workspace_revision_artifacts(project_id,id,content_sha256),
  FOREIGN KEY(project_id,scope_revision_id) REFERENCES bid_workspace_scope_revision_artifacts(project_id,id),
  FOREIGN KEY(project_id,workspace_id,requirement_projection_id,requirement_projection_sha256)
    REFERENCES bid_workspace_requirement_projection_artifacts(project_id,workspace_id,id,content_sha256),
  FOREIGN KEY(project_id,document_settings_revision_id) REFERENCES bid_document_settings_revision_artifacts(project_id,id),
  CHECK ((parent_revision_id IS NULL)=(parent_sha256 IS NULL)),
  CHECK ((quote_snapshot_id IS NULL)=(quote_snapshot_sha256 IS NULL)),
  CHECK (content_sha256=kb_bid_v2_sha256_bytes(canonical_payload))
);

CREATE TABLE bid_workspace_node_occurrences (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  workspace_revision_id uuid NOT NULL,
  node_revision_id uuid NOT NULL,
  parent_occurrence_id uuid,
  ordinal integer NOT NULL CHECK (ordinal >= 0),
  depth integer NOT NULL CHECK (depth BETWEEN 0 AND 32),
  UNIQUE(workspace_revision_id,node_revision_id),
  UNIQUE(workspace_revision_id,parent_occurrence_id,ordinal),
  UNIQUE(workspace_revision_id,id),
  UNIQUE(project_id,workspace_revision_id,node_revision_id),
  UNIQUE(project_id,workspace_revision_id,id,node_revision_id,ordinal),
  FOREIGN KEY(project_id,workspace_revision_id) REFERENCES bid_workspace_revision_artifacts(project_id,id),
  FOREIGN KEY(project_id,node_revision_id) REFERENCES bid_outline_node_revision_artifacts(project_id,id),
  FOREIGN KEY(workspace_revision_id,parent_occurrence_id)
    REFERENCES bid_workspace_node_occurrences(workspace_revision_id,id)
);

CREATE TABLE bid_workspace_block_occurrences (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  workspace_revision_id uuid NOT NULL,
  node_occurrence_id uuid NOT NULL REFERENCES bid_workspace_node_occurrences(id),
  block_revision_id uuid NOT NULL,
  ordinal integer NOT NULL CHECK (ordinal >= 0),
  UNIQUE(workspace_revision_id,block_revision_id),
  UNIQUE(node_occurrence_id,ordinal),
  UNIQUE(project_id,workspace_revision_id,id,node_occurrence_id,block_revision_id,ordinal),
  FOREIGN KEY(project_id,workspace_revision_id) REFERENCES bid_workspace_revision_artifacts(project_id,id),
  FOREIGN KEY(workspace_revision_id,node_occurrence_id)
    REFERENCES bid_workspace_node_occurrences(workspace_revision_id,id),
  FOREIGN KEY(project_id,block_revision_id) REFERENCES bid_content_block_revision_artifacts(project_id,id)
);

CREATE TABLE bid_workspace_binding_occurrences (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  workspace_revision_id uuid NOT NULL,
  binding_revision_id uuid NOT NULL,
  ordinal integer NOT NULL CHECK (ordinal >= 0),
  UNIQUE(workspace_revision_id,binding_revision_id),
  UNIQUE(workspace_revision_id,ordinal),
  FOREIGN KEY(project_id,workspace_revision_id) REFERENCES bid_workspace_revision_artifacts(project_id,id),
  FOREIGN KEY(project_id,binding_revision_id) REFERENCES bid_outline_fulfillment_binding_revision_artifacts(project_id,id)
);

CREATE TABLE bid_workspace_heads (
  scope_id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  artifact_id uuid NOT NULL,
  artifact_sha256 kb_sha256 NOT NULL,
  generation bigint NOT NULL CHECK (generation > 0),
  created_at timestamptz NOT NULL,
  FOREIGN KEY(project_id,scope_id) REFERENCES bid_submission_workspaces(project_id,id),
  FOREIGN KEY(project_id,artifact_id,artifact_sha256) REFERENCES bid_workspace_revision_artifacts(project_id,id,content_sha256)
);

CREATE TABLE bid_outline_lineage_edges (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES bid_projects(id),
  workspace_id uuid NOT NULL,
  operation text NOT NULL CHECK (operation IN ('split','merge')),
  from_lineage_id uuid NOT NULL,
  to_lineage_id uuid NOT NULL,
  workspace_revision_id uuid NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(workspace_revision_id,from_lineage_id,to_lineage_id),
  FOREIGN KEY(project_id,workspace_id) REFERENCES bid_submission_workspaces(project_id,id),
  FOREIGN KEY(project_id,workspace_revision_id) REFERENCES bid_workspace_revision_artifacts(project_id,id)
);

CREATE TABLE bid_outline_checkpoint_artifacts (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  workspace_revision_id uuid NOT NULL,
  requirement_projection_id uuid NOT NULL,
  requirement_projection_sha256 kb_sha256 NOT NULL,
  canonical_payload bytea NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  actor kb_actor_identity NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(project_id,id),
  UNIQUE(project_id,workspace_id,id,workspace_revision_id,requirement_projection_id,requirement_projection_sha256,content_sha256),
  FOREIGN KEY(project_id,workspace_id) REFERENCES bid_submission_workspaces(project_id,id),
  FOREIGN KEY(project_id,workspace_id,workspace_revision_id,requirement_projection_id,requirement_projection_sha256)
    REFERENCES bid_workspace_revision_artifacts(project_id,workspace_id,id,requirement_projection_id,requirement_projection_sha256),
  FOREIGN KEY(project_id,workspace_id,requirement_projection_id,requirement_projection_sha256)
    REFERENCES bid_workspace_requirement_projection_artifacts(project_id,workspace_id,id,content_sha256),
  CHECK (content_sha256=kb_bid_v2_sha256_bytes(canonical_payload))
);

CREATE TABLE bid_async_request_snapshot_artifacts (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES bid_projects(id),
  workspace_id uuid,
  request_kind text NOT NULL CHECK (request_kind IN ('tender_document_process','requirement_set_compile','content_generate','submission_export','docx_compose','docx_layout')),
  revision bigint NOT NULL CHECK (revision > 0),
  frozen_input_sha256 kb_sha256 NOT NULL,
  request_payload bytea NOT NULL,
  request_sha256 kb_sha256 NOT NULL,
  status text NOT NULL CHECK (status IN ('pending','succeeded','failed')),
  max_run_attempts integer NOT NULL DEFAULT 4 CHECK (max_run_attempts=4),
  current_attempt integer NOT NULL DEFAULT 0 CHECK (current_attempt BETWEEN 0 AND max_run_attempts),
  result_identity jsonb,
  error_code text CHECK (error_code IS NULL OR error_code IN (
    'INPUT_SCHEMA_INVALID','FROZEN_INPUT_MISSING','FROZEN_INPUT_DIGEST_MISMATCH',
    'WORKSPACE_CAS_CONFLICT','STRUCTURE_EVIDENCE_INSUFFICIENT','AGENT_OUTPUT_INVALID','REQUIREMENT_COMPILE_FAILED','EVIDENCE_UNAVAILABLE','ASSET_MISSING',
    'ASSET_DIGEST_MISMATCH','ATTACHMENT_PREPARATION_FAILED','RENDER_SCHEMA_INVALID','RENDERER_FAILED','OBJECT_COMMIT_FAILED',
    'AGENT_MAP_FAILED','AGENT_GROUPING_FAILED','AGENT_SEMANTIC_VALIDATION_FAILED',
    'AGENT_REQUIREMENT_CLOSURE_FAILED','AGENT_OBLIGATION_COVERAGE_FAILED',
    'AGENT_TURN_TIMEOUT','AGENT_TURN_BUDGET_EXCEEDED','REQUEST_ATTEMPT_BUDGET_EXCEEDED','AGENT_TOOL_BUDGET_EXCEEDED',
    'CONTENT_RETRIEVAL_INVALID_REQUEST','CONTENT_RETRIEVAL_UNAVAILABLE','CONTENT_RETRIEVAL_QUOTA_EXCEEDED',
    'CONTENT_RETRIEVAL_INVALID_HIT','CONTENT_RETRIEVAL_POLICY_REVOKED','CONTENT_RETRIEVAL_DIGEST_MISMATCH',
    'CONTENT_DIVERGENT_AGENT_INPUT_REPLAY','CONTENT_MATCH_TIMEOUT',
    'TENDER_DOCUMENT_PROCESS_TIMEOUT','REQUIREMENT_COMPILE_TIMEOUT','SUBMISSION_EXPORT_TIMEOUT',
    'AGENT_TEXT_BUDGET_EXCEEDED','AGENT_IMAGE_BUDGET_EXCEEDED','AGENT_DEADLINE_EXCEEDED','AGENT_PROVIDER_ERROR',
    'AGENT_PROVIDER_UNAVAILABLE'
  )),
  created_at timestamptz NOT NULL DEFAULT now(),
  finished_at timestamptz,
  UNIQUE(request_kind,id,revision),
  UNIQUE(id,frozen_input_sha256),
  UNIQUE(id,project_id,workspace_id,request_kind,revision,request_sha256),
  UNIQUE(id,project_id,workspace_id,request_kind,revision,request_sha256,frozen_input_sha256),
  CHECK (request_sha256=kb_bid_v2_sha256_bytes(request_payload)),
  CHECK ((status='pending' AND finished_at IS NULL AND error_code IS NULL)
      OR (status='succeeded' AND finished_at IS NOT NULL AND error_code IS NULL)
      OR (status='failed' AND finished_at IS NOT NULL AND error_code IS NOT NULL))
);

CREATE TABLE bid_async_stage_receipts (
  request_artifact_id uuid NOT NULL REFERENCES bid_async_request_snapshot_artifacts(id),
  stage_kind text NOT NULL CHECK (stage_kind IN ('conversion','extraction','requirement_compile','evidence_match','agent_generate','assessment','attachment_prepare','render_snapshot','manifest','render','object_commit','package')),
  frozen_input_sha256 kb_sha256 NOT NULL,
  result_identity jsonb NOT NULL CHECK (jsonb_typeof(result_identity)='object'),
  result_sha256 kb_sha256 NOT NULL,
  completed_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY(request_artifact_id,stage_kind,frozen_input_sha256)
);

CREATE TABLE bid_candidate_artifacts (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  candidate_kind text NOT NULL CHECK (candidate_kind='content'),
  base_workspace_revision_id uuid NOT NULL,
  base_workspace_sha256 kb_sha256 NOT NULL,
  request_artifact_id uuid NOT NULL,
  request_kind text NOT NULL CHECK (request_kind IN ('content_generate')),
  request_revision bigint NOT NULL CHECK (request_revision > 0),
  request_sha256 kb_sha256 NOT NULL,
  request_operation text NOT NULL CHECK (request_operation IN ('generate')),
  state text NOT NULL CHECK (state IN ('proposed','accepted','rejected')),
  canonical_payload bytea NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  canonical_payload_sha256 kb_sha256 GENERATED ALWAYS AS (kb_bid_v2_sha256_bytes(canonical_payload)) STORED,
  created_at timestamptz NOT NULL DEFAULT now(),
  decided_at timestamptz,
  UNIQUE(project_id,id),
  FOREIGN KEY(project_id,workspace_id) REFERENCES bid_submission_workspaces(project_id,id),
  FOREIGN KEY(request_artifact_id,project_id,workspace_id,request_kind,request_revision,request_sha256)
    REFERENCES bid_async_request_snapshot_artifacts(id,project_id,workspace_id,request_kind,revision,request_sha256),
  FOREIGN KEY(project_id,workspace_id,base_workspace_revision_id,base_workspace_sha256)
    REFERENCES bid_workspace_revision_artifacts(project_id,workspace_id,id,content_sha256),
  CHECK (CASE
      WHEN candidate_kind='content' THEN request_kind='content_generate' AND request_operation='generate'
      ELSE false END),
  CHECK (content_sha256=kb_bid_v2_sha256_bytes(canonical_payload)),
  CHECK ((state='proposed')=(decided_at IS NULL))
);

ALTER TABLE bid_outline_fulfillment_binding_revision_artifacts
  ADD FOREIGN KEY(candidate_id) REFERENCES bid_candidate_artifacts(id);


CREATE TABLE bid_outline_candidate_block_targets_v1 (
  candidate_id uuid NOT NULL REFERENCES bid_candidate_artifacts(id),
  block_ref uuid NOT NULL,
  owner_node_ref uuid NOT NULL,
  block_ordinal integer NOT NULL CHECK (block_ordinal>=0),
  PRIMARY KEY(candidate_id,block_ref),
  UNIQUE(candidate_id,owner_node_ref,block_ordinal)
);


CREATE TABLE bid_candidate_operations (
  candidate_id uuid NOT NULL REFERENCES bid_candidate_artifacts(id),
  ordinal integer NOT NULL CHECK (ordinal >= 0),
  operation jsonb NOT NULL CHECK (jsonb_typeof(operation)='object'),
  operation_sha256 kb_sha256 NOT NULL,
  PRIMARY KEY(candidate_id,ordinal),
  CHECK (operation_sha256=kb_bid_v2_sha256_bytes(convert_to(operation::text,'UTF8')))
);

CREATE TABLE bid_candidate_decision_receipts (
  candidate_id uuid PRIMARY KEY REFERENCES bid_candidate_artifacts(id),
  actor kb_actor_identity NOT NULL,
  accepted_operation_ordinals integer[] NOT NULL,
  accepted_operations jsonb NOT NULL CHECK (jsonb_typeof(accepted_operations)='array'),
  resulting_workspace_revision_id uuid,
  response_payload bytea NOT NULL,
  response_sha256 kb_sha256 NOT NULL,
  decided_at timestamptz NOT NULL DEFAULT now(),
  CHECK(response_sha256=kb_bid_v2_sha256_bytes(response_payload))
);

-- Bidding owns only its source-image bindings to the Shared ObjectRegistry.
ALTER TABLE bid_tender_source_image_revision_artifacts
  ADD FOREIGN KEY(original_object_ref,original_sha256,original_media_type,original_object_state)
    REFERENCES object_registry(object_ref,digest,media_type,state),
  ADD FOREIGN KEY(ocr_object_ref,ocr_sha256,ocr_media_type,ocr_byte_length,ocr_object_state)
    REFERENCES object_registry(object_ref,digest,media_type,byte_length,state);

CREATE TABLE bid_evidence_match_reports (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  requirement_revision_id uuid NOT NULL,
  node_lineage_id uuid,
  retrieval_contract_version text NOT NULL CHECK (octet_length(retrieval_contract_version) BETWEEN 1 AND 128),
  knowledge_scope_attestation_id uuid NOT NULL,
  knowledge_scope_attestation_sha256 kb_sha256 NOT NULL,
  canonical_payload bytea NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(project_id,id),
  UNIQUE(project_id,workspace_id,id),
  UNIQUE(project_id,workspace_id,requirement_revision_id,id),
  FOREIGN KEY(project_id,workspace_id) REFERENCES bid_submission_workspaces(project_id,id),
  FOREIGN KEY(project_id,requirement_revision_id) REFERENCES bid_requirement_revision_artifacts(project_id,id),
  FOREIGN KEY(knowledge_scope_attestation_id,knowledge_scope_attestation_sha256)
    REFERENCES knowledge_matching_scope_attestations_v2(id,content_sha256),
  CHECK (content_sha256=kb_bid_v2_sha256_bytes(canonical_payload))
);

CREATE TABLE bid_evidence_bundle_artifacts (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  requirement_revision_id uuid NOT NULL,
  matching_report_id uuid NOT NULL,
  canonical_payload jsonb NOT NULL CHECK (jsonb_typeof(canonical_payload)='object'),
  content_sha256 kb_sha256 NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(project_id,id),
  UNIQUE(project_id,workspace_id,id),
  UNIQUE(project_id,workspace_id,id,content_sha256),
  FOREIGN KEY(project_id,workspace_id) REFERENCES bid_submission_workspaces(project_id,id),
  FOREIGN KEY(project_id,requirement_revision_id) REFERENCES bid_requirement_revision_artifacts(project_id,id),
  FOREIGN KEY(project_id,workspace_id,requirement_revision_id,matching_report_id)
    REFERENCES bid_evidence_match_reports(project_id,workspace_id,requirement_revision_id,id),
  CHECK (canonical_payload->>'bundle_sha256'=content_sha256),
  CHECK (content_sha256=kb_bid_v2_sha256_bytes(convert_to((canonical_payload-'bundle_sha256')::text,'UTF8')))
);

CREATE TABLE bid_evidence_bundle_items (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  evidence_bundle_id uuid NOT NULL,
  ordinal integer NOT NULL CHECK (ordinal>=0),
  item_kind text NOT NULL CHECK (item_kind IN ('text_quote','image','no_evidence')),
  source_media_revision_id uuid,
  item_payload jsonb NOT NULL CHECK (jsonb_typeof(item_payload)='object'),
  content_sha256 kb_sha256 NOT NULL,
  UNIQUE(evidence_bundle_id,id),
  UNIQUE(evidence_bundle_id,id,content_sha256),
  UNIQUE(evidence_bundle_id,ordinal),
  UNIQUE(project_id,workspace_id,evidence_bundle_id,id,source_media_revision_id),
  FOREIGN KEY(project_id,workspace_id,evidence_bundle_id)
    REFERENCES bid_evidence_bundle_artifacts(project_id,workspace_id,id),
  CHECK ((item_kind='image')=(source_media_revision_id IS NOT NULL)),
  CHECK (CASE WHEN kb_bid_v2_uuid_text(item_payload->>'evidence_item_id')
              THEN (item_payload->>'evidence_item_id')::uuid=id ELSE false END),
  CHECK (jsonb_typeof(item_payload->'kind') IS NOT DISTINCT FROM 'string'
         AND item_payload->>'kind' IS NOT DISTINCT FROM item_kind),
  CHECK (content_sha256=kb_bid_v2_sha256_bytes(convert_to(item_payload::text,'UTF8')))
);

CREATE TABLE bid_evidence_selection_artifacts (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  selection_kind text NOT NULL CHECK (selection_kind IN ('user_pick_set','system_proposed','accepted')),
  matching_report_id uuid NOT NULL,
  canonical_payload bytea NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  actor kb_actor_identity,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(project_id,id),
  UNIQUE(project_id,workspace_id,id,content_sha256,selection_kind),
  UNIQUE(project_id,workspace_id,id,content_sha256,selection_kind,matching_report_id),
  FOREIGN KEY(project_id,workspace_id) REFERENCES bid_submission_workspaces(project_id,id),
  FOREIGN KEY(project_id,matching_report_id) REFERENCES bid_evidence_match_reports(project_id,id),
  FOREIGN KEY(project_id,workspace_id,matching_report_id) REFERENCES bid_evidence_match_reports(project_id,workspace_id,id),
  CHECK (content_sha256=kb_bid_v2_sha256_bytes(canonical_payload))
);

CREATE TABLE bid_evidence_asset_artifacts (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  evidence_bundle_id uuid NOT NULL,
  evidence_item_id uuid NOT NULL,
  image_artifact_revision_id uuid NOT NULL,
  object_ref kb_object_ref NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  object_state text NOT NULL DEFAULT 'available' CHECK (object_state='available'),
  media_type text NOT NULL CHECK (media_type IN ('image/png','image/jpeg','image/webp')),
  width integer NOT NULL CHECK (width > 0),
  height integer NOT NULL CHECK (height > 0),
  page_ordinal integer CHECK (page_ordinal >= 0),
  bounding_region jsonb CHECK (bounding_region IS NULL OR jsonb_typeof(bounding_region)='object'),
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(project_id,id),
  UNIQUE(workspace_id,evidence_bundle_id,evidence_item_id),
  FOREIGN KEY(project_id,workspace_id) REFERENCES bid_submission_workspaces(project_id,id),
  FOREIGN KEY(project_id,workspace_id,evidence_bundle_id,evidence_item_id,image_artifact_revision_id)
    REFERENCES bid_evidence_bundle_items(project_id,workspace_id,evidence_bundle_id,id,source_media_revision_id),
  FOREIGN KEY(image_artifact_revision_id,object_ref,content_sha256,media_type,object_state)
    REFERENCES knowledge_image_artifact_revisions(id,object_ref,content_sha256,media_type,object_state),
  FOREIGN KEY(object_ref,content_sha256,media_type,object_state)
    REFERENCES object_registry(object_ref,digest,media_type,state),
  UNIQUE(id,workspace_id,object_ref,content_sha256,media_type,object_state),
  CHECK (object_ref='objects/'||content_sha256)
);

CREATE FUNCTION kb_bid_v2_validate_evidence_asset_media_identity()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
BEGIN
  IF NOT EXISTS (
    SELECT 1 FROM knowledge_image_artifact_revisions media
    WHERE media.id=NEW.image_artifact_revision_id
      AND media.object_ref=NEW.object_ref AND media.content_sha256=NEW.content_sha256
      AND media.media_type=NEW.media_type AND media.object_state=NEW.object_state
      AND media.width=NEW.width AND media.height=NEW.height
      AND media.page_ordinal IS NOT DISTINCT FROM NEW.page_ordinal
      AND media.bounding_region IS NOT DISTINCT FROM NEW.bounding_region
  ) THEN
    RAISE EXCEPTION 'EvidenceAsset knowledge media qualified identity mismatch' USING ERRCODE='23503';
  END IF;
  RETURN NEW;
END $$;
CREATE TRIGGER bid_evidence_asset_media_identity_valid
BEFORE INSERT OR UPDATE ON bid_evidence_asset_artifacts
FOR EACH ROW EXECUTE FUNCTION kb_bid_v2_validate_evidence_asset_media_identity();

-- EvidenceBundleV1 publication is validated in PostgreSQL against the same closed
-- contract as the checked-in Draft 2020-12 schema. The canonical digest excludes
-- only bundle_sha256, avoiding an impossible self-referential hash.
CREATE FUNCTION kb_bid_v2_validate_evidence_bundle_payload()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE p jsonb:=NEW.canonical_payload; item jsonb; bounds jsonb;
BEGIN
  IF NOT kb_bid_v2_json_keys_exact(p,ARRAY['schema_version','evidence_bundle_id','project_id','workspace_id','workspace_scope','requirement_revision_id','matching_report_id','knowledge_scope_attestation_id','knowledge_scope_attestation_sha256','items','created_at','bundle_sha256'])
     OR p->'schema_version' IS DISTINCT FROM '1'::jsonb OR p->>'workspace_scope' IS DISTINCT FROM 'project_wide'
     OR NOT kb_bid_v2_uuid_text(p->>'evidence_bundle_id') OR NOT kb_bid_v2_uuid_text(p->>'project_id')
     OR NOT kb_bid_v2_uuid_text(p->>'workspace_id') OR NOT kb_bid_v2_uuid_text(p->>'requirement_revision_id')
     OR NOT kb_bid_v2_uuid_text(p->>'matching_report_id') OR NOT kb_bid_v2_uuid_text(p->>'knowledge_scope_attestation_id')
     OR jsonb_typeof(p->'knowledge_scope_attestation_sha256') IS DISTINCT FROM 'string' OR NOT kb_bid_v2_sha256_text(p->>'knowledge_scope_attestation_sha256')
     OR jsonb_typeof(p->'bundle_sha256') IS DISTINCT FROM 'string' OR NOT kb_bid_v2_sha256_text(p->>'bundle_sha256') OR jsonb_typeof(p->'created_at') IS DISTINCT FROM 'string'
     OR jsonb_typeof(p->'items') IS DISTINCT FROM 'array' OR jsonb_array_length(p->'items') NOT BETWEEN 1 AND 100000
     OR (p->>'evidence_bundle_id')::uuid IS DISTINCT FROM NEW.id OR (p->>'project_id')::uuid IS DISTINCT FROM NEW.project_id
     OR (p->>'workspace_id')::uuid IS DISTINCT FROM NEW.workspace_id OR (p->>'requirement_revision_id')::uuid IS DISTINCT FROM NEW.requirement_revision_id
     OR (p->>'matching_report_id')::uuid IS DISTINCT FROM NEW.matching_report_id
     OR NOT kb_bid_v2_rfc3339_datetime_matches(p->>'created_at',NEW.created_at)
     OR p->>'bundle_sha256' IS DISTINCT FROM NEW.content_sha256
     OR NEW.content_sha256<>kb_bid_v2_sha256_bytes(convert_to((p-'bundle_sha256')::text,'UTF8'))
     OR NOT EXISTS (SELECT 1 FROM bid_evidence_match_reports report
        WHERE report.project_id=NEW.project_id AND report.workspace_id=NEW.workspace_id
          AND report.requirement_revision_id=NEW.requirement_revision_id AND report.id=NEW.matching_report_id
          AND report.knowledge_scope_attestation_id=(p->>'knowledge_scope_attestation_id')::uuid
          AND report.knowledge_scope_attestation_sha256=p->>'knowledge_scope_attestation_sha256')
  THEN RAISE EXCEPTION 'EvidenceBundleV1 root contract invalid' USING ERRCODE='23514'; END IF;
  IF EXISTS (SELECT 1 FROM jsonb_array_elements(p->'items') x GROUP BY x->>'evidence_item_id' HAVING count(*)<>1)
  THEN RAISE EXCEPTION 'EvidenceBundleV1 duplicate evidence_item_id' USING ERRCODE='23514'; END IF;
  FOR item IN SELECT value FROM jsonb_array_elements(p->'items') LOOP
    IF jsonb_typeof(item) IS DISTINCT FROM 'object' OR NOT kb_bid_v2_uuid_text(item->>'evidence_item_id') THEN
      RAISE EXCEPTION 'EvidenceBundleV1 malformed item identity' USING ERRCODE='23514';
    END IF;
    IF item->>'kind' IS NOT DISTINCT FROM 'text_quote' THEN
      PERFORM kb_knowledge_verify_attested_text_hit_v2(
        (p->>'knowledge_scope_attestation_id')::uuid,p->>'knowledge_scope_attestation_sha256',
        NEW.requirement_revision_id,item);
      IF NOT kb_bid_v2_json_keys_exact(item,ARRAY['kind','evidence_item_id','document_id','source_chunk_id','product_version_id','workspace_kind','frozen_document_display_name','quote_utf8','quote_sha256','quote_start_offset','quote_end_offset','retrieval_rank','retrieval_contract_version'])
         OR NOT kb_bid_v2_uuid_text(item->>'document_id') OR NOT kb_bid_v2_uuid_text(item->>'source_chunk_id')
         OR NOT kb_bid_v2_uuid_text(item->>'product_version_id') OR jsonb_typeof(item->'workspace_kind') IS DISTINCT FROM 'string' OR COALESCE(item->>'workspace_kind','') NOT IN ('product_line','company')
         OR jsonb_typeof(item->'frozen_document_display_name') IS DISTINCT FROM 'string' OR octet_length(item->>'frozen_document_display_name') NOT BETWEEN 1 AND 1024
         OR jsonb_typeof(item->'quote_utf8') IS DISTINCT FROM 'string' OR octet_length(item->>'quote_utf8') NOT BETWEEN 1 AND 1048576
         OR jsonb_typeof(item->'quote_sha256') IS DISTINCT FROM 'string' OR NOT kb_bid_v2_sha256_text(item->>'quote_sha256')
         OR item->>'quote_sha256' IS DISTINCT FROM kb_bid_v2_sha256_bytes(convert_to(item->>'quote_utf8','UTF8'))
         OR COALESCE(item->>'quote_start_offset','') !~ '^(0|[1-9][0-9]*)$'
         OR jsonb_typeof(item->'quote_start_offset') IS DISTINCT FROM 'number' OR jsonb_typeof(item->'quote_end_offset') IS DISTINCT FROM 'number' OR jsonb_typeof(item->'retrieval_rank') IS DISTINCT FROM 'number'
         OR item->>'quote_end_offset' !~ '^[1-9][0-9]*$' OR item->>'retrieval_rank' !~ '^[1-9][0-9]*$'
         OR (item->>'quote_end_offset')::bigint <= (item->>'quote_start_offset')::bigint
         OR jsonb_typeof(item->'retrieval_contract_version') IS DISTINCT FROM 'string' OR octet_length(item->>'retrieval_contract_version') NOT BETWEEN 1 AND 128
      THEN RAISE EXCEPTION 'EvidenceBundleV1 text item invalid' USING ERRCODE='23514'; END IF;
    ELSIF item->>'kind' IS NOT DISTINCT FROM 'image' THEN
      PERFORM kb_knowledge_verify_attested_image_hit_v3(
        (p->>'knowledge_scope_attestation_id')::uuid,p->>'knowledge_scope_attestation_sha256',
        NEW.requirement_revision_id,item);
      IF NOT kb_bid_v2_json_keys_exact(item,ARRAY['kind','evidence_item_id','document_id','source_chunk_id','product_version_id','workspace_kind','frozen_document_display_name','quote_utf8','quote_sha256','quote_start_offset','quote_end_offset','retrieval_rank','retrieval_contract_version','image_artifact_revision_id','object_ref','sha256','media_type','width','height','page_ordinal','bounding_region'])
      THEN RAISE EXCEPTION 'EvidenceBundleV1 image item keys invalid' USING ERRCODE='23514'; END IF;
      IF NOT kb_bid_v2_uuid_text(item->>'document_id') OR NOT kb_bid_v2_uuid_text(item->>'source_chunk_id')
         OR NOT kb_bid_v2_uuid_text(item->>'product_version_id') OR COALESCE(item->>'workspace_kind','') NOT IN ('product_line','company')
         OR jsonb_typeof(item->'quote_utf8') IS DISTINCT FROM 'string' OR octet_length(item->>'quote_utf8') NOT BETWEEN 1 AND 1048576
         OR NOT kb_bid_v2_sha256_text(item->>'quote_sha256') OR item->>'quote_sha256' IS DISTINCT FROM kb_bid_v2_sha256_bytes(convert_to(item->>'quote_utf8','UTF8'))
         OR COALESCE(item->>'quote_start_offset','')!~'^(0|[1-9][0-9]*)$' OR COALESCE(item->>'quote_end_offset','')!~'^[1-9][0-9]*$'
         OR COALESCE(item->>'retrieval_rank','')!~'^[1-9][0-9]*$' OR (item->>'quote_end_offset')::bigint<=(item->>'quote_start_offset')::bigint
         OR jsonb_typeof(item->'retrieval_contract_version') IS DISTINCT FROM 'string'
         OR NOT kb_bid_v2_uuid_text(item->>'image_artifact_revision_id') OR item->>'object_ref' IS DISTINCT FROM ('objects/'||(item->>'sha256'))
         OR jsonb_typeof(item->'sha256') IS DISTINCT FROM 'string' OR NOT kb_bid_v2_sha256_text(item->>'sha256')
         OR jsonb_typeof(item->'media_type') IS DISTINCT FROM 'string' OR COALESCE(item->>'media_type','') NOT IN ('image/png','image/jpeg','image/webp')
         OR jsonb_typeof(item->'width') IS DISTINCT FROM 'number' OR jsonb_typeof(item->'height') IS DISTINCT FROM 'number'
         OR COALESCE(item->>'width','') !~ '^[1-9][0-9]*$' OR COALESCE(item->>'height','') !~ '^[1-9][0-9]*$'
         OR jsonb_typeof(item->'frozen_document_display_name') IS DISTINCT FROM 'string' OR octet_length(item->>'frozen_document_display_name') NOT BETWEEN 1 AND 1024
         OR (jsonb_typeof(item->'page_ordinal') IS DISTINCT FROM 'null'
             AND (jsonb_typeof(item->'page_ordinal') IS DISTINCT FROM 'number'
                  OR COALESCE(item->>'page_ordinal','') !~ '^(0|[1-9][0-9]*)$'))
      THEN RAISE EXCEPTION 'EvidenceBundleV1 image item invalid' USING ERRCODE='23514'; END IF;
      IF item ? 'bounding_region' AND jsonb_typeof(item->'bounding_region') IS DISTINCT FROM 'null' THEN
        bounds:=item->'bounding_region';
        IF NOT kb_bid_v2_json_keys_exact(bounds,ARRAY['left','top','right','bottom'])
           OR EXISTS (SELECT 1 FROM jsonb_each(bounds) v WHERE jsonb_typeof(v.value) IS DISTINCT FROM 'number' OR (v.value#>>'{}')::numeric NOT BETWEEN 0 AND 1)
           OR (bounds->>'left')::numeric>(bounds->>'right')::numeric OR (bounds->>'top')::numeric>(bounds->>'bottom')::numeric
        THEN RAISE EXCEPTION 'EvidenceBundleV1 bounding region invalid' USING ERRCODE='23514'; END IF;
      END IF;
    ELSIF item->>'kind' IS NOT DISTINCT FROM 'no_evidence' THEN
      IF NOT kb_bid_v2_json_keys_exact(item,ARRAY['kind','evidence_item_id','reason_code'])
         OR jsonb_typeof(item->'reason_code') IS DISTINCT FROM 'string'
         OR COALESCE(item->>'reason_code','') NOT IN ('NO_ELIGIBLE_VERSION','NO_MATCHING_HIT','MATCH_TRUNCATED','SOURCE_UNAVAILABLE')
      THEN RAISE EXCEPTION 'EvidenceBundleV1 no-evidence item invalid' USING ERRCODE='23514'; END IF;
    ELSE RAISE EXCEPTION 'EvidenceBundleV1 item kind invalid' USING ERRCODE='23514'; END IF;
  END LOOP;
  RETURN NEW;
END $$;
CREATE TRIGGER bid_evidence_bundle_payload_valid BEFORE INSERT OR UPDATE ON bid_evidence_bundle_artifacts
FOR EACH ROW EXECUTE FUNCTION kb_bid_v2_validate_evidence_bundle_payload();

CREATE FUNCTION kb_bid_v2_verify_evidence_bundle_projection()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE bundle_id uuid; p jsonb; actual jsonb;
BEGIN
 IF TG_TABLE_NAME='bid_evidence_bundle_artifacts' THEN bundle_id:=NEW.id; ELSE bundle_id:=NEW.evidence_bundle_id; END IF;
 SELECT canonical_payload INTO p FROM bid_evidence_bundle_artifacts WHERE id=bundle_id;
 IF p IS NULL THEN RETURN NULL; END IF;
 SELECT COALESCE(jsonb_agg(item_payload ORDER BY ordinal),'[]'::jsonb) INTO actual FROM bid_evidence_bundle_items WHERE evidence_bundle_id=bundle_id;
 IF actual<>p->'items' THEN RAISE EXCEPTION 'EvidenceBundleV1 item projection mismatch' USING ERRCODE='23514'; END IF;
 IF EXISTS (
   SELECT 1 FROM jsonb_array_elements(p->'items') item
   WHERE item->>'kind'='image' AND NOT EXISTS (
     SELECT 1 FROM bid_evidence_asset_artifacts asset
     JOIN object_registry registry ON registry.object_ref=asset.object_ref AND registry.digest=asset.content_sha256
       AND registry.media_type=asset.media_type AND registry.state=asset.object_state
     WHERE asset.evidence_bundle_id=bundle_id AND asset.evidence_item_id=(item->>'evidence_item_id')::uuid
       AND asset.image_artifact_revision_id=(item->>'image_artifact_revision_id')::uuid
       AND asset.object_ref=item->>'object_ref' AND asset.content_sha256=item->>'sha256'
       AND asset.media_type=item->>'media_type' AND asset.width=(item->>'width')::integer AND asset.height=(item->>'height')::integer
       AND asset.page_ordinal IS NOT DISTINCT FROM CASE WHEN item ? 'page_ordinal' AND jsonb_typeof(item->'page_ordinal')<>'null' THEN (item->>'page_ordinal')::integer ELSE NULL END
       AND asset.bounding_region IS NOT DISTINCT FROM CASE WHEN item ? 'bounding_region' AND jsonb_typeof(item->'bounding_region')<>'null' THEN item->'bounding_region' ELSE NULL END)
 ) OR EXISTS (
   SELECT 1 FROM bid_evidence_asset_artifacts asset WHERE asset.evidence_bundle_id=bundle_id
     AND NOT EXISTS (SELECT 1 FROM jsonb_array_elements(p->'items') item
       WHERE item->>'kind'='image' AND (item->>'evidence_item_id')::uuid=asset.evidence_item_id)
 ) THEN RAISE EXCEPTION 'EvidenceBundleV1 media projection mismatch' USING ERRCODE='23514'; END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER bid_evidence_bundle_projection_complete AFTER INSERT ON bid_evidence_bundle_artifacts
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION kb_bid_v2_verify_evidence_bundle_projection();
CREATE CONSTRAINT TRIGGER bid_evidence_bundle_item_projection_complete AFTER INSERT ON bid_evidence_bundle_items
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION kb_bid_v2_verify_evidence_bundle_projection();
CREATE CONSTRAINT TRIGGER bid_evidence_asset_projection_complete AFTER INSERT ON bid_evidence_asset_artifacts
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION kb_bid_v2_verify_evidence_bundle_projection();

CREATE TABLE bid_workspace_asset_artifacts (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  object_ref kb_object_ref NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  media_type text NOT NULL CHECK (octet_length(media_type) BETWEEN 1 AND 256),
  file_name text NOT NULL CHECK (octet_length(file_name) BETWEEN 1 AND 1024),
  object_state text NOT NULL DEFAULT 'available' CHECK (object_state='available'),
  byte_length bigint NOT NULL CHECK (byte_length > 0),
  width_px integer CHECK (width_px > 0),
  height_px integer CHECK (height_px > 0),
  page_count integer CHECK (page_count > 0 AND page_count <= 1000),
  source text NOT NULL CHECK (source IN ('human_upload','ai_evidence')),
  created_by kb_actor_identity NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(project_id,id),
  UNIQUE(project_id,workspace_id,id),
  UNIQUE(id,workspace_id,object_ref,content_sha256,media_type,object_state),
  FOREIGN KEY(project_id,workspace_id) REFERENCES bid_submission_workspaces(project_id,id),
  FOREIGN KEY(object_ref,content_sha256,media_type,object_state)
    REFERENCES object_registry(object_ref,digest,media_type,state),
  CHECK (object_ref='objects/'||content_sha256),
  CHECK ((media_type IN ('image/png','image/jpeg','image/webp') AND width_px IS NOT NULL AND height_px IS NOT NULL AND page_count IS NULL)
      OR (media_type='application/pdf' AND width_px IS NULL AND height_px IS NULL AND page_count IS NOT NULL)
      OR (media_type NOT IN ('image/png','image/jpeg','image/webp','application/pdf') AND width_px IS NULL AND height_px IS NULL AND page_count IS NULL))
);

CREATE TABLE bid_workspace_asset_retirement_artifacts (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  asset_revision_id uuid NOT NULL UNIQUE,
  retired_by kb_actor_identity NOT NULL,
  reason text NOT NULL CHECK(octet_length(reason) BETWEEN 1 AND 1024),
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(project_id,id),
  FOREIGN KEY(project_id,workspace_id,asset_revision_id)
    REFERENCES bid_workspace_asset_artifacts(project_id,workspace_id,id)
);

CREATE TABLE bid_submission_fulfillment_evidence_revision_artifacts (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  evidence_lineage_id uuid NOT NULL,
  revision bigint NOT NULL CHECK(revision>0),
  workspace_revision_id uuid NOT NULL,
  binding_revision_id uuid NOT NULL,
  target_revision_id uuid NOT NULL,
  target_kind text NOT NULL CHECK (target_kind IN ('block','table_row','structured_value','asset','quote_snapshot')),
  dependency_sha256 kb_sha256 NOT NULL,
  canonical_payload bytea NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(project_id,id),
  UNIQUE(project_id,evidence_lineage_id,revision),
  FOREIGN KEY(project_id,workspace_id) REFERENCES bid_submission_workspaces(project_id,id),
  FOREIGN KEY(project_id,workspace_revision_id) REFERENCES bid_workspace_revision_artifacts(project_id,id),
  FOREIGN KEY(project_id,binding_revision_id) REFERENCES bid_outline_fulfillment_binding_revision_artifacts(project_id,id),
  CHECK (content_sha256=kb_bid_v2_sha256_bytes(canonical_payload))
);

-- Evidence applicability is derived against the requested immutable WorkspaceRevision;
-- edits never copy or mutate historical evidence rows.
CREATE FUNCTION kb_bid_v2_fulfillment_evidence_is_current(
  p_workspace_revision_id uuid,p_binding_revision_id uuid
) RETURNS boolean LANGUAGE sql STABLE SET search_path=pg_catalog,public AS $$
SELECT EXISTS (
  SELECT 1 FROM bid_submission_fulfillment_evidence_revision_artifacts evidence
  JOIN bid_outline_fulfillment_binding_revision_artifacts binding
    ON binding.id=evidence.binding_revision_id
  JOIN bid_workspace_revision_artifacts revision
    ON revision.id=p_workspace_revision_id AND revision.workspace_id=evidence.workspace_id
  WHERE evidence.binding_revision_id=p_binding_revision_id
    AND binding.state='bound'
    AND binding.requirement_projection_id=revision.requirement_projection_id
    AND (
      (evidence.target_kind IN ('block','table_row','structured_value') AND EXISTS (
        SELECT 1 FROM bid_workspace_block_occurrences occurrence
        JOIN bid_content_block_revision_artifacts block ON block.id=occurrence.block_revision_id
        WHERE occurrence.workspace_revision_id=p_workspace_revision_id
          AND block.id=evidence.target_revision_id
          AND block.content_sha256=evidence.dependency_sha256))
      OR (evidence.target_kind='quote_snapshot'
        AND revision.quote_snapshot_id=evidence.target_revision_id
        AND revision.quote_snapshot_sha256=evidence.dependency_sha256)
      OR (evidence.target_kind='asset' AND EXISTS (
        SELECT 1 FROM bid_workspace_asset_artifacts asset
        WHERE asset.workspace_id=revision.workspace_id AND asset.id=evidence.target_revision_id
          AND asset.content_sha256=evidence.dependency_sha256
          AND NOT EXISTS (SELECT 1 FROM bid_workspace_asset_retirement_artifacts retirement
            WHERE retirement.asset_revision_id=asset.id)))
    )
) $$;

CREATE TABLE bid_outline_assessment_snapshot_artifacts (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  workspace_revision_id uuid NOT NULL,
  requirement_projection_id uuid NOT NULL,
  scope_revision_id uuid NOT NULL,
  document_settings_revision_id uuid NOT NULL,
  asset_set_sha256 kb_sha256 NOT NULL,
  quote_snapshot_id uuid,
  quote_snapshot_sha256 kb_sha256,
  status text NOT NULL CHECK (status IN ('ready','has_warnings','has_critical_warnings')),
  CHECK ((quote_snapshot_id IS NULL)=(quote_snapshot_sha256 IS NULL)),
  assessment_input_sha256 kb_sha256 NOT NULL,
  canonical_payload bytea NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(workspace_id,assessment_input_sha256),
  UNIQUE(project_id,id),
  UNIQUE(project_id,workspace_id,id,workspace_revision_id,scope_revision_id,requirement_projection_id,document_settings_revision_id,content_sha256),
  FOREIGN KEY(project_id,workspace_id) REFERENCES bid_submission_workspaces(project_id,id),
  FOREIGN KEY(project_id,workspace_revision_id) REFERENCES bid_workspace_revision_artifacts(project_id,id),
  FOREIGN KEY(project_id,requirement_projection_id) REFERENCES bid_workspace_requirement_projection_artifacts(project_id,id),
  FOREIGN KEY(project_id,scope_revision_id) REFERENCES bid_workspace_scope_revision_artifacts(project_id,id),
  FOREIGN KEY(project_id,document_settings_revision_id) REFERENCES bid_document_settings_revision_artifacts(project_id,id),
  FOREIGN KEY(project_id,workspace_id,workspace_revision_id,scope_revision_id,requirement_projection_id,document_settings_revision_id)
    REFERENCES bid_workspace_revision_artifacts(project_id,workspace_id,id,scope_revision_id,requirement_projection_id,document_settings_revision_id),
  CHECK (content_sha256=kb_bid_v2_sha256_bytes(canonical_payload))
);

CREATE TABLE bid_submission_assessment_snapshot_artifacts (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  workspace_revision_id uuid NOT NULL,
  requirement_projection_id uuid NOT NULL,
  scope_revision_id uuid NOT NULL,
  document_settings_revision_id uuid NOT NULL,
  asset_set_sha256 kb_sha256 NOT NULL,
  quote_snapshot_id uuid,
  quote_snapshot_sha256 kb_sha256,
  status text NOT NULL CHECK (status IN ('ready','has_warnings','has_critical_warnings')),
  CHECK ((quote_snapshot_id IS NULL)=(quote_snapshot_sha256 IS NULL)),
  assessment_input_sha256 kb_sha256 NOT NULL,
  canonical_payload bytea NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(workspace_id,assessment_input_sha256),
  UNIQUE(project_id,id),
  UNIQUE(project_id,workspace_id,id),
  UNIQUE(project_id,workspace_id,id,workspace_revision_id,scope_revision_id,requirement_projection_id,document_settings_revision_id,content_sha256),
  FOREIGN KEY(project_id,workspace_id) REFERENCES bid_submission_workspaces(project_id,id),
  FOREIGN KEY(project_id,workspace_revision_id) REFERENCES bid_workspace_revision_artifacts(project_id,id),
  FOREIGN KEY(project_id,requirement_projection_id) REFERENCES bid_workspace_requirement_projection_artifacts(project_id,id),
  FOREIGN KEY(project_id,scope_revision_id) REFERENCES bid_workspace_scope_revision_artifacts(project_id,id),
  FOREIGN KEY(project_id,document_settings_revision_id) REFERENCES bid_document_settings_revision_artifacts(project_id,id),
  FOREIGN KEY(project_id,workspace_id,workspace_revision_id,scope_revision_id,requirement_projection_id,document_settings_revision_id)
    REFERENCES bid_workspace_revision_artifacts(project_id,workspace_id,id,scope_revision_id,requirement_projection_id,document_settings_revision_id),
  CHECK (content_sha256=kb_bid_v2_sha256_bytes(canonical_payload))
);

CREATE TABLE bid_submission_assessment_snapshot_evidence_items (
  assessment_snapshot_id uuid NOT NULL,
  project_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  ordinal integer NOT NULL CHECK (ordinal>=0),
  selection_id uuid NOT NULL,
  selection_sha256 kb_sha256 NOT NULL,
  matching_report_id uuid NOT NULL,
  evidence_bundle_id uuid NOT NULL,
  evidence_bundle_sha256 kb_sha256 NOT NULL,
  evidence_item_id uuid NOT NULL,
  evidence_item_sha256 kb_sha256 NOT NULL,
  selection_kind text NOT NULL DEFAULT 'accepted' CHECK (selection_kind='accepted'),
  PRIMARY KEY(assessment_snapshot_id,ordinal),
  UNIQUE(assessment_snapshot_id,selection_id,evidence_item_id),
  FOREIGN KEY(project_id,workspace_id,assessment_snapshot_id)
    REFERENCES bid_submission_assessment_snapshot_artifacts(project_id,workspace_id,id),
  FOREIGN KEY(project_id,workspace_id,selection_id,selection_sha256,selection_kind,matching_report_id)
    REFERENCES bid_evidence_selection_artifacts(project_id,workspace_id,id,content_sha256,selection_kind,matching_report_id),
  FOREIGN KEY(project_id,workspace_id,matching_report_id)
    REFERENCES bid_evidence_match_reports(project_id,workspace_id,id),
  FOREIGN KEY(project_id,workspace_id,evidence_bundle_id,evidence_bundle_sha256)
    REFERENCES bid_evidence_bundle_artifacts(project_id,workspace_id,id,content_sha256),
  FOREIGN KEY(evidence_bundle_id,evidence_item_id,evidence_item_sha256)
    REFERENCES bid_evidence_bundle_items(evidence_bundle_id,id,content_sha256)
);

CREATE TABLE bid_quote_snapshot_artifacts (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
  revision bigint NOT NULL CHECK (revision > 0),
  currency text NOT NULL CHECK (currency='CNY'),
  canonical_payload bytea NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  actor kb_actor_identity NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(project_id,revision),
  UNIQUE(project_id,id),
  UNIQUE(project_id,id,content_sha256),
  CHECK (content_sha256=kb_bid_v2_sha256_bytes(canonical_payload))
);

CREATE TABLE bid_quote_snapshot_object_identities (
  quote_snapshot_id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  object_ref kb_object_ref NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  media_type text NOT NULL DEFAULT 'application/json' CHECK (media_type='application/json'),
  object_state text NOT NULL DEFAULT 'available' CHECK (object_state='available'),
  UNIQUE(quote_snapshot_id,project_id,object_ref,content_sha256,media_type,object_state),
  FOREIGN KEY(project_id,quote_snapshot_id,content_sha256)
    REFERENCES bid_quote_snapshot_artifacts(project_id,id,content_sha256),
  FOREIGN KEY(object_ref,content_sha256,media_type,object_state)
    REFERENCES object_registry(object_ref,digest,media_type,state),
  CHECK (object_ref='objects/'||content_sha256)
);

CREATE TABLE bid_quote_snapshot_current (
  scope_id uuid PRIMARY KEY REFERENCES bid_projects(id) ON DELETE RESTRICT,
  artifact_id uuid NOT NULL,
  artifact_sha256 kb_sha256 NOT NULL,
  generation bigint NOT NULL CHECK (generation > 0),
  created_at timestamptz NOT NULL,
  FOREIGN KEY(scope_id,artifact_id,artifact_sha256)
    REFERENCES bid_quote_snapshot_artifacts(project_id,id,content_sha256)
);

ALTER TABLE bid_workspace_revision_artifacts
  ADD CONSTRAINT bid_workspace_revision_quote_snapshot_fk
  FOREIGN KEY(project_id,quote_snapshot_id,quote_snapshot_sha256)
  REFERENCES bid_quote_snapshot_artifacts(project_id,id,content_sha256);
ALTER TABLE bid_outline_assessment_snapshot_artifacts
  ADD CONSTRAINT bid_outline_assessment_quote_snapshot_fk
  FOREIGN KEY(project_id,quote_snapshot_id,quote_snapshot_sha256)
  REFERENCES bid_quote_snapshot_artifacts(project_id,id,content_sha256),
  ADD CONSTRAINT bid_outline_assessment_workspace_quote_fk
  FOREIGN KEY(project_id,workspace_id,workspace_revision_id,scope_revision_id,requirement_projection_id,
    document_settings_revision_id,quote_snapshot_id,quote_snapshot_sha256)
  REFERENCES bid_workspace_revision_artifacts(project_id,workspace_id,id,scope_revision_id,
    requirement_projection_id,document_settings_revision_id,quote_snapshot_id,quote_snapshot_sha256);
ALTER TABLE bid_submission_assessment_snapshot_artifacts
  ADD CONSTRAINT bid_submission_assessment_quote_snapshot_fk
  FOREIGN KEY(project_id,quote_snapshot_id,quote_snapshot_sha256)
  REFERENCES bid_quote_snapshot_artifacts(project_id,id,content_sha256),
  ADD CONSTRAINT bid_submission_assessment_workspace_quote_fk
  FOREIGN KEY(project_id,workspace_id,workspace_revision_id,scope_revision_id,requirement_projection_id,
    document_settings_revision_id,quote_snapshot_id,quote_snapshot_sha256)
  REFERENCES bid_workspace_revision_artifacts(project_id,workspace_id,id,scope_revision_id,
    requirement_projection_id,document_settings_revision_id,quote_snapshot_id,quote_snapshot_sha256);

-- Bindings use a closed tagged target union. The trigger preserves the compact
-- target_id wire shape while enforcing the real relational identity for each
-- target kind and project/workspace scope.
CREATE FUNCTION kb_bid_v2_validate_fulfillment_binding_target()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
BEGIN
  IF NEW.target_kind='outline_node' THEN
    PERFORM 1 FROM bid_outline_node_lineages
      WHERE project_id=NEW.project_id AND workspace_id=NEW.workspace_id AND id=NEW.target_id;
  ELSIF NEW.target_kind='content_block' THEN
    PERFORM 1 FROM bid_outline_candidate_block_targets_v1 target
      WHERE target.candidate_id=NEW.candidate_id AND target.block_ref=NEW.target_id
        AND target.owner_node_ref=NEW.target_node_id;
  ELSIF NEW.target_kind='response_table' THEN
    PERFORM 1 FROM bid_content_block_lineages lineage
      WHERE lineage.project_id=NEW.project_id AND lineage.workspace_id=NEW.workspace_id
        AND lineage.id=NEW.target_id
        AND EXISTS (
          SELECT 1 FROM bid_content_block_revision_artifacts revision
          WHERE revision.project_id=lineage.project_id
            AND revision.workspace_id=lineage.workspace_id
            AND revision.lineage_id=lineage.id
            AND revision.block_kind='table'
        );
  ELSIF NEW.target_kind='structured_form' THEN
    PERFORM 1 FROM bid_tender_structured_form_definition_artifacts
      WHERE project_id=NEW.project_id AND id=NEW.target_id;
  ELSIF NEW.target_kind='quote' THEN
    PERFORM 1 FROM bid_quote_snapshot_artifacts
      WHERE project_id=NEW.project_id AND id=NEW.target_id;
  END IF;
  IF NOT FOUND THEN
    RAISE EXCEPTION 'invalid % fulfillment binding target % for project % workspace %',
      NEW.target_kind,NEW.target_id,NEW.project_id,NEW.workspace_id USING ERRCODE='23503';
  END IF;
  RETURN NEW;
END
$$;

CREATE TRIGGER bid_outline_fulfillment_binding_target_fk
BEFORE INSERT ON bid_outline_fulfillment_binding_revision_artifacts
FOR EACH ROW EXECUTE FUNCTION kb_bid_v2_validate_fulfillment_binding_target();

CREATE TABLE bid_render_style_contract_artifacts (
  id uuid PRIMARY KEY,
  version bigint NOT NULL UNIQUE CHECK (version > 0),
  schema_version smallint NOT NULL CHECK (schema_version=1),
  canonical_payload bytea NOT NULL,
  content_sha256 kb_sha256 NOT NULL UNIQUE,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(id,content_sha256),
  CHECK (content_sha256=kb_bid_v2_sha256_bytes(canonical_payload))
);

-- ContentGenerate keeps the generic async request envelope while projecting
-- every frozen authoring identity into typed relational columns. This closes
-- same-workspace splicing without imposing V2 fields on the other four jobs.
CREATE TABLE bid_content_generation_request_identities (
  request_artifact_id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  request_kind text NOT NULL DEFAULT 'content_generate' CHECK (request_kind='content_generate'),
  request_revision bigint NOT NULL CHECK (request_revision > 0),
  request_sha256 kb_sha256 NOT NULL,
  frozen_input_sha256 kb_sha256 NOT NULL,
  request_operation text NOT NULL CHECK (request_operation IN ('match_only','generate')),
  base_workspace_revision_id uuid NOT NULL,
  base_workspace_sha256 kb_sha256 NOT NULL,
  requirement_projection_id uuid NOT NULL,
  requirement_projection_sha256 kb_sha256 NOT NULL,
  outline_checkpoint_id uuid NOT NULL,
  outline_checkpoint_sha256 kb_sha256 NOT NULL,
  scope_revision_id uuid NOT NULL,
  scope_revision_sha256 kb_sha256 NOT NULL,
  document_settings_revision_id uuid NOT NULL,
  document_settings_sha256 kb_sha256 NOT NULL,
  render_style_contract_id uuid NOT NULL,
  render_style_contract_sha256 kb_sha256 NOT NULL,
  evidence_selection_mode text NOT NULL CHECK (evidence_selection_mode IN ('system_proposed','user_pick_set')),
  evidence_selection_sha256 kb_sha256 NOT NULL,
  pick_set_kind text CHECK (pick_set_kind IS NULL OR pick_set_kind='user_pick_set'),
  pick_set_artifact_id uuid,
  pick_set_sha256 kb_sha256,
  pick_set_matching_report_id uuid,
  matching_policy_id uuid,
  matching_policy_sha256 kb_sha256,
  retrieval_identity_payload bytea,
  retrieval_identity_sha256 kb_sha256,
  quote_snapshot_id uuid,
  quote_snapshot_sha256 kb_sha256,
  prompt_contract_id uuid,
  prompt_contract_sha256 kb_sha256,
  prompt_utf8 bytea,
  prompt_sha256 kb_sha256,
  output_schema_id text,
  output_schema_utf8 bytea,
  output_schema_sha256 kb_sha256,
  template_contract_id uuid,
  template_contract_sha256 kb_sha256,
  model_contract_id uuid,
  model_contract_sha256 kb_sha256,
  agent_contract_id uuid,
  agent_contract_sha256 kb_sha256,
  runtime_contract_payload bytea,
  runtime_contract_sha256 kb_sha256,
  target_kind text NOT NULL CHECK (target_kind IN ('node','subtree','workspace')),
  target_node_lineage_id uuid,
  target_node_revision_id uuid,
  target_workspace_revision_id uuid,
  fill_policy text NOT NULL CHECK (fill_policy IN ('empty_only','append_candidate','missing_requirements_only')),
  insertion_node_revision_id uuid,
  insertion_block_revision_id uuid,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(request_artifact_id,project_id,workspace_id),
  UNIQUE(request_artifact_id,frozen_input_sha256,request_operation),
  UNIQUE(request_artifact_id,request_kind,request_operation,project_id,workspace_id,request_revision,request_sha256,base_workspace_revision_id,base_workspace_sha256),
  FOREIGN KEY(request_artifact_id,project_id,workspace_id,request_kind,request_revision,request_sha256,frozen_input_sha256)
    REFERENCES bid_async_request_snapshot_artifacts(id,project_id,workspace_id,request_kind,revision,request_sha256,frozen_input_sha256),
  FOREIGN KEY(project_id,workspace_id,base_workspace_revision_id,base_workspace_sha256)
    REFERENCES bid_workspace_revision_artifacts(project_id,workspace_id,id,content_sha256),
  FOREIGN KEY(project_id,workspace_id,base_workspace_revision_id,scope_revision_id,requirement_projection_id,document_settings_revision_id)
    REFERENCES bid_workspace_revision_artifacts(project_id,workspace_id,id,scope_revision_id,requirement_projection_id,document_settings_revision_id),
  FOREIGN KEY(project_id,workspace_id,requirement_projection_id,requirement_projection_sha256)
    REFERENCES bid_workspace_requirement_projection_artifacts(project_id,workspace_id,id,content_sha256),
  FOREIGN KEY(project_id,workspace_id,outline_checkpoint_id,base_workspace_revision_id,requirement_projection_id,requirement_projection_sha256,outline_checkpoint_sha256)
    REFERENCES bid_outline_checkpoint_artifacts(project_id,workspace_id,id,workspace_revision_id,requirement_projection_id,requirement_projection_sha256,content_sha256),
  FOREIGN KEY(project_id,workspace_id,scope_revision_id,scope_revision_sha256)
    REFERENCES bid_workspace_scope_revision_artifacts(project_id,workspace_id,id,content_sha256),
  FOREIGN KEY(project_id,workspace_id,document_settings_revision_id,document_settings_sha256)
    REFERENCES bid_document_settings_revision_artifacts(project_id,workspace_id,id,content_sha256),
  FOREIGN KEY(render_style_contract_id,render_style_contract_sha256)
    REFERENCES bid_render_style_contract_artifacts(id,content_sha256),
  FOREIGN KEY(project_id,workspace_id,pick_set_artifact_id,pick_set_sha256,pick_set_kind,pick_set_matching_report_id)
    REFERENCES bid_evidence_selection_artifacts(project_id,workspace_id,id,content_sha256,selection_kind,matching_report_id),
  FOREIGN KEY(project_id,quote_snapshot_id,quote_snapshot_sha256)
    REFERENCES bid_quote_snapshot_artifacts(project_id,id,content_sha256),
  FOREIGN KEY(project_id,workspace_id,target_node_revision_id,target_node_lineage_id)
    REFERENCES bid_outline_node_revision_artifacts(project_id,workspace_id,id,lineage_id),
  FOREIGN KEY(project_id,base_workspace_revision_id,target_node_revision_id)
    REFERENCES bid_workspace_node_occurrences(project_id,workspace_revision_id,node_revision_id),
  FOREIGN KEY(project_id,base_workspace_revision_id,insertion_node_revision_id)
    REFERENCES bid_workspace_node_occurrences(project_id,workspace_revision_id,node_revision_id),
  CHECK (CASE
      WHEN evidence_selection_mode='system_proposed' THEN
        pick_set_kind IS NULL AND pick_set_artifact_id IS NULL AND pick_set_sha256 IS NULL
        AND pick_set_matching_report_id IS NULL AND matching_policy_id IS NOT NULL AND matching_policy_sha256 IS NOT NULL
      WHEN evidence_selection_mode='user_pick_set' THEN
        pick_set_kind='user_pick_set' AND pick_set_artifact_id IS NOT NULL AND pick_set_sha256 IS NOT NULL
        AND pick_set_matching_report_id IS NOT NULL AND matching_policy_id IS NULL AND matching_policy_sha256 IS NULL
      ELSE false END),
  CHECK ((quote_snapshot_id IS NULL)=(quote_snapshot_sha256 IS NULL)),
  CHECK (CASE evidence_selection_mode
    WHEN 'system_proposed' THEN retrieval_identity_payload IS NOT NULL
      AND retrieval_identity_sha256=kb_bid_v2_sha256_bytes(retrieval_identity_payload)
      AND convert_from(retrieval_identity_payload,'UTF8')::jsonb IS NOT NULL
    WHEN 'user_pick_set' THEN retrieval_identity_payload IS NULL AND retrieval_identity_sha256 IS NULL
    ELSE false END),
  CHECK (CASE
      WHEN request_operation='generate' THEN
        prompt_contract_id IS NOT NULL AND prompt_contract_sha256 IS NOT NULL
        AND prompt_utf8 IS NOT NULL AND prompt_sha256=kb_bid_v2_sha256_bytes(prompt_utf8)
        AND output_schema_id='urn:knowledgebrain:bid:content-generation-output:v1'
        AND output_schema_utf8 IS NOT NULL
        AND output_schema_sha256=kb_bid_v2_sha256_bytes(output_schema_utf8)
        AND template_contract_id IS NOT NULL AND template_contract_sha256 IS NOT NULL
        AND model_contract_id IS NOT NULL AND model_contract_sha256 IS NOT NULL
        AND agent_contract_id IS NOT NULL AND agent_contract_sha256 IS NOT NULL
        AND runtime_contract_payload IS NOT NULL
        AND runtime_contract_sha256=kb_bid_v2_sha256_bytes(runtime_contract_payload)
      WHEN request_operation='match_only' THEN
        prompt_contract_id IS NULL AND prompt_contract_sha256 IS NULL AND prompt_utf8 IS NULL
        AND prompt_sha256 IS NULL AND output_schema_id IS NULL AND output_schema_utf8 IS NULL AND output_schema_sha256 IS NULL
        AND template_contract_id IS NULL AND template_contract_sha256 IS NULL
        AND model_contract_id IS NULL AND model_contract_sha256 IS NULL
        AND agent_contract_id IS NULL AND agent_contract_sha256 IS NULL
        AND runtime_contract_payload IS NULL AND runtime_contract_sha256 IS NULL
      ELSE false END),
  CHECK (CASE
      WHEN target_kind IN ('node','subtree') THEN target_node_lineage_id IS NOT NULL AND target_node_revision_id IS NOT NULL AND target_workspace_revision_id IS NULL
      WHEN target_kind='workspace' THEN target_node_lineage_id IS NULL AND target_node_revision_id IS NULL AND target_workspace_revision_id IS NOT NULL AND target_workspace_revision_id=base_workspace_revision_id
      ELSE false END),
  CHECK (CASE
      WHEN insertion_node_revision_id IS NULL THEN insertion_block_revision_id IS NULL
      WHEN fill_policy='append_candidate' THEN true
      ELSE false END)
);

CREATE TABLE bid_content_generation_request_evidence_bundles (
  request_artifact_id uuid NOT NULL,
  project_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  ordinal integer NOT NULL CHECK (ordinal >= 0),
  evidence_bundle_id uuid NOT NULL,
  evidence_bundle_sha256 kb_sha256 NOT NULL,
  PRIMARY KEY(request_artifact_id,ordinal),
  UNIQUE(request_artifact_id,evidence_bundle_id),
  FOREIGN KEY(request_artifact_id,project_id,workspace_id)
    REFERENCES bid_content_generation_request_identities(request_artifact_id,project_id,workspace_id),
  FOREIGN KEY(project_id,workspace_id,evidence_bundle_id,evidence_bundle_sha256)
    REFERENCES bid_evidence_bundle_artifacts(project_id,workspace_id,id,content_sha256)
);

CREATE FUNCTION kb_bid_v2_validate_content_generation_anchor()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
BEGIN
  IF NEW.insertion_node_revision_id IS NOT NULL THEN
    IF NEW.target_kind='node'
      AND NEW.insertion_node_revision_id IS DISTINCT FROM NEW.target_node_revision_id THEN
      RAISE EXCEPTION 'CONTENT_GENERATION_INPUT_INVALID: insertion anchor is outside the frozen target node'
        USING ERRCODE='23514';
    ELSIF NEW.target_kind='subtree' AND NOT EXISTS (
      WITH RECURSIVE target_tree(id) AS (
        SELECT occurrence.id
        FROM bid_workspace_node_occurrences occurrence
        WHERE occurrence.project_id=NEW.project_id
          AND occurrence.workspace_revision_id=NEW.base_workspace_revision_id
          AND occurrence.node_revision_id=NEW.target_node_revision_id
        UNION ALL
        SELECT child.id
        FROM bid_workspace_node_occurrences child
        JOIN target_tree parent ON child.parent_occurrence_id=parent.id
        WHERE child.project_id=NEW.project_id
          AND child.workspace_revision_id=NEW.base_workspace_revision_id
      )
      SELECT 1
      FROM target_tree
      JOIN bid_workspace_node_occurrences anchor
        ON anchor.workspace_revision_id=NEW.base_workspace_revision_id
       AND anchor.id=target_tree.id
      WHERE anchor.node_revision_id=NEW.insertion_node_revision_id
    ) THEN
      RAISE EXCEPTION 'CONTENT_GENERATION_INPUT_INVALID: insertion anchor is outside the frozen target subtree'
        USING ERRCODE='23514';
    END IF;
  END IF;
  IF NEW.insertion_block_revision_id IS NOT NULL AND NOT EXISTS (
    SELECT 1
    FROM bid_workspace_block_occurrences block_occurrence
    JOIN bid_workspace_node_occurrences node_occurrence
      ON node_occurrence.workspace_revision_id=block_occurrence.workspace_revision_id
     AND node_occurrence.id=block_occurrence.node_occurrence_id
    WHERE block_occurrence.project_id=NEW.project_id
      AND block_occurrence.workspace_revision_id=NEW.base_workspace_revision_id
      AND block_occurrence.block_revision_id=NEW.insertion_block_revision_id
      AND node_occurrence.node_revision_id=NEW.insertion_node_revision_id
  ) THEN
    RAISE EXCEPTION 'CONTENT_GENERATION_INPUT_INVALID: insertion block is outside the frozen anchor node'
      USING ERRCODE='23514';
  END IF;
  RETURN NEW;
END $$;
CREATE TRIGGER bid_content_generation_request_anchor_valid
BEFORE INSERT ON bid_content_generation_request_identities
FOR EACH ROW EXECUTE FUNCTION kb_bid_v2_validate_content_generation_anchor();

ALTER TABLE bid_candidate_artifacts
  ADD CONSTRAINT bid_content_candidate_request_identity_fk
  FOREIGN KEY(request_artifact_id,request_kind,request_operation,project_id,workspace_id,request_revision,request_sha256,base_workspace_revision_id,base_workspace_sha256)
  REFERENCES bid_content_generation_request_identities(request_artifact_id,request_kind,request_operation,project_id,workspace_id,request_revision,request_sha256,base_workspace_revision_id,base_workspace_sha256);

CREATE TABLE bid_renderer_contract_artifacts (
  id uuid PRIMARY KEY,
  format text NOT NULL CHECK (format IN ('docx','pdf')),
  version bigint NOT NULL CHECK (version > 0),
  schema_version smallint NOT NULL CHECK (schema_version=1),
  canonical_payload bytea NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  approved_at timestamptz NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(format,version),
  UNIQUE(format,id,content_sha256),
  CHECK (content_sha256=kb_bid_v2_sha256_bytes(canonical_payload))
);

CREATE TABLE bid_attachment_preparation_revision_artifacts (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  source_asset_revision_id uuid NOT NULL,
  revision bigint NOT NULL CHECK (revision > 0),
  status text NOT NULL CHECK (status IN ('pending','ready','failed')),
  page_assets jsonb NOT NULL CHECK (jsonb_typeof(page_assets)='array'),
  canonical_payload jsonb NOT NULL CHECK (jsonb_typeof(canonical_payload)='object'),
  preparation_sha256 kb_sha256 NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(workspace_id,source_asset_revision_id,revision),
  UNIQUE(project_id,id),
  UNIQUE(project_id,workspace_id,id),
  UNIQUE(project_id,workspace_id,id,preparation_sha256),
  UNIQUE(project_id,workspace_id,id,status,preparation_sha256),
  FOREIGN KEY(project_id,workspace_id) REFERENCES bid_submission_workspaces(project_id,id),
  FOREIGN KEY(project_id,workspace_id,source_asset_revision_id)
    REFERENCES bid_workspace_asset_artifacts(project_id,workspace_id,id),
  CHECK (canonical_payload->>'preparation_sha256'=preparation_sha256),
  CHECK (preparation_sha256=kb_bid_v2_sha256_bytes(convert_to((canonical_payload-'preparation_sha256')::text,'UTF8'))),
  CHECK (page_assets=canonical_payload->'page_assets')
);

CREATE TABLE bid_attachment_preparation_asset_items (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  attachment_preparation_revision_id uuid NOT NULL,
  ordinal integer NOT NULL CHECK (ordinal>=0),
  page_number integer NOT NULL CHECK (page_number>0),
  object_ref kb_object_ref NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  media_type text NOT NULL CHECK (media_type IN ('image/png','image/jpeg','image/webp')),
  object_state text NOT NULL DEFAULT 'available' CHECK (object_state='available'),
  geometry jsonb NOT NULL CHECK (jsonb_typeof(geometry)='object'),
  UNIQUE(attachment_preparation_revision_id,ordinal),
  UNIQUE(attachment_preparation_revision_id,page_number),
  UNIQUE(id,workspace_id,object_ref,content_sha256,media_type,object_state),
  FOREIGN KEY(project_id,workspace_id,attachment_preparation_revision_id)
    REFERENCES bid_attachment_preparation_revision_artifacts(project_id,workspace_id,id),
  FOREIGN KEY(object_ref,content_sha256,media_type,object_state)
    REFERENCES object_registry(object_ref,digest,media_type,state),
  CHECK (object_ref='objects/'||content_sha256)
);

CREATE FUNCTION kb_bid_v2_validate_attachment_preparation_payload()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE p jsonb:=NEW.canonical_payload; page jsonb; geometry jsonb;
BEGIN
  IF NOT kb_bid_v2_json_keys_exact(p,ARRAY['schema_version','attachment_preparation_revision_id','project_id','workspace_id','source_asset_revision_id','revision','status','page_assets','preparation_sha256'])
     OR p->'schema_version' IS DISTINCT FROM '1'::jsonb
     OR NOT kb_bid_v2_uuid_text(p->>'attachment_preparation_revision_id')
     OR NOT kb_bid_v2_uuid_text(p->>'project_id') OR NOT kb_bid_v2_uuid_text(p->>'workspace_id')
     OR NOT kb_bid_v2_uuid_text(p->>'source_asset_revision_id')
     OR jsonb_typeof(p->'revision') IS DISTINCT FROM 'number' OR p->>'revision' !~ '^[1-9][0-9]*$'
     OR jsonb_typeof(p->'status') IS DISTINCT FROM 'string' OR COALESCE(p->>'status','') NOT IN ('pending','ready','failed')
     OR jsonb_typeof(p->'page_assets') IS DISTINCT FROM 'array' OR jsonb_array_length(p->'page_assets')>100000
     OR (p->>'status'='ready' AND jsonb_array_length(p->'page_assets')=0)
     OR jsonb_typeof(p->'preparation_sha256') IS DISTINCT FROM 'string' OR NOT kb_bid_v2_sha256_text(p->>'preparation_sha256')
     OR (p->>'attachment_preparation_revision_id')::uuid IS DISTINCT FROM NEW.id
     OR (p->>'project_id')::uuid IS DISTINCT FROM NEW.project_id OR (p->>'workspace_id')::uuid IS DISTINCT FROM NEW.workspace_id
     OR (p->>'source_asset_revision_id')::uuid IS DISTINCT FROM NEW.source_asset_revision_id
     OR (p->>'revision')::bigint IS DISTINCT FROM NEW.revision OR p->>'status' IS DISTINCT FROM NEW.status
     OR p->'page_assets' IS DISTINCT FROM NEW.page_assets OR p->>'preparation_sha256' IS DISTINCT FROM NEW.preparation_sha256
     OR NEW.preparation_sha256<>kb_bid_v2_sha256_bytes(convert_to((p-'preparation_sha256')::text,'UTF8'))
     OR NOT EXISTS (SELECT 1 FROM bid_workspace_asset_artifacts source
       WHERE source.project_id=NEW.project_id AND source.workspace_id=NEW.workspace_id AND source.id=NEW.source_asset_revision_id)
  THEN RAISE EXCEPTION 'AttachmentPreparation canonical root invalid' USING ERRCODE='23514'; END IF;
  IF EXISTS (SELECT 1 FROM jsonb_array_elements(p->'page_assets') value GROUP BY value->>'page_asset_id' HAVING count(*)<>1)
     OR EXISTS (SELECT 1 FROM jsonb_array_elements(p->'page_assets') value GROUP BY value->>'page_number' HAVING count(*)<>1)
  THEN RAISE EXCEPTION 'AttachmentPreparation duplicate page identity' USING ERRCODE='23514'; END IF;
  FOR page IN SELECT value FROM jsonb_array_elements(p->'page_assets') LOOP
    geometry:=page->'geometry';
    IF NOT kb_bid_v2_json_keys_exact(page,ARRAY['page_asset_id','page_number','object_ref','sha256','media_type','geometry'])
       OR NOT kb_bid_v2_uuid_text(page->>'page_asset_id')
       OR jsonb_typeof(page->'page_number') IS DISTINCT FROM 'number' OR page->>'page_number' !~ '^[1-9][0-9]*$'
       OR page->>'object_ref' IS DISTINCT FROM ('objects/'||(page->>'sha256'))
       OR jsonb_typeof(page->'sha256') IS DISTINCT FROM 'string' OR NOT kb_bid_v2_sha256_text(page->>'sha256')
       OR jsonb_typeof(page->'media_type') IS DISTINCT FROM 'string' OR COALESCE(page->>'media_type','') NOT IN ('image/png','image/jpeg','image/webp')
       OR NOT kb_bid_v2_json_keys_exact(geometry,ARRAY['width_px','height_px'])
       OR jsonb_typeof(geometry->'width_px') IS DISTINCT FROM 'number' OR geometry->>'width_px' !~ '^[1-9][0-9]*$'
       OR jsonb_typeof(geometry->'height_px') IS DISTINCT FROM 'number' OR geometry->>'height_px' !~ '^[1-9][0-9]*$'
    THEN RAISE EXCEPTION 'AttachmentPreparation page asset invalid' USING ERRCODE='23514'; END IF;
  END LOOP;
  RETURN NEW;
END $$;
CREATE TRIGGER bid_attachment_preparation_payload_valid
BEFORE INSERT OR UPDATE ON bid_attachment_preparation_revision_artifacts
FOR EACH ROW EXECUTE FUNCTION kb_bid_v2_validate_attachment_preparation_payload();

CREATE FUNCTION kb_bid_v2_verify_attachment_preparation_projection()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE preparation_id uuid; expected jsonb; actual jsonb;
BEGIN
  IF TG_TABLE_NAME='bid_attachment_preparation_revision_artifacts' THEN preparation_id:=NEW.id; ELSE preparation_id:=NEW.attachment_preparation_revision_id; END IF;
  SELECT canonical_payload->'page_assets' INTO expected FROM bid_attachment_preparation_revision_artifacts WHERE id=preparation_id;
  IF expected IS NULL THEN RETURN NULL; END IF;
  SELECT COALESCE(jsonb_agg(jsonb_build_object(
    'page_asset_id',id,'page_number',page_number,'object_ref',object_ref,'sha256',content_sha256,
    'media_type',media_type,'geometry',geometry) ORDER BY ordinal),'[]'::jsonb)
    INTO actual FROM bid_attachment_preparation_asset_items WHERE attachment_preparation_revision_id=preparation_id;
  IF actual<>expected THEN RAISE EXCEPTION 'AttachmentPreparation ordered page projection mismatch' USING ERRCODE='23514'; END IF;
  RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER bid_attachment_preparation_projection_complete
AFTER INSERT ON bid_attachment_preparation_revision_artifacts DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION kb_bid_v2_verify_attachment_preparation_projection();
CREATE CONSTRAINT TRIGGER bid_attachment_preparation_page_projection_complete
AFTER INSERT ON bid_attachment_preparation_asset_items DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION kb_bid_v2_verify_attachment_preparation_projection();

CREATE TABLE bid_attachment_preparation_contract_artifacts (
  id uuid PRIMARY KEY,
  version bigint NOT NULL CHECK(version>0),
  canonical_payload bytea NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(id,content_sha256),
  CHECK(content_sha256=kb_bid_v2_sha256_bytes(canonical_payload))
);
INSERT INTO bid_attachment_preparation_contract_artifacts(id,version,canonical_payload,content_sha256)
SELECT '00000000-0000-5000-8000-000000000305',1,payload,kb_bid_v2_sha256_bytes(payload)
FROM (VALUES(convert_to('{"kind":"poppler-pdftoppm","version":1,"format":"png","dpi":144}','UTF8'))) seeded(payload);



CREATE TABLE bid_render_font_artifacts (
  id uuid PRIMARY KEY,
  object_ref kb_object_ref NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  object_state text NOT NULL DEFAULT 'available' CHECK (object_state='available'),
  media_type text NOT NULL CHECK (media_type IN ('font/ttf','font/otf','application/font-sfnt')),
  family text NOT NULL CHECK (octet_length(family) BETWEEN 1 AND 128),
  script text NOT NULL CHECK (script IN ('cjk','latin')),
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(id,object_ref,content_sha256,media_type,family,script),
  FOREIGN KEY(object_ref,content_sha256,media_type,object_state)
    REFERENCES object_registry(object_ref,digest,media_type,state),
  CHECK (object_ref='objects/'||content_sha256)
);

CREATE TABLE bid_render_document_snapshot_artifacts (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  schema_version smallint NOT NULL CHECK (schema_version=2),
  workspace_revision_id uuid NOT NULL,
  workspace_sha256 kb_sha256 NOT NULL,
  scope_revision_id uuid NOT NULL,
  outline_checkpoint_id uuid NOT NULL,
  outline_checkpoint_sha256 kb_sha256 NOT NULL,
  requirement_projection_id uuid NOT NULL,
  requirement_projection_sha256 kb_sha256 NOT NULL,
  document_settings_revision_id uuid NOT NULL,
  document_settings_sha256 kb_sha256 NOT NULL,
  submission_assessment_snapshot_id uuid NOT NULL,
  submission_assessment_snapshot_sha256 kb_sha256 NOT NULL,
  output_mode text NOT NULL CHECK (output_mode IN ('preview','review_draft','submission')),
  format text NOT NULL CHECK (format IN ('html','docx','pdf')),
  mode_options jsonb NOT NULL CHECK (jsonb_typeof(mode_options)='object'),
  content_block_schema_version smallint NOT NULL CHECK (content_block_schema_version=1),
  content_block_schema_sha256 kb_sha256 NOT NULL,
  render_operation_contract_version bigint NOT NULL CHECK (render_operation_contract_version > 0),
  render_operation_contract_sha256 kb_sha256 NOT NULL,
  docx_renderer_format text NOT NULL DEFAULT 'docx' CHECK (docx_renderer_format='docx'),
  docx_renderer_contract_id uuid NOT NULL,
  docx_renderer_contract_sha256 kb_sha256 NOT NULL,
  pdf_renderer_format text NOT NULL DEFAULT 'pdf' CHECK (pdf_renderer_format='pdf'),
  pdf_renderer_contract_id uuid NOT NULL,
  pdf_renderer_contract_sha256 kb_sha256 NOT NULL,
  style_contract_id uuid NOT NULL,
  style_contract_sha256 kb_sha256 NOT NULL,
  page_size text NOT NULL CHECK (page_size='A4'),
  page_width_mm numeric NOT NULL CHECK (page_width_mm=210),
  page_height_mm numeric NOT NULL CHECK (page_height_mm=297),
  margins_mm jsonb NOT NULL CHECK (jsonb_typeof(margins_mm)='object'),
  numbering_policy text NOT NULL CHECK (numbering_policy IN ('decimal','chinese','none')),
  toc_policy text NOT NULL CHECK (toc_policy IN ('none','included')),
  canonical_payload jsonb NOT NULL CHECK (jsonb_typeof(canonical_payload)='object'),
  content_sha256 kb_sha256 NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(project_id,id),
  UNIQUE(project_id,workspace_id,id),
  UNIQUE(project_id,id,workspace_revision_id),
  UNIQUE(project_id,workspace_id,id,workspace_revision_id),
  UNIQUE(project_id,workspace_id,id,output_mode,format,mode_options),
  FOREIGN KEY(project_id,workspace_id) REFERENCES bid_submission_workspaces(project_id,id),
  FOREIGN KEY(project_id,workspace_id,workspace_revision_id,workspace_sha256)
    REFERENCES bid_workspace_revision_artifacts(project_id,workspace_id,id,content_sha256),
  FOREIGN KEY(project_id,workspace_id,workspace_revision_id,scope_revision_id,requirement_projection_id,document_settings_revision_id)
    REFERENCES bid_workspace_revision_artifacts(project_id,workspace_id,id,scope_revision_id,requirement_projection_id,document_settings_revision_id),
  FOREIGN KEY(project_id,workspace_id,outline_checkpoint_id,workspace_revision_id,requirement_projection_id,requirement_projection_sha256,outline_checkpoint_sha256)
    REFERENCES bid_outline_checkpoint_artifacts(project_id,workspace_id,id,workspace_revision_id,requirement_projection_id,requirement_projection_sha256,content_sha256),
  FOREIGN KEY(project_id,workspace_id,requirement_projection_id,requirement_projection_sha256)
    REFERENCES bid_workspace_requirement_projection_artifacts(project_id,workspace_id,id,content_sha256),
  FOREIGN KEY(project_id,workspace_id,document_settings_revision_id,document_settings_sha256)
    REFERENCES bid_document_settings_revision_artifacts(project_id,workspace_id,id,content_sha256),
  FOREIGN KEY(project_id,workspace_id,submission_assessment_snapshot_id,workspace_revision_id,scope_revision_id,requirement_projection_id,document_settings_revision_id,submission_assessment_snapshot_sha256)
    REFERENCES bid_submission_assessment_snapshot_artifacts(project_id,workspace_id,id,workspace_revision_id,scope_revision_id,requirement_projection_id,document_settings_revision_id,content_sha256),
  FOREIGN KEY(docx_renderer_format,docx_renderer_contract_id,docx_renderer_contract_sha256)
    REFERENCES bid_renderer_contract_artifacts(format,id,content_sha256),
  FOREIGN KEY(pdf_renderer_format,pdf_renderer_contract_id,pdf_renderer_contract_sha256)
    REFERENCES bid_renderer_contract_artifacts(format,id,content_sha256),
  FOREIGN KEY(style_contract_id,style_contract_sha256)
    REFERENCES bid_render_style_contract_artifacts(id,content_sha256),
  CHECK ((output_mode='preview')=(format='html')),
  CHECK (
    mode_options ? 'watermark'
    AND mode_options - 'watermark' = '{}'::jsonb
    AND jsonb_typeof(mode_options->'watermark') IN ('null','string')
  ),
  CHECK (output_mode='review_draft' OR mode_options @> '{"watermark":null}'::jsonb),
  CHECK (output_mode<>'submission' OR mode_options @> '{"watermark":null}'::jsonb),
  CHECK (canonical_payload->>'snapshot_sha256'=content_sha256),
  CHECK (content_sha256=kb_bid_v2_sha256_bytes(convert_to((canonical_payload-'snapshot_sha256')::text,'UTF8')))
);

CREATE TABLE bid_render_snapshot_node_occurrences (
  render_snapshot_id uuid NOT NULL,
  project_id uuid NOT NULL,
  workspace_revision_id uuid NOT NULL,
  node_occurrence_id uuid NOT NULL,
  node_revision_id uuid NOT NULL,
  ordinal integer NOT NULL CHECK (ordinal >= 0),
  PRIMARY KEY(render_snapshot_id,node_occurrence_id),
  FOREIGN KEY(project_id,workspace_revision_id,node_occurrence_id,node_revision_id,ordinal)
    REFERENCES bid_workspace_node_occurrences(project_id,workspace_revision_id,id,node_revision_id,ordinal),
  FOREIGN KEY(project_id,render_snapshot_id,workspace_revision_id)
    REFERENCES bid_render_document_snapshot_artifacts(project_id,id,workspace_revision_id)
);

CREATE TABLE bid_render_snapshot_block_occurrences (
  render_snapshot_id uuid NOT NULL,
  project_id uuid NOT NULL,
  workspace_revision_id uuid NOT NULL,
  node_occurrence_id uuid NOT NULL,
  block_occurrence_id uuid NOT NULL,
  block_revision_id uuid NOT NULL,
  block_sha256 kb_sha256 NOT NULL,
  ordinal integer NOT NULL CHECK (ordinal >= 0),
  PRIMARY KEY(render_snapshot_id,block_occurrence_id),
  UNIQUE(render_snapshot_id,node_occurrence_id,ordinal),
  FOREIGN KEY(render_snapshot_id,node_occurrence_id)
    REFERENCES bid_render_snapshot_node_occurrences(render_snapshot_id,node_occurrence_id),
  FOREIGN KEY(project_id,workspace_revision_id,block_occurrence_id,node_occurrence_id,block_revision_id,ordinal)
    REFERENCES bid_workspace_block_occurrences(project_id,workspace_revision_id,id,node_occurrence_id,block_revision_id,ordinal),
  FOREIGN KEY(project_id,block_revision_id,block_sha256)
    REFERENCES bid_content_block_revision_artifacts(project_id,id,content_sha256)
);

CREATE TABLE bid_render_snapshot_asset_items (
  render_snapshot_id uuid NOT NULL REFERENCES bid_render_document_snapshot_artifacts(id),
  ordinal integer NOT NULL CHECK (ordinal >= 0),
  asset_revision_id uuid NOT NULL,
  object_ref kb_object_ref NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  object_state text NOT NULL DEFAULT 'available' CHECK (object_state='available'),
  media_type text NOT NULL CHECK (octet_length(media_type) BETWEEN 1 AND 256),
  provenance text NOT NULL CHECK (provenance IN ('knowledge_evidence','manual_workspace','prepared_attachment','quote_snapshot')),
  PRIMARY KEY(render_snapshot_id,ordinal),
  UNIQUE(render_snapshot_id,asset_revision_id),
  FOREIGN KEY(object_ref,content_sha256,media_type,object_state)
    REFERENCES object_registry(object_ref,digest,media_type,state),
  CHECK (object_ref='objects/'||content_sha256)
);

CREATE TABLE bid_render_snapshot_font_items (
  render_snapshot_id uuid NOT NULL REFERENCES bid_render_document_snapshot_artifacts(id),
  ordinal integer NOT NULL CHECK (ordinal >= 0),
  font_artifact_id uuid NOT NULL,
  object_ref kb_object_ref NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  object_state text NOT NULL DEFAULT 'available' CHECK (object_state='available'),
  media_type text NOT NULL CHECK (media_type IN ('font/ttf','font/otf','application/font-sfnt')),
  family text NOT NULL CHECK (octet_length(family) BETWEEN 1 AND 128),
  script text NOT NULL CHECK (script IN ('cjk','latin')),
  PRIMARY KEY(render_snapshot_id,ordinal),
  UNIQUE(render_snapshot_id,font_artifact_id),
  FOREIGN KEY(object_ref,content_sha256,media_type,object_state)
    REFERENCES object_registry(object_ref,digest,media_type,state),
  FOREIGN KEY(font_artifact_id,object_ref,content_sha256,media_type,family,script)
    REFERENCES bid_render_font_artifacts(id,object_ref,content_sha256,media_type,family,script),
  CHECK (object_ref='objects/'||content_sha256)
);

CREATE TABLE bid_render_snapshot_form_definition_items (
  render_snapshot_id uuid NOT NULL,
  project_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  ordinal integer NOT NULL CHECK (ordinal >= 0),
  form_definition_revision_id uuid NOT NULL,
  canonical_sha256 kb_sha256 NOT NULL,
  PRIMARY KEY(render_snapshot_id,ordinal),
  UNIQUE(render_snapshot_id,form_definition_revision_id),
  FOREIGN KEY(project_id,workspace_id,render_snapshot_id)
    REFERENCES bid_render_document_snapshot_artifacts(project_id,workspace_id,id),
  FOREIGN KEY(project_id,form_definition_revision_id,canonical_sha256)
    REFERENCES bid_tender_structured_form_definition_artifacts(project_id,id,content_sha256)
);

CREATE TABLE bid_render_snapshot_attachment_preparation_items (
  render_snapshot_id uuid NOT NULL,
  project_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  ordinal integer NOT NULL CHECK (ordinal >= 0),
  attachment_preparation_revision_id uuid NOT NULL,
  preparation_status text NOT NULL DEFAULT 'ready' CHECK (preparation_status='ready'),
  canonical_sha256 kb_sha256 NOT NULL,
  PRIMARY KEY(render_snapshot_id,ordinal),
  UNIQUE(render_snapshot_id,attachment_preparation_revision_id),
  FOREIGN KEY(project_id,workspace_id,render_snapshot_id)
    REFERENCES bid_render_document_snapshot_artifacts(project_id,workspace_id,id),
  FOREIGN KEY(project_id,workspace_id,attachment_preparation_revision_id,preparation_status,canonical_sha256)
    REFERENCES bid_attachment_preparation_revision_artifacts(project_id,workspace_id,id,status,preparation_sha256)
);

CREATE FUNCTION kb_bid_v2_validate_render_snapshot_payload()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE payload jsonb:=NEW.canonical_payload;
BEGIN
  IF jsonb_typeof(payload)<>'object' OR NOT payload ?& ARRAY[
    'schema_version','render_snapshot_id','project_id','workspace_id','workspace_scope',
    'workspace_scope_revision_id','workspace_revision_id','workspace_sha256',
    'outline_checkpoint_id','outline_checkpoint_sha256','requirement_projection_revision_id',
    'requirement_projection_sha256','document_settings_revision_id','document_settings_sha256',
    'submission_assessment_snapshot_id','submission_assessment_snapshot_sha256','output_mode','format',
    'mode_options','ordered_nodes','assets','form_definition_occurrences',
    'attachment_preparation_occurrences','content_block_schema_version','content_block_schema_sha256',
    'render_operation_contract_version','render_operation_contract_sha256','docx_renderer_contract_id',
    'docx_renderer_contract_sha256','pdf_renderer_contract_id','pdf_renderer_contract_sha256',
    'style_contract_id','style_contract_sha256','page_geometry','font_artifact_identities',
    'numbering_policy','toc_policy','snapshot_sha256'
  ] THEN RAISE EXCEPTION 'render snapshot canonical payload missing required keys' USING ERRCODE='23514'; END IF;
  IF (payload->>'schema_version')::smallint<>NEW.schema_version
     OR (payload->>'render_snapshot_id')::uuid<>NEW.id
     OR (payload->>'project_id')::uuid<>NEW.project_id
     OR (payload->>'workspace_id')::uuid<>NEW.workspace_id
     OR payload->>'workspace_scope'<>'project_wide'
     OR (payload->>'workspace_scope_revision_id')::uuid<>NEW.scope_revision_id
     OR (payload->>'workspace_revision_id')::uuid<>NEW.workspace_revision_id
     OR payload->>'workspace_sha256'<>NEW.workspace_sha256
     OR (payload->>'outline_checkpoint_id')::uuid<>NEW.outline_checkpoint_id
     OR payload->>'outline_checkpoint_sha256'<>NEW.outline_checkpoint_sha256
     OR (payload->>'requirement_projection_revision_id')::uuid<>NEW.requirement_projection_id
     OR payload->>'requirement_projection_sha256'<>NEW.requirement_projection_sha256
     OR (payload->>'document_settings_revision_id')::uuid<>NEW.document_settings_revision_id
     OR payload->>'document_settings_sha256'<>NEW.document_settings_sha256
     OR (payload->>'submission_assessment_snapshot_id')::uuid<>NEW.submission_assessment_snapshot_id
     OR payload->>'submission_assessment_snapshot_sha256'<>NEW.submission_assessment_snapshot_sha256
     OR payload->>'output_mode'<>NEW.output_mode OR payload->>'format'<>NEW.format
     OR payload->'mode_options'<>NEW.mode_options
     OR (payload->>'content_block_schema_version')::smallint<>NEW.content_block_schema_version
     OR payload->>'content_block_schema_sha256'<>NEW.content_block_schema_sha256
     OR (payload->>'render_operation_contract_version')::bigint<>NEW.render_operation_contract_version
     OR payload->>'render_operation_contract_sha256'<>NEW.render_operation_contract_sha256
     OR (payload->>'docx_renderer_contract_id')::uuid<>NEW.docx_renderer_contract_id
     OR payload->>'docx_renderer_contract_sha256'<>NEW.docx_renderer_contract_sha256
     OR (payload->>'pdf_renderer_contract_id')::uuid<>NEW.pdf_renderer_contract_id
     OR payload->>'pdf_renderer_contract_sha256'<>NEW.pdf_renderer_contract_sha256
     OR (payload->>'style_contract_id')::uuid<>NEW.style_contract_id
     OR payload->>'style_contract_sha256'<>NEW.style_contract_sha256
     OR payload->>'numbering_policy'<>NEW.numbering_policy OR payload->>'toc_policy'<>NEW.toc_policy
     OR payload->>'snapshot_sha256'<>NEW.content_sha256
  THEN RAISE EXCEPTION 'render snapshot canonical payload identity mismatch' USING ERRCODE='23514'; END IF;
  IF jsonb_typeof(payload->'ordered_nodes')<>'array' OR jsonb_typeof(payload->'assets')<>'array'
     OR jsonb_typeof(payload->'form_definition_occurrences')<>'array'
     OR jsonb_typeof(payload->'attachment_preparation_occurrences')<>'array'
     OR jsonb_typeof(payload->'font_artifact_identities')<>'array'
     OR jsonb_array_length(payload->'font_artifact_identities')<1
  THEN RAISE EXCEPTION 'render snapshot canonical collection invalid' USING ERRCODE='23514'; END IF;
  IF payload->'page_geometry'<>jsonb_build_object(
       'page_size',NEW.page_size,'width_mm',NEW.page_width_mm,'height_mm',NEW.page_height_mm,'margins_mm',NEW.margins_mm)
     OR NOT NEW.margins_mm ?& ARRAY['top','right','bottom','left']
     OR NEW.margins_mm - ARRAY['top','right','bottom','left']::text[]<>'{}'::jsonb
  THEN RAISE EXCEPTION 'render snapshot page geometry mismatch' USING ERRCODE='23514'; END IF;
  IF EXISTS (
    SELECT 1 FROM jsonb_array_elements(payload->'font_artifact_identities') font
    WHERE NOT font ?& ARRAY['font_artifact_id','object_ref','sha256','media_type','family','script']
       OR font->>'object_ref' <> ('objects/'||(font->>'sha256'))
       OR font->>'script' NOT IN ('cjk','latin')
       OR font->>'media_type' NOT IN ('font/ttf','font/otf','application/font-sfnt')
       OR NOT EXISTS (
          SELECT 1 FROM bid_render_font_artifacts font_artifact
          JOIN object_registry registry ON registry.object_ref=font_artifact.object_ref
            AND registry.digest=font_artifact.content_sha256 AND registry.state=font_artifact.object_state
          WHERE font_artifact.id=(font->>'font_artifact_id')::uuid
            AND font_artifact.object_ref=font->>'object_ref' AND font_artifact.content_sha256=font->>'sha256'
            AND font_artifact.media_type=font->>'media_type' AND font_artifact.family=font->>'family'
            AND font_artifact.script=font->>'script' AND registry.state='available')
  ) THEN RAISE EXCEPTION 'render snapshot font identity unavailable' USING ERRCODE='23514'; END IF;
  IF EXISTS (
    SELECT 1 FROM jsonb_array_elements(payload->'assets') asset
    WHERE NOT asset ?& ARRAY['asset_revision_id','object_ref','sha256','media_type','provenance']
       OR asset->>'object_ref' <> ('objects/'||(asset->>'sha256'))
       OR NOT EXISTS (SELECT 1 FROM object_registry registry
          WHERE registry.object_ref=asset->>'object_ref' AND registry.digest=asset->>'sha256'
            AND registry.media_type=asset->>'media_type' AND registry.state='available')
  ) THEN RAISE EXCEPTION 'render snapshot asset identity unavailable' USING ERRCODE='23514'; END IF;
  IF EXISTS (
    SELECT 1 FROM jsonb_array_elements(payload->'ordered_nodes') node
    WHERE NOT node ?& ARRAY['node_occurrence_id','node_revision_id','parent_occurrence_id','ordinal','depth','title','render_role','block_occurrences']
       OR jsonb_typeof(node->'block_occurrences')<>'array'
       OR NOT EXISTS (
         SELECT 1 FROM bid_workspace_node_occurrences occurrence
         JOIN bid_outline_node_revision_artifacts revision
           ON revision.project_id=occurrence.project_id AND revision.id=occurrence.node_revision_id
         WHERE occurrence.project_id=NEW.project_id AND occurrence.workspace_revision_id=NEW.workspace_revision_id
           AND occurrence.id=(node->>'node_occurrence_id')::uuid
           AND occurrence.node_revision_id=(node->>'node_revision_id')::uuid
           AND occurrence.parent_occurrence_id IS NOT DISTINCT FROM (node->>'parent_occurrence_id')::uuid
           AND occurrence.ordinal=(node->>'ordinal')::integer AND occurrence.depth=(node->>'depth')::integer
           AND revision.title=node->>'title' AND revision.render_role=node->>'render_role')
       OR EXISTS (
         SELECT 1 FROM jsonb_array_elements(node->'block_occurrences') block
         WHERE NOT block ?& ARRAY['block_occurrence_id','block_revision_id','ordinal','block_sha256']
            OR NOT EXISTS (
              SELECT 1 FROM bid_workspace_block_occurrences occurrence
              JOIN bid_content_block_revision_artifacts revision
                ON revision.project_id=occurrence.project_id AND revision.id=occurrence.block_revision_id
              WHERE occurrence.project_id=NEW.project_id AND occurrence.workspace_revision_id=NEW.workspace_revision_id
                AND occurrence.node_occurrence_id=(node->>'node_occurrence_id')::uuid
                AND occurrence.id=(block->>'block_occurrence_id')::uuid
                AND occurrence.block_revision_id=(block->>'block_revision_id')::uuid
                AND occurrence.ordinal=(block->>'ordinal')::integer
                AND revision.content_sha256=block->>'block_sha256'))
  ) THEN RAISE EXCEPTION 'render snapshot ordered occurrence mismatch' USING ERRCODE='23514'; END IF;
  IF EXISTS (
    SELECT 1 FROM jsonb_array_elements(payload->'form_definition_occurrences') item
    WHERE NOT EXISTS (SELECT 1 FROM bid_tender_structured_form_definition_artifacts form_value
      WHERE form_value.project_id=NEW.project_id
        AND form_value.id=(item->>'form_definition_revision_id')::uuid
        AND form_value.content_sha256=item->>'canonical_sha256')
  ) THEN RAISE EXCEPTION 'render snapshot form identity mismatch' USING ERRCODE='23514'; END IF;
  IF EXISTS (
    SELECT 1 FROM jsonb_array_elements(payload->'attachment_preparation_occurrences') item
    WHERE item->>'status'<>'ready' OR NOT EXISTS (
      SELECT 1 FROM bid_attachment_preparation_revision_artifacts preparation
      WHERE preparation.project_id=NEW.project_id AND preparation.workspace_id=NEW.workspace_id
        AND preparation.id=(item->>'attachment_preparation_revision_id')::uuid
        AND preparation.status='ready' AND preparation.preparation_sha256=item->>'canonical_sha256')
  ) THEN RAISE EXCEPTION 'render snapshot attachment preparation identity mismatch' USING ERRCODE='23514'; END IF;
  RETURN NEW;
EXCEPTION WHEN invalid_text_representation OR numeric_value_out_of_range THEN
  RAISE EXCEPTION 'render snapshot canonical payload has malformed identity' USING ERRCODE='23514';
END $$;
CREATE TRIGGER bid_render_document_snapshot_payload_valid
BEFORE INSERT OR UPDATE ON bid_render_document_snapshot_artifacts
FOR EACH ROW EXECUTE FUNCTION kb_bid_v2_validate_render_snapshot_payload();

-- RenderDocumentSnapshotV2 uses the same non-self-referential digest rule as
-- EvidenceBundleV1 and rejects closed-schema violations even on owner INSERT.
CREATE FUNCTION kb_bid_v2_validate_render_snapshot_strict()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE p jsonb:=NEW.canonical_payload; node jsonb; block jsonb; item jsonb; g jsonb; m jsonb;
BEGIN
 IF NOT kb_bid_v2_json_keys_exact(p,ARRAY[
  'schema_version','render_snapshot_id','project_id','project_title','workspace_id','workspace_scope','workspace_scope_revision_id',
  'workspace_revision_id','workspace_sha256','outline_checkpoint_id','outline_checkpoint_sha256',
  'requirement_projection_revision_id','requirement_projection_sha256','document_settings_revision_id','document_settings_sha256',
  'submission_assessment_snapshot_id','submission_assessment_snapshot_sha256','output_mode','format','mode_options','ordered_nodes',
  'assets','form_definition_occurrences','attachment_preparation_occurrences','content_block_schema_version','content_block_schema_sha256',
  'render_operation_contract_version','render_operation_contract_sha256','docx_renderer_contract_id','docx_renderer_contract_sha256',
  'pdf_renderer_contract_id','pdf_renderer_contract_sha256','style_contract_id','style_contract_sha256','page_geometry',
  'font_artifact_identities','numbering_policy','toc_policy','snapshot_sha256'])
 OR p->'schema_version' IS DISTINCT FROM '2'::jsonb OR p->'content_block_schema_version' IS DISTINCT FROM '1'::jsonb
 OR EXISTS (SELECT 1 FROM unnest(ARRAY['render_snapshot_id','project_id','workspace_id','workspace_scope_revision_id','workspace_revision_id','outline_checkpoint_id','requirement_projection_revision_id','document_settings_revision_id','submission_assessment_snapshot_id','docx_renderer_contract_id','pdf_renderer_contract_id','style_contract_id']::text[]) key WHERE NOT kb_bid_v2_uuid_text(p->>key))
 OR jsonb_typeof(p->'project_title') IS DISTINCT FROM 'string' OR octet_length(p->>'project_title') NOT BETWEEN 1 AND 1024
 OR jsonb_typeof(p->'workspace_scope') IS DISTINCT FROM 'string' OR p->>'workspace_scope' IS DISTINCT FROM 'project_wide'
 OR jsonb_typeof(p->'output_mode') IS DISTINCT FROM 'string' OR COALESCE(p->>'output_mode','') NOT IN ('preview','review_draft','submission')
 OR jsonb_typeof(p->'format') IS DISTINCT FROM 'string' OR COALESCE(p->>'format','') NOT IN ('html','docx','pdf')
 OR jsonb_typeof(p->'numbering_policy') IS DISTINCT FROM 'string' OR octet_length(p->>'numbering_policy') NOT BETWEEN 1 AND 128
 OR jsonb_typeof(p->'toc_policy') IS DISTINCT FROM 'string' OR octet_length(p->>'toc_policy') NOT BETWEEN 1 AND 128
 OR jsonb_typeof(p->'render_operation_contract_version') IS DISTINCT FROM 'number' OR COALESCE(p->>'render_operation_contract_version','') !~ '^[1-9][0-9]*$'
 OR jsonb_typeof(p->'snapshot_sha256')<>'string' OR NOT kb_bid_v2_sha256_text(p->>'snapshot_sha256')
 OR EXISTS (SELECT 1 FROM unnest(ARRAY['workspace_sha256','outline_checkpoint_sha256','requirement_projection_sha256','document_settings_sha256','submission_assessment_snapshot_sha256','content_block_schema_sha256','render_operation_contract_sha256','docx_renderer_contract_sha256','pdf_renderer_contract_sha256','style_contract_sha256']::text[]) key WHERE jsonb_typeof(p->key)<>'string' OR NOT kb_bid_v2_sha256_text(p->>key))
 OR p->>'snapshot_sha256'<>NEW.content_sha256
 OR NEW.content_sha256<>kb_bid_v2_sha256_bytes(convert_to((p-'snapshot_sha256')::text,'UTF8'))
 OR jsonb_typeof(p->'ordered_nodes') IS DISTINCT FROM 'array' OR jsonb_array_length(p->'ordered_nodes')>10000
 OR jsonb_typeof(p->'assets') IS DISTINCT FROM 'array' OR jsonb_array_length(p->'assets')>100000
 OR jsonb_typeof(p->'form_definition_occurrences') IS DISTINCT FROM 'array' OR jsonb_array_length(p->'form_definition_occurrences')>100000
 OR jsonb_typeof(p->'attachment_preparation_occurrences') IS DISTINCT FROM 'array' OR jsonb_array_length(p->'attachment_preparation_occurrences')>100000
 OR jsonb_typeof(p->'font_artifact_identities') IS DISTINCT FROM 'array' OR jsonb_array_length(p->'font_artifact_identities') NOT BETWEEN 1 AND 32
 OR NOT kb_bid_v2_json_keys_exact(p->'mode_options',ARRAY['watermark'])
 OR COALESCE(jsonb_typeof(p->'mode_options'->'watermark'),'missing') NOT IN ('string','null')
 OR (jsonb_typeof(p->'mode_options'->'watermark')='string' AND char_length(p->'mode_options'->>'watermark') NOT BETWEEN 1 AND 128)
 THEN RAISE EXCEPTION 'RenderDocumentSnapshotV2 closed root invalid' USING ERRCODE='23514'; END IF;
 g:=p->'page_geometry'; m:=g->'margins_mm';
 IF NOT kb_bid_v2_json_keys_exact(g,ARRAY['page_size','width_mm','height_mm','margins_mm'])
 OR g->>'page_size' IS DISTINCT FROM 'A4' OR g->'width_mm' IS DISTINCT FROM '210'::jsonb OR g->'height_mm' IS DISTINCT FROM '297'::jsonb
 OR NOT kb_bid_v2_json_keys_exact(m,ARRAY['top','right','bottom','left'])
 OR EXISTS (SELECT 1 FROM jsonb_each(m) value WHERE jsonb_typeof(value.value)<>'number' OR (value.value#>>'{}')::numeric NOT BETWEEN 5 AND 80)
 THEN RAISE EXCEPTION 'RenderDocumentSnapshotV2 page geometry invalid' USING ERRCODE='23514'; END IF;
 IF EXISTS (SELECT 1 FROM jsonb_array_elements(p->'ordered_nodes') x GROUP BY x->>'node_occurrence_id' HAVING count(*)<>1)
 OR EXISTS (SELECT 1 FROM jsonb_array_elements(p->'ordered_nodes') n,jsonb_array_elements(n->'block_occurrences') x GROUP BY x->>'block_occurrence_id' HAVING count(*)<>1)
 OR EXISTS (SELECT 1 FROM jsonb_array_elements(p->'assets') x GROUP BY x->>'asset_revision_id' HAVING count(*)<>1)
 OR EXISTS (SELECT 1 FROM jsonb_array_elements(p->'font_artifact_identities') x GROUP BY x->>'font_artifact_id' HAVING count(*)<>1)
 OR EXISTS (SELECT 1 FROM jsonb_array_elements(p->'form_definition_occurrences') x GROUP BY x->>'form_definition_revision_id' HAVING count(*)<>1)
 OR EXISTS (SELECT 1 FROM jsonb_array_elements(p->'attachment_preparation_occurrences') x GROUP BY x->>'attachment_preparation_revision_id' HAVING count(*)<>1)
 THEN RAISE EXCEPTION 'RenderDocumentSnapshotV2 duplicate occurrence identity' USING ERRCODE='23514'; END IF;
 FOR node IN SELECT value FROM jsonb_array_elements(p->'ordered_nodes') LOOP
  IF NOT kb_bid_v2_json_keys_exact(node,ARRAY['node_occurrence_id','node_revision_id','parent_occurrence_id','ordinal','depth','title','render_role','block_occurrences'])
   OR NOT kb_bid_v2_uuid_text(node->>'node_occurrence_id') OR NOT kb_bid_v2_uuid_text(node->>'node_revision_id')
   OR (jsonb_typeof(node->'parent_occurrence_id')<>'null' AND NOT kb_bid_v2_uuid_text(node->>'parent_occurrence_id'))
   OR jsonb_typeof(node->'ordinal')<>'number' OR jsonb_typeof(node->'depth')<>'number'
   OR node->>'ordinal' !~ '^(0|[1-9][0-9]*)$' OR node->>'depth' !~ '^(0|[1-9][0-9]*)$' OR (node->>'depth')::integer>32
   OR jsonb_typeof(node->'title') IS DISTINCT FROM 'string' OR octet_length(node->>'title') NOT BETWEEN 1 AND 1024
   OR jsonb_typeof(node->'render_role') IS DISTINCT FROM 'string' OR COALESCE(node->>'render_role','') NOT IN ('section','front_matter','toc','appendix','hidden')
   OR jsonb_typeof(node->'block_occurrences') IS DISTINCT FROM 'array' OR jsonb_array_length(node->'block_occurrences')>100000
  THEN RAISE EXCEPTION 'RenderDocumentSnapshotV2 node invalid' USING ERRCODE='23514'; END IF;
  FOR block IN SELECT value FROM jsonb_array_elements(node->'block_occurrences') LOOP
   IF NOT kb_bid_v2_json_keys_exact(block,ARRAY['block_occurrence_id','block_revision_id','ordinal','block_sha256'])
    OR NOT kb_bid_v2_uuid_text(block->>'block_occurrence_id') OR NOT kb_bid_v2_uuid_text(block->>'block_revision_id')
    OR jsonb_typeof(block->'ordinal')<>'number' OR block->>'ordinal' !~ '^(0|[1-9][0-9]*)$'
    OR jsonb_typeof(block->'block_sha256')<>'string' OR NOT kb_bid_v2_sha256_text(block->>'block_sha256')
   THEN RAISE EXCEPTION 'RenderDocumentSnapshotV2 block invalid' USING ERRCODE='23514'; END IF;
  END LOOP;
 END LOOP;
 FOR item IN SELECT value FROM jsonb_array_elements(p->'assets') LOOP
  IF NOT kb_bid_v2_json_keys_exact(item,ARRAY['asset_revision_id','object_ref','sha256','media_type','provenance'])
   OR NOT kb_bid_v2_uuid_text(item->>'asset_revision_id') OR jsonb_typeof(item->'sha256')<>'string' OR NOT kb_bid_v2_sha256_text(item->>'sha256')
   OR item->>'object_ref' IS DISTINCT FROM ('objects/'||(item->>'sha256')) OR jsonb_typeof(item->'media_type') IS DISTINCT FROM 'string'
   OR octet_length(item->>'media_type') NOT BETWEEN 1 AND 256
   OR jsonb_typeof(item->'provenance') IS DISTINCT FROM 'string' OR COALESCE(item->>'provenance','') NOT IN ('knowledge_evidence','manual_workspace','prepared_attachment','quote_snapshot')
  THEN RAISE EXCEPTION 'RenderDocumentSnapshotV2 asset invalid' USING ERRCODE='23514'; END IF;
 END LOOP;
 FOR item IN SELECT value FROM jsonb_array_elements(p->'font_artifact_identities') LOOP
  IF NOT kb_bid_v2_json_keys_exact(item,ARRAY['font_artifact_id','object_ref','sha256','media_type','family','script'])
   OR NOT kb_bid_v2_uuid_text(item->>'font_artifact_id') OR item->>'object_ref' IS DISTINCT FROM ('objects/'||(item->>'sha256'))
   OR jsonb_typeof(item->'sha256') IS DISTINCT FROM 'string' OR NOT kb_bid_v2_sha256_text(item->>'sha256')
   OR jsonb_typeof(item->'media_type') IS DISTINCT FROM 'string' OR COALESCE(item->>'media_type','') NOT IN ('font/ttf','font/otf','application/font-sfnt')
   OR jsonb_typeof(item->'family') IS DISTINCT FROM 'string' OR octet_length(item->>'family') NOT BETWEEN 1 AND 128
   OR jsonb_typeof(item->'script') IS DISTINCT FROM 'string' OR COALESCE(item->>'script','') NOT IN ('cjk','latin')
  THEN RAISE EXCEPTION 'RenderDocumentSnapshotV2 font invalid' USING ERRCODE='23514'; END IF;
 END LOOP;
 FOR item IN SELECT value FROM jsonb_array_elements(p->'form_definition_occurrences') LOOP
  IF NOT kb_bid_v2_json_keys_exact(item,ARRAY['form_definition_revision_id','canonical_sha256'])
   OR NOT kb_bid_v2_uuid_text(item->>'form_definition_revision_id') OR jsonb_typeof(item->'canonical_sha256')<>'string' OR NOT kb_bid_v2_sha256_text(item->>'canonical_sha256')
  THEN RAISE EXCEPTION 'RenderDocumentSnapshotV2 form invalid' USING ERRCODE='23514'; END IF;
 END LOOP;
 FOR item IN SELECT value FROM jsonb_array_elements(p->'attachment_preparation_occurrences') LOOP
  IF NOT kb_bid_v2_json_keys_exact(item,ARRAY['attachment_preparation_revision_id','status','canonical_sha256'])
   OR NOT kb_bid_v2_uuid_text(item->>'attachment_preparation_revision_id')
   OR jsonb_typeof(item->'status') IS DISTINCT FROM 'string' OR item->>'status' IS DISTINCT FROM 'ready'
   OR jsonb_typeof(item->'canonical_sha256') IS DISTINCT FROM 'string' OR NOT kb_bid_v2_sha256_text(item->>'canonical_sha256')
  THEN RAISE EXCEPTION 'RenderDocumentSnapshotV2 preparation invalid' USING ERRCODE='23514'; END IF;
 END LOOP;
 RETURN NEW;
EXCEPTION WHEN invalid_text_representation OR numeric_value_out_of_range THEN
 RAISE EXCEPTION 'RenderDocumentSnapshotV2 malformed scalar' USING ERRCODE='23514';
END $$;
CREATE TRIGGER bid_render_document_snapshot_strict_valid BEFORE INSERT OR UPDATE ON bid_render_document_snapshot_artifacts
FOR EACH ROW EXECUTE FUNCTION kb_bid_v2_validate_render_snapshot_strict();

CREATE FUNCTION kb_bid_v2_validate_render_asset_provenance()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE snap bid_render_document_snapshot_artifacts%ROWTYPE;
BEGIN
 SELECT * INTO STRICT snap FROM bid_render_document_snapshot_artifacts WHERE id=NEW.render_snapshot_id;
 IF NEW.provenance='knowledge_evidence' THEN
  PERFORM 1 FROM bid_evidence_asset_artifacts a WHERE a.id=NEW.asset_revision_id AND a.project_id=snap.project_id AND a.workspace_id=snap.workspace_id
   AND a.object_ref=NEW.object_ref AND a.content_sha256=NEW.content_sha256 AND a.media_type=NEW.media_type AND a.object_state=NEW.object_state;
 ELSIF NEW.provenance='manual_workspace' THEN
  PERFORM 1 FROM bid_workspace_asset_artifacts a WHERE a.id=NEW.asset_revision_id AND a.project_id=snap.project_id AND a.workspace_id=snap.workspace_id
   AND a.object_ref=NEW.object_ref AND a.content_sha256=NEW.content_sha256 AND a.media_type=NEW.media_type AND a.object_state=NEW.object_state;
 ELSIF NEW.provenance='prepared_attachment' THEN
  PERFORM 1 FROM bid_attachment_preparation_asset_items a WHERE a.id=NEW.asset_revision_id AND a.project_id=snap.project_id AND a.workspace_id=snap.workspace_id
   AND a.object_ref=NEW.object_ref AND a.content_sha256=NEW.content_sha256 AND a.media_type=NEW.media_type AND a.object_state=NEW.object_state;
 ELSIF NEW.provenance='quote_snapshot' THEN
  PERFORM 1 FROM bid_quote_snapshot_object_identities a WHERE a.quote_snapshot_id=NEW.asset_revision_id AND a.project_id=snap.project_id
   AND a.object_ref=NEW.object_ref AND a.content_sha256=NEW.content_sha256 AND a.media_type=NEW.media_type AND a.object_state=NEW.object_state;
 END IF;
 IF NOT FOUND THEN RAISE EXCEPTION 'render asset provenance identity mismatch' USING ERRCODE='23514'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER bid_render_snapshot_asset_provenance_valid BEFORE INSERT OR UPDATE ON bid_render_snapshot_asset_items
FOR EACH ROW EXECUTE FUNCTION kb_bid_v2_validate_render_asset_provenance();

CREATE FUNCTION kb_bid_v2_verify_render_snapshot_projection()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE sid uuid; p jsonb; actual jsonb;
BEGIN
 IF TG_TABLE_NAME='bid_render_document_snapshot_artifacts' THEN sid:=NEW.id; ELSE sid:=NEW.render_snapshot_id; END IF;
 SELECT canonical_payload INTO p FROM bid_render_document_snapshot_artifacts WHERE id=sid;
 IF p IS NULL THEN RETURN NULL; END IF;
 SELECT COALESCE(jsonb_agg(jsonb_build_object(
   'node_occurrence_id',n.node_occurrence_id,'node_revision_id',n.node_revision_id,'parent_occurrence_id',w.parent_occurrence_id,
   'ordinal',n.ordinal,'depth',w.depth,'title',r.title,'render_role',r.render_role,
   'block_occurrences',COALESCE((SELECT jsonb_agg(jsonb_build_object('block_occurrence_id',b.block_occurrence_id,'block_revision_id',b.block_revision_id,'ordinal',b.ordinal,'block_sha256',b.block_sha256) ORDER BY b.ordinal)
      FROM bid_render_snapshot_block_occurrences b WHERE b.render_snapshot_id=sid AND b.node_occurrence_id=n.node_occurrence_id),'[]'::jsonb)) ORDER BY w.depth,n.ordinal,n.node_occurrence_id),'[]'::jsonb)
 INTO actual FROM bid_render_snapshot_node_occurrences n
 JOIN bid_workspace_node_occurrences w ON w.project_id=n.project_id AND w.workspace_revision_id=n.workspace_revision_id AND w.id=n.node_occurrence_id
 JOIN bid_outline_node_revision_artifacts r ON r.project_id=n.project_id AND r.id=n.node_revision_id WHERE n.render_snapshot_id=sid;
 IF actual<>p->'ordered_nodes' THEN RAISE EXCEPTION 'render node/block projection mismatch' USING ERRCODE='23514'; END IF;
 SELECT COALESCE(jsonb_agg(jsonb_build_object('asset_revision_id',asset_revision_id,'object_ref',object_ref,'sha256',content_sha256,'media_type',media_type,'provenance',provenance) ORDER BY ordinal),'[]'::jsonb)
 INTO actual FROM bid_render_snapshot_asset_items WHERE render_snapshot_id=sid;
 IF actual<>p->'assets' THEN RAISE EXCEPTION 'render asset projection mismatch' USING ERRCODE='23514'; END IF;
 SELECT COALESCE(jsonb_agg(jsonb_build_object('font_artifact_id',font_artifact_id,'object_ref',object_ref,'sha256',content_sha256,'media_type',media_type,'family',family,'script',script) ORDER BY ordinal),'[]'::jsonb)
 INTO actual FROM bid_render_snapshot_font_items WHERE render_snapshot_id=sid;
 IF actual<>p->'font_artifact_identities' THEN RAISE EXCEPTION 'render font projection mismatch' USING ERRCODE='23514'; END IF;
 SELECT COALESCE(jsonb_agg(jsonb_build_object('form_definition_revision_id',form_definition_revision_id,'canonical_sha256',canonical_sha256) ORDER BY ordinal),'[]'::jsonb)
 INTO actual FROM bid_render_snapshot_form_definition_items WHERE render_snapshot_id=sid;
 IF actual<>p->'form_definition_occurrences' THEN RAISE EXCEPTION 'render form projection mismatch' USING ERRCODE='23514'; END IF;
 SELECT COALESCE(jsonb_agg(jsonb_build_object('attachment_preparation_revision_id',attachment_preparation_revision_id,'status',preparation_status,'canonical_sha256',canonical_sha256) ORDER BY ordinal),'[]'::jsonb)
 INTO actual FROM bid_render_snapshot_attachment_preparation_items WHERE render_snapshot_id=sid;
 IF actual<>p->'attachment_preparation_occurrences' THEN RAISE EXCEPTION 'render preparation projection mismatch' USING ERRCODE='23514'; END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER bid_render_snapshot_projection_complete AFTER INSERT ON bid_render_document_snapshot_artifacts DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION kb_bid_v2_verify_render_snapshot_projection();
CREATE CONSTRAINT TRIGGER bid_render_node_projection_complete AFTER INSERT ON bid_render_snapshot_node_occurrences DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION kb_bid_v2_verify_render_snapshot_projection();
CREATE CONSTRAINT TRIGGER bid_render_block_projection_complete AFTER INSERT ON bid_render_snapshot_block_occurrences DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION kb_bid_v2_verify_render_snapshot_projection();
CREATE CONSTRAINT TRIGGER bid_render_asset_projection_complete AFTER INSERT ON bid_render_snapshot_asset_items DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION kb_bid_v2_verify_render_snapshot_projection();
CREATE CONSTRAINT TRIGGER bid_render_font_projection_complete AFTER INSERT ON bid_render_snapshot_font_items DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION kb_bid_v2_verify_render_snapshot_projection();
CREATE CONSTRAINT TRIGGER bid_render_form_projection_complete AFTER INSERT ON bid_render_snapshot_form_definition_items DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION kb_bid_v2_verify_render_snapshot_projection();
CREATE CONSTRAINT TRIGGER bid_render_preparation_projection_complete AFTER INSERT ON bid_render_snapshot_attachment_preparation_items DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION kb_bid_v2_verify_render_snapshot_projection();

CREATE TABLE bid_submission_manifest_artifacts (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  request_artifact_id uuid NOT NULL UNIQUE,
  source jsonb NOT NULL CHECK (jsonb_typeof(source)='object'),
  canonical_payload bytea NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(project_id,id),
  UNIQUE(project_id,workspace_id,id),
  FOREIGN KEY(project_id,workspace_id) REFERENCES bid_submission_workspaces(project_id,id),
  CHECK (content_sha256=kb_bid_v2_sha256_bytes(canonical_payload)),
  CHECK (convert_from(canonical_payload,'UTF8')::jsonb->'source' IS NOT DISTINCT FROM source)
);

CREATE TABLE bid_submission_manifest_dependencies (
  manifest_id uuid NOT NULL REFERENCES bid_submission_manifest_artifacts(id),
  dependency_kind text NOT NULL CHECK (dependency_kind IN ('document_set','requirement_set','docx_round','docx_version')),
  dependency_id uuid NOT NULL,
  dependency_sha256 kb_sha256 NOT NULL,
  ordinal integer NOT NULL CHECK (ordinal >= 0),
  PRIMARY KEY(manifest_id,dependency_kind,dependency_id),
  UNIQUE(manifest_id,ordinal)
);

CREATE FUNCTION kb_bid_v2_manifest_expected_dependencies(p_manifest_id uuid)
RETURNS TABLE(dependency_kind text,dependency_id uuid,dependency_sha256 kb_sha256)
LANGUAGE sql STABLE SET search_path=pg_catalog,public AS $$
 SELECT item.kind,(m.source->>item.id_key)::uuid,(m.source->>item.sha_key)::kb_sha256
 FROM bid_submission_manifest_artifacts m CROSS JOIN (VALUES
   ('document_set','document_set_id','document_set_sha256'),
   ('requirement_set','requirement_set_id','requirement_set_sha256'),
   ('docx_round','round_id','round_sha256'),
   ('docx_version','version_id','docx_sha256')) item(kind,id_key,sha_key)
 WHERE m.id=p_manifest_id
$$;

CREATE FUNCTION kb_bid_v2_validate_manifest_dependency()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
BEGIN
 IF NOT EXISTS (SELECT 1 FROM kb_bid_v2_manifest_expected_dependencies(NEW.manifest_id) expected
  WHERE expected.dependency_kind=NEW.dependency_kind AND expected.dependency_id=NEW.dependency_id
    AND expected.dependency_sha256=NEW.dependency_sha256)
 THEN RAISE EXCEPTION 'manifest dependency identity is not in frozen snapshot' USING ERRCODE='23514'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER bid_submission_manifest_dependency_valid BEFORE INSERT OR UPDATE ON bid_submission_manifest_dependencies
FOR EACH ROW EXECUTE FUNCTION kb_bid_v2_validate_manifest_dependency();

CREATE FUNCTION kb_bid_v2_verify_manifest_dependency_set()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE mid uuid; expected_count integer; actual_count integer;
BEGIN
 IF TG_TABLE_NAME='bid_submission_manifest_artifacts' THEN mid:=NEW.id; ELSE mid:=NEW.manifest_id; END IF;
 SELECT count(*) INTO expected_count FROM kb_bid_v2_manifest_expected_dependencies(mid);
 SELECT count(*) INTO actual_count FROM bid_submission_manifest_dependencies WHERE manifest_id=mid;
 IF expected_count=0 OR actual_count<>expected_count
  OR EXISTS (SELECT dependency_kind,dependency_id,dependency_sha256 FROM kb_bid_v2_manifest_expected_dependencies(mid)
             EXCEPT SELECT dependency_kind,dependency_id,dependency_sha256 FROM bid_submission_manifest_dependencies WHERE manifest_id=mid)
  OR EXISTS (SELECT dependency_kind,dependency_id,dependency_sha256 FROM bid_submission_manifest_dependencies WHERE manifest_id=mid
             EXCEPT SELECT dependency_kind,dependency_id,dependency_sha256 FROM kb_bid_v2_manifest_expected_dependencies(mid))
  OR (SELECT COALESCE(array_agg(ordinal ORDER BY ordinal),'{}'::integer[]) FROM bid_submission_manifest_dependencies WHERE manifest_id=mid)
     <> COALESCE(ARRAY(SELECT generate_series(0,actual_count-1)),'{}'::integer[])
 THEN RAISE EXCEPTION 'manifest dependency set incomplete or divergent' USING ERRCODE='23514'; END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER bid_submission_manifest_dependency_set_complete AFTER INSERT ON bid_submission_manifest_artifacts
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION kb_bid_v2_verify_manifest_dependency_set();
CREATE CONSTRAINT TRIGGER bid_submission_manifest_dependency_row_complete AFTER INSERT ON bid_submission_manifest_dependencies
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION kb_bid_v2_verify_manifest_dependency_set();

CREATE TABLE bid_submission_output_artifacts (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  manifest_id uuid NOT NULL,
  format text NOT NULL CHECK (format IN ('docx','pdf')),
  object_ref kb_object_ref NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  media_type text NOT NULL,
  byte_length bigint NOT NULL CHECK (byte_length > 0),
  object_state text NOT NULL DEFAULT 'available' CHECK (object_state='available'),
  owner_kind text NOT NULL DEFAULT 'bid_submission_output' CHECK (owner_kind='bid_submission_output'),
  owner_id uuid NOT NULL,
  owner_occurrence text NOT NULL CHECK (octet_length(owner_occurrence) BETWEEN 1 AND 128),
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(project_id,id),
  UNIQUE(project_id,workspace_id,id),
  UNIQUE(project_id,workspace_id,manifest_id,id,format),
  UNIQUE(manifest_id,format),
  FOREIGN KEY(project_id,workspace_id) REFERENCES bid_submission_workspaces(project_id,id),
  FOREIGN KEY(project_id,workspace_id,manifest_id)
    REFERENCES bid_submission_manifest_artifacts(project_id,workspace_id,id),
  FOREIGN KEY(object_ref,content_sha256,media_type,byte_length,object_state)
    REFERENCES object_registry(object_ref,digest,media_type,byte_length,state),
  FOREIGN KEY(object_ref,owner_kind,owner_id,owner_occurrence)
    REFERENCES object_owner_references(object_ref,owner_kind,owner_id,occurrence),
  CHECK (object_ref='objects/'||content_sha256),
  CHECK (owner_id=id),
  CHECK (owner_occurrence='output:'||project_id::text||':'||workspace_id::text||':'||manifest_id::text),
  CHECK ((format='pdf' AND media_type='application/pdf') OR
         (format='docx' AND media_type='application/vnd.openxmlformats-officedocument.wordprocessingml.document'))
);

CREATE TABLE bid_submission_assessment_report_artifacts (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  manifest_id uuid NOT NULL UNIQUE,
  docx_output_id uuid NOT NULL UNIQUE,
  pdf_output_id uuid NOT NULL UNIQUE,
  docx_format text NOT NULL DEFAULT 'docx' CHECK(docx_format='docx'),
  pdf_format text NOT NULL DEFAULT 'pdf' CHECK(pdf_format='pdf'),
  canonical_payload bytea NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(project_id,id),
  FOREIGN KEY(project_id,workspace_id,manifest_id,docx_output_id,docx_format)
    REFERENCES bid_submission_output_artifacts(project_id,workspace_id,manifest_id,id,format),
  FOREIGN KEY(project_id,workspace_id,manifest_id,pdf_output_id,pdf_format)
    REFERENCES bid_submission_output_artifacts(project_id,workspace_id,manifest_id,id,format),
  CHECK(content_sha256=kb_bid_v2_sha256_bytes(canonical_payload))
);

-- Composite identities close every immutable lineage and pointer. These
-- constraints reject cross-document/workspace pairings that project-only FKs
-- cannot distinguish.
ALTER TABLE bid_document_role_revision_artifacts
  ADD UNIQUE(project_id,document_id,id),
  ADD UNIQUE(project_id,document_id,revision,id);
ALTER TABLE bid_document_role_current
  ADD FOREIGN KEY(project_id,scope_id,generation,artifact_id)
    REFERENCES bid_document_role_revision_artifacts(project_id,document_id,revision,id);
ALTER TABLE bid_document_relation_revision_artifacts
  ADD UNIQUE(project_id,relation_lineage_id,revision,id),
  ADD UNIQUE(project_id,id,content_sha256);
ALTER TABLE bid_document_relation_current
  ADD FOREIGN KEY(project_id,scope_id,generation,artifact_id)
    REFERENCES bid_document_relation_revision_artifacts(project_id,relation_lineage_id,revision,id);
ALTER TABLE bid_converted_source_artifacts
  ADD UNIQUE(project_id,document_id,id),
  ADD UNIQUE(project_id,document_id,revision,id);
ALTER TABLE bid_tender_source_image_revision_artifacts
  ADD FOREIGN KEY(project_id,document_id,source_revision_id)
    REFERENCES bid_converted_source_artifacts(project_id,document_id,id);
ALTER TABLE bid_document_set_artifacts
  ADD UNIQUE(project_id,revision,id),
  ADD UNIQUE(project_id,revision,id,content_sha256);
ALTER TABLE bid_document_set_items
  ADD FOREIGN KEY(project_id,document_id,role_revision_id)
    REFERENCES bid_document_role_revision_artifacts(project_id,document_id,id),
  ADD FOREIGN KEY(project_id,document_id,source_revision_id)
    REFERENCES bid_converted_source_artifacts(project_id,document_id,id);
ALTER TABLE bid_document_set_current
  ADD FOREIGN KEY(scope_id,generation,artifact_id,artifact_sha256)
    REFERENCES bid_document_set_artifacts(project_id,revision,id,content_sha256);
ALTER TABLE bid_source_unit_lineages
  ADD UNIQUE(project_id,document_id,id);
ALTER TABLE bid_source_unit_revision_artifacts
  ADD UNIQUE(project_id,document_id,id),
  ADD FOREIGN KEY(project_id,document_id,lineage_id)
    REFERENCES bid_source_unit_lineages(project_id,document_id,id),
  ADD FOREIGN KEY(project_id,document_id,source_revision_id)
    REFERENCES bid_converted_source_artifacts(project_id,document_id,id);
ALTER TABLE bid_source_unit_disposition_set_artifacts
  ADD UNIQUE(project_id,document_set_id,revision,id),
  ADD UNIQUE(project_id,document_set_id,revision,id,content_sha256),
  ADD FOREIGN KEY(project_id,document_set_sequence,document_set_id)
    REFERENCES bid_document_set_artifacts(project_id,revision,id);
ALTER TABLE bid_source_unit_disposition_set_current
  ADD FOREIGN KEY(project_id,document_set_id,generation,artifact_id,artifact_sha256)
    REFERENCES bid_source_unit_disposition_set_artifacts(project_id,document_set_id,revision,id,content_sha256);
ALTER TABLE bid_requirement_set_artifacts
  ADD UNIQUE(project_id,id,content_sha256),
  ADD UNIQUE(project_id,document_set_sequence,disposition_set_sequence,id,content_sha256),
  ADD UNIQUE(project_id,revision,id,content_sha256),
  ADD FOREIGN KEY(project_id,document_set_sequence,document_set_id)
    REFERENCES bid_document_set_artifacts(project_id,revision,id),
  ADD FOREIGN KEY(project_id,document_set_id,disposition_set_sequence,disposition_set_id)
    REFERENCES bid_source_unit_disposition_set_artifacts(project_id,document_set_id,revision,id);
ALTER TABLE bid_requirement_set_current
  ADD FOREIGN KEY(scope_id,document_set_sequence,disposition_set_sequence,artifact_id,artifact_sha256)
    REFERENCES bid_requirement_set_artifacts(project_id,document_set_sequence,disposition_set_sequence,id,content_sha256);
ALTER TABLE bid_requirement_supersession_revision_artifacts
  ADD UNIQUE(project_id,lineage_id,revision,id,content_sha256),
  ADD FOREIGN KEY(project_id,amendment_document_relation_revision_id,amendment_document_relation_sha256)
    REFERENCES bid_document_relation_revision_artifacts(project_id,id,content_sha256);
ALTER TABLE bid_requirement_supersession_current
  ADD FOREIGN KEY(project_id,scope_id,generation,artifact_id,artifact_sha256)
    REFERENCES bid_requirement_supersession_revision_artifacts(project_id,lineage_id,revision,id,content_sha256);
ALTER TABLE bid_workspace_scope_revision_artifacts
  ADD UNIQUE(project_id,workspace_id,id);
ALTER TABLE bid_document_settings_revision_artifacts
  ADD UNIQUE(project_id,workspace_id,id);
ALTER TABLE bid_workspace_requirement_projection_artifacts
  ADD UNIQUE(project_id,workspace_id,id),
  ADD UNIQUE(project_id,workspace_id,revision,id,content_sha256);
ALTER TABLE bid_workspace_requirement_projection_current
  ADD FOREIGN KEY(project_id,scope_id,generation,artifact_id,artifact_sha256)
    REFERENCES bid_workspace_requirement_projection_artifacts(project_id,workspace_id,revision,id,content_sha256);
ALTER TABLE bid_outline_fulfillment_binding_revision_artifacts
  ADD FOREIGN KEY(project_id,workspace_id,requirement_projection_id)
    REFERENCES bid_workspace_requirement_projection_artifacts(project_id,workspace_id,id);
ALTER TABLE bid_workspace_revision_artifacts
  ADD UNIQUE(project_id,workspace_id,id),
  ADD FOREIGN KEY(project_id,workspace_id,scope_revision_id)
    REFERENCES bid_workspace_scope_revision_artifacts(project_id,workspace_id,id),
  ADD FOREIGN KEY(project_id,workspace_id,requirement_projection_id)
    REFERENCES bid_workspace_requirement_projection_artifacts(project_id,workspace_id,id),
  ADD FOREIGN KEY(project_id,workspace_id,document_settings_revision_id)
    REFERENCES bid_document_settings_revision_artifacts(project_id,workspace_id,id);
ALTER TABLE bid_outline_assessment_snapshot_artifacts
  ADD FOREIGN KEY(project_id,workspace_id,workspace_revision_id)
    REFERENCES bid_workspace_revision_artifacts(project_id,workspace_id,id),
  ADD FOREIGN KEY(project_id,workspace_id,scope_revision_id)
    REFERENCES bid_workspace_scope_revision_artifacts(project_id,workspace_id,id),
  ADD FOREIGN KEY(project_id,workspace_id,requirement_projection_id)
    REFERENCES bid_workspace_requirement_projection_artifacts(project_id,workspace_id,id),
  ADD FOREIGN KEY(project_id,workspace_id,document_settings_revision_id)
    REFERENCES bid_document_settings_revision_artifacts(project_id,workspace_id,id);
ALTER TABLE bid_submission_assessment_snapshot_artifacts
  ADD FOREIGN KEY(project_id,workspace_id,workspace_revision_id)
    REFERENCES bid_workspace_revision_artifacts(project_id,workspace_id,id),
  ADD FOREIGN KEY(project_id,workspace_id,scope_revision_id)
    REFERENCES bid_workspace_scope_revision_artifacts(project_id,workspace_id,id),
  ADD FOREIGN KEY(project_id,workspace_id,requirement_projection_id)
    REFERENCES bid_workspace_requirement_projection_artifacts(project_id,workspace_id,id),
  ADD FOREIGN KEY(project_id,workspace_id,document_settings_revision_id)
    REFERENCES bid_document_settings_revision_artifacts(project_id,workspace_id,id);

-- Phase 0 request snapshots freeze typed identities for all five job kinds.
-- SHA-only agent schema fields are paired with stable contract artifact IDs so
-- no request can reinterpret a digest under another contract kind.
CREATE TABLE bid_authoring_contract_artifacts (
  id uuid PRIMARY KEY,
  contract_kind text NOT NULL CHECK (contract_kind IN ('converter','prompt','template','model','agent','matching_policy','vision_model','vision_operation')),
  schema_version smallint NOT NULL CHECK (schema_version=1),
  canonical_payload bytea NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(id,content_sha256),
  UNIQUE(contract_kind,id,content_sha256),
  CHECK (content_sha256=kb_bid_v2_sha256_bytes(canonical_payload))
);

ALTER TABLE bid_converted_source_artifacts
  ADD FOREIGN KEY(converter_contract_id,converter_contract_sha256)
    REFERENCES bid_authoring_contract_artifacts(id,content_sha256);
ALTER TABLE bid_tender_source_image_revision_artifacts
  ADD FOREIGN KEY(model_contract_id,model_contract_sha256)
    REFERENCES bid_authoring_contract_artifacts(id,content_sha256),
  ADD FOREIGN KEY(operation_contract_id,operation_contract_sha256)
    REFERENCES bid_authoring_contract_artifacts(id,content_sha256);

ALTER TABLE bid_documents
  ADD UNIQUE(project_id,id,original_sha256);
ALTER TABLE bid_document_role_revision_artifacts
  ADD UNIQUE(project_id,document_id,id,content_sha256);
ALTER TABLE bid_document_set_artifacts
  ADD UNIQUE(project_id,id,content_sha256);
ALTER TABLE bid_source_unit_disposition_set_artifacts
  ADD UNIQUE(project_id,id,content_sha256);
ALTER TABLE bid_requirement_set_artifacts
  ADD UNIQUE(project_id,id,content_sha256,document_set_id,disposition_set_id);
ALTER TABLE bid_workspace_requirement_projection_artifacts
  ADD UNIQUE(project_id,workspace_id,id,content_sha256,requirement_set_id);
ALTER TABLE bid_workspace_revision_artifacts
  ADD UNIQUE(project_id,workspace_id,id,scope_revision_id,requirement_projection_id);
ALTER TABLE bid_async_request_snapshot_artifacts
  ADD UNIQUE(id,project_id,request_kind,revision,request_sha256,frozen_input_sha256),
  ADD CHECK (CASE
    WHEN request_kind IN ('tender_document_process','requirement_set_compile') THEN workspace_id IS NULL
    WHEN request_kind IN ('content_generate','submission_export','docx_compose','docx_layout') THEN workspace_id IS NOT NULL
    ELSE false END);
ALTER TABLE bid_content_generation_request_identities
  ADD FOREIGN KEY(matching_policy_id,matching_policy_sha256)
    REFERENCES bid_authoring_contract_artifacts(id,content_sha256),
  ADD FOREIGN KEY(prompt_contract_id,prompt_contract_sha256)
    REFERENCES bid_authoring_contract_artifacts(id,content_sha256),
  ADD FOREIGN KEY(template_contract_id,template_contract_sha256)
    REFERENCES bid_authoring_contract_artifacts(id,content_sha256),
  ADD FOREIGN KEY(model_contract_id,model_contract_sha256)
    REFERENCES bid_authoring_contract_artifacts(id,content_sha256),
  ADD FOREIGN KEY(agent_contract_id,agent_contract_sha256)
    REFERENCES bid_authoring_contract_artifacts(id,content_sha256);

CREATE FUNCTION kb_bid_v2_validate_content_contract_kinds()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
BEGIN
  IF (NEW.request_operation='generate' AND (
       NOT EXISTS (SELECT 1 FROM bid_authoring_contract_artifacts WHERE id=NEW.prompt_contract_id AND content_sha256=NEW.prompt_contract_sha256 AND contract_kind='prompt')
       OR NOT EXISTS (SELECT 1 FROM bid_authoring_contract_artifacts WHERE id=NEW.template_contract_id AND content_sha256=NEW.template_contract_sha256 AND contract_kind='template')
       OR NOT EXISTS (SELECT 1 FROM bid_authoring_contract_artifacts WHERE id=NEW.model_contract_id AND content_sha256=NEW.model_contract_sha256 AND contract_kind='model')
       OR NOT EXISTS (SELECT 1 FROM bid_authoring_contract_artifacts WHERE id=NEW.agent_contract_id AND content_sha256=NEW.agent_contract_sha256 AND contract_kind='agent')))
     OR (NEW.evidence_selection_mode='system_proposed' AND NOT EXISTS (
       SELECT 1 FROM bid_authoring_contract_artifacts WHERE id=NEW.matching_policy_id AND content_sha256=NEW.matching_policy_sha256 AND contract_kind='matching_policy')) THEN
    RAISE EXCEPTION 'ContentGenerate contract identity kind mismatch' USING ERRCODE='23514';
  END IF;
  RETURN NEW;
END $$;
CREATE TRIGGER bid_content_generation_contract_kinds
BEFORE INSERT ON bid_content_generation_request_identities
FOR EACH ROW EXECUTE FUNCTION kb_bid_v2_validate_content_contract_kinds();

CREATE TABLE bid_tender_document_process_request_identities (
  request_artifact_id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  request_kind text NOT NULL DEFAULT 'tender_document_process' CHECK (request_kind='tender_document_process'),
  request_revision bigint NOT NULL CHECK (request_revision>0),
  request_sha256 kb_sha256 NOT NULL,
  frozen_input_sha256 kb_sha256 NOT NULL,
  document_id uuid NOT NULL,
  document_sha256 kb_sha256 NOT NULL,
  role_revision_id uuid NOT NULL,
  role_revision_sha256 kb_sha256 NOT NULL,
  converter_contract_id uuid NOT NULL,
  converter_contract_sha256 kb_sha256 NOT NULL,
  UNIQUE(request_artifact_id,request_kind,project_id,request_revision,request_sha256),
  FOREIGN KEY(request_artifact_id,project_id,request_kind,request_revision,request_sha256,frozen_input_sha256)
    REFERENCES bid_async_request_snapshot_artifacts(id,project_id,request_kind,revision,request_sha256,frozen_input_sha256),
  FOREIGN KEY(project_id,document_id,document_sha256)
    REFERENCES bid_documents(project_id,id,original_sha256),
  FOREIGN KEY(project_id,document_id,role_revision_id,role_revision_sha256)
    REFERENCES bid_document_role_revision_artifacts(project_id,document_id,id,content_sha256),
  FOREIGN KEY(converter_contract_id,converter_contract_sha256)
    REFERENCES bid_authoring_contract_artifacts(id,content_sha256)
);

CREATE TABLE bid_requirement_set_compile_request_identities (
  request_artifact_id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  request_kind text NOT NULL DEFAULT 'requirement_set_compile' CHECK (request_kind='requirement_set_compile'),
  request_revision bigint NOT NULL CHECK (request_revision>0),
  request_sha256 kb_sha256 NOT NULL,
  agent_runtime jsonb CHECK (agent_runtime IS NULL OR jsonb_typeof(agent_runtime)='object'),
  frozen_input_sha256 kb_sha256 NOT NULL,
  document_set_revision_id uuid NOT NULL,
  document_set_sha256 kb_sha256 NOT NULL,
  disposition_set_revision_id uuid NOT NULL,
  disposition_set_sha256 kb_sha256 NOT NULL,
  UNIQUE(request_artifact_id,request_kind,project_id,request_revision,request_sha256),
  UNIQUE(request_artifact_id,project_id,document_set_revision_id,request_revision,frozen_input_sha256),
  FOREIGN KEY(request_artifact_id,project_id,request_kind,request_revision,request_sha256,frozen_input_sha256)
    REFERENCES bid_async_request_snapshot_artifacts(id,project_id,request_kind,revision,request_sha256,frozen_input_sha256),
  FOREIGN KEY(project_id,document_set_revision_id,document_set_sha256)
    REFERENCES bid_document_set_artifacts(project_id,id,content_sha256),
  FOREIGN KEY(project_id,disposition_set_revision_id,disposition_set_sha256)
    REFERENCES bid_source_unit_disposition_set_artifacts(project_id,id,content_sha256),
  FOREIGN KEY(project_id,disposition_set_revision_id,document_set_revision_id)
    REFERENCES bid_source_unit_disposition_set_artifacts(project_id,id,document_set_id)
);


CREATE FUNCTION kb_bid_v2_jcs_number(p_text text)
RETURNS text LANGUAGE plpgsql IMMUTABLE STRICT PARALLEL SAFE SET search_path=pg_catalog,public AS $$
DECLARE negative boolean:=left(p_text,1)='-'; unsigned_value text;
  mantissa text; exponent_part integer:=0; dot_position integer; fractional_count integer:=0;
  digits text; decimal_position integer; scientific_exponent integer; result_value text;
BEGIN
  unsigned_value:=CASE WHEN negative THEN substring(p_text FROM 2) ELSE p_text END;
  IF position('e' IN lower(unsigned_value))>0 THEN
    mantissa:=split_part(lower(unsigned_value),'e',1);
    exponent_part:=split_part(lower(unsigned_value),'e',2)::integer;
  ELSE mantissa:=unsigned_value; END IF;
  dot_position:=position('.' IN mantissa);
  IF dot_position>0 THEN fractional_count:=length(mantissa)-dot_position; END IF;
  digits:=replace(mantissa,'.','');
  digits:=regexp_replace(digits,'^0+','');
  IF digits='' THEN RETURN '0'; END IF;
  WHILE right(digits,1)='0' LOOP
    digits:=left(digits,length(digits)-1); exponent_part:=exponent_part+1;
  END LOOP;
  decimal_position:=length(digits)+exponent_part-fractional_count;
  scientific_exponent:=decimal_position-1;
  IF scientific_exponent>=-6 AND scientific_exponent<21 THEN
    IF decimal_position<=0 THEN result_value:='0.'||repeat('0',-decimal_position)||digits;
    ELSIF decimal_position>=length(digits) THEN result_value:=digits||repeat('0',decimal_position-length(digits));
    ELSE result_value:=left(digits,decimal_position)||'.'||substring(digits FROM decimal_position+1); END IF;
  ELSE
    result_value:=left(digits,1)||CASE WHEN length(digits)>1 THEN '.'||substring(digits FROM 2) ELSE '' END
      ||'e'||CASE WHEN scientific_exponent>=0 THEN '+' ELSE '' END||scientific_exponent::text;
  END IF;
  RETURN CASE WHEN negative THEN '-' ELSE '' END||result_value;
END $$;

CREATE FUNCTION kb_bid_v2_jcs(p_value jsonb)
RETURNS text LANGUAGE plpgsql IMMUTABLE STRICT PARALLEL SAFE SET search_path=pg_catalog,public AS $$
DECLARE kind text; result_value text;
BEGIN
  kind:=jsonb_typeof(p_value);
  IF kind='null' THEN RETURN 'null';
  ELSIF kind='boolean' THEN RETURN CASE WHEN p_value='true'::jsonb THEN 'true' ELSE 'false' END;
  ELSIF kind='string' THEN RETURN to_jsonb(p_value#>>'{}')::text;
  ELSIF kind='number' THEN RETURN kb_bid_v2_jcs_number(p_value#>>'{}');
  ELSIF kind='array' THEN
    SELECT '['||coalesce(string_agg(kb_bid_v2_jcs(item),',' ORDER BY item_ordinal),'')||']'
      INTO result_value FROM jsonb_array_elements(p_value) WITH ORDINALITY value(item,item_ordinal);
    RETURN result_value;
  ELSIF kind='object' THEN
    SELECT '{'||coalesce(string_agg(kb_bid_v2_jcs(to_jsonb(key))||':'||kb_bid_v2_jcs(value),',' ORDER BY key COLLATE "C"),'')||'}'
      INTO result_value FROM jsonb_each(p_value);
    RETURN result_value;
  END IF;
  RAISE EXCEPTION 'JCS_VALUE_UNSUPPORTED' USING ERRCODE='22023';
END $$;

CREATE TABLE bid_docx_composition_request_identities (
  request_artifact_id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  request_kind text NOT NULL DEFAULT 'docx_compose' CHECK (request_kind='docx_compose'),
  request_revision bigint NOT NULL CHECK (request_revision>0),
  request_sha256 kb_sha256 NOT NULL,
  frozen_input_sha256 kb_sha256 NOT NULL,
  document_set_id uuid NOT NULL,
  document_set_sha256 kb_sha256 NOT NULL,
  requirement_set_id uuid NOT NULL,
  requirement_set_sha256 kb_sha256 NOT NULL,
  source_request_id uuid NOT NULL,
  source_request_revision bigint NOT NULL,
  source_request_sha256 kb_sha256 NOT NULL,
  expected_round_id uuid,
  expected_version_id uuid,
  expected_docx_sha256 kb_sha256,
  actor kb_actor_identity NOT NULL,
  frozen_input jsonb NOT NULL,
  contract_definition jsonb NOT NULL,
  contract_sha256 kb_sha256 NOT NULL,
  CHECK ((expected_round_id IS NULL)=(expected_version_id IS NULL)
    AND (expected_version_id IS NULL)=(expected_docx_sha256 IS NULL)),
  CHECK (frozen_input_sha256=kb_bid_v2_sha256_bytes(convert_to(kb_bid_v2_jcs(frozen_input),'UTF8'))),
  CHECK (contract_sha256=kb_bid_v2_sha256_bytes(convert_to(kb_bid_v2_jcs(contract_definition),'UTF8'))),
  CHECK (frozen_input->>'contract_sha256' IS NOT DISTINCT FROM contract_sha256::text
    AND frozen_input->'config' IS NOT DISTINCT FROM contract_definition->'config'),
  CHECK (frozen_input->>'workspace_id' IS NOT DISTINCT FROM workspace_id::text
    AND frozen_input->>'actor' IS NOT DISTINCT FROM actor::text),
  CHECK (jsonb_typeof(frozen_input) IS NOT DISTINCT FROM 'object'
    AND kb_bid_v2_json_keys_exact(frozen_input,ARRAY['schema_version','workspace_id','actor','basis','expected',
      'source_request','source_input_sha256','analysis_sha256','config','contract_sha256'])
    AND frozen_input->'schema_version' IS NOT DISTINCT FROM '1'::jsonb),
  CHECK (frozen_input->'basis' IS NOT DISTINCT FROM jsonb_build_object('document_set_id',document_set_id,
    'document_set_sha256',document_set_sha256,'requirement_set_id',requirement_set_id,'requirement_set_sha256',requirement_set_sha256)),
  CHECK (frozen_input->'source_request' IS NOT DISTINCT FROM jsonb_build_object('request_artifact_id',source_request_id,
    'request_revision',source_request_revision,'frozen_input_sha256',source_request_sha256)),
  CHECK (frozen_input->'expected' IS NOT DISTINCT FROM CASE WHEN expected_version_id IS NULL THEN 'null'::jsonb
    ELSE jsonb_build_object('version_id',expected_version_id,'docx_sha256',expected_docx_sha256) END),
  CHECK (jsonb_typeof(contract_definition) IS NOT DISTINCT FROM 'object'
    AND kb_bid_v2_json_keys_exact(contract_definition,ARRAY['checkpoint_contract_version','runtime_adapter','config','main','reviewer','tools','review_tools'])
    AND contract_definition->'checkpoint_contract_version' IS NOT DISTINCT FROM '3'::jsonb
    AND contract_definition->>'runtime_adapter' IS NOT DISTINCT FROM 'rig-chat-0.42.0/4'
    AND jsonb_typeof(contract_definition->'config') IS NOT DISTINCT FROM 'object'
    AND jsonb_typeof(contract_definition->'main') IS NOT DISTINCT FROM 'array'
    AND jsonb_typeof(contract_definition->'reviewer') IS NOT DISTINCT FROM 'array'
    AND jsonb_typeof(contract_definition->'tools') IS NOT DISTINCT FROM 'array'
    AND jsonb_typeof(contract_definition->'review_tools') IS NOT DISTINCT FROM 'array'),
  FOREIGN KEY(request_artifact_id,project_id,workspace_id,request_kind,request_revision,request_sha256,frozen_input_sha256)
    REFERENCES bid_async_request_snapshot_artifacts(id,project_id,workspace_id,request_kind,revision,request_sha256,frozen_input_sha256),
  FOREIGN KEY(project_id,workspace_id) REFERENCES bid_submission_workspaces(project_id,id),
  FOREIGN KEY(project_id,document_set_id,document_set_sha256)
    REFERENCES bid_document_set_artifacts(project_id,id,content_sha256),
  FOREIGN KEY(project_id,requirement_set_id,requirement_set_sha256)
    REFERENCES bid_requirement_set_artifacts(project_id,id,content_sha256),
  FOREIGN KEY(source_request_id,project_id,document_set_id,source_request_revision,source_request_sha256)
    REFERENCES bid_requirement_set_compile_request_identities(request_artifact_id,project_id,document_set_revision_id,request_revision,frozen_input_sha256)
);

CREATE TABLE bid_submission_export_request_identities (
  request_artifact_id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  request_kind text NOT NULL DEFAULT 'submission_export' CHECK (request_kind IN ('submission_export','docx_layout')),
  request_revision bigint NOT NULL CHECK (request_revision>0),
  request_sha256 kb_sha256 NOT NULL,
  frozen_input_sha256 kb_sha256 NOT NULL,
  round_id uuid NOT NULL,
  version_id uuid NOT NULL,
  docx_sha256 kb_sha256 NOT NULL,
  source jsonb NOT NULL CHECK (jsonb_typeof(source)='object'),
  UNIQUE(request_artifact_id,request_kind,project_id,workspace_id,request_revision,request_sha256),
  UNIQUE(request_artifact_id,project_id,workspace_id,request_revision,frozen_input_sha256),
  UNIQUE(project_id,workspace_id,request_artifact_id,source),
  FOREIGN KEY(request_artifact_id,project_id,workspace_id,request_kind,request_revision,request_sha256,frozen_input_sha256)
    REFERENCES bid_async_request_snapshot_artifacts(id,project_id,workspace_id,request_kind,revision,request_sha256,frozen_input_sha256),
  FOREIGN KEY(project_id,workspace_id) REFERENCES bid_submission_workspaces(project_id,id),
  CHECK(source->>'round_id' IS NOT DISTINCT FROM round_id::text
    AND source->>'version_id' IS NOT DISTINCT FROM version_id::text
    AND source->>'docx_sha256' IS NOT DISTINCT FROM docx_sha256::text),
  frozen_context jsonb CHECK (frozen_context IS NULL OR jsonb_typeof(frozen_context)='object'),
  CHECK(
    (frozen_context IS NULL AND frozen_input_sha256=kb_bid_v2_sha256_bytes(convert_to(source::text,'UTF8')))
    OR (frozen_context IS NOT NULL
      AND kb_bid_v2_json_keys_exact(frozen_context,ARRAY['analysis_identity','execution_contract','layout_result'])
      AND frozen_input_sha256=kb_bid_v2_sha256_bytes(convert_to(kb_bid_v2_jcs(jsonb_build_object('source',source,'context',frozen_context)),'UTF8')))
  )
);
ALTER TABLE bid_submission_manifest_artifacts
  ADD FOREIGN KEY(project_id,workspace_id,request_artifact_id,source)
    REFERENCES bid_submission_export_request_identities(project_id,workspace_id,request_artifact_id,source);

CREATE FUNCTION kb_bid_v2_validate_request_contract_kinds()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
BEGIN
  IF TG_TABLE_NAME='bid_tender_document_process_request_identities' THEN
    IF NOT EXISTS (SELECT 1 FROM bid_authoring_contract_artifacts WHERE id=NEW.converter_contract_id AND content_sha256=NEW.converter_contract_sha256 AND contract_kind='converter') THEN
      RAISE EXCEPTION 'TenderDocumentProcess converter contract kind mismatch' USING ERRCODE='23514';
    END IF;
  END IF;
  RETURN NEW;
END $$;
CREATE TRIGGER bid_tender_document_process_contract_kind
BEFORE INSERT ON bid_tender_document_process_request_identities
FOR EACH ROW EXECUTE FUNCTION kb_bid_v2_validate_request_contract_kinds();

CREATE FUNCTION kb_bid_v2_verify_request_typed_projection()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE projection_count integer;
BEGIN
  SELECT
    (SELECT count(*) FROM bid_tender_document_process_request_identities WHERE request_artifact_id=NEW.id)+
    (SELECT count(*) FROM bid_requirement_set_compile_request_identities WHERE request_artifact_id=NEW.id)+
    (SELECT count(*) FROM bid_docx_composition_request_identities WHERE request_artifact_id=NEW.id)+
    (SELECT count(*) FROM bid_content_generation_request_identities WHERE request_artifact_id=NEW.id)+
    (SELECT count(*) FROM bid_submission_export_request_identities WHERE request_artifact_id=NEW.id)
  INTO projection_count;
  IF projection_count<>1
     OR (NEW.request_kind='tender_document_process' AND NOT EXISTS (SELECT 1 FROM bid_tender_document_process_request_identities WHERE request_artifact_id=NEW.id))
     OR (NEW.request_kind='requirement_set_compile' AND NOT EXISTS (SELECT 1 FROM bid_requirement_set_compile_request_identities WHERE request_artifact_id=NEW.id))
     OR (NEW.request_kind='docx_compose' AND NOT EXISTS (SELECT 1 FROM bid_docx_composition_request_identities WHERE request_artifact_id=NEW.id))
     OR (NEW.request_kind='content_generate' AND NOT EXISTS (SELECT 1 FROM bid_content_generation_request_identities WHERE request_artifact_id=NEW.id))
     OR (NEW.request_kind IN ('submission_export','docx_layout') AND NOT EXISTS (SELECT 1 FROM bid_submission_export_request_identities WHERE request_artifact_id=NEW.id)) THEN
    RAISE EXCEPTION 'async request must have exactly one matching typed projection' USING ERRCODE='23514';
  END IF;
  RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER bid_async_request_typed_projection_complete
AFTER INSERT ON bid_async_request_snapshot_artifacts
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION kb_bid_v2_verify_request_typed_projection();

CREATE FUNCTION kb_bid_v2_guard_async_request_initial_state()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
BEGIN
  IF NEW.status IS DISTINCT FROM 'pending' THEN
    RAISE EXCEPTION 'async request initial status must be pending' USING ERRCODE='23514';
  END IF;
  RETURN NEW;
END $$;
CREATE TRIGGER bid_async_request_initial_state_guard
BEFORE INSERT ON bid_async_request_snapshot_artifacts
FOR EACH ROW EXECUTE FUNCTION kb_bid_v2_guard_async_request_initial_state();

CREATE FUNCTION kb_bid_v2_guard_async_request_transition()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
BEGIN
  IF TG_OP='DELETE' THEN
    RAISE EXCEPTION 'async request snapshots cannot be deleted' USING ERRCODE='42501';
  END IF;
  IF (to_jsonb(NEW)-ARRAY['status','result_identity','error_code','finished_at','current_attempt'])
       IS DISTINCT FROM (to_jsonb(OLD)-ARRAY['status','result_identity','error_code','finished_at','current_attempt']) THEN
    RAISE EXCEPTION 'async request frozen identity cannot change' USING ERRCODE='42501';
  END IF;
  IF OLD.status='pending' AND NEW.status='pending'
     AND NEW.current_attempt=OLD.current_attempt+1
     AND NEW.result_identity IS NOT DISTINCT FROM OLD.result_identity
     AND NEW.error_code IS NOT DISTINCT FROM OLD.error_code
     AND NEW.finished_at IS NOT DISTINCT FROM OLD.finished_at THEN
    RETURN NEW;
  END IF;
  IF OLD.status<>'pending' OR NEW.status NOT IN ('succeeded','failed')
     OR NEW.current_attempt<>OLD.current_attempt THEN
    RAISE EXCEPTION 'invalid async request status transition' USING ERRCODE='23514';
  END IF;
  RETURN NEW;
END $$;
CREATE TRIGGER bid_async_request_transition_guard
BEFORE UPDATE OR DELETE ON bid_async_request_snapshot_artifacts
FOR EACH ROW EXECUTE FUNCTION kb_bid_v2_guard_async_request_transition();
CREATE TRIGGER bid_async_request_no_truncate
BEFORE TRUNCATE ON bid_async_request_snapshot_artifacts
FOR EACH STATEMENT EXECUTE FUNCTION kb_reject_append_only();

ALTER TABLE bid_candidate_artifacts
  DROP CONSTRAINT bid_content_candidate_request_identity_fk;
CREATE FUNCTION kb_bid_v2_validate_candidate_request_identity()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
BEGIN
    PERFORM 1 FROM bid_content_generation_request_identities request
     WHERE request.request_artifact_id=NEW.request_artifact_id
       AND request.request_kind='content_generate' AND request.request_operation='generate'
       AND request.project_id=NEW.project_id AND request.workspace_id=NEW.workspace_id
       AND request.request_revision=NEW.request_revision AND request.request_sha256=NEW.request_sha256
       AND request.base_workspace_revision_id=NEW.base_workspace_revision_id
       AND request.base_workspace_sha256=NEW.base_workspace_sha256;
  IF NOT FOUND THEN
    RAISE EXCEPTION 'candidate does not match its typed request/base identity' USING ERRCODE='23503';
  END IF;
  RETURN NEW;
END $$;
CREATE TRIGGER bid_candidate_typed_request_identity
BEFORE INSERT ON bid_candidate_artifacts
FOR EACH ROW EXECUTE FUNCTION kb_bid_v2_validate_candidate_request_identity();

CREATE FUNCTION kb_bid_v2_guard_candidate_initial_state()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
BEGIN
  IF NEW.state IS DISTINCT FROM 'proposed' OR NEW.decided_at IS NOT NULL THEN
    RAISE EXCEPTION 'candidate initial state must be proposed and undecided' USING ERRCODE='23514';
  END IF;
  RETURN NEW;
END $$;
CREATE TRIGGER bid_candidate_initial_state_guard
BEFORE INSERT ON bid_candidate_artifacts
FOR EACH ROW EXECUTE FUNCTION kb_bid_v2_guard_candidate_initial_state();

CREATE FUNCTION kb_bid_v2_guard_candidate_transition()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
BEGIN
  IF TG_OP='DELETE' THEN
    RAISE EXCEPTION 'candidates cannot be deleted' USING ERRCODE='42501';
  END IF;
  IF (to_jsonb(NEW)-ARRAY['state','decided_at','canonical_payload_sha256']) IS DISTINCT FROM (to_jsonb(OLD)-ARRAY['state','decided_at','canonical_payload_sha256']) THEN
    RAISE EXCEPTION 'candidate frozen identity cannot change' USING ERRCODE='42501';
  END IF;
  IF OLD.state<>'proposed' OR NEW.state NOT IN ('accepted','rejected') THEN
    RAISE EXCEPTION 'invalid candidate decision transition' USING ERRCODE='23514';
  END IF;
  RETURN NEW;
END $$;
CREATE TRIGGER bid_candidate_transition_guard
BEFORE UPDATE OR DELETE ON bid_candidate_artifacts
FOR EACH ROW EXECUTE FUNCTION kb_bid_v2_guard_candidate_transition();
CREATE TRIGGER bid_candidate_no_truncate
BEFORE TRUNCATE ON bid_candidate_artifacts
FOR EACH STATEMENT EXECUTE FUNCTION kb_reject_append_only();

-- All immutable artifacts reject update/delete/truncate. Current pointers and
-- business request status rows are deliberately excluded.
DO $$
DECLARE relation_name text;
BEGIN
  FOREACH relation_name IN ARRAY ARRAY[
    'bid_document_role_revision_artifacts','bid_document_relation_revision_artifacts',
    'bid_converted_source_artifacts','bid_document_set_artifacts','bid_source_unit_revision_artifacts','bid_tender_source_image_revision_artifacts',
    'bid_source_unit_disposition_set_artifacts','bid_tender_structured_form_definition_artifacts',
    'bid_requirement_set_artifacts','bid_requirement_revision_artifacts','bid_requirement_source_revision_artifacts',
    'bid_requirement_supersession_revision_artifacts','bid_workspace_scope_revision_artifacts',
    'bid_workspace_requirement_projection_artifacts','bid_document_settings_revision_artifacts',
    'bid_outline_node_revision_artifacts','bid_content_block_revision_artifacts',
    'bid_outline_fulfillment_binding_revision_artifacts','bid_workspace_revision_artifacts',
    'bid_outline_checkpoint_artifacts','bid_evidence_match_reports','bid_evidence_bundle_artifacts',
    'bid_evidence_selection_artifacts','bid_evidence_asset_artifacts','bid_workspace_asset_artifacts','bid_workspace_asset_retirement_artifacts',
    'bid_submission_fulfillment_evidence_revision_artifacts','bid_outline_assessment_snapshot_artifacts',
    'bid_submission_assessment_snapshot_artifacts','bid_submission_assessment_snapshot_evidence_items',
    'bid_quote_snapshot_artifacts','bid_quote_snapshot_object_identities',
    'bid_render_style_contract_artifacts','bid_authoring_contract_artifacts','bid_renderer_contract_artifacts','bid_render_font_artifacts','bid_attachment_preparation_revision_artifacts','bid_attachment_preparation_asset_items',
    'bid_attachment_preparation_contract_artifacts',
    'bid_render_document_snapshot_artifacts','bid_submission_manifest_artifacts','bid_submission_output_artifacts','bid_submission_assessment_report_artifacts',
    'bid_document_set_items','bid_source_unit_disposition_set_items','bid_requirement_set_items',
    'bid_workspace_requirement_projection_items','bid_workspace_node_occurrences','bid_workspace_block_occurrences',
    'bid_workspace_binding_occurrences','bid_outline_lineage_edges','bid_async_stage_receipts',
    'bid_tender_document_process_request_identities','bid_requirement_set_compile_request_identities',
    'bid_content_generation_request_identities','bid_docx_composition_request_identities',
    'bid_submission_export_request_identities','bid_content_generation_request_evidence_bundles',
    'bid_candidate_operations',
    'bid_candidate_decision_receipts','bid_evidence_bundle_items',
    'bid_render_snapshot_node_occurrences','bid_render_snapshot_block_occurrences',
    'bid_render_snapshot_asset_items','bid_render_snapshot_font_items','bid_render_snapshot_form_definition_items',
    'bid_render_snapshot_attachment_preparation_items','bid_submission_manifest_dependencies'
  ] LOOP
    EXECUTE format('CREATE TRIGGER %I_immutable BEFORE UPDATE OR DELETE ON %I FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only()', relation_name, relation_name);
    EXECUTE format('CREATE TRIGGER %I_no_truncate BEFORE TRUNCATE ON %I FOR EACH STATEMENT EXECUTE FUNCTION kb_reject_append_only()', relation_name, relation_name);
  END LOOP;
END
$$;

DO $$
DECLARE relation_name text;
BEGIN
  FOREACH relation_name IN ARRAY ARRAY[
    'bid_document_role_current','bid_document_relation_current','bid_document_set_current',
    'bid_source_unit_disposition_set_current','bid_requirement_set_current',
    'bid_requirement_supersession_current','bid_workspace_requirement_projection_current',
    'bid_workspace_heads','bid_quote_snapshot_current'
  ] LOOP
    EXECUTE format('CREATE TRIGGER %I_guard BEFORE UPDATE OR DELETE ON %I FOR EACH ROW EXECUTE FUNCTION kb_bid_v2_guard_current_pointer()', relation_name, relation_name);
  END LOOP;
END
$$;

-- Foundation CAS functions. Later phases add command-specific publication
-- functions; runtime identities never receive direct table DML.
CREATE FUNCTION kb_bid_v2_advance_workspace_head(
  p_workspace_id uuid,
  p_expected_revision_id uuid,
  p_expected_sha256 kb_sha256,
  p_new_revision_id uuid,
  p_new_sha256 kb_sha256
) RETURNS boolean
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE head bid_workspace_heads%ROWTYPE;
BEGIN
  SELECT * INTO head FROM bid_workspace_heads WHERE scope_id=p_workspace_id FOR UPDATE;
  IF NOT FOUND OR head.artifact_id<>p_expected_revision_id OR head.artifact_sha256<>p_expected_sha256 THEN
    RETURN false;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM bid_workspace_revision_artifacts r
    WHERE r.project_id=head.project_id AND r.workspace_id=p_workspace_id
      AND r.id=p_new_revision_id AND r.content_sha256=p_new_sha256) THEN
    RAISE EXCEPTION 'new workspace revision identity is invalid' USING ERRCODE='23514';
  END IF;
  UPDATE bid_workspace_heads SET artifact_id=p_new_revision_id,
    artifact_sha256=p_new_sha256,generation=generation+1 WHERE scope_id=p_workspace_id;
  RETURN true;
END
$$;

CREATE FUNCTION kb_bid_v2_advance_document_set(
  p_project_id uuid,p_expected_artifact_id uuid,p_expected_sha256 kb_sha256,
  p_new_artifact_id uuid,p_new_sha256 kb_sha256
) RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE candidate bid_document_set_artifacts%ROWTYPE; DECLARE head bid_document_set_current%ROWTYPE;
BEGIN
  SELECT * INTO STRICT candidate FROM bid_document_set_artifacts WHERE project_id=p_project_id AND id=p_new_artifact_id AND content_sha256=p_new_sha256;
  SELECT * INTO head FROM bid_document_set_current WHERE scope_id=p_project_id FOR UPDATE;
  IF NOT FOUND THEN
    IF p_expected_artifact_id IS NOT NULL OR p_expected_sha256 IS NOT NULL OR candidate.revision<>1 THEN RETURN false; END IF;
    INSERT INTO bid_document_set_current(scope_id,artifact_id,artifact_sha256,generation,created_at) VALUES(p_project_id,candidate.id,candidate.content_sha256,candidate.revision,candidate.created_at); RETURN true;
  END IF;
  IF head.artifact_id=candidate.id AND head.artifact_sha256=candidate.content_sha256 THEN RETURN true; END IF;
  IF head.artifact_id IS DISTINCT FROM p_expected_artifact_id OR head.artifact_sha256 IS DISTINCT FROM p_expected_sha256 OR candidate.revision<>head.generation+1 THEN RETURN false; END IF;
  UPDATE bid_document_set_current SET artifact_id=candidate.id,artifact_sha256=candidate.content_sha256,generation=candidate.revision WHERE scope_id=p_project_id; RETURN true;
END $$;

CREATE FUNCTION kb_bid_v2_advance_disposition_set(
  p_project_id uuid,p_expected_artifact_id uuid,p_expected_sha256 kb_sha256,
  p_new_artifact_id uuid,p_new_sha256 kb_sha256
) RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE candidate bid_source_unit_disposition_set_artifacts%ROWTYPE; DECLARE head bid_source_unit_disposition_set_current%ROWTYPE;
BEGIN
  SELECT * INTO STRICT candidate FROM bid_source_unit_disposition_set_artifacts WHERE project_id=p_project_id AND id=p_new_artifact_id AND content_sha256=p_new_sha256;
  SELECT * INTO head FROM bid_source_unit_disposition_set_current WHERE scope_id=p_project_id FOR UPDATE;
  IF NOT FOUND THEN
    IF p_expected_artifact_id IS NOT NULL OR p_expected_sha256 IS NOT NULL OR candidate.revision<>1 THEN RETURN false; END IF;
    INSERT INTO bid_source_unit_disposition_set_current(scope_id,project_id,document_set_id,artifact_id,artifact_sha256,generation,created_at) VALUES(p_project_id,p_project_id,candidate.document_set_id,candidate.id,candidate.content_sha256,candidate.revision,candidate.created_at); RETURN true;
  END IF;
  IF head.artifact_id=candidate.id AND head.artifact_sha256=candidate.content_sha256 THEN RETURN true; END IF;
  IF head.artifact_id IS DISTINCT FROM p_expected_artifact_id OR head.artifact_sha256 IS DISTINCT FROM p_expected_sha256 OR candidate.revision<>head.generation+1 THEN RETURN false; END IF;
  UPDATE bid_source_unit_disposition_set_current SET document_set_id=candidate.document_set_id,artifact_id=candidate.id,artifact_sha256=candidate.content_sha256,generation=candidate.revision WHERE scope_id=p_project_id; RETURN true;
END $$;

CREATE FUNCTION kb_bid_v2_advance_requirement_supersession(
  p_project_id uuid,p_lineage_id uuid,p_expected_artifact_id uuid,p_expected_sha256 kb_sha256,
  p_new_artifact_id uuid,p_new_sha256 kb_sha256
) RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE candidate bid_requirement_supersession_revision_artifacts%ROWTYPE; DECLARE head bid_requirement_supersession_current%ROWTYPE;
BEGIN
  SELECT * INTO STRICT candidate FROM bid_requirement_supersession_revision_artifacts WHERE project_id=p_project_id AND lineage_id=p_lineage_id AND id=p_new_artifact_id AND content_sha256=p_new_sha256;
  SELECT * INTO head FROM bid_requirement_supersession_current WHERE scope_id=p_lineage_id FOR UPDATE;
  IF NOT FOUND THEN
    IF p_expected_artifact_id IS NOT NULL OR p_expected_sha256 IS NOT NULL OR candidate.revision<>1 THEN RETURN false; END IF;
    INSERT INTO bid_requirement_supersession_current(scope_id,project_id,artifact_id,artifact_sha256,generation,created_at) VALUES(p_lineage_id,p_project_id,candidate.id,candidate.content_sha256,candidate.revision,candidate.created_at); RETURN true;
  END IF;
  IF head.artifact_id=candidate.id AND head.artifact_sha256=candidate.content_sha256 THEN RETURN true; END IF;
  IF head.artifact_id IS DISTINCT FROM p_expected_artifact_id OR head.artifact_sha256 IS DISTINCT FROM p_expected_sha256 OR candidate.revision<>head.generation+1 THEN RETURN false; END IF;
  UPDATE bid_requirement_supersession_current SET artifact_id=candidate.id,artifact_sha256=candidate.content_sha256,generation=candidate.revision WHERE scope_id=p_lineage_id; RETURN true;
END $$;

CREATE FUNCTION kb_bid_v2_advance_requirement_projection(
  p_project_id uuid,p_workspace_id uuid,p_expected_artifact_id uuid,p_expected_sha256 kb_sha256,
  p_new_artifact_id uuid,p_new_sha256 kb_sha256
) RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE candidate bid_workspace_requirement_projection_artifacts%ROWTYPE; DECLARE head bid_workspace_requirement_projection_current%ROWTYPE;
BEGIN
  SELECT * INTO STRICT candidate FROM bid_workspace_requirement_projection_artifacts WHERE project_id=p_project_id AND workspace_id=p_workspace_id AND id=p_new_artifact_id AND content_sha256=p_new_sha256;
  SELECT * INTO head FROM bid_workspace_requirement_projection_current WHERE scope_id=p_workspace_id FOR UPDATE;
  IF NOT FOUND THEN
    IF p_expected_artifact_id IS NOT NULL OR p_expected_sha256 IS NOT NULL OR candidate.revision<>1 THEN RETURN false; END IF;
    INSERT INTO bid_workspace_requirement_projection_current(scope_id,project_id,artifact_id,artifact_sha256,generation,created_at) VALUES(p_workspace_id,p_project_id,candidate.id,candidate.content_sha256,candidate.revision,candidate.created_at); RETURN true;
  END IF;
  IF head.artifact_id=candidate.id AND head.artifact_sha256=candidate.content_sha256 THEN RETURN true; END IF;
  IF head.artifact_id IS DISTINCT FROM p_expected_artifact_id OR head.artifact_sha256 IS DISTINCT FROM p_expected_sha256 OR candidate.revision<>head.generation+1 THEN RETURN false; END IF;
  UPDATE bid_workspace_requirement_projection_current SET artifact_id=candidate.id,artifact_sha256=candidate.content_sha256,generation=candidate.revision WHERE scope_id=p_workspace_id; RETURN true;
END $$;

CREATE FUNCTION kb_bid_v2_advance_requirement_set(
  p_project_id uuid,p_expected_artifact_id uuid,p_expected_sha256 kb_sha256,
  p_new_artifact_id uuid,p_new_sha256 kb_sha256
) RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE candidate bid_requirement_set_artifacts%ROWTYPE; head bid_requirement_set_current%ROWTYPE;
BEGIN
  SELECT * INTO STRICT candidate FROM bid_requirement_set_artifacts
    WHERE project_id=p_project_id AND id=p_new_artifact_id AND content_sha256=p_new_sha256;
  SELECT * INTO STRICT head FROM bid_requirement_set_current WHERE scope_id=p_project_id FOR UPDATE;
  IF head.artifact_id IS DISTINCT FROM p_expected_artifact_id
     OR head.artifact_sha256 IS DISTINCT FROM p_expected_sha256
     OR candidate.revision<>head.generation+1 THEN RETURN false; END IF;
  UPDATE bid_requirement_set_current SET artifact_id=candidate.id,
    artifact_sha256=candidate.content_sha256,generation=candidate.revision,
    document_set_sequence=candidate.document_set_sequence,
    disposition_set_sequence=candidate.disposition_set_sequence
    WHERE scope_id=p_project_id;
  RETURN true;
END $$;

CREATE FUNCTION kb_bid_v2_publish_requirement_set(
  p_artifact_id uuid,p_artifact_sha256 kb_sha256
) RETURNS text LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE candidate bid_requirement_set_artifacts%ROWTYPE; DECLARE current_value bid_requirement_set_current%ROWTYPE;
BEGIN
  SELECT * INTO STRICT candidate FROM bid_requirement_set_artifacts
    WHERE id=p_artifact_id AND content_sha256=p_artifact_sha256;
  -- Lock the project even before a current row exists so concurrent first
  -- publications and out-of-order Oxana completion serialize deterministically.
  PERFORM 1 FROM bid_projects WHERE id=candidate.project_id FOR UPDATE;
  SELECT * INTO current_value FROM bid_requirement_set_current
    WHERE scope_id=candidate.project_id FOR UPDATE;
  IF FOUND THEN
    IF (candidate.document_set_sequence,candidate.disposition_set_sequence)
       < (current_value.document_set_sequence,current_value.disposition_set_sequence) THEN
      RETURN 'superseded';
    END IF;
    IF (candidate.document_set_sequence,candidate.disposition_set_sequence)
       = (current_value.document_set_sequence,current_value.disposition_set_sequence) THEN
      IF current_value.artifact_id<>candidate.id
         OR current_value.artifact_sha256<>candidate.content_sha256 THEN
        RAISE EXCEPTION 'same requirement input has conflicting artifact' USING ERRCODE='23505';
      END IF;
      RETURN 'replayed';
    END IF;
    UPDATE bid_requirement_set_current SET
      artifact_id=candidate.id,
      artifact_sha256=candidate.content_sha256,
      generation=current_value.generation+1,
      document_set_sequence=candidate.document_set_sequence,
      disposition_set_sequence=candidate.disposition_set_sequence
      WHERE scope_id=candidate.project_id;
  ELSE
    INSERT INTO bid_requirement_set_current(
      scope_id,artifact_id,artifact_sha256,generation,
      document_set_sequence,disposition_set_sequence,created_at
    ) VALUES(
      candidate.project_id,candidate.id,candidate.content_sha256,1,
      candidate.document_set_sequence,candidate.disposition_set_sequence,candidate.created_at
    );
  END IF;
  RETURN 'published';
END $$;

-- Worker-only frozen loader. Runtime workers receive no direct bidding table
-- access; this SECURITY DEFINER seam verifies the complete typed request,
-- converter contract, role revision, and available source object identity.
CREATE FUNCTION kb_bid_v2_load_tender_document_process_input(
  p_request_artifact_id uuid,p_request_revision bigint,p_frozen_input_sha256 kb_sha256
) RETURNS TABLE(
  request_artifact_id uuid,request_revision bigint,frozen_input_sha256 kb_sha256,
  project_id uuid,document_id uuid,document_sha256 kb_sha256,
  role_revision_id uuid,role_revision_sha256 kb_sha256,
  converter_contract_id uuid,converter_contract_sha256 kb_sha256,
  file_name text,media_type text,original_object_ref kb_object_ref,byte_length bigint
) LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
  SELECT typed.request_artifact_id,typed.request_revision,typed.frozen_input_sha256,
    typed.project_id,typed.document_id,typed.document_sha256,
    typed.role_revision_id,typed.role_revision_sha256,
    typed.converter_contract_id,typed.converter_contract_sha256,
    document.file_name,document.media_type,document.original_object_ref,document.byte_length
  FROM bid_tender_document_process_request_identities typed
  JOIN bid_async_request_snapshot_artifacts request_value
    ON request_value.id=typed.request_artifact_id
   AND request_value.project_id=typed.project_id
   AND request_value.request_kind='tender_document_process'
   AND request_value.revision=typed.request_revision
   AND request_value.frozen_input_sha256=typed.frozen_input_sha256
  JOIN bid_documents document
    ON document.project_id=typed.project_id AND document.id=typed.document_id
   AND document.original_sha256=typed.document_sha256
  JOIN bid_document_role_revision_artifacts role_value
    ON role_value.project_id=typed.project_id AND role_value.document_id=typed.document_id
   AND role_value.id=typed.role_revision_id
   AND role_value.content_sha256=typed.role_revision_sha256
  JOIN bid_authoring_contract_artifacts converter
    ON converter.id=typed.converter_contract_id
   AND converter.content_sha256=typed.converter_contract_sha256
   AND converter.contract_kind='converter'
  JOIN object_registry object_value
    ON object_value.object_ref=document.original_object_ref
   AND object_value.digest=document.original_sha256
   AND object_value.media_type=document.media_type
   AND object_value.byte_length=document.byte_length
   AND object_value.state='available'
  WHERE typed.request_artifact_id=p_request_artifact_id
    AND typed.request_revision=p_request_revision
    AND typed.frozen_input_sha256=p_frozen_input_sha256
$$;

-- Atomic one-document publication seam for the active V2 worker. Runtime
-- callers cannot insert source/image/unit artifacts directly. Canonical bytes
-- are passed as hex so PostgreSQL validates the exact SHA rather than relying
-- on caller JSON serialization.
CREATE FUNCTION kb_bid_v2_publish_tender_document_process(
  p_request_artifact_id uuid,
  p_request_revision bigint,
  p_frozen_input_sha256 kb_sha256,
  p_project_id uuid,
  p_document_id uuid,
  p_document_sha256 kb_sha256,
  p_source jsonb,
  p_images jsonb,
  p_units jsonb,
  p_actor kb_actor_identity
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE
  request_value bid_async_request_snapshot_artifacts%ROWTYPE;
  typed_request bid_tender_document_process_request_identities%ROWTYPE;
  source_id uuid;
  source_sha kb_sha256;
  source_ref kb_object_ref;
  source_media text;
  source_length bigint;
  source_staging uuid;
  source_payload bytea;
  image_value jsonb;
  unit_value jsonb;
  image_payload bytea;
  unit_payload bytea;
  span_payload bytea;
  text_payload bytea;
  result_value jsonb;
  result_sha kb_sha256;
  computed_image_set_sha kb_sha256;
  computed_source_unit_set_sha kb_sha256;
  source_json jsonb;
  prior bid_async_stage_receipts%ROWTYPE;
  expected_ordinal integer:=0;
  image_count integer:=0;
  form_id uuid; form_payload bytea; form_sha kb_sha256;
BEGIN
  IF jsonb_typeof(p_source)<>'object'
     OR NOT kb_bid_v2_json_keys_exact(p_source,ARRAY[
       'id','revision','staging_id','object_ref','sha256','media_type','byte_length',
       'canonical_payload_hex','converter_contract_id','converter_contract_sha256',
       'image_asset_set_sha256','source_unit_set_sha256'])
     OR jsonb_typeof(p_images)<>'array'
     OR jsonb_typeof(p_units)<>'array' OR jsonb_array_length(p_units) NOT BETWEEN 1 AND 100000 THEN
    RAISE EXCEPTION 'TenderDocumentProcess publication envelope invalid' USING ERRCODE='23514';
  END IF;
  SELECT * INTO STRICT request_value FROM bid_async_request_snapshot_artifacts
   WHERE id=p_request_artifact_id AND request_kind='tender_document_process'
     AND revision=p_request_revision AND frozen_input_sha256=p_frozen_input_sha256
   FOR UPDATE;
  SELECT * INTO STRICT typed_request FROM bid_tender_document_process_request_identities
   WHERE request_artifact_id=request_value.id;
  IF typed_request.project_id<>p_project_id OR typed_request.document_id<>p_document_id
     OR typed_request.document_sha256<>p_document_sha256 THEN
    RAISE EXCEPTION 'TenderDocumentProcess frozen document tuple mismatch' USING ERRCODE='23514';
  END IF;

  source_id:=(p_source->>'id')::uuid;
  source_ref:=(p_source->>'object_ref')::kb_object_ref;
  source_sha:=(p_source->>'sha256')::kb_sha256;
  source_media:=p_source->>'media_type';
  source_length:=(p_source->>'byte_length')::bigint;
  source_staging:=(p_source->>'staging_id')::uuid;
  source_payload:=decode(p_source->>'canonical_payload_hex','hex');
  source_json:=convert_from(source_payload,'UTF8')::jsonb;
  IF source_ref<>'objects/'||source_sha OR source_media<>'application/json'
     OR source_length<>octet_length(source_payload)
     OR source_sha<>kb_bid_v2_sha256_bytes(source_payload)
     OR (p_source->>'revision')::bigint<>p_request_revision
     OR (p_source->>'converter_contract_id')::uuid<>typed_request.converter_contract_id
     OR (p_source->>'converter_contract_sha256')::kb_sha256<>typed_request.converter_contract_sha256
     OR NOT kb_bid_v2_json_keys_exact(source_json,ARRAY[
       'schema_version','source_purpose','project_id','document_id','document_sha256',
       'converter_contract_id','converter_contract_sha256','markdown','structured_source_units'])
     OR source_json->>'schema_version'<>'2'
     OR source_json->>'source_purpose'<>'tender_requirements_and_structure_only'
     OR (source_json->>'project_id')::uuid<>p_project_id
     OR (source_json->>'document_id')::uuid<>p_document_id
     OR (source_json->>'document_sha256')::kb_sha256<>p_document_sha256
     OR (source_json->>'converter_contract_id')::uuid<>typed_request.converter_contract_id
     OR (source_json->>'converter_contract_sha256')::kb_sha256<>typed_request.converter_contract_sha256
     OR jsonb_typeof(source_json->'markdown') IS DISTINCT FROM 'string'
     OR jsonb_typeof(source_json->'structured_source_units') IS DISTINCT FROM 'array' THEN
    RAISE EXCEPTION 'TenderDocumentProcess converted source identity mismatch' USING ERRCODE='23514';
  END IF;

  FOR unit_value IN SELECT value FROM jsonb_array_elements(p_units) LOOP
    IF (unit_value->>'ordinal')::integer<>expected_ordinal
       OR unit_value->>'source_purpose'<>'tender_requirements_and_structure_only'
       OR unit_value->>'unit_kind' NOT IN ('section','table_row','form_region','attachment_region','image_ocr_region')
       OR (unit_value->>'revision')::bigint<>p_request_revision THEN
      RAISE EXCEPTION 'TenderDocumentProcess SourceUnit ordering or purpose invalid' USING ERRCODE='23514';
    END IF;
    unit_payload:=decode(unit_value->>'canonical_payload_hex','hex');
    span_payload:=decode(unit_value->>'source_span_payload_hex','hex');
    text_payload:=decode(unit_value->>'text_utf8_hex','hex');
    IF unit_value->>'content_sha256'<>kb_bid_v2_sha256_bytes(unit_payload)
       OR unit_value->>'source_span_sha256'<>kb_bid_v2_sha256_bytes(span_payload)
       OR unit_value->>'text_sha256'<>kb_bid_v2_sha256_bytes(text_payload)
       OR convert_from(span_payload,'UTF8')::jsonb IS DISTINCT FROM unit_value->'source_span_v2'
       OR convert_from(unit_payload,'UTF8')::jsonb->>'source_purpose'<>'tender_requirements_and_structure_only'
       OR (unit_value->'source_span_v2')->>'source_purpose'<>'tender_requirements_and_structure_only'
       OR ((unit_value->>'unit_kind')='image_ocr_region') IS DISTINCT FROM (unit_value->>'image_artifact_id' IS NOT NULL) THEN
      RAISE EXCEPTION 'TenderDocumentProcess SourceUnit canonical identity mismatch' USING ERRCODE='23514';
    END IF;
    expected_ordinal:=expected_ordinal+1;
  END LOOP;

  SELECT kb_bid_v2_sha256_bytes(convert_to(COALESCE(
      string_agg(value->>'content_sha256','' ORDER BY value->>'content_sha256'),''),'UTF8'))
    INTO computed_image_set_sha FROM jsonb_array_elements(p_images);
  SELECT kb_bid_v2_sha256_bytes(convert_to(COALESCE(
      string_agg(value->>'content_sha256','' ORDER BY (value->>'ordinal')::integer),''),'UTF8'))
    INTO computed_source_unit_set_sha FROM jsonb_array_elements(p_units);
  IF (p_source->>'image_asset_set_sha256')::kb_sha256<>computed_image_set_sha
     OR (p_source->>'source_unit_set_sha256')::kb_sha256<>computed_source_unit_set_sha THEN
    RAISE EXCEPTION 'TenderDocumentProcess publication set digest mismatch' USING ERRCODE='23514';
  END IF;

  FOR image_value IN SELECT value FROM jsonb_array_elements(p_images) LOOP
    image_payload:=decode(image_value->>'canonical_payload_hex','hex');
    IF image_value->>'source_purpose'<>'tender_requirements_and_structure_only'
       OR image_value->>'content_sha256'<>kb_bid_v2_sha256_bytes(image_payload)
       OR image_value->>'original_object_ref'<>'objects/'||(image_value->>'original_sha256')
       OR image_value->>'ocr_object_ref'<>'objects/'||(image_value->>'ocr_sha256')
       OR (image_value->>'original_byte_length')::bigint<=0
       OR (image_value->>'ocr_byte_length')::bigint<=0
       OR image_value->>'ocr_media_type'<>'text/plain'
       OR image_value->>'model_contract_sha256'<>kb_bid_v2_sha256_bytes(decode(image_value->>'model_contract_payload_hex','hex'))
       OR image_value->>'operation_contract_sha256'<>kb_bid_v2_sha256_bytes(decode(image_value->>'operation_contract_payload_hex','hex')) THEN
      RAISE EXCEPTION 'TenderDocumentProcess image canonical identity mismatch' USING ERRCODE='23514';
    END IF;
    image_count:=image_count+1;
  END LOOP;

  result_value:=jsonb_build_object(
    'request_artifact_id',p_request_artifact_id,
    'converted_source_revision_id',source_id,
    'converted_source_sha256',source_sha,
    'image_asset_set_sha256',computed_image_set_sha,
    'source_unit_set_sha256',computed_source_unit_set_sha,
    'source_unit_count',jsonb_array_length(p_units),
    'image_ocr_region_count',image_count
  );
  result_sha:=kb_bid_v2_sha256_bytes(convert_to(result_value::text,'UTF8'));
  SELECT * INTO prior FROM bid_async_stage_receipts
   WHERE request_artifact_id=p_request_artifact_id AND stage_kind='extraction'
     AND frozen_input_sha256=p_frozen_input_sha256;
  IF FOUND THEN
    IF prior.result_sha256<>result_sha OR prior.result_identity<>result_value
       OR request_value.status<>'succeeded' THEN
      RAISE EXCEPTION 'TenderDocumentProcess replay conflicts with first result' USING ERRCODE='23505';
    END IF;
    RETURN result_value||jsonb_build_object('replayed',true);
  END IF;
  IF request_value.status<>'pending' THEN
    RAISE EXCEPTION 'TenderDocumentProcess request is already terminal' USING ERRCODE='23514';
  END IF;

  PERFORM kb_object_upload_commit(source_staging,source_ref,source_sha,source_media,
    source_length,'bid_converted_source',source_id,'structured-source',p_actor);
  INSERT INTO bid_converted_source_artifacts(
    id,project_id,document_id,revision,source_object_ref,source_sha256,
    converter_contract_id,converter_contract_sha256,image_asset_set_sha256)
  VALUES(source_id,p_project_id,p_document_id,p_request_revision,source_ref,source_sha,
    (p_source->>'converter_contract_id')::uuid,
    (p_source->>'converter_contract_sha256')::kb_sha256,
    (p_source->>'image_asset_set_sha256')::kb_sha256);

  FOR image_value IN SELECT value FROM jsonb_array_elements(p_images) LOOP
    INSERT INTO bid_authoring_contract_artifacts(id,contract_kind,schema_version,canonical_payload,content_sha256)
    VALUES((image_value->>'model_contract_id')::uuid,'vision_model',1,
      decode(image_value->>'model_contract_payload_hex','hex'),(image_value->>'model_contract_sha256')::kb_sha256)
    ON CONFLICT (id) DO NOTHING;
    IF NOT EXISTS (SELECT 1 FROM bid_authoring_contract_artifacts WHERE id=(image_value->>'model_contract_id')::uuid
      AND contract_kind='vision_model' AND content_sha256=(image_value->>'model_contract_sha256')::kb_sha256) THEN
      RAISE EXCEPTION 'TenderDocumentProcess vision model contract conflict' USING ERRCODE='23505';
    END IF;
    INSERT INTO bid_authoring_contract_artifacts(id,contract_kind,schema_version,canonical_payload,content_sha256)
    VALUES((image_value->>'operation_contract_id')::uuid,'vision_operation',1,
      decode(image_value->>'operation_contract_payload_hex','hex'),(image_value->>'operation_contract_sha256')::kb_sha256)
    ON CONFLICT (id) DO NOTHING;
    IF NOT EXISTS (SELECT 1 FROM bid_authoring_contract_artifacts WHERE id=(image_value->>'operation_contract_id')::uuid
      AND contract_kind='vision_operation' AND content_sha256=(image_value->>'operation_contract_sha256')::kb_sha256) THEN
      RAISE EXCEPTION 'TenderDocumentProcess vision operation contract conflict' USING ERRCODE='23505';
    END IF;
    PERFORM kb_object_upload_commit((image_value->>'original_staging_id')::uuid,
      (image_value->>'original_object_ref')::kb_object_ref,(image_value->>'original_sha256')::kb_sha256,
      image_value->>'original_media_type',(image_value->>'original_byte_length')::bigint,
      'bid_tender_source_image',(image_value->>'id')::uuid,'original',p_actor);
    PERFORM kb_object_upload_commit((image_value->>'ocr_staging_id')::uuid,
      (image_value->>'ocr_object_ref')::kb_object_ref,(image_value->>'ocr_sha256')::kb_sha256,
      image_value->>'ocr_media_type',(image_value->>'ocr_byte_length')::bigint,
      'bid_tender_source_image',(image_value->>'id')::uuid,'ocr-text',p_actor);
    INSERT INTO bid_tender_source_image_revision_artifacts(
      id,project_id,document_id,source_revision_id,ordinal,source_purpose,source_locator,
      original_object_ref,original_sha256,original_media_type,original_byte_length,
      ocr_object_ref,ocr_sha256,ocr_media_type,ocr_byte_length,
      model_contract_id,model_contract_sha256,operation_contract_id,operation_contract_sha256,
      canonical_payload,content_sha256)
    VALUES((image_value->>'id')::uuid,p_project_id,p_document_id,source_id,
      (image_value->>'ordinal')::integer,image_value->>'source_purpose',image_value->'source_locator',
      (image_value->>'original_object_ref')::kb_object_ref,(image_value->>'original_sha256')::kb_sha256,
      image_value->>'original_media_type',(image_value->>'original_byte_length')::bigint,
      (image_value->>'ocr_object_ref')::kb_object_ref,(image_value->>'ocr_sha256')::kb_sha256,
      image_value->>'ocr_media_type',(image_value->>'ocr_byte_length')::bigint,
      (image_value->>'model_contract_id')::uuid,(image_value->>'model_contract_sha256')::kb_sha256,
      (image_value->>'operation_contract_id')::uuid,(image_value->>'operation_contract_sha256')::kb_sha256,
      decode(image_value->>'canonical_payload_hex','hex'),(image_value->>'content_sha256')::kb_sha256);
  END LOOP;

  FOR unit_value IN SELECT value FROM jsonb_array_elements(p_units) LOOP
    INSERT INTO bid_source_unit_lineages(id,project_id,document_id)
    VALUES((unit_value->>'lineage_id')::uuid,p_project_id,p_document_id)
    ON CONFLICT (id) DO NOTHING;
    IF NOT EXISTS (SELECT 1 FROM bid_source_unit_lineages WHERE id=(unit_value->>'lineage_id')::uuid
      AND project_id=p_project_id AND document_id=p_document_id) THEN
      RAISE EXCEPTION 'TenderDocumentProcess SourceUnit lineage conflict' USING ERRCODE='23505';
    END IF;
    INSERT INTO bid_source_unit_revision_artifacts(
      id,project_id,lineage_id,revision,document_id,source_revision_id,unit_kind,ordinal,
      source_locator,source_span_sha256,text_utf8,text_sha256,canonical_payload,content_sha256)
    VALUES((unit_value->>'id')::uuid,p_project_id,(unit_value->>'lineage_id')::uuid,
      (unit_value->>'revision')::bigint,p_document_id,source_id,unit_value->>'unit_kind',
      (unit_value->>'ordinal')::integer,unit_value->'source_span_v2',
      (unit_value->>'source_span_sha256')::kb_sha256,decode(unit_value->>'text_utf8_hex','hex'),
      (unit_value->>'text_sha256')::kb_sha256,decode(unit_value->>'canonical_payload_hex','hex'),
      (unit_value->>'content_sha256')::kb_sha256);
    IF unit_value->>'unit_kind'='form_region'
      AND jsonb_typeof(unit_value->'grid')='object' THEN
      form_id:=gen_random_uuid();
      form_payload:=kb_bid_v2_json_payload(jsonb_strip_nulls(jsonb_build_object(
        'schema_version',3,
        'kind','grid',
        'form_definition_revision_id',form_id,
        'source_unit_revision_id',(unit_value->>'id')::uuid,
        'title','source_unit:'||(unit_value->>'id'),
        'row_count',(unit_value->'grid'->>'row_count')::integer,
        'column_count',(unit_value->'grid'->>'column_count')::integer,
        'cells',unit_value->'grid'->'cells',
        'widths_mm',CASE WHEN jsonb_typeof(unit_value->'grid'->'widths_mm')='array'
          THEN unit_value->'grid'->'widths_mm' ELSE NULL END
      )));
      form_sha:=kb_bid_v2_sha256_bytes(form_payload);
      INSERT INTO bid_tender_structured_form_definition_artifacts(id,project_id,source_unit_revision_id,
        schema_version,canonical_payload,content_sha256)
      VALUES(form_id,p_project_id,(unit_value->>'id')::uuid,3,form_payload,form_sha);
    END IF;
  END LOOP;

INSERT INTO bid_async_stage_receipts(request_artifact_id,stage_kind,frozen_input_sha256,result_identity,result_sha256)
VALUES(p_request_artifact_id,'extraction',p_frozen_input_sha256,result_value,result_sha);
UPDATE bid_documents SET parse_status='ready'
WHERE id=p_document_id AND project_id=p_project_id;
UPDATE bid_async_request_snapshot_artifacts SET status='succeeded',result_identity=result_value,
finished_at=clock_timestamp() WHERE id=p_request_artifact_id;
  RETURN result_value||jsonb_build_object('replayed',false);
END $$;

CREATE VIEW bidding_v2_projects AS SELECT id,owner_user_id,title,status,created_at,ended_at FROM bid_projects;
CREATE VIEW bidding_v2_workspace_heads AS
 SELECT w.project_id,w.id workspace_id,h.artifact_id workspace_revision_id,h.artifact_sha256,h.generation
 FROM bid_submission_workspaces w JOIN bid_workspace_heads h ON h.scope_id=w.id;
CREATE VIEW bidding_v2_async_requests AS
 SELECT id,project_id,workspace_id,request_kind,revision,frozen_input_sha256,status,result_identity,error_code,
   created_at,finished_at
 FROM bid_async_request_snapshot_artifacts;
CREATE VIEW bidding_v2_outputs AS
 SELECT id,project_id,workspace_id,manifest_id,format,object_ref,content_sha256,byte_length,created_at
 FROM bid_submission_output_artifacts;

CREATE FUNCTION kb_bid_v2_json_payload(p jsonb) RETURNS bytea
LANGUAGE sql IMMUTABLE AS $$ SELECT convert_to(p::text,'UTF8') $$;

CREATE FUNCTION kb_bid_v2_load_workspace_revision(
  p_workspace_id uuid,p_revision_id uuid,p_revision_sha256 kb_sha256
) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE
  w bid_submission_workspaces%ROWTYPE;
  head bid_workspace_heads%ROWTYPE;
  rev bid_workspace_revision_artifacts%ROWTYPE;
  settings bid_document_settings_revision_artifacts%ROWTYPE;
  nodes jsonb := '[]'::jsonb;
  blocks jsonb := '[]'::jsonb;
  bindings jsonb := '[]'::jsonb;
  node_rec record;
  binding_rec record;
  block_ids uuid[];
  block_id uuid;
  block_rec bid_content_block_revision_artifacts%ROWTYPE;
  block_json jsonb;
  ds_id uuid;
  ds_sha kb_sha256;
  projection_id uuid;
  projection_sha kb_sha256;
  checkpoint_id uuid;
  checkpoint_sha kb_sha256;
  quote_value jsonb;
BEGIN
  SELECT * INTO w FROM bid_submission_workspaces WHERE id=p_workspace_id;
  IF NOT FOUND THEN RETURN NULL; END IF;
  SELECT * INTO rev FROM bid_workspace_revision_artifacts
    WHERE workspace_id=p_workspace_id AND id=p_revision_id AND content_sha256=p_revision_sha256;
  IF NOT FOUND THEN RETURN NULL; END IF;
  SELECT * INTO settings FROM bid_document_settings_revision_artifacts WHERE id=rev.document_settings_revision_id;
  SELECT convert_from(quote.canonical_payload,'UTF8')::jsonb||jsonb_build_object(
      'artifact_id',quote.id,'sha256',quote.content_sha256,
      'revision',quote.revision,'currency',quote.currency)
    INTO quote_value FROM bid_quote_snapshot_artifacts quote
    WHERE quote.project_id=w.project_id AND quote.id=rev.quote_snapshot_id
      AND quote.content_sha256=rev.quote_snapshot_sha256;
  projection_id:=rev.requirement_projection_id;
  projection_sha:=rev.requirement_projection_sha256;
  SELECT requirement_set.document_set_id, document_set.content_sha256 INTO ds_id, ds_sha
    FROM bid_workspace_requirement_projection_artifacts projection
    JOIN bid_requirement_set_artifacts requirement_set ON requirement_set.id=projection.requirement_set_id
    JOIN bid_document_set_artifacts document_set ON document_set.id=requirement_set.document_set_id
    WHERE projection.id=projection_id AND projection.content_sha256=projection_sha;
  SELECT checkpoint.id,checkpoint.content_sha256 INTO checkpoint_id,checkpoint_sha
    FROM bid_outline_checkpoint_artifacts checkpoint
    WHERE checkpoint.workspace_id=w.id AND checkpoint.workspace_revision_id=rev.id
      AND checkpoint.requirement_projection_id=projection_id
      AND checkpoint.requirement_projection_sha256=projection_sha
    ORDER BY checkpoint.created_at DESC,checkpoint.id DESC LIMIT 1;
  FOR node_rec IN
    WITH RECURSIVE ordered_nodes AS (
      SELECT occurrence.*,ARRAY[lpad(occurrence.ordinal::text,10,'0')||':'||occurrence.id::text] tree_path
      FROM bid_workspace_node_occurrences occurrence
      WHERE occurrence.workspace_revision_id=rev.id AND occurrence.parent_occurrence_id IS NULL
      UNION ALL
      SELECT child.*,parent.tree_path||(lpad(child.ordinal::text,10,'0')||':'||child.id::text)
      FROM bid_workspace_node_occurrences child
      JOIN ordered_nodes parent ON parent.id=child.parent_occurrence_id
      WHERE child.workspace_revision_id=rev.id
    )
    SELECT occ.id occ_id, occ.parent_occurrence_id, occ.ordinal, occ.depth,
           n.lineage_id, n.id revision_id, n.title, n.semantic_role, n.render_role, n.tombstone
      FROM ordered_nodes occ
      JOIN bid_outline_node_revision_artifacts n ON n.id=occ.node_revision_id AND n.project_id=occ.project_id
     ORDER BY occ.tree_path
  LOOP
    SELECT coalesce(array_agg(b.lineage_id ORDER BY bloc.ordinal), ARRAY[]::uuid[])
      INTO block_ids
      FROM bid_workspace_block_occurrences bloc
      JOIN bid_content_block_revision_artifacts b ON b.id=bloc.block_revision_id AND b.project_id=bloc.project_id
     WHERE bloc.node_occurrence_id=node_rec.occ_id;
    nodes := nodes || jsonb_build_array(jsonb_build_object(
      'lineage_id', node_rec.lineage_id,
      'revision_id', node_rec.revision_id,
      'parent_lineage_id', (
        SELECT n2.lineage_id FROM bid_workspace_node_occurrences pocc
          JOIN bid_outline_node_revision_artifacts n2 ON n2.id=pocc.node_revision_id
         WHERE pocc.id=node_rec.parent_occurrence_id),
      'ordinal', node_rec.ordinal,
      'depth', node_rec.depth,
      'title', node_rec.title,
      'semantic_role', node_rec.semantic_role,
      'render_role', node_rec.render_role,
      'stale', node_rec.tombstone,
      'block_lineage_ids', to_jsonb(coalesce(block_ids, ARRAY[]::uuid[]))
    ));
  END LOOP;
  FOR block_rec IN
    SELECT DISTINCT b.* FROM bid_workspace_block_occurrences bloc
      JOIN bid_content_block_revision_artifacts b ON b.id=bloc.block_revision_id AND b.project_id=bloc.project_id
     WHERE bloc.workspace_revision_id=rev.id
  LOOP
    block_json := jsonb_build_object('content',block_rec.block_payload) || jsonb_build_object(
      'schema_version', block_rec.schema_version,
      'block_revision_id', block_rec.id,
      'lineage_id', block_rec.lineage_id,
      'revision', block_rec.revision,
      'kind', block_rec.block_kind,
      'origin', block_rec.origin,
      'content_sha256', block_rec.content_sha256
    );
    blocks := blocks || jsonb_build_array(block_json);
  END LOOP;
  FOR binding_rec IN
    SELECT b.* FROM bid_workspace_binding_occurrences bo
      JOIN bid_outline_fulfillment_binding_revision_artifacts b
        ON b.id=bo.binding_revision_id AND b.project_id=bo.project_id
     WHERE bo.workspace_revision_id=rev.id ORDER BY bo.ordinal
  LOOP
    bindings := bindings || jsonb_build_array(jsonb_build_object(
      'binding_lineage_id',binding_rec.lineage_id,
      'binding_revision_id',binding_rec.id,
      'revision',binding_rec.revision,
      'need_occurrence_id',binding_rec.need_occurrence_id,
      'requirement_projection_revision_id',binding_rec.requirement_projection_id,
      'requirement_projection_sha256',projection_sha,
      'channel',binding_rec.channel,
      'target',jsonb_build_object(
        'kind',binding_rec.target_kind,
        CASE binding_rec.target_kind
          WHEN 'outline_node' THEN 'node_lineage_id'
          WHEN 'response_table' THEN 'block_lineage_id'
          WHEN 'structured_form' THEN 'form_definition_revision_id'
          ELSE 'quote_snapshot_id'
        END,binding_rec.target_id),
      'node_lineage_id',CASE WHEN binding_rec.target_kind='outline_node' THEN binding_rec.target_id ELSE NULL END,
      'reason',binding_rec.reason,
      'stale',binding_rec.requirement_projection_id IS DISTINCT FROM projection_id
    ));
  END LOOP;
  RETURN jsonb_build_object(
    'workspace_id', w.id,
    'project_id', w.project_id,
    'revision_id', rev.id,
    'sha256', rev.content_sha256,
    'scope', 'project_wide',
    'outline_checkpoint_id', checkpoint_id,
    'outline_checkpoint_sha256', checkpoint_sha,
    'requirement_projection_revision_id', projection_id,
    'requirement_projection_sha256', projection_sha,
    'document_settings_revision_id', settings.id,
    'document_settings_sha256', settings.content_sha256,
    'document_settings', settings.settings,
    'document_set_revision_id', ds_id,
    'document_set_sha256', ds_sha,
    'nodes', nodes,
    'blocks', blocks,
    'bindings', bindings,
    'quote_snapshot', quote_value
  );
END $$;

CREATE FUNCTION kb_bid_v2_load_workspace(p_workspace_id uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE head bid_workspace_heads%ROWTYPE;
BEGIN
  SELECT * INTO head FROM bid_workspace_heads WHERE scope_id=p_workspace_id;
  IF NOT FOUND THEN RETURN NULL; END IF;
  RETURN kb_bid_v2_load_workspace_revision(p_workspace_id,head.artifact_id,head.artifact_sha256);
END $$;

CREATE FUNCTION kb_bid_create_project_v2(
  p_id uuid, p_title text, p_owner_user_id uuid, p_actor kb_actor_identity
) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE
  workspace_id uuid := gen_random_uuid();
  scope_id uuid := gen_random_uuid();
  settings_id uuid := gen_random_uuid();
  docset_id uuid := gen_random_uuid();
  disp_id uuid := gen_random_uuid();
  reqset_id uuid := gen_random_uuid();
  proj_id uuid := gen_random_uuid();
  rev_id uuid := gen_random_uuid();
  payload bytea; sha kb_sha256;
  settings jsonb := '{"page_size":"A4","margins_mm":{"top":25.4,"right":25.4,"bottom":25.4,"left":25.4},"body_font_pt":12,"line_spacing":1.5,"heading_numbering":"decimal","header":"","footer":"","page_number":"footer_center"}'::jsonb;
  empty jsonb := '{"schema_version":1,"items":[]}'::jsonb;
BEGIN
  IF p_actor <> 'user:'||p_owner_user_id::text THEN
    RAISE EXCEPTION 'PROJECT_OWNER_ACTOR_MISMATCH' USING ERRCODE='42501';
  END IF;
  INSERT INTO bid_projects(id,owner_user_id,title,status) VALUES(p_id,p_owner_user_id,p_title,'open');
  INSERT INTO bid_submission_workspaces(id,project_id) VALUES(workspace_id,p_id);
  payload := kb_bid_v2_json_payload(empty); sha := kb_bid_v2_sha256_bytes(payload);
  INSERT INTO bid_document_set_artifacts(id,project_id,revision,canonical_payload,content_sha256,actor)
    VALUES(docset_id,p_id,1,payload,sha,p_actor);
  PERFORM kb_bid_v2_advance_document_set(p_id,NULL,NULL,docset_id,sha);
  payload := kb_bid_v2_json_payload(empty); sha := kb_bid_v2_sha256_bytes(payload);
  INSERT INTO bid_source_unit_disposition_set_artifacts(id,project_id,document_set_id,document_set_sequence,revision,canonical_payload,content_sha256,actor)
    VALUES(disp_id,p_id,docset_id,1,1,payload,sha,p_actor);
  PERFORM kb_bid_v2_advance_disposition_set(p_id,NULL,NULL,disp_id,sha);
  payload := kb_bid_v2_json_payload(empty); sha := kb_bid_v2_sha256_bytes(payload);
  INSERT INTO bid_requirement_set_artifacts(id,project_id,document_set_id,document_set_sequence,disposition_set_id,disposition_set_sequence,revision,canonical_payload,content_sha256)
    VALUES(reqset_id,p_id,docset_id,1,disp_id,1,1,payload,sha);
  PERFORM kb_bid_v2_publish_requirement_set(reqset_id,sha);
  payload := kb_bid_v2_json_payload(empty); sha := kb_bid_v2_sha256_bytes(payload);
  INSERT INTO bid_workspace_requirement_projection_artifacts(id,project_id,workspace_id,requirement_set_id,revision,canonical_payload,content_sha256)
    VALUES(proj_id,p_id,workspace_id,reqset_id,1,payload,sha);
  PERFORM kb_bid_v2_advance_requirement_projection(p_id,workspace_id,NULL,NULL,proj_id,sha);
  payload := kb_bid_v2_json_payload(jsonb_build_object('scope_kind','project_wide')); sha := kb_bid_v2_sha256_bytes(payload);
  INSERT INTO bid_workspace_scope_revision_artifacts(id,project_id,workspace_id,revision,scope_kind,canonical_payload,content_sha256)
    VALUES(scope_id,p_id,workspace_id,1,'project_wide',payload,sha);
  payload := kb_bid_v2_json_payload(settings); sha := kb_bid_v2_sha256_bytes(payload);
  INSERT INTO bid_document_settings_revision_artifacts(id,project_id,workspace_id,revision,schema_version,settings,canonical_payload,content_sha256,actor)
    VALUES(settings_id,p_id,workspace_id,1,1,settings,payload,sha,p_actor);
  payload := kb_bid_v2_json_payload(jsonb_build_object('schema_version',1,'nodes','[]'::jsonb,'blocks','[]'::jsonb));
  sha := kb_bid_v2_sha256_bytes(payload);
  INSERT INTO bid_workspace_revision_artifacts(id,project_id,workspace_id,revision,scope_revision_id,requirement_projection_id,requirement_projection_sha256,document_settings_revision_id,canonical_payload,content_sha256,actor)
    VALUES(rev_id,p_id,workspace_id,1,scope_id,proj_id,(SELECT content_sha256 FROM bid_workspace_requirement_projection_artifacts WHERE id=proj_id),settings_id,payload,sha,p_actor);
  INSERT INTO bid_workspace_heads(scope_id,project_id,artifact_id,artifact_sha256,generation,created_at)
    VALUES(workspace_id,p_id,rev_id,sha,1,clock_timestamp());
  RETURN jsonb_build_object('id',p_id,'title',p_title,'status','open','ended_at',NULL,'workspace_id',workspace_id,'owner_user_id',p_owner_user_id);
END $$;

CREATE FUNCTION kb_bid_v2_commit_workspace_mutation(
  p_workspace_id uuid,
  p_expected_revision_id uuid,
  p_expected_sha256 kb_sha256,
  p_snapshot jsonb,
  p_actor kb_actor_identity
) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE
  head bid_workspace_heads%ROWTYPE;
  cur bid_workspace_revision_artifacts%ROWTYPE;
  w bid_submission_workspaces%ROWTYPE;
  node jsonb; block jsonb; binding jsonb; edge jsonb; deleted record;
  lineage uuid; rev uuid; parent uuid; parent_occ uuid; occ uuid; block_occ uuid;
  payload bytea; sha kb_sha256; settings_id uuid; settings_sha kb_sha256;
  new_rev uuid := gen_random_uuid(); new_sha kb_sha256; new_payload bytea;
  settings jsonb; ordinal int; depth int; printable_width numeric;
  node_map jsonb := '{}'::jsonb;
BEGIN
  SELECT * INTO w FROM bid_submission_workspaces WHERE id=p_workspace_id;
  IF NOT FOUND THEN RAISE EXCEPTION 'WORKSPACE_NOT_FOUND' USING ERRCODE='P0002'; END IF;
  PERFORM kb_bid_v2_require_project_owner(w.project_id,p_actor);
  SELECT * INTO head FROM bid_workspace_heads WHERE scope_id=p_workspace_id FOR UPDATE;
  IF head.artifact_id IS DISTINCT FROM p_expected_revision_id OR head.artifact_sha256 IS DISTINCT FROM p_expected_sha256 THEN
    RAISE EXCEPTION 'WORKSPACE_HEAD_CAS_MISMATCH' USING ERRCODE='40001';
  END IF;
  SELECT * INTO cur FROM bid_workspace_revision_artifacts WHERE id=head.artifact_id;
  settings := coalesce(p_snapshot->'document_settings', '{}'::jsonb);
  IF settings->>'page_size' IS DISTINCT FROM 'A4' THEN settings := settings || jsonb_build_object('page_size','A4'); END IF;
  payload := kb_bid_v2_json_payload(settings); settings_sha := kb_bid_v2_sha256_bytes(payload);
  IF settings_sha = (SELECT content_sha256 FROM bid_document_settings_revision_artifacts WHERE id=cur.document_settings_revision_id) THEN
    settings_id := cur.document_settings_revision_id;
  ELSE
    settings_id := gen_random_uuid();
    INSERT INTO bid_document_settings_revision_artifacts(id,project_id,workspace_id,revision,schema_version,settings,canonical_payload,content_sha256,actor)
      VALUES(settings_id,w.project_id,p_workspace_id,
        (SELECT coalesce(max(revision),0)+1 FROM bid_document_settings_revision_artifacts WHERE workspace_id=p_workspace_id),
        1,settings,payload,settings_sha,p_actor);
  END IF;
  FOR node IN SELECT value FROM jsonb_array_elements(coalesce(p_snapshot->'nodes','[]'::jsonb))
  LOOP
    lineage := (node->>'lineage_id')::uuid;
    rev := (node->>'revision_id')::uuid;
    INSERT INTO bid_outline_node_lineages(id,project_id,workspace_id)
      VALUES(lineage,w.project_id,p_workspace_id) ON CONFLICT (id) DO NOTHING;
    INSERT INTO bid_outline_node_revision_artifacts(id,project_id,workspace_id,lineage_id,revision,title,semantic_role,render_role,origin,canonical_payload,content_sha256)
      VALUES(rev,w.project_id,p_workspace_id,lineage,
        (SELECT coalesce(max(revision),0)+1 FROM bid_outline_node_revision_artifacts WHERE lineage_id=lineage),
        node->>'title', node->>'semantic_role', node->>'render_role', 'human',
        kb_bid_v2_json_payload(node), kb_bid_v2_sha256_bytes(kb_bid_v2_json_payload(node)))
      ON CONFLICT (id) DO NOTHING;
  END LOOP;
  printable_width:=210
    -coalesce((settings#>>'{margins_mm,left}')::numeric,25.4)
    -coalesce((settings#>>'{margins_mm,right}')::numeric,25.4);
  FOR block IN SELECT value FROM jsonb_array_elements(coalesce(p_snapshot->'blocks','[]'::jsonb))
  LOOP
    lineage := (block->>'lineage_id')::uuid;
    rev := coalesce((block->>'block_revision_id')::uuid, gen_random_uuid());
    IF block->>'kind' IN ('image','attachment_ref') AND NOT EXISTS (
      SELECT 1 FROM bid_workspace_asset_artifacts asset
      WHERE asset.project_id=w.project_id AND asset.workspace_id=p_workspace_id
        AND asset.id=(block#>>'{content,asset_revision_id}')::uuid
        AND NOT EXISTS (SELECT 1 FROM bid_workspace_asset_retirement_artifacts retired WHERE retired.asset_revision_id=asset.id)
    ) THEN RAISE EXCEPTION 'WORKSPACE_ASSET_REFERENCE_INVALID' USING ERRCODE='23514'; END IF;
    IF block->>'kind'='attachment_ref' AND block#>>'{content,render_mode}'='embedded_pages'
       AND (block#>>'{content,preparation_revision_id}') IS NOT NULL AND NOT EXISTS (
         SELECT 1 FROM bid_attachment_preparation_revision_artifacts preparation
         WHERE preparation.project_id=w.project_id AND preparation.workspace_id=p_workspace_id
           AND preparation.id=(block#>>'{content,preparation_revision_id}')::uuid
           AND preparation.source_asset_revision_id=(block#>>'{content,asset_revision_id}')::uuid
           AND preparation.status='ready')
    THEN RAISE EXCEPTION 'ATTACHMENT_PREPARATION_REFERENCE_INVALID' USING ERRCODE='23514'; END IF;
    IF block->>'kind'='structured_form' AND NOT EXISTS (
      SELECT 1 FROM bid_tender_structured_form_definition_artifacts form
      WHERE form.project_id=w.project_id AND form.id=(block#>>'{content,form_definition_revision_id}')::uuid)
    THEN RAISE EXCEPTION 'STRUCTURED_FORM_REFERENCE_INVALID' USING ERRCODE='23514'; END IF;
    IF block->>'kind'='table' AND (SELECT sum((width#>>'{}')::numeric)
      FROM jsonb_array_elements(block#>'{content,widths_mm}') width)>printable_width
    THEN RAISE EXCEPTION 'TABLE_EXCEEDS_PRINTABLE_WIDTH' USING ERRCODE='23514'; END IF;
    IF block->>'kind'='image' AND (block#>>'{content,width_mm}')::numeric>printable_width
    THEN RAISE EXCEPTION 'IMAGE_EXCEEDS_PRINTABLE_WIDTH' USING ERRCODE='23514'; END IF;
    INSERT INTO bid_content_block_lineages(id,project_id,workspace_id)
      VALUES(lineage,w.project_id,p_workspace_id) ON CONFLICT (id) DO NOTHING;
    INSERT INTO bid_content_block_revision_artifacts(id,project_id,workspace_id,lineage_id,revision,schema_version,block_kind,block_payload,origin,canonical_payload,content_sha256)
      VALUES(rev,w.project_id,p_workspace_id,lineage,
        (SELECT coalesce(max(revision),0)+1 FROM bid_content_block_revision_artifacts WHERE lineage_id=lineage),
        1, block->>'kind', coalesce(block->'content', '{}'::jsonb), coalesce(block->>'origin','human'),
        kb_bid_v2_json_payload(block), kb_bid_v2_sha256_bytes(kb_bid_v2_json_payload(block)))
      ON CONFLICT (id) DO NOTHING;
  END LOOP;
  FOR deleted IN
    SELECT artifact.* FROM bid_workspace_node_occurrences occurrence
    JOIN bid_outline_node_revision_artifacts artifact ON artifact.id=occurrence.node_revision_id
    WHERE occurrence.workspace_revision_id=cur.id AND NOT EXISTS (
      SELECT 1 FROM jsonb_array_elements(coalesce(p_snapshot->'nodes','[]'::jsonb)) value
      WHERE (value->>'lineage_id')::uuid=artifact.lineage_id)
  LOOP
    payload:=kb_bid_v2_json_payload(jsonb_build_object('schema_version',1,'lineage_id',deleted.lineage_id,
      'revision',deleted.revision+1,'tombstone',true,'deleted_from_workspace_revision_id',cur.id));sha:=kb_bid_v2_sha256_bytes(payload);
    INSERT INTO bid_outline_node_revision_artifacts(id,project_id,workspace_id,lineage_id,revision,title,
      semantic_role,render_role,origin,tombstone,canonical_payload,content_sha256)
    VALUES(gen_random_uuid(),w.project_id,p_workspace_id,deleted.lineage_id,deleted.revision+1,deleted.title,
      deleted.semantic_role,deleted.render_role,'human',true,payload,sha);
  END LOOP;
  FOR deleted IN
    SELECT DISTINCT artifact.* FROM bid_workspace_block_occurrences occurrence
    JOIN bid_content_block_revision_artifacts artifact ON artifact.id=occurrence.block_revision_id
    WHERE occurrence.workspace_revision_id=cur.id AND NOT EXISTS (
      SELECT 1 FROM jsonb_array_elements(coalesce(p_snapshot->'blocks','[]'::jsonb)) value
      WHERE (value->>'lineage_id')::uuid=artifact.lineage_id)
  LOOP
    payload:=kb_bid_v2_json_payload(jsonb_build_object('schema_version',1,'lineage_id',deleted.lineage_id,
      'revision',deleted.revision+1,'tombstone',true,'deleted_from_workspace_revision_id',cur.id));sha:=kb_bid_v2_sha256_bytes(payload);
    INSERT INTO bid_content_block_revision_artifacts(id,project_id,workspace_id,lineage_id,revision,schema_version,
      block_kind,block_payload,origin,tombstone,canonical_payload,content_sha256)
    VALUES(gen_random_uuid(),w.project_id,p_workspace_id,deleted.lineage_id,deleted.revision+1,1,deleted.block_kind,
      '{}'::jsonb,'human',true,payload,sha);
  END LOOP;
  FOR binding IN SELECT value FROM jsonb_array_elements(coalesce(p_snapshot->'bindings','[]'::jsonb))
  LOOP
    lineage := (binding->>'binding_lineage_id')::uuid;
    rev := (binding->>'binding_revision_id')::uuid;
    IF (binding->>'requirement_projection_revision_id')::uuid IS DISTINCT FROM cur.requirement_projection_id
       OR binding->>'requirement_projection_sha256' IS DISTINCT FROM cur.requirement_projection_sha256
       OR coalesce(binding->>'state','bound') NOT IN ('bound','unbound','superseded')
       OR NOT EXISTS (SELECT 1 FROM bid_workspace_requirement_projection_items item
         JOIN bid_requirement_revision_artifacts requirement ON requirement.id=item.requirement_revision_id
         WHERE item.projection_id=cur.requirement_projection_id
           AND (binding->>'need_occurrence_id')::uuid=ANY(kb_bid_v2_fulfillment_need_ids(requirement.fulfillment_expr))) THEN
      RAISE EXCEPTION 'BINDING_REQUIREMENT_PROJECTION_MISMATCH' USING ERRCODE='23514';
    END IF;
    IF coalesce(binding->>'state','bound')='bound' AND (
      (binding#>>'{target,kind}'='outline_node' AND NOT EXISTS (SELECT 1 FROM jsonb_array_elements(p_snapshot->'nodes') value WHERE (value->>'lineage_id')::uuid=(binding#>>'{target,node_lineage_id}')::uuid))
      OR (binding#>>'{target,kind}'='response_table' AND NOT EXISTS (SELECT 1 FROM jsonb_array_elements(p_snapshot->'blocks') value WHERE (value->>'lineage_id')::uuid=(binding#>>'{target,block_lineage_id}')::uuid AND value->>'kind'='table'))
      OR (binding#>>'{target,kind}'='structured_form' AND NOT EXISTS (SELECT 1 FROM bid_tender_structured_form_definition_artifacts form WHERE form.project_id=w.project_id AND form.id=(binding#>>'{target,form_definition_revision_id}')::uuid))
      OR (binding#>>'{target,kind}'='quote' AND NOT EXISTS (SELECT 1 FROM bid_quote_snapshot_artifacts quote
        WHERE quote.project_id=w.project_id AND quote.id=cur.quote_snapshot_id
          AND quote.content_sha256=cur.quote_snapshot_sha256
          AND quote.id=(binding#>>'{target,quote_snapshot_id}')::uuid))
    ) THEN RAISE EXCEPTION 'BINDING_TARGET_INVALID' USING ERRCODE='23514'; END IF;
    INSERT INTO bid_outline_fulfillment_binding_lineages(id,project_id,workspace_id)
      VALUES(lineage,w.project_id,p_workspace_id) ON CONFLICT (id) DO NOTHING;
    INSERT INTO bid_outline_fulfillment_binding_revision_artifacts(
      id,project_id,workspace_id,lineage_id,revision,need_occurrence_id,
      requirement_projection_id,channel,target_kind,target_id,target_node_id,candidate_id,state,reason,actor,
      canonical_payload,content_sha256)
    VALUES(rev,w.project_id,p_workspace_id,lineage,coalesce((binding->>'revision')::bigint,1),
      (binding->>'need_occurrence_id')::uuid,
      (binding->>'requirement_projection_revision_id')::uuid,binding->>'channel',
      binding#>>'{target,kind}',coalesce(
        (binding#>>'{target,node_lineage_id}')::uuid,
        (binding#>>'{target,block_lineage_id}')::uuid,
        (binding#>>'{target,form_definition_revision_id}')::uuid,
        (binding#>>'{target,quote_snapshot_id}')::uuid),
      CASE WHEN binding#>>'{target,kind}'='outline_node' THEN (binding#>>'{target,node_lineage_id}')::uuid END,NULL,
      coalesce(binding->>'state','bound'),binding->>'reason',p_actor,kb_bid_v2_json_payload(binding),
      kb_bid_v2_sha256_bytes(kb_bid_v2_json_payload(binding)))
    ON CONFLICT (id) DO NOTHING;
  END LOOP;
  new_payload := kb_bid_v2_json_payload(p_snapshot); new_sha := kb_bid_v2_sha256_bytes(new_payload);
  INSERT INTO bid_workspace_revision_artifacts(id,project_id,workspace_id,revision,parent_revision_id,parent_sha256,scope_revision_id,requirement_projection_id,requirement_projection_sha256,document_settings_revision_id,quote_snapshot_id,quote_snapshot_sha256,canonical_payload,content_sha256,actor)
    VALUES(new_rev,w.project_id,p_workspace_id,cur.revision+1,cur.id,cur.content_sha256,cur.scope_revision_id,cur.requirement_projection_id,cur.requirement_projection_sha256,settings_id,cur.quote_snapshot_id,cur.quote_snapshot_sha256,new_payload,new_sha,p_actor);
  -- Insert occurrences in explicit depth order: every parent must already exist.
  FOR node IN
    SELECT value FROM jsonb_array_elements(coalesce(p_snapshot->'nodes','[]'::jsonb))
     ORDER BY coalesce((value->>'depth')::int,0),coalesce((value->>'ordinal')::int,0),value->>'lineage_id'
  LOOP
    occ := gen_random_uuid();
    lineage := (node->>'lineage_id')::uuid;
    rev := (node->>'revision_id')::uuid;
    parent := NULLIF(node->>'parent_lineage_id','null')::uuid;
    parent_occ := CASE WHEN parent IS NULL THEN NULL ELSE NULLIF(node_map->>parent::text,'')::uuid END;
    depth := coalesce((node->>'depth')::int,0);
    IF parent IS NOT NULL AND parent_occ IS NULL THEN
      RAISE EXCEPTION 'WORKSPACE_PARENT_OCCURRENCE_MISSING' USING ERRCODE='23514';
    END IF;
    INSERT INTO bid_workspace_node_occurrences(id,project_id,workspace_revision_id,node_revision_id,parent_occurrence_id,ordinal,depth)
      VALUES(occ,w.project_id,new_rev,rev,parent_occ,coalesce((node->>'ordinal')::int,0),depth);
    node_map := node_map || jsonb_build_object(lineage::text, occ);
    ordinal := 0;
    FOR block_occ IN SELECT (jsonb_array_elements_text(coalesce(node->'block_lineage_ids','[]'::jsonb)))::uuid
    LOOP
      SELECT b.id INTO rev FROM bid_content_block_revision_artifacts b
        WHERE b.lineage_id=block_occ AND b.project_id=w.project_id
        ORDER BY b.revision DESC LIMIT 1;
      INSERT INTO bid_workspace_block_occurrences(id,project_id,workspace_revision_id,node_occurrence_id,block_revision_id,ordinal)
        VALUES(gen_random_uuid(),w.project_id,new_rev,occ,rev,ordinal);
      ordinal := ordinal+1;
    END LOOP;
  END LOOP;
  ordinal := 0;
  FOR binding IN SELECT value FROM jsonb_array_elements(coalesce(p_snapshot->'bindings','[]'::jsonb))
  LOOP
    INSERT INTO bid_workspace_binding_occurrences(
      id,project_id,workspace_revision_id,binding_revision_id,ordinal)
    VALUES(gen_random_uuid(),w.project_id,new_rev,
      (binding->>'binding_revision_id')::uuid,ordinal);
    ordinal := ordinal+1;
  END LOOP;
  FOR edge IN SELECT value FROM jsonb_array_elements(coalesce(p_snapshot->'lineage_edges','[]'::jsonb))
  LOOP
    INSERT INTO bid_outline_lineage_edges(
      id,project_id,workspace_id,operation,from_lineage_id,to_lineage_id,workspace_revision_id)
    VALUES(gen_random_uuid(),w.project_id,p_workspace_id,
      CASE edge->>'kind' WHEN 'split_from' THEN 'split' ELSE 'merge' END,
      (edge->>'from_lineage_id')::uuid,(edge->>'to_lineage_id')::uuid,new_rev);
  END LOOP;
  IF NOT kb_bid_v2_advance_workspace_head(p_workspace_id,p_expected_revision_id,p_expected_sha256,new_rev,new_sha) THEN
    RAISE EXCEPTION 'WORKSPACE_HEAD_CAS_MISMATCH' USING ERRCODE='40001';
  END IF;
  RETURN kb_bid_v2_load_workspace(p_workspace_id);
END $$;

-- Only accepting a current generated candidate records new fulfillment evidence.
-- Generic Workspace edits deliberately leave prior evidence immutable so its
-- effective state becomes stale when exact target/dependency identities change.
CREATE FUNCTION kb_bid_v2_record_accepted_candidate_evidence(
  p_workspace_id uuid,p_workspace_revision_id uuid,p_candidate_id uuid,p_selected_ordinals integer[]
) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE candidate bid_candidate_artifacts%ROWTYPE; operation record; binding record;
  block_revision bid_content_block_revision_artifacts%ROWTYPE; target_node_lineage_id uuid;
  evidence_kind text; evidence_lineage uuid; evidence_revision bigint; payload bytea; digest kb_sha256;
BEGIN
  SELECT * INTO STRICT candidate FROM bid_candidate_artifacts WHERE id=p_candidate_id AND workspace_id=p_workspace_id;
  FOR operation IN
    SELECT candidate_operation.operation FROM bid_candidate_operations candidate_operation
    WHERE candidate_operation.candidate_id=p_candidate_id
      AND candidate_operation.ordinal=ANY(p_selected_ordinals)
      AND candidate_operation.operation->>'kind'='insert_block'
    ORDER BY candidate_operation.ordinal
  LOOP
    target_node_lineage_id:=(operation.operation->>'target_node_lineage_id')::uuid;
    SELECT block.* INTO STRICT block_revision
    FROM bid_workspace_block_occurrences occurrence
    JOIN bid_content_block_revision_artifacts block ON block.id=occurrence.block_revision_id
    WHERE occurrence.workspace_revision_id=p_workspace_revision_id
      AND block.lineage_id=(operation.operation#>>'{block,lineage_id}')::uuid;
    FOR binding IN
      SELECT value.* FROM bid_workspace_binding_occurrences occurrence
      JOIN bid_outline_fulfillment_binding_revision_artifacts value
        ON value.id=occurrence.binding_revision_id
      WHERE occurrence.workspace_revision_id=p_workspace_revision_id AND value.state='bound'
        AND value.requirement_projection_id=(SELECT revision.requirement_projection_id
          FROM bid_workspace_revision_artifacts revision WHERE revision.id=p_workspace_revision_id)
        AND ((value.target_kind='outline_node' AND value.target_id=target_node_lineage_id)
          OR (value.target_kind='response_table' AND value.target_id=block_revision.lineage_id)
          OR (value.target_kind='structured_form' AND block_revision.block_kind='structured_form'
            AND value.target_id=(block_revision.block_payload->>'form_definition_revision_id')::uuid))
      ORDER BY occurrence.ordinal
    LOOP
      evidence_kind:=CASE WHEN binding.target_kind='structured_form' THEN 'structured_value' ELSE 'block' END;
      IF NOT EXISTS (SELECT 1 FROM bid_submission_fulfillment_evidence_revision_artifacts existing
          WHERE existing.binding_revision_id=binding.id AND existing.target_revision_id=block_revision.id
            AND existing.target_kind=evidence_kind AND existing.dependency_sha256=block_revision.content_sha256) THEN
        evidence_lineage:=kb_bid_v2_deterministic_uuid('fulfillment-evidence:'||binding.lineage_id::text||':'||block_revision.id::text);
        SELECT coalesce(max(value.revision),0)+1 INTO evidence_revision
          FROM bid_submission_fulfillment_evidence_revision_artifacts value
          WHERE value.project_id=candidate.project_id AND value.evidence_lineage_id=evidence_lineage;
        payload:=kb_bid_v2_json_payload(jsonb_build_object('schema_version',1,
          'evidence_lineage_id',evidence_lineage,'revision',evidence_revision,
          'workspace_revision_id',p_workspace_revision_id,'binding_revision_id',binding.id,
          'target_revision_id',block_revision.id,'target_kind',evidence_kind,
          'dependency_sha256',block_revision.content_sha256));
        digest:=kb_bid_v2_sha256_bytes(payload);
        INSERT INTO bid_submission_fulfillment_evidence_revision_artifacts(
          id,project_id,workspace_id,evidence_lineage_id,revision,workspace_revision_id,
          binding_revision_id,target_revision_id,target_kind,dependency_sha256,canonical_payload,content_sha256)
        VALUES(gen_random_uuid(),candidate.project_id,p_workspace_id,evidence_lineage,evidence_revision,
          p_workspace_revision_id,binding.id,block_revision.id,evidence_kind,block_revision.content_sha256,payload,digest);
      END IF;
    END LOOP;
  END LOOP;
END $$;

-- User-visible V2 vertical-flow application procedures.  These functions keep
-- every mutation behind an authenticated actor, shared idempotency receipt and
-- aggregate CAS; the API role never receives direct DML on authoring tables.
CREATE FUNCTION kb_bid_v2_require_project_owner(
  p_project_id uuid,p_actor kb_actor_identity
) RETURNS void LANGUAGE plpgsql STABLE SET search_path=pg_catalog,public AS $$
BEGIN
  IF p_actor NOT LIKE 'user:%' THEN
    RAISE EXCEPTION 'USER_ACTOR_REQUIRED' USING ERRCODE='42501';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM bid_projects WHERE id=p_project_id) THEN
    RAISE EXCEPTION 'PROJECT_NOT_FOUND' USING ERRCODE='P0002';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM bid_projects
      WHERE id=p_project_id AND owner_user_id=split_part(p_actor,':',2)::uuid) THEN
    RAISE EXCEPTION 'PROJECT_OWNER_REQUIRED' USING ERRCODE='42501';
  END IF;
END $$;

CREATE FUNCTION kb_bid_v2_idempotency_begin(
  p_actor kb_actor_identity,p_operation text,p_key text,
  p_request_bytes bytea,p_request_sha256 kb_sha256
) RETURNS bytea LANGUAGE plpgsql SET search_path=pg_catalog,public AS $$
DECLARE saved idempotency_requests%ROWTYPE;
BEGIN
  IF p_request_sha256<>kb_bid_v2_sha256_bytes(p_request_bytes) THEN
    RAISE EXCEPTION 'REQUEST_PAYLOAD_HASH_MISMATCH' USING ERRCODE='22023';
  END IF;
  INSERT INTO idempotency_requests(actor_identity,operation,idempotency_key,schema_version,
    request_bytes,request_sha256,state)
  VALUES(p_actor,p_operation,p_key,1,p_request_bytes,p_request_sha256,'intent')
  ON CONFLICT DO NOTHING;
  SELECT * INTO STRICT saved FROM idempotency_requests
    WHERE actor_identity=p_actor AND operation=p_operation AND idempotency_key=p_key FOR UPDATE;
  IF saved.request_sha256<>p_request_sha256 OR saved.request_bytes<>p_request_bytes THEN
    RAISE EXCEPTION 'IDEMPOTENCY_PAYLOAD_MISMATCH' USING ERRCODE='23505';
  END IF;
  IF saved.state='completed' THEN RETURN saved.response_bytes; END IF;
  RETURN NULL;
END $$;

CREATE FUNCTION kb_bid_v2_idempotency_replay(
  p_actor kb_actor_identity,p_operation text,p_key text,
  p_request_bytes bytea,p_request_sha256 kb_sha256
) RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE saved idempotency_requests%ROWTYPE;
BEGIN
  IF p_request_sha256<>kb_bid_v2_sha256_bytes(p_request_bytes) THEN
    RAISE EXCEPTION 'REQUEST_PAYLOAD_HASH_MISMATCH' USING ERRCODE='22023';
  END IF;
  SELECT * INTO saved FROM idempotency_requests
    WHERE actor_identity=p_actor AND operation=p_operation AND idempotency_key=p_key;
  IF NOT FOUND THEN RETURN NULL; END IF;
  IF saved.request_sha256<>p_request_sha256 OR saved.request_bytes<>p_request_bytes THEN
    RAISE EXCEPTION 'IDEMPOTENCY_PAYLOAD_MISMATCH' USING ERRCODE='23505';
  END IF;
  IF saved.state='completed' THEN RETURN convert_from(saved.response_bytes,'UTF8')::jsonb; END IF;
  RETURN NULL;
END $$;

CREATE FUNCTION kb_bid_v2_idempotency_complete(
  p_actor kb_actor_identity,p_operation text,p_key text,
  p_status integer,p_response bytea
) RETURNS void LANGUAGE plpgsql SET search_path=pg_catalog,public AS $$
BEGIN
  UPDATE idempotency_requests SET state='completed',response_status=p_status,
    response_bytes=p_response,response_sha256=kb_bid_v2_sha256_bytes(p_response),
    completed_at=clock_timestamp()
  WHERE actor_identity=p_actor AND operation=p_operation
    AND idempotency_key=p_key AND state='intent';
  IF NOT FOUND THEN
    RAISE EXCEPTION 'IDEMPOTENCY_INTENT_MISSING' USING ERRCODE='40001';
  END IF;
END $$;

CREATE FUNCTION kb_bid_v2_create_project(
  p_id uuid,p_title text,p_owner_user_id uuid,p_actor kb_actor_identity,
  p_idempotency_key text,p_request_bytes bytea,p_request_sha256 kb_sha256
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE replay bytea; response jsonb; response_bytes bytea;
BEGIN
  IF p_actor<>'user:'||p_owner_user_id::text THEN
    RAISE EXCEPTION 'PROJECT_OWNER_ACTOR_MISMATCH' USING ERRCODE='42501';
  END IF;
  replay:=kb_bid_v2_idempotency_begin(p_actor,'bid.v2.project.create',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  response:=kb_bid_create_project_v2(p_id,p_title,p_owner_user_id,p_actor);
  response_bytes:=convert_to(response::text,'UTF8');
  INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,
    request_sha256,response_sha256,entity_kind,entity_locator,after_revision,after_sha256)
  VALUES(gen_random_uuid(),1,'bid.v2.project.create',p_actor,p_idempotency_key,p_request_sha256,
    kb_bid_v2_sha256_bytes(response_bytes),'bid_v2_project',jsonb_build_object('project_id',p_id),
    1,kb_bid_v2_sha256_bytes(response_bytes));
  PERFORM kb_bid_v2_idempotency_complete(p_actor,'bid.v2.project.create',p_idempotency_key,201,response_bytes);
  RETURN response;
END $$;

CREATE FUNCTION kb_bid_v2_list_projects(
  p_owner_user_id uuid,p_actor kb_actor_identity
) RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
BEGIN
  IF p_actor NOT LIKE 'user:%' THEN
    RAISE EXCEPTION 'USER_ACTOR_REQUIRED' USING ERRCODE='42501';
  END IF;
  RETURN COALESCE((SELECT jsonb_agg(jsonb_build_object(
      'id',p.id,'title',p.title,'status',p.status,'ended_at',p.ended_at,
      'workspace_id',w.id,'owner_user_id',p.owner_user_id) ORDER BY p.created_at,p.id)
    FROM bid_projects p JOIN bid_submission_workspaces w ON w.project_id=p.id),'[]'::jsonb);
END $$;

CREATE FUNCTION kb_bid_v2_get_project(
  p_project_id uuid,p_actor kb_actor_identity
) RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE response jsonb;
BEGIN
  PERFORM kb_bid_v2_require_project_owner(p_project_id,p_actor);
  SELECT jsonb_build_object('id',p.id,'title',p.title,'status',p.status,
    'ended_at',p.ended_at,'workspace_id',w.id,'owner_user_id',p.owner_user_id)
  INTO response FROM bid_projects p JOIN bid_submission_workspaces w ON w.project_id=p.id
  WHERE p.id=p_project_id;
  RETURN response;
END $$;

CREATE FUNCTION kb_bid_v2_end_project(
  p_project_id uuid,p_actor kb_actor_identity,p_idempotency_key text,
  p_request_bytes bytea,p_request_sha256 kb_sha256
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE replay bytea; response jsonb; response_bytes bytea;
BEGIN
  PERFORM kb_bid_v2_require_project_owner(p_project_id,p_actor);
  replay:=kb_bid_v2_idempotency_begin(p_actor,'bid.v2.project.end',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  UPDATE bid_projects SET status='ended',ended_at=clock_timestamp()
    WHERE id=p_project_id AND status='open';
  response:=jsonb_build_object('project_id',p_project_id,'status','ended');
  response_bytes:=convert_to(response::text,'UTF8');
  INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,
    request_sha256,response_sha256,entity_kind,entity_locator,after_revision,after_sha256)
  VALUES(gen_random_uuid(),1,'bid.v2.project.end',p_actor,p_idempotency_key,p_request_sha256,
    kb_bid_v2_sha256_bytes(response_bytes),'bid_v2_project',jsonb_build_object('project_id',p_project_id),
    1,kb_bid_v2_sha256_bytes(response_bytes));
  PERFORM kb_bid_v2_idempotency_complete(p_actor,'bid.v2.project.end',p_idempotency_key,200,response_bytes);
  RETURN response;
END $$;

CREATE FUNCTION kb_bid_v2_upload_tender_document(
  p_staging_id uuid,p_document_id uuid,p_request_artifact_id uuid,p_project_id uuid,
  p_file_name text,p_media_type text,p_byte_length bigint,p_object_ref kb_object_ref,
  p_original_sha256 kb_sha256,p_actor kb_actor_identity,p_idempotency_key text,
  p_request_bytes bytea,p_request_sha256 kb_sha256
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE
  replay bytea; response jsonb; response_bytes bytea; role_id uuid:=gen_random_uuid();
  role_value text; role_payload bytea; role_sha kb_sha256;
  converter_id uuid:='00000000-0000-5000-8000-000000000001';
  converter_payload bytea:=convert_to('docreader-grpc-structured-source-v3','UTF8');
  converter_sha kb_sha256; frozen_payload bytea; frozen_sha kb_sha256;
  job_payload jsonb; job_bytes bytea; job_sha kb_sha256;
BEGIN
  PERFORM kb_bid_v2_require_project_owner(p_project_id,p_actor);
  replay:=kb_bid_v2_idempotency_begin(p_actor,'bid.v2.tender.upload',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN
    PERFORM kb_object_upload_abandon(p_staging_id,p_actor);
    RETURN convert_from(replay,'UTF8')::jsonb;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM bid_projects WHERE id=p_project_id AND status='open') THEN
    RAISE EXCEPTION 'PROJECT_ENDED' USING ERRCODE='55000';
  END IF;
  IF EXISTS (
    SELECT 1 FROM bid_documents
     WHERE project_id=p_project_id AND original_sha256=p_original_sha256
  ) THEN
    PERFORM kb_object_upload_abandon(p_staging_id,p_actor);
    RAISE EXCEPTION 'TENDER_DOCUMENT_DUPLICATE' USING ERRCODE='23514';
  END IF;
  PERFORM kb_object_upload_commit(p_staging_id,p_object_ref,p_original_sha256,p_media_type,
    p_byte_length,'bid_document',p_document_id,'original',p_actor);
  INSERT INTO bid_documents(id,project_id,file_name,media_type,byte_length,original_object_ref,original_sha256,parse_status)
  VALUES(p_document_id,p_project_id,p_file_name,p_media_type,p_byte_length,p_object_ref,p_original_sha256,'pending');
  role_value:=CASE
    WHEN p_file_name~*'(clarification|澄清)' THEN 'clarification'
    WHEN p_file_name~*'(amendment|补充|变更)' THEN 'amendment'
    WHEN p_file_name~*'(boq|清单|报价)' THEN 'bill_of_quantities'
    WHEN p_file_name~*'(technical|技术)' THEN 'technical_specification'
    WHEN NOT EXISTS (SELECT 1 FROM bid_documents d WHERE d.project_id=p_project_id AND d.id<>p_document_id) THEN 'primary_tender'
    ELSE 'other_attachment' END;
  role_payload:=kb_bid_v2_json_payload(jsonb_build_object('schema_version',1,'project_id',p_project_id,
    'document_id',p_document_id,'revision',1,'role',role_value,'provenance','system_suggested'));
  role_sha:=kb_bid_v2_sha256_bytes(role_payload);
  INSERT INTO bid_document_role_revision_artifacts(id,project_id,document_id,revision,role,provenance,
    canonical_payload,content_sha256,actor)
  VALUES(role_id,p_project_id,p_document_id,1,role_value,'system_suggested',role_payload,role_sha,p_actor);
  INSERT INTO bid_document_role_current(scope_id,project_id,artifact_id,generation,created_at)
  VALUES(p_document_id,p_project_id,role_id,1,clock_timestamp());
  converter_sha:=kb_bid_v2_sha256_bytes(converter_payload);
  INSERT INTO bid_authoring_contract_artifacts(id,contract_kind,schema_version,canonical_payload,content_sha256)
  -- Concurrent first uploads may race on either unique index (id or id/digest).
  -- The exact contract assertion below still rejects a different definition.
  VALUES(converter_id,'converter',1,converter_payload,converter_sha) ON CONFLICT DO NOTHING;
  IF NOT EXISTS (SELECT 1 FROM bid_authoring_contract_artifacts
      WHERE id=converter_id AND content_sha256=converter_sha AND contract_kind='converter') THEN
    RAISE EXCEPTION 'CONVERTER_CONTRACT_CONFLICT' USING ERRCODE='23505';
  END IF;
  frozen_payload:=kb_bid_v2_json_payload(jsonb_build_object('schema_version',1,'project_id',p_project_id,
    'document_id',p_document_id,'document_sha256',p_original_sha256,'role_revision_id',role_id,
    'role_revision_sha256',role_sha,'converter_contract_id',converter_id,
    'converter_contract_sha256',converter_sha));
  frozen_sha:=kb_bid_v2_sha256_bytes(frozen_payload);
  job_payload:=jsonb_build_object('job_kind','tender_document_process','request',jsonb_build_object(
    'request_artifact_id',p_request_artifact_id,'request_revision',1,'frozen_input_sha256',frozen_sha),
    'project_id',p_project_id,'document_revision_id',p_document_id);
  job_bytes:=kb_bid_v2_json_payload(job_payload); job_sha:=kb_bid_v2_sha256_bytes(job_bytes);
  INSERT INTO bid_async_request_snapshot_artifacts(id,project_id,workspace_id,request_kind,revision,
    frozen_input_sha256,request_payload,request_sha256,status)
  VALUES(p_request_artifact_id,p_project_id,NULL,'tender_document_process',1,frozen_sha,job_bytes,job_sha,'pending');
  INSERT INTO bid_tender_document_process_request_identities(request_artifact_id,project_id,request_revision,
    request_sha256,frozen_input_sha256,document_id,document_sha256,role_revision_id,role_revision_sha256,
    converter_contract_id,converter_contract_sha256)
  VALUES(p_request_artifact_id,p_project_id,1,job_sha,frozen_sha,p_document_id,p_original_sha256,
    role_id,role_sha,converter_id,converter_sha);
  response:=jsonb_build_object('id',p_document_id,'project_id',p_project_id,'file_name',p_file_name,
    'media_type',p_media_type,'byte_length',p_byte_length,'original_sha256',p_original_sha256,
    'parse_status','pending','conversion_generation',1,'error_code',NULL,
    'document_role',role_value,'role_revision_id',role_id,
    'role_revision_sha256',role_sha,'role_provenance','system_suggested',
    'request_artifact_id',p_request_artifact_id,'request_revision',1,'request_sha256',job_sha,
    'frozen_input_sha256',frozen_sha);
  response_bytes:=convert_to(response::text,'UTF8');
  INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,
    request_sha256,response_sha256,entity_kind,entity_locator,after_revision,after_sha256)
  VALUES(gen_random_uuid(),1,'bid.v2.tender.upload',p_actor,p_idempotency_key,p_request_sha256,
    kb_bid_v2_sha256_bytes(response_bytes),'bid_v2_document',jsonb_build_object('document_id',p_document_id),1,p_original_sha256);
  PERFORM kb_bid_v2_idempotency_complete(p_actor,'bid.v2.tender.upload',p_idempotency_key,201,response_bytes);
  RETURN response;
END $$;

CREATE FUNCTION kb_bid_v2_list_tender_documents(
  p_project_id uuid,p_actor kb_actor_identity
) RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
BEGIN
  PERFORM kb_bid_v2_require_project_owner(p_project_id,p_actor);
  RETURN COALESCE((SELECT jsonb_agg(jsonb_build_object(
      'id',d.id,'project_id',d.project_id,'file_name',d.file_name,'media_type',d.media_type,
      'byte_length',d.byte_length,'original_sha256',d.original_sha256,'parse_status',d.parse_status,
      'conversion_generation',(SELECT count(*) FROM bid_tender_document_process_request_identities attempt
        WHERE attempt.project_id=d.project_id AND attempt.document_id=d.id),
      'error_code',(SELECT request_value.error_code FROM bid_tender_document_process_request_identities attempt
        JOIN bid_async_request_snapshot_artifacts request_value ON request_value.id=attempt.request_artifact_id
        WHERE attempt.project_id=d.project_id AND attempt.document_id=d.id
        ORDER BY request_value.created_at DESC,request_value.id DESC LIMIT 1),
      'document_role',r.role,'role_revision_id',r.id,'role_revision_sha256',r.content_sha256,
      'role_provenance',r.provenance,'source_revision_id',s.id,'source_revision_sha256',s.source_sha256)
      ORDER BY d.created_at,d.id)
    FROM bid_documents d
    JOIN bid_document_role_current rc ON rc.scope_id=d.id AND rc.project_id=d.project_id
    JOIN bid_document_role_revision_artifacts r ON r.id=rc.artifact_id AND r.project_id=d.project_id
    LEFT JOIN LATERAL (SELECT value.id,value.source_sha256 FROM bid_converted_source_artifacts value
      WHERE value.project_id=d.project_id AND value.document_id=d.id ORDER BY value.revision DESC LIMIT 1) s ON true
    WHERE d.project_id=p_project_id),'[]'::jsonb);
END $$;

CREATE FUNCTION kb_bid_v2_retry_tender_document(
  p_project_id uuid,p_document_id uuid,p_request_artifact_id uuid,p_expected_generation bigint,p_actor kb_actor_identity,
  p_idempotency_key text,p_request_bytes bytea,p_request_sha256 kb_sha256
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE replay bytea; response jsonb; response_bytes bytea; document_value bid_documents%ROWTYPE;
  role_value bid_document_role_revision_artifacts%ROWTYPE; converter bid_authoring_contract_artifacts%ROWTYPE;
  frozen_payload bytea; frozen_sha kb_sha256; job_payload jsonb; job_bytes bytea; job_sha kb_sha256;
  generation_value bigint;
BEGIN
  PERFORM kb_bid_v2_require_project_owner(p_project_id,p_actor);
  replay:=kb_bid_v2_idempotency_begin(p_actor,'bid.v2.tender.retry',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  SELECT * INTO STRICT document_value FROM bid_documents
    WHERE project_id=p_project_id AND id=p_document_id FOR UPDATE;
  SELECT count(*) INTO generation_value FROM bid_tender_document_process_request_identities
    WHERE project_id=p_project_id AND document_id=p_document_id;
  IF generation_value<>p_expected_generation THEN
    RAISE EXCEPTION 'TENDER_DOCUMENT_GENERATION_CAS_MISMATCH' USING ERRCODE='40001';
  END IF;
  IF document_value.parse_status<>'failed' THEN
    RAISE EXCEPTION 'TENDER_DOCUMENT_NOT_RETRYABLE' USING ERRCODE='23514';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM bid_projects WHERE id=p_project_id AND status='open') THEN
    RAISE EXCEPTION 'PROJECT_ENDED' USING ERRCODE='55000';
  END IF;
  SELECT role.* INTO STRICT role_value
    FROM bid_document_role_current head JOIN bid_document_role_revision_artifacts role
      ON role.project_id=head.project_id AND role.id=head.artifact_id
    WHERE head.project_id=p_project_id AND head.scope_id=p_document_id;
  SELECT * INTO STRICT converter FROM bid_authoring_contract_artifacts
    WHERE id='00000000-0000-5000-8000-000000000001' AND contract_kind='converter';
  frozen_payload:=kb_bid_v2_json_payload(jsonb_build_object('schema_version',1,'project_id',p_project_id,
    'document_id',p_document_id,'document_sha256',document_value.original_sha256,
    'role_revision_id',role_value.id,'role_revision_sha256',role_value.content_sha256,
    'converter_contract_id',converter.id,'converter_contract_sha256',converter.content_sha256));
  frozen_sha:=kb_bid_v2_sha256_bytes(frozen_payload);
  job_payload:=jsonb_build_object('job_kind','tender_document_process','request',jsonb_build_object(
    'request_artifact_id',p_request_artifact_id,'request_revision',1,'frozen_input_sha256',frozen_sha),
    'project_id',p_project_id,'document_revision_id',p_document_id);
  job_bytes:=kb_bid_v2_json_payload(job_payload); job_sha:=kb_bid_v2_sha256_bytes(job_bytes);
  INSERT INTO bid_async_request_snapshot_artifacts(id,project_id,workspace_id,request_kind,revision,
    frozen_input_sha256,request_payload,request_sha256,status)
  VALUES(p_request_artifact_id,p_project_id,NULL,'tender_document_process',1,frozen_sha,job_bytes,job_sha,'pending');
  INSERT INTO bid_tender_document_process_request_identities(request_artifact_id,project_id,request_revision,
    request_sha256,frozen_input_sha256,document_id,document_sha256,role_revision_id,role_revision_sha256,
    converter_contract_id,converter_contract_sha256)
  VALUES(p_request_artifact_id,p_project_id,1,job_sha,frozen_sha,p_document_id,document_value.original_sha256,
    role_value.id,role_value.content_sha256,converter.id,converter.content_sha256);
  UPDATE bid_documents SET parse_status='pending'
    WHERE project_id=p_project_id AND id=p_document_id;
  SELECT count(*) INTO generation_value FROM bid_tender_document_process_request_identities
    WHERE project_id=p_project_id AND document_id=p_document_id;
  response:=jsonb_build_object('id',p_document_id,'project_id',p_project_id,
    'file_name',document_value.file_name,'media_type',document_value.media_type,
    'byte_length',document_value.byte_length,'original_sha256',document_value.original_sha256,
    'parse_status','pending','conversion_generation',generation_value,'error_code',NULL,
    'document_role',role_value.role,'role_revision_id',role_value.id,
    'role_revision_sha256',role_value.content_sha256,'role_provenance',role_value.provenance,
    'request_artifact_id',p_request_artifact_id,'request_revision',1,'request_sha256',job_sha,
    'frozen_input_sha256',frozen_sha);
  response_bytes:=convert_to(response::text,'UTF8');
  INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,
    request_sha256,response_sha256,entity_kind,entity_locator,after_revision,after_sha256)
  VALUES(gen_random_uuid(),1,'bid.v2.tender.retry',p_actor,p_idempotency_key,p_request_sha256,
    kb_bid_v2_sha256_bytes(response_bytes),'bid_v2_document',jsonb_build_object('document_id',p_document_id),
    generation_value,document_value.original_sha256);
  PERFORM kb_bid_v2_idempotency_complete(p_actor,'bid.v2.tender.retry',p_idempotency_key,202,response_bytes);
  RETURN response;
END $$;

CREATE FUNCTION kb_bid_v2_patch_document_role(
  p_project_id uuid,p_document_id uuid,p_role text,p_expected_artifact_id uuid,
  p_expected_sha256 kb_sha256,p_actor kb_actor_identity,p_idempotency_key text,
  p_request_bytes bytea,p_request_sha256 kb_sha256
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE replay bytea; response jsonb; response_bytes bytea; head bid_document_role_current%ROWTYPE;
  prior bid_document_role_revision_artifacts%ROWTYPE; new_id uuid:=gen_random_uuid();
  new_revision bigint; provenance text; payload bytea; sha kb_sha256;
BEGIN
  PERFORM kb_bid_v2_require_project_owner(p_project_id,p_actor);
  replay:=kb_bid_v2_idempotency_begin(p_actor,'bid.v2.document.role',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  SELECT * INTO STRICT head FROM bid_document_role_current WHERE scope_id=p_document_id AND project_id=p_project_id FOR UPDATE;
  SELECT * INTO STRICT prior FROM bid_document_role_revision_artifacts WHERE id=head.artifact_id AND project_id=p_project_id;
  IF head.artifact_id<>p_expected_artifact_id OR prior.content_sha256<>p_expected_sha256 THEN
    RAISE EXCEPTION 'DOCUMENT_ROLE_CAS_MISMATCH' USING ERRCODE='40001';
  END IF;
  new_revision:=head.generation+1;
  provenance:=CASE WHEN prior.role=p_role THEN 'human_confirmed' ELSE 'human_modified' END;
  payload:=kb_bid_v2_json_payload(jsonb_build_object('schema_version',1,'project_id',p_project_id,
    'document_id',p_document_id,'revision',new_revision,'role',p_role,'provenance',provenance));
  sha:=kb_bid_v2_sha256_bytes(payload);
  INSERT INTO bid_document_role_revision_artifacts(id,project_id,document_id,revision,role,provenance,
    canonical_payload,content_sha256,actor)
  VALUES(new_id,p_project_id,p_document_id,new_revision,p_role,provenance,payload,sha,p_actor);
  UPDATE bid_document_role_current SET artifact_id=new_id,generation=new_revision WHERE scope_id=p_document_id;
  response:=jsonb_build_object('id',p_document_id,'project_id',p_project_id,'document_role',p_role,
    'role_revision_id',new_id,'role_revision_sha256',sha,'role_provenance',provenance);
  response_bytes:=convert_to(response::text,'UTF8');
  INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,
    request_sha256,response_sha256,entity_kind,entity_locator,before_revision,before_sha256,after_revision,after_sha256)
  VALUES(gen_random_uuid(),1,'bid.v2.document.role',p_actor,p_idempotency_key,p_request_sha256,
    kb_bid_v2_sha256_bytes(response_bytes),'bid_v2_document_role',jsonb_build_object('document_id',p_document_id),
    prior.revision,prior.content_sha256,new_revision,sha);
  PERFORM kb_bid_v2_idempotency_complete(p_actor,'bid.v2.document.role',p_idempotency_key,200,response_bytes);
  RETURN response;
END $$;

CREATE FUNCTION kb_bid_v2_upsert_document_relation(
  p_project_id uuid,p_lineage_id uuid,p_from_document_id uuid,p_to_document_id uuid,
  p_relation_kind text,p_applicability jsonb,p_expected_artifact_id uuid,p_expected_sha256 kb_sha256,
  p_actor kb_actor_identity,p_idempotency_key text,p_request_bytes bytea,p_request_sha256 kb_sha256
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE replay bytea; response jsonb; response_bytes bytea; head bid_document_relation_current%ROWTYPE;
  prior bid_document_relation_revision_artifacts%ROWTYPE; new_id uuid:=gen_random_uuid();
  new_revision bigint; payload bytea; sha kb_sha256;
BEGIN
  PERFORM kb_bid_v2_require_project_owner(p_project_id,p_actor);
  replay:=kb_bid_v2_idempotency_begin(p_actor,'bid.v2.document.relation',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  IF p_from_document_id=p_to_document_id OR NOT EXISTS(
      SELECT 1 FROM bid_documents a JOIN bid_documents b ON b.project_id=a.project_id
      WHERE a.project_id=p_project_id AND a.id=p_from_document_id AND b.id=p_to_document_id) THEN
    RAISE EXCEPTION 'DOCUMENT_RELATION_ENDPOINT_INVALID' USING ERRCODE='23514';
  END IF;
  SELECT * INTO head FROM bid_document_relation_current WHERE scope_id=p_lineage_id FOR UPDATE;
  IF FOUND THEN
    SELECT * INTO STRICT prior FROM bid_document_relation_revision_artifacts WHERE id=head.artifact_id AND project_id=p_project_id;
    IF head.project_id<>p_project_id OR head.artifact_id IS DISTINCT FROM p_expected_artifact_id
       OR prior.content_sha256 IS DISTINCT FROM p_expected_sha256 THEN
      RAISE EXCEPTION 'DOCUMENT_RELATION_CAS_MISMATCH' USING ERRCODE='40001';
    END IF;
    new_revision:=head.generation+1;
  ELSE
    IF p_expected_artifact_id IS NOT NULL OR p_expected_sha256 IS NOT NULL THEN
      RAISE EXCEPTION 'DOCUMENT_RELATION_CAS_MISMATCH' USING ERRCODE='40001';
    END IF;
    new_revision:=1;
  END IF;
  payload:=kb_bid_v2_json_payload(jsonb_build_object('schema_version',1,'project_id',p_project_id,
    'relation_lineage_id',p_lineage_id,'revision',new_revision,'from_document_id',p_from_document_id,
    'to_document_id',p_to_document_id,'relation_kind',p_relation_kind,
    'applicability',COALESCE(p_applicability,'{}'::jsonb),'tombstone',false));
  sha:=kb_bid_v2_sha256_bytes(payload);
  INSERT INTO bid_document_relation_revision_artifacts(id,project_id,relation_lineage_id,revision,
    from_document_id,to_document_id,relation_kind,applicability,tombstone,canonical_payload,content_sha256,actor)
  VALUES(new_id,p_project_id,p_lineage_id,new_revision,p_from_document_id,p_to_document_id,p_relation_kind,
    COALESCE(p_applicability,'{}'::jsonb),false,payload,sha,p_actor);
  IF new_revision=1 THEN
    INSERT INTO bid_document_relation_current(scope_id,project_id,artifact_id,generation,created_at)
    VALUES(p_lineage_id,p_project_id,new_id,1,clock_timestamp());
  ELSE
    UPDATE bid_document_relation_current SET artifact_id=new_id,generation=new_revision WHERE scope_id=p_lineage_id;
  END IF;
  response:=jsonb_build_object('lineage_id',p_lineage_id,'revision_id',new_id,'revision_sha256',sha,
    'from_document_id',p_from_document_id,'to_document_id',p_to_document_id,
    'relation_kind',p_relation_kind,'applicability',COALESCE(p_applicability,'{}'::jsonb));
  response_bytes:=convert_to(response::text,'UTF8');
  INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,
    request_sha256,response_sha256,entity_kind,entity_locator,after_revision,after_sha256)
  VALUES(gen_random_uuid(),1,'bid.v2.document.relation',p_actor,p_idempotency_key,p_request_sha256,
    kb_bid_v2_sha256_bytes(response_bytes),'bid_v2_document_relation',jsonb_build_object('lineage_id',p_lineage_id),
    new_revision,sha);
  PERFORM kb_bid_v2_idempotency_complete(p_actor,'bid.v2.document.relation',p_idempotency_key,200,response_bytes);
  RETURN response;
END $$;

CREATE FUNCTION kb_bid_v2_list_document_relations(
  p_project_id uuid,p_actor kb_actor_identity
) RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
BEGIN
  PERFORM kb_bid_v2_require_project_owner(p_project_id,p_actor);
  RETURN COALESCE((SELECT jsonb_agg(jsonb_build_object('lineage_id',r.relation_lineage_id,
      'revision_id',r.id,'revision_sha256',r.content_sha256,'from_document_id',r.from_document_id,
      'to_document_id',r.to_document_id,'relation_kind',r.relation_kind,'applicability',r.applicability)
      ORDER BY r.created_at,r.relation_lineage_id)
    FROM bid_document_relation_current c JOIN bid_document_relation_revision_artifacts r
      ON r.project_id=c.project_id AND r.id=c.artifact_id
    WHERE c.project_id=p_project_id AND NOT r.tombstone),'[]'::jsonb);
END $$;

CREATE FUNCTION kb_bid_v2_freeze_document_set(
  p_project_id uuid,p_document_ids uuid[],p_expected_artifact_id uuid,p_expected_sha256 kb_sha256,
  p_request_artifact_id uuid,p_actor kb_actor_identity,p_idempotency_key text,
  p_request_bytes bytea,p_request_sha256 kb_sha256, p_agent_runtime jsonb DEFAULT NULL
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE
  replay bytea; response jsonb; response_bytes bytea; head bid_document_set_current%ROWTYPE;
  disposition_head bid_source_unit_disposition_set_current%ROWTYPE;
  set_id uuid:=gen_random_uuid(); set_revision bigint; set_payload bytea; set_sha kb_sha256;
  disposition_id uuid:=gen_random_uuid(); disposition_revision bigint; disposition_payload bytea; disposition_sha kb_sha256;
  frozen_payload bytea; frozen_sha kb_sha256; job_payload jsonb; job_bytes bytea; job_sha kb_sha256;
  item jsonb; item_list jsonb:='[]'::jsonb; relation_items jsonb:='[]'::jsonb;
  disposition_items jsonb:='[]'::jsonb; warnings jsonb:='[]'::jsonb;
  document_key uuid; role_value bid_document_role_revision_artifacts%ROWTYPE;
  source_value bid_converted_source_artifacts%ROWTYPE; unit_value bid_source_unit_revision_artifacts%ROWTYPE;
  document_status text; source_disposition text; ordinal_value integer:=0;
BEGIN
  PERFORM kb_bid_v2_require_project_owner(p_project_id,p_actor);
  replay:=kb_bid_v2_idempotency_begin(p_actor,'bid.v2.document_set.freeze',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  IF COALESCE(array_length(p_document_ids,1),0)=0
     OR (SELECT count(*) FROM unnest(p_document_ids) value)<>(SELECT count(DISTINCT value) FROM unnest(p_document_ids) value) THEN
    RAISE EXCEPTION 'DOCUMENT_SET_MEMBERS_INVALID' USING ERRCODE='23514';
  END IF;
  SELECT * INTO STRICT head FROM bid_document_set_current WHERE scope_id=p_project_id FOR UPDATE;
  IF head.artifact_id IS DISTINCT FROM p_expected_artifact_id OR head.artifact_sha256 IS DISTINCT FROM p_expected_sha256 THEN
    RAISE EXCEPTION 'DOCUMENT_SET_CAS_MISMATCH' USING ERRCODE='40001';
  END IF;
  set_revision:=head.generation+1;
  FOREACH document_key IN ARRAY p_document_ids LOOP
    SELECT role_artifact.* INTO STRICT role_value
      FROM bid_document_role_current role_head JOIN bid_document_role_revision_artifacts role_artifact
        ON role_artifact.project_id=role_head.project_id AND role_artifact.id=role_head.artifact_id
      JOIN bid_documents document_value ON document_value.project_id=role_artifact.project_id
        AND document_value.id=role_artifact.document_id
      WHERE role_head.scope_id=document_key AND role_artifact.project_id=p_project_id;
    SELECT document_value.parse_status INTO STRICT document_status FROM bid_documents document_value
      WHERE document_value.project_id=p_project_id AND document_value.id=document_key;
    source_value:=NULL;
    SELECT source_artifact.* INTO source_value FROM bid_converted_source_artifacts source_artifact
      WHERE source_artifact.project_id=p_project_id AND source_artifact.document_id=document_key
      ORDER BY source_artifact.revision DESC LIMIT 1;
    IF document_status<>'ready' THEN source_value:=NULL; END IF;
    source_disposition:=CASE WHEN document_status='ready' AND source_value.id IS NOT NULL THEN 'ready'
      WHEN document_status='failed' THEN 'failed' WHEN document_status='pending' THEN 'pending' ELSE 'unresolved' END;
    item:=jsonb_build_object('document_id',document_key,'document_sha256',
      (SELECT original_sha256 FROM bid_documents WHERE id=document_key),'role_revision_id',role_value.id,
      'role_revision_sha256',role_value.content_sha256,'source_revision_id',source_value.id,
      'source_revision_sha256',source_value.source_sha256,'disposition',source_disposition,'ordinal',ordinal_value);
    item_list:=item_list||jsonb_build_array(item);
    IF source_disposition<>'ready' THEN
      warnings:=warnings||jsonb_build_array(jsonb_build_object(
        'code','DOCUMENT_INPUT_NOT_READY','document_id',document_key,
        'disposition',source_disposition,'message','DocumentSet froze the available inputs; this document was not ready'));
    END IF;
    ordinal_value:=ordinal_value+1;
  END LOOP;
  SELECT coalesce(jsonb_agg(jsonb_build_object(
      'relation_lineage_id',relation.relation_lineage_id,'relation_revision_id',relation.id,
      'relation_sha256',relation.content_sha256,'from_document_id',relation.from_document_id,
      'to_document_id',relation.to_document_id,'relation_kind',relation.relation_kind,
      'applicability',relation.applicability) ORDER BY relation.relation_lineage_id),'[]'::jsonb)
    INTO relation_items
    FROM bid_document_relation_current relation_head
    JOIN bid_document_relation_revision_artifacts relation ON relation.id=relation_head.artifact_id
    WHERE relation.project_id=p_project_id AND NOT relation.tombstone
      AND relation.from_document_id=ANY(p_document_ids) AND relation.to_document_id=ANY(p_document_ids);
  set_payload:=kb_bid_v2_json_payload(jsonb_build_object('schema_version',1,'project_id',p_project_id,
    'revision',set_revision,'items',item_list,'relations',relation_items));
  set_sha:=kb_bid_v2_sha256_bytes(set_payload);
  INSERT INTO bid_document_set_artifacts(id,project_id,revision,canonical_payload,content_sha256,actor)
  VALUES(set_id,p_project_id,set_revision,set_payload,set_sha,p_actor);
  INSERT INTO bid_document_set_items(document_set_id,project_id,document_id,ordinal,role_revision_id,source_revision_id,disposition)
  SELECT set_id,p_project_id,(value->>'document_id')::uuid,(value->>'ordinal')::integer,
    (value->>'role_revision_id')::uuid,(value->>'source_revision_id')::uuid,value->>'disposition'
  FROM jsonb_array_elements(item_list);
  IF NOT kb_bid_v2_advance_document_set(p_project_id,p_expected_artifact_id,p_expected_sha256,set_id,set_sha) THEN
    RAISE EXCEPTION 'DOCUMENT_SET_CAS_MISMATCH' USING ERRCODE='40001';
  END IF;
  SELECT * INTO STRICT disposition_head FROM bid_source_unit_disposition_set_current
    WHERE scope_id=p_project_id FOR UPDATE;
  disposition_revision:=disposition_head.generation+1;
  ordinal_value:=0;
  FOR unit_value IN
    SELECT unit_artifact.* FROM bid_document_set_items set_item
      JOIN bid_source_unit_revision_artifacts unit_artifact
        ON unit_artifact.project_id=set_item.project_id AND unit_artifact.source_revision_id=set_item.source_revision_id
      WHERE set_item.document_set_id=set_id ORDER BY set_item.ordinal,unit_artifact.ordinal,unit_artifact.id
  LOOP
    source_disposition:='unresolved';
    disposition_items:=disposition_items||jsonb_build_array(jsonb_build_object(
      'source_unit_revision_id',unit_value.id,'disposition',source_disposition,
      'reason','awaiting_agent_analysis',
      'ordinal',ordinal_value));
    ordinal_value:=ordinal_value+1;
  END LOOP;
  disposition_payload:=kb_bid_v2_json_payload(jsonb_build_object('schema_version',1,'project_id',p_project_id,
    'document_set_id',set_id,'revision',disposition_revision,'items',disposition_items));
  disposition_sha:=kb_bid_v2_sha256_bytes(disposition_payload);
  INSERT INTO bid_source_unit_disposition_set_artifacts(id,project_id,document_set_id,document_set_sequence,
    revision,canonical_payload,content_sha256,actor)
  VALUES(disposition_id,p_project_id,set_id,set_revision,disposition_revision,disposition_payload,disposition_sha,p_actor);
  INSERT INTO bid_source_unit_disposition_set_items(disposition_set_id,project_id,source_unit_revision_id,disposition,reason)
  SELECT disposition_id,p_project_id,(value->>'source_unit_revision_id')::uuid,value->>'disposition',value->>'reason'
    FROM jsonb_array_elements(disposition_items);
  IF NOT kb_bid_v2_advance_disposition_set(p_project_id,disposition_head.artifact_id,
      disposition_head.artifact_sha256,disposition_id,disposition_sha) THEN
    RAISE EXCEPTION 'DISPOSITION_SET_CAS_MISMATCH' USING ERRCODE='40001';
  END IF;
  frozen_payload:=kb_bid_v2_json_payload(jsonb_build_object('schema_version',1,'project_id',p_project_id,
    'document_set_revision_id',set_id,'document_set_sha256',set_sha,
    'disposition_set_revision_id',disposition_id,'disposition_set_sha256',disposition_sha,'agent_runtime',p_agent_runtime));
  frozen_sha:=kb_bid_v2_sha256_bytes(frozen_payload);
  job_payload:=jsonb_build_object('job_kind','requirement_set_compile','request',jsonb_build_object(
    'request_artifact_id',p_request_artifact_id,'request_revision',1,'frozen_input_sha256',frozen_sha),
    'project_id',p_project_id,'document_set_revision_id',set_id,
    'disposition_set_revision_id',disposition_id);
  job_bytes:=kb_bid_v2_json_payload(job_payload); job_sha:=kb_bid_v2_sha256_bytes(job_bytes);
  INSERT INTO bid_async_request_snapshot_artifacts(id,project_id,workspace_id,request_kind,revision,
    frozen_input_sha256,request_payload,request_sha256,status)
  VALUES(p_request_artifact_id,p_project_id,NULL,'requirement_set_compile',1,frozen_sha,job_bytes,job_sha,'pending');
  INSERT INTO bid_requirement_set_compile_request_identities(request_artifact_id,project_id,request_revision,
    request_sha256,frozen_input_sha256,document_set_revision_id,document_set_sha256,
    disposition_set_revision_id,disposition_set_sha256,agent_runtime)
  VALUES(p_request_artifact_id,p_project_id,1,job_sha,frozen_sha,set_id,set_sha,disposition_id,disposition_sha,p_agent_runtime);
  response:=jsonb_build_object('artifact_id',set_id,'sha256',set_sha,'revision',set_revision,
    'disposition_set_artifact_id',disposition_id,'disposition_set_sha256',disposition_sha,
    'request_artifact_id',p_request_artifact_id,'request_revision',1,'request_sha256',job_sha,
    'frozen_input_sha256',frozen_sha,'warnings',warnings);
  response_bytes:=convert_to(response::text,'UTF8');
  INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,
    request_sha256,response_sha256,entity_kind,entity_locator,before_revision,before_sha256,after_revision,after_sha256)
  VALUES(gen_random_uuid(),1,'bid.v2.document_set.freeze',p_actor,p_idempotency_key,p_request_sha256,
    kb_bid_v2_sha256_bytes(response_bytes),'bid_v2_document_set',jsonb_build_object('project_id',p_project_id),
    head.generation,head.artifact_sha256,set_revision,set_sha);
  PERFORM kb_bid_v2_idempotency_complete(p_actor,'bid.v2.document_set.freeze',p_idempotency_key,201,response_bytes);
  RETURN response;
END $$;

CREATE FUNCTION kb_bid_v2_list_document_sets(
  p_project_id uuid,p_actor kb_actor_identity
) RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
BEGIN
  PERFORM kb_bid_v2_require_project_owner(p_project_id,p_actor);
  RETURN coalesce((SELECT jsonb_agg(jsonb_build_object(
    'artifact_id',document_set.id,'sha256',document_set.content_sha256,'revision',document_set.revision,
    'items',coalesce((SELECT jsonb_agg(jsonb_build_object(
      'document_id',item.document_id,'ordinal',item.ordinal,'role_revision_id',item.role_revision_id,
      'source_revision_id',item.source_revision_id,'disposition',item.disposition)
      ORDER BY item.ordinal,item.document_id) FROM bid_document_set_items item
      WHERE item.document_set_id=document_set.id),'[]'::jsonb))
    ORDER BY document_set.revision DESC,document_set.id DESC)
    FROM bid_document_set_artifacts document_set WHERE document_set.project_id=p_project_id),'[]'::jsonb);
END $$;

CREATE FUNCTION kb_bid_v2_get_document_set(
  p_project_id uuid,p_document_set_id uuid,p_actor kb_actor_identity
) RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE result_value jsonb;
BEGIN
  PERFORM kb_bid_v2_require_project_owner(p_project_id,p_actor);
  SELECT jsonb_build_object('artifact_id',document_set.id,'sha256',document_set.content_sha256,
    'revision',document_set.revision,'items',coalesce((SELECT jsonb_agg(jsonb_build_object(
      'document_id',item.document_id,'ordinal',item.ordinal,'role_revision_id',item.role_revision_id,
      'source_revision_id',item.source_revision_id,'disposition',item.disposition)
      ORDER BY item.ordinal,item.document_id) FROM bid_document_set_items item
      WHERE item.document_set_id=document_set.id),'[]'::jsonb))
    INTO result_value FROM bid_document_set_artifacts document_set
    WHERE document_set.project_id=p_project_id AND document_set.id=p_document_set_id;
  RETURN result_value;
END $$;

CREATE FUNCTION kb_bid_v2_publish_disposition_set(
  p_project_id uuid,p_document_set_id uuid,p_items jsonb,
  p_expected_artifact_id uuid,p_expected_sha256 kb_sha256,p_request_artifact_id uuid,
  p_actor kb_actor_identity,p_idempotency_key text,p_request_bytes bytea,p_request_sha256 kb_sha256, p_agent_runtime jsonb DEFAULT NULL
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE
  replay bytea; response jsonb; response_bytes bytea;
  document_head bid_document_set_current%ROWTYPE; document_value bid_document_set_artifacts%ROWTYPE;
  disposition_head bid_source_unit_disposition_set_current%ROWTYPE;
  disposition_id uuid:=gen_random_uuid(); disposition_revision bigint; normalized_items jsonb;
  disposition_payload bytea; disposition_sha kb_sha256;
  frozen_payload bytea; frozen_sha kb_sha256; job_payload jsonb; job_bytes bytea; job_sha kb_sha256;
BEGIN
  PERFORM kb_bid_v2_require_project_owner(p_project_id,p_actor);
  replay:=kb_bid_v2_idempotency_begin(p_actor,'bid.v2.disposition_set.publish',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  IF jsonb_typeof(p_items)<>'array' OR jsonb_array_length(p_items)=0 THEN
    RAISE EXCEPTION 'DISPOSITION_SET_ITEMS_INVALID' USING ERRCODE='23514';
  END IF;
  SELECT * INTO STRICT document_head FROM bid_document_set_current WHERE scope_id=p_project_id;
  IF document_head.artifact_id<>p_document_set_id THEN
    RAISE EXCEPTION 'DOCUMENT_SET_NOT_CURRENT' USING ERRCODE='40001';
  END IF;
  SELECT * INTO STRICT document_value FROM bid_document_set_artifacts
    WHERE project_id=p_project_id AND id=p_document_set_id;
  SELECT * INTO STRICT disposition_head FROM bid_source_unit_disposition_set_current
    WHERE scope_id=p_project_id FOR UPDATE;
  IF disposition_head.artifact_id IS DISTINCT FROM p_expected_artifact_id
     OR disposition_head.artifact_sha256 IS DISTINCT FROM p_expected_sha256 THEN
    RAISE EXCEPTION 'DISPOSITION_SET_CAS_MISMATCH' USING ERRCODE='40001';
  END IF;
  IF EXISTS (SELECT 1 FROM jsonb_array_elements(p_items) item
      WHERE jsonb_typeof(item)<>'object'
         OR (item->>'source_unit_revision_id') IS NULL
         OR (item->>'disposition') NOT IN ('requirement','non_requirement','unresolved')
         OR (item ? 'reason' AND item->>'reason' IS NULL)
         OR octet_length(COALESCE(item->>'reason','x'))>4096)
     OR (SELECT count(*) FROM jsonb_array_elements(p_items)) <>
        (SELECT count(DISTINCT item->>'source_unit_revision_id') FROM jsonb_array_elements(p_items) item) THEN
    RAISE EXCEPTION 'DISPOSITION_SET_ITEMS_INVALID' USING ERRCODE='23514';
  END IF;
  IF EXISTS (
      SELECT expected.id FROM bid_document_set_items set_item
      JOIN bid_source_unit_revision_artifacts expected
        ON expected.project_id=set_item.project_id AND expected.source_revision_id=set_item.source_revision_id
      WHERE set_item.document_set_id=p_document_set_id
      EXCEPT SELECT (item->>'source_unit_revision_id')::uuid FROM jsonb_array_elements(p_items) item
    ) OR EXISTS (
      SELECT (item->>'source_unit_revision_id')::uuid FROM jsonb_array_elements(p_items) item
      EXCEPT SELECT expected.id FROM bid_document_set_items set_item
      JOIN bid_source_unit_revision_artifacts expected
        ON expected.project_id=set_item.project_id AND expected.source_revision_id=set_item.source_revision_id
      WHERE set_item.document_set_id=p_document_set_id
    ) THEN
    RAISE EXCEPTION 'DISPOSITION_SET_COVERAGE_INVALID' USING ERRCODE='23514';
  END IF;
  SELECT jsonb_agg(jsonb_build_object(
      'source_unit_revision_id',item->>'source_unit_revision_id',
      'disposition',item->>'disposition','reason',item->>'reason')
      ORDER BY item->>'source_unit_revision_id') INTO normalized_items
    FROM jsonb_array_elements(p_items) item;
  disposition_revision:=disposition_head.generation+1;
  disposition_payload:=kb_bid_v2_json_payload(jsonb_build_object('schema_version',1,
    'project_id',p_project_id,'document_set_id',p_document_set_id,
    'revision',disposition_revision,'items',normalized_items));
  disposition_sha:=kb_bid_v2_sha256_bytes(disposition_payload);
  INSERT INTO bid_source_unit_disposition_set_artifacts(id,project_id,document_set_id,document_set_sequence,
    revision,canonical_payload,content_sha256,actor)
  VALUES(disposition_id,p_project_id,p_document_set_id,document_value.revision,disposition_revision,
    disposition_payload,disposition_sha,p_actor);
  INSERT INTO bid_source_unit_disposition_set_items(disposition_set_id,project_id,
    source_unit_revision_id,disposition,reason)
  SELECT disposition_id,p_project_id,(item->>'source_unit_revision_id')::uuid,
    item->>'disposition',NULLIF(item->>'reason','') FROM jsonb_array_elements(normalized_items) item;
  IF NOT kb_bid_v2_advance_disposition_set(p_project_id,p_expected_artifact_id,p_expected_sha256,
      disposition_id,disposition_sha) THEN
    RAISE EXCEPTION 'DISPOSITION_SET_CAS_MISMATCH' USING ERRCODE='40001';
  END IF;
  frozen_payload:=kb_bid_v2_json_payload(jsonb_build_object('schema_version',1,'project_id',p_project_id,
    'document_set_revision_id',p_document_set_id,'document_set_sha256',document_value.content_sha256,
    'disposition_set_revision_id',disposition_id,'disposition_set_sha256',disposition_sha,'agent_runtime',p_agent_runtime));
  frozen_sha:=kb_bid_v2_sha256_bytes(frozen_payload);
  job_payload:=jsonb_build_object('job_kind','requirement_set_compile','request',jsonb_build_object(
    'request_artifact_id',p_request_artifact_id,'request_revision',1,'frozen_input_sha256',frozen_sha),
    'project_id',p_project_id,'document_set_revision_id',p_document_set_id,
    'disposition_set_revision_id',disposition_id);
  job_bytes:=kb_bid_v2_json_payload(job_payload); job_sha:=kb_bid_v2_sha256_bytes(job_bytes);
  INSERT INTO bid_async_request_snapshot_artifacts(id,project_id,workspace_id,request_kind,revision,
    frozen_input_sha256,request_payload,request_sha256,status)
  VALUES(p_request_artifact_id,p_project_id,NULL,'requirement_set_compile',1,frozen_sha,job_bytes,job_sha,'pending');
  INSERT INTO bid_requirement_set_compile_request_identities(request_artifact_id,project_id,request_revision,
    request_sha256,frozen_input_sha256,document_set_revision_id,document_set_sha256,
    disposition_set_revision_id,disposition_set_sha256,agent_runtime)
  VALUES(p_request_artifact_id,p_project_id,1,job_sha,frozen_sha,p_document_set_id,
    document_value.content_sha256,disposition_id,disposition_sha,p_agent_runtime);
  response:=jsonb_build_object('artifact_id',disposition_id,'sha256',disposition_sha,
    'revision',disposition_revision,'document_set_revision_id',p_document_set_id,
    'request_artifact_id',p_request_artifact_id,'request_revision',1,'request_sha256',job_sha,
    'frozen_input_sha256',frozen_sha);
  response_bytes:=convert_to(response::text,'UTF8');
  INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,
    request_sha256,response_sha256,entity_kind,entity_locator,before_revision,before_sha256,after_revision,after_sha256)
  VALUES(gen_random_uuid(),1,'bid.v2.disposition_set.publish',p_actor,p_idempotency_key,p_request_sha256,
    kb_bid_v2_sha256_bytes(response_bytes),'bid_v2_disposition_set',jsonb_build_object('project_id',p_project_id),
    disposition_head.generation,disposition_head.artifact_sha256,disposition_revision,disposition_sha);
  PERFORM kb_bid_v2_idempotency_complete(p_actor,'bid.v2.disposition_set.publish',p_idempotency_key,201,response_bytes);
  RETURN response;
END $$;

CREATE FUNCTION kb_bid_v2_list_source_units(
  p_project_id uuid,p_actor kb_actor_identity
) RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
BEGIN
  PERFORM kb_bid_v2_require_project_owner(p_project_id,p_actor);
  RETURN COALESCE((SELECT jsonb_agg(jsonb_build_object(
      'source_unit_revision_id',u.id,'lineage_id',u.lineage_id,'revision',u.revision,
      'document_id',u.document_id,'kind',u.unit_kind,'ordinal',u.ordinal,
      'source_locator',u.source_locator,'text',convert_from(u.text_utf8,'UTF8'),
      'content_sha256',u.content_sha256,'disposition',COALESCE(disposition.disposition,'unresolved'))
      ORDER BY d.created_at,u.ordinal,u.id)
    FROM bid_source_unit_revision_artifacts u JOIN bid_documents d ON d.id=u.document_id
    LEFT JOIN bid_source_unit_disposition_set_current disposition_head ON disposition_head.scope_id=p_project_id
    LEFT JOIN bid_source_unit_disposition_set_items disposition
      ON disposition.disposition_set_id=disposition_head.artifact_id
      AND disposition.source_unit_revision_id=u.id
    WHERE u.project_id=p_project_id),'[]'::jsonb);
END $$;

CREATE FUNCTION kb_bid_v2_list_structured_forms(
  p_project_id uuid,p_actor kb_actor_identity
) RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
BEGIN
  PERFORM kb_bid_v2_require_project_owner(p_project_id,p_actor);
  RETURN coalesce((SELECT jsonb_agg(convert_from(form.canonical_payload,'UTF8')::jsonb||jsonb_build_object(
    'form_definition_revision_id',form.id,'source_unit_revision_id',form.source_unit_revision_id,
    'canonical_sha256',form.content_sha256) ORDER BY source.document_id,source.ordinal,form.id)
    FROM bid_tender_structured_form_definition_artifacts form
    JOIN bid_source_unit_revision_artifacts source ON source.project_id=form.project_id
      AND source.id=form.source_unit_revision_id
    WHERE form.project_id=p_project_id),'[]'::jsonb);
END $$;

CREATE FUNCTION kb_bid_v2_advance_workspace_projection(
  p_workspace_id uuid,p_expected_projection_id uuid,p_expected_projection_sha256 kb_sha256,
  p_new_projection_id uuid,p_new_projection_sha256 kb_sha256,p_actor kb_actor_identity
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE workspace bid_submission_workspaces%ROWTYPE; head bid_workspace_heads%ROWTYPE;
  old_revision bid_workspace_revision_artifacts%ROWTYPE; settings bid_document_settings_revision_artifacts%ROWTYPE;
  new_revision_id uuid:=gen_random_uuid(); new_revision bigint; payload bytea; digest kb_sha256;
  node record; block record; binding record; edge record; evidence record; node_map jsonb:='{}'::jsonb;
  new_occurrence_id uuid; new_parent_id uuid; evidence_revision bigint; evidence_payload bytea; evidence_sha kb_sha256;
BEGIN
  SELECT * INTO STRICT workspace FROM bid_submission_workspaces WHERE id=p_workspace_id;
  SELECT * INTO STRICT head FROM bid_workspace_heads WHERE scope_id=p_workspace_id FOR UPDATE;
  SELECT * INTO STRICT old_revision FROM bid_workspace_revision_artifacts WHERE id=head.artifact_id;
  IF old_revision.requirement_projection_id IS DISTINCT FROM p_expected_projection_id OR
     old_revision.requirement_projection_sha256 IS DISTINCT FROM p_expected_projection_sha256 THEN
    RAISE EXCEPTION 'WORKSPACE_PROJECTION_CAS_MISMATCH' USING ERRCODE='40001';
  END IF;
  PERFORM 1 FROM bid_workspace_requirement_projection_artifacts
    WHERE id=p_new_projection_id AND project_id=workspace.project_id AND workspace_id=p_workspace_id
      AND content_sha256=p_new_projection_sha256;
  IF NOT FOUND THEN RAISE EXCEPTION 'NEW_WORKSPACE_PROJECTION_INVALID' USING ERRCODE='23514'; END IF;
  SELECT * INTO STRICT settings FROM bid_document_settings_revision_artifacts
    WHERE id=old_revision.document_settings_revision_id;
  SELECT coalesce(max(revision),0)+1 INTO new_revision FROM bid_workspace_revision_artifacts
    WHERE workspace_id=p_workspace_id;
  payload:=kb_bid_v2_json_payload(jsonb_build_object('schema_version',1,'reason','requirement_projection_advanced',
    'parent_revision_id',old_revision.id,'parent_sha256',old_revision.content_sha256,
    'scope_revision_id',old_revision.scope_revision_id,
    'requirement_projection_id',p_new_projection_id,'requirement_projection_sha256',p_new_projection_sha256,
    'document_settings_revision_id',settings.id,'document_settings_sha256',settings.content_sha256,
    'node_revision_ids',coalesce((SELECT jsonb_agg(node_revision_id ORDER BY depth,ordinal,id)
      FROM bid_workspace_node_occurrences WHERE workspace_revision_id=old_revision.id),'[]'::jsonb),
    'block_revision_ids',coalesce((SELECT jsonb_agg(block_revision_id ORDER BY ordinal,id)
      FROM bid_workspace_block_occurrences WHERE workspace_revision_id=old_revision.id),'[]'::jsonb),
    'binding_revision_ids',coalesce((SELECT jsonb_agg(binding_revision_id ORDER BY ordinal,id)
      FROM bid_workspace_binding_occurrences WHERE workspace_revision_id=old_revision.id),'[]'::jsonb)));
  digest:=kb_bid_v2_sha256_bytes(payload);
  INSERT INTO bid_workspace_revision_artifacts(id,project_id,workspace_id,revision,parent_revision_id,parent_sha256,
    scope_revision_id,requirement_projection_id,requirement_projection_sha256,document_settings_revision_id,
    quote_snapshot_id,quote_snapshot_sha256,canonical_payload,content_sha256,actor)
  VALUES(new_revision_id,workspace.project_id,p_workspace_id,new_revision,old_revision.id,old_revision.content_sha256,
    old_revision.scope_revision_id,p_new_projection_id,p_new_projection_sha256,old_revision.document_settings_revision_id,
    old_revision.quote_snapshot_id,old_revision.quote_snapshot_sha256,
    payload,digest,p_actor);
  FOR node IN SELECT * FROM bid_workspace_node_occurrences WHERE workspace_revision_id=old_revision.id
    ORDER BY depth,ordinal,id
  LOOP
    new_occurrence_id:=gen_random_uuid();
    new_parent_id:=CASE WHEN node.parent_occurrence_id IS NULL THEN NULL
      ELSE (node_map->>node.parent_occurrence_id::text)::uuid END;
    INSERT INTO bid_workspace_node_occurrences(id,project_id,workspace_revision_id,node_revision_id,
      parent_occurrence_id,ordinal,depth)
    VALUES(new_occurrence_id,workspace.project_id,new_revision_id,node.node_revision_id,
      new_parent_id,node.ordinal,node.depth);
    node_map:=node_map||jsonb_build_object(node.id::text,new_occurrence_id);
  END LOOP;
  FOR block IN SELECT * FROM bid_workspace_block_occurrences WHERE workspace_revision_id=old_revision.id
    ORDER BY ordinal,id
  LOOP
    INSERT INTO bid_workspace_block_occurrences(id,project_id,workspace_revision_id,node_occurrence_id,
      block_revision_id,ordinal)
    VALUES(gen_random_uuid(),workspace.project_id,new_revision_id,
      (node_map->>block.node_occurrence_id::text)::uuid,block.block_revision_id,block.ordinal);
  END LOOP;
  FOR binding IN SELECT * FROM bid_workspace_binding_occurrences WHERE workspace_revision_id=old_revision.id
    ORDER BY ordinal,id
  LOOP
    INSERT INTO bid_workspace_binding_occurrences(id,project_id,workspace_revision_id,binding_revision_id,ordinal)
    VALUES(gen_random_uuid(),workspace.project_id,new_revision_id,binding.binding_revision_id,binding.ordinal);
  END LOOP;
  FOR edge IN SELECT * FROM bid_outline_lineage_edges WHERE workspace_revision_id=old_revision.id
  LOOP
    INSERT INTO bid_outline_lineage_edges(id,project_id,workspace_id,operation,from_lineage_id,to_lineage_id,
      workspace_revision_id,created_at)
    VALUES(gen_random_uuid(),workspace.project_id,p_workspace_id,edge.operation,edge.from_lineage_id,
      edge.to_lineage_id,new_revision_id,edge.created_at);
  END LOOP;
  IF NOT kb_bid_v2_advance_workspace_head(p_workspace_id,old_revision.id,old_revision.content_sha256,
      new_revision_id,digest) THEN
    RAISE EXCEPTION 'WORKSPACE_HEAD_CAS_MISMATCH' USING ERRCODE='40001';
  END IF;
  RETURN jsonb_build_object('revision_id',new_revision_id,'sha256',digest);
END $$;

CREATE FUNCTION kb_bid_v2_compile_requirement_set(
  p_request_artifact_id uuid,p_request_revision bigint,p_frozen_input_sha256 kb_sha256,
  p_actor kb_actor_identity
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE
  request_value bid_async_request_snapshot_artifacts%ROWTYPE;
  typed bid_requirement_set_compile_request_identities%ROWTYPE;
  current_value bid_requirement_set_current%ROWTYPE; projection_head bid_workspace_requirement_projection_current%ROWTYPE;
  workspace_value bid_submission_workspaces%ROWTYPE; source_value bid_source_unit_revision_artifacts%ROWTYPE;
  requirement_id uuid; requirement_lineage uuid; requirement_payload bytea; requirement_sha kb_sha256;
  requirement_kind text; requiredness text; policy text; requirement_items jsonb:='[]'::jsonb;
  set_id uuid:=gen_random_uuid(); set_revision bigint; set_payload bytea; set_sha kb_sha256;
  projection_id uuid:=gen_random_uuid(); projection_revision bigint; projection_payload bytea; projection_sha kb_sha256;
  ordinal_value integer:=0; publication_status text; result_value jsonb; result_sha kb_sha256; prior bid_async_stage_receipts%ROWTYPE;
BEGIN
  IF p_actor<>'system:requirement-set-compile-v2' THEN
    RAISE EXCEPTION 'SYSTEM_ACTOR_REQUIRED' USING ERRCODE='42501';
  END IF;
  SELECT * INTO STRICT request_value FROM bid_async_request_snapshot_artifacts
    WHERE id=p_request_artifact_id AND request_kind='requirement_set_compile'
      AND revision=p_request_revision AND frozen_input_sha256=p_frozen_input_sha256 FOR UPDATE;
  SELECT * INTO STRICT typed FROM bid_requirement_set_compile_request_identities
    WHERE request_artifact_id=p_request_artifact_id;
  SELECT * INTO prior FROM bid_async_stage_receipts WHERE request_artifact_id=p_request_artifact_id
    AND stage_kind='requirement_compile' AND frozen_input_sha256=p_frozen_input_sha256;
  IF FOUND THEN RETURN prior.result_identity||jsonb_build_object('replayed',true); END IF;
  IF request_value.status<>'pending' THEN
    RAISE EXCEPTION 'REQUIREMENT_COMPILE_REQUEST_NOT_PENDING' USING ERRCODE='23514';
  END IF;
  -- Serialize artifact revision allocation independently from publication generation.
  -- Multiple frozen compile requests may complete out of order; each still needs a
  -- unique immutable artifact revision before the newer-input publication check.
  PERFORM 1 FROM bid_projects WHERE id=typed.project_id FOR UPDATE;
  SELECT coalesce(max(revision),0)+1 INTO set_revision
    FROM bid_requirement_set_artifacts WHERE project_id=typed.project_id;
  FOR source_value IN
    SELECT u.* FROM bid_source_unit_disposition_set_items disposition
      JOIN bid_source_unit_revision_artifacts u ON u.project_id=disposition.project_id
        AND u.id=disposition.source_unit_revision_id
      WHERE disposition.disposition_set_id=typed.disposition_set_revision_id
        AND disposition.disposition='requirement'
      ORDER BY u.document_id,u.ordinal,u.id
  LOOP
    requirement_id:=gen_random_uuid(); requirement_lineage:=gen_random_uuid();
    requirement_kind:=CASE
      WHEN convert_from(source_value.text_utf8,'UTF8')~*'(资格|资质|license|qualification)' THEN 'qualification'
      WHEN convert_from(source_value.text_utf8,'UTF8')~*'(价格|报价|price|pricing)' THEN 'pricing'
      WHEN convert_from(source_value.text_utf8,'UTF8')~*'(商务|付款|commercial|payment)' THEN 'commercial'
      WHEN convert_from(source_value.text_utf8,'UTF8')~*'(交付|工期|delivery|schedule)' THEN 'delivery'
      WHEN convert_from(source_value.text_utf8,'UTF8')~*'(评分|评审|score|evaluation)' THEN 'evaluation'
      WHEN source_value.unit_kind IN ('table_row','form_region') THEN 'format'
      ELSE 'technical' END;
    requiredness:=CASE WHEN convert_from(source_value.text_utf8,'UTF8')~*'(必须|应当|不得|must|shall|required)' THEN 'mandatory' ELSE 'informational' END;
    policy:=CASE WHEN requiredness='mandatory' THEN 'must_comply' ELSE 'explicit_response' END;
    requirement_payload:=kb_bid_v2_json_payload(jsonb_build_object('schema_version',1,'lineage_id',requirement_lineage,
      'revision',1,'requirement_kind',requirement_kind,'requiredness',requiredness,'compliance_policy',policy,
      'lifecycle','current','text',convert_from(source_value.text_utf8,'UTF8'),
      'fulfillment_expr',jsonb_build_object('kind','need','need_occurrence_id',requirement_id,'channel','narrative_content'),
      'applicability',jsonb_build_object('fragments',jsonb_build_array('source_unit:'||source_value.id::text))));
    requirement_sha:=kb_bid_v2_sha256_bytes(requirement_payload);
    INSERT INTO bid_requirement_revision_artifacts(id,project_id,lineage_id,revision,requirement_kind,
      requiredness,compliance_policy,lifecycle,text_utf8,text_sha256,fulfillment_expr,applicability,tombstone,
      canonical_payload,content_sha256,actor)
    VALUES(requirement_id,typed.project_id,requirement_lineage,1,requirement_kind,requiredness,policy,'current',
      source_value.text_utf8,source_value.text_sha256,
      jsonb_build_object('kind','need','need_occurrence_id',requirement_id,'channel','narrative_content'),
      jsonb_build_object('fragments',jsonb_build_array('source_unit:'||source_value.id::text)),false,
      requirement_payload,requirement_sha,p_actor);
    INSERT INTO bid_requirement_source_revision_artifacts(id,project_id,requirement_revision_id,
      source_unit_revision_id,quote_start_offset,quote_end_offset,quote_sha256)
    VALUES(gen_random_uuid(),typed.project_id,requirement_id,source_value.id,0,
      octet_length(source_value.text_utf8),source_value.text_sha256);
    requirement_items:=requirement_items||jsonb_build_array(jsonb_build_object('requirement_revision_id',requirement_id,
      'content_sha256',requirement_sha,
      'effective_applicability',jsonb_build_object('fragments',jsonb_build_array('source_unit:'||source_value.id::text)),
      'ordinal',ordinal_value));
    ordinal_value:=ordinal_value+1;
  END LOOP;
  set_payload:=kb_bid_v2_json_payload(jsonb_build_object('schema_version',1,'project_id',typed.project_id,
    'document_set_revision_id',typed.document_set_revision_id,
    'disposition_set_revision_id',typed.disposition_set_revision_id,'revision',set_revision,'items',requirement_items));
  set_sha:=kb_bid_v2_sha256_bytes(set_payload);
  INSERT INTO bid_requirement_set_artifacts(id,project_id,document_set_id,document_set_sequence,
    disposition_set_id,disposition_set_sequence,revision,canonical_payload,content_sha256)
  SELECT set_id,typed.project_id,typed.document_set_revision_id,d.revision,
    typed.disposition_set_revision_id,s.revision,set_revision,set_payload,set_sha
    FROM bid_document_set_artifacts d,bid_source_unit_disposition_set_artifacts s
    WHERE d.id=typed.document_set_revision_id AND s.id=typed.disposition_set_revision_id;
  INSERT INTO bid_requirement_set_items(requirement_set_id,project_id,requirement_revision_id,effective_applicability,ordinal)
  SELECT set_id,typed.project_id,(value->>'requirement_revision_id')::uuid,value->'effective_applicability',
    (value->>'ordinal')::integer FROM jsonb_array_elements(requirement_items);
  publication_status:=kb_bid_v2_publish_requirement_set(set_id,set_sha);
  IF publication_status='superseded' THEN
    result_value:=jsonb_build_object('requirement_set_id',set_id,'requirement_set_sha256',set_sha,
      'requirement_count',ordinal_value,'status','succeeded','published_current',false,'replayed',false);
    result_sha:=kb_bid_v2_sha256_bytes(convert_to(result_value::text,'UTF8'));
    INSERT INTO bid_async_stage_receipts(request_artifact_id,stage_kind,frozen_input_sha256,result_identity,result_sha256)
    VALUES(p_request_artifact_id,'requirement_compile',p_frozen_input_sha256,result_value,result_sha);
    UPDATE bid_async_request_snapshot_artifacts SET status='succeeded',result_identity=result_value,
      finished_at=clock_timestamp() WHERE id=p_request_artifact_id;
    RETURN result_value;
  END IF;
  SELECT * INTO STRICT workspace_value FROM bid_submission_workspaces WHERE project_id=typed.project_id;
  SELECT * INTO STRICT projection_head FROM bid_workspace_requirement_projection_current
    WHERE scope_id=workspace_value.id FOR UPDATE;
  projection_revision:=projection_head.generation+1;
  projection_payload:=kb_bid_v2_json_payload(jsonb_build_object('schema_version',1,'workspace_id',workspace_value.id,
    'requirement_set_id',set_id,'revision',projection_revision,'items',requirement_items));
  projection_sha:=kb_bid_v2_sha256_bytes(projection_payload);
  INSERT INTO bid_workspace_requirement_projection_artifacts(id,project_id,workspace_id,requirement_set_id,
    revision,canonical_payload,content_sha256)
  VALUES(projection_id,typed.project_id,workspace_value.id,set_id,projection_revision,projection_payload,projection_sha);
  INSERT INTO bid_workspace_requirement_projection_items(projection_id,project_id,requirement_revision_id,effective_applicability,ordinal)
  SELECT projection_id,typed.project_id,(value->>'requirement_revision_id')::uuid,value->'effective_applicability',
    (value->>'ordinal')::integer FROM jsonb_array_elements(requirement_items);
  IF NOT kb_bid_v2_advance_requirement_projection(typed.project_id,workspace_value.id,
      projection_head.artifact_id,projection_head.artifact_sha256,projection_id,projection_sha) THEN
    RAISE EXCEPTION 'REQUIREMENT_PROJECTION_CAS_MISMATCH' USING ERRCODE='40001';
  END IF;
  result_value:=jsonb_build_object('requirement_set_id',set_id,'requirement_set_sha256',set_sha,
    'requirement_count',ordinal_value,'requirement_projection_id',projection_id,
    'requirement_projection_sha256',projection_sha,'published_current',true,
    'workspace_apply_required',true,'replayed',false);
  result_sha:=kb_bid_v2_sha256_bytes(convert_to(result_value::text,'UTF8'));
  INSERT INTO bid_async_stage_receipts(request_artifact_id,stage_kind,frozen_input_sha256,result_identity,result_sha256)
  VALUES(p_request_artifact_id,'requirement_compile',p_frozen_input_sha256,result_value,result_sha);
  UPDATE bid_async_request_snapshot_artifacts SET status='succeeded',result_identity=result_value,
    finished_at=clock_timestamp() WHERE id=p_request_artifact_id;
  RETURN result_value;
END $$;

CREATE FUNCTION kb_bid_v2_mark_requirement_set_compile_failed(
  p_request_artifact_id uuid,p_request_revision bigint,p_frozen_input_sha256 kb_sha256,p_error_code text
) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
BEGIN
  PERFORM 1 FROM bid_requirement_set_compile_request_identities
    WHERE request_artifact_id=p_request_artifact_id AND request_revision=p_request_revision
      AND frozen_input_sha256=p_frozen_input_sha256;
  IF NOT FOUND THEN RAISE EXCEPTION 'FROZEN_INPUT_MISSING' USING ERRCODE='P0002'; END IF;
  UPDATE bid_async_request_snapshot_artifacts SET status='failed',
    error_code=CASE WHEN p_error_code IN ('INPUT_SCHEMA_INVALID','FROZEN_INPUT_MISSING',
      'FROZEN_INPUT_DIGEST_MISMATCH','REQUIREMENT_COMPILE_TIMEOUT') THEN p_error_code
      ELSE 'REQUIREMENT_COMPILE_FAILED' END,
    finished_at=clock_timestamp()
    WHERE id=p_request_artifact_id AND request_kind='requirement_set_compile' AND status='pending';
END $$;

CREATE FUNCTION kb_bid_v2_get_requirement_set_compile_request(
  p_project_id uuid,p_request_artifact_id uuid,p_actor kb_actor_identity
) RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE result_value jsonb;
BEGIN
  PERFORM kb_bid_v2_require_project_owner(p_project_id,p_actor);
  SELECT jsonb_build_object(
    'request_artifact_id',request_value.id,'kind','RequirementSetCompile','status',request_value.status,
    'request_revision',identity_value.request_revision,'request_sha256',identity_value.request_sha256,
    'frozen_input_sha256',identity_value.frozen_input_sha256,
    'document_set_revision_id',identity_value.document_set_revision_id,
    'document_set_sha256',identity_value.document_set_sha256,
    'disposition_set_revision_id',identity_value.disposition_set_revision_id,
    'disposition_set_sha256',identity_value.disposition_set_sha256,
    'result_identity',request_value.result_identity,'error_code',request_value.error_code,
    'progress',(SELECT progress_detail FROM bid_tender_agent_run_artifacts r WHERE r.request_artifact_id=request_value.id AND r.attempt=request_value.current_attempt))
  INTO result_value
  FROM bid_async_request_snapshot_artifacts request_value
  JOIN bid_requirement_set_compile_request_identities identity_value
    ON identity_value.request_artifact_id=request_value.id
  WHERE request_value.id=p_request_artifact_id AND request_value.project_id=p_project_id
    AND request_value.request_kind='requirement_set_compile'
    AND identity_value.project_id=p_project_id;
  RETURN result_value;
END $$;

CREATE FUNCTION kb_bid_v2_patch_requirement(
  p_project_id uuid,p_requirement_revision_id uuid,p_expected_set_id uuid,p_expected_set_sha256 kb_sha256,
  p_requirement_kind text,p_requiredness text,p_compliance_policy text,p_lifecycle text,p_text text,
  p_fulfillment_expr jsonb,p_applicability jsonb,p_actor kb_actor_identity,p_idempotency_key text,
  p_request_bytes bytea,p_request_sha256 kb_sha256
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE replay bytea; response jsonb; response_bytes bytea;
  head bid_requirement_set_current%ROWTYPE; old_set bid_requirement_set_artifacts%ROWTYPE;
  old_requirement bid_requirement_revision_artifacts%ROWTYPE;
  new_requirement_id uuid:=gen_random_uuid(); new_requirement_revision bigint;
  requirement_payload bytea; requirement_sha kb_sha256; text_bytes bytea; text_sha kb_sha256;
  new_set_id uuid:=gen_random_uuid(); new_set_revision bigint; set_items jsonb; set_payload bytea; set_sha kb_sha256;
  workspace_value bid_submission_workspaces%ROWTYPE; projection_head bid_workspace_requirement_projection_current%ROWTYPE;
  projection_id uuid:=gen_random_uuid(); projection_revision bigint; projection_payload bytea; projection_sha kb_sha256;
BEGIN
  PERFORM kb_bid_v2_require_project_owner(p_project_id,p_actor);
  replay:=kb_bid_v2_idempotency_begin(p_actor,'bid.v2.requirement.patch',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  IF p_requirement_kind NOT IN ('qualification','technical','commercial','pricing','delivery','evaluation','format','attachment','other')
     OR p_requiredness NOT IN ('mandatory','optional','informational')
     OR p_compliance_policy NOT IN ('must_comply','explicit_response','deviation_allowed','scored')
     OR p_lifecycle NOT IN ('current','superseded','withdrawn','unresolved')
     OR octet_length(p_text)=0 OR NOT kb_bid_v2_fulfillment_expr_valid(p_fulfillment_expr)
     OR NOT kb_bid_v2_applicability_valid(p_applicability) THEN
    RAISE EXCEPTION 'REQUIREMENT_PATCH_INVALID' USING ERRCODE='23514';
  END IF;
  SELECT * INTO STRICT head FROM bid_requirement_set_current WHERE scope_id=p_project_id FOR UPDATE;
  IF head.artifact_id<>p_expected_set_id OR head.artifact_sha256<>p_expected_set_sha256 THEN
    RAISE EXCEPTION 'REQUIREMENT_SET_CAS_MISMATCH' USING ERRCODE='40001';
  END IF;
  SELECT * INTO STRICT old_set FROM bid_requirement_set_artifacts
    WHERE project_id=p_project_id AND id=head.artifact_id;
  SELECT requirement.* INTO STRICT old_requirement
    FROM bid_requirement_set_items item JOIN bid_requirement_revision_artifacts requirement
      ON requirement.project_id=item.project_id AND requirement.id=item.requirement_revision_id
    WHERE item.requirement_set_id=old_set.id AND requirement.id=p_requirement_revision_id;
  SELECT COALESCE(max(revision),0)+1 INTO new_requirement_revision
    FROM bid_requirement_revision_artifacts
    WHERE project_id=p_project_id AND lineage_id=old_requirement.lineage_id;
  text_bytes:=convert_to(p_text,'UTF8'); text_sha:=kb_bid_v2_sha256_bytes(text_bytes);
  requirement_payload:=kb_bid_v2_json_payload(jsonb_build_object('schema_version',1,
    'lineage_id',old_requirement.lineage_id,'revision',new_requirement_revision,
    'requirement_kind',p_requirement_kind,'requiredness',p_requiredness,
    'compliance_policy',p_compliance_policy,'lifecycle',p_lifecycle,'text',p_text,
    'fulfillment_expr',p_fulfillment_expr,'applicability',p_applicability));
  requirement_sha:=kb_bid_v2_sha256_bytes(requirement_payload);
  INSERT INTO bid_requirement_revision_artifacts(id,project_id,lineage_id,revision,requirement_kind,
    requiredness,compliance_policy,lifecycle,text_utf8,text_sha256,fulfillment_expr,applicability,
    tombstone,canonical_payload,content_sha256,actor)
  VALUES(new_requirement_id,p_project_id,old_requirement.lineage_id,new_requirement_revision,
    p_requirement_kind,p_requiredness,p_compliance_policy,p_lifecycle,text_bytes,text_sha,
    p_fulfillment_expr,p_applicability,false,requirement_payload,requirement_sha,p_actor);
  INSERT INTO bid_requirement_source_revision_artifacts(id,project_id,requirement_revision_id,
    source_unit_revision_id,quote_start_offset,quote_end_offset,quote_sha256)
  SELECT gen_random_uuid(),project_id,new_requirement_id,source_unit_revision_id,
    quote_start_offset,quote_end_offset,quote_sha256
    FROM bid_requirement_source_revision_artifacts
    WHERE project_id=p_project_id AND requirement_revision_id=old_requirement.id;
  SELECT jsonb_agg(jsonb_build_object('requirement_revision_id',
      CASE WHEN item.requirement_revision_id=old_requirement.id THEN new_requirement_id ELSE item.requirement_revision_id END,
      'effective_applicability',CASE WHEN item.requirement_revision_id=old_requirement.id
        THEN p_applicability ELSE item.effective_applicability END,
      'ordinal',item.ordinal) ORDER BY item.ordinal) INTO set_items
    FROM bid_requirement_set_items item WHERE item.requirement_set_id=old_set.id;
  new_set_revision:=head.generation+1;
  set_payload:=kb_bid_v2_json_payload(jsonb_build_object('schema_version',1,'project_id',p_project_id,
    'document_set_revision_id',old_set.document_set_id,'disposition_set_revision_id',old_set.disposition_set_id,
    'revision',new_set_revision,'items',set_items)); set_sha:=kb_bid_v2_sha256_bytes(set_payload);
  INSERT INTO bid_requirement_set_artifacts(id,project_id,document_set_id,document_set_sequence,
    disposition_set_id,disposition_set_sequence,revision,canonical_payload,content_sha256)
  VALUES(new_set_id,p_project_id,old_set.document_set_id,old_set.document_set_sequence,
    old_set.disposition_set_id,old_set.disposition_set_sequence,new_set_revision,set_payload,set_sha);
  INSERT INTO bid_requirement_set_items(requirement_set_id,project_id,requirement_revision_id,effective_applicability,ordinal)
  SELECT new_set_id,p_project_id,(item->>'requirement_revision_id')::uuid,item->'effective_applicability',
    (item->>'ordinal')::integer FROM jsonb_array_elements(set_items) item;
  IF NOT kb_bid_v2_advance_requirement_set(p_project_id,p_expected_set_id,p_expected_set_sha256,
      new_set_id,set_sha) THEN RAISE EXCEPTION 'REQUIREMENT_SET_CAS_MISMATCH' USING ERRCODE='40001'; END IF;
  SELECT * INTO STRICT workspace_value FROM bid_submission_workspaces WHERE project_id=p_project_id;
  SELECT * INTO STRICT projection_head FROM bid_workspace_requirement_projection_current
    WHERE scope_id=workspace_value.id FOR UPDATE;
  projection_revision:=projection_head.generation+1;
  projection_payload:=kb_bid_v2_json_payload(jsonb_build_object('schema_version',1,
    'workspace_id',workspace_value.id,'requirement_set_id',new_set_id,
    'revision',projection_revision,'items',set_items)); projection_sha:=kb_bid_v2_sha256_bytes(projection_payload);
  INSERT INTO bid_workspace_requirement_projection_artifacts(id,project_id,workspace_id,requirement_set_id,
    revision,canonical_payload,content_sha256)
  VALUES(projection_id,p_project_id,workspace_value.id,new_set_id,projection_revision,projection_payload,projection_sha);
  INSERT INTO bid_workspace_requirement_projection_items(projection_id,project_id,requirement_revision_id,effective_applicability,ordinal)
  SELECT projection_id,p_project_id,(item->>'requirement_revision_id')::uuid,item->'effective_applicability',
    (item->>'ordinal')::integer FROM jsonb_array_elements(set_items) item;
  IF NOT kb_bid_v2_advance_requirement_projection(p_project_id,workspace_value.id,
      projection_head.artifact_id,projection_head.artifact_sha256,projection_id,projection_sha) THEN
    RAISE EXCEPTION 'REQUIREMENT_PROJECTION_CAS_MISMATCH' USING ERRCODE='40001';
  END IF;
  response:=jsonb_build_object('requirement_revision_id',new_requirement_id,
    'lineage_id',old_requirement.lineage_id,'revision',new_requirement_revision,
    'content_sha256',requirement_sha,'requirement_set_id',new_set_id,
    'requirement_set_sha256',set_sha,'requirement_projection_id',projection_id,
    'requirement_projection_sha256',projection_sha,'workspace_apply_required',true);
  response_bytes:=convert_to(response::text,'UTF8');
  INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,
    response_sha256,entity_kind,entity_locator,before_revision,before_sha256,after_revision,after_sha256)
  VALUES(gen_random_uuid(),1,'bid.v2.requirement.patch',p_actor,p_idempotency_key,p_request_sha256,
    kb_bid_v2_sha256_bytes(response_bytes),'bid_v2_requirement',jsonb_build_object('lineage_id',old_requirement.lineage_id),
    old_requirement.revision,old_requirement.content_sha256,new_requirement_revision,requirement_sha);
  PERFORM kb_bid_v2_idempotency_complete(p_actor,'bid.v2.requirement.patch',p_idempotency_key,200,response_bytes);
  RETURN response;
END $$;

CREATE FUNCTION kb_bid_v2_publish_requirement_supersession(
  p_project_id uuid,p_lineage_id uuid,p_old_requirement_revision_id uuid,p_new_requirement_revision_id uuid,
  p_applicability jsonb,p_tombstone boolean,p_expected_artifact_id uuid,p_expected_sha256 kb_sha256,
  p_actor kb_actor_identity,p_idempotency_key text,p_request_bytes bytea,p_request_sha256 kb_sha256
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE replay bytea; response jsonb; response_bytes bytea;
  head bid_requirement_supersession_current%ROWTYPE; head_exists boolean:=false;
  prior_edge bid_requirement_supersession_revision_artifacts%ROWTYPE;
  new_id uuid:=gen_random_uuid(); new_revision bigint;
  payload bytea; sha kb_sha256; set_head bid_requirement_set_current%ROWTYPE;
  old_set bid_requirement_set_artifacts%ROWTYPE; old_requirement bid_requirement_revision_artifacts%ROWTYPE;
  new_requirement bid_requirement_revision_artifacts%ROWTYPE; old_effective jsonb; new_effective jsonb;
  prior_old_effective jsonb; prior_new_effective jsonb; base_items jsonb;
  old_fragments text[]; new_fragments text[]; edge_fragments text[]; remaining_fragments text[];
  prior_fragments text[]; restored_old_fragments text[]; restored_new_fragments text[];
  prior_old_ordinal integer; prior_new_ordinal integer;
  old_sources uuid[]; new_sources uuid[]; relation_identity jsonb;
  new_set_id uuid:=gen_random_uuid(); set_payload bytea; set_sha kb_sha256; set_items jsonb;
  workspace_value bid_submission_workspaces%ROWTYPE; projection_head bid_workspace_requirement_projection_current%ROWTYPE;
  projection_id uuid:=gen_random_uuid(); projection_payload bytea; projection_sha kb_sha256; projection_revision bigint;
BEGIN
  PERFORM kb_bid_v2_require_project_owner(p_project_id,p_actor);
  replay:=kb_bid_v2_idempotency_begin(p_actor,'bid.v2.requirement.supersession',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  IF p_old_requirement_revision_id=p_new_requirement_revision_id
     OR NOT kb_bid_v2_applicability_valid(p_applicability) THEN
    RAISE EXCEPTION 'REQUIREMENT_SUPERSESSION_INVALID' USING ERRCODE='23514';
  END IF;
  SELECT * INTO STRICT set_head FROM bid_requirement_set_current WHERE scope_id=p_project_id FOR UPDATE;
  SELECT * INTO STRICT old_set FROM bid_requirement_set_artifacts WHERE id=set_head.artifact_id;
  SELECT * INTO head FROM bid_requirement_supersession_current WHERE scope_id=p_lineage_id FOR UPDATE;
  head_exists:=FOUND;
  IF head_exists THEN
    IF head.project_id<>p_project_id OR head.artifact_id IS DISTINCT FROM p_expected_artifact_id
       OR head.artifact_sha256 IS DISTINCT FROM p_expected_sha256 THEN
      RAISE EXCEPTION 'REQUIREMENT_SUPERSESSION_CAS_MISMATCH' USING ERRCODE='40001';
    END IF;
    SELECT * INTO STRICT prior_edge FROM bid_requirement_supersession_revision_artifacts WHERE id=head.artifact_id;
    new_revision:=head.generation+1;
  ELSE
    IF p_expected_artifact_id IS NOT NULL OR p_expected_sha256 IS NOT NULL OR p_tombstone THEN
      RAISE EXCEPTION 'REQUIREMENT_SUPERSESSION_CAS_MISMATCH' USING ERRCODE='40001';
    END IF;
    new_revision:=1;
  END IF;
  IF p_tombstone AND (prior_edge.old_requirement_revision_id IS DISTINCT FROM p_old_requirement_revision_id
      OR prior_edge.new_requirement_revision_id IS DISTINCT FROM p_new_requirement_revision_id
      OR prior_edge.applicability IS DISTINCT FROM p_applicability) THEN
    RAISE EXCEPTION 'REQUIREMENT_SUPERSESSION_TOMBSTONE_IDENTITY_MISMATCH' USING ERRCODE='23514';
  END IF;
  IF head_exists AND NOT prior_edge.tombstone THEN
    SELECT effective_applicability,ordinal INTO prior_old_effective,prior_old_ordinal
      FROM bid_requirement_set_items WHERE requirement_set_id=old_set.id
        AND requirement_revision_id=prior_edge.old_requirement_revision_id;
    SELECT effective_applicability,ordinal INTO prior_new_effective,prior_new_ordinal
      FROM bid_requirement_set_items WHERE requirement_set_id=old_set.id
        AND requirement_revision_id=prior_edge.new_requirement_revision_id;
    prior_fragments:=kb_bid_v2_applicability_fragments(prior_edge.applicability);
    restored_old_fragments:=ARRAY(SELECT DISTINCT fragment FROM unnest(
      coalesce(kb_bid_v2_applicability_fragments(prior_old_effective),'{}'::text[])||prior_fragments) fragment ORDER BY fragment);
    restored_new_fragments:=ARRAY(SELECT fragment FROM unnest(
      coalesce(kb_bid_v2_applicability_fragments(prior_new_effective),'{}'::text[])) fragment
      WHERE NOT fragment=ANY(prior_fragments) ORDER BY fragment);
    SELECT jsonb_agg(jsonb_build_object('requirement_revision_id',requirement_revision_id,
        'effective_applicability',effective_applicability,'ordinal',ordinal) ORDER BY ordinal,requirement_revision_id)
      INTO base_items FROM (
        SELECT item.requirement_revision_id,item.effective_applicability,item.ordinal
        FROM bid_requirement_set_items item WHERE item.requirement_set_id=old_set.id
          AND item.requirement_revision_id NOT IN (
            prior_edge.old_requirement_revision_id,prior_edge.new_requirement_revision_id)
        UNION ALL
        SELECT prior_edge.old_requirement_revision_id,kb_bid_v2_applicability_from_fragments(restored_old_fragments),
          coalesce(prior_old_ordinal,prior_new_ordinal,0)
        UNION ALL
        SELECT prior_edge.new_requirement_revision_id,kb_bid_v2_applicability_from_fragments(restored_new_fragments),
          coalesce(prior_new_ordinal,prior_old_ordinal,0) WHERE cardinality(restored_new_fragments)>0
      ) restored;
  ELSE
    SELECT jsonb_agg(jsonb_build_object('requirement_revision_id',item.requirement_revision_id,
        'effective_applicability',item.effective_applicability,'ordinal',item.ordinal)
        ORDER BY item.ordinal,item.requirement_revision_id) INTO base_items
      FROM bid_requirement_set_items item WHERE item.requirement_set_id=old_set.id;
  END IF;
  SELECT * INTO STRICT old_requirement FROM bid_requirement_revision_artifacts
    WHERE project_id=p_project_id AND id=p_old_requirement_revision_id
      AND EXISTS (SELECT 1 FROM jsonb_array_elements(base_items) item
        WHERE (item->>'requirement_revision_id')::uuid=p_old_requirement_revision_id);
  SELECT item->'effective_applicability' INTO STRICT old_effective FROM jsonb_array_elements(base_items) item
    WHERE (item->>'requirement_revision_id')::uuid=p_old_requirement_revision_id;
  SELECT * INTO STRICT new_requirement FROM bid_requirement_revision_artifacts
    WHERE project_id=p_project_id AND id=p_new_requirement_revision_id;
  SELECT item->'effective_applicability' INTO new_effective FROM jsonb_array_elements(base_items) item
    WHERE (item->>'requirement_revision_id')::uuid=p_new_requirement_revision_id;
  old_fragments:=kb_bid_v2_applicability_fragments(old_effective);
  new_fragments:=kb_bid_v2_applicability_fragments(new_requirement.applicability);
  edge_fragments:=kb_bid_v2_applicability_fragments(p_applicability);
  IF (old_fragments=ARRAY['*']::text[] AND edge_fragments<>ARRAY['*']::text[])
     OR (old_fragments<>ARRAY['*']::text[] AND NOT edge_fragments<@old_fragments)
     OR (new_fragments<>ARRAY['*']::text[] AND NOT edge_fragments<@new_fragments)
     OR (new_effective IS NOT NULL AND edge_fragments&&kb_bid_v2_applicability_fragments(new_effective)) THEN
    RAISE EXCEPTION 'REQUIREMENT_SUPERSESSION_APPLICABILITY_AMBIGUOUS' USING ERRCODE='23514';
  END IF;
  remaining_fragments:=ARRAY(SELECT fragment FROM unnest(old_fragments) fragment
    WHERE NOT fragment=ANY(edge_fragments) ORDER BY fragment);
  SELECT array_agg(source_unit_revision_id ORDER BY source_unit_revision_id) INTO old_sources
    FROM bid_requirement_source_revision_artifacts WHERE project_id=p_project_id
      AND requirement_revision_id=p_old_requirement_revision_id;
  SELECT array_agg(source_unit_revision_id ORDER BY source_unit_revision_id) INTO new_sources
    FROM bid_requirement_source_revision_artifacts WHERE project_id=p_project_id
      AND requirement_revision_id=p_new_requirement_revision_id;
  IF coalesce(cardinality(old_sources),0)=0 OR coalesce(cardinality(new_sources),0)=0
     OR EXISTS (SELECT 1 FROM unnest(old_sources||new_sources) source_ids(source_id)
       JOIN bid_source_unit_revision_artifacts source ON source.project_id=p_project_id AND source.id=source_ids.source_id
       WHERE NOT EXISTS (SELECT 1 FROM bid_document_set_items set_item
         WHERE set_item.document_set_id=old_set.document_set_id AND set_item.document_id=source.document_id)) THEN
    RAISE EXCEPTION 'REQUIREMENT_SUPERSESSION_SOURCE_SET_INVALID' USING ERRCODE='23514';
  END IF;
  SELECT jsonb_build_object('relation_lineage_id',relation.relation_lineage_id,
      'relation_revision_id',relation.id,'relation_sha256',relation.content_sha256,
      'relation_kind',relation.relation_kind,'applicability',relation.applicability)
    INTO relation_identity
    FROM bid_document_relation_current relation_head
    JOIN bid_document_relation_revision_artifacts relation ON relation.id=relation_head.artifact_id
    WHERE relation.project_id=p_project_id AND NOT relation.tombstone
      AND EXISTS (SELECT 1 FROM bid_source_unit_revision_artifacts old_source
        WHERE old_source.id=ANY(old_sources) AND old_source.document_id IN (relation.from_document_id,relation.to_document_id))
      AND EXISTS (SELECT 1 FROM bid_source_unit_revision_artifacts new_source
        WHERE new_source.id=ANY(new_sources) AND new_source.document_id IN (relation.from_document_id,relation.to_document_id))
    ORDER BY relation.relation_lineage_id LIMIT 1;
  IF relation_identity IS NULL AND EXISTS (
      SELECT 1 FROM bid_source_unit_revision_artifacts old_source,bid_source_unit_revision_artifacts new_source
      WHERE old_source.id=ANY(old_sources) AND new_source.id=ANY(new_sources)
        AND old_source.document_id<>new_source.document_id) THEN
    RAISE EXCEPTION 'REQUIREMENT_SUPERSESSION_DOCUMENT_RELATION_MISSING' USING ERRCODE='23514';
  END IF;
  IF NOT p_tombstone AND EXISTS (WITH RECURSIVE reachable(node) AS (
      SELECT p_new_requirement_revision_id
      UNION
      SELECT edge.new_requirement_revision_id FROM reachable path
      JOIN bid_requirement_supersession_current current_edge ON current_edge.project_id=p_project_id AND current_edge.scope_id<>p_lineage_id
      JOIN bid_requirement_supersession_revision_artifacts edge ON edge.id=current_edge.artifact_id
        AND edge.old_requirement_revision_id=path.node AND NOT edge.tombstone)
    SELECT 1 FROM reachable WHERE node=p_old_requirement_revision_id) THEN
    RAISE EXCEPTION 'REQUIREMENT_SUPERSESSION_CYCLE' USING ERRCODE='23514';
  END IF;
  payload:=kb_bid_v2_json_payload(jsonb_build_object('schema_version',1,'lineage_id',p_lineage_id,
    'revision',new_revision,'old_requirement_revision_id',p_old_requirement_revision_id,
    'new_requirement_revision_id',p_new_requirement_revision_id,'old_source_unit_revision_ids',to_jsonb(old_sources),
    'new_source_unit_revision_ids',to_jsonb(new_sources),'amendment_document_relation',relation_identity,
    'applicability',p_applicability,'tombstone',p_tombstone));
  sha:=kb_bid_v2_sha256_bytes(payload);
  INSERT INTO bid_requirement_supersession_revision_artifacts(id,project_id,lineage_id,revision,
    old_requirement_revision_id,new_requirement_revision_id,old_source_unit_revision_ids,
    new_source_unit_revision_ids,amendment_document_relation_revision_id,amendment_document_relation_sha256,
    applicability,tombstone,canonical_payload,content_sha256,actor)
  VALUES(new_id,p_project_id,p_lineage_id,new_revision,p_old_requirement_revision_id,
    p_new_requirement_revision_id,old_sources,new_sources,
    (relation_identity->>'relation_revision_id')::uuid,(relation_identity->>'relation_sha256')::kb_sha256,
    p_applicability,p_tombstone,payload,sha,p_actor);
  IF NOT kb_bid_v2_advance_requirement_supersession(p_project_id,p_lineage_id,
      p_expected_artifact_id,p_expected_sha256,new_id,sha) THEN
    RAISE EXCEPTION 'REQUIREMENT_SUPERSESSION_CAS_MISMATCH' USING ERRCODE='40001'; END IF;
  SELECT jsonb_agg(jsonb_build_object('requirement_revision_id',requirement_revision_id,
      'effective_applicability',effective_applicability,'ordinal',new_ordinal)
      ORDER BY new_ordinal) INTO set_items FROM (
    SELECT item.requirement_revision_id,item.effective_applicability,
      row_number() OVER(ORDER BY item.source_ordinal,item.requirement_revision_id)-1 new_ordinal
    FROM (
      SELECT (base_item->>'requirement_revision_id')::uuid requirement_revision_id,
        base_item->'effective_applicability' effective_applicability,
        (base_item->>'ordinal')::integer source_ordinal
      FROM jsonb_array_elements(base_items) base_item
      WHERE p_tombstone OR (base_item->>'requirement_revision_id')::uuid
        NOT IN (p_old_requirement_revision_id,p_new_requirement_revision_id)
      UNION ALL
      SELECT p_old_requirement_revision_id,kb_bid_v2_applicability_from_fragments(remaining_fragments),
        (SELECT (base_item->>'ordinal')::integer FROM jsonb_array_elements(base_items) base_item
          WHERE (base_item->>'requirement_revision_id')::uuid=p_old_requirement_revision_id)
      WHERE NOT p_tombstone AND cardinality(remaining_fragments)>0
      UNION ALL
      SELECT p_new_requirement_revision_id,kb_bid_v2_applicability_from_fragments(
          CASE WHEN new_effective IS NULL THEN edge_fragments
            ELSE kb_bid_v2_applicability_fragments(new_effective)||edge_fragments END),
        (SELECT (base_item->>'ordinal')::integer FROM jsonb_array_elements(base_items) base_item
          WHERE (base_item->>'requirement_revision_id')::uuid=p_old_requirement_revision_id)
      WHERE NOT p_tombstone
    ) item) effective;
    set_payload:=kb_bid_v2_json_payload(jsonb_build_object('schema_version',1,'project_id',p_project_id,
      'document_set_revision_id',old_set.document_set_id,'disposition_set_revision_id',old_set.disposition_set_id,
      'revision',set_head.generation+1,'supersession_revision_id',new_id,'items',set_items));
    set_sha:=kb_bid_v2_sha256_bytes(set_payload);
    INSERT INTO bid_requirement_set_artifacts(id,project_id,document_set_id,document_set_sequence,
      disposition_set_id,disposition_set_sequence,revision,canonical_payload,content_sha256)
    VALUES(new_set_id,p_project_id,old_set.document_set_id,old_set.document_set_sequence,
      old_set.disposition_set_id,old_set.disposition_set_sequence,set_head.generation+1,set_payload,set_sha);
    INSERT INTO bid_requirement_set_items(requirement_set_id,project_id,requirement_revision_id,effective_applicability,ordinal)
    SELECT new_set_id,p_project_id,(item->>'requirement_revision_id')::uuid,item->'effective_applicability',
      (item->>'ordinal')::integer FROM jsonb_array_elements(set_items) item;
    IF NOT kb_bid_v2_advance_requirement_set(p_project_id,set_head.artifact_id,set_head.artifact_sha256,new_set_id,set_sha) THEN
      RAISE EXCEPTION 'REQUIREMENT_SET_CAS_MISMATCH' USING ERRCODE='40001'; END IF;
    SELECT * INTO STRICT workspace_value FROM bid_submission_workspaces WHERE project_id=p_project_id;
    SELECT * INTO STRICT projection_head FROM bid_workspace_requirement_projection_current WHERE scope_id=workspace_value.id FOR UPDATE;
    projection_revision:=projection_head.generation+1;
    projection_payload:=kb_bid_v2_json_payload(jsonb_build_object('schema_version',1,'workspace_id',workspace_value.id,
      'requirement_set_id',new_set_id,'revision',projection_revision,'supersession_revision_id',new_id,'items',set_items));
    projection_sha:=kb_bid_v2_sha256_bytes(projection_payload);
    INSERT INTO bid_workspace_requirement_projection_artifacts(id,project_id,workspace_id,requirement_set_id,
      revision,canonical_payload,content_sha256)
    VALUES(projection_id,p_project_id,workspace_value.id,new_set_id,projection_revision,projection_payload,projection_sha);
    INSERT INTO bid_workspace_requirement_projection_items(projection_id,project_id,requirement_revision_id,effective_applicability,ordinal)
    SELECT projection_id,p_project_id,(item->>'requirement_revision_id')::uuid,item->'effective_applicability',
      (item->>'ordinal')::integer FROM jsonb_array_elements(set_items) item;
    IF NOT kb_bid_v2_advance_requirement_projection(p_project_id,workspace_value.id,projection_head.artifact_id,
      projection_head.artifact_sha256,projection_id,projection_sha) THEN
      RAISE EXCEPTION 'REQUIREMENT_PROJECTION_CAS_MISMATCH' USING ERRCODE='40001'; END IF;
  response:=jsonb_build_object('artifact_id',new_id,'lineage_id',p_lineage_id,
    'revision',new_revision,'sha256',sha,'tombstone',p_tombstone,
    'requirement_set_id',new_set_id,'requirement_set_sha256',set_sha,
    'requirement_projection_id',projection_id,'requirement_projection_sha256',projection_sha,
    'workspace_apply_required',true);
  response_bytes:=convert_to(response::text,'UTF8');
  INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,
    response_sha256,entity_kind,entity_locator,after_revision,after_sha256)
  VALUES(gen_random_uuid(),1,'bid.v2.requirement.supersession',p_actor,p_idempotency_key,p_request_sha256,
    kb_bid_v2_sha256_bytes(response_bytes),'bid_v2_requirement_supersession',jsonb_build_object('lineage_id',p_lineage_id),
    new_revision,sha);
  PERFORM kb_bid_v2_idempotency_complete(p_actor,'bid.v2.requirement.supersession',p_idempotency_key,201,response_bytes);
  RETURN response;
END $$;

CREATE FUNCTION kb_bid_v2_list_requirements(
  p_project_id uuid,p_actor kb_actor_identity
) RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE current_value bid_requirement_set_current%ROWTYPE;
BEGIN
  PERFORM kb_bid_v2_require_project_owner(p_project_id,p_actor);
  SELECT * INTO current_value FROM bid_requirement_set_current WHERE scope_id=p_project_id;
  RETURN COALESCE((SELECT jsonb_agg(jsonb_build_object(
      'requirement_revision_id',r.id,'lineage_id',r.lineage_id,'revision',r.revision,
      'requirement_kind',r.requirement_kind,'requiredness',r.requiredness,
      'compliance_policy',r.compliance_policy,'lifecycle',r.lifecycle,
      'text',convert_from(r.text_utf8,'UTF8'),'content_sha256',r.content_sha256,
      'requirement_set_id',current_value.artifact_id,'requirement_set_sha256',current_value.artifact_sha256,
      'fulfillment_expr',r.fulfillment_expr,'applicability',r.applicability,
      'effective_applicability',item.effective_applicability,
      'source_unit_revision_ids',COALESCE((SELECT jsonb_agg(source.source_unit_revision_id ORDER BY source.id)
        FROM bid_requirement_source_revision_artifacts source
        WHERE source.project_id=r.project_id AND source.requirement_revision_id=r.id),'[]'::jsonb))
      ORDER BY item.ordinal)
    FROM bid_requirement_set_items item JOIN bid_requirement_revision_artifacts r
      ON r.project_id=item.project_id AND r.id=item.requirement_revision_id
    WHERE item.requirement_set_id=current_value.artifact_id),'[]'::jsonb);
END $$;

CREATE FUNCTION kb_bid_v2_domain_sha256(p_domain text,p_payload bytea)
RETURNS kb_sha256 LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE SET search_path=pg_catalog,public AS $$
  SELECT encode(digest(convert_to(p_domain,'UTF8')||decode('00','hex')||p_payload,'sha256'),'hex')::kb_sha256
$$;

CREATE FUNCTION kb_bid_v2_uuid_v5(p_namespace uuid,p_name bytea)
RETURNS uuid LANGUAGE plpgsql IMMUTABLE STRICT PARALLEL SAFE SET search_path=pg_catalog,public AS $$
DECLARE digest_value bytea; hex_value text;
BEGIN
  digest_value:=digest(uuid_send(p_namespace)||p_name,'sha1');
  digest_value:=set_byte(digest_value,6,(get_byte(digest_value,6)&15)|80);
  digest_value:=set_byte(digest_value,8,(get_byte(digest_value,8)&63)|128);
  hex_value:=encode(substring(digest_value FROM 1 FOR 16),'hex');
  RETURN (substring(hex_value,1,8)||'-'||substring(hex_value,9,4)||'-'||substring(hex_value,13,4)||'-'||substring(hex_value,17,4)||'-'||substring(hex_value,21,12))::uuid;
END $$;


CREATE FUNCTION kb_bid_v2_jsonb_storage_order_compact(p_value jsonb)
RETURNS text LANGUAGE plpgsql IMMUTABLE STRICT PARALLEL SAFE SET search_path=pg_catalog,public AS $$
DECLARE result_value text; pair record; item jsonb; separator text:='';
BEGIN
  IF jsonb_typeof(p_value)='object' THEN
    result_value:='{';
    FOR pair IN SELECT key,value FROM jsonb_each(p_value) LOOP
      result_value:=result_value||separator||to_jsonb(pair.key)::text||':'||kb_bid_v2_jsonb_storage_order_compact(pair.value);
      separator:=',';
    END LOOP;
    RETURN result_value||'}';
  ELSIF jsonb_typeof(p_value)='array' THEN
    result_value:='[';
    FOR item IN SELECT value FROM jsonb_array_elements(p_value) LOOP
      result_value:=result_value||separator||kb_bid_v2_jsonb_storage_order_compact(item);
      separator:=',';
    END LOOP;
    RETURN result_value||']';
  END IF;
  RETURN p_value::text;
END $$;


-- AgentRun diagnostics are bounded in UTF-8 bytes, not PostgreSQL characters.
CREATE FUNCTION kb_bid_v2_diagnostic_prefix(p_message text)
RETURNS text LANGUAGE plpgsql IMMUTABLE STRICT PARALLEL SAFE
SET search_path=pg_catalog AS $$
DECLARE bounded bytea:=convert_to(left(p_message,8192),'UTF8'); byte_count integer:=8192;
BEGIN
  IF octet_length(bounded)<=byte_count THEN RETURN convert_from(bounded,'UTF8'); END IF;
  -- byte_count indexes the first excluded byte; back up over continuation bytes.
  WHILE get_byte(bounded,byte_count) BETWEEN 128 AND 191 LOOP
    byte_count:=byte_count-1;
  END LOOP;
  RETURN convert_from(substring(bounded FROM 1 FOR byte_count),'UTF8');
END $$;


CREATE FUNCTION kb_bid_v2_get_async_request(
  p_workspace_id uuid,p_request_id uuid,p_actor kb_actor_identity
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE request_value bid_async_request_snapshot_artifacts%ROWTYPE; owner_project_id uuid;
BEGIN
  SELECT workspace.project_id INTO STRICT owner_project_id
    FROM bid_submission_workspaces workspace WHERE workspace.id=p_workspace_id;
  PERFORM kb_bid_v2_require_project_owner(owner_project_id,p_actor);
  SELECT * INTO request_value FROM bid_async_request_snapshot_artifacts
    WHERE id=p_request_id AND workspace_id=p_workspace_id;
  IF NOT FOUND THEN RETURN NULL; END IF;
  RETURN jsonb_build_object('request_artifact_id',request_value.id,
    'request_revision',request_value.revision,'frozen_input_sha256',request_value.frozen_input_sha256,
    'request_sha256',request_value.request_sha256,
    'kind',CASE request_value.request_kind       WHEN 'content_generate' THEN 'ContentGenerate' WHEN 'submission_export' THEN 'SubmissionExport'
      ELSE request_value.request_kind END,
    'status',request_value.status,'result_identity',request_value.result_identity,'error_code',request_value.error_code,
    'operation',CASE WHEN request_value.request_kind='content_generate' THEN (
      SELECT identity.request_operation FROM bid_content_generation_request_identities identity
      WHERE identity.request_artifact_id=request_value.id) END,
    'progress',NULL);
END $$;

CREATE FUNCTION kb_bid_v2_list_workspace_async_requests(
  p_workspace_id uuid,p_actor kb_actor_identity
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE owner_project_id uuid;
BEGIN
  SELECT workspace.project_id INTO STRICT owner_project_id
    FROM bid_submission_workspaces workspace WHERE workspace.id=p_workspace_id;
  PERFORM kb_bid_v2_require_project_owner(owner_project_id,p_actor);
  RETURN coalesce((SELECT jsonb_agg(jsonb_build_object(
      'request_artifact_id',request_value.id,'request_revision',request_value.revision,
      'frozen_input_sha256',request_value.frozen_input_sha256,'request_sha256',request_value.request_sha256,
      'kind',CASE request_value.request_kind         WHEN 'content_generate' THEN 'ContentGenerate' WHEN 'submission_export' THEN 'SubmissionExport'
        ELSE request_value.request_kind END,
      'status',request_value.status,'result_identity',request_value.result_identity,'error_code',request_value.error_code,
      'operation',CASE WHEN request_value.request_kind='content_generate' THEN (
        SELECT identity.request_operation FROM bid_content_generation_request_identities identity
        WHERE identity.request_artifact_id=request_value.id) END,
      'progress',NULL)
      ORDER BY request_value.created_at DESC)
    FROM bid_async_request_snapshot_artifacts request_value
    WHERE request_value.workspace_id=p_workspace_id
      AND request_value.created_at>=clock_timestamp()-interval '7 days'
      AND request_value.request_kind IN ('content_generate','submission_export')),'[]'::jsonb);
END $$;

-- One STABLE operation uses the caller's statement snapshot for ACL, publication and CAS.
-- No locks/writes, latest-revision substitution, or generation owner/lease requirement.

CREATE FUNCTION kb_bid_v2_get_candidate(
  p_workspace_id uuid,p_candidate_id uuid,p_actor kb_actor_identity
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE candidate bid_candidate_artifacts%ROWTYPE; owner_project_id uuid; payload jsonb; head bid_workspace_heads%ROWTYPE;
BEGIN
  SELECT workspace.project_id INTO STRICT owner_project_id
    FROM bid_submission_workspaces workspace WHERE workspace.id=p_workspace_id;
  PERFORM kb_bid_v2_require_project_owner(owner_project_id,p_actor);
  SELECT * INTO candidate FROM bid_candidate_artifacts WHERE id=p_candidate_id AND workspace_id=p_workspace_id;
  IF NOT FOUND THEN RETURN NULL; END IF;
  SELECT * INTO STRICT head FROM bid_workspace_heads WHERE scope_id=p_workspace_id;
  payload:=convert_from(candidate.canonical_payload,'UTF8')::jsonb;
  RETURN jsonb_build_object('candidate_id',candidate.id,'kind',candidate.candidate_kind,
    'status',CASE WHEN candidate.state='proposed' AND
      (candidate.base_workspace_revision_id IS DISTINCT FROM head.artifact_id OR
       candidate.base_workspace_sha256 IS DISTINCT FROM head.artifact_sha256)
      THEN 'obsolete' ELSE candidate.state END,
    'stored_status',candidate.state,'base_workspace_revision_id',candidate.base_workspace_revision_id,
    'base_workspace_sha256',candidate.base_workspace_sha256)
    || payload;
END $$;

CREATE FUNCTION kb_bid_v2_accept_candidate(
  p_workspace_id uuid,p_candidate_id uuid,p_expected_revision_id uuid,p_expected_sha256 kb_sha256,
  p_snapshot jsonb,p_selected_ordinals integer[],p_actor kb_actor_identity,p_idempotency_key text,
  p_request_bytes bytea,p_request_sha256 kb_sha256
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE candidate bid_candidate_artifacts%ROWTYPE; head bid_workspace_heads%ROWTYPE;
  replay bytea; response jsonb; response_bytes bytea; candidate_payload jsonb;
  selection record; accepted_id uuid; accepted_json jsonb; accepted_payload bytea; accepted_sha kb_sha256;
  before_generation bigint; after_generation bigint;
BEGIN
  SELECT * INTO STRICT candidate FROM bid_candidate_artifacts
    WHERE id=p_candidate_id AND workspace_id=p_workspace_id FOR UPDATE;
  PERFORM kb_bid_v2_require_project_owner(candidate.project_id,p_actor);
  replay:=kb_bid_v2_idempotency_begin(p_actor,'bid.v2.candidate.accept',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  IF candidate.state='accepted' THEN
    SELECT response_payload INTO STRICT response_bytes FROM bid_candidate_decision_receipts WHERE candidate_id=p_candidate_id;
    PERFORM kb_bid_v2_idempotency_complete(p_actor,'bid.v2.candidate.accept',p_idempotency_key,200,response_bytes);
    RETURN convert_from(response_bytes,'UTF8')::jsonb;
  END IF;
  IF candidate.state<>'proposed' THEN RAISE EXCEPTION 'CANDIDATE_NOT_PROPOSED' USING ERRCODE='23514'; END IF;
  SELECT * INTO STRICT head FROM bid_workspace_heads WHERE scope_id=p_workspace_id FOR UPDATE;
  IF head.artifact_id IS DISTINCT FROM candidate.base_workspace_revision_id OR
     head.artifact_sha256 IS DISTINCT FROM candidate.base_workspace_sha256 THEN
    response:=jsonb_build_object('candidate_id',p_candidate_id,'status','obsolete',
      'stored_status',candidate.state,'error_code','CANDIDATE_OBSOLETE',
      'current_workspace_revision_id',head.artifact_id,'current_workspace_sha256',head.artifact_sha256);
    response_bytes:=convert_to(response::text,'UTF8');
    PERFORM kb_bid_v2_idempotency_complete(p_actor,'bid.v2.candidate.accept',p_idempotency_key,409,response_bytes);
    RETURN response;
  END IF;
  IF candidate.base_workspace_revision_id IS DISTINCT FROM p_expected_revision_id OR
     candidate.base_workspace_sha256 IS DISTINCT FROM p_expected_sha256 THEN
    RAISE EXCEPTION 'CANDIDATE_BASE_CAS_MISMATCH' USING ERRCODE='40001';
  END IF;
  IF cardinality(coalesce(p_selected_ordinals,ARRAY[]::integer[]))<>
       (SELECT count(DISTINCT ordinal) FROM unnest(coalesce(p_selected_ordinals,ARRAY[]::integer[])) ordinal)
     OR EXISTS (SELECT 1 FROM unnest(coalesce(p_selected_ordinals,ARRAY[]::integer[])) selected
       WHERE NOT EXISTS (SELECT 1 FROM bid_candidate_operations operation
         WHERE operation.candidate_id=p_candidate_id AND operation.ordinal=selected))
     OR (EXISTS (SELECT 1 FROM bid_candidate_operations WHERE candidate_id=p_candidate_id)
       AND cardinality(coalesce(p_selected_ordinals,ARRAY[]::integer[]))=0) THEN
    RAISE EXCEPTION 'CANDIDATE_SELECTION_INVALID' USING ERRCODE='23514';
  END IF;
  before_generation:=head.generation;
  response:=kb_bid_v2_commit_workspace_mutation(p_workspace_id,p_expected_revision_id,p_expected_sha256,p_snapshot,p_actor);
  PERFORM kb_bid_v2_record_accepted_candidate_evidence(
    p_workspace_id,(response->>'revision_id')::uuid,p_candidate_id,p_selected_ordinals);
  UPDATE bid_candidate_artifacts SET state='accepted',decided_at=clock_timestamp() WHERE id=p_candidate_id;
  candidate_payload:=convert_from(candidate.canonical_payload,'UTF8')::jsonb;
  FOR selection IN
    SELECT selected.matching_report_id,array_agg(DISTINCT selected.item_id ORDER BY selected.item_id) item_ids
    FROM (
      SELECT bundle.matching_report_id,(claim->>'evidence_item_id')::uuid item_id
      FROM jsonb_array_elements(coalesce(candidate_payload->'factual_claims','[]'::jsonb)) claim
      JOIN bid_evidence_bundle_artifacts bundle ON bundle.id=(claim->>'evidence_bundle_id')::uuid
      WHERE EXISTS (SELECT 1 FROM bid_candidate_operations operation
        WHERE operation.candidate_id=p_candidate_id AND operation.ordinal=ANY(coalesce(p_selected_ordinals,ARRAY[]::integer[]))
          AND operation.operation->>'client_operation_ref'=claim->>'client_operation_ref')
      UNION ALL
      SELECT bundle.matching_report_id,item.id
      FROM bid_candidate_operations operation
      JOIN bid_content_generation_request_evidence_bundles link ON link.request_artifact_id=candidate.request_artifact_id
      JOIN bid_evidence_bundle_artifacts bundle ON bundle.id=link.evidence_bundle_id
      JOIN bid_evidence_bundle_items item ON item.evidence_bundle_id=bundle.id
        AND item.id=(operation.operation#>>'{block,content,asset_revision_id}')::uuid AND item.item_kind='image'
      WHERE operation.candidate_id=p_candidate_id
        AND operation.ordinal=ANY(coalesce(p_selected_ordinals,ARRAY[]::integer[]))
        AND operation.operation#>>'{block,kind}'='image'
    ) selected GROUP BY selected.matching_report_id
  LOOP
    accepted_id:=kb_bid_v2_deterministic_uuid(p_candidate_id::text||':'||selection.matching_report_id::text||':accepted');
    accepted_json:=jsonb_build_object('schema_version',1,'selection_id',accepted_id,
      'selection_kind','accepted','matching_report_id',selection.matching_report_id,
      'candidate_id',p_candidate_id,'selected_evidence_item_ids',selection.item_ids);
    accepted_payload:=kb_bid_v2_json_payload(accepted_json);accepted_sha:=kb_bid_v2_sha256_bytes(accepted_payload);
    INSERT INTO bid_evidence_selection_artifacts(id,project_id,workspace_id,selection_kind,matching_report_id,
      canonical_payload,content_sha256,actor)
    VALUES(accepted_id,candidate.project_id,p_workspace_id,'accepted',selection.matching_report_id,
      accepted_payload,accepted_sha,p_actor);
  END LOOP;
  SELECT generation INTO STRICT after_generation FROM bid_workspace_heads WHERE scope_id=p_workspace_id;
  response_bytes:=convert_to(response::text,'UTF8');
  INSERT INTO bid_candidate_decision_receipts(candidate_id,actor,accepted_operation_ordinals,
    accepted_operations,resulting_workspace_revision_id,response_payload,response_sha256)
  VALUES(p_candidate_id,p_actor,coalesce(p_selected_ordinals,ARRAY[]::integer[]),
    coalesce((SELECT jsonb_agg(jsonb_build_object(
        'ordinal',operation.ordinal,'operation_sha256',operation.operation_sha256)
      ORDER BY operation.ordinal)
      FROM bid_candidate_operations operation
      WHERE operation.candidate_id=p_candidate_id
        AND operation.ordinal=ANY(coalesce(p_selected_ordinals,ARRAY[]::integer[]))),'[]'::jsonb),
    (response->>'revision_id')::uuid,response_bytes,kb_bid_v2_sha256_bytes(response_bytes));
  INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
    entity_kind,entity_locator,before_revision,before_sha256,after_revision,after_sha256)
  VALUES(gen_random_uuid(),1,'bid.v2.candidate.accept',p_actor,p_idempotency_key,p_request_sha256,
    kb_bid_v2_sha256_bytes(response_bytes),'bid_v2_workspace',jsonb_build_object('workspace_id',p_workspace_id,'candidate_id',p_candidate_id),
    before_generation,p_expected_sha256,after_generation,(response->>'sha256')::kb_sha256);
  PERFORM kb_bid_v2_idempotency_complete(p_actor,'bid.v2.candidate.accept',p_idempotency_key,200,response_bytes);
  RETURN response;
END $$;

CREATE FUNCTION kb_bid_v2_reject_candidate(
  p_workspace_id uuid,p_candidate_id uuid,p_actor kb_actor_identity,p_idempotency_key text,
  p_request_bytes bytea,p_request_sha256 kb_sha256
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE candidate bid_candidate_artifacts%ROWTYPE; replay bytea; response jsonb; response_bytes bytea;
BEGIN
  SELECT * INTO STRICT candidate FROM bid_candidate_artifacts
    WHERE id=p_candidate_id AND workspace_id=p_workspace_id FOR UPDATE;
  PERFORM kb_bid_v2_require_project_owner(candidate.project_id,p_actor);
  replay:=kb_bid_v2_idempotency_begin(p_actor,'bid.v2.candidate.reject',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  IF candidate.state<>'proposed' THEN RAISE EXCEPTION 'CANDIDATE_NOT_PROPOSED' USING ERRCODE='23514'; END IF;
  UPDATE bid_candidate_artifacts SET state='rejected',decided_at=clock_timestamp() WHERE id=p_candidate_id;
  response:=kb_bid_v2_get_candidate(p_workspace_id,p_candidate_id,p_actor);
  response_bytes:=convert_to(response::text,'UTF8');
  INSERT INTO bid_candidate_decision_receipts(
    candidate_id,actor,accepted_operation_ordinals,accepted_operations,response_payload,response_sha256)
  VALUES(p_candidate_id,p_actor,ARRAY[]::integer[],'[]'::jsonb,
    response_bytes,kb_bid_v2_sha256_bytes(response_bytes));
  PERFORM kb_bid_v2_idempotency_complete(p_actor,'bid.v2.candidate.reject',p_idempotency_key,200,response_bytes);
  RETURN response;
END $$;

CREATE FUNCTION kb_bid_v2_commit_workspace_mutation_idempotent(
  p_workspace_id uuid,p_expected_revision_id uuid,p_expected_sha256 kb_sha256,
  p_snapshot jsonb,p_actor kb_actor_identity,p_idempotency_key text,
  p_request_bytes bytea,p_request_sha256 kb_sha256
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE replay bytea; response jsonb; response_bytes bytea; before_generation bigint; after_generation bigint;
BEGIN
  replay:=kb_bid_v2_idempotency_begin(p_actor,'bid.v2.workspace.mutate',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  SELECT generation INTO STRICT before_generation FROM bid_workspace_heads WHERE scope_id=p_workspace_id;
  response:=kb_bid_v2_commit_workspace_mutation(p_workspace_id,p_expected_revision_id,
    p_expected_sha256,p_snapshot,p_actor);
  SELECT generation INTO STRICT after_generation FROM bid_workspace_heads WHERE scope_id=p_workspace_id;
  response_bytes:=convert_to(response::text,'UTF8');
  INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,
    request_sha256,response_sha256,entity_kind,entity_locator,before_revision,before_sha256,after_revision,after_sha256)
  VALUES(gen_random_uuid(),1,'bid.v2.workspace.mutate',p_actor,p_idempotency_key,p_request_sha256,
    kb_bid_v2_sha256_bytes(response_bytes),'bid_v2_workspace',jsonb_build_object('workspace_id',p_workspace_id),
    before_generation,p_expected_sha256,after_generation,(response->>'sha256')::kb_sha256);
  PERFORM kb_bid_v2_idempotency_complete(p_actor,'bid.v2.workspace.mutate',p_idempotency_key,200,response_bytes);
  RETURN response;
END $$;

CREATE FUNCTION kb_bid_v2_load_workspace_for_actor(
  p_workspace_id uuid,p_actor kb_actor_identity
) RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE project_id uuid;
BEGIN
  SELECT value.project_id INTO project_id FROM bid_submission_workspaces value WHERE value.id=p_workspace_id;
  IF NOT FOUND THEN RAISE EXCEPTION 'WORKSPACE_NOT_FOUND' USING ERRCODE='P0002'; END IF;
  PERFORM kb_bid_v2_require_project_owner(project_id,p_actor);
  RETURN kb_bid_v2_load_workspace(p_workspace_id);
END $$;

CREATE FUNCTION kb_bid_v2_get_requirement_projection(
  p_workspace_id uuid,p_actor kb_actor_identity
) RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE workspace bid_submission_workspaces%ROWTYPE; current_value bid_workspace_requirement_projection_current%ROWTYPE;
BEGIN
  SELECT * INTO STRICT workspace FROM bid_submission_workspaces WHERE id=p_workspace_id;
  PERFORM kb_bid_v2_require_project_owner(workspace.project_id,p_actor);
  SELECT * INTO STRICT current_value FROM bid_workspace_requirement_projection_current WHERE scope_id=p_workspace_id;
  RETURN (SELECT jsonb_build_object('artifact_id',projection.id,'sha256',projection.content_sha256,
    'revision',projection.revision,'requirement_set_id',projection.requirement_set_id,
    'items',coalesce((SELECT jsonb_agg(jsonb_build_object('requirement_revision_id',item.requirement_revision_id,
      'effective_applicability',item.effective_applicability,'ordinal',item.ordinal)
      ORDER BY item.ordinal,item.requirement_revision_id)
      FROM bid_workspace_requirement_projection_items item WHERE item.projection_id=projection.id),'[]'::jsonb))
    FROM bid_workspace_requirement_projection_artifacts projection WHERE projection.id=current_value.artifact_id);
END $$;

CREATE FUNCTION kb_bid_v2_refresh_requirement_projection(
  p_workspace_id uuid,p_expected_artifact_id uuid,p_expected_sha256 kb_sha256,
  p_expected_workspace_revision_id uuid,p_expected_workspace_sha256 kb_sha256,p_actor kb_actor_identity,
  p_idempotency_key text,p_request_bytes bytea,p_request_sha256 kb_sha256
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE workspace bid_submission_workspaces%ROWTYPE; current_value bid_workspace_requirement_projection_current%ROWTYPE;
  head bid_workspace_heads%ROWTYPE; replay bytea; response jsonb; response_bytes bytea; before_generation bigint;
BEGIN
  replay:=kb_bid_v2_idempotency_begin(p_actor,'bid.v2.requirement-projection.refresh',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  SELECT * INTO STRICT workspace FROM bid_submission_workspaces WHERE id=p_workspace_id;
  PERFORM kb_bid_v2_require_project_owner(workspace.project_id,p_actor);
  SELECT * INTO STRICT current_value FROM bid_workspace_requirement_projection_current WHERE scope_id=p_workspace_id FOR SHARE;
  IF current_value.artifact_id IS DISTINCT FROM p_expected_artifact_id OR current_value.artifact_sha256 IS DISTINCT FROM p_expected_sha256 THEN
    RAISE EXCEPTION 'REQUIREMENT_PROJECTION_CAS_MISMATCH' USING ERRCODE='40001';
  END IF;
  SELECT * INTO STRICT head FROM bid_workspace_heads WHERE scope_id=p_workspace_id FOR UPDATE;
  before_generation:=head.generation;
  IF head.artifact_id IS DISTINCT FROM p_expected_workspace_revision_id
     OR head.artifact_sha256 IS DISTINCT FROM p_expected_workspace_sha256 THEN
    RAISE EXCEPTION 'WORKSPACE_HEAD_CAS_MISMATCH' USING ERRCODE='40001';
  END IF;
  IF EXISTS (SELECT 1 FROM bid_workspace_revision_artifacts revision WHERE revision.id=head.artifact_id
    AND revision.requirement_projection_id=current_value.artifact_id
    AND revision.requirement_projection_sha256=current_value.artifact_sha256) THEN
    response:=kb_bid_v2_load_workspace(p_workspace_id);
  ELSE
    PERFORM kb_bid_v2_advance_workspace_projection(p_workspace_id,
      (SELECT requirement_projection_id FROM bid_workspace_revision_artifacts WHERE id=head.artifact_id),
      (SELECT requirement_projection_sha256 FROM bid_workspace_revision_artifacts WHERE id=head.artifact_id),
      current_value.artifact_id,current_value.artifact_sha256,p_actor);
    response:=kb_bid_v2_load_workspace(p_workspace_id);
  END IF;
  response_bytes:=convert_to(response::text,'UTF8');
  INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
    entity_kind,entity_locator,before_revision,before_sha256,after_revision,after_sha256)
  SELECT gen_random_uuid(),1,'bid.v2.requirement-projection.refresh',p_actor,p_idempotency_key,p_request_sha256,
    kb_bid_v2_sha256_bytes(response_bytes),'bid_v2_workspace',jsonb_build_object('workspace_id',p_workspace_id),
    before_generation,head.artifact_sha256,workspace_head.generation,workspace_head.artifact_sha256
  FROM bid_workspace_heads workspace_head WHERE workspace_head.scope_id=p_workspace_id;
  PERFORM kb_bid_v2_idempotency_complete(p_actor,'bid.v2.requirement-projection.refresh',p_idempotency_key,200,response_bytes);
  RETURN response;
END $$;

CREATE FUNCTION kb_bid_v2_list_workspace_assets(
  p_workspace_id uuid,p_actor kb_actor_identity
) RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE project_id uuid;
BEGIN
  SELECT value.project_id INTO project_id FROM bid_submission_workspaces value WHERE value.id=p_workspace_id;
  IF NOT FOUND THEN RAISE EXCEPTION 'WORKSPACE_NOT_FOUND' USING ERRCODE='P0002'; END IF;
  PERFORM kb_bid_v2_require_project_owner(project_id,p_actor);
  RETURN coalesce((SELECT jsonb_agg(jsonb_build_object(
    'asset_revision_id',a.id,'media_type',a.media_type,'file_name',a.file_name,
    'byte_length',a.byte_length,'width_px',a.width_px,'height_px',a.height_px,'page_count',a.page_count,
    'object_ref',a.object_ref,'content_sha256',a.content_sha256
  ) ORDER BY a.created_at,a.id) FROM bid_workspace_asset_artifacts a
    WHERE a.workspace_id=p_workspace_id AND NOT EXISTS (
      SELECT 1 FROM bid_workspace_asset_retirement_artifacts retirement WHERE retirement.asset_revision_id=a.id)),'[]'::jsonb);
END $$;

CREATE FUNCTION kb_bid_v2_upload_workspace_asset(
  p_workspace_id uuid,p_asset_id uuid,p_staging_id uuid,p_file_name text,
  p_media_type text,p_byte_length bigint,p_width_px integer,p_height_px integer,p_page_count integer,
  p_object_ref kb_object_ref,p_content_sha256 kb_sha256,p_actor kb_actor_identity,
  p_idempotency_key text,p_request_bytes bytea,p_request_sha256 kb_sha256
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE project_id uuid; replay bytea; response jsonb; response_bytes bytea;
BEGIN
  replay:=kb_bid_v2_idempotency_begin(p_actor,'bid.v2.workspace.asset.upload',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  SELECT value.project_id INTO project_id FROM bid_submission_workspaces value WHERE value.id=p_workspace_id FOR SHARE;
  IF NOT FOUND THEN RAISE EXCEPTION 'WORKSPACE_NOT_FOUND' USING ERRCODE='P0002'; END IF;
  PERFORM kb_bid_v2_require_project_owner(project_id,p_actor);
  IF p_byte_length<=0 OR octet_length(p_file_name) NOT BETWEEN 1 AND 1024
     OR (p_media_type IN ('image/png','image/jpeg','image/webp') AND (p_width_px IS NULL OR p_height_px IS NULL OR p_page_count IS NOT NULL))
     OR (p_media_type='application/pdf' AND (p_width_px IS NOT NULL OR p_height_px IS NOT NULL OR p_page_count IS NULL))
     OR (p_media_type NOT IN ('image/png','image/jpeg','image/webp','application/pdf') AND (p_width_px IS NOT NULL OR p_height_px IS NOT NULL OR p_page_count IS NOT NULL)) THEN
    RAISE EXCEPTION 'ASSET_METADATA_INVALID' USING ERRCODE='23514';
  END IF;
  PERFORM kb_object_upload_commit(p_staging_id,p_object_ref,p_content_sha256,p_media_type,p_byte_length,
    'bid_workspace_asset',p_asset_id,'payload',p_actor);
  INSERT INTO bid_workspace_asset_artifacts(
    id,project_id,workspace_id,object_ref,content_sha256,media_type,file_name,byte_length,width_px,height_px,page_count,source,created_by)
  VALUES(p_asset_id,project_id,p_workspace_id,p_object_ref,p_content_sha256,p_media_type,
    p_file_name,p_byte_length,p_width_px,p_height_px,p_page_count,'human_upload',p_actor);
  response:=jsonb_build_object('asset_revision_id',p_asset_id,'media_type',p_media_type,
    'file_name',p_file_name,'byte_length',p_byte_length,'width_px',p_width_px,'height_px',p_height_px,
    'page_count',p_page_count,'object_ref',p_object_ref,'content_sha256',p_content_sha256);
  response_bytes:=convert_to(response::text,'UTF8');
  INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
    entity_kind,entity_locator,after_revision,after_sha256)
  VALUES(gen_random_uuid(),1,'bid.v2.workspace.asset.upload',p_actor,p_idempotency_key,p_request_sha256,
    kb_bid_v2_sha256_bytes(response_bytes),'bid_v2_workspace_asset',jsonb_build_object('workspace_id',p_workspace_id,'asset_id',p_asset_id),1,p_content_sha256);
  PERFORM kb_bid_v2_idempotency_complete(p_actor,'bid.v2.workspace.asset.upload',p_idempotency_key,201,response_bytes);
  RETURN response;
END $$;

CREATE FUNCTION kb_bid_v2_prepare_workspace_attachment(
  p_workspace_id uuid,p_source_asset_revision_id uuid,p_preparation_id uuid,
  p_page_source_asset_ids uuid[],p_page_item_ids uuid[],p_widths_px integer[],p_heights_px integer[],
  p_actor kb_actor_identity,p_idempotency_key text,p_request_bytes bytea,p_request_sha256 kb_sha256
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE source_asset bid_workspace_asset_artifacts%ROWTYPE; replay bytea; next_revision bigint;
  page_assets jsonb; payload jsonb; preparation_sha kb_sha256; response jsonb; response_bytes bytea;
BEGIN
  replay:=kb_bid_v2_idempotency_begin(p_actor,'bid.v2.workspace.attachment.prepare',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  SELECT * INTO STRICT source_asset FROM bid_workspace_asset_artifacts
    WHERE workspace_id=p_workspace_id AND id=p_source_asset_revision_id FOR SHARE;
  PERFORM kb_bid_v2_require_project_owner(source_asset.project_id,p_actor);
  IF source_asset.media_type NOT IN ('image/png','image/jpeg','image/webp')
     OR EXISTS (SELECT 1 FROM bid_workspace_asset_retirement_artifacts WHERE asset_revision_id=source_asset.id)
     OR p_page_source_asset_ids IS DISTINCT FROM ARRAY[source_asset.id]
     OR cardinality(coalesce(p_page_source_asset_ids,ARRAY[]::uuid[]))<>1
     OR cardinality(p_page_source_asset_ids)<>cardinality(p_page_item_ids)
     OR cardinality(p_page_source_asset_ids)<>cardinality(p_widths_px)
     OR cardinality(p_page_source_asset_ids)<>cardinality(p_heights_px)
     OR cardinality(p_page_item_ids)<>(SELECT count(DISTINCT id) FROM unnest(p_page_item_ids) id)
     OR EXISTS (SELECT 1 FROM generate_subscripts(p_page_source_asset_ids,1) position
       WHERE p_widths_px[position]<=0 OR p_heights_px[position]<=0
       OR NOT EXISTS (SELECT 1 FROM bid_workspace_asset_artifacts page
         WHERE page.project_id=source_asset.project_id AND page.workspace_id=p_workspace_id
           AND page.id=p_page_source_asset_ids[position]
           AND page.media_type IN ('image/png','image/jpeg','image/webp')
           AND NOT EXISTS (SELECT 1 FROM bid_workspace_asset_retirement_artifacts retired
             WHERE retired.asset_revision_id=page.id)))
  THEN RAISE EXCEPTION 'ATTACHMENT_FORMAT_UNSUPPORTED' USING ERRCODE='23514'; END IF;
  SELECT coalesce(max(revision),0)+1 INTO next_revision FROM bid_attachment_preparation_revision_artifacts
    WHERE workspace_id=p_workspace_id AND source_asset_revision_id=p_source_asset_revision_id;
  SELECT jsonb_agg(jsonb_build_object('page_asset_id',p_page_item_ids[position],
    'page_number',position,'object_ref',page.object_ref,'sha256',page.content_sha256,'media_type',page.media_type,
    'geometry',jsonb_build_object('width_px',p_widths_px[position],'height_px',p_heights_px[position])) ORDER BY position)
  INTO page_assets FROM generate_subscripts(p_page_source_asset_ids,1) position
  JOIN bid_workspace_asset_artifacts page ON page.id=p_page_source_asset_ids[position];
  payload:=jsonb_build_object('schema_version',1,'attachment_preparation_revision_id',p_preparation_id,
    'project_id',source_asset.project_id,'workspace_id',p_workspace_id,'source_asset_revision_id',source_asset.id,
    'revision',next_revision,'status','ready','page_assets',page_assets);
  preparation_sha:=kb_bid_v2_sha256_bytes(convert_to(payload::text,'UTF8'));
  payload:=payload||jsonb_build_object('preparation_sha256',preparation_sha);
  INSERT INTO bid_attachment_preparation_revision_artifacts(id,project_id,workspace_id,source_asset_revision_id,
    revision,status,page_assets,canonical_payload,preparation_sha256)
  VALUES(p_preparation_id,source_asset.project_id,p_workspace_id,source_asset.id,next_revision,'ready',page_assets,payload,preparation_sha);
  INSERT INTO bid_attachment_preparation_asset_items(id,project_id,workspace_id,attachment_preparation_revision_id,
    ordinal,page_number,object_ref,content_sha256,media_type,geometry)
  SELECT p_page_item_ids[position],source_asset.project_id,p_workspace_id,p_preparation_id,position-1,position,
    page.object_ref,page.content_sha256,page.media_type,
    jsonb_build_object('width_px',p_widths_px[position],'height_px',p_heights_px[position])
  FROM generate_subscripts(p_page_source_asset_ids,1) position
  JOIN bid_workspace_asset_artifacts page ON page.id=p_page_source_asset_ids[position];
  INSERT INTO object_owner_references(object_ref,owner_kind,owner_id,occurrence,created_by)
  SELECT page.object_ref,'bid_attachment_preparation',p_preparation_id,'page:'||position,p_actor
  FROM generate_subscripts(p_page_source_asset_ids,1) position
  JOIN bid_workspace_asset_artifacts page ON page.id=p_page_source_asset_ids[position];
  SET CONSTRAINTS ALL IMMEDIATE;
  response:=jsonb_build_object('attachment_preparation_revision_id',p_preparation_id,'revision',next_revision,
    'status','ready','preparation_sha256',preparation_sha,'source_asset_revision_id',source_asset.id,'page_assets',page_assets);
  response_bytes:=convert_to(response::text,'UTF8');
  PERFORM kb_bid_v2_idempotency_complete(p_actor,'bid.v2.workspace.attachment.prepare',p_idempotency_key,201,response_bytes);
  RETURN response;
END $$;



CREATE FUNCTION kb_bid_v2_retire_workspace_asset(
  p_workspace_id uuid,p_asset_revision_id uuid,p_reason text,p_actor kb_actor_identity,
  p_idempotency_key text,p_request_bytes bytea,p_request_sha256 kb_sha256
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE asset bid_workspace_asset_artifacts%ROWTYPE; head bid_workspace_heads%ROWTYPE;
  replay bytea; response jsonb; response_bytes bytea; retirement_id uuid:=gen_random_uuid();
BEGIN
  replay:=kb_bid_v2_idempotency_begin(p_actor,'bid.v2.workspace.asset.retire',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  SELECT * INTO STRICT asset FROM bid_workspace_asset_artifacts
    WHERE id=p_asset_revision_id AND workspace_id=p_workspace_id FOR SHARE;
  PERFORM kb_bid_v2_require_project_owner(asset.project_id,p_actor);
  SELECT * INTO STRICT head FROM bid_workspace_heads WHERE scope_id=p_workspace_id;
  IF EXISTS (SELECT 1 FROM bid_workspace_block_occurrences occurrence
    JOIN bid_content_block_revision_artifacts block ON block.id=occurrence.block_revision_id
    WHERE occurrence.workspace_revision_id=head.artifact_id
      AND block.block_kind IN ('image','attachment_ref')
      AND convert_from(block.canonical_payload,'UTF8')::jsonb#>>'{content,asset_revision_id}'=p_asset_revision_id::text) THEN
    RAISE EXCEPTION 'ASSET_IN_CURRENT_WORKSPACE' USING ERRCODE='23514';
  END IF;
  INSERT INTO bid_workspace_asset_retirement_artifacts(id,project_id,workspace_id,asset_revision_id,retired_by,reason)
  VALUES(retirement_id,asset.project_id,p_workspace_id,p_asset_revision_id,p_actor,p_reason);
  response:=jsonb_build_object('asset_revision_id',p_asset_revision_id,'status','retired','retirement_id',retirement_id);
  response_bytes:=convert_to(response::text,'UTF8');
  INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
    entity_kind,entity_locator,after_revision,after_sha256)
  VALUES(gen_random_uuid(),1,'bid.v2.workspace.asset.retire',p_actor,p_idempotency_key,p_request_sha256,
    kb_bid_v2_sha256_bytes(response_bytes),'bid_v2_workspace_asset',jsonb_build_object('workspace_id',p_workspace_id,'asset_id',p_asset_revision_id),1,asset.content_sha256);
  PERFORM kb_bid_v2_idempotency_complete(p_actor,'bid.v2.workspace.asset.retire',p_idempotency_key,200,response_bytes);
  RETURN response;
END $$;

CREATE FUNCTION kb_bid_v2_create_outline_checkpoint(
  p_workspace_id uuid,p_expected_revision_id uuid,p_expected_sha256 kb_sha256,p_checkpoint_id uuid,
  p_actor kb_actor_identity,p_idempotency_key text,p_request_bytes bytea,p_request_sha256 kb_sha256
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE workspace bid_submission_workspaces%ROWTYPE; head bid_workspace_heads%ROWTYPE;
  revision bid_workspace_revision_artifacts%ROWTYPE; replay bytea; payload bytea; digest kb_sha256;
  response jsonb; response_bytes bytea;
BEGIN
  replay:=kb_bid_v2_idempotency_begin(p_actor,'bid.v2.outline.checkpoint.create',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  SELECT * INTO STRICT workspace FROM bid_submission_workspaces WHERE id=p_workspace_id;
  PERFORM kb_bid_v2_require_project_owner(workspace.project_id,p_actor);
  SELECT * INTO STRICT head FROM bid_workspace_heads WHERE scope_id=p_workspace_id FOR UPDATE;
  IF head.artifact_id<>p_expected_revision_id OR head.artifact_sha256<>p_expected_sha256 THEN
    RAISE EXCEPTION 'WORKSPACE_CAS_CONFLICT' USING ERRCODE='40001';
  END IF;
  SELECT * INTO STRICT revision FROM bid_workspace_revision_artifacts WHERE id=head.artifact_id;
  payload:=kb_bid_v2_json_payload(jsonb_build_object('schema_version',1,'workspace_id',p_workspace_id,
    'workspace_revision_id',revision.id,'workspace_sha256',revision.content_sha256,
    'requirement_projection_id',revision.requirement_projection_id,
    'requirement_projection_sha256',revision.requirement_projection_sha256));
  digest:=kb_bid_v2_sha256_bytes(payload);
  INSERT INTO bid_outline_checkpoint_artifacts(id,project_id,workspace_id,workspace_revision_id,
    requirement_projection_id,requirement_projection_sha256,canonical_payload,content_sha256,actor)
  VALUES(p_checkpoint_id,workspace.project_id,p_workspace_id,revision.id,revision.requirement_projection_id,
    revision.requirement_projection_sha256,payload,digest,p_actor);
  response:=jsonb_build_object('artifact_id',p_checkpoint_id,'sha256',digest);
  response_bytes:=convert_to(response::text,'UTF8');
  INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
    entity_kind,entity_locator,after_revision,after_sha256)
  VALUES(gen_random_uuid(),1,'bid.v2.outline.checkpoint.create',p_actor,p_idempotency_key,p_request_sha256,
    kb_bid_v2_sha256_bytes(response_bytes),'bid_v2_outline_checkpoint',jsonb_build_object('workspace_id',p_workspace_id,'checkpoint_id',p_checkpoint_id),1,digest);
  PERFORM kb_bid_v2_idempotency_complete(p_actor,'bid.v2.outline.checkpoint.create',p_idempotency_key,201,response_bytes);
  RETURN response;
END $$;

-- Frozen authoring contracts used by the first production ContentGenerate worker.
INSERT INTO bid_authoring_contract_artifacts(id,contract_kind,schema_version,canonical_payload,content_sha256)
SELECT id,kind,1,payload,kb_bid_v2_sha256_bytes(payload) FROM (VALUES
  ('00000000-0000-5000-8000-000000000201'::uuid,'matching_policy',convert_to('{"kind":"workspace_scope_v2","version":1}','UTF8')),
  ('00000000-0000-5000-8000-000000000202'::uuid,'prompt',convert_to('{"kind":"content_prompt","output_schema_sha256":"14187ee75ad1c275e45f830a106273fdad92b9423f3062912ba45c7f94b0fccd","prompt_sha256":"01a838f1e21f00f8d5d4edd6a05b6b00a0bd3900cd7a0a26a49e36986f89975c","version":1}','UTF8')),
  ('00000000-0000-5000-8000-000000000203'::uuid,'template',convert_to('{"kind":"content_template","version":1}','UTF8')),
  ('00000000-0000-5000-8000-000000000204'::uuid,'model',convert_to('{"kind":"configured_chat_model","operation":"content","version":1}','UTF8')),
  ('00000000-0000-5000-8000-000000000205'::uuid,'agent',convert_to('{"kind":"content_agent","output_schema_sha256":"14187ee75ad1c275e45f830a106273fdad92b9423f3062912ba45c7f94b0fccd","prompt_sha256":"01a838f1e21f00f8d5d4edd6a05b6b00a0bd3900cd7a0a26a49e36986f89975c","version":1}','UTF8'))
) seeded(id,kind,payload) ON CONFLICT (id) DO NOTHING;
INSERT INTO bid_render_style_contract_artifacts(id,version,schema_version,canonical_payload,content_sha256)
SELECT '00000000-0000-5000-8000-000000000301'::uuid,1000,1,payload,kb_bid_v2_sha256_bytes(payload)
FROM (VALUES(convert_to('{"kind":"default_bid_style","version":1}','UTF8'))) seeded(payload)
ON CONFLICT (id) DO NOTHING;
INSERT INTO bid_renderer_contract_artifacts(id,format,version,schema_version,canonical_payload,content_sha256,approved_at)
SELECT id,format,1,1,payload,kb_bid_v2_sha256_bytes(payload),'2026-01-01T00:00:00Z'::timestamptz FROM (VALUES
('00000000-0000-5000-8000-000000000302'::uuid,'docx',convert_to('{"kind":"knowledgebrain.bid.v2.docx","version":1}','UTF8')),
('00000000-0000-5000-8000-000000000303'::uuid,'pdf',convert_to('{"kind":"knowledgebrain.bid.v2.pdf","font_sha256":"5d0df56f107605387e0de494b22dfc7fb05d8d79ffd981474e7be11dbe571882","version":1}','UTF8'))
) seeded(id,format,payload) ON CONFLICT (id) DO NOTHING;

CREATE FUNCTION kb_bid_v2_content_runtime_payload(p jsonb) RETURNS bytea
LANGUAGE plpgsql IMMUTABLE STRICT SET search_path=pg_catalog,public AS $$
BEGIN
  IF NOT kb_bid_v2_json_keys_exact(p,ARRAY['base_url','credential_ref','endpoint','max_tokens','model_id','protocol',
    'reasoning_effort','response_mode','schema_version','stream','temperature','timeout_ms','transport_retries']) THEN
    RAISE EXCEPTION 'CONTENT_GENERATION_INPUT_INVALID' USING ERRCODE='23514';
  END IF;
  RETURN convert_to('{"base_url":'||(p->'base_url')::text||',"credential_ref":'||(p->'credential_ref')::text||
    ',"endpoint":'||(p->'endpoint')::text||',"max_tokens":'||(p->'max_tokens')::text||
    ',"model_id":'||(p->'model_id')::text||',"protocol":'||(p->'protocol')::text||
    ',"reasoning_effort":'||(p->'reasoning_effort')::text||',"response_mode":'||(p->'response_mode')::text||
    ',"schema_version":'||(p->'schema_version')::text||',"stream":'||(p->'stream')::text||
    ',"temperature":'||(p->'temperature')::text||',"timeout_ms":'||(p->'timeout_ms')::text||
    ',"transport_retries":'||(p->'transport_retries')::text||'}','UTF8');
END $$;

CREATE FUNCTION kb_bid_v2_create_content_request(
  p_workspace_id uuid,p_expected_revision_id uuid,p_expected_sha256 kb_sha256,
  p_request_operation text,p_target_kind text,p_target_node_lineage_id uuid,p_fill_policy text,
  p_insertion_anchor jsonb,p_evidence_selection_mode text,p_pick_set_artifact_id uuid,
  p_retrieval_identity jsonb,p_retrieval_identity_utf8 bytea,
  p_runtime_contract jsonb,p_prompt_utf8 bytea,p_output_schema_utf8 bytea,
  p_actor kb_actor_identity,p_idempotency_key text,p_request_bytes bytea,p_request_sha256 kb_sha256
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE workspace bid_submission_workspaces%ROWTYPE; head bid_workspace_heads%ROWTYPE;
  revision bid_workspace_revision_artifacts%ROWTYPE; checkpoint bid_outline_checkpoint_artifacts%ROWTYPE;
  settings bid_document_settings_revision_artifacts%ROWTYPE; scope bid_workspace_scope_revision_artifacts%ROWTYPE;
  projection bid_workspace_requirement_projection_artifacts%ROWTYPE;
  target_node bid_outline_node_revision_artifacts%ROWTYPE; pick_set bid_evidence_selection_artifacts%ROWTYPE;
  request_id uuid:=gen_random_uuid(); frozen_payload bytea; frozen_sha kb_sha256;
  job_payload jsonb; job_bytes bytea; job_sha kb_sha256;
  selection_sha kb_sha256; replay bytea; response jsonb; response_bytes bytea; operation_name text;
  matching_sha kb_sha256; prompt_sha kb_sha256; template_sha kb_sha256; model_sha kb_sha256; agent_sha kb_sha256;
  style_sha kb_sha256; quote_id uuid; quote_sha kb_sha256;
  retrieval_payload bytea; retrieval_sha kb_sha256; runtime_payload bytea; runtime_sha kb_sha256;
  prompt_bytes_sha kb_sha256; output_schema_sha kb_sha256;
BEGIN
  IF p_request_operation NOT IN ('match_only','generate') OR p_target_kind NOT IN ('node','subtree','workspace')
     OR p_fill_policy NOT IN ('empty_only','append_candidate','missing_requirements_only')
     OR p_evidence_selection_mode NOT IN ('system_proposed','user_pick_set')
     OR (p_evidence_selection_mode='system_proposed' AND (
       jsonb_typeof(p_retrieval_identity)<>'object'
       OR p_retrieval_identity_utf8 IS NULL
       OR convert_from(p_retrieval_identity_utf8,'UTF8')::jsonb IS DISTINCT FROM p_retrieval_identity
       OR NOT kb_bid_v2_json_keys_exact(p_retrieval_identity,ARRAY[
         'canonical_embedding_revision_utf8','canonical_policy_utf8','canonical_rerank_revision_utf8',
         'contract_version','eligible_scope_sha256','embedding_credential_ref','embedding_revision_sha256',
         'library_version_ids','max_chunk_bytes','max_hits','max_total_bytes','mode','policy_sha256',
         'product_version_ids','rerank_credential_ref','rerank_revision_sha256','schema_version'])
       OR p_retrieval_identity->>'mode'<>'exact'
       OR p_retrieval_identity->>'schema_version'<>'1'
       OR p_retrieval_identity->>'policy_sha256' !~ '^[0-9a-f]{64}$'
       OR p_retrieval_identity->>'eligible_scope_sha256' !~ '^[0-9a-f]{64}$'
       OR jsonb_typeof(p_retrieval_identity->'product_version_ids')<>'array'
       OR jsonb_typeof(p_retrieval_identity->'library_version_ids')<>'array'))
     OR (p_evidence_selection_mode='user_pick_set' AND
       (p_retrieval_identity IS NOT NULL OR p_retrieval_identity_utf8 IS NOT NULL))
     OR (p_request_operation='generate' AND
       (p_runtime_contract IS NULL OR p_prompt_utf8 IS NULL OR p_output_schema_utf8 IS NULL))
     OR (p_request_operation='match_only' AND
       (p_runtime_contract IS NOT NULL OR p_prompt_utf8 IS NOT NULL OR p_output_schema_utf8 IS NOT NULL)) THEN
    RAISE EXCEPTION 'CONTENT_GENERATION_INPUT_INVALID' USING ERRCODE='23514';
  END IF;
  operation_name:=CASE p_request_operation WHEN 'match_only' THEN 'bid.v2.evidence.match' ELSE 'bid.v2.content.generate' END;
  replay:=kb_bid_v2_idempotency_begin(p_actor,operation_name,p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  SELECT * INTO STRICT workspace FROM bid_submission_workspaces WHERE id=p_workspace_id;
  PERFORM kb_bid_v2_require_project_owner(workspace.project_id,p_actor);
  SELECT * INTO STRICT head FROM bid_workspace_heads WHERE scope_id=p_workspace_id FOR SHARE;
  IF head.artifact_id IS DISTINCT FROM p_expected_revision_id OR head.artifact_sha256 IS DISTINCT FROM p_expected_sha256 THEN
    RAISE EXCEPTION 'WORKSPACE_HEAD_CAS_MISMATCH' USING ERRCODE='40001';
  END IF;
  SELECT * INTO STRICT revision FROM bid_workspace_revision_artifacts WHERE id=head.artifact_id;
  SELECT * INTO STRICT projection FROM bid_workspace_requirement_projection_artifacts
    WHERE id=revision.requirement_projection_id AND content_sha256=revision.requirement_projection_sha256;
  SELECT * INTO STRICT checkpoint FROM bid_outline_checkpoint_artifacts
    WHERE workspace_id=p_workspace_id AND workspace_revision_id=revision.id
      AND requirement_projection_id=revision.requirement_projection_id
      AND requirement_projection_sha256=revision.requirement_projection_sha256
    ORDER BY created_at DESC,id DESC LIMIT 1;
  SELECT * INTO STRICT settings FROM bid_document_settings_revision_artifacts
    WHERE id=revision.document_settings_revision_id;
  SELECT * INTO STRICT scope FROM bid_workspace_scope_revision_artifacts
    WHERE id=revision.scope_revision_id;
  quote_id:=revision.quote_snapshot_id;
  quote_sha:=revision.quote_snapshot_sha256;
  IF p_target_kind IN ('node','subtree') THEN
    IF p_target_node_lineage_id IS NULL THEN RAISE EXCEPTION 'CONTENT_TARGET_INVALID' USING ERRCODE='23514'; END IF;
    SELECT node.* INTO STRICT target_node FROM bid_outline_node_revision_artifacts node
      JOIN bid_workspace_node_occurrences occurrence ON occurrence.project_id=node.project_id
        AND occurrence.node_revision_id=node.id AND occurrence.workspace_revision_id=revision.id
      WHERE node.project_id=workspace.project_id AND node.workspace_id=p_workspace_id
        AND node.lineage_id=p_target_node_lineage_id;
  ELSIF p_target_node_lineage_id IS NOT NULL THEN
    RAISE EXCEPTION 'CONTENT_TARGET_INVALID' USING ERRCODE='23514';
  END IF;
  IF p_evidence_selection_mode='user_pick_set' THEN
    SELECT * INTO STRICT pick_set FROM bid_evidence_selection_artifacts
      WHERE id=p_pick_set_artifact_id AND project_id=workspace.project_id AND workspace_id=p_workspace_id
        AND selection_kind='user_pick_set';
    IF p_target_kind IN ('node','subtree') AND NOT EXISTS (SELECT 1 FROM bid_evidence_match_reports report
      WHERE report.id=pick_set.matching_report_id AND report.node_lineage_id=p_target_node_lineage_id) THEN
      RAISE EXCEPTION 'EVIDENCE_PICK_SET_TARGET_MISMATCH' USING ERRCODE='23514';
    END IF;
  ELSIF p_pick_set_artifact_id IS NOT NULL THEN
    RAISE EXCEPTION 'EVIDENCE_SELECTION_INVALID' USING ERRCODE='23514';
  END IF;
  SELECT content_sha256 INTO STRICT matching_sha FROM bid_authoring_contract_artifacts WHERE id='00000000-0000-5000-8000-000000000201';
  SELECT content_sha256 INTO STRICT prompt_sha FROM bid_authoring_contract_artifacts WHERE id='00000000-0000-5000-8000-000000000202';
  SELECT content_sha256 INTO STRICT template_sha FROM bid_authoring_contract_artifacts WHERE id='00000000-0000-5000-8000-000000000203';
  SELECT content_sha256 INTO STRICT model_sha FROM bid_authoring_contract_artifacts WHERE id='00000000-0000-5000-8000-000000000204';
  SELECT content_sha256 INTO STRICT agent_sha FROM bid_authoring_contract_artifacts WHERE id='00000000-0000-5000-8000-000000000205';
  SELECT content_sha256 INTO STRICT style_sha FROM bid_render_style_contract_artifacts WHERE id='00000000-0000-5000-8000-000000000301';
  retrieval_payload:=p_retrieval_identity_utf8;
  retrieval_sha:=kb_bid_v2_sha256_bytes(retrieval_payload);
  IF p_request_operation='generate' THEN
    runtime_payload:=kb_bid_v2_content_runtime_payload(p_runtime_contract);
    runtime_sha:=kb_bid_v2_sha256_bytes(runtime_payload);
    prompt_bytes_sha:=kb_bid_v2_sha256_bytes(p_prompt_utf8);
    output_schema_sha:=kb_bid_v2_sha256_bytes(p_output_schema_utf8);
    IF NOT EXISTS (
      SELECT 1 FROM bid_authoring_contract_artifacts contract
      WHERE contract.id='00000000-0000-5000-8000-000000000202'
        AND convert_from(contract.canonical_payload,'UTF8')::jsonb->>'prompt_sha256'=prompt_bytes_sha
        AND convert_from(contract.canonical_payload,'UTF8')::jsonb->>'output_schema_sha256'=output_schema_sha)
      OR NOT EXISTS (
      SELECT 1 FROM bid_authoring_contract_artifacts contract
      WHERE contract.id='00000000-0000-5000-8000-000000000205'
        AND convert_from(contract.canonical_payload,'UTF8')::jsonb->>'prompt_sha256'=prompt_bytes_sha
        AND convert_from(contract.canonical_payload,'UTF8')::jsonb->>'output_schema_sha256'=output_schema_sha) THEN
      RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH' USING ERRCODE='23514';
    END IF;
  END IF;
  selection_sha:=kb_bid_v2_sha256_bytes(kb_bid_v2_json_payload(jsonb_build_object(
    'mode',p_evidence_selection_mode,'pick_set_artifact_id',p_pick_set_artifact_id,
    'pick_set_sha256',CASE WHEN p_evidence_selection_mode='user_pick_set' THEN pick_set.content_sha256 END,
    'matching_report_id',CASE WHEN p_evidence_selection_mode='user_pick_set' THEN pick_set.matching_report_id END)));
  frozen_payload:=kb_bid_v2_json_payload(jsonb_build_object(
    'workspace_revision_id',revision.id,'workspace_sha256',revision.content_sha256,
    'requirement_projection_id',projection.id,'requirement_projection_sha256',projection.content_sha256,
    'outline_checkpoint_id',checkpoint.id,'outline_checkpoint_sha256',checkpoint.content_sha256,
    'scope_revision_id',revision.scope_revision_id,'scope_revision_sha256',scope.content_sha256,
    'document_settings_revision_id',settings.id,'document_settings_sha256',settings.content_sha256,
    'quote_snapshot_id',quote_id,'quote_snapshot_sha256',quote_sha,
    'render_style_contract_id','00000000-0000-5000-8000-000000000301','render_style_contract_sha256',style_sha,
    'target_kind',p_target_kind,'target_node_lineage_id',p_target_node_lineage_id,
    'fill_policy',p_fill_policy,'insertion_anchor',p_insertion_anchor,'evidence_selection_sha256',selection_sha,
    'retrieval_identity',p_retrieval_identity,
    'retrieval_identity_utf8',CASE WHEN p_retrieval_identity_utf8 IS NULL THEN NULL
      ELSE convert_from(p_retrieval_identity_utf8,'UTF8') END,
    'agent_contract',CASE WHEN p_request_operation='generate' THEN jsonb_build_object(
      'runtime_contract',p_runtime_contract,'runtime_contract_sha256',runtime_sha,
      'prompt_utf8',convert_from(p_prompt_utf8,'UTF8'),'prompt_sha256',prompt_bytes_sha,
      'output_schema_id','urn:knowledgebrain:bid:content-generation-output:v1',
      'output_schema_utf8',convert_from(p_output_schema_utf8,'UTF8'),
      'output_schema_sha256',output_schema_sha) ELSE NULL END));
  frozen_sha:=kb_bid_v2_sha256_bytes(frozen_payload);
  job_payload:=jsonb_build_object('job_kind','content_generate','request',jsonb_build_object(
    'request_artifact_id',request_id,'request_revision',1,'frozen_input_sha256',frozen_sha),
    'project_id',workspace.project_id,'workspace_id',p_workspace_id,
    'base_workspace_revision_id',revision.id,'operation',p_request_operation);
  job_bytes:=kb_bid_v2_json_payload(job_payload); job_sha:=kb_bid_v2_sha256_bytes(job_bytes);
  INSERT INTO bid_async_request_snapshot_artifacts(id,project_id,workspace_id,request_kind,revision,
    frozen_input_sha256,request_payload,request_sha256,status)
  VALUES(request_id,workspace.project_id,p_workspace_id,'content_generate',1,frozen_sha,job_bytes,job_sha,'pending');
  INSERT INTO bid_content_generation_request_identities(
    request_artifact_id,project_id,workspace_id,request_revision,request_sha256,frozen_input_sha256,
    request_operation,base_workspace_revision_id,base_workspace_sha256,
    requirement_projection_id,requirement_projection_sha256,outline_checkpoint_id,outline_checkpoint_sha256,
    scope_revision_id,scope_revision_sha256,document_settings_revision_id,document_settings_sha256,
    render_style_contract_id,render_style_contract_sha256,evidence_selection_mode,evidence_selection_sha256,
    pick_set_kind,pick_set_artifact_id,pick_set_sha256,pick_set_matching_report_id,
    matching_policy_id,matching_policy_sha256,retrieval_identity_payload,retrieval_identity_sha256,
    quote_snapshot_id,quote_snapshot_sha256,
    prompt_contract_id,prompt_contract_sha256,prompt_utf8,prompt_sha256,
    output_schema_id,output_schema_utf8,output_schema_sha256,
    template_contract_id,template_contract_sha256,model_contract_id,model_contract_sha256,
    agent_contract_id,agent_contract_sha256,runtime_contract_payload,runtime_contract_sha256,
    target_kind,target_node_lineage_id,target_node_revision_id,
    target_workspace_revision_id,fill_policy,insertion_node_revision_id,insertion_block_revision_id)
  VALUES(request_id,workspace.project_id,p_workspace_id,1,job_sha,frozen_sha,p_request_operation,
    revision.id,revision.content_sha256,projection.id,projection.content_sha256,checkpoint.id,checkpoint.content_sha256,
    revision.scope_revision_id,scope.content_sha256,settings.id,settings.content_sha256,
    '00000000-0000-5000-8000-000000000301',style_sha,p_evidence_selection_mode,selection_sha,
    CASE WHEN p_evidence_selection_mode='user_pick_set' THEN 'user_pick_set' END,
    CASE WHEN p_evidence_selection_mode='user_pick_set' THEN pick_set.id END,
    CASE WHEN p_evidence_selection_mode='user_pick_set' THEN pick_set.content_sha256 END,
    CASE WHEN p_evidence_selection_mode='user_pick_set' THEN pick_set.matching_report_id END,
    CASE WHEN p_evidence_selection_mode='system_proposed' THEN '00000000-0000-5000-8000-000000000201'::uuid END,
    CASE WHEN p_evidence_selection_mode='system_proposed' THEN matching_sha END,
    retrieval_payload,retrieval_sha,quote_id,quote_sha,
    CASE WHEN p_request_operation='generate' THEN '00000000-0000-5000-8000-000000000202'::uuid END,
    CASE WHEN p_request_operation='generate' THEN prompt_sha END,
    CASE WHEN p_request_operation='generate' THEN p_prompt_utf8 END,
    CASE WHEN p_request_operation='generate' THEN prompt_bytes_sha END,
    CASE WHEN p_request_operation='generate' THEN 'urn:knowledgebrain:bid:content-generation-output:v1' END,
    CASE WHEN p_request_operation='generate' THEN p_output_schema_utf8 END,
    CASE WHEN p_request_operation='generate' THEN output_schema_sha END,
    CASE WHEN p_request_operation='generate' THEN '00000000-0000-5000-8000-000000000203'::uuid END,
    CASE WHEN p_request_operation='generate' THEN template_sha END,
    CASE WHEN p_request_operation='generate' THEN '00000000-0000-5000-8000-000000000204'::uuid END,
    CASE WHEN p_request_operation='generate' THEN model_sha END,
    CASE WHEN p_request_operation='generate' THEN '00000000-0000-5000-8000-000000000205'::uuid END,
    CASE WHEN p_request_operation='generate' THEN agent_sha END,
    runtime_payload,runtime_sha,
    p_target_kind,CASE WHEN p_target_kind IN ('node','subtree') THEN target_node.lineage_id END,
    CASE WHEN p_target_kind IN ('node','subtree') THEN target_node.id END,
    CASE WHEN p_target_kind='workspace' THEN revision.id END,p_fill_policy,
    CASE WHEN p_insertion_anchor IS NOT NULL THEN (p_insertion_anchor->>'node_revision_id')::uuid END,
    CASE WHEN p_insertion_anchor IS NOT NULL AND jsonb_typeof(p_insertion_anchor->'block_revision_id')<>'null' THEN (p_insertion_anchor->>'block_revision_id')::uuid END);
  response:=jsonb_build_object('request_artifact_id',request_id,'kind','ContentGenerate','status','pending',
    'result_identity',NULL,'error_code',NULL,'request_revision',1,'request_sha256',job_sha,
    'frozen_input_sha256',frozen_sha,'project_id',workspace.project_id,
    'workspace_id',p_workspace_id,'base_workspace_revision_id',revision.id,
    'operation',p_request_operation);
  response_bytes:=convert_to(response::text,'UTF8');
  PERFORM kb_bid_v2_idempotency_complete(p_actor,operation_name,p_idempotency_key,202,response_bytes);
  RETURN response;
END $$;

CREATE FUNCTION kb_bid_v2_load_content_generation_input(
  p_request_artifact_id uuid,p_request_revision bigint,p_frozen_input_sha256 kb_sha256
) RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE typed bid_content_generation_request_identities%ROWTYPE;
BEGIN
  SELECT * INTO STRICT typed FROM bid_content_generation_request_identities
    WHERE request_artifact_id=p_request_artifact_id AND request_revision=p_request_revision
      AND frozen_input_sha256=p_frozen_input_sha256;
  RETURN jsonb_build_object(
    'request_artifact_id',typed.request_artifact_id,'project_id',typed.project_id,'workspace_id',typed.workspace_id,
    'operation',typed.request_operation,'base_workspace_revision_id',typed.base_workspace_revision_id,
    'base_workspace_sha256',typed.base_workspace_sha256,'target_kind',typed.target_kind,
    'target_node_lineage_id',typed.target_node_lineage_id,'fill_policy',typed.fill_policy,
    'evidence_selection_mode',typed.evidence_selection_mode,'pick_set_artifact_id',typed.pick_set_artifact_id,
    'generation_dependency_sha256',typed.frozen_input_sha256,
    'retrieval_identity',CASE WHEN typed.retrieval_identity_payload IS NULL THEN NULL
      ELSE convert_from(typed.retrieval_identity_payload,'UTF8')::jsonb END,
    'retrieval_identity_utf8',CASE WHEN typed.retrieval_identity_payload IS NULL THEN NULL
      ELSE convert_from(typed.retrieval_identity_payload,'UTF8') END,
    'retrieval_identity_sha256',typed.retrieval_identity_sha256,
    'agent_contract',CASE WHEN typed.request_operation='generate' THEN jsonb_build_object(
      'runtime_contract',convert_from(typed.runtime_contract_payload,'UTF8')::jsonb,
      'runtime_contract_sha256',typed.runtime_contract_sha256,
      'prompt_contract_id',typed.prompt_contract_id,'prompt_contract_sha256',typed.prompt_contract_sha256,
      'prompt_utf8',convert_from(typed.prompt_utf8,'UTF8'),'prompt_sha256',typed.prompt_sha256,
      'output_schema_id',typed.output_schema_id,
      'output_schema_utf8',convert_from(typed.output_schema_utf8,'UTF8'),
      'output_schema_sha256',typed.output_schema_sha256,
      'template_contract_id',typed.template_contract_id,'template_contract_sha256',typed.template_contract_sha256,
      'model_contract_id',typed.model_contract_id,'model_contract_sha256',typed.model_contract_sha256,
      'agent_contract_id',typed.agent_contract_id,'agent_contract_sha256',typed.agent_contract_sha256)
      ELSE NULL END,
    'quote_snapshot',CASE WHEN typed.quote_snapshot_id IS NULL THEN NULL ELSE (
      SELECT convert_from(quote.canonical_payload,'UTF8')::jsonb||jsonb_build_object(
        'artifact_id',quote.id,'sha256',quote.content_sha256)
      FROM bid_quote_snapshot_artifacts quote WHERE quote.project_id=typed.project_id
        AND quote.id=typed.quote_snapshot_id AND quote.content_sha256=typed.quote_snapshot_sha256) END,
    'insertion_anchor',CASE WHEN typed.insertion_node_revision_id IS NULL THEN NULL ELSE jsonb_build_object(
      'node_revision_id',typed.insertion_node_revision_id,'block_revision_id',typed.insertion_block_revision_id) END,
    'target_nodes',coalesce((WITH RECURSIVE target_occurrences AS (
      SELECT occurrence.* FROM bid_workspace_node_occurrences occurrence
      JOIN bid_outline_node_revision_artifacts node ON node.project_id=occurrence.project_id AND node.id=occurrence.node_revision_id
      WHERE occurrence.project_id=typed.project_id AND occurrence.workspace_revision_id=typed.base_workspace_revision_id
        AND (typed.target_kind='workspace' OR node.lineage_id=typed.target_node_lineage_id)
      UNION ALL
      SELECT child.* FROM bid_workspace_node_occurrences child
      JOIN target_occurrences parent ON parent.id=child.parent_occurrence_id
      WHERE typed.target_kind='subtree' AND child.workspace_revision_id=typed.base_workspace_revision_id
    ) SELECT jsonb_agg(jsonb_build_object(
      'node_lineage_id',node.lineage_id,'node_revision_id',node.id,'title',node.title,
      'block_count',(SELECT count(*) FROM bid_workspace_block_occurrences block
        WHERE block.project_id=typed.project_id AND block.workspace_revision_id=typed.base_workspace_revision_id
          AND block.node_occurrence_id=occurrence.id),
      'blocks',coalesce((SELECT jsonb_agg(jsonb_build_object('block_lineage_id',block.lineage_id,
        'block_revision_id',block.id,'ordinal',block_occurrence.ordinal) ORDER BY block_occurrence.ordinal)
        FROM bid_workspace_block_occurrences block_occurrence
        JOIN bid_content_block_revision_artifacts block ON block.project_id=block_occurrence.project_id
          AND block.id=block_occurrence.block_revision_id
        WHERE block_occurrence.workspace_revision_id=typed.base_workspace_revision_id
          AND block_occurrence.node_occurrence_id=occurrence.id),'[]'::jsonb))
      ORDER BY occurrence.depth,occurrence.ordinal,node.id)
      FROM target_occurrences occurrence
      JOIN bid_outline_node_revision_artifacts node ON node.project_id=occurrence.project_id AND node.id=occurrence.node_revision_id),'[]'::jsonb),
    'requirements',coalesce((SELECT jsonb_agg(jsonb_build_object(
      'requirement_revision_id',requirement.id,'requirement_text',convert_from(requirement.text_utf8,'UTF8'),
      'requirement_identity_sha256',kb_bid_v2_sha256_bytes(requirement.text_utf8),
      'requirement_kind',requirement.requirement_kind,'mandatory',requirement.requiredness='mandatory',
    'effective_applicability',item.effective_applicability)
      ORDER BY item.ordinal,requirement.id)
      FROM bid_workspace_requirement_projection_items item
      JOIN bid_requirement_revision_artifacts requirement ON requirement.project_id=item.project_id AND requirement.id=item.requirement_revision_id
      WHERE item.projection_id=typed.requirement_projection_id
        AND (typed.fill_policy<>'missing_requirements_only' OR NOT EXISTS (
          SELECT 1 FROM bid_workspace_binding_occurrences binding_occurrence
          JOIN bid_outline_fulfillment_binding_revision_artifacts binding ON binding.id=binding_occurrence.binding_revision_id
          JOIN bid_submission_fulfillment_evidence_revision_artifacts evidence ON evidence.binding_revision_id=binding.id
            AND kb_bid_v2_fulfillment_evidence_is_current(typed.base_workspace_revision_id,binding.id)
          WHERE binding_occurrence.workspace_revision_id=typed.base_workspace_revision_id AND binding.state='bound'
            AND binding.need_occurrence_id=ANY(kb_bid_v2_fulfillment_need_ids(requirement.fulfillment_expr))))
        AND (typed.evidence_selection_mode<>'user_pick_set' OR requirement.id=(SELECT report.requirement_revision_id
          FROM bid_evidence_selection_artifacts selection JOIN bid_evidence_match_reports report ON report.id=selection.matching_report_id
          WHERE selection.id=typed.pick_set_artifact_id))),'[]'::jsonb));
END $$;

CREATE FUNCTION kb_bid_v2_load_user_pick_evidence(
  p_request_artifact_id uuid,p_request_revision bigint,p_frozen_input_sha256 kb_sha256
) RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE typed bid_content_generation_request_identities%ROWTYPE; result_value jsonb;
BEGIN
  SELECT * INTO STRICT typed FROM bid_content_generation_request_identities
    WHERE request_artifact_id=p_request_artifact_id AND request_revision=p_request_revision
      AND frozen_input_sha256=p_frozen_input_sha256 AND evidence_selection_mode='user_pick_set';
  SELECT jsonb_build_object('attestation_id',report.knowledge_scope_attestation_id,
      'attestation_sha256',report.knowledge_scope_attestation_sha256,
      'canonical_scope',kb_knowledge_load_matching_attestation_v2(
        report.knowledge_scope_attestation_id,report.knowledge_scope_attestation_sha256),
      'requirement_revision_id',report.requirement_revision_id,'evidence_bundle_id',bundle.id,
      'items',coalesce((SELECT jsonb_agg(item.item_payload ORDER BY item.ordinal)
        FROM bid_evidence_bundle_items item WHERE item.evidence_bundle_id=bundle.id
          AND item.id IN (SELECT jsonb_array_elements_text(
            convert_from(selection.canonical_payload,'UTF8')::jsonb->'selected_evidence_item_ids')::uuid)),'[]'::jsonb))
    INTO result_value
    FROM bid_evidence_selection_artifacts selection
    JOIN bid_evidence_match_reports report ON report.id=selection.matching_report_id
    JOIN bid_evidence_bundle_artifacts bundle ON bundle.matching_report_id=report.id
    WHERE selection.id=typed.pick_set_artifact_id AND selection.content_sha256=typed.pick_set_sha256
      AND selection.matching_report_id=typed.pick_set_matching_report_id AND selection.selection_kind='user_pick_set'
      AND selection.project_id=typed.project_id AND selection.workspace_id=typed.workspace_id
      AND report.requirement_revision_id IN (SELECT requirement_revision_id
        FROM bid_workspace_requirement_projection_items WHERE projection_id=typed.requirement_projection_id)
      AND (SELECT count(*) FROM bid_evidence_bundle_items item WHERE item.evidence_bundle_id=bundle.id
        AND item.id IN (SELECT jsonb_array_elements_text(
          convert_from(selection.canonical_payload,'UTF8')::jsonb->'selected_evidence_item_ids')::uuid))
        =jsonb_array_length(convert_from(selection.canonical_payload,'UTF8')::jsonb->'selected_evidence_item_ids');
  IF result_value IS NULL THEN RAISE EXCEPTION 'FROZEN_USER_PICK_SET_INVALID' USING ERRCODE='23514'; END IF;
  PERFORM kb_knowledge_require_matching_attestation_v2(
    (result_value->>'attestation_id')::uuid,(result_value->>'attestation_sha256')::kb_sha256);
  RETURN result_value;
END $$;

CREATE FUNCTION kb_bid_v2_publish_content_generation(
  p_request_artifact_id uuid,p_request_revision bigint,p_frozen_input_sha256 kb_sha256,
  p_attestation_id uuid,p_attestation_sha256 kb_sha256,p_matches jsonb,
  p_candidate_id uuid,p_candidate_payload bytea,p_candidate_sha256 kb_sha256,p_operations jsonb,
  p_attempt integer,p_execution_owner_token uuid
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE typed bid_content_generation_request_identities%ROWTYPE; request_value bid_async_request_snapshot_artifacts%ROWTYPE;
  staged_input_sha kb_sha256;
  requirement record; requirement_match jsonb; evidence_item jsonb; evidence_items jsonb;
  report_id uuid; report_payload bytea; report_sha kb_sha256; bundle_id uuid; item_id uuid;
  selection_id uuid; selection_json jsonb; selection_payload bytea; selection_sha kb_sha256;
  item_payload jsonb; bundle_bare jsonb; bundle_payload jsonb; bundle_sha kb_sha256; created timestamptz;
  candidate_json jsonb; ordinal_value integer:=0; item_ordinal integer; operation_value jsonb; published_identity jsonb; result_sha kb_sha256;
  terminal_at timestamptz;
BEGIN
  SELECT * INTO STRICT request_value FROM bid_async_request_snapshot_artifacts WHERE id=p_request_artifact_id FOR UPDATE;
  IF request_value.status='succeeded' THEN RETURN request_value.result_identity; END IF;
  IF request_value.status<>'pending' THEN RAISE EXCEPTION 'REQUEST_NOT_PENDING' USING ERRCODE='23514'; END IF;
  SELECT * INTO STRICT typed FROM bid_content_generation_request_identities
    WHERE request_artifact_id=p_request_artifact_id AND request_revision=p_request_revision
      AND frozen_input_sha256=p_frozen_input_sha256;
  IF typed.request_operation='generate' THEN
    IF p_attempt IS NULL OR p_execution_owner_token IS NULL THEN
      RAISE EXCEPTION 'REQUEST_ATTEMPT_SUPERSEDED' USING ERRCODE='40001';
    END IF;
    terminal_at:=kb_bid_v2_content_lock_owner(p_request_artifact_id,p_request_revision,
      p_frozen_input_sha256,p_attempt,p_execution_owner_token);
    SELECT artifact.input_sha256 INTO staged_input_sha FROM bid_content_agent_input_artifacts artifact
      WHERE artifact.request_artifact_id=p_request_artifact_id
        AND artifact.frozen_input_sha256=p_frozen_input_sha256;
    IF staged_input_sha IS NULL OR NOT EXISTS (
      SELECT 1 FROM bid_content_agent_boundary_attempts boundary
      WHERE boundary.request_artifact_id=p_request_artifact_id
        AND boundary.frozen_input_sha256=p_frozen_input_sha256
        AND boundary.request_operation='generate' AND boundary.stage_kind='content_generate'
        AND boundary.batch_ordinal=0 AND boundary.input_sha256=staged_input_sha
        AND boundary.prompt_contract_id=typed.prompt_contract_id
        AND boundary.prompt_contract_sha256=typed.prompt_contract_sha256
        AND boundary.prompt_sha256=typed.prompt_sha256
        AND boundary.schema_contract_id=typed.output_schema_id
        AND boundary.schema_contract_sha256=typed.output_schema_sha256
        AND boundary.agent_contract_id=typed.agent_contract_id
        AND boundary.agent_contract_sha256=typed.agent_contract_sha256
        AND boundary.model_contract_id=typed.model_contract_id
        AND boundary.model_contract_sha256=typed.model_contract_sha256
        AND boundary.runtime_contract_sha256=typed.runtime_contract_sha256) THEN
      RAISE EXCEPTION 'CONTENT_DIVERGENT_AGENT_INPUT_REPLAY' USING ERRCODE='23514';
    END IF;
  ELSE
    IF p_attempt IS NOT NULL OR p_execution_owner_token IS NOT NULL THEN
      RAISE EXCEPTION 'REQUEST_ATTEMPT_SUPERSEDED' USING ERRCODE='40001';
    END IF;
    PERFORM kb_bid_v2_content_match_lock(p_request_artifact_id,p_request_revision,p_frozen_input_sha256);
    terminal_at:=clock_timestamp();
  END IF;
  PERFORM kb_knowledge_require_matching_attestation_v2(p_attestation_id,p_attestation_sha256);
  IF jsonb_typeof(p_matches)<>'array'
     OR EXISTS (SELECT 1 FROM jsonb_array_elements(p_matches) value
       WHERE NOT kb_bid_v2_json_keys_exact(value,ARRAY['requirement_revision_id','evidence_bundle_id','items'])
         OR NOT kb_bid_v2_uuid_text(value->>'requirement_revision_id')
         OR NOT kb_bid_v2_uuid_text(value->>'evidence_bundle_id') OR jsonb_typeof(value->'items')<>'array')
     OR EXISTS (SELECT 1 FROM jsonb_array_elements(p_matches) value GROUP BY value->>'requirement_revision_id' HAVING count(*)<>1)
     OR (typed.evidence_selection_mode='system_proposed' AND EXISTS (SELECT 1 FROM bid_workspace_requirement_projection_items item
       WHERE item.projection_id=typed.requirement_projection_id AND NOT EXISTS (
         SELECT 1 FROM jsonb_array_elements(p_matches) value
         WHERE (value->>'requirement_revision_id')::uuid=item.requirement_revision_id)))
     OR EXISTS (SELECT 1 FROM jsonb_array_elements(p_matches) value WHERE NOT EXISTS (
         SELECT 1 FROM bid_workspace_requirement_projection_items item
         WHERE item.projection_id=typed.requirement_projection_id
           AND item.requirement_revision_id=(value->>'requirement_revision_id')::uuid)) THEN
    RAISE EXCEPTION 'EVIDENCE_ATTESTATION_INVALID' USING ERRCODE='23514';
  END IF;
  FOR requirement IN
    SELECT revision.id FROM bid_workspace_requirement_projection_items item
    JOIN bid_requirement_revision_artifacts revision ON revision.project_id=item.project_id AND revision.id=item.requirement_revision_id
    WHERE item.projection_id=typed.requirement_projection_id
      AND EXISTS (SELECT 1 FROM jsonb_array_elements(p_matches) value
        WHERE (value->>'requirement_revision_id')::uuid=revision.id)
    ORDER BY item.ordinal,revision.id
  LOOP
    SELECT value INTO STRICT requirement_match FROM jsonb_array_elements(p_matches) value
      WHERE (value->>'requirement_revision_id')::uuid=requirement.id;
    report_id:=gen_random_uuid(); bundle_id:=(requirement_match->>'evidence_bundle_id')::uuid; created:=clock_timestamp();
    report_payload:=kb_bid_v2_json_payload(jsonb_build_object('schema_version',2,'request_artifact_id',p_request_artifact_id,
      'requirement_revision_id',requirement.id,'retrieval_contract_version','knowledge-evidence-v2',
      'matches',requirement_match->'items'));
    report_sha:=kb_bid_v2_sha256_bytes(report_payload);
    INSERT INTO bid_evidence_match_reports(id,project_id,workspace_id,requirement_revision_id,node_lineage_id,
      retrieval_contract_version,knowledge_scope_attestation_id,knowledge_scope_attestation_sha256,
      canonical_payload,content_sha256)
    VALUES(report_id,typed.project_id,typed.workspace_id,requirement.id,typed.target_node_lineage_id,
      'knowledge-evidence-v2',p_attestation_id,p_attestation_sha256,report_payload,report_sha);
    evidence_items:='[]'::jsonb;
    IF jsonb_array_length(requirement_match->'items')=0 THEN
      item_id:=kb_bid_v2_deterministic_uuid(p_request_artifact_id::text||':'||requirement.id::text||':no-evidence');
      item_payload:=jsonb_build_object('kind','no_evidence','evidence_item_id',item_id,'reason_code','NO_MATCHING_HIT');
      evidence_items:=jsonb_build_array(item_payload);
    ELSE
      FOR evidence_item IN SELECT value FROM jsonb_array_elements(requirement_match->'items') LOOP
        IF evidence_item->>'kind' NOT IN ('text_quote','image')
           OR NOT kb_bid_v2_uuid_text(evidence_item->>'evidence_item_id') THEN
          RAISE EXCEPTION 'EVIDENCE_ATTESTATION_INVALID' USING ERRCODE='23514';
        END IF;
        item_id:=(evidence_item->>'evidence_item_id')::uuid;
        item_payload:=evidence_item;
        evidence_items:=evidence_items||jsonb_build_array(item_payload);
      END LOOP;
    END IF;
    bundle_bare:=jsonb_build_object('schema_version',1,'evidence_bundle_id',bundle_id,'project_id',typed.project_id,
      'workspace_id',typed.workspace_id,'workspace_scope','project_wide','requirement_revision_id',requirement.id,
      'matching_report_id',report_id,'knowledge_scope_attestation_id',p_attestation_id,
      'knowledge_scope_attestation_sha256',p_attestation_sha256,'items',evidence_items,
      'created_at',to_char(created AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'));
    bundle_sha:=kb_bid_v2_sha256_bytes(convert_to(bundle_bare::text,'UTF8'));
    bundle_payload:=bundle_bare||jsonb_build_object('bundle_sha256',bundle_sha);
    INSERT INTO bid_evidence_bundle_artifacts(id,project_id,workspace_id,requirement_revision_id,matching_report_id,
      canonical_payload,content_sha256,created_at)
    VALUES(bundle_id,typed.project_id,typed.workspace_id,requirement.id,report_id,bundle_payload,bundle_sha,created);
    item_ordinal:=0;
    FOR item_payload IN SELECT value FROM jsonb_array_elements(evidence_items) LOOP
      INSERT INTO bid_evidence_bundle_items(id,project_id,workspace_id,evidence_bundle_id,ordinal,item_kind,
        source_media_revision_id,item_payload,content_sha256)
      VALUES((item_payload->>'evidence_item_id')::uuid,typed.project_id,typed.workspace_id,bundle_id,item_ordinal,
        item_payload->>'kind',CASE WHEN item_payload->>'kind'='image' THEN (item_payload->>'image_artifact_revision_id')::uuid END,
        item_payload,kb_bid_v2_sha256_bytes(convert_to(item_payload::text,'UTF8')));
      IF item_payload->>'kind'='image' THEN
        INSERT INTO bid_evidence_asset_artifacts(id,project_id,workspace_id,evidence_bundle_id,evidence_item_id,
          image_artifact_revision_id,object_ref,content_sha256,media_type,width,height,page_ordinal,bounding_region)
        VALUES((item_payload->>'evidence_item_id')::uuid,typed.project_id,typed.workspace_id,bundle_id,
          (item_payload->>'evidence_item_id')::uuid,(item_payload->>'image_artifact_revision_id')::uuid,
          (item_payload->>'object_ref')::kb_object_ref,(item_payload->>'sha256')::kb_sha256,item_payload->>'media_type',
          (item_payload->>'width')::integer,(item_payload->>'height')::integer,
          CASE WHEN jsonb_typeof(item_payload->'page_ordinal')='null' THEN NULL ELSE (item_payload->>'page_ordinal')::integer END,
          CASE WHEN jsonb_typeof(item_payload->'bounding_region')='null' THEN NULL ELSE item_payload->'bounding_region' END);
        INSERT INTO object_owner_references(object_ref,owner_kind,owner_id,occurrence,created_by)
        VALUES((item_payload->>'object_ref')::kb_object_ref,'bid_evidence_asset',(item_payload->>'evidence_item_id')::uuid,
          'frozen-media','system:content-generate-v2');
        INSERT INTO bid_workspace_asset_artifacts(id,project_id,workspace_id,object_ref,content_sha256,
          media_type,file_name,byte_length,width_px,height_px,source,created_by)
        SELECT (item_payload->>'evidence_item_id')::uuid,typed.project_id,typed.workspace_id,
          registry.object_ref,registry.digest,registry.media_type,item_payload->>'frozen_document_display_name',
          registry.byte_length,(item_payload->>'width')::integer,(item_payload->>'height')::integer,
          'ai_evidence','system:content-generate-v2'
        FROM object_registry registry WHERE registry.object_ref=(item_payload->>'object_ref')::kb_object_ref
          AND registry.digest=(item_payload->>'sha256')::kb_sha256 AND registry.state='available';
        IF NOT FOUND THEN RAISE EXCEPTION 'EVIDENCE_MEDIA_OBJECT_UNAVAILABLE' USING ERRCODE='23514'; END IF;
        INSERT INTO object_owner_references(object_ref,owner_kind,owner_id,occurrence,created_by)
        VALUES((item_payload->>'object_ref')::kb_object_ref,'bid_workspace_asset',(item_payload->>'evidence_item_id')::uuid,
          'evidence-media','system:content-generate-v2');
      END IF;
      item_ordinal:=item_ordinal+1;
    END LOOP;
    IF typed.evidence_selection_mode='system_proposed' THEN
      selection_id:=kb_bid_v2_deterministic_uuid(p_request_artifact_id::text||':'||report_id::text||':system-proposed');
      selection_json:=jsonb_build_object('schema_version',1,'selection_id',selection_id,
        'selection_kind','system_proposed','matching_report_id',report_id,
        'selected_evidence_item_ids',coalesce((SELECT jsonb_agg((item->>'evidence_item_id')::uuid ORDER BY ordinal)
          FROM jsonb_array_elements(evidence_items) WITH ORDINALITY value(item,ordinal)
          WHERE item->>'kind'<>'no_evidence'),'[]'::jsonb));
      selection_payload:=kb_bid_v2_json_payload(selection_json);selection_sha:=kb_bid_v2_sha256_bytes(selection_payload);
      INSERT INTO bid_evidence_selection_artifacts(id,project_id,workspace_id,selection_kind,matching_report_id,
        canonical_payload,content_sha256,actor)
      VALUES(selection_id,typed.project_id,typed.workspace_id,'system_proposed',report_id,
        selection_payload,selection_sha,NULL);
    END IF;
    INSERT INTO bid_content_generation_request_evidence_bundles(request_artifact_id,project_id,workspace_id,ordinal,
      evidence_bundle_id,evidence_bundle_sha256)
    VALUES(p_request_artifact_id,typed.project_id,typed.workspace_id,ordinal_value,bundle_id,bundle_sha);
    ordinal_value:=ordinal_value+1;
  END LOOP;
  result_sha:=kb_bid_v2_sha256_bytes(kb_bid_v2_json_payload(jsonb_build_object('evidence_bundle_count',ordinal_value)));
  INSERT INTO bid_async_stage_receipts(request_artifact_id,stage_kind,frozen_input_sha256,result_identity,result_sha256)
  VALUES(p_request_artifact_id,'evidence_match',p_frozen_input_sha256,
    jsonb_build_object('evidence_bundle_count',ordinal_value),result_sha);
  IF typed.request_operation='generate' THEN
    IF p_candidate_id IS NULL OR p_candidate_payload IS NULL OR p_candidate_sha256 IS NULL
       OR p_candidate_sha256<>kb_bid_v2_sha256_bytes(p_candidate_payload) THEN
      RAISE EXCEPTION 'AGENT_OUTPUT_INVALID' USING ERRCODE='23514';
    END IF;
    candidate_json:=convert_from(p_candidate_payload,'UTF8')::jsonb;
    IF NOT kb_bid_v2_json_keys_exact(candidate_json,ARRAY['schema_version','operations','factual_claims','notices'])
       OR candidate_json->'schema_version' IS DISTINCT FROM '1'::jsonb
       OR jsonb_typeof(candidate_json->'operations') IS DISTINCT FROM 'array'
       OR jsonb_typeof(candidate_json->'factual_claims') IS DISTINCT FROM 'array'
       OR jsonb_typeof(candidate_json->'notices') IS DISTINCT FROM 'array'
       OR candidate_json->'operations' IS DISTINCT FROM p_operations THEN
      RAISE EXCEPTION 'AGENT_OUTPUT_INVALID' USING ERRCODE='23514';
    END IF;
    IF EXISTS (SELECT 1 FROM jsonb_array_elements(candidate_json->'factual_claims') claim
         WHERE NOT kb_bid_v2_json_keys_exact(claim,ARRAY['client_operation_ref','utf8_start','utf8_end','evidence_bundle_id','evidence_item_id'])
           OR NOT kb_bid_v2_uuid_text(claim->>'evidence_bundle_id') OR NOT kb_bid_v2_uuid_text(claim->>'evidence_item_id')
           OR coalesce(claim->>'utf8_start','')!~'^(0|[1-9][0-9]*)$' OR coalesce(claim->>'utf8_end','')!~'^[1-9][0-9]*$'
           OR (claim->>'utf8_end')::bigint<=(claim->>'utf8_start')::bigint
           OR NOT EXISTS (SELECT 1 FROM jsonb_array_elements(candidate_json->'operations') operation
             WHERE operation->>'client_operation_ref'=claim->>'client_operation_ref')
           OR NOT EXISTS (SELECT 1 FROM bid_content_generation_request_evidence_bundles request_bundle
             JOIN bid_evidence_bundle_items item ON item.evidence_bundle_id=request_bundle.evidence_bundle_id
             WHERE request_bundle.request_artifact_id=p_request_artifact_id
               AND request_bundle.evidence_bundle_id=(claim->>'evidence_bundle_id')::uuid
               AND item.id=(claim->>'evidence_item_id')::uuid AND item.item_kind<>'no_evidence'))
       OR EXISTS (SELECT 1 FROM jsonb_path_query(candidate_json->'operations',
           'strict $.** ? (@.kind == "evidence_ref")') reference
         WHERE NOT kb_bid_v2_uuid_text(reference->>'evidence_bundle_id')
           OR NOT kb_bid_v2_uuid_text(reference->>'evidence_item_id')
           OR NOT EXISTS (SELECT 1 FROM bid_content_generation_request_evidence_bundles request_bundle
             JOIN bid_evidence_bundle_items item ON item.evidence_bundle_id=request_bundle.evidence_bundle_id
             WHERE request_bundle.request_artifact_id=p_request_artifact_id
               AND request_bundle.evidence_bundle_id=(reference->>'evidence_bundle_id')::uuid
               AND item.id=(reference->>'evidence_item_id')::uuid AND item.item_kind<>'no_evidence'))
       OR EXISTS (SELECT 1 FROM jsonb_array_elements(candidate_json->'operations') operation
         WHERE operation#>>'{block,kind}'='image' AND (
           NOT kb_bid_v2_uuid_text(operation#>>'{block,content,asset_revision_id}')
           OR NOT EXISTS (SELECT 1 FROM bid_content_generation_request_evidence_bundles request_bundle
             JOIN bid_evidence_bundle_items item ON item.evidence_bundle_id=request_bundle.evidence_bundle_id
             WHERE request_bundle.request_artifact_id=p_request_artifact_id
               AND item.id=(operation#>>'{block,content,asset_revision_id}')::uuid AND item.item_kind='image'))) THEN
      RAISE EXCEPTION 'AGENT_EVIDENCE_REFERENCE_INVALID' USING ERRCODE='23514';
    END IF;
    INSERT INTO bid_candidate_artifacts(id,project_id,workspace_id,candidate_kind,
      base_workspace_revision_id,base_workspace_sha256,request_artifact_id,request_kind,request_revision,
      request_sha256,request_operation,state,canonical_payload,content_sha256)
    VALUES(p_candidate_id,typed.project_id,typed.workspace_id,'content',typed.base_workspace_revision_id,
      typed.base_workspace_sha256,p_request_artifact_id,'content_generate',typed.request_revision,
      typed.request_sha256,'generate','proposed',p_candidate_payload,p_candidate_sha256);
    ordinal_value:=0;
    FOR operation_value IN SELECT value FROM jsonb_array_elements(candidate_json->'operations') LOOP
      INSERT INTO bid_candidate_operations(candidate_id,ordinal,operation,operation_sha256)
      VALUES(p_candidate_id,ordinal_value,operation_value,
        kb_bid_v2_sha256_bytes(convert_to(operation_value::text,'UTF8')));
      ordinal_value:=ordinal_value+1;
    END LOOP;
    INSERT INTO bid_async_stage_receipts(request_artifact_id,stage_kind,frozen_input_sha256,result_identity,result_sha256)
    VALUES(p_request_artifact_id,'agent_generate',p_frozen_input_sha256,
      jsonb_build_object('artifact_id',p_candidate_id,'sha256',p_candidate_sha256),p_candidate_sha256);
    published_identity:=jsonb_build_object('artifact_id',p_candidate_id,'sha256',p_candidate_sha256);
  ELSE
    published_identity:=jsonb_build_object('artifact_id',p_request_artifact_id,'sha256',result_sha);
  END IF;
  IF typed.request_operation='generate' THEN
    UPDATE bid_content_agent_run_artifacts SET status='succeeded',progress_phase='succeeded',
      progress_detail=jsonb_build_object('phase','succeeded','candidate_id',p_candidate_id,
        'candidate_sha256',p_candidate_sha256),progress_sequence=progress_sequence+1,
      lease_expires_at=terminal_at,heartbeat_at=terminal_at,updated_at=terminal_at,
      last_error_code=NULL,last_error_message=NULL,last_error_at=NULL
    WHERE request_artifact_id=p_request_artifact_id AND attempt=p_attempt;
  END IF;
  UPDATE bid_async_request_snapshot_artifacts SET status='succeeded',result_identity=published_identity,
    finished_at=terminal_at,error_code=NULL WHERE id=p_request_artifact_id AND status='pending';
  IF NOT FOUND THEN RAISE EXCEPTION 'content request terminal transition failed' USING ERRCODE='40001'; END IF;
  RETURN published_identity;
END $$;

CREATE FUNCTION kb_bid_v2_mark_content_generation_failed(
  p_request_artifact_id uuid,p_request_revision bigint,p_frozen_input_sha256 kb_sha256,
  p_error_code text,p_error_message text,p_attempt integer,p_execution_owner_token uuid
) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE request_value bid_async_request_snapshot_artifacts%ROWTYPE;
  typed bid_content_generation_request_identities%ROWTYPE; terminal_at timestamptz;
BEGIN
  IF p_error_code NOT IN ('INPUT_SCHEMA_INVALID','FROZEN_INPUT_MISSING','FROZEN_INPUT_DIGEST_MISMATCH',
      'WORKSPACE_CAS_CONFLICT','AGENT_OUTPUT_INVALID','AGENT_TURN_TIMEOUT','AGENT_PROVIDER_UNAVAILABLE',
      'AGENT_DEADLINE_EXCEEDED','AGENT_TURN_BUDGET_EXCEEDED','REQUEST_ATTEMPT_BUDGET_EXCEEDED',
      'CONTENT_RETRIEVAL_INVALID_REQUEST','CONTENT_RETRIEVAL_UNAVAILABLE','CONTENT_RETRIEVAL_QUOTA_EXCEEDED',
      'CONTENT_RETRIEVAL_INVALID_HIT','CONTENT_RETRIEVAL_POLICY_REVOKED','CONTENT_RETRIEVAL_DIGEST_MISMATCH',
      'CONTENT_DIVERGENT_AGENT_INPUT_REPLAY','CONTENT_MATCH_TIMEOUT') THEN
    RAISE EXCEPTION 'unknown ContentGenerate terminal error code' USING ERRCODE='22023';
  END IF;
  SELECT * INTO request_value FROM bid_async_request_snapshot_artifacts WHERE id=p_request_artifact_id FOR UPDATE;
  IF request_value.id IS NULL OR request_value.revision<>p_request_revision
     OR request_value.frozen_input_sha256<>p_frozen_input_sha256 THEN
    RAISE EXCEPTION 'FROZEN_INPUT_MISSING' USING ERRCODE='P0002';
  END IF;
  IF request_value.status<>'pending' THEN RETURN; END IF;
  SELECT * INTO STRICT typed FROM bid_content_generation_request_identities
    WHERE request_artifact_id=p_request_artifact_id AND request_revision=p_request_revision
      AND frozen_input_sha256=p_frozen_input_sha256;
  IF typed.request_operation='generate' THEN
    IF p_attempt IS NULL OR p_execution_owner_token IS NULL THEN
      RAISE EXCEPTION 'REQUEST_ATTEMPT_SUPERSEDED' USING ERRCODE='40001';
    END IF;
    terminal_at:=kb_bid_v2_content_lock_owner(p_request_artifact_id,p_request_revision,
      p_frozen_input_sha256,p_attempt,p_execution_owner_token);
    UPDATE bid_content_agent_run_artifacts SET status='failed',progress_phase='failed',
      progress_detail=jsonb_build_object('phase','failed','error_code',p_error_code),
      progress_sequence=progress_sequence+1,last_error_code=p_error_code,
      last_error_message=kb_bid_v2_diagnostic_prefix(coalesce(p_error_message,'')),last_error_at=terminal_at,
      lease_expires_at=terminal_at,heartbeat_at=terminal_at,updated_at=terminal_at
    WHERE request_artifact_id=p_request_artifact_id AND attempt=p_attempt;
  ELSE
    IF p_attempt IS NOT NULL OR p_execution_owner_token IS NOT NULL THEN
      RAISE EXCEPTION 'REQUEST_ATTEMPT_SUPERSEDED' USING ERRCODE='40001';
    END IF;
    PERFORM kb_bid_v2_content_match_lock(p_request_artifact_id,p_request_revision,p_frozen_input_sha256);
    terminal_at:=clock_timestamp();
  END IF;
  UPDATE bid_async_request_snapshot_artifacts SET status='failed',error_code=p_error_code,
    finished_at=terminal_at WHERE id=p_request_artifact_id AND status='pending';
  IF NOT FOUND THEN RAISE EXCEPTION 'content request terminal transition failed' USING ERRCODE='40001'; END IF;
END $$;

CREATE FUNCTION kb_bid_v2_create_evidence_pick_set(
  p_workspace_id uuid,p_matching_report_id uuid,p_selected_item_ids uuid[],p_actor kb_actor_identity,
  p_idempotency_key text,p_request_bytes bytea,p_request_sha256 kb_sha256
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE report bid_evidence_match_reports%ROWTYPE; selection_id uuid:=gen_random_uuid();
  replay bytea; payload_json jsonb; payload bytea; digest kb_sha256; response jsonb; response_bytes bytea;
BEGIN
  replay:=kb_bid_v2_idempotency_begin(p_actor,'bid.v2.evidence-pick-set.create',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  SELECT * INTO STRICT report FROM bid_evidence_match_reports
    WHERE id=p_matching_report_id AND workspace_id=p_workspace_id;
  PERFORM kb_bid_v2_require_project_owner(report.project_id,p_actor);
  IF cardinality(coalesce(p_selected_item_ids,ARRAY[]::uuid[]))=0
     OR cardinality(p_selected_item_ids)<>(SELECT count(DISTINCT id) FROM unnest(p_selected_item_ids) id)
     OR NOT EXISTS (SELECT 1 FROM bid_evidence_bundle_artifacts bundle
       WHERE bundle.matching_report_id=p_matching_report_id
       AND (SELECT count(*) FROM bid_evidence_bundle_items item
         WHERE item.evidence_bundle_id=bundle.id AND item.item_kind<>'no_evidence'
           AND item.id=ANY(p_selected_item_ids))=cardinality(p_selected_item_ids)) THEN
    RAISE EXCEPTION 'EVIDENCE_PICK_SET_INVALID' USING ERRCODE='23514';
  END IF;
  payload_json:=jsonb_build_object('schema_version',1,'selection_id',selection_id,
    'selection_kind','user_pick_set','matching_report_id',p_matching_report_id,
    'selected_evidence_item_ids',to_jsonb(p_selected_item_ids));
  payload:=kb_bid_v2_json_payload(payload_json);digest:=kb_bid_v2_sha256_bytes(payload);
  INSERT INTO bid_evidence_selection_artifacts(id,project_id,workspace_id,selection_kind,matching_report_id,
    canonical_payload,content_sha256,actor)
  VALUES(selection_id,report.project_id,p_workspace_id,'user_pick_set',p_matching_report_id,payload,digest,p_actor);
  response:=payload_json||jsonb_build_object('sha256',digest);response_bytes:=convert_to(response::text,'UTF8');
  INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
    entity_kind,entity_locator,after_revision,after_sha256)
  VALUES(gen_random_uuid(),1,'bid.v2.evidence-pick-set.create',p_actor,p_idempotency_key,p_request_sha256,
    kb_bid_v2_sha256_bytes(response_bytes),'evidence_pick_set',jsonb_build_object('workspace_id',p_workspace_id,'selection_id',selection_id),1,digest);
  PERFORM kb_bid_v2_idempotency_complete(p_actor,'bid.v2.evidence-pick-set.create',p_idempotency_key,201,response_bytes);
  RETURN response;
END $$;

CREATE FUNCTION kb_bid_v2_list_evidence_pick_sets(p_workspace_id uuid,p_actor kb_actor_identity)
RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE project_id uuid;
BEGIN
  SELECT workspace.project_id INTO STRICT project_id FROM bid_submission_workspaces workspace WHERE workspace.id=p_workspace_id;
  PERFORM kb_bid_v2_require_project_owner(project_id,p_actor);
  RETURN coalesce((SELECT jsonb_agg(convert_from(selection.canonical_payload,'UTF8')::jsonb
    ||jsonb_build_object('sha256',selection.content_sha256) ORDER BY selection.created_at,selection.id)
    FROM bid_evidence_selection_artifacts selection WHERE selection.workspace_id=p_workspace_id
      AND selection.selection_kind='user_pick_set'),'[]'::jsonb);
END $$;

CREATE FUNCTION kb_bid_v2_create_node_evidence_pick_set(
  p_workspace_id uuid,p_node_lineage_id uuid,p_matching_report_id uuid,p_selected_item_ids uuid[],
  p_actor kb_actor_identity,p_idempotency_key text,p_request_bytes bytea,p_request_sha256 kb_sha256
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
BEGIN
  PERFORM 1 FROM bid_evidence_match_reports report
  WHERE report.id=p_matching_report_id AND report.workspace_id=p_workspace_id
    AND report.node_lineage_id=p_node_lineage_id;
  IF NOT FOUND THEN RAISE EXCEPTION 'NODE_EVIDENCE_REPORT_NOT_FOUND' USING ERRCODE='P0002'; END IF;
  RETURN kb_bid_v2_create_evidence_pick_set(p_workspace_id,p_matching_report_id,p_selected_item_ids,
    p_actor,p_idempotency_key,p_request_bytes,p_request_sha256);
END $$;

CREATE FUNCTION kb_bid_v2_get_node_evidence(
  p_workspace_id uuid,p_node_lineage_id uuid,p_actor kb_actor_identity
) RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE project_id uuid;
BEGIN
  SELECT workspace.project_id INTO STRICT project_id FROM bid_submission_workspaces workspace WHERE workspace.id=p_workspace_id;
  PERFORM kb_bid_v2_require_project_owner(project_id,p_actor);
  IF NOT EXISTS (SELECT 1 FROM bid_outline_node_lineages lineage
    WHERE lineage.id=p_node_lineage_id AND lineage.workspace_id=p_workspace_id) THEN
    RAISE EXCEPTION 'NODE_NOT_FOUND' USING ERRCODE='P0002';
  END IF;
  RETURN jsonb_build_object('node_lineage_id',p_node_lineage_id,
    'bundles',coalesce((SELECT jsonb_agg(bundle.canonical_payload
      ORDER BY bundle.created_at,bundle.id) FROM bid_evidence_bundle_artifacts bundle
      JOIN bid_evidence_match_reports report ON report.id=bundle.matching_report_id
      WHERE bundle.workspace_id=p_workspace_id AND report.node_lineage_id=p_node_lineage_id),'[]'::jsonb),
    'pick_sets',coalesce((SELECT jsonb_agg(convert_from(selection.canonical_payload,'UTF8')::jsonb
      ORDER BY selection.created_at,selection.id) FROM bid_evidence_selection_artifacts selection
      JOIN bid_evidence_match_reports report ON report.id=selection.matching_report_id
      WHERE selection.workspace_id=p_workspace_id AND selection.selection_kind='user_pick_set'
        AND report.node_lineage_id=p_node_lineage_id),'[]'::jsonb));
END $$;

CREATE FUNCTION kb_bid_v2_get_evidence_overview(p_workspace_id uuid,p_actor kb_actor_identity)
RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE project_id uuid;
BEGIN
  SELECT workspace.project_id INTO STRICT project_id FROM bid_submission_workspaces workspace WHERE workspace.id=p_workspace_id;
  PERFORM kb_bid_v2_require_project_owner(project_id,p_actor);
  RETURN jsonb_build_object(
    'node_lineage_id',NULL,
    'covered_requirement_ids',coalesce((SELECT jsonb_agg(DISTINCT bundle.requirement_revision_id)
      FROM bid_evidence_bundle_artifacts bundle WHERE bundle.workspace_id=p_workspace_id
        AND EXISTS (SELECT 1 FROM bid_evidence_bundle_items item
          WHERE item.evidence_bundle_id=bundle.id AND item.item_kind<>'no_evidence')),'[]'::jsonb),
    'missing_requirement_ids',coalesce((SELECT jsonb_agg(DISTINCT bundle.requirement_revision_id)
      FROM bid_evidence_bundle_artifacts bundle WHERE bundle.workspace_id=p_workspace_id
        AND NOT EXISTS (SELECT 1 FROM bid_evidence_bundle_items item
          WHERE item.evidence_bundle_id=bundle.id AND item.item_kind<>'no_evidence')),'[]'::jsonb),
    'bundles',coalesce((SELECT jsonb_agg(jsonb_build_object(
      'evidence_bundle_id',bundle.id,'title',CASE WHEN EXISTS (SELECT 1 FROM bid_evidence_bundle_items item
        WHERE item.evidence_bundle_id=bundle.id AND item.item_kind<>'no_evidence') THEN '证据包' ELSE '未找到可用证据' END,
      'requirement_revision_id',bundle.requirement_revision_id,'matching_report_id',bundle.matching_report_id,
      'sha256',bundle.content_sha256,'items',bundle.canonical_payload->'items')
      ORDER BY bundle.created_at,bundle.id) FROM bid_evidence_bundle_artifacts bundle
      WHERE bundle.workspace_id=p_workspace_id),'[]'::jsonb));
END $$;

CREATE FUNCTION kb_bid_v2_quote_snapshot_payload_valid(
  p_payload jsonb,p_project_id uuid,p_quote_id uuid,p_revision bigint,p_actor kb_actor_identity
) RETURNS boolean LANGUAGE plpgsql IMMUTABLE PARALLEL SAFE SET search_path=pg_catalog,public AS $$
DECLARE line jsonb; ordinal bigint:=0; basis numeric; net numeric; tax numeric; gross numeric;
  quantity numeric; unit_price numeric; tax_rate numeric; net_total numeric:=0; tax_total numeric:=0; gross_total numeric:=0;
BEGIN
  IF NOT kb_bid_v2_json_keys_exact(p_payload,ARRAY['schema_version','quote_id','project_id','revision','currency_code','currency_scale',
      'tax_mode','title','notes','lines','net_total','tax_total','gross_total','ceiling','no_ceiling_review','fact_revision','pricing_revision','pricing_set_sha256'])
    OR p_payload->>'schema_version'<>'1' OR p_payload->>'quote_id'<>p_quote_id::text
    OR p_payload->>'project_id'<>p_project_id::text OR p_payload->>'revision'<>p_revision::text
    OR p_payload->>'currency_code'<>'CNY' OR p_payload->>'currency_scale'<>'2'
    OR p_payload->>'tax_mode' NOT IN ('tax_inclusive','tax_exclusive')
    OR jsonb_typeof(p_payload->'title')<>'string' OR octet_length(btrim(p_payload->>'title')) NOT BETWEEN 1 AND 256
    OR (jsonb_typeof(p_payload->'notes') NOT IN ('string','null'))
    OR (jsonb_typeof(p_payload->'notes')='string' AND octet_length(p_payload->>'notes')>4096)
    OR jsonb_typeof(p_payload->'lines')<>'array' OR jsonb_array_length(p_payload->'lines') NOT BETWEEN 1 AND 10000
    OR jsonb_typeof(p_payload->'ceiling')<>'null' OR jsonb_typeof(p_payload->'fact_revision')<>'null'
    OR jsonb_typeof(p_payload->'pricing_revision')<>'null' OR jsonb_typeof(p_payload->'pricing_set_sha256')<>'null'
    OR NOT kb_bid_v2_json_keys_exact(p_payload->'no_ceiling_review',ARRAY['reviewed','reason','actor_kind','actor_id','at'])
    OR p_payload#>>'{no_ceiling_review,reviewed}'<>'true' OR p_payload#>>'{no_ceiling_review,actor_kind}'<>'user'
    OR p_payload#>>'{no_ceiling_review,actor_id}'<>substr(p_actor,6)
    OR octet_length(btrim(p_payload#>>'{no_ceiling_review,reason}')) NOT BETWEEN 1 AND 1024
    OR coalesce(p_payload#>>'{no_ceiling_review,at}','') !~ '^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}\.[0-9]{6}Z$'
  THEN RETURN false; END IF;
  FOR line IN SELECT value FROM jsonb_array_elements(p_payload->'lines') value LOOP
    IF NOT kb_bid_v2_json_keys_exact(line,ARRAY['id','ordinal','description','pricing_mode','quantity','unit','unit_price','entered_amount',
        'tax_rate','basis_amount','net_amount','tax_amount','gross_amount','user_confirmed'])
      OR NOT kb_bid_v2_uuid_text(line->>'id') OR line->>'ordinal'<>ordinal::text
      OR jsonb_typeof(line->'description')<>'string' OR octet_length(btrim(line->>'description')) NOT BETWEEN 1 AND 4096
      OR line->>'pricing_mode' NOT IN ('unit_price','lump_sum') OR line->>'user_confirmed'<>'true'
      OR coalesce(line->>'tax_rate','') !~ '^(0|1)\.[0-9]{6}$'
      OR coalesce(line->>'basis_amount','') !~ '^(0|[1-9][0-9]{0,17})\.[0-9]{2}$'
      OR coalesce(line->>'net_amount','') !~ '^(0|[1-9][0-9]{0,17})\.[0-9]{2}$'
      OR coalesce(line->>'tax_amount','') !~ '^(0|[1-9][0-9]{0,17})\.[0-9]{2}$'
      OR coalesce(line->>'gross_amount','') !~ '^(0|[1-9][0-9]{0,17})\.[0-9]{2}$'
    THEN RETURN false; END IF;
    tax_rate:=(line->>'tax_rate')::numeric;
    IF line->>'pricing_mode'='unit_price' THEN
      IF jsonb_typeof(line->'quantity')<>'string' OR coalesce(line->>'quantity','') !~ '^(0|[1-9][0-9]{0,8})\.[0-9]{6}$'
        OR (line->>'quantity')::numeric<=0 OR jsonb_typeof(line->'unit')<>'string'
        OR octet_length(btrim(line->>'unit')) NOT BETWEEN 1 AND 64
        OR jsonb_typeof(line->'unit_price')<>'string' OR coalesce(line->>'unit_price','') !~ '^(0|[1-9][0-9]{0,11})\.[0-9]{6}$'
        OR jsonb_typeof(line->'entered_amount')<>'null' THEN RETURN false; END IF;
      quantity:=(line->>'quantity')::numeric;unit_price:=(line->>'unit_price')::numeric;
      basis:=round(quantity*unit_price,2);
    ELSE
      IF jsonb_typeof(line->'quantity')<>'null' OR jsonb_typeof(line->'unit')<>'null'
        OR jsonb_typeof(line->'unit_price')<>'null' OR jsonb_typeof(line->'entered_amount')<>'string'
        OR coalesce(line->>'entered_amount','') !~ '^(0|[1-9][0-9]{0,17})\.[0-9]{2}$' THEN RETURN false; END IF;
      basis:=(line->>'entered_amount')::numeric;
    END IF;
    IF p_payload->>'tax_mode'='tax_exclusive' THEN net:=basis;tax:=round(net*tax_rate,2);gross:=net+tax;
    ELSE gross:=basis;net:=round(gross/(1+tax_rate),2);tax:=gross-net; END IF;
    IF basis<>(line->>'basis_amount')::numeric OR net<>(line->>'net_amount')::numeric
      OR tax<>(line->>'tax_amount')::numeric OR gross<>(line->>'gross_amount')::numeric THEN RETURN false; END IF;
    net_total:=net_total+net;tax_total:=tax_total+tax;gross_total:=gross_total+gross;ordinal:=ordinal+1;
  END LOOP;
  RETURN coalesce(p_payload->>'net_total','')~'^(0|[1-9][0-9]{0,17})\.[0-9]{2}$'
    AND coalesce(p_payload->>'tax_total','')~'^(0|[1-9][0-9]{0,17})\.[0-9]{2}$'
    AND coalesce(p_payload->>'gross_total','')~'^(0|[1-9][0-9]{0,17})\.[0-9]{2}$'
    AND net_total=(p_payload->>'net_total')::numeric AND tax_total=(p_payload->>'tax_total')::numeric
    AND gross_total=(p_payload->>'gross_total')::numeric;
EXCEPTION WHEN OTHERS THEN RETURN false;
END $$;

CREATE FUNCTION kb_bid_v2_next_quote_snapshot_revision(p_project_id uuid,p_actor kb_actor_identity)
RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
BEGIN
  PERFORM kb_bid_v2_require_project_owner(p_project_id,p_actor);
  RETURN jsonb_build_object('quote_id',kb_bid_v2_deterministic_uuid('quote:'||p_project_id::text),
    'next_revision',coalesce((SELECT max(revision)+1 FROM bid_quote_snapshot_artifacts WHERE project_id=p_project_id),1));
END $$;

CREATE FUNCTION kb_bid_v2_advance_workspace_quote(
  p_project_id uuid,p_quote_snapshot_id uuid,p_quote_snapshot_sha256 kb_sha256,
  p_expected_workspace_revision_id uuid,p_expected_workspace_sha256 kb_sha256,p_actor kb_actor_identity
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE workspace bid_submission_workspaces%ROWTYPE; head bid_workspace_heads%ROWTYPE;
  current_quote bid_quote_snapshot_current%ROWTYPE;
  old_revision bid_workspace_revision_artifacts%ROWTYPE; new_revision_id uuid:=gen_random_uuid();
  new_revision bigint; payload bytea; digest kb_sha256; node record; block record; binding record;
  edge record; evidence record; checkpoint record; node_map jsonb:='{}'::jsonb; new_occurrence_id uuid;
  new_parent_id uuid; evidence_revision bigint; evidence_payload bytea; evidence_sha kb_sha256;
  checkpoint_payload bytea; checkpoint_sha kb_sha256;
BEGIN
  SELECT * INTO STRICT workspace FROM bid_submission_workspaces WHERE project_id=p_project_id;
  PERFORM kb_bid_v2_require_project_owner(p_project_id,p_actor);
  SELECT * INTO STRICT head FROM bid_workspace_heads WHERE scope_id=workspace.id FOR UPDATE;
  IF head.artifact_id IS DISTINCT FROM p_expected_workspace_revision_id
     OR head.artifact_sha256 IS DISTINCT FROM p_expected_workspace_sha256 THEN
    RAISE EXCEPTION 'WORKSPACE_HEAD_CAS_MISMATCH' USING ERRCODE='40001';
  END IF;
  SELECT * INTO STRICT old_revision FROM bid_workspace_revision_artifacts
    WHERE id=head.artifact_id AND content_sha256=head.artifact_sha256;
  SELECT * INTO STRICT current_quote FROM bid_quote_snapshot_current
    WHERE scope_id=p_project_id FOR SHARE;
  IF current_quote.artifact_id IS DISTINCT FROM p_quote_snapshot_id
     OR current_quote.artifact_sha256 IS DISTINCT FROM p_quote_snapshot_sha256 THEN
    RAISE EXCEPTION 'QUOTE_SNAPSHOT_NOT_CURRENT' USING ERRCODE='40001';
  END IF;
  PERFORM 1 FROM bid_quote_snapshot_artifacts
    WHERE project_id=p_project_id AND id=p_quote_snapshot_id AND content_sha256=p_quote_snapshot_sha256;
  IF NOT FOUND THEN RAISE EXCEPTION 'QUOTE_SNAPSHOT_IDENTITY_INVALID' USING ERRCODE='23514'; END IF;
  IF old_revision.quote_snapshot_id IS NOT DISTINCT FROM p_quote_snapshot_id
     AND old_revision.quote_snapshot_sha256 IS NOT DISTINCT FROM p_quote_snapshot_sha256 THEN
    RETURN jsonb_build_object('revision_id',old_revision.id,'sha256',old_revision.content_sha256,'replayed',true);
  END IF;
  SELECT coalesce(max(revision),0)+1 INTO new_revision FROM bid_workspace_revision_artifacts
    WHERE workspace_id=workspace.id;
  payload:=kb_bid_v2_json_payload(jsonb_build_object('schema_version',1,'reason','quote_snapshot_advanced',
    'parent_revision_id',old_revision.id,'parent_sha256',old_revision.content_sha256,
    'scope_revision_id',old_revision.scope_revision_id,
    'requirement_projection_id',old_revision.requirement_projection_id,
    'requirement_projection_sha256',old_revision.requirement_projection_sha256,
    'document_settings_revision_id',old_revision.document_settings_revision_id,
    'quote_snapshot_id',p_quote_snapshot_id,'quote_snapshot_sha256',p_quote_snapshot_sha256,
    'node_revision_ids',coalesce((SELECT jsonb_agg(node_revision_id ORDER BY depth,ordinal,id)
      FROM bid_workspace_node_occurrences WHERE workspace_revision_id=old_revision.id),'[]'::jsonb),
    'block_revision_ids',coalesce((SELECT jsonb_agg(block_revision_id ORDER BY ordinal,id)
      FROM bid_workspace_block_occurrences WHERE workspace_revision_id=old_revision.id),'[]'::jsonb),
    'binding_revision_ids',coalesce((SELECT jsonb_agg(binding_revision_id ORDER BY ordinal,id)
      FROM bid_workspace_binding_occurrences WHERE workspace_revision_id=old_revision.id),'[]'::jsonb)));
  digest:=kb_bid_v2_sha256_bytes(payload);
  INSERT INTO bid_workspace_revision_artifacts(id,project_id,workspace_id,revision,parent_revision_id,parent_sha256,
    scope_revision_id,requirement_projection_id,requirement_projection_sha256,document_settings_revision_id,
    quote_snapshot_id,quote_snapshot_sha256,canonical_payload,content_sha256,actor)
  VALUES(new_revision_id,p_project_id,workspace.id,new_revision,old_revision.id,old_revision.content_sha256,
    old_revision.scope_revision_id,old_revision.requirement_projection_id,old_revision.requirement_projection_sha256,
    old_revision.document_settings_revision_id,p_quote_snapshot_id,p_quote_snapshot_sha256,payload,digest,p_actor);
  FOR node IN SELECT * FROM bid_workspace_node_occurrences WHERE workspace_revision_id=old_revision.id
    ORDER BY depth,ordinal,id
  LOOP
    new_occurrence_id:=gen_random_uuid();
    new_parent_id:=CASE WHEN node.parent_occurrence_id IS NULL THEN NULL
      ELSE (node_map->>node.parent_occurrence_id::text)::uuid END;
    INSERT INTO bid_workspace_node_occurrences(id,project_id,workspace_revision_id,node_revision_id,
      parent_occurrence_id,ordinal,depth)
    VALUES(new_occurrence_id,p_project_id,new_revision_id,node.node_revision_id,new_parent_id,node.ordinal,node.depth);
    node_map:=node_map||jsonb_build_object(node.id::text,new_occurrence_id);
  END LOOP;
  FOR block IN SELECT * FROM bid_workspace_block_occurrences WHERE workspace_revision_id=old_revision.id
    ORDER BY ordinal,id
  LOOP
    INSERT INTO bid_workspace_block_occurrences(id,project_id,workspace_revision_id,node_occurrence_id,
      block_revision_id,ordinal)
    VALUES(gen_random_uuid(),p_project_id,new_revision_id,
      (node_map->>block.node_occurrence_id::text)::uuid,block.block_revision_id,block.ordinal);
  END LOOP;
  FOR binding IN SELECT * FROM bid_workspace_binding_occurrences WHERE workspace_revision_id=old_revision.id
    ORDER BY ordinal,id
  LOOP
    INSERT INTO bid_workspace_binding_occurrences(id,project_id,workspace_revision_id,binding_revision_id,ordinal)
    VALUES(gen_random_uuid(),p_project_id,new_revision_id,binding.binding_revision_id,binding.ordinal);
  END LOOP;
  FOR evidence IN
    SELECT value.* FROM bid_workspace_binding_occurrences occurrence
    JOIN bid_outline_fulfillment_binding_revision_artifacts value
      ON value.id=occurrence.binding_revision_id
    WHERE occurrence.workspace_revision_id=new_revision_id AND value.state='bound'
      AND value.requirement_projection_id=old_revision.requirement_projection_id
      AND value.target_kind='quote' AND value.target_id=p_quote_snapshot_id
    ORDER BY occurrence.ordinal
  LOOP
    IF NOT EXISTS (SELECT 1 FROM bid_submission_fulfillment_evidence_revision_artifacts existing
        WHERE existing.binding_revision_id=evidence.id AND existing.target_revision_id=p_quote_snapshot_id
          AND existing.target_kind='quote_snapshot' AND existing.dependency_sha256=p_quote_snapshot_sha256) THEN
      SELECT coalesce(max(value.revision),0)+1 INTO evidence_revision
        FROM bid_submission_fulfillment_evidence_revision_artifacts value
        WHERE value.project_id=p_project_id AND value.evidence_lineage_id=
          kb_bid_v2_deterministic_uuid('fulfillment-evidence:'||evidence.lineage_id::text||':'||p_quote_snapshot_id::text);
      evidence_payload:=kb_bid_v2_json_payload(jsonb_build_object('schema_version',1,
        'evidence_lineage_id',kb_bid_v2_deterministic_uuid('fulfillment-evidence:'||evidence.lineage_id::text||':'||p_quote_snapshot_id::text),
        'revision',evidence_revision,'workspace_revision_id',new_revision_id,
        'binding_revision_id',evidence.id,'target_revision_id',p_quote_snapshot_id,
        'target_kind','quote_snapshot','dependency_sha256',p_quote_snapshot_sha256));
      evidence_sha:=kb_bid_v2_sha256_bytes(evidence_payload);
      INSERT INTO bid_submission_fulfillment_evidence_revision_artifacts(
        id,project_id,workspace_id,evidence_lineage_id,revision,workspace_revision_id,
        binding_revision_id,target_revision_id,target_kind,dependency_sha256,canonical_payload,content_sha256)
      VALUES(gen_random_uuid(),p_project_id,workspace.id,
        kb_bid_v2_deterministic_uuid('fulfillment-evidence:'||evidence.lineage_id::text||':'||p_quote_snapshot_id::text),
        evidence_revision,new_revision_id,evidence.id,p_quote_snapshot_id,'quote_snapshot',
        p_quote_snapshot_sha256,evidence_payload,evidence_sha);
    END IF;
  END LOOP;
  FOR edge IN SELECT * FROM bid_outline_lineage_edges WHERE workspace_revision_id=old_revision.id
  LOOP
    INSERT INTO bid_outline_lineage_edges(id,project_id,workspace_id,operation,from_lineage_id,to_lineage_id,
      workspace_revision_id,created_at)
    VALUES(gen_random_uuid(),p_project_id,workspace.id,edge.operation,edge.from_lineage_id,
      edge.to_lineage_id,new_revision_id,edge.created_at);
  END LOOP;
  FOR checkpoint IN SELECT * FROM bid_outline_checkpoint_artifacts
    WHERE workspace_revision_id=old_revision.id ORDER BY created_at DESC,id DESC LIMIT 1
  LOOP
    checkpoint_payload:=kb_bid_v2_json_payload(jsonb_build_object('schema_version',1,
      'workspace_id',workspace.id,'workspace_revision_id',new_revision_id,'workspace_sha256',digest,
      'requirement_projection_id',old_revision.requirement_projection_id,
      'requirement_projection_sha256',old_revision.requirement_projection_sha256));
    checkpoint_sha:=kb_bid_v2_sha256_bytes(checkpoint_payload);
    INSERT INTO bid_outline_checkpoint_artifacts(id,project_id,workspace_id,workspace_revision_id,
      requirement_projection_id,requirement_projection_sha256,canonical_payload,content_sha256,actor)
    VALUES(gen_random_uuid(),p_project_id,workspace.id,new_revision_id,old_revision.requirement_projection_id,
      old_revision.requirement_projection_sha256,checkpoint_payload,checkpoint_sha,p_actor);
  END LOOP;
  IF NOT kb_bid_v2_advance_workspace_head(workspace.id,old_revision.id,old_revision.content_sha256,
      new_revision_id,digest) THEN
    RAISE EXCEPTION 'WORKSPACE_HEAD_CAS_MISMATCH' USING ERRCODE='40001';
  END IF;
  RETURN jsonb_build_object('revision_id',new_revision_id,'sha256',digest,'replayed',false);
END $$;

CREATE FUNCTION kb_bid_v2_apply_quote_snapshot(
  p_workspace_id uuid,p_quote_snapshot_id uuid,p_quote_snapshot_sha256 kb_sha256,
  p_expected_workspace_revision_id uuid,p_expected_workspace_sha256 kb_sha256,
  p_actor kb_actor_identity,p_idempotency_key text,p_request_bytes bytea,p_request_sha256 kb_sha256
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE project_id uuid; replay bytea; response jsonb; response_bytes bytea; advance_response jsonb;
BEGIN
  replay:=kb_bid_v2_idempotency_begin(p_actor,'bid.v2.quote_snapshot.apply',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  SELECT workspace.project_id INTO STRICT project_id FROM bid_submission_workspaces workspace
    WHERE workspace.id=p_workspace_id;
  PERFORM kb_bid_v2_require_project_owner(project_id,p_actor);
  advance_response:=kb_bid_v2_advance_workspace_quote(project_id,p_quote_snapshot_id,p_quote_snapshot_sha256,
    p_expected_workspace_revision_id,p_expected_workspace_sha256,p_actor);
  response:=kb_bid_v2_load_workspace(p_workspace_id)||jsonb_build_object(
    'quote_snapshot_id',p_quote_snapshot_id,'quote_snapshot_sha256',p_quote_snapshot_sha256,
    'applied',true,'replayed',advance_response->'replayed');
  response_bytes:=convert_to(response::text,'UTF8');
  PERFORM kb_bid_v2_idempotency_complete(p_actor,'bid.v2.quote_snapshot.apply',p_idempotency_key,200,response_bytes);
  RETURN response;
END $$;

CREATE FUNCTION kb_bid_v2_publish_quote_snapshot(
  p_project_id uuid,p_snapshot_id uuid,p_expected_revision bigint,p_staging_id uuid,
  p_object_ref kb_object_ref,p_content_sha256 kb_sha256,p_byte_length bigint,p_canonical_payload bytea,
  p_actor kb_actor_identity,p_idempotency_key text,p_request_bytes bytea,p_request_sha256 kb_sha256
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE replay bytea; response jsonb; response_bytes bytea; project_status text; actual_revision bigint;
  quote_id uuid:=kb_bid_v2_deterministic_uuid('quote:'||p_project_id::text);payload jsonb;
BEGIN
  PERFORM kb_bid_v2_require_project_owner(p_project_id,p_actor);
  replay:=kb_bid_v2_idempotency_begin(p_actor,'bid.v2.quote_snapshot.publish',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  SELECT status INTO STRICT project_status FROM bid_projects WHERE id=p_project_id FOR UPDATE;
  IF project_status<>'open' THEN RAISE EXCEPTION 'PROJECT_ENDED' USING ERRCODE='55000'; END IF;
  SELECT coalesce(max(revision)+1,1) INTO actual_revision FROM bid_quote_snapshot_artifacts WHERE project_id=p_project_id;
  IF p_expected_revision<>actual_revision THEN RAISE EXCEPTION 'QUOTE_REVISION_CONFLICT' USING ERRCODE='40001'; END IF;
  IF p_content_sha256<>kb_bid_v2_sha256_bytes(p_canonical_payload) OR p_object_ref<>'objects/'||p_content_sha256
    OR p_byte_length<>octet_length(p_canonical_payload) THEN RAISE EXCEPTION 'QUOTE_SNAPSHOT_OBJECT_IDENTITY_INVALID' USING ERRCODE='23514'; END IF;
  payload:=convert_from(p_canonical_payload,'UTF8')::jsonb;
  IF NOT kb_bid_v2_quote_snapshot_payload_valid(payload,p_project_id,quote_id,p_expected_revision,p_actor)
    THEN RAISE EXCEPTION 'QUOTE_SNAPSHOT_SCHEMA_INVALID' USING ERRCODE='23514'; END IF;
  PERFORM kb_object_upload_commit(p_staging_id,p_object_ref,p_content_sha256,'application/json',p_byte_length,
    'bid_quote_snapshot',p_snapshot_id,'canonical',p_actor);
  INSERT INTO bid_quote_snapshot_artifacts(id,project_id,revision,currency,canonical_payload,content_sha256,actor)
  VALUES(p_snapshot_id,p_project_id,p_expected_revision,'CNY',p_canonical_payload,p_content_sha256,p_actor);
  INSERT INTO bid_quote_snapshot_object_identities(quote_snapshot_id,project_id,object_ref,content_sha256)
  VALUES(p_snapshot_id,p_project_id,p_object_ref,p_content_sha256);
  INSERT INTO bid_quote_snapshot_current(scope_id,artifact_id,artifact_sha256,generation,created_at)
  VALUES(p_project_id,p_snapshot_id,p_content_sha256,p_expected_revision,clock_timestamp())
  ON CONFLICT(scope_id) DO UPDATE SET artifact_id=EXCLUDED.artifact_id,
    artifact_sha256=EXCLUDED.artifact_sha256,generation=EXCLUDED.generation,
    created_at=bid_quote_snapshot_current.created_at;
  response:=jsonb_build_object('quote_snapshot_id',p_snapshot_id,'quote_id',quote_id,'revision',p_expected_revision,
    'currency','CNY','sha256',p_content_sha256,'object_ref',p_object_ref,'byte_length',p_byte_length,
    'published_current',true,'workspace_apply_required',true);
  response_bytes:=convert_to(response::text,'UTF8');
  INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
    entity_kind,entity_locator,after_revision,after_sha256)
  VALUES(gen_random_uuid(),1,'bid.v2.quote_snapshot.publish',p_actor,p_idempotency_key,p_request_sha256,
    kb_bid_v2_sha256_bytes(response_bytes),'bid_quote_snapshot',jsonb_build_object('project_id',p_project_id,'quote_snapshot_id',p_snapshot_id),
    p_expected_revision,p_content_sha256);
  PERFORM kb_bid_v2_idempotency_complete(p_actor,'bid.v2.quote_snapshot.publish',p_idempotency_key,201,response_bytes);
  RETURN response;
END $$;

CREATE FUNCTION kb_bid_v2_list_quote_snapshots(p_project_id uuid,p_actor kb_actor_identity)
RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
BEGIN
  PERFORM kb_bid_v2_require_project_owner(p_project_id,p_actor);
  RETURN coalesce((SELECT jsonb_agg(convert_from(snapshot.canonical_payload,'UTF8')::jsonb||jsonb_build_object(
      'quote_snapshot_id',snapshot.id,'sha256',snapshot.content_sha256,'object_ref',object_identity.object_ref,
      'created_at',snapshot.created_at,'is_current',current_value.artifact_id=snapshot.id) ORDER BY snapshot.revision DESC)
    FROM bid_quote_snapshot_artifacts snapshot JOIN bid_quote_snapshot_object_identities object_identity ON object_identity.quote_snapshot_id=snapshot.id
    LEFT JOIN bid_quote_snapshot_current current_value ON current_value.scope_id=snapshot.project_id
    WHERE snapshot.project_id=p_project_id),'[]'::jsonb);
END $$;

CREATE FUNCTION kb_bid_v2_get_quote_snapshot(p_project_id uuid,p_snapshot_id uuid,p_actor kb_actor_identity)
RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE result jsonb;
BEGIN
  PERFORM kb_bid_v2_require_project_owner(p_project_id,p_actor);
  SELECT convert_from(snapshot.canonical_payload,'UTF8')::jsonb||jsonb_build_object(
      'quote_snapshot_id',snapshot.id,'sha256',snapshot.content_sha256,'object_ref',object_identity.object_ref,
      'created_at',snapshot.created_at,'is_current',current_value.artifact_id=snapshot.id)
    INTO result FROM bid_quote_snapshot_artifacts snapshot JOIN bid_quote_snapshot_object_identities object_identity ON object_identity.quote_snapshot_id=snapshot.id
    LEFT JOIN bid_quote_snapshot_current current_value ON current_value.scope_id=snapshot.project_id
    WHERE snapshot.project_id=p_project_id AND snapshot.id=p_snapshot_id;
  IF result IS NULL THEN RAISE EXCEPTION 'QUOTE_SNAPSHOT_NOT_FOUND' USING ERRCODE='P0002'; END IF;
  RETURN result;
END $$;

CREATE FUNCTION kb_bid_v2_get_current_assessments(
  p_workspace_id uuid,p_actor kb_actor_identity
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE workspace bid_submission_workspaces%ROWTYPE; head bid_workspace_heads%ROWTYPE;
  revision bid_workspace_revision_artifacts%ROWTYPE; asset_sha kb_sha256; input_payload bytea; input_sha kb_sha256;
  quote_id uuid; quote_sha kb_sha256;
  outline_id uuid:=gen_random_uuid(); submission_id uuid:=gen_random_uuid();
  outline_existing uuid; submission_existing uuid; requirement_id uuid;
  outline_issues jsonb:='[]'::jsonb; submission_issues jsonb:='[]'::jsonb;
  outline_status text; submission_status text; payload bytea; digest kb_sha256;
BEGIN
  SELECT * INTO STRICT workspace FROM bid_submission_workspaces WHERE id=p_workspace_id;
  PERFORM kb_bid_v2_require_project_owner(workspace.project_id,p_actor);
  SELECT * INTO STRICT head FROM bid_workspace_heads WHERE scope_id=p_workspace_id;
  SELECT * INTO STRICT revision FROM bid_workspace_revision_artifacts
    WHERE id=head.artifact_id AND content_sha256=head.artifact_sha256;
  quote_id:=revision.quote_snapshot_id;
  quote_sha:=revision.quote_snapshot_sha256;
  asset_sha:=kb_bid_v2_sha256_bytes(kb_bid_v2_json_payload(coalesce((SELECT jsonb_agg(jsonb_build_object(
    'asset_revision_id',asset.id,'sha256',asset.content_sha256,'media_type',asset.media_type)
    ORDER BY asset.id) FROM bid_workspace_asset_artifacts asset WHERE asset.workspace_id=p_workspace_id AND EXISTS (
      SELECT 1 FROM bid_workspace_block_occurrences occurrence
      JOIN bid_content_block_revision_artifacts block ON block.id=occurrence.block_revision_id
      WHERE occurrence.workspace_revision_id=revision.id
        AND block.block_payload->>'asset_revision_id'=asset.id::text)),'[]'::jsonb)));
  input_payload:=kb_bid_v2_json_payload(jsonb_build_object('workspace_revision_id',revision.id,
    'workspace_sha256',revision.content_sha256,'scope_revision_id',revision.scope_revision_id,
    'requirement_projection_id',revision.requirement_projection_id,
    'requirement_projection_sha256',revision.requirement_projection_sha256,
    'document_settings_revision_id',revision.document_settings_revision_id,'asset_set_sha256',asset_sha,
    'quote_snapshot_id',quote_id,'quote_snapshot_sha256',quote_sha));
  input_sha:=kb_bid_v2_sha256_bytes(input_payload);
  IF NOT EXISTS (SELECT 1 FROM bid_workspace_node_occurrences WHERE workspace_revision_id=revision.id) THEN
    outline_issues:=outline_issues||jsonb_build_array(jsonb_build_object('issue_id',kb_bid_v2_deterministic_uuid(input_sha||':OUTLINE_EMPTY'),
      'code','OUTLINE_EMPTY','severity','high','message','大纲为空，请先创建或接受章节结构'));
  END IF;
  FOR requirement_id IN SELECT requirement.id FROM bid_workspace_requirement_projection_items item
    JOIN bid_requirement_revision_artifacts requirement ON requirement.project_id=item.project_id
      AND requirement.id=item.requirement_revision_id
    WHERE item.projection_id=revision.requirement_projection_id AND requirement.requiredness='mandatory'
      AND requirement.lifecycle='current' AND NOT EXISTS (
        SELECT 1 FROM bid_workspace_binding_occurrences occurrence
        JOIN bid_outline_fulfillment_binding_revision_artifacts binding
          ON binding.project_id=occurrence.project_id AND binding.id=occurrence.binding_revision_id
        WHERE occurrence.workspace_revision_id=revision.id
          AND binding.need_occurrence_id=ANY(kb_bid_v2_fulfillment_need_ids(requirement.fulfillment_expr))
          AND binding.requirement_projection_id=revision.requirement_projection_id AND binding.state='bound')
  LOOP
    outline_issues:=outline_issues||jsonb_build_array(jsonb_build_object('issue_id',kb_bid_v2_deterministic_uuid(input_sha||':MANDATORY_REQUIREMENT_UNBOUND:'||requirement_id::text),
      'code','MANDATORY_REQUIREMENT_UNBOUND','severity','high','message','必选要求尚未映射到投标内容'));
  END LOOP;
  submission_issues:=outline_issues;
  IF quote_id IS NULL AND EXISTS (SELECT 1 FROM bid_workspace_requirement_projection_items item
      JOIN bid_requirement_revision_artifacts requirement ON requirement.project_id=item.project_id
        AND requirement.id=item.requirement_revision_id
      WHERE item.projection_id=revision.requirement_projection_id
        AND requirement.lifecycle='current' AND requirement.requirement_kind='pricing') THEN
    submission_issues:=submission_issues||jsonb_build_array(jsonb_build_object(
      'issue_id',kb_bid_v2_deterministic_uuid(input_sha||':QUOTE_SNAPSHOT_MISSING'),
      'code','QUOTE_SNAPSHOT_MISSING','severity','warning',
      'message','报价要求存在但尚未冻结报价快照，请复核'));
  END IF;
  IF EXISTS (SELECT 1 FROM bid_workspace_requirement_projection_artifacts projection
      JOIN bid_requirement_set_artifacts requirement_set ON requirement_set.id=projection.requirement_set_id
      JOIN bid_document_set_items item ON item.document_set_id=requirement_set.document_set_id
      WHERE projection.id=revision.requirement_projection_id AND item.disposition<>'ready') THEN
    submission_issues:=submission_issues||jsonb_build_array(jsonb_build_object(
      'issue_id',kb_bid_v2_deterministic_uuid(input_sha||':DOCUMENT_INPUT_NOT_READY'),
      'code','DOCUMENT_INPUT_NOT_READY','severity','warning',
      'message','部分当前招标文件尚未成功解析，评估仅覆盖已就绪输入'));
  END IF;
  IF EXISTS (SELECT 1 FROM bid_workspace_requirement_projection_items item
      JOIN bid_requirement_revision_artifacts requirement ON requirement.project_id=item.project_id
        AND requirement.id=item.requirement_revision_id
      WHERE item.projection_id=revision.requirement_projection_id AND requirement.lifecycle='unresolved') THEN
    submission_issues:=submission_issues||jsonb_build_array(jsonb_build_object(
      'issue_id',kb_bid_v2_deterministic_uuid(input_sha||':UNRESOLVED_REQUIREMENT'),
      'code','UNRESOLVED_REQUIREMENT','severity','warning','message','存在尚未消歧的招标要求，请人工复核'));
  END IF;
  IF EXISTS (SELECT 1 FROM bid_workspace_requirement_projection_items item
      JOIN bid_requirement_revision_artifacts requirement ON requirement.project_id=item.project_id
        AND requirement.id=item.requirement_revision_id
      WHERE item.projection_id=revision.requirement_projection_id AND requirement.lifecycle='current'
        AND requirement.compliance_policy='deviation_allowed' AND NOT EXISTS (
          SELECT 1 FROM bid_workspace_binding_occurrences occurrence
          JOIN bid_outline_fulfillment_binding_revision_artifacts binding ON binding.id=occurrence.binding_revision_id
          JOIN bid_submission_fulfillment_evidence_revision_artifacts evidence ON evidence.binding_revision_id=binding.id
            AND kb_bid_v2_fulfillment_evidence_is_current(revision.id,binding.id)
          WHERE occurrence.workspace_revision_id=revision.id AND binding.state='bound'
            AND binding.requirement_projection_id=revision.requirement_projection_id
            AND binding.need_occurrence_id=ANY(kb_bid_v2_fulfillment_need_ids(requirement.fulfillment_expr)))) THEN
    submission_issues:=submission_issues||jsonb_build_array(jsonb_build_object(
      'issue_id',kb_bid_v2_deterministic_uuid(input_sha||':DEVIATION_REVIEW_REQUIRED'),
      'code','DEVIATION_REVIEW_REQUIRED','severity','warning','message','允许偏离的要求尚无当前履约依据，请复核偏离说明'));
  END IF;
  IF EXISTS (SELECT 1 FROM bid_workspace_requirement_projection_items item
      JOIN bid_requirement_revision_artifacts requirement ON requirement.project_id=item.project_id
        AND requirement.id=item.requirement_revision_id
      WHERE item.projection_id=revision.requirement_projection_id AND requirement.lifecycle='current'
        AND requirement.compliance_policy='scored' AND NOT EXISTS (
          SELECT 1 FROM bid_workspace_binding_occurrences occurrence
          JOIN bid_outline_fulfillment_binding_revision_artifacts binding ON binding.id=occurrence.binding_revision_id
          JOIN bid_submission_fulfillment_evidence_revision_artifacts evidence ON evidence.binding_revision_id=binding.id
            AND kb_bid_v2_fulfillment_evidence_is_current(revision.id,binding.id)
          WHERE occurrence.workspace_revision_id=revision.id AND binding.state='bound'
            AND binding.requirement_projection_id=revision.requirement_projection_id
            AND binding.need_occurrence_id=ANY(kb_bid_v2_fulfillment_need_ids(requirement.fulfillment_expr)))) THEN
    submission_issues:=submission_issues||jsonb_build_array(jsonb_build_object(
      'issue_id',kb_bid_v2_deterministic_uuid(input_sha||':SCORING_EVIDENCE_MISSING'),
      'code','SCORING_EVIDENCE_MISSING','severity','warning','message','评分项缺少当前履约依据，可能造成评分损失'));
  END IF;
  IF EXISTS (SELECT 1 FROM bid_workspace_block_occurrences occurrence
      JOIN bid_content_block_revision_artifacts block ON block.id=occurrence.block_revision_id
      WHERE occurrence.workspace_revision_id=revision.id AND block.block_kind='structured_form'
        AND (jsonb_array_length(coalesce(block.block_payload->'field_values','[]'::jsonb))=0 OR EXISTS (
          SELECT 1 FROM jsonb_array_elements(coalesce(block.block_payload->'field_values','[]'::jsonb)) field
          WHERE btrim(coalesce(field->>'value',''))=''))) THEN
    submission_issues:=submission_issues||jsonb_build_array(jsonb_build_object(
      'issue_id',kb_bid_v2_deterministic_uuid(input_sha||':STRUCTURED_FORM_INCOMPLETE'),
      'code','STRUCTURED_FORM_INCOMPLETE','severity','warning','message','存在未填写完整的结构化响应表单'));
  END IF;
  IF EXISTS (SELECT 1 FROM bid_workspace_block_occurrences occurrence
      JOIN bid_content_block_revision_artifacts block ON block.id=occurrence.block_revision_id
      WHERE occurrence.workspace_revision_id=revision.id AND block.block_kind='attachment_ref'
        AND nullif(block.block_payload->>'preparation_revision_id','') IS NULL) THEN
    submission_issues:=submission_issues||jsonb_build_array(jsonb_build_object(
      'issue_id',kb_bid_v2_deterministic_uuid(input_sha||':ATTACHMENT_PREPARATION_MISSING'),
      'code','ATTACHMENT_PREPARATION_MISSING','severity','warning','message','存在尚未准备为可渲染版本的附件'));
  END IF;
  IF EXISTS (SELECT 1 FROM bid_workspace_requirement_projection_items item
      JOIN bid_requirement_revision_artifacts requirement ON requirement.project_id=item.project_id
        AND requirement.id=item.requirement_revision_id
      WHERE item.projection_id=revision.requirement_projection_id AND requirement.requiredness='mandatory'
        AND requirement.lifecycle='current' AND EXISTS (
          SELECT 1 FROM bid_workspace_binding_occurrences occurrence
          JOIN bid_outline_fulfillment_binding_revision_artifacts binding
            ON binding.project_id=occurrence.project_id AND binding.id=occurrence.binding_revision_id
          WHERE occurrence.workspace_revision_id=revision.id AND binding.state='bound'
            AND binding.requirement_projection_id=revision.requirement_projection_id
            AND binding.need_occurrence_id=ANY(kb_bid_v2_fulfillment_need_ids(requirement.fulfillment_expr)))
        AND NOT EXISTS (
          SELECT 1 FROM bid_workspace_binding_occurrences occurrence
          JOIN bid_outline_fulfillment_binding_revision_artifacts binding
            ON binding.project_id=occurrence.project_id AND binding.id=occurrence.binding_revision_id
          JOIN bid_submission_fulfillment_evidence_revision_artifacts evidence
            ON evidence.binding_revision_id=binding.id
            AND kb_bid_v2_fulfillment_evidence_is_current(revision.id,binding.id)
          WHERE occurrence.workspace_revision_id=revision.id AND binding.state='bound'
            AND binding.requirement_projection_id=revision.requirement_projection_id
            AND binding.need_occurrence_id=ANY(kb_bid_v2_fulfillment_need_ids(requirement.fulfillment_expr)))) THEN
    submission_issues:=submission_issues||jsonb_build_array(jsonb_build_object(
      'issue_id',kb_bid_v2_deterministic_uuid(input_sha||':FULFILLMENT_EVIDENCE_STALE_OR_MISSING'),
      'code','FULFILLMENT_EVIDENCE_STALE_OR_MISSING','severity','warning',
      'message','部分必选要求的履约证据缺失或已过期，请复核'));
  END IF;
  IF EXISTS (SELECT 1 FROM bid_workspace_requirement_projection_items item
      JOIN bid_requirement_revision_artifacts requirement ON requirement.project_id=item.project_id
        AND requirement.id=item.requirement_revision_id
      WHERE item.projection_id=revision.requirement_projection_id AND requirement.requiredness='mandatory'
        AND requirement.lifecycle='current' AND NOT EXISTS (
          SELECT 1 FROM bid_evidence_selection_artifacts selection
          JOIN bid_evidence_match_reports report ON report.id=selection.matching_report_id
          WHERE selection.workspace_id=p_workspace_id AND selection.selection_kind='accepted'
            AND report.requirement_revision_id=requirement.id
            AND jsonb_array_length(convert_from(selection.canonical_payload,'UTF8')::jsonb->'selected_evidence_item_ids')>0)) THEN
    submission_issues:=submission_issues||jsonb_build_array(jsonb_build_object('issue_id',kb_bid_v2_deterministic_uuid(input_sha||':NO_ELIGIBLE_EVIDENCE'),
      'code','NO_ELIGIBLE_EVIDENCE','severity','warning','message','部分必选要求没有可用企业证据，仍可导出但必须人工复核'));
  END IF;
  outline_status:=CASE WHEN EXISTS (SELECT 1 FROM jsonb_array_elements(outline_issues) issue WHERE issue->>'severity'='high')
    THEN 'has_critical_warnings' WHEN jsonb_array_length(outline_issues)>0 THEN 'has_warnings' ELSE 'ready' END;
  submission_status:=CASE WHEN EXISTS (SELECT 1 FROM jsonb_array_elements(submission_issues) issue WHERE issue->>'severity'='high')
    THEN 'has_critical_warnings' WHEN jsonb_array_length(submission_issues)>0 THEN 'has_warnings' ELSE 'ready' END;
  SELECT id INTO outline_existing FROM bid_outline_assessment_snapshot_artifacts
    WHERE workspace_id=p_workspace_id AND assessment_input_sha256=input_sha;
  IF outline_existing IS NULL THEN
    payload:=kb_bid_v2_json_payload(jsonb_build_object('assessment_snapshot_id',outline_id,'assessment_kind','outline',
      'workspace_revision_id',revision.id,'workspace_sha256',revision.content_sha256,
      'scope_revision_id',revision.scope_revision_id,'requirement_projection_id',revision.requirement_projection_id,
      'requirement_projection_sha256',revision.requirement_projection_sha256,
      'document_settings_revision_id',revision.document_settings_revision_id,'asset_set_sha256',asset_sha,
      'quote_snapshot',CASE WHEN quote_id IS NULL THEN NULL ELSE jsonb_build_object('artifact_id',quote_id,'sha256',quote_sha) END,
      'assessment_input_sha256',input_sha,'status',outline_status,'issues',outline_issues)); digest:=kb_bid_v2_sha256_bytes(payload);
    INSERT INTO bid_outline_assessment_snapshot_artifacts(id,project_id,workspace_id,workspace_revision_id,
      requirement_projection_id,scope_revision_id,document_settings_revision_id,asset_set_sha256,
      quote_snapshot_id,quote_snapshot_sha256,status,assessment_input_sha256,canonical_payload,content_sha256)
    VALUES(outline_id,workspace.project_id,p_workspace_id,revision.id,revision.requirement_projection_id,
      revision.scope_revision_id,revision.document_settings_revision_id,asset_sha,
      quote_id,quote_sha,outline_status,input_sha,payload,digest);
  ELSE outline_id:=outline_existing; END IF;
  SELECT id INTO submission_existing FROM bid_submission_assessment_snapshot_artifacts
    WHERE workspace_id=p_workspace_id AND assessment_input_sha256=input_sha;
  IF submission_existing IS NULL THEN
    payload:=kb_bid_v2_json_payload(jsonb_build_object('assessment_snapshot_id',submission_id,'assessment_kind','submission',
      'workspace_revision_id',revision.id,'workspace_sha256',revision.content_sha256,
      'scope_revision_id',revision.scope_revision_id,'requirement_projection_id',revision.requirement_projection_id,
      'requirement_projection_sha256',revision.requirement_projection_sha256,
      'document_settings_revision_id',revision.document_settings_revision_id,'asset_set_sha256',asset_sha,
      'quote_snapshot',CASE WHEN quote_id IS NULL THEN NULL ELSE jsonb_build_object('artifact_id',quote_id,'sha256',quote_sha) END,
      'assessment_input_sha256',input_sha,'status',submission_status,'issues',submission_issues)); digest:=kb_bid_v2_sha256_bytes(payload);
    INSERT INTO bid_submission_assessment_snapshot_artifacts(id,project_id,workspace_id,workspace_revision_id,
      requirement_projection_id,scope_revision_id,document_settings_revision_id,asset_set_sha256,
      quote_snapshot_id,quote_snapshot_sha256,status,assessment_input_sha256,canonical_payload,content_sha256)
    VALUES(submission_id,workspace.project_id,p_workspace_id,revision.id,revision.requirement_projection_id,
      revision.scope_revision_id,revision.document_settings_revision_id,asset_sha,
      quote_id,quote_sha,submission_status,input_sha,payload,digest);
    INSERT INTO bid_submission_assessment_snapshot_evidence_items(assessment_snapshot_id,project_id,workspace_id,
      ordinal,selection_id,selection_sha256,matching_report_id,evidence_bundle_id,evidence_bundle_sha256,
      evidence_item_id,evidence_item_sha256)
    SELECT submission_id,workspace.project_id,p_workspace_id,row_number() OVER (
        ORDER BY frozen.selection_id,frozen.item_ordinal,frozen.evidence_item_id)-1,
      frozen.selection_id,frozen.selection_sha256,frozen.matching_report_id,frozen.evidence_bundle_id,
      frozen.evidence_bundle_sha256,frozen.evidence_item_id,frozen.evidence_item_sha256
    FROM (
      SELECT DISTINCT ON (selection.id,item.id) selection.id selection_id,selection.content_sha256 selection_sha256,
        selection.matching_report_id,bundle.id evidence_bundle_id,bundle.content_sha256 evidence_bundle_sha256,
        item.id evidence_item_id,item.content_sha256 evidence_item_sha256,item.ordinal item_ordinal
      FROM bid_evidence_selection_artifacts selection
      JOIN bid_evidence_bundle_artifacts bundle ON bundle.matching_report_id=selection.matching_report_id
      JOIN bid_evidence_bundle_items item ON item.evidence_bundle_id=bundle.id
      JOIN bid_candidate_artifacts candidate ON candidate.id=(convert_from(selection.canonical_payload,'UTF8')::jsonb->>'candidate_id')::uuid
      JOIN bid_candidate_decision_receipts receipt ON receipt.candidate_id=candidate.id
      JOIN bid_candidate_operations operation ON operation.candidate_id=candidate.id
        AND operation.ordinal=ANY(receipt.accepted_operation_ordinals)
      JOIN bid_workspace_block_occurrences occurrence ON occurrence.workspace_revision_id=revision.id
      JOIN bid_content_block_revision_artifacts current_block ON current_block.id=occurrence.block_revision_id
        AND current_block.lineage_id=(operation.operation#>>'{block,lineage_id}')::uuid
      WHERE selection.workspace_id=p_workspace_id AND selection.selection_kind='accepted'
        AND item.id IN (SELECT jsonb_array_elements_text(
          convert_from(selection.canonical_payload,'UTF8')::jsonb->'selected_evidence_item_ids')::uuid)
        AND (operation.operation#>>'{block,content,asset_revision_id}'=item.id::text OR EXISTS (
          SELECT 1 FROM jsonb_array_elements(convert_from(candidate.canonical_payload,'UTF8')::jsonb->'factual_claims') claim
          WHERE claim->>'client_operation_ref'=operation.operation->>'client_operation_ref'
            AND claim->>'evidence_bundle_id'=bundle.id::text AND claim->>'evidence_item_id'=item.id::text))
      ORDER BY selection.id,item.id,operation.ordinal
    ) frozen;
  ELSE submission_id:=submission_existing; END IF;
  RETURN jsonb_build_object(
    'outline',(SELECT convert_from(canonical_payload,'UTF8')::jsonb FROM bid_outline_assessment_snapshot_artifacts WHERE id=outline_id),
    'submission',(SELECT convert_from(canonical_payload,'UTF8')::jsonb FROM bid_submission_assessment_snapshot_artifacts WHERE id=submission_id));
END $$;

CREATE FUNCTION kb_bid_v2_load_preview_input(p_workspace_id uuid,p_actor kb_actor_identity)
RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE workspace bid_submission_workspaces%ROWTYPE; title text; revision_id uuid; workspace_value jsonb;
BEGIN
  SELECT * INTO STRICT workspace FROM bid_submission_workspaces WHERE id=p_workspace_id;
  PERFORM kb_bid_v2_require_project_owner(workspace.project_id,p_actor);
  SELECT project.title INTO STRICT title FROM bid_projects project WHERE project.id=workspace.project_id;
  SELECT artifact_id INTO STRICT revision_id FROM bid_workspace_heads WHERE scope_id=p_workspace_id;
  workspace_value:=kb_bid_v2_load_workspace(p_workspace_id);
  RETURN jsonb_build_object('title',title,'workspace',workspace_value,
    'assets',coalesce((SELECT jsonb_agg(value ORDER BY value->>'asset_revision_id') FROM (
      SELECT jsonb_build_object('asset_revision_id',asset.id,'object_ref',asset.object_ref,'sha256',asset.content_sha256,
        'media_type',asset.media_type,'file_name',asset.file_name) value
      FROM bid_workspace_asset_artifacts asset WHERE asset.workspace_id=p_workspace_id AND EXISTS (
        SELECT 1 FROM bid_workspace_block_occurrences occurrence
        JOIN bid_content_block_revision_artifacts block ON block.id=occurrence.block_revision_id
        WHERE occurrence.workspace_revision_id=revision_id AND block.block_kind IN ('image','attachment_ref')
          AND (block.block_payload->>'asset_revision_id')::uuid=asset.id)
      UNION ALL
      SELECT jsonb_build_object('asset_revision_id',page.id,'object_ref',page.object_ref,'sha256',page.content_sha256,
        'media_type',page.media_type,'file_name','attachment-page-'||page.page_number) value
      FROM bid_attachment_preparation_asset_items page JOIN bid_attachment_preparation_revision_artifacts preparation
        ON preparation.id=page.attachment_preparation_revision_id
      WHERE preparation.workspace_id=p_workspace_id AND preparation.status='ready' AND EXISTS (
        SELECT 1 FROM bid_workspace_block_occurrences occurrence
        JOIN bid_content_block_revision_artifacts block ON block.id=occurrence.block_revision_id
        WHERE occurrence.workspace_revision_id=revision_id AND block.block_kind='attachment_ref'
          AND (block.block_payload->>'preparation_revision_id')::uuid=preparation.id
      )
    ) resources),'[]'::jsonb),
    'forms',coalesce((SELECT jsonb_agg(convert_from(form.canonical_payload,'UTF8')::jsonb ORDER BY form.id)
      FROM bid_tender_structured_form_definition_artifacts form WHERE form.project_id=workspace.project_id AND EXISTS (
        SELECT 1 FROM bid_workspace_block_occurrences occurrence
        JOIN bid_content_block_revision_artifacts block ON block.id=occurrence.block_revision_id
        WHERE occurrence.workspace_revision_id=revision_id AND block.block_kind='structured_form'
          AND (block.block_payload->>'form_definition_revision_id')::uuid=form.id)),'[]'::jsonb),
    'preparations',coalesce((SELECT jsonb_agg(preparation.canonical_payload ORDER BY preparation.id)
      FROM bid_attachment_preparation_revision_artifacts preparation
      WHERE preparation.workspace_id=p_workspace_id AND preparation.status='ready' AND EXISTS (
        SELECT 1 FROM bid_workspace_block_occurrences occurrence
        JOIN bid_content_block_revision_artifacts block ON block.id=occurrence.block_revision_id
        WHERE occurrence.workspace_revision_id=revision_id AND block.block_kind='attachment_ref'
          AND (block.block_payload->>'preparation_revision_id')::uuid=preparation.id)),'[]'::jsonb));
END $$;

CREATE FUNCTION kb_bid_v2_get_preview_html(p_workspace_id uuid,p_actor kb_actor_identity)
RETURNS text LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE workspace_value jsonb; node jsonb; block jsonb; html text:='<!doctype html><html lang="zh-CN"><head><meta charset="utf-8"><title>投标文件预览</title></head><body>';
BEGIN
  workspace_value:=kb_bid_v2_load_workspace_for_actor(p_workspace_id,p_actor);
  FOR node IN SELECT value FROM jsonb_array_elements(workspace_value->'nodes') ORDER BY (value->>'ordinal')::integer LOOP
    html:=html||'<section><h2>'||replace(replace(replace(node->>'title','&','&amp;'),'<','&lt;'),'>','&gt;')||'</h2>';
    FOR block IN SELECT item.value FROM jsonb_array_elements(workspace_value->'blocks') item
      WHERE EXISTS (SELECT 1 FROM jsonb_array_elements_text(node->'block_lineage_ids') id WHERE id=block->>'lineage_id')
    LOOP
      IF block->>'kind'='rich_text' THEN
        html:=html||'<p>'||replace(replace(replace(coalesce((SELECT string_agg(inline->>'text','')
          FROM jsonb_array_elements(block#>'{content,nodes}') rich
          CROSS JOIN LATERAL jsonb_array_elements(coalesce(rich->'content','[]'::jsonb)) inline
          WHERE inline->>'kind'='text'),''),'&','&amp;'),'<','&lt;'),'>','&gt;')||'</p>';
      ELSIF block->>'kind'='page_break' THEN html:=html||'<hr class="page-break">';
      ELSE html:=html||'<div data-block-kind="'||block->>'kind'||'">['||block->>'kind'||']</div>';
      END IF;
    END LOOP;
    html:=html||'</section>';
  END LOOP;
  RETURN html||'</body></html>';
END $$;



CREATE FUNCTION kb_bid_v2_mark_tender_document_failed(
  p_request_artifact_id uuid,p_request_revision bigint,
  p_frozen_input_sha256 kb_sha256,p_error_code text
) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE typed bid_tender_document_process_request_identities%ROWTYPE;
  request_value bid_async_request_snapshot_artifacts%ROWTYPE;
BEGIN
  SELECT * INTO STRICT request_value FROM bid_async_request_snapshot_artifacts
    WHERE id=p_request_artifact_id FOR UPDATE;
  IF request_value.revision<>p_request_revision
     OR request_value.frozen_input_sha256<>p_frozen_input_sha256
     OR request_value.request_kind<>'tender_document_process' THEN
    RAISE EXCEPTION 'REQUEST_OBSOLETE' USING ERRCODE='P0001';
  END IF;
  SELECT * INTO typed FROM bid_tender_document_process_request_identities
    WHERE request_artifact_id=p_request_artifact_id
      AND request_revision=p_request_revision
      AND frozen_input_sha256=p_frozen_input_sha256
      AND request_kind='tender_document_process';
  IF NOT FOUND THEN
    RAISE EXCEPTION 'REQUEST_OBSOLETE' USING ERRCODE='P0001';
  END IF;
  IF request_value.status<>'pending' THEN RETURN; END IF;
  IF p_error_code NOT IN ('INPUT_SCHEMA_INVALID','FROZEN_INPUT_MISSING','FROZEN_INPUT_DIGEST_MISMATCH',
      'WORKSPACE_CAS_CONFLICT','AGENT_OUTPUT_INVALID','EVIDENCE_UNAVAILABLE','ASSET_MISSING',
      'ASSET_DIGEST_MISMATCH','ATTACHMENT_PREPARATION_FAILED','RENDER_SCHEMA_INVALID','RENDERER_FAILED',
      'OBJECT_COMMIT_FAILED','TENDER_DOCUMENT_PROCESS_TIMEOUT') THEN
    RAISE EXCEPTION 'unknown TenderDocumentProcess terminal error code' USING ERRCODE='P0001';
  END IF;
  UPDATE bid_documents SET parse_status='failed'
    WHERE id=typed.document_id AND project_id=typed.project_id AND parse_status IN ('pending','processing');
  UPDATE bid_async_request_snapshot_artifacts SET status='failed',
    error_code=p_error_code,finished_at=clock_timestamp()
    WHERE id=p_request_artifact_id AND revision=p_request_revision
      AND frozen_input_sha256=p_frozen_input_sha256 AND request_kind='tender_document_process'
      AND status='pending';
END $$;


CREATE TRIGGER bid_outline_candidate_block_targets_append_only_guard BEFORE UPDATE OR DELETE ON bid_outline_candidate_block_targets_v1
FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();
CREATE TRIGGER bid_outline_candidate_block_targets_append_only_truncate_guard BEFORE TRUNCATE ON bid_outline_candidate_block_targets_v1
FOR EACH STATEMENT EXECUTE FUNCTION kb_reject_append_only();

CREATE TABLE bid_tender_agent_checkpoint_artifacts (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  request_artifact_id uuid NOT NULL,
  frozen_input_sha256 kb_sha256 NOT NULL,
  stage_kind text NOT NULL CHECK (stage_kind IN ('analysis_checkpoint','composition_checkpoint','export_review_checkpoint','layout_checkpoint')),
  batch_ordinal integer NOT NULL CHECK (batch_ordinal>=0),
  contract_sha256 kb_sha256 NOT NULL,
  canonical_input bytea NOT NULL,
  input_sha256 kb_sha256 NOT NULL,
  canonical_payload bytea NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(request_artifact_id,frozen_input_sha256,stage_kind,batch_ordinal),
  FOREIGN KEY(request_artifact_id,frozen_input_sha256)
    REFERENCES bid_async_request_snapshot_artifacts(id,frozen_input_sha256),
  CHECK (input_sha256=kb_bid_v2_sha256_bytes(canonical_input)),
  CHECK (content_sha256=kb_bid_v2_sha256_bytes(canonical_payload))
);

CREATE TRIGGER bid_tender_agent_checkpoint_artifacts_append_only BEFORE UPDATE OR DELETE ON bid_tender_agent_checkpoint_artifacts
FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();
CREATE TRIGGER bid_tender_agent_checkpoint_artifacts_no_truncate BEFORE TRUNCATE ON bid_tender_agent_checkpoint_artifacts
FOR EACH STATEMENT EXECUTE FUNCTION kb_reject_append_only();

CREATE TABLE bid_tender_agent_call_attempts (
  request_artifact_id uuid NOT NULL,
  frozen_input_sha256 kb_sha256 NOT NULL,
  stage_kind text NOT NULL CHECK (stage_kind IN ('analysis_main','analysis_review','composition_main','composition_review','export_review')),
  batch_ordinal integer NOT NULL CHECK (batch_ordinal>=0),
  input_sha256 kb_sha256 NOT NULL,
  stage_contract_sha256 kb_sha256 NOT NULL,
  system_prompt_utf8_sha256 kb_sha256 NOT NULL,
  prompt_contract_id uuid NOT NULL,
  prompt_contract_sha256 kb_sha256 NOT NULL,
  schema_contract_id text NOT NULL CHECK (schema_contract_id IN ('tender_analysis_tools_v1','docx_composition_tools_v1','export_review_tools_v1')),
  provider_body bytea NOT NULL,
  provider_body_sha256 kb_sha256 NOT NULL,
  CHECK(provider_body_sha256=kb_bid_v2_sha256_bytes(provider_body)),
  CHECK(provider_body=convert_to(kb_bid_v2_jcs(convert_from(provider_body,'UTF8')::jsonb),'UTF8')),
  schema_contract_sha256 kb_sha256 NOT NULL,
  agent_contract_id uuid NOT NULL,
  agent_contract_sha256 kb_sha256 NOT NULL,
  model_contract_id uuid NOT NULL,
  model_contract_sha256 kb_sha256 NOT NULL,
  runtime_contract_sha256 kb_sha256 NOT NULL,
  call_ordinal integer NOT NULL CHECK (call_ordinal BETWEEN 1 AND 3),
  reserved_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(request_artifact_id,frozen_input_sha256,stage_kind,batch_ordinal,call_ordinal),
  FOREIGN KEY(request_artifact_id,frozen_input_sha256)
    REFERENCES bid_async_request_snapshot_artifacts(id,frozen_input_sha256)
);

CREATE TRIGGER bid_tender_agent_call_attempts_append_only BEFORE UPDATE OR DELETE ON bid_tender_agent_call_attempts
FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();
CREATE TRIGGER bid_tender_agent_call_attempts_no_truncate BEFORE TRUNCATE ON bid_tender_agent_call_attempts
FOR EACH STATEMENT EXECUTE FUNCTION kb_reject_append_only();

CREATE TABLE bid_tender_agent_run_artifacts (
  request_artifact_id uuid NOT NULL,
  frozen_input_sha256 kb_sha256 NOT NULL,
  attempt integer NOT NULL CHECK (attempt BETWEEN 1 AND 4),
  status text NOT NULL CHECK (status IN ('running','retry_yielded','superseded','succeeded','failed')),
  execution_owner_token uuid NOT NULL,
  lease_acquired_at timestamptz NOT NULL,
  lease_expires_at timestamptz NOT NULL,
  heartbeat_at timestamptz NOT NULL,
  hard_deadline_at timestamptz NOT NULL,
  progress_stage text NOT NULL CHECK (progress_stage IN ('analyzing','mapping','reviewing','generating')),
  progress_phase text NOT NULL DEFAULT 'analyzing' CHECK (progress_phase IN (
    'analyzing','mapping','grouping','reducing','collecting','drafting','routing','verifying',
    'publishing','retrying','succeeded','failed')),
  progress_detail jsonb NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(progress_detail)='object'),
  progress_sequence bigint NOT NULL DEFAULT 0 CHECK (progress_sequence>=0),
  last_error_code text,
  last_error_message text CHECK (last_error_message IS NULL OR octet_length(last_error_message)<=8192),
  last_error_at timestamptz,
  attempt_turn_count integer NOT NULL DEFAULT 0 CHECK (attempt_turn_count>=0),
  attempt_tool_call_count integer NOT NULL DEFAULT 0 CHECK (attempt_tool_call_count>=0),
  turn_count integer NOT NULL DEFAULT 0 CHECK (turn_count>=0),
  tool_call_count integer NOT NULL DEFAULT 0 CHECK (tool_call_count>=0),
  text_bytes_read bigint NOT NULL DEFAULT 0 CHECK (text_bytes_read>=0),
  images_read integer NOT NULL DEFAULT 0 CHECK (images_read>=0),
  checkpoint_sha256 kb_sha256,
  started_at timestamptz NOT NULL,
  updated_at timestamptz NOT NULL,
  PRIMARY KEY(request_artifact_id,attempt),
  UNIQUE(request_artifact_id,frozen_input_sha256,attempt,execution_owner_token),
  FOREIGN KEY(request_artifact_id,frozen_input_sha256)
    REFERENCES bid_async_request_snapshot_artifacts(id,frozen_input_sha256),
  CHECK (heartbeat_at>=lease_acquired_at AND hard_deadline_at=lease_acquired_at+interval '46 minutes'),
  CHECK (lease_expires_at<=hard_deadline_at),
  CHECK ((last_error_code IS NULL)=(last_error_at IS NULL)),
  CHECK ((last_error_code IS NULL)=(last_error_message IS NULL))
);

CREATE FUNCTION kb_bid_v2_tender_agent_run_history_guard() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
BEGIN
  IF TG_OP='DELETE' OR NEW.request_artifact_id<>OLD.request_artifact_id
     OR NEW.frozen_input_sha256<>OLD.frozen_input_sha256 OR NEW.attempt<>OLD.attempt
     OR NEW.execution_owner_token<>OLD.execution_owner_token
     OR NEW.lease_acquired_at<>OLD.lease_acquired_at
     OR NEW.hard_deadline_at<>OLD.hard_deadline_at OR NEW.started_at<>OLD.started_at THEN
    RAISE EXCEPTION 'AgentRun immutable identity cannot change' USING ERRCODE='42501';
  END IF;
  RETURN NEW;
END $$;
CREATE TRIGGER bid_tender_agent_run_history_guard
BEFORE UPDATE OR DELETE ON bid_tender_agent_run_artifacts
FOR EACH ROW EXECUTE FUNCTION kb_bid_v2_tender_agent_run_history_guard();


CREATE TABLE bid_content_agent_input_artifacts (
  request_artifact_id uuid PRIMARY KEY,
  frozen_input_sha256 kb_sha256 NOT NULL,
  canonical_payload bytea NOT NULL,
  input_sha256 kb_sha256 NOT NULL,
  created_by_attempt integer NOT NULL CHECK (created_by_attempt BETWEEN 1 AND 4),
  created_by_execution_owner_token uuid NOT NULL,
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  UNIQUE(request_artifact_id,frozen_input_sha256,input_sha256),
  FOREIGN KEY(request_artifact_id,frozen_input_sha256)
    REFERENCES bid_async_request_snapshot_artifacts(id,frozen_input_sha256),
  CHECK (input_sha256=kb_bid_v2_sha256_bytes(canonical_payload)),
  CHECK (convert_from(canonical_payload,'UTF8')::jsonb IS NOT NULL)
);

CREATE FUNCTION kb_bid_v2_content_input_history_guard() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
BEGIN
  RAISE EXCEPTION 'Content Agent input history is immutable' USING ERRCODE='42501';
END $$;
CREATE TRIGGER bid_content_agent_input_history_guard
BEFORE UPDATE OR DELETE ON bid_content_agent_input_artifacts
FOR EACH ROW EXECUTE FUNCTION kb_bid_v2_content_input_history_guard();
CREATE TRIGGER bid_content_agent_input_no_truncate
BEFORE TRUNCATE ON bid_content_agent_input_artifacts
FOR EACH STATEMENT EXECUTE FUNCTION kb_bid_v2_content_input_history_guard();

CREATE TABLE bid_content_agent_run_artifacts (
  request_artifact_id uuid NOT NULL,
  frozen_input_sha256 kb_sha256 NOT NULL,
  attempt integer NOT NULL CHECK (attempt BETWEEN 1 AND 4),
  status text NOT NULL CHECK (status IN ('running','retry_yielded','superseded','succeeded','failed')),
  execution_owner_token uuid NOT NULL DEFAULT gen_random_uuid(),
  lease_acquired_at timestamptz NOT NULL,
  lease_expires_at timestamptz NOT NULL,
  heartbeat_at timestamptz NOT NULL,
  hard_deadline_at timestamptz NOT NULL,
  progress_phase text NOT NULL CHECK (progress_phase IN ('retrieving','generating','validating','publishing','retrying','succeeded','failed')),
  progress_detail jsonb NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(progress_detail)='object'),
  progress_sequence bigint NOT NULL DEFAULT 0 CHECK (progress_sequence>=0),
  last_error_code text CHECK (last_error_code IS NULL OR last_error_code IN (
    'INTERNAL','REQUEST_ATTEMPT_SUPERSEDED','REQUEST_ATTEMPT_BUDGET_EXCEEDED',
    'INPUT_SCHEMA_INVALID','FROZEN_INPUT_MISSING','FROZEN_INPUT_DIGEST_MISMATCH','WORKSPACE_CAS_CONFLICT',
    'CONTENT_RETRIEVAL_INVALID_REQUEST','CONTENT_RETRIEVAL_UNAVAILABLE','CONTENT_RETRIEVAL_QUOTA_EXCEEDED',
    'CONTENT_RETRIEVAL_INVALID_HIT','CONTENT_RETRIEVAL_POLICY_REVOKED','CONTENT_RETRIEVAL_DIGEST_MISMATCH',
    'CONTENT_DIVERGENT_AGENT_INPUT_REPLAY','AGENT_PROVIDER_UNAVAILABLE','AGENT_TURN_TIMEOUT','AGENT_OUTPUT_INVALID','AGENT_TURN_BUDGET_EXCEEDED',
    'AGENT_DEADLINE_EXCEEDED','CONTENT_MATCH_TIMEOUT')),
  last_error_message text CHECK (last_error_message IS NULL OR octet_length(last_error_message)<=8192),
  last_error_at timestamptz,
  started_at timestamptz NOT NULL,
  updated_at timestamptz NOT NULL,
  PRIMARY KEY(request_artifact_id,attempt),
  UNIQUE(request_artifact_id,frozen_input_sha256,attempt,execution_owner_token),
  FOREIGN KEY(request_artifact_id,frozen_input_sha256)
    REFERENCES bid_async_request_snapshot_artifacts(id,frozen_input_sha256),
  CHECK (heartbeat_at>=lease_acquired_at AND hard_deadline_at=lease_acquired_at+interval '46 minutes'),
  CHECK (lease_expires_at<=hard_deadline_at),
  CHECK ((last_error_code IS NULL)=(last_error_at IS NULL)),
  CHECK ((last_error_code IS NULL)=(last_error_message IS NULL))
);
CREATE UNIQUE INDEX bid_content_agent_one_running
  ON bid_content_agent_run_artifacts(request_artifact_id) WHERE status='running';

CREATE FUNCTION kb_bid_v2_content_run_history_guard() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
BEGIN
  IF TG_OP='DELETE' OR NEW.request_artifact_id<>OLD.request_artifact_id
     OR NEW.frozen_input_sha256<>OLD.frozen_input_sha256 OR NEW.attempt<>OLD.attempt
     OR NEW.execution_owner_token<>OLD.execution_owner_token
     OR NEW.lease_acquired_at<>OLD.lease_acquired_at
     OR NEW.hard_deadline_at<>OLD.hard_deadline_at OR NEW.started_at<>OLD.started_at THEN
    RAISE EXCEPTION 'Content AgentRun immutable identity cannot change' USING ERRCODE='42501';
  END IF;
  RETURN NEW;
END $$;
CREATE TRIGGER bid_content_agent_run_history_guard
BEFORE UPDATE OR DELETE ON bid_content_agent_run_artifacts
FOR EACH ROW EXECUTE FUNCTION kb_bid_v2_content_run_history_guard();
CREATE TRIGGER bid_content_agent_run_no_truncate
BEFORE TRUNCATE ON bid_content_agent_run_artifacts
FOR EACH STATEMENT EXECUTE FUNCTION kb_bid_v2_content_input_history_guard();

CREATE FUNCTION kb_bid_v2_content_run_kind_guard() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM bid_content_generation_request_identities typed
    WHERE typed.request_artifact_id=NEW.request_artifact_id
      AND typed.frozen_input_sha256=NEW.frozen_input_sha256
      AND typed.request_operation='generate') THEN
    RAISE EXCEPTION 'Content AgentRun requires ContentGenerate(generate)' USING ERRCODE='23514';
  END IF;
  RETURN NEW;
END $$;
CREATE TRIGGER bid_content_agent_run_kind_guard
BEFORE INSERT ON bid_content_agent_run_artifacts
FOR EACH ROW EXECUTE FUNCTION kb_bid_v2_content_run_kind_guard();
ALTER TABLE bid_content_agent_input_artifacts ADD FOREIGN KEY(
  request_artifact_id,frozen_input_sha256,created_by_attempt,created_by_execution_owner_token)
  REFERENCES bid_content_agent_run_artifacts(
    request_artifact_id,frozen_input_sha256,attempt,execution_owner_token);

CREATE TABLE bid_content_agent_boundary_attempts (
  request_artifact_id uuid NOT NULL,
  frozen_input_sha256 kb_sha256 NOT NULL,
  request_operation text NOT NULL DEFAULT 'generate' CHECK (request_operation='generate'),
  stage_kind text NOT NULL CHECK (stage_kind='content_generate'),
  batch_ordinal integer NOT NULL CHECK (batch_ordinal=0),
  input_sha256 kb_sha256 NOT NULL,
  prompt_contract_id uuid NOT NULL,
  prompt_contract_sha256 kb_sha256 NOT NULL,
  prompt_sha256 kb_sha256 NOT NULL,
  schema_contract_id text NOT NULL CHECK (schema_contract_id='urn:knowledgebrain:bid:content-generation-output:v1'),
  schema_contract_sha256 kb_sha256 NOT NULL,
  agent_contract_id uuid NOT NULL,
  agent_contract_sha256 kb_sha256 NOT NULL,
  model_contract_id uuid NOT NULL,
  model_contract_sha256 kb_sha256 NOT NULL,
  runtime_contract_sha256 kb_sha256 NOT NULL,
  call_ordinal integer NOT NULL CHECK (call_ordinal BETWEEN 1 AND 3),
  reserved_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY(request_artifact_id,frozen_input_sha256,stage_kind,batch_ordinal,call_ordinal),
  FOREIGN KEY(request_artifact_id,frozen_input_sha256,request_operation)
    REFERENCES bid_content_generation_request_identities(request_artifact_id,frozen_input_sha256,request_operation),
  FOREIGN KEY(request_artifact_id,frozen_input_sha256,input_sha256)
    REFERENCES bid_content_agent_input_artifacts(request_artifact_id,frozen_input_sha256,input_sha256),
  FOREIGN KEY(prompt_contract_id,prompt_contract_sha256)
    REFERENCES bid_authoring_contract_artifacts(id,content_sha256),
  FOREIGN KEY(agent_contract_id,agent_contract_sha256)
    REFERENCES bid_authoring_contract_artifacts(id,content_sha256),
  FOREIGN KEY(model_contract_id,model_contract_sha256)
    REFERENCES bid_authoring_contract_artifacts(id,content_sha256)
);
CREATE FUNCTION kb_bid_v2_content_boundary_history_guard() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
BEGIN
  RAISE EXCEPTION 'Content Agent boundary history is immutable' USING ERRCODE='42501';
END $$;
CREATE TRIGGER bid_content_agent_boundary_history_guard
BEFORE UPDATE OR DELETE ON bid_content_agent_boundary_attempts
FOR EACH ROW EXECUTE FUNCTION kb_bid_v2_content_boundary_history_guard();
CREATE TRIGGER bid_content_agent_boundary_no_truncate
BEFORE TRUNCATE ON bid_content_agent_boundary_attempts
FOR EACH STATEMENT EXECUTE FUNCTION kb_bid_v2_content_boundary_history_guard();

CREATE FUNCTION kb_bid_v2_content_lock_owner(
  p_request_artifact_id uuid,p_request_revision bigint,p_frozen_input_sha256 kb_sha256,
  p_attempt integer,p_execution_owner_token uuid
) RETURNS timestamptz LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE request_value bid_async_request_snapshot_artifacts%ROWTYPE;
  run_value bid_content_agent_run_artifacts%ROWTYPE; now_value timestamptz;
BEGIN
  SELECT * INTO request_value FROM bid_async_request_snapshot_artifacts WHERE id=p_request_artifact_id FOR UPDATE;
  SELECT * INTO run_value FROM bid_content_agent_run_artifacts
    WHERE request_artifact_id=p_request_artifact_id AND attempt=p_attempt FOR UPDATE;
  now_value:=clock_timestamp();
  IF request_value.id IS NULL OR request_value.request_kind<>'content_generate'
     OR request_value.revision<>p_request_revision
     OR request_value.frozen_input_sha256<>p_frozen_input_sha256 OR request_value.status<>'pending'
     OR NOT EXISTS (SELECT 1 FROM bid_content_generation_request_identities typed
       WHERE typed.request_artifact_id=p_request_artifact_id AND typed.request_operation='generate') THEN
    RAISE EXCEPTION 'REQUEST_OBSOLETE' USING ERRCODE='P0002';
  END IF;
  IF request_value.current_attempt<>p_attempt OR run_value.request_artifact_id IS NULL
     OR run_value.frozen_input_sha256<>p_frozen_input_sha256
     OR run_value.execution_owner_token<>p_execution_owner_token OR run_value.status<>'running'
     OR run_value.lease_expires_at<=now_value OR run_value.hard_deadline_at<=now_value THEN
    RAISE EXCEPTION 'REQUEST_ATTEMPT_SUPERSEDED' USING ERRCODE='40001';
  END IF;
  RETURN now_value;
END $$;

CREATE FUNCTION kb_bid_v2_content_agent_input_get(
  p_request_artifact_id uuid,p_frozen_input_sha256 kb_sha256
) RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE artifact bid_content_agent_input_artifacts%ROWTYPE;
BEGIN
  SELECT * INTO artifact FROM bid_content_agent_input_artifacts
    WHERE request_artifact_id=p_request_artifact_id AND frozen_input_sha256=p_frozen_input_sha256;
  IF artifact.request_artifact_id IS NULL THEN RETURN NULL; END IF;
  RETURN jsonb_build_object('input_sha256',artifact.input_sha256,
    'payload',convert_from(artifact.canonical_payload,'UTF8')::jsonb);
END $$;

CREATE FUNCTION kb_bid_v2_content_agent_input_put(
  p_request_artifact_id uuid,p_request_revision bigint,p_frozen_input_sha256 kb_sha256,
  p_attempt integer,p_execution_owner_token uuid,p_canonical_payload bytea,p_input_sha256 kb_sha256
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE existing bid_content_agent_input_artifacts%ROWTYPE;
BEGIN
  PERFORM kb_bid_v2_content_lock_owner(p_request_artifact_id,p_request_revision,
    p_frozen_input_sha256,p_attempt,p_execution_owner_token);
  IF p_input_sha256<>kb_bid_v2_sha256_bytes(p_canonical_payload) THEN
    RAISE EXCEPTION 'CONTENT_DIVERGENT_AGENT_INPUT_REPLAY' USING ERRCODE='23514';
  END IF;
  SELECT * INTO existing FROM bid_content_agent_input_artifacts
    WHERE request_artifact_id=p_request_artifact_id FOR UPDATE;
  IF existing.request_artifact_id IS NOT NULL THEN
    IF existing.frozen_input_sha256<>p_frozen_input_sha256
       OR existing.input_sha256<>p_input_sha256 OR existing.canonical_payload<>p_canonical_payload THEN
      RAISE EXCEPTION 'CONTENT_DIVERGENT_AGENT_INPUT_REPLAY' USING ERRCODE='23514';
    END IF;
    RETURN jsonb_build_object('input_sha256',existing.input_sha256,
      'payload',convert_from(existing.canonical_payload,'UTF8')::jsonb,'replayed',true);
  END IF;
  INSERT INTO bid_content_agent_input_artifacts(request_artifact_id,frozen_input_sha256,
    canonical_payload,input_sha256,created_by_attempt,created_by_execution_owner_token)
  VALUES(p_request_artifact_id,p_frozen_input_sha256,p_canonical_payload,p_input_sha256,
    p_attempt,p_execution_owner_token);
  RETURN jsonb_build_object('input_sha256',p_input_sha256,
    'payload',convert_from(p_canonical_payload,'UTF8')::jsonb,'replayed',false);
END $$;

CREATE FUNCTION kb_bid_v2_content_run_claim(
  p_request_artifact_id uuid,p_request_revision bigint,p_frozen_input_sha256 kb_sha256
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE request_value bid_async_request_snapshot_artifacts%ROWTYPE;
  current_run bid_content_agent_run_artifacts%ROWTYPE;
  next_attempt integer; token uuid; claimed_at timestamptz;
BEGIN
  SELECT * INTO request_value FROM bid_async_request_snapshot_artifacts
    WHERE id=p_request_artifact_id FOR UPDATE;
  IF request_value.id IS NULL OR request_value.revision<>p_request_revision
     OR request_value.frozen_input_sha256<>p_frozen_input_sha256
     OR request_value.request_kind<>'content_generate'
     OR NOT EXISTS (SELECT 1 FROM bid_content_generation_request_identities typed
       WHERE typed.request_artifact_id=p_request_artifact_id AND typed.request_operation='generate') THEN
    RETURN jsonb_build_object('disposition','obsolete');
  END IF;
  IF request_value.current_attempt>0 THEN
    SELECT * INTO current_run FROM bid_content_agent_run_artifacts
      WHERE request_artifact_id=p_request_artifact_id AND attempt=request_value.current_attempt FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Content AgentRun current attempt is missing' USING ERRCODE='23514'; END IF;
  END IF;
  claimed_at:=clock_timestamp();
  IF request_value.status<>'pending' THEN RETURN jsonb_build_object('disposition','obsolete'); END IF;
  IF request_value.current_attempt>0 THEN
    IF current_run.status='running' AND current_run.lease_expires_at>claimed_at
       AND current_run.hard_deadline_at>claimed_at THEN
      RETURN jsonb_build_object('disposition','live_owner','attempt',current_run.attempt);
    END IF;
    IF current_run.status='running' THEN
      UPDATE bid_content_agent_run_artifacts SET status='superseded',lease_expires_at=least(lease_expires_at,claimed_at),
        heartbeat_at=greatest(heartbeat_at,claimed_at),last_error_code='REQUEST_ATTEMPT_SUPERSEDED',
        last_error_message='Content AgentRun lease expired or hard deadline elapsed',last_error_at=claimed_at,
        progress_sequence=progress_sequence+1,updated_at=claimed_at
      WHERE request_artifact_id=p_request_artifact_id AND attempt=current_run.attempt;
    ELSIF current_run.status IN ('succeeded','failed') THEN
      RETURN jsonb_build_object('disposition','obsolete');
    END IF;
  END IF;
  IF request_value.current_attempt>=request_value.max_run_attempts THEN
    UPDATE bid_content_agent_run_artifacts SET status='failed',lease_expires_at=least(lease_expires_at,claimed_at),
      heartbeat_at=greatest(heartbeat_at,claimed_at),last_error_code='REQUEST_ATTEMPT_BUDGET_EXCEEDED',
      last_error_message='maximum Content AgentRun attempts exhausted',last_error_at=claimed_at,
      progress_phase='failed',progress_sequence=progress_sequence+1,updated_at=claimed_at
    WHERE request_artifact_id=p_request_artifact_id AND attempt=request_value.current_attempt
      AND status IN ('retry_yielded','superseded');
    UPDATE bid_async_request_snapshot_artifacts SET status='failed',
      error_code='REQUEST_ATTEMPT_BUDGET_EXCEEDED',finished_at=claimed_at
    WHERE id=p_request_artifact_id AND status='pending';
    RETURN jsonb_build_object('disposition','exhausted');
  END IF;
  next_attempt:=request_value.current_attempt+1; token:=gen_random_uuid();
  UPDATE bid_async_request_snapshot_artifacts SET current_attempt=next_attempt WHERE id=p_request_artifact_id;
  INSERT INTO bid_content_agent_run_artifacts(request_artifact_id,frozen_input_sha256,attempt,status,
    execution_owner_token,lease_acquired_at,lease_expires_at,heartbeat_at,hard_deadline_at,
    progress_phase,progress_detail,progress_sequence,started_at,updated_at)
  VALUES(p_request_artifact_id,p_frozen_input_sha256,next_attempt,'running',token,
    claimed_at,claimed_at+interval '30 seconds',claimed_at,claimed_at+interval '46 minutes',
    'retrieving',jsonb_build_object('phase','retrieving','attempt',next_attempt,
      'max_attempts',request_value.max_run_attempts),1,claimed_at,claimed_at);
  RETURN jsonb_build_object('disposition','claimed','attempt',next_attempt,
    'execution_owner_token',token,'lease_expires_at',claimed_at+interval '30 seconds',
    'heartbeat_at',claimed_at,'hard_deadline_at',claimed_at+interval '46 minutes',
    'max_attempts',request_value.max_run_attempts);
END $$;

CREATE FUNCTION kb_bid_v2_content_run_heartbeat(
  p_request_artifact_id uuid,p_request_revision bigint,p_frozen_input_sha256 kb_sha256,
  p_attempt integer,p_execution_owner_token uuid
) RETURNS timestamptz LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE heartbeat_time timestamptz; next_expiry timestamptz; deadline timestamptz;
BEGIN
  heartbeat_time:=kb_bid_v2_content_lock_owner(p_request_artifact_id,p_request_revision,
    p_frozen_input_sha256,p_attempt,p_execution_owner_token);
  SELECT hard_deadline_at INTO STRICT deadline FROM bid_content_agent_run_artifacts
    WHERE request_artifact_id=p_request_artifact_id AND attempt=p_attempt;
  next_expiry:=least(heartbeat_time+interval '30 seconds',deadline);
  UPDATE bid_content_agent_run_artifacts SET heartbeat_at=heartbeat_time,
    lease_expires_at=next_expiry,updated_at=heartbeat_time
  WHERE request_artifact_id=p_request_artifact_id AND attempt=p_attempt;
  RETURN next_expiry;
END $$;

CREATE FUNCTION kb_bid_v2_content_run_progress(
  p_request_artifact_id uuid,p_request_revision bigint,p_frozen_input_sha256 kb_sha256,
  p_attempt integer,p_execution_owner_token uuid,p_phase text,p_detail jsonb
) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE now_value timestamptz;
BEGIN
  IF p_phase NOT IN ('retrieving','generating','validating','publishing','retrying')
     OR jsonb_typeof(p_detail)<>'object' THEN
    RAISE EXCEPTION 'invalid Content AgentRun progress' USING ERRCODE='22023';
  END IF;
  now_value:=kb_bid_v2_content_lock_owner(p_request_artifact_id,p_request_revision,
    p_frozen_input_sha256,p_attempt,p_execution_owner_token);
  UPDATE bid_content_agent_run_artifacts SET progress_phase=p_phase,progress_detail=p_detail,
    progress_sequence=progress_sequence+1,updated_at=now_value
  WHERE request_artifact_id=p_request_artifact_id AND attempt=p_attempt;
END $$;

CREATE FUNCTION kb_bid_v2_content_run_yield_for_retry(
  p_request_artifact_id uuid,p_request_revision bigint,p_frozen_input_sha256 kb_sha256,
  p_attempt integer,p_execution_owner_token uuid,p_error_code text,p_error_message text
) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE now_value timestamptz;
BEGIN
  IF p_error_code<>'INTERNAL' THEN RAISE EXCEPTION 'invalid retry-yield error code' USING ERRCODE='22023'; END IF;
  now_value:=kb_bid_v2_content_lock_owner(p_request_artifact_id,p_request_revision,
    p_frozen_input_sha256,p_attempt,p_execution_owner_token);
  UPDATE bid_content_agent_run_artifacts SET status='retry_yielded',lease_expires_at=now_value,
    heartbeat_at=now_value,last_error_code=p_error_code,
    last_error_message=kb_bid_v2_diagnostic_prefix(coalesce(p_error_message,'')),last_error_at=now_value,
    progress_phase='retrying',progress_detail=jsonb_build_object('phase','retrying','last_error_code',p_error_code),
    progress_sequence=progress_sequence+1,updated_at=now_value
  WHERE request_artifact_id=p_request_artifact_id AND attempt=p_attempt;
END $$;

CREATE FUNCTION kb_bid_v2_content_boundary_attempt_claim(
  p_request_artifact_id uuid,p_request_revision bigint,p_frozen_input_sha256 kb_sha256,
  p_input_sha256 kb_sha256,p_prompt_contract_id uuid,p_prompt_contract_sha256 kb_sha256,
  p_prompt_sha256 kb_sha256,p_schema_contract_id text,p_schema_contract_sha256 kb_sha256,
  p_agent_contract_id uuid,p_agent_contract_sha256 kb_sha256,
  p_model_contract_id uuid,p_model_contract_sha256 kb_sha256,p_runtime_contract_sha256 kb_sha256,
  p_attempt integer,p_execution_owner_token uuid
) RETURNS integer LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE now_value timestamptz; next_ordinal integer; typed bid_content_generation_request_identities%ROWTYPE;
BEGIN
  now_value:=kb_bid_v2_content_lock_owner(p_request_artifact_id,p_request_revision,
    p_frozen_input_sha256,p_attempt,p_execution_owner_token);
  SELECT * INTO STRICT typed FROM bid_content_generation_request_identities
    WHERE request_artifact_id=p_request_artifact_id AND request_operation='generate';
  IF NOT EXISTS (SELECT 1 FROM bid_content_agent_input_artifacts artifact
       WHERE artifact.request_artifact_id=p_request_artifact_id
         AND artifact.frozen_input_sha256=p_frozen_input_sha256
         AND artifact.input_sha256=p_input_sha256) THEN
    RAISE EXCEPTION 'CONTENT_DIVERGENT_AGENT_INPUT_REPLAY' USING ERRCODE='23514';
  END IF;
  IF typed.prompt_contract_id<>p_prompt_contract_id OR typed.prompt_contract_sha256<>p_prompt_contract_sha256
     OR typed.prompt_sha256<>p_prompt_sha256 OR typed.output_schema_id<>p_schema_contract_id
     OR typed.output_schema_sha256<>p_schema_contract_sha256
     OR typed.agent_contract_id<>p_agent_contract_id OR typed.agent_contract_sha256<>p_agent_contract_sha256
     OR typed.model_contract_id<>p_model_contract_id OR typed.model_contract_sha256<>p_model_contract_sha256
     OR typed.runtime_contract_sha256<>p_runtime_contract_sha256 THEN
    RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH' USING ERRCODE='23514';
  END IF;
  SELECT coalesce(max(call_ordinal),0)+1 INTO next_ordinal
  FROM bid_content_agent_boundary_attempts
  WHERE request_artifact_id=p_request_artifact_id AND frozen_input_sha256=p_frozen_input_sha256
    AND stage_kind='content_generate' AND batch_ordinal=0;
  IF next_ordinal>3 THEN RAISE EXCEPTION 'AGENT_TURN_BUDGET_EXCEEDED' USING ERRCODE='23514'; END IF;
  INSERT INTO bid_content_agent_boundary_attempts(request_artifact_id,frozen_input_sha256,
    request_operation,stage_kind,batch_ordinal,input_sha256,prompt_contract_id,prompt_contract_sha256,prompt_sha256,
    schema_contract_id,schema_contract_sha256,agent_contract_id,agent_contract_sha256,
    model_contract_id,model_contract_sha256,runtime_contract_sha256,call_ordinal,reserved_at)
  VALUES(p_request_artifact_id,p_frozen_input_sha256,'generate','content_generate',0,p_input_sha256,
    p_prompt_contract_id,p_prompt_contract_sha256,p_prompt_sha256,p_schema_contract_id,
    p_schema_contract_sha256,p_agent_contract_id,p_agent_contract_sha256,p_model_contract_id,
    p_model_contract_sha256,p_runtime_contract_sha256,next_ordinal,now_value);
  RETURN next_ordinal;
END $$;

CREATE FUNCTION kb_bid_v2_content_match_lock(
  p_request_artifact_id uuid,p_request_revision bigint,p_frozen_input_sha256 kb_sha256
) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE request_value bid_async_request_snapshot_artifacts%ROWTYPE;
BEGIN
  SELECT * INTO request_value FROM bid_async_request_snapshot_artifacts WHERE id=p_request_artifact_id FOR UPDATE;
  IF request_value.id IS NULL OR request_value.request_kind<>'content_generate'
     OR request_value.revision<>p_request_revision OR request_value.frozen_input_sha256<>p_frozen_input_sha256
     OR request_value.status<>'pending'
     OR NOT EXISTS (SELECT 1 FROM bid_content_generation_request_identities typed
       WHERE typed.request_artifact_id=p_request_artifact_id AND typed.request_operation='match_only')
     OR EXISTS (SELECT 1 FROM bid_content_agent_run_artifacts run
       WHERE run.request_artifact_id=p_request_artifact_id) THEN
    RAISE EXCEPTION 'REQUEST_OBSOLETE' USING ERRCODE='P0002';
  END IF;
END $$;

CREATE FUNCTION kb_bid_v2_authoring_job_payload(p_request_artifact_id uuid)
RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE request_value bid_async_request_snapshot_artifacts%ROWTYPE; payload jsonb; payload_bytes bytea;
BEGIN
  SELECT * INTO STRICT request_value FROM bid_async_request_snapshot_artifacts
    WHERE id=p_request_artifact_id;
  CASE request_value.request_kind
    WHEN 'tender_document_process' THEN
      SELECT jsonb_build_object('job_kind','tender_document_process','request',jsonb_build_object(
        'request_artifact_id',typed.request_artifact_id,'request_revision',typed.request_revision,
        'frozen_input_sha256',typed.frozen_input_sha256),'project_id',typed.project_id,
        'document_revision_id',typed.document_id) INTO STRICT payload
      FROM bid_tender_document_process_request_identities typed
      WHERE typed.request_artifact_id=request_value.id AND typed.request_revision=request_value.revision
        AND typed.request_sha256=request_value.request_sha256
        AND typed.frozen_input_sha256=request_value.frozen_input_sha256;
    WHEN 'requirement_set_compile' THEN
      SELECT jsonb_build_object('job_kind','requirement_set_compile','request',jsonb_build_object(
        'request_artifact_id',typed.request_artifact_id,'request_revision',typed.request_revision,
        'frozen_input_sha256',typed.frozen_input_sha256),'project_id',typed.project_id,
        'document_set_revision_id',typed.document_set_revision_id,
        'disposition_set_revision_id',typed.disposition_set_revision_id) INTO STRICT payload
      FROM bid_requirement_set_compile_request_identities typed
      WHERE typed.request_artifact_id=request_value.id AND typed.request_revision=request_value.revision
        AND typed.request_sha256=request_value.request_sha256
        AND typed.frozen_input_sha256=request_value.frozen_input_sha256;
    WHEN 'docx_compose' THEN
      SELECT jsonb_build_object('job_kind','docx_compose','request',jsonb_build_object(
        'request_artifact_id',typed.request_artifact_id,'request_revision',typed.request_revision,
        'frozen_input_sha256',typed.frozen_input_sha256),'project_id',typed.project_id,
        'workspace_id',typed.workspace_id) INTO STRICT payload
      FROM bid_docx_composition_request_identities typed
      WHERE typed.request_artifact_id=request_value.id AND typed.request_revision=request_value.revision
        AND typed.request_sha256=request_value.request_sha256
        AND typed.frozen_input_sha256=request_value.frozen_input_sha256;
    WHEN 'content_generate' THEN
      SELECT jsonb_build_object('job_kind','content_generate','request',jsonb_build_object(
        'request_artifact_id',typed.request_artifact_id,'request_revision',typed.request_revision,
        'frozen_input_sha256',typed.frozen_input_sha256),'project_id',typed.project_id,
        'workspace_id',typed.workspace_id,'base_workspace_revision_id',typed.base_workspace_revision_id,
        'operation',typed.request_operation) INTO STRICT payload
      FROM bid_content_generation_request_identities typed
      WHERE typed.request_artifact_id=request_value.id AND typed.request_revision=request_value.revision
        AND typed.request_sha256=request_value.request_sha256
        AND typed.frozen_input_sha256=request_value.frozen_input_sha256;
    WHEN 'submission_export' THEN
      SELECT jsonb_build_object('job_kind','submission_export','request',jsonb_build_object(
        'request_artifact_id',typed.request_artifact_id,'request_revision',typed.request_revision,
        'frozen_input_sha256',typed.frozen_input_sha256),'project_id',typed.project_id,
        'workspace_id',typed.workspace_id) INTO STRICT payload
      FROM bid_submission_export_request_identities typed
      WHERE typed.request_artifact_id=request_value.id AND typed.request_revision=request_value.revision
        AND typed.request_sha256=request_value.request_sha256
        AND typed.frozen_input_sha256=request_value.frozen_input_sha256;
    ELSE RAISE EXCEPTION 'invalid Request job kind' USING ERRCODE='23514';
  END CASE;
  payload_bytes:=kb_bid_v2_json_payload(payload);
  IF request_value.request_payload<>payload_bytes
     OR request_value.request_sha256<>kb_bid_v2_sha256_bytes(payload_bytes) THEN
    RAISE EXCEPTION 'REQUEST_JOB_PAYLOAD_MISMATCH' USING ERRCODE='23514';
  END IF;
  RETURN payload;
END $$;

CREATE FUNCTION kb_bid_v2_load_authoring_job_payload(
  p_request_artifact_id uuid,p_request_revision bigint,p_frozen_input_sha256 kb_sha256
) RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE request_value bid_async_request_snapshot_artifacts%ROWTYPE;
BEGIN
  SELECT * INTO request_value FROM bid_async_request_snapshot_artifacts
    WHERE id=p_request_artifact_id AND revision=p_request_revision
      AND frozen_input_sha256=p_frozen_input_sha256;
  IF NOT FOUND THEN RAISE EXCEPTION 'FROZEN_INPUT_MISSING' USING ERRCODE='P0002'; END IF;
  RETURN jsonb_build_object(
    'request_artifact_id',request_value.id,'request_revision',request_value.revision,
    'frozen_input_sha256',request_value.frozen_input_sha256,
    'request_sha256',request_value.request_sha256,'request_kind',request_value.request_kind,
    'status',request_value.status,'job_payload',kb_bid_v2_authoring_job_payload(request_value.id));
END $$;


-- DOCX rounds are independent from the legacy Workspace body. Starting a round
-- freezes the complete source/requirement basis and owns an initial DOCX object;
-- it never copies old blocks, responses, quotes, assessments or page mappings.
CREATE TABLE bid_docx_round_artifacts (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  revision bigint NOT NULL CHECK (revision>0),
  document_set_id uuid NOT NULL,
  requirement_set_id uuid NOT NULL,
  canonical_payload bytea NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  actor kb_actor_identity NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(workspace_id,revision),
  UNIQUE(project_id,workspace_id,id),
  FOREIGN KEY(project_id,workspace_id) REFERENCES bid_submission_workspaces(project_id,id),
  FOREIGN KEY(project_id,document_set_id) REFERENCES bid_document_set_artifacts(project_id,id),
  FOREIGN KEY(project_id,requirement_set_id) REFERENCES bid_requirement_set_artifacts(project_id,id),
  CHECK (content_sha256=kb_bid_v2_sha256_bytes(canonical_payload))
);

CREATE TABLE bid_docx_version_artifacts (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  round_id uuid NOT NULL,
  revision bigint NOT NULL CHECK (revision>0),
  parent_version_id uuid,
  object_ref kb_object_ref NOT NULL,
  docx_sha256 kb_sha256 NOT NULL,
  byte_length bigint NOT NULL CHECK (byte_length>0),
  actor kb_actor_identity NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(round_id,revision),
  UNIQUE(project_id,workspace_id,round_id,id),
  UNIQUE(project_id,workspace_id,round_id,id,docx_sha256),
  FOREIGN KEY(project_id,workspace_id,round_id) REFERENCES bid_docx_round_artifacts(project_id,workspace_id,id),
  FOREIGN KEY(project_id,workspace_id,round_id,parent_version_id)
    REFERENCES bid_docx_version_artifacts(project_id,workspace_id,round_id,id),
  CHECK ((revision=1)=(parent_version_id IS NULL)),
  CHECK (object_ref='objects/'||docx_sha256)
);

ALTER TABLE bid_docx_composition_request_identities
  ADD FOREIGN KEY(project_id,workspace_id,expected_round_id,expected_version_id,expected_docx_sha256)
    REFERENCES bid_docx_version_artifacts(project_id,workspace_id,round_id,id,docx_sha256);

CREATE TABLE bid_docx_current (
  scope_id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  round_id uuid NOT NULL,
  version_id uuid NOT NULL,
  docx_sha256 kb_sha256 NOT NULL,
  editor_key uuid,
  editor_base_version_id uuid,
  pending_save_id uuid,
  editor_error jsonb,
  CHECK ((editor_key IS NULL)=(editor_base_version_id IS NULL)),
  CHECK (editor_key IS NOT NULL OR (pending_save_id IS NULL AND editor_error IS NULL)),
  CHECK (editor_error IS NULL OR (jsonb_typeof(editor_error)='object'
    AND kb_bid_v2_json_keys_exact(editor_error,ARRAY['kind','code'])
    AND editor_error->>'kind' IN ('callback','command') AND jsonb_typeof(editor_error->'code')='number')),
  FOREIGN KEY(project_id,scope_id,round_id,editor_base_version_id)
    REFERENCES bid_docx_version_artifacts(project_id,workspace_id,round_id,id),
  FOREIGN KEY(project_id,scope_id,round_id,version_id,docx_sha256)
    REFERENCES bid_docx_version_artifacts(project_id,workspace_id,round_id,id,docx_sha256)
);

CREATE FUNCTION kb_bid_v2_create_docx_round(
  p_workspace_id uuid,p_staging_id uuid,p_input jsonb,p_actor kb_actor_identity,p_idempotency_key text
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE
  workspace bid_submission_workspaces%ROWTYPE; head bid_docx_current%ROWTYPE;
  document_head bid_document_set_current%ROWTYPE; requirement_head bid_requirement_set_current%ROWTYPE;
  disposition_head bid_source_unit_disposition_set_current%ROWTYPE;
  requirement_set bid_requirement_set_artifacts%ROWTYPE;
  round_id uuid:=gen_random_uuid(); version_id uuid:=gen_random_uuid(); round_revision bigint;
  request_bytes bytea; request_sha kb_sha256; replay bytea;
  payload bytea; payload_sha kb_sha256; response jsonb; response_bytes bytea;
  docx_sha kb_sha256; byte_length_value bigint;
BEGIN
  SELECT * INTO STRICT workspace FROM bid_submission_workspaces WHERE id=p_workspace_id;
  PERFORM kb_bid_v2_require_project_owner(workspace.project_id,p_actor);
  IF jsonb_typeof(p_input) IS DISTINCT FROM 'object'
    OR NOT kb_bid_v2_json_keys_exact(p_input,ARRAY['document_set_id','document_set_sha256',
      'requirement_set_id','requirement_set_sha256','expected_version_id','expected_docx_sha256',
      'docx_sha256','byte_length']) THEN
    RAISE EXCEPTION 'DOCX_ROUND_INPUT_INVALID' USING ERRCODE='23514';
  END IF;
  -- The ephemeral staging UUID is not part of replay identity. A retry may
  -- stage identical bytes again, but may not change its basis, CAS or content.
  request_bytes:=kb_bid_v2_json_payload(jsonb_build_object('workspace_id',p_workspace_id,'input',p_input));
  request_sha:=kb_bid_v2_sha256_bytes(request_bytes);
  replay:=kb_bid_v2_idempotency_begin(p_actor,'bid.v2.docx_round.create',p_idempotency_key,request_bytes,request_sha);
  IF replay IS NOT NULL THEN
    response:=convert_from(replay,'UTF8')::jsonb;
    IF EXISTS (SELECT 1 FROM object_upload_staging WHERE id=p_staging_id) THEN
      PERFORM kb_object_upload_commit(p_staging_id,'objects/'||(p_input->>'docx_sha256'),
        (p_input->>'docx_sha256')::kb_sha256,
        'application/vnd.openxmlformats-officedocument.wordprocessingml.document',(p_input->>'byte_length')::bigint,
        'bid_docx_version',(response->>'version_id')::uuid,'document',p_actor);
    END IF;
    RETURN response;
  END IF;
  -- SHARE prevents project closure but allows the FK KEY SHARE lock taken by
  -- a concurrent collection freeze while it owns the DocumentSet head lock.
  PERFORM 1 FROM bid_projects WHERE id=workspace.project_id AND status='open' FOR SHARE;
  IF NOT FOUND THEN RAISE EXCEPTION 'PROJECT_ENDED' USING ERRCODE='55000'; END IF;
  -- Serializes the first creation too, when no DOCX current row exists yet.
  PERFORM 1 FROM bid_submission_workspaces WHERE id=p_workspace_id FOR UPDATE;
  SELECT * INTO head FROM bid_docx_current WHERE scope_id=p_workspace_id FOR UPDATE;
  IF head.version_id IS DISTINCT FROM (p_input->>'expected_version_id')::uuid
    OR head.docx_sha256 IS DISTINCT FROM p_input->>'expected_docx_sha256' THEN
    RAISE EXCEPTION 'DOCX_VERSION_CAS_MISMATCH' USING ERRCODE='40001';
  END IF;
  SELECT * INTO STRICT document_head FROM bid_document_set_current WHERE scope_id=workspace.project_id FOR SHARE;
  SELECT * INTO STRICT disposition_head FROM bid_source_unit_disposition_set_current WHERE scope_id=workspace.project_id FOR SHARE;
  SELECT * INTO STRICT requirement_head FROM bid_requirement_set_current WHERE scope_id=workspace.project_id FOR SHARE;
  IF document_head.artifact_id IS DISTINCT FROM (p_input->>'document_set_id')::uuid
    OR document_head.artifact_sha256 IS DISTINCT FROM p_input->>'document_set_sha256'
    OR requirement_head.artifact_id IS DISTINCT FROM (p_input->>'requirement_set_id')::uuid
    OR requirement_head.artifact_sha256 IS DISTINCT FROM p_input->>'requirement_set_sha256' THEN
    RAISE EXCEPTION 'DOCX_ROUND_BASIS_CHANGED' USING ERRCODE='40001';
  END IF;
  SELECT * INTO STRICT requirement_set FROM bid_requirement_set_artifacts WHERE id=requirement_head.artifact_id;
  IF requirement_set.document_set_id IS DISTINCT FROM document_head.artifact_id
    OR requirement_set.disposition_set_id IS DISTINCT FROM disposition_head.artifact_id THEN
    RAISE EXCEPTION 'DOCX_ROUND_REQUIREMENTS_NOT_CURRENT' USING ERRCODE='40001';
  END IF;
  docx_sha:=(p_input->>'docx_sha256')::kb_sha256;
  byte_length_value:=(p_input->>'byte_length')::bigint;
  IF docx_sha IS NULL OR byte_length_value IS NULL OR byte_length_value<=0 THEN
    RAISE EXCEPTION 'DOCX_ROUND_INPUT_INVALID' USING ERRCODE='23514';
  END IF;
  SELECT coalesce(max(revision),0)+1 INTO round_revision FROM bid_docx_round_artifacts WHERE workspace_id=p_workspace_id;
  payload:=kb_bid_v2_json_payload(jsonb_build_object('schema_version',1,
    'project_id',workspace.project_id,'workspace_id',p_workspace_id,'revision',round_revision,
    'document_set_id',document_head.artifact_id,'document_set_sha256',document_head.artifact_sha256,
    'requirement_set_id',requirement_head.artifact_id,'requirement_set_sha256',requirement_head.artifact_sha256));
  payload_sha:=kb_bid_v2_sha256_bytes(payload);
  PERFORM kb_object_upload_commit(p_staging_id,'objects/'||docx_sha,docx_sha,
    'application/vnd.openxmlformats-officedocument.wordprocessingml.document',byte_length_value,
    'bid_docx_version',version_id,'document',p_actor);
  INSERT INTO bid_docx_round_artifacts(id,project_id,workspace_id,revision,document_set_id,
    requirement_set_id,canonical_payload,content_sha256,actor)
  VALUES(round_id,workspace.project_id,p_workspace_id,round_revision,document_head.artifact_id,
    requirement_head.artifact_id,payload,payload_sha,p_actor);
  INSERT INTO bid_docx_version_artifacts(id,project_id,workspace_id,round_id,revision,parent_version_id,
    object_ref,docx_sha256,byte_length,actor)
  VALUES(version_id,workspace.project_id,p_workspace_id,round_id,1,NULL,'objects/'||docx_sha,docx_sha,byte_length_value,p_actor);
  INSERT INTO bid_docx_current(scope_id,project_id,round_id,version_id,docx_sha256)
  VALUES(p_workspace_id,workspace.project_id,round_id,version_id,docx_sha)
  ON CONFLICT(scope_id) DO UPDATE SET round_id=EXCLUDED.round_id,version_id=EXCLUDED.version_id,docx_sha256=EXCLUDED.docx_sha256,
    editor_key=NULL,editor_base_version_id=NULL,pending_save_id=NULL,editor_error=NULL;
  response:=jsonb_build_object('round_id',round_id,'round_revision',round_revision,'version_id',version_id,
    'docx_sha256',docx_sha,'byte_length',byte_length_value,'document_set_id',document_head.artifact_id,
    'requirement_set_id',requirement_head.artifact_id);
  response_bytes:=convert_to(response::text,'UTF8');
  INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,
    response_sha256,entity_kind,entity_locator,after_revision,after_sha256)
  VALUES(gen_random_uuid(),1,'bid.v2.docx_round.create',p_actor,p_idempotency_key,request_sha,
    kb_bid_v2_sha256_bytes(response_bytes),'bid_v2_docx_round',jsonb_build_object('project_id',workspace.project_id,
      'workspace_id',p_workspace_id,'round_id',round_id),round_revision,payload_sha);
  PERFORM kb_bid_v2_idempotency_complete(p_actor,'bid.v2.docx_round.create',p_idempotency_key,201,response_bytes);
  RETURN response;
END $$;

CREATE FUNCTION kb_bid_v2_get_docx_version(p_workspace_id uuid,p_version_id uuid,p_actor kb_actor_identity)
RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE project_id_value uuid; value jsonb;
BEGIN
  SELECT project_id INTO STRICT project_id_value FROM bid_submission_workspaces WHERE id=p_workspace_id;
  PERFORM kb_bid_v2_require_project_owner(project_id_value,p_actor);
  SELECT jsonb_build_object('project_id',version.project_id,'workspace_id',version.workspace_id,
    'round_id',version.round_id,'version_id',version.id,'revision',version.revision,
    'object_ref',version.object_ref,'docx_sha256',version.docx_sha256,'byte_length',version.byte_length,
    'round_basis',convert_from(round_value.canonical_payload,'UTF8')::jsonb)
    INTO value FROM bid_docx_version_artifacts version
    JOIN bid_docx_round_artifacts round_value ON round_value.id=version.round_id
    WHERE version.workspace_id=p_workspace_id AND version.id=p_version_id;
  IF value IS NULL THEN RAISE EXCEPTION 'DOCX_VERSION_NOT_FOUND' USING ERRCODE='P0002'; END IF;
  RETURN value;
END $$;

-- Read the authoritative heads in one snapshot, including an empty requirement
-- set. Historical lists and a legacy Workspace projection are not round input.
CREATE FUNCTION kb_bid_v2_get_docx_round_basis(p_workspace_id uuid,p_actor kb_actor_identity)
RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE project_value uuid; result_value jsonb;
BEGIN
  SELECT project_id INTO STRICT project_value FROM bid_submission_workspaces WHERE id=p_workspace_id;
  PERFORM kb_bid_v2_require_project_owner(project_value,p_actor);
  SELECT jsonb_build_object('document_set_id',d.artifact_id,'document_set_sha256',d.artifact_sha256,
    'requirement_set_id',r.artifact_id,'requirement_set_sha256',r.artifact_sha256)
    INTO result_value FROM bid_document_set_current d
    JOIN bid_requirement_set_current r ON r.scope_id=d.scope_id
    JOIN bid_source_unit_disposition_set_current s ON s.scope_id=d.scope_id
    JOIN bid_requirement_set_artifacts a ON a.id=r.artifact_id
      AND a.document_set_id=d.artifact_id AND a.disposition_set_id=s.artifact_id
    WHERE d.scope_id=project_value;
  RETURN result_value;
END $$;

CREATE FUNCTION kb_bid_v2_get_current_docx(p_workspace_id uuid,p_actor kb_actor_identity)
RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE project_id_value uuid; head bid_docx_current%ROWTYPE;
BEGIN
  SELECT project_id INTO STRICT project_id_value FROM bid_submission_workspaces WHERE id=p_workspace_id;
  PERFORM kb_bid_v2_require_project_owner(project_id_value,p_actor);
  SELECT * INTO head FROM bid_docx_current WHERE scope_id=p_workspace_id;
  IF NOT FOUND THEN RETURN NULL; END IF;
  RETURN kb_bid_v2_get_docx_version(p_workspace_id,head.version_id,p_actor)||jsonb_build_object('editor',
    jsonb_build_object('key',head.editor_key,'base_version_id',head.editor_base_version_id,
      'pending_save_id',head.pending_save_id,'save_error',head.editor_error));
END $$;

-- Session state belongs to the existing current pointer. Historical documents,
-- operation receipts and audit remain in their existing stores.
CREATE FUNCTION kb_bid_v2_get_docx_editor(p_workspace_id uuid,p_editor_key uuid,p_actor kb_actor_identity)
RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE project_value uuid; head bid_docx_current%ROWTYPE;
BEGIN
  SELECT project_id INTO STRICT project_value FROM bid_submission_workspaces WHERE id=p_workspace_id;
  PERFORM kb_bid_v2_require_project_owner(project_value,p_actor);
  SELECT * INTO STRICT head FROM bid_docx_current WHERE scope_id=p_workspace_id;
  IF p_editor_key IS NULL OR head.editor_key IS DISTINCT FROM p_editor_key THEN
    RAISE EXCEPTION 'DOCX_EDITOR_STALE' USING ERRCODE='40001';
  END IF;
  RETURN jsonb_build_object('project_id',head.project_id,'workspace_id',head.scope_id,'round_id',head.round_id,
    'editor_key',head.editor_key,'base',kb_bid_v2_get_docx_version(p_workspace_id,head.editor_base_version_id,p_actor),
    'version_id',head.version_id,'docx_sha256',head.docx_sha256,
    'pending_save_id',head.pending_save_id,'save_error',head.editor_error);
END $$;

-- Read the immutable publication receipt for one forcesave. Current-version
-- advancement alone is not evidence that this particular save succeeded.
CREATE FUNCTION kb_bid_v2_get_docx_saved_receipt(
  p_workspace_id uuid,p_editor_key uuid,p_save_id uuid,p_actor kb_actor_identity
) RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE project_value uuid; saved idempotency_requests%ROWTYPE;
  request_value jsonb; response_value jsonb; version_value bid_docx_version_artifacts%ROWTYPE;
BEGIN
  SELECT project_id INTO STRICT project_value FROM bid_submission_workspaces WHERE id=p_workspace_id;
  PERFORM kb_bid_v2_require_project_owner(project_value,p_actor);
  IF p_editor_key IS NULL OR p_save_id IS NULL THEN
    RAISE EXCEPTION 'DOCX_EDITOR_INPUT_INVALID' USING ERRCODE='23514';
  END IF;
  SELECT * INTO saved FROM idempotency_requests WHERE actor_identity=p_actor
    AND operation='bid.v2.docx_editor.save'
    AND idempotency_key=p_editor_key::text||':'||p_save_id::text AND state='completed' AND response_status=200;
  IF NOT FOUND THEN RETURN NULL; END IF;
  request_value:=convert_from(saved.request_bytes,'UTF8')::jsonb;
  IF request_value->>'workspace_id' IS DISTINCT FROM p_workspace_id::text
    OR request_value#>>'{input,editor_key}' IS DISTINCT FROM p_editor_key::text
    OR request_value#>>'{input,save_id}' IS DISTINCT FROM p_save_id::text
    OR request_value#>'{input,final}' IS DISTINCT FROM 'false'::jsonb THEN
    RETURN NULL;
  END IF;
  response_value:=convert_from(saved.response_bytes,'UTF8')::jsonb;
  SELECT * INTO STRICT version_value FROM bid_docx_version_artifacts
    WHERE id=(response_value->>'version_id')::uuid AND workspace_id=p_workspace_id
      AND project_id=project_value AND round_id=(response_value->>'round_id')::uuid
      AND docx_sha256=response_value->>'docx_sha256'
      AND parent_version_id=(response_value->>'parent_version_id')::uuid
      AND revision=(response_value->>'revision')::bigint
      AND byte_length=(response_value->>'byte_length')::bigint;
  RETURN jsonb_build_object('save_id',p_save_id,'editor_key',p_editor_key,
    'version_id',version_value.id,'docx_sha256',version_value.docx_sha256,
    'round_id',version_value.round_id,'parent_version_id',version_value.parent_version_id,
    'revision',version_value.revision);
END $$;

-- A closed DOCX-specific command set, not a generic workflow dispatcher.
-- Open/request_save are called by authorized users; save/status only after the
-- API has verified its scoped callback capability and Document Server signature.
CREATE FUNCTION kb_bid_v2_docx_editor_command(
  p_workspace_id uuid,p_action text,p_input jsonb,p_actor kb_actor_identity,p_idempotency_key text
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE
  project_value uuid; head bid_docx_current%ROWTYPE; fields text[];
  request_bytes bytea; request_sha kb_sha256; replay bytea; response jsonb; response_bytes bytea;
  operation_value text; key_value uuid; save_value uuid; version_value uuid; revision_value bigint;
  sha_value kb_sha256; length_value bigint; final_value boolean; code_value integer; kind_value text;
BEGIN
  SELECT project_id INTO STRICT project_value FROM bid_submission_workspaces WHERE id=p_workspace_id;
  PERFORM kb_bid_v2_require_project_owner(project_value,p_actor);
  fields:=CASE p_action
    WHEN 'open' THEN ARRAY['expected_version_id','expected_docx_sha256']
    WHEN 'request_save' THEN ARRAY['editor_key','expected_version_id','expected_docx_sha256']
    WHEN 'save' THEN ARRAY['editor_key','save_id','final','callback_sha256','docx_sha256','byte_length','staging_id']
    WHEN 'status' THEN ARRAY['editor_key','save_id','kind','code'] ELSE NULL END;
  IF fields IS NULL OR jsonb_typeof(p_input) IS DISTINCT FROM 'object'
    OR NOT kb_bid_v2_json_keys_exact(p_input,fields) THEN
    RAISE EXCEPTION 'DOCX_EDITOR_INPUT_INVALID' USING ERRCODE='23514';
  END IF;
  IF p_action IN ('open','request_save') AND
    ((p_input->>'expected_version_id') IS NULL OR (p_input->>'expected_docx_sha256') IS NULL) THEN
    RAISE EXCEPTION 'DOCX_EDITOR_INPUT_INVALID' USING ERRCODE='23514';
  END IF;
  IF p_action<>'open' THEN
    key_value:=(p_input->>'editor_key')::uuid;
    IF key_value IS NULL THEN RAISE EXCEPTION 'DOCX_EDITOR_INPUT_INVALID' USING ERRCODE='23514'; END IF;
  END IF;
  IF p_action='save' THEN
    IF jsonb_typeof(p_input->'final') IS DISTINCT FROM 'boolean' THEN
      RAISE EXCEPTION 'DOCX_EDITOR_INPUT_INVALID' USING ERRCODE='23514';
    END IF;
    final_value:=(p_input->>'final')::boolean; save_value:=(p_input->>'save_id')::uuid;
    sha_value:=(p_input->>'docx_sha256')::kb_sha256; length_value:=(p_input->>'byte_length')::bigint;
    IF jsonb_typeof(p_input->'callback_sha256') IS DISTINCT FROM 'string'
      OR (p_input->>'callback_sha256')::kb_sha256 IS NULL THEN
      RAISE EXCEPTION 'DOCX_EDITOR_INPUT_INVALID' USING ERRCODE='23514';
    END IF;
    IF sha_value IS NULL OR length_value IS NULL OR length_value<=0
      OR (final_value AND save_value IS NOT NULL) OR (NOT final_value AND save_value IS NULL)
      OR (p_input->>'staging_id') IS NULL THEN
      RAISE EXCEPTION 'DOCX_EDITOR_INPUT_INVALID' USING ERRCODE='23514';
    END IF;
  END IF;
  IF p_action='status' THEN
    kind_value:=p_input->>'kind'; code_value:=(p_input->>'code')::integer;
    save_value:=(p_input->>'save_id')::uuid;
    IF kind_value IS NULL OR code_value IS NULL
      OR kind_value NOT IN ('callback','command')
      OR (kind_value='callback' AND (code_value NOT IN (1,3,4,7)
        OR (code_value=7)<>(save_value IS NOT NULL)))
      OR (kind_value='command' AND (code_value<=0 OR save_value IS NULL)) THEN
      RAISE EXCEPTION 'DOCX_EDITOR_INPUT_INVALID' USING ERRCODE='23514';
    END IF;
  END IF;
  operation_value:='bid.v2.docx_editor.'||p_action;
  -- The authenticated notification is the retry identity. Its first successful
  -- publication binds immutable bytes in the response. A cache URL need not
  -- remain available merely to acknowledge that same notification again.
  request_bytes:=kb_bid_v2_json_payload(jsonb_build_object('workspace_id',p_workspace_id,
    'input',CASE WHEN p_action='save' THEN p_input-ARRAY['staging_id','docx_sha256','byte_length']
      ELSE p_input-'staging_id' END));
  request_sha:=kb_bid_v2_sha256_bytes(request_bytes);
  replay:=kb_bid_v2_idempotency_begin(p_actor,operation_value,p_idempotency_key,request_bytes,request_sha);
  -- Same lock order as new-round publication. No network operation holds these locks.
  PERFORM 1 FROM bid_projects WHERE id=project_value AND status='open' FOR SHARE;
  IF NOT FOUND THEN RAISE EXCEPTION 'PROJECT_ENDED' USING ERRCODE='55000'; END IF;
  PERFORM 1 FROM bid_submission_workspaces WHERE id=p_workspace_id FOR UPDATE;
  SELECT * INTO STRICT head FROM bid_docx_current WHERE scope_id=p_workspace_id FOR UPDATE;
  IF replay IS NOT NULL THEN
    response:=convert_from(replay,'UTF8')::jsonb;
    IF p_action='open' AND head.editor_key IS DISTINCT FROM (response->>'editor_key')::uuid THEN
      RAISE EXCEPTION 'DOCX_EDITOR_STALE' USING ERRCODE='40001';
    END IF;
    -- Concurrent first deliveries can both fetch before either publishes. Never
    -- attach different downloaded bytes to the winner's immutable version.
    IF p_action='save' AND (response->>'docx_sha256' IS DISTINCT FROM sha_value::text
      OR (response->>'byte_length')::bigint IS DISTINCT FROM length_value) THEN
      RAISE EXCEPTION 'IDEMPOTENCY_PAYLOAD_MISMATCH' USING ERRCODE='23505';
    END IF;
    IF p_action='save' AND EXISTS(SELECT 1 FROM object_upload_staging WHERE id=(p_input->>'staging_id')::uuid) THEN
      PERFORM kb_object_upload_commit((p_input->>'staging_id')::uuid,'objects/'||sha_value,sha_value,
        'application/vnd.openxmlformats-officedocument.wordprocessingml.document',length_value,
        'bid_docx_version',(response->>'version_id')::uuid,'document',p_actor);
    END IF;
    IF p_action='request_save' THEN
      RETURN response||jsonb_build_object('dispatch',false,'pending',head.pending_save_id IS NOT DISTINCT FROM (response->>'save_id')::uuid);
    END IF;
    RETURN response;
  END IF;
  IF p_action<>'open' AND head.editor_key IS DISTINCT FROM key_value THEN
    RAISE EXCEPTION 'DOCX_EDITOR_STALE' USING ERRCODE='40001';
  END IF;
  IF p_action IN ('open','request_save') AND (head.version_id IS DISTINCT FROM (p_input->>'expected_version_id')::uuid
    OR head.docx_sha256 IS DISTINCT FROM p_input->>'expected_docx_sha256') THEN
    RAISE EXCEPTION 'DOCX_VERSION_CAS_MISMATCH' USING ERRCODE='40001';
  END IF;
  CASE p_action
    WHEN 'open' THEN
      IF head.editor_key IS NULL THEN
        UPDATE bid_docx_current SET editor_key=gen_random_uuid(),editor_base_version_id=version_id
          WHERE scope_id=p_workspace_id RETURNING * INTO head;
      END IF;
      response:=jsonb_build_object('editor_key',head.editor_key,'round_id',head.round_id,
        'base_version_id',head.editor_base_version_id);
    WHEN 'request_save' THEN
      IF head.pending_save_id IS NOT NULL THEN
        RAISE EXCEPTION 'DOCX_SAVE_PENDING' USING ERRCODE='40001';
      END IF;
      save_value:=gen_random_uuid();
      UPDATE bid_docx_current SET pending_save_id=save_value WHERE scope_id=p_workspace_id;
      response:=jsonb_build_object('editor_key',head.editor_key,'save_id',save_value);
    WHEN 'save' THEN
      IF NOT final_value AND head.pending_save_id IS DISTINCT FROM save_value THEN
        RAISE EXCEPTION 'DOCX_SAVE_CORRELATION_MISMATCH' USING ERRCODE='40001';
      END IF;
      SELECT revision+1 INTO STRICT revision_value FROM bid_docx_version_artifacts WHERE id=head.version_id;
      version_value:=gen_random_uuid();
      PERFORM kb_object_upload_commit((p_input->>'staging_id')::uuid,'objects/'||sha_value,sha_value,
        'application/vnd.openxmlformats-officedocument.wordprocessingml.document',length_value,
        'bid_docx_version',version_value,'document',p_actor);
      INSERT INTO bid_docx_version_artifacts(id,project_id,workspace_id,round_id,revision,parent_version_id,
        object_ref,docx_sha256,byte_length,actor)
      VALUES(version_value,head.project_id,p_workspace_id,head.round_id,revision_value,head.version_id,
        'objects/'||sha_value,sha_value,length_value,p_actor);
      UPDATE bid_docx_current SET version_id=version_value,docx_sha256=sha_value,pending_save_id=NULL,editor_error=NULL,
        editor_key=CASE WHEN final_value THEN NULL ELSE editor_key END,
        editor_base_version_id=CASE WHEN final_value THEN NULL ELSE editor_base_version_id END
        WHERE scope_id=p_workspace_id;
      response:=jsonb_build_object('version_id',version_value,'docx_sha256',sha_value,'round_id',head.round_id,
        'parent_version_id',head.version_id,'revision',revision_value,'final',final_value,'byte_length',length_value);
    WHEN 'status' THEN
      IF kind_value='command' OR code_value=7 THEN
        IF head.pending_save_id IS DISTINCT FROM save_value THEN
          RAISE EXCEPTION 'DOCX_SAVE_CORRELATION_MISMATCH' USING ERRCODE='40001';
        END IF;
        UPDATE bid_docx_current SET pending_save_id=NULL,editor_error=jsonb_build_object('kind',kind_value,'code',code_value)
          WHERE scope_id=p_workspace_id;
      ELSIF code_value=3 THEN
        UPDATE bid_docx_current SET editor_error=jsonb_build_object('kind',kind_value,'code',code_value)
          WHERE scope_id=p_workspace_id;
      END IF;
      -- 1/4 are presence notifications. They never publish, clear errors or rotate a key.
      response:=jsonb_build_object('saved',false,'kind',kind_value,'code',code_value);
  END CASE;
  response_bytes:=convert_to(response::text,'UTF8');
  INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
    entity_kind,entity_locator,after_revision,after_sha256)
  VALUES(gen_random_uuid(),1,operation_value,p_actor,p_idempotency_key,request_sha,kb_bid_v2_sha256_bytes(response_bytes),
    'bid_v2_docx_editor',jsonb_build_object('project_id',head.project_id,'workspace_id',p_workspace_id,
      'round_id',head.round_id,'editor_key',coalesce(key_value,head.editor_key)),
    coalesce(revision_value,1),kb_bid_v2_sha256_bytes(response_bytes));
  PERFORM kb_bid_v2_idempotency_complete(p_actor,operation_value,p_idempotency_key,200,response_bytes);
  IF p_action='request_save' THEN RETURN response||jsonb_build_object('dispatch',true,'pending',true); END IF;
  RETURN response;
END $$;

CREATE FUNCTION kb_bid_v2_replay_docx_editor_save(p_workspace_id uuid,p_input jsonb,p_actor kb_actor_identity,p_key text)
RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE project_value uuid; request_bytes bytea;
BEGIN
  SELECT project_id INTO STRICT project_value FROM bid_submission_workspaces WHERE id=p_workspace_id;
  PERFORM kb_bid_v2_require_project_owner(project_value,p_actor);
  IF jsonb_typeof(p_input) IS DISTINCT FROM 'object'
    OR NOT kb_bid_v2_json_keys_exact(p_input,ARRAY['editor_key','save_id','final','callback_sha256'])
    OR (p_input->>'editor_key')::uuid IS NULL
    OR jsonb_typeof(p_input->'final') IS DISTINCT FROM 'boolean'
    OR jsonb_typeof(p_input->'callback_sha256') IS DISTINCT FROM 'string'
    OR (p_input->>'callback_sha256')::kb_sha256 IS NULL
    OR ((p_input->>'final')::boolean AND p_input->>'save_id' IS NOT NULL)
    OR (NOT (p_input->>'final')::boolean AND (p_input->>'save_id')::uuid IS NULL) THEN
    RAISE EXCEPTION 'DOCX_EDITOR_INPUT_INVALID' USING ERRCODE='23514';
  END IF;
  request_bytes:=kb_bid_v2_json_payload(jsonb_build_object('workspace_id',p_workspace_id,'input',p_input));
  RETURN kb_bid_v2_idempotency_replay(p_actor,'bid.v2.docx_editor.save',p_key,
    request_bytes,kb_bid_v2_sha256_bytes(request_bytes));
END $$;

-- Read-only replay before any physical write. An already committed DOCX must
-- not be rewritten merely because an HTTP client retries its initial upload.
CREATE FUNCTION kb_bid_v2_replay_docx_round(
  p_workspace_id uuid,p_input jsonb,p_actor kb_actor_identity,p_idempotency_key text
) RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE project_id_value uuid; request_bytes bytea;
BEGIN
  SELECT project_id INTO STRICT project_id_value FROM bid_submission_workspaces WHERE id=p_workspace_id;
  PERFORM kb_bid_v2_require_project_owner(project_id_value,p_actor);
  request_bytes:=kb_bid_v2_json_payload(jsonb_build_object('workspace_id',p_workspace_id,'input',p_input));
  RETURN kb_bid_v2_idempotency_replay(p_actor,'bid.v2.docx_round.create',p_idempotency_key,
    request_bytes,kb_bid_v2_sha256_bytes(request_bytes));
END $$;


-- Tender analysis Agent: same fenced execution ledger, new semantic workflow.
CREATE FUNCTION kb_bid_v2_tender_agent_lock_owner(
  p_request_artifact_id uuid,p_frozen_input_sha256 kb_sha256,p_attempt integer,p_execution_owner_token uuid
) RETURNS timestamptz LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE request_value bid_async_request_snapshot_artifacts%ROWTYPE;
  run_value bid_tender_agent_run_artifacts%ROWTYPE; now_value timestamptz;
BEGIN
  SELECT * INTO request_value FROM bid_async_request_snapshot_artifacts WHERE id=p_request_artifact_id FOR UPDATE;
  SELECT * INTO run_value FROM bid_tender_agent_run_artifacts
    WHERE request_artifact_id=p_request_artifact_id AND attempt=p_attempt FOR UPDATE;
  now_value:=clock_timestamp();
  IF request_value.id IS NULL OR request_value.request_kind NOT IN ('requirement_set_compile','docx_compose','submission_export','docx_layout')
     OR request_value.frozen_input_sha256<>p_frozen_input_sha256 OR request_value.status<>'pending' THEN
    RAISE EXCEPTION 'REQUEST_OBSOLETE' USING ERRCODE='P0002';
  END IF;
  IF request_value.current_attempt<>p_attempt OR run_value.request_artifact_id IS NULL
     OR run_value.frozen_input_sha256<>p_frozen_input_sha256
     OR run_value.execution_owner_token<>p_execution_owner_token OR run_value.status<>'running'
     OR run_value.lease_expires_at<=now_value OR run_value.hard_deadline_at<=now_value THEN
    RAISE EXCEPTION 'REQUEST_ATTEMPT_SUPERSEDED' USING ERRCODE='40001';
  END IF;
  RETURN now_value;
END $$;

CREATE FUNCTION kb_bid_v2_tender_agent_claim(
  p_request_artifact_id uuid,p_request_revision bigint,p_frozen_input_sha256 kb_sha256
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE request_value bid_async_request_snapshot_artifacts%ROWTYPE;
  current_run bid_tender_agent_run_artifacts%ROWTYPE;
  next_attempt integer; token uuid; claimed_at timestamptz;
BEGIN
  SELECT * INTO request_value FROM bid_async_request_snapshot_artifacts
    WHERE id=p_request_artifact_id FOR UPDATE;
  IF request_value.id IS NULL OR request_value.revision<>p_request_revision
     OR request_value.frozen_input_sha256<>p_frozen_input_sha256
     OR request_value.request_kind NOT IN ('requirement_set_compile','docx_compose','submission_export','docx_layout') THEN
    RETURN jsonb_build_object('disposition','obsolete');
  END IF;
  IF request_value.current_attempt>0 THEN
    SELECT * INTO current_run FROM bid_tender_agent_run_artifacts
      WHERE request_artifact_id=p_request_artifact_id AND attempt=request_value.current_attempt FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'AgentRun current attempt is missing' USING ERRCODE='23514'; END IF;
  END IF;
  claimed_at:=clock_timestamp();
  IF request_value.status<>'pending' THEN RETURN jsonb_build_object('disposition','obsolete'); END IF;
  IF request_value.current_attempt>0 THEN
    IF current_run.frozen_input_sha256<>p_frozen_input_sha256 THEN
      RAISE EXCEPTION 'AgentRun frozen identity mismatch' USING ERRCODE='23514';
    END IF;
    IF current_run.status='running' AND current_run.lease_expires_at>claimed_at
       AND current_run.hard_deadline_at>claimed_at THEN
      RETURN jsonb_build_object('disposition','live_owner','attempt',current_run.attempt);
    END IF;
    IF current_run.status='running' THEN
      UPDATE bid_tender_agent_run_artifacts SET status='superseded',lease_expires_at=least(lease_expires_at,claimed_at),
        heartbeat_at=greatest(heartbeat_at,claimed_at),last_error_code='REQUEST_ATTEMPT_SUPERSEDED',
        last_error_message='AgentRun lease expired or hard deadline elapsed',last_error_at=claimed_at,
        progress_sequence=progress_sequence+1,updated_at=claimed_at
      WHERE request_artifact_id=p_request_artifact_id AND attempt=current_run.attempt;
    ELSIF current_run.status IN ('succeeded','failed') THEN
      RETURN jsonb_build_object('disposition','obsolete');
    END IF;
  END IF;
  IF request_value.current_attempt>=request_value.max_run_attempts THEN
    UPDATE bid_tender_agent_run_artifacts SET status='failed',lease_expires_at=least(lease_expires_at,claimed_at),
      heartbeat_at=greatest(heartbeat_at,claimed_at),last_error_code='REQUEST_ATTEMPT_BUDGET_EXCEEDED',
      last_error_message='maximum AgentRun attempts exhausted',last_error_at=claimed_at,
      progress_phase='failed',progress_sequence=progress_sequence+1,updated_at=claimed_at
    WHERE request_artifact_id=p_request_artifact_id AND attempt=request_value.current_attempt
      AND status IN ('retry_yielded','superseded');
    UPDATE bid_async_request_snapshot_artifacts SET status='failed',
      error_code='REQUEST_ATTEMPT_BUDGET_EXCEEDED',finished_at=claimed_at
    WHERE id=p_request_artifact_id AND status='pending';
    RETURN jsonb_build_object('disposition','exhausted');
  END IF;
  next_attempt:=request_value.current_attempt+1; token:=gen_random_uuid();
  UPDATE bid_async_request_snapshot_artifacts SET current_attempt=next_attempt WHERE id=p_request_artifact_id;
  INSERT INTO bid_tender_agent_run_artifacts(request_artifact_id,frozen_input_sha256,attempt,status,
    execution_owner_token,lease_acquired_at,lease_expires_at,heartbeat_at,hard_deadline_at,
    progress_stage,progress_phase,progress_detail,progress_sequence,started_at,updated_at)
  VALUES(p_request_artifact_id,p_frozen_input_sha256,next_attempt,'running',token,
    claimed_at,claimed_at+interval '30 seconds',claimed_at,claimed_at+interval '46 minutes',
    CASE WHEN request_value.request_kind='docx_compose' THEN 'generating' ELSE 'analyzing' END,
    CASE WHEN request_value.request_kind='docx_compose' THEN 'drafting' ELSE 'analyzing' END,
    jsonb_build_object('phase',CASE WHEN request_value.request_kind='docx_compose' THEN 'drafting' ELSE 'analyzing' END,'attempt',next_attempt,
      'max_attempts',request_value.max_run_attempts),1,claimed_at,claimed_at);
  RETURN jsonb_build_object('disposition','claimed','attempt',next_attempt,
    'execution_owner_token',token,'lease_expires_at',claimed_at+interval '30 seconds',
    'heartbeat_at',claimed_at,'hard_deadline_at',claimed_at+interval '46 minutes',
    'max_attempts',request_value.max_run_attempts);
END $$;

CREATE FUNCTION kb_bid_v2_tender_agent_heartbeat(
  p_request_artifact_id uuid,p_frozen_input_sha256 kb_sha256,p_attempt integer,p_execution_owner_token uuid
) RETURNS timestamptz LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE request_value bid_async_request_snapshot_artifacts%ROWTYPE;
  run_value bid_tender_agent_run_artifacts%ROWTYPE; heartbeat_time timestamptz; next_expiry timestamptz;
BEGIN
  SELECT * INTO request_value FROM bid_async_request_snapshot_artifacts WHERE id=p_request_artifact_id FOR UPDATE;
  SELECT * INTO run_value FROM bid_tender_agent_run_artifacts
    WHERE request_artifact_id=p_request_artifact_id AND attempt=p_attempt FOR UPDATE;
  heartbeat_time:=clock_timestamp();
  IF request_value.id IS NULL OR request_value.status<>'pending' OR request_value.request_kind NOT IN ('requirement_set_compile','docx_compose','submission_export','docx_layout')
     OR request_value.frozen_input_sha256<>p_frozen_input_sha256 OR request_value.current_attempt<>p_attempt
     OR run_value.request_artifact_id IS NULL OR run_value.frozen_input_sha256<>p_frozen_input_sha256
     OR run_value.execution_owner_token<>p_execution_owner_token OR run_value.status<>'running'
     OR run_value.lease_expires_at<=heartbeat_time OR run_value.hard_deadline_at<=heartbeat_time THEN
    RAISE EXCEPTION 'REQUEST_ATTEMPT_SUPERSEDED' USING ERRCODE='40001';
  END IF;
  next_expiry:=least(heartbeat_time+interval '30 seconds',run_value.hard_deadline_at);
  UPDATE bid_tender_agent_run_artifacts SET heartbeat_at=heartbeat_time,
    lease_expires_at=next_expiry,updated_at=heartbeat_time
  WHERE request_artifact_id=p_request_artifact_id AND attempt=p_attempt;
  RETURN next_expiry;
END $$;

CREATE FUNCTION kb_bid_v2_tender_agent_yield_for_retry(
  p_request_artifact_id uuid,p_frozen_input_sha256 kb_sha256,p_attempt integer,
  p_execution_owner_token uuid,p_error_code text,p_error_message text
) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE now_value timestamptz;
BEGIN
  IF p_error_code<>'INTERNAL' THEN RAISE EXCEPTION 'invalid retry-yield error code' USING ERRCODE='22023'; END IF;
  now_value:=kb_bid_v2_tender_agent_lock_owner(p_request_artifact_id,p_frozen_input_sha256,p_attempt,p_execution_owner_token);
  UPDATE bid_tender_agent_run_artifacts SET status='retry_yielded',lease_expires_at=now_value,
    heartbeat_at=now_value,last_error_code=p_error_code,
    last_error_message=kb_bid_v2_diagnostic_prefix(coalesce(p_error_message,'')),last_error_at=now_value,
    progress_phase='retrying',progress_detail=progress_detail||jsonb_build_object(
      'phase','retrying','last_error_code',p_error_code,'last_error_message',kb_bid_v2_diagnostic_prefix(coalesce(p_error_message,''))),
    progress_sequence=progress_sequence+1,updated_at=now_value
  WHERE request_artifact_id=p_request_artifact_id AND attempt=p_attempt;
END $$;

CREATE FUNCTION kb_bid_v2_load_tender_analysis_input(p_request_id uuid,p_revision bigint,p_sha kb_sha256)
RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE typed bid_requirement_set_compile_request_identities%ROWTYPE; collection jsonb;
BEGIN
  SELECT * INTO STRICT typed FROM bid_requirement_set_compile_request_identities
    WHERE request_artifact_id=p_request_id AND request_revision=p_revision AND frozen_input_sha256=p_sha;
  SELECT convert_from(canonical_payload,'UTF8')::jsonb INTO STRICT collection
    FROM bid_document_set_artifacts WHERE id=typed.document_set_revision_id;
  RETURN jsonb_build_object('runtime',typed.agent_runtime,'input',jsonb_build_object(
    'schema_version',1,'project_id',typed.project_id,'document_set_id',typed.document_set_revision_id,
    'documents',collection->'items','document_relations',coalesce(collection->'relations','[]'::jsonb),
    'decisions',coalesce((SELECT jsonb_agg(jsonb_build_object('source_id',i.source_unit_revision_id,
        'disposition',i.disposition,'reason',i.reason) ORDER BY i.source_unit_revision_id)
      FROM bid_source_unit_disposition_set_items i
      WHERE i.disposition_set_id=typed.disposition_set_revision_id AND i.reason<>'awaiting_agent_analysis'),'[]'::jsonb),
    'source_units',coalesce((SELECT jsonb_agg(jsonb_build_object('source_unit_revision_id',s.id,
        'document_id',s.document_id,'ordinal',s.ordinal,'locator',s.source_locator->'locator',
        'text',convert_from(s.text_utf8,'UTF8')) ORDER BY i.ordinal,s.ordinal,s.id)
      FROM bid_document_set_items i JOIN bid_source_unit_revision_artifacts s
        ON s.project_id=i.project_id AND s.source_revision_id=i.source_revision_id
      WHERE i.document_set_id=typed.document_set_revision_id),'[]'::jsonb),
    'structured_forms',coalesce((SELECT jsonb_agg(jsonb_build_object('form_definition_revision_id',f.id,
        'source_unit_revision_id',f.source_unit_revision_id,'definition',convert_from(f.canonical_payload,'UTF8')::jsonb) ORDER BY f.id)
      FROM bid_document_set_items i JOIN bid_source_unit_revision_artifacts s
        ON s.project_id=i.project_id AND s.source_revision_id=i.source_revision_id
      JOIN bid_tender_structured_form_definition_artifacts f ON f.project_id=s.project_id AND f.source_unit_revision_id=s.id
      WHERE i.document_set_id=typed.document_set_revision_id),'[]'::jsonb)));
END $$;

CREATE FUNCTION kb_bid_v2_tender_agent_checkpoint_get(p_request_id uuid,p_sha kb_sha256)
RETURNS jsonb LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
  SELECT convert_from(canonical_payload,'UTF8')::jsonb FROM bid_tender_agent_checkpoint_artifacts
    WHERE request_artifact_id=p_request_id AND frozen_input_sha256=p_sha AND stage_kind='analysis_checkpoint'
    ORDER BY batch_ordinal DESC LIMIT 1
$$;

-- A source-view request resolves only an original belonging to this frozen
-- collection and active owner. The Agent never supplies a path or download URL.
CREATE FUNCTION kb_bid_v2_tender_source_view_input(p_request_id uuid,p_sha kb_sha256,
  p_attempt integer,p_token uuid,p_source_id uuid)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE result_value jsonb;
BEGIN
  PERFORM kb_bid_v2_tender_agent_lock_owner(p_request_id,p_sha,p_attempt,p_token);
  SELECT jsonb_build_object('source_id',s.id,'document_id',d.id,'sha256',d.original_sha256,
    'media_type',d.media_type,'byte_length',d.byte_length,'locator',s.source_locator->'locator')
    INTO result_value
    FROM bid_requirement_set_compile_request_identities r
    JOIN bid_document_set_artifacts collection ON collection.id=r.document_set_revision_id
    CROSS JOIN LATERAL jsonb_array_elements(convert_from(collection.canonical_payload,'UTF8')::jsonb->'items') frozen
    JOIN bid_document_set_items i ON i.document_set_id=r.document_set_revision_id
    JOIN bid_source_unit_revision_artifacts s ON s.project_id=i.project_id
      AND s.source_revision_id=i.source_revision_id AND s.document_id=i.document_id
    JOIN bid_documents d ON d.project_id=s.project_id AND d.id=s.document_id
    JOIN object_registry o ON o.object_ref=d.original_object_ref AND o.digest=d.original_sha256
      AND o.media_type=d.media_type AND o.byte_length=d.byte_length AND o.state='available'
    WHERE r.request_artifact_id=p_request_id AND r.frozen_input_sha256=p_sha AND s.id=p_source_id
      AND frozen->>'document_id'=d.id::text AND frozen->>'document_sha256'=d.original_sha256::text;
  IF result_value IS NULL THEN
    RAISE EXCEPTION 'AGENT_OUTPUT_INVALID: source original not in frozen collection or not ready' USING ERRCODE='23514';
  END IF;
  RETURN result_value;
END $$;

CREATE FUNCTION kb_bid_v2_tender_agent_reserve(p_request_id uuid,p_sha kb_sha256,p_attempt integer,p_token uuid,
    p_turn integer,p_role text,p_body bytea)
RETURNS integer LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE stamp timestamptz; runtime jsonb; prior jsonb; body jsonb; role_key text; stage text;
  config_sha kb_sha256; body_sha kb_sha256; tools_sha kb_sha256; prompt_sha kb_sha256; ordinal_value integer;
  dispatch_packet jsonb; initial_input jsonb; initial_revision bigint; initial_source jsonb;
BEGIN
  stamp:=kb_bid_v2_tender_agent_lock_owner(p_request_id,p_sha,p_attempt,p_token);
  SELECT agent_runtime INTO STRICT runtime FROM bid_requirement_set_compile_request_identities WHERE request_artifact_id=p_request_id;
  prior:=kb_bid_v2_tender_agent_checkpoint_get(p_request_id,p_sha);
  IF p_turn<0 OR p_turn<>coalesce((prior->>'turn')::integer,0)
      OR p_role<>coalesce(prior->>'role','main') OR p_role NOT IN ('main','reviewer')
      OR coalesce((prior->>'done')::boolean,false)
      OR coalesce(prior#>'{journal,pending,response}','null'::jsonb)<>'null'::jsonb THEN
    RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: turn or role changed' USING ERRCODE='23514';
  END IF;
  IF runtime->'checkpoint_contract_version' IS DISTINCT FROM '3'::jsonb
      OR runtime->>'runtime_adapter' IS DISTINCT FROM 'rig-chat-0.42.0/4'
      OR runtime->>'repair_task_policy' IS DISTINCT FROM 'main-repair-tasks-v1'
      OR runtime->>'main_dispatch_policy' IS DISTINCT FROM 'main-dispatch-v1' THEN RAISE EXCEPTION 'AGENT_PROVIDER_UNAVAILABLE: frozen runtime missing' USING ERRCODE='23514'; END IF;
  IF p_turn>=(runtime#>>'{limits,max_turns}')::integer
      OR coalesce((prior->>'tool_calls')::integer,0)>=(runtime#>>'{limits,max_tool_calls}')::integer
      OR coalesce((prior->>'read_bytes')::bigint,0)>=(runtime#>>'{limits,max_read_bytes}')::bigint THEN
    RAISE EXCEPTION 'AGENT_TURN_BUDGET_EXCEEDED' USING ERRCODE='23514';
  END IF;
  body:=convert_from(p_body,'UTF8')::jsonb;
  config_sha:=kb_bid_v2_sha256_bytes(convert_to(kb_bid_v2_jcs(runtime),'UTF8'));
  body_sha:=kb_bid_v2_sha256_bytes(p_body);
  tools_sha:=kb_bid_v2_sha256_bytes(convert_to(kb_bid_v2_jcs(body->'tools'),'UTF8'));
  prompt_sha:=kb_bid_v2_sha256_bytes(convert_to(kb_bid_v2_jcs(body#>'{messages,0,content}'),'UTF8'));
  role_key:=CASE WHEN p_role='main' THEN 'main_prompt_sha256' ELSE 'review_prompt_sha256' END;
  stage:=CASE WHEN p_role='main' THEN 'analysis_main' ELSE 'analysis_review' END;
  IF p_body IS DISTINCT FROM convert_to(kb_bid_v2_jcs(body),'UTF8')
      OR body->>'model' IS DISTINCT FROM runtime#>>'{provider,model_id}'
      OR body->'max_tokens' IS DISTINCT FROM runtime#>'{provider,max_tokens}'
      OR coalesce(body->'reasoning_effort','null'::jsonb) IS DISTINCT FROM runtime#>'{provider,reasoning_effort}'
      OR body->'stream_options' IS DISTINCT FROM '{"include_usage":true}'::jsonb
      OR body->'stream' IS DISTINCT FROM 'true'::jsonb OR body->>'tool_choice' IS DISTINCT FROM 'required'
      OR body#>>'{messages,0,role}' IS DISTINCT FROM 'system'
      OR octet_length(p_body)>(runtime#>>'{limits,max_context_bytes}')::bigint THEN
    RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: provider contract changed' USING ERRCODE='23514';
  END IF;
  IF coalesce((runtime#>'{limits,draft_path}')::boolean,true) THEN
    IF p_role IS DISTINCT FROM 'main'
        OR NOT kb_bid_v2_sha256_text(runtime->>'fill_tools_sha256')
        OR NOT kb_bid_v2_sha256_text(runtime->>'fill_prompt_sha256')
        OR (prompt_sha::text IS DISTINCT FROM runtime->>'main_prompt_sha256'
          AND prompt_sha::text IS DISTINCT FROM runtime->>'fill_prompt_sha256')
        OR (tools_sha::text IS DISTINCT FROM runtime->>'tools_sha256'
          AND tools_sha::text IS DISTINCT FROM runtime->>'fill_tools_sha256') THEN
      RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: draft outline/fill contract changed' USING ERRCODE='23514';
    END IF;
  ELSE
    IF prompt_sha::text IS DISTINCT FROM runtime->>role_key
        OR tools_sha::text IS DISTINCT FROM runtime->>(CASE WHEN p_role='main' THEN 'tools_sha256' ELSE 'review_tools_sha256' END) THEN
      RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: provider contract changed' USING ERRCODE='23514';
    END IF;
    IF p_role='main' THEN
      dispatch_packet:=((body#>>ARRAY['messages',(jsonb_array_length(body->'messages')-1)::text,'content'])::jsonb)->'main_dispatch';
      IF NOT kb_bid_v2_json_keys_exact(dispatch_packet,ARRAY['owner','source_scope','dependencies_sha256'])
        OR NOT kb_bid_v2_json_keys_exact(dispatch_packet->'owner',ARRAY['kind','id'])
        OR coalesce(dispatch_packet#>>'{owner,kind}','') NOT IN ('ordinary','repair')
        OR jsonb_typeof(dispatch_packet->'source_scope') IS DISTINCT FROM 'array'
        OR NOT kb_bid_v2_sha256_text(dispatch_packet->>'dependencies_sha256')
        OR (prior#>'{dispatch,active}'<>'null'::jsonb AND dispatch_packet->'owner' IS DISTINCT FROM prior#>'{dispatch,active}')
        OR (prior#>'{dispatch,active}'='null'::jsonb AND prior#>'{dispatch,entries}'<>'{}'::jsonb) THEN
        RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: reserved Main owner changed or missing' USING ERRCODE='23514';
      END IF;
      IF prior IS NULL OR prior#>'{dispatch,entries}'='{}'::jsonb THEN
        SELECT request_revision INTO STRICT initial_revision FROM bid_requirement_set_compile_request_identities WHERE request_artifact_id=p_request_id;
        initial_input:=kb_bid_v2_load_tender_analysis_input(p_request_id,initial_revision,p_sha)->'input';
        initial_source:=coalesce(initial_input#>'{source_units,0,source_unit_revision_id}','null'::jsonb);
        IF dispatch_packet#>>'{owner,kind}'<>'ordinary'
          OR dispatch_packet#>>'{owner,id}' IS DISTINCT FROM kb_bid_v2_sha256_bytes(convert_to(kb_bid_v2_jcs(jsonb_build_array(
            'main-dispatch-v1',kb_bid_v2_sha256_bytes(convert_to(kb_bid_v2_jcs(initial_input),'UTF8'))::text,initial_source)),'UTF8'))::text THEN
          RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: initial Main source owner changed' USING ERRCODE='23514';
        END IF;
      END IF;
    END IF;
  END IF;
  IF EXISTS(SELECT 1 FROM bid_tender_agent_call_attempts WHERE request_artifact_id=p_request_id
      AND frozen_input_sha256=p_sha AND batch_ordinal=p_turn AND
      (stage_kind<>stage OR provider_body<>p_body OR stage_contract_sha256<>config_sha)) THEN
    RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: replay body changed' USING ERRCODE='23514';
  END IF;
  SELECT coalesce(max(call_ordinal),0)+1 INTO ordinal_value FROM bid_tender_agent_call_attempts
    WHERE request_artifact_id=p_request_id AND frozen_input_sha256=p_sha AND batch_ordinal=p_turn;
  IF ordinal_value>3 THEN RETURN NULL; END IF;
  INSERT INTO bid_tender_agent_call_attempts(request_artifact_id,frozen_input_sha256,stage_kind,batch_ordinal,
    input_sha256,stage_contract_sha256,system_prompt_utf8_sha256,prompt_contract_id,prompt_contract_sha256,
    schema_contract_id,schema_contract_sha256,agent_contract_id,agent_contract_sha256,model_contract_id,
    model_contract_sha256,runtime_contract_sha256,provider_body,provider_body_sha256,call_ordinal,reserved_at)
  VALUES(p_request_id,p_sha,stage,p_turn,body_sha,config_sha,prompt_sha,
    kb_bid_v2_deterministic_uuid(config_sha::text||':'||p_role),prompt_sha,'tender_analysis_tools_v1',tools_sha,
    kb_bid_v2_deterministic_uuid(config_sha::text||':agent'),config_sha,
    kb_bid_v2_deterministic_uuid(config_sha::text||':provider'),config_sha,config_sha,p_body,body_sha,ordinal_value,stamp);
  RETURN ordinal_value;
END $$;

CREATE FUNCTION kb_bid_v2_tender_agent_checkpoint_put(p_request_id uuid,p_sha kb_sha256,p_attempt integer,p_token uuid,
    p_state jsonb,p_progress jsonb)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE stamp timestamptz; runtime jsonb; config_sha kb_sha256; prior jsonb; payload bytea; turn_value integer; sequence_value integer; pending_value jsonb; prior_pending jsonb; response_value jsonb; role_value text; prior_payload bytea;
  tasks jsonb; prior_tasks jsonb; task_entry record; alias_entry record; old_entry jsonb; counter_key text;
  feedback_findings jsonb; feedback_sha text; charged_task text;
  dispatch jsonb; prior_dispatch jsonb; dispatch_owner jsonb; charged_owner jsonb;
  dispatch_entry record; prior_entry jsonb; dispatch_cap numeric; source_input jsonb; source_revision bigint;
  saved_body jsonb; saved_packet jsonb;
BEGIN
  stamp:=kb_bid_v2_tender_agent_lock_owner(p_request_id,p_sha,p_attempt,p_token);
  SELECT agent_runtime INTO STRICT runtime FROM bid_requirement_set_compile_request_identities WHERE request_artifact_id=p_request_id;
  config_sha:=kb_bid_v2_sha256_bytes(convert_to(kb_bid_v2_jcs(runtime),'UTF8'));
  turn_value:=(p_state->>'turn')::integer;
  sequence_value:=(p_state#>>'{journal,sequence}')::integer;
  payload:=convert_to(kb_bid_v2_jcs(p_state),'UTF8');
  SELECT canonical_payload INTO prior_payload FROM bid_tender_agent_checkpoint_artifacts
    WHERE request_artifact_id=p_request_id AND frozen_input_sha256=p_sha AND stage_kind='analysis_checkpoint' AND batch_ordinal=sequence_value;
  IF FOUND THEN
    IF prior_payload IS DISTINCT FROM payload THEN RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: divergent checkpoint' USING ERRCODE='23514'; END IF;
    RETURN;
  END IF;
  prior:=kb_bid_v2_tender_agent_checkpoint_get(p_request_id,p_sha);
  pending_value:=p_state#>'{journal,pending}'; prior_pending:=coalesce(prior#>'{journal,pending}','null'::jsonb);
  role_value:=p_state->>'role';
  IF runtime->>'repair_task_policy' IS DISTINCT FROM 'main-repair-tasks-v1'
    OR runtime->>'main_dispatch_policy' IS DISTINCT FROM 'main-dispatch-v1'
    OR NOT kb_bid_v2_json_keys_exact(p_state->'journal',ARRAY['sequence','pending','session'])
    OR NOT kb_bid_v2_json_keys_exact(p_state->'repair',ARRAY['feedback_sha256','baseline','results','tasks'])
    OR jsonb_typeof(p_state#>'{repair,baseline}') IS DISTINCT FROM 'object'
    OR jsonb_typeof(p_state#>'{repair,results}') IS DISTINCT FROM 'object'
    OR (p_state#>'{repair,feedback_sha256}' IS DISTINCT FROM 'null'::jsonb AND (
      jsonb_typeof(p_state#>'{repair,feedback_sha256}') IS DISTINCT FROM 'string'
      OR p_state#>>'{repair,feedback_sha256}' !~ '^[0-9a-f]{64}$'))
    OR NOT (p_state ? 'source_review')
    OR (role_value='reviewer' AND jsonb_typeof(p_state->'source_review') IS DISTINCT FROM 'object')
    OR (p_state->'source_review' IS DISTINCT FROM 'null'::jsonb AND (
      NOT kb_bid_v2_json_keys_exact(p_state->'source_review',ARRAY[
        'schema_version','manifest_sha256','results','active_task','dependencies',
        'finding_revisions','candidate_revisions','completed_analysis_sha256'])
      OR p_state#>'{source_review,schema_version}' IS DISTINCT FROM '1'::jsonb
      OR jsonb_typeof(p_state#>'{source_review,manifest_sha256}') IS DISTINCT FROM 'string'
      OR jsonb_typeof(p_state#>'{source_review,results}') IS DISTINCT FROM 'object'
      OR jsonb_typeof(p_state#>'{source_review,finding_revisions}') IS DISTINCT FROM 'object'
      OR jsonb_typeof(p_state#>'{source_review,candidate_revisions}') IS DISTINCT FROM 'object'
      OR NOT kb_bid_v2_json_keys_exact(p_state#>'{source_review,dependencies}',ARRAY['source_ids','references','global'])
      OR jsonb_typeof(p_state#>'{source_review,dependencies,source_ids}') IS DISTINCT FROM 'array'
      OR jsonb_typeof(p_state#>'{source_review,dependencies,references}') IS DISTINCT FROM 'array'
      OR jsonb_typeof(p_state#>'{source_review,dependencies,global}') IS DISTINCT FROM 'boolean'))
    OR jsonb_typeof(p_state#>'{journal,sequence}') IS DISTINCT FROM 'number'
    OR (pending_value IS DISTINCT FROM 'null'::jsonb AND jsonb_typeof(p_state#>'{journal,session}') IS DISTINCT FROM 'object')
    OR (p_state#>'{journal,session}' IS DISTINCT FROM 'null'::jsonb AND (
      NOT kb_bid_v2_json_keys_exact(p_state#>'{journal,session}',ARRAY['run','prefix','suffix'])
      OR jsonb_typeof(p_state#>'{journal,session,run}') IS DISTINCT FROM 'object'
      OR p_state#>'{journal,session,prefix}' IS DISTINCT FROM '2'::jsonb
      OR p_state#>'{journal,session,suffix}' IS DISTINCT FROM '1'::jsonb
      OR octet_length(convert_to(kb_bid_v2_jcs(p_state#>'{journal,session}'),'UTF8'))>(runtime#>>'{limits,max_context_bytes}')::bigint))
    OR jsonb_typeof(p_state->'turn') IS DISTINCT FROM 'number'
    OR jsonb_typeof(p_state->'tool_calls') IS DISTINCT FROM 'number'
    OR jsonb_typeof(p_state->'read_bytes') IS DISTINCT FROM 'number'
    OR sequence_value IS NULL OR sequence_value<>coalesce((prior#>>'{journal,sequence}')::integer,0)+1
    OR turn_value IS NULL OR turn_value<0 OR p_state->>'config_sha256' IS DISTINCT FROM config_sha::text
    OR (prior IS NOT NULL AND p_state->>'input_sha256' IS DISTINCT FROM prior->>'input_sha256')
    OR role_value IS NULL OR role_value NOT IN ('main','reviewer')
    OR coalesce((prior#>>'{done}')::boolean,false)
    OR coalesce((p_state->>'tool_calls')::bigint,-1)<coalesce((prior->>'tool_calls')::bigint,0)
    OR coalesce((p_state->>'read_bytes')::bigint,-1)<coalesce((prior->>'read_bytes')::bigint,0)
    OR turn_value>((runtime->'limits')->>'max_turns')::integer
    OR (p_state->>'tool_calls')::bigint>((runtime->'limits')->>'max_tool_calls')::bigint
    OR (p_state->>'read_bytes')::bigint>((runtime->'limits')->>'max_read_bytes')::bigint THEN
    RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: checkpoint sequence, identity or budget' USING ERRCODE='23514';
  END IF;
  -- Stable Main owners live in this checkpoint; preparation never schedules.
  dispatch:=p_state->'dispatch';
  prior_dispatch:=coalesce(prior->'dispatch','{"active":null,"entries":{},"last_committed_turn":null}'::jsonb);
  dispatch_owner:=dispatch->'active';
  dispatch_cap:=least((runtime#>>'{limits,max_turns}')::numeric,
    (runtime#>>'{limits,max_focus_turns}')::numeric*((runtime#>>'{limits,max_focus_replans}')::numeric+1));
  SELECT request_revision INTO STRICT source_revision FROM bid_requirement_set_compile_request_identities WHERE request_artifact_id=p_request_id;
  source_input:=kb_bid_v2_load_tender_analysis_input(p_request_id,source_revision,p_sha)->'input';
  IF p_state->>'input_sha256' IS DISTINCT FROM kb_bid_v2_sha256_bytes(convert_to(kb_bid_v2_jcs(source_input),'UTF8'))::text
    OR NOT kb_bid_v2_json_keys_exact(dispatch,ARRAY['active','entries','last_committed_turn'])
    OR jsonb_typeof(dispatch->'entries') IS DISTINCT FROM 'object'
    OR (dispatch->'last_committed_turn'<>'null'::jsonb AND (
      jsonb_typeof(dispatch->'last_committed_turn') IS DISTINCT FROM 'number'
      OR dispatch->>'last_committed_turn' !~ '^(0|[1-9][0-9]*)$'
      OR (dispatch->>'last_committed_turn')::numeric>turn_value))
    OR (dispatch_owner<>'null'::jsonb AND (
      role_value<>'main' OR NOT kb_bid_v2_json_keys_exact(dispatch_owner,ARRAY['kind','id'])
      OR coalesce(dispatch_owner->>'kind','') NOT IN ('ordinary','repair')
      OR jsonb_typeof(dispatch_owner->'id') IS DISTINCT FROM 'string')) THEN
    RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: Main dispatch shape' USING ERRCODE='23514';
  END IF;
  IF dispatch->'entries'<>'{}'::jsonb AND (SELECT count(*) FROM jsonb_object_keys(dispatch->'entries'))<>jsonb_array_length(source_input->'source_units')+1 THEN
    RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: frozen Main root inventory changed' USING ERRCODE='23514';
  END IF;
  FOR dispatch_entry IN SELECT key,value FROM jsonb_each(dispatch->'entries') LOOP
    prior_entry:=prior_dispatch#>ARRAY['entries',dispatch_entry.key];
    IF NOT kb_bid_v2_json_keys_exact(dispatch_entry.value,ARRAY['source_id','spent_batches','spent_replans','watch','attempted_dependencies','completed_dependencies'])
      OR dispatch_entry.key IS DISTINCT FROM kb_bid_v2_sha256_bytes(convert_to(kb_bid_v2_jcs(jsonb_build_array(
        'main-dispatch-v1',p_state->>'input_sha256',dispatch_entry.value->'source_id')),'UTF8'))::text
      OR (dispatch_entry.value->'source_id'<>'null'::jsonb AND NOT EXISTS(SELECT 1 FROM jsonb_array_elements(source_input->'source_units') source
        WHERE source->'source_unit_revision_id'=dispatch_entry.value->'source_id'))
      OR NOT kb_bid_v2_json_keys_exact(dispatch_entry.value->'watch',ARRAY['no_progress_turns','focus_turns','replans','recovery'])
      OR coalesce(dispatch_entry.value#>>'{watch,recovery}','') NOT IN ('running','replan','blocked')
      OR jsonb_typeof(dispatch_entry.value->'attempted_dependencies') IS DISTINCT FROM 'array'
      OR (dispatch_entry.value->'completed_dependencies'<>'null'::jsonb AND NOT kb_bid_v2_sha256_text(dispatch_entry.value->>'completed_dependencies')) THEN
      RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: Main root shape or identity' USING ERRCODE='23514';
    END IF;
    FOREACH counter_key IN ARRAY ARRAY['spent_batches','spent_replans'] LOOP
      IF jsonb_typeof(dispatch_entry.value->counter_key) IS DISTINCT FROM 'number'
        OR dispatch_entry.value->>counter_key !~ '^(0|[1-9][0-9]*)$'
        OR (dispatch_entry.value->>counter_key)::numeric<coalesce((prior_entry->>counter_key)::numeric,0) THEN
        RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: Main root cost decreased' USING ERRCODE='23514';
      END IF;
    END LOOP;
    FOREACH counter_key IN ARRAY ARRAY['no_progress_turns','focus_turns','replans'] LOOP
      IF jsonb_typeof(dispatch_entry.value#>ARRAY['watch',counter_key]) IS DISTINCT FROM 'number'
        OR dispatch_entry.value#>>ARRAY['watch',counter_key] !~ '^(0|[1-9][0-9]*)$' THEN
        RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: Main root watch' USING ERRCODE='23514';
      END IF;
    END LOOP;
    IF (dispatch_entry.value->>'spent_batches')::numeric>dispatch_cap
      OR (dispatch_entry.value->>'spent_replans')::numeric>(runtime#>>'{limits,max_focus_replans}')::numeric
      OR (dispatch_entry.value->>'spent_replans')::numeric<(dispatch_entry.value#>>'{watch,replans}')::numeric
      OR NOT (coalesce(prior_entry->'attempted_dependencies','[]'::jsonb) <@ (dispatch_entry.value->'attempted_dependencies'))
      OR EXISTS(SELECT 1 FROM jsonb_array_elements(dispatch_entry.value->'attempted_dependencies') dep
        WHERE jsonb_typeof(dep)<>'string' OR NOT kb_bid_v2_sha256_text(dep#>>'{}')) THEN
      RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: Main root allowance or dependency history' USING ERRCODE='23514';
    END IF;
  END LOOP;
  IF EXISTS(SELECT 1 FROM jsonb_object_keys(prior_dispatch->'entries') id WHERE NOT (dispatch->'entries' ? id))
    OR (dispatch_owner->>'kind'='ordinary' AND (NOT (dispatch->'entries' ? (dispatch_owner->>'id'))
      OR p_state#>'{repair,tasks,active}'<>'null'::jsonb
      OR p_state#>'{main_progress,watch}' IS DISTINCT FROM dispatch#>ARRAY['entries',dispatch_owner->>'id','watch']
      OR (dispatch#>ARRAY['entries',dispatch_owner->>'id','source_id']<>'null'::jsonb AND NOT (
        p_state#>'{main_work,source_scope}' @> jsonb_build_array(dispatch#>ARRAY['entries',dispatch_owner->>'id','source_id'])))))
    OR (dispatch_owner->>'kind'='repair' AND (dispatch_owner->'id' IS DISTINCT FROM p_state#>'{repair,tasks,active}'
      OR p_state#>'{main_progress,watch}' IS DISTINCT FROM p_state#>ARRAY['repair','tasks','entries',dispatch_owner->>'id','watch'])) THEN
    RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: Main dispatch detached owner or watch' USING ERRCODE='23514';
  END IF;
  charged_owner:=NULL;
  IF prior_pending->'response'<>'null'::jsonb AND pending_value='null'::jsonb AND prior->>'role'='main' THEN
    saved_body:=(prior_pending->>'body')::jsonb;
    saved_packet:=(saved_body#>>ARRAY['messages',(jsonb_array_length(saved_body->'messages')-1)::text,'content'])::jsonb;
    charged_owner:=saved_packet#>'{main_dispatch,owner}';
    IF NOT kb_bid_v2_json_keys_exact(charged_owner,ARRAY['kind','id'])
      OR coalesce(charged_owner->>'kind','') NOT IN ('ordinary','repair')
      OR (prior_dispatch->'active'<>'null'::jsonb AND charged_owner IS DISTINCT FROM prior_dispatch->'active')
      OR dispatch->>'last_committed_turn' IS DISTINCT FROM turn_value::text THEN
      RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: Main committed owner changed' USING ERRCODE='23514';
    END IF;
  ELSIF dispatch->'last_committed_turn' IS DISTINCT FROM prior_dispatch->'last_committed_turn' THEN
    RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: Main charged outside committed batch' USING ERRCODE='23514';
  END IF;
  FOR dispatch_entry IN SELECT key,value FROM jsonb_each(dispatch->'entries') LOOP
    IF (dispatch_entry.value->>'spent_batches')::numeric IS DISTINCT FROM
      coalesce((prior_dispatch#>>ARRAY['entries',dispatch_entry.key,'spent_batches'])::numeric,0)
      +(CASE WHEN charged_owner->>'kind'='ordinary' AND charged_owner->>'id'=dispatch_entry.key THEN 1 ELSE 0 END) THEN
      RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: Main batch must charge old owner exactly once' USING ERRCODE='23514';
    END IF;
  END LOOP;
  -- Finding execution is domain checkpoint metadata, not a second Agent table.
  -- Content versions may change; already spent attempts must not disappear.
  tasks:=p_state#>'{repair,tasks}';
  prior_tasks:=coalesce(prior#>'{repair,tasks}',
    '{"active":null,"aliases":{},"entries":{},"feedback_sha256":null,"last_committed_turn":null}'::jsonb);
  SELECT coalesce(jsonb_agg(value ORDER BY key),'[]'::jsonb) INTO feedback_findings
    FROM jsonb_each(p_state->'review_draft');
  IF p_state->'review' IS DISTINCT FROM 'null'::jsonb THEN
    feedback_findings:=p_state#>'{review,findings}';
  END IF;
  feedback_sha:=kb_bid_v2_sha256_bytes(convert_to(kb_bid_v2_jcs(
    jsonb_build_array(p_state->'review_rounds',feedback_findings)),'UTF8'))::text;
  IF NOT kb_bid_v2_json_keys_exact(tasks,ARRAY['active','aliases','entries','feedback_sha256','last_committed_turn'])
    OR jsonb_typeof(tasks->'aliases') IS DISTINCT FROM 'object'
    OR jsonb_typeof(tasks->'entries') IS DISTINCT FROM 'object'
    OR (tasks->'active' IS DISTINCT FROM 'null'::jsonb AND (
      jsonb_typeof(tasks->'active') IS DISTINCT FROM 'string' OR btrim(tasks->>'active')=''))
    OR (tasks->'feedback_sha256' IS DISTINCT FROM 'null'::jsonb AND (
      jsonb_typeof(tasks->'feedback_sha256') IS DISTINCT FROM 'string'
      OR NOT kb_bid_v2_sha256_text(tasks->>'feedback_sha256')))
    OR (tasks->'last_committed_turn' IS DISTINCT FROM 'null'::jsonb AND (
      jsonb_typeof(tasks->'last_committed_turn') IS DISTINCT FROM 'number'
      OR tasks->>'last_committed_turn' !~ '^(0|[1-9][0-9]*)$')) THEN
    RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: repair task shape' USING ERRCODE='23514';
  END IF;
  IF tasks->'feedback_sha256' IS DISTINCT FROM prior_tasks->'feedback_sha256'
    AND tasks->>'feedback_sha256' IS DISTINCT FROM feedback_sha THEN
    RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: repair task feedback identity' USING ERRCODE='23514';
  END IF;
  IF (tasks->>'last_committed_turn')::numeric>turn_value
    OR (prior_tasks->'last_committed_turn'<>'null'::jsonb AND (
      tasks->'last_committed_turn'='null'::jsonb
      OR (tasks->>'last_committed_turn')::numeric<(prior_tasks->>'last_committed_turn')::numeric)) THEN
    RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: repair task accounting' USING ERRCODE='23514';
  END IF;
  FOR task_entry IN SELECT key,value FROM jsonb_each(tasks->'entries') LOOP
    IF btrim(task_entry.key)=''
      OR NOT kb_bid_v2_json_keys_exact(task_entry.value,ARRAY[
        'finding_sha256','watch','committed_turns','attempted_dependencies','inherited_blocked'])
      OR jsonb_typeof(task_entry.value->'finding_sha256') IS DISTINCT FROM 'string'
      OR NOT kb_bid_v2_sha256_text(task_entry.value->>'finding_sha256')
      OR NOT kb_bid_v2_json_keys_exact(task_entry.value->'watch',ARRAY[
        'no_progress_turns','focus_turns','replans','recovery'])
      OR coalesce(task_entry.value#>>'{watch,recovery}','') NOT IN ('running','replan','blocked')
      OR jsonb_typeof(task_entry.value->'committed_turns') IS DISTINCT FROM 'number'
      OR task_entry.value->>'committed_turns' !~ '^(0|[1-9][0-9]*)$'
      OR jsonb_typeof(task_entry.value->'attempted_dependencies') IS DISTINCT FROM 'array'
      OR jsonb_typeof(task_entry.value->'inherited_blocked') IS DISTINCT FROM 'boolean' THEN
      RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: repair task shape' USING ERRCODE='23514';
    END IF;
    FOREACH counter_key IN ARRAY ARRAY['no_progress_turns','focus_turns','replans'] LOOP
      IF jsonb_typeof(task_entry.value#>ARRAY['watch',counter_key]) IS DISTINCT FROM 'number'
        OR task_entry.value#>>ARRAY['watch',counter_key] !~ '^(0|[1-9][0-9]*)$' THEN
        RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: repair task shape' USING ERRCODE='23514';
      END IF;
    END LOOP;
    IF EXISTS(SELECT 1 FROM jsonb_array_elements(task_entry.value->'attempted_dependencies') dependency
      WHERE jsonb_typeof(dependency) IS DISTINCT FROM 'string'
        OR NOT kb_bid_v2_sha256_text(dependency#>>'{}'))
      OR (SELECT count(*)<>count(DISTINCT dependency) FROM
        jsonb_array_elements(task_entry.value->'attempted_dependencies') dependency) THEN
      RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: repair task shape' USING ERRCODE='23514';
    END IF;
  END LOOP;
  FOR alias_entry IN SELECT key,value FROM jsonb_each(tasks->'aliases') LOOP
    IF btrim(alias_entry.key)='' OR jsonb_typeof(alias_entry.value) IS DISTINCT FROM 'string'
      OR NOT (tasks->'entries' ? (alias_entry.value#>>'{}'))
      OR tasks#>ARRAY['aliases',alias_entry.value#>>'{}'] IS DISTINCT FROM alias_entry.value THEN
      RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: repair task alias' USING ERRCODE='23514';
    END IF;
    IF NOT (prior_tasks->'aliases' ? alias_entry.key) AND (
      NOT (p_state->'review_draft' ? alias_entry.key)
      OR tasks#>>ARRAY['entries',alias_entry.value#>>'{}','finding_sha256'] IS DISTINCT FROM
        kb_bid_v2_sha256_bytes(convert_to(kb_bid_v2_jcs(p_state#>ARRAY['review_draft',alias_entry.key]),'UTF8'))::text) THEN
      RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: repair task finding identity' USING ERRCODE='23514';
    END IF;
  END LOOP;
  IF EXISTS(SELECT 1 FROM jsonb_object_keys(tasks->'entries') id WHERE NOT (tasks->'aliases' ? id))
    OR (tasks->'active'<>'null'::jsonb AND (
      NOT (tasks->'entries' ? (tasks->>'active'))
      OR tasks#>ARRAY['aliases',tasks->>'active'] IS DISTINCT FROM tasks->'active'))
    OR EXISTS(SELECT 1 FROM jsonb_object_keys(prior_tasks->'aliases') id WHERE NOT (tasks->'aliases' ? id)) THEN
    RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: repair task alias' USING ERRCODE='23514';
  END IF;
  FOR task_entry IN SELECT key,value FROM jsonb_each(prior_tasks->'entries') LOOP
    old_entry:=tasks#>ARRAY['entries',task_entry.key];
    IF old_entry IS NULL
      OR (old_entry->>'committed_turns')::numeric<(task_entry.value->>'committed_turns')::numeric
      OR (old_entry#>>'{watch,replans}')::numeric<(task_entry.value#>>'{watch,replans}')::numeric
      OR NOT ((task_entry.value->'attempted_dependencies') <@ (old_entry->'attempted_dependencies'))
      OR (task_entry.value->'inherited_blocked'='true'::jsonb AND old_entry->'inherited_blocked'<>'true'::jsonb) THEN
      RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: repair task accounting' USING ERRCODE='23514';
    END IF;
  END LOOP;
  FOR task_entry IN SELECT key,value FROM jsonb_each(tasks->'entries') LOOP
    IF task_entry.value->'finding_sha256' IS DISTINCT FROM prior_tasks#>ARRAY['entries',task_entry.key,'finding_sha256']
      AND (tasks->>'feedback_sha256' IS DISTINCT FROM feedback_sha
        OR NOT EXISTS(SELECT 1 FROM jsonb_each(tasks->'aliases') alias
          JOIN jsonb_each(p_state->'review_draft') finding ON finding.key=alias.key
          WHERE alias.value=to_jsonb(task_entry.key) AND task_entry.value->>'finding_sha256'=
            kb_bid_v2_sha256_bytes(convert_to(kb_bid_v2_jcs(finding.value),'UTF8'))::text)) THEN
      RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: repair task finding identity' USING ERRCODE='23514';
    END IF;
  END LOOP;
  -- Exact aliases share one ledger. Merging identities must carry the sum of
  -- their former canonical costs, while retaining retired entry history.
  IF EXISTS(SELECT 1 FROM jsonb_each(tasks->'entries') entry
      WHERE (entry.value->>'committed_turns')::numeric<(
        SELECT coalesce(sum((prior_tasks#>>ARRAY['entries',previous.target,'committed_turns'])::numeric),0)
        FROM (SELECT DISTINCT prior_tasks#>>ARRAY['aliases',alias.key] AS target
          FROM jsonb_each(tasks->'aliases') alias
          WHERE alias.value=to_jsonb(entry.key) AND prior_tasks->'aliases' ? alias.key) previous)) THEN
    RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: repair task merge lost accounting' USING ERRCODE='23514';
  END IF;
  -- Main repair dispositions persist in the existing journal. They are never
  -- independent review results or permission to publish an extraction.
  IF (p_state#>'{repair,feedback_sha256}'='null'::jsonb AND (
      p_state#>'{repair,baseline}'<>'{}'::jsonb OR p_state#>'{repair,results}'<>'{}'::jsonb))
    OR EXISTS(SELECT 1 FROM jsonb_each(p_state#>'{repair,baseline}') AS entry WHERE
      entry.key !~ '^(record|relation):.+' OR jsonb_typeof(entry.value)<>'string'
      OR entry.value#>>'{}' !~ '^[0-9a-f]{64}$')
    OR EXISTS(SELECT 1 FROM jsonb_each(p_state#>'{repair,results}') AS entry WHERE
      entry.key !~ '^[0-9a-f]{64}$'
      OR NOT kb_bid_v2_json_keys_exact(entry.value,ARRAY['conclusion','summary','sources','candidate_versions'])
      OR coalesce(entry.value->>'conclusion','') NOT IN ('revised','disputed')
      OR jsonb_typeof(entry.value->'summary') IS DISTINCT FROM 'string'
      OR btrim(entry.value->>'summary')=''
      OR jsonb_typeof(entry.value->'sources') IS DISTINCT FROM 'array'
      OR entry.value->'sources'='[]'::jsonb
      OR jsonb_typeof(entry.value->'candidate_versions') IS DISTINCT FROM 'object') THEN
    RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: repair disposition shape' USING ERRCODE='23514';
  END IF;
  IF pending_value IS DISTINCT FROM 'null'::jsonb AND (
    NOT kb_bid_v2_json_keys_exact(pending_value,ARRAY['turn','role','body','response'])
    OR pending_value->'turn' IS DISTINCT FROM p_state->'turn'
    OR pending_value->>'role' IS DISTINCT FROM role_value
    OR jsonb_typeof(pending_value->'body') IS DISTINCT FROM 'string') THEN
    RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: pending request identity' USING ERRCODE='23514';
  END IF;
  IF prior_pending='null'::jsonb THEN
    -- Reservation and this prepared checkpoint commit in one transaction.
    IF turn_value<>coalesce((prior->>'turn')::integer,0)
      OR role_value IS DISTINCT FROM (coalesce(prior->>'role','main'))
      OR p_state#>'{done}' IS DISTINCT FROM 'false'::jsonb
      OR pending_value->'response' IS DISTINCT FROM 'null'::jsonb
      OR NOT EXISTS(SELECT 1 FROM bid_tender_agent_call_attempts WHERE request_artifact_id=p_request_id
        AND frozen_input_sha256=p_sha AND batch_ordinal=turn_value AND stage_kind=CASE WHEN role_value='reviewer' THEN 'analysis_review' ELSE 'analysis_main' END
        AND provider_body=convert_to(pending_value->>'body','UTF8')) THEN
      RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: prepared checkpoint requires reserved exact request' USING ERRCODE='23514';
    END IF;
    IF prior IS NOT NULL AND (((p_state-ARRAY['journal','transcript']) #- '{pending_coverage,views}') IS DISTINCT FROM
      ((prior-ARRAY['journal','transcript']) #- '{pending_coverage,views}')
      OR NOT (coalesce(p_state#>'{pending_coverage,views}','{}'::jsonb) <@ coalesce(prior#>'{pending_coverage,views}','{}'::jsonb))) THEN
      RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: preparation changed business state' USING ERRCODE='23514';
    END IF;
    IF prior IS NULL AND (p_state->'dispatch' IS DISTINCT FROM '{"active":null,"entries":{},"last_committed_turn":null}'::jsonb
      OR p_state->'tool_calls' IS DISTINCT FROM '0'::jsonb
      OR p_state->'read_bytes' IS DISTINCT FROM '0'::jsonb
      OR p_state->'transcript' IS DISTINCT FROM '[]'::jsonb
      OR p_state#>'{analysis,records}' IS DISTINCT FROM '{}'::jsonb
      OR p_state#>'{analysis,relations}' IS DISTINCT FROM '{}'::jsonb
      OR p_state#>'{analysis,dispositions}' IS DISTINCT FROM '{}'::jsonb
      OR p_state->'review' IS DISTINCT FROM 'null'::jsonb
      OR ((p_state->'repair')-'tasks') IS DISTINCT FROM '{"feedback_sha256":null,"baseline":{},"results":{}}'::jsonb
      OR (tasks-'feedback_sha256') IS DISTINCT FROM '{"active":null,"aliases":{},"entries":{},"last_committed_turn":null}'::jsonb
      OR p_state->'review_draft' IS DISTINCT FROM '{}'::jsonb
      OR p_state->'source_review' IS DISTINCT FROM 'null'::jsonb
      OR p_state->'review_rounds' IS DISTINCT FROM '0'::jsonb
      OR p_state->'source_views' IS DISTINCT FROM '{}'::jsonb
      OR p_state->'pending_coverage' IS DISTINCT FROM 'null'::jsonb
      OR p_state->'main_progress' IS DISTINCT FROM '{"watch":{"no_progress_turns":0,"focus_turns":0,"replans":0,"recovery":"running"},"seen":[],"completions":[],"blockers":[]}'::jsonb
      OR p_state->'reviewer_progress' IS DISTINCT FROM '{"watch":{"no_progress_turns":0,"focus_turns":0,"replans":0,"recovery":"running"},"seen":[],"completions":[],"blockers":[]}'::jsonb
      OR p_state->'main_work' IS DISTINCT FROM 'null'::jsonb
      OR p_state->'reviewer_work' IS DISTINCT FROM 'null'::jsonb
      OR p_state#>'{analysis,coverage}' IS DISTINCT FROM '{"metadata":{},"text":{},"form_cells":{},"candidate":{},"views":{},"view_failures":{}}'::jsonb
      OR p_state#>'{reviewer_coverage}' IS DISTINCT FROM '{"metadata":{},"text":{},"form_cells":{},"candidate":{},"views":{},"view_failures":{}}'::jsonb) THEN
      RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: initial checkpoint must be empty' USING ERRCODE='23514';
    END IF;
  ELSIF prior_pending->'response'='null'::jsonb THEN
    -- No evidence, counters or tool mutations until the full response is durable.
    IF (p_state-'journal') IS DISTINCT FROM (prior-'journal')
      OR p_state#>'{journal,session}' IS DISTINCT FROM prior#>'{journal,session}'
      OR (pending_value-'response') IS DISTINCT FROM (prior_pending-'response')
      OR jsonb_typeof(pending_value->'response') IS DISTINCT FROM 'object' THEN
      RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: response boundary changed prepared state' USING ERRCODE='23514';
    END IF;
    response_value:=pending_value->'response';
    IF NOT kb_bid_v2_json_keys_exact(response_value,ARRAY['content','tool_calls','finish_reason','usage'])
      OR jsonb_typeof(response_value->'content') IS DISTINCT FROM 'string'
      OR response_value->>'finish_reason' IS DISTINCT FROM 'tool_calls'
      OR jsonb_typeof(response_value->'tool_calls') IS DISTINCT FROM 'array' THEN
      RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: incomplete saved response' USING ERRCODE='23514';
    END IF;
    IF jsonb_array_length(response_value->'tool_calls')=0 OR EXISTS(
      SELECT 1 FROM jsonb_array_elements(response_value->'tool_calls') c
      WHERE NOT kb_bid_v2_json_keys_exact(c,ARRAY['id','name','arguments'])
        OR jsonb_typeof(c->'id') IS DISTINCT FROM 'string' OR btrim(c->>'id')=''
        OR jsonb_typeof(c->'name') IS DISTINCT FROM 'string' OR btrim(c->>'name')=''
        OR jsonb_typeof(c->'arguments') IS DISTINCT FROM 'string')
      OR (SELECT count(*)<>count(DISTINCT c->>'id') FROM jsonb_array_elements(response_value->'tool_calls') c) THEN
      RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: invalid saved tool calls' USING ERRCODE='23514';
    END IF;
  ELSE
    IF pending_value IS DISTINCT FROM 'null'::jsonb OR turn_value<>(prior->>'turn')::integer+1
      OR (p_state->>'tool_calls')::bigint<=(prior->>'tool_calls')::bigint
      OR (p_state->>'tool_calls')::bigint>(prior->>'tool_calls')::bigint+jsonb_array_length(prior_pending#>'{response,tool_calls}') THEN
      RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: tool commit must follow saved response' USING ERRCODE='23514';
    END IF;
    charged_task:=CASE WHEN prior->>'role'='main' THEN prior_tasks->>'active' END;
    IF (charged_task IS NOT NULL AND tasks->>'last_committed_turn' IS DISTINCT FROM turn_value::text)
      OR (charged_task IS NULL AND tasks->'last_committed_turn' IS DISTINCT FROM prior_tasks->'last_committed_turn')
      OR EXISTS(SELECT 1 FROM jsonb_each(tasks->'entries') entry
        WHERE (entry.value->>'committed_turns')::numeric<>greatest(
          coalesce((prior_tasks#>>ARRAY['entries',entry.key,'committed_turns'])::numeric,0)
            +CASE WHEN charged_task=entry.key THEN 1 ELSE 0 END,
          (SELECT coalesce(sum((prior_tasks#>>ARRAY['entries',previous.target,'committed_turns'])::numeric
              +CASE WHEN charged_task=previous.target THEN 1 ELSE 0 END),0)
            FROM (SELECT DISTINCT prior_tasks#>>ARRAY['aliases',alias.key] AS target
              FROM jsonb_each(tasks->'aliases') alias
              WHERE alias.value=to_jsonb(entry.key) AND prior_tasks->'aliases' ? alias.key) previous))) THEN
      RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: committed batch repair task charge' USING ERRCODE='23514';
    END IF;
  END IF;
  INSERT INTO bid_tender_agent_checkpoint_artifacts(request_artifact_id,frozen_input_sha256,stage_kind,batch_ordinal,
    contract_sha256,canonical_input,input_sha256,canonical_payload,content_sha256)
  VALUES(p_request_id,p_sha,'analysis_checkpoint',sequence_value,config_sha,convert_to(p_sha::text,'UTF8'),
    kb_bid_v2_sha256_bytes(convert_to(p_sha::text,'UTF8')),payload,kb_bid_v2_sha256_bytes(payload));
  UPDATE bid_tender_agent_run_artifacts SET progress_stage=CASE WHEN p_state->>'role'='reviewer' THEN 'reviewing' ELSE 'analyzing' END,
    progress_phase=CASE WHEN p_state->>'role'='reviewer' THEN 'verifying' ELSE 'collecting' END,
    progress_detail=p_progress,progress_sequence=progress_sequence+1,turn_count=turn_value,
    tool_call_count=(p_state->>'tool_calls')::integer,text_bytes_read=(p_state->>'read_bytes')::bigint,
    checkpoint_sha256=kb_bid_v2_sha256_bytes(payload),updated_at=stamp
    WHERE request_artifact_id=p_request_id AND attempt=p_attempt;
END $$;

CREATE FUNCTION kb_bid_v2_tender_agent_fail(p_request_id uuid,p_sha kb_sha256,p_attempt integer,p_token uuid,p_code text,p_message text)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE stamp timestamptz;
BEGIN
  stamp:=kb_bid_v2_tender_agent_lock_owner(p_request_id,p_sha,p_attempt,p_token);
  UPDATE bid_tender_agent_run_artifacts SET status='failed',progress_phase='failed',lease_expires_at=least(lease_expires_at,stamp),
    last_error_code=p_code,last_error_message=kb_bid_v2_diagnostic_prefix(p_message),last_error_at=stamp,updated_at=stamp
    WHERE request_artifact_id=p_request_id AND attempt=p_attempt;
  UPDATE bid_async_request_snapshot_artifacts SET status='failed',error_code=p_code,finished_at=stamp
    WHERE id=p_request_id;
END $$;

CREATE FUNCTION kb_bid_v2_get_tender_analysis(p_project_id uuid,p_set_id uuid,p_actor kb_actor_identity,p_kind text,p_offset integer,p_limit integer)
RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE payload jsonb; result_value jsonb; rows_value jsonb; total_value bigint;
BEGIN
  PERFORM kb_bid_v2_require_project_owner(p_project_id,p_actor);
  IF p_offset<0 OR p_limit<1 OR p_limit>100 THEN RAISE EXCEPTION 'invalid analysis page' USING ERRCODE='22023'; END IF;
  SELECT convert_from(canonical_payload,'UTF8')::jsonb INTO payload FROM bid_requirement_set_artifacts
    WHERE project_id=p_project_id AND id=p_set_id;
  IF payload IS NULL THEN RETURN NULL; END IF;
  result_value:=payload->'analysis_result';
  IF result_value IS NULL THEN RETURN jsonb_build_object('available',false,'requirement_set_id',p_set_id); END IF;
  IF p_kind NOT IN ('all','fact','rule','requirement','template','unresolved','relation','disposition','finding') THEN
    RAISE EXCEPTION 'invalid analysis kind' USING ERRCODE='22023';
  END IF;
  WITH entries AS (
    SELECT key,value FROM jsonb_each(CASE p_kind WHEN 'relation' THEN result_value#>'{analysis,relations}'
      WHEN 'disposition' THEN result_value#>'{analysis,dispositions}' ELSE result_value#>'{analysis,records}' END)
      WHERE p_kind IN ('all','relation','disposition') OR value#>>'{data,kind}'=p_kind
    UNION ALL SELECT ordinal::text,value FROM jsonb_array_elements(result_value#>'{review,findings}') WITH ORDINALITY f(value,ordinal) WHERE p_kind='finding'
  ), numbered AS (SELECT key,value,row_number() OVER(ORDER BY key) AS row_number FROM entries)
  SELECT count(*),coalesce(jsonb_agg(jsonb_build_object('id',key,'value',value) ORDER BY key)
    FILTER(WHERE row_number>p_offset AND row_number<=p_offset+p_limit),'[]'::jsonb) INTO total_value,rows_value FROM numbered;
  RETURN jsonb_build_object('available',true,'schema_version',1,'requirement_set_id',p_set_id,
    'quality',result_value->'quality','input_sha256',result_value->'frozen_input_sha256',
    'document_set_id',payload->'document_set_revision_id','kind',p_kind,'offset',p_offset,'total',total_value,'items',rows_value);
END $$;

CREATE FUNCTION kb_bid_v2_get_tender_outline(
  p_project_id uuid,p_actor kb_actor_identity
) RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE request_id uuid; request_status text; frozen_sha kb_sha256;
  requirement_set_id uuid; extracted_from text:='none'; analysis_records jsonb:='{}'::jsonb;
  checkpoint jsonb; payload jsonb;
BEGIN
  PERFORM kb_bid_v2_require_project_owner(p_project_id,p_actor);
  SELECT r.id,r.status,r.frozen_input_sha256 INTO request_id,request_status,frozen_sha
    FROM bid_async_request_snapshot_artifacts r
    WHERE r.project_id=p_project_id AND r.request_kind='requirement_set_compile'
    ORDER BY r.revision DESC,r.created_at DESC,r.id DESC LIMIT 1;
  IF request_status='succeeded' THEN
    SELECT (r.result_identity->>'requirement_set_id')::uuid INTO requirement_set_id
      FROM bid_async_request_snapshot_artifacts r WHERE r.id=request_id;
    IF requirement_set_id IS NOT NULL THEN
      SELECT convert_from(canonical_payload,'UTF8')::jsonb#>'{analysis_result,analysis,records}'
        INTO payload FROM bid_requirement_set_artifacts
        WHERE project_id=p_project_id AND id=requirement_set_id;
      IF jsonb_typeof(payload)='object' THEN
        SELECT coalesce(jsonb_object_agg(key,value),'{}'::jsonb) INTO analysis_records
          FROM jsonb_each(payload) rec(key,value)
          WHERE value#>>'{data,kind}' IN ('template','rule');
        extracted_from:='published';
      END IF;
    END IF;
  ELSIF request_id IS NOT NULL THEN
    checkpoint:=kb_bid_v2_tender_agent_checkpoint_get(request_id,frozen_sha);
    payload:=checkpoint#>'{analysis,records}';
    IF jsonb_typeof(payload)='object' THEN
      SELECT coalesce(jsonb_object_agg(key,value),'{}'::jsonb) INTO analysis_records
        FROM jsonb_each(payload) rec(key,value)
        WHERE value#>>'{data,kind}' IN ('template','rule');
      extracted_from:='checkpoint';
    END IF;
  END IF;
  RETURN jsonb_build_object(
    'quality','draft',
    'compile_status',request_status,
    'extracted_from',extracted_from,
    'records',coalesce(analysis_records,'{}'::jsonb),
    'documents',coalesce((
      SELECT jsonb_agg(jsonb_build_object(
        'id',d.id,'file_name',d.file_name,'parse_status',d.parse_status,
        'headings',coalesce((
          SELECT jsonb_agg(jsonb_build_object(
            'ordinal',u.ordinal,'kind',u.unit_kind,
            'heading_path',coalesce(u.source_locator#>>'{locator,heading_path}',''),
            'page_ordinal',u.source_locator#>'{locator,page_ordinal}')
            ORDER BY u.ordinal,u.id)
          FROM bid_source_unit_revision_artifacts u
          WHERE u.project_id=p_project_id AND u.document_id=d.id),'[]'::jsonb))
        ORDER BY d.created_at,d.id)
      FROM bid_documents d WHERE d.project_id=p_project_id),'[]'::jsonb));
END $$;

-- V2 global conclusions must remain grounded in the exact frozen graph and
-- this reviewer's delivered evidence. A five-element array is not acceptance.
CREATE FUNCTION kb_bid_v2_analysis_global_review_valid(p_input jsonb,p_analysis jsonb,p_review jsonb)
RETURNS boolean LANGUAGE plpgsql IMMUTABLE SET search_path=pg_catalog,public AS $$
DECLARE required_keys text[]:=ARRAY['source_coverage','collection_consistency','composition_order_format','cross_references','rule_items'];
  check_value jsonb; span_value jsonb; record_value jsonb; source_value jsonb; form_value jsonb;
  key_value text; record_id text; finding_id text; seen_keys text[]:=ARRAY[]::text[];
  expected_scope text; expected_contract text; input_sha text; coverage jsonb;
  start_value bigint; end_value bigint; cell_index bigint; limit_source boolean; frozen_missing boolean; source_bytes bytea;
BEGIN
  input_sha:=kb_bid_v2_sha256_bytes(convert_to(kb_bid_v2_jcs(p_input),'UTF8'));
  expected_scope:=kb_bid_v2_sha256_bytes(convert_to(kb_bid_v2_jcs(jsonb_build_object(
    'input',input_sha,'records',p_analysis->'records','relations',p_analysis->'relations','dispositions',p_analysis->'dispositions')),'UTF8'));
  expected_contract:=kb_bid_v2_sha256_bytes(convert_to(kb_bid_v2_jcs(jsonb_build_object(
    'version',2,'keys',to_jsonb(required_keys),'evidence','role-local-current-candidates-and-originals','finding_identity','content-sha256')),'UTF8'));
  IF p_review->>'contract_sha256' IS DISTINCT FROM expected_contract
    OR p_review->>'analysis_sha256' IS DISTINCT FROM kb_bid_v2_sha256_bytes(convert_to(kb_bid_v2_jcs(p_analysis),'UTF8'))::text
    OR jsonb_typeof(p_review->'global_checks') IS DISTINCT FROM 'array'
    OR jsonb_array_length(p_review->'global_checks')<>cardinality(required_keys)
    OR jsonb_typeof(p_review->'findings') IS DISTINCT FROM 'array' THEN RETURN false; END IF;
  coverage:=p_review->'coverage';
  FOR check_value IN SELECT value FROM jsonb_array_elements(p_review->'global_checks') LOOP
    key_value:=check_value->>'key';
    IF key_value IS NULL OR NOT key_value=ANY(required_keys) OR key_value=ANY(seen_keys)
      OR NOT kb_bid_v2_json_keys_exact(check_value,ARRAY['key','scope_sha256','conclusion','grounds','record_ids','finding_ids'])
      OR check_value->>'scope_sha256' IS DISTINCT FROM expected_scope
      OR p_analysis#>ARRAY['review_global_checks',key_value] IS DISTINCT FROM check_value
      OR jsonb_typeof(check_value->'grounds') IS DISTINCT FROM 'array'
      OR jsonb_typeof(check_value->'record_ids') IS DISTINCT FROM 'array'
      OR jsonb_typeof(check_value->'finding_ids') IS DISTINCT FROM 'array'
      OR coalesce(check_value->>'conclusion','') NOT IN ('pass','findings','source_limited')
      OR ((check_value->>'conclusion'='findings') IS DISTINCT FROM (jsonb_array_length(check_value->'finding_ids')>0))
      OR EXISTS(SELECT 1 FROM jsonb_array_elements(check_value->'record_ids') v WHERE jsonb_typeof(v)<>'string')
      OR EXISTS(SELECT 1 FROM jsonb_array_elements(check_value->'finding_ids') v WHERE jsonb_typeof(v)<>'string')
      OR EXISTS(SELECT 1 FROM jsonb_array_elements_text(check_value->'record_ids') v GROUP BY v HAVING count(*)>1)
      OR EXISTS(SELECT 1 FROM jsonb_array_elements_text(check_value->'finding_ids') v GROUP BY v HAVING count(*)>1)
    THEN RETURN false; END IF;
    seen_keys:=array_append(seen_keys,key_value);
    limit_source:=EXISTS(SELECT 1 FROM jsonb_array_elements(p_input->'documents') d
      WHERE d->>'disposition' IS NOT NULL AND d->>'disposition'<>'ready');
    frozen_missing:=limit_source;
    FOR record_id IN SELECT value FROM jsonb_array_elements_text(check_value->'record_ids') LOOP
      record_value:=p_analysis#>ARRAY['records',record_id];
      IF record_value IS NULL OR coverage#>>ARRAY['candidate','record:'||record_id]
        IS DISTINCT FROM kb_bid_v2_sha256_bytes(convert_to(kb_bid_v2_jcs(record_value),'UTF8'))::text THEN RETURN false; END IF;
      limit_source:=limit_source OR record_value#>>'{data,kind}'='unresolved'
        OR record_value#>>'{data,applicability,state}'='unknown'
        OR record_value#>>'{data,strength}'='unknown'
        OR EXISTS(SELECT 1 FROM jsonb_array_elements(coalesce(record_value#>'{data,compliance}','[]'::jsonb)) c WHERE c->>'policy'='unknown');
      limit_source:=coalesce(limit_source,false);
    END LOOP;
    FOR finding_id IN SELECT value FROM jsonb_array_elements_text(check_value->'finding_ids') LOOP
      IF NOT EXISTS(SELECT 1 FROM jsonb_array_elements(p_review->'findings') f
        WHERE kb_bid_v2_sha256_bytes(convert_to(kb_bid_v2_jcs(f),'UTF8'))::text=finding_id) THEN RETURN false; END IF;
    END LOOP;
    IF jsonb_array_length(check_value->'grounds')=0 THEN
      IF (jsonb_array_length(p_input->'source_units')>0 OR p_analysis->'records'<>'{}'::jsonb)
        AND key_value<>'collection_consistency'
        AND NOT (check_value->>'conclusion'='source_limited' AND frozen_missing) THEN RETURN false; END IF;
      IF jsonb_array_length(p_input->'documents')>0 THEN
        IF NOT EXISTS(SELECT 1 FROM jsonb_array_elements(coalesce(coverage#>'{metadata,documents}','[]'::jsonb)) r
          WHERE (r->>0)::bigint<=0 AND (r->>1)::bigint>=jsonb_array_length(p_input->'documents')) THEN RETURN false; END IF;
      ELSIF jsonb_array_length(p_input->'source_units')>0 OR p_analysis->'records'<>'{}'::jsonb THEN RETURN false;
      END IF;
    END IF;
    FOR span_value IN SELECT value FROM jsonb_array_elements(check_value->'grounds') LOOP
      SELECT s INTO source_value FROM jsonb_array_elements(p_input->'source_units') s
        WHERE s->>'source_unit_revision_id'=span_value->>'source_id';
      IF source_value IS NULL THEN RETURN false; END IF;
      start_value:=(span_value->>'start')::bigint; end_value:=(span_value->>'end')::bigint;
      IF start_value IS NULL OR end_value IS NULL OR start_value<0 OR end_value<0 THEN RETURN false; END IF;
      limit_source:=limit_source OR coalesce(p_analysis#>>ARRAY['dispositions',span_value->>'source_id','state']='unresolved',false);
      IF jsonb_typeof(span_value->'grid_cell')='object' THEN
        IF start_value<>0 OR end_value<>0 OR coalesce(span_value->'view_id','null'::jsonb)<>'null'::jsonb THEN RETURN false; END IF;
        SELECT f INTO form_value FROM jsonb_array_elements(p_input->'structured_forms') f
          WHERE f->>'form_definition_revision_id'=span_value#>>'{grid_cell,form_id}'
            AND f->>'source_unit_revision_id'=span_value->>'source_id';
        IF form_value IS NULL OR NOT EXISTS(SELECT 1 FROM jsonb_array_elements(form_value#>'{definition,cells}') c
          WHERE c->'row'=span_value#>'{grid_cell,row}' AND c->'column'=span_value#>'{grid_cell,column}') THEN RETURN false; END IF;
        cell_index:=(span_value#>>'{grid_cell,row}')::bigint*(form_value#>>'{definition,column_count}')::bigint
          +(span_value#>>'{grid_cell,column}')::bigint;
        IF NOT EXISTS(SELECT 1 FROM jsonb_array_elements(coalesce(coverage#>ARRAY['form_cells',span_value#>>'{grid_cell,form_id}'],'[]'::jsonb)) r
          WHERE (r->>0)::bigint<=cell_index AND (r->>1)::bigint>=cell_index+1) THEN RETURN false; END IF;
      ELSIF span_value->>'view_id' IS NOT NULL THEN
        IF start_value<>0 OR end_value<>0 OR coverage#>>ARRAY['views',span_value->>'view_id','source_id']
          IS DISTINCT FROM span_value->>'source_id' THEN RETURN false; END IF;
      ELSE
        source_bytes:=convert_to(source_value->>'text','UTF8');
        IF start_value>=end_value OR end_value>octet_length(source_bytes) THEN RETURN false; END IF;
        PERFORM convert_from(substring(source_bytes FROM start_value::integer+1 FOR (end_value-start_value)::integer),'UTF8');
        IF NOT EXISTS(SELECT 1 FROM jsonb_array_elements(coalesce(coverage#>ARRAY['text',span_value->>'source_id'],'[]'::jsonb)) r
          WHERE (r->>0)::bigint<=start_value AND (r->>1)::bigint>=end_value) THEN RETURN false; END IF;
      END IF;
    END LOOP;
    IF check_value->>'conclusion'='source_limited' AND NOT limit_source THEN RETURN false; END IF;
  END LOOP;
  RETURN cardinality(seen_keys)=cardinality(required_keys);
EXCEPTION WHEN data_exception THEN RETURN false;
END $$;

CREATE FUNCTION kb_bid_v2_publish_requirement_set_v4(
  p_request_artifact_id uuid,p_request_revision bigint,p_frozen_input_sha256 kb_sha256,
  p_compiled jsonb,p_actor kb_actor_identity,p_attempt integer,p_token uuid,
  p_docx_staging uuid DEFAULT NULL
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE
  checkpoint jsonb; final_disposition uuid:=gen_random_uuid(); final_disposition_revision bigint;
  final_disposition_payload bytea; final_disposition_sha kb_sha256; disposition_items jsonb;
  can_publish boolean; original_disposition uuid;
  request_value bid_async_request_snapshot_artifacts%ROWTYPE;
  typed bid_requirement_set_compile_request_identities%ROWTYPE;
  prior bid_async_stage_receipts%ROWTYPE; source_value bid_source_unit_revision_artifacts%ROWTYPE;
  requirement_value jsonb; source_identity jsonb; requirement_id uuid; requirement_lineage uuid;
  requirement_revision bigint; requirement_payload bytea; requirement_sha kb_sha256;
  requirement_text bytea; requirement_text_sha kb_sha256; fulfillment jsonb; applicability_value jsonb;
  requirement_items jsonb:='[]'::jsonb; ordinal_value integer:=0;
  set_id uuid:=gen_random_uuid(); set_revision bigint; set_payload bytea; set_sha kb_sha256;
  workspace_value bid_submission_workspaces%ROWTYPE;
  projection_head bid_workspace_requirement_projection_current%ROWTYPE;
  projection_id uuid:=gen_random_uuid(); projection_revision bigint;
  projection_payload bytea; projection_sha kb_sha256; publication_status text;
  result_value jsonb; result_sha kb_sha256; global_input jsonb; first_tools jsonb; global_contract boolean;
  draft_bytes bytea; draft_sha kb_sha256; draft_length bigint; filled_draft boolean;
  draft_round uuid; draft_version uuid; draft_revision bigint; draft_payload bytea; draft_payload_sha kb_sha256;
  docx_head bid_docx_current%ROWTYPE; draft_docx jsonb;
BEGIN
  PERFORM kb_bid_v2_tender_agent_lock_owner(p_request_artifact_id,p_frozen_input_sha256,p_attempt,p_token);
  checkpoint:=kb_bid_v2_tender_agent_checkpoint_get(p_request_artifact_id,p_frozen_input_sha256);
  SELECT convert_from(provider_body,'UTF8')::jsonb->'tools' INTO first_tools
    FROM bid_tender_agent_call_attempts WHERE request_artifact_id=p_request_artifact_id
      AND frozen_input_sha256=p_frozen_input_sha256 ORDER BY batch_ordinal,call_ordinal LIMIT 1;
  global_contract:=EXISTS(SELECT 1 FROM jsonb_array_elements(coalesce(first_tools,'[]'::jsonb)) t
    WHERE t#>>'{function,name}'='put_analysis_check');
  IF global_contract AND p_compiled#>'{analysis_result,schema_version}' IS DISTINCT FROM '2'::jsonb THEN
    RAISE EXCEPTION 'AGENT_OUTPUT_INVALID: global-check contract cannot downgrade to v1' USING ERRCODE='23514';
  END IF;
  IF p_compiled#>'{analysis_result,review,draft}' = 'true'::jsonb THEN
    IF checkpoint->'done' IS DISTINCT FROM 'true'::jsonb
        OR checkpoint#>'{journal,pending}' IS DISTINCT FROM 'null'::jsonb
        OR p_compiled#>'{analysis_result,analysis}' IS DISTINCT FROM checkpoint->'analysis'
        OR p_compiled#>'{analysis_result,review}' IS DISTINCT FROM checkpoint->'review'
        OR p_compiled#>'{analysis_result,source_views}' IS DISTINCT FROM checkpoint->'source_views'
        OR p_compiled#>>'{analysis_result,frozen_input_sha256}' IS DISTINCT FROM checkpoint->>'input_sha256'
        OR p_compiled#>'{analysis_result,schema_version}' IS DISTINCT FROM '2'::jsonb
        OR jsonb_typeof(checkpoint#>'{source_review}') IS DISTINCT FROM 'null'
        OR jsonb_typeof(p_compiled#>'{analysis_result,review,findings}') IS DISTINCT FROM 'array'
        OR jsonb_array_length(p_compiled#>'{analysis_result,review,findings}')<>0
        OR p_compiled#>>'{analysis_result,quality}' IS DISTINCT FROM 'needs_review'
        OR jsonb_typeof(p_compiled#>'{analysis_result,analysis,draft_plan}') IS DISTINCT FROM 'array'
        OR jsonb_array_length(coalesce(p_compiled#>'{analysis_result,analysis,draft_plan}','[]'::jsonb))=0 THEN
      RAISE EXCEPTION 'AGENT_OUTPUT_INVALID: draft analysis checkpoint missing or forged independent review' USING ERRCODE='23514';
    END IF;
  ELSE
  IF p_compiled#>'{analysis_result,schema_version}'='2'::jsonb THEN
    global_input:=kb_bid_v2_load_tender_analysis_input(p_request_artifact_id,p_request_revision,p_frozen_input_sha256)->'input';
    IF NOT kb_bid_v2_analysis_global_review_valid(global_input,p_compiled#>'{analysis_result,analysis}',p_compiled#>'{analysis_result,review}') THEN
      RAISE EXCEPTION 'AGENT_OUTPUT_INVALID: global analysis checks missing, stale or unsupported' USING ERRCODE='23514';
    END IF;
  END IF;
  IF checkpoint#>'{main_progress,blockers}' IS DISTINCT FROM '[]'::jsonb
      OR checkpoint#>'{reviewer_progress,blockers}' IS DISTINCT FROM '[]'::jsonb
      OR checkpoint->'done' IS DISTINCT FROM 'true'::jsonb
      OR checkpoint#>'{source_review,schema_version}' IS DISTINCT FROM '1'::jsonb
      OR checkpoint#>'{source_review,active_task}' IS DISTINCT FROM 'null'::jsonb
      OR checkpoint#>'{source_review,completed_analysis_sha256}' IS DISTINCT FROM checkpoint#>'{review,analysis_sha256}'
      OR jsonb_typeof(checkpoint#>'{source_review,results}') IS DISTINCT FROM 'object'
      OR EXISTS(SELECT 1 FROM jsonb_each(checkpoint#>'{source_review,results}') item
        WHERE coalesce(item.value#>>'{judgment,status}','') NOT IN ('checked','findings'))
      OR checkpoint#>'{journal,pending}' IS DISTINCT FROM 'null'::jsonb
      OR p_compiled#>'{analysis_result,analysis}' IS DISTINCT FROM checkpoint->'analysis'
      OR p_compiled#>'{analysis_result,review}' IS DISTINCT FROM checkpoint->'review'
      OR p_compiled#>'{analysis_result,source_views}' IS DISTINCT FROM checkpoint->'source_views'
      OR p_compiled#>>'{analysis_result,frozen_input_sha256}' IS DISTINCT FROM checkpoint->>'input_sha256'
      OR p_compiled#>'{analysis_result,schema_version}' NOT IN ('1'::jsonb,'2'::jsonb) THEN
    RAISE EXCEPTION 'AGENT_OUTPUT_INVALID: independent review checkpoint missing or changed' USING ERRCODE='23514';
  END IF;
  END IF;
  IF p_actor<>'system:requirement-set-compile-v4' THEN
    RAISE EXCEPTION 'SYSTEM_ACTOR_REQUIRED' USING ERRCODE='42501';
  END IF;
  IF p_compiled#>'{analysis_result,review,draft}' IS DISTINCT FROM 'true'::jsonb AND p_docx_staging IS NOT NULL THEN
    RAISE EXCEPTION 'AGENT_OUTPUT_INVALID: official analysis cannot stage draft DOCX' USING ERRCODE='23514';
  END IF;
  SELECT * INTO STRICT request_value FROM bid_async_request_snapshot_artifacts
    WHERE id=p_request_artifact_id AND request_kind='requirement_set_compile'
      AND revision=p_request_revision AND frozen_input_sha256=p_frozen_input_sha256 FOR UPDATE;
  SELECT * INTO STRICT typed FROM bid_requirement_set_compile_request_identities
    WHERE request_artifact_id=p_request_artifact_id;
  SELECT * INTO prior FROM bid_async_stage_receipts WHERE request_artifact_id=p_request_artifact_id
    AND stage_kind='requirement_compile' AND frozen_input_sha256=p_frozen_input_sha256;
  IF FOUND THEN RETURN prior.result_identity||jsonb_build_object('replayed',true); END IF;
  IF request_value.status<>'pending' THEN
    RAISE EXCEPTION 'REQUIREMENT_COMPILE_REQUEST_NOT_PENDING' USING ERRCODE='23514';
  END IF;
  IF jsonb_typeof(p_compiled)<>'object'
    OR NOT kb_bid_v2_json_keys_exact(p_compiled,ARRAY['schema_version','source_unit_revision_ids','requirements','notices','analysis_result'])
    OR p_compiled->'schema_version' IS DISTINCT FROM '4'::jsonb
    OR jsonb_typeof(p_compiled->'source_unit_revision_ids')<>'array'
    OR jsonb_typeof(p_compiled->'requirements')<>'array'
    OR jsonb_array_length(p_compiled->'requirements') NOT BETWEEN 0 AND 100000
    OR jsonb_typeof(p_compiled->'notices')<>'array' THEN
    RAISE EXCEPTION 'REQUIREMENT_COMPILE_OUTPUT_INVALID' USING ERRCODE='23514';
  END IF;
  IF EXISTS (
    (SELECT source.id FROM bid_source_unit_disposition_set_items disposition
      JOIN bid_source_unit_revision_artifacts source
        ON source.project_id=disposition.project_id AND source.id=disposition.source_unit_revision_id
      WHERE disposition.disposition_set_id=typed.disposition_set_revision_id)
    EXCEPT
    (SELECT value::uuid FROM jsonb_array_elements_text(p_compiled->'source_unit_revision_ids') value)
  ) OR EXISTS (
    (SELECT value::uuid FROM jsonb_array_elements_text(p_compiled->'source_unit_revision_ids') value)
    EXCEPT
    (SELECT source.id FROM bid_source_unit_disposition_set_items disposition
      JOIN bid_source_unit_revision_artifacts source
        ON source.project_id=disposition.project_id AND source.id=disposition.source_unit_revision_id
      WHERE disposition.disposition_set_id=typed.disposition_set_revision_id)
  ) THEN
    RAISE EXCEPTION 'REQUIREMENT_COMPILE_SOURCE_CLOSURE_INVALID' USING ERRCODE='23514';
  END IF;
  PERFORM 1 FROM bid_projects WHERE id=typed.project_id FOR UPDATE;
  original_disposition:=typed.disposition_set_revision_id;
  IF p_compiled#>'{analysis_result,review,draft}' IS DISTINCT FROM 'true'::jsonb
      AND ((SELECT count(*) FROM jsonb_object_keys(coalesce(p_compiled#>'{analysis_result,analysis,dispositions}','{}'::jsonb)))
      <>jsonb_array_length(p_compiled->'source_unit_revision_ids') OR EXISTS(
      SELECT 1 FROM jsonb_object_keys(p_compiled#>'{analysis_result,analysis,dispositions}') source_id
      WHERE NOT (p_compiled->'source_unit_revision_ids' ? source_id))) THEN
    RAISE EXCEPTION 'AGENT_OUTPUT_INVALID: source partition incomplete' USING ERRCODE='23514';
  END IF;
  -- Only the current frozen input may advance the source decision chain.
  -- Late results retain their original basis and analysis in history; allocating
  -- a new decision revision for them would consume a future current generation.
  SELECT d.artifact_id=typed.document_set_revision_id AND u.artifact_id=original_disposition,
      u.generation+1 INTO can_publish,final_disposition_revision
    FROM bid_document_set_current d JOIN bid_source_unit_disposition_set_current u ON u.scope_id=d.scope_id
    WHERE d.scope_id=typed.project_id FOR UPDATE OF d,u;
  IF can_publish AND p_compiled#>'{analysis_result,review,draft}' IS DISTINCT FROM 'true'::jsonb THEN
    SELECT coalesce(jsonb_agg(jsonb_build_object('source_unit_revision_id',key,'disposition',value->'state','reason',value->'reason') ORDER BY key),'[]'::jsonb)
      INTO disposition_items FROM jsonb_each(p_compiled#>'{analysis_result,analysis,dispositions}');
    final_disposition_payload:=kb_bid_v2_json_payload(jsonb_build_object('schema_version',1,'project_id',typed.project_id,
      'document_set_id',typed.document_set_revision_id,'revision',final_disposition_revision,'items',disposition_items));
    final_disposition_sha:=kb_bid_v2_sha256_bytes(final_disposition_payload);
    INSERT INTO bid_source_unit_disposition_set_artifacts(id,project_id,document_set_id,document_set_sequence,
      revision,canonical_payload,content_sha256,actor)
    SELECT final_disposition,typed.project_id,typed.document_set_revision_id,revision,final_disposition_revision,
      final_disposition_payload,final_disposition_sha,p_actor FROM bid_document_set_artifacts WHERE id=typed.document_set_revision_id;
    INSERT INTO bid_source_unit_disposition_set_items(disposition_set_id,project_id,source_unit_revision_id,disposition,reason)
      SELECT final_disposition,typed.project_id,key::uuid,value->>'state',value->>'reason'
      FROM jsonb_each(p_compiled#>'{analysis_result,analysis,dispositions}');
    IF NOT kb_bid_v2_advance_disposition_set(typed.project_id,original_disposition,
        typed.disposition_set_sha256,final_disposition,final_disposition_sha) THEN
      RAISE EXCEPTION 'DISPOSITION_SET_CAS_MISMATCH' USING ERRCODE='40001';
    END IF;
    typed.disposition_set_revision_id:=final_disposition;
    typed.disposition_set_sha256:=final_disposition_sha;
  END IF;

  SELECT coalesce(max(revision),0)+1 INTO set_revision
    FROM bid_requirement_set_artifacts WHERE project_id=typed.project_id;
  FOR requirement_value IN SELECT value FROM jsonb_array_elements(p_compiled->'requirements') LOOP
    IF jsonb_typeof(requirement_value)<>'object'
      OR NOT kb_bid_v2_json_keys_exact(requirement_value,ARRAY['requirement_ref','requirement_kind','requiredness',
        'compliance_policy','requirement_text','response_needs','applicability','source_unit_revision_ids','structured_form_revision_ids','source_spans'])
      OR NOT kb_bid_v2_sha256_text(requirement_value->>'requirement_ref')
      OR requirement_value->>'requirement_kind' NOT IN ('qualification','technical','commercial','pricing','delivery','evaluation','format','attachment','other','personnel','rejection')
      OR requirement_value->>'requiredness' NOT IN ('mandatory','optional','informational','unknown')
      OR requirement_value->>'compliance_policy' NOT IN ('must_comply','explicit_response','deviation_allowed','scored','unknown')
      OR jsonb_typeof(requirement_value->'response_needs') IS DISTINCT FROM 'array'
      OR (requirement_value->>'compliance_policy'='explicit_response'
        AND jsonb_array_length(requirement_value->'response_needs')=0)
      OR octet_length(requirement_value->>'requirement_text') NOT BETWEEN 1 AND 32768
      OR jsonb_typeof(requirement_value->'source_unit_revision_ids')<>'array'
      OR jsonb_array_length(requirement_value->'source_unit_revision_ids') NOT BETWEEN 1 AND 1000
      OR jsonb_typeof(requirement_value->'structured_form_revision_ids')<>'array'
      OR jsonb_typeof(requirement_value->'applicability')<>'object'
      OR NOT kb_bid_v2_json_keys_exact(requirement_value->'applicability',ARRAY['status','reason','source_unit_revision_ids'])
      OR requirement_value->'applicability'->>'status' NOT IN ('required','optional','conditional','not_applicable','unknown') THEN
      RAISE EXCEPTION 'REQUIREMENT_COMPILE_REQUIREMENT_INVALID' USING ERRCODE='23514';
    END IF;
    IF EXISTS (SELECT 1 FROM jsonb_array_elements_text(requirement_value->'source_unit_revision_ids') source_id
      WHERE NOT EXISTS (SELECT 1 FROM bid_source_unit_disposition_set_items disposition
        WHERE disposition.disposition_set_id=typed.disposition_set_revision_id
          AND disposition.source_unit_revision_id=source_id::uuid)) THEN
      RAISE EXCEPTION 'REQUIREMENT_COMPILE_SOURCE_SCOPE_INVALID' USING ERRCODE='23514';
    END IF;
    IF EXISTS (SELECT 1 FROM jsonb_array_elements_text(requirement_value->'structured_form_revision_ids') form_id
      WHERE NOT EXISTS (SELECT 1 FROM bid_tender_structured_form_definition_artifacts form
        WHERE form.project_id=typed.project_id AND form.id=form_id::uuid
          AND form.source_unit_revision_id IN (SELECT source_id::uuid
            FROM jsonb_array_elements_text(requirement_value->'source_unit_revision_ids') source_id))) THEN
      RAISE EXCEPTION 'REQUIREMENT_COMPILE_FORM_SCOPE_INVALID' USING ERRCODE='23514';
    END IF;
    requirement_id:=gen_random_uuid();
    requirement_lineage:=kb_bid_v2_deterministic_uuid(
      typed.project_id::text||':requirement-v4:'||(requirement_value->>'requirement_ref'));
    SELECT coalesce(max(revision),0)+1 INTO requirement_revision
      FROM bid_requirement_revision_artifacts
      WHERE project_id=typed.project_id AND lineage_id=requirement_lineage;
    requirement_text:=convert_to(requirement_value->>'requirement_text','UTF8');
    requirement_text_sha:=kb_bid_v2_sha256_bytes(requirement_text);
    IF EXISTS (SELECT 1 FROM jsonb_array_elements(requirement_value->'response_needs') need
        WHERE need->>'channel' IS NULL OR need->>'channel' NOT IN
          ('narrative_content','response_table','deviation_statement','structured_form','evidence_attachment','quotation')) THEN
      RAISE EXCEPTION 'AGENT_OUTPUT_INVALID: response channel invalid' USING ERRCODE='23514';
    END IF;
    SELECT jsonb_build_object('kind','all_of','children',coalesce(jsonb_agg(
      jsonb_build_object('kind','need','need_occurrence_id',
        kb_bid_v2_deterministic_uuid(requirement_id::text||':response:'||position::text),
        'channel',need->>'channel') ORDER BY position),'[]'::jsonb)) INTO fulfillment
      FROM jsonb_array_elements(requirement_value->'response_needs') WITH ORDINALITY entry(need,position);
    SELECT jsonb_build_object('fragments',jsonb_agg('source_unit:'||(source_id#>>'{}') ORDER BY source_id#>>'{}'))
      INTO applicability_value
      FROM jsonb_array_elements(requirement_value->'source_unit_revision_ids') source_id;
    requirement_payload:=kb_bid_v2_json_payload(jsonb_build_object('schema_version',1,
      'lineage_id',requirement_lineage,'revision',requirement_revision,
      'compiler_ref',requirement_value->>'requirement_ref',
      'requirement_kind',requirement_value->>'requirement_kind',
      'requiredness',requirement_value->>'requiredness',
      'compliance_policy',requirement_value->>'compliance_policy','lifecycle','current',
      'text',requirement_value->>'requirement_text','fulfillment_expr',fulfillment,
      'applicability',applicability_value,
      'compiled_applicability',requirement_value->'applicability',
      'response_needs',requirement_value->'response_needs',
      'structured_form_revision_ids',requirement_value->'structured_form_revision_ids'));
    requirement_sha:=kb_bid_v2_sha256_bytes(requirement_payload);
    INSERT INTO bid_requirement_revision_artifacts(id,project_id,lineage_id,revision,requirement_kind,
      requiredness,compliance_policy,lifecycle,text_utf8,text_sha256,fulfillment_expr,applicability,tombstone,
      canonical_payload,content_sha256,actor)
    VALUES(requirement_id,typed.project_id,requirement_lineage,requirement_revision,
      requirement_value->>'requirement_kind',requirement_value->>'requiredness',
      requirement_value->>'compliance_policy','current',requirement_text,requirement_text_sha,
      fulfillment,applicability_value,false,requirement_payload,requirement_sha,p_actor);
    FOR source_identity IN SELECT value FROM jsonb_array_elements(requirement_value->'source_spans') LOOP
      SELECT * INTO STRICT source_value FROM bid_source_unit_revision_artifacts
        WHERE project_id=typed.project_id AND id=(source_identity->>'source_id')::uuid;
      IF NOT (requirement_value->'source_unit_revision_ids' ? (source_identity->>'source_id'))
          OR (source_identity->>'start')::bigint<0
          OR (source_identity->>'end')::bigint<=(source_identity->>'start')::bigint
          OR (source_identity->>'end')::bigint>octet_length(source_value.text_utf8) THEN
        RAISE EXCEPTION 'AGENT_OUTPUT_INVALID: source span invalid' USING ERRCODE='23514';
      END IF;
      -- UTF-8 decoding rejects offsets inside a multibyte character.
      PERFORM convert_from(substring(source_value.text_utf8 FROM (source_identity->>'start')::integer+1
        FOR (source_identity->>'end')::integer-(source_identity->>'start')::integer),'UTF8');
      INSERT INTO bid_requirement_source_revision_artifacts(id,project_id,requirement_revision_id,
        source_unit_revision_id,quote_start_offset,quote_end_offset,quote_sha256)
      VALUES(gen_random_uuid(),typed.project_id,requirement_id,source_value.id,
        (source_identity->>'start')::bigint,(source_identity->>'end')::bigint,
        kb_bid_v2_sha256_bytes(substring(source_value.text_utf8 FROM (source_identity->>'start')::integer+1
          FOR (source_identity->>'end')::integer-(source_identity->>'start')::integer))) ON CONFLICT DO NOTHING;
    END LOOP;
    requirement_items:=requirement_items||jsonb_build_array(jsonb_build_object(
      'requirement_revision_id',requirement_id,'content_sha256',requirement_sha,
      'effective_applicability',applicability_value,'ordinal',ordinal_value));
    ordinal_value:=ordinal_value+1;
  END LOOP;
  set_payload:=kb_bid_v2_json_payload(jsonb_build_object('schema_version',1,'project_id',typed.project_id,
    'document_set_revision_id',typed.document_set_revision_id,
    'disposition_set_revision_id',typed.disposition_set_revision_id,'revision',set_revision,
    'compiler_version',4,'analysis_result',p_compiled->'analysis_result','items',requirement_items));
  set_sha:=kb_bid_v2_sha256_bytes(set_payload);
  INSERT INTO bid_requirement_set_artifacts(id,project_id,document_set_id,document_set_sequence,
    disposition_set_id,disposition_set_sequence,revision,canonical_payload,content_sha256)
  SELECT set_id,typed.project_id,typed.document_set_revision_id,d.revision,
    typed.disposition_set_revision_id,s.revision,set_revision,set_payload,set_sha
    FROM bid_document_set_artifacts d,bid_source_unit_disposition_set_artifacts s
    WHERE d.id=typed.document_set_revision_id AND s.id=typed.disposition_set_revision_id;
  INSERT INTO bid_requirement_set_items(requirement_set_id,project_id,requirement_revision_id,effective_applicability,ordinal)
  SELECT set_id,typed.project_id,(value->>'requirement_revision_id')::uuid,value->'effective_applicability',
    (value->>'ordinal')::integer FROM jsonb_array_elements(requirement_items);
  PERFORM kb_bid_v2_tender_agent_lock_owner(p_request_artifact_id,p_frozen_input_sha256,p_attempt,p_token);
  IF can_publish THEN
    publication_status:=kb_bid_v2_publish_requirement_set(set_id,set_sha);
  ELSE
    publication_status:='superseded';
  END IF;
  IF publication_status='superseded' THEN
    result_value:=jsonb_build_object('status','succeeded','published_current',false,
      'workspace_apply_required',false,'requirement_set_id',set_id,'requirement_set_sha256',set_sha,
      'document_set_revision_id',typed.document_set_revision_id,
      'document_set_sha256',typed.document_set_sha256,'requirement_count',ordinal_value,
      'compiler_version',4,'analysis_quality',p_compiled#>'{analysis_result,quality}','replayed',false);
    result_sha:=kb_bid_v2_sha256_bytes(convert_to(result_value::text,'UTF8'));
    INSERT INTO bid_async_stage_receipts(request_artifact_id,stage_kind,frozen_input_sha256,result_identity,result_sha256)
    VALUES(p_request_artifact_id,'requirement_compile',p_frozen_input_sha256,result_value,result_sha);
    UPDATE bid_tender_agent_run_artifacts SET status='succeeded',progress_phase='succeeded',
      lease_expires_at=least(lease_expires_at,clock_timestamp()),updated_at=clock_timestamp()
      WHERE request_artifact_id=p_request_artifact_id AND attempt=p_attempt;
    UPDATE bid_async_request_snapshot_artifacts SET status='succeeded',result_identity=result_value,
      finished_at=clock_timestamp() WHERE id=p_request_artifact_id;
    RETURN result_value;
  ELSIF publication_status<>'published' THEN
    RAISE EXCEPTION 'REQUIREMENT_SET_PUBLICATION_STATUS_INVALID' USING ERRCODE='23514';
  END IF;
  SELECT * INTO STRICT workspace_value FROM bid_submission_workspaces WHERE project_id=typed.project_id;
  SELECT * INTO STRICT projection_head FROM bid_workspace_requirement_projection_current
    WHERE scope_id=workspace_value.id FOR UPDATE;
  projection_revision:=projection_head.generation+1;
  projection_payload:=kb_bid_v2_json_payload(jsonb_build_object('schema_version',1,
    'workspace_id',workspace_value.id,'requirement_set_id',set_id,
    'revision',projection_revision,'compiler_version',4,'items',requirement_items));
  projection_sha:=kb_bid_v2_sha256_bytes(projection_payload);
  INSERT INTO bid_workspace_requirement_projection_artifacts(id,project_id,workspace_id,requirement_set_id,
    revision,canonical_payload,content_sha256)
  VALUES(projection_id,typed.project_id,workspace_value.id,set_id,projection_revision,projection_payload,projection_sha);
  INSERT INTO bid_workspace_requirement_projection_items(projection_id,project_id,requirement_revision_id,effective_applicability,ordinal)
  SELECT projection_id,typed.project_id,(value->>'requirement_revision_id')::uuid,value->'effective_applicability',
    (value->>'ordinal')::integer FROM jsonb_array_elements(requirement_items);
  IF NOT kb_bid_v2_advance_requirement_projection(typed.project_id,workspace_value.id,
      projection_head.artifact_id,projection_head.artifact_sha256,projection_id,projection_sha) THEN
    RAISE EXCEPTION 'REQUIREMENT_PROJECTION_CAS_MISMATCH' USING ERRCODE='40001';
  END IF;
  draft_docx:='null'::jsonb;
  filled_draft:=EXISTS(SELECT 1 FROM jsonb_array_elements(coalesce(p_compiled#>'{analysis_result,analysis,draft_plan}','[]'::jsonb)) item
    WHERE item->>'status'='filled');
  IF p_compiled#>'{analysis_result,review,draft}' = 'true'::jsonb THEN
    IF filled_draft THEN
      IF p_docx_staging IS NULL THEN
        RAISE EXCEPTION 'AGENT_OUTPUT_INVALID: filled draft requires staged DOCX' USING ERRCODE='23514';
      END IF;
      draft_bytes:=decode(checkpoint->>'draft_docx_base64','base64');
      draft_sha:=kb_bid_v2_sha256_bytes(draft_bytes);
      draft_length:=octet_length(draft_bytes);
      IF draft_bytes IS NULL OR draft_length<=0
          OR checkpoint->>'draft_compile_object_id' IS DISTINCT FROM ('objects/'||draft_sha::text) THEN
        RAISE EXCEPTION 'AGENT_OUTPUT_INVALID: draft DOCX bytes missing or digest mismatch' USING ERRCODE='23514';
      END IF;
      PERFORM 1 FROM bid_submission_workspaces WHERE id=workspace_value.id FOR UPDATE;
      SELECT * INTO docx_head FROM bid_docx_current WHERE scope_id=workspace_value.id FOR UPDATE;
      IF docx_head.editor_key IS NOT NULL OR docx_head.pending_save_id IS NOT NULL THEN
        RAISE EXCEPTION 'DOCX_VERSION_CAS_MISMATCH' USING ERRCODE='40001';
      END IF;
      draft_round:=gen_random_uuid();
      draft_version:=gen_random_uuid();
      SELECT coalesce(max(revision),0)+1 INTO draft_revision FROM bid_docx_round_artifacts WHERE workspace_id=workspace_value.id;
      draft_payload:=kb_bid_v2_json_payload(jsonb_build_object('schema_version',1,
        'project_id',typed.project_id,'workspace_id',workspace_value.id,'revision',draft_revision,
        'document_set_id',typed.document_set_revision_id,'document_set_sha256',typed.document_set_sha256,
        'requirement_set_id',set_id,'requirement_set_sha256',set_sha));
      draft_payload_sha:=kb_bid_v2_sha256_bytes(draft_payload);
      PERFORM kb_object_upload_commit(p_docx_staging,'objects/'||draft_sha,draft_sha,
        'application/vnd.openxmlformats-officedocument.wordprocessingml.document',draft_length,
        'bid_docx_version',draft_version,'document',p_actor);
      INSERT INTO bid_docx_round_artifacts(id,project_id,workspace_id,revision,document_set_id,
        requirement_set_id,canonical_payload,content_sha256,actor)
      VALUES(draft_round,typed.project_id,workspace_value.id,draft_revision,typed.document_set_revision_id,
        set_id,draft_payload,draft_payload_sha,p_actor);
      INSERT INTO bid_docx_version_artifacts(id,project_id,workspace_id,round_id,revision,parent_version_id,
        object_ref,docx_sha256,byte_length,actor)
      VALUES(draft_version,typed.project_id,workspace_value.id,draft_round,1,NULL,'objects/'||draft_sha,draft_sha,draft_length,p_actor);
      INSERT INTO bid_docx_current(scope_id,project_id,round_id,version_id,docx_sha256)
      VALUES(workspace_value.id,typed.project_id,draft_round,draft_version,draft_sha)
      ON CONFLICT(scope_id) DO UPDATE SET round_id=EXCLUDED.round_id,version_id=EXCLUDED.version_id,docx_sha256=EXCLUDED.docx_sha256,
        editor_key=NULL,editor_base_version_id=NULL,pending_save_id=NULL,editor_error=NULL;
      draft_docx:=jsonb_build_object('version_id',draft_version,'round_id',draft_round,'round_revision',draft_revision,
        'docx_sha256',draft_sha,'byte_length',draft_length,'object_ref','objects/'||draft_sha);
    ELSIF p_docx_staging IS NOT NULL THEN
      RAISE EXCEPTION 'AGENT_OUTPUT_INVALID: outline-only draft cannot stage DOCX' USING ERRCODE='23514';
    END IF;
  ELSIF p_docx_staging IS NOT NULL THEN
    RAISE EXCEPTION 'AGENT_OUTPUT_INVALID: official analysis cannot stage draft DOCX' USING ERRCODE='23514';
  END IF;
  result_value:=jsonb_build_object('status','succeeded','published_current',true,
    'workspace_apply_required',false,'requirement_set_id',set_id,'requirement_set_sha256',set_sha,
    'document_set_revision_id',typed.document_set_revision_id,
    'document_set_sha256',typed.document_set_sha256,'requirement_count',ordinal_value,
    'requirement_projection_id',projection_id,'requirement_projection_sha256',projection_sha,
    'compiler_version',4,'analysis_quality',p_compiled#>'{analysis_result,quality}','replayed',false,
    'draft_docx',draft_docx);
  result_sha:=kb_bid_v2_sha256_bytes(convert_to(result_value::text,'UTF8'));
  INSERT INTO bid_async_stage_receipts(request_artifact_id,stage_kind,frozen_input_sha256,result_identity,result_sha256)
  VALUES(p_request_artifact_id,'requirement_compile',p_frozen_input_sha256,result_value,result_sha);
  UPDATE bid_tender_agent_run_artifacts SET status='succeeded',progress_phase='succeeded',
      lease_expires_at=least(lease_expires_at,clock_timestamp()),updated_at=clock_timestamp()
      WHERE request_artifact_id=p_request_artifact_id AND attempt=p_attempt;
    UPDATE bid_async_request_snapshot_artifacts SET status='succeeded',result_identity=result_value,
    finished_at=clock_timestamp() WHERE id=p_request_artifact_id;
  RETURN result_value;
END $$;

-- Resolve a specific immutable published analysis, including its original input
-- decisions. Current source heads are deliberately not used for history reads.
CREATE FUNCTION kb_bid_v2_load_docx_composition_source(
  p_workspace_id uuid,p_basis jsonb,p_actor kb_actor_identity
) RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE project_value uuid; requirement_value bid_requirement_set_artifacts%ROWTYPE;
  source_request bid_async_request_snapshot_artifacts%ROWTYPE; payload jsonb; source_input jsonb;
BEGIN
  SELECT project_id INTO STRICT project_value FROM bid_submission_workspaces WHERE id=p_workspace_id;
  PERFORM kb_bid_v2_require_project_owner(project_value,p_actor);
  IF jsonb_typeof(p_basis) IS DISTINCT FROM 'object'
    OR NOT kb_bid_v2_json_keys_exact(p_basis,ARRAY['document_set_id','document_set_sha256','requirement_set_id','requirement_set_sha256'])
    OR NOT coalesce(kb_bid_v2_uuid_text(p_basis->>'document_set_id'),false)
    OR NOT coalesce(kb_bid_v2_uuid_text(p_basis->>'requirement_set_id'),false)
    OR NOT coalesce(kb_bid_v2_sha256_text(p_basis->>'document_set_sha256'),false)
    OR NOT coalesce(kb_bid_v2_sha256_text(p_basis->>'requirement_set_sha256'),false) THEN
    RAISE EXCEPTION 'DOCX_COMPOSITION_INPUT_INVALID' USING ERRCODE='23514';
  END IF;
  SELECT a.* INTO requirement_value FROM bid_requirement_set_artifacts a
    JOIN bid_document_set_artifacts d ON d.project_id=a.project_id AND d.id=a.document_set_id
    WHERE a.project_id=project_value AND a.id=(p_basis->>'requirement_set_id')::uuid
      AND a.content_sha256=p_basis->>'requirement_set_sha256'
      AND d.id=(p_basis->>'document_set_id')::uuid AND d.content_sha256=p_basis->>'document_set_sha256';
  IF NOT FOUND THEN RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: composition basis' USING ERRCODE='23514'; END IF;
  payload:=convert_from(requirement_value.canonical_payload,'UTF8')::jsonb;
  IF payload->'compiler_version' IS DISTINCT FROM '4'::jsonb
    OR jsonb_typeof(payload->'analysis_result') IS DISTINCT FROM 'object' THEN
    RAISE EXCEPTION 'DOCX_COMPOSITION_ANALYSIS_MISSING' USING ERRCODE='23514';
  END IF;
  IF payload#>'{analysis_result,review,draft}' = 'true'::jsonb THEN
    RAISE EXCEPTION 'DOCX_COMPOSITION_INPUT_INVALID: official composition rejects draft analysis' USING ERRCODE='23514';
  END IF;
  SELECT r.* INTO source_request FROM bid_async_stage_receipts receipt
    JOIN bid_async_request_snapshot_artifacts r ON r.id=receipt.request_artifact_id
      AND r.frozen_input_sha256=receipt.frozen_input_sha256
    JOIN bid_requirement_set_compile_request_identities i ON i.request_artifact_id=r.id
      AND i.document_set_revision_id=requirement_value.document_set_id
    WHERE r.project_id=project_value AND r.request_kind='requirement_set_compile' AND r.status='succeeded'
      AND receipt.stage_kind='requirement_compile'
      AND receipt.result_identity->>'requirement_set_id'=requirement_value.id::text
      AND receipt.result_identity->>'requirement_set_sha256'=requirement_value.content_sha256::text;
  IF NOT FOUND THEN RAISE EXCEPTION 'DOCX_COMPOSITION_ANALYSIS_MISSING' USING ERRCODE='23514'; END IF;
  source_input:=kb_bid_v2_load_tender_analysis_input(source_request.id,source_request.revision,source_request.frozen_input_sha256)->'input';
  IF payload#>>'{analysis_result,frozen_input_sha256}' IS DISTINCT FROM
      kb_bid_v2_sha256_bytes(convert_to(kb_bid_v2_jcs(source_input),'UTF8'))::text THEN
    RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: analysis source input' USING ERRCODE='23514';
  END IF;
  RETURN jsonb_build_object('source_request',jsonb_build_object('request_artifact_id',source_request.id,
      'request_revision',source_request.revision,'frozen_input_sha256',source_request.frozen_input_sha256),
    'input',source_input,'analysis',payload->'analysis_result');
END $$;

-- One statement snapshot checks the user's selected source/version identities.
-- The eventual new-round publication must repeat the existing CAS: drafting
-- does not reserve the current document or prevent the user from editing it.
CREATE FUNCTION kb_bid_v2_prepare_docx_composition_source(
  p_workspace_id uuid,p_basis jsonb,p_expected jsonb,p_actor kb_actor_identity
) RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE source_value jsonb; current_version jsonb;
BEGIN
  source_value:=kb_bid_v2_load_docx_composition_source(p_workspace_id,p_basis,p_actor);
  IF source_value#>'{analysis,review,draft}' = 'true'::jsonb THEN
    RAISE EXCEPTION 'DOCX_COMPOSITION_INPUT_INVALID: official composition rejects draft analysis' USING ERRCODE='23514';
  END IF;
  IF p_basis IS DISTINCT FROM kb_bid_v2_get_docx_round_basis(p_workspace_id,p_actor) THEN
    RAISE EXCEPTION 'DOCX_ROUND_BASIS_CHANGED' USING ERRCODE='40001';
  END IF;
  IF p_expected IS DISTINCT FROM 'null'::jsonb AND p_expected IS NOT NULL AND (
    jsonb_typeof(p_expected) IS DISTINCT FROM 'object'
    OR NOT kb_bid_v2_json_keys_exact(p_expected,ARRAY['version_id','docx_sha256'])
    OR NOT coalesce(kb_bid_v2_uuid_text(p_expected->>'version_id'),false)
    OR NOT coalesce(kb_bid_v2_sha256_text(p_expected->>'docx_sha256'),false)) THEN
    RAISE EXCEPTION 'DOCX_COMPOSITION_INPUT_INVALID' USING ERRCODE='23514';
  END IF;
  SELECT jsonb_build_object('version_id',version_id,'docx_sha256',docx_sha256) INTO current_version
    FROM bid_docx_current WHERE scope_id=p_workspace_id;
  IF coalesce(p_expected,'null'::jsonb) IS DISTINCT FROM coalesce(current_version,'null'::jsonb) THEN
    RAISE EXCEPTION 'DOCX_VERSION_CAS_MISMATCH' USING ERRCODE='40001';
  END IF;
  RETURN source_value;
END $$;

CREATE FUNCTION kb_bid_v2_create_docx_composition_request(
  p_id uuid,p_snapshot jsonb,p_contract jsonb,p_actor kb_actor_identity,p_key text
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE workspace_value bid_submission_workspaces%ROWTYPE; source_value jsonb; head bid_docx_current%ROWTYPE;
  frozen_bytes bytea; frozen_sha kb_sha256; job_payload jsonb; job_bytes bytea; job_sha kb_sha256;
  replay bytea; response jsonb; response_bytes bytea;
BEGIN
  IF jsonb_typeof(p_snapshot) IS DISTINCT FROM 'object'
    OR NOT kb_bid_v2_json_keys_exact(p_snapshot,ARRAY['schema_version','workspace_id','actor','basis','expected',
      'source_request','source_input_sha256','analysis_sha256','config','contract_sha256'])
    OR p_snapshot->'schema_version' IS DISTINCT FROM '1'::jsonb
    OR p_snapshot->>'actor' IS DISTINCT FROM p_actor::text
    OR jsonb_typeof(p_contract) IS DISTINCT FROM 'object'
    OR NOT kb_bid_v2_json_keys_exact(p_contract,ARRAY['checkpoint_contract_version','runtime_adapter','config','main','reviewer','tools','review_tools'])
    OR p_contract->'checkpoint_contract_version' IS DISTINCT FROM '3'::jsonb
    OR p_contract->>'runtime_adapter' IS DISTINCT FROM 'rig-chat-0.42.0/4'
    OR p_snapshot->'config' IS DISTINCT FROM p_contract->'config'
    OR p_snapshot->>'contract_sha256' IS DISTINCT FROM kb_bid_v2_sha256_bytes(convert_to(kb_bid_v2_jcs(p_contract),'UTF8'))::text THEN
    RAISE EXCEPTION 'DOCX_COMPOSITION_INPUT_INVALID' USING ERRCODE='23514';
  END IF;
  IF jsonb_typeof(p_snapshot#>'{config,limits}') IS DISTINCT FROM 'object'
    OR NOT kb_bid_v2_json_keys_exact(p_snapshot#>'{config,limits}',ARRAY['max_turns','max_tool_calls','max_physical_calls',
      'max_read_bytes','max_context_bytes','max_tool_result_bytes','max_review_rounds','max_docx_bytes','max_context_tokens','image_token_reserve','token_safety_margin','max_no_progress_turns','max_focus_turns','max_focus_replans'])
    OR EXISTS(SELECT 1 FROM unnest(ARRAY['max_turns','max_tool_calls','max_physical_calls','max_read_bytes',
      'max_context_bytes','max_tool_result_bytes','max_review_rounds','max_docx_bytes','max_context_tokens','image_token_reserve','token_safety_margin','max_no_progress_turns','max_focus_turns','max_focus_replans']) key
      WHERE NOT coalesce((p_snapshot#>>ARRAY['config','limits',key]) ~ '^[1-9][0-9]*$',false))
    OR (p_snapshot#>>'{config,limits,max_context_bytes}')::bigint <= (p_snapshot#>>'{config,limits,max_tool_result_bytes}')::bigint
    OR (p_snapshot#>>'{config,limits,token_safety_margin}')::numeric
      + (p_snapshot#>>'{config,provider,max_tokens}')::numeric
      >= (p_snapshot#>>'{config,limits,max_context_tokens}')::numeric THEN
    RAISE EXCEPTION 'DOCX_COMPOSITION_INPUT_INVALID: explicit positive budgets required' USING ERRCODE='23514';
  END IF;
  SELECT * INTO STRICT workspace_value FROM bid_submission_workspaces WHERE id=(p_snapshot->>'workspace_id')::uuid;
  PERFORM kb_bid_v2_require_project_owner(workspace_value.project_id,p_actor);
  frozen_bytes:=convert_to(kb_bid_v2_jcs(p_snapshot),'UTF8'); frozen_sha:=kb_bid_v2_sha256_bytes(frozen_bytes);
  replay:=kb_bid_v2_idempotency_begin(p_actor,'bid.v2.docx_compose.create',p_key,frozen_bytes,frozen_sha);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  -- Source/version CAS is rechecked at eventual publication too. The request
  -- freezes its selected identities but does not reserve the user's document.
  PERFORM 1 FROM bid_projects WHERE id=workspace_value.project_id AND status='open' FOR SHARE;
  IF NOT FOUND THEN RAISE EXCEPTION 'PROJECT_ENDED' USING ERRCODE='55000'; END IF;
  source_value:=kb_bid_v2_prepare_docx_composition_source(workspace_value.id,p_snapshot->'basis',p_snapshot->'expected',p_actor);
  IF p_snapshot->'source_request' IS DISTINCT FROM source_value->'source_request'
    OR p_snapshot->>'source_input_sha256' IS DISTINCT FROM kb_bid_v2_sha256_bytes(convert_to(kb_bid_v2_jcs(source_value->'input'),'UTF8'))::text
    OR p_snapshot->>'analysis_sha256' IS DISTINCT FROM kb_bid_v2_sha256_bytes(convert_to(kb_bid_v2_jcs(source_value->'analysis'),'UTF8'))::text THEN
    RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: composition source changed' USING ERRCODE='23514';
  END IF;
  -- Resolve the selected immutable version, not a later current row. Missing
  -- expected means first creation; the version foreign key prevents cross-scope binding.
  IF p_snapshot->'expected' IS DISTINCT FROM 'null'::jsonb THEN
    SELECT v.round_id,v.id,v.docx_sha256 INTO head.round_id,head.version_id,head.docx_sha256
      FROM bid_docx_version_artifacts v WHERE v.project_id=workspace_value.project_id AND v.workspace_id=workspace_value.id
        AND v.id=(p_snapshot#>>'{expected,version_id}')::uuid AND v.docx_sha256=p_snapshot#>>'{expected,docx_sha256}';
    IF NOT FOUND THEN RAISE EXCEPTION 'DOCX_VERSION_CAS_MISMATCH' USING ERRCODE='40001'; END IF;
  END IF;
  job_payload:=jsonb_build_object('job_kind','docx_compose','project_id',workspace_value.project_id,
    'workspace_id',workspace_value.id,'request',jsonb_build_object('request_artifact_id',p_id,'request_revision',1,'frozen_input_sha256',frozen_sha));
  job_bytes:=kb_bid_v2_json_payload(job_payload); job_sha:=kb_bid_v2_sha256_bytes(job_bytes);
  INSERT INTO bid_async_request_snapshot_artifacts(id,project_id,workspace_id,request_kind,revision,
    frozen_input_sha256,request_payload,request_sha256,status)
  VALUES(p_id,workspace_value.project_id,workspace_value.id,'docx_compose',1,frozen_sha,job_bytes,job_sha,'pending');
  INSERT INTO bid_docx_composition_request_identities(request_artifact_id,project_id,workspace_id,request_revision,
    request_sha256,frozen_input_sha256,document_set_id,document_set_sha256,requirement_set_id,requirement_set_sha256,
    source_request_id,source_request_revision,source_request_sha256,expected_round_id,expected_version_id,
    expected_docx_sha256,actor,frozen_input,contract_definition,contract_sha256)
  VALUES(p_id,workspace_value.project_id,workspace_value.id,1,job_sha,frozen_sha,
    (p_snapshot#>>'{basis,document_set_id}')::uuid,(p_snapshot#>>'{basis,document_set_sha256}')::kb_sha256,
    (p_snapshot#>>'{basis,requirement_set_id}')::uuid,(p_snapshot#>>'{basis,requirement_set_sha256}')::kb_sha256,
    (p_snapshot#>>'{source_request,request_artifact_id}')::uuid,(p_snapshot#>>'{source_request,request_revision}')::bigint,
    (p_snapshot#>>'{source_request,frozen_input_sha256}')::kb_sha256,head.round_id,head.version_id,head.docx_sha256,
    p_actor,p_snapshot,p_contract,(p_snapshot->>'contract_sha256')::kb_sha256);
  response:=jsonb_build_object('request_artifact_id',p_id,'request_revision',1,'frozen_input_sha256',frozen_sha,
    'request_sha256',job_sha,'status','pending'); response_bytes:=convert_to(response::text,'UTF8');
  INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
    entity_kind,entity_locator,after_revision,after_sha256)
  VALUES(gen_random_uuid(),1,'bid.v2.docx_compose.create',p_actor,p_key,frozen_sha,kb_bid_v2_sha256_bytes(response_bytes),
    'bid_v2_docx_composition_request',jsonb_build_object('workspace_id',workspace_value.id,'request_artifact_id',p_id),1,frozen_sha);
  PERFORM kb_bid_v2_idempotency_complete(p_actor,'bid.v2.docx_compose.create',p_key,202,response_bytes);
  RETURN response;
END $$;

CREATE FUNCTION kb_bid_v2_load_docx_composition_request(p_id uuid,p_revision bigint,p_sha kb_sha256)
RETURNS jsonb LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
  SELECT frozen_input FROM bid_docx_composition_request_identities
    WHERE request_artifact_id=p_id AND request_revision=p_revision AND frozen_input_sha256=p_sha
$$;

-- Worker-scoped delivery attestation; does not expose other request kinds.
CREATE FUNCTION kb_bid_v2_load_docx_composition_job(p_id uuid,p_revision bigint,p_sha kb_sha256)
RETURNS jsonb LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
  SELECT kb_bid_v2_authoring_job_payload(request_artifact_id) FROM bid_docx_composition_request_identities
    WHERE request_artifact_id=p_id AND request_revision=p_revision AND frozen_input_sha256=p_sha
$$;

CREATE FUNCTION kb_bid_v2_docx_composition_checkpoint_get(p_id uuid,p_sha kb_sha256)
RETURNS jsonb LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
  SELECT convert_from(canonical_payload,'UTF8')::jsonb FROM bid_tender_agent_checkpoint_artifacts
    WHERE request_artifact_id=p_id AND frozen_input_sha256=p_sha AND stage_kind='composition_checkpoint'
    ORDER BY batch_ordinal DESC LIMIT 1
$$;

CREATE FUNCTION kb_bid_v2_docx_composition_reserve(p_id uuid,p_sha kb_sha256,p_attempt integer,p_token uuid,
  p_turn integer,p_reviewing boolean,p_body bytea)
RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE typed bid_docx_composition_request_identities%ROWTYPE; prior jsonb; body jsonb; limits jsonb;
  stamp timestamptz; stage text; prompt_key text; tools_key text; ordinal_value integer; count_value bigint;
  body_sha kb_sha256; prompt_sha kb_sha256; tools_sha kb_sha256;
BEGIN
  stamp:=kb_bid_v2_tender_agent_lock_owner(p_id,p_sha,p_attempt,p_token);
  SELECT * INTO STRICT typed FROM bid_docx_composition_request_identities WHERE request_artifact_id=p_id AND frozen_input_sha256=p_sha;
  prior:=kb_bid_v2_docx_composition_checkpoint_get(p_id,p_sha); limits:=typed.frozen_input#>'{config,limits}';
  IF p_turn IS NULL OR p_reviewing IS NULL OR p_body IS NULL
    OR p_turn<>coalesce((prior->>'turn')::integer,0)
    OR p_reviewing<>coalesce((prior#>>'{workspace,reviewing}')::boolean,false)
    OR coalesce((prior#>>'{workspace,done}')::boolean,false)
    OR coalesce(prior#>'{journal,pending,response}','null'::jsonb)<>'null'::jsonb THEN
    RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: composition turn or role changed' USING ERRCODE='23514';
  END IF;
  SELECT count(*) INTO count_value FROM bid_tender_agent_call_attempts WHERE request_artifact_id=p_id AND frozen_input_sha256=p_sha;
  IF p_turn>=(limits->>'max_turns')::integer OR count_value>=(limits->>'max_physical_calls')::bigint
    OR coalesce((prior->>'tool_calls')::bigint,0)>=(limits->>'max_tool_calls')::bigint
    OR coalesce((prior->>'read_bytes')::bigint,0)>=(limits->>'max_read_bytes')::bigint THEN
    RAISE EXCEPTION 'AGENT_TURN_BUDGET_EXCEEDED' USING ERRCODE='23514';
  END IF;
  body:=convert_from(p_body,'UTF8')::jsonb;
  prompt_key:=CASE WHEN p_reviewing THEN 'reviewer' ELSE 'main' END;
  tools_key:=CASE WHEN p_reviewing THEN 'review_tools' ELSE 'tools' END;
  stage:=CASE WHEN p_reviewing THEN 'composition_review' ELSE 'composition_main' END;
  IF NOT kb_bid_v2_json_keys_exact(body-'reasoning_effort',ARRAY['model','stream','stream_options','max_tokens','tool_choice','tools','messages'])
    OR p_body IS DISTINCT FROM convert_to(kb_bid_v2_jcs(body),'UTF8')
    OR body->>'model' IS DISTINCT FROM typed.frozen_input#>>'{config,provider,model_id}'
    OR body->'max_tokens' IS DISTINCT FROM typed.frozen_input#>'{config,provider,max_tokens}'
    OR coalesce(body->'reasoning_effort','null'::jsonb) IS DISTINCT FROM typed.frozen_input#>'{config,provider,reasoning_effort}'
    OR body->'stream' IS DISTINCT FROM 'true'::jsonb OR body->>'tool_choice' IS DISTINCT FROM 'required'
    OR body#>>'{messages,0,role}' IS DISTINCT FROM 'system'
    OR body->'stream_options' IS DISTINCT FROM '{"include_usage":true}'::jsonb
    OR body#>'{messages,0,content}' IS DISTINCT FROM typed.contract_definition->prompt_key
    OR body->'tools' IS DISTINCT FROM typed.contract_definition->tools_key
    OR octet_length(p_body)>(limits->>'max_context_bytes')::bigint THEN
    RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: composition provider contract changed' USING ERRCODE='23514';
  END IF;
  IF EXISTS(SELECT 1 FROM bid_tender_agent_call_attempts WHERE request_artifact_id=p_id AND frozen_input_sha256=p_sha
      AND batch_ordinal=p_turn AND (stage_kind<>stage OR provider_body<>p_body OR stage_contract_sha256<>typed.contract_sha256)) THEN
    RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: composition replay body changed' USING ERRCODE='23514';
  END IF;
  SELECT coalesce(max(call_ordinal),0)+1 INTO ordinal_value FROM bid_tender_agent_call_attempts
    WHERE request_artifact_id=p_id AND frozen_input_sha256=p_sha AND batch_ordinal=p_turn;
  IF ordinal_value>3 THEN RAISE EXCEPTION 'AGENT_PROVIDER_UNAVAILABLE: composition boundary exhausted' USING ERRCODE='23514'; END IF;
  body_sha:=kb_bid_v2_sha256_bytes(p_body);
  prompt_sha:=kb_bid_v2_sha256_bytes(convert_to(kb_bid_v2_jcs(body#>'{messages,0,content}'),'UTF8'));
  tools_sha:=kb_bid_v2_sha256_bytes(convert_to(kb_bid_v2_jcs(body->'tools'),'UTF8'));
  INSERT INTO bid_tender_agent_call_attempts(request_artifact_id,frozen_input_sha256,stage_kind,batch_ordinal,
    input_sha256,stage_contract_sha256,system_prompt_utf8_sha256,prompt_contract_id,prompt_contract_sha256,
    schema_contract_id,schema_contract_sha256,agent_contract_id,agent_contract_sha256,model_contract_id,
    model_contract_sha256,runtime_contract_sha256,provider_body,provider_body_sha256,call_ordinal,reserved_at)
  VALUES(p_id,p_sha,stage,p_turn,body_sha,typed.contract_sha256,prompt_sha,
    kb_bid_v2_deterministic_uuid(typed.contract_sha256::text||':'||prompt_key),prompt_sha,'docx_composition_tools_v1',tools_sha,
    kb_bid_v2_deterministic_uuid(typed.contract_sha256::text||':agent'),typed.contract_sha256,
    kb_bid_v2_deterministic_uuid(typed.contract_sha256::text||':provider'),typed.contract_sha256,typed.contract_sha256,
    p_body,body_sha,ordinal_value,stamp);
  RETURN count_value+1;
END $$;

CREATE FUNCTION kb_bid_v2_docx_composition_checkpoint_put(p_id uuid,p_sha kb_sha256,p_attempt integer,p_token uuid,p_state jsonb)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE typed bid_docx_composition_request_identities%ROWTYPE; prior jsonb; payload bytea; prior_payload bytea;
  stamp timestamptz; turn_value integer; sequence_value integer; pending_value jsonb; prior_pending jsonb; response_value jsonb; role_value text; tools_value integer; read_value bigint; reviewing boolean; done_value boolean;
BEGIN
  stamp:=kb_bid_v2_tender_agent_lock_owner(p_id,p_sha,p_attempt,p_token);
  SELECT * INTO STRICT typed FROM bid_docx_composition_request_identities WHERE request_artifact_id=p_id AND frozen_input_sha256=p_sha;
  IF jsonb_typeof(p_state) IS DISTINCT FROM 'object'
    OR NOT kb_bid_v2_json_keys_exact(p_state-'pending_delivery',ARRAY['journal','contract_sha256','workspace','turn','tool_calls','read_bytes','transcript','main_work','review_work','main_progress','review_progress'])
    OR p_state->>'contract_sha256' IS DISTINCT FROM typed.contract_sha256::text
    OR p_state#>>'{workspace,draft,analysis_sha256}' IS DISTINCT FROM typed.frozen_input->>'analysis_sha256'
    OR jsonb_typeof(p_state#>'{workspace,reviewing}') IS DISTINCT FROM 'boolean'
    OR jsonb_typeof(p_state#>'{workspace,done}') IS DISTINCT FROM 'boolean'
    OR jsonb_typeof(p_state->'transcript') IS DISTINCT FROM 'array' THEN
    RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: composition checkpoint identity' USING ERRCODE='23514';
  END IF;
  turn_value:=(p_state->>'turn')::integer; tools_value:=(p_state->>'tool_calls')::integer; read_value:=(p_state->>'read_bytes')::bigint;
  reviewing:=(p_state#>>'{workspace,reviewing}')::boolean; done_value:=(p_state#>>'{workspace,done}')::boolean;
  sequence_value:=(p_state#>>'{journal,sequence}')::integer;
  IF p_state->>'pending_delivery' IS NOT NULL AND (
    NOT kb_bid_v2_json_keys_exact(p_state->'pending_delivery',ARRAY['reviewing','coverage','inspected','messages','view_ids'])
    OR jsonb_typeof(p_state#>'{pending_delivery,reviewing}') IS DISTINCT FROM 'boolean'
    OR jsonb_typeof(p_state#>'{pending_delivery,coverage}') IS DISTINCT FROM 'object'
    OR jsonb_typeof(p_state#>'{pending_delivery,inspected}') IS DISTINCT FROM 'object'
    OR jsonb_typeof(p_state#>'{pending_delivery,messages}') IS DISTINCT FROM 'object'
    OR p_state#>'{pending_delivery,messages}'='{}'::jsonb
    OR jsonb_typeof(p_state#>'{pending_delivery,view_ids}') IS DISTINCT FROM 'array'
    OR p_state#>'{workspace,done}' IS DISTINCT FROM 'false'::jsonb
    OR p_state#>'{pending_delivery,reviewing}' IS DISTINCT FROM p_state#>'{workspace,reviewing}') THEN
    RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: composition pending delivery identity' USING ERRCODE='23514';
  END IF;
  payload:=convert_to(kb_bid_v2_jcs(p_state),'UTF8');
  SELECT canonical_payload INTO prior_payload FROM bid_tender_agent_checkpoint_artifacts
    WHERE request_artifact_id=p_id AND frozen_input_sha256=p_sha AND stage_kind='composition_checkpoint' AND batch_ordinal=sequence_value;
  IF FOUND THEN
    IF prior_payload IS DISTINCT FROM payload THEN RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: divergent composition checkpoint' USING ERRCODE='23514'; END IF;
    RETURN;
  END IF;
  prior:=kb_bid_v2_docx_composition_checkpoint_get(p_id,p_sha);
  pending_value:=p_state#>'{journal,pending}'; prior_pending:=coalesce(prior#>'{journal,pending}','null'::jsonb);
  role_value:=CASE WHEN reviewing THEN 'reviewer' ELSE 'main' END;
  IF NOT kb_bid_v2_json_keys_exact(p_state->'journal',ARRAY['sequence','pending','session'])
    OR jsonb_typeof(p_state#>'{journal,sequence}') IS DISTINCT FROM 'number'
    OR (pending_value IS DISTINCT FROM 'null'::jsonb AND jsonb_typeof(p_state#>'{journal,session}') IS DISTINCT FROM 'object')
    OR (p_state#>'{journal,session}' IS DISTINCT FROM 'null'::jsonb AND (
      NOT kb_bid_v2_json_keys_exact(p_state#>'{journal,session}',ARRAY['run','prefix','suffix'])
      OR jsonb_typeof(p_state#>'{journal,session,run}') IS DISTINCT FROM 'object'
      OR p_state#>'{journal,session,prefix}' IS DISTINCT FROM '2'::jsonb
      OR p_state#>'{journal,session,suffix}' IS DISTINCT FROM '0'::jsonb
      OR octet_length(convert_to(kb_bid_v2_jcs(p_state#>'{journal,session}'),'UTF8'))>(typed.frozen_input#>>'{config,limits,max_context_bytes}')::bigint))
    OR jsonb_typeof(p_state->'turn') IS DISTINCT FROM 'number'
    OR jsonb_typeof(p_state->'tool_calls') IS DISTINCT FROM 'number'
    OR jsonb_typeof(p_state->'read_bytes') IS DISTINCT FROM 'number'
    OR sequence_value IS NULL OR sequence_value<>coalesce((prior#>>'{journal,sequence}')::integer,0)+1
    OR turn_value IS NULL OR turn_value<0 OR p_state->>'contract_sha256' IS DISTINCT FROM typed.contract_sha256::text
    OR role_value IS NULL OR role_value NOT IN ('main','reviewer')
    OR coalesce((prior#>>'{workspace,done}')::boolean,false)
    OR coalesce((p_state->>'tool_calls')::bigint,-1)<coalesce((prior->>'tool_calls')::bigint,0)
    OR coalesce((p_state->>'read_bytes')::bigint,-1)<coalesce((prior->>'read_bytes')::bigint,0)
    OR turn_value>((typed.frozen_input#>'{config,limits}')->>'max_turns')::integer
    OR (p_state->>'tool_calls')::bigint>((typed.frozen_input#>'{config,limits}')->>'max_tool_calls')::bigint
    OR (p_state->>'read_bytes')::bigint>((typed.frozen_input#>'{config,limits}')->>'max_read_bytes')::bigint THEN
    RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: checkpoint sequence, identity or budget' USING ERRCODE='23514';
  END IF;
  IF pending_value IS DISTINCT FROM 'null'::jsonb AND (
    NOT kb_bid_v2_json_keys_exact(pending_value,ARRAY['turn','role','body','response'])
    OR pending_value->'turn' IS DISTINCT FROM p_state->'turn'
    OR pending_value->>'role' IS DISTINCT FROM role_value
    OR jsonb_typeof(pending_value->'body') IS DISTINCT FROM 'string') THEN
    RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: pending request identity' USING ERRCODE='23514';
  END IF;
  IF prior_pending='null'::jsonb THEN
    -- Reservation and this prepared checkpoint commit in one transaction.
    IF turn_value<>coalesce((prior->>'turn')::integer,0)
      OR role_value IS DISTINCT FROM (CASE WHEN coalesce((prior#>>'{workspace,reviewing}')::boolean,false) THEN 'reviewer' ELSE 'main' END)
      OR p_state#>'{workspace,done}' IS DISTINCT FROM 'false'::jsonb
      OR pending_value->'response' IS DISTINCT FROM 'null'::jsonb
      OR NOT EXISTS(SELECT 1 FROM bid_tender_agent_call_attempts WHERE request_artifact_id=p_id
        AND frozen_input_sha256=p_sha AND batch_ordinal=turn_value AND stage_kind=CASE WHEN role_value='reviewer' THEN 'composition_review' ELSE 'composition_main' END
        AND provider_body=convert_to(pending_value->>'body','UTF8')) THEN
      RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: prepared checkpoint requires reserved exact request' USING ERRCODE='23514';
    END IF;
    IF prior IS NOT NULL AND (p_state-'journal') IS DISTINCT FROM (prior-'journal') THEN
      RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: preparation changed business state' USING ERRCODE='23514';
    END IF;
    IF prior IS NULL AND (p_state->'tool_calls' IS DISTINCT FROM '0'::jsonb
      OR p_state->'read_bytes' IS DISTINCT FROM '0'::jsonb
      OR p_state->'main_progress' IS DISTINCT FROM '{"watch":{"no_progress_turns":0,"focus_turns":0,"replans":0,"recovery":"running"},"seen":[],"completions":[],"blockers":[]}'::jsonb
      OR p_state->'review_progress' IS DISTINCT FROM '{"watch":{"no_progress_turns":0,"focus_turns":0,"replans":0,"recovery":"running"},"seen":[],"completions":[],"blockers":[]}'::jsonb
      OR p_state->'main_work' IS DISTINCT FROM 'null'::jsonb
      OR p_state->'review_work' IS DISTINCT FROM 'null'::jsonb
      OR p_state->'transcript' IS DISTINCT FROM '[]'::jsonb
      OR p_state#>'{workspace,draft,sections}' IS DISTINCT FROM '{}'::jsonb
      OR p_state#>'{workspace,artifact}' IS DISTINCT FROM 'null'::jsonb
      OR p_state#>'{workspace,findings}' IS DISTINCT FROM '[]'::jsonb
      OR p_state#>'{workspace,review_rounds}' IS DISTINCT FROM '0'::jsonb
      OR p_state#>'{workspace,inspected}' IS DISTINCT FROM '{}'::jsonb
      OR p_state#>'{workspace,source_coverage}' IS DISTINCT FROM '{"metadata":{},"text":{},"form_cells":{},"candidate":{},"views":{},"view_failures":{}}'::jsonb
      OR p_state#>'{workspace,review_coverage}' IS DISTINCT FROM '{"metadata":{},"text":{},"form_cells":{},"candidate":{},"views":{},"view_failures":{}}'::jsonb) THEN
      RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: initial checkpoint must be empty' USING ERRCODE='23514';
    END IF;
  ELSIF prior_pending->'response'='null'::jsonb THEN
    -- No evidence, counters or tool mutations until the full response is durable.
    IF (p_state-'journal') IS DISTINCT FROM (prior-'journal')
      OR p_state#>'{journal,session}' IS DISTINCT FROM prior#>'{journal,session}'
      OR (pending_value-'response') IS DISTINCT FROM (prior_pending-'response')
      OR jsonb_typeof(pending_value->'response') IS DISTINCT FROM 'object' THEN
      RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: response boundary changed prepared state' USING ERRCODE='23514';
    END IF;
    response_value:=pending_value->'response';
    IF NOT kb_bid_v2_json_keys_exact(response_value,ARRAY['content','tool_calls','finish_reason','usage'])
      OR jsonb_typeof(response_value->'content') IS DISTINCT FROM 'string'
      OR response_value->>'finish_reason' IS DISTINCT FROM 'tool_calls'
      OR jsonb_typeof(response_value->'tool_calls') IS DISTINCT FROM 'array' THEN
      RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: incomplete saved response' USING ERRCODE='23514';
    END IF;
    IF jsonb_array_length(response_value->'tool_calls')=0 OR EXISTS(
      SELECT 1 FROM jsonb_array_elements(response_value->'tool_calls') c
      WHERE NOT kb_bid_v2_json_keys_exact(c,ARRAY['id','name','arguments'])
        OR jsonb_typeof(c->'id') IS DISTINCT FROM 'string' OR btrim(c->>'id')=''
        OR jsonb_typeof(c->'name') IS DISTINCT FROM 'string' OR btrim(c->>'name')=''
        OR jsonb_typeof(c->'arguments') IS DISTINCT FROM 'string')
      OR (SELECT count(*)<>count(DISTINCT c->>'id') FROM jsonb_array_elements(response_value->'tool_calls') c) THEN
      RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: invalid saved tool calls' USING ERRCODE='23514';
    END IF;
  ELSE
    IF pending_value IS DISTINCT FROM 'null'::jsonb OR turn_value<>(prior->>'turn')::integer+1
      OR (p_state->>'tool_calls')::bigint<=(prior->>'tool_calls')::bigint
      OR (p_state->>'tool_calls')::bigint>(prior->>'tool_calls')::bigint+jsonb_array_length(prior_pending#>'{response,tool_calls}') THEN
      RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: tool commit must follow saved response' USING ERRCODE='23514';
    END IF;
  END IF;
  INSERT INTO bid_tender_agent_checkpoint_artifacts(request_artifact_id,frozen_input_sha256,stage_kind,batch_ordinal,
    contract_sha256,canonical_input,input_sha256,canonical_payload,content_sha256)
  VALUES(p_id,p_sha,'composition_checkpoint',sequence_value,typed.contract_sha256,convert_to(p_sha::text,'UTF8'),
    kb_bid_v2_sha256_bytes(convert_to(p_sha::text,'UTF8')),payload,kb_bid_v2_sha256_bytes(payload));
  UPDATE bid_tender_agent_run_artifacts SET progress_stage=CASE WHEN reviewing THEN 'reviewing' ELSE 'generating' END,
    progress_phase=CASE WHEN done_value THEN 'publishing' WHEN reviewing THEN 'verifying' ELSE 'drafting' END,
    progress_detail=jsonb_build_object('turn',turn_value,'tool_calls',tools_value,'read_bytes',read_value,
      'sections',(SELECT count(*) FROM jsonb_object_keys(p_state#>'{workspace,draft,sections}')),'reviewing',reviewing,'reviewed',done_value),
    progress_sequence=progress_sequence+1,turn_count=turn_value,tool_call_count=tools_value,text_bytes_read=read_value,
    checkpoint_sha256=kb_bid_v2_sha256_bytes(payload),updated_at=stamp
    WHERE request_artifact_id=p_id AND attempt=p_attempt;
END $$;


CREATE FUNCTION kb_bid_v2_export_review_checkpoint_get(p_id uuid,p_sha kb_sha256)
RETURNS jsonb LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
  SELECT convert_from(canonical_payload,'UTF8')::jsonb FROM bid_tender_agent_checkpoint_artifacts
    WHERE request_artifact_id=p_id AND frozen_input_sha256=p_sha AND stage_kind='export_review_checkpoint'
    ORDER BY batch_ordinal DESC LIMIT 1
$$;

CREATE FUNCTION kb_bid_v2_export_review_checkpoint_put(p_id uuid,p_sha kb_sha256,p_attempt integer,p_token uuid,p_state jsonb)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE typed bid_submission_export_request_identities%ROWTYPE; prior jsonb; payload bytea; prior_payload bytea;
  stamp timestamptz; turn_value integer; sequence_value integer; pending_value jsonb; prior_pending jsonb; response_value jsonb; limits jsonb;
BEGIN
  stamp:=kb_bid_v2_tender_agent_lock_owner(p_id,p_sha,p_attempt,p_token);
  SELECT * INTO STRICT typed FROM bid_submission_export_request_identities WHERE request_artifact_id=p_id AND frozen_input_sha256=p_sha;
  IF typed.request_kind<>'submission_export'
    OR jsonb_typeof(p_state) IS DISTINCT FROM 'object'
    OR NOT kb_bid_v2_json_keys_exact(p_state,ARRAY['journal','contract_sha256','inventory','analysis','obligations','pending_delivery','tender_coverage','output_coverage','reviews','turn','tool_calls','read_bytes','transcript','progress','done'])
    OR jsonb_typeof(p_state->'transcript') IS DISTINCT FROM 'array'
    OR jsonb_typeof(p_state#>'{done}') IS DISTINCT FROM 'boolean'
    OR p_state#>>'{inventory,docx_sha256}' IS DISTINCT FROM typed.docx_sha256::text
    OR p_state->>'contract_sha256' IS DISTINCT FROM typed.frozen_context#>>'{execution_contract,contract_sha256}'
    OR jsonb_typeof(p_state->'obligations') IS DISTINCT FROM 'object'
    OR p_state->'inventory' IS DISTINCT FROM (SELECT result_identity->'inventory' FROM bid_async_stage_receipts WHERE request_artifact_id=p_id AND frozen_input_sha256=p_sha AND stage_kind='render_snapshot') THEN
    RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: export-review checkpoint identity' USING ERRCODE='23514';
  END IF;
  turn_value:=(p_state->>'turn')::integer;
  sequence_value:=(p_state#>>'{journal,sequence}')::integer;
  IF p_state->'pending_delivery' IS DISTINCT FROM 'null'::jsonb AND (
    NOT kb_bid_v2_json_keys_exact(p_state->'pending_delivery',ARRAY['tender_coverage','output_coverage','messages','view_refs'])
    OR jsonb_typeof(p_state#>'{pending_delivery,tender_coverage}') IS DISTINCT FROM 'object'
    OR jsonb_typeof(p_state#>'{pending_delivery,output_coverage}') IS DISTINCT FROM 'object'
    OR jsonb_typeof(p_state#>'{pending_delivery,view_refs}') IS DISTINCT FROM 'array'
    OR jsonb_typeof(p_state#>'{pending_delivery,messages}') IS DISTINCT FROM 'object'
    OR p_state#>'{pending_delivery,messages}'='{}'::jsonb
    OR p_state->'done' IS DISTINCT FROM 'false'::jsonb) THEN
    RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: export pending evidence delivery' USING ERRCODE='23514';
  END IF;
  payload:=convert_to(kb_bid_v2_jcs(p_state),'UTF8');
  SELECT canonical_payload INTO prior_payload FROM bid_tender_agent_checkpoint_artifacts
    WHERE request_artifact_id=p_id AND frozen_input_sha256=p_sha AND stage_kind='export_review_checkpoint' AND batch_ordinal=sequence_value;
  IF FOUND THEN
    IF prior_payload IS DISTINCT FROM payload THEN RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: divergent export-review checkpoint' USING ERRCODE='23514'; END IF;
    RETURN;
  END IF;
  prior:=kb_bid_v2_export_review_checkpoint_get(p_id,p_sha);
  pending_value:=p_state#>'{journal,pending}'; prior_pending:=coalesce(prior#>'{journal,pending}','null'::jsonb);
  limits:=typed.frozen_context#>'{execution_contract,definition,config,limits}';
  IF NOT kb_bid_v2_json_keys_exact(p_state->'journal',ARRAY['sequence','pending','session'])
    OR jsonb_typeof(p_state#>'{journal,sequence}') IS DISTINCT FROM 'number'
    OR (pending_value IS DISTINCT FROM 'null'::jsonb AND jsonb_typeof(p_state#>'{journal,session}') IS DISTINCT FROM 'object')
    OR (p_state#>'{journal,session}' IS DISTINCT FROM 'null'::jsonb AND (
      NOT kb_bid_v2_json_keys_exact(p_state#>'{journal,session}',ARRAY['run','prefix','suffix'])
      OR p_state#>'{journal,session,prefix}' IS DISTINCT FROM '2'::jsonb
      OR p_state#>'{journal,session,suffix}' IS DISTINCT FROM '0'::jsonb))
    OR jsonb_typeof(p_state->'turn') IS DISTINCT FROM 'number'
    OR jsonb_typeof(p_state->'tool_calls') IS DISTINCT FROM 'number'
    OR jsonb_typeof(p_state->'read_bytes') IS DISTINCT FROM 'number'
    OR sequence_value IS NULL OR sequence_value<>coalesce((prior#>>'{journal,sequence}')::integer,0)+1
    OR turn_value IS NULL OR turn_value<0
    OR (prior IS NOT NULL AND p_state->>'contract_sha256' IS DISTINCT FROM prior->>'contract_sha256')
    OR coalesce((prior#>>'{done}')::boolean,false)
    OR turn_value>(limits->>'max_turns')::integer
    OR (p_state->>'tool_calls')::bigint>(limits->>'max_tool_calls')::bigint
    OR (p_state->>'read_bytes')::bigint>(limits->>'max_read_bytes')::bigint
    OR (prior IS NOT NULL AND (p_state->'analysis' IS DISTINCT FROM prior->'analysis'
      OR p_state->'obligations' IS DISTINCT FROM prior->'obligations'))
    OR coalesce((p_state->>'tool_calls')::bigint,-1)<coalesce((prior->>'tool_calls')::bigint,0)
    OR coalesce((p_state->>'read_bytes')::bigint,-1)<coalesce((prior->>'read_bytes')::bigint,0) THEN
    RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: export-review checkpoint sequence' USING ERRCODE='23514';
  END IF;
  IF pending_value IS DISTINCT FROM 'null'::jsonb AND (
    NOT kb_bid_v2_json_keys_exact(pending_value,ARRAY['turn','role','body','response'])
    OR pending_value->'turn' IS DISTINCT FROM p_state->'turn'
    OR pending_value->>'role' IS DISTINCT FROM 'reviewer'
    OR jsonb_typeof(pending_value->'body') IS DISTINCT FROM 'string') THEN
    RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: export pending request identity' USING ERRCODE='23514';
  END IF;
  IF prior_pending='null'::jsonb THEN
    IF turn_value<>coalesce((prior->>'turn')::integer,0)
      OR p_state->'done' IS DISTINCT FROM 'false'::jsonb
      OR pending_value->'response' IS DISTINCT FROM 'null'::jsonb
      OR NOT EXISTS(SELECT 1 FROM bid_tender_agent_call_attempts WHERE request_artifact_id=p_id
        AND frozen_input_sha256=p_sha AND batch_ordinal=turn_value AND stage_kind='export_review'
        AND stage_contract_sha256=(p_state->>'contract_sha256')::kb_sha256
        AND provider_body=convert_to(pending_value->>'body','UTF8')) THEN
      RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: export prepared checkpoint requires reserved exact request' USING ERRCODE='23514';
    END IF;
    IF prior IS NOT NULL AND (p_state-ARRAY['journal','transcript']) IS DISTINCT FROM (prior-ARRAY['journal','transcript']) THEN
      RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: export preparation changed business state' USING ERRCODE='23514';
    END IF;
    IF prior IS NULL AND (p_state->'tool_calls' IS DISTINCT FROM '0'::jsonb
      OR p_state->'read_bytes' IS DISTINCT FROM '0'::jsonb OR p_state->'reviews' IS DISTINCT FROM '{}'::jsonb
      OR p_state->'transcript' IS DISTINCT FROM '[]'::jsonb
      OR p_state->'output_coverage' IS DISTINCT FROM '{"units":{},"views":{}}'::jsonb
      OR p_state->'tender_coverage' IS DISTINCT FROM '{"metadata":{},"text":{},"form_cells":{},"candidate":{},"views":{},"view_failures":{}}'::jsonb
      OR p_state->'progress' IS DISTINCT FROM '{"watch":{"no_progress_turns":0,"focus_turns":0,"replans":0,"recovery":"running"},"seen":[],"completions":[],"blockers":[]}'::jsonb
      OR p_state->>'pending_delivery' IS NOT NULL) THEN
      RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: initial export checkpoint must be empty' USING ERRCODE='23514';
    END IF;
  ELSIF prior_pending->'response'='null'::jsonb THEN
    IF (p_state-'journal') IS DISTINCT FROM (prior-'journal')
      OR p_state#>'{journal,session}' IS DISTINCT FROM prior#>'{journal,session}'
      OR (pending_value-'response') IS DISTINCT FROM (prior_pending-'response')
      OR jsonb_typeof(pending_value->'response') IS DISTINCT FROM 'object' THEN
      RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: export response boundary changed prepared state' USING ERRCODE='23514';
    END IF;
    response_value:=pending_value->'response';
    IF NOT kb_bid_v2_json_keys_exact(response_value,ARRAY['content','tool_calls','finish_reason','usage'])
      OR jsonb_typeof(response_value->'content') IS DISTINCT FROM 'string'
      OR response_value->>'finish_reason' IS DISTINCT FROM 'tool_calls'
      OR jsonb_typeof(response_value->'tool_calls') IS DISTINCT FROM 'array' THEN
      RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: incomplete export response' USING ERRCODE='23514';
    END IF;
    IF jsonb_array_length(response_value->'tool_calls')=0 OR EXISTS(
      SELECT 1 FROM jsonb_array_elements(response_value->'tool_calls') c
      WHERE NOT kb_bid_v2_json_keys_exact(c,ARRAY['id','name','arguments'])
        OR jsonb_typeof(c->'id') IS DISTINCT FROM 'string' OR btrim(c->>'id')=''
        OR jsonb_typeof(c->'name') IS DISTINCT FROM 'string' OR btrim(c->>'name')=''
        OR jsonb_typeof(c->'arguments') IS DISTINCT FROM 'string')
      OR (SELECT count(*)<>count(DISTINCT c->>'id') FROM jsonb_array_elements(response_value->'tool_calls') c) THEN
      RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: invalid export tool calls' USING ERRCODE='23514';
    END IF;
  ELSE
    IF pending_value IS DISTINCT FROM 'null'::jsonb OR turn_value<>(prior->>'turn')::integer+1
      OR (p_state->>'tool_calls')::bigint<=(prior->>'tool_calls')::bigint
      OR (p_state->>'tool_calls')::bigint>(prior->>'tool_calls')::bigint+jsonb_array_length(prior_pending#>'{response,tool_calls}') THEN
      RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: export commit must follow saved response' USING ERRCODE='23514';
    END IF;
  END IF;
  INSERT INTO bid_tender_agent_checkpoint_artifacts(request_artifact_id,frozen_input_sha256,stage_kind,batch_ordinal,
    contract_sha256,canonical_input,input_sha256,canonical_payload,content_sha256)
  VALUES(p_id,p_sha,'export_review_checkpoint',sequence_value,(p_state->>'contract_sha256')::kb_sha256,convert_to(p_sha::text,'UTF8'),
    kb_bid_v2_sha256_bytes(convert_to(p_sha::text,'UTF8')),payload,kb_bid_v2_sha256_bytes(payload));
  UPDATE bid_tender_agent_run_artifacts SET progress_stage='reviewing',
    progress_phase=CASE WHEN (p_state->>'done')::boolean THEN 'publishing' ELSE 'verifying' END,
    progress_detail=jsonb_build_object('turn',turn_value,'tool_calls',p_state->'tool_calls','read_bytes',p_state->'read_bytes','reviewed',p_state->'done'),
    progress_sequence=progress_sequence+1,turn_count=turn_value,tool_call_count=(p_state->>'tool_calls')::integer,
    text_bytes_read=(p_state->>'read_bytes')::bigint,checkpoint_sha256=kb_bid_v2_sha256_bytes(payload),updated_at=stamp
    WHERE request_artifact_id=p_id AND attempt=p_attempt;
END $$;




CREATE FUNCTION kb_bid_v2_layout_checkpoint_get(p_id uuid,p_sha kb_sha256)
RETURNS jsonb LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
  SELECT convert_from(canonical_payload,'UTF8')::jsonb FROM bid_tender_agent_checkpoint_artifacts
    WHERE request_artifact_id=p_id AND frozen_input_sha256=p_sha AND stage_kind='layout_checkpoint'
    ORDER BY batch_ordinal DESC LIMIT 1;
$$;

CREATE FUNCTION kb_bid_v2_layout_checkpoint_put(p_id uuid,p_sha kb_sha256,p_attempt integer,p_token uuid,p_state jsonb)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE typed bid_submission_export_request_identities%ROWTYPE; prior jsonb; payload bytea;
  stored bid_tender_agent_checkpoint_artifacts%ROWTYPE;
  revision_value integer; iteration_value bigint; max_iterations_value bigint;
BEGIN
  PERFORM kb_bid_v2_tender_agent_lock_owner(p_id,p_sha,p_attempt,p_token);
  SELECT * INTO STRICT typed FROM bid_submission_export_request_identities WHERE request_artifact_id=p_id AND frozen_input_sha256=p_sha;
  IF typed.request_kind<>'docx_layout'
    OR jsonb_typeof(p_state) IS DISTINCT FROM 'object'
    OR NOT kb_bid_v2_json_keys_exact(p_state,ARRAY['revision','state','baseline_sha256','iteration','max_iterations','operation_id','expected_old','export_request_id','diagnosis'])
    OR coalesce(p_state->>'state','') NOT IN ('measure','await_editor','await_save','ready','failed')
    OR p_state->>'baseline_sha256' IS DISTINCT FROM typed.docx_sha256::text
    OR jsonb_typeof(p_state->'revision') IS DISTINCT FROM 'number'
    OR jsonb_typeof(p_state->'iteration') IS DISTINCT FROM 'number'
    OR jsonb_typeof(p_state->'max_iterations') IS DISTINCT FROM 'number'
    OR p_state->>'revision' !~ '^[0-9]+$'
    OR p_state->>'iteration' !~ '^[0-9]+$'
    OR p_state->>'max_iterations' !~ '^[0-9]+$'
    OR (p_state->>'state'<>'ready' AND p_state->>'export_request_id' IS NOT NULL) THEN
    RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: layout checkpoint identity' USING ERRCODE='23514';
  END IF;
  revision_value:=(p_state->>'revision')::integer;
  iteration_value:=(p_state->>'iteration')::bigint;
  max_iterations_value:=(p_state->>'max_iterations')::bigint;
  payload:=convert_to(kb_bid_v2_jcs(p_state),'UTF8');
  SELECT * INTO stored FROM bid_tender_agent_checkpoint_artifacts
    WHERE request_artifact_id=p_id AND frozen_input_sha256=p_sha AND stage_kind='layout_checkpoint' AND batch_ordinal=revision_value;
  IF FOUND THEN
    IF stored.canonical_payload IS DISTINCT FROM payload OR stored.contract_sha256 IS DISTINCT FROM p_sha THEN
      RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: divergent layout checkpoint' USING ERRCODE='23514';
    END IF;
    RETURN;
  END IF;
  SELECT * INTO stored FROM bid_tender_agent_checkpoint_artifacts
    WHERE request_artifact_id=p_id AND frozen_input_sha256=p_sha AND stage_kind='layout_checkpoint'
    ORDER BY batch_ordinal DESC LIMIT 1;
  -- The table is append-only; select the latest revision, never overwrite it.
  prior:=convert_from(stored.canonical_payload,'UTF8')::jsonb;
  IF revision_value<>coalesce((prior->>'revision')::integer,0)+1
    OR iteration_value>max_iterations_value OR max_iterations_value>4294967295
    OR (prior IS NULL AND iteration_value<>0)
    OR (prior IS NOT NULL AND (
      stored.contract_sha256 IS DISTINCT FROM p_sha
      OR prior->>'state' IN ('ready','failed')
      OR max_iterations_value<>(prior->>'max_iterations')::bigint
      OR iteration_value<(prior->>'iteration')::bigint)) THEN
    RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: layout checkpoint sequence or budget changed' USING ERRCODE='23514';
  END IF;
  INSERT INTO bid_tender_agent_checkpoint_artifacts(
    request_artifact_id,frozen_input_sha256,stage_kind,batch_ordinal,contract_sha256,
    canonical_input,input_sha256,canonical_payload,content_sha256)
  VALUES(p_id,p_sha,'layout_checkpoint',revision_value,p_sha,convert_to(p_sha::text,'UTF8'),
    kb_bid_v2_sha256_bytes(convert_to(p_sha::text,'UTF8')),payload,kb_bid_v2_sha256_bytes(payload));
END $$;

CREATE FUNCTION kb_bid_v2_export_review_reserve(p_id uuid,p_sha kb_sha256,p_attempt integer,p_token uuid,
  p_turn integer,p_contract kb_sha256,p_body bytea)
RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE typed bid_submission_export_request_identities%ROWTYPE; prior jsonb; body jsonb;
  stamp timestamptz; ordinal_value integer; count_value bigint; body_sha kb_sha256; prompt_sha kb_sha256; tools_sha kb_sha256;
BEGIN
  stamp:=kb_bid_v2_tender_agent_lock_owner(p_id,p_sha,p_attempt,p_token);
  SELECT * INTO STRICT typed FROM bid_submission_export_request_identities WHERE request_artifact_id=p_id AND frozen_input_sha256=p_sha;
  prior:=kb_bid_v2_export_review_checkpoint_get(p_id,p_sha);
  IF typed.request_kind<>'submission_export'
    OR p_turn IS NULL OR p_body IS NULL OR p_contract IS NULL
    OR p_contract::text IS DISTINCT FROM typed.frozen_context#>>'{execution_contract,contract_sha256}'
    OR p_turn<>coalesce((prior->>'turn')::integer,0)
    OR coalesce((prior#>>'{done}')::boolean,false)
    OR coalesce(prior#>'{journal,pending,response}','null'::jsonb)<>'null'::jsonb
    OR (prior IS NOT NULL AND p_contract::text IS DISTINCT FROM prior->>'contract_sha256') THEN
    RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: export-review turn or contract changed' USING ERRCODE='23514';
  END IF;
  SELECT count(*) INTO count_value FROM bid_tender_agent_call_attempts WHERE request_artifact_id=p_id AND frozen_input_sha256=p_sha;
  body:=convert_from(p_body,'UTF8')::jsonb;
  IF NOT kb_bid_v2_json_keys_exact(body-'reasoning_effort',ARRAY['model','stream','stream_options','max_tokens','tool_choice','tools','messages'])
    OR p_body IS DISTINCT FROM convert_to(kb_bid_v2_jcs(body),'UTF8')
    OR body->'stream' IS DISTINCT FROM 'true'::jsonb OR body->>'tool_choice' IS DISTINCT FROM 'required'
    OR body#>>'{messages,0,role}' IS DISTINCT FROM 'system'
    OR body->'stream_options' IS DISTINCT FROM '{"include_usage":true}'::jsonb
    OR body->'tools' IS DISTINCT FROM typed.frozen_context#>'{execution_contract,definition,tools}'
    OR body#>'{messages,0,content}' IS DISTINCT FROM typed.frozen_context#>'{execution_contract,definition,reviewer}'
    OR body->>'model' IS DISTINCT FROM typed.frozen_context#>>'{execution_contract,definition,config,provider,model_id}'
    OR body->'max_tokens' IS DISTINCT FROM typed.frozen_context#>'{execution_contract,definition,config,provider,max_tokens}'
    OR coalesce(body->'reasoning_effort','null'::jsonb) IS DISTINCT FROM coalesce(typed.frozen_context#>'{execution_contract,definition,config,provider,reasoning_effort}','null'::jsonb)
    OR p_turn>=(typed.frozen_context#>>'{execution_contract,definition,config,limits,max_turns}')::integer
    OR count_value>=(typed.frozen_context#>>'{execution_contract,definition,config,limits,max_physical_calls}')::bigint
    OR octet_length(p_body)>(typed.frozen_context#>>'{execution_contract,definition,config,limits,max_context_bytes}')::bigint THEN
    RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: export-review provider contract changed' USING ERRCODE='23514';
  END IF;
  IF EXISTS(SELECT 1 FROM bid_tender_agent_call_attempts WHERE request_artifact_id=p_id AND frozen_input_sha256=p_sha
      AND batch_ordinal=p_turn AND (stage_kind<>'export_review' OR provider_body<>p_body OR stage_contract_sha256<>p_contract)) THEN
    RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: export-review replay body changed' USING ERRCODE='23514';
  END IF;
  SELECT coalesce(max(call_ordinal),0)+1 INTO ordinal_value FROM bid_tender_agent_call_attempts
    WHERE request_artifact_id=p_id AND frozen_input_sha256=p_sha AND batch_ordinal=p_turn;
  IF ordinal_value>3 THEN RAISE EXCEPTION 'AGENT_PROVIDER_UNAVAILABLE: export-review boundary exhausted' USING ERRCODE='23514'; END IF;
  body_sha:=kb_bid_v2_sha256_bytes(p_body);
  prompt_sha:=kb_bid_v2_sha256_bytes(convert_to(kb_bid_v2_jcs(body#>'{messages,0,content}'),'UTF8'));
  tools_sha:=kb_bid_v2_sha256_bytes(convert_to(kb_bid_v2_jcs(body->'tools'),'UTF8'));
  INSERT INTO bid_tender_agent_call_attempts(request_artifact_id,frozen_input_sha256,stage_kind,batch_ordinal,
    input_sha256,stage_contract_sha256,system_prompt_utf8_sha256,prompt_contract_id,prompt_contract_sha256,
    schema_contract_id,schema_contract_sha256,agent_contract_id,agent_contract_sha256,model_contract_id,
    model_contract_sha256,runtime_contract_sha256,provider_body,provider_body_sha256,call_ordinal,reserved_at)
  VALUES(p_id,p_sha,'export_review',p_turn,body_sha,p_contract,prompt_sha,
    kb_bid_v2_deterministic_uuid(p_contract::text||':reviewer'),prompt_sha,'export_review_tools_v1',tools_sha,
    kb_bid_v2_deterministic_uuid(p_contract::text||':agent'),p_contract,
    kb_bid_v2_deterministic_uuid(p_contract::text||':provider'),p_contract,p_contract,
    p_body,body_sha,ordinal_value,stamp);
  RETURN count_value+1;
END $$;

CREATE FUNCTION kb_bid_v2_load_export_review_basis(p_id uuid,p_sha kb_sha256)
RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE typed bid_submission_export_request_identities%ROWTYPE;
  set_row bid_requirement_set_artifacts%ROWTYPE; source_request bid_async_request_snapshot_artifacts%ROWTYPE; payload jsonb;
  identity jsonb; input_value jsonb;
BEGIN
  SELECT * INTO STRICT typed FROM bid_submission_export_request_identities
    WHERE request_artifact_id=p_id AND frozen_input_sha256=p_sha;
  IF typed.request_kind<>'submission_export' THEN
    RAISE EXCEPTION 'REQUEST_OBSOLETE: export review requires a submission_export request' USING ERRCODE='23514';
  END IF;
  identity:=typed.frozen_context->'analysis_identity';
  IF identity IS NULL OR identity->>'schema_version' IS DISTINCT FROM '2' THEN
    RETURN jsonb_build_object('allowed',false,'reason','analysis_unavailable');
  END IF;
  SELECT * INTO set_row FROM bid_requirement_set_artifacts
    WHERE id=(identity->>'id')::uuid AND content_sha256=(identity->>'sha256')::kb_sha256
      AND project_id=typed.project_id;
  IF NOT FOUND THEN
    RETURN jsonb_build_object('allowed',false,'reason','analysis_identity_missing');
  END IF;
  payload:=convert_from(set_row.canonical_payload,'UTF8')::jsonb;
  IF payload#>'{analysis_result,schema_version}' IS DISTINCT FROM '2'::jsonb THEN
    RETURN jsonb_build_object('allowed',false,'reason','analysis_not_v2');
  END IF;
  -- The published disposition set contains model decisions, not the original
  -- frozen decisions. Reuse the analysis request's immutable input loader.
  SELECT r.* INTO source_request FROM bid_async_stage_receipts receipt
    JOIN bid_async_request_snapshot_artifacts r ON r.id=receipt.request_artifact_id
      AND r.frozen_input_sha256=receipt.frozen_input_sha256
    JOIN bid_requirement_set_compile_request_identities i ON i.request_artifact_id=r.id
      AND i.document_set_revision_id=set_row.document_set_id
    WHERE r.project_id=typed.project_id AND r.request_kind='requirement_set_compile' AND r.status='succeeded'
      AND receipt.stage_kind='requirement_compile'
      AND receipt.result_identity->>'requirement_set_id'=set_row.id::text
      AND receipt.result_identity->>'requirement_set_sha256'=set_row.content_sha256::text;
  IF NOT FOUND THEN
    RETURN jsonb_build_object('allowed',false,'reason','analysis_publication_missing');
  END IF;
  input_value:=kb_bid_v2_load_tender_analysis_input(source_request.id,source_request.revision,source_request.frozen_input_sha256)->'input';
  IF payload#>>'{analysis_result,frozen_input_sha256}' IS DISTINCT FROM
      kb_bid_v2_sha256_bytes(convert_to(kb_bid_v2_jcs(input_value),'UTF8'))::text THEN
    RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: export review analysis source input' USING ERRCODE='23514';
  END IF;
  RETURN jsonb_build_object('allowed',true,'analysis_result',payload->'analysis_result','input',input_value);
END $$;

-- A reviewed checkpoint is not a published document. Both object transfers, the
-- new round/current pointer, receipt and terminal state commit together.
CREATE FUNCTION kb_bid_v2_publish_docx_composition(p_id uuid,p_sha kb_sha256,p_attempt integer,p_token uuid,
  p_checkpoint_sha kb_sha256,p_docx_staging uuid,p_manifest_staging uuid)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE typed bid_docx_composition_request_identities%ROWTYPE; checkpoint jsonb; manifest jsonb;
  plan_value jsonb; plan_reviews jsonb;
  docx_bytes bytea; manifest_bytes bytea; docx_sha kb_sha256; manifest_sha kb_sha256;
  response jsonb; stamp timestamptz;
BEGIN
  PERFORM kb_bid_v2_tender_agent_lock_owner(p_id,p_sha,p_attempt,p_token);
  SELECT * INTO STRICT typed FROM bid_docx_composition_request_identities
    WHERE request_artifact_id=p_id AND frozen_input_sha256=p_sha;
  checkpoint:=kb_bid_v2_docx_composition_checkpoint_get(p_id,p_sha);
  manifest:=checkpoint#>'{workspace,artifact,manifest}';
  IF checkpoint IS NULL OR p_checkpoint_sha IS DISTINCT FROM kb_bid_v2_sha256_bytes(convert_to(kb_bid_v2_jcs(checkpoint),'UTF8'))
    OR checkpoint->>'contract_sha256' IS DISTINCT FROM typed.contract_sha256::text
    OR checkpoint#>'{main_progress,blockers}' IS DISTINCT FROM '[]'::jsonb
    OR checkpoint#>'{review_progress,blockers}' IS DISTINCT FROM '[]'::jsonb
    OR checkpoint#>'{workspace,done}' IS DISTINCT FROM 'true'::jsonb
    OR checkpoint#>'{journal,pending}' IS DISTINCT FROM 'null'::jsonb
    OR checkpoint#>'{workspace,reviewing}' IS DISTINCT FROM 'false'::jsonb
    OR checkpoint#>'{workspace,findings}' IS DISTINCT FROM '[]'::jsonb
    OR manifest->>'analysis_sha256' IS DISTINCT FROM typed.frozen_input->>'analysis_sha256'
    OR manifest->>'draft_sha256' IS DISTINCT FROM kb_bid_v2_sha256_bytes(convert_to(kb_bid_v2_jcs(checkpoint#>'{workspace,draft}'),'UTF8'))::text
    OR coalesce(manifest->>'status','') NOT IN ('reviewed_template','reviewed_template_with_open_items') THEN
    RAISE EXCEPTION 'AGENT_OUTPUT_INVALID: reviewed composition checkpoint required' USING ERRCODE='23514';
  END IF;
  plan_value:=checkpoint#>'{workspace,draft,plan}';
  plan_reviews:=checkpoint#>'{workspace,plan_reviews}';
  IF jsonb_typeof(plan_value) IS DISTINCT FROM 'object' OR plan_value='{}'::jsonb
    OR jsonb_typeof(plan_reviews) IS DISTINCT FROM 'object'
    OR manifest->>'plan_sha256' IS DISTINCT FROM kb_bid_v2_sha256_bytes(convert_to(kb_bid_v2_jcs(plan_value),'UTF8'))::text THEN
    RAISE EXCEPTION 'AGENT_OUTPUT_INVALID: current composition plan review required' USING ERRCODE='23514';
  END IF;
  IF EXISTS(SELECT 1 FROM jsonb_each(plan_value) p FULL JOIN jsonb_each(plan_reviews) r ON p.key=r.key
    WHERE p.key IS NULL OR r.key IS NULL OR p.value->>'id' IS DISTINCT FROM p.key
      OR r.value->>'item_id' IS DISTINCT FROM p.key
      OR r.value->>'artifact_sha256' IS DISTINCT FROM manifest->>'docx_sha256'
      OR r.value->>'draft_sha256' IS DISTINCT FROM manifest->>'draft_sha256'
      OR coalesce(r.value->>'conclusion','') NOT IN ('pass','source_limited')
      OR r.value->'finding_ids' IS DISTINCT FROM '[]'::jsonb) THEN
    RAISE EXCEPTION 'AGENT_OUTPUT_INVALID: composition plan findings or stale item review remain' USING ERRCODE='23514';
  END IF;
  docx_bytes:=decode(checkpoint#>>'{workspace,artifact,docx_base64}','base64');
  docx_sha:=kb_bid_v2_sha256_bytes(docx_bytes);
  IF docx_bytes IS NULL OR octet_length(docx_bytes)=0
    OR octet_length(docx_bytes)>(typed.frozen_input#>>'{config,limits,max_docx_bytes}')::bigint
    OR manifest->>'docx_sha256' IS DISTINCT FROM docx_sha::text THEN
    RAISE EXCEPTION 'AGENT_OUTPUT_INVALID: reviewed DOCX digest changed' USING ERRCODE='23514';
  END IF;
  manifest_bytes:=convert_to(kb_bid_v2_jcs(manifest),'UTF8');
  manifest_sha:=kb_bid_v2_sha256_bytes(manifest_bytes);
  -- Request-level replay belongs to the atomic object_commit receipt. Use a
  -- fresh internal round key: a caller-supplied upload key must never turn this
  -- first publication into create_docx_round's replay branch, bypassing its CAS.
  response:=kb_bid_v2_create_docx_round(typed.workspace_id,p_docx_staging,
    (typed.frozen_input->'basis')||jsonb_build_object(
      'expected_version_id',typed.frozen_input#>'{expected,version_id}',
      'expected_docx_sha256',typed.frozen_input#>'{expected,docx_sha256}',
      'docx_sha256',docx_sha,'byte_length',octet_length(docx_bytes)),typed.actor,'docx-compose:'||gen_random_uuid()::text);
  PERFORM kb_object_upload_commit(p_manifest_staging,'objects/'||manifest_sha,manifest_sha,
    'application/json',octet_length(manifest_bytes),'bid_docx_version',
    (response->>'version_id')::uuid,'composition_manifest',typed.actor);
  -- Locks and object transfer may have waited longer than the execution lease.
  stamp:=kb_bid_v2_tender_agent_lock_owner(p_id,p_sha,p_attempt,p_token);
  response:=response||jsonb_build_object('schema_version',1,'request_artifact_id',p_id,
    'workspace_id',typed.workspace_id,'frozen_input_sha256',p_sha,'checkpoint_sha256',p_checkpoint_sha,
    'status','succeeded','composition_status',manifest->>'status',
    'manifest',jsonb_build_object('object_ref','objects/'||manifest_sha,'sha256',manifest_sha,
      'byte_length',octet_length(manifest_bytes)));
  INSERT INTO bid_async_stage_receipts(request_artifact_id,stage_kind,frozen_input_sha256,result_identity,result_sha256)
    VALUES(p_id,'object_commit',p_sha,response,kb_bid_v2_sha256_bytes(convert_to(kb_bid_v2_jcs(response),'UTF8')));
  UPDATE bid_tender_agent_run_artifacts SET status='succeeded',progress_phase='succeeded',
    lease_expires_at=least(lease_expires_at,stamp),updated_at=stamp
    WHERE request_artifact_id=p_id AND attempt=p_attempt;
  UPDATE bid_async_request_snapshot_artifacts SET status='succeeded',result_identity=response,finished_at=stamp WHERE id=p_id;
  RETURN response;
END $$;

-- Worker replay is read-only and independent of an expired execution lease.
-- The caller must also verify both physical objects before reporting success.
CREATE FUNCTION kb_bid_v2_replay_docx_composition(p_id uuid,p_sha kb_sha256)
RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE receipt bid_async_stage_receipts%ROWTYPE; typed bid_docx_composition_request_identities%ROWTYPE;
BEGIN
  SELECT * INTO STRICT typed FROM bid_docx_composition_request_identities
    WHERE request_artifact_id=p_id AND frozen_input_sha256=p_sha;
  SELECT * INTO receipt FROM bid_async_stage_receipts
    WHERE request_artifact_id=p_id AND frozen_input_sha256=p_sha AND stage_kind='object_commit';
  IF NOT FOUND THEN RETURN NULL; END IF;
  IF receipt.result_sha256 IS DISTINCT FROM kb_bid_v2_sha256_bytes(convert_to(kb_bid_v2_jcs(receipt.result_identity),'UTF8'))
    OR NOT EXISTS(SELECT 1 FROM bid_async_request_snapshot_artifacts WHERE id=p_id
      AND status='succeeded' AND result_identity=receipt.result_identity)
    OR NOT EXISTS(SELECT 1 FROM bid_docx_version_artifacts v
      JOIN object_owner_references d ON d.owner_kind='bid_docx_version' AND d.owner_id=v.id AND d.occurrence='document' AND d.object_ref=v.object_ref
      JOIN object_registry dr ON dr.object_ref=d.object_ref AND dr.state='available' AND dr.digest=v.docx_sha256 AND dr.byte_length=v.byte_length
      JOIN object_owner_references m ON m.owner_kind='bid_docx_version' AND m.owner_id=v.id AND m.occurrence='composition_manifest'
      JOIN object_registry mr ON mr.object_ref=m.object_ref AND mr.state='available' AND mr.media_type='application/json'
      WHERE v.workspace_id=typed.workspace_id AND v.id=(receipt.result_identity->>'version_id')::uuid
        AND v.docx_sha256=receipt.result_identity->>'docx_sha256' AND v.byte_length=(receipt.result_identity->>'byte_length')::bigint
        AND mr.object_ref=receipt.result_identity#>>'{manifest,object_ref}' AND mr.digest=receipt.result_identity#>>'{manifest,sha256}'
        AND mr.byte_length=(receipt.result_identity#>>'{manifest,byte_length}')::bigint) THEN
    RAISE EXCEPTION 'AGENT_OUTPUT_INVALID: composition publication identity changed' USING ERRCODE='23514';
  END IF;
  RETURN receipt.result_identity;
END $$;

-- Exact-version lookup: later manual versions do not inherit old placements.
CREATE FUNCTION kb_bid_v2_get_docx_composition_manifest(p_workspace uuid,p_version uuid,p_actor kb_actor_identity)
RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE version_value jsonb; manifest_value jsonb;
BEGIN
  version_value:=kb_bid_v2_get_docx_version(p_workspace,p_version,p_actor);
  SELECT jsonb_build_object('version_id',p_version,'docx_sha256',version_value->>'docx_sha256',
    'object_ref',r.object_ref,'sha256',r.digest,'byte_length',r.byte_length) INTO manifest_value
    FROM object_owner_references o JOIN object_registry r ON r.object_ref=o.object_ref
    WHERE o.owner_kind='bid_docx_version' AND o.owner_id=p_version AND o.occurrence='composition_manifest'
      AND r.state='available' AND r.media_type='application/json';
  RETURN manifest_value;
END $$;

-- An initialized empty requirement set is a valid upload basis but is not an
-- analyzed source from which an Agent can compose a complete template.
CREATE FUNCTION kb_bid_v2_get_docx_composition_basis(p_workspace uuid,p_actor kb_actor_identity)
RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE basis_value jsonb;
BEGIN
  basis_value:=kb_bid_v2_get_docx_round_basis(p_workspace,p_actor);
  IF basis_value IS NULL OR NOT EXISTS(SELECT 1 FROM bid_requirement_set_artifacts a
    JOIN bid_async_stage_receipts r ON r.stage_kind='requirement_compile'
      AND r.result_identity->>'requirement_set_id'=a.id::text AND r.result_identity->>'compiler_version'='4'
    WHERE a.id=(basis_value->>'requirement_set_id')::uuid
      AND (convert_from(a.canonical_payload,'UTF8')::jsonb)->'analysis_result' IS NOT NULL) THEN
    RETURN NULL;
  END IF;
  RETURN basis_value;
END $$;

-- HTTP intent identity excludes server-resolved runtime. Replays must load the
-- original request even after runtime configuration or current sources change.
CREATE FUNCTION kb_bid_v2_replay_docx_composition_submission(p_workspace uuid,p_input jsonb,p_actor kb_actor_identity,p_key text)
RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE project_value uuid; payload bytea;
BEGIN
  SELECT project_id INTO STRICT project_value FROM bid_submission_workspaces WHERE id=p_workspace;
  PERFORM kb_bid_v2_require_project_owner(project_value,p_actor);
  IF jsonb_typeof(p_input) IS DISTINCT FROM 'object' OR NOT kb_bid_v2_json_keys_exact(p_input,ARRAY['basis','expected']) THEN
    RAISE EXCEPTION 'DOCX_COMPOSITION_INPUT_INVALID' USING ERRCODE='23514';
  END IF;
  payload:=convert_to(kb_bid_v2_jcs(jsonb_build_object('workspace_id',p_workspace,'input',p_input)),'UTF8');
  RETURN kb_bid_v2_idempotency_replay(p_actor,'bid.v2.docx_compose.submit',p_key,payload,kb_bid_v2_sha256_bytes(payload));
END $$;

CREATE FUNCTION kb_bid_v2_submit_docx_composition_request(p_snapshot jsonb,p_contract jsonb,p_actor kb_actor_identity,p_key text)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE workspace_value uuid; project_value uuid; payload bytea; sha kb_sha256; replay bytea; response jsonb; response_bytes bytea;
BEGIN
  workspace_value:=(p_snapshot->>'workspace_id')::uuid;
  SELECT project_id INTO STRICT project_value FROM bid_submission_workspaces WHERE id=workspace_value;
  PERFORM kb_bid_v2_require_project_owner(project_value,p_actor);
  payload:=convert_to(kb_bid_v2_jcs(jsonb_build_object('workspace_id',workspace_value,
    'input',jsonb_build_object('basis',p_snapshot->'basis','expected',p_snapshot->'expected'))),'UTF8');
  sha:=kb_bid_v2_sha256_bytes(payload);
  replay:=kb_bid_v2_idempotency_begin(p_actor,'bid.v2.docx_compose.submit',p_key,payload,sha);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  response:=kb_bid_v2_create_docx_composition_request(gen_random_uuid(),p_snapshot,p_contract,p_actor,gen_random_uuid()::text);
  response_bytes:=convert_to(response::text,'UTF8');
  INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
    entity_kind,entity_locator,after_revision,after_sha256)
  VALUES(gen_random_uuid(),1,'bid.v2.docx_compose.submit',p_actor,p_key,sha,kb_bid_v2_sha256_bytes(response_bytes),
    'bid_v2_docx_composition_request',jsonb_build_object('workspace_id',workspace_value,'request_artifact_id',response->'request_artifact_id'),
    (response->>'request_revision')::bigint,(response->>'frozen_input_sha256')::kb_sha256);
  PERFORM kb_bid_v2_idempotency_complete(p_actor,'bid.v2.docx_compose.submit',p_key,202,response_bytes);
  RETURN response;
END $$;

CREATE FUNCTION kb_bid_v2_get_docx_composition_request(p_workspace uuid,p_id uuid,p_actor kb_actor_identity)
RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE project_value uuid; result_value jsonb;
BEGIN
  SELECT project_id INTO STRICT project_value FROM bid_submission_workspaces WHERE id=p_workspace;
  PERFORM kb_bid_v2_require_project_owner(project_value,p_actor);
  SELECT jsonb_build_object('request_artifact_id',r.id,'request_revision',r.revision,'frozen_input_sha256',r.frozen_input_sha256,
    'workspace_id',t.workspace_id,'status',r.status,'result_identity',r.result_identity,'error_code',r.error_code,
    'basis',t.frozen_input->'basis','expected',t.frozen_input->'expected',
    'progress',CASE WHEN a.request_artifact_id IS NULL THEN NULL ELSE jsonb_build_object(
      'phase',a.progress_phase,'sequence',a.progress_sequence,'detail',a.progress_detail,'attempt',a.attempt) END)
    INTO result_value FROM bid_docx_composition_request_identities t
    JOIN bid_async_request_snapshot_artifacts r ON r.id=t.request_artifact_id
    LEFT JOIN bid_tender_agent_run_artifacts a ON a.request_artifact_id=r.id AND a.attempt=r.current_attempt
    WHERE t.workspace_id=p_workspace AND (p_id IS NULL OR r.id=p_id)
    ORDER BY r.created_at DESC,r.id DESC LIMIT 1;
  RETURN result_value;
END $$;

-- Formal exports preserve a saved DOCX identity. The legacy workspace renderer
-- remains a preview consumer only; it cannot produce submission packages.
ALTER TABLE bid_submission_export_request_identities
  ADD FOREIGN KEY(project_id,workspace_id,round_id,version_id,docx_sha256)
    REFERENCES bid_docx_version_artifacts(project_id,workspace_id,round_id,id,docx_sha256);

CREATE FUNCTION kb_bid_v2_submission_docx_source(p_workspace uuid,p_version uuid,p_sha kb_sha256)
RETURNS jsonb LANGUAGE sql STABLE SET search_path=pg_catalog,public AS $$
 SELECT jsonb_build_object('round_id',r.id,'round_sha256',r.content_sha256,
   'version_id',v.id,'docx_sha256',v.docx_sha256,'object_ref',v.object_ref,'byte_length',v.byte_length,
   'document_set_id',ds.id,'document_set_sha256',ds.content_sha256,
   'requirement_set_id',rs.id,'requirement_set_sha256',rs.content_sha256)
 FROM bid_docx_version_artifacts v
 JOIN bid_docx_round_artifacts r ON r.id=v.round_id AND r.workspace_id=v.workspace_id AND r.project_id=v.project_id
 JOIN bid_document_set_artifacts ds ON ds.id=r.document_set_id AND ds.project_id=r.project_id
 JOIN bid_requirement_set_artifacts rs ON rs.id=r.requirement_set_id AND rs.project_id=r.project_id AND rs.document_set_id=ds.id
 JOIN object_registry o ON o.object_ref=v.object_ref AND o.digest=v.docx_sha256 AND o.byte_length=v.byte_length
   AND o.media_type='application/vnd.openxmlformats-officedocument.wordprocessingml.document' AND o.state='available'
 JOIN object_owner_references owned ON owned.object_ref=v.object_ref AND owned.owner_kind='bid_docx_version'
   AND owned.owner_id=v.id AND owned.occurrence='document'
 WHERE v.workspace_id=p_workspace AND v.id=p_version AND v.docx_sha256=p_sha
$$;
CREATE FUNCTION kb_bid_v2_validate_submission_docx_source()
RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog,public AS $$
BEGIN
 IF NEW.source IS DISTINCT FROM kb_bid_v2_submission_docx_source(NEW.workspace_id,NEW.version_id,NEW.docx_sha256) THEN
   RAISE EXCEPTION 'SUBMISSION_DOCX_SOURCE_INVALID' USING ERRCODE='23514';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER bid_submission_docx_source_valid BEFORE INSERT ON bid_submission_export_request_identities
FOR EACH ROW EXECUTE FUNCTION kb_bid_v2_validate_submission_docx_source();

CREATE FUNCTION kb_bid_v2_create_submission_export_request(
 p_workspace_id uuid,p_expected_version_id uuid,p_expected_sha256 kb_sha256,
 p_actor kb_actor_identity,p_idempotency_key text,p_request_bytes bytea,p_request_sha256 kb_sha256,
 p_frozen_context jsonb DEFAULT NULL
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE workspace bid_submission_workspaces%ROWTYPE; head bid_docx_current%ROWTYPE;
 source_value jsonb; frozen_sha kb_sha256; request_id uuid:=gen_random_uuid();
 job_bytes bytea; job_sha kb_sha256; replay bytea; response jsonb;
 context_value jsonb; set_payload jsonb; analysis_identity jsonb;
BEGIN
 SELECT * INTO STRICT workspace FROM bid_submission_workspaces WHERE id=p_workspace_id;
 PERFORM kb_bid_v2_require_project_owner(workspace.project_id,p_actor);
 replay:=kb_bid_v2_idempotency_begin(p_actor,'bid.v2.submission-export.create',p_idempotency_key,p_request_bytes,p_request_sha256);
 IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
 PERFORM 1 FROM bid_projects WHERE id=workspace.project_id AND status='open' FOR SHARE;
 IF NOT FOUND THEN RAISE EXCEPTION 'PROJECT_ENDED' USING ERRCODE='55000'; END IF;
 PERFORM 1 FROM bid_submission_workspaces WHERE id=p_workspace_id FOR UPDATE;
 SELECT * INTO STRICT head FROM bid_docx_current WHERE scope_id=p_workspace_id FOR UPDATE;
 IF head.version_id IS DISTINCT FROM p_expected_version_id OR head.docx_sha256 IS DISTINCT FROM p_expected_sha256 THEN
   RAISE EXCEPTION 'DOCX_VERSION_CAS_MISMATCH' USING ERRCODE='40001';
 END IF;
 IF head.pending_save_id IS NOT NULL THEN RAISE EXCEPTION 'DOCX_SAVE_PENDING' USING ERRCODE='40001'; END IF;
 IF head.editor_error IS NOT NULL THEN RAISE EXCEPTION 'DOCX_SAVE_ERROR' USING ERRCODE='55000'; END IF;
 source_value:=kb_bid_v2_submission_docx_source(p_workspace_id,head.version_id,head.docx_sha256);
 IF source_value IS NULL THEN RAISE EXCEPTION 'SUBMISSION_DOCX_SOURCE_INVALID' USING ERRCODE='23514'; END IF;
 IF p_frozen_context IS NOT NULL AND (
   jsonb_typeof(p_frozen_context) IS DISTINCT FROM 'object'
   OR NOT kb_bid_v2_json_keys_exact(p_frozen_context,ARRAY['analysis_identity','execution_contract','layout_result'])
 ) THEN RAISE EXCEPTION 'SUBMISSION_EXPORT_CONTEXT_INVALID' USING ERRCODE='23514'; END IF;
   SELECT convert_from(canonical_payload,'UTF8')::jsonb INTO set_payload
     FROM bid_requirement_set_artifacts
     WHERE id=(source_value->>'requirement_set_id')::uuid
       AND content_sha256=(source_value->>'requirement_set_sha256')::kb_sha256;
   IF set_payload#>'{analysis_result,review,draft}' = 'true'::jsonb THEN
     RAISE EXCEPTION 'SUBMISSION_EXPORT_CONTEXT_INVALID: official export rejects draft analysis' USING ERRCODE='23514';
   END IF;
   IF set_payload IS NOT NULL AND jsonb_typeof(set_payload->'analysis_result')='object' THEN
     analysis_identity:=jsonb_build_object(
       'id',source_value->>'requirement_set_id',
       'sha256',source_value->>'requirement_set_sha256',
       'schema_version',coalesce(set_payload#>'{analysis_result,schema_version}','1'::jsonb),
       'analysis_sha256',set_payload#>'{analysis_result,review,analysis_sha256}');
   ELSE
     analysis_identity:=NULL;
   END IF;
   context_value:=jsonb_build_object(
     'analysis_identity',analysis_identity,
     'execution_contract',p_frozen_context->'execution_contract',
     'layout_result',p_frozen_context->'layout_result');
 IF p_frozen_context->>'analysis_identity' IS NOT NULL AND p_frozen_context->'analysis_identity' IS DISTINCT FROM analysis_identity THEN
   RAISE EXCEPTION 'SUBMISSION_EXPORT_CONTEXT_INVALID: source analysis changed' USING ERRCODE='23514';
 END IF;
 IF context_value->>'execution_contract' IS NOT NULL AND (
   NOT kb_bid_v2_json_keys_exact(context_value->'execution_contract',ARRAY['schema_version','definition','contract_sha256'])
   OR context_value#>'{execution_contract,schema_version}' IS DISTINCT FROM '1'::jsonb
   OR jsonb_typeof(context_value#>'{execution_contract,definition,config}') IS DISTINCT FROM 'object'
   OR context_value#>>'{execution_contract,contract_sha256}' IS DISTINCT FROM kb_bid_v2_sha256_bytes(convert_to(kb_bid_v2_jcs(context_value#>'{execution_contract,definition}'),'UTF8'))::text) THEN
   RAISE EXCEPTION 'SUBMISSION_EXPORT_CONTEXT_INVALID: execution contract' USING ERRCODE='23514';
 END IF;
 frozen_sha:=kb_bid_v2_sha256_bytes(convert_to(kb_bid_v2_jcs(jsonb_build_object('source',source_value,'context',context_value)),'UTF8'));
 job_bytes:=kb_bid_v2_json_payload(jsonb_build_object('job_kind','submission_export','request',jsonb_build_object(
   'request_artifact_id',request_id,'request_revision',1,'frozen_input_sha256',frozen_sha),
   'project_id',workspace.project_id,'workspace_id',p_workspace_id));
 job_sha:=kb_bid_v2_sha256_bytes(job_bytes);
 INSERT INTO bid_async_request_snapshot_artifacts(id,project_id,workspace_id,request_kind,revision,
   frozen_input_sha256,request_payload,request_sha256,status)
 VALUES(request_id,workspace.project_id,p_workspace_id,'submission_export',1,frozen_sha,job_bytes,job_sha,'pending');
 INSERT INTO bid_submission_export_request_identities(request_artifact_id,project_id,workspace_id,request_revision,
   request_sha256,frozen_input_sha256,round_id,version_id,docx_sha256,source,frozen_context)
 VALUES(request_id,workspace.project_id,p_workspace_id,1,job_sha,frozen_sha,head.round_id,head.version_id,head.docx_sha256,source_value,context_value);
 response:=jsonb_build_object('request_artifact_id',request_id,'kind','SubmissionExport','status','pending',
   'result_identity',NULL,'error_code',NULL,'request_revision',1,'request_sha256',job_sha,
   'frozen_input_sha256',frozen_sha,'project_id',workspace.project_id,'workspace_id',p_workspace_id,
   'source',source_value,'frozen_context',context_value);
 PERFORM kb_bid_v2_idempotency_complete(p_actor,'bid.v2.submission-export.create',p_idempotency_key,202,kb_bid_v2_json_payload(response));
 RETURN response;
END $$;

CREATE FUNCTION kb_bid_v2_load_submission_export_input(p_request_artifact_id uuid,p_request_revision bigint,p_frozen_input_sha256 kb_sha256)
RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE typed bid_submission_export_request_identities%ROWTYPE; request_value bid_async_request_snapshot_artifacts%ROWTYPE;
BEGIN
 SELECT * INTO STRICT typed FROM bid_submission_export_request_identities
 WHERE request_artifact_id=p_request_artifact_id AND request_revision=p_request_revision AND frozen_input_sha256=p_frozen_input_sha256;
 SELECT * INTO STRICT request_value FROM bid_async_request_snapshot_artifacts WHERE id=typed.request_artifact_id AND status IN ('pending','succeeded');
 RETURN jsonb_build_object('request',to_jsonb(typed),'project_id',typed.project_id,'workspace_id',typed.workspace_id,
   'project_title',(SELECT title FROM bid_projects WHERE id=typed.project_id),'source',typed.source,
   'published',request_value.result_identity,
   'render',(SELECT result_identity FROM bid_async_stage_receipts WHERE request_artifact_id=typed.request_artifact_id AND frozen_input_sha256=typed.frozen_input_sha256 AND stage_kind='render'),
   'render_snapshot',(SELECT result_identity FROM bid_async_stage_receipts WHERE request_artifact_id=typed.request_artifact_id AND frozen_input_sha256=typed.frozen_input_sha256 AND stage_kind='render_snapshot'));
END $$;

CREATE FUNCTION kb_bid_v2_load_submission_export_source(p_workspace_id uuid,p_request_id uuid,p_version_id uuid,p_docx_sha256 kb_sha256)
RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE source_value jsonb;
BEGIN
 SELECT typed.source INTO STRICT source_value FROM bid_submission_export_request_identities typed
 JOIN bid_async_request_snapshot_artifacts request_value ON request_value.id=typed.request_artifact_id AND request_value.status IN ('pending','succeeded')
 WHERE typed.request_artifact_id=p_request_id AND typed.workspace_id=p_workspace_id
   AND typed.version_id=p_version_id AND typed.docx_sha256=p_docx_sha256
   AND typed.source=kb_bid_v2_submission_docx_source(p_workspace_id,p_version_id,p_docx_sha256);
 RETURN source_value;
END $$;

CREATE FUNCTION kb_bid_v2_submission_export_render_put(p_id uuid,p_sha kb_sha256,p_attempt integer,p_token uuid,p_staging uuid,p_value jsonb)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE typed bid_submission_export_request_identities%ROWTYPE; prior bid_async_stage_receipts%ROWTYPE; pdf jsonb; sha kb_sha256;
BEGIN
 PERFORM kb_bid_v2_tender_agent_lock_owner(p_id,p_sha,p_attempt,p_token);
 SELECT * INTO STRICT typed FROM bid_submission_export_request_identities WHERE request_artifact_id=p_id AND frozen_input_sha256=p_sha AND request_kind='submission_export';
 pdf:=p_value->'pdf';
 IF NOT kb_bid_v2_json_keys_exact(p_value,ARRAY['schema_version','source','pdf'])
   OR p_value->'schema_version' IS DISTINCT FROM '1'::jsonb OR p_value->'source' IS DISTINCT FROM typed.source
   OR NOT kb_bid_v2_json_keys_exact(pdf,ARRAY['object_ref','sha256','media_type','byte_length'])
   OR pdf->>'object_ref' IS DISTINCT FROM 'objects/'||(pdf->>'sha256')
   OR pdf->>'media_type' IS DISTINCT FROM 'application/pdf' OR coalesce((pdf->>'byte_length')::bigint,0)<=0 THEN
   RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: export render identity' USING ERRCODE='23514';
 END IF;
 sha:=kb_bid_v2_sha256_bytes(convert_to(kb_bid_v2_jcs(p_value),'UTF8'));
 SELECT * INTO prior FROM bid_async_stage_receipts WHERE request_artifact_id=p_id AND frozen_input_sha256=p_sha AND stage_kind='render';
 IF FOUND AND (prior.result_sha256<>sha OR prior.result_identity IS DISTINCT FROM p_value) THEN
   RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: PDF already frozen' USING ERRCODE='23514';
 END IF;
 IF prior.request_artifact_id IS NULL OR EXISTS(SELECT 1 FROM object_upload_staging WHERE id=p_staging) THEN
   PERFORM kb_object_upload_commit(p_staging,(pdf->>'object_ref')::kb_object_ref,(pdf->>'sha256')::kb_sha256,
     'application/pdf',(pdf->>'byte_length')::bigint,'bid_submission_export_request',p_id,'render:pdf','system:submission-export-v2');
 END IF;
 IF prior.request_artifact_id IS NULL THEN
   INSERT INTO bid_async_stage_receipts(request_artifact_id,stage_kind,frozen_input_sha256,result_identity,result_sha256)
     VALUES(p_id,'render',p_sha,p_value,sha);
 END IF;
 RETURN p_value;
END $$;

CREATE FUNCTION kb_bid_v2_submission_export_snapshot_put(p_id uuid,p_sha kb_sha256,p_attempt integer,p_token uuid,p_value jsonb,p_image_stages jsonb DEFAULT '{}'::jsonb)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE typed bid_submission_export_request_identities%ROWTYPE; render jsonb; prior bid_async_stage_receipts%ROWTYPE; sha kb_sha256; image_entry record; image jsonb; image_key text;
BEGIN
 PERFORM kb_bid_v2_tender_agent_lock_owner(p_id,p_sha,p_attempt,p_token);
 SELECT * INTO STRICT typed FROM bid_submission_export_request_identities WHERE request_artifact_id=p_id AND frozen_input_sha256=p_sha AND request_kind='submission_export';
 SELECT result_identity INTO STRICT render FROM bid_async_stage_receipts WHERE request_artifact_id=p_id AND frozen_input_sha256=p_sha AND stage_kind='render';
 IF NOT kb_bid_v2_json_keys_exact(p_value,ARRAY['schema_version','inventory','inventory_sha256'])
   OR p_value->'schema_version' IS DISTINCT FROM '2'::jsonb
   OR jsonb_typeof(p_value#>'{inventory,images}') IS DISTINCT FROM 'object'
   OR jsonb_typeof(p_image_stages) IS DISTINCT FROM 'object'
   OR jsonb_typeof(p_value#>'{inventory,units}') IS DISTINCT FROM 'array'
   OR jsonb_typeof(p_value#>'{inventory,parser_manifests}') IS DISTINCT FROM 'array'
   OR jsonb_array_length(p_value#>'{inventory,parser_manifests}')<>2
   OR EXISTS(SELECT 1 FROM jsonb_array_elements(p_value#>'{inventory,parser_manifests}') m
     WHERE m->'schema_version' IS DISTINCT FROM '1'::jsonb OR m->>'profile' IS DISTINCT FROM 'output_inventory_v1'
       OR jsonb_typeof(m->'parser') IS DISTINCT FROM 'string' OR coalesce(m->>'parser','')='' OR jsonb_typeof(m->'config') IS DISTINCT FROM 'object')
   OR (SELECT count(DISTINCT m->>'file_sha256') FROM jsonb_array_elements(p_value#>'{inventory,parser_manifests}') m WHERE m->>'file_sha256' IN (typed.docx_sha256::text,render#>>'{pdf,sha256}'))<>2
   OR p_value#>>'{inventory,docx_sha256}' IS DISTINCT FROM typed.docx_sha256::text
   OR p_value#>'{inventory,pdf_sha256}' IS DISTINCT FROM render#>'{pdf,sha256}'
   OR p_value->>'inventory_sha256' IS DISTINCT FROM kb_bid_v2_sha256_bytes(convert_to(kb_bid_v2_jcs(p_value->'inventory'),'UTF8'))::text THEN
   RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: export inventory identity' USING ERRCODE='23514';
 END IF;
 -- Every image address is frozen from the parser manifests; no untracked
 -- pixel object or cross-file image reference may enter the inventory.
 IF EXISTS(SELECT 1 FROM jsonb_array_elements(p_value#>'{inventory,parser_manifests}') m,
     jsonb_each_text(coalesce(m->'image_sha256','{}'::jsonb)) entry
     WHERE NOT p_value#>'{inventory,images}' ? entry.value)
   OR EXISTS(SELECT 1 FROM jsonb_array_elements(p_value#>'{inventory,units}') unit,
     jsonb_array_elements_text(coalesce(unit->'image_sha256s','[]'::jsonb)) image_sha
     WHERE NOT p_value#>'{inventory,images}' ? image_sha
       OR NOT EXISTS(SELECT 1 FROM jsonb_array_elements(p_value#>'{inventory,parser_manifests}') m,
         jsonb_each_text(coalesce(m->'image_sha256','{}'::jsonb)) entry
         WHERE m->>'file_sha256'=unit->>'file_sha256' AND entry.value=image_sha))
   OR EXISTS(SELECT 1 FROM jsonb_object_keys(p_image_stages) stage_key WHERE NOT p_value#>'{inventory,images}' ? stage_key) THEN
   RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: export image manifest references' USING ERRCODE='23514';
 END IF;
 FOR image_entry IN SELECT key,value FROM jsonb_each(p_value#>'{inventory,images}') LOOP
   image:=image_entry.value; image_key:=image_entry.key;
   IF image_key !~ '^[0-9a-f]{64}$'
     OR NOT kb_bid_v2_json_keys_exact(image,ARRAY['sha256','object_ref','media_type','byte_length','width','height'])
     OR image->>'sha256' IS DISTINCT FROM image_key OR image->>'object_ref' IS DISTINCT FROM 'objects/'||image_key
     OR jsonb_typeof(image->'media_type') IS DISTINCT FROM 'string' OR coalesce(image->>'media_type','')=''
     OR coalesce(image->>'byte_length','') !~ '^[1-9][0-9]*$'
     OR coalesce(image->>'width','') !~ '^[0-9]+$' OR coalesce(image->>'height','') !~ '^[0-9]+$'
     OR NOT EXISTS(SELECT 1 FROM jsonb_array_elements(p_value#>'{inventory,parser_manifests}') m,
       jsonb_each_text(coalesce(m->'image_sha256','{}'::jsonb)) entry WHERE entry.value=image_key) THEN
     RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: export image object identity' USING ERRCODE='23514';
   END IF;
 END LOOP;
 sha:=kb_bid_v2_sha256_bytes(convert_to(kb_bid_v2_jcs(p_value),'UTF8'));
 SELECT * INTO prior FROM bid_async_stage_receipts WHERE request_artifact_id=p_id AND frozen_input_sha256=p_sha AND stage_kind='render_snapshot';
 IF FOUND THEN
   IF prior.result_sha256<>sha OR prior.result_identity IS DISTINCT FROM p_value THEN
     RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: export inventory already frozen' USING ERRCODE='23514';
   END IF;
 END IF;
 FOR image_entry IN SELECT key,value FROM jsonb_each(p_value#>'{inventory,images}') LOOP
   image:=image_entry.value; image_key:=image_entry.key;
   IF p_image_stages ? image_key THEN
     PERFORM kb_object_upload_commit((p_image_stages->>image_key)::uuid,(image->>'object_ref')::kb_object_ref,
       image_key::kb_sha256,image->>'media_type',(image->>'byte_length')::bigint,
       'bid_submission_export_request',p_id,'inventory:image:'||image_key,'system:submission-export-v2');
   ELSIF prior.request_artifact_id IS NULL THEN
     RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: export image staging missing' USING ERRCODE='23514';
   END IF;
   IF NOT EXISTS(SELECT 1 FROM object_registry o JOIN object_owner_references owned ON owned.object_ref=o.object_ref
     WHERE o.object_ref=image->>'object_ref' AND o.digest=image_key::kb_sha256 AND o.media_type=image->>'media_type'
       AND o.byte_length=(image->>'byte_length')::bigint AND o.state='available'
       AND owned.owner_kind='bid_submission_export_request' AND owned.owner_id=p_id AND owned.occurrence='inventory:image:'||image_key) THEN
     RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: frozen export image owner missing' USING ERRCODE='23514';
   END IF;
 END LOOP;
 IF prior.request_artifact_id IS NOT NULL THEN RETURN prior.result_identity; END IF;
 INSERT INTO bid_async_stage_receipts(request_artifact_id,stage_kind,frozen_input_sha256,result_identity,result_sha256)
   VALUES(p_id,'render_snapshot',p_sha,p_value,sha);
 RETURN p_value;
END $$;

CREATE FUNCTION kb_bid_v2_publish_submission_export(
 p_request_artifact_id uuid,p_request_revision bigint,p_frozen_input_sha256 kb_sha256,p_manifest_id uuid,
 p_docx jsonb,p_pdf jsonb,p_report jsonb,p_actor kb_actor_identity,p_attempt integer,p_token uuid
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE typed bid_submission_export_request_identities%ROWTYPE; request_value bid_async_request_snapshot_artifacts%ROWTYPE;
 report_payload bytea; report_sha kb_sha256; report_id uuid; manifest_payload bytea; manifest_sha kb_sha256;
 render jsonb; snapshot jsonb; review_checkpoint jsonb; contract text; review_status text; check_status text; stamp timestamptz;
 docx_identity jsonb; pdf_identity jsonb; result_value jsonb; item jsonb; format_value text; output_id uuid; image_entry record; image jsonb;
BEGIN
 IF p_actor IS DISTINCT FROM 'system:submission-export-v2' THEN RAISE EXCEPTION 'SYSTEM_ACTOR_REQUIRED' USING ERRCODE='42501'; END IF;
 SELECT * INTO STRICT typed FROM bid_submission_export_request_identities
 WHERE request_artifact_id=p_request_artifact_id AND request_revision=p_request_revision AND frozen_input_sha256=p_frozen_input_sha256;
 SELECT * INTO STRICT request_value FROM bid_async_request_snapshot_artifacts WHERE id=p_request_artifact_id FOR UPDATE;
 IF request_value.status='succeeded' THEN RETURN request_value.result_identity; END IF;
 IF typed.request_kind<>'submission_export' THEN RAISE EXCEPTION 'REQUEST_OBSOLETE' USING ERRCODE='P0002'; END IF;
 IF request_value.status<>'pending' THEN RAISE EXCEPTION 'SUBMISSION_EXPORT_NOT_PENDING' USING ERRCODE='55000'; END IF;
 stamp:=kb_bid_v2_tender_agent_lock_owner(p_request_artifact_id,p_frozen_input_sha256,p_attempt,p_token);
 IF p_manifest_id IS NULL OR jsonb_typeof(p_docx) IS DISTINCT FROM 'object' OR jsonb_typeof(p_pdf) IS DISTINCT FROM 'object' THEN
   RAISE EXCEPTION 'SUBMISSION_OUTPUT_IDENTITY_INVALID' USING ERRCODE='23514';
 END IF;
 FOR item,format_value IN SELECT p_docx,'docx' UNION ALL SELECT p_pdf,'pdf' LOOP
   IF NOT kb_bid_v2_json_keys_exact(item,ARRAY['staging_id','artifact_id','object_ref','sha256','media_type','byte_length'])
     OR (format_value='docx' AND item->>'staging_id' IS NULL) OR item->>'artifact_id' IS NULL OR item->>'sha256' IS NULL
     OR item->>'object_ref' IS DISTINCT FROM 'objects/'||(item->>'sha256')
     OR (item->>'byte_length')::bigint IS NULL OR (item->>'byte_length')::bigint<=0
     OR item->>'media_type' IS DISTINCT FROM (CASE WHEN format_value='docx'
       THEN 'application/vnd.openxmlformats-officedocument.wordprocessingml.document' ELSE 'application/pdf' END) THEN
     RAISE EXCEPTION 'SUBMISSION_OUTPUT_IDENTITY_INVALID' USING ERRCODE='23514';
   END IF;
 END LOOP;
 IF p_docx->>'sha256' IS DISTINCT FROM typed.docx_sha256::text
   OR p_docx->'byte_length' IS DISTINCT FROM typed.source->'byte_length'
   OR p_docx->>'artifact_id'=p_pdf->>'artifact_id' THEN
   RAISE EXCEPTION 'SUBMISSION_DOCX_IDENTITY_MISMATCH' USING ERRCODE='23514';
 END IF;
 SELECT result_identity INTO STRICT render FROM bid_async_stage_receipts WHERE request_artifact_id=p_request_artifact_id AND frozen_input_sha256=p_frozen_input_sha256 AND stage_kind='render';
 SELECT result_identity INTO STRICT snapshot FROM bid_async_stage_receipts WHERE request_artifact_id=p_request_artifact_id AND frozen_input_sha256=p_frozen_input_sha256 AND stage_kind='render_snapshot';
 IF render->'source' IS DISTINCT FROM typed.source OR render->'pdf' IS DISTINCT FROM p_pdf-ARRAY['staging_id','artifact_id']
   OR snapshot->'schema_version' IS DISTINCT FROM '2'::jsonb
   OR p_report->'output_images' IS DISTINCT FROM snapshot#>'{inventory,images}'
   OR p_report#>'{output_inventory,inventory_sha256}' IS DISTINCT FROM snapshot->'inventory_sha256' THEN
   RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: published export differs from frozen files or inventory' USING ERRCODE='23514';
 END IF;
 IF NOT EXISTS(SELECT 1 FROM object_registry o JOIN object_owner_references owned ON owned.object_ref=o.object_ref
   WHERE o.object_ref=p_pdf->>'object_ref' AND o.digest=(p_pdf->>'sha256')::kb_sha256 AND o.state='available'
     AND o.byte_length=(p_pdf->>'byte_length')::bigint AND o.media_type='application/pdf'
     AND owned.owner_kind='bid_submission_export_request' AND owned.owner_id=p_request_artifact_id AND owned.occurrence='render:pdf') THEN
   RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: frozen PDF owner missing' USING ERRCODE='23514';
 END IF;
 FOR image_entry IN SELECT key,value FROM jsonb_each(snapshot#>'{inventory,images}') LOOP
   image:=image_entry.value;
   IF NOT EXISTS(SELECT 1 FROM object_registry o JOIN object_owner_references owned ON owned.object_ref=o.object_ref
     WHERE o.object_ref=image->>'object_ref' AND o.digest=image_entry.key::kb_sha256 AND o.media_type=image->>'media_type'
       AND o.byte_length=(image->>'byte_length')::bigint AND o.state='available'
       AND owned.owner_kind='bid_submission_export_request' AND owned.owner_id=p_request_artifact_id AND owned.occurrence='inventory:image:'||image_entry.key) THEN
     RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: frozen export image owner missing' USING ERRCODE='23514';
   END IF;
 END LOOP;
 contract:=typed.frozen_context#>>'{execution_contract,contract_sha256}';
 IF contract IS NOT NULL AND typed.frozen_context#>'{analysis_identity,schema_version}'='2'::jsonb THEN
   review_checkpoint:=kb_bid_v2_export_review_checkpoint_get(p_request_artifact_id,p_frozen_input_sha256);
   review_status:=CASE
     WHEN EXISTS(SELECT 1 FROM jsonb_each(review_checkpoint->'reviews') r WHERE r.value->>'conclusion'='findings') THEN 'reviewed_with_findings'
     WHEN EXISTS(SELECT 1 FROM jsonb_each(review_checkpoint->'reviews') r WHERE r.value->>'conclusion'='not_checked') THEN 'not_checked'
     WHEN EXISTS(SELECT 1 FROM jsonb_each(review_checkpoint->'reviews') r WHERE r.value->>'conclusion'='source_limited') THEN 'reviewed_with_source_limitations'
     ELSE 'reviewed' END;
   check_status:=CASE review_status WHEN 'reviewed' THEN 'pass' WHEN 'reviewed_with_findings' THEN 'fail' ELSE 'not_checked' END;
   IF review_checkpoint->>'contract_sha256' IS DISTINCT FROM contract
     OR review_checkpoint->'done' IS DISTINCT FROM 'true'::jsonb
     OR review_checkpoint->'inventory' IS DISTINCT FROM snapshot->'inventory'
     OR p_report->>'execution_contract_sha256' IS DISTINCT FROM contract
     OR p_report->>'review_checkpoint_sha256' IS DISTINCT FROM kb_bid_v2_sha256_bytes(convert_to(kb_bid_v2_jcs(review_checkpoint),'UTF8'))::text
     OR p_report#>'{export_review,inventory_sha256}' IS DISTINCT FROM snapshot->'inventory_sha256'
     OR p_report#>'{export_review,docx_sha256}' IS DISTINCT FROM to_jsonb(typed.docx_sha256)
     OR p_report#>'{export_review,pdf_sha256}' IS DISTINCT FROM render#>'{pdf,sha256}'
     OR p_report#>'{export_review,reviews}' IS DISTINCT FROM review_checkpoint->'reviews'
     OR p_report#>'{export_review,obligations}' IS DISTINCT FROM review_checkpoint->'obligations'
     OR p_report#>>'{export_review,status}' IS DISTINCT FROM review_status
     OR (SELECT count(*) FROM jsonb_array_elements(p_report->'checks') c WHERE c->>'id'='export_review' AND c->>'status'=check_status)<>1
     OR EXISTS(SELECT 1 FROM jsonb_array_elements(p_report->'checks') c WHERE c->>'id'='export_review' AND c->>'status' IS DISTINCT FROM check_status) THEN
     RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: completed same-contract export review required' USING ERRCODE='23514';
   END IF;
 ELSIF p_report->>'export_review' IS NOT NULL OR EXISTS(SELECT 1 FROM jsonb_array_elements(p_report->'checks') c WHERE c->>'id'='export_review' AND c->>'status'<>'not_checked') THEN
   RAISE EXCEPTION 'FROZEN_INPUT_DIGEST_MISMATCH: unfrozen semantic review cannot be published' USING ERRCODE='23514';
 END IF;
 docx_identity:=p_docx-ARRAY['staging_id','object_ref','media_type'];
 pdf_identity:=p_pdf-ARRAY['staging_id','object_ref','media_type'];
 IF jsonb_typeof(p_report) IS DISTINCT FROM 'object' OR p_report->'schema_version' IS DISTINCT FROM '2'::jsonb
   OR p_report->'source' IS DISTINCT FROM typed.source
   OR p_report->'outputs' IS DISTINCT FROM jsonb_build_object('docx',docx_identity,'pdf',pdf_identity)
   OR jsonb_typeof(p_report->'checks') IS DISTINCT FROM 'array' THEN
   RAISE EXCEPTION 'SUBMISSION_REPORT_IDENTITY_INVALID' USING ERRCODE='23514';
 END IF;
 IF jsonb_array_length(p_report->'checks')=0 OR EXISTS(SELECT 1 FROM jsonb_array_elements(p_report->'checks') check_value
   WHERE jsonb_typeof(check_value) IS DISTINCT FROM 'object' OR coalesce(check_value->>'id','')=''
     OR coalesce(check_value->>'status','') NOT IN ('pass','fail','not_checked')
     OR jsonb_typeof(check_value->'detail') IS DISTINCT FROM 'string') THEN
   RAISE EXCEPTION 'SUBMISSION_REPORT_CHECKS_INVALID' USING ERRCODE='23514';
 END IF;
 report_id:=kb_bid_v2_deterministic_uuid(p_manifest_id::text||':assessment-report');
 report_payload:=kb_bid_v2_json_payload(p_report);report_sha:=kb_bid_v2_sha256_bytes(report_payload);
 manifest_payload:=kb_bid_v2_json_payload(jsonb_build_object('schema_version',2,'request_artifact_id',p_request_artifact_id,
   'source',typed.source,'outputs',jsonb_build_object('docx',docx_identity,'pdf',pdf_identity),'output_images',snapshot#>'{inventory,images}','report_sha256',report_sha));
 manifest_sha:=kb_bid_v2_sha256_bytes(manifest_payload);
 INSERT INTO bid_submission_manifest_artifacts(id,project_id,workspace_id,request_artifact_id,source,canonical_payload,content_sha256)
 VALUES(p_manifest_id,typed.project_id,typed.workspace_id,p_request_artifact_id,typed.source,manifest_payload,manifest_sha);
 INSERT INTO bid_submission_manifest_dependencies(manifest_id,dependency_kind,dependency_id,dependency_sha256,ordinal)
 SELECT p_manifest_id,dependency_kind,dependency_id,dependency_sha256,(row_number() OVER(ORDER BY dependency_kind)-1)::integer
 FROM kb_bid_v2_manifest_expected_dependencies(p_manifest_id);
 FOR item,format_value IN SELECT p_docx,'docx' UNION ALL SELECT p_pdf,'pdf' LOOP
   output_id:=(item->>'artifact_id')::uuid;
   IF item->>'staging_id' IS NULL THEN
     -- A saved PDF already has a durable, identity-checked request owner.
     PERFORM kb_object_reference_add((item->>'object_ref')::kb_object_ref,(item->>'sha256')::kb_sha256,
       item->>'media_type',(item->>'byte_length')::bigint,'bid_submission_output',output_id,
       'output:'||typed.project_id||':'||typed.workspace_id||':'||p_manifest_id,p_actor);
   ELSE
   PERFORM kb_object_upload_commit((item->>'staging_id')::uuid,(item->>'object_ref')::kb_object_ref,
     (item->>'sha256')::kb_sha256,item->>'media_type',(item->>'byte_length')::bigint,
     'bid_submission_output',output_id,'output:'||typed.project_id||':'||typed.workspace_id||':'||p_manifest_id,p_actor);
   END IF;
   INSERT INTO bid_submission_output_artifacts(id,project_id,workspace_id,manifest_id,format,object_ref,content_sha256,
     media_type,byte_length,owner_id,owner_occurrence)
   VALUES(output_id,typed.project_id,typed.workspace_id,p_manifest_id,format_value,(item->>'object_ref')::kb_object_ref,
     (item->>'sha256')::kb_sha256,item->>'media_type',(item->>'byte_length')::bigint,output_id,
     'output:'||typed.project_id||':'||typed.workspace_id||':'||p_manifest_id);
 END LOOP;
 FOR image_entry IN SELECT key,value FROM jsonb_each(snapshot#>'{inventory,images}') LOOP
   image:=image_entry.value;
   PERFORM kb_object_reference_add((image->>'object_ref')::kb_object_ref,image_entry.key::kb_sha256,
     image->>'media_type',(image->>'byte_length')::bigint,'bid_submission_manifest',p_manifest_id,'evidence:image:'||image_entry.key,p_actor);
   PERFORM kb_object_reference_add((image->>'object_ref')::kb_object_ref,image_entry.key::kb_sha256,
     image->>'media_type',(image->>'byte_length')::bigint,'bid_submission_assessment_report',report_id,'evidence:image:'||image_entry.key,p_actor);
 END LOOP;
 INSERT INTO bid_submission_assessment_report_artifacts(id,project_id,workspace_id,manifest_id,docx_output_id,pdf_output_id,canonical_payload,content_sha256)
 VALUES(report_id,typed.project_id,typed.workspace_id,p_manifest_id,(p_docx->>'artifact_id')::uuid,(p_pdf->>'artifact_id')::uuid,report_payload,report_sha);
 result_value:=jsonb_build_object('artifact_id',p_manifest_id,'manifest_id',p_manifest_id,'manifest_sha256',manifest_sha,
   'source',typed.source,'outputs',jsonb_build_object('docx',docx_identity,'pdf',pdf_identity),'assessment_report_id',report_id,'assessment_report_sha256',report_sha);
 INSERT INTO bid_async_stage_receipts(request_artifact_id,stage_kind,frozen_input_sha256,result_identity,result_sha256)
 VALUES(p_request_artifact_id,'package',p_frozen_input_sha256,result_value,kb_bid_v2_sha256_bytes(kb_bid_v2_json_payload(result_value)));
 stamp:=kb_bid_v2_tender_agent_lock_owner(p_request_artifact_id,p_frozen_input_sha256,p_attempt,p_token);
 UPDATE bid_tender_agent_run_artifacts SET status='succeeded',progress_phase='succeeded',lease_expires_at=least(lease_expires_at,stamp),updated_at=stamp WHERE request_artifact_id=p_request_artifact_id AND attempt=p_attempt;
 UPDATE bid_async_request_snapshot_artifacts SET status='succeeded',result_identity=result_value,finished_at=clock_timestamp() WHERE id=p_request_artifact_id;
 RETURN result_value;
END $$;

CREATE FUNCTION kb_bid_v2_mark_submission_export_failed(p_request_artifact_id uuid,p_request_revision bigint,p_frozen_input_sha256 kb_sha256,p_error_code text)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
BEGIN
 IF p_error_code NOT IN ('ASSET_MISSING','ASSET_DIGEST_MISMATCH','RENDER_SCHEMA_INVALID','RENDERER_FAILED','OBJECT_COMMIT_FAILED','SUBMISSION_EXPORT_TIMEOUT') THEN
   RAISE EXCEPTION 'unknown SubmissionExport terminal error code' USING ERRCODE='22023';
 END IF;
 UPDATE bid_async_request_snapshot_artifacts SET status='failed',error_code=p_error_code,finished_at=clock_timestamp()
 WHERE id=p_request_artifact_id AND request_kind='submission_export' AND revision=p_request_revision
   AND frozen_input_sha256=p_frozen_input_sha256 AND status='pending';
END $$;

CREATE FUNCTION kb_bid_v2_list_submission_exports(p_workspace_id uuid,p_actor kb_actor_identity)
RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE project_value uuid;
BEGIN
 SELECT project_id INTO STRICT project_value FROM bid_submission_workspaces WHERE id=p_workspace_id;
 PERFORM kb_bid_v2_require_project_owner(project_value,p_actor);
 RETURN coalesce((SELECT jsonb_agg(request_value.result_identity||jsonb_build_object('export_id',m.id,'status','ready','created_at',m.created_at)
   ORDER BY m.created_at DESC,m.id DESC) FROM bid_submission_manifest_artifacts m
   JOIN bid_async_request_snapshot_artifacts request_value ON request_value.id=m.request_artifact_id AND request_value.status='succeeded'
   WHERE m.workspace_id=p_workspace_id),'[]'::jsonb);
END $$;

CREATE FUNCTION kb_bid_v2_get_submission_export(p_workspace_id uuid,p_output_id uuid,p_actor kb_actor_identity)
RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE project_value uuid; result_value jsonb;
BEGIN
 SELECT project_id INTO STRICT project_value FROM bid_submission_workspaces WHERE id=p_workspace_id;
 PERFORM kb_bid_v2_require_project_owner(project_value,p_actor);
 SELECT request_value.result_identity||jsonb_build_object('export_id',m.id,'status','ready','created_at',m.created_at)
 INTO result_value FROM bid_submission_manifest_artifacts m
 JOIN bid_async_request_snapshot_artifacts request_value ON request_value.id=m.request_artifact_id AND request_value.status='succeeded'
 WHERE m.workspace_id=p_workspace_id AND (m.id=p_output_id OR EXISTS(
   SELECT 1 FROM bid_submission_output_artifacts output WHERE output.manifest_id=m.id AND output.id=p_output_id));
 IF result_value IS NULL THEN RAISE EXCEPTION 'SUBMISSION_EXPORT_NOT_FOUND' USING ERRCODE='P0002'; END IF;
 RETURN result_value;
END $$;

CREATE FUNCTION kb_bid_v2_get_submission_assessment_report(p_workspace_id uuid,p_output_id uuid,p_actor kb_actor_identity)
RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE project_value uuid; result_value jsonb;
BEGIN
 SELECT project_id INTO STRICT project_value FROM bid_submission_workspaces WHERE id=p_workspace_id;
 PERFORM kb_bid_v2_require_project_owner(project_value,p_actor);
 SELECT convert_from(canonical_payload,'UTF8')::jsonb||jsonb_build_object('content_sha256',content_sha256)
 INTO result_value FROM bid_submission_assessment_report_artifacts
 WHERE workspace_id=p_workspace_id AND p_output_id IN (manifest_id,docx_output_id,pdf_output_id);
 IF result_value IS NULL THEN RAISE EXCEPTION 'ASSESSMENT_REPORT_NOT_FOUND' USING ERRCODE='P0002'; END IF;
 RETURN result_value;
END $$;

CREATE FUNCTION kb_bid_v2_get_submission_export_object(p_workspace_id uuid,p_output_id uuid,p_actor kb_actor_identity)
RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE project_value uuid; result_value jsonb;
BEGIN
 SELECT project_id INTO STRICT project_value FROM bid_submission_workspaces WHERE id=p_workspace_id;
 PERFORM kb_bid_v2_require_project_owner(project_value,p_actor);
 SELECT jsonb_build_object('object_ref',object_ref,'sha256',content_sha256,'media_type',media_type,
   'byte_length',byte_length,'file_name','投标稿.'||format) INTO result_value
 FROM bid_submission_output_artifacts WHERE workspace_id=p_workspace_id AND id=p_output_id;
 RETURN result_value;
END $$;

REVOKE ALL ON ALL TABLES IN SCHEMA public FROM PUBLIC;
REVOKE ALL ON ALL FUNCTIONS IN SCHEMA public FROM PUBLIC;
GRANT SELECT ON bid_attachment_preparation_contract_artifacts,
  bid_authoring_contract_artifacts,bid_render_style_contract_artifacts,
  bid_renderer_contract_artifacts
  TO kb_migrator,kb_runtime_api,kb_runtime_worker,kb_runtime_retention;
GRANT SELECT ON bidding_v2_projects,bidding_v2_workspace_heads,bidding_v2_async_requests,bidding_v2_outputs,
  bid_async_request_snapshot_artifacts
  TO kb_runtime_api,kb_runtime_worker;
GRANT EXECUTE ON FUNCTION kb_bid_v2_tender_agent_claim(uuid,bigint,kb_sha256),
  kb_bid_v2_tender_source_view_input(uuid,kb_sha256,integer,uuid,uuid),
  kb_bid_v2_tender_agent_heartbeat(uuid,kb_sha256,integer,uuid),
  kb_bid_v2_tender_agent_yield_for_retry(uuid,kb_sha256,integer,uuid,text,text),
  kb_bid_v2_load_tender_analysis_input(uuid,bigint,kb_sha256),
  kb_bid_v2_tender_agent_checkpoint_get(uuid,kb_sha256),
  kb_bid_v2_tender_agent_reserve(uuid,kb_sha256,integer,uuid,integer,text,bytea),
  kb_bid_v2_tender_agent_checkpoint_put(uuid,kb_sha256,integer,uuid,jsonb,jsonb),
  kb_bid_v2_tender_agent_fail(uuid,kb_sha256,integer,uuid,text,text),
  kb_bid_v2_publish_requirement_set_v4(uuid,bigint,kb_sha256,jsonb,kb_actor_identity,integer,uuid,uuid),
  kb_bid_v2_publish_requirement_set(uuid,kb_sha256),
  kb_bid_v2_load_tender_document_process_input(uuid,bigint,kb_sha256),
  kb_bid_v2_publish_tender_document_process(uuid,bigint,kb_sha256,uuid,uuid,kb_sha256,jsonb,jsonb,jsonb,kb_actor_identity),
  kb_bid_v2_compile_requirement_set(uuid,bigint,kb_sha256,kb_actor_identity),
  kb_bid_v2_mark_requirement_set_compile_failed(uuid,bigint,kb_sha256,text),
  kb_bid_v2_load_content_generation_input(uuid,bigint,kb_sha256),
  kb_bid_v2_load_user_pick_evidence(uuid,bigint,kb_sha256),
  kb_bid_v2_content_lock_owner(uuid,bigint,kb_sha256,integer,uuid),
  kb_bid_v2_content_match_lock(uuid,bigint,kb_sha256),
  kb_bid_v2_content_agent_input_get(uuid,kb_sha256),
  kb_bid_v2_content_agent_input_put(uuid,bigint,kb_sha256,integer,uuid,bytea,kb_sha256),
  kb_bid_v2_content_run_claim(uuid,bigint,kb_sha256),
  kb_bid_v2_content_run_heartbeat(uuid,bigint,kb_sha256,integer,uuid),
  kb_bid_v2_content_run_progress(uuid,bigint,kb_sha256,integer,uuid,text,jsonb),
  kb_bid_v2_content_run_yield_for_retry(uuid,bigint,kb_sha256,integer,uuid,text,text),
  kb_bid_v2_content_boundary_attempt_claim(uuid,bigint,kb_sha256,kb_sha256,uuid,kb_sha256,kb_sha256,text,kb_sha256,uuid,kb_sha256,uuid,kb_sha256,kb_sha256,integer,uuid),
  kb_bid_v2_publish_content_generation(uuid,bigint,kb_sha256,uuid,kb_sha256,jsonb,uuid,bytea,kb_sha256,jsonb,integer,uuid),
  kb_bid_v2_mark_content_generation_failed(uuid,bigint,kb_sha256,text,text,integer,uuid),
  kb_bid_v2_load_submission_export_input(uuid,bigint,kb_sha256),
  kb_bid_v2_submission_export_render_put(uuid,kb_sha256,integer,uuid,uuid,jsonb),
  kb_bid_v2_submission_export_snapshot_put(uuid,kb_sha256,integer,uuid,jsonb,jsonb),
  kb_bid_v2_publish_submission_export(uuid,bigint,kb_sha256,uuid,jsonb,jsonb,jsonb,kb_actor_identity,integer,uuid),
  kb_bid_v2_mark_submission_export_failed(uuid,bigint,kb_sha256,text),
  kb_bid_v2_mark_tender_document_failed(uuid,bigint,kb_sha256,text)
  TO kb_runtime_worker;
GRANT EXECUTE ON FUNCTION kb_bid_v2_get_docx_composition_basis(uuid,kb_actor_identity),
  kb_bid_v2_replay_docx_composition_submission(uuid,jsonb,kb_actor_identity,text),
  kb_bid_v2_submit_docx_composition_request(jsonb,jsonb,kb_actor_identity,text),
  kb_bid_v2_get_docx_composition_request(uuid,uuid,kb_actor_identity),
  kb_bid_v2_create_docx_composition_request(uuid,jsonb,jsonb,kb_actor_identity,text)
  TO kb_runtime_api;
GRANT EXECUTE ON FUNCTION kb_bid_v2_load_docx_composition_request(uuid,bigint,kb_sha256),
  kb_bid_v2_load_docx_composition_job(uuid,bigint,kb_sha256),
  kb_bid_v2_publish_docx_composition(uuid,kb_sha256,integer,uuid,kb_sha256,uuid,uuid),
  kb_bid_v2_replay_docx_composition(uuid,kb_sha256),
  kb_bid_v2_docx_composition_checkpoint_get(uuid,kb_sha256),
  kb_bid_v2_docx_composition_reserve(uuid,kb_sha256,integer,uuid,integer,boolean,bytea),
  kb_bid_v2_docx_composition_checkpoint_put(uuid,kb_sha256,integer,uuid,jsonb),
  kb_bid_v2_export_review_checkpoint_get(uuid,kb_sha256),
  kb_bid_v2_export_review_checkpoint_put(uuid,kb_sha256,integer,uuid,jsonb),
  kb_bid_v2_load_export_review_basis(uuid,kb_sha256),
  kb_bid_v2_export_review_reserve(uuid,kb_sha256,integer,uuid,integer,kb_sha256,bytea),
  kb_bid_v2_layout_checkpoint_get(uuid,kb_sha256),
  kb_bid_v2_layout_checkpoint_put(uuid,kb_sha256,integer,uuid,jsonb)
  TO kb_runtime_worker;
GRANT EXECUTE ON FUNCTION kb_bid_v2_load_docx_composition_source(uuid,jsonb,kb_actor_identity),
  kb_bid_v2_prepare_docx_composition_source(uuid,jsonb,jsonb,kb_actor_identity)
  TO kb_runtime_api,kb_runtime_worker;
GRANT EXECUTE ON FUNCTION kb_bid_v2_get_tender_analysis(uuid,uuid,kb_actor_identity,text,integer,integer),
  kb_bid_v2_get_tender_outline(uuid,kb_actor_identity),
  kb_bid_v2_create_docx_round(uuid,uuid,jsonb,kb_actor_identity,text),
  kb_bid_v2_get_docx_round_basis(uuid,kb_actor_identity),
  kb_bid_v2_get_docx_editor(uuid,uuid,kb_actor_identity),
  kb_bid_v2_get_docx_saved_receipt(uuid,uuid,uuid,kb_actor_identity),
  kb_bid_v2_replay_docx_editor_save(uuid,jsonb,kb_actor_identity,text),
  kb_bid_v2_docx_editor_command(uuid,text,jsonb,kb_actor_identity,text),
  kb_bid_v2_get_docx_composition_manifest(uuid,uuid,kb_actor_identity),
  kb_bid_v2_get_docx_version(uuid,uuid,kb_actor_identity),
  kb_bid_v2_get_current_docx(uuid,kb_actor_identity),
  kb_bid_v2_replay_docx_round(uuid,jsonb,kb_actor_identity,text),
  kb_bid_v2_create_project(uuid,text,uuid,kb_actor_identity,text,bytea,kb_sha256),
  kb_bid_v2_list_projects(uuid,kb_actor_identity),
  kb_bid_v2_get_project(uuid,kb_actor_identity),
  kb_bid_v2_next_quote_snapshot_revision(uuid,kb_actor_identity),
  kb_bid_v2_publish_quote_snapshot(uuid,uuid,bigint,uuid,kb_object_ref,kb_sha256,bigint,bytea,kb_actor_identity,text,bytea,kb_sha256),
  kb_bid_v2_list_quote_snapshots(uuid,kb_actor_identity),
  kb_bid_v2_get_quote_snapshot(uuid,uuid,kb_actor_identity),
  kb_bid_v2_apply_quote_snapshot(uuid,uuid,kb_sha256,uuid,kb_sha256,kb_actor_identity,text,bytea,kb_sha256),
  kb_bid_v2_end_project(uuid,kb_actor_identity,text,bytea,kb_sha256),
  kb_bid_v2_upload_tender_document(uuid,uuid,uuid,uuid,text,text,bigint,kb_object_ref,kb_sha256,kb_actor_identity,text,bytea,kb_sha256),
  kb_bid_v2_retry_tender_document(uuid,uuid,uuid,bigint,kb_actor_identity,text,bytea,kb_sha256),
  kb_bid_v2_list_tender_documents(uuid,kb_actor_identity),
  kb_bid_v2_patch_document_role(uuid,uuid,text,uuid,kb_sha256,kb_actor_identity,text,bytea,kb_sha256),
  kb_bid_v2_upsert_document_relation(uuid,uuid,uuid,uuid,text,jsonb,uuid,kb_sha256,kb_actor_identity,text,bytea,kb_sha256),
  kb_bid_v2_list_document_relations(uuid,kb_actor_identity),
  kb_bid_v2_list_document_sets(uuid,kb_actor_identity),
  kb_bid_v2_get_document_set(uuid,uuid,kb_actor_identity),
  kb_bid_v2_freeze_document_set(uuid,uuid[],uuid,kb_sha256,uuid,kb_actor_identity,text,bytea,kb_sha256,jsonb),
  kb_bid_v2_publish_disposition_set(uuid,uuid,jsonb,uuid,kb_sha256,uuid,kb_actor_identity,text,bytea,kb_sha256,jsonb),
  kb_bid_v2_list_source_units(uuid,kb_actor_identity),
  kb_bid_v2_list_structured_forms(uuid,kb_actor_identity),
  kb_bid_v2_list_requirements(uuid,kb_actor_identity),
  kb_bid_v2_get_requirement_set_compile_request(uuid,uuid,kb_actor_identity),
  kb_bid_v2_patch_requirement(uuid,uuid,uuid,kb_sha256,text,text,text,text,text,jsonb,jsonb,kb_actor_identity,text,bytea,kb_sha256),
  kb_bid_v2_publish_requirement_supersession(uuid,uuid,uuid,uuid,jsonb,boolean,uuid,kb_sha256,kb_actor_identity,text,bytea,kb_sha256),
  kb_bid_v2_load_workspace_for_actor(uuid,kb_actor_identity),
  kb_bid_v2_get_requirement_projection(uuid,kb_actor_identity),
  kb_bid_v2_refresh_requirement_projection(uuid,uuid,kb_sha256,uuid,kb_sha256,kb_actor_identity,text,bytea,kb_sha256),
  kb_bid_v2_list_workspace_assets(uuid,kb_actor_identity),
  kb_bid_v2_upload_workspace_asset(uuid,uuid,uuid,text,text,bigint,integer,integer,integer,kb_object_ref,kb_sha256,kb_actor_identity,text,bytea,kb_sha256),
  kb_bid_v2_prepare_workspace_attachment(uuid,uuid,uuid,uuid[],uuid[],integer[],integer[],kb_actor_identity,text,bytea,kb_sha256),
  kb_bid_v2_retire_workspace_asset(uuid,uuid,text,kb_actor_identity,text,bytea,kb_sha256),
  kb_bid_v2_create_outline_checkpoint(uuid,uuid,kb_sha256,uuid,kb_actor_identity,text,bytea,kb_sha256),
  kb_bid_v2_commit_workspace_mutation_idempotent(uuid,uuid,kb_sha256,jsonb,kb_actor_identity,text,bytea,kb_sha256),
  kb_bid_v2_idempotency_replay(kb_actor_identity,text,text,bytea,kb_sha256),
  kb_bid_v2_create_content_request(uuid,uuid,kb_sha256,text,text,uuid,text,jsonb,text,uuid,jsonb,bytea,jsonb,bytea,bytea,kb_actor_identity,text,bytea,kb_sha256),
  kb_bid_v2_create_evidence_pick_set(uuid,uuid,uuid[],kb_actor_identity,text,bytea,kb_sha256),
  kb_bid_v2_create_node_evidence_pick_set(uuid,uuid,uuid,uuid[],kb_actor_identity,text,bytea,kb_sha256),
  kb_bid_v2_list_evidence_pick_sets(uuid,kb_actor_identity),
  kb_bid_v2_get_node_evidence(uuid,uuid,kb_actor_identity),
  kb_bid_v2_get_evidence_overview(uuid,kb_actor_identity),
  kb_bid_v2_get_current_assessments(uuid,kb_actor_identity),
  kb_bid_v2_load_preview_input(uuid,kb_actor_identity),
  kb_bid_v2_get_preview_html(uuid,kb_actor_identity),
  kb_bid_v2_create_submission_export_request(uuid,uuid,kb_sha256,kb_actor_identity,text,bytea,kb_sha256,jsonb),
  kb_bid_v2_load_submission_export_source(uuid,uuid,uuid,kb_sha256),
  kb_bid_v2_list_submission_exports(uuid,kb_actor_identity),
  kb_bid_v2_get_submission_export(uuid,uuid,kb_actor_identity),
  kb_bid_v2_get_submission_assessment_report(uuid,uuid,kb_actor_identity),
  kb_bid_v2_get_submission_export_object(uuid,uuid,kb_actor_identity),
  kb_bid_v2_get_async_request(uuid,uuid,kb_actor_identity),
  kb_bid_v2_list_workspace_async_requests(uuid,kb_actor_identity),
  kb_bid_v2_get_candidate(uuid,uuid,kb_actor_identity),
  kb_bid_v2_accept_candidate(uuid,uuid,uuid,kb_sha256,jsonb,integer[],kb_actor_identity,text,bytea,kb_sha256),
  kb_bid_v2_reject_candidate(uuid,uuid,kb_actor_identity,text,bytea,kb_sha256),
  kb_bid_v2_load_authoring_job_payload(uuid,bigint,kb_sha256)
  TO kb_runtime_api;
