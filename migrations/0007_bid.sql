-- Bid* tables. Workspace.kind / documents.index_ready live in 0001.

CREATE TABLE IF NOT EXISTS bid_projects (
    id uuid PRIMARY KEY,
    title text NOT NULL,
    owner_name text NOT NULL DEFAULT '',
    expires_at timestamptz,
    status text NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'ended')),
    ended_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    extract_lock_token uuid,
    extract_lock_kind text,
    extract_lock_at timestamptz,
    extract_lock_section_id uuid,
    match_generation bigint NOT NULL DEFAULT 0,
    match_dirty boolean NOT NULL DEFAULT false,
    CHECK (
        (extract_lock_token IS NULL AND extract_lock_kind IS NULL
            AND extract_lock_at IS NULL AND extract_lock_section_id IS NULL)
        OR (extract_lock_token IS NOT NULL AND extract_lock_kind = 'full'
            AND extract_lock_at IS NOT NULL AND extract_lock_section_id IS NULL)
        OR (extract_lock_token IS NOT NULL AND extract_lock_kind = 'section_retry'
            AND extract_lock_at IS NOT NULL AND extract_lock_section_id IS NOT NULL)
    )
);

CREATE TABLE IF NOT EXISTS bid_documents (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL REFERENCES bid_projects (id) ON DELETE CASCADE,
    file_name text NOT NULL,
    file_hash text NOT NULL,
    file_size bigint NOT NULL DEFAULT 0,
    object_key text NOT NULL,
    parse_status text NOT NULL DEFAULT 'pending'
        CHECK (parse_status IN ('pending', 'processing', 'completed', 'failed')),
    markdown_ref text,
    parsed_at timestamptz,
    error_message text NOT NULL DEFAULT '',
    conversion_generation bigint NOT NULL DEFAULT 0,
    conversion_claim_token uuid,
    conversion_heartbeat_at timestamptz,
    multimodal_status text NOT NULL DEFAULT 'pending'
        CHECK (multimodal_status IN ('pending', 'running', 'done', 'failed', 'skipped')),
    multimodal_error text NOT NULL DEFAULT '',
    created_at timestamptz NOT NULL DEFAULT now(),
    CHECK (
        (parse_status = 'processing' AND conversion_claim_token IS NOT NULL
            AND conversion_heartbeat_at IS NOT NULL)
        OR (parse_status <> 'processing' AND conversion_claim_token IS NULL
            AND conversion_heartbeat_at IS NULL)
    ),
    CHECK (parse_status <> 'completed' OR multimodal_status IN ('done', 'skipped'))
);

CREATE TABLE IF NOT EXISTS bid_sections (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL REFERENCES bid_projects (id) ON DELETE CASCADE,
    document_id uuid NOT NULL REFERENCES bid_documents (id) ON DELETE CASCADE,
    section_key text NOT NULL,
    heading_path text NOT NULL DEFAULT '',
    hint_family text NOT NULL DEFAULT 'unknown'
        CHECK (hint_family IN ('technical', 'commercial', 'skip', 'unknown')),
    body text NOT NULL DEFAULT '',
    extract_status text NOT NULL DEFAULT 'pending'
        CHECK (extract_status IN ('pending', 'running', 'done', 'failed', 'skipped')),
    error_message text NOT NULL DEFAULT '',
    merge_into uuid REFERENCES bid_sections (id) ON DELETE SET NULL,
    UNIQUE (document_id, section_key)
);

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'bid_projects_extract_lock_section_fk'
    ) THEN
        ALTER TABLE bid_projects
            ADD CONSTRAINT bid_projects_extract_lock_section_fk
            FOREIGN KEY (extract_lock_section_id) REFERENCES bid_sections (id) ON DELETE RESTRICT;
    END IF;
END $$;

