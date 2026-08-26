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
            'system:clause-lifecycle',
            'system:first-launch',
            'system:kind-router-promotion',
            'system:knowledge-document-delete',
            'system:knowledge-document-ingest',
            'system:matching-invalidation',
            'system:matching-publication',
            'system:retention-consumer'
        )
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

-- Open-operation recovery has independent immutable policy/feature snapshots
-- and its own durable claim ledger. Runtime workers can only use the checked
-- bidding recovery functions granted by the bidding baseline; they receive no
-- direct DML on these control-plane relations.
CREATE TABLE live_recovery_policy_snapshots (
    id uuid PRIMARY KEY,
    schema_version smallint NOT NULL CHECK (schema_version = 1),
    claim_lease_ms integer NOT NULL CHECK (claim_lease_ms BETWEEN 5000 AND 300000),
    max_batch_size integer NOT NULL CHECK (max_batch_size BETWEEN 1 AND 128),
    max_global_concurrency integer NOT NULL CHECK (max_global_concurrency BETWEEN 1 AND 32),
    max_concurrency_by_kind jsonb NOT NULL CHECK (
        jsonb_typeof(max_concurrency_by_kind) = 'object'
        AND max_concurrency_by_kind ?& ARRAY['dirty_manifest','orphan_target','orphan_match_job']
        AND max_concurrency_by_kind
              - 'dirty_manifest' - 'orphan_target' - 'orphan_match_job' = '{}'::jsonb
        AND (max_concurrency_by_kind->>'dirty_manifest')::integer BETWEEN 1 AND 32
        AND (max_concurrency_by_kind->>'orphan_target')::integer BETWEEN 1 AND 32
        AND (max_concurrency_by_kind->>'orphan_match_job')::integer BETWEEN 1 AND 32
    ),
    canonical_payload bytea NOT NULL,
    content_sha256 kb_sha256 NOT NULL UNIQUE,
    created_at timestamptz NOT NULL DEFAULT now(),
    CHECK (octet_length(canonical_payload) BETWEEN 1 AND 4096),
    CHECK (content_sha256 = encode(digest(canonical_payload, 'sha256'), 'hex'))
);
CREATE TRIGGER live_recovery_policy_snapshots_immutable
BEFORE UPDATE OR DELETE ON live_recovery_policy_snapshots
FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();
CREATE TRIGGER live_recovery_policy_snapshots_no_truncate
BEFORE TRUNCATE ON live_recovery_policy_snapshots
FOR EACH STATEMENT EXECUTE FUNCTION kb_reject_append_only();

CREATE TABLE live_recovery_feature_snapshots (
    id uuid PRIMARY KEY,
    schema_version smallint NOT NULL CHECK (schema_version = 1),
    live_recovery_enabled boolean NOT NULL,
    intended_state_sha256 kb_sha256 NOT NULL,
    canonical_payload bytea NOT NULL,
    content_sha256 kb_sha256 NOT NULL UNIQUE,
    created_at timestamptz NOT NULL DEFAULT now(),
    CHECK (octet_length(canonical_payload) BETWEEN 1 AND 4096),
    CHECK (content_sha256 = encode(digest(canonical_payload, 'sha256'), 'hex'))
);
CREATE TRIGGER live_recovery_feature_snapshots_immutable
BEFORE UPDATE OR DELETE ON live_recovery_feature_snapshots
FOR EACH ROW EXECUTE FUNCTION kb_reject_append_only();
CREATE TRIGGER live_recovery_feature_snapshots_no_truncate
BEFORE TRUNCATE ON live_recovery_feature_snapshots
FOR EACH STATEMENT EXECUTE FUNCTION kb_reject_append_only();

INSERT INTO live_recovery_policy_snapshots(
    id,schema_version,claim_lease_ms,max_batch_size,max_global_concurrency,
    max_concurrency_by_kind,canonical_payload,content_sha256,created_at
)
SELECT '638e256e-b95f-55af-a335-dad1f72f1592',1,60000,32,8,
       '{"dirty_manifest":1,"orphan_match_job":2,"orphan_target":4}'::jsonb,
       payload,encode(digest(payload,'sha256'),'hex'),'1970-01-01 UTC'
  FROM (VALUES (convert_to(
    '{"claim_lease_ms":60000,"max_batch_size":32,"max_concurrency_by_kind":{"dirty_manifest":1,"orphan_match_job":2,"orphan_target":4},"max_global_concurrency":8,"schema_version":1}',
    'UTF8'))) AS seed(payload);

