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
    child_ids:=kb_bid_v2_fulfillment_need_ids(child); IF child_ids IS NULL THEN RETURN NULL; END IF;
    ids:=ids||child_ids;
  END LOOP;
  RETURN ids;
END $$;

CREATE FUNCTION kb_bid_v2_fulfillment_expr_valid(value jsonb)
RETURNS boolean LANGUAGE sql IMMUTABLE PARALLEL SAFE SET search_path=pg_catalog,public AS $$
  SELECT ids IS NOT NULL AND cardinality(ids)>0
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
  SELECT COALESCE(value ~* '^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$',false)
$$;

CREATE FUNCTION kb_bid_v2_sha256_text(value text)
RETURNS boolean LANGUAGE sql IMMUTABLE PARALLEL SAFE
SET search_path=pg_catalog,public AS $$ SELECT COALESCE(value ~ '^[0-9a-f]{64}$',false) $$;

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
  source_locator jsonb NOT NULL CHECK (jsonb_typeof(source_locator)='object'),
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
  ocr_media_type text NOT NULL CHECK (ocr_media_type='text/plain;charset=utf-8'),
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
  schema_version smallint NOT NULL CHECK (schema_version=1),
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
  requirement_kind text NOT NULL CHECK (requirement_kind IN ('qualification','technical','commercial','pricing','delivery','evaluation','format','attachment','other')),
  requiredness text NOT NULL CHECK (requiredness IN ('mandatory','optional','informational')),
  compliance_policy text NOT NULL CHECK (compliance_policy IN ('must_comply','explicit_response','deviation_allowed','scored')),
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
RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog,public AS $$
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
  dependency_sha256 kb_sha256,
  stale boolean NOT NULL DEFAULT false,
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
  target_kind text NOT NULL CHECK (target_kind IN ('outline_node','response_table','structured_form','quote')),
  target_id uuid NOT NULL,
  state text NOT NULL CHECK (state IN ('bound','unbound','superseded')),
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
  request_kind text NOT NULL CHECK (request_kind IN ('tender_document_process','requirement_set_compile','outline_generate','content_generate','submission_export')),
  revision bigint NOT NULL CHECK (revision > 0),
  frozen_input_sha256 kb_sha256 NOT NULL,
  request_payload bytea NOT NULL,
  request_sha256 kb_sha256 NOT NULL,
  status text NOT NULL CHECK (status IN ('pending','succeeded','failed','obsolete')),
  result_identity jsonb,
  error_code text CHECK (error_code IS NULL OR error_code IN (
    'INPUT_SCHEMA_INVALID','FROZEN_INPUT_MISSING','FROZEN_INPUT_DIGEST_MISMATCH','REQUEST_OBSOLETE',
    'WORKSPACE_CAS_CONFLICT','AGENT_OUTPUT_INVALID','REQUIREMENT_COMPILE_FAILED','EVIDENCE_UNAVAILABLE','ASSET_MISSING',
    'ASSET_DIGEST_MISMATCH','ATTACHMENT_PREPARATION_FAILED','RENDER_SCHEMA_INVALID','RENDERER_FAILED','OBJECT_COMMIT_FAILED'
  )),
  created_at timestamptz NOT NULL DEFAULT now(),
  finished_at timestamptz,
  UNIQUE(request_kind,id,revision),
  UNIQUE(id,project_id,workspace_id,request_kind,revision,request_sha256),
  UNIQUE(id,project_id,workspace_id,request_kind,revision,request_sha256,frozen_input_sha256),
  CHECK (request_sha256=kb_bid_v2_sha256_bytes(request_payload)),
  CHECK ((status='pending' AND finished_at IS NULL AND error_code IS NULL)
      OR (status IN ('succeeded','obsolete') AND finished_at IS NOT NULL AND error_code IS NULL)
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
  candidate_kind text NOT NULL CHECK (candidate_kind IN ('outline','content')),
  base_workspace_revision_id uuid NOT NULL,
  base_workspace_sha256 kb_sha256 NOT NULL,
  request_artifact_id uuid NOT NULL,
  request_kind text NOT NULL CHECK (request_kind IN ('outline_generate','content_generate')),
  request_revision bigint NOT NULL CHECK (request_revision > 0),
  request_sha256 kb_sha256 NOT NULL,
  request_operation text NOT NULL CHECK (request_operation IN ('outline_generate','generate')),
  state text NOT NULL CHECK (state IN ('proposed','accepted','rejected','obsolete')),
  canonical_payload bytea NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  decided_at timestamptz,
  UNIQUE(project_id,id),
  FOREIGN KEY(project_id,workspace_id) REFERENCES bid_submission_workspaces(project_id,id),
  FOREIGN KEY(request_artifact_id,project_id,workspace_id,request_kind,request_revision,request_sha256)
    REFERENCES bid_async_request_snapshot_artifacts(id,project_id,workspace_id,request_kind,revision,request_sha256),
  FOREIGN KEY(project_id,workspace_id,base_workspace_revision_id,base_workspace_sha256)
    REFERENCES bid_workspace_revision_artifacts(project_id,workspace_id,id,content_sha256),
  CHECK (CASE
      WHEN candidate_kind='outline' THEN request_kind='outline_generate' AND request_operation='outline_generate'
      WHEN candidate_kind='content' THEN request_kind='content_generate' AND request_operation='generate'
      ELSE false END),
  CHECK (content_sha256=kb_bid_v2_sha256_bytes(canonical_payload)),
  CHECK ((state='proposed')=(decided_at IS NULL))
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
  resulting_workspace_revision_id uuid,
  response_payload bytea NOT NULL,
  response_sha256 kb_sha256 NOT NULL,
  decided_at timestamptz NOT NULL DEFAULT now(),
  CHECK(response_sha256=kb_bid_v2_sha256_bytes(response_payload))
);

-- V2 artifacts bind the shared ObjectRegistry identity including availability;
-- no bidding-local object registry is introduced.
ALTER TABLE object_registry
  ADD UNIQUE(object_ref,digest,state),
  ADD UNIQUE(object_ref,digest,media_type,state),
  ADD UNIQUE(object_ref,digest,media_type,byte_length,state);
ALTER TABLE bid_tender_source_image_revision_artifacts
  ADD FOREIGN KEY(original_object_ref,original_sha256,original_media_type,original_object_state)
    REFERENCES object_registry(object_ref,digest,media_type,state),
  ADD FOREIGN KEY(ocr_object_ref,ocr_sha256,ocr_media_type,ocr_byte_length,ocr_object_state)
    REFERENCES object_registry(object_ref,digest,media_type,byte_length,state);
ALTER TABLE knowledge_image_artifact_revisions
  ADD FOREIGN KEY(object_ref,content_sha256,media_type,object_state)
    REFERENCES object_registry(object_ref,digest,media_type,state),
  ADD UNIQUE NULLS NOT DISTINCT(id,object_ref,content_sha256,media_type,object_state,width,height,page_ordinal,bounding_region);
ALTER TABLE knowledge_matching_scope_attestations_v2
  ADD UNIQUE(id,content_sha256);

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
RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog,public AS $$
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
RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog,public AS $$
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
RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog,public AS $$
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
  source text NOT NULL CHECK (source IN ('human_upload','ai_evidence')),
  created_by kb_actor_identity NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(project_id,id),
  UNIQUE(project_id,workspace_id,id),
  UNIQUE(id,workspace_id,object_ref,content_sha256,media_type,object_state),
  FOREIGN KEY(project_id,workspace_id) REFERENCES bid_submission_workspaces(project_id,id),
  FOREIGN KEY(object_ref,content_sha256,media_type,object_state)
    REFERENCES object_registry(object_ref,digest,media_type,state),
  CHECK (object_ref='objects/'||content_sha256)
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
  state text NOT NULL CHECK(state IN ('current','stale','withdrawn')),
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
RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog,public AS $$
BEGIN
  IF NEW.target_kind='outline_node' THEN
    PERFORM 1 FROM bid_outline_node_lineages
      WHERE project_id=NEW.project_id AND workspace_id=NEW.workspace_id AND id=NEW.target_id;
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
  quote_snapshot_id uuid,
  quote_snapshot_sha256 kb_sha256,
  prompt_contract_id uuid NOT NULL,
  prompt_contract_sha256 kb_sha256 NOT NULL,
  template_contract_id uuid NOT NULL,
  template_contract_sha256 kb_sha256 NOT NULL,
  model_contract_id uuid NOT NULL,
  model_contract_sha256 kb_sha256 NOT NULL,
  agent_contract_id uuid NOT NULL,
  agent_contract_sha256 kb_sha256 NOT NULL,
  target_kind text NOT NULL CHECK (target_kind IN ('node','subtree','workspace')),
  target_node_lineage_id uuid,
  target_node_revision_id uuid,
  target_workspace_revision_id uuid,
  fill_policy text NOT NULL CHECK (fill_policy IN ('empty_only','append_candidate','missing_requirements_only')),
  insertion_node_revision_id uuid,
  insertion_block_revision_id uuid,
  insertion_utf8_offset bigint CHECK (insertion_utf8_offset IS NULL OR insertion_utf8_offset >= 0),
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(request_artifact_id,project_id,workspace_id),
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
  CHECK (CASE
      WHEN target_kind IN ('node','subtree') THEN target_node_lineage_id IS NOT NULL AND target_node_revision_id IS NOT NULL AND target_workspace_revision_id IS NULL
      WHEN target_kind='workspace' THEN target_node_lineage_id IS NULL AND target_node_revision_id IS NULL AND target_workspace_revision_id IS NOT NULL AND target_workspace_revision_id=base_workspace_revision_id
      ELSE false END),
  CHECK (CASE
      WHEN insertion_node_revision_id IS NULL THEN insertion_block_revision_id IS NULL AND insertion_utf8_offset IS NULL
      WHEN fill_policy='append_candidate' THEN insertion_block_revision_id IS NOT NULL OR insertion_utf8_offset IS NULL
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
RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog,public AS $$
BEGIN
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
    RAISE EXCEPTION 'ContentGenerate insertion anchor is not in the frozen base workspace revision' USING ERRCODE='23503';
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
RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog,public AS $$
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
RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog,public AS $$
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

CREATE TABLE bid_pdf_attachment_preparation_attestations (
  preparation_revision_id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  request_artifact_id uuid NOT NULL,
  request_revision bigint NOT NULL CHECK(request_revision>0),
  frozen_input_sha256 kb_sha256 NOT NULL,
  source_asset_revision_id uuid NOT NULL,
  source_object_ref kb_object_ref NOT NULL,
  source_sha256 kb_sha256 NOT NULL,
  source_media_type text NOT NULL CHECK(source_media_type='application/pdf'),
  source_object_state text NOT NULL DEFAULT 'available' CHECK(source_object_state='available'),
  preparation_sha256 kb_sha256 NOT NULL,
  contract_id uuid NOT NULL,
  contract_sha256 kb_sha256 NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(request_artifact_id,source_asset_revision_id),
  UNIQUE(project_id,workspace_id,preparation_revision_id,preparation_sha256),
  FOREIGN KEY(request_artifact_id) REFERENCES bid_async_request_snapshot_artifacts(id),
  FOREIGN KEY(project_id,workspace_id,preparation_revision_id,preparation_sha256)
    REFERENCES bid_attachment_preparation_revision_artifacts(project_id,workspace_id,id,preparation_sha256),
  FOREIGN KEY(source_asset_revision_id,workspace_id,source_object_ref,source_sha256,source_media_type,source_object_state)
    REFERENCES bid_workspace_asset_artifacts(id,workspace_id,object_ref,content_sha256,media_type,object_state),
  FOREIGN KEY(contract_id,contract_sha256)
    REFERENCES bid_attachment_preparation_contract_artifacts(id,content_sha256),
  CHECK(source_object_ref='objects/'||source_sha256)
);

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
    mode_options ?& ARRAY['watermark','include_assessment_notices','include_knowledge_sources']
    AND mode_options - ARRAY['watermark','include_assessment_notices','include_knowledge_sources']::text[] = '{}'::jsonb
    AND jsonb_typeof(mode_options->'watermark') IN ('null','string')
    AND jsonb_typeof(mode_options->'include_assessment_notices')='boolean'
    AND jsonb_typeof(mode_options->'include_knowledge_sources')='boolean'
  ),
  CHECK (output_mode='review_draft' OR mode_options @> '{"watermark":null}'::jsonb),
  CHECK (output_mode<>'submission' OR mode_options @> '{"watermark":null,"include_assessment_notices":false,"include_knowledge_sources":false}'::jsonb),
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
RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog,public AS $$
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
RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog,public AS $$
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
 OR NOT kb_bid_v2_json_keys_exact(p->'mode_options',ARRAY['watermark','include_assessment_notices','include_knowledge_sources'])
 OR COALESCE(jsonb_typeof(p->'mode_options'->'watermark'),'missing') NOT IN ('string','null')
 OR (jsonb_typeof(p->'mode_options'->'watermark')='string' AND octet_length(p->'mode_options'->>'watermark') NOT BETWEEN 1 AND 128)
 OR jsonb_typeof(p->'mode_options'->'include_assessment_notices') IS DISTINCT FROM 'boolean'
 OR jsonb_typeof(p->'mode_options'->'include_knowledge_sources') IS DISTINCT FROM 'boolean'
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
RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog,public AS $$
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
RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog,public AS $$
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
  render_snapshot_id uuid NOT NULL,
  output_mode text NOT NULL CHECK (output_mode IN ('review_draft','submission')),
  format text NOT NULL CHECK (format IN ('docx','pdf')),
  mode_options jsonb NOT NULL CHECK (jsonb_typeof(mode_options)='object'),
  canonical_payload bytea NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(project_id,id),
  UNIQUE(project_id,workspace_id,id,format),
  FOREIGN KEY(project_id,workspace_id) REFERENCES bid_submission_workspaces(project_id,id),
  FOREIGN KEY(project_id,workspace_id,render_snapshot_id,output_mode,format,mode_options)
    REFERENCES bid_render_document_snapshot_artifacts(project_id,workspace_id,id,output_mode,format,mode_options),
  CHECK (content_sha256=kb_bid_v2_sha256_bytes(canonical_payload))
);

CREATE TABLE bid_submission_manifest_dependencies (
  manifest_id uuid NOT NULL REFERENCES bid_submission_manifest_artifacts(id),
  dependency_kind text NOT NULL CHECK (dependency_kind IN ('document_set','requirement_projection','workspace','outline_checkpoint','document_settings','assessment','render_snapshot','asset','quote_snapshot','style','renderer','font','form_definition','attachment_preparation','scope')),
  dependency_id uuid NOT NULL,
  dependency_sha256 kb_sha256 NOT NULL,
  ordinal integer NOT NULL CHECK (ordinal >= 0),
  PRIMARY KEY(manifest_id,dependency_kind,dependency_id),
  UNIQUE(manifest_id,ordinal)
);

CREATE FUNCTION kb_bid_v2_manifest_expected_dependencies(p_manifest_id uuid)
RETURNS TABLE(dependency_kind text,dependency_id uuid,dependency_sha256 kb_sha256)
LANGUAGE sql STABLE SET search_path=pg_catalog,public AS $$
 WITH manifest AS (
  SELECT m.*,r.workspace_revision_id,r.workspace_sha256,r.scope_revision_id,r.outline_checkpoint_id,
   r.outline_checkpoint_sha256,r.requirement_projection_id,r.requirement_projection_sha256,r.document_settings_revision_id,r.document_settings_sha256,
   r.submission_assessment_snapshot_id,r.submission_assessment_snapshot_sha256,r.style_contract_id,r.style_contract_sha256,
   r.docx_renderer_contract_id,r.docx_renderer_contract_sha256,r.pdf_renderer_contract_id,r.pdf_renderer_contract_sha256,r.content_sha256 render_sha
  FROM bid_submission_manifest_artifacts m JOIN bid_render_document_snapshot_artifacts r ON r.id=m.render_snapshot_id WHERE m.id=p_manifest_id
 ), base(dependency_kind,dependency_id,dependency_sha256) AS (
  SELECT 'render_snapshot'::text,render_snapshot_id,render_sha FROM manifest
  UNION ALL SELECT 'workspace',workspace_revision_id,workspace_sha256 FROM manifest
  UNION ALL SELECT 'outline_checkpoint',outline_checkpoint_id,outline_checkpoint_sha256 FROM manifest
  UNION ALL SELECT 'assessment',submission_assessment_snapshot_id,submission_assessment_snapshot_sha256 FROM manifest
  UNION ALL SELECT 'document_settings',document_settings_revision_id,document_settings_sha256 FROM manifest
  UNION ALL SELECT 'requirement_projection',requirement_projection_id,requirement_projection_sha256 FROM manifest
  UNION ALL SELECT 'scope',scope_revision_id,s.content_sha256 FROM manifest JOIN bid_workspace_scope_revision_artifacts s ON s.id=scope_revision_id
  UNION ALL SELECT 'style',style_contract_id,style_contract_sha256 FROM manifest
  UNION ALL SELECT 'renderer',docx_renderer_contract_id,docx_renderer_contract_sha256 FROM manifest
  UNION ALL SELECT 'renderer',pdf_renderer_contract_id,pdf_renderer_contract_sha256 FROM manifest
  UNION ALL SELECT 'document_set',rs.document_set_id,ds.content_sha256 FROM manifest
   JOIN bid_workspace_requirement_projection_artifacts p ON p.id=requirement_projection_id
   JOIN bid_requirement_set_artifacts rs ON rs.id=p.requirement_set_id JOIN bid_document_set_artifacts ds ON ds.id=rs.document_set_id
 ), children(dependency_kind,dependency_id,dependency_sha256) AS (
  SELECT 'asset'::text,a.asset_revision_id,a.content_sha256 FROM manifest JOIN bid_render_snapshot_asset_items a ON a.render_snapshot_id=manifest.render_snapshot_id
  UNION ALL SELECT 'font',f.font_artifact_id,f.content_sha256 FROM manifest JOIN bid_render_snapshot_font_items f ON f.render_snapshot_id=manifest.render_snapshot_id
  UNION ALL SELECT 'form_definition',f.form_definition_revision_id,f.canonical_sha256 FROM manifest JOIN bid_render_snapshot_form_definition_items f ON f.render_snapshot_id=manifest.render_snapshot_id
  UNION ALL SELECT 'attachment_preparation',p.attachment_preparation_revision_id,p.canonical_sha256 FROM manifest JOIN bid_render_snapshot_attachment_preparation_items p ON p.render_snapshot_id=manifest.render_snapshot_id
  UNION ALL SELECT 'quote_snapshot',a.quote_snapshot_id,a.quote_snapshot_sha256 FROM manifest JOIN bid_submission_assessment_snapshot_artifacts a ON a.id=manifest.submission_assessment_snapshot_id WHERE a.quote_snapshot_id IS NOT NULL
 ) SELECT DISTINCT base.dependency_kind,base.dependency_id,base.dependency_sha256 FROM base
 UNION SELECT DISTINCT children.dependency_kind,children.dependency_id,children.dependency_sha256 FROM children
$$;

CREATE FUNCTION kb_bid_v2_validate_manifest_dependency()
RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog,public AS $$
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
RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog,public AS $$
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
  FOREIGN KEY(project_id,workspace_id) REFERENCES bid_submission_workspaces(project_id,id),
  FOREIGN KEY(project_id,workspace_id,manifest_id,format)
    REFERENCES bid_submission_manifest_artifacts(project_id,workspace_id,id,format),
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
  submission_output_id uuid NOT NULL UNIQUE,
  manifest_id uuid NOT NULL,
  submission_assessment_snapshot_id uuid NOT NULL,
  canonical_payload bytea NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(project_id,id),
  FOREIGN KEY(project_id,workspace_id,submission_output_id) REFERENCES bid_submission_output_artifacts(project_id,workspace_id,id),
  FOREIGN KEY(project_id,manifest_id) REFERENCES bid_submission_manifest_artifacts(project_id,id),
  FOREIGN KEY(project_id,submission_assessment_snapshot_id) REFERENCES bid_submission_assessment_snapshot_artifacts(project_id,id),
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
    WHEN request_kind IN ('outline_generate','content_generate','submission_export') THEN workspace_id IS NOT NULL
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
RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog,public AS $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM bid_authoring_contract_artifacts WHERE id=NEW.prompt_contract_id AND content_sha256=NEW.prompt_contract_sha256 AND contract_kind='prompt')
     OR NOT EXISTS (SELECT 1 FROM bid_authoring_contract_artifacts WHERE id=NEW.template_contract_id AND content_sha256=NEW.template_contract_sha256 AND contract_kind='template')
     OR NOT EXISTS (SELECT 1 FROM bid_authoring_contract_artifacts WHERE id=NEW.model_contract_id AND content_sha256=NEW.model_contract_sha256 AND contract_kind='model')
     OR NOT EXISTS (SELECT 1 FROM bid_authoring_contract_artifacts WHERE id=NEW.agent_contract_id AND content_sha256=NEW.agent_contract_sha256 AND contract_kind='agent')
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
  frozen_input_sha256 kb_sha256 NOT NULL,
  document_set_revision_id uuid NOT NULL,
  document_set_sha256 kb_sha256 NOT NULL,
  disposition_set_revision_id uuid NOT NULL,
  disposition_set_sha256 kb_sha256 NOT NULL,
  UNIQUE(request_artifact_id,request_kind,project_id,request_revision,request_sha256),
  FOREIGN KEY(request_artifact_id,project_id,request_kind,request_revision,request_sha256,frozen_input_sha256)
    REFERENCES bid_async_request_snapshot_artifacts(id,project_id,request_kind,revision,request_sha256,frozen_input_sha256),
  FOREIGN KEY(project_id,document_set_revision_id,document_set_sha256)
    REFERENCES bid_document_set_artifacts(project_id,id,content_sha256),
  FOREIGN KEY(project_id,disposition_set_revision_id,disposition_set_sha256)
    REFERENCES bid_source_unit_disposition_set_artifacts(project_id,id,content_sha256),
  FOREIGN KEY(project_id,disposition_set_revision_id,document_set_revision_id)
    REFERENCES bid_source_unit_disposition_set_artifacts(project_id,id,document_set_id)
);

