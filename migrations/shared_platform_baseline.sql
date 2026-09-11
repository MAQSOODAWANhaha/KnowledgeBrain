-- KnowledgeBrain final V1 fresh baseline: shared runtime foundation.
-- Actor, idempotency, audit, maintenance, queue, ObjectRegistry, and retention
-- are owned here. This slice is create-only and contains no repair/backfill DDL.

CREATE EXTENSION IF NOT EXISTS pgcrypto;

CREATE FUNCTION kb_actor_identity_valid(value text)
RETURNS boolean
LANGUAGE sql
IMMUTABLE
STRICT
PARALLEL SAFE
SET search_path = pg_catalog
AS $$
    SELECT value ~ '^(user|api_key):[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
        OR value IN (
            'system:bid-convert-worker',
            'system:bid-attachment-preparation',
            'system:bid-extraction-worker',
            'system:content-generate-v2',
            'system:clause-lifecycle',
            'system:kind-router-promotion',
            'system:maintenance',
            'system:knowledge-document-delete',
            'system:knowledge-document-ingest',
            'system:matching-invalidation',
            'system:matching-publication',
            'system:requirement-set-compile-v2',
            'system:requirement-set-compile-v3',
            'system:requirement-set-compile-v4',
            'system:retention-consumer',
            'system:submission-export-v2',
            'system:tender-document-process-v2'
        )
$$;

CREATE DOMAIN kb_actor_identity AS text CHECK (kb_actor_identity_valid(VALUE));
CREATE DOMAIN kb_sha256 AS text CHECK (VALUE ~ '^[0-9a-f]{64}$');
CREATE DOMAIN kb_object_ref AS text CHECK (VALUE ~ '^objects/[0-9a-f]{64}$');

CREATE FUNCTION kb_reject_append_only()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
BEGIN
    RAISE EXCEPTION 'append-only relation cannot be changed' USING ERRCODE = '42501';
END
$$;
REVOKE ALL ON FUNCTION kb_reject_append_only() FROM PUBLIC;

CREATE TABLE platform_schema_snapshot (
    singleton boolean PRIMARY KEY CHECK (singleton),
    schema_revision text NOT NULL,
    shared_baseline_sha256 char(64) NOT NULL,
    knowledge_baseline_sha256 char(64) NOT NULL,
    bidding_baseline_sha256 char(64) NOT NULL,
    manifest_contract_version integer NOT NULL,
    catalog_manifest_sha256 char(64) NOT NULL,
    postgres_server_version_num integer NOT NULL,
    extensions jsonb NOT NULL,
    release_descriptor_sha256 char(64) NOT NULL,
    deployment_namespace_id uuid NOT NULL,
    created_at timestamptz NOT NULL
);

CREATE TABLE platform_role_contracts (
    role_name text PRIMARY KEY,
    login boolean NOT NULL,
    purpose text NOT NULL CHECK (octet_length(purpose) BETWEEN 1 AND 128)
);
INSERT INTO platform_role_contracts(role_name, login, purpose) VALUES
    ('kb_app_owner', false, 'owns the application catalog'),
    ('kb_migrator', true, 'explicit fresh-schema bootstrap writer'),
    ('kb_runtime_api', true, 'runtime HTTP identity'),
    ('kb_runtime_retention', true, 'exclusive physical object deletion identity'),
    ('kb_runtime_worker', true, 'runtime asynchronous job identity');

-- Runtime identities must not be able to shadow hardened helper dependencies
-- through attacker-controlled temporary relations.
DO $$
BEGIN
EXECUTE format('REVOKE TEMPORARY ON DATABASE %I FROM PUBLIC', current_database());
IF has_database_privilege('kb_runtime_api',current_database(),'TEMPORARY')
   OR has_database_privilege('kb_runtime_worker',current_database(),'TEMPORARY')
   OR has_database_privilege('kb_runtime_retention',current_database(),'TEMPORARY') THEN
  RAISE EXCEPTION 'runtime database roles must not have TEMPORARY privilege'
    USING ERRCODE='42501';
END IF;
END
$$;
GRANT USAGE ON SCHEMA public TO
    kb_runtime_api, kb_runtime_worker, kb_runtime_retention;

