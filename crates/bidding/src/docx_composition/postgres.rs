//! Version-bound source resolution for durable composition requests. The small
//! snapshot references existing immutable analysis/source records; it does not
//! duplicate page images or copy the prior bid into the model's input.
use super::{CompositionMode, agent::Config, validate_basis};
use crate::{
    agent_error::AgentError,
    docx_round::{DocxRoundBasis, DocxVersionIdentity},
    tender_analysis::{AnalysisResult, FrozenInput, digest},
};
use platform::BidAuthoringRequestIdentityV2;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenCompositionRequest {
    pub schema_version: u8,
    pub workspace_id: Uuid,
    pub actor: String,
    pub basis: DocxRoundBasis,
    pub expected: Option<DocxVersionIdentity>,
    pub mode: CompositionMode,
    /// Only draft fill freezes a read-back seed; official composition writes the
    /// whole document from the analysis and has nothing to seed.
    pub seed_plan_sha256: Option<String>,
    pub source_request: BidAuthoringRequestIdentityV2,
    pub source_input_sha256: String,
    pub analysis_sha256: String,
    pub config: Config,
    pub contract_sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    source_request: BidAuthoringRequestIdentityV2,
    input: FrozenInput,
    analysis: AnalysisResult,
}

/// Source bytes are loaded from the referenced immutable records on each run.
/// Persist `request` and its digest; never recreate it from current heads on retry.
pub struct PreparedComposition {
    pub request: FrozenCompositionRequest,
    pub input: FrozenInput,
    pub analysis: AnalysisResult,
}

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

fn invalid(e: impl std::fmt::Display) -> AgentError {
    AgentError::new("AGENT_OUTPUT_INVALID", e.to_string())
}
fn changed(message: &str) -> AgentError {
    AgentError::new("FROZEN_INPUT_DIGEST_MISMATCH", message)
}

impl FrozenCompositionRequest {
    pub fn sha256(&self) -> Result<String, AgentError> {
        digest(self).map_err(invalid)
    }

    fn validate(&self, source: &Source) -> Result<(), AgentError> {
        self.source_request.validate().map_err(invalid)?;
        if self.schema_version != 1
            || self.workspace_id.is_nil()
            || self.mode != CompositionMode::Official
            || self.seed_plan_sha256.is_some()
            || self.source_request != source.source_request
            || self.source_input_sha256 != digest(&source.input).map_err(invalid)?
            || self.analysis_sha256 != digest(&source.analysis).map_err(invalid)?
            || self.contract_sha256 != self.config.contract_sha256()?
            || source.input.document_set_id != self.basis.document_set_id.to_string()
        {
            return Err(changed("composition source or runtime identity changed"));
        }
        validate_basis(&source.input, &source.analysis).map_err(invalid)
    }
}

/// Prepare only from the user's selected current source and DOCX identities.
/// No model call, request enqueue, document write or new-round publication occurs.
/// Publication must independently recheck the frozen basis and expected version.
pub async fn prepare(
    pool: &PgPool,
    workspace_id: Uuid,
    basis: DocxRoundBasis,
    expected: Option<DocxVersionIdentity>,
    actor: &str,
    config: Config,
) -> Result<PreparedComposition, AgentError> {
    let contract_sha256 = config.contract_sha256()?;
    let sqlx::types::Json(source): sqlx::types::Json<Source> = sqlx::query_scalar(
        "SELECT kb_bid_v2_prepare_docx_composition_source($1,$2,$3,$4::kb_actor_identity)",
    )
    .bind(workspace_id)
    .bind(sqlx::types::Json(&basis))
    .bind(sqlx::types::Json(&expected))
    .bind(actor)
    .fetch_one(pool)
    .await
    .map_err(db_error)?;
    let request = FrozenCompositionRequest {
        schema_version: 1,
        workspace_id,
        actor: actor.into(),
        basis,
        expected,
        mode: CompositionMode::Official,
        seed_plan_sha256: None,
        source_request: source.source_request.clone(),
        source_input_sha256: digest(&source.input).map_err(invalid)?,
        analysis_sha256: digest(&source.analysis).map_err(invalid)?,
        config,
        contract_sha256,
    };
    request.validate(&source)?;
    Ok(PreparedComposition {
        request,
        input: source.input,
        analysis: source.analysis,
    })
}