INSERT INTO live_recovery_feature_snapshots(
    id,schema_version,live_recovery_enabled,intended_state_sha256,
    canonical_payload,content_sha256,created_at
)
SELECT '87c935bb-4d16-5ba3-a4c3-28eab0d92960',1,true,
       '8fb8882fa007b69a9d704d8d86b16fff6ffc7c5c4d01a697dec484c34bad4ce0',
       payload,encode(digest(payload,'sha256'),'hex'),'1970-01-01 UTC'
  FROM (VALUES (convert_to(
    '{"intended_state_sha256":"8fb8882fa007b69a9d704d8d86b16fff6ffc7c5c4d01a697dec484c34bad4ce0","live_recovery_enabled":true,"schema_version":1,"task_type":"system:live-recovery:v1"}',
    'UTF8'))) AS seed(payload);

CREATE TABLE live_recovery_configuration (
    singleton_key boolean PRIMARY KEY DEFAULT true CHECK (singleton_key),
    policy_snapshot_id uuid NOT NULL REFERENCES live_recovery_policy_snapshots(id) ON DELETE RESTRICT,
    feature_snapshot_id uuid NOT NULL REFERENCES live_recovery_feature_snapshots(id) ON DELETE RESTRICT
);
INSERT INTO live_recovery_configuration(singleton_key,policy_snapshot_id,feature_snapshot_id)
VALUES(true,'638e256e-b95f-55af-a335-dad1f72f1592','87c935bb-4d16-5ba3-a4c3-28eab0d92960');

CREATE TABLE system_live_recovery_claims (
    id uuid PRIMARY KEY,
    recovery_kind text NOT NULL CHECK (recovery_kind IN (
        'dirty_manifest','orphan_target','orphan_match_job'
    )),
    target_kind text NOT NULL CHECK (target_kind IN (
        'matching_manifest','document_conversion','extraction_target',
        'attachment_preparation','submission_render','matching_job'
    )),
    durable_id uuid NOT NULL,
    generation bigint NOT NULL CHECK (generation > 0),
    observed_watermark bigint NOT NULL CHECK (observed_watermark >= 0),
    observed_stage text NOT NULL CHECK (octet_length(observed_stage) BETWEEN 1 AND 64),
    observed_heartbeat_at timestamptz,
    observed_owner_token uuid,
    observed_attempt integer CHECK (observed_attempt IS NULL OR observed_attempt > 0),
    recovery_epoch bigint NOT NULL CHECK (recovery_epoch > 0),
    policy_snapshot_id uuid NOT NULL REFERENCES live_recovery_policy_snapshots(id) ON DELETE RESTRICT,
    feature_snapshot_id uuid NOT NULL REFERENCES live_recovery_feature_snapshots(id) ON DELETE RESTRICT,
    original_snapshots jsonb NOT NULL CHECK (
        jsonb_typeof(original_snapshots) = 'array'
        AND jsonb_array_length(original_snapshots) <= 8
        AND octet_length(original_snapshots::text) <= 4096
    ),
    status text NOT NULL CHECK (status IN ('pending','running','completed','noop','failed')),
    claim_token uuid,
    attempt integer NOT NULL DEFAULT 0 CHECK (attempt BETWEEN 0 AND 1000),
    claimed_by text CHECK (claimed_by IS NULL OR octet_length(claimed_by) BETWEEN 1 AND 128),
    claim_lease_ms integer NOT NULL CHECK (claim_lease_ms BETWEEN 5000 AND 300000),
    heartbeat_at timestamptz,
    action_applied boolean NOT NULL DEFAULT false,
    last_error_code text CHECK (last_error_code IS NULL OR octet_length(last_error_code) BETWEEN 1 AND 128),
    terminal_code text CHECK (terminal_code IS NULL OR octet_length(terminal_code) BETWEEN 1 AND 128),
    receipt jsonb CHECK (receipt IS NULL OR (
        jsonb_typeof(receipt) = 'object' AND octet_length(receipt::text) <= 8192
    )),
    discovered_at timestamptz NOT NULL DEFAULT now(),
    completed_at timestamptz,
    UNIQUE (recovery_kind,durable_id,generation,recovery_epoch),
    CHECK (
        (status = 'pending' AND claim_token IS NULL AND claimed_by IS NULL AND heartbeat_at IS NULL
            AND completed_at IS NULL AND terminal_code IS NULL AND receipt IS NULL)
        OR
        (status = 'running' AND claim_token IS NOT NULL AND claimed_by IS NOT NULL
            AND heartbeat_at IS NOT NULL AND completed_at IS NULL AND terminal_code IS NULL AND receipt IS NULL)
        OR
        (status IN ('completed','noop','failed') AND completed_at IS NOT NULL
            AND terminal_code IS NOT NULL)
    )
);
CREATE UNIQUE INDEX system_live_recovery_claims_active_identity_idx
    ON system_live_recovery_claims(recovery_kind,durable_id)
    WHERE status IN ('pending','running');