CREATE TABLE IF NOT EXISTS bid_section_retry_jobs (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL REFERENCES bid_projects (id) ON DELETE CASCADE,
    section_id uuid NOT NULL REFERENCES bid_sections (id) ON DELETE CASCADE,
    status text NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'running', 'done', 'failed')),
    claim_token uuid,
    heartbeat_at timestamptz,
    error_message text NOT NULL DEFAULT '',
    created_at timestamptz NOT NULL DEFAULT now(),
    finished_at timestamptz,
    CHECK (
        (status = 'running' AND claim_token IS NOT NULL AND heartbeat_at IS NOT NULL)
        OR (status <> 'running' AND claim_token IS NULL AND heartbeat_at IS NULL)
    )
);
CREATE UNIQUE INDEX IF NOT EXISTS bid_section_retry_jobs_active_uidx
    ON bid_section_retry_jobs (section_id) WHERE status IN ('pending', 'running');

CREATE TABLE IF NOT EXISTS bid_extract_runs (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL REFERENCES bid_projects (id) ON DELETE CASCADE,
    document_id uuid REFERENCES bid_documents (id) ON DELETE CASCADE,
    status text NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'running', 'done', 'failed')),
    triggered_by text NOT NULL DEFAULT 'auto' CHECK (triggered_by IN ('auto', 'manual')),
    section_total integer NOT NULL DEFAULT 0,
    section_done integer NOT NULL DEFAULT 0,
    extractor_mode text NOT NULL DEFAULT 'hybrid'
        CHECK (extractor_mode IN ('agent', 'hybrid', 'heuristic')),
    model_id text NOT NULL DEFAULT '',
    policy_version text NOT NULL DEFAULT '',
    prompt_version text NOT NULL DEFAULT '',
    diagnostics jsonb NOT NULL DEFAULT '{}'::jsonb,
    claim_token uuid,
    heartbeat_at timestamptz,
    error_message text NOT NULL DEFAULT '',
    started_at timestamptz,
    finished_at timestamptz,
    conversion_generation bigint,
    CHECK (
        (status = 'running' AND claim_token IS NOT NULL AND heartbeat_at IS NOT NULL)
        OR (status <> 'running' AND claim_token IS NULL AND heartbeat_at IS NULL)
    )
);

CREATE UNIQUE INDEX IF NOT EXISTS bid_extract_runs_auto_conversion_uidx
    ON bid_extract_runs (document_id, conversion_generation)
    WHERE triggered_by = 'auto';
CREATE UNIQUE INDEX IF NOT EXISTS bid_extract_runs_one_running_project_uidx
    ON bid_extract_runs (project_id) WHERE status = 'running';
CREATE INDEX IF NOT EXISTS bid_extract_runs_project_latest_idx
    ON bid_extract_runs (project_id, started_at DESC, id DESC);
CREATE INDEX IF NOT EXISTS bid_extract_runs_status_heartbeat_idx
    ON bid_extract_runs (status, heartbeat_at);

CREATE TABLE IF NOT EXISTS bid_clauses (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL REFERENCES bid_projects (id) ON DELETE CASCADE,
    extract_run_id uuid REFERENCES bid_extract_runs (id) ON DELETE SET NULL,
    section_id uuid REFERENCES bid_sections (id) ON DELETE SET NULL,
    source_document_id uuid REFERENCES bid_documents (id) ON DELETE SET NULL,
    source_span jsonb,
    family_conflict boolean NOT NULL DEFAULT false,
    extraction_meta jsonb NOT NULL DEFAULT '{}'::jsonb,
    raw_text text NOT NULL DEFAULT '',
    text text NOT NULL DEFAULT '',
    family text NOT NULL CHECK (family IN ('technical', 'commercial')),
    must boolean NOT NULL DEFAULT false,
    status text NOT NULL DEFAULT 'draft'
        CHECK (status IN ('draft', 'confirmed', 'rejected', 'superseded')),
    deviate boolean NOT NULL DEFAULT false,
    deviate_note text NOT NULL DEFAULT '',
    assessment text NOT NULL DEFAULT 'unset'
        CHECK (assessment IN ('unset', 'meet', 'partial', 'deviate', 'fail')),
    confirmed_at timestamptz,
    superseded_by_run_id uuid REFERENCES bid_extract_runs (id) ON DELETE SET NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS bid_clauses_source_status_idx
    ON bid_clauses (source_document_id, status);
CREATE INDEX IF NOT EXISTS bid_clauses_section_status_idx
    ON bid_clauses (section_id, status);
CREATE INDEX IF NOT EXISTS bid_clauses_project_status_idx
    ON bid_clauses (project_id, status);

CREATE TABLE IF NOT EXISTS bid_match_jobs (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL REFERENCES bid_projects (id) ON DELETE CASCADE,
    status text NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'running', 'done', 'failed')),
    tech_status text NOT NULL DEFAULT 'pending'
        CHECK (tech_status IN ('pending', 'skipped', 'running', 'done', 'failed')),
    commercial_status text NOT NULL DEFAULT 'pending'
        CHECK (commercial_status IN ('pending', 'skipped', 'running', 'done', 'failed')),
    debounce_key text NOT NULL DEFAULT '',
    tech_candidates jsonb NOT NULL DEFAULT '[]'::jsonb,
    error_message text NOT NULL DEFAULT '',
    unit_id uuid,
    job_kind text NOT NULL CHECK (job_kind IN ('technical', 'commercial')),
    generation bigint NOT NULL DEFAULT 0,
    claim_token uuid,
    heartbeat_at timestamptz,
    started_at timestamptz,
    finished_at timestamptz,
    CHECK (
        (status = 'running' AND claim_token IS NOT NULL AND heartbeat_at IS NOT NULL)
        OR (status <> 'running' AND claim_token IS NULL AND heartbeat_at IS NULL)
    )
);