/// Restore against the persisted request digest and original immutable sources,
/// even after later uploads, analysis publications or user edits. The original
/// expected DOCX version remains frozen; restoring never approves overwriting it.
pub async fn restore(
    pool: &PgPool,
    request: FrozenCompositionRequest,
    expected_request_sha256: &str,
) -> Result<PreparedComposition, AgentError> {
    if request.sha256()? != expected_request_sha256 {
        return Err(changed("composition request snapshot changed"));
    }
    let sqlx::types::Json(source): sqlx::types::Json<Source> = sqlx::query_scalar(
        "SELECT kb_bid_v2_load_docx_composition_source($1,$2,$3::kb_actor_identity)",
    )
    .bind(request.workspace_id)
    .bind(sqlx::types::Json(&request.basis))
    .bind(&request.actor)
    .fetch_one(pool)
    .await
    .map_err(db_error)?;
    request.validate(&source)?;
    Ok(PreparedComposition {
        request,
        input: source.input,
        analysis: source.analysis,
    })
}

/// Persist the prepared identity with idempotency/audit. Transport dispatch and
/// final publication are separate integration steps; this function does not enqueue.
pub async fn create_request(
    pool: &PgPool,
    request: &FrozenCompositionRequest,
    idempotency_key: &str,
) -> Result<serde_json::Value, AgentError> {
    // Revalidate source semantics and the running binary's frozen contract before
    // a persisted request can become executable. SQL rechecks source/version CAS.
    restore(pool, request.clone(), &request.sha256()?).await?;
    sqlx::query_scalar(
        "SELECT kb_bid_v2_create_docx_composition_request($1,$2,$3,$4::kb_actor_identity,$5)",
    )
    .bind(Uuid::new_v4())
    .bind(sqlx::types::Json(request))
    .bind(request.config.contract_definition())
    .bind(&request.actor)
    .bind(idempotency_key)
    .fetch_one(pool)
    .await
    .map_err(db_error)
}

pub async fn load_request(
    pool: &PgPool,
    identity: &BidAuthoringRequestIdentityV2,
) -> Result<PreparedComposition, AgentError> {
    identity.validate().map_err(invalid)?;
    let snapshot: Option<sqlx::types::Json<FrozenCompositionRequest>> =
        sqlx::query_scalar("SELECT kb_bid_v2_load_docx_composition_request($1,$2,$3::kb_sha256)")
            .bind(identity.request_artifact_id)
            .bind(identity.request_revision)
            .bind(&identity.frozen_input_sha256)
            .fetch_one(pool)
            .await
            .map_err(db_error)?;
    let request = snapshot
        .ok_or_else(|| AgentError::new("FROZEN_INPUT_MISSING", "composition request missing"))?
        .0;
    restore(pool, request, &identity.frozen_input_sha256).await
}

/// Shares the authoring execution owner and append-only boundary/checkpoint
/// ledgers with extraction, while using distinct composition stages and contracts.
pub struct PgJournal<'a> {
    pub pool: &'a PgPool,
    pub request: &'a BidAuthoringRequestIdentityV2,
    pub owner: &'a crate::bid_authoring_v2::AgentRunLease,
}

