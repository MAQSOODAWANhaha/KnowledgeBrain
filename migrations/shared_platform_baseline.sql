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
        OR value ~ '^system:[a-z0-9][a-z0-9._-]{0,63}$'
$$;

CREATE DOMAIN kb_actor_identity AS text CHECK (kb_actor_identity_valid(VALUE));
CREATE DOMAIN kb_sha256 AS text CHECK (VALUE ~ '^[0-9a-f]{64}$');
CREATE DOMAIN kb_object_ref AS text CHECK (VALUE ~ '^objects/[0-9a-f]{64}$');

CREATE FUNCTION kb_reject_append_only()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, public
AS $$
BEGIN
    RAISE EXCEPTION 'append-only relation cannot be changed' USING ERRCODE = '42501';
END
$$;
REVOKE ALL ON FUNCTION kb_reject_append_only() FROM PUBLIC;

CREATE TABLE platform_role_contracts (
    role_name text PRIMARY KEY,
    login boolean NOT NULL,
    purpose text NOT NULL CHECK (octet_length(purpose) BETWEEN 1 AND 128)
);
INSERT INTO platform_role_contracts(role_name, login, purpose) VALUES
    ('kb_app_owner', false, 'owns application catalog after first-launch handoff'),
    ('kb_first_launch_verifier', true, 'one-shot catalog and seed verifier'),
    ('kb_launch_attestor', false, 'launch attestation capability'),
    ('kb_launch_ingress', false, 'launch ingress capability'),
    ('kb_launch_operator', false, 'launch operator capability'),
    ('kb_launch_owner', false, 'owns immutable first-launch ledger'),
    ('kb_launch_reset_dispatcher', false, 'launch reset dispatch capability'),
    ('kb_launch_router', false, 'launch routing capability'),
    ('kb_launch_signature_verifier', false, 'launch signature verification capability'),
    ('kb_migrator', true, 'fresh-baseline writer disabled after handoff'),
    ('kb_runtime_api', true, 'runtime HTTP identity'),
    ('kb_runtime_retention', true, 'exclusive physical object deletion identity'),
    ('kb_runtime_worker', true, 'runtime asynchronous job identity');

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
VALUES (true, 'maintenance', 0, 'system:first-launch', '1970-01-01 UTC');

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
 ('document.process', 1, 1, '{"claim_lease_ms":300000,"queue":"default","schema_version":1}',
  encode(digest(convert_to('{"claim_lease_ms":300000,"queue":"default","schema_version":1}', 'UTF8'), 'sha256'), 'hex'),
  '1970-01-01 UTC'),
 ('object.retention', 1, 1, '{"claim_lease_ms":60000,"queue":"retention","schema_version":1}',
  encode(digest(convert_to('{"claim_lease_ms":60000,"queue":"retention","schema_version":1}', 'UTF8'), 'sha256'), 'hex'),
  '1970-01-01 UTC');
INSERT INTO queue_contract_current(contract_key, version, generation)
VALUES ('document.process', 1, 0), ('object.retention', 1, 0);
CREATE TRIGGER queue_contract_artifacts_immutable
BEFORE UPDATE OR DELETE ON queue_contract_artifacts
FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();

CREATE TABLE queue_jobs (
    id uuid PRIMARY KEY,
    contract_key text NOT NULL,
    contract_version integer NOT NULL,
    scope_kind text NOT NULL CHECK (scope_kind ~ '^[a-z][a-z0-9_.-]{0,63}$'),
    scope_id uuid NOT NULL,
    payload jsonb NOT NULL CHECK (jsonb_typeof(payload) = 'object'),
    payload_sha256 kb_sha256 NOT NULL,
    state text NOT NULL CHECK (state IN ('queued', 'claimed', 'completed', 'failed', 'dead')),
    attempt integer NOT NULL DEFAULT 0 CHECK (attempt BETWEEN 0 AND 1000),
    claim_token uuid,
    claimed_by text,
    heartbeat_at timestamptz,
    lease_until timestamptz,
    available_at timestamptz NOT NULL DEFAULT now(),
    created_at timestamptz NOT NULL DEFAULT now(),
    completed_at timestamptz,
    last_error_code text,
    FOREIGN KEY (contract_key, contract_version)
        REFERENCES queue_contract_artifacts(contract_key, version) ON DELETE RESTRICT,
    CHECK (
        (state = 'claimed' AND claim_token IS NOT NULL AND claimed_by IS NOT NULL
            AND heartbeat_at IS NOT NULL AND lease_until IS NOT NULL)
        OR
        (state <> 'claimed' AND claim_token IS NULL AND claimed_by IS NULL
            AND heartbeat_at IS NULL AND lease_until IS NULL)
    )
);
CREATE INDEX queue_jobs_claim_idx ON queue_jobs(contract_key, available_at, created_at, id)
    WHERE state IN ('queued', 'failed');

