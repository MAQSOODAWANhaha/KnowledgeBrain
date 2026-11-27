SET LOCAL search_path = public, pg_catalog;

-- BEGIN web_source_settings
CREATE TABLE tender_project_source_settings (
    project_id uuid PRIMARY KEY REFERENCES tender_projects(id) ON DELETE RESTRICT,
    revision bigint NOT NULL DEFAULT 1 CHECK (revision > 0),
    settings jsonb NOT NULL CHECK (jsonb_typeof(settings) = 'object')
);
-- END web_source_settings