CREATE TABLE idempotency_requests (
    actor_identity kb_actor_identity NOT NULL,
    operation text NOT NULL CHECK (operation ~ '^[a-z][a-z0-9_.-]{0,127}$'),
    idempotency_key text NOT NULL CHECK (octet_length(idempotency_key) BETWEEN 1 AND 200),
    schema_version smallint NOT NULL CHECK (schema_version = 1),
    request_bytes bytea NOT NULL,
    request_sha256 kb_sha256 NOT NULL,
    state text NOT NULL CHECK (state IN ('intent', 'completed')),
    response_status integer,
    response_bytes bytea,
    response_sha256 kb_sha256,
    created_at timestamptz NOT NULL DEFAULT now(),
    completed_at timestamptz,
    PRIMARY KEY (actor_identity, operation, idempotency_key),
    CHECK (request_sha256 = encode(digest(request_bytes, 'sha256'), 'hex')),
    CHECK (
        (state = 'intent' AND response_status IS NULL AND response_bytes IS NULL
            AND response_sha256 IS NULL AND completed_at IS NULL)
        OR
        (state = 'completed' AND response_status BETWEEN 100 AND 599
            AND response_bytes IS NOT NULL AND response_sha256 IS NOT NULL
            AND response_sha256 = encode(digest(response_bytes, 'sha256'), 'hex')
            AND completed_at IS NOT NULL)
    )
);

CREATE FUNCTION kb_guard_idempotency_request()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
BEGIN
    IF TG_OP = 'DELETE'
       OR OLD.actor_identity IS DISTINCT FROM NEW.actor_identity
       OR OLD.operation IS DISTINCT FROM NEW.operation
       OR OLD.idempotency_key IS DISTINCT FROM NEW.idempotency_key
       OR OLD.schema_version IS DISTINCT FROM NEW.schema_version
       OR OLD.request_bytes IS DISTINCT FROM NEW.request_bytes
       OR OLD.request_sha256 IS DISTINCT FROM NEW.request_sha256
       OR OLD.created_at IS DISTINCT FROM NEW.created_at
       OR OLD.state <> 'intent' OR NEW.state <> 'completed'
       OR OLD.response_status IS NOT NULL OR OLD.response_bytes IS NOT NULL
       OR OLD.response_sha256 IS NOT NULL OR OLD.completed_at IS NOT NULL
       OR NEW.response_status IS NULL OR NEW.response_bytes IS NULL
       OR NEW.response_sha256 IS NULL OR NEW.completed_at IS NULL
    THEN
        RAISE EXCEPTION 'idempotency request transition is immutable or invalid'
            USING ERRCODE = '42501';
    END IF;
    RETURN NEW;
END
$$;
CREATE TRIGGER idempotency_requests_guard
BEFORE UPDATE OR DELETE ON idempotency_requests
FOR EACH ROW EXECUTE FUNCTION kb_guard_idempotency_request();
CREATE TRIGGER idempotency_requests_no_truncate
BEFORE TRUNCATE ON idempotency_requests
FOR EACH STATEMENT EXECUTE FUNCTION kb_reject_append_only();

CREATE TABLE audit_events (
    id uuid PRIMARY KEY,
    schema_version smallint NOT NULL CHECK (schema_version = 1),
    operation text NOT NULL CHECK (operation ~ '^[a-z][a-z0-9_.-]{0,127}$'),
    actor_identity kb_actor_identity NOT NULL,
    idempotency_key text,
    request_sha256 kb_sha256 NOT NULL,
    response_sha256 kb_sha256 NOT NULL,
    entity_kind text NOT NULL CHECK (entity_kind ~ '^[a-z][a-z0-9_.-]{0,127}$'),
    entity_locator jsonb NOT NULL CHECK (jsonb_typeof(entity_locator) = 'object'),
    before_revision bigint,
    before_sha256 kb_sha256,
    after_revision bigint,
    after_sha256 kb_sha256,
    occurred_at timestamptz NOT NULL DEFAULT now(),
    CHECK ((before_revision IS NULL) = (before_sha256 IS NULL)),
    CHECK ((after_revision IS NULL) = (after_sha256 IS NULL))
);
CREATE INDEX audit_events_entity_timeline_idx
    ON audit_events(entity_kind, occurred_at, id);
CREATE TRIGGER audit_events_immutable
BEFORE UPDATE OR DELETE ON audit_events
FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();
CREATE TRIGGER audit_events_no_truncate
BEFORE TRUNCATE ON audit_events
FOR EACH STATEMENT EXECUTE FUNCTION kb_reject_append_only();