#[async_trait::async_trait]
impl super::agent::Journal for PgJournal<'_> {
    async fn load(&self) -> Result<Option<super::agent::Checkpoint>, AgentError> {
        let value: Option<sqlx::types::Json<super::agent::Checkpoint>> = sqlx::query_scalar(
            "SELECT kb_bid_v2_docx_composition_checkpoint_get($1,$2::kb_sha256)",
        )
        .bind(self.request.request_artifact_id)
        .bind(&self.request.frozen_input_sha256)
        .fetch_one(self.pool)
        .await
        .map_err(db_error)?;
        Ok(value.map(|v| v.0))
    }

    async fn reserve(
        &self,
        state: &super::agent::Checkpoint,
        body: &[u8],
    ) -> Result<usize, AgentError> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let count: i64 = sqlx::query_scalar(
            "SELECT kb_bid_v2_docx_composition_reserve($1,$2::kb_sha256,$3,$4,$5,$6,$7)",
        )
        .bind(self.request.request_artifact_id)
        .bind(&self.request.frozen_input_sha256)
        .bind(self.owner.attempt)
        .bind(self.owner.execution_owner_token)
        .bind(i32::try_from(state.turn).map_err(invalid)?)
        .bind(state.workspace.reviewing)
        .bind(body)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_error)?;
        sqlx::query("SELECT kb_bid_v2_docx_composition_checkpoint_put($1,$2::kb_sha256,$3,$4,$5)")
            .bind(self.request.request_artifact_id)
            .bind(&self.request.frozen_input_sha256)
            .bind(self.owner.attempt)
            .bind(self.owner.execution_owner_token)
            .bind(sqlx::types::Json(state))
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        usize::try_from(count).map_err(invalid)
    }

    async fn save(&self, state: &super::agent::Checkpoint) -> Result<(), AgentError> {
        sqlx::query("SELECT kb_bid_v2_docx_composition_checkpoint_put($1,$2::kb_sha256,$3,$4,$5)")
            .bind(self.request.request_artifact_id)
            .bind(&self.request.frozen_input_sha256)
            .bind(self.owner.attempt)
            .bind(self.owner.execution_owner_token)
            .bind(sqlx::types::Json(state))
            .execute(self.pool)
            .await
            .map_err(db_error)?;
        Ok(())
    }
}

/// Exact bytes attested by the persisted independent review. The worker stages
/// and writes these through the shared object registry before `publish_staged`.
pub struct ReviewedPublication {
    pub docx: crate::docx_round::InitialDocx,
    pub manifest: Vec<u8>,
    pub manifest_sha256: String,
    pub actor: String,
    checkpoint_sha256: String,
}

pub async fn prepare_publication(
    pool: &PgPool,
    identity: &BidAuthoringRequestIdentityV2,
) -> Result<ReviewedPublication, AgentError> {
    use base64::Engine as _;
    use sha2::{Digest, Sha256};
    let prepared = load_request(pool, identity).await?;
    let checkpoint: Option<sqlx::types::Json<super::agent::Checkpoint>> =
        sqlx::query_scalar("SELECT kb_bid_v2_docx_composition_checkpoint_get($1,$2::kb_sha256)")
            .bind(identity.request_artifact_id)
            .bind(&identity.frozen_input_sha256)
            .fetch_one(pool)
            .await
            .map_err(db_error)?;
    let checkpoint = checkpoint
        .ok_or_else(|| invalid("reviewed checkpoint missing"))?
        .0;
    let artifact = super::agent::reviewed_artifact(
        &prepared.input,
        &prepared.analysis,
        &prepared.request.config,
        &checkpoint,
    )?;
    let docx = crate::docx_round::InitialDocx::new(
        base64::engine::general_purpose::STANDARD
            .decode(&artifact.docx_base64)
            .map_err(invalid)?,
    )
    .map_err(invalid)?;
    let manifest = serde_json_canonicalizer::to_vec(&artifact.manifest).map_err(invalid)?;
    Ok(ReviewedPublication {
        docx,
        manifest_sha256: hex::encode(Sha256::digest(&manifest)),
        manifest,
        actor: prepared.request.actor,
        checkpoint_sha256: digest(&checkpoint).map_err(invalid)?,
    })
}

async fn verify_object(sha256: &str, byte_length: u64) -> Result<(), AgentError> {
    use sha2::{Digest, Sha256};
    if sha256.len() != 64
        || !sha256
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    {
        return Err(invalid("invalid published object digest"));
    }
    let sha256 = sha256.to_owned();
    tokio::task::spawn_blocking(move || {
        let bytes = platform::read_blob(&sha256).map_err(|e| {
            AgentError::new("INTERNAL", format!("published object unavailable: {e}"))
        })?;
        if bytes.len() as u64 != byte_length || hex::encode(Sha256::digest(&bytes)) != sha256 {
            return Err(invalid(
                "published object bytes do not match reviewed identity",
            ));
        }
        Ok(())
    })
    .await
    .map_err(|e| AgentError::new("INTERNAL", e.to_string()))?
}

