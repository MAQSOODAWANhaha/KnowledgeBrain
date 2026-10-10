-- KnowledgeBrain bidding baseline: outline, response, and the document
-- they compile. Create-only. No upgrade chain. No system actor.
-- Object bytes stay in the shared object registry.

CREATE FUNCTION kb_bid_v2_sha256_bytes(p_bytes bytea)
RETURNS kb_sha256
LANGUAGE sql
IMMUTABLE
STRICT
PARALLEL SAFE
SET search_path = pg_catalog, public
AS $$ SELECT encode(public.digest(p_bytes, 'sha256'), 'hex')::kb_sha256 $$;

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
  media_type text NOT NULL CHECK (octet_length(media_type) BETWEEN 1 AND 256),
  object_ref kb_object_ref NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  byte_length bigint NOT NULL CHECK (byte_length > 0),
  object_state text NOT NULL DEFAULT 'available' CHECK (object_state = 'available'),
  FOREIGN KEY (object_ref, content_sha256, media_type, byte_length, object_state)
    REFERENCES object_registry(object_ref, digest, media_type, byte_length, state),
  created_by kb_actor_identity,
  created_at timestamptz NOT NULL DEFAULT now(),
  CHECK (object_ref = 'objects/' || content_sha256)
);

CREATE TABLE bid_outline_runs (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
  status text NOT NULL CHECK (status IN ('pending','running','failed','paused_budget','published')),
  checkpoint jsonb NOT NULL CHECK (jsonb_typeof(checkpoint) = 'object'),
  outline_sha256 kb_sha256,
  frozen_input_sha256 kb_sha256 NOT NULL,
  bindings_sha256 kb_sha256,
  published_by kb_actor_identity,
  lease_epoch bigint NOT NULL DEFAULT 0 CHECK (lease_epoch >= 0),
  lease_token uuid,
  lease_until timestamptz,
  created_by kb_actor_identity,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  CHECK ((status = 'published') = (outline_sha256 IS NOT NULL)),
  CHECK ((lease_token IS NULL) = (lease_until IS NULL))
);

CREATE TABLE bid_frozen_inputs (
  input_sha256 kb_sha256 NOT NULL,
  project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
  document_set_id text,
  publication_id uuid UNIQUE,
  PRIMARY KEY (project_id, input_sha256)
);

ALTER TABLE bid_outline_runs
  ADD FOREIGN KEY (project_id, frozen_input_sha256)
    REFERENCES bid_frozen_inputs(project_id, input_sha256);

-- One immutable object manifest per frozen snapshot. Images and canonical JSON
-- acquire the same durable snapshot owner in the registration transaction.
CREATE TABLE bid_frozen_input_objects (
  project_id uuid NOT NULL,
  input_sha256 kb_sha256 NOT NULL,
  occurrence text NOT NULL CHECK (octet_length(occurrence) BETWEEN 1 AND 128),
  object_ref kb_object_ref NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  media_type text NOT NULL,
  byte_length bigint NOT NULL CHECK (byte_length > 0),
  object_state text NOT NULL DEFAULT 'available' CHECK (object_state = 'available'),
  PRIMARY KEY (project_id, input_sha256, occurrence),
  FOREIGN KEY (project_id, input_sha256) REFERENCES bid_frozen_inputs(project_id, input_sha256),
  FOREIGN KEY (object_ref, content_sha256, media_type, byte_length, object_state)
    REFERENCES object_registry(object_ref, digest, media_type, byte_length, state),
  CHECK (object_ref = 'objects/' || content_sha256)
);

CREATE TABLE bid_outline_artifacts (
  sha256 kb_sha256 PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
  run_id uuid NOT NULL UNIQUE REFERENCES bid_outline_runs(id) ON DELETE RESTRICT,
  frozen_input_sha256 kb_sha256 NOT NULL,
  artifact jsonb NOT NULL CHECK (jsonb_typeof(artifact) = 'object'),
  created_by kb_actor_identity,
  created_at timestamptz NOT NULL DEFAULT now(),
  CONSTRAINT bid_outline_artifacts_frozen_input_fkey
    FOREIGN KEY (project_id, frozen_input_sha256)
    REFERENCES bid_frozen_inputs(project_id, input_sha256) ON DELETE RESTRICT
);

ALTER TABLE bid_outline_runs
  ADD FOREIGN KEY (outline_sha256) REFERENCES bid_outline_artifacts(sha256) ON DELETE RESTRICT;

CREATE TABLE bid_outline_chapters (
  outline_sha256 kb_sha256 NOT NULL REFERENCES bid_outline_artifacts(sha256) ON DELETE RESTRICT,
  chapter_id text NOT NULL CHECK (octet_length(chapter_id) BETWEEN 1 AND 256),
  parent_id text,
  ordinal integer NOT NULL CHECK (ordinal >= 0),
  title text NOT NULL CHECK (octet_length(title) BETWEEN 1 AND 2048),
  purpose text NOT NULL CHECK (purpose IN ('group','response')),
  PRIMARY KEY (outline_sha256, chapter_id),
  UNIQUE NULLS NOT DISTINCT (outline_sha256, parent_id, ordinal),
  FOREIGN KEY (outline_sha256, parent_id)
    REFERENCES bid_outline_chapters(outline_sha256, chapter_id)
    DEFERRABLE INITIALLY DEFERRED
);

CREATE TABLE bid_outline_attachment_bindings (
  outline_sha256 kb_sha256 NOT NULL,
  form_id text NOT NULL CHECK (octet_length(form_id) BETWEEN 1 AND 256),
  chapter_id text NOT NULL,
  PRIMARY KEY (outline_sha256, form_id),
  FOREIGN KEY (outline_sha256, chapter_id)
    REFERENCES bid_outline_chapters(outline_sha256, chapter_id)
);

