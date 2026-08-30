-- Live retention worker contract upgrade. Safe for an existing database.
BEGIN;

CREATE OR REPLACE FUNCTION kb_retention_claim(
    p_claim_token uuid,
    p_worker_name text,
    p_lease_ms integer
)
RETURNS TABLE(object_ref kb_object_ref, digest kb_sha256, byte_length bigint, attempt integer)
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
SET plan_cache_mode = force_custom_plan
AS $$
DECLARE
    claimed_ref kb_object_ref;
    claimed_digest kb_sha256;
    claimed_bytes bigint;
    claimed_attempt integer;
BEGIN
    IF p_worker_name !~ '^[a-z0-9][a-z0-9._-]{0,63}$' OR p_lease_ms NOT BETWEEN 1000 AND 300000 THEN
        RAISE EXCEPTION 'invalid retention claim parameters' USING ERRCODE = '22023';
    END IF;

    -- A worker generates the claim token before the request. If the response is
    -- lost, the same token resumes and renews the exact claim without consuming
    -- another attempt.
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
      INTO claimed_ref, claimed_digest, claimed_bytes, claimed_attempt
      FROM resumed JOIN object_registry registry USING (object_ref)
     WHERE registry.state = 'deleting'
       AND NOT EXISTS (
           SELECT 1 FROM object_owner_references refs
            WHERE refs.object_ref = resumed.object_ref
       );
    IF FOUND THEN
        object_ref := claimed_ref;
        digest := claimed_digest;
        byte_length := claimed_bytes;
        attempt := claimed_attempt;
        RETURN NEXT;
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
      INTO claimed_ref, claimed_digest, claimed_bytes, claimed_attempt
      FROM claimed JOIN object_registry registry USING (object_ref);
    IF FOUND THEN
        object_ref := claimed_ref;
        digest := claimed_digest;
        byte_length := claimed_bytes;
        attempt := claimed_attempt;
        RETURN NEXT;
    END IF;
    RETURN;
END
$$;

CREATE OR REPLACE FUNCTION kb_retention_heartbeat(
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

CREATE OR REPLACE FUNCTION kb_retention_complete(
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

CREATE OR REPLACE FUNCTION kb_retention_fail(
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

REVOKE ALL ON FUNCTION
  kb_retention_claim(uuid, text, integer),
  kb_retention_heartbeat(kb_object_ref, uuid, integer),
  kb_retention_complete(kb_object_ref, uuid),
  kb_retention_fail(kb_object_ref, uuid, text)
FROM PUBLIC;
GRANT EXECUTE ON FUNCTION
  kb_retention_claim(uuid, text, integer),
  kb_retention_heartbeat(kb_object_ref, uuid, integer),
  kb_retention_complete(kb_object_ref, uuid),
  kb_retention_fail(kb_object_ref, uuid, text)
TO kb_runtime_retention;

COMMIT;