CREATE TABLE application_maintenance_gate (
    singleton_key boolean PRIMARY KEY DEFAULT true CHECK (singleton_key),
    mode text NOT NULL CHECK (mode IN ('maintenance', 'open', 'draining', 'rollback')),
    generation bigint NOT NULL CHECK (generation >= 0),
    updated_by kb_actor_identity NOT NULL,
    updated_at timestamptz NOT NULL,
    CHECK (isfinite(updated_at))
);
INSERT INTO application_maintenance_gate
    (singleton_key, mode, generation, updated_by, updated_at)
VALUES (true, 'open', 0, 'system:maintenance', '1970-01-01 UTC');

CREATE TABLE maintenance_gate_audit (
    id uuid PRIMARY KEY,
    from_mode text NOT NULL CHECK (from_mode IN ('maintenance', 'open', 'draining', 'rollback')),
    to_mode text NOT NULL CHECK (to_mode IN ('maintenance', 'open', 'draining', 'rollback')),
    generation bigint NOT NULL CHECK (generation > 0),
    actor_identity kb_actor_identity NOT NULL,
    reason text NOT NULL CHECK (octet_length(reason) BETWEEN 1 AND 512),
    occurred_at timestamptz NOT NULL DEFAULT now()
);
CREATE TRIGGER maintenance_gate_audit_immutable
BEFORE UPDATE OR DELETE ON maintenance_gate_audit
FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();
CREATE TRIGGER maintenance_gate_audit_no_truncate
BEFORE TRUNCATE ON maintenance_gate_audit
FOR EACH STATEMENT EXECUTE FUNCTION kb_reject_append_only();

CREATE TABLE queue_contract_artifacts (
    contract_key text NOT NULL,
    version integer NOT NULL CHECK (version > 0),
    schema_version smallint NOT NULL CHECK (schema_version = 1),
    canonical_payload jsonb NOT NULL CHECK (jsonb_typeof(canonical_payload) = 'object'),
    content_sha256 kb_sha256 NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (contract_key, version),
    CHECK (contract_key ~ '^[a-z][a-z0-9_.-]{0,63}$')
);
CREATE TABLE queue_contract_current (
    contract_key text PRIMARY KEY,
    version integer NOT NULL,
    generation bigint NOT NULL CHECK (generation >= 0),
    FOREIGN KEY (contract_key, version)
        REFERENCES queue_contract_artifacts(contract_key, version) ON DELETE RESTRICT
);
INSERT INTO queue_contract_artifacts(
    contract_key, version, schema_version, canonical_payload, content_sha256, created_at
)
VALUES
 ('bid.tender_document_process.v2', 1, 1, '{"queue":"bid-authoring-v2","schema_version":1,"task_type":"bid:tender_document_process:v2"}',
  encode(digest(convert_to('{"queue":"bid-authoring-v2","schema_version":1,"task_type":"bid:tender_document_process:v2"}', 'UTF8'), 'sha256'), 'hex'),
  '1970-01-01 UTC'),
 ('bid.requirement_set_compile.v2', 1, 1, '{"queue":"bid-authoring-v2","schema_version":1,"task_type":"bid:requirement_set_compile:v2"}',
  encode(digest(convert_to('{"queue":"bid-authoring-v2","schema_version":1,"task_type":"bid:requirement_set_compile:v2"}', 'UTF8'), 'sha256'), 'hex'),
  '1970-01-01 UTC'),
 ('bid.content_generate.v2', 1, 1, '{"queue":"bid-authoring-v2","schema_version":1,"task_type":"bid:content_generate:v2"}',
  encode(digest(convert_to('{"queue":"bid-authoring-v2","schema_version":1,"task_type":"bid:content_generate:v2"}', 'UTF8'), 'sha256'), 'hex'),
  '1970-01-01 UTC'),
 ('bid.submission_export.v2', 1, 1, '{"queue":"bid-authoring-v2","schema_version":1,"task_type":"bid:submission_export:v2"}',
  encode(digest(convert_to('{"queue":"bid-authoring-v2","schema_version":1,"task_type":"bid:submission_export:v2"}', 'UTF8'), 'sha256'), 'hex'),
  '1970-01-01 UTC'),
 ('document.process', 1, 1, '{"claim_lease_ms":300000,"queue":"default","schema_version":1}',
  encode(digest(convert_to('{"claim_lease_ms":300000,"queue":"default","schema_version":1}', 'UTF8'), 'sha256'), 'hex'),
  '1970-01-01 UTC'),
 ('object.retention', 1, 1, '{"queue":"retention","schema_version":1,"task_type":"object:retention"}',
  encode(digest(convert_to('{"queue":"retention","schema_version":1,"task_type":"object:retention"}', 'UTF8'), 'sha256'), 'hex'),
  '1970-01-01 UTC'),
 ('object.upload_expire', 1, 1, '{"queue":"retention","schema_version":1,"task_type":"object:upload_expire"}',
  encode(digest(convert_to('{"queue":"retention","schema_version":1,"task_type":"object:upload_expire"}', 'UTF8'), 'sha256'), 'hex'),
  '1970-01-01 UTC');
