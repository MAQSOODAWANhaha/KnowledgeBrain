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
  UNIQUE(project_id,document_set_id,disposition_set_id),
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
  fulfillment_expr jsonb NOT NULL CHECK (jsonb_typeof(fulfillment_expr)='object'),
  applicability jsonb NOT NULL CHECK (jsonb_typeof(applicability)='object'),
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

CREATE TABLE bid_requirement_supersession_revision_artifacts (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  lineage_id uuid NOT NULL,
  revision bigint NOT NULL CHECK (revision > 0),
  old_requirement_revision_id uuid NOT NULL,
  new_requirement_revision_id uuid NOT NULL,
  applicability jsonb NOT NULL CHECK (jsonb_typeof(applicability)='object'),
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
  FOREIGN KEY(project_id,workspace_id) REFERENCES bid_submission_workspaces(project_id,id),
  FOREIGN KEY(project_id,parent_revision_id,parent_sha256) REFERENCES bid_workspace_revision_artifacts(project_id,id,content_sha256),
  FOREIGN KEY(project_id,scope_revision_id) REFERENCES bid_workspace_scope_revision_artifacts(project_id,id),
  FOREIGN KEY(project_id,workspace_id,requirement_projection_id,requirement_projection_sha256)
    REFERENCES bid_workspace_requirement_projection_artifacts(project_id,workspace_id,id,content_sha256),
  FOREIGN KEY(project_id,document_settings_revision_id) REFERENCES bid_document_settings_revision_artifacts(project_id,id),
  CHECK ((parent_revision_id IS NULL)=(parent_sha256 IS NULL)),
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
    'WORKSPACE_CAS_CONFLICT','AGENT_OUTPUT_INVALID','EVIDENCE_UNAVAILABLE','ASSET_MISSING',
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
  stage_kind text NOT NULL CHECK (stage_kind IN ('conversion','extraction','requirement_compile','evidence_match','agent_generate','assessment','attachment_prepare','render_snapshot','manifest','render','object_commit')),
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
  response_sha256 kb_sha256 NOT NULL,
  decided_at timestamptz NOT NULL DEFAULT now()
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
DECLARE p jsonb:=NEW.canonical_payload; item jsonb; keys text[]; bounds jsonb;
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
      IF NOT kb_bid_v2_json_keys_exact(item,ARRAY['kind','evidence_item_id','document_id','source_chunk_id','product_version_id','workspace_kind','frozen_document_display_name','quote_utf8','quote_sha256','quote_start_offset','quote_end_offset','retrieval_rank','retrieval_contract_version'])
         OR NOT kb_bid_v2_uuid_text(item->>'document_id') OR NOT kb_bid_v2_uuid_text(item->>'source_chunk_id')
         OR NOT kb_bid_v2_uuid_text(item->>'product_version_id') OR jsonb_typeof(item->'workspace_kind') IS DISTINCT FROM 'string' OR COALESCE(item->>'workspace_kind','') NOT IN ('product_line','company')
         OR jsonb_typeof(item->'frozen_document_display_name') IS DISTINCT FROM 'string' OR octet_length(item->>'frozen_document_display_name') NOT BETWEEN 1 AND 1024
         OR jsonb_typeof(item->'quote_utf8') IS DISTINCT FROM 'string' OR octet_length(item->>'quote_utf8') NOT BETWEEN 1 AND 1048576
         OR jsonb_typeof(item->'quote_sha256') IS DISTINCT FROM 'string' OR NOT kb_bid_v2_sha256_text(item->>'quote_sha256')
         OR item->>'quote_sha256' IS DISTINCT FROM kb_bid_v2_sha256_bytes(convert_to(item->>'quote_utf8','UTF8'))
         OR NOT EXISTS (
           SELECT 1 FROM documents knowledge_document
           JOIN chunks knowledge_chunk
             ON knowledge_chunk.id=(item->>'source_chunk_id')::uuid
            AND knowledge_chunk.document_id=knowledge_document.id
            AND knowledge_chunk.product_version_id=knowledge_document.product_version_id
          WHERE knowledge_document.id=(item->>'document_id')::uuid
            AND knowledge_document.product_version_id=(item->>'product_version_id')::uuid
            AND knowledge_document.deleted_at IS NULL
            AND knowledge_document.parse_status='completed'
            AND knowledge_document.index_ready
         ) OR COALESCE(item->>'quote_start_offset','') !~ '^(0|[1-9][0-9]*)$'
         OR jsonb_typeof(item->'quote_start_offset') IS DISTINCT FROM 'number' OR jsonb_typeof(item->'quote_end_offset') IS DISTINCT FROM 'number' OR jsonb_typeof(item->'retrieval_rank') IS DISTINCT FROM 'number'
         OR item->>'quote_end_offset' !~ '^[1-9][0-9]*$' OR item->>'retrieval_rank' !~ '^[1-9][0-9]*$'
         OR (item->>'quote_end_offset')::bigint <= (item->>'quote_start_offset')::bigint
         OR jsonb_typeof(item->'retrieval_contract_version') IS DISTINCT FROM 'string' OR octet_length(item->>'retrieval_contract_version') NOT BETWEEN 1 AND 128
      THEN RAISE EXCEPTION 'EvidenceBundleV1 text item invalid' USING ERRCODE='23514'; END IF;
    ELSIF item->>'kind' IS NOT DISTINCT FROM 'image' THEN
      keys:=ARRAY(SELECT key FROM jsonb_object_keys(item) key ORDER BY key);
      IF keys<>ARRAY(SELECT key FROM unnest(ARRAY['kind','evidence_item_id','image_artifact_revision_id','object_ref','sha256','media_type','width','height','frozen_document_display_name','page_ordinal','bounding_region']::text[]) key ORDER BY key)
         AND keys<>ARRAY(SELECT key FROM unnest(ARRAY['kind','evidence_item_id','image_artifact_revision_id','object_ref','sha256','media_type','width','height','frozen_document_display_name','page_ordinal']::text[]) key ORDER BY key)
         AND keys<>ARRAY(SELECT key FROM unnest(ARRAY['kind','evidence_item_id','image_artifact_revision_id','object_ref','sha256','media_type','width','height','frozen_document_display_name','bounding_region']::text[]) key ORDER BY key)
         AND keys<>ARRAY(SELECT key FROM unnest(ARRAY['kind','evidence_item_id','image_artifact_revision_id','object_ref','sha256','media_type','width','height','frozen_document_display_name']::text[]) key ORDER BY key)
      THEN RAISE EXCEPTION 'EvidenceBundleV1 image item keys invalid' USING ERRCODE='23514'; END IF;
      IF NOT kb_bid_v2_uuid_text(item->>'image_artifact_revision_id') OR item->>'object_ref' IS DISTINCT FROM ('objects/'||(item->>'sha256'))
         OR jsonb_typeof(item->'sha256') IS DISTINCT FROM 'string' OR NOT kb_bid_v2_sha256_text(item->>'sha256')
         OR jsonb_typeof(item->'media_type') IS DISTINCT FROM 'string' OR COALESCE(item->>'media_type','') NOT IN ('image/png','image/jpeg','image/webp')
         OR jsonb_typeof(item->'width') IS DISTINCT FROM 'number' OR jsonb_typeof(item->'height') IS DISTINCT FROM 'number'
         OR COALESCE(item->>'width','') !~ '^[1-9][0-9]*$' OR COALESCE(item->>'height','') !~ '^[1-9][0-9]*$'
         OR jsonb_typeof(item->'frozen_document_display_name') IS DISTINCT FROM 'string' OR octet_length(item->>'frozen_document_display_name') NOT BETWEEN 1 AND 1024
         OR (item ? 'page_ordinal' AND jsonb_typeof(item->'page_ordinal') IS DISTINCT FROM 'null'
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
  object_state text NOT NULL DEFAULT 'available' CHECK (object_state='available'),
  byte_length bigint NOT NULL CHECK (byte_length > 0),
  source text NOT NULL CHECK (source='human_upload'),
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

CREATE TABLE bid_submission_fulfillment_evidence_revision_artifacts (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  workspace_revision_id uuid NOT NULL,
  binding_revision_id uuid NOT NULL,
  target_revision_id uuid NOT NULL,
  target_kind text NOT NULL CHECK (target_kind IN ('block','table_row','structured_value','asset','quote_snapshot')),
  dependency_sha256 kb_sha256 NOT NULL,
  stale boolean NOT NULL DEFAULT false,
  canonical_payload bytea NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(project_id,id),
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
  generation bigint NOT NULL CHECK (generation > 0),
  created_at timestamptz NOT NULL,
  FOREIGN KEY(scope_id,artifact_id) REFERENCES bid_quote_snapshot_artifacts(project_id,id)
);

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

ALTER TABLE bid_outline_assessment_snapshot_artifacts
  ADD CONSTRAINT bid_outline_assessment_quote_snapshot_fk
  FOREIGN KEY(project_id,quote_snapshot_id,quote_snapshot_sha256)
  REFERENCES bid_quote_snapshot_artifacts(project_id,id,content_sha256);

ALTER TABLE bid_submission_assessment_snapshot_artifacts
  ADD CONSTRAINT bid_submission_assessment_quote_snapshot_fk
  FOREIGN KEY(project_id,quote_snapshot_id,quote_snapshot_sha256)
  REFERENCES bid_quote_snapshot_artifacts(project_id,id,content_sha256);

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
  media_type text NOT NULL CHECK (media_type IN ('image/png','image/jpeg','image/webp','application/pdf')),
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
       OR jsonb_typeof(page->'media_type') IS DISTINCT FROM 'string' OR COALESCE(page->>'media_type','') NOT IN ('image/png','image/jpeg','image/webp','application/pdf')
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
  UNIQUE(render_snapshot_id,ordinal),
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
  'schema_version','render_snapshot_id','project_id','workspace_id','workspace_scope','workspace_scope_revision_id',
  'workspace_revision_id','workspace_sha256','outline_checkpoint_id','outline_checkpoint_sha256',
  'requirement_projection_revision_id','requirement_projection_sha256','document_settings_revision_id','document_settings_sha256',
  'submission_assessment_snapshot_id','submission_assessment_snapshot_sha256','output_mode','format','mode_options','ordered_nodes',
  'assets','form_definition_occurrences','attachment_preparation_occurrences','content_block_schema_version','content_block_schema_sha256',
  'render_operation_contract_version','render_operation_contract_sha256','docx_renderer_contract_id','docx_renderer_contract_sha256',
  'pdf_renderer_contract_id','pdf_renderer_contract_sha256','style_contract_id','style_contract_sha256','page_geometry',
  'font_artifact_identities','numbering_policy','toc_policy','snapshot_sha256'])
 OR p->'schema_version' IS DISTINCT FROM '2'::jsonb OR p->'content_block_schema_version' IS DISTINCT FROM '1'::jsonb
 OR EXISTS (SELECT 1 FROM unnest(ARRAY['render_snapshot_id','project_id','workspace_id','workspace_scope_revision_id','workspace_revision_id','outline_checkpoint_id','requirement_projection_revision_id','document_settings_revision_id','submission_assessment_snapshot_id','docx_renderer_contract_id','pdf_renderer_contract_id','style_contract_id']::text[]) key WHERE NOT kb_bid_v2_uuid_text(p->>key))
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
      FROM bid_render_snapshot_block_occurrences b WHERE b.render_snapshot_id=sid AND b.node_occurrence_id=n.node_occurrence_id),'[]'::jsonb)) ORDER BY n.ordinal),'[]'::jsonb)
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
  ADD UNIQUE(project_id,relation_lineage_id,revision,id);
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
  ADD UNIQUE(project_id,lineage_id,revision,id,content_sha256);
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
    'bid_evidence_selection_artifacts','bid_evidence_asset_artifacts','bid_workspace_asset_artifacts',
    'bid_submission_fulfillment_evidence_revision_artifacts','bid_outline_assessment_snapshot_artifacts',
    'bid_submission_assessment_snapshot_artifacts','bid_quote_snapshot_artifacts','bid_quote_snapshot_object_identities',
    'bid_render_style_contract_artifacts','bid_authoring_contract_artifacts','bid_renderer_contract_artifacts','bid_render_font_artifacts','bid_attachment_preparation_revision_artifacts','bid_attachment_preparation_asset_items',
    'bid_render_document_snapshot_artifacts','bid_submission_manifest_artifacts','bid_submission_output_artifacts',
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

