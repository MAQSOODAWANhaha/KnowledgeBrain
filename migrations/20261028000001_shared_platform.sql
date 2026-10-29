SET LOCAL search_path = public, pg_catalog;

-- Fresh application storage. User authorization belongs to business services.
-- Object ownership, upload transfer and retention are serialized by Rust
-- transactions. SQL owns relational integrity, not business permissions.
CREATE EXTENSION IF NOT EXISTS pgcrypto;

CREATE DOMAIN kb_sha256 AS text CHECK (VALUE ~ '^[0-9a-f]{64}$');
CREATE DOMAIN kb_object_ref AS text CHECK (VALUE ~ '^objects/[0-9a-f]{64}$');

CREATE TABLE platform_users (
    id uuid PRIMARY KEY,
    email text NOT NULL UNIQUE,
    password_hash text,
    ldap_dn text,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE platform_api_keys (
    id uuid PRIMARY KEY,
    name text NOT NULL,
    key_hash text NOT NULL UNIQUE,
    prefix text NOT NULL,
    owner_user_id uuid NOT NULL REFERENCES platform_users (id),
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX platform_api_keys_owner_idx ON platform_api_keys (owner_user_id);

CREATE TABLE platform_models (
    id text PRIMARY KEY,
    kind text NOT NULL CHECK (kind IN ('embedding', 'chat', 'vlm', 'asr')),
    endpoint text,
    api_key_enc text,
    dimension integer,
    extra jsonb NOT NULL DEFAULT '{}'::jsonb
);

CREATE TABLE platform_audit_events (
    id uuid PRIMARY KEY,
    schema_version smallint NOT NULL CHECK (schema_version = 1),
    operation text NOT NULL CHECK (operation ~ '^[a-z][a-z0-9_.-]{0,127}$'),
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
CREATE INDEX platform_audit_events_entity_timeline_idx
    ON platform_audit_events(entity_kind, occurred_at, id);

CREATE TABLE platform_maintenance_state (
    singleton_key boolean PRIMARY KEY DEFAULT true CHECK (singleton_key),
    mode text NOT NULL CHECK (mode IN ('maintenance', 'open', 'draining', 'rollback')),
    generation bigint NOT NULL CHECK (generation >= 0),
    updated_at timestamptz NOT NULL,
    CHECK (isfinite(updated_at))
);
INSERT INTO platform_maintenance_state
    (singleton_key, mode, generation, updated_at)
VALUES (true, 'open', 0, '1970-01-01 UTC');

CREATE TABLE platform_objects (
    object_ref kb_object_ref PRIMARY KEY,
    content_sha256 kb_sha256 NOT NULL UNIQUE,
    media_type text NOT NULL CHECK (media_type ~ '^[a-z0-9][a-z0-9!#$&^_.+-]{0,63}/[a-z0-9][a-z0-9!#$&^_.+-]{0,63}$'),
    byte_length bigint NOT NULL CHECK (byte_length >= 0),
    state text NOT NULL CHECK (state IN ('available', 'deleting', 'deleted')),
    registered_at timestamptz NOT NULL DEFAULT now(),
    deleting_at timestamptz,
    deleted_at timestamptz,
    UNIQUE(object_ref,content_sha256,state),
    UNIQUE(object_ref,content_sha256,media_type,state),
    UNIQUE(object_ref,content_sha256,media_type,byte_length,state),
    CHECK (object_ref = 'objects/' || content_sha256),
    CHECK (
        (state = 'available' AND deleting_at IS NULL AND deleted_at IS NULL)
        OR (state = 'deleting' AND deleting_at IS NOT NULL AND deleted_at IS NULL)
        OR (state = 'deleted' AND deleting_at IS NOT NULL AND deleted_at IS NOT NULL)
    )
);

CREATE TABLE platform_object_references (
    object_ref kb_object_ref NOT NULL REFERENCES platform_objects(object_ref) ON DELETE RESTRICT,
    owner_kind text NOT NULL CHECK (owner_kind ~ '^[a-z][a-z0-9_.-]{0,63}$'),
    owner_id uuid NOT NULL,
    occurrence text NOT NULL CHECK (octet_length(occurrence) BETWEEN 1 AND 128),
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (object_ref, owner_kind, owner_id, occurrence),
    UNIQUE (owner_kind, owner_id, occurrence)
);

CREATE TABLE platform_object_uploads (
    id uuid PRIMARY KEY,
    object_ref kb_object_ref NOT NULL REFERENCES platform_objects(object_ref) ON DELETE RESTRICT,
    owner_user_id uuid REFERENCES platform_users(id) ON DELETE RESTRICT,
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL DEFAULT (now() + interval '24 hours'),
    CHECK (expires_at > created_at)
);
CREATE INDEX platform_object_uploads_expiry_idx
    ON platform_object_uploads(expires_at, id);

CREATE INDEX platform_object_uploads_object_idx ON platform_object_uploads(object_ref);

CREATE TABLE platform_object_deletion_requests (
    id uuid PRIMARY KEY,
    object_ref kb_object_ref NOT NULL UNIQUE REFERENCES platform_objects(object_ref) ON DELETE RESTRICT,
    content_sha256 kb_sha256 NOT NULL,
    byte_length bigint NOT NULL CHECK (byte_length >= 0),
    requested_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE(id, object_ref, content_sha256, byte_length),
    CHECK (object_ref = 'objects/' || content_sha256)
);

CREATE TABLE platform_object_deletion_receipts (
    object_ref kb_object_ref PRIMARY KEY,
    content_sha256 kb_sha256 NOT NULL UNIQUE,
    byte_length bigint NOT NULL CHECK (byte_length >= 0),
    deletion_id uuid NOT NULL UNIQUE,
    deleted_at timestamptz NOT NULL,
    CONSTRAINT platform_object_deletion_receipts_request_fk
        FOREIGN KEY(deletion_id,object_ref,content_sha256,byte_length)
        REFERENCES platform_object_deletion_requests(id,object_ref,content_sha256,byte_length),
    CHECK (object_ref = 'objects/' || content_sha256)
);

CREATE VIEW platform_available_objects AS
SELECT object_ref, content_sha256, media_type, byte_length, registered_at
  FROM platform_objects WHERE state = 'available';
