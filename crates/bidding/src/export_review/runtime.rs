//! Execute only the request-frozen reviewer contract under the export owner's lease.
use super::FrozenContext;
use super::agent::{self, ConfiguredModel, FrozenFiles, Report};
use super::postgres::PgJournal;
use crate::agent_error::AgentError;
use crate::bid_authoring_v2::AgentRunLease;
use crate::tender_analysis::{AnalysisResult, FrozenInput};
use platform::BidAuthoringRequestIdentityV2;
use serde_json::Value;
use sqlx::PgPool;
use tokio_util::sync::CancellationToken;

/// Reads the same objects already staged by the export pipeline.
pub(crate) struct OutputImages<'a> {
    pub objects: &'a dyn crate::submission_export::ExportIo,
}

#[async_trait::async_trait]
impl agent::visual::ImageReader for OutputImages<'_> {
    async fn read(
        &self,
        image: &super::OutputImage,
        cancel: &CancellationToken,
    ) -> Result<Vec<u8>, AgentError> {
        let max_bytes = usize::try_from(image.byte_length).map_err(|_| {
            AgentError::new(
                "FROZEN_INPUT_DIGEST_MISMATCH",
                "output image byte length exceeds addressable memory",
            )
        })?;
        self.objects
            .read_blob(&image.sha256, max_bytes, cancel)
            .await
            .map_err(Into::into)
    }
}

pub async fn run_if_allowed(
    pool: &PgPool,
    request: &BidAuthoringRequestIdentityV2,
    owner: &AgentRunLease,
    context: Option<&FrozenContext>,
    files: FrozenFiles<'_>,
    cancel: &CancellationToken,
) -> Result<Option<Report>, AgentError> {
    let Some(context) = context.filter(|c| c.allows_semantic_export_review()) else {
        return Ok(None);
    };
    let config = context
        .execution_contract
        .as_ref()
        .expect("checked frozen execution")
        .config()?;
    let basis: Value =
        sqlx::query_scalar("SELECT kb_bid_v2_load_export_review_basis($1,$2::kb_sha256)")
            .bind(request.request_artifact_id)
            .bind(&request.frozen_input_sha256)
            .fetch_one(pool)
            .await
            .map_err(crate::tender_analysis::postgres::db_error)?;
    if basis["allowed"] != true {
        return Err(AgentError::new(
            "FROZEN_INPUT_DIGEST_MISMATCH",
            "frozen export review basis unavailable",
        ));
    }
    let frozen: FrozenInput = serde_json::from_value(basis["input"].clone())
        .map_err(|e| AgentError::new("FROZEN_INPUT_DIGEST_MISMATCH", e.to_string()))?;
    let result: AnalysisResult = serde_json::from_value(basis["analysis_result"].clone())
        .map_err(|e| AgentError::new("FROZEN_INPUT_DIGEST_MISMATCH", e.to_string()))?;
    let journal = PgJournal {
        pool,
        request,
        owner,
    };
    agent::run(
        &frozen,
        &result,
        files,
        &config,
        &journal,
        &ConfiguredModel,
        cancel,
    )
    .await
    .map(Some)
}