CREATE TABLE bid_outline_generation_request_identities (
  request_artifact_id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  request_kind text NOT NULL DEFAULT 'outline_generate' CHECK (request_kind='outline_generate'),
  request_revision bigint NOT NULL CHECK (request_revision>0),
  request_sha256 kb_sha256 NOT NULL,
  frozen_input_sha256 kb_sha256 NOT NULL,
  base_workspace_revision_id uuid NOT NULL,
  base_workspace_sha256 kb_sha256 NOT NULL,
  document_set_revision_id uuid NOT NULL,
  document_set_sha256 kb_sha256 NOT NULL,
  disposition_set_revision_id uuid NOT NULL,
  disposition_set_sha256 kb_sha256 NOT NULL,
  requirement_set_revision_id uuid NOT NULL,
  requirement_set_sha256 kb_sha256 NOT NULL,
  requirement_projection_id uuid NOT NULL,
  requirement_projection_sha256 kb_sha256 NOT NULL,
  scope_revision_id uuid NOT NULL,
  scope_revision_sha256 kb_sha256 NOT NULL,
  prompt_contract_id uuid NOT NULL,
  prompt_contract_sha256 kb_sha256 NOT NULL,
  template_contract_id uuid NOT NULL,
  template_contract_sha256 kb_sha256 NOT NULL,
  model_contract_id uuid NOT NULL,
  model_contract_sha256 kb_sha256 NOT NULL,
  agent_contract_id uuid NOT NULL,
  agent_contract_sha256 kb_sha256 NOT NULL,
  structured_form_identities jsonb NOT NULL DEFAULT '[]'::jsonb CHECK (jsonb_typeof(structured_form_identities)='array'),
  UNIQUE(request_artifact_id,request_kind,project_id,workspace_id,request_revision,request_sha256,base_workspace_revision_id,base_workspace_sha256),
  FOREIGN KEY(request_artifact_id,project_id,workspace_id,request_kind,request_revision,request_sha256,frozen_input_sha256)
    REFERENCES bid_async_request_snapshot_artifacts(id,project_id,workspace_id,request_kind,revision,request_sha256,frozen_input_sha256),
  FOREIGN KEY(project_id,workspace_id,base_workspace_revision_id,base_workspace_sha256)
    REFERENCES bid_workspace_revision_artifacts(project_id,workspace_id,id,content_sha256),
  FOREIGN KEY(project_id,workspace_id,base_workspace_revision_id,scope_revision_id,requirement_projection_id)
    REFERENCES bid_workspace_revision_artifacts(project_id,workspace_id,id,scope_revision_id,requirement_projection_id),
  FOREIGN KEY(project_id,workspace_id,requirement_projection_id,requirement_projection_sha256,requirement_set_revision_id)
    REFERENCES bid_workspace_requirement_projection_artifacts(project_id,workspace_id,id,content_sha256,requirement_set_id),
  FOREIGN KEY(project_id,requirement_set_revision_id,requirement_set_sha256,document_set_revision_id,disposition_set_revision_id)
    REFERENCES bid_requirement_set_artifacts(project_id,id,content_sha256,document_set_id,disposition_set_id),
  FOREIGN KEY(project_id,document_set_revision_id,document_set_sha256)
    REFERENCES bid_document_set_artifacts(project_id,id,content_sha256),
  FOREIGN KEY(project_id,disposition_set_revision_id,disposition_set_sha256)
    REFERENCES bid_source_unit_disposition_set_artifacts(project_id,id,content_sha256),
  FOREIGN KEY(project_id,workspace_id,scope_revision_id,scope_revision_sha256)
    REFERENCES bid_workspace_scope_revision_artifacts(project_id,workspace_id,id,content_sha256),
  FOREIGN KEY(prompt_contract_id,prompt_contract_sha256) REFERENCES bid_authoring_contract_artifacts(id,content_sha256),
  FOREIGN KEY(template_contract_id,template_contract_sha256) REFERENCES bid_authoring_contract_artifacts(id,content_sha256),
  FOREIGN KEY(model_contract_id,model_contract_sha256) REFERENCES bid_authoring_contract_artifacts(id,content_sha256),
  FOREIGN KEY(agent_contract_id,agent_contract_sha256) REFERENCES bid_authoring_contract_artifacts(id,content_sha256)
);

CREATE TABLE bid_submission_export_request_identities (
  request_artifact_id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  request_kind text NOT NULL DEFAULT 'submission_export' CHECK (request_kind='submission_export'),
  request_revision bigint NOT NULL CHECK (request_revision>0),
  request_sha256 kb_sha256 NOT NULL,
  frozen_input_sha256 kb_sha256 NOT NULL,
  workspace_revision_id uuid NOT NULL,
  workspace_sha256 kb_sha256 NOT NULL,
  outline_checkpoint_id uuid NOT NULL,
  outline_checkpoint_sha256 kb_sha256 NOT NULL,
  requirement_projection_id uuid NOT NULL,
  requirement_projection_sha256 kb_sha256 NOT NULL,
  scope_revision_id uuid NOT NULL,
  scope_revision_sha256 kb_sha256 NOT NULL,
  document_settings_revision_id uuid NOT NULL,
  document_settings_sha256 kb_sha256 NOT NULL,
  render_style_contract_id uuid NOT NULL,
  render_style_contract_sha256 kb_sha256 NOT NULL,
  output_mode text NOT NULL CHECK (output_mode IN ('review_draft','submission')),
  format text NOT NULL CHECK (format IN ('docx','pdf')),
  mode_options jsonb NOT NULL CHECK (jsonb_typeof(mode_options)='object'),
  UNIQUE(request_artifact_id,request_kind,project_id,workspace_id,request_revision,request_sha256),
  UNIQUE(request_artifact_id,project_id,workspace_id,request_revision,frozen_input_sha256),
  FOREIGN KEY(request_artifact_id,project_id,workspace_id,request_kind,request_revision,request_sha256,frozen_input_sha256)
    REFERENCES bid_async_request_snapshot_artifacts(id,project_id,workspace_id,request_kind,revision,request_sha256,frozen_input_sha256),
  FOREIGN KEY(project_id,workspace_id,workspace_revision_id,workspace_sha256)
    REFERENCES bid_workspace_revision_artifacts(project_id,workspace_id,id,content_sha256),
  FOREIGN KEY(project_id,workspace_id,workspace_revision_id,scope_revision_id,requirement_projection_id,document_settings_revision_id)
    REFERENCES bid_workspace_revision_artifacts(project_id,workspace_id,id,scope_revision_id,requirement_projection_id,document_settings_revision_id),
  FOREIGN KEY(project_id,workspace_id,outline_checkpoint_id,workspace_revision_id,requirement_projection_id,requirement_projection_sha256,outline_checkpoint_sha256)
    REFERENCES bid_outline_checkpoint_artifacts(project_id,workspace_id,id,workspace_revision_id,requirement_projection_id,requirement_projection_sha256,content_sha256),
  FOREIGN KEY(project_id,workspace_id,requirement_projection_id,requirement_projection_sha256)
    REFERENCES bid_workspace_requirement_projection_artifacts(project_id,workspace_id,id,content_sha256),
  FOREIGN KEY(project_id,workspace_id,scope_revision_id,scope_revision_sha256)
    REFERENCES bid_workspace_scope_revision_artifacts(project_id,workspace_id,id,content_sha256),
  FOREIGN KEY(project_id,workspace_id,document_settings_revision_id,document_settings_sha256)
    REFERENCES bid_document_settings_revision_artifacts(project_id,workspace_id,id,content_sha256),
  FOREIGN KEY(render_style_contract_id,render_style_contract_sha256)
    REFERENCES bid_render_style_contract_artifacts(id,content_sha256),
  CHECK (
    mode_options ?& ARRAY['watermark','include_assessment_notices','include_knowledge_sources']
    AND mode_options - ARRAY['watermark','include_assessment_notices','include_knowledge_sources']::text[] = '{}'::jsonb
    AND COALESCE(jsonb_typeof(mode_options->'watermark'),'missing') IN ('null','string')
    AND (jsonb_typeof(mode_options->'watermark')<>'string' OR octet_length(mode_options->>'watermark') BETWEEN 1 AND 128)
    AND jsonb_typeof(mode_options->'include_assessment_notices') IS NOT DISTINCT FROM 'boolean'
    AND jsonb_typeof(mode_options->'include_knowledge_sources') IS NOT DISTINCT FROM 'boolean'
  ),
  CHECK (
    output_mode='review_draft'
    OR mode_options @> '{"watermark":null,"include_assessment_notices":false,"include_knowledge_sources":false}'::jsonb
  )
);

ALTER TABLE bid_pdf_attachment_preparation_attestations
  ADD CONSTRAINT bid_pdf_attachment_preparation_request_fk
  FOREIGN KEY(request_artifact_id,project_id,workspace_id,request_revision,frozen_input_sha256)
  REFERENCES bid_submission_export_request_identities(request_artifact_id,project_id,workspace_id,request_revision,frozen_input_sha256);

CREATE FUNCTION kb_bid_v2_validate_request_contract_kinds()
RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog,public AS $$
BEGIN
  IF TG_TABLE_NAME='bid_tender_document_process_request_identities' THEN
    IF NOT EXISTS (SELECT 1 FROM bid_authoring_contract_artifacts WHERE id=NEW.converter_contract_id AND content_sha256=NEW.converter_contract_sha256 AND contract_kind='converter') THEN
      RAISE EXCEPTION 'TenderDocumentProcess converter contract kind mismatch' USING ERRCODE='23514';
    END IF;
  ELSIF TG_TABLE_NAME='bid_outline_generation_request_identities' THEN
    IF NOT EXISTS (SELECT 1 FROM bid_authoring_contract_artifacts WHERE id=NEW.prompt_contract_id AND content_sha256=NEW.prompt_contract_sha256 AND contract_kind='prompt')
       OR NOT EXISTS (SELECT 1 FROM bid_authoring_contract_artifacts WHERE id=NEW.template_contract_id AND content_sha256=NEW.template_contract_sha256 AND contract_kind='template')
       OR NOT EXISTS (SELECT 1 FROM bid_authoring_contract_artifacts WHERE id=NEW.model_contract_id AND content_sha256=NEW.model_contract_sha256 AND contract_kind='model')
       OR NOT EXISTS (SELECT 1 FROM bid_authoring_contract_artifacts WHERE id=NEW.agent_contract_id AND content_sha256=NEW.agent_contract_sha256 AND contract_kind='agent') THEN
      RAISE EXCEPTION 'OutlineGenerate contract identity kind mismatch' USING ERRCODE='23514';
    END IF;
  END IF;
  RETURN NEW;
END $$;
CREATE TRIGGER bid_tender_document_process_contract_kind
BEFORE INSERT ON bid_tender_document_process_request_identities
FOR EACH ROW EXECUTE FUNCTION kb_bid_v2_validate_request_contract_kinds();
CREATE TRIGGER bid_outline_generation_contract_kinds
BEFORE INSERT ON bid_outline_generation_request_identities
FOR EACH ROW EXECUTE FUNCTION kb_bid_v2_validate_request_contract_kinds();

CREATE FUNCTION kb_bid_v2_verify_request_typed_projection()
RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog,public AS $$
DECLARE projection_count integer;
BEGIN
  SELECT
    (SELECT count(*) FROM bid_tender_document_process_request_identities WHERE request_artifact_id=NEW.id)+
    (SELECT count(*) FROM bid_requirement_set_compile_request_identities WHERE request_artifact_id=NEW.id)+
    (SELECT count(*) FROM bid_outline_generation_request_identities WHERE request_artifact_id=NEW.id)+
    (SELECT count(*) FROM bid_content_generation_request_identities WHERE request_artifact_id=NEW.id)+
    (SELECT count(*) FROM bid_submission_export_request_identities WHERE request_artifact_id=NEW.id)
  INTO projection_count;
  IF projection_count<>1
     OR (NEW.request_kind='tender_document_process' AND NOT EXISTS (SELECT 1 FROM bid_tender_document_process_request_identities WHERE request_artifact_id=NEW.id))
     OR (NEW.request_kind='requirement_set_compile' AND NOT EXISTS (SELECT 1 FROM bid_requirement_set_compile_request_identities WHERE request_artifact_id=NEW.id))
     OR (NEW.request_kind='outline_generate' AND NOT EXISTS (SELECT 1 FROM bid_outline_generation_request_identities WHERE request_artifact_id=NEW.id))
     OR (NEW.request_kind='content_generate' AND NOT EXISTS (SELECT 1 FROM bid_content_generation_request_identities WHERE request_artifact_id=NEW.id))
     OR (NEW.request_kind='submission_export' AND NOT EXISTS (SELECT 1 FROM bid_submission_export_request_identities WHERE request_artifact_id=NEW.id)) THEN
    RAISE EXCEPTION 'async request must have exactly one matching typed projection' USING ERRCODE='23514';
  END IF;
  RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER bid_async_request_typed_projection_complete
AFTER INSERT ON bid_async_request_snapshot_artifacts
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION kb_bid_v2_verify_request_typed_projection();

CREATE FUNCTION kb_bid_v2_guard_async_request_initial_state()
RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog,public AS $$
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
RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog,public AS $$
BEGIN
  IF TG_OP='DELETE' THEN
    RAISE EXCEPTION 'async request snapshots cannot be deleted' USING ERRCODE='42501';
  END IF;
  IF (to_jsonb(NEW)-ARRAY['status','result_identity','error_code','finished_at'])
       IS DISTINCT FROM (to_jsonb(OLD)-ARRAY['status','result_identity','error_code','finished_at']) THEN
    RAISE EXCEPTION 'async request frozen identity cannot change' USING ERRCODE='42501';
  END IF;
  IF OLD.status<>'pending' OR NEW.status NOT IN ('succeeded','failed','obsolete') THEN
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
RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog,public AS $$
BEGIN
  IF NEW.candidate_kind='outline' THEN
    PERFORM 1 FROM bid_outline_generation_request_identities request
     WHERE request.request_artifact_id=NEW.request_artifact_id
       AND request.project_id=NEW.project_id AND request.workspace_id=NEW.workspace_id
       AND request.request_revision=NEW.request_revision AND request.request_sha256=NEW.request_sha256
       AND request.base_workspace_revision_id=NEW.base_workspace_revision_id
       AND request.base_workspace_sha256=NEW.base_workspace_sha256;
  ELSE
    PERFORM 1 FROM bid_content_generation_request_identities request
     WHERE request.request_artifact_id=NEW.request_artifact_id
       AND request.request_kind='content_generate' AND request.request_operation='generate'
       AND request.project_id=NEW.project_id AND request.workspace_id=NEW.workspace_id
       AND request.request_revision=NEW.request_revision AND request.request_sha256=NEW.request_sha256
       AND request.base_workspace_revision_id=NEW.base_workspace_revision_id
       AND request.base_workspace_sha256=NEW.base_workspace_sha256;
  END IF;
  IF NOT FOUND THEN
    RAISE EXCEPTION 'candidate does not match its typed request/base identity' USING ERRCODE='23503';
  END IF;
  RETURN NEW;
END $$;
CREATE TRIGGER bid_candidate_typed_request_identity
BEFORE INSERT ON bid_candidate_artifacts
FOR EACH ROW EXECUTE FUNCTION kb_bid_v2_validate_candidate_request_identity();