INSERT INTO queue_contract_current(contract_key, version, generation)
VALUES ('bid.tender_document_process.v2', 1, 0),
       ('bid.requirement_set_compile.v2', 1, 0),
       ('bid.content_generate.v2', 1, 0),
       ('bid.submission_export.v2', 1, 0),
       ('document.process', 1, 0),
       ('object.retention', 1, 0),
       ('object.upload_expire', 1, 0);
CREATE TRIGGER queue_contract_artifacts_immutable
BEFORE UPDATE OR DELETE ON queue_contract_artifacts
FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();

CREATE TABLE object_registry (
    object_ref kb_object_ref PRIMARY KEY,
    digest kb_sha256 NOT NULL UNIQUE,
    media_type text NOT NULL CHECK (media_type ~ '^[a-z0-9][a-z0-9!#$&^_.+-]{0,63}/[a-z0-9][a-z0-9!#$&^_.+-]{0,63}$'),
    byte_length bigint NOT NULL CHECK (byte_length >= 0),
    state text NOT NULL CHECK (state IN ('available', 'deleting', 'deleted')),
    registered_at timestamptz NOT NULL DEFAULT now(),
    deleting_at timestamptz,
    deleted_at timestamptz,
    UNIQUE(object_ref,digest,state),
    UNIQUE(object_ref,digest,media_type,state),
    UNIQUE(object_ref,digest,media_type,byte_length,state),
    CHECK (object_ref = 'objects/' || digest),
    CHECK (
        (state = 'available' AND deleting_at IS NULL AND deleted_at IS NULL)
        OR (state = 'deleting' AND deleting_at IS NOT NULL AND deleted_at IS NULL)
        OR (state = 'deleted' AND deleting_at IS NOT NULL AND deleted_at IS NOT NULL)
    )
);

CREATE TABLE object_owner_references (
    object_ref kb_object_ref NOT NULL REFERENCES object_registry(object_ref) ON DELETE RESTRICT,
    owner_kind text NOT NULL CHECK (owner_kind ~ '^[a-z][a-z0-9_.-]{0,63}$'),
    owner_id uuid NOT NULL,
    occurrence text NOT NULL CHECK (octet_length(occurrence) BETWEEN 1 AND 128),
    created_by kb_actor_identity NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (object_ref, owner_kind, owner_id, occurrence),
    UNIQUE (owner_kind, owner_id, occurrence)
);

CREATE TABLE object_upload_staging (
    id uuid PRIMARY KEY,
    object_ref kb_object_ref NOT NULL REFERENCES object_registry(object_ref) ON DELETE RESTRICT,
    created_by kb_actor_identity NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL DEFAULT (now() + interval '24 hours'),
    CHECK (expires_at > created_at)
);
CREATE INDEX object_upload_staging_expiry_idx
    ON object_upload_staging(expires_at, id);

CREATE TABLE object_deletion_artifacts (
    id uuid PRIMARY KEY,
    object_ref kb_object_ref NOT NULL UNIQUE REFERENCES object_registry(object_ref) ON DELETE RESTRICT,
    digest kb_sha256 NOT NULL,
    byte_length bigint NOT NULL CHECK (byte_length >= 0),
    requested_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE(id, object_ref, digest, byte_length),
    CHECK (object_ref = 'objects/' || digest)
);
CREATE TRIGGER object_deletion_artifacts_immutable
BEFORE UPDATE OR DELETE ON object_deletion_artifacts
FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();
CREATE TRIGGER object_deletion_artifacts_no_truncate
BEFORE TRUNCATE ON object_deletion_artifacts
FOR EACH STATEMENT EXECUTE FUNCTION kb_reject_append_only();