CREATE TABLE object_registry (
    object_ref kb_object_ref PRIMARY KEY,
    digest kb_sha256 NOT NULL UNIQUE,
    media_type text NOT NULL CHECK (media_type ~ '^[a-z0-9][a-z0-9!#$&^_.+-]{0,63}/[a-z0-9][a-z0-9!#$&^_.+-]{0,63}$'),
    byte_length bigint NOT NULL CHECK (byte_length >= 0),
    state text NOT NULL CHECK (state IN ('available', 'deleting', 'deleted')),
    registered_at timestamptz NOT NULL DEFAULT now(),
    deleting_at timestamptz,
    deleted_at timestamptz,
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
    PRIMARY KEY (object_ref, owner_kind, owner_id, occurrence)
);
CREATE INDEX object_owner_references_owner_idx
    ON object_owner_references(owner_kind, owner_id, occurrence);

CREATE TABLE object_retention_outbox (
    object_ref kb_object_ref PRIMARY KEY REFERENCES object_registry(object_ref) ON DELETE RESTRICT,
    state text NOT NULL CHECK (state IN ('queued', 'claimed', 'retry')),
    attempt integer NOT NULL DEFAULT 0 CHECK (attempt BETWEEN 0 AND 1000),
    claim_token uuid,
    claimed_by text,
    heartbeat_at timestamptz,
    lease_until timestamptz,
    next_attempt_at timestamptz NOT NULL DEFAULT now(),
    last_error_code text,
    created_at timestamptz NOT NULL DEFAULT now(),
    CHECK (
        (state = 'claimed' AND claim_token IS NOT NULL AND claimed_by IS NOT NULL
            AND heartbeat_at IS NOT NULL AND lease_until IS NOT NULL)
        OR
        (state <> 'claimed' AND claim_token IS NULL AND claimed_by IS NULL
            AND heartbeat_at IS NULL AND lease_until IS NULL)
    )
);
CREATE INDEX object_retention_outbox_claim_idx
    ON object_retention_outbox(next_attempt_at, created_at, object_ref)
    WHERE state IN ('queued', 'retry');

CREATE TABLE object_retention_tombstones (
    object_ref kb_object_ref PRIMARY KEY,
    digest kb_sha256 NOT NULL UNIQUE,
    byte_length bigint NOT NULL CHECK (byte_length >= 0),
    deleted_by kb_actor_identity NOT NULL,
    claim_token uuid NOT NULL UNIQUE,
    deleted_at timestamptz NOT NULL,
    CHECK (object_ref = 'objects/' || digest)
);
CREATE TRIGGER object_retention_tombstones_immutable
BEFORE UPDATE OR DELETE ON object_retention_tombstones
FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();
CREATE TRIGGER object_retention_tombstones_no_truncate
BEFORE TRUNCATE ON object_retention_tombstones
FOR EACH STATEMENT EXECUTE FUNCTION kb_reject_append_only();

CREATE TABLE object_retention_attempt_receipts (
    object_ref kb_object_ref NOT NULL,
    claim_token uuid NOT NULL,
    attempt integer NOT NULL CHECK (attempt > 0),
    outcome text NOT NULL CHECK (outcome IN ('deleted', 'retry')),
    error_code text,
    occurred_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (object_ref, claim_token),
    CHECK ((outcome = 'deleted') = (error_code IS NULL))
);
CREATE TRIGGER object_retention_attempt_receipts_immutable
BEFORE UPDATE OR DELETE ON object_retention_attempt_receipts
FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();
CREATE TRIGGER object_retention_attempt_receipts_no_truncate
BEFORE TRUNCATE ON object_retention_attempt_receipts
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
           OR registry.byte_length <> p_byte_length OR registry.state <> 'available' THEN
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
END
$$;