CREATE FUNCTION kb_bid_v2_guard_candidate_initial_state()
RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog,public AS $$
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
RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog,public AS $$
BEGIN
  IF TG_OP='DELETE' THEN
    RAISE EXCEPTION 'candidates cannot be deleted' USING ERRCODE='42501';
  END IF;
  IF (to_jsonb(NEW)-ARRAY['state','decided_at']) IS DISTINCT FROM (to_jsonb(OLD)-ARRAY['state','decided_at']) THEN
    RAISE EXCEPTION 'candidate frozen identity cannot change' USING ERRCODE='42501';
  END IF;
  IF OLD.state<>'proposed' OR NEW.state NOT IN ('accepted','rejected','obsolete') THEN
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
    'bid_attachment_preparation_contract_artifacts','bid_pdf_attachment_preparation_attestations',
    'bid_render_document_snapshot_artifacts','bid_submission_manifest_artifacts','bid_submission_output_artifacts','bid_submission_assessment_report_artifacts',
    'bid_document_set_items','bid_source_unit_disposition_set_items','bid_requirement_set_items',
    'bid_workspace_requirement_projection_items','bid_workspace_node_occurrences','bid_workspace_block_occurrences',
    'bid_workspace_binding_occurrences','bid_outline_lineage_edges','bid_async_stage_receipts',
    'bid_tender_document_process_request_identities','bid_requirement_set_compile_request_identities',
    'bid_outline_generation_request_identities','bid_content_generation_request_identities',
    'bid_submission_export_request_identities','bid_content_generation_request_evidence_bundles',
    'bid_candidate_operations','bid_candidate_decision_receipts','bid_evidence_bundle_items',
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
      RETURN 'obsolete';
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
  form_id uuid; form_payload bytea; form_sha kb_sha256; form_fields jsonb;
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
       OR image_value->>'ocr_media_type'<>'text/plain;charset=utf-8'
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
    IF unit_value->>'unit_kind'='form_region' THEN
      form_id:=gen_random_uuid();
      SELECT coalesce(jsonb_agg(jsonb_build_object('field_id','field_'||line.ordinality,
        'label',coalesce(nullif(btrim(split_part(line.value,':',1)),''),'字段 '||line.ordinality),
        'field_type','text','required',true) ORDER BY line.ordinality),
        jsonb_build_array(jsonb_build_object('field_id','field_1','label','表单内容','field_type','text','required',true)))
      INTO form_fields FROM regexp_split_to_table(convert_from(decode(unit_value->>'text_utf8_hex','hex'),'UTF8'),E'\\r?\\n')
        WITH ORDINALITY line(value,ordinality) WHERE btrim(line.value)<>'';
      form_payload:=kb_bid_v2_json_payload(jsonb_build_object('schema_version',1,
        'form_definition_revision_id',form_id,'source_unit_revision_id',(unit_value->>'id')::uuid,
        'title','招标文件表单','fields',form_fields));
      form_sha:=kb_bid_v2_sha256_bytes(form_payload);
      INSERT INTO bid_tender_structured_form_definition_artifacts(id,project_id,source_unit_revision_id,
        schema_version,canonical_payload,content_sha256)
      VALUES(form_id,p_project_id,(unit_value->>'id')::uuid,1,form_payload,form_sha);
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
 SELECT id,project_id,workspace_id,request_kind,revision,frozen_input_sha256,status,result_identity,error_code,created_at,finished_at
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
      'dependency_sha256', block_rec.dependency_sha256,
      'stale', block_rec.stale,
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
  settings jsonb := '{"page_size":"A4","margins_mm":{"top":25.4,"right":25.4,"bottom":25.4,"left":25.4},"cjk_font":"Noto Sans CJK SC","latin_font":"Times New Roman","body_font_pt":12,"line_spacing":1.5,"heading_numbering":"decimal","header":"","footer":"","page_number":"footer_center"}'::jsonb;
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
  node jsonb; block jsonb; binding jsonb; edge jsonb; deleted record; evidence record;
  lineage uuid; rev uuid; parent uuid; parent_occ uuid; occ uuid; block_occ uuid;
  payload bytea; sha kb_sha256; settings_id uuid; settings_sha kb_sha256;
  new_rev uuid := gen_random_uuid(); new_sha kb_sha256; new_payload bytea;
  settings jsonb; ordinal int; depth int; printable_width numeric;
  evidence_target uuid; evidence_kind text; evidence_lineage uuid; evidence_revision bigint;
  evidence_state text; evidence_dependency kb_sha256;
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
    INSERT INTO bid_content_block_revision_artifacts(id,project_id,workspace_id,lineage_id,revision,schema_version,block_kind,block_payload,origin,dependency_sha256,stale,canonical_payload,content_sha256)
      VALUES(rev,w.project_id,p_workspace_id,lineage,
        (SELECT coalesce(max(revision),0)+1 FROM bid_content_block_revision_artifacts WHERE lineage_id=lineage),
        1, block->>'kind', coalesce(block->'content', '{}'::jsonb), coalesce(block->>'origin','human'),
        NULLIF(block->>'dependency_sha256','')::kb_sha256, coalesce((block->>'stale')::boolean,false),
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
      block_kind,block_payload,origin,dependency_sha256,stale,tombstone,canonical_payload,content_sha256)
    VALUES(gen_random_uuid(),w.project_id,p_workspace_id,deleted.lineage_id,deleted.revision+1,1,deleted.block_kind,
      '{}'::jsonb,'human',deleted.dependency_sha256,true,true,payload,sha);
  END LOOP;
  FOR binding IN SELECT value FROM jsonb_array_elements(coalesce(p_snapshot->'bindings','[]'::jsonb))
  LOOP
    lineage := (binding->>'binding_lineage_id')::uuid;
    rev := (binding->>'binding_revision_id')::uuid;
    IF (binding->>'requirement_projection_revision_id')::uuid IS DISTINCT FROM cur.requirement_projection_id
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
      requirement_projection_id,channel,target_kind,target_id,state,reason,actor,
      canonical_payload,content_sha256)
    VALUES(rev,w.project_id,p_workspace_id,lineage,coalesce((binding->>'revision')::bigint,1),
      (binding->>'need_occurrence_id')::uuid,
      (binding->>'requirement_projection_revision_id')::uuid,binding->>'channel',
      binding#>>'{target,kind}',coalesce(
        (binding#>>'{target,node_lineage_id}')::uuid,
        (binding#>>'{target,block_lineage_id}')::uuid,
        (binding#>>'{target,form_definition_revision_id}')::uuid,
        (binding#>>'{target,quote_snapshot_id}')::uuid),
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
  FOR evidence IN
    SELECT binding.*,occurrence.ordinal binding_ordinal FROM bid_workspace_binding_occurrences occurrence
    JOIN bid_outline_fulfillment_binding_revision_artifacts binding ON binding.id=occurrence.binding_revision_id
    WHERE occurrence.workspace_revision_id=new_rev AND binding.state='bound'
      AND binding.requirement_projection_id=cur.requirement_projection_id ORDER BY occurrence.ordinal
  LOOP
    evidence_target:=NULL;evidence_kind:=NULL;evidence_dependency:=NULL;evidence_state:='current';
    IF evidence.target_kind='outline_node' THEN
      SELECT block.id,'block',block.content_sha256 INTO evidence_target,evidence_kind,evidence_dependency FROM bid_workspace_node_occurrences node_occurrence
      JOIN bid_outline_node_revision_artifacts node ON node.id=node_occurrence.node_revision_id
      JOIN bid_workspace_block_occurrences block_occurrence ON block_occurrence.node_occurrence_id=node_occurrence.id
      JOIN bid_content_block_revision_artifacts block ON block.id=block_occurrence.block_revision_id
      WHERE node_occurrence.workspace_revision_id=new_rev AND node.lineage_id=evidence.target_id
      ORDER BY block_occurrence.ordinal LIMIT 1;
    ELSIF evidence.target_kind='response_table' THEN
      SELECT block.id,'block',block.content_sha256 INTO evidence_target,evidence_kind,evidence_dependency FROM bid_workspace_block_occurrences block_occurrence
      JOIN bid_content_block_revision_artifacts block ON block.id=block_occurrence.block_revision_id
      WHERE block_occurrence.workspace_revision_id=new_rev AND block.lineage_id=evidence.target_id AND block.block_kind='table' LIMIT 1;
    ELSIF evidence.target_kind='structured_form' THEN
      SELECT block.id,'structured_value',block.content_sha256 INTO evidence_target,evidence_kind,evidence_dependency FROM bid_workspace_block_occurrences block_occurrence
      JOIN bid_content_block_revision_artifacts block ON block.id=block_occurrence.block_revision_id
      WHERE block_occurrence.workspace_revision_id=new_rev AND block.block_kind='structured_form'
        AND (block.block_payload->>'form_definition_revision_id')::uuid=evidence.target_id LIMIT 1;
    ELSIF evidence.target_kind='quote' THEN
      SELECT quote.id,'quote_snapshot',quote.content_sha256 INTO evidence_target,evidence_kind,evidence_dependency
      FROM bid_quote_snapshot_artifacts quote WHERE quote.id=evidence.target_id AND quote.project_id=w.project_id;
    END IF;
    IF evidence_target IS NOT NULL THEN
      evidence_lineage:=kb_bid_v2_deterministic_uuid('fulfillment-evidence:'||evidence.lineage_id::text||':'||evidence_target::text);
      SELECT coalesce(max(revision),0)+1 INTO evidence_revision FROM bid_submission_fulfillment_evidence_revision_artifacts
        WHERE project_id=w.project_id AND evidence_lineage_id=evidence_lineage;
      SELECT prior.state INTO evidence_state FROM bid_submission_fulfillment_evidence_revision_artifacts prior
        WHERE prior.project_id=w.project_id AND prior.evidence_lineage_id=evidence_lineage
        ORDER BY prior.revision DESC LIMIT 1;
      evidence_state:=coalesce(evidence_state,'current');
      payload:=kb_bid_v2_json_payload(jsonb_build_object('schema_version',1,'evidence_lineage_id',evidence_lineage,
        'revision',evidence_revision,'workspace_revision_id',new_rev,'binding_revision_id',evidence.id,
        'target_revision_id',evidence_target,'target_kind',evidence_kind,'state',evidence_state,
        'dependency_sha256',evidence_dependency));sha:=kb_bid_v2_sha256_bytes(payload);
      INSERT INTO bid_submission_fulfillment_evidence_revision_artifacts(id,project_id,workspace_id,evidence_lineage_id,
        revision,workspace_revision_id,binding_revision_id,target_revision_id,target_kind,dependency_sha256,state,
        canonical_payload,content_sha256)
      VALUES(gen_random_uuid(),w.project_id,p_workspace_id,evidence_lineage,evidence_revision,new_rev,evidence.id,
        evidence_target,evidence_kind,evidence_dependency,evidence_state,payload,sha);
    END IF;
  END LOOP;
  -- Preserve stale/withdrawn or otherwise non-revalidated evidence across unrelated edits.
  FOR evidence IN
    SELECT prior.* FROM bid_submission_fulfillment_evidence_revision_artifacts prior
    WHERE prior.workspace_revision_id=cur.id AND NOT EXISTS (
      SELECT 1 FROM bid_submission_fulfillment_evidence_revision_artifacts current_evidence
      WHERE current_evidence.workspace_revision_id=new_rev
        AND current_evidence.evidence_lineage_id=prior.evidence_lineage_id)
  LOOP
    SELECT coalesce(max(revision),0)+1 INTO evidence_revision
      FROM bid_submission_fulfillment_evidence_revision_artifacts
      WHERE project_id=w.project_id AND evidence_lineage_id=evidence.evidence_lineage_id;
    payload:=kb_bid_v2_json_payload(jsonb_build_object('schema_version',1,
      'evidence_lineage_id',evidence.evidence_lineage_id,'revision',evidence_revision,
      'workspace_revision_id',new_rev,'binding_revision_id',evidence.binding_revision_id,
      'target_revision_id',evidence.target_revision_id,'target_kind',evidence.target_kind,
      'state',evidence.state,'dependency_sha256',evidence.dependency_sha256));
    sha:=kb_bid_v2_sha256_bytes(payload);
    INSERT INTO bid_submission_fulfillment_evidence_revision_artifacts(id,project_id,workspace_id,
      evidence_lineage_id,revision,workspace_revision_id,binding_revision_id,target_revision_id,target_kind,
      dependency_sha256,state,canonical_payload,content_sha256)
    VALUES(gen_random_uuid(),w.project_id,p_workspace_id,evidence.evidence_lineage_id,evidence_revision,
      new_rev,evidence.binding_revision_id,evidence.target_revision_id,evidence.target_kind,
      evidence.dependency_sha256,evidence.state,payload,sha);
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

-- User-visible V2 vertical-flow application procedures.  These functions keep
-- every mutation behind an authenticated actor, shared idempotency receipt and
-- aggregate CAS; the API role never receives direct DML on authoring tables.
CREATE FUNCTION kb_bid_v2_require_project_owner(
  p_project_id uuid,p_actor kb_actor_identity
) RETURNS void LANGUAGE plpgsql STABLE SET search_path=pg_catalog,public AS $$
DECLARE owner_id uuid;
BEGIN
  IF p_actor NOT LIKE 'user:%' THEN
    RAISE EXCEPTION 'USER_ACTOR_REQUIRED' USING ERRCODE='42501';
  END IF;
  SELECT owner_user_id INTO owner_id FROM bid_projects WHERE id=p_project_id;
  IF NOT FOUND THEN RAISE EXCEPTION 'PROJECT_NOT_FOUND' USING ERRCODE='P0002'; END IF;
  IF p_actor<>'user:'||owner_id::text THEN
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
  IF p_actor<>'user:'||p_owner_user_id::text THEN
    RAISE EXCEPTION 'PROJECT_OWNER_ACTOR_MISMATCH' USING ERRCODE='42501';
  END IF;
  RETURN COALESCE((SELECT jsonb_agg(jsonb_build_object(
      'id',p.id,'title',p.title,'status',p.status,'ended_at',p.ended_at,
      'workspace_id',w.id,'owner_user_id',p.owner_user_id) ORDER BY p.created_at,p.id)
    FROM bid_projects p JOIN bid_submission_workspaces w ON w.project_id=p.id
    WHERE p.owner_user_id=p_owner_user_id),'[]'::jsonb);
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
  converter_payload bytea:=convert_to('docparser-structured-source-v2','UTF8');
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
  VALUES(converter_id,'converter',1,converter_payload,converter_sha) ON CONFLICT (id) DO NOTHING;
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
    'request_artifact_id',p_request_artifact_id,'request_revision',1,'frozen_input_sha256',frozen_sha);
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
    'request_artifact_id',p_request_artifact_id,'request_revision',1,'frozen_input_sha256',frozen_sha);
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
  p_request_bytes bytea,p_request_sha256 kb_sha256
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
    source_disposition:=CASE WHEN unit_value.unit_kind IN ('table_row','form_region')
      OR convert_from(unit_value.text_utf8,'UTF8')~*'(必须|应当|不得|须|must|shall|required|要求|资格|评分|报价)'
      THEN 'requirement' ELSE 'unresolved' END;
    disposition_items:=disposition_items||jsonb_build_array(jsonb_build_object(
      'source_unit_revision_id',unit_value.id,'disposition',source_disposition,
      'reason',CASE WHEN source_disposition='requirement' THEN 'deterministic_requirement_signal' ELSE 'initial_uncertain' END,
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
    'disposition_set_revision_id',disposition_id,'disposition_set_sha256',disposition_sha));
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
    disposition_set_revision_id,disposition_set_sha256)
  VALUES(p_request_artifact_id,p_project_id,1,job_sha,frozen_sha,set_id,set_sha,disposition_id,disposition_sha);
  response:=jsonb_build_object('artifact_id',set_id,'sha256',set_sha,'revision',set_revision,
    'disposition_set_artifact_id',disposition_id,'disposition_set_sha256',disposition_sha,
    'request_artifact_id',p_request_artifact_id,'request_revision',1,'frozen_input_sha256',frozen_sha,
    'warnings',warnings);
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
  p_actor kb_actor_identity,p_idempotency_key text,p_request_bytes bytea,p_request_sha256 kb_sha256
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
    'disposition_set_revision_id',disposition_id,'disposition_set_sha256',disposition_sha));
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
    disposition_set_revision_id,disposition_set_sha256)
  VALUES(p_request_artifact_id,p_project_id,1,job_sha,frozen_sha,p_document_set_id,
    document_value.content_sha256,disposition_id,disposition_sha);
  response:=jsonb_build_object('artifact_id',disposition_id,'sha256',disposition_sha,
    'revision',disposition_revision,'document_set_revision_id',p_document_set_id,
    'request_artifact_id',p_request_artifact_id,'request_revision',1,'frozen_input_sha256',frozen_sha);
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
  p_new_projection_id uuid,p_new_projection_sha256 kb_sha256
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
    payload,digest,'system:requirement-set-compile-v2');
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
  FOR evidence IN SELECT * FROM bid_submission_fulfillment_evidence_revision_artifacts
    WHERE workspace_revision_id=old_revision.id ORDER BY evidence_lineage_id
  LOOP
    SELECT coalesce(max(revision),0)+1 INTO evidence_revision
      FROM bid_submission_fulfillment_evidence_revision_artifacts
      WHERE project_id=workspace.project_id AND evidence_lineage_id=evidence.evidence_lineage_id;
    evidence_payload:=kb_bid_v2_json_payload(jsonb_build_object('schema_version',1,
      'evidence_lineage_id',evidence.evidence_lineage_id,'revision',evidence_revision,
      'workspace_revision_id',new_revision_id,'binding_revision_id',evidence.binding_revision_id,
      'target_revision_id',evidence.target_revision_id,'target_kind',evidence.target_kind,
      'state','stale','dependency_sha256',evidence.dependency_sha256));
    evidence_sha:=kb_bid_v2_sha256_bytes(evidence_payload);
    INSERT INTO bid_submission_fulfillment_evidence_revision_artifacts(id,project_id,workspace_id,
      evidence_lineage_id,revision,workspace_revision_id,binding_revision_id,target_revision_id,target_kind,
      dependency_sha256,state,canonical_payload,content_sha256)
    VALUES(gen_random_uuid(),workspace.project_id,p_workspace_id,evidence.evidence_lineage_id,evidence_revision,
      new_revision_id,evidence.binding_revision_id,evidence.target_revision_id,evidence.target_kind,
      evidence.dependency_sha256,'stale',evidence_payload,evidence_sha);
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
  UPDATE bid_candidate_artifacts SET state='obsolete',decided_at=clock_timestamp()
    WHERE workspace_id=p_workspace_id AND base_workspace_revision_id=old_revision.id AND state='proposed';
  UPDATE bid_async_request_snapshot_artifacts request_value SET status='obsolete',finished_at=clock_timestamp()
    WHERE request_value.status='pending' AND request_value.id IN (
      SELECT request_artifact_id FROM bid_outline_generation_request_identities
        WHERE workspace_id=p_workspace_id AND base_workspace_revision_id=old_revision.id
      UNION ALL
      SELECT request_artifact_id FROM bid_content_generation_request_identities
        WHERE workspace_id=p_workspace_id AND base_workspace_revision_id=old_revision.id);
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
  IF publication_status='obsolete' THEN
    result_value:=jsonb_build_object('requirement_set_id',set_id,'requirement_set_sha256',set_sha,
      'requirement_count',ordinal_value,'status','obsolete','replayed',false);
    result_sha:=kb_bid_v2_sha256_bytes(convert_to(result_value::text,'UTF8'));
    INSERT INTO bid_async_stage_receipts(request_artifact_id,stage_kind,frozen_input_sha256,result_identity,result_sha256)
    VALUES(p_request_artifact_id,'requirement_compile',p_frozen_input_sha256,result_value,result_sha);
    UPDATE bid_async_request_snapshot_artifacts SET status='obsolete',result_identity=result_value,
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
  PERFORM kb_bid_v2_advance_workspace_projection(workspace_value.id,
    projection_head.artifact_id,projection_head.artifact_sha256,projection_id,projection_sha);
  result_value:=jsonb_build_object('requirement_set_id',set_id,'requirement_set_sha256',set_sha,
    'requirement_count',ordinal_value,'requirement_projection_id',projection_id,
    'requirement_projection_sha256',projection_sha,'replayed',false);
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
      'FROZEN_INPUT_DIGEST_MISMATCH','REQUEST_OBSOLETE') THEN p_error_code
      ELSE 'REQUIREMENT_COMPILE_FAILED' END,
    finished_at=clock_timestamp()
    WHERE id=p_request_artifact_id AND request_kind='requirement_set_compile' AND status='pending';
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
  PERFORM kb_bid_v2_advance_workspace_projection(workspace_value.id,
    projection_head.artifact_id,projection_head.artifact_sha256,projection_id,projection_sha);
  response:=jsonb_build_object('requirement_revision_id',new_requirement_id,
    'lineage_id',old_requirement.lineage_id,'revision',new_requirement_revision,
    'content_sha256',requirement_sha,'requirement_set_id',new_set_id,
    'requirement_set_sha256',set_sha,'requirement_projection_id',projection_id,
    'requirement_projection_sha256',projection_sha);
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
    PERFORM kb_bid_v2_advance_workspace_projection(workspace_value.id,projection_head.artifact_id,
      projection_head.artifact_sha256,projection_id,projection_sha);
  response:=jsonb_build_object('artifact_id',new_id,'lineage_id',p_lineage_id,
    'revision',new_revision,'sha256',sha,'tombstone',p_tombstone,
    'requirement_set_id',new_set_id,'requirement_set_sha256',set_sha,
    'requirement_projection_id',projection_id,'requirement_projection_sha256',projection_sha);
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

-- Minimal deterministic authoring contracts keep the first vertical slice
-- reproducible; model-backed replacements remain pinned by the same identities.
INSERT INTO bid_authoring_contract_artifacts(id,contract_kind,schema_version,canonical_payload,content_sha256)
SELECT id,kind,1,payload,kb_bid_v2_sha256_bytes(payload) FROM (VALUES
  ('00000000-0000-5000-8000-000000000101'::uuid,'prompt',convert_to('{"kind":"outline_prompt","version":1}','UTF8')),
  ('00000000-0000-5000-8000-000000000102'::uuid,'template',convert_to('{"kind":"outline_template","version":1}','UTF8')),
  ('00000000-0000-5000-8000-000000000103'::uuid,'model',convert_to('{"kind":"configured_chat_model","operation":"outline","version":1}','UTF8')),
  ('00000000-0000-5000-8000-000000000104'::uuid,'agent',convert_to('{"kind":"outline_agent","version":1}','UTF8'))
) seeded(id,kind,payload) ON CONFLICT (id) DO NOTHING;