CREATE TABLE bid_outline_template_slots (
  outline_sha256 kb_sha256 NOT NULL REFERENCES bid_outline_artifacts(sha256) ON DELETE RESTRICT,
  slot_id text NOT NULL CHECK (octet_length(slot_id) BETWEEN 1 AND 256),
  chapter_id text NOT NULL,
  kind text NOT NULL CHECK (kind IN (
    'fixed_text','tender_value','instruction','bidder_blank','signature','preserved'
  )),
  body text NOT NULL,
  response_required boolean NOT NULL,
  match_query text NOT NULL,
  PRIMARY KEY (outline_sha256, slot_id),
  FOREIGN KEY (outline_sha256, chapter_id)
    REFERENCES bid_outline_chapters(outline_sha256, chapter_id),
  CHECK (response_required = (kind IN ('bidder_blank','signature'))),
  CHECK (
    (response_required AND body = '' AND octet_length(match_query) > 0)
    OR (NOT response_required AND match_query = '')
  )
);

CREATE TABLE bid_response_sets (
  sha256 kb_sha256 PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
  outline_sha256 kb_sha256 NOT NULL REFERENCES bid_outline_artifacts(sha256) ON DELETE RESTRICT,
  created_by kb_actor_identity,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE bid_response_slots (
  response_sha256 kb_sha256 NOT NULL REFERENCES bid_response_sets(sha256) ON DELETE RESTRICT,
  slot_id text NOT NULL,
  chapter_id text NOT NULL,
  status text NOT NULL CHECK (status IN ('matched','no_evidence')),
  body text NOT NULL,
  evidence jsonb NOT NULL CHECK (jsonb_typeof(evidence) = 'array'),
  PRIMARY KEY (response_sha256, slot_id),
  CHECK (
    (status = 'no_evidence' AND body = '【待人工补充】' AND evidence = '[]'::jsonb)
    OR (status = 'matched' AND body <> '【待人工补充】')
  )
);

CREATE TABLE bid_docx_versions (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
  outline_sha256 kb_sha256 NOT NULL REFERENCES bid_outline_artifacts(sha256) ON DELETE RESTRICT,
  revision bigint NOT NULL CHECK (revision > 0),
  parent_id uuid,
  object_ref kb_object_ref NOT NULL,
  docx_sha256 kb_sha256 NOT NULL,
  byte_length bigint NOT NULL CHECK (byte_length > 0),
  media_type text NOT NULL DEFAULT 'application/vnd.openxmlformats-officedocument.wordprocessingml.document'
    CHECK (media_type = 'application/vnd.openxmlformats-officedocument.wordprocessingml.document'),
  object_state text NOT NULL DEFAULT 'available' CHECK (object_state = 'available'),
  FOREIGN KEY (object_ref, docx_sha256, media_type, byte_length, object_state)
    REFERENCES object_registry(object_ref, digest, media_type, byte_length, state),
  created_by kb_actor_identity,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (project_id, outline_sha256, revision),
  UNIQUE (project_id, outline_sha256, id),
  FOREIGN KEY (project_id, outline_sha256, parent_id)
    REFERENCES bid_docx_versions(project_id, outline_sha256, id),
  CHECK ((revision = 1) = (parent_id IS NULL)),
  CHECK (object_ref = 'objects/' || docx_sha256)
);

CREATE TABLE bid_docx_current (
  project_id uuid NOT NULL,
  outline_sha256 kb_sha256 NOT NULL,
  version_id uuid NOT NULL,
  docx_sha256 kb_sha256 NOT NULL,
  editor_key uuid,
  editor_base_version_id uuid,
  pending_save_id uuid,
  editor_error jsonb,
  PRIMARY KEY (project_id, outline_sha256),
  CHECK ((editor_key IS NULL) = (editor_base_version_id IS NULL)),
  CHECK (editor_key IS NOT NULL OR (pending_save_id IS NULL AND editor_error IS NULL)),
  FOREIGN KEY (project_id, outline_sha256, version_id)
    REFERENCES bid_docx_versions(project_id, outline_sha256, id),
  FOREIGN KEY (project_id, outline_sha256, editor_base_version_id)
    REFERENCES bid_docx_versions(project_id, outline_sha256, id)
);

CREATE TABLE bid_submission_exports (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL,
  outline_sha256 kb_sha256 NOT NULL,
  response_sha256 kb_sha256 NOT NULL REFERENCES bid_response_sets(sha256) ON DELETE RESTRICT,
  docx_version_id uuid NOT NULL,
  object_ref kb_object_ref NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  media_type text NOT NULL,
  byte_length bigint NOT NULL CHECK (byte_length > 0),
  object_state text NOT NULL DEFAULT 'available' CHECK (object_state = 'available'),
  FOREIGN KEY (object_ref, content_sha256, media_type, byte_length, object_state)
    REFERENCES object_registry(object_ref, digest, media_type, byte_length, state),
  created_by kb_actor_identity,
  created_at timestamptz NOT NULL DEFAULT now(),
  FOREIGN KEY (project_id, outline_sha256, docx_version_id)
    REFERENCES bid_docx_versions(project_id, outline_sha256, id),
  CHECK (object_ref = 'objects/' || content_sha256)
);

CREATE FUNCTION kb_bid_v2_require_project_owner(
  p_project_id uuid, p_actor kb_actor_identity
) RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE
  owner uuid;
BEGIN
  SELECT owner_user_id INTO STRICT owner FROM bid_projects WHERE id = p_project_id;
  IF p_actor IS NOT NULL AND p_actor IS DISTINCT FROM ('user:' || owner::text)::kb_actor_identity THEN
    RAISE EXCEPTION 'project owner mismatch' USING ERRCODE = '42501';
  END IF;
END
$$;

-- Frozen identities are scoped to their project. Registering an existing SHA
-- may not silently change the document set it identifies.
CREATE FUNCTION kb_bid_v2_register_frozen_input(
  p_project_id uuid, p_sha256 kb_sha256, p_document_set_id text
) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, public AS $$
BEGIN
  INSERT INTO bid_frozen_inputs(project_id, input_sha256, document_set_id)
  VALUES (p_project_id, p_sha256, p_document_set_id)
  ON CONFLICT (project_id, input_sha256) DO NOTHING;
  IF NOT EXISTS (SELECT 1 FROM bid_frozen_inputs WHERE project_id = p_project_id
      AND input_sha256 = p_sha256 AND document_set_id IS NOT DISTINCT FROM p_document_set_id) THEN
    RAISE EXCEPTION 'BID_FROZEN_INPUT_IDENTITY_MISMATCH' USING ERRCODE = '23514';
  END IF;
END $$;

CREATE FUNCTION kb_bid_v2_publish_frozen_input(
  p_project_id uuid, p_sha256 kb_sha256, p_input_bytes bytea, p_document_set_id text,
  p_publication_id uuid, p_objects jsonb, p_actor kb_actor_identity
) RETURNS kb_sha256 LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, public AS $$
DECLARE
  v_owner_id uuid;
  item jsonb;
  staging_id uuid;
  frozen jsonb;
  occurrence_key text;
  required_keys text[] := ARRAY['frozen-json'];
BEGIN
  PERFORM kb_bid_v2_require_project_owner(p_project_id, p_actor);
  IF p_publication_id IS NULL OR jsonb_typeof(p_objects) IS DISTINCT FROM 'array'
      OR jsonb_array_length(p_objects) = 0 THEN
    RAISE EXCEPTION 'frozen input object manifest missing' USING ERRCODE = '23514';
  END IF;
  IF (SELECT count(*) FROM jsonb_array_elements(p_objects) obj
       WHERE obj->>'occurrence' = 'frozen-json' AND obj->>'digest' = p_sha256::text
         AND obj->>'media_type' = 'application/json') <> 1
     OR (SELECT count(DISTINCT obj->>'occurrence') FROM jsonb_array_elements(p_objects) obj)
         <> jsonb_array_length(p_objects) THEN
    RAISE EXCEPTION 'invalid frozen input object manifest' USING ERRCODE = '23514';
  END IF;
  IF kb_bid_v2_sha256_bytes(p_input_bytes) IS DISTINCT FROM p_sha256 THEN
    RAISE EXCEPTION 'frozen bytes digest mismatch' USING ERRCODE='23514';
  END IF;
  frozen := convert_from(p_input_bytes,'UTF8')::jsonb;
  IF frozen->>'schema_version' IS DISTINCT FROM '2'
      OR frozen->>'project_id' IS DISTINCT FROM p_project_id::text
      OR frozen->>'document_set_id' IS DISTINCT FROM p_document_set_id
      OR jsonb_typeof(frozen->'documents') IS DISTINCT FROM 'array'
      OR jsonb_typeof(frozen->'source_units') IS DISTINCT FROM 'array'
      OR jsonb_array_length(frozen->'documents') = 0
      OR NOT EXISTS(SELECT 1 FROM jsonb_array_elements(p_objects) obj
        WHERE obj->>'occurrence'='frozen-json' AND (obj->>'byte_length')::bigint=octet_length(p_input_bytes)) THEN
    RAISE EXCEPTION 'frozen object identity mismatch' USING ERRCODE='23514';
  END IF;
  FOR item IN SELECT value FROM jsonb_array_elements(frozen->'documents')
  LOOP
    occurrence_key := 'source:' || kb_bid_v2_sha256_bytes(convert_to(item->>'document_id','UTF8'));
    required_keys := array_append(required_keys,occurrence_key);
    IF item->>'document_revision' IS DISTINCT FROM item#>>'{source_contract,document_revision}'
        OR NOT EXISTS(SELECT 1 FROM jsonb_array_elements(p_objects) obj
          WHERE obj->>'occurrence'=occurrence_key AND obj->>'digest'=item->>'document_revision') THEN
      RAISE EXCEPTION 'frozen document differs from staged source bytes' USING ERRCODE='23514';
    END IF;
  END LOOP;
  FOR item IN SELECT value FROM jsonb_array_elements(frozen->'source_units')
      WHERE value#>>'{locator,image_ref}' IS NOT NULL
  LOOP
    occurrence_key := 'image:' || kb_bid_v2_sha256_bytes(convert_to(
      octet_length(item->>'document_id')::text || ':' || (item->>'document_id') ||
      octet_length(item#>>'{locator,unit_id}')::text || ':' || (item#>>'{locator,unit_id}'),'UTF8'));
    required_keys := array_append(required_keys,occurrence_key);
    IF NOT EXISTS(SELECT 1 FROM jsonb_array_elements(p_objects) obj
        WHERE obj->>'occurrence'=occurrence_key AND obj->>'object_ref'=item#>>'{locator,image_ref}') THEN
      RAISE EXCEPTION 'frozen image differs from staged image bytes' USING ERRCODE='23514';
    END IF;
  END LOOP;
  IF cardinality(required_keys) <> jsonb_array_length(p_objects)
      OR EXISTS(SELECT 1 FROM jsonb_array_elements(p_objects) obj WHERE NOT obj->>'occurrence'=ANY(required_keys)) THEN
    RAISE EXCEPTION 'frozen object manifest has extra or duplicated evidence' USING ERRCODE='23514';
  END IF;
  PERFORM kb_bid_v2_register_frozen_input(p_project_id, p_sha256, p_document_set_id);
  SELECT publication_id INTO v_owner_id FROM bid_frozen_inputs
  WHERE project_id = p_project_id AND input_sha256 = p_sha256 FOR UPDATE;
  IF v_owner_id IS NULL THEN
    v_owner_id := p_publication_id;
    UPDATE bid_frozen_inputs SET publication_id = v_owner_id
    WHERE project_id = p_project_id AND input_sha256 = p_sha256;
  ELSIF EXISTS (SELECT 1 FROM bid_frozen_input_objects existing
    WHERE existing.project_id = p_project_id AND existing.input_sha256 = p_sha256
      AND NOT EXISTS (SELECT 1 FROM jsonb_array_elements(p_objects) obj
        WHERE obj->>'occurrence' = existing.occurrence AND obj->>'object_ref' = existing.object_ref
          AND obj->>'digest' = existing.content_sha256 AND obj->>'media_type' = existing.media_type
          AND (obj->>'byte_length')::bigint = existing.byte_length))
      OR (SELECT count(*) FROM bid_frozen_input_objects
          WHERE project_id = p_project_id AND input_sha256 = p_sha256) <> jsonb_array_length(p_objects) THEN
    RAISE EXCEPTION 'frozen input object manifest changed' USING ERRCODE = '23514';
  END IF;
  FOR item IN SELECT value FROM jsonb_array_elements(p_objects)
      ORDER BY value->>'object_ref', value->>'occurrence'
  LOOP
    staging_id := (item->>'staging_id')::uuid;
    IF staging_id IS NOT NULL
        AND NOT EXISTS (SELECT 1 FROM object_upload_staging WHERE id=staging_id)
        AND EXISTS (SELECT 1 FROM object_owner_references
          WHERE owner_kind='bid_frozen_input' AND object_owner_references.owner_id=v_owner_id
            AND occurrence=item->>'occurrence' AND object_ref=item->>'object_ref') THEN
      -- The exact immutable manifest already owns the object; recover a lost
      -- commit ACK without requiring staging that the transaction consumed.
      staging_id := NULL;
    END IF;
    PERFORM kb_object_publish_reference(staging_id,
      (item->>'object_ref')::kb_object_ref, (item->>'digest')::kb_sha256,
      item->>'media_type', (item->>'byte_length')::bigint,
      'bid_frozen_input', v_owner_id, item->>'occurrence', p_actor);
    INSERT INTO bid_frozen_input_objects(project_id, input_sha256, occurrence,
      object_ref, content_sha256, media_type, byte_length)
    VALUES (p_project_id, p_sha256, item->>'occurrence', (item->>'object_ref')::kb_object_ref,
      (item->>'digest')::kb_sha256, item->>'media_type', (item->>'byte_length')::bigint)
    ON CONFLICT (project_id, input_sha256, occurrence) DO NOTHING;
  END LOOP;
  RETURN p_sha256;
END $$;

CREATE FUNCTION kb_bid_v2_outline_claim(
  p_run_id uuid, p_project_id uuid, p_frozen_input_sha256 kb_sha256,
  p_token uuid, p_seconds integer
) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, public AS $$
DECLARE claimed bid_outline_runs%ROWTYPE;
BEGIN
  IF p_token IS NULL OR p_seconds IS NULL OR p_seconds NOT BETWEEN 1 AND 3600 THEN
    RAISE EXCEPTION 'invalid outline lease' USING ERRCODE = '23514';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM bid_frozen_inputs WHERE project_id = p_project_id
      AND input_sha256 = p_frozen_input_sha256) THEN
    RAISE EXCEPTION 'BID_FROZEN_INPUT_UNKNOWN' USING ERRCODE = '23503';
  END IF;
  INSERT INTO bid_outline_runs(id, project_id, frozen_input_sha256, status, checkpoint)
  VALUES (p_run_id, p_project_id, p_frozen_input_sha256, 'pending', '{}')
  ON CONFLICT (id) DO NOTHING;
  UPDATE bid_outline_runs
  SET status = 'running', lease_token = p_token, lease_epoch = lease_epoch + 1,
      lease_until = clock_timestamp() + make_interval(secs => p_seconds),
      updated_at = clock_timestamp()
  WHERE id = p_run_id AND project_id = p_project_id
    AND frozen_input_sha256 = p_frozen_input_sha256 AND status IN ('pending','running','failed')
    AND (lease_until IS NULL OR lease_until <= clock_timestamp())
  RETURNING * INTO claimed;
  IF claimed.id IS NULL THEN
    RAISE EXCEPTION 'outline run is published or leased or identity mismatched' USING ERRCODE = '40001';
  END IF;
  RETURN jsonb_build_object('run_id', claimed.id, 'status', claimed.status,
      'lease_token', claimed.lease_token, 'lease_epoch', claimed.lease_epoch);
END $$;

CREATE FUNCTION kb_bid_v2_outline_renew(
  p_run_id uuid, p_token uuid, p_epoch bigint, p_seconds integer
) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, public AS $$
BEGIN
  IF p_seconds IS NULL OR p_seconds NOT BETWEEN 1 AND 3600 THEN
    RAISE EXCEPTION 'invalid outline lease' USING ERRCODE = '23514';
  END IF;
  UPDATE bid_outline_runs
  SET lease_until = clock_timestamp() + make_interval(secs => p_seconds), updated_at = clock_timestamp()
  WHERE id = p_run_id AND status = 'running' AND lease_token = p_token
    AND lease_epoch = p_epoch AND lease_until > clock_timestamp();
  IF NOT FOUND THEN
    RAISE EXCEPTION 'BID_OUTLINE_LEASE_LOST' USING ERRCODE = '40001';
  END IF;
END $$;

-- A retryable failure may release only its still-current live acquisition.
CREATE FUNCTION kb_bid_v2_outline_release(
  p_run_id uuid, p_token uuid, p_epoch bigint
) RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, public AS $$
BEGIN
  UPDATE bid_outline_runs SET status = 'pending', lease_token = NULL,
    lease_until = NULL, updated_at = clock_timestamp()
  WHERE id = p_run_id AND status = 'running' AND lease_token = p_token
    AND lease_epoch = p_epoch AND lease_until > clock_timestamp();
  RETURN FOUND;
END $$;

CREATE FUNCTION kb_bid_v2_outline_pause_budget(
  p_run_id uuid, p_token uuid, p_epoch bigint
) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, public AS $$
BEGIN
  UPDATE bid_outline_runs SET status = 'paused_budget', lease_token = NULL,
    lease_until = NULL, updated_at = clock_timestamp()
  WHERE id = p_run_id AND status = 'running' AND lease_token = p_token
    AND lease_epoch = p_epoch AND lease_until > clock_timestamp();
  IF NOT FOUND THEN
    RAISE EXCEPTION 'BID_OUTLINE_LEASE_LOST' USING ERRCODE = '40001';
  END IF;
END $$;

-- The lock is held by the caller's transaction while the application validates
-- an explicit operator grant against the previous immutable Config hash.
CREATE FUNCTION kb_bid_v2_outline_paused_checkpoint(p_run_id uuid)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, public AS $$
DECLARE current_run bid_outline_runs%ROWTYPE;
BEGIN
  SELECT * INTO current_run FROM bid_outline_runs WHERE id=p_run_id FOR UPDATE;
  IF NOT FOUND OR current_run.status <> 'paused_budget' THEN
    RAISE EXCEPTION 'outline run is not paused for budget' USING ERRCODE='40001';
  END IF;
  RETURN jsonb_build_object('checkpoint',current_run.checkpoint,
    'checkpoint_sha256',kb_bid_v2_sha256_bytes(convert_to(current_run.checkpoint::text,'UTF8')));
END $$;

CREATE FUNCTION kb_bid_v2_outline_resume_budget(
  p_run_id uuid, p_expected_checkpoint_sha256 kb_sha256, p_checkpoint jsonb
) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, public AS $$
DECLARE current_run bid_outline_runs%ROWTYPE;
BEGIN
  SELECT * INTO current_run FROM bid_outline_runs WHERE id=p_run_id FOR UPDATE;
  IF NOT FOUND OR current_run.status <> 'paused_budget'
      OR kb_bid_v2_sha256_bytes(convert_to(current_run.checkpoint::text,'UTF8')) IS DISTINCT FROM p_expected_checkpoint_sha256 THEN
    RAISE EXCEPTION 'paused checkpoint changed' USING ERRCODE='40001';
  END IF;
  IF (p_checkpoint - 'config_sha256' #- '{journal,budget,paused_reason}')
       IS DISTINCT FROM (current_run.checkpoint - 'config_sha256' #- '{journal,budget,paused_reason}')
      OR current_run.checkpoint->>'done' IS DISTINCT FROM 'false'
      OR current_run.checkpoint#>>'{journal,budget,paused_reason}' IS NULL
      OR p_checkpoint->>'config_sha256' IS NULL
      OR p_checkpoint->>'config_sha256' = current_run.checkpoint->>'config_sha256'
      OR p_checkpoint#>>'{journal,budget,paused_reason}' IS NOT NULL THEN
    RAISE EXCEPTION 'budget resume must preserve all work and accounting' USING ERRCODE='23514';
  END IF;
  UPDATE bid_outline_runs SET checkpoint=p_checkpoint, status='pending',
    lease_token=NULL, lease_until=NULL, updated_at=clock_timestamp() WHERE id=p_run_id;
END $$;

CREATE FUNCTION kb_bid_v2_outline_checkpoint(
  p_run_id uuid, p_token uuid, p_epoch bigint, p_checkpoint jsonb
) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, public AS $$
BEGIN
  UPDATE bid_outline_runs SET checkpoint = p_checkpoint, updated_at = clock_timestamp()
  WHERE id = p_run_id AND status = 'running' AND lease_token = p_token
    AND lease_epoch = p_epoch AND lease_until > clock_timestamp();
  IF NOT FOUND THEN
    RAISE EXCEPTION 'BID_OUTLINE_LEASE_LOST' USING ERRCODE = '40001';
  END IF;
END $$;

CREATE FUNCTION kb_bid_v2_publish_outline(
  p_run_id uuid,
  p_project_id uuid,
  p_frozen_input_sha256 kb_sha256,
  p_lease_token uuid,
  p_lease_epoch bigint,
  p_artifact_bytes bytea,
  p_bindings jsonb,
  p_actor kb_actor_identity
) RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE
  digest kb_sha256;
  artifact jsonb;
  chapter jsonb;
  slot jsonb;
  binding jsonb;
  chapter_count integer := 0;
  slot_count integer := 0;
  current_run bid_outline_runs%ROWTYPE;
  binding_digest kb_sha256;
BEGIN
  PERFORM kb_bid_v2_require_project_owner(p_project_id, p_actor);
  IF NOT EXISTS (
    SELECT 1 FROM bid_frozen_inputs WHERE input_sha256 = p_frozen_input_sha256
      AND project_id = p_project_id
  ) THEN
    RAISE EXCEPTION 'BID_FROZEN_INPUT_UNKNOWN: %', p_frozen_input_sha256
      USING ERRCODE = '23503';
  END IF;
  digest := kb_bid_v2_sha256_bytes(p_artifact_bytes);
  artifact := convert_from(p_artifact_bytes, 'UTF8')::jsonb;
  IF p_bindings IS NULL OR jsonb_typeof(p_bindings) <> 'array' THEN
    RAISE EXCEPTION 'attachment bindings must be an array' USING ERRCODE = '23514';
  END IF;
  IF jsonb_typeof(artifact->'chapters') IS DISTINCT FROM 'array'
      OR jsonb_typeof(artifact->'templates') IS DISTINCT FROM 'array' THEN
    RAISE EXCEPTION 'outline artifact needs chapters and templates' USING ERRCODE = '23514';
  END IF;
  -- Lock and fence the operation before any artifact/chapter/status write.
  SELECT * INTO current_run FROM bid_outline_runs WHERE id = p_run_id FOR UPDATE;
  IF NOT FOUND OR current_run.project_id IS DISTINCT FROM p_project_id
      OR current_run.frozen_input_sha256 IS DISTINCT FROM p_frozen_input_sha256
      OR p_lease_token IS NULL OR current_run.lease_token IS DISTINCT FROM p_lease_token
      OR p_lease_epoch IS NULL OR current_run.lease_epoch IS DISTINCT FROM p_lease_epoch THEN
    RAISE EXCEPTION 'BID_OUTLINE_LEASE_LOST' USING ERRCODE = '40001';
  END IF;
  IF current_run.status <> 'published' AND (current_run.status <> 'running'
      OR current_run.lease_until <= clock_timestamp() OR current_run.lease_until IS NULL) THEN
    RAISE EXCEPTION 'BID_OUTLINE_LEASE_LOST' USING ERRCODE = '40001';
  END IF;
  IF artifact->>'project_id' IS DISTINCT FROM p_project_id::text
      OR artifact->>'frozen_input_sha256' IS DISTINCT FROM p_frozen_input_sha256::text THEN
    RAISE EXCEPTION 'outline artifact identity mismatch' USING ERRCODE = '23514';
  END IF;
  IF current_run.checkpoint->>'done' IS DISTINCT FROM 'true'
      OR current_run.checkpoint#>>'{outline_run,tool_draft,finished}' IS DISTINCT FROM 'true'
      OR current_run.checkpoint#>>'{journal,publication_receipt,artifact_sha256}' IS DISTINCT FROM digest::text
      OR current_run.checkpoint#>'{journal,publication_receipt,bindings}' IS DISTINCT FROM p_bindings THEN
    RAISE EXCEPTION 'BID_OUTLINE_PUBLICATION_CANDIDATE_MISMATCH' USING ERRCODE='23514';
  END IF;
  binding_digest := kb_bid_v2_sha256_bytes(convert_to(p_bindings::text, 'UTF8'));
  IF current_run.status = 'published' THEN
    IF current_run.outline_sha256 IS DISTINCT FROM digest
        OR current_run.bindings_sha256 IS DISTINCT FROM binding_digest
        OR NOT EXISTS (SELECT 1 FROM bid_outline_artifacts WHERE sha256 = digest
          AND project_id = p_project_id AND frozen_input_sha256 = p_frozen_input_sha256)
        OR current_run.published_by IS DISTINCT FROM p_actor THEN
      RAISE EXCEPTION 'BID_OUTLINE_REPLAY_MISMATCH' USING ERRCODE = '23514';
    END IF;
    -- Only the original successful fenced operation can recover a lost ACK.
    RETURN jsonb_build_object('outline_sha256', digest, 'replayed', true);
  END IF;
  INSERT INTO bid_outline_artifacts(
    sha256, project_id, run_id, frozen_input_sha256, artifact, created_by
  ) VALUES (
    digest, p_project_id, p_run_id, p_frozen_input_sha256, artifact, p_actor
  ) ON CONFLICT (sha256) DO NOTHING;
  IF NOT FOUND THEN
    -- Identical content may be produced by another independently authorized
    -- run. Reuse immutable content only after fencing this run; this is a new
    -- completed operation, never an unfenced generic replay exemption.
    IF NOT EXISTS (SELECT 1 FROM bid_outline_artifacts existing
        JOIN bid_outline_runs original ON original.id=existing.run_id
        WHERE existing.sha256=digest AND existing.project_id=p_project_id
          AND existing.frozen_input_sha256=p_frozen_input_sha256
          AND existing.artifact=convert_from(p_artifact_bytes,'UTF8')::jsonb
          AND original.bindings_sha256=binding_digest) THEN
      RAISE EXCEPTION 'BID_OUTLINE_CONTENT_IDENTITY_MISMATCH' USING ERRCODE='23514';
    END IF;
    UPDATE bid_outline_runs SET status='published', outline_sha256=digest,
      bindings_sha256=binding_digest, published_by=p_actor, updated_at=clock_timestamp()
    WHERE id=p_run_id;
    RETURN jsonb_build_object('outline_sha256',digest,'replayed',false);
  END IF;
  FOR chapter IN SELECT value FROM jsonb_array_elements(artifact->'chapters')
  LOOP
    INSERT INTO bid_outline_chapters(
      outline_sha256, chapter_id, parent_id, ordinal, title, purpose
    ) VALUES (
      digest,
      chapter->>'id',
      NULLIF(chapter->>'parent_id', ''),
      (chapter->>'order')::integer,
      chapter->>'title',
      chapter->>'purpose'
    );
    chapter_count := chapter_count + 1;
  END LOOP;
  FOR slot IN SELECT value FROM jsonb_array_elements(artifact->'templates')
  LOOP
    INSERT INTO bid_outline_template_slots(
      outline_sha256, slot_id, chapter_id, kind, body, response_required, match_query
    ) VALUES (
      digest,
      slot->>'slot_id',
      slot->>'chapter_id',
      slot->>'kind',
      COALESCE(slot->>'text', ''),
      (slot->>'response_required')::boolean,
      COALESCE(slot->>'match_query', '')
    );
    slot_count := slot_count + 1;
  END LOOP;
  IF chapter_count = 0 AND slot_count = 0 THEN
    RAISE EXCEPTION 'outline published no chapters or template content' USING ERRCODE = '23514';
  END IF;
  IF EXISTS (
    SELECT 1 FROM bid_outline_template_slots slot
    JOIN bid_outline_chapters chapter
      ON chapter.outline_sha256 = slot.outline_sha256
     AND chapter.chapter_id = slot.chapter_id
    WHERE slot.outline_sha256 = digest
      AND chapter.purpose = 'group'
      AND slot.response_required
  ) THEN
    RAISE EXCEPTION 'group chapter cannot carry a knowledge response' USING ERRCODE = '23514';
  END IF;
  FOR binding IN SELECT value FROM jsonb_array_elements(p_bindings)
  LOOP
    INSERT INTO bid_outline_attachment_bindings(outline_sha256, form_id, chapter_id)
    VALUES (digest, binding->>'form_id', binding->>'chapter_id');
  END LOOP;
  UPDATE bid_outline_runs
  SET status = 'published', outline_sha256 = digest, bindings_sha256 = binding_digest,
      published_by = p_actor, updated_at = clock_timestamp()
  WHERE id = p_run_id AND project_id = p_project_id AND status <> 'published';
  RETURN jsonb_build_object('outline_sha256', digest, 'replayed', false);
END
$$;

CREATE FUNCTION kb_bid_v2_publish_response(
  p_project_id uuid,
  p_outline_sha256 kb_sha256,
  p_response_bytes bytea,
  p_actor kb_actor_identity
) RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE
  digest kb_sha256 := kb_bid_v2_sha256_bytes(p_response_bytes);
  payload jsonb := convert_from(p_response_bytes, 'UTF8')::jsonb;
  slot jsonb;
BEGIN
  PERFORM kb_bid_v2_require_project_owner(p_project_id, p_actor);
  IF payload->>'outline_sha256' IS DISTINCT FROM p_outline_sha256::text THEN
    RAISE EXCEPTION 'response is not bound to this outline' USING ERRCODE = '23514';
  END IF;
  IF NOT EXISTS (
    SELECT 1 FROM bid_outline_artifacts
    WHERE sha256 = p_outline_sha256 AND project_id = p_project_id
  ) THEN
    RAISE EXCEPTION 'outline artifact is missing' USING ERRCODE = '23503';
  END IF;
  IF EXISTS (SELECT 1 FROM bid_response_sets WHERE sha256 = digest) THEN
    RETURN jsonb_build_object('response_sha256', digest, 'replayed', true);
  END IF;
  INSERT INTO bid_response_sets(sha256, project_id, outline_sha256, created_by)
  VALUES (digest, p_project_id, p_outline_sha256, p_actor);
  FOR slot IN SELECT value FROM jsonb_array_elements(payload->'responses')
  LOOP
    IF NOT EXISTS (
      SELECT 1 FROM bid_outline_template_slots
      WHERE outline_sha256 = p_outline_sha256
        AND slot_id = slot->>'slot_id'
        AND chapter_id = slot->>'chapter_id'
        AND response_required
    ) THEN
      RAISE EXCEPTION 'response slot % is not an outline response slot', slot->>'slot_id'
        USING ERRCODE = '23514';
    END IF;
    INSERT INTO bid_response_slots(
      response_sha256, slot_id, chapter_id, status, body, evidence
    ) VALUES (
      digest,
      slot->>'slot_id',
      slot->>'chapter_id',
      slot->>'status',
      slot->>'text',
      COALESCE(slot->'evidence_ids', '[]'::jsonb)
    );
  END LOOP;
  RETURN jsonb_build_object('response_sha256', digest, 'replayed', false);
END
$$;

CREATE FUNCTION kb_bid_v2_publish_document(
  p_document_id uuid, p_project_id uuid, p_file_name text, p_media_type text,
  p_object_ref kb_object_ref, p_content_sha256 kb_sha256, p_byte_length bigint,
  p_staging_id uuid, p_actor kb_actor_identity
) RETURNS uuid LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, public AS $$
BEGIN
  PERFORM kb_bid_v2_require_project_owner(p_project_id, p_actor);
  PERFORM kb_object_publish_reference(
    p_staging_id, p_object_ref, p_content_sha256, p_media_type, p_byte_length,
    'bid_document', p_document_id, 'source', p_actor
  );
  INSERT INTO bid_documents(id, project_id, file_name, media_type, object_ref,
    content_sha256, byte_length, created_by)
  VALUES (p_document_id, p_project_id, p_file_name, p_media_type, p_object_ref,
    p_content_sha256, p_byte_length, p_actor);
  RETURN p_document_id;
END $$;

CREATE FUNCTION kb_bid_v2_put_docx_version(
  p_project_id uuid,
  p_outline_sha256 kb_sha256,
  p_object_ref kb_object_ref,
  p_docx_sha256 kb_sha256,
  p_byte_length bigint,
  p_expected_revision bigint,
  p_staging_id uuid,
  p_actor kb_actor_identity
) RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE
  current_revision bigint;
  next_revision bigint;
  parent uuid;
  version_id uuid := gen_random_uuid();
BEGIN
  PERFORM kb_bid_v2_require_project_owner(p_project_id, p_actor);
  IF p_object_ref IS DISTINCT FROM ('objects/' || p_docx_sha256)::kb_object_ref THEN
    RAISE EXCEPTION 'docx object ref does not match its digest' USING ERRCODE = '23514';
  END IF;
  PERFORM 1 FROM bid_outline_artifacts
  WHERE sha256 = p_outline_sha256 AND project_id = p_project_id FOR UPDATE;
  IF NOT FOUND THEN
    RAISE EXCEPTION 'outline artifact is missing' USING ERRCODE = '23503';
  END IF;
  SELECT version.revision, head.version_id INTO current_revision, parent
  FROM bid_docx_current head
  JOIN bid_docx_versions version
    ON version.project_id = head.project_id
   AND version.outline_sha256 = head.outline_sha256
   AND version.id = head.version_id
  WHERE head.project_id = p_project_id AND head.outline_sha256 = p_outline_sha256
  FOR UPDATE OF head;
  current_revision := COALESCE(current_revision, 0);
  IF current_revision IS DISTINCT FROM p_expected_revision THEN
    RAISE EXCEPTION 'DOCX_VERSION_CAS_MISMATCH' USING ERRCODE = '40001';
  END IF;
  IF current_revision = 0 THEN
    parent := NULL;
  END IF;
  next_revision := current_revision + 1;
  PERFORM kb_object_publish_reference(
    p_staging_id, p_object_ref, p_docx_sha256,
    'application/vnd.openxmlformats-officedocument.wordprocessingml.document', p_byte_length,
    'bid_docx_version', version_id, 'payload', p_actor
  );
  INSERT INTO bid_docx_versions(
    id, project_id, outline_sha256, revision, parent_id, object_ref, docx_sha256, byte_length, created_by
  ) VALUES (
    version_id, p_project_id, p_outline_sha256, next_revision, parent,
    p_object_ref, p_docx_sha256, p_byte_length, p_actor
  );
  INSERT INTO bid_docx_current(project_id, outline_sha256, version_id, docx_sha256)
  VALUES (p_project_id, p_outline_sha256, version_id, p_docx_sha256)
  ON CONFLICT (project_id, outline_sha256) DO UPDATE
  SET version_id = EXCLUDED.version_id,
      docx_sha256 = EXCLUDED.docx_sha256,
      editor_key = NULL,
      editor_base_version_id = NULL,
      pending_save_id = NULL,
      editor_error = NULL;
  RETURN jsonb_build_object('version_id', version_id, 'revision', next_revision);
END
$$;

CREATE FUNCTION kb_bid_v2_open_docx_editor(
  p_project_id uuid,
  p_outline_sha256 kb_sha256,
  p_editor_key uuid,
  p_actor kb_actor_identity
) RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE
  head bid_docx_current%ROWTYPE;
BEGIN
  PERFORM kb_bid_v2_require_project_owner(p_project_id, p_actor);
  SELECT * INTO STRICT head
  FROM bid_docx_current
  WHERE project_id = p_project_id AND outline_sha256 = p_outline_sha256
  FOR UPDATE;
  IF head.pending_save_id IS NOT NULL THEN
    RAISE EXCEPTION 'DOCX_SAVE_PENDING' USING ERRCODE = '40001';
  END IF;
  UPDATE bid_docx_current
  SET editor_key = p_editor_key, editor_base_version_id = head.version_id
  WHERE project_id = p_project_id AND outline_sha256 = p_outline_sha256;
  RETURN jsonb_build_object('editor_key', p_editor_key, 'version_id', head.version_id);
END
$$;

CREATE FUNCTION kb_bid_v2_publish_submission_export(
  p_project_id uuid,
  p_outline_sha256 kb_sha256,
  p_response_sha256 kb_sha256,
  p_object_ref kb_object_ref,
  p_content_sha256 kb_sha256,
  p_media_type text,
  p_byte_length bigint,
  p_staging_id uuid,
  p_actor kb_actor_identity
) RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE
  version_id uuid;
  export_id uuid := gen_random_uuid();
BEGIN
  PERFORM kb_bid_v2_require_project_owner(p_project_id, p_actor);
  IF p_object_ref IS DISTINCT FROM ('objects/' || p_content_sha256)::kb_object_ref THEN
    RAISE EXCEPTION 'export object ref does not match its digest' USING ERRCODE = '23514';
  END IF;
  SELECT bid_docx_current.version_id INTO STRICT version_id
  FROM bid_docx_current
  WHERE project_id = p_project_id AND outline_sha256 = p_outline_sha256 FOR SHARE;
  IF NOT EXISTS (
    SELECT 1 FROM bid_response_sets
    WHERE sha256 = p_response_sha256 AND outline_sha256 = p_outline_sha256 AND project_id = p_project_id
  ) THEN
    RAISE EXCEPTION 'export requires the response for this outline' USING ERRCODE = '23503';
  END IF;
  PERFORM kb_object_publish_reference(
    p_staging_id, p_object_ref, p_content_sha256, p_media_type, p_byte_length,
    'bid_submission_export', export_id, 'payload', p_actor
  );
  INSERT INTO bid_submission_exports(
    id, project_id, outline_sha256, response_sha256, docx_version_id,
    object_ref, content_sha256, media_type, byte_length, created_by
  ) VALUES (
    export_id, p_project_id, p_outline_sha256, p_response_sha256, version_id,
    p_object_ref, p_content_sha256, p_media_type, p_byte_length, p_actor
  );
  RETURN jsonb_build_object('export_id', export_id, 'docx_version_id', version_id);
END
$$;

REVOKE ALL ON FUNCTION
  kb_bid_v2_sha256_bytes(bytea),
  kb_bid_v2_require_project_owner(uuid, kb_actor_identity),
  kb_bid_v2_register_frozen_input(uuid, kb_sha256, text),
  kb_bid_v2_publish_frozen_input(uuid, kb_sha256, bytea, text, uuid, jsonb, kb_actor_identity),
  kb_bid_v2_outline_claim(uuid, uuid, kb_sha256, uuid, integer),
  kb_bid_v2_outline_renew(uuid, uuid, bigint, integer),
  kb_bid_v2_outline_checkpoint(uuid, uuid, bigint, jsonb),
  kb_bid_v2_outline_pause_budget(uuid, uuid, bigint),
  kb_bid_v2_outline_release(uuid, uuid, bigint),
  kb_bid_v2_outline_paused_checkpoint(uuid),
  kb_bid_v2_outline_resume_budget(uuid, kb_sha256, jsonb),
  kb_bid_v2_publish_document(uuid, uuid, text, text, kb_object_ref, kb_sha256, bigint, uuid, kb_actor_identity),
  kb_bid_v2_publish_outline(uuid, uuid, kb_sha256, uuid, bigint, bytea, jsonb, kb_actor_identity),
  kb_bid_v2_publish_response(uuid, kb_sha256, bytea, kb_actor_identity),
  kb_bid_v2_put_docx_version(uuid, kb_sha256, kb_object_ref, kb_sha256, bigint, bigint, uuid, kb_actor_identity),
  kb_bid_v2_open_docx_editor(uuid, kb_sha256, uuid, kb_actor_identity),
  kb_bid_v2_publish_submission_export(uuid, kb_sha256, kb_sha256, kb_object_ref, kb_sha256, text, bigint, uuid, kb_actor_identity)
FROM PUBLIC;

GRANT SELECT ON
  bid_projects, bid_documents, bid_frozen_inputs, bid_frozen_input_objects, bid_outline_runs, bid_outline_artifacts, bid_outline_chapters,
  bid_outline_attachment_bindings, bid_outline_template_slots, bid_response_sets, bid_response_slots,
  bid_docx_versions, bid_docx_current, bid_submission_exports
TO kb_runtime_api, kb_runtime_worker;

GRANT INSERT, UPDATE ON bid_projects TO kb_runtime_api;

GRANT EXECUTE ON FUNCTION
  kb_bid_v2_sha256_bytes(bytea),
  kb_bid_v2_require_project_owner(uuid, kb_actor_identity),
  kb_bid_v2_publish_frozen_input(uuid, kb_sha256, bytea, text, uuid, jsonb, kb_actor_identity),
  kb_bid_v2_outline_claim(uuid, uuid, kb_sha256, uuid, integer),
  kb_bid_v2_outline_renew(uuid, uuid, bigint, integer),
  kb_bid_v2_outline_checkpoint(uuid, uuid, bigint, jsonb),
  kb_bid_v2_outline_pause_budget(uuid, uuid, bigint),
  kb_bid_v2_outline_release(uuid, uuid, bigint),
  kb_bid_v2_outline_paused_checkpoint(uuid),
  kb_bid_v2_outline_resume_budget(uuid, kb_sha256, jsonb),
  kb_bid_v2_publish_document(uuid, uuid, text, text, kb_object_ref, kb_sha256, bigint, uuid, kb_actor_identity),
  kb_bid_v2_publish_outline(uuid, uuid, kb_sha256, uuid, bigint, bytea, jsonb, kb_actor_identity),
  kb_bid_v2_publish_response(uuid, kb_sha256, bytea, kb_actor_identity),
  kb_bid_v2_put_docx_version(uuid, kb_sha256, kb_object_ref, kb_sha256, bigint, bigint, uuid, kb_actor_identity),
  kb_bid_v2_open_docx_editor(uuid, kb_sha256, uuid, kb_actor_identity),
  kb_bid_v2_publish_submission_export(uuid, kb_sha256, kb_sha256, kb_object_ref, kb_sha256, text, bigint, uuid, kb_actor_identity)
TO kb_runtime_api, kb_runtime_worker;
