use sqlx::{Connection, PgConnection, PgPool};

pub const KNOWLEDGE_BASE_BASELINE: &str =
    include_str!("../../../migrations/knowledge_base_baseline.sql");
pub const SHARED_PLATFORM_BASELINE: &str =
    include_str!("../../../migrations/shared_platform_baseline.sql");
pub const BIDDING_BASELINE: &str = include_str!("../../../migrations/bidding_v2_baseline.sql");

const DEFAULT_DATABASE_URL: &str =
    "postgres://knowledgebrain:knowledgebrain@127.0.0.1:15432/knowledgebrain";
const BOOTSTRAP_LOCK_ID: i64 = 0x4b_42_53_43_48_45_4d_41;

fn database_url_from_env_value(
    value: Result<String, std::env::VarError>,
) -> Result<String, std::env::VarError> {
    match value {
        Ok(value) => Ok(value),
        Err(std::env::VarError::NotPresent) => Ok(DEFAULT_DATABASE_URL.into()),
        Err(error) => Err(error),
    }
}

pub fn database_url() -> Result<String, std::env::VarError> {
    database_url_from_env_value(std::env::var("DATABASE_URL"))
}

static POOL: tokio::sync::OnceCell<PgPool> = tokio::sync::OnceCell::const_new();

async fn open_pool() -> Result<PgPool, sqlx::Error> {
    let database_url =
        database_url().map_err(|error| sqlx::Error::Configuration(Box::new(error)))?;
    sqlx::postgres::PgPoolOptions::new()
        .max_connections(16)
        .acquire_timeout(std::time::Duration::from_secs(10))
        .connect(&database_url)
        .await
}

/// Process-wide runtime pool. Runtime startup only connects; schema bootstrap is
/// an explicit deployment/test action and is never a readiness or launch gate.
pub async fn connect() -> Result<PgPool, sqlx::Error> {
    POOL.get_or_try_init(open_pool).await.cloned()
}