CREATE INDEX system_live_recovery_claims_pending_idx
    ON system_live_recovery_claims(recovery_epoch,discovered_at,id)
    WHERE status = 'pending';
CREATE INDEX system_live_recovery_claims_running_idx
    ON system_live_recovery_claims(heartbeat_at,id)
    WHERE status = 'running';

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
 ('bid.render-submission.v1', 1, 1, '{"queue":"bid-render-v1","schema_version":1,"task_type":"bid:render-submission:v1"}',
  encode(digest(convert_to('{"queue":"bid-render-v1","schema_version":1,"task_type":"bid:render-submission:v1"}', 'UTF8'), 'sha256'), 'hex'),
  '1970-01-01 UTC'),
 ('document.process', 1, 1, '{"claim_lease_ms":300000,"queue":"default","schema_version":1}',
  encode(digest(convert_to('{"claim_lease_ms":300000,"queue":"default","schema_version":1}', 'UTF8'), 'sha256'), 'hex'),
  '1970-01-01 UTC'),
 ('object.retention', 1, 1, '{"claim_lease_ms":60000,"queue":"retention","schema_version":1}',
  encode(digest(convert_to('{"claim_lease_ms":60000,"queue":"retention","schema_version":1}', 'UTF8'), 'sha256'), 'hex'),
  '1970-01-01 UTC');
INSERT INTO queue_contract_current(contract_key, version, generation)
VALUES ('bid.render-submission.v1', 1, 0),
       ('document.process', 1, 0),
       ('object.retention', 1, 0);
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
CREATE UNIQUE INDEX object_retention_outbox_claim_token_uidx
    ON object_retention_outbox(claim_token)
    WHERE claim_token IS NOT NULL;

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
    UNIQUE (claim_token),
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
       OR registry.byte_length <> p_byte_length OR registry.state <> 'available' THEN
        RAISE EXCEPTION 'object upload staging content identity mismatch'
            USING ERRCODE = '23514';
    END IF;
    PERFORM kb_object_reference_add(
        p_object_ref, p_digest, p_media_type, p_byte_length,
        p_owner_kind, p_owner_id, p_occurrence, p_actor
    );
    DELETE FROM object_upload_staging WHERE id = p_staging_id;
    PERFORM kb_object_reference_remove(
        p_object_ref, 'object_upload_staging', p_staging_id, 'payload'
    );
END
$$;

CREATE FUNCTION kb_object_upload_abandon(
    p_staging_id uuid,
    p_actor kb_actor_identity
)
RETURNS boolean
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE
    staging object_upload_staging%ROWTYPE;
BEGIN
    SELECT * INTO staging FROM object_upload_staging
     WHERE id = p_staging_id FOR UPDATE;
    IF NOT FOUND THEN
        RETURN false;
    END IF;
    IF staging.created_by <> p_actor THEN
        RAISE EXCEPTION 'object upload staging owner mismatch' USING ERRCODE = '42501';
    END IF;
    DELETE FROM object_upload_staging WHERE id = p_staging_id;
    RETURN kb_object_reference_remove(
        staging.object_ref, 'object_upload_staging', p_staging_id, 'payload'
    );
END
$$;

CREATE FUNCTION kb_object_upload_expire()
RETURNS integer
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
DECLARE
    staging object_upload_staging%ROWTYPE;
    expired_count integer := 0;
