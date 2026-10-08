//! Production queue workers for the bidding v2 publish chain (B4 wiring).
//!
//! The Oxana glue (`FromContext`, `oxana::Worker`) lives in the worker crate's
//! `bidding.rs` as thin adapters; everything here is plain business logic so it
//! stays unit-testable without a queue.
//!
//! Chain wired by this module:
//! - `bid:tender_document_process:v2`: frozen tender input -> analysis agent ->
//!   `DbJournal::publish_outline` -> `outline::store::publish` ->
//!   `bid_outline_runs.status = 'published'`
//! - `bid:content_generate:v2`: latest published outline -> `match_queries` ->
//!   v2 knowledge retrieval -> `respond` -> `response::store::publish`

use crate::analysis::agent::{Config, ConfiguredModel};
use crate::analysis::FrozenInput;
use crate::journal_db::DbJournal;
use crate::outline;
use crate::response;
use crate::response::EvidenceHit;
use knowledge::knowledge_retrieval::{
    KnowledgeEvidenceScopeV2, KnowledgeRetrievalPortV3, ProductEvidenceRequestV1,
    RetrievalPolicyIdentityV1,
};
use knowledge::PostgresKnowledgeRetrievalAdapter;
use platform::{ContentGenerateJobV2, ContentGenerateOperationV2, TenderDocumentProcessJobV2};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// Deterministic run id for a tender job, derived from the request identity.
///
/// Retries and replays converge on the same `bid_outline_runs` row, which is
/// what makes `kb_bid_v2_publish_outline`'s `ON CONFLICT DO NOTHING` /
/// replay short-circuit idempotent.
pub fn tender_run_id(job: &TenderDocumentProcessJobV2) -> Uuid {
    let mut hasher = Sha256::new();
    hasher.update(b"bid:tender_document_process:v2:");
    hasher.update(job.request.request_artifact_id.as_bytes());
    hasher.update(b":");
    hasher.update(job.request.request_revision.to_string().as_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    // Mark v4/variant bits for tooling that keys off them; determinism comes
    // from the hash, not randomness.
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

/// Load and validate the frozen tender input.
///
/// The freeze step (docreader parse -> canonical FrozenInput JSON) is
/// upstream's responsibility: the API freezes the tender and stores the bytes
/// in the blob store under `objects/{frozen_input_sha256}` before enqueueing.
/// The worker only loads by digest and verifies.
async fn load_frozen_input(frozen_input_sha256: &str) -> Result<FrozenInput, String> {
    if frozen_input_sha256.len() != 64
        || !frozen_input_sha256
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err(format!(
            "FROZEN_INPUT_SHA_INVALID: {frozen_input_sha256}"
        ));
    }
    let bytes = platform::read_blob(frozen_input_sha256)
        .map_err(|error| format!("FROZEN_INPUT_MISSING: objects/{frozen_input_sha256}: {error}"))?;
    // Defense in depth: the blob store is addressed by digest, but verify anyway.
    let actual = hex::encode(Sha256::digest(&bytes));
    if actual != frozen_input_sha256 {
        return Err(format!(
            "FROZEN_INPUT_DIGEST_MISMATCH: expected {frozen_input_sha256}, got {actual}"
        ));
    }
    serde_json::from_slice::<FrozenInput>(&bytes)
        .map_err(|error| format!("FROZEN_INPUT_INVALID: {error}"))
}

fn agent_error_message(error: &crate::agent_error::AgentError) -> String {
    format!("{}: {}", error.code, error.message)
}

/// `bid:tender_document_process:v2` handler body.
pub async fn run_tender_document_process(
    pool: &PgPool,
    job: &TenderDocumentProcessJobV2,
    cancel: &CancellationToken,
) -> Result<(), String> {
    job.request
        .validate()
        .map_err(|error| format!("BID_REQUEST_INVALID: {error}"))?;
    let input = load_frozen_input(&job.request.frozen_input_sha256).await?;
    let input_project_id: Uuid = input
        .project_id
        .parse()
        .map_err(|error| format!("FROZEN_INPUT_PROJECT_INVALID: {error}"))?;
    if input_project_id != job.project_id {
        return Err(format!(
            "FROZEN_INPUT_PROJECT_MISMATCH: input belongs to {input_project_id}, job targets {}",
            job.project_id
        ));
    }
    let config =
        Config::from_environment_for(&input).map_err(|error| agent_error_message(&error))?;
    let journal = DbJournal::new(pool.clone(), job.project_id, tender_run_id(job));
    let model = ConfiguredModel;
    // `finalize_run` inside calls `journal.publish_outline`, which persists
    // the artifact via `outline::store::publish`.
    crate::analysis::agent::run(&input, &config, &journal, &model, cancel)
        .await
        .map_err(|error| agent_error_message(&error))?;
    Ok(())
}

/// Resolve the currently supported v2 retrieval policy.
///
/// A future iteration should freeze the policy at request time (like the
/// frozen input); for the minimal wiring the worker uses the active supported
/// policy from `knowledge_retrieval_policies_v2`.
async fn resolve_supported_policy(pool: &PgPool) -> Result<RetrievalPolicyIdentityV1, String> {
    let row: Option<(String, String, i64, i64, i64)> = sqlx::query_as(
        "SELECT policy_sha256, contract_version, max_hits, max_chunk_bytes, max_total_bytes
         FROM knowledge_retrieval_policies_v2
         WHERE support_state = 'supported'
         ORDER BY policy_sha256
         LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .map_err(|error| format!("RETRIEVAL_POLICY_LOOKUP_FAILED: {error}"))?;
    let (policy_sha256, contract_version, max_hits, max_chunk_bytes, max_total_bytes) =
        row.ok_or("RETRIEVAL_POLICY_UNAVAILABLE: no supported knowledge_retrieval_policies_v2 row")?;
    Ok(RetrievalPolicyIdentityV1 {
        contract_version,
        policy_sha256,
        max_hits: u32::try_from(max_hits)
            .map_err(|_| "RETRIEVAL_POLICY_INVALID: max_hits out of range")?,
        max_chunk_bytes: u32::try_from(max_chunk_bytes)
            .map_err(|_| "RETRIEVAL_POLICY_INVALID: max_chunk_bytes out of range")?,
        max_total_bytes: u64::try_from(max_total_bytes)
            .map_err(|_| "RETRIEVAL_POLICY_INVALID: max_total_bytes out of range")?,
    })
}

fn sha256_hex(value: &str) -> String {
    hex::encode(Sha256::digest(value.as_bytes()))
}

/// `bid:content_generate:v2` handler body.
pub async fn run_content_generate(
    pool: &PgPool,
    job: &ContentGenerateJobV2,
) -> Result<(), String> {
    job.request
        .validate()
        .map_err(|error| format!("BID_REQUEST_INVALID: {error}"))?;
    let artifact = outline::store::load_published(pool, job.project_id).await?;
    let queries = response::match_queries(&artifact)?;
    if job.operation == ContentGenerateOperationV2::MatchOnly {
        tracing::info!(
            project_id = %job.project_id,
            workspace_id = %job.workspace_id,
            query_count = queries.len(),
            "content_generate match-only: queries computed, no retrieval"
        );
        return Ok(());
    }
    let policy = resolve_supported_policy(pool).await?;
    let adapter = PostgresKnowledgeRetrievalAdapter::new(pool.clone());
    let mut hits: Vec<EvidenceHit> = Vec::new();
    for query in &queries {
        // Empty `product_version_ids` means "all current eligible product
        // versions" per the v2 contract — no project->version mapping needed
        // for the minimal wiring.
        let scope = KnowledgeEvidenceScopeV2::ProductLine(ProductEvidenceRequestV1 {
            schema_version: 1,
            requirement_identity_sha256: sha256_hex(&query.slot_id),
            requirement_text: query.query.clone(),
            product_version_ids: Vec::new(),
            retrieval_policy: policy.clone(),
        });
        let batch = adapter
            .retrieve_evidence_v3(scope)
            .await
            .map_err(|error| error.to_string())?;
        hits.extend(batch.hits.into_iter().map(|hit| EvidenceHit {
            evidence_id: hit.source_chunk_id.to_string(),
            document_id: hit.document_id.to_string(),
            slot_id: query.slot_id.clone(),
            text: hit.chunk_utf8,
        }));
    }
    let response_set = response::respond(&artifact, &hits)?;
    response::store::publish(pool, job.project_id, &response_set)
        .await
        .map_err(|error| error.to_string())?;
    tracing::info!(
        project_id = %job.project_id,
        workspace_id = %job.workspace_id,
        outline_sha256 = %response_set.outline_sha256,
        response_count = response_set.responses.len(),
        "content_generate published response set"
    );
    Ok(())
}

/// Queue-facing tender worker: owns pool + shutdown, delegates to [`run_tender_document_process`].
pub struct TenderDocumentProcessV2Worker {
    pool: Option<PgPool>,
    shutdown: CancellationToken,
}

impl TenderDocumentProcessV2Worker {
    pub fn new(pool: Option<PgPool>, shutdown: CancellationToken) -> Self {
        Self { pool, shutdown }
    }

    pub async fn run(&self, job: &TenderDocumentProcessJobV2) -> Result<(), String> {
        let Some(pool) = self.pool.clone() else {
            return Err("postgres not configured".into());
        };
        run_tender_document_process(&pool, job, &self.shutdown).await
    }
}

/// Queue-facing content worker: owns pool, delegates to [`run_content_generate`].
pub struct ContentGenerateV2Worker {
    pool: Option<PgPool>,
}

impl ContentGenerateV2Worker {
    pub fn new(pool: Option<PgPool>) -> Self {
        Self { pool }
    }

    pub async fn run(&self, job: &ContentGenerateJobV2) -> Result<(), String> {
        let Some(pool) = self.pool.clone() else {
            return Err("postgres not configured".into());
        };
        run_content_generate(&pool, job).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use platform::BidAuthoringRequestIdentityV2;

    fn tender_job(revision: i64) -> TenderDocumentProcessJobV2 {
        TenderDocumentProcessJobV2 {
            request: BidAuthoringRequestIdentityV2 {
                request_artifact_id: Uuid::from_u128(0x1111),
                request_revision: revision,
                frozen_input_sha256: "a".repeat(64),
            },
            project_id: Uuid::from_u128(0x2222),
            document_revision_id: Uuid::from_u128(0x3333),
        }
    }

    #[test]
    fn tender_run_id_is_deterministic_per_request_identity() {
        let a = tender_run_id(&tender_job(7));
        let b = tender_run_id(&tender_job(7));
        assert_eq!(a, b);
        assert_eq!(a.get_version_num(), 4);
    }

    #[test]
    fn tender_run_id_differs_across_revisions() {
        assert_ne!(tender_run_id(&tender_job(7)), tender_run_id(&tender_job(8)));
    }

    #[tokio::test]
    async fn load_frozen_input_rejects_bad_sha() {
        let error = load_frozen_input("not-a-sha").await.unwrap_err();
        assert!(error.starts_with("FROZEN_INPUT_SHA_INVALID"), "{error}");
    }

    #[tokio::test]
    async fn load_frozen_input_reports_missing_blob() {
        let error = load_frozen_input(&"b".repeat(64)).await.unwrap_err();
        assert!(error.starts_with("FROZEN_INPUT_MISSING"), "{error}");
    }
}
