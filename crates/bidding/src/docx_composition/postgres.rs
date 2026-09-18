//! Shared fill error mapping and composition report lookup.
use crate::agent_error::AgentError;
use sqlx::PgPool;
use uuid::Uuid;

pub(super) fn db_error(error: sqlx::Error) -> AgentError {
    if let Some(db) = error.as_database_error() {
        match db.message() {
            "DOCX_VERSION_CAS_MISMATCH"
            | "DOCX_ROUND_BASIS_CHANGED"
            | "DOCX_ROUND_REQUIREMENTS_NOT_CURRENT" => {
                return AgentError::new("WORKSPACE_CAS_CONFLICT", db.message());
            }
            "DOCX_COMPOSITION_ANALYSIS_MISSING" => {
                return AgentError::new("FROZEN_INPUT_MISSING", db.message());
            }
            _ => {}
        }
    }
    crate::tender_analysis::postgres::db_error(error)
}

pub async fn get_manifest_identity(
    pool: &PgPool,
    workspace: Uuid,
    version: Uuid,
    actor: &str,
) -> Result<Option<serde_json::Value>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT kb_bid_v2_get_docx_composition_manifest($1,$2,$3::kb_actor_identity)",
    )
    .bind(workspace)
    .bind(version)
    .bind(actor)
    .fetch_one(pool)
    .await
}