BEGIN
    FOR staging IN
        SELECT * FROM object_upload_staging
         WHERE expires_at <= clock_timestamp()
         ORDER BY expires_at, id
         FOR UPDATE SKIP LOCKED
         -- The retention service invokes this function once per expiry tick.
         LIMIT 100
    LOOP
        DELETE FROM object_upload_staging WHERE id = staging.id;
        PERFORM kb_object_reference_remove(
            staging.object_ref, 'object_upload_staging', staging.id, 'payload'
        );
        expired_count := expired_count + 1;
    END LOOP;
    RETURN expired_count;
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

    -- A worker generates the claim token before the request. If the response is
    -- lost, the same token resumes and renews the exact claim without consuming
    -- another attempt.
    RETURN QUERY
    WITH resumed AS (
        UPDATE object_retention_outbox outbox
           SET heartbeat_at = clock_timestamp(),
               lease_until = clock_timestamp() + make_interval(secs => p_lease_ms / 1000.0)
         WHERE outbox.state = 'claimed' AND outbox.claim_token = p_claim_token
           AND outbox.claimed_by = p_worker_name
           AND outbox.lease_until > clock_timestamp()
         RETURNING outbox.object_ref, outbox.attempt
    )
    SELECT registry.object_ref, registry.digest, registry.byte_length, resumed.attempt
      FROM resumed JOIN object_registry registry USING (object_ref)
     WHERE registry.state = 'deleting'
       AND NOT EXISTS (
           SELECT 1 FROM object_owner_references refs
            WHERE refs.object_ref = resumed.object_ref
       );
    IF FOUND THEN
        RETURN;
    END IF;
    IF EXISTS (
        SELECT 1 FROM object_retention_outbox WHERE claim_token = p_claim_token
        UNION ALL
        SELECT 1 FROM object_retention_attempt_receipts WHERE claim_token = p_claim_token
        UNION ALL
        SELECT 1 FROM object_retention_tombstones WHERE claim_token = p_claim_token
    ) THEN
        RAISE EXCEPTION 'retention claim token cannot be reused' USING ERRCODE = '22023';
    END IF;

    RETURN QUERY
    WITH candidate AS (
        SELECT outbox.object_ref, outbox.state AS prior_state,
               outbox.claim_token AS prior_claim_token, outbox.attempt AS prior_attempt
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
    ), expired_receipt AS (
        INSERT INTO object_retention_attempt_receipts AS receipt(
            object_ref, claim_token, attempt, outcome, error_code
        )
        SELECT candidate.object_ref, candidate.prior_claim_token,
               candidate.prior_attempt, 'retry', 'LEASE_EXPIRED'
          FROM candidate
         WHERE candidate.prior_state = 'claimed'
        RETURNING receipt.object_ref
    ), claimed AS (
        UPDATE object_retention_outbox outbox
           SET state = 'claimed', attempt = outbox.attempt + 1,
               claim_token = p_claim_token, claimed_by = p_worker_name,
               heartbeat_at = clock_timestamp(),
               lease_until = clock_timestamp() + make_interval(secs => p_lease_ms / 1000.0),
               last_error_code = NULL
          FROM candidate
         WHERE outbox.object_ref = candidate.object_ref
           AND (candidate.prior_state <> 'claimed' OR EXISTS (
               SELECT 1 FROM expired_receipt
                WHERE expired_receipt.object_ref = candidate.object_ref
           ))
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
    outbox_found boolean;
BEGIN
    IF p_error_code !~ '^[A-Z][A-Z0-9_]{0,63}$' THEN
        RAISE EXCEPTION 'invalid retention error code' USING ERRCODE = '22023';
    END IF;
    SELECT * INTO item FROM object_retention_outbox
     WHERE object_ref = p_object_ref FOR UPDATE;
    outbox_found := FOUND;
    IF EXISTS (
        SELECT 1 FROM object_retention_attempt_receipts
         WHERE object_ref = p_object_ref AND claim_token = p_claim_token
           AND outcome = 'retry' AND error_code = p_error_code
    ) THEN
        RETURN true;
    END IF;
    IF EXISTS (
        SELECT 1 FROM object_retention_attempt_receipts
         WHERE claim_token = p_claim_token
    ) THEN
        RAISE EXCEPTION 'retention failure receipt conflicts with prior outcome'
            USING ERRCODE = '40001';
    END IF;
    IF NOT outbox_found OR item.state <> 'claimed' OR item.claim_token <> p_claim_token
       OR item.lease_until <= clock_timestamp() THEN
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
GRANT SELECT ON application_maintenance_gate
TO kb_runtime_retention;
GRANT EXECUTE ON FUNCTION kb_actor_identity_valid(text)
TO kb_runtime_api, kb_runtime_worker, kb_runtime_retention;
GRANT EXECUTE ON FUNCTION kb_register_knowledge_document_object(uuid, text, kb_actor_identity, text, uuid),
    kb_release_knowledge_document_object(uuid, kb_actor_identity, text, uuid),
    kb_object_upload_stage(uuid, kb_object_ref, kb_sha256, text, bigint, kb_actor_identity),
    kb_object_upload_abandon(uuid, kb_actor_identity)
TO kb_runtime_api, kb_runtime_worker;
GRANT EXECUTE ON FUNCTION kb_object_upload_expire()
TO kb_runtime_retention;
GRANT EXECUTE ON FUNCTION kb_retention_claim(uuid, text, integer),
    kb_retention_heartbeat(kb_object_ref, uuid, integer),
    kb_retention_complete(kb_object_ref, uuid),
    kb_retention_fail(kb_object_ref, uuid, text)
TO kb_runtime_retention;