CREATE TABLE object_retention_tombstones (
    object_ref kb_object_ref PRIMARY KEY,
    digest kb_sha256 NOT NULL UNIQUE,
    byte_length bigint NOT NULL CHECK (byte_length >= 0),
    deleted_by kb_actor_identity NOT NULL,
    deletion_id uuid NOT NULL UNIQUE,
    deleted_at timestamptz NOT NULL,
    CHECK (object_ref = 'objects/' || digest)
);
CREATE TRIGGER object_retention_tombstones_immutable
BEFORE UPDATE OR DELETE ON object_retention_tombstones
FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();
CREATE TRIGGER object_retention_tombstones_no_truncate
BEFORE TRUNCATE ON object_retention_tombstones
FOR EACH STATEMENT EXECUTE FUNCTION kb_reject_append_only();

-- Internal ObjectRegistry seam used by domain SECURITY DEFINER mutations. It is
-- intentionally not granted to runtime logins.
CREATE FUNCTION kb_object_reference_add(
    p_object_ref kb_object_ref,
    p_digest kb_sha256,
    p_media_type text,
    p_byte_length bigint,
    p_owner_kind text,
    p_owner_id uuid,
    p_occurrence text,
    p_actor kb_actor_identity
)
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE
    registry object_registry%ROWTYPE;
BEGIN
    SELECT * INTO registry FROM object_registry WHERE object_ref = p_object_ref FOR UPDATE;
    IF FOUND THEN
        IF registry.digest <> p_digest OR registry.media_type <> p_media_type
           OR registry.byte_length <> p_byte_length THEN
            RAISE EXCEPTION 'object registry identity mismatch or object unavailable'
                USING ERRCODE = '23514';
        END IF;
        IF registry.state <> 'available' THEN
            RAISE EXCEPTION 'object registry identity mismatch or object unavailable'
                USING ERRCODE = '23514';
        END IF;
    ELSE
        IF EXISTS (SELECT 1 FROM object_retention_tombstones WHERE object_ref = p_object_ref) THEN
            RAISE EXCEPTION 'deleted object digest cannot be revived' USING ERRCODE = '23514';
        END IF;
        INSERT INTO object_registry(object_ref, digest, media_type, byte_length, state)
        VALUES (p_object_ref, p_digest, p_media_type, p_byte_length, 'available');
    END IF;
    INSERT INTO object_owner_references(object_ref, owner_kind, owner_id, occurrence, created_by)
    VALUES (p_object_ref, p_owner_kind, p_owner_id, p_occurrence, p_actor)
    ON CONFLICT DO NOTHING;
    IF NOT EXISTS (
        SELECT 1 FROM object_owner_references
         WHERE object_ref = p_object_ref AND owner_kind = p_owner_kind
           AND owner_id = p_owner_id AND occurrence = p_occurrence
    ) THEN
        RAISE EXCEPTION 'object owner occurrence already references another object'
            USING ERRCODE = '23514';
    END IF;
END
$$;

CREATE FUNCTION kb_object_reference_remove(
    p_object_ref kb_object_ref,
    p_owner_kind text,
    p_owner_id uuid,
    p_occurrence text,
    p_deletion_id uuid
)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE
    registry object_registry%ROWTYPE;
    deletion object_deletion_artifacts%ROWTYPE;
BEGIN
    SELECT * INTO STRICT registry FROM object_registry WHERE object_ref = p_object_ref FOR UPDATE;
    DELETE FROM object_owner_references
     WHERE object_ref = p_object_ref AND owner_kind = p_owner_kind
       AND owner_id = p_owner_id AND occurrence = p_occurrence;
    IF NOT EXISTS (SELECT 1 FROM object_owner_references WHERE object_ref = p_object_ref) THEN
        UPDATE object_registry SET state = 'deleting', deleting_at = clock_timestamp()
         WHERE object_ref = p_object_ref AND state = 'available';
        IF FOUND THEN
            INSERT INTO object_deletion_artifacts(id,object_ref,digest,byte_length)
            VALUES(p_deletion_id,registry.object_ref,registry.digest,registry.byte_length);
        END IF;
    END IF;
    SELECT * INTO deletion FROM object_deletion_artifacts WHERE id=p_deletion_id;
    IF FOUND THEN
        IF deletion.object_ref<>p_object_ref OR deletion.digest<>registry.digest
           OR deletion.byte_length<>registry.byte_length THEN
            RAISE EXCEPTION 'object deletion identity mismatch' USING ERRCODE='23514';
        END IF;
        RETURN jsonb_build_object('deletion_id',deletion.id,'object_ref',deletion.object_ref,
          'digest',deletion.digest,'byte_length',deletion.byte_length);
    END IF;
    RETURN NULL;
