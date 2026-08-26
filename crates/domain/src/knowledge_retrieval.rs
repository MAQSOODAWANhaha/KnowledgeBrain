//! Cross-domain evidence retrieval seam.
//!
//! Requests contain only knowledge-base concepts.  Every hit is a complete
//! point-in-time snapshot so consumers never need to reread a live document or
//! chunk after this interface returns.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
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
#[serde(deny_unknown_fields)]
pub struct EligibleEvidenceVersionV1 {
    pub product_id: Uuid,
    pub product_version_id: Uuid,
    pub workspace_kind: String,
    pub frozen_display_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeEvidenceBatchV1 {
    pub schema_version: u16,
    pub eligible_versions: Vec<EligibleEvidenceVersionV1>,
    pub hits: Vec<KnowledgeEvidenceHitV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProductEvidenceHitV1(pub KnowledgeEvidenceHitV1);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CompanyEvidenceHitV1(pub KnowledgeEvidenceHitV1);

/// Validate the complete point-in-time snapshots returned by a retrieval port
/// before a consumer freezes them into its own domain. This check deliberately
/// lives on the port contract so every adapter, including test and remote
/// implementations, is held to the same byte and quota invariants.
pub fn validate_evidence_hit_batch(
    expected_workspace_kind: &str,
    hits: &[KnowledgeEvidenceHitV1],
    policy: &RetrievalPolicyIdentityV1,
) -> Result<(), KnowledgeRetrievalError> {
    if !matches!(expected_workspace_kind, "product_line" | "company")
        || policy.contract_version.is_empty()
        || policy.max_hits == 0
        || policy.max_chunk_bytes == 0
        || policy.max_total_bytes == 0
        || hits.len() > policy.max_hits as usize
    {
        return Err(KnowledgeRetrievalError::InvalidHit(
            "invalid expected scope or returned-hit quota".into(),
        ));
    }

    let mut total_bytes = 0u64;
    let mut identities = HashSet::new();
    for (index, hit) in hits.iter().enumerate() {
        let chunk = hit.chunk_utf8.as_bytes();
        let start = usize::try_from(hit.quote_start_offset).map_err(|_| {
            KnowledgeRetrievalError::InvalidHit("quote start offset overflow".into())
        })?;
        let end = usize::try_from(hit.quote_end_offset)
            .map_err(|_| KnowledgeRetrievalError::InvalidHit("quote end offset overflow".into()))?;
        total_bytes = total_bytes
            .checked_add(chunk.len() as u64)
            .ok_or_else(|| KnowledgeRetrievalError::InvalidHit("hit byte quota overflow".into()))?;

        if hit.schema_version != KNOWLEDGE_EVIDENCE_SCHEMA_V1
            || hit.workspace_kind != expected_workspace_kind
            || hit.document_id.is_nil()
            || hit.source_chunk_id.is_nil()
            || hit.product_id.is_nil()
            || hit.product_version_id.is_nil()
            || hit.frozen_document_display_name.is_empty()
            || hit.retrieval_contract_version != policy.contract_version
            || hit.offset_unit != UTF8_BYTE_OFFSET_UNIT
            || hit.chunk_byte_length != chunk.len() as u64
            || hit.chunk_byte_length > u64::from(policy.max_chunk_bytes)
            || hit.chunk_sha256 != hex::encode(Sha256::digest(chunk))
            || start >= end
            || end > chunk.len()
            || !hit.chunk_utf8.is_char_boundary(start)
            || !hit.chunk_utf8.is_char_boundary(end)
            || hit.retrieval_rank != index as u32 + 1
            || !is_decimal_literal(&hit.retrieval_raw_score)
            || !identities.insert((
                hit.document_id,
                hit.source_chunk_id,
                hit.quote_start_offset,
                hit.quote_end_offset,
            ))
        {
            return Err(KnowledgeRetrievalError::InvalidHit(
                "evidence scope, bytes, digest, offset, rank, or identity is invalid".into(),
            ));
        }
    }
    if total_bytes > policy.max_total_bytes {
        return Err(KnowledgeRetrievalError::InvalidHit(
            "returned-hit byte quota exceeded".into(),
        ));
    }
    Ok(())
}

pub fn validate_evidence_batch(
    expected_workspace_kind: &str,
    batch: &KnowledgeEvidenceBatchV1,
    policy: &RetrievalPolicyIdentityV1,
) -> Result<(), KnowledgeRetrievalError> {
    if batch.schema_version != KNOWLEDGE_EVIDENCE_SCHEMA_V1 {
        return Err(KnowledgeRetrievalError::InvalidHit(
            "invalid evidence batch schema".into(),
        ));
    }
    let mut eligible = HashSet::new();
    for version in &batch.eligible_versions {
        if version.product_id.is_nil()
            || version.product_version_id.is_nil()
            || version.workspace_kind != expected_workspace_kind
            || version.frozen_display_name.is_empty()
            || !eligible.insert((version.product_id, version.product_version_id))
        {
            return Err(KnowledgeRetrievalError::InvalidHit(
                "invalid or duplicate eligible evidence version".into(),
            ));
        }
    }
    validate_evidence_hit_batch(expected_workspace_kind, &batch.hits, policy)?;
    if batch
        .hits
        .iter()
        .any(|hit| !eligible.contains(&(hit.product_id, hit.product_version_id)))
    {
        return Err(KnowledgeRetrievalError::InvalidHit(
            "evidence hit is outside the eligible version scope".into(),
        ));
    }
    Ok(())
}

fn is_decimal_literal(value: &str) -> bool {
    let value = value.strip_prefix('-').unwrap_or(value);
    let mut parts = value.split('.');
    let integer = parts.next().unwrap_or_default();
    let fraction = parts.next();
    !integer.is_empty()
        && integer.bytes().all(|byte| byte.is_ascii_digit())
        && fraction
            .is_none_or(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
        && parts.next().is_none()
}

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
    ) -> Result<KnowledgeEvidenceBatchV1, KnowledgeRetrievalError>;

    async fn retrieve_company_evidence(
        &self,
        request: CompanyEvidenceRequestV1,
    ) -> Result<KnowledgeEvidenceBatchV1, KnowledgeRetrievalError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    fn hit(chunk: &str) -> KnowledgeEvidenceHitV1 {
        KnowledgeEvidenceHitV1 {
            schema_version: KNOWLEDGE_EVIDENCE_SCHEMA_V1,
            document_id: Uuid::from_u128(1),
            source_chunk_id: Uuid::from_u128(2),
            product_id: Uuid::from_u128(3),
            product_version_id: Uuid::from_u128(4),
            workspace_kind: "product_line".into(),
            frozen_document_display_name: "manual.pdf".into(),
            chunk_utf8: chunk.into(),
            chunk_sha256: hex::encode(Sha256::digest(chunk.as_bytes())),
            chunk_byte_length: chunk.len() as u64,
            quote_start_offset: 3,
            quote_end_offset: 4,
            offset_unit: UTF8_BYTE_OFFSET_UNIT.into(),
            retrieval_rank: 1,
            retrieval_raw_score: "0.500000".into(),
            retrieval_contract_version: "knowledge-evidence-v1".into(),
        }
    }

    fn policy() -> RetrievalPolicyIdentityV1 {
        RetrievalPolicyIdentityV1 {
            contract_version: "knowledge-evidence-v1".into(),
            policy_sha256: "a".repeat(64),
            max_hits: 2,
            max_chunk_bytes: 1024,
            max_total_bytes: 2048,
        }
    }

    #[test]
    fn returned_hit_batch_validates_complete_snapshot_before_freeze() {
        let value = hit("中A文");
        validate_evidence_hit_batch("product_line", &[value], &policy()).unwrap();
    }

    #[test]
    fn returned_hit_batch_rejects_wrong_route_invalid_bytes_and_noncanonical_rank() {
        let value = hit("中A文");
        assert!(
            validate_evidence_hit_batch("company", std::slice::from_ref(&value), &policy())
                .is_err()
        );

        let mut invalid_offset = value.clone();
        invalid_offset.quote_start_offset = 1;
        assert!(validate_evidence_hit_batch("product_line", &[invalid_offset], &policy()).is_err());

        let mut invalid_rank = value;
        invalid_rank.retrieval_rank = 2;
        assert!(validate_evidence_hit_batch("product_line", &[invalid_rank], &policy()).is_err());
    }

    #[test]
    fn eligible_scope_is_independent_from_the_hit_quota_and_allows_no_evidence() {
        let batch = KnowledgeEvidenceBatchV1 {
            schema_version: KNOWLEDGE_EVIDENCE_SCHEMA_V1,
            eligible_versions: (1..=65)
                .map(|value| EligibleEvidenceVersionV1 {
                    product_id: Uuid::from_u128(value),
                    product_version_id: Uuid::from_u128(value + 100),
                    workspace_kind: "product_line".into(),
                    frozen_display_name: format!("v{value}"),
                })
                .collect(),
            hits: Vec::new(),
        };
        validate_evidence_batch("product_line", &batch, &policy()).unwrap();
    }

    #[test]
    fn evidence_hit_must_belong_to_the_frozen_eligible_scope() {
        let batch = KnowledgeEvidenceBatchV1 {
            schema_version: KNOWLEDGE_EVIDENCE_SCHEMA_V1,
            eligible_versions: Vec::new(),
            hits: vec![hit("中A文")],
        };
        assert!(validate_evidence_batch("product_line", &batch, &policy()).is_err());
    }
}