async fn schema_slice_state(
    connection: &mut PgConnection,
) -> Result<(bool, bool, bool, bool), sqlx::Error> {
    sqlx::query_as(
        "WITH
         knowledge(name) AS (VALUES
           ('users'),('workspaces'),('documents'),('chunks'),('knowledge_matching_scope_attestations_v2'),
           ('knowledge_image_artifact_revisions'),('knowledge_image_ocr_chunk_artifact_mappings')),
         shared(name) AS (VALUES
           ('object_registry'),('object_upload_staging'),('object_owner_references'),
           ('idempotency_requests'),('audit_events'),('queue_contract_current')),
         bidding(name) AS (VALUES
           ('bid_projects'),('bid_submission_workspaces'),('bid_workspace_revision_artifacts'),
           ('bid_workspace_asset_artifacts'),('bid_workspace_asset_retirement_artifacts'),('bid_outline_checkpoint_artifacts'),
           ('bid_content_generation_request_identities'),('bid_evidence_bundle_artifacts'),
           ('bid_evidence_asset_artifacts'),('bid_evidence_selection_artifacts'),
           ('bid_outline_assessment_snapshot_artifacts'),('bid_submission_assessment_snapshot_artifacts'),
           ('bid_submission_assessment_snapshot_evidence_items'),('bid_tender_structured_form_definition_artifacts'),
           ('bid_attachment_preparation_revision_artifacts'),('bid_attachment_preparation_asset_items'),
           ('bid_attachment_preparation_contract_artifacts'),('bid_pdf_attachment_preparation_attestations'),
           ('bid_submission_export_request_identities'),('bid_render_document_snapshot_artifacts'),
           ('bid_submission_manifest_artifacts'),('bid_submission_output_artifacts'),
           ('bid_submission_assessment_report_artifacts'),('bid_quote_snapshot_artifacts'),
           ('bid_quote_snapshot_object_identities')),
         bidding_functions(signature) AS (VALUES
           ('kb_bid_v2_create_project(uuid,text,uuid,kb_actor_identity,text,bytea,kb_sha256)'),
           ('kb_bid_v2_load_tender_document_process_input(uuid,bigint,kb_sha256)'),
           ('kb_bid_v2_retry_tender_document(uuid,uuid,uuid,bigint,kb_actor_identity,text,bytea,kb_sha256)'),
           ('kb_bid_v2_commit_workspace_mutation_idempotent(uuid,uuid,kb_sha256,jsonb,kb_actor_identity,text,bytea,kb_sha256)'),
           ('kb_bid_v2_publish_outline_generation(uuid,bigint,kb_sha256,uuid,bytea,kb_sha256,jsonb)'),
           ('kb_bid_v2_publish_content_generation(uuid,bigint,kb_sha256,uuid,kb_sha256,jsonb,uuid,bytea,kb_sha256,jsonb)'),
           ('kb_bid_v2_create_evidence_pick_set(uuid,uuid,uuid[],kb_actor_identity,text,bytea,kb_sha256)'),
           ('kb_bid_v2_create_node_evidence_pick_set(uuid,uuid,uuid,uuid[],kb_actor_identity,text,bytea,kb_sha256)'),
           ('kb_bid_v2_list_evidence_pick_sets(uuid,kb_actor_identity)'),
           ('kb_bid_v2_get_node_evidence(uuid,uuid,kb_actor_identity)'),
           ('kb_bid_v2_get_evidence_overview(uuid,kb_actor_identity)'),
           ('kb_bid_v2_get_current_assessments(uuid,kb_actor_identity)'),
           ('kb_bid_v2_get_preview_html(uuid,kb_actor_identity)'),
           ('kb_bid_v2_next_quote_snapshot_revision(uuid,kb_actor_identity)'),
           ('kb_bid_v2_publish_quote_snapshot(uuid,uuid,bigint,uuid,kb_object_ref,kb_sha256,bigint,bytea,kb_actor_identity,text,bytea,kb_sha256)'),
           ('kb_bid_v2_list_quote_snapshots(uuid,kb_actor_identity)'),
           ('kb_bid_v2_get_quote_snapshot(uuid,uuid,kb_actor_identity)'),
           ('kb_bid_v2_prepare_workspace_attachment(uuid,uuid,uuid,uuid[],uuid[],integer[],integer[],kb_actor_identity,text,bytea,kb_sha256)'),
           ('kb_bid_v2_create_outline_checkpoint(uuid,uuid,kb_sha256,uuid,kb_actor_identity,text,bytea,kb_sha256)'),
           ('kb_bid_v2_accept_candidate(uuid,uuid,uuid,kb_sha256,jsonb,integer[],kb_actor_identity,text,bytea,kb_sha256)'),
           ('kb_bid_v2_advance_workspace_projection(uuid,uuid,kb_sha256,uuid,kb_sha256)'),
           ('kb_bid_v2_get_requirement_projection(uuid,kb_actor_identity)'),
           ('kb_bid_v2_refresh_requirement_projection(uuid,uuid,kb_sha256,kb_actor_identity,text,bytea,kb_sha256)'),
           ('kb_bid_v2_retire_workspace_asset(uuid,uuid,text,kb_actor_identity,text,bytea,kb_sha256)'),
           ('kb_bid_v2_load_user_pick_evidence(uuid,bigint,kb_sha256)'),
           ('kb_bid_v2_load_submission_export_input(uuid,bigint,kb_sha256)'),
           ('kb_bid_v2_mark_requirement_set_compile_failed(uuid,bigint,kb_sha256,text)'),
           ('kb_bid_v2_publish_pdf_attachment_preparation(uuid,bigint,kb_sha256,uuid,uuid,uuid[],uuid[],kb_object_ref[],kb_sha256[],text[],bigint[],integer[],integer[],kb_actor_identity)'),
           ('kb_bid_v2_prepare_submission_export(uuid,bigint,kb_sha256,uuid,kb_object_ref,kb_sha256,text,uuid,uuid,kb_actor_identity)'),
           ('kb_bid_v2_load_submission_manifest_render_input(uuid,kb_sha256)'),
           ('kb_bid_v2_publish_submission_export(uuid,bigint,kb_sha256,uuid,kb_object_ref,kb_sha256,text,uuid,uuid,uuid,uuid,kb_object_ref,kb_sha256,text,bigint,kb_actor_identity)'),
           ('kb_bid_v2_list_submission_exports(uuid,kb_actor_identity)'),
           ('kb_bid_v2_get_submission_export(uuid,uuid,kb_actor_identity)'),
           ('kb_bid_v2_get_submission_assessment_report(uuid,uuid,kb_actor_identity)'),
           ('kb_bid_v2_get_submission_export_object(uuid,uuid,kb_actor_identity)'))
         SELECT
           (SELECT count(*)=7 FROM knowledge WHERE to_regclass('public.'||name) IS NOT NULL),
           (SELECT count(*)=6 FROM shared WHERE to_regclass('public.'||name) IS NOT NULL),
           ((SELECT count(*)=25 FROM bidding WHERE to_regclass('public.'||name) IS NOT NULL)
             AND (SELECT count(*)=35 FROM bidding_functions
               WHERE to_regprocedure('public.'||signature) IS NOT NULL)
             AND EXISTS (SELECT 1 FROM pg_attribute attribute
               WHERE attribute.attrelid=to_regclass('public.bid_workspace_asset_artifacts')
                 AND attribute.attname='file_name' AND NOT attribute.attisdropped)
             AND EXISTS (SELECT 1 FROM pg_attribute attribute
               WHERE attribute.attrelid=to_regclass('public.bid_workspace_revision_artifacts')
                 AND attribute.attname='requirement_projection_sha256' AND NOT attribute.attisdropped)
             AND EXISTS (SELECT 1 FROM pg_attribute attribute
               WHERE attribute.attrelid=to_regclass('public.bid_workspace_revision_artifacts')
                 AND attribute.attname='quote_snapshot_id' AND NOT attribute.attisdropped)
             AND EXISTS (SELECT 1 FROM pg_attribute attribute
               WHERE attribute.attrelid=to_regclass('public.bid_render_document_snapshot_artifacts')
                 AND attribute.attname='submission_assessment_snapshot_id' AND NOT attribute.attisdropped)),
           EXISTS (SELECT 1 FROM pg_class relation JOIN pg_namespace namespace ON namespace.oid=relation.relnamespace
             WHERE namespace.nspname='public' AND relation.relkind IN ('r','p','v','m'))",
    )
    .fetch_one(connection)
    .await
}

