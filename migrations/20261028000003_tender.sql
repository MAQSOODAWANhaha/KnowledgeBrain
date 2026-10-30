SET LOCAL search_path = public, pg_catalog;

-- Current Python tender workflow and Word editing. Fresh initialization only.
-- Ownership is checked by application services; leases and receipts remain durable.

CREATE TABLE tender_projects (
  id uuid PRIMARY KEY,
  owner_user_id uuid NOT NULL REFERENCES platform_users(id) ON DELETE RESTRICT,
  title text NOT NULL CHECK (octet_length(title) BETWEEN 1 AND 1024),
  status text NOT NULL CHECK (status IN ('open','ended')),
  created_at timestamptz NOT NULL DEFAULT now(),
  ended_at timestamptz,
  CHECK ((status='open' AND ended_at IS NULL) OR (status='ended' AND ended_at IS NOT NULL))
);

CREATE INDEX tender_projects_owner_created_idx ON tender_projects(owner_user_id, created_at DESC);

CREATE TABLE tender_documents (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES tender_projects(id) ON DELETE RESTRICT,
  file_name text NOT NULL CHECK (octet_length(file_name) BETWEEN 1 AND 1024),
  media_type text NOT NULL CHECK (octet_length(media_type) BETWEEN 1 AND 256),
  object_ref kb_object_ref NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  byte_length bigint NOT NULL CHECK (byte_length > 0),
  object_state text NOT NULL DEFAULT 'available' CHECK (object_state = 'available'),
  FOREIGN KEY (object_ref, content_sha256, media_type, byte_length, object_state)
    REFERENCES platform_objects(object_ref, content_sha256, media_type, byte_length, state),
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(id, project_id),
  CHECK (object_ref = 'objects/' || content_sha256)
);

CREATE INDEX tender_documents_project_created_idx ON tender_documents(project_id, created_at);

