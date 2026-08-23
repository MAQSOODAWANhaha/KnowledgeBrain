//! Cross-domain evidence retrieval seam.
//!
//! Requests contain only knowledge-base concepts.  Every hit is a complete
//! point-in-time snapshot so consumers never need to reread a live document or
//! chunk after this interface returns.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const KNOWLEDGE_EVIDENCE_SCHEMA_V1: u16 = 1;
pub const UTF8_BYTE_OFFSET_UNIT: &str = "utf8_byte";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalPolicyIdentityV1 {
    pub contract_version: String,
    pub policy_sha256: String,
    pub max_hits: u32,
    pub max_chunk_bytes: u32,
    pub max_total_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductEvidenceRequestV1 {
    pub schema_version: u16,
    pub requirement_identity_sha256: String,
    pub requirement_text: String,
    /// Empty means all current eligible product versions. A non-empty list is
    /// an exact, already-frozen version selection.
    pub product_version_ids: Vec<Uuid>,
    pub retrieval_policy: RetrievalPolicyIdentityV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyEvidenceRequestV1 {
    pub schema_version: u16,
    pub requirement_identity_sha256: String,
    pub requirement_text: String,
    /// Empty means all current eligible company-library versions.
    pub library_version_ids: Vec<Uuid>,
    pub retrieval_policy: RetrievalPolicyIdentityV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeEvidenceHitV1 {
    pub schema_version: u16,
    pub document_id: Uuid,
    pub source_chunk_id: Uuid,
    pub product_id: Uuid,
    pub product_version_id: Uuid,
    pub workspace_kind: String,
    pub frozen_document_display_name: String,
    pub chunk_utf8: String,
    pub chunk_sha256: String,
    pub chunk_byte_length: u64,
    pub quote_start_offset: u64,
    pub quote_end_offset: u64,
    pub offset_unit: String,
    pub retrieval_rank: u32,
    /// An exact decimal string. Matching canonicalizes it to fixed scale.
    pub retrieval_raw_score: String,
    pub retrieval_contract_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProductEvidenceHitV1(pub KnowledgeEvidenceHitV1);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CompanyEvidenceHitV1(pub KnowledgeEvidenceHitV1);

#[derive(Debug, thiserror::Error)]
pub enum KnowledgeRetrievalError {
    #[error("invalid evidence request: {0}")]
    InvalidRequest(String),
    #[error("knowledge retrieval unavailable: {0}")]
    Unavailable(String),
    #[error("invalid evidence hit: {0}")]
    InvalidHit(String),
}

#[async_trait]
pub trait KnowledgeRetrievalPort: Send + Sync {
    async fn retrieve_product_evidence(
        &self,
        request: ProductEvidenceRequestV1,
    ) -> Result<Vec<ProductEvidenceHitV1>, KnowledgeRetrievalError>;

    async fn retrieve_company_evidence(
        &self,
        request: CompanyEvidenceRequestV1,
    ) -> Result<Vec<CompanyEvidenceHitV1>, KnowledgeRetrievalError>;
}