CREATE FUNCTION kb_object_reference_remove(
    p_object_ref kb_object_ref,
    p_owner_kind text,
    p_owner_id uuid,
    p_occurrence text
)
RETURNS boolean
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE
    scheduled boolean := false;
BEGIN
    PERFORM 1 FROM object_registry WHERE object_ref = p_object_ref FOR UPDATE;
    DELETE FROM object_owner_references
     WHERE object_ref = p_object_ref AND owner_kind = p_owner_kind
       AND owner_id = p_owner_id AND occurrence = p_occurrence;
    IF NOT EXISTS (SELECT 1 FROM object_owner_references WHERE object_ref = p_object_ref) THEN
        UPDATE object_registry SET state = 'deleting', deleting_at = clock_timestamp()
         WHERE object_ref = p_object_ref AND state = 'available';
        IF FOUND THEN
            INSERT INTO object_retention_outbox(object_ref, state) VALUES (p_object_ref, 'queued');
            scheduled := true;
        END IF;
    END IF;
    RETURN scheduled;
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

CREATE FUNCTION kb_register_knowledge_document_object(
    p_document_id uuid,
    p_media_type text,
    p_actor kb_actor_identity,
    p_idempotency_key text,
    p_audit_id uuid
)
RETURNS kb_object_ref
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE
    doc documents%ROWTYPE;
    request_bytes bytea;
    replay record;
    response_bytes bytea;
BEGIN
    SELECT * INTO STRICT doc FROM documents WHERE id = p_document_id FOR SHARE;
    request_bytes := convert_to(jsonb_build_object(
        'schema_version', 1, 'document_id', p_document_id, 'object_ref', doc.object_ref,
        'digest', doc.file_hash, 'media_type', p_media_type, 'byte_length', doc.file_size
    )::text, 'UTF8');
    SELECT * INTO replay FROM kb_begin_intent(
        p_actor, 'knowledge.document.object.register', p_idempotency_key, request_bytes
    );
    IF replay.replayed THEN
        RETURN convert_from(replay.response_bytes, 'UTF8')::kb_object_ref;
    END IF;

    PERFORM kb_object_reference_add(
        doc.object_ref, doc.file_hash, p_media_type, doc.file_size,
        'knowledge_document', p_document_id, 'original', p_actor
    );

    response_bytes := convert_to(doc.object_ref::text, 'UTF8');
    INSERT INTO audit_events(
        id, schema_version, operation, actor_identity, idempotency_key,
        request_sha256, response_sha256, entity_kind, entity_locator,
        after_revision, after_sha256
    ) VALUES (
        p_audit_id, 1, 'knowledge.document.object.register', p_actor, p_idempotency_key,
        encode(digest(request_bytes, 'sha256'), 'hex'),
        encode(digest(response_bytes, 'sha256'), 'hex'),
        'knowledge_document', jsonb_build_object('document_id', p_document_id),
        1, doc.file_hash
    );
    PERFORM kb_complete_intent(
        p_actor, 'knowledge.document.object.register', p_idempotency_key, 200, response_bytes
    );
    RETURN doc.object_ref;
END
$$;

CREATE FUNCTION kb_release_knowledge_document_object(
    p_document_id uuid,
    p_actor kb_actor_identity,
    p_idempotency_key text,
    p_audit_id uuid
)
RETURNS boolean
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE
    doc documents%ROWTYPE;
    request_bytes bytea;
    replay record;
    response_bytes bytea;
    scheduled boolean := false;