-- BEGIN analysis
CREATE TABLE tender_analysis_runs (
    run_id text PRIMARY KEY,
    request_key text NOT NULL UNIQUE,
    source_version integer NOT NULL DEFAULT 1,
    domain_revision integer NOT NULL DEFAULT 1,
    status text NOT NULL DEFAULT 'pending',
    epoch bigint NOT NULL DEFAULT 0,
    lease_until timestamptz,
    lease_command_id text,
    cancel_requested boolean NOT NULL DEFAULT false,
    checklist_digest text,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    updated_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
CREATE TABLE tender_analysis_source_versions (
    run_id text NOT NULL REFERENCES tender_analysis_runs(run_id),
    source_version integer NOT NULL,
    request_key text NOT NULL,
    manifest_ref jsonb NOT NULL,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (run_id, source_version),
    UNIQUE (run_id, request_key)
);
CREATE TABLE tender_analysis_revisions (
    run_id text NOT NULL,
    source_version integer NOT NULL,
    domain_revision integer NOT NULL,
    request_key text NOT NULL,
    corrected_items_ref jsonb,
    scope_decisions_ref jsonb,
    upstream_digest text NOT NULL DEFAULT '',
    reason text NOT NULL DEFAULT '',
    execution_spec jsonb,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (run_id, source_version, domain_revision),
    UNIQUE (run_id, request_key),
    FOREIGN KEY (run_id, source_version) REFERENCES tender_analysis_source_versions(run_id, source_version)
);
CREATE TABLE tender_analysis_commands (
    command_id text PRIMARY KEY,
    run_id text NOT NULL,
    source_version integer NOT NULL,
    domain_revision integer NOT NULL,
    kind text NOT NULL CHECK (kind IN ('start', 'resume')),
    payload jsonb NOT NULL DEFAULT '{}',
    status text NOT NULL DEFAULT 'pending',
    claim_token text,
    claim_until timestamptz,
    published_at timestamptz,
    started_at timestamptz,
    completed_at timestamptz,
    receipt jsonb,
    last_error jsonb,
    delivery_count integer NOT NULL DEFAULT 0,
    execution_count integer NOT NULL DEFAULT 0,
    infrastructure_failures integer NOT NULL DEFAULT 0,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    FOREIGN KEY (run_id, source_version, domain_revision) REFERENCES tender_analysis_revisions(run_id, source_version, domain_revision)
);
CREATE INDEX tender_analysis_commands_run_idx
    ON tender_analysis_commands(run_id, source_version, domain_revision, created_at);
CREATE INDEX tender_analysis_commands_dispatch_idx
    ON tender_analysis_commands(status, created_at);
CREATE TABLE tender_analysis_artifacts (
    run_id text NOT NULL,
    source_version integer NOT NULL,
    domain_revision integer NOT NULL,
    stage text NOT NULL,
    scope text NOT NULL DEFAULT '',
    artifact_ref jsonb NOT NULL,
    input_digest text NOT NULL DEFAULT '',
    metadata jsonb NOT NULL DEFAULT '{}',
    epoch bigint NOT NULL,
    committed_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (run_id, source_version, domain_revision, stage, scope),
    FOREIGN KEY (run_id, source_version) REFERENCES tender_analysis_source_versions(run_id, source_version)
);
CREATE TABLE tender_analysis_confirmations (
    run_id text NOT NULL,
    source_version integer NOT NULL,
    domain_revision integer NOT NULL,
    checklist_digest text NOT NULL,
    request_key text NOT NULL,
    command_id text NOT NULL REFERENCES tender_analysis_commands(command_id),
    issue_resolutions jsonb,
    confirmed_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (run_id, source_version, domain_revision),
    UNIQUE (run_id, request_key),
    FOREIGN KEY (run_id, source_version, domain_revision) REFERENCES tender_analysis_revisions(run_id, source_version, domain_revision)
);
CREATE TABLE tender_analysis_model_requests (
    request_id text PRIMARY KEY,
    run_id text NOT NULL,
    source_version integer NOT NULL,
    domain_revision integer NOT NULL,
    epoch bigint NOT NULL,
    model text NOT NULL,
    task text NOT NULL,
    status text NOT NULL DEFAULT 'prepared',
    metadata jsonb NOT NULL DEFAULT '{}',
    usage jsonb,
    response_ref jsonb,
    error jsonb,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    sent_at timestamptz,
    finished_at timestamptz,
    FOREIGN KEY (run_id, source_version, domain_revision) REFERENCES tender_analysis_revisions(run_id, source_version, domain_revision)
);
CREATE INDEX tender_analysis_model_requests_replay_idx
    ON tender_analysis_model_requests(run_id, source_version, domain_revision, task, (metadata->>'request_digest'))
    WHERE status IN ('sent', 'uncertain', 'succeeded');
CREATE INDEX tender_analysis_model_requests_source_reuse_idx
    ON tender_analysis_model_requests(run_id, source_version, task, (metadata->>'request_digest'))
    WHERE status = 'succeeded';
-- END analysis

-- BEGIN authoring
CREATE TABLE tender_authoring_executions (
    execution_id text PRIMARY KEY,
    authoring_input jsonb NOT NULL,
    execution_spec jsonb NOT NULL,
    status text NOT NULL DEFAULT 'pending',
    epoch bigint NOT NULL DEFAULT 0,
    lease_until timestamptz,
    cancel_requested boolean NOT NULL DEFAULT false,
    result_ref jsonb,
    receipts jsonb NOT NULL DEFAULT '[]',
    error jsonb,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    updated_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
CREATE TABLE tender_authoring_model_requests (
    request_id text PRIMARY KEY,
    execution_id text NOT NULL REFERENCES tender_authoring_executions(execution_id),
    epoch bigint NOT NULL,
    model text NOT NULL,
    task text NOT NULL,
    status text NOT NULL DEFAULT 'prepared',
    metadata jsonb NOT NULL,
    usage jsonb,
    response_ref jsonb,
    error jsonb,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    sent_at timestamptz,
    finished_at timestamptz
);
CREATE INDEX tender_authoring_requests_execution_idx
    ON tender_authoring_model_requests(execution_id, created_at);
-- END authoring

-- BEGIN web
CREATE TABLE tender_document_sources (
    document_id uuid PRIMARY KEY,
    project_id uuid NOT NULL REFERENCES tender_projects(id),
    original_ref jsonb NOT NULL,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    FOREIGN KEY(document_id, project_id) REFERENCES tender_documents(id, project_id)
);
CREATE TABLE tender_project_analyses (
    run_id text PRIMARY KEY REFERENCES tender_analysis_runs(run_id),
    project_id uuid NOT NULL REFERENCES tender_projects(id),
    request_key text NOT NULL,
    source_only boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    UNIQUE(project_id, request_key),
    UNIQUE(run_id, project_id)
);
CREATE INDEX tender_project_analyses_project_created_idx ON tender_project_analyses(project_id, created_at);
CREATE TABLE tender_authoring_jobs (
    execution_id text PRIMARY KEY,
    project_id uuid NOT NULL REFERENCES tender_projects(id),
    run_id text NOT NULL,
    payload jsonb NOT NULL,
    status text NOT NULL DEFAULT 'pending',
    claim_until timestamptz,
    resume_keys jsonb NOT NULL DEFAULT '[]',
    error jsonb,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    FOREIGN KEY(run_id, project_id) REFERENCES tender_project_analyses(run_id, project_id)
);
CREATE INDEX tender_authoring_jobs_project_created_idx
    ON tender_authoring_jobs(project_id, created_at);
CREATE INDEX tender_authoring_jobs_dispatch_idx
    ON tender_authoring_jobs(status, created_at);
CREATE TABLE tender_outline_revisions (
    revision_id uuid PRIMARY KEY,
    project_id uuid NOT NULL REFERENCES tender_projects(id),
    revision bigint NOT NULL CHECK (revision > 0),
    parent_id uuid REFERENCES tender_outline_revisions(revision_id),
    execution_id text NOT NULL REFERENCES tender_authoring_jobs(execution_id),
    outline_ref jsonb NOT NULL,
    request_key text NOT NULL,
    request_digest text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    UNIQUE(project_id, revision),
    UNIQUE(project_id, request_key)
);
-- END web

-- Immutable Word drafts for the Python tender workflow. Generated candidates do
-- not replace a selected draft; an editor save advances only its exact parent.
CREATE TABLE tender_draft_versions (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES tender_projects(id),
  execution_id text NOT NULL,
  round_id uuid NOT NULL,
  revision bigint NOT NULL CHECK (revision > 0),
  parent_version_id uuid,
  source text NOT NULL CHECK (source IN ('generated', 'manual', 'recovered')),
  object_ref kb_object_ref NOT NULL,
  content_sha256 kb_sha256 NOT NULL,
  byte_length bigint NOT NULL CHECK (byte_length > 0),
  media_type text NOT NULL DEFAULT 'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
  object_state text NOT NULL DEFAULT 'available' CHECK (object_state = 'available'),
  outline jsonb NOT NULL CHECK (jsonb_typeof(outline) = 'object'),
  created_by_user_id uuid NOT NULL REFERENCES platform_users(id),
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (project_id, revision),
  UNIQUE (id, project_id),
  FOREIGN KEY(parent_version_id, project_id) REFERENCES tender_draft_versions(id, project_id),
  FOREIGN KEY (object_ref, content_sha256, media_type, byte_length, object_state)
    REFERENCES platform_objects(object_ref, content_sha256, media_type, byte_length, state),
  CHECK (object_ref = 'objects/' || content_sha256)
);
CREATE UNIQUE INDEX tender_draft_versions_generated_execution_uidx
  ON tender_draft_versions(project_id, execution_id) WHERE source = 'generated';

CREATE TABLE tender_editor_sessions (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES tender_projects(id),
  base_version_id uuid NOT NULL,
  owner_user_id uuid NOT NULL REFERENCES platform_users(id),
  mode text NOT NULL CHECK (mode IN ('view', 'edit')),
  request_key text NOT NULL,
  request_sha256 kb_sha256 NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  closed_at timestamptz,
  UNIQUE (project_id, request_key),
  UNIQUE (id, project_id),
  FOREIGN KEY(base_version_id, project_id) REFERENCES tender_draft_versions(id, project_id)
);

CREATE TABLE tender_draft_state (
  project_id uuid PRIMARY KEY REFERENCES tender_projects(id),
  version_id uuid NOT NULL,
  session_id uuid,
  pending_save_id uuid,
  editor_error jsonb,
  FOREIGN KEY(version_id, project_id) REFERENCES tender_draft_versions(id, project_id),
  FOREIGN KEY(session_id, project_id) REFERENCES tender_editor_sessions(id, project_id)
);

CREATE TABLE tender_draft_saves (
  id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES tender_projects(id),
  session_id uuid NOT NULL,
  parent_version_id uuid NOT NULL,
  parent_sha256 kb_sha256 NOT NULL,
  request_key text NOT NULL,
  request_sha256 kb_sha256 NOT NULL,
  state text NOT NULL CHECK (state IN ('pending', 'saved', 'failed')),
  result_version_id uuid,
  callback_sha256 kb_sha256,
  error_code bigint,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (project_id, request_key),
  UNIQUE (id, project_id),
  FOREIGN KEY(session_id, project_id) REFERENCES tender_editor_sessions(id, project_id),
  FOREIGN KEY(parent_version_id, project_id) REFERENCES tender_draft_versions(id, project_id),
  FOREIGN KEY(result_version_id, project_id) REFERENCES tender_draft_versions(id, project_id),
  CHECK ((state = 'saved') = (result_version_id IS NOT NULL))
);

ALTER TABLE tender_draft_state ADD CONSTRAINT tender_draft_state_pending_save_fk
  FOREIGN KEY(pending_save_id, project_id) REFERENCES tender_draft_saves(id, project_id)
  DEFERRABLE INITIALLY DEFERRED;

CREATE TABLE tender_draft_recoveries (
  session_id uuid PRIMARY KEY REFERENCES tender_editor_sessions(id),
  callback_sha256 kb_sha256 NOT NULL,
  version_id uuid NOT NULL REFERENCES tender_draft_versions(id)
);
