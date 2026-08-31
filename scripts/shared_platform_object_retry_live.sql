\set ON_ERROR_STOP on
BEGIN;

DO $$
DECLARE
    object_bytes bytea := convert_to('object retry reclaim fixture v1', 'UTF8');
    object_digest kb_sha256 := encode(digest(object_bytes, 'sha256'), 'hex');
    object_reference kb_object_ref;
    committing_bytes bytea := convert_to('object commit fail-closed fixture v1', 'UTF8');
    committing_digest kb_sha256 := encode(digest(committing_bytes, 'sha256'), 'hex');
    committing_reference kb_object_ref;
    actor kb_actor_identity := 'system:tender-document-process-v2';
BEGIN
    object_reference := ('objects/' || object_digest)::kb_object_ref;
    committing_reference := ('objects/' || committing_digest)::kb_object_ref;

    PERFORM kb_object_upload_stage(
        'aaa10000-0000-4000-8000-000000000001', object_reference,
        object_digest, 'application/json', octet_length(object_bytes), actor
    );
    IF NOT kb_object_upload_abandon('aaa10000-0000-4000-8000-000000000001', actor) THEN
        RAISE EXCEPTION 'initial staging reference was not released';
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM object_registry registry
        JOIN object_retention_outbox outbox USING (object_ref)
        WHERE registry.object_ref = object_reference
          AND registry.state = 'deleting' AND outbox.state = 'queued'
    ) THEN
        RAISE EXCEPTION 'released staging object was not queued for retention';
    END IF;

    -- An Oxana retry may safely reclaim an exact immutable identity before
    -- retention has claimed physical deletion.
    PERFORM kb_object_upload_stage(
        'aaa10000-0000-4000-8000-000000000002', object_reference,
        object_digest, 'application/json', octet_length(object_bytes), actor
    );
    IF NOT EXISTS (
        SELECT 1 FROM object_registry
        WHERE object_ref = object_reference AND state = 'available'
    ) OR EXISTS (
        SELECT 1 FROM object_retention_outbox WHERE object_ref = object_reference
    ) OR NOT EXISTS (
        SELECT 1 FROM object_upload_staging
        WHERE id = 'aaa10000-0000-4000-8000-000000000002'
    ) THEN
        RAISE EXCEPTION 'queued retention object was not reclaimed atomically';
    END IF;
    PERFORM kb_object_upload_abandon('aaa10000-0000-4000-8000-000000000002', actor);

    UPDATE object_retention_outbox
       SET state = 'claimed', attempt = attempt + 1,
           claim_token = 'aaa10000-0000-4000-8000-000000000003',
           claimed_by = 'object-retry-live', heartbeat_at = clock_timestamp(),
           lease_until = clock_timestamp() + interval '5 minutes'
     WHERE object_ref = object_reference;
    BEGIN
        PERFORM kb_object_upload_stage(
            'aaa10000-0000-4000-8000-000000000004', object_reference,
            object_digest, 'application/json', octet_length(object_bytes), actor
        );
        RAISE EXCEPTION 'claimed retention object was incorrectly reclaimed';
    EXCEPTION WHEN check_violation THEN
        IF SQLERRM <> 'object registry identity mismatch or object unavailable' THEN
            RAISE;
        END IF;
    END;
    IF NOT EXISTS (
        SELECT 1 FROM object_registry registry
        JOIN object_retention_outbox outbox USING (object_ref)
        WHERE registry.object_ref = object_reference
          AND registry.state = 'deleting' AND outbox.state = 'claimed'
    ) OR EXISTS (
        SELECT 1 FROM object_upload_staging
        WHERE id = 'aaa10000-0000-4000-8000-000000000004'
    ) THEN
        RAISE EXCEPTION 'claimed retention failure was not fail-closed';
    END IF;

    UPDATE object_retention_outbox
       SET state = 'queued', claim_token = NULL, claimed_by = NULL,
           heartbeat_at = NULL, lease_until = NULL
     WHERE object_ref = object_reference;
    INSERT INTO object_retention_tombstones(
        object_ref, digest, byte_length, deleted_by, claim_token, deleted_at
    ) VALUES (
        object_reference, object_digest, octet_length(object_bytes), actor,
        'aaa10000-0000-4000-8000-000000000005', clock_timestamp()
    );
    BEGIN
        PERFORM kb_object_upload_stage(
            'aaa10000-0000-4000-8000-000000000006', object_reference,
            object_digest, 'application/json', octet_length(object_bytes), actor
        );
        RAISE EXCEPTION 'tombstoned retention object was incorrectly reclaimed';
    EXCEPTION WHEN check_violation THEN
        IF SQLERRM <> 'object registry identity mismatch or object unavailable' THEN
            RAISE;
        END IF;
    END;
    IF NOT EXISTS (
        SELECT 1 FROM object_retention_outbox
        WHERE object_ref = object_reference AND state = 'queued'
    ) OR EXISTS (
        SELECT 1 FROM object_upload_staging
        WHERE id = 'aaa10000-0000-4000-8000-000000000006'
    ) THEN
        RAISE EXCEPTION 'tombstoned retention failure was not atomic';
    END IF;
    BEGIN
        DELETE FROM object_retention_tombstones WHERE object_ref = object_reference;
        RAISE EXCEPTION 'retention tombstone delete was accepted';
    EXCEPTION WHEN insufficient_privilege THEN NULL;
    END;
    IF NOT EXISTS (
        SELECT 1 FROM object_retention_tombstones WHERE object_ref = object_reference
    ) THEN
        RAISE EXCEPTION 'retention tombstone delete was not fail-closed';
    END IF;

    -- A staged row cannot make an unavailable registry identity commit-able.
    -- The stage, reference, outbox, and registry transition must all survive a
    -- rejected commit so retention remains authoritative.
    PERFORM kb_object_upload_stage(
        'aaa10000-0000-4000-8000-000000000007', committing_reference,
        committing_digest, 'application/json', octet_length(committing_bytes), actor
    );
    UPDATE object_registry
       SET state = 'deleting', deleting_at = clock_timestamp()
     WHERE object_ref = committing_reference;
    INSERT INTO object_retention_outbox(object_ref, state)
    VALUES (committing_reference, 'queued');
    BEGIN
        PERFORM kb_object_upload_commit(
            'aaa10000-0000-4000-8000-000000000007', committing_reference,
            committing_digest, 'application/json', octet_length(committing_bytes),
            'workspace_asset', 'aaa10000-0000-4000-8000-000000000008', 'payload', actor
        );
        RAISE EXCEPTION 'deleting registry identity was committed';
    EXCEPTION WHEN check_violation THEN
        IF SQLERRM <> 'object upload staging content identity mismatch' THEN
            RAISE;
        END IF;
    END;
    IF NOT EXISTS (
        SELECT 1 FROM object_registry registry
        JOIN object_retention_outbox outbox USING (object_ref)
        JOIN object_upload_staging staging USING (object_ref)
        WHERE registry.object_ref = committing_reference
          AND registry.state = 'deleting' AND outbox.state = 'queued'
          AND staging.id = 'aaa10000-0000-4000-8000-000000000007'
    ) THEN
        RAISE EXCEPTION 'rejected object commit was not atomic';
    END IF;
END
$$;

ROLLBACK;
\echo shared-platform-object-retry-ok