-- Atomic one-document publication seam for the inactive V2 worker. Runtime
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
  prior bid_async_stage_receipts%ROWTYPE;
  expected_ordinal integer:=0;
  image_count integer:=0;
BEGIN
  IF jsonb_typeof(p_source)<>'object' OR jsonb_typeof(p_images)<>'array'
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
  IF source_ref<>'objects/'||source_sha OR source_media<>'application/json'
     OR source_length<>octet_length(source_payload)
     OR source_sha<>kb_bid_v2_sha256_bytes(source_payload)
     OR (p_source->>'revision')::bigint<>p_request_revision THEN
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
    converter_contract_sha256,image_asset_set_sha256)
  VALUES(source_id,p_project_id,p_document_id,p_request_revision,source_ref,source_sha,
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
  END LOOP;

  INSERT INTO bid_async_stage_receipts(request_artifact_id,stage_kind,frozen_input_sha256,result_identity,result_sha256)
  VALUES(p_request_artifact_id,'extraction',p_frozen_input_sha256,result_value,result_sha);
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

CREATE FUNCTION kb_bid_v2_load_workspace(p_workspace_id uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE
  w bid_submission_workspaces%ROWTYPE;
  head bid_workspace_heads%ROWTYPE;
  rev bid_workspace_revision_artifacts%ROWTYPE;
  settings bid_document_settings_revision_artifacts%ROWTYPE;
  nodes jsonb := '[]'::jsonb;
  blocks jsonb := '[]'::jsonb;
  node_rec record;
  block_ids uuid[];
  block_id uuid;
  block_rec bid_content_block_revision_artifacts%ROWTYPE;
  block_json jsonb;
  ds_id uuid;
  ds_sha kb_sha256;
BEGIN
  SELECT * INTO w FROM bid_submission_workspaces WHERE id=p_workspace_id;
  IF NOT FOUND THEN RETURN NULL; END IF;
  SELECT * INTO head FROM bid_workspace_heads WHERE scope_id=p_workspace_id;
  IF NOT FOUND THEN RETURN NULL; END IF;
  SELECT * INTO rev FROM bid_workspace_revision_artifacts WHERE id=head.artifact_id AND content_sha256=head.artifact_sha256;
  SELECT * INTO settings FROM bid_document_settings_revision_artifacts WHERE id=rev.document_settings_revision_id;
  SELECT artifact_id, artifact_sha256 INTO ds_id, ds_sha FROM bid_document_set_current WHERE scope_id=w.project_id;
  FOR node_rec IN
    SELECT occ.id occ_id, occ.parent_occurrence_id, occ.ordinal, occ.depth,
           n.lineage_id, n.id revision_id, n.title, n.semantic_role, n.render_role, n.tombstone
      FROM bid_workspace_node_occurrences occ
      JOIN bid_outline_node_revision_artifacts n ON n.id=occ.node_revision_id AND n.project_id=occ.project_id
     WHERE occ.workspace_revision_id=rev.id
     ORDER BY occ.depth, occ.ordinal, occ.id
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
    block_json := block_rec.block_payload;
    block_json := block_json || jsonb_build_object(
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
  RETURN jsonb_build_object(
    'workspace_id', w.id,
    'project_id', w.project_id,
    'revision_id', rev.id,
    'sha256', rev.content_sha256,
    'scope', 'project_wide',
    'outline_checkpoint_id', NULL,
    'outline_checkpoint_sha256', NULL,
    'requirement_projection_revision_id', rev.requirement_projection_id,
    'requirement_projection_sha256', rev.requirement_projection_sha256,
    'document_settings_revision_id', settings.id,
    'document_settings_sha256', settings.content_sha256,
    'document_settings', settings.settings,
    'document_set_revision_id', ds_id,
    'document_set_sha256', ds_sha,
    'nodes', nodes,
    'blocks', blocks,
    'bindings', '[]'::jsonb,
    'quote_snapshot', NULL
  );
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
  node jsonb; block jsonb;
  lineage uuid; rev uuid; parent uuid; parent_occ uuid; occ uuid; block_occ uuid;
  payload bytea; sha kb_sha256; settings_id uuid; settings_sha kb_sha256;
  new_rev uuid := gen_random_uuid(); new_sha kb_sha256; new_payload bytea;
  settings jsonb; ordinal int; depth int;
  node_map jsonb := '{}'::jsonb;
BEGIN
  SELECT * INTO w FROM bid_submission_workspaces WHERE id=p_workspace_id;
  IF NOT FOUND THEN RAISE EXCEPTION 'WORKSPACE_NOT_FOUND' USING ERRCODE='P0002'; END IF;
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
  FOR block IN SELECT value FROM jsonb_array_elements(coalesce(p_snapshot->'blocks','[]'::jsonb))
  LOOP
    lineage := (block->>'lineage_id')::uuid;
    rev := coalesce((block->>'block_revision_id')::uuid, gen_random_uuid());
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
  new_payload := kb_bid_v2_json_payload(p_snapshot); new_sha := kb_bid_v2_sha256_bytes(new_payload);
  INSERT INTO bid_workspace_revision_artifacts(id,project_id,workspace_id,revision,parent_revision_id,parent_sha256,scope_revision_id,requirement_projection_id,requirement_projection_sha256,document_settings_revision_id,canonical_payload,content_sha256,actor)
    VALUES(new_rev,w.project_id,p_workspace_id,cur.revision+1,cur.id,cur.content_sha256,cur.scope_revision_id,cur.requirement_projection_id,cur.requirement_projection_sha256,settings_id,new_payload,new_sha,p_actor);
  FOR node IN SELECT value FROM jsonb_array_elements(coalesce(p_snapshot->'nodes','[]'::jsonb)) ORDER BY coalesce((value->>'ordinal')::int,0)
  LOOP
    NULL; -- occurrences filled below after parent map
  END LOOP;
  -- Insert occurrences in depth order: parents before children.
  FOR node IN
    SELECT value FROM jsonb_array_elements(coalesce(p_snapshot->'nodes','[]'::jsonb))
     ORDER BY CASE WHEN value->>'parent_lineage_id' IS NULL OR value->>'parent_lineage_id'='null' THEN 0 ELSE 1 END, coalesce((value->>'ordinal')::int,0)
  LOOP
    occ := gen_random_uuid();
    lineage := (node->>'lineage_id')::uuid;
    rev := (node->>'revision_id')::uuid;
    parent := NULLIF(node->>'parent_lineage_id','null')::uuid;
    parent_occ := NULLIF(node_map->>parent::text,'')::uuid;
    depth := CASE WHEN parent IS NULL THEN 0 ELSE 1 END;
    IF parent IS NOT NULL THEN
      depth := 1;
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
  IF NOT kb_bid_v2_advance_workspace_head(p_workspace_id,p_expected_revision_id,p_expected_sha256,new_rev,new_sha) THEN
    RAISE EXCEPTION 'WORKSPACE_HEAD_CAS_MISMATCH' USING ERRCODE='40001';
  END IF;
  RETURN kb_bid_v2_load_workspace(p_workspace_id);
END $$;

REVOKE ALL ON ALL TABLES IN SCHEMA public FROM PUBLIC;
REVOKE ALL ON ALL FUNCTIONS IN SCHEMA public FROM PUBLIC;
GRANT SELECT ON bidding_v2_projects,bidding_v2_workspace_heads,bidding_v2_async_requests,bidding_v2_outputs
  TO kb_runtime_api,kb_runtime_worker;
GRANT EXECUTE ON FUNCTION kb_bid_v2_advance_workspace_head(uuid,uuid,kb_sha256,uuid,kb_sha256)
  TO kb_runtime_api;
GRANT EXECUTE ON FUNCTION kb_bid_v2_advance_document_set(uuid,uuid,kb_sha256,uuid,kb_sha256),
  kb_bid_v2_advance_disposition_set(uuid,uuid,kb_sha256,uuid,kb_sha256),
  kb_bid_v2_advance_requirement_supersession(uuid,uuid,uuid,kb_sha256,uuid,kb_sha256),
  kb_bid_v2_advance_requirement_projection(uuid,uuid,uuid,kb_sha256,uuid,kb_sha256)
  TO kb_runtime_api;
GRANT EXECUTE ON FUNCTION kb_bid_v2_publish_requirement_set(uuid,kb_sha256),
  kb_bid_v2_publish_tender_document_process(uuid,bigint,kb_sha256,uuid,uuid,kb_sha256,jsonb,jsonb,jsonb,kb_actor_identity)
  TO kb_runtime_worker;
GRANT EXECUTE ON FUNCTION kb_bid_v2_json_payload(jsonb),
  kb_bid_v2_load_workspace(uuid),
  kb_bid_create_project_v2(uuid,text,uuid,kb_actor_identity),
  kb_bid_v2_commit_workspace_mutation(uuid,uuid,kb_sha256,jsonb,kb_actor_identity)
  TO kb_runtime_api;