CREATE FUNCTION kb_bid_v2_create_outline_candidate(
  p_workspace_id uuid,p_expected_revision_id uuid,p_expected_sha256 kb_sha256,
  p_document_set_id uuid,p_document_set_sha256 kb_sha256,p_actor kb_actor_identity,
  p_idempotency_key text,p_request_bytes bytea,p_request_sha256 kb_sha256
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE workspace bid_submission_workspaces%ROWTYPE; head bid_workspace_heads%ROWTYPE;
  workspace_rev bid_workspace_revision_artifacts%ROWTYPE;
  projection bid_workspace_requirement_projection_artifacts%ROWTYPE;
  requirement_set bid_requirement_set_artifacts%ROWTYPE;
  disposition bid_source_unit_disposition_set_artifacts%ROWTYPE;
  scope_rev bid_workspace_scope_revision_artifacts%ROWTYPE;
  request_id uuid:=gen_random_uuid(); frozen_payload bytea; frozen_sha kb_sha256;
  response jsonb; response_bytes bytea; replay bytea; form_identities jsonb;
  prompt_sha kb_sha256; template_sha kb_sha256; model_sha kb_sha256; agent_sha kb_sha256;
BEGIN
  replay:=kb_bid_v2_idempotency_begin(p_actor,'bid.v2.outline.generate',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  SELECT * INTO STRICT workspace FROM bid_submission_workspaces WHERE id=p_workspace_id;
  PERFORM kb_bid_v2_require_project_owner(workspace.project_id,p_actor);
  SELECT * INTO STRICT head FROM bid_workspace_heads WHERE scope_id=p_workspace_id FOR SHARE;
  IF head.artifact_id IS DISTINCT FROM p_expected_revision_id OR head.artifact_sha256 IS DISTINCT FROM p_expected_sha256 THEN
    RAISE EXCEPTION 'WORKSPACE_HEAD_CAS_MISMATCH' USING ERRCODE='40001';
  END IF;
  SELECT * INTO STRICT workspace_rev FROM bid_workspace_revision_artifacts WHERE id=head.artifact_id;
  SELECT * INTO STRICT projection FROM bid_workspace_requirement_projection_artifacts
    WHERE id=workspace_rev.requirement_projection_id AND content_sha256=workspace_rev.requirement_projection_sha256;
  SELECT * INTO STRICT requirement_set FROM bid_requirement_set_artifacts WHERE id=projection.requirement_set_id;
  SELECT * INTO STRICT disposition FROM bid_source_unit_disposition_set_artifacts WHERE id=requirement_set.disposition_set_id;
  SELECT * INTO STRICT scope_rev FROM bid_workspace_scope_revision_artifacts WHERE id=workspace_rev.scope_revision_id;
  IF requirement_set.document_set_id IS DISTINCT FROM p_document_set_id OR
     (SELECT content_sha256 FROM bid_document_set_artifacts WHERE id=p_document_set_id) IS DISTINCT FROM p_document_set_sha256 THEN
    RAISE EXCEPTION 'DOCUMENT_SET_CAS_MISMATCH' USING ERRCODE='40001';
  END IF;
  SELECT content_sha256 INTO STRICT prompt_sha FROM bid_authoring_contract_artifacts WHERE id='00000000-0000-5000-8000-000000000101';
  SELECT content_sha256 INTO STRICT template_sha FROM bid_authoring_contract_artifacts WHERE id='00000000-0000-5000-8000-000000000102';
  SELECT content_sha256 INTO STRICT model_sha FROM bid_authoring_contract_artifacts WHERE id='00000000-0000-5000-8000-000000000103';
  SELECT content_sha256 INTO STRICT agent_sha FROM bid_authoring_contract_artifacts WHERE id='00000000-0000-5000-8000-000000000104';
  SELECT coalesce(jsonb_agg(jsonb_build_object('form_definition_revision_id',form.id,
      'form_definition_sha256',form.content_sha256,'source_unit_revision_id',form.source_unit_revision_id)
      ORDER BY form.source_unit_revision_id,form.id),'[]'::jsonb) INTO form_identities
    FROM bid_tender_structured_form_definition_artifacts form
    JOIN bid_source_unit_disposition_set_items disposition_item
      ON disposition_item.project_id=form.project_id AND disposition_item.source_unit_revision_id=form.source_unit_revision_id
    JOIN bid_source_unit_revision_artifacts source ON source.project_id=form.project_id AND source.id=form.source_unit_revision_id
    JOIN bid_document_set_items document_item ON document_item.project_id=source.project_id
      AND document_item.document_set_id=p_document_set_id AND document_item.document_id=source.document_id
    WHERE disposition_item.disposition_set_id=disposition.id;
  frozen_payload:=kb_bid_v2_json_payload(jsonb_build_object(
    'workspace_revision_id',workspace_rev.id,'workspace_sha256',workspace_rev.content_sha256,
    'document_set_revision_id',p_document_set_id,'document_set_sha256',p_document_set_sha256,
    'requirement_projection_revision_id',projection.id,'requirement_projection_sha256',projection.content_sha256,
    'scope_revision_id',scope_rev.id,'scope_revision_sha256',scope_rev.content_sha256,
    'prompt_contract_sha256',prompt_sha,'template_contract_sha256',template_sha,
    'model_contract_sha256',model_sha,'agent_contract_sha256',agent_sha,
    'structured_form_identities',form_identities));
  frozen_sha:=kb_bid_v2_sha256_bytes(frozen_payload);
  INSERT INTO bid_async_request_snapshot_artifacts(
    id,project_id,workspace_id,request_kind,revision,frozen_input_sha256,request_payload,request_sha256,status)
  VALUES(request_id,workspace.project_id,p_workspace_id,'outline_generate',1,frozen_sha,p_request_bytes,p_request_sha256,'pending');
  INSERT INTO bid_outline_generation_request_identities(
    request_artifact_id,project_id,workspace_id,request_revision,request_sha256,frozen_input_sha256,
    base_workspace_revision_id,base_workspace_sha256,document_set_revision_id,document_set_sha256,
    disposition_set_revision_id,disposition_set_sha256,requirement_set_revision_id,requirement_set_sha256,
    requirement_projection_id,requirement_projection_sha256,scope_revision_id,scope_revision_sha256,
    prompt_contract_id,prompt_contract_sha256,template_contract_id,template_contract_sha256,
    model_contract_id,model_contract_sha256,agent_contract_id,agent_contract_sha256,structured_form_identities)
  VALUES(request_id,workspace.project_id,p_workspace_id,1,p_request_sha256,frozen_sha,
    workspace_rev.id,workspace_rev.content_sha256,p_document_set_id,p_document_set_sha256,
    disposition.id,disposition.content_sha256,requirement_set.id,requirement_set.content_sha256,
    projection.id,projection.content_sha256,scope_rev.id,scope_rev.content_sha256,
    '00000000-0000-5000-8000-000000000101',prompt_sha,
    '00000000-0000-5000-8000-000000000102',template_sha,
    '00000000-0000-5000-8000-000000000103',model_sha,
    '00000000-0000-5000-8000-000000000104',agent_sha,form_identities);
  response:=jsonb_build_object('request_artifact_id',request_id,'kind','OutlineGenerate','status','pending',
    'result_identity',NULL,'error_code',NULL,'request_revision',1,'request_sha256',p_request_sha256,
    'frozen_input_sha256',frozen_sha,'project_id',workspace.project_id,'workspace_id',p_workspace_id,
    'base_workspace_revision_id',workspace_rev.id);
  response_bytes:=convert_to(response::text,'UTF8');
  PERFORM kb_bid_v2_idempotency_complete(p_actor,'bid.v2.outline.generate',p_idempotency_key,202,response_bytes);
  RETURN response;
END $$;

CREATE FUNCTION kb_bid_v2_load_outline_generation_input(
  p_request_artifact_id uuid,p_request_revision bigint,p_frozen_input_sha256 kb_sha256
) RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE typed bid_outline_generation_request_identities%ROWTYPE;
BEGIN
  SELECT * INTO STRICT typed FROM bid_outline_generation_request_identities
    WHERE request_artifact_id=p_request_artifact_id AND request_revision=p_request_revision
      AND frozen_input_sha256=p_frozen_input_sha256;
  RETURN jsonb_build_object('schema_version',1,'request_artifact_id',typed.request_artifact_id,
    'project_id',typed.project_id,'workspace_id',typed.workspace_id,
    'base_workspace_revision_id',typed.base_workspace_revision_id,'base_workspace_sha256',typed.base_workspace_sha256,
    'document_set_revision_id',typed.document_set_revision_id,'document_set_sha256',typed.document_set_sha256,
    'disposition_set_revision_id',typed.disposition_set_revision_id,
    'disposition_set_sha256',typed.disposition_set_sha256,
    'requirement_set_revision_id',typed.requirement_set_revision_id,
    'requirement_set_sha256',typed.requirement_set_sha256,
    'requirement_projection_revision_id',typed.requirement_projection_id,
    'requirement_projection_sha256',typed.requirement_projection_sha256,
    'workspace_scope_revision_id',typed.scope_revision_id,'workspace_scope_sha256',typed.scope_revision_sha256,
    'prompt_contract_id',typed.prompt_contract_id,'prompt_contract_sha256',typed.prompt_contract_sha256,
    'template_contract_id',typed.template_contract_id,'template_contract_sha256',typed.template_contract_sha256,
    'model_contract_id',typed.model_contract_id,'model_contract_sha256',typed.model_contract_sha256,
    'agent_contract_id',typed.agent_contract_id,'agent_contract_sha256',typed.agent_contract_sha256,
    'workspace_scope',(SELECT convert_from(scope.canonical_payload,'UTF8')::jsonb
      FROM bid_workspace_scope_revision_artifacts scope
      WHERE scope.id=typed.scope_revision_id AND scope.content_sha256=typed.scope_revision_sha256),
    'document_set',jsonb_build_object('artifact_id',typed.document_set_revision_id,
      'sha256',typed.document_set_sha256,
      'relations',coalesce((SELECT convert_from(document_set.canonical_payload,'UTF8')::jsonb->'relations'
        FROM bid_document_set_artifacts document_set WHERE document_set.id=typed.document_set_revision_id),'[]'::jsonb),
      'items',coalesce((SELECT convert_from(document_set.canonical_payload,'UTF8')::jsonb->'items'
        FROM bid_document_set_artifacts document_set
        WHERE document_set.id=typed.document_set_revision_id
          AND document_set.content_sha256=typed.document_set_sha256),'[]'::jsonb)),
    'source_units',coalesce((SELECT jsonb_agg(jsonb_build_object(
      'source_unit_revision_id',source.id,'document_id',source.document_id,
      'source_revision_id',source.source_revision_id,'unit_kind',source.unit_kind,'ordinal',source.ordinal,
      'source_locator',source.source_locator,'source_span_sha256',source.source_span_sha256,
      'text',convert_from(source.text_utf8,'UTF8'),'text_sha256',source.text_sha256,
      'disposition',disposition.disposition,'disposition_reason',disposition.reason)
      ORDER BY set_item.ordinal,source.ordinal,source.id)
      FROM bid_source_unit_disposition_set_items disposition
      JOIN bid_source_unit_revision_artifacts source
        ON source.project_id=disposition.project_id AND source.id=disposition.source_unit_revision_id
      JOIN bid_document_set_items set_item ON set_item.document_set_id=typed.document_set_revision_id
        AND set_item.project_id=source.project_id AND set_item.document_id=source.document_id
      WHERE disposition.disposition_set_id=typed.disposition_set_revision_id),'[]'::jsonb),
    'structured_forms',coalesce((SELECT jsonb_agg(jsonb_build_object(
      'form_definition_revision_id',form.id,'form_definition_sha256',form.content_sha256,
      'source_unit_revision_id',form.source_unit_revision_id,
      'definition',convert_from(form.canonical_payload,'UTF8')::jsonb)
      ORDER BY form.source_unit_revision_id,form.id)
      FROM jsonb_array_elements(typed.structured_form_identities) identity_value
      JOIN bid_tender_structured_form_definition_artifacts form
        ON form.id=(identity_value->>'form_definition_revision_id')::uuid
          AND form.content_sha256=(identity_value->>'form_definition_sha256')::kb_sha256
          AND form.source_unit_revision_id=(identity_value->>'source_unit_revision_id')::uuid),'[]'::jsonb),
    'requirements',coalesce((SELECT jsonb_agg(jsonb_build_object(
      'need_occurrence_id',requirement.id,'requirement_revision_id',requirement.id,
      'requirement_text',convert_from(requirement.text_utf8,'UTF8'),'requirement_kind',requirement.requirement_kind,
      'requiredness',requirement.requiredness,'compliance_policy',requirement.compliance_policy,
      'applicability',requirement.applicability,'effective_applicability',item.effective_applicability,
      'source_unit_revision_ids',coalesce((SELECT jsonb_agg(source.source_unit_revision_id ORDER BY source.source_unit_revision_id)
        FROM bid_requirement_source_revision_artifacts source WHERE source.requirement_revision_id=requirement.id),'[]'::jsonb))
      ORDER BY item.ordinal,requirement.id)
      FROM bid_workspace_requirement_projection_items item
      JOIN bid_requirement_revision_artifacts requirement ON requirement.project_id=item.project_id AND requirement.id=item.requirement_revision_id
      WHERE item.projection_id=typed.requirement_projection_id),'[]'::jsonb),
    'current_outline',coalesce((SELECT jsonb_agg(jsonb_build_object('node_lineage_id',node.lineage_id,
      'node_revision_id',node.id,'parent_lineage_id',parent_node.lineage_id,'ordinal',occurrence.ordinal,
      'title',node.title,'semantic_role',node.semantic_role,'render_role',node.render_role)
      ORDER BY occurrence.depth,occurrence.ordinal,node.id)
      FROM bid_workspace_node_occurrences occurrence JOIN bid_outline_node_revision_artifacts node
        ON node.project_id=occurrence.project_id AND node.id=occurrence.node_revision_id
      LEFT JOIN bid_workspace_node_occurrences parent_occurrence ON parent_occurrence.id=occurrence.parent_occurrence_id
      LEFT JOIN bid_outline_node_revision_artifacts parent_node ON parent_node.id=parent_occurrence.node_revision_id
      WHERE occurrence.project_id=typed.project_id AND occurrence.workspace_revision_id=typed.base_workspace_revision_id),'[]'::jsonb));
END $$;

CREATE FUNCTION kb_bid_v2_publish_outline_generation(
  p_request_artifact_id uuid,p_request_revision bigint,p_frozen_input_sha256 kb_sha256,
  p_candidate_id uuid,p_candidate_payload bytea,p_candidate_sha256 kb_sha256,p_nodes jsonb
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE typed bid_outline_generation_request_identities%ROWTYPE; request_value bid_async_request_snapshot_artifacts%ROWTYPE;
  candidate_json jsonb; node_value jsonb; ordinal_value integer:=0; published_identity jsonb;
BEGIN
  SELECT * INTO STRICT typed FROM bid_outline_generation_request_identities
    WHERE request_artifact_id=p_request_artifact_id AND request_revision=p_request_revision
      AND frozen_input_sha256=p_frozen_input_sha256 FOR UPDATE;
  SELECT * INTO STRICT request_value FROM bid_async_request_snapshot_artifacts WHERE id=p_request_artifact_id FOR UPDATE;
  IF request_value.status='succeeded' THEN RETURN request_value.result_identity; END IF;
  IF request_value.status<>'pending' OR p_candidate_id IS NULL OR p_candidate_payload IS NULL
     OR p_candidate_sha256 IS NULL OR p_candidate_sha256<>kb_bid_v2_sha256_bytes(p_candidate_payload)
     OR jsonb_typeof(p_nodes)<>'array' OR jsonb_array_length(p_nodes) NOT BETWEEN 1 AND 1000 THEN
    RAISE EXCEPTION 'AGENT_OUTPUT_INVALID' USING ERRCODE='23514';
  END IF;
  candidate_json:=convert_from(p_candidate_payload,'UTF8')::jsonb;
  IF NOT kb_bid_v2_json_keys_exact(candidate_json,ARRAY['schema_version','nodes','bindings','notices'])
     OR candidate_json->'schema_version' IS DISTINCT FROM '1'::jsonb OR candidate_json->'nodes' IS DISTINCT FROM p_nodes
     OR (SELECT count(*) FROM jsonb_array_elements(p_nodes) node WHERE jsonb_typeof(node->'parent_client_node_ref')='null')<>1
     OR EXISTS (SELECT 1 FROM jsonb_array_elements(p_nodes) node GROUP BY node->>'client_node_ref' HAVING count(*)<>1)
     OR EXISTS (SELECT 1 FROM jsonb_array_elements(p_nodes) node
       WHERE jsonb_typeof(node)<>'object' OR NOT kb_bid_v2_json_keys_exact(node,ARRAY['client_node_ref','parent_client_node_ref','ordinal','title','semantic_role','render_role','origin_source_unit_revision_ids'])
         OR (jsonb_typeof(node->'parent_client_node_ref')<>'null' AND NOT EXISTS (SELECT 1 FROM jsonb_array_elements(p_nodes) parent WHERE parent->>'client_node_ref'=node->>'parent_client_node_ref')))
     OR EXISTS (SELECT 1 FROM (SELECT node->'parent_client_node_ref' parent,count(*) count_value,
          count(DISTINCT (node->>'ordinal')::integer) distinct_value,min((node->>'ordinal')::integer) min_value,max((node->>'ordinal')::integer) max_value
        FROM jsonb_array_elements(p_nodes) node GROUP BY node->'parent_client_node_ref') siblings
        WHERE distinct_value<>count_value OR min_value<>0 OR max_value<>count_value-1)
     OR EXISTS (SELECT 1 FROM jsonb_array_elements(p_nodes) node,
          jsonb_array_elements_text(node->'origin_source_unit_revision_ids') source_id
        WHERE NOT EXISTS (SELECT 1 FROM bid_workspace_requirement_projection_items projection_item
          JOIN bid_requirement_source_revision_artifacts source ON source.requirement_revision_id=projection_item.requirement_revision_id
          WHERE projection_item.projection_id=typed.requirement_projection_id AND source.source_unit_revision_id=source_id::uuid))
     OR EXISTS (SELECT 1 FROM jsonb_array_elements(candidate_json->'bindings') binding
        WHERE NOT EXISTS (SELECT 1 FROM bid_workspace_requirement_projection_items projection_item
          JOIN bid_requirement_revision_artifacts requirement ON requirement.id=projection_item.requirement_revision_id
          WHERE projection_item.projection_id=typed.requirement_projection_id
            AND (binding->>'need_occurrence_id')::uuid=ANY(kb_bid_v2_fulfillment_need_ids(requirement.fulfillment_expr)))
          OR NOT EXISTS (SELECT 1 FROM jsonb_array_elements(p_nodes) node WHERE node->>'client_node_ref'=binding->>'target_client_node_ref'))
  THEN RAISE EXCEPTION 'AGENT_OUTPUT_INVALID' USING ERRCODE='23514'; END IF;
  INSERT INTO bid_candidate_artifacts(id,project_id,workspace_id,candidate_kind,base_workspace_revision_id,
    base_workspace_sha256,request_artifact_id,request_kind,request_revision,request_sha256,
    request_operation,state,canonical_payload,content_sha256)
  VALUES(p_candidate_id,typed.project_id,typed.workspace_id,'outline',typed.base_workspace_revision_id,
    typed.base_workspace_sha256,p_request_artifact_id,'outline_generate',typed.request_revision,typed.request_sha256,
    'outline_generate','proposed',p_candidate_payload,p_candidate_sha256);
  FOR node_value IN SELECT value FROM jsonb_array_elements(p_nodes) LOOP
    INSERT INTO bid_candidate_operations(candidate_id,ordinal,operation,operation_sha256)
    VALUES(p_candidate_id,ordinal_value,node_value,kb_bid_v2_sha256_bytes(convert_to(node_value::text,'UTF8')));
    ordinal_value:=ordinal_value+1;
  END LOOP;
  published_identity:=jsonb_build_object('artifact_id',p_candidate_id,'sha256',p_candidate_sha256);
  INSERT INTO bid_async_stage_receipts(request_artifact_id,stage_kind,frozen_input_sha256,result_identity,result_sha256)
  VALUES(p_request_artifact_id,'agent_generate',p_frozen_input_sha256,published_identity,p_candidate_sha256);
  UPDATE bid_async_request_snapshot_artifacts SET status='succeeded',result_identity=published_identity,
    finished_at=clock_timestamp() WHERE id=p_request_artifact_id;
  RETURN published_identity;
END $$;

CREATE FUNCTION kb_bid_v2_mark_outline_generation_failed(
  p_request_artifact_id uuid,p_request_revision bigint,p_frozen_input_sha256 kb_sha256,p_error_code text
) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
BEGIN
  PERFORM 1 FROM bid_outline_generation_request_identities
    WHERE request_artifact_id=p_request_artifact_id AND request_revision=p_request_revision
      AND frozen_input_sha256=p_frozen_input_sha256;
  IF NOT FOUND THEN RAISE EXCEPTION 'FROZEN_INPUT_MISSING' USING ERRCODE='P0002'; END IF;
  UPDATE bid_async_request_snapshot_artifacts SET status='failed',
    error_code=CASE WHEN p_error_code IN ('INPUT_SCHEMA_INVALID','FROZEN_INPUT_MISSING','FROZEN_INPUT_DIGEST_MISMATCH',
      'REQUEST_OBSOLETE','WORKSPACE_CAS_CONFLICT','AGENT_OUTPUT_INVALID') THEN p_error_code ELSE 'AGENT_OUTPUT_INVALID' END,
    finished_at=clock_timestamp() WHERE id=p_request_artifact_id AND status='pending';
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
    'kind',CASE request_value.request_kind WHEN 'outline_generate' THEN 'OutlineGenerate'
      WHEN 'content_generate' THEN 'ContentGenerate' WHEN 'submission_export' THEN 'SubmissionExport'
      ELSE request_value.request_kind END,
    'status',request_value.status,'result_identity',request_value.result_identity,'error_code',request_value.error_code);
END $$;

CREATE FUNCTION kb_bid_v2_get_candidate(
  p_workspace_id uuid,p_candidate_id uuid,p_actor kb_actor_identity
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE candidate bid_candidate_artifacts%ROWTYPE; owner_project_id uuid; payload jsonb;
BEGIN
  SELECT workspace.project_id INTO STRICT owner_project_id
    FROM bid_submission_workspaces workspace WHERE workspace.id=p_workspace_id;
  PERFORM kb_bid_v2_require_project_owner(owner_project_id,p_actor);
  SELECT * INTO candidate FROM bid_candidate_artifacts WHERE id=p_candidate_id AND workspace_id=p_workspace_id;
  IF NOT FOUND THEN RETURN NULL; END IF;
  payload:=convert_from(candidate.canonical_payload,'UTF8')::jsonb;
  RETURN jsonb_build_object('candidate_id',candidate.id,'kind',candidate.candidate_kind,
    'status',candidate.state,'base_workspace_revision_id',candidate.base_workspace_revision_id,
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
  replay:=kb_bid_v2_idempotency_begin(p_actor,'bid.v2.candidate.accept',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  SELECT * INTO STRICT candidate FROM bid_candidate_artifacts
    WHERE id=p_candidate_id AND workspace_id=p_workspace_id FOR UPDATE;
  PERFORM kb_bid_v2_require_project_owner(candidate.project_id,p_actor);
  IF candidate.state='accepted' THEN
    SELECT response_payload INTO STRICT response_bytes FROM bid_candidate_decision_receipts WHERE candidate_id=p_candidate_id;
    PERFORM kb_bid_v2_idempotency_complete(p_actor,'bid.v2.candidate.accept',p_idempotency_key,200,response_bytes);
    RETURN convert_from(response_bytes,'UTF8')::jsonb;
  END IF;
  IF candidate.state<>'proposed' THEN RAISE EXCEPTION 'CANDIDATE_NOT_PROPOSED' USING ERRCODE='23514'; END IF;
  SELECT * INTO STRICT head FROM bid_workspace_heads WHERE scope_id=p_workspace_id FOR UPDATE;
  IF head.artifact_id IS DISTINCT FROM candidate.base_workspace_revision_id OR
     head.artifact_sha256 IS DISTINCT FROM candidate.base_workspace_sha256 THEN
    UPDATE bid_candidate_artifacts SET state='obsolete',decided_at=clock_timestamp() WHERE id=p_candidate_id;
    response:=jsonb_build_object('candidate_id',p_candidate_id,'status','obsolete',
      'error_code','CANDIDATE_OBSOLETE','current_workspace_revision_id',head.artifact_id,
      'current_workspace_sha256',head.artifact_sha256);
    response_bytes:=convert_to(response::text,'UTF8');
    INSERT INTO bid_candidate_decision_receipts(candidate_id,actor,accepted_operation_ordinals,response_payload,response_sha256)
    VALUES(p_candidate_id,p_actor,ARRAY[]::integer[],response_bytes,kb_bid_v2_sha256_bytes(response_bytes));
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
    resulting_workspace_revision_id,response_payload,response_sha256)
  VALUES(p_candidate_id,p_actor,coalesce(p_selected_ordinals,ARRAY[]::integer[]),
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
  replay:=kb_bid_v2_idempotency_begin(p_actor,'bid.v2.candidate.reject',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  SELECT * INTO STRICT candidate FROM bid_candidate_artifacts
    WHERE id=p_candidate_id AND workspace_id=p_workspace_id FOR UPDATE;
  PERFORM kb_bid_v2_require_project_owner(candidate.project_id,p_actor);
  IF candidate.state<>'proposed' THEN RAISE EXCEPTION 'CANDIDATE_NOT_PROPOSED' USING ERRCODE='23514'; END IF;
  UPDATE bid_candidate_artifacts SET state='rejected',decided_at=clock_timestamp() WHERE id=p_candidate_id;
  response:=kb_bid_v2_get_candidate(p_workspace_id,p_candidate_id,p_actor);
  response_bytes:=convert_to(response::text,'UTF8');
  INSERT INTO bid_candidate_decision_receipts(candidate_id,actor,accepted_operation_ordinals,response_payload,response_sha256)
  VALUES(p_candidate_id,p_actor,ARRAY[]::integer[],response_bytes,kb_bid_v2_sha256_bytes(response_bytes));
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
  p_workspace_id uuid,p_expected_artifact_id uuid,p_expected_sha256 kb_sha256,p_actor kb_actor_identity,
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
  SELECT * INTO STRICT head FROM bid_workspace_heads WHERE scope_id=p_workspace_id;before_generation:=head.generation;
  IF EXISTS (SELECT 1 FROM bid_workspace_revision_artifacts revision WHERE revision.id=head.artifact_id
    AND revision.requirement_projection_id=current_value.artifact_id
    AND revision.requirement_projection_sha256=current_value.artifact_sha256) THEN
    response:=kb_bid_v2_load_workspace(p_workspace_id);
  ELSE
    PERFORM kb_bid_v2_advance_workspace_projection(p_workspace_id,
      (SELECT requirement_projection_id FROM bid_workspace_revision_artifacts WHERE id=head.artifact_id),
      (SELECT requirement_projection_sha256 FROM bid_workspace_revision_artifacts WHERE id=head.artifact_id),
      current_value.artifact_id,current_value.artifact_sha256);
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
    'byte_length',a.byte_length,'object_ref',a.object_ref,'content_sha256',a.content_sha256
  ) ORDER BY a.created_at,a.id) FROM bid_workspace_asset_artifacts a
    WHERE a.workspace_id=p_workspace_id AND NOT EXISTS (
      SELECT 1 FROM bid_workspace_asset_retirement_artifacts retirement WHERE retirement.asset_revision_id=a.id)),'[]'::jsonb);
END $$;

CREATE FUNCTION kb_bid_v2_upload_workspace_asset(
  p_workspace_id uuid,p_asset_id uuid,p_staging_id uuid,p_file_name text,
  p_media_type text,p_byte_length bigint,p_object_ref kb_object_ref,p_content_sha256 kb_sha256,
  p_actor kb_actor_identity,p_idempotency_key text,p_request_bytes bytea,p_request_sha256 kb_sha256
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE project_id uuid; replay bytea; response jsonb; response_bytes bytea;
BEGIN
  replay:=kb_bid_v2_idempotency_begin(p_actor,'bid.v2.workspace.asset.upload',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  SELECT value.project_id INTO project_id FROM bid_submission_workspaces value WHERE value.id=p_workspace_id FOR SHARE;
  IF NOT FOUND THEN RAISE EXCEPTION 'WORKSPACE_NOT_FOUND' USING ERRCODE='P0002'; END IF;
  PERFORM kb_bid_v2_require_project_owner(project_id,p_actor);
  IF p_byte_length<=0 OR octet_length(p_file_name) NOT BETWEEN 1 AND 1024 THEN
    RAISE EXCEPTION 'ASSET_METADATA_INVALID' USING ERRCODE='23514';
  END IF;
  INSERT INTO bid_workspace_asset_artifacts(
    id,project_id,workspace_id,object_ref,content_sha256,media_type,file_name,byte_length,source,created_by)
  VALUES(p_asset_id,project_id,p_workspace_id,p_object_ref,p_content_sha256,p_media_type,
    p_file_name,p_byte_length,'human_upload',p_actor);
  PERFORM kb_object_upload_commit(p_staging_id,p_object_ref,p_content_sha256,p_media_type,p_byte_length,
    'bid_workspace_asset',p_asset_id,'payload',p_actor);
  response:=jsonb_build_object('asset_revision_id',p_asset_id,'media_type',p_media_type,
    'file_name',p_file_name,'byte_length',p_byte_length,'object_ref',p_object_ref,'content_sha256',p_content_sha256);
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

CREATE FUNCTION kb_bid_v2_publish_pdf_attachment_preparation(
  p_request_artifact_id uuid,p_request_revision bigint,p_frozen_input_sha256 kb_sha256,
  p_source_asset_revision_id uuid,p_preparation_id uuid,p_page_item_ids uuid[],p_staging_ids uuid[],
  p_object_refs kb_object_ref[],p_content_sha256s kb_sha256[],p_media_types text[],p_byte_lengths bigint[],
  p_widths_px integer[],p_heights_px integer[],p_actor kb_actor_identity
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE typed bid_submission_export_request_identities%ROWTYPE; request_value bid_async_request_snapshot_artifacts%ROWTYPE;
  source_asset bid_workspace_asset_artifacts%ROWTYPE; existing record; next_revision bigint;
  page_assets jsonb:='[]'::jsonb; payload jsonb; preparation_sha kb_sha256; contract_sha kb_sha256;
  position integer; response jsonb; item_count integer:=cardinality(coalesce(p_page_item_ids,ARRAY[]::uuid[]));
BEGIN
  IF p_actor<>'system:submission-export-v2' THEN
    RAISE EXCEPTION 'SYSTEM_ACTOR_REQUIRED' USING ERRCODE='42501';
  END IF;
  SELECT * INTO STRICT typed FROM bid_submission_export_request_identities
    WHERE request_artifact_id=p_request_artifact_id AND request_revision=p_request_revision
      AND frozen_input_sha256=p_frozen_input_sha256;
  SELECT * INTO STRICT request_value FROM bid_async_request_snapshot_artifacts
    WHERE id=p_request_artifact_id AND status='pending' FOR UPDATE;
  SELECT preparation.*,attestation.preparation_sha256 attested_sha INTO existing
    FROM bid_pdf_attachment_preparation_attestations attestation
    JOIN bid_attachment_preparation_revision_artifacts preparation
      ON preparation.id=attestation.preparation_revision_id
    WHERE attestation.request_artifact_id=p_request_artifact_id
      AND attestation.source_asset_revision_id=p_source_asset_revision_id;
  IF FOUND THEN
    RETURN existing.canonical_payload||jsonb_build_object('replayed',true);
  END IF;
  SELECT * INTO STRICT source_asset FROM bid_workspace_asset_artifacts
    WHERE project_id=typed.project_id AND workspace_id=typed.workspace_id
      AND id=p_source_asset_revision_id AND media_type='application/pdf'
      AND NOT EXISTS (SELECT 1 FROM bid_workspace_asset_retirement_artifacts retired
        WHERE retired.asset_revision_id=bid_workspace_asset_artifacts.id) FOR SHARE;
  IF NOT EXISTS (SELECT 1 FROM bid_workspace_block_occurrences occurrence
      JOIN bid_content_block_revision_artifacts block ON block.id=occurrence.block_revision_id
      WHERE occurrence.workspace_revision_id=typed.workspace_revision_id
        AND block.block_kind='attachment_ref' AND block.block_payload->>'render_mode'='embedded_pages'
        AND (block.block_payload->>'asset_revision_id')::uuid=source_asset.id) THEN
    RAISE EXCEPTION 'FROZEN_PDF_ATTACHMENT_NOT_FOUND' USING ERRCODE='23514';
  END IF;
  IF item_count NOT BETWEEN 1 AND 10000
     OR item_count<>cardinality(p_staging_ids) OR item_count<>cardinality(p_object_refs)
     OR item_count<>cardinality(p_content_sha256s) OR item_count<>cardinality(p_media_types)
     OR item_count<>cardinality(p_byte_lengths) OR item_count<>cardinality(p_widths_px)
     OR item_count<>cardinality(p_heights_px)
     OR item_count<>(SELECT count(DISTINCT id) FROM unnest(p_page_item_ids) id)
     OR EXISTS (SELECT 1 FROM generate_subscripts(p_page_item_ids,1) item
       WHERE p_object_refs[item] IS DISTINCT FROM 'objects/'||p_content_sha256s[item]
         OR p_media_types[item] IS DISTINCT FROM 'image/png'
         OR p_byte_lengths[item]<=0 OR p_widths_px[item]<=0 OR p_heights_px[item]<=0)
  THEN RAISE EXCEPTION 'PDF_ATTACHMENT_PAGE_SET_INVALID' USING ERRCODE='23514'; END IF;
  SELECT content_sha256 INTO STRICT contract_sha FROM bid_attachment_preparation_contract_artifacts
    WHERE id='00000000-0000-5000-8000-000000000305';
  SELECT coalesce(max(revision),0)+1 INTO next_revision FROM bid_attachment_preparation_revision_artifacts
    WHERE workspace_id=typed.workspace_id AND source_asset_revision_id=source_asset.id;
  FOR position IN SELECT generate_subscripts(p_page_item_ids,1) LOOP
    PERFORM kb_object_upload_commit(p_staging_ids[position],p_object_refs[position],p_content_sha256s[position],
      p_media_types[position],p_byte_lengths[position],'bid_attachment_preparation',p_preparation_id,
      'page:'||position,p_actor);
    page_assets:=page_assets||jsonb_build_array(jsonb_build_object('page_asset_id',p_page_item_ids[position],
      'page_number',position,'object_ref',p_object_refs[position],'sha256',p_content_sha256s[position],
      'media_type',p_media_types[position],
      'geometry',jsonb_build_object('width_px',p_widths_px[position],'height_px',p_heights_px[position])));
  END LOOP;
  payload:=jsonb_build_object('schema_version',1,'attachment_preparation_revision_id',p_preparation_id,
    'project_id',typed.project_id,'workspace_id',typed.workspace_id,'source_asset_revision_id',source_asset.id,
    'revision',next_revision,'status','ready','page_assets',page_assets);
  preparation_sha:=kb_bid_v2_sha256_bytes(convert_to(payload::text,'UTF8'));
  payload:=payload||jsonb_build_object('preparation_sha256',preparation_sha);
  INSERT INTO bid_attachment_preparation_revision_artifacts(id,project_id,workspace_id,source_asset_revision_id,
    revision,status,page_assets,canonical_payload,preparation_sha256)
  VALUES(p_preparation_id,typed.project_id,typed.workspace_id,source_asset.id,next_revision,'ready',
    page_assets,payload,preparation_sha);
  FOR position IN SELECT generate_subscripts(p_page_item_ids,1) LOOP
    INSERT INTO bid_attachment_preparation_asset_items(id,project_id,workspace_id,
      attachment_preparation_revision_id,ordinal,page_number,object_ref,content_sha256,media_type,geometry)
    VALUES(p_page_item_ids[position],typed.project_id,typed.workspace_id,p_preparation_id,position-1,position,
      p_object_refs[position],p_content_sha256s[position],p_media_types[position],
      jsonb_build_object('width_px',p_widths_px[position],'height_px',p_heights_px[position]));
  END LOOP;
  INSERT INTO bid_pdf_attachment_preparation_attestations(preparation_revision_id,project_id,workspace_id,
    request_artifact_id,request_revision,frozen_input_sha256,source_asset_revision_id,source_object_ref,source_sha256,
    source_media_type,preparation_sha256,contract_id,contract_sha256)
  VALUES(p_preparation_id,typed.project_id,typed.workspace_id,p_request_artifact_id,p_request_revision,p_frozen_input_sha256,
    source_asset.id,source_asset.object_ref,source_asset.content_sha256,source_asset.media_type,preparation_sha,
    '00000000-0000-5000-8000-000000000305',contract_sha);
  SET CONSTRAINTS ALL IMMEDIATE;
  response:=payload||jsonb_build_object('replayed',false);
  RETURN response;
END $$;

CREATE FUNCTION kb_bid_v2_resolve_export_attachment_preparation(
  p_request_artifact_id uuid,p_workspace_revision_id uuid,p_block_revision_id uuid
) RETURNS uuid LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE block_value bid_content_block_revision_artifacts%ROWTYPE; source_asset bid_workspace_asset_artifacts%ROWTYPE;
  result uuid;
BEGIN
  SELECT block.* INTO STRICT block_value FROM bid_workspace_block_occurrences occurrence
    JOIN bid_content_block_revision_artifacts block ON block.id=occurrence.block_revision_id
    WHERE occurrence.workspace_revision_id=p_workspace_revision_id
      AND occurrence.block_revision_id=p_block_revision_id AND block.block_kind='attachment_ref';
  SELECT * INTO STRICT source_asset FROM bid_workspace_asset_artifacts
    WHERE id=(block_value.block_payload->>'asset_revision_id')::uuid
      AND workspace_id=block_value.workspace_id;
  IF source_asset.media_type='application/pdf' THEN
    SELECT attestation.preparation_revision_id INTO result
      FROM bid_pdf_attachment_preparation_attestations attestation
      JOIN bid_attachment_preparation_revision_artifacts preparation
        ON preparation.id=attestation.preparation_revision_id AND preparation.status='ready'
      WHERE attestation.request_artifact_id=p_request_artifact_id
        AND attestation.source_asset_revision_id=source_asset.id;
  ELSE
    result:=nullif(block_value.block_payload->>'preparation_revision_id','')::uuid;
  END IF;
  RETURN result;
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
  ('00000000-0000-5000-8000-000000000202'::uuid,'prompt',convert_to('{"kind":"content_prompt","version":1}','UTF8')),
  ('00000000-0000-5000-8000-000000000203'::uuid,'template',convert_to('{"kind":"content_template","version":1}','UTF8')),
  ('00000000-0000-5000-8000-000000000204'::uuid,'model',convert_to('{"kind":"configured_chat_model","operation":"content","version":1}','UTF8')),
  ('00000000-0000-5000-8000-000000000205'::uuid,'agent',convert_to('{"kind":"content_agent","version":1}','UTF8'))
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

CREATE FUNCTION kb_bid_v2_create_content_request(
  p_workspace_id uuid,p_expected_revision_id uuid,p_expected_sha256 kb_sha256,
  p_request_operation text,p_target_kind text,p_target_node_lineage_id uuid,p_fill_policy text,
  p_insertion_anchor jsonb,p_evidence_selection_mode text,p_pick_set_artifact_id uuid,
  p_actor kb_actor_identity,p_idempotency_key text,p_request_bytes bytea,p_request_sha256 kb_sha256
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE workspace bid_submission_workspaces%ROWTYPE; head bid_workspace_heads%ROWTYPE;
  revision bid_workspace_revision_artifacts%ROWTYPE; checkpoint bid_outline_checkpoint_artifacts%ROWTYPE;
  settings bid_document_settings_revision_artifacts%ROWTYPE; scope bid_workspace_scope_revision_artifacts%ROWTYPE;
  projection bid_workspace_requirement_projection_artifacts%ROWTYPE;
  target_node bid_outline_node_revision_artifacts%ROWTYPE; pick_set bid_evidence_selection_artifacts%ROWTYPE;
  request_id uuid:=gen_random_uuid(); frozen_payload bytea; frozen_sha kb_sha256;
  selection_sha kb_sha256; replay bytea; response jsonb; response_bytes bytea; operation_name text;
  matching_sha kb_sha256; prompt_sha kb_sha256; template_sha kb_sha256; model_sha kb_sha256; agent_sha kb_sha256;
  style_sha kb_sha256; quote_id uuid; quote_sha kb_sha256;
BEGIN
  IF p_request_operation NOT IN ('match_only','generate') OR p_target_kind NOT IN ('node','subtree','workspace')
     OR p_fill_policy NOT IN ('empty_only','append_candidate','missing_requirements_only')
     OR p_evidence_selection_mode NOT IN ('system_proposed','user_pick_set') THEN
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
    'fill_policy',p_fill_policy,'insertion_anchor',p_insertion_anchor,'evidence_selection_sha256',selection_sha));
  frozen_sha:=kb_bid_v2_sha256_bytes(frozen_payload);
  INSERT INTO bid_async_request_snapshot_artifacts(id,project_id,workspace_id,request_kind,revision,
    frozen_input_sha256,request_payload,request_sha256,status)
  VALUES(request_id,workspace.project_id,p_workspace_id,'content_generate',1,frozen_sha,p_request_bytes,p_request_sha256,'pending');
  INSERT INTO bid_content_generation_request_identities(
    request_artifact_id,project_id,workspace_id,request_revision,request_sha256,frozen_input_sha256,
    request_operation,base_workspace_revision_id,base_workspace_sha256,
    requirement_projection_id,requirement_projection_sha256,outline_checkpoint_id,outline_checkpoint_sha256,
    scope_revision_id,scope_revision_sha256,document_settings_revision_id,document_settings_sha256,
    render_style_contract_id,render_style_contract_sha256,evidence_selection_mode,evidence_selection_sha256,
    pick_set_kind,pick_set_artifact_id,pick_set_sha256,pick_set_matching_report_id,
    matching_policy_id,matching_policy_sha256,quote_snapshot_id,quote_snapshot_sha256,
    prompt_contract_id,prompt_contract_sha256,template_contract_id,template_contract_sha256,model_contract_id,model_contract_sha256,
    agent_contract_id,agent_contract_sha256,target_kind,target_node_lineage_id,target_node_revision_id,
    target_workspace_revision_id,fill_policy,insertion_node_revision_id,insertion_block_revision_id,insertion_utf8_offset)
  VALUES(request_id,workspace.project_id,p_workspace_id,1,p_request_sha256,frozen_sha,p_request_operation,
    revision.id,revision.content_sha256,projection.id,projection.content_sha256,checkpoint.id,checkpoint.content_sha256,
    revision.scope_revision_id,scope.content_sha256,settings.id,settings.content_sha256,
    '00000000-0000-5000-8000-000000000301',style_sha,p_evidence_selection_mode,selection_sha,
    CASE WHEN p_evidence_selection_mode='user_pick_set' THEN 'user_pick_set' END,
    CASE WHEN p_evidence_selection_mode='user_pick_set' THEN pick_set.id END,
    CASE WHEN p_evidence_selection_mode='user_pick_set' THEN pick_set.content_sha256 END,
    CASE WHEN p_evidence_selection_mode='user_pick_set' THEN pick_set.matching_report_id END,
    CASE WHEN p_evidence_selection_mode='system_proposed' THEN '00000000-0000-5000-8000-000000000201'::uuid END,
    CASE WHEN p_evidence_selection_mode='system_proposed' THEN matching_sha END,
    quote_id,quote_sha,'00000000-0000-5000-8000-000000000202',prompt_sha,'00000000-0000-5000-8000-000000000203',template_sha,
    '00000000-0000-5000-8000-000000000204',model_sha,'00000000-0000-5000-8000-000000000205',agent_sha,
    p_target_kind,CASE WHEN p_target_kind IN ('node','subtree') THEN target_node.lineage_id END,
    CASE WHEN p_target_kind IN ('node','subtree') THEN target_node.id END,
    CASE WHEN p_target_kind='workspace' THEN revision.id END,p_fill_policy,
    CASE WHEN p_insertion_anchor IS NOT NULL THEN (p_insertion_anchor->>'node_revision_id')::uuid END,
    CASE WHEN p_insertion_anchor IS NOT NULL AND jsonb_typeof(p_insertion_anchor->'block_revision_id')<>'null' THEN (p_insertion_anchor->>'block_revision_id')::uuid END,
    CASE WHEN p_insertion_anchor IS NOT NULL AND jsonb_typeof(p_insertion_anchor->'utf8_offset')<>'null' THEN (p_insertion_anchor->>'utf8_offset')::bigint END);
  response:=jsonb_build_object('request_artifact_id',request_id,'kind','ContentGenerate','status','pending',
    'result_identity',NULL,'error_code',NULL,'request_revision',1,'request_sha256',p_request_sha256,
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
    'quote_snapshot',CASE WHEN typed.quote_snapshot_id IS NULL THEN NULL ELSE (
      SELECT convert_from(quote.canonical_payload,'UTF8')::jsonb||jsonb_build_object(
        'artifact_id',quote.id,'sha256',quote.content_sha256)
      FROM bid_quote_snapshot_artifacts quote WHERE quote.project_id=typed.project_id
        AND quote.id=typed.quote_snapshot_id AND quote.content_sha256=typed.quote_snapshot_sha256) END,
    'insertion_anchor',CASE WHEN typed.insertion_node_revision_id IS NULL THEN NULL ELSE jsonb_build_object(
      'node_revision_id',typed.insertion_node_revision_id,'block_revision_id',typed.insertion_block_revision_id,
      'utf8_offset',typed.insertion_utf8_offset) END,
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
            AND evidence.workspace_revision_id=typed.base_workspace_revision_id AND evidence.state='current'
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
  p_candidate_id uuid,p_candidate_payload bytea,p_candidate_sha256 kb_sha256,p_operations jsonb
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE typed bid_content_generation_request_identities%ROWTYPE; request_value bid_async_request_snapshot_artifacts%ROWTYPE;
  requirement record; requirement_match jsonb; evidence_item jsonb; evidence_items jsonb;
  report_id uuid; report_payload bytea; report_sha kb_sha256; bundle_id uuid; item_id uuid;
  selection_id uuid; selection_json jsonb; selection_payload bytea; selection_sha kb_sha256;
  item_payload jsonb; bundle_bare jsonb; bundle_payload jsonb; bundle_sha kb_sha256; created timestamptz;
  candidate_json jsonb; ordinal_value integer:=0; item_ordinal integer; operation_value jsonb; published_identity jsonb; result_sha kb_sha256;
BEGIN
  SELECT * INTO STRICT typed FROM bid_content_generation_request_identities
    WHERE request_artifact_id=p_request_artifact_id AND request_revision=p_request_revision
      AND frozen_input_sha256=p_frozen_input_sha256 FOR UPDATE;
  SELECT * INTO STRICT request_value FROM bid_async_request_snapshot_artifacts WHERE id=p_request_artifact_id FOR UPDATE;
  IF request_value.status='succeeded' THEN RETURN request_value.result_identity; END IF;
  IF request_value.status<>'pending' THEN RAISE EXCEPTION 'REQUEST_NOT_PENDING' USING ERRCODE='23514'; END IF;
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
          media_type,file_name,byte_length,source,created_by)
        SELECT (item_payload->>'evidence_item_id')::uuid,typed.project_id,typed.workspace_id,
          registry.object_ref,registry.digest,registry.media_type,item_payload->>'frozen_document_display_name',
          registry.byte_length,'ai_evidence','system:content-generate-v2'
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
  UPDATE bid_async_request_snapshot_artifacts SET status='succeeded',result_identity=published_identity,
    finished_at=clock_timestamp() WHERE id=p_request_artifact_id;
  RETURN published_identity;
END $$;

CREATE FUNCTION kb_bid_v2_mark_content_generation_failed(
  p_request_artifact_id uuid,p_request_revision bigint,p_frozen_input_sha256 kb_sha256,p_error_code text
) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
BEGIN
  PERFORM 1 FROM bid_content_generation_request_identities
    WHERE request_artifact_id=p_request_artifact_id AND request_revision=p_request_revision
      AND frozen_input_sha256=p_frozen_input_sha256;
  IF NOT FOUND THEN RAISE EXCEPTION 'FROZEN_INPUT_MISSING' USING ERRCODE='P0002'; END IF;
  UPDATE bid_async_request_snapshot_artifacts SET status='failed',
    error_code=CASE WHEN p_error_code IN ('INPUT_SCHEMA_INVALID','FROZEN_INPUT_MISSING','FROZEN_INPUT_DIGEST_MISMATCH',
      'REQUEST_OBSOLETE','WORKSPACE_CAS_CONFLICT','AGENT_OUTPUT_INVALID','EVIDENCE_UNAVAILABLE')
      THEN p_error_code ELSE 'AGENT_OUTPUT_INVALID' END,
    finished_at=clock_timestamp()
  WHERE id=p_request_artifact_id AND status='pending';
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
  p_project_id uuid,p_quote_snapshot_id uuid,p_quote_snapshot_sha256 kb_sha256,p_actor kb_actor_identity
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE workspace bid_submission_workspaces%ROWTYPE; head bid_workspace_heads%ROWTYPE;
  old_revision bid_workspace_revision_artifacts%ROWTYPE; new_revision_id uuid:=gen_random_uuid();
  new_revision bigint; payload bytea; digest kb_sha256; node record; block record; binding record;
  edge record; evidence record; checkpoint record; node_map jsonb:='{}'::jsonb; new_occurrence_id uuid;
  new_parent_id uuid; evidence_revision bigint; evidence_payload bytea; evidence_sha kb_sha256;
  checkpoint_payload bytea; checkpoint_sha kb_sha256;
BEGIN
  SELECT * INTO STRICT workspace FROM bid_submission_workspaces WHERE project_id=p_project_id;
  SELECT * INTO STRICT head FROM bid_workspace_heads WHERE scope_id=workspace.id FOR UPDATE;
  SELECT * INTO STRICT old_revision FROM bid_workspace_revision_artifacts
    WHERE id=head.artifact_id AND content_sha256=head.artifact_sha256;
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
  FOR evidence IN SELECT * FROM bid_submission_fulfillment_evidence_revision_artifacts
    WHERE workspace_revision_id=old_revision.id ORDER BY evidence_lineage_id
  LOOP
    SELECT coalesce(max(revision),0)+1 INTO evidence_revision
      FROM bid_submission_fulfillment_evidence_revision_artifacts
      WHERE project_id=p_project_id AND evidence_lineage_id=evidence.evidence_lineage_id;
    evidence_payload:=kb_bid_v2_json_payload(jsonb_build_object('schema_version',1,
      'evidence_lineage_id',evidence.evidence_lineage_id,'revision',evidence_revision,
      'workspace_revision_id',new_revision_id,'binding_revision_id',evidence.binding_revision_id,
      'target_revision_id',evidence.target_revision_id,'target_kind',evidence.target_kind,
      'state','stale','dependency_sha256',evidence.dependency_sha256));
    evidence_sha:=kb_bid_v2_sha256_bytes(evidence_payload);
    INSERT INTO bid_submission_fulfillment_evidence_revision_artifacts(id,project_id,workspace_id,
      evidence_lineage_id,revision,workspace_revision_id,binding_revision_id,target_revision_id,target_kind,
      dependency_sha256,state,canonical_payload,content_sha256)
    VALUES(gen_random_uuid(),p_project_id,workspace.id,evidence.evidence_lineage_id,evidence_revision,
      new_revision_id,evidence.binding_revision_id,evidence.target_revision_id,evidence.target_kind,
      evidence.dependency_sha256,'stale',evidence_payload,evidence_sha);
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
  UPDATE bid_candidate_artifacts SET state='obsolete',decided_at=clock_timestamp()
    WHERE workspace_id=workspace.id AND base_workspace_revision_id=old_revision.id AND state='proposed';
  UPDATE bid_async_request_snapshot_artifacts request_value SET status='obsolete',finished_at=clock_timestamp()
    WHERE request_value.status='pending' AND request_value.id IN (
      SELECT request_artifact_id FROM bid_outline_generation_request_identities
        WHERE workspace_id=workspace.id AND base_workspace_revision_id=old_revision.id
      UNION ALL
      SELECT request_artifact_id FROM bid_content_generation_request_identities
        WHERE workspace_id=workspace.id AND base_workspace_revision_id=old_revision.id);
  RETURN jsonb_build_object('revision_id',new_revision_id,'sha256',digest,'replayed',false);
END $$;

CREATE FUNCTION kb_bid_v2_publish_quote_snapshot(
  p_project_id uuid,p_snapshot_id uuid,p_expected_revision bigint,p_staging_id uuid,
  p_object_ref kb_object_ref,p_content_sha256 kb_sha256,p_byte_length bigint,p_canonical_payload bytea,
  p_actor kb_actor_identity,p_idempotency_key text,p_request_bytes bytea,p_request_sha256 kb_sha256
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE replay bytea; response jsonb; response_bytes bytea; project_status text; actual_revision bigint;
  quote_id uuid:=kb_bid_v2_deterministic_uuid('quote:'||p_project_id::text);payload jsonb; workspace_receipt jsonb;
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
  workspace_receipt:=kb_bid_v2_advance_workspace_quote(p_project_id,p_snapshot_id,p_content_sha256,p_actor);
  response:=jsonb_build_object('quote_snapshot_id',p_snapshot_id,'quote_id',quote_id,'revision',p_expected_revision,
    'currency','CNY','sha256',p_content_sha256,'object_ref',p_object_ref,'byte_length',p_byte_length,
    'workspace_revision',workspace_receipt);
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
            AND evidence.workspace_revision_id=revision.id AND evidence.state='current'
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
            AND evidence.workspace_revision_id=revision.id AND evidence.state='current'
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
            ON evidence.binding_revision_id=binding.id AND evidence.workspace_revision_id=revision.id
          WHERE occurrence.workspace_revision_id=revision.id AND binding.state='bound'
            AND binding.requirement_projection_id=revision.requirement_projection_id
            AND binding.need_occurrence_id=ANY(kb_bid_v2_fulfillment_need_ids(requirement.fulfillment_expr))
            AND evidence.state='current')) THEN
    submission_issues:=submission_issues||jsonb_build_array(jsonb_build_object(
      'issue_id',kb_bid_v2_deterministic_uuid(input_sha||':FULFILLMENT_EVIDENCE_STALE_OR_MISSING'),
      'code','FULFILLMENT_EVIDENCE_STALE_OR_MISSING','severity','warning',
      'message','部分必选要求的履约证据缺失或已过期，请复核'));
  END IF;
  IF EXISTS (SELECT 1 FROM bid_workspace_block_occurrences occurrence
      JOIN bid_content_block_revision_artifacts block ON block.project_id=occurrence.project_id
        AND block.id=occurrence.block_revision_id
      WHERE occurrence.workspace_revision_id=revision.id AND block.stale) THEN
    submission_issues:=submission_issues||jsonb_build_array(jsonb_build_object('issue_id',kb_bid_v2_deterministic_uuid(input_sha||':STALE_CONTENT'),
      'code','STALE_CONTENT','severity','warning','message','存在依赖已变化的内容块，请复核'));
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
          AND ((block.block_payload->>'preparation_revision_id')::uuid=preparation.id
            OR ((block.block_payload->>'preparation_revision_id') IS NULL AND preparation.id=(
              SELECT attestation.preparation_revision_id
              FROM bid_pdf_attachment_preparation_attestations attestation
              JOIN bid_submission_export_request_identities request_value
                ON request_value.request_artifact_id=attestation.request_artifact_id
              WHERE request_value.workspace_revision_id=revision_id
                AND attestation.source_asset_revision_id=(block.block_payload->>'asset_revision_id')::uuid
              ORDER BY attestation.created_at DESC,attestation.preparation_revision_id DESC LIMIT 1)))
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
          AND ((block.block_payload->>'preparation_revision_id')::uuid=preparation.id
            OR ((block.block_payload->>'preparation_revision_id') IS NULL AND preparation.id=(
              SELECT attestation.preparation_revision_id
              FROM bid_pdf_attachment_preparation_attestations attestation
              JOIN bid_submission_export_request_identities request_value
                ON request_value.request_artifact_id=attestation.request_artifact_id
              WHERE request_value.workspace_revision_id=revision_id
                AND attestation.source_asset_revision_id=(block.block_payload->>'asset_revision_id')::uuid
              ORDER BY attestation.created_at DESC,attestation.preparation_revision_id DESC LIMIT 1))))),'[]'::jsonb));
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