END
$$;

-- Runtime uploaders first register a short-lived platform-owned reference,
-- then write the physical bytes. A domain mutation atomically transfers that
-- reference to its final owner. Failed or abandoned mutations never leave an
-- unregistered physical object, and crashed uploaders are reclaimed by the
-- required retention service through the expiry function below.
CREATE FUNCTION kb_object_upload_stage(
    p_staging_id uuid,
    p_object_ref kb_object_ref,
    p_digest kb_sha256,
    p_media_type text,
    p_byte_length bigint,
    p_actor kb_actor_identity
)
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
BEGIN
    PERFORM kb_object_reference_add(
        p_object_ref, p_digest, p_media_type, p_byte_length,
        'object_upload_staging', p_staging_id, 'payload', p_actor
    );
    INSERT INTO object_upload_staging(id, object_ref, created_by)
    VALUES (p_staging_id, p_object_ref, p_actor)
    ON CONFLICT (id) DO NOTHING;
    IF NOT EXISTS (
        SELECT 1 FROM object_upload_staging
         WHERE id = p_staging_id AND object_ref = p_object_ref AND created_by = p_actor
    ) THEN
        RAISE EXCEPTION 'object upload staging identity mismatch'
            USING ERRCODE = '23514';
    END IF;
END
$$;

-- Internal transfer seam. Domain SECURITY DEFINER mutations call this in the
-- same transaction as their business row, audit, pointer, and receipt writes.
-- It is deliberately not granted to runtime roles.
CREATE FUNCTION kb_object_upload_commit(
    p_staging_id uuid,
    p_object_ref kb_object_ref,
    p_digest kb_sha256,
    p_media_type text,
    p_byte_length bigint,
    p_owner_kind text,
    p_owner_id uuid,
    p_occurrence text,
    p_actor kb_actor_identity
)
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE
    staging object_upload_staging%ROWTYPE;
    registry object_registry%ROWTYPE;
BEGIN
    SELECT * INTO STRICT staging FROM object_upload_staging
     WHERE id = p_staging_id FOR UPDATE;
    IF staging.object_ref <> p_object_ref OR staging.created_by <> p_actor THEN
        RAISE EXCEPTION 'object upload staging owner mismatch' USING ERRCODE = '23514';
    END IF;
    SELECT * INTO STRICT registry FROM object_registry
     WHERE object_ref = staging.object_ref FOR UPDATE;
    IF registry.digest <> p_digest OR registry.media_type <> p_media_type
       OR registry.byte_length <> p_byte_length THEN
        RAISE EXCEPTION 'object upload staging content identity mismatch'
            USING ERRCODE = '23514';
    END IF;
    IF registry.state <> 'available' THEN
        RAISE EXCEPTION 'object upload staging content identity mismatch'
            USING ERRCODE = '23514';
    END IF;
    PERFORM kb_object_reference_add(
        p_object_ref, p_digest, p_media_type, p_byte_length,
        p_owner_kind, p_owner_id, p_occurrence, p_actor
    );
    DELETE FROM object_upload_staging WHERE id = p_staging_id;
    PERFORM kb_object_reference_remove(
        p_object_ref, 'object_upload_staging', p_staging_id, 'payload', p_staging_id
    );
END
$$;

CREATE FUNCTION kb_object_upload_abandon(
    p_staging_id uuid,
    p_actor kb_actor_identity
)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE
    staging object_upload_staging%ROWTYPE;
    deletion object_deletion_artifacts%ROWTYPE;
BEGIN
    SELECT * INTO staging FROM object_upload_staging WHERE id = p_staging_id FOR UPDATE;
    IF NOT FOUND THEN
        SELECT * INTO deletion FROM object_deletion_artifacts WHERE id=p_staging_id;
        IF FOUND THEN
            RETURN jsonb_build_object('deletion_id',deletion.id,'object_ref',deletion.object_ref,
              'digest',deletion.digest,'byte_length',deletion.byte_length);
        END IF;
        RETURN NULL;
    END IF;
    IF staging.created_by <> p_actor THEN
        RAISE EXCEPTION 'object upload staging owner mismatch' USING ERRCODE = '42501';
    END IF;
    DELETE FROM object_upload_staging WHERE id = p_staging_id;
    RETURN kb_object_reference_remove(
        staging.object_ref, 'object_upload_staging', p_staging_id, 'payload', p_staging_id
    );
END
$$;

