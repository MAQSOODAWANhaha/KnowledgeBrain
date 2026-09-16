//! Optional product-path runner. Missing model config or a non-v2 analysis
//! skips semantic review; it does not inherit composition approval.
use super::agent::{self, Config, ConfiguredModel, FrozenFiles, Report};
use super::postgres::PgJournal;
use crate::agent_error::AgentError;
use super::FrozenContext;
use crate::bid_authoring_v2::AgentRunLease;
use crate::tender_analysis::{AnalysisResult, FrozenInput};
use platform::BidAuthoringRequestIdentityV2;
use serde_json::Value;
use sqlx::PgPool;
use tokio_util::sync::CancellationToken;

fn skip_provider(error: &AgentError) -> bool {
    error.code == "AGENT_PROVIDER_UNAVAILABLE"
}

pub async fn run_if_allowed(
    pool: &PgPool,
    request: &BidAuthoringRequestIdentityV2,
    context: Option<&FrozenContext>,
    docx: &[u8],
    pdf: &[u8],
    cancel: &CancellationToken,
) -> Result<Option<Report>, AgentError> {
    if !context.is_some_and(FrozenContext::allows_semantic_export_review) {
        return Ok(None);
    }
    let config = match Config::from_environment() {
        Ok(config) => config,
        Err(error) if skip_provider(&error) => return Ok(None),
        Err(error) => return Err(error),
    };
    let basis: Value = sqlx::query_scalar(
        "SELECT kb_bid_v2_load_export_review_basis($1,$2::kb_sha256)",
    )
    .bind(request.request_artifact_id)
    .bind(&request.frozen_input_sha256)
    .fetch_one(pool)
    .await
    .map_err(|e| AgentError::new("INTERNAL", e.to_string()))?;
    if basis["allowed"] != true {
        return Ok(None);
    }
    let frozen: FrozenInput =
        serde_json::from_value(basis["input"].clone()).map_err(|e| {
            AgentError::new("INTERNAL", e.to_string())
        })?;
    let result: AnalysisResult = serde_json::from_value(basis["analysis_result"].clone())
        .map_err(|e| AgentError::new("INTERNAL", e.to_string()))?;
    if result.schema_version != 2 {
        return Ok(None);
    }
    let claim: Value =
        sqlx::query_scalar("SELECT kb_bid_v2_tender_agent_claim($1,$2,$3::kb_sha256)")
            .bind(request.request_artifact_id)
            .bind(request.request_revision)
            .bind(&request.frozen_input_sha256)
            .fetch_one(pool)
            .await
            .map_err(|e| AgentError::new("INTERNAL", e.to_string()))?;
    match claim["disposition"].as_str() {
        Some("claimed") => {}
        Some("obsolete" | "live_owner" | "exhausted") => return Ok(None),
        _ => {
            return Err(AgentError::new("INTERNAL", "unknown export-review claim disposition"))
        }
    }
    let owner = AgentRunLease {
        attempt: claim["attempt"]
            .as_i64()
            .and_then(|n| i32::try_from(n).ok())
            .unwrap_or(1),
        max_attempts: claim["max_attempts"]
            .as_i64()
            .and_then(|n| i32::try_from(n).ok())
            .unwrap_or(4),
        execution_owner_token: claim["execution_owner_token"]
            .as_str()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| {
                AgentError::new("INTERNAL", "export-review owner token missing")
            })?,
    };
    let journal = PgJournal {
        pool,
        request,
        owner: &owner,
    };
    match agent::run(
        &frozen,
        &result,
        FrozenFiles {
            docx,
            pdf: Some(pdf),
        },
        &config,
        &journal,
        &ConfiguredModel,
        cancel,
    )
    .await
    {
        Ok(report) => Ok(Some(report)),
        Err(error) if skip_provider(&error) => Ok(None),
        Err(error) => Err(error),
    }
}