/// Apply the single fresh schema directly. This helper is checksum-, manifest-,
/// marker-, and compatibility-free. An already-complete schema is accepted;
/// a partial schema must be reset rather than silently repaired.
pub async fn apply_fresh_baseline(pool: &PgPool) -> Result<(), sqlx::Error> {
    let mut lock_connection = pool.acquire().await?;
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(BOOTSTRAP_LOCK_ID)
        .execute(&mut *lock_connection)
        .await?;

    let result = async {
        match schema_slice_state(&mut lock_connection).await? {
            (true, true, true, _) => return Ok(()),
            (false, false, false, false) => {}
            state => {
                return Err(sqlx::Error::Protocol(format!(
                    "fresh schema is partial or stale (knowledge={}, shared={}, bidding={}, objects_present={}); reset the database",
                    state.0, state.1, state.2, state.3
                )));
            }
        }

        let mut transaction = (*lock_connection).begin().await?;
        sqlx::raw_sql(KNOWLEDGE_BASE_BASELINE).execute(&mut *transaction).await?;
        sqlx::raw_sql(SHARED_PLATFORM_BASELINE).execute(&mut *transaction).await?;
        sqlx::raw_sql(BIDDING_BASELINE).execute(&mut *transaction).await?;
        transaction.commit().await
    }
    .await;

    let unlock = sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(BOOTSTRAP_LOCK_ID)
        .execute(&mut *lock_connection)
        .await;
    result?;
    unlock?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_database_url_only_when_absent() {
        assert_eq!(
            database_url_from_env_value(Err(std::env::VarError::NotPresent)).unwrap(),
            DEFAULT_DATABASE_URL
        );
        assert!(
            database_url_from_env_value(Err(std::env::VarError::NotUnicode(
                std::ffi::OsString::from("bad")
            )))
            .is_err()
        );
    }

    #[test]
    fn fresh_schema_contains_only_target_bidding_contract() {
        assert!(BIDDING_BASELINE.contains("CREATE TABLE bid_submission_workspaces"));
    }

    #[test]
    fn runtime_connect_never_bootstraps_schema() {
        let source = include_str!("db.rs");
        let connect_body = source
            .split("pub async fn connect()")
            .nth(1)
            .expect("connect body")
            .split("async fn schema_slice_state")
            .next()
            .expect("connect end");
        assert!(!connect_body.contains("apply_fresh_baseline"));
        assert!(!connect_body.contains("raw_sql"));
    }
}