CREATE FUNCTION kb_object_upload_expiry_candidates()
RETURNS SETOF uuid
LANGUAGE sql
VOLATILE
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
    SELECT staging.id
      FROM object_upload_staging staging
     WHERE staging.expires_at <= clock_timestamp()
     ORDER BY staging.expires_at, staging.id
     LIMIT 100
$$;

CREATE FUNCTION kb_object_upload_expire_one(p_staging_id uuid)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE
    staging object_upload_staging%ROWTYPE;
    deletion object_deletion_artifacts%ROWTYPE;
    deletion_payload jsonb;
BEGIN
    SELECT * INTO deletion FROM object_deletion_artifacts WHERE id=p_staging_id FOR SHARE;
    IF FOUND THEN
        RETURN jsonb_build_object('state','expired','staging_id',p_staging_id,'deletion',
          jsonb_build_object('deletion_id',deletion.id,'object_ref',deletion.object_ref,
            'digest',deletion.digest,'byte_length',deletion.byte_length));
    END IF;
    SELECT * INTO staging FROM object_upload_staging WHERE id=p_staging_id FOR UPDATE;
    IF NOT FOUND THEN
        RETURN jsonb_build_object('state','not_current','staging_id',p_staging_id,'deletion',NULL);
    END IF;
    DELETE FROM object_upload_staging WHERE id=p_staging_id;
    deletion_payload:=kb_object_reference_remove(
        staging.object_ref,'object_upload_staging',staging.id,'payload',staging.id);
    RETURN jsonb_build_object('state','expired','staging_id',p_staging_id,'deletion',deletion_payload);
END
$$;

CREATE FUNCTION kb_retention_preflight(
    p_deletion_id uuid,
    p_object_ref kb_object_ref,
    p_digest kb_sha256,
    p_byte_length bigint
)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE
    deletion object_deletion_artifacts%ROWTYPE;
    registry object_registry%ROWTYPE;
    tombstone object_retention_tombstones%ROWTYPE;
BEGIN
    SELECT * INTO tombstone FROM object_retention_tombstones
     WHERE deletion_id=p_deletion_id FOR SHARE;
    IF FOUND THEN
        IF tombstone.object_ref=p_object_ref AND tombstone.digest=p_digest
           AND tombstone.byte_length=p_byte_length THEN
            RETURN jsonb_build_object('state','completed');
        END IF;
        RETURN jsonb_build_object('state','mismatch');
    END IF;
    SELECT * INTO deletion FROM object_deletion_artifacts WHERE id=p_deletion_id FOR SHARE;
    IF NOT FOUND THEN RETURN jsonb_build_object('state','mismatch'); END IF;
    SELECT * INTO registry FROM object_registry WHERE object_ref=deletion.object_ref FOR UPDATE;
    IF deletion.object_ref=p_object_ref AND deletion.digest=p_digest
       AND deletion.byte_length=p_byte_length AND registry.object_ref=p_object_ref
       AND registry.digest=p_digest AND registry.byte_length=p_byte_length
       AND registry.state='deleting'
       AND NOT EXISTS (SELECT 1 FROM object_owner_references WHERE object_ref=p_object_ref) THEN
        RETURN jsonb_build_object('state','current');
    END IF;
    RETURN jsonb_build_object('state','mismatch');
END
$$;

CREATE FUNCTION kb_begin_intent(
    p_actor kb_actor_identity,
    p_operation text,
    p_key text,
    p_request_bytes bytea
)
RETURNS TABLE(replayed boolean, response_status integer, response_bytes bytea)
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE
    existing idempotency_requests%ROWTYPE;
    request_hash kb_sha256 := encode(digest(p_request_bytes, 'sha256'), 'hex');
BEGIN
    SELECT * INTO existing FROM idempotency_requests
     WHERE actor_identity = p_actor AND operation = p_operation AND idempotency_key = p_key
     FOR UPDATE;
    IF FOUND THEN
        IF existing.request_sha256 <> request_hash OR existing.request_bytes <> p_request_bytes THEN
            RAISE EXCEPTION 'IDEMPOTENCY_PAYLOAD_MISMATCH' USING ERRCODE = '23505';
        END IF;
        IF existing.state = 'completed' THEN
            RETURN QUERY SELECT true, existing.response_status, existing.response_bytes;
            RETURN;
        END IF;
        RAISE EXCEPTION 'IDEMPOTENCY_INTENT_IN_PROGRESS' USING ERRCODE = '40001';
    END IF;
    INSERT INTO idempotency_requests(
        actor_identity, operation, idempotency_key, schema_version,
        request_bytes, request_sha256, state
    ) VALUES (p_actor, p_operation, p_key, 1, p_request_bytes, request_hash, 'intent');
    RETURN QUERY SELECT false, NULL::integer, NULL::bytea;