BEGIN
    SELECT * INTO STRICT doc FROM documents WHERE id = p_document_id FOR SHARE;
    request_bytes := convert_to(jsonb_build_object(
        'schema_version', 1, 'document_id', p_document_id, 'object_ref', doc.object_ref
    )::text, 'UTF8');
    SELECT * INTO replay FROM kb_begin_intent(
        p_actor, 'knowledge.document.object.release', p_idempotency_key, request_bytes
    );
    IF replay.replayed THEN
        RETURN convert_from(replay.response_bytes, 'UTF8')::boolean;
    END IF;

    scheduled := kb_object_reference_remove(
        doc.object_ref, 'knowledge_document', p_document_id, 'original'
    );
    UPDATE documents
       SET deleted_at = COALESCE(deleted_at, clock_timestamp()), updated_at = clock_timestamp()
     WHERE id = p_document_id;

    response_bytes := convert_to(scheduled::text, 'UTF8');
    INSERT INTO audit_events(
        id, schema_version, operation, actor_identity, idempotency_key,
        request_sha256, response_sha256, entity_kind, entity_locator,
        before_revision, before_sha256
    ) VALUES (
        p_audit_id, 1, 'knowledge.document.object.release', p_actor, p_idempotency_key,
        encode(digest(request_bytes, 'sha256'), 'hex'),
        encode(digest(response_bytes, 'sha256'), 'hex'),
        'knowledge_document', jsonb_build_object('document_id', p_document_id),
        1, doc.file_hash
    );
    PERFORM kb_complete_intent(
        p_actor, 'knowledge.document.object.release', p_idempotency_key, 200, response_bytes
    );
    RETURN scheduled;
END
$$;

CREATE FUNCTION kb_retention_claim(
    p_claim_token uuid,
    p_worker_name text,
    p_lease_ms integer
)
RETURNS TABLE(object_ref kb_object_ref, digest kb_sha256, byte_length bigint, attempt integer)
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
BEGIN
    IF p_worker_name !~ '^[a-z0-9][a-z0-9._-]{0,63}$' OR p_lease_ms NOT BETWEEN 1000 AND 300000 THEN
        RAISE EXCEPTION 'invalid retention claim parameters' USING ERRCODE = '22023';
    END IF;
    RETURN QUERY
    WITH candidate AS (
        SELECT outbox.object_ref
          FROM object_retention_outbox outbox
          JOIN object_registry registry USING (object_ref)
         WHERE registry.state = 'deleting'
           AND NOT EXISTS (SELECT 1 FROM object_owner_references refs
                            WHERE refs.object_ref = outbox.object_ref)
           AND ((outbox.state IN ('queued', 'retry') AND outbox.next_attempt_at <= clock_timestamp())
                OR (outbox.state = 'claimed' AND outbox.lease_until <= clock_timestamp()))
         ORDER BY outbox.next_attempt_at, outbox.created_at, outbox.object_ref
         FOR UPDATE OF outbox SKIP LOCKED
         LIMIT 1
    ), claimed AS (
        UPDATE object_retention_outbox outbox
           SET state = 'claimed', attempt = outbox.attempt + 1,
               claim_token = p_claim_token, claimed_by = p_worker_name,
               heartbeat_at = clock_timestamp(),
               lease_until = clock_timestamp() + make_interval(secs => p_lease_ms / 1000.0),
               last_error_code = NULL
          FROM candidate
         WHERE outbox.object_ref = candidate.object_ref
         RETURNING outbox.object_ref, outbox.attempt
    )
    SELECT registry.object_ref, registry.digest, registry.byte_length, claimed.attempt
      FROM claimed JOIN object_registry registry USING (object_ref);
END
$$;

CREATE FUNCTION kb_retention_heartbeat(
    p_object_ref kb_object_ref,
    p_claim_token uuid,
    p_lease_ms integer
)
RETURNS boolean
LANGUAGE sql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
    WITH changed AS (
        UPDATE object_retention_outbox
           SET heartbeat_at = clock_timestamp(),
               lease_until = clock_timestamp() + make_interval(secs => p_lease_ms / 1000.0)
         WHERE object_ref = p_object_ref AND state = 'claimed'
           AND claim_token = p_claim_token AND lease_until > clock_timestamp()
           AND p_lease_ms BETWEEN 1000 AND 300000
         RETURNING 1
    ) SELECT EXISTS(SELECT 1 FROM changed)
$$;

CREATE FUNCTION kb_retention_complete(
    p_object_ref kb_object_ref,
    p_claim_token uuid
)
RETURNS boolean
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE
    item object_retention_outbox%ROWTYPE;
    registry object_registry%ROWTYPE;