CREATE FUNCTION kb_bid_v2_create_submission_export_request(
  p_workspace_id uuid,p_expected_revision_id uuid,p_expected_sha256 kb_sha256,
  p_output_mode text,p_format text,p_mode_options jsonb,p_actor kb_actor_identity,
  p_idempotency_key text,p_request_bytes bytea,p_request_sha256 kb_sha256
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE workspace bid_submission_workspaces%ROWTYPE; head bid_workspace_heads%ROWTYPE;
  revision bid_workspace_revision_artifacts%ROWTYPE; checkpoint bid_outline_checkpoint_artifacts%ROWTYPE;
  scope bid_workspace_scope_revision_artifacts%ROWTYPE; settings bid_document_settings_revision_artifacts%ROWTYPE;
  assessment bid_submission_assessment_snapshot_artifacts%ROWTYPE;
  style_sha kb_sha256; request_id uuid:=gen_random_uuid(); frozen_payload bytea; frozen_sha kb_sha256;
  checkpoint_payload bytea; checkpoint_sha kb_sha256; replay bytea; response jsonb; response_bytes bytea;
BEGIN
  SELECT * INTO STRICT workspace FROM bid_submission_workspaces WHERE id=p_workspace_id;
  PERFORM kb_bid_v2_require_project_owner(workspace.project_id,p_actor);
  replay:=kb_bid_v2_idempotency_begin(p_actor,'bid.v2.submission-export.create',p_idempotency_key,p_request_bytes,p_request_sha256);
  IF replay IS NOT NULL THEN RETURN convert_from(replay,'UTF8')::jsonb; END IF;
  IF p_output_mode NOT IN ('review_draft','submission') OR p_format NOT IN ('docx','pdf') OR
     NOT kb_bid_v2_json_keys_exact(p_mode_options,ARRAY['watermark','include_assessment_notices','include_knowledge_sources']) THEN
    RAISE EXCEPTION 'SUBMISSION_EXPORT_OPTIONS_INVALID' USING ERRCODE='23514';
  END IF;
  IF p_output_mode='submission' AND p_mode_options IS DISTINCT FROM
      '{"watermark":null,"include_assessment_notices":false,"include_knowledge_sources":false}'::jsonb THEN
    RAISE EXCEPTION 'SUBMISSION_EXPORT_OPTIONS_INVALID' USING ERRCODE='23514';
  END IF;
  SELECT * INTO STRICT head FROM bid_workspace_heads WHERE scope_id=p_workspace_id FOR UPDATE;
  IF head.artifact_id IS DISTINCT FROM p_expected_revision_id OR head.artifact_sha256 IS DISTINCT FROM p_expected_sha256 THEN
    RAISE EXCEPTION 'WORKSPACE_HEAD_CAS_MISMATCH' USING ERRCODE='40001';
  END IF;
  SELECT * INTO STRICT revision FROM bid_workspace_revision_artifacts WHERE id=head.artifact_id;
  SELECT * INTO checkpoint FROM bid_outline_checkpoint_artifacts
    WHERE workspace_id=p_workspace_id AND workspace_revision_id=revision.id
      AND requirement_projection_id=revision.requirement_projection_id
      AND requirement_projection_sha256=revision.requirement_projection_sha256
    ORDER BY created_at DESC,id DESC LIMIT 1;
  IF NOT FOUND THEN
    checkpoint_payload:=kb_bid_v2_json_payload(jsonb_build_object('schema_version',1,'workspace_id',p_workspace_id,
      'workspace_revision_id',revision.id,'workspace_sha256',revision.content_sha256,
      'requirement_projection_id',revision.requirement_projection_id,
      'requirement_projection_sha256',revision.requirement_projection_sha256));
    checkpoint_sha:=kb_bid_v2_sha256_bytes(checkpoint_payload);
    INSERT INTO bid_outline_checkpoint_artifacts(id,project_id,workspace_id,workspace_revision_id,
      requirement_projection_id,requirement_projection_sha256,canonical_payload,content_sha256,actor)
    VALUES(kb_bid_v2_deterministic_uuid('submission-export-checkpoint:'||revision.id::text),workspace.project_id,
      p_workspace_id,revision.id,revision.requirement_projection_id,revision.requirement_projection_sha256,
      checkpoint_payload,checkpoint_sha,p_actor)
    RETURNING * INTO checkpoint;
  END IF;
  SELECT * INTO STRICT scope FROM bid_workspace_scope_revision_artifacts WHERE id=revision.scope_revision_id;
  SELECT * INTO STRICT settings FROM bid_document_settings_revision_artifacts WHERE id=revision.document_settings_revision_id;
  SELECT content_sha256 INTO STRICT style_sha FROM bid_render_style_contract_artifacts
    WHERE id='00000000-0000-5000-8000-000000000301';
  frozen_payload:=kb_bid_v2_json_payload(jsonb_build_object('workspace_revision_id',revision.id,
    'workspace_sha256',revision.content_sha256,'outline_checkpoint_id',checkpoint.id,
    'outline_checkpoint_sha256',checkpoint.content_sha256,'requirement_projection_id',revision.requirement_projection_id,
    'requirement_projection_sha256',revision.requirement_projection_sha256,'scope_revision_id',scope.id,
    'scope_revision_sha256',scope.content_sha256,'document_settings_revision_id',settings.id,
    'document_settings_sha256',settings.content_sha256,'render_style_contract_id','00000000-0000-5000-8000-000000000301',
    'render_style_contract_sha256',style_sha,'output_mode',p_output_mode,'format',p_format,'mode_options',p_mode_options));
  frozen_sha:=kb_bid_v2_sha256_bytes(frozen_payload);
  INSERT INTO bid_async_request_snapshot_artifacts(id,project_id,workspace_id,request_kind,revision,
    frozen_input_sha256,request_payload,request_sha256,status)
  VALUES(request_id,workspace.project_id,p_workspace_id,'submission_export',1,frozen_sha,p_request_bytes,p_request_sha256,'pending');
  INSERT INTO bid_submission_export_request_identities(request_artifact_id,project_id,workspace_id,request_revision,
    request_sha256,frozen_input_sha256,workspace_revision_id,workspace_sha256,outline_checkpoint_id,
    outline_checkpoint_sha256,requirement_projection_id,requirement_projection_sha256,scope_revision_id,
    scope_revision_sha256,document_settings_revision_id,document_settings_sha256,render_style_contract_id,
    render_style_contract_sha256,output_mode,format,mode_options)
  VALUES(request_id,workspace.project_id,p_workspace_id,1,p_request_sha256,frozen_sha,revision.id,revision.content_sha256,
    checkpoint.id,checkpoint.content_sha256,revision.requirement_projection_id,revision.requirement_projection_sha256,
    scope.id,scope.content_sha256,settings.id,settings.content_sha256,'00000000-0000-5000-8000-000000000301',
    style_sha,p_output_mode,p_format,p_mode_options);
  PERFORM kb_bid_v2_get_current_assessments(p_workspace_id,p_actor);
  SELECT * INTO STRICT assessment FROM bid_submission_assessment_snapshot_artifacts
    WHERE workspace_id=p_workspace_id AND workspace_revision_id=revision.id
    ORDER BY created_at DESC,id DESC LIMIT 1;
  INSERT INTO bid_async_stage_receipts(request_artifact_id,stage_kind,frozen_input_sha256,result_identity,result_sha256)
  VALUES(request_id,'assessment',frozen_sha,
    jsonb_build_object('artifact_id',assessment.id,'sha256',assessment.content_sha256),assessment.content_sha256);
  response:=jsonb_build_object('request_artifact_id',request_id,'kind','SubmissionExport','status','pending',
    'result_identity',NULL,'error_code',NULL,'request_revision',1,'request_sha256',p_request_sha256,
    'frozen_input_sha256',frozen_sha,'project_id',workspace.project_id,'workspace_id',p_workspace_id,
    'base_workspace_revision_id',revision.id);
  response_bytes:=convert_to(response::text,'UTF8');
  PERFORM kb_bid_v2_idempotency_complete(p_actor,'bid.v2.submission-export.create',p_idempotency_key,202,response_bytes);
  RETURN response;
END $$;

CREATE FUNCTION kb_bid_v2_load_submission_export_input(
  p_request_artifact_id uuid,p_request_revision bigint,p_frozen_input_sha256 kb_sha256
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE typed bid_submission_export_request_identities%ROWTYPE; owner_actor kb_actor_identity;
BEGIN
  SELECT * INTO STRICT typed FROM bid_submission_export_request_identities
    WHERE request_artifact_id=p_request_artifact_id AND request_revision=p_request_revision
      AND frozen_input_sha256=p_frozen_input_sha256;
  SELECT ('user:'||owner_user_id)::kb_actor_identity INTO STRICT owner_actor
    FROM bid_projects WHERE id=typed.project_id;
  RETURN jsonb_build_object('request',to_jsonb(typed),
    'project_title',(SELECT title FROM bid_projects WHERE id=typed.project_id),
    'workspace',kb_bid_v2_load_workspace_revision(typed.workspace_id,typed.workspace_revision_id,typed.workspace_sha256),
    'assets',coalesce((SELECT jsonb_agg(item ORDER BY item->>'asset_revision_id') FROM (
      SELECT jsonb_build_object('asset_revision_id',asset.id,'object_ref',asset.object_ref,
        'sha256',asset.content_sha256,'media_type',asset.media_type,'file_name',asset.file_name,
        'provenance','manual_workspace') item
      FROM bid_workspace_asset_artifacts asset WHERE asset.workspace_id=typed.workspace_id AND EXISTS (
        SELECT 1 FROM bid_workspace_block_occurrences occurrence JOIN bid_content_block_revision_artifacts block
          ON block.project_id=occurrence.project_id AND block.id=occurrence.block_revision_id
        WHERE occurrence.workspace_revision_id=typed.workspace_revision_id
          AND block.block_kind IN ('image','attachment_ref')
          AND (convert_from(block.canonical_payload,'UTF8')::jsonb#>>'{content,asset_revision_id}')::uuid=asset.id)
      UNION ALL
      SELECT jsonb_build_object('asset_revision_id',page.id,'object_ref',page.object_ref,
        'sha256',page.content_sha256,'media_type',page.media_type,'file_name','附件第'||page.page_number||'页',
        'width_px',page.geometry->'width_px','height_px',page.geometry->'height_px','provenance','prepared_attachment') item
      FROM bid_attachment_preparation_asset_items page
      JOIN bid_attachment_preparation_revision_artifacts preparation
        ON preparation.id=page.attachment_preparation_revision_id
      WHERE preparation.workspace_id=typed.workspace_id AND preparation.status='ready' AND EXISTS (
        SELECT 1 FROM bid_workspace_block_occurrences occurrence JOIN bid_content_block_revision_artifacts block
          ON block.project_id=occurrence.project_id AND block.id=occurrence.block_revision_id
        WHERE occurrence.workspace_revision_id=typed.workspace_revision_id AND block.block_kind='attachment_ref'
          AND convert_from(block.canonical_payload,'UTF8')::jsonb#>>'{content,render_mode}'='embedded_pages'
          AND kb_bid_v2_resolve_export_attachment_preparation(p_request_artifact_id,
            typed.workspace_revision_id,occurrence.block_revision_id)=preparation.id)
      UNION ALL
      SELECT jsonb_build_object('asset_revision_id',quote_object.quote_snapshot_id,
        'object_ref',quote_object.object_ref,'sha256',quote_object.content_sha256,
        'media_type',quote_object.media_type,'file_name','quote-snapshot.json','provenance','quote_snapshot') item
      FROM bid_async_stage_receipts receipt
      JOIN bid_submission_assessment_snapshot_artifacts assessment
        ON assessment.id=(receipt.result_identity->>'artifact_id')::uuid
        AND assessment.content_sha256=receipt.result_identity->>'sha256'
      JOIN bid_quote_snapshot_object_identities quote_object
        ON quote_object.project_id=assessment.project_id
        AND quote_object.quote_snapshot_id=assessment.quote_snapshot_id
        AND quote_object.content_sha256=assessment.quote_snapshot_sha256
      WHERE receipt.request_artifact_id=p_request_artifact_id AND receipt.stage_kind='assessment'
        AND receipt.frozen_input_sha256=p_frozen_input_sha256
    ) frozen_asset),'[]'::jsonb),
    'form_definitions',coalesce((SELECT jsonb_agg(convert_from(form.canonical_payload,'UTF8')::jsonb ORDER BY form.id)
      FROM bid_tender_structured_form_definition_artifacts form WHERE EXISTS (
        SELECT 1 FROM bid_workspace_block_occurrences occurrence JOIN bid_content_block_revision_artifacts block
          ON block.project_id=occurrence.project_id AND block.id=occurrence.block_revision_id
        WHERE occurrence.workspace_revision_id=typed.workspace_revision_id AND block.block_kind='structured_form'
          AND (convert_from(block.canonical_payload,'UTF8')::jsonb#>>'{content,form_definition_revision_id}')::uuid=form.id)),'[]'::jsonb),
    'attachment_preparations',coalesce((SELECT jsonb_agg(preparation.canonical_payload ORDER BY preparation.id)
      FROM bid_attachment_preparation_revision_artifacts preparation
      WHERE preparation.workspace_id=typed.workspace_id AND preparation.status='ready' AND EXISTS (
        SELECT 1 FROM bid_workspace_block_occurrences occurrence JOIN bid_content_block_revision_artifacts block
          ON block.project_id=occurrence.project_id AND block.id=occurrence.block_revision_id
        WHERE occurrence.workspace_revision_id=typed.workspace_revision_id AND block.block_kind='attachment_ref'
          AND convert_from(block.canonical_payload,'UTF8')::jsonb#>>'{content,render_mode}'='embedded_pages'
          AND kb_bid_v2_resolve_export_attachment_preparation(p_request_artifact_id,
            typed.workspace_revision_id,occurrence.block_revision_id)=preparation.id)),'[]'::jsonb));
END $$;

CREATE FUNCTION kb_bid_v2_transition_submission_export(
  p_request_artifact_id uuid,p_request_revision bigint,p_frozen_input_sha256 kb_sha256,
  p_font_staging_id uuid,p_font_object_ref kb_object_ref,p_font_sha256 kb_sha256,p_font_media_type text,
  p_snapshot_id uuid,p_manifest_id uuid,p_output_staging_id uuid,p_output_id uuid,
  p_output_object_ref kb_object_ref,p_output_sha256 kb_sha256,p_output_media_type text,p_output_byte_length bigint,
  p_actor kb_actor_identity
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE typed bid_submission_export_request_identities%ROWTYPE; request_value bid_async_request_snapshot_artifacts%ROWTYPE;
  revision bid_workspace_revision_artifacts%ROWTYPE; settings bid_document_settings_revision_artifacts%ROWTYPE;
  assessment bid_submission_assessment_snapshot_artifacts%ROWTYPE; docx_contract bid_renderer_contract_artifacts%ROWTYPE;
  pdf_contract bid_renderer_contract_artifacts%ROWTYPE; snapshot_id uuid:=p_snapshot_id; manifest_id uuid:=p_manifest_id;
  nodes jsonb; assets jsonb; forms jsonb; preparations jsonb; font_items jsonb; payload jsonb; snapshot_sha kb_sha256;
  manifest_payload bytea; manifest_sha kb_sha256; published_identity jsonb; result_sha kb_sha256; dependency record;
  report_id uuid:=kb_bid_v2_deterministic_uuid(p_output_id::text||':assessment-report'); report_payload bytea; report_sha kb_sha256;
  operation_sha kb_sha256:=kb_bid_v2_sha256_bytes(convert_to('{"kind":"layout_document_v2","version":1}','UTF8'));
  ordinal_value integer:=0; prior bid_async_stage_receipts%ROWTYPE;
  prepared_manifest bid_submission_manifest_artifacts%ROWTYPE;
  prepared_snapshot bid_render_document_snapshot_artifacts%ROWTYPE;
BEGIN
  SELECT request_row.* INTO STRICT request_value FROM bid_async_request_snapshot_artifacts request_row
    WHERE request_row.id=p_request_artifact_id AND request_row.revision=p_request_revision
      AND request_row.frozen_input_sha256=p_frozen_input_sha256 FOR UPDATE;
  IF request_value.status='succeeded' THEN
    SELECT * INTO STRICT prior FROM bid_async_stage_receipts WHERE request_artifact_id=p_request_artifact_id AND stage_kind='package';
    RETURN prior.result_identity;
  END IF;
  IF request_value.status<>'pending' THEN RAISE EXCEPTION 'SUBMISSION_EXPORT_NOT_PENDING' USING ERRCODE='23514'; END IF;
  SELECT * INTO STRICT typed FROM bid_submission_export_request_identities WHERE request_artifact_id=p_request_artifact_id;
  SELECT * INTO prior FROM bid_async_stage_receipts
    WHERE request_artifact_id=p_request_artifact_id AND stage_kind='manifest'
      AND frozen_input_sha256=p_frozen_input_sha256;
  IF p_output_id IS NOT NULL AND FOUND THEN
    SELECT * INTO STRICT prepared_manifest FROM bid_submission_manifest_artifacts
      WHERE id=(prior.result_identity->>'artifact_id')::uuid
        AND content_sha256=prior.result_identity->>'sha256';
    SELECT * INTO STRICT prepared_snapshot FROM bid_render_document_snapshot_artifacts
      WHERE id=prepared_manifest.render_snapshot_id;
    IF prepared_manifest.id<>p_manifest_id OR prepared_snapshot.id<>p_snapshot_id
      OR NOT EXISTS (SELECT 1 FROM bid_render_snapshot_font_items font
        WHERE font.render_snapshot_id=prepared_snapshot.id AND font.object_ref=p_font_object_ref
          AND font.content_sha256=p_font_sha256 AND font.media_type=p_font_media_type) THEN
      RAISE EXCEPTION 'PREPARED_EXPORT_IDENTITY_MISMATCH' USING ERRCODE='23514';
    END IF;
    IF p_output_object_ref<>'objects/'||p_output_sha256 OR p_output_byte_length<=0 OR
       (typed.format='pdf' AND p_output_media_type<>'application/pdf') OR
       (typed.format='docx' AND p_output_media_type<>'application/vnd.openxmlformats-officedocument.wordprocessingml.document') THEN
      RAISE EXCEPTION 'SUBMISSION_OUTPUT_IDENTITY_INVALID' USING ERRCODE='23514';
    END IF;
    PERFORM 1 FROM object_registry WHERE object_ref=p_output_object_ref AND digest=p_output_sha256
      AND media_type=p_output_media_type AND byte_length=p_output_byte_length AND state='available';
    IF NOT FOUND THEN RAISE EXCEPTION 'SUBMISSION_OUTPUT_NOT_AVAILABLE' USING ERRCODE='23514'; END IF;
    PERFORM kb_object_upload_commit(p_output_staging_id,p_output_object_ref,p_output_sha256,p_output_media_type,
      p_output_byte_length,'bid_submission_output',p_output_id,
      'output:'||typed.project_id||':'||typed.workspace_id||':'||prepared_manifest.id,p_actor);
    SELECT * INTO STRICT assessment FROM bid_submission_assessment_snapshot_artifacts
      WHERE id=prepared_snapshot.submission_assessment_snapshot_id
        AND content_sha256=prepared_snapshot.submission_assessment_snapshot_sha256;
    INSERT INTO bid_submission_output_artifacts(id,project_id,workspace_id,manifest_id,format,object_ref,content_sha256,
      media_type,byte_length,owner_id,owner_occurrence)
    VALUES(p_output_id,typed.project_id,typed.workspace_id,prepared_manifest.id,typed.format,p_output_object_ref,p_output_sha256,
      p_output_media_type,p_output_byte_length,p_output_id,
      'output:'||typed.project_id||':'||typed.workspace_id||':'||prepared_manifest.id);
    report_payload:=kb_bid_v2_json_payload(jsonb_build_object('schema_version',1,'assessment_report_id',report_id,
      'submission_output_id',p_output_id,'manifest_id',prepared_manifest.id,
      'submission_assessment_snapshot_id',assessment.id,'submission_assessment_snapshot_sha256',assessment.content_sha256,
      'assessment',convert_from(assessment.canonical_payload,'UTF8')::jsonb,
      'selected_evidence',coalesce((SELECT jsonb_agg(jsonb_build_object(
        'selection_id',frozen.selection_id,'matching_report_id',frozen.matching_report_id,
        'selected_evidence_item_ids',frozen.item_ids,'items',frozen.items) ORDER BY frozen.first_ordinal)
        FROM (SELECT evidence.selection_id,evidence.matching_report_id,min(evidence.ordinal) first_ordinal,
          jsonb_agg(item.id ORDER BY evidence.ordinal) item_ids,
          jsonb_agg(item.item_payload ORDER BY evidence.ordinal) items
          FROM bid_submission_assessment_snapshot_evidence_items evidence
          JOIN bid_evidence_bundle_items item ON item.evidence_bundle_id=evidence.evidence_bundle_id
            AND item.id=evidence.evidence_item_id AND item.content_sha256=evidence.evidence_item_sha256
          WHERE evidence.assessment_snapshot_id=assessment.id
          GROUP BY evidence.selection_id,evidence.matching_report_id) frozen),'[]'::jsonb)));
    report_sha:=kb_bid_v2_sha256_bytes(report_payload);
    INSERT INTO bid_submission_assessment_report_artifacts(id,project_id,workspace_id,submission_output_id,manifest_id,
      submission_assessment_snapshot_id,canonical_payload,content_sha256)
    VALUES(report_id,typed.project_id,typed.workspace_id,p_output_id,prepared_manifest.id,assessment.id,report_payload,report_sha);
    published_identity:=jsonb_build_object('artifact_id',p_output_id,'sha256',p_output_sha256,
      'manifest_id',prepared_manifest.id,'render_snapshot_id',prepared_snapshot.id,'format',typed.format);
    result_sha:=kb_bid_v2_sha256_bytes(convert_to(published_identity::text,'UTF8'));
    INSERT INTO bid_async_stage_receipts(request_artifact_id,stage_kind,frozen_input_sha256,result_identity,result_sha256)
    VALUES(p_request_artifact_id,'render',p_frozen_input_sha256,
        jsonb_build_object('artifact_id',p_output_id,'sha256',p_output_sha256),p_output_sha256),
      (p_request_artifact_id,'object_commit',p_frozen_input_sha256,
        jsonb_build_object('artifact_id',p_output_id,'sha256',p_output_sha256),p_output_sha256),
      (p_request_artifact_id,'package',p_frozen_input_sha256,published_identity,result_sha);
    UPDATE bid_async_request_snapshot_artifacts SET status='succeeded',result_identity=published_identity,
      finished_at=clock_timestamp() WHERE id=p_request_artifact_id;
    RETURN published_identity;
  END IF;
  IF p_output_id IS NULL AND FOUND THEN
    RETURN prior.result_identity||jsonb_build_object('replayed',true);
  END IF;
  IF p_output_id IS NOT NULL THEN
    RAISE EXCEPTION 'SUBMISSION_EXPORT_NOT_PREPARED' USING ERRCODE='23514';
  END IF;
  IF p_font_object_ref<>'objects/'||p_font_sha256 OR p_font_sha256<>'5d0df56f107605387e0de494b22dfc7fb05d8d79ffd981474e7be11dbe571882'
     OR p_font_media_type<>'font/otf' THEN RAISE EXCEPTION 'RENDER_FONT_IDENTITY_INVALID' USING ERRCODE='23514'; END IF;
  PERFORM 1 FROM object_registry WHERE object_ref=p_font_object_ref AND digest=p_font_sha256
    AND media_type=p_font_media_type AND state='available';
  IF NOT FOUND THEN RAISE EXCEPTION 'RENDER_FONT_NOT_AVAILABLE' USING ERRCODE='23514'; END IF;
  PERFORM kb_object_upload_commit(p_font_staging_id,p_font_object_ref,p_font_sha256,p_font_media_type,
    (SELECT byte_length FROM object_registry WHERE object_ref=p_font_object_ref),
    'bid_render_font','00000000-0000-5000-8000-000000000304','font:cjk',p_actor);
  SELECT * INTO STRICT revision FROM bid_workspace_revision_artifacts
    WHERE id=typed.workspace_revision_id AND content_sha256=typed.workspace_sha256;
  SELECT * INTO STRICT settings FROM bid_document_settings_revision_artifacts
    WHERE id=typed.document_settings_revision_id AND content_sha256=typed.document_settings_sha256;
  SELECT * INTO STRICT prior FROM bid_async_stage_receipts
    WHERE request_artifact_id=p_request_artifact_id AND stage_kind='assessment'
      AND frozen_input_sha256=p_frozen_input_sha256;
  SELECT * INTO STRICT assessment FROM bid_submission_assessment_snapshot_artifacts
    WHERE id=(prior.result_identity->>'artifact_id')::uuid
      AND content_sha256=prior.result_identity->>'sha256'
      AND workspace_id=typed.workspace_id AND workspace_revision_id=typed.workspace_revision_id;
  IF assessment.quote_snapshot_id IS NOT NULL AND NOT EXISTS (
      SELECT 1 FROM bid_quote_snapshot_object_identities quote_object
      WHERE quote_object.project_id=typed.project_id
        AND quote_object.quote_snapshot_id=assessment.quote_snapshot_id
        AND quote_object.content_sha256=assessment.quote_snapshot_sha256) THEN
    RAISE EXCEPTION 'QUOTE_SNAPSHOT_OBJECT_MISSING' USING ERRCODE='23514';
  END IF;
  SELECT * INTO STRICT docx_contract FROM bid_renderer_contract_artifacts WHERE id='00000000-0000-5000-8000-000000000302';
  SELECT * INTO STRICT pdf_contract FROM bid_renderer_contract_artifacts WHERE id='00000000-0000-5000-8000-000000000303';
  INSERT INTO bid_render_font_artifacts(id,object_ref,content_sha256,media_type,family,script)
  VALUES('00000000-0000-5000-8000-000000000304',p_font_object_ref,p_font_sha256,p_font_media_type,'Noto Sans JP','cjk')
  ON CONFLICT(id) DO NOTHING;
  IF EXISTS (SELECT 1 FROM bid_workspace_block_occurrences occurrence JOIN bid_content_block_revision_artifacts block
      ON block.project_id=occurrence.project_id AND block.id=occurrence.block_revision_id
      WHERE occurrence.workspace_revision_id=typed.workspace_revision_id AND block.block_kind='attachment_ref'
        AND convert_from(block.canonical_payload,'UTF8')::jsonb#>>'{content,render_mode}'='embedded_pages'
        AND NOT EXISTS (SELECT 1 FROM bid_attachment_preparation_revision_artifacts preparation
          WHERE preparation.id=kb_bid_v2_resolve_export_attachment_preparation(p_request_artifact_id,
              typed.workspace_revision_id,occurrence.block_revision_id)
            AND preparation.project_id=typed.project_id AND preparation.workspace_id=typed.workspace_id
            AND preparation.source_asset_revision_id=(convert_from(block.canonical_payload,'UTF8')::jsonb#>>'{content,asset_revision_id}')::uuid
            AND preparation.status='ready')) THEN
    RAISE EXCEPTION 'ATTACHMENT_PREPARATION_REQUIRED' USING ERRCODE='23514';
  END IF;
  SELECT coalesce(jsonb_agg(jsonb_build_object('node_occurrence_id',occurrence.id,'node_revision_id',node.id,
    'parent_occurrence_id',occurrence.parent_occurrence_id,'ordinal',occurrence.ordinal,'depth',occurrence.depth,
    'title',node.title,'render_role',node.render_role,'block_occurrences',coalesce((SELECT jsonb_agg(jsonb_build_object(
      'block_occurrence_id',block_occurrence.id,'block_revision_id',block.id,'ordinal',block_occurrence.ordinal,
      'block_sha256',block.content_sha256) ORDER BY block_occurrence.ordinal,block_occurrence.id)
      FROM bid_workspace_block_occurrences block_occurrence JOIN bid_content_block_revision_artifacts block
        ON block.project_id=block_occurrence.project_id AND block.id=block_occurrence.block_revision_id
      WHERE block_occurrence.workspace_revision_id=typed.workspace_revision_id AND block_occurrence.node_occurrence_id=occurrence.id),'[]'::jsonb))
      ORDER BY occurrence.depth,occurrence.ordinal,occurrence.id),'[]'::jsonb) INTO nodes
  FROM bid_workspace_node_occurrences occurrence JOIN bid_outline_node_revision_artifacts node
    ON node.project_id=occurrence.project_id AND node.id=occurrence.node_revision_id
  WHERE occurrence.workspace_revision_id=typed.workspace_revision_id;
  SELECT coalesce(jsonb_agg(item ORDER BY item->>'asset_revision_id'),'[]'::jsonb) INTO assets FROM (
    SELECT jsonb_build_object('asset_revision_id',asset.id,'object_ref',asset.object_ref,'sha256',asset.content_sha256,
      'media_type',asset.media_type,'provenance','manual_workspace') item
    FROM bid_workspace_asset_artifacts asset WHERE asset.workspace_id=typed.workspace_id AND EXISTS (
      SELECT 1 FROM bid_workspace_block_occurrences occurrence JOIN bid_content_block_revision_artifacts block
        ON block.project_id=occurrence.project_id AND block.id=occurrence.block_revision_id
      WHERE occurrence.workspace_revision_id=typed.workspace_revision_id AND block.block_kind IN ('image','attachment_ref')
        AND (convert_from(block.canonical_payload,'UTF8')::jsonb#>>'{content,asset_revision_id}')::uuid=asset.id)
    UNION ALL
    SELECT jsonb_build_object('asset_revision_id',page.id,'object_ref',page.object_ref,'sha256',page.content_sha256,
      'media_type',page.media_type,'provenance','prepared_attachment') item
    FROM bid_attachment_preparation_asset_items page JOIN bid_attachment_preparation_revision_artifacts preparation
      ON preparation.id=page.attachment_preparation_revision_id
    WHERE preparation.workspace_id=typed.workspace_id AND preparation.status='ready' AND EXISTS (
      SELECT 1 FROM bid_workspace_block_occurrences occurrence JOIN bid_content_block_revision_artifacts block
        ON block.project_id=occurrence.project_id AND block.id=occurrence.block_revision_id
      WHERE occurrence.workspace_revision_id=typed.workspace_revision_id AND block.block_kind='attachment_ref'
        AND convert_from(block.canonical_payload,'UTF8')::jsonb#>>'{content,render_mode}'='embedded_pages'
        AND kb_bid_v2_resolve_export_attachment_preparation(p_request_artifact_id,
          typed.workspace_revision_id,occurrence.block_revision_id)=preparation.id)
    UNION ALL
    SELECT jsonb_build_object('asset_revision_id',quote_object.quote_snapshot_id,
      'object_ref',quote_object.object_ref,'sha256',quote_object.content_sha256,
      'media_type',quote_object.media_type,'provenance','quote_snapshot') item
    FROM bid_quote_snapshot_object_identities quote_object
    WHERE quote_object.project_id=typed.project_id
      AND quote_object.quote_snapshot_id=assessment.quote_snapshot_id
      AND quote_object.content_sha256=assessment.quote_snapshot_sha256
  ) render_asset;
  SELECT coalesce(jsonb_agg(jsonb_build_object('form_definition_revision_id',form.id,
    'canonical_sha256',form.content_sha256) ORDER BY form.id),'[]'::jsonb) INTO forms
  FROM bid_tender_structured_form_definition_artifacts form WHERE EXISTS (
    SELECT 1 FROM bid_workspace_block_occurrences occurrence JOIN bid_content_block_revision_artifacts block
      ON block.project_id=occurrence.project_id AND block.id=occurrence.block_revision_id
    WHERE occurrence.workspace_revision_id=typed.workspace_revision_id AND block.block_kind='structured_form'
      AND (convert_from(block.canonical_payload,'UTF8')::jsonb#>>'{content,form_definition_revision_id}')::uuid=form.id);
  SELECT coalesce(jsonb_agg(jsonb_build_object('attachment_preparation_revision_id',preparation.id,
    'status',preparation.status,'canonical_sha256',preparation.preparation_sha256) ORDER BY preparation.id),'[]'::jsonb)
    INTO preparations FROM bid_attachment_preparation_revision_artifacts preparation
    WHERE preparation.workspace_id=typed.workspace_id AND preparation.status='ready' AND EXISTS (
      SELECT 1 FROM bid_workspace_block_occurrences occurrence JOIN bid_content_block_revision_artifacts block
        ON block.project_id=occurrence.project_id AND block.id=occurrence.block_revision_id
      WHERE occurrence.workspace_revision_id=typed.workspace_revision_id AND block.block_kind='attachment_ref'
        AND convert_from(block.canonical_payload,'UTF8')::jsonb#>>'{content,render_mode}'='embedded_pages'
        AND kb_bid_v2_resolve_export_attachment_preparation(p_request_artifact_id,
          typed.workspace_revision_id,occurrence.block_revision_id)=preparation.id);
  font_items:=jsonb_build_array(jsonb_build_object('font_artifact_id','00000000-0000-5000-8000-000000000304',
    'object_ref',p_font_object_ref,'sha256',p_font_sha256,'media_type',p_font_media_type,'family','Noto Sans JP','script','cjk'));
  payload:=jsonb_build_object('schema_version',2,'render_snapshot_id',snapshot_id,'project_id',typed.project_id,
    'project_title',(SELECT title FROM bid_projects WHERE id=typed.project_id),'workspace_id',typed.workspace_id,'workspace_scope','project_wide','workspace_scope_revision_id',typed.scope_revision_id,
    'workspace_revision_id',typed.workspace_revision_id,'workspace_sha256',typed.workspace_sha256,
    'outline_checkpoint_id',typed.outline_checkpoint_id,'outline_checkpoint_sha256',typed.outline_checkpoint_sha256,
    'requirement_projection_revision_id',typed.requirement_projection_id,'requirement_projection_sha256',typed.requirement_projection_sha256,
    'document_settings_revision_id',typed.document_settings_revision_id,'document_settings_sha256',typed.document_settings_sha256,
    'submission_assessment_snapshot_id',assessment.id,'submission_assessment_snapshot_sha256',assessment.content_sha256,
    'output_mode',typed.output_mode,'format',typed.format,'mode_options',typed.mode_options,'ordered_nodes',nodes,'assets',assets,
    'form_definition_occurrences',forms,'attachment_preparation_occurrences',preparations,'content_block_schema_version',1,
    'content_block_schema_sha256','4d0027af37854644f824b8df208cfffddb4ab9612abd367219fc725f8dead696',
    'render_operation_contract_version',1,'render_operation_contract_sha256',operation_sha,
    'docx_renderer_contract_id',docx_contract.id,'docx_renderer_contract_sha256',docx_contract.content_sha256,
    'pdf_renderer_contract_id',pdf_contract.id,'pdf_renderer_contract_sha256',pdf_contract.content_sha256,
    'style_contract_id',typed.render_style_contract_id,'style_contract_sha256',typed.render_style_contract_sha256,
    'page_geometry',jsonb_build_object('page_size','A4','width_mm',210,'height_mm',297,
      'margins_mm',coalesce(settings.settings->'margins_mm','{"top":25.4,"right":25.4,"bottom":25.4,"left":25.4}'::jsonb)),
    'font_artifact_identities',font_items,'numbering_policy',coalesce(settings.settings->>'heading_numbering','decimal'),
    'toc_policy','included');
  snapshot_sha:=kb_bid_v2_sha256_bytes(convert_to(payload::text,'UTF8'));
  payload:=payload||jsonb_build_object('snapshot_sha256',snapshot_sha);
  INSERT INTO bid_render_document_snapshot_artifacts(id,project_id,workspace_id,schema_version,workspace_revision_id,
    workspace_sha256,scope_revision_id,outline_checkpoint_id,outline_checkpoint_sha256,requirement_projection_id,
    requirement_projection_sha256,document_settings_revision_id,document_settings_sha256,
    submission_assessment_snapshot_id,submission_assessment_snapshot_sha256,output_mode,format,mode_options,
    content_block_schema_version,content_block_schema_sha256,render_operation_contract_version,render_operation_contract_sha256,
    docx_renderer_contract_id,docx_renderer_contract_sha256,pdf_renderer_contract_id,pdf_renderer_contract_sha256,
    style_contract_id,style_contract_sha256,page_size,page_width_mm,page_height_mm,margins_mm,numbering_policy,toc_policy,
    canonical_payload,content_sha256)
  VALUES(snapshot_id,typed.project_id,typed.workspace_id,2,typed.workspace_revision_id,typed.workspace_sha256,typed.scope_revision_id,
    typed.outline_checkpoint_id,typed.outline_checkpoint_sha256,typed.requirement_projection_id,typed.requirement_projection_sha256,
    typed.document_settings_revision_id,typed.document_settings_sha256,assessment.id,assessment.content_sha256,typed.output_mode,
    typed.format,typed.mode_options,1,'4d0027af37854644f824b8df208cfffddb4ab9612abd367219fc725f8dead696',1,operation_sha,
    docx_contract.id,docx_contract.content_sha256,pdf_contract.id,pdf_contract.content_sha256,typed.render_style_contract_id,
    typed.render_style_contract_sha256,'A4',210,297,payload#>'{page_geometry,margins_mm}',payload->>'numbering_policy',
    payload->>'toc_policy',payload,snapshot_sha);
  INSERT INTO bid_render_snapshot_node_occurrences(render_snapshot_id,project_id,workspace_revision_id,node_occurrence_id,node_revision_id,ordinal)
    SELECT snapshot_id,typed.project_id,typed.workspace_revision_id,id,node_revision_id,ordinal
    FROM bid_workspace_node_occurrences WHERE workspace_revision_id=typed.workspace_revision_id;
  INSERT INTO bid_render_snapshot_block_occurrences(render_snapshot_id,project_id,workspace_revision_id,node_occurrence_id,
    block_occurrence_id,block_revision_id,block_sha256,ordinal)
    SELECT snapshot_id,typed.project_id,typed.workspace_revision_id,occurrence.node_occurrence_id,occurrence.id,occurrence.block_revision_id,
      block.content_sha256,occurrence.ordinal FROM bid_workspace_block_occurrences occurrence JOIN bid_content_block_revision_artifacts block
      ON block.project_id=occurrence.project_id AND block.id=occurrence.block_revision_id
    WHERE occurrence.workspace_revision_id=typed.workspace_revision_id;
  INSERT INTO bid_render_snapshot_asset_items(render_snapshot_id,ordinal,asset_revision_id,object_ref,content_sha256,media_type,provenance)
    SELECT snapshot_id,row_number() OVER(ORDER BY item.asset_revision_id)-1,item.asset_revision_id,item.object_ref,
      item.content_sha256,item.media_type,item.provenance FROM (
      SELECT asset.id asset_revision_id,asset.object_ref,asset.content_sha256,asset.media_type,'manual_workspace'::text provenance
      FROM bid_workspace_asset_artifacts asset WHERE asset.workspace_id=typed.workspace_id AND EXISTS (
        SELECT 1 FROM bid_workspace_block_occurrences occurrence JOIN bid_content_block_revision_artifacts block
          ON block.project_id=occurrence.project_id AND block.id=occurrence.block_revision_id
        WHERE occurrence.workspace_revision_id=typed.workspace_revision_id AND block.block_kind IN ('image','attachment_ref')
          AND (convert_from(block.canonical_payload,'UTF8')::jsonb#>>'{content,asset_revision_id}')::uuid=asset.id)
      UNION ALL
      SELECT page.id,page.object_ref,page.content_sha256,page.media_type,'prepared_attachment'::text
      FROM bid_attachment_preparation_asset_items page JOIN bid_attachment_preparation_revision_artifacts preparation
        ON preparation.id=page.attachment_preparation_revision_id
      WHERE preparation.workspace_id=typed.workspace_id AND preparation.status='ready' AND EXISTS (
        SELECT 1 FROM bid_workspace_block_occurrences occurrence JOIN bid_content_block_revision_artifacts block
          ON block.project_id=occurrence.project_id AND block.id=occurrence.block_revision_id
        WHERE occurrence.workspace_revision_id=typed.workspace_revision_id AND block.block_kind='attachment_ref'
          AND convert_from(block.canonical_payload,'UTF8')::jsonb#>>'{content,render_mode}'='embedded_pages'
          AND kb_bid_v2_resolve_export_attachment_preparation(p_request_artifact_id,
            typed.workspace_revision_id,occurrence.block_revision_id)=preparation.id)
      UNION ALL
      SELECT quote_object.quote_snapshot_id,quote_object.object_ref,quote_object.content_sha256,
        quote_object.media_type,'quote_snapshot'::text
      FROM bid_quote_snapshot_object_identities quote_object
      WHERE quote_object.project_id=typed.project_id
        AND quote_object.quote_snapshot_id=assessment.quote_snapshot_id
        AND quote_object.content_sha256=assessment.quote_snapshot_sha256
    ) item;
  INSERT INTO bid_render_snapshot_font_items(render_snapshot_id,ordinal,font_artifact_id,object_ref,content_sha256,media_type,family,script)
    VALUES(snapshot_id,0,'00000000-0000-5000-8000-000000000304',p_font_object_ref,p_font_sha256,p_font_media_type,'Noto Sans JP','cjk');
  INSERT INTO bid_render_snapshot_form_definition_items(render_snapshot_id,project_id,workspace_id,ordinal,form_definition_revision_id,canonical_sha256)
    SELECT snapshot_id,typed.project_id,typed.workspace_id,row_number() OVER(ORDER BY form.id)-1,form.id,form.content_sha256
    FROM bid_tender_structured_form_definition_artifacts form WHERE EXISTS (
      SELECT 1 FROM bid_workspace_block_occurrences occurrence JOIN bid_content_block_revision_artifacts block
      ON block.project_id=occurrence.project_id AND block.id=occurrence.block_revision_id
      WHERE occurrence.workspace_revision_id=typed.workspace_revision_id AND block.block_kind='structured_form'
        AND (convert_from(block.canonical_payload,'UTF8')::jsonb#>>'{content,form_definition_revision_id}')::uuid=form.id);
  INSERT INTO bid_render_snapshot_attachment_preparation_items(render_snapshot_id,project_id,workspace_id,ordinal,
    attachment_preparation_revision_id,preparation_status,canonical_sha256)
    SELECT snapshot_id,typed.project_id,typed.workspace_id,row_number() OVER(ORDER BY preparation.id)-1,
      preparation.id,preparation.status,preparation.preparation_sha256
    FROM bid_attachment_preparation_revision_artifacts preparation
    WHERE preparation.workspace_id=typed.workspace_id AND preparation.status='ready' AND EXISTS (
      SELECT 1 FROM bid_workspace_block_occurrences occurrence JOIN bid_content_block_revision_artifacts block
        ON block.project_id=occurrence.project_id AND block.id=occurrence.block_revision_id
      WHERE occurrence.workspace_revision_id=typed.workspace_revision_id AND block.block_kind='attachment_ref'
        AND convert_from(block.canonical_payload,'UTF8')::jsonb#>>'{content,render_mode}'='embedded_pages'
        AND kb_bid_v2_resolve_export_attachment_preparation(p_request_artifact_id,
          typed.workspace_revision_id,occurrence.block_revision_id)=preparation.id);
  manifest_payload:=kb_bid_v2_json_payload(jsonb_build_object('schema_version',1,'manifest_id',manifest_id,
    'render_snapshot_id',snapshot_id,'render_snapshot_sha256',snapshot_sha,'output_mode',typed.output_mode,
    'format',typed.format,'mode_options',typed.mode_options));
  manifest_sha:=kb_bid_v2_sha256_bytes(manifest_payload);
  INSERT INTO bid_submission_manifest_artifacts(id,project_id,workspace_id,render_snapshot_id,output_mode,format,
    mode_options,canonical_payload,content_sha256)
  VALUES(manifest_id,typed.project_id,typed.workspace_id,snapshot_id,typed.output_mode,typed.format,typed.mode_options,
    manifest_payload,manifest_sha);
  FOR dependency IN SELECT * FROM kb_bid_v2_manifest_expected_dependencies(manifest_id)
    ORDER BY dependency_kind,dependency_id LOOP
    INSERT INTO bid_submission_manifest_dependencies(manifest_id,dependency_kind,dependency_id,dependency_sha256,ordinal)
    VALUES(manifest_id,dependency.dependency_kind,dependency.dependency_id,dependency.dependency_sha256,ordinal_value);
    ordinal_value:=ordinal_value+1;
  END LOOP;
  published_identity:=jsonb_build_object('artifact_id',manifest_id,'sha256',manifest_sha,
    'render_snapshot_id',snapshot_id,'render_snapshot_sha256',snapshot_sha,'format',typed.format);
  INSERT INTO bid_async_stage_receipts(request_artifact_id,stage_kind,frozen_input_sha256,result_identity,result_sha256)
  VALUES(p_request_artifact_id,'attachment_prepare',p_frozen_input_sha256,
      jsonb_build_object('artifact_id',kb_bid_v2_deterministic_uuid(p_request_artifact_id::text||':attachment-prepare'),
        'sha256',kb_bid_v2_sha256_bytes(convert_to(preparations::text,'UTF8'))),
      kb_bid_v2_sha256_bytes(convert_to(preparations::text,'UTF8'))),
    (p_request_artifact_id,'render_snapshot',p_frozen_input_sha256,
      jsonb_build_object('artifact_id',snapshot_id,'sha256',snapshot_sha),snapshot_sha),
    (p_request_artifact_id,'manifest',p_frozen_input_sha256,published_identity,manifest_sha);
  RETURN published_identity;
END $$;

CREATE FUNCTION kb_bid_v2_prepare_submission_export(
  p_request_artifact_id uuid,p_request_revision bigint,p_frozen_input_sha256 kb_sha256,
  p_font_staging_id uuid,p_font_object_ref kb_object_ref,p_font_sha256 kb_sha256,p_font_media_type text,
  p_snapshot_id uuid,p_manifest_id uuid,p_actor kb_actor_identity
) RETURNS jsonb LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
  SELECT kb_bid_v2_transition_submission_export($1,$2,$3,$4,$5,$6,$7,$8,$9,
    NULL::uuid,NULL::uuid,NULL::kb_object_ref,NULL::kb_sha256,NULL::text,NULL::bigint,$10)
$$;

CREATE FUNCTION kb_bid_v2_publish_submission_export(
  p_request_artifact_id uuid,p_request_revision bigint,p_frozen_input_sha256 kb_sha256,
  p_font_staging_id uuid,p_font_object_ref kb_object_ref,p_font_sha256 kb_sha256,p_font_media_type text,
  p_snapshot_id uuid,p_manifest_id uuid,p_output_staging_id uuid,p_output_id uuid,
  p_output_object_ref kb_object_ref,p_output_sha256 kb_sha256,p_output_media_type text,p_output_byte_length bigint,
  p_actor kb_actor_identity
) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
BEGIN
  IF p_font_staging_id IS NULL OR p_font_object_ref IS NULL OR p_font_sha256 IS NULL
    OR p_font_media_type IS NULL OR p_snapshot_id IS NULL OR p_manifest_id IS NULL THEN
    RAISE EXCEPTION 'PREPARED_EXPORT_IDENTITY_MISMATCH' USING ERRCODE='23514';
  END IF;
  IF p_output_staging_id IS NULL OR p_output_id IS NULL OR p_output_object_ref IS NULL
    OR p_output_sha256 IS NULL OR p_output_media_type IS NULL OR p_output_byte_length IS NULL THEN
    RAISE EXCEPTION 'SUBMISSION_OUTPUT_IDENTITY_INVALID' USING ERRCODE='23514';
  END IF;
  RETURN kb_bid_v2_transition_submission_export(p_request_artifact_id,p_request_revision,p_frozen_input_sha256,
    p_font_staging_id,p_font_object_ref,p_font_sha256,p_font_media_type,p_snapshot_id,p_manifest_id,
    p_output_staging_id,p_output_id,p_output_object_ref,p_output_sha256,p_output_media_type,p_output_byte_length,p_actor);
END $$;

CREATE FUNCTION kb_bid_v2_load_submission_manifest_render_input(
  p_manifest_id uuid,p_manifest_sha256 kb_sha256
) RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE manifest bid_submission_manifest_artifacts%ROWTYPE;
  snapshot bid_render_document_snapshot_artifacts%ROWTYPE; result_value jsonb; workspace_value jsonb; quote_value jsonb;
BEGIN
  SELECT * INTO STRICT manifest FROM bid_submission_manifest_artifacts
    WHERE id=p_manifest_id AND content_sha256=p_manifest_sha256;
  SELECT * INTO STRICT snapshot FROM bid_render_document_snapshot_artifacts
    WHERE id=manifest.render_snapshot_id
      AND content_sha256=(convert_from(manifest.canonical_payload,'UTF8')::jsonb->>'render_snapshot_sha256')::kb_sha256;
  SELECT convert_from(quote.canonical_payload,'UTF8')::jsonb||jsonb_build_object(
      'artifact_id',quote.id,'sha256',quote.content_sha256)
    INTO quote_value FROM bid_submission_assessment_snapshot_artifacts assessment
    JOIN bid_quote_snapshot_artifacts quote ON quote.project_id=assessment.project_id
      AND quote.id=assessment.quote_snapshot_id AND quote.content_sha256=assessment.quote_snapshot_sha256
    WHERE assessment.id=snapshot.submission_assessment_snapshot_id
      AND assessment.content_sha256=snapshot.submission_assessment_snapshot_sha256;
  workspace_value:=kb_bid_v2_load_workspace_revision(
    snapshot.workspace_id,snapshot.workspace_revision_id,snapshot.workspace_sha256);
  IF workspace_value IS NULL THEN RAISE EXCEPTION 'MANIFEST_WORKSPACE_IDENTITY_INVALID' USING ERRCODE='23514'; END IF;
  workspace_value:=jsonb_set(workspace_value,'{quote_snapshot}',coalesce(quote_value,'null'::jsonb),true);
  result_value:=jsonb_build_object(
    'request',jsonb_build_object('output_mode',snapshot.output_mode,'format',snapshot.format,'mode_options',snapshot.mode_options),
    'project_title',snapshot.canonical_payload->>'project_title',
    'workspace',workspace_value,
    'assets',coalesce((SELECT jsonb_agg(jsonb_build_object(
      'asset_revision_id',item.asset_revision_id,'object_ref',item.object_ref,'sha256',item.content_sha256,
      'media_type',item.media_type,'file_name',coalesce(asset.file_name,
        CASE WHEN item.provenance='prepared_attachment' THEN '附件第'||page.page_number||'页'
             WHEN item.provenance='quote_snapshot' THEN 'quote-snapshot.json' END),
      'provenance',item.provenance) ORDER BY item.ordinal)
      FROM bid_render_snapshot_asset_items item
      LEFT JOIN bid_workspace_asset_artifacts asset ON asset.id=item.asset_revision_id
      LEFT JOIN bid_attachment_preparation_asset_items page ON page.id=item.asset_revision_id
      WHERE item.render_snapshot_id=snapshot.id),'[]'::jsonb),
    'form_definitions',coalesce((SELECT jsonb_agg(convert_from(form.canonical_payload,'UTF8')::jsonb ORDER BY item.ordinal)
      FROM bid_render_snapshot_form_definition_items item
      JOIN bid_tender_structured_form_definition_artifacts form
        ON form.project_id=item.project_id AND form.id=item.form_definition_revision_id
          AND form.content_sha256=item.canonical_sha256
      WHERE item.render_snapshot_id=snapshot.id),'[]'::jsonb),
    'attachment_preparations',coalesce((SELECT jsonb_agg(preparation.canonical_payload ORDER BY item.ordinal)
      FROM bid_render_snapshot_attachment_preparation_items item
      JOIN bid_attachment_preparation_revision_artifacts preparation
        ON preparation.project_id=item.project_id AND preparation.id=item.attachment_preparation_revision_id
          AND preparation.status=item.preparation_status AND preparation.preparation_sha256=item.canonical_sha256
      WHERE item.render_snapshot_id=snapshot.id),'[]'::jsonb),
    'prepared_manifest',jsonb_build_object('manifest_id',manifest.id,'manifest_sha256',manifest.content_sha256,
      'render_snapshot_id',snapshot.id,'render_snapshot_sha256',snapshot.content_sha256));
  RETURN result_value;
END $$;

CREATE FUNCTION kb_bid_v2_mark_submission_export_failed(
  p_request_artifact_id uuid,p_request_revision bigint,p_frozen_input_sha256 kb_sha256,p_error_code text
) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
BEGIN
  UPDATE bid_async_request_snapshot_artifacts SET status='failed',error_code=p_error_code,finished_at=clock_timestamp()
  WHERE id=p_request_artifact_id AND request_kind='submission_export' AND revision=p_request_revision
    AND frozen_input_sha256=p_frozen_input_sha256 AND status='pending';
END $$;

CREATE FUNCTION kb_bid_v2_list_submission_exports(p_workspace_id uuid,p_actor kb_actor_identity)
RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE workspace bid_submission_workspaces%ROWTYPE;
BEGIN
 SELECT * INTO STRICT workspace FROM bid_submission_workspaces WHERE id=p_workspace_id;
 PERFORM kb_bid_v2_require_project_owner(workspace.project_id,p_actor);
 RETURN coalesce((SELECT jsonb_agg(jsonb_build_object('export_id',output.id,'manifest_id',output.manifest_id,
   'format',output.format,'mode',manifest.output_mode,'status','ready','sha256',output.content_sha256,
   'byte_length',output.byte_length,'created_at',output.created_at) ORDER BY output.created_at DESC,output.id DESC)
   FROM bid_submission_output_artifacts output JOIN bid_submission_manifest_artifacts manifest ON manifest.id=output.manifest_id
   WHERE output.workspace_id=p_workspace_id),'[]'::jsonb);
END $$;

CREATE FUNCTION kb_bid_v2_get_submission_export(p_workspace_id uuid,p_output_id uuid,p_actor kb_actor_identity)
RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE workspace bid_submission_workspaces%ROWTYPE; result jsonb;
BEGIN
 SELECT * INTO STRICT workspace FROM bid_submission_workspaces WHERE id=p_workspace_id;
 PERFORM kb_bid_v2_require_project_owner(workspace.project_id,p_actor);
 SELECT jsonb_build_object('export_id',output.id,'manifest_id',output.manifest_id,'render_snapshot_id',manifest.render_snapshot_id,
   'format',output.format,'mode',manifest.output_mode,'mode_options',manifest.mode_options,'status','ready',
   'sha256',output.content_sha256,'byte_length',output.byte_length,'media_type',output.media_type,
   'assessment_report_id',report.id,'assessment_report_sha256',report.content_sha256,'created_at',output.created_at,
   'attachment_preparations',coalesce((SELECT jsonb_agg(jsonb_build_object(
      'attachment_preparation_revision_id',dependency.dependency_id,'sha256',dependency.dependency_sha256)
      ORDER BY dependency.ordinal)
     FROM bid_submission_manifest_dependencies dependency
     WHERE dependency.manifest_id=manifest.id AND dependency.dependency_kind='attachment_preparation'),'[]'::jsonb))
 INTO result FROM bid_submission_output_artifacts output JOIN bid_submission_manifest_artifacts manifest ON manifest.id=output.manifest_id
 JOIN bid_submission_assessment_report_artifacts report ON report.submission_output_id=output.id
 WHERE output.id=p_output_id AND output.workspace_id=p_workspace_id;
 IF result IS NULL THEN RAISE EXCEPTION 'SUBMISSION_EXPORT_NOT_FOUND' USING ERRCODE='P0002'; END IF;
 RETURN result;
END $$;

CREATE FUNCTION kb_bid_v2_get_submission_assessment_report(p_workspace_id uuid,p_output_id uuid,p_actor kb_actor_identity)
RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE workspace bid_submission_workspaces%ROWTYPE; result jsonb;
BEGIN
 SELECT * INTO STRICT workspace FROM bid_submission_workspaces WHERE id=p_workspace_id;
 PERFORM kb_bid_v2_require_project_owner(workspace.project_id,p_actor);
 SELECT convert_from(report.canonical_payload,'UTF8')::jsonb||jsonb_build_object('content_sha256',report.content_sha256)
 INTO result FROM bid_submission_assessment_report_artifacts report
 WHERE report.submission_output_id=p_output_id AND report.workspace_id=p_workspace_id;
 IF result IS NULL THEN RAISE EXCEPTION 'ASSESSMENT_REPORT_NOT_FOUND' USING ERRCODE='P0002'; END IF;
 RETURN result;
END $$;

CREATE FUNCTION kb_bid_v2_get_submission_export_object(p_workspace_id uuid,p_output_id uuid,p_actor kb_actor_identity)
RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE workspace bid_submission_workspaces%ROWTYPE;
BEGIN
 SELECT * INTO STRICT workspace FROM bid_submission_workspaces WHERE id=p_workspace_id;
 PERFORM kb_bid_v2_require_project_owner(workspace.project_id,p_actor);
 RETURN (SELECT jsonb_build_object('object_ref',object_ref,'sha256',content_sha256,'media_type',media_type,
   'byte_length',byte_length,'file_name','submission.'||format) FROM bid_submission_output_artifacts
   WHERE id=p_output_id AND workspace_id=p_workspace_id);
END $$;

-- Publication is the only path that marks a tender document ready.
CREATE FUNCTION kb_bid_v2_mark_tender_document_failed(
  p_request_artifact_id uuid,p_error_code text
) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE typed bid_tender_document_process_request_identities%ROWTYPE;
BEGIN
  SELECT * INTO STRICT typed FROM bid_tender_document_process_request_identities
    WHERE request_artifact_id=p_request_artifact_id;
  UPDATE bid_documents SET parse_status='failed' WHERE id=typed.document_id AND project_id=typed.project_id;
  UPDATE bid_async_request_snapshot_artifacts SET status='failed',
    error_code=CASE WHEN p_error_code IN ('INPUT_SCHEMA_INVALID','FROZEN_INPUT_MISSING','FROZEN_INPUT_DIGEST_MISMATCH',
      'REQUEST_OBSOLETE','WORKSPACE_CAS_CONFLICT','AGENT_OUTPUT_INVALID','EVIDENCE_UNAVAILABLE','ASSET_MISSING',
      'ASSET_DIGEST_MISMATCH','ATTACHMENT_PREPARATION_FAILED','RENDER_SCHEMA_INVALID','RENDERER_FAILED',
      'OBJECT_COMMIT_FAILED') THEN p_error_code ELSE 'AGENT_OUTPUT_INVALID' END,
    finished_at=clock_timestamp() WHERE id=p_request_artifact_id AND status='pending';
END $$;

REVOKE ALL ON ALL TABLES IN SCHEMA public FROM PUBLIC;
REVOKE ALL ON ALL FUNCTIONS IN SCHEMA public FROM PUBLIC;
GRANT SELECT ON bidding_v2_projects,bidding_v2_workspace_heads,bidding_v2_async_requests,bidding_v2_outputs
  TO kb_runtime_api,kb_runtime_worker;
GRANT EXECUTE ON FUNCTION kb_bid_v2_publish_requirement_set(uuid,kb_sha256),
  kb_bid_v2_load_tender_document_process_input(uuid,bigint,kb_sha256),
  kb_bid_v2_publish_tender_document_process(uuid,bigint,kb_sha256,uuid,uuid,kb_sha256,jsonb,jsonb,jsonb,kb_actor_identity),
  kb_bid_v2_compile_requirement_set(uuid,bigint,kb_sha256,kb_actor_identity),
  kb_bid_v2_mark_requirement_set_compile_failed(uuid,bigint,kb_sha256,text),
  kb_bid_v2_load_outline_generation_input(uuid,bigint,kb_sha256),
  kb_bid_v2_publish_outline_generation(uuid,bigint,kb_sha256,uuid,bytea,kb_sha256,jsonb),
  kb_bid_v2_mark_outline_generation_failed(uuid,bigint,kb_sha256,text),
  kb_bid_v2_load_content_generation_input(uuid,bigint,kb_sha256),
  kb_bid_v2_load_user_pick_evidence(uuid,bigint,kb_sha256),
  kb_bid_v2_publish_content_generation(uuid,bigint,kb_sha256,uuid,kb_sha256,jsonb,uuid,bytea,kb_sha256,jsonb),
  kb_bid_v2_mark_content_generation_failed(uuid,bigint,kb_sha256,text),
  kb_bid_v2_load_submission_export_input(uuid,bigint,kb_sha256),
  kb_bid_v2_publish_pdf_attachment_preparation(uuid,bigint,kb_sha256,uuid,uuid,uuid[],uuid[],kb_object_ref[],kb_sha256[],text[],bigint[],integer[],integer[],kb_actor_identity),
  kb_bid_v2_prepare_submission_export(uuid,bigint,kb_sha256,uuid,kb_object_ref,kb_sha256,text,uuid,uuid,kb_actor_identity),
  kb_bid_v2_load_submission_manifest_render_input(uuid,kb_sha256),
  kb_bid_v2_publish_submission_export(uuid,bigint,kb_sha256,uuid,kb_object_ref,kb_sha256,text,uuid,uuid,uuid,uuid,kb_object_ref,kb_sha256,text,bigint,kb_actor_identity),
  kb_bid_v2_mark_submission_export_failed(uuid,bigint,kb_sha256,text),
  kb_bid_v2_mark_tender_document_failed(uuid,text)
  TO kb_runtime_worker;
GRANT EXECUTE ON FUNCTION kb_bid_v2_create_project(uuid,text,uuid,kb_actor_identity,text,bytea,kb_sha256),
  kb_bid_v2_list_projects(uuid,kb_actor_identity),
  kb_bid_v2_get_project(uuid,kb_actor_identity),
  kb_bid_v2_next_quote_snapshot_revision(uuid,kb_actor_identity),
  kb_bid_v2_publish_quote_snapshot(uuid,uuid,bigint,uuid,kb_object_ref,kb_sha256,bigint,bytea,kb_actor_identity,text,bytea,kb_sha256),
  kb_bid_v2_list_quote_snapshots(uuid,kb_actor_identity),
  kb_bid_v2_get_quote_snapshot(uuid,uuid,kb_actor_identity),
  kb_bid_v2_end_project(uuid,kb_actor_identity,text,bytea,kb_sha256),
  kb_bid_v2_upload_tender_document(uuid,uuid,uuid,uuid,text,text,bigint,kb_object_ref,kb_sha256,kb_actor_identity,text,bytea,kb_sha256),
  kb_bid_v2_retry_tender_document(uuid,uuid,uuid,bigint,kb_actor_identity,text,bytea,kb_sha256),
  kb_bid_v2_list_tender_documents(uuid,kb_actor_identity),
  kb_bid_v2_patch_document_role(uuid,uuid,text,uuid,kb_sha256,kb_actor_identity,text,bytea,kb_sha256),
  kb_bid_v2_upsert_document_relation(uuid,uuid,uuid,uuid,text,jsonb,uuid,kb_sha256,kb_actor_identity,text,bytea,kb_sha256),
  kb_bid_v2_list_document_relations(uuid,kb_actor_identity),
  kb_bid_v2_list_document_sets(uuid,kb_actor_identity),
  kb_bid_v2_get_document_set(uuid,uuid,kb_actor_identity),
  kb_bid_v2_freeze_document_set(uuid,uuid[],uuid,kb_sha256,uuid,kb_actor_identity,text,bytea,kb_sha256),
  kb_bid_v2_publish_disposition_set(uuid,uuid,jsonb,uuid,kb_sha256,uuid,kb_actor_identity,text,bytea,kb_sha256),
  kb_bid_v2_list_source_units(uuid,kb_actor_identity),
  kb_bid_v2_list_structured_forms(uuid,kb_actor_identity),
  kb_bid_v2_list_requirements(uuid,kb_actor_identity),
  kb_bid_v2_patch_requirement(uuid,uuid,uuid,kb_sha256,text,text,text,text,text,jsonb,jsonb,kb_actor_identity,text,bytea,kb_sha256),
  kb_bid_v2_publish_requirement_supersession(uuid,uuid,uuid,uuid,jsonb,boolean,uuid,kb_sha256,kb_actor_identity,text,bytea,kb_sha256),
  kb_bid_v2_load_workspace_for_actor(uuid,kb_actor_identity),
  kb_bid_v2_get_requirement_projection(uuid,kb_actor_identity),
  kb_bid_v2_refresh_requirement_projection(uuid,uuid,kb_sha256,kb_actor_identity,text,bytea,kb_sha256),
  kb_bid_v2_list_workspace_assets(uuid,kb_actor_identity),
  kb_bid_v2_upload_workspace_asset(uuid,uuid,uuid,text,text,bigint,kb_object_ref,kb_sha256,kb_actor_identity,text,bytea,kb_sha256),
  kb_bid_v2_prepare_workspace_attachment(uuid,uuid,uuid,uuid[],uuid[],integer[],integer[],kb_actor_identity,text,bytea,kb_sha256),
  kb_bid_v2_retire_workspace_asset(uuid,uuid,text,kb_actor_identity,text,bytea,kb_sha256),
  kb_bid_v2_create_outline_checkpoint(uuid,uuid,kb_sha256,uuid,kb_actor_identity,text,bytea,kb_sha256),
  kb_bid_v2_commit_workspace_mutation_idempotent(uuid,uuid,kb_sha256,jsonb,kb_actor_identity,text,bytea,kb_sha256),
  kb_bid_v2_create_outline_candidate(uuid,uuid,kb_sha256,uuid,kb_sha256,kb_actor_identity,text,bytea,kb_sha256),
  kb_bid_v2_create_content_request(uuid,uuid,kb_sha256,text,text,uuid,text,jsonb,text,uuid,kb_actor_identity,text,bytea,kb_sha256),
  kb_bid_v2_create_evidence_pick_set(uuid,uuid,uuid[],kb_actor_identity,text,bytea,kb_sha256),
  kb_bid_v2_create_node_evidence_pick_set(uuid,uuid,uuid,uuid[],kb_actor_identity,text,bytea,kb_sha256),
  kb_bid_v2_list_evidence_pick_sets(uuid,kb_actor_identity),
  kb_bid_v2_get_node_evidence(uuid,uuid,kb_actor_identity),
  kb_bid_v2_get_evidence_overview(uuid,kb_actor_identity),
  kb_bid_v2_get_current_assessments(uuid,kb_actor_identity),
  kb_bid_v2_load_preview_input(uuid,kb_actor_identity),
  kb_bid_v2_get_preview_html(uuid,kb_actor_identity),
  kb_bid_v2_create_submission_export_request(uuid,uuid,kb_sha256,text,text,jsonb,kb_actor_identity,text,bytea,kb_sha256),
  kb_bid_v2_list_submission_exports(uuid,kb_actor_identity),
  kb_bid_v2_get_submission_export(uuid,uuid,kb_actor_identity),
  kb_bid_v2_get_submission_assessment_report(uuid,uuid,kb_actor_identity),
  kb_bid_v2_get_submission_export_object(uuid,uuid,kb_actor_identity),
  kb_bid_v2_get_async_request(uuid,uuid,kb_actor_identity),
  kb_bid_v2_get_candidate(uuid,uuid,kb_actor_identity),
  kb_bid_v2_accept_candidate(uuid,uuid,uuid,kb_sha256,jsonb,integer[],kb_actor_identity,text,bytea,kb_sha256),
  kb_bid_v2_reject_candidate(uuid,uuid,kb_actor_identity,text,bytea,kb_sha256)
  TO kb_runtime_api;