END
$$;

CREATE FUNCTION kb_complete_intent(
    p_actor kb_actor_identity,
    p_operation text,
    p_key text,
    p_response_status integer,
    p_response_bytes bytea
)
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
BEGIN
    UPDATE idempotency_requests
       SET state = 'completed', response_status = p_response_status,
           response_bytes = p_response_bytes,
           response_sha256 = encode(digest(p_response_bytes, 'sha256'), 'hex'),
           completed_at = clock_timestamp()
     WHERE actor_identity = p_actor AND operation = p_operation
       AND idempotency_key = p_key AND state = 'intent';
    IF NOT FOUND THEN
        RAISE EXCEPTION 'idempotency intent is not current' USING ERRCODE = '40001';
    END IF;
END
$$;

CREATE FUNCTION kb_retention_complete(
    p_deletion_id uuid,
    p_object_ref kb_object_ref,
    p_digest kb_sha256
)
RETURNS boolean
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE
    deletion object_deletion_artifacts%ROWTYPE;
    registry object_registry%ROWTYPE;
BEGIN
    IF EXISTS (SELECT 1 FROM object_retention_tombstones
               WHERE object_ref=p_object_ref AND deletion_id=p_deletion_id AND digest=p_digest) THEN
        RETURN true;
    END IF;
    SELECT * INTO STRICT deletion FROM object_deletion_artifacts WHERE id=p_deletion_id FOR SHARE;
    SELECT * INTO STRICT registry FROM object_registry WHERE object_ref=p_object_ref FOR UPDATE;
    IF deletion.object_ref<>p_object_ref OR deletion.digest<>p_digest
       OR registry.digest<>p_digest OR registry.state<>'deleting'
       OR EXISTS (SELECT 1 FROM object_owner_references WHERE object_ref=p_object_ref) THEN
        RAISE EXCEPTION 'object deletion business fence mismatch' USING ERRCODE='40001';
    END IF;
    INSERT INTO object_retention_tombstones(
        object_ref,digest,byte_length,deleted_by,deletion_id,deleted_at
    ) VALUES(p_object_ref,p_digest,registry.byte_length,
      'system:retention-consumer',p_deletion_id,clock_timestamp());
    UPDATE object_registry SET state='deleted',deleted_at=clock_timestamp()
     WHERE object_ref=p_object_ref;
    RETURN true;
END
$$;

CREATE VIEW available_object_registry AS
SELECT object_ref, digest, media_type, byte_length, registered_at
  FROM object_registry WHERE state = 'available';

-- Runtime roles get no direct platform table writes. Domain-owned object
-- mutations are checked SECURITY DEFINER functions in their owning slices.
-- Retention has the only database capability which can complete deletion state.
REVOKE ALL ON ALL TABLES IN SCHEMA public FROM PUBLIC;
REVOKE ALL ON ALL FUNCTIONS IN SCHEMA public FROM PUBLIC;
GRANT SELECT ON platform_schema_snapshot, platform_role_contracts,
    application_maintenance_gate, queue_contract_artifacts, queue_contract_current
TO kb_migrator, kb_runtime_api, kb_runtime_worker, kb_runtime_retention;
GRANT SELECT ON available_object_registry
TO kb_runtime_api, kb_runtime_worker;
GRANT EXECUTE ON FUNCTION kb_actor_identity_valid(text)
TO kb_runtime_api, kb_runtime_worker, kb_runtime_retention;
GRANT EXECUTE ON FUNCTION kb_object_upload_stage(uuid, kb_object_ref, kb_sha256, text, bigint, kb_actor_identity),
    kb_object_upload_abandon(uuid, kb_actor_identity)
TO kb_runtime_api, kb_runtime_worker;
GRANT EXECUTE ON FUNCTION kb_object_upload_expiry_candidates()
TO kb_runtime_retention;
GRANT EXECUTE ON FUNCTION kb_object_upload_expire_one(uuid),
    kb_retention_preflight(uuid, kb_object_ref, kb_sha256, bigint),
    kb_retention_complete(uuid, kb_object_ref, kb_sha256)
TO kb_runtime_retention;