BEGIN
    IF EXISTS (SELECT 1 FROM object_retention_tombstones
               WHERE object_ref = p_object_ref AND claim_token = p_claim_token) THEN
        RETURN true;
    END IF;
    SELECT * INTO STRICT item FROM object_retention_outbox
     WHERE object_ref = p_object_ref FOR UPDATE;
    SELECT * INTO STRICT registry FROM object_registry
     WHERE object_ref = p_object_ref FOR UPDATE;
    IF item.state <> 'claimed' OR item.claim_token <> p_claim_token
       OR item.lease_until <= clock_timestamp() OR registry.state <> 'deleting'
       OR EXISTS (SELECT 1 FROM object_owner_references WHERE object_ref = p_object_ref) THEN
        RAISE EXCEPTION 'retention completion lost claim or object became referenced'
            USING ERRCODE = '40001';
    END IF;
    INSERT INTO object_retention_attempt_receipts(object_ref, claim_token, attempt, outcome)
    VALUES (p_object_ref, p_claim_token, item.attempt, 'deleted');
    INSERT INTO object_retention_tombstones(
        object_ref, digest, byte_length, deleted_by, claim_token, deleted_at
    ) VALUES (
        p_object_ref, registry.digest, registry.byte_length,
        'system:retention-consumer', p_claim_token, clock_timestamp()
    );
    DELETE FROM object_retention_outbox WHERE object_ref = p_object_ref;
    UPDATE object_registry SET state = 'deleted', deleted_at = clock_timestamp()
     WHERE object_ref = p_object_ref;
    RETURN true;
END
$$;

CREATE FUNCTION kb_retention_fail(
    p_object_ref kb_object_ref,
    p_claim_token uuid,
    p_error_code text
)
RETURNS boolean
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE
    item object_retention_outbox%ROWTYPE;
BEGIN
    IF p_error_code !~ '^[A-Z][A-Z0-9_]{0,63}$' THEN
        RAISE EXCEPTION 'invalid retention error code' USING ERRCODE = '22023';
    END IF;
    SELECT * INTO STRICT item FROM object_retention_outbox
     WHERE object_ref = p_object_ref FOR UPDATE;
    IF item.state <> 'claimed' OR item.claim_token <> p_claim_token THEN
        RAISE EXCEPTION 'retention failure lost claim' USING ERRCODE = '40001';
    END IF;
    INSERT INTO object_retention_attempt_receipts(
        object_ref, claim_token, attempt, outcome, error_code
    ) VALUES (p_object_ref, p_claim_token, item.attempt, 'retry', p_error_code);
    UPDATE object_retention_outbox
       SET state = 'retry', claim_token = NULL, claimed_by = NULL,
           heartbeat_at = NULL, lease_until = NULL, last_error_code = p_error_code,
           next_attempt_at = clock_timestamp()
               + make_interval(secs => LEAST(3600, (2 ^ LEAST(item.attempt, 10))::integer))
     WHERE object_ref = p_object_ref;
    RETURN true;
END
$$;

CREATE VIEW available_object_registry AS
SELECT object_ref, digest, media_type, byte_length, registered_at
  FROM object_registry WHERE state = 'available';

CREATE FUNCTION kb_validate_knowledge_document_object_reference()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
BEGIN
    IF NEW.deleted_at IS NULL AND NOT EXISTS (
        SELECT 1
          FROM object_registry registry
          JOIN object_owner_references reference_value
            ON reference_value.object_ref = registry.object_ref
         WHERE registry.object_ref = NEW.object_ref
           AND registry.digest = NEW.file_hash
           AND registry.byte_length = NEW.file_size
           AND registry.state = 'available'
           AND reference_value.owner_kind = 'knowledge_document'
           AND reference_value.owner_id = NEW.id
           AND reference_value.occurrence = 'original'
    ) THEN
        RAISE EXCEPTION 'knowledge document object reference is absent or unavailable'
            USING ERRCODE = '23514';
    ELSIF NEW.deleted_at IS NOT NULL AND EXISTS (
        SELECT 1 FROM object_owner_references reference_value
         WHERE reference_value.object_ref = NEW.object_ref
           AND reference_value.owner_kind = 'knowledge_document'
           AND reference_value.owner_id = NEW.id
    ) THEN
        RAISE EXCEPTION 'deleted knowledge document still owns an object reference'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END
