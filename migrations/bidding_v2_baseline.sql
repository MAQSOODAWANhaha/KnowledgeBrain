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
  created_by kb_actor_identity,
  created_at timestamptz NOT NULL DEFAULT now(),
  CHECK (object_ref = 'objects/' || content_sha256)
);

CREATE TABLE bid_outline_runs (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
  status text NOT NULL CHECK (status IN ('pending','running','failed','published')),
  checkpoint jsonb NOT NULL CHECK (jsonb_typeof(checkpoint) = 'object'),
  outline_sha256 kb_sha256,
  lease_token uuid,
  lease_until timestamptz,
  created_by kb_actor_identity,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  CHECK ((status = 'published') = (outline_sha256 IS NOT NULL)),
  CHECK ((lease_token IS NULL) = (lease_until IS NULL))
);

CREATE TABLE bid_outline_artifacts (
  sha256 kb_sha256 PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES bid_projects(id) ON DELETE RESTRICT,
  run_id uuid NOT NULL UNIQUE REFERENCES bid_outline_runs(id) ON DELETE RESTRICT,
  frozen_input_sha256 kb_sha256 NOT NULL,
  artifact jsonb NOT NULL CHECK (jsonb_typeof(artifact) = 'object'),
  created_by kb_actor_identity,
  created_at timestamptz NOT NULL DEFAULT now()
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

CREATE FUNCTION kb_bid_v2_outline_claim(
  p_run_id uuid, p_token uuid, p_seconds integer
) RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE
  claimed bid_outline_runs%ROWTYPE;
BEGIN
  UPDATE bid_outline_runs
  SET status = 'running',
      lease_token = p_token,
      lease_until = clock_timestamp() + make_interval(secs => p_seconds),
      updated_at = clock_timestamp()
  WHERE id = p_run_id
    AND status <> 'published'
    AND (lease_until IS NULL OR lease_until < clock_timestamp() OR lease_token = p_token)
  RETURNING * INTO claimed;
  IF claimed.id IS NULL THEN
    RAISE EXCEPTION 'outline run is published or leased' USING ERRCODE = '40001';
  END IF;
  RETURN jsonb_build_object('run_id', claimed.id, 'status', claimed.status, 'lease_token', claimed.lease_token);
END
$$;

CREATE FUNCTION kb_bid_v2_publish_outline(
  p_run_id uuid,
  p_project_id uuid,
  p_frozen_input_sha256 kb_sha256,
  p_artifact_bytes bytea,
  p_bindings jsonb,
  p_actor kb_actor_identity
) RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE
  digest kb_sha256 := kb_bid_v2_sha256_bytes(p_artifact_bytes);
  artifact jsonb := convert_from(p_artifact_bytes, 'UTF8')::jsonb;
  chapter jsonb;
  slot jsonb;
  binding jsonb;
  chapter_count integer := 0;
  slot_count integer := 0;
BEGIN
  PERFORM kb_bid_v2_require_project_owner(p_project_id, p_actor);
  IF p_bindings IS NULL OR jsonb_typeof(p_bindings) <> 'array' THEN
    RAISE EXCEPTION 'attachment bindings must be an array' USING ERRCODE = '23514';
  END IF;
  IF jsonb_typeof(artifact->'chapters') <> 'array' OR jsonb_typeof(artifact->'templates') <> 'array' THEN
    RAISE EXCEPTION 'outline artifact needs chapters and templates' USING ERRCODE = '23514';
  END IF;
  IF EXISTS (SELECT 1 FROM bid_outline_artifacts WHERE sha256 = digest) THEN
    RETURN jsonb_build_object('outline_sha256', digest, 'replayed', true);
  END IF;
  INSERT INTO bid_outline_runs(id, project_id, status, checkpoint, created_by)
  VALUES (p_run_id, p_project_id, 'pending', '{}'::jsonb, p_actor)
  ON CONFLICT (id) DO NOTHING;
  INSERT INTO bid_outline_artifacts(
    sha256, project_id, run_id, frozen_input_sha256, artifact, created_by
  ) VALUES (
    digest, p_project_id, p_run_id, p_frozen_input_sha256, artifact, p_actor
  );
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
  SET status = 'published', outline_sha256 = digest, updated_at = now()
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

CREATE FUNCTION kb_bid_v2_put_docx_version(
  p_project_id uuid,
  p_outline_sha256 kb_sha256,
  p_object_ref kb_object_ref,
  p_docx_sha256 kb_sha256,
  p_byte_length bigint,
  p_expected_revision bigint,
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
  WHERE project_id = p_project_id AND outline_sha256 = p_outline_sha256;
  IF NOT EXISTS (
    SELECT 1 FROM bid_response_sets
    WHERE sha256 = p_response_sha256 AND outline_sha256 = p_outline_sha256 AND project_id = p_project_id
  ) THEN
    RAISE EXCEPTION 'export requires the response for this outline' USING ERRCODE = '23503';
  END IF;
  INSERT INTO bid_submission_exports(
    id, project_id, outline_sha256, response_sha256, docx_version_id,
    object_ref, content_sha256, created_by
  ) VALUES (
    export_id, p_project_id, p_outline_sha256, p_response_sha256, version_id,
    p_object_ref, p_content_sha256, p_actor
  );
  RETURN jsonb_build_object('export_id', export_id, 'docx_version_id', version_id);
END
$$;

REVOKE ALL ON FUNCTION
  kb_bid_v2_sha256_bytes(bytea),
  kb_bid_v2_require_project_owner(uuid, kb_actor_identity),
  kb_bid_v2_outline_claim(uuid, uuid, integer),
  kb_bid_v2_publish_outline(uuid, uuid, kb_sha256, bytea, jsonb, kb_actor_identity),
  kb_bid_v2_publish_response(uuid, kb_sha256, bytea, kb_actor_identity),
  kb_bid_v2_put_docx_version(uuid, kb_sha256, kb_object_ref, kb_sha256, bigint, bigint, kb_actor_identity),
  kb_bid_v2_open_docx_editor(uuid, kb_sha256, uuid, kb_actor_identity),
  kb_bid_v2_publish_submission_export(uuid, kb_sha256, kb_sha256, kb_object_ref, kb_sha256, kb_actor_identity)
FROM PUBLIC;

GRANT SELECT, INSERT, UPDATE ON
  bid_projects, bid_documents, bid_outline_runs, bid_outline_artifacts, bid_outline_chapters,
  bid_outline_attachment_bindings, bid_outline_template_slots, bid_response_sets, bid_response_slots,
  bid_docx_versions, bid_docx_current, bid_submission_exports
TO kb_runtime_api, kb_runtime_worker;

GRANT EXECUTE ON FUNCTION
  kb_bid_v2_sha256_bytes(bytea),
  kb_bid_v2_require_project_owner(uuid, kb_actor_identity),
  kb_bid_v2_outline_claim(uuid, uuid, integer),
  kb_bid_v2_publish_outline(uuid, uuid, kb_sha256, bytea, jsonb, kb_actor_identity),
  kb_bid_v2_publish_response(uuid, kb_sha256, bytea, kb_actor_identity),
  kb_bid_v2_put_docx_version(uuid, kb_sha256, kb_object_ref, kb_sha256, bigint, bigint, kb_actor_identity),
  kb_bid_v2_open_docx_editor(uuid, kb_sha256, uuid, kb_actor_identity),
  kb_bid_v2_publish_submission_export(uuid, kb_sha256, kb_sha256, kb_object_ref, kb_sha256, kb_actor_identity)
TO kb_runtime_api, kb_runtime_worker;