/// Read before staging or writing on every retry, including a lost commit ACK.
/// Neither current heads nor an expired owner may redirect this original result.
/// Missing or corrupt files are errors, never a reason to silently overwrite them.
pub async fn replay_publication(
    pool: &PgPool,
    identity: &BidAuthoringRequestIdentityV2,
) -> Result<Option<serde_json::Value>, AgentError> {
    let receipt = replay_receipt(pool, identity).await?;
    if let Some(value) = &receipt {
        verify_object(
            value["docx_sha256"]
                .as_str()
                .ok_or_else(|| invalid("DOCX identity missing"))?,
            value["byte_length"]
                .as_u64()
                .ok_or_else(|| invalid("DOCX length missing"))?,
        )
        .await?;
        verify_object(
            value["manifest"]["sha256"]
                .as_str()
                .ok_or_else(|| invalid("manifest identity missing"))?,
            value["manifest"]["byte_length"]
                .as_u64()
                .ok_or_else(|| invalid("manifest length missing"))?,
        )
        .await?;
    }
    Ok(receipt)
}

pub(super) async fn replay_receipt(
    pool: &PgPool,
    identity: &BidAuthoringRequestIdentityV2,
) -> Result<Option<serde_json::Value>, AgentError> {
    identity.validate().map_err(invalid)?;
    // Attest the full transport identity, including request revision.
    let exists: Option<serde_json::Value> =
        sqlx::query_scalar("SELECT kb_bid_v2_load_docx_composition_request($1,$2,$3::kb_sha256)")
            .bind(identity.request_artifact_id)
            .bind(identity.request_revision)
            .bind(&identity.frozen_input_sha256)
            .fetch_one(pool)
            .await
            .map_err(db_error)?;
    if exists.is_none() {
        return Err(AgentError::new(
            "FROZEN_INPUT_MISSING",
            "composition request missing",
        ));
    }
    let receipt: Option<serde_json::Value> =
        sqlx::query_scalar("SELECT kb_bid_v2_replay_docx_composition($1,$2::kb_sha256)")
            .bind(identity.request_artifact_id)
            .bind(&identity.frozen_input_sha256)
            .fetch_one(pool)
            .await
            .map_err(db_error)?;
    Ok(receipt)
}

/// Atomically consume both staging identities and publish the new round. Caller
/// retains its existing staging cleanup guards on *any* error, including an
/// ambiguous commit ACK; the next delivery uses `replay_publication` first.
/// A successful replay never consumes caller staging and is deliberately separate.
pub async fn publish_staged(
    pool: &PgPool,
    identity: &BidAuthoringRequestIdentityV2,
    owner: &crate::bid_authoring_v2::AgentRunLease,
    docx_staging: Uuid,
    manifest_staging: Uuid,
) -> Result<serde_json::Value, AgentError> {
    let publication = prepare_publication(pool, identity).await?;
    verify_object(
        publication.docx.sha256(),
        publication.docx.bytes().len() as u64,
    )
    .await?;
    verify_object(
        &publication.manifest_sha256,
        publication.manifest.len() as u64,
    )
    .await?;
    commit_publication(
        pool,
        identity,
        owner,
        &publication,
        docx_staging,
        manifest_staging,
    )
    .await
}

/// Internal SQL boundary. Callers must verify both physical objects first.
pub(super) async fn commit_publication(
    pool: &PgPool,
    identity: &BidAuthoringRequestIdentityV2,
    owner: &crate::bid_authoring_v2::AgentRunLease,
    publication: &ReviewedPublication,
    docx_staging: Uuid,
    manifest_staging: Uuid,
) -> Result<serde_json::Value, AgentError> {
    sqlx::query_scalar(
        "SELECT kb_bid_v2_publish_docx_composition($1,$2::kb_sha256,$3,$4,$5::kb_sha256,$6,$7)",
    )
    .bind(identity.request_artifact_id)
    .bind(&identity.frozen_input_sha256)
    .bind(owner.attempt)
    .bind(owner.execution_owner_token)
    .bind(&publication.checkpoint_sha256)
    .bind(docx_staging)
    .bind(manifest_staging)
    .fetch_one(pool)
    .await
    .map_err(db_error)
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
