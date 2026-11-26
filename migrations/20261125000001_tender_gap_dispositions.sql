ALTER TABLE tender_authoring_jobs
    ADD CONSTRAINT tender_authoring_jobs_execution_project_uidx UNIQUE (execution_id, project_id);

CREATE TABLE tender_authoring_gap_dispositions (
    execution_id text NOT NULL,
    project_id uuid NOT NULL REFERENCES tender_projects(id),
    gap_id text NOT NULL,
    status text NOT NULL,
    note text NOT NULL DEFAULT '',
    updated_by_user_id uuid NOT NULL REFERENCES platform_users(id),
    first_handled_at timestamptz,
    handled_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (execution_id, gap_id),
    CONSTRAINT tender_authoring_gap_dispositions_job_fk
        FOREIGN KEY (execution_id, project_id)
        REFERENCES tender_authoring_jobs(execution_id, project_id)
);

CREATE INDEX tender_authoring_gap_dispositions_project_idx
    ON tender_authoring_gap_dispositions(project_id, execution_id);