CREATE INDEX IF NOT EXISTS bid_match_jobs_pending_generation_idx
    ON bid_match_jobs (project_id, generation, status);
CREATE UNIQUE INDEX IF NOT EXISTS bid_match_jobs_generation_kind_unit_uidx
    ON bid_match_jobs (project_id, generation, job_kind, unit_id) NULLS NOT DISTINCT;

CREATE TABLE IF NOT EXISTS bid_picks (
    project_id uuid NOT NULL REFERENCES bid_projects (id) ON DELETE CASCADE,
    unit_id uuid NOT NULL,
    product_id uuid NOT NULL REFERENCES products (id) ON DELETE CASCADE,
    version_id uuid NOT NULL REFERENCES product_versions (id) ON DELETE CASCADE,
    score double precision NOT NULL DEFAULT 0,
    coverage double precision NOT NULL DEFAULT 0,
    picked_at timestamptz NOT NULL DEFAULT now(),
    clauses jsonb NOT NULL DEFAULT '[]'::jsonb,
    PRIMARY KEY (project_id, unit_id, product_id)
);

CREATE TABLE IF NOT EXISTS bid_commercial_hits (
    project_id uuid NOT NULL REFERENCES bid_projects (id) ON DELETE CASCADE,
    clause_id uuid NOT NULL REFERENCES bid_clauses (id) ON DELETE CASCADE,
    outcome text NOT NULL CHECK (outcome IN ('hit', 'miss')),
    document_id uuid REFERENCES documents (id) ON DELETE SET NULL,
    version_id uuid REFERENCES product_versions (id) ON DELETE SET NULL,
    file_name text,
    score double precision,
    product_id uuid REFERENCES products (id) ON DELETE SET NULL,
    matched_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, clause_id)
);

CREATE TABLE IF NOT EXISTS bid_shots (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL REFERENCES bid_projects (id) ON DELETE CASCADE,
    clause_id uuid NOT NULL REFERENCES bid_clauses (id) ON DELETE CASCADE,
    product_id uuid NOT NULL REFERENCES products (id) ON DELETE CASCADE,
    version_id uuid NOT NULL REFERENCES product_versions (id) ON DELETE CASCADE,
    source text NOT NULL CHECK (source IN ('matched', 'uploaded')),
    object_key text NOT NULL,
    kb_document_id uuid REFERENCES documents (id) ON DELETE SET NULL,
    kb_image_ref text
);

CREATE TABLE IF NOT EXISTS bid_booklet_parts (
    project_id uuid NOT NULL REFERENCES bid_projects (id) ON DELETE CASCADE,
    part_key text NOT NULL,
    markdown text NOT NULL DEFAULT '',
    generated_at timestamptz,
    edited_at timestamptz,
    stale boolean NOT NULL DEFAULT false,
    PRIMARY KEY (project_id, part_key)
);