$$;
CREATE CONSTRAINT TRIGGER documents_object_reference_contract
AFTER INSERT OR UPDATE OF object_ref, file_hash, file_size, deleted_at ON documents
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION kb_validate_knowledge_document_object_reference();

CREATE FUNCTION kb_guard_knowledge_document_delete()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM object_owner_references reference_value
         WHERE reference_value.owner_kind = 'knowledge_document'
           AND reference_value.owner_id = OLD.id
    ) THEN
        RAISE EXCEPTION 'knowledge document cannot be deleted while it owns an object reference'
            USING ERRCODE = '23514';
    END IF;
    RETURN OLD;
END
$$;
CREATE TRIGGER documents_object_reference_delete_guard
BEFORE DELETE ON documents
FOR EACH ROW EXECUTE FUNCTION kb_guard_knowledge_document_delete();

-- One-shot launch evidence. The bootstrap-owned handoff routines validate these
-- rows and then remove migration/verifier authority.
CREATE TABLE production_launch_state (
    singleton_key boolean PRIMARY KEY DEFAULT true CHECK (singleton_key),
    state text NOT NULL CHECK (state IN ('preflight', 'verified', 'exposed')),
    cutover_id uuid,
    cutover_epoch bigint NOT NULL DEFAULT 0 CHECK (cutover_epoch >= 0),
    evidence_epoch bigint NOT NULL DEFAULT 0 CHECK (evidence_epoch >= 0),
    traffic_exposure_started_at timestamptz,
    reset_authority_revoked_at timestamptz,
    first_production_request_at timestamptz
);
INSERT INTO production_launch_state(singleton_key, state) VALUES (true, 'preflight');

CREATE TABLE production_first_launch_catalog_verifications (
    singleton_key boolean PRIMARY KEY DEFAULT true CHECK (singleton_key),
    allowlist_sha256 kb_sha256 NOT NULL,
    catalog_sha256 kb_sha256 NOT NULL,
    rows_sha256 kb_sha256 NOT NULL,
    manifest_sha256 kb_sha256 NOT NULL,
    verified_at timestamptz NOT NULL DEFAULT now()
);
CREATE TRIGGER production_first_launch_catalog_verifications_immutable
BEFORE UPDATE OR DELETE ON production_first_launch_catalog_verifications
FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();
CREATE TRIGGER production_first_launch_catalog_verifications_no_truncate
BEFORE TRUNCATE ON production_first_launch_catalog_verifications
FOR EACH STATEMENT EXECUTE FUNCTION kb_reject_append_only();

-- Runtime roles get no direct platform table writes. Knowledge-document object
-- mutations are checked SECURITY DEFINER functions. Retention has the only
-- database capability which can complete physical-deletion state.
REVOKE ALL ON ALL TABLES IN SCHEMA public FROM PUBLIC;
REVOKE ALL ON ALL FUNCTIONS IN SCHEMA public FROM PUBLIC;
GRANT SELECT ON application_maintenance_gate, queue_contract_current,
    available_object_registry, production_first_launch_catalog_verifications
TO kb_runtime_api, kb_runtime_worker;
GRANT EXECUTE ON FUNCTION kb_actor_identity_valid(text)
TO kb_runtime_api, kb_runtime_worker, kb_runtime_retention;
GRANT EXECUTE ON FUNCTION kb_register_knowledge_document_object(uuid, text, kb_actor_identity, text, uuid),
    kb_release_knowledge_document_object(uuid, kb_actor_identity, text, uuid)
TO kb_runtime_api, kb_runtime_worker;
GRANT EXECUTE ON FUNCTION kb_retention_claim(uuid, text, integer),
    kb_retention_heartbeat(kb_object_ref, uuid, integer),
    kb_retention_complete(kb_object_ref, uuid),
    kb_retention_fail(kb_object_ref, uuid, text)
TO kb_runtime_retention;
