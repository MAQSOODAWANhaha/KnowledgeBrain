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
pub const KNOWLEDGE_EVIDENCE_SCHEMA_V2: u16 = 2;
pub const KNOWLEDGE_EVIDENCE_CONTRACT_V1: &str = "knowledge-evidence-v1";
pub const KNOWLEDGE_EVIDENCE_CONTRACT_V2: &str = "knowledge-evidence-v2";
pub const RETRIEVAL_POLICY_SCHEMA_V2: u16 = 2;
pub const UTF8_BYTE_OFFSET_UNIT: &str = "utf8_byte";

const RETRIEVAL_POLICY_MAX_TOP_K: u32 = 1_000_000;
const RETRIEVAL_POLICY_MAX_TIMEOUT_MS: u32 = 3_600_000;
const RETRIEVAL_POLICY_MAX_HITS: u32 = 1_000_000;
const RETRIEVAL_POLICY_MAX_CHUNK_BYTES: u32 = 1_073_741_824;
const RETRIEVAL_POLICY_MAX_TOTAL_BYTES: u64 = 1_099_511_627_776;
const RETRIEVAL_POLICY_MAX_RRF_K: u32 = 1_000_000;
const RETRIEVAL_POLICY_MAX_WEIGHT_MILLIONTHS: u32 = 1_000_000_000;
const MILLIONTHS_ONE: u32 = 1_000_000;
pub const RETRIEVAL_NORMALIZATION_VERSION_V2: &str = "unicode-whitespace-lowercase-v1";
pub const RETRIEVAL_TRUSTED_SOURCE_TYPES_V2: [&str; 3] = ["text", "parent_text", "image_ocr"];
pub const RETRIEVAL_A_PRIMARY_COMPARATOR_V2: [&str; 3] = [
    "chunk_byte_length ASC",
    "document_id ASC",
    "source_chunk_id ASC",
];
pub const RETRIEVAL_A_VERSION_COMPARATOR_V2: [&str; 2] =
    ["product_id ASC", "product_version_id ASC"];
pub const RETRIEVAL_B_EXACT_COMPARATOR_V2: [&str; 5] = [
    "product_id ASC",
    "product_version_id ASC",
    "chunk_byte_length ASC",
    "document_id ASC",
    "source_chunk_id ASC",
];
pub const RETRIEVAL_C_SEMANTIC_COMPARATOR_V2: [&str; 3] = [
    "normalized_rerank_score DESC",
    "pre_rerank_rrf_rank ASC",
    "complete_source_identity ASC",
];
pub const RETRIEVAL_QUOTA_SEMANTICS_VERSION_V2: &str = "fair-exact-prefix-fail-closed-v1";
pub const RETRIEVAL_KEYWORD_TOKENIZER_V2: &str = "latin-numeric-cjk-bigram";
pub const RETRIEVAL_KEYWORD_TOKENIZER_VERSION_V2: &str = "v1";
pub const RETRIEVAL_EMBEDDING_POLICY_V2: &str = "declared-version-model";
pub const RETRIEVAL_EMBEDDING_POLICY_VERSION_V2: &str = "v1";
pub const RETRIEVAL_RERANK_PROTOCOL_VERSION_V2: &str = "indexed-json-v1";
pub const RETRIEVAL_RERANK_SCORE_NORMALIZATION_VERSION_V2: &str = "unit-interval-millionths-v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalPolicyIdentityV1 {
    pub contract_version: String,
    pub policy_sha256: String,
    pub max_hits: u32,
    pub max_chunk_bytes: u32,
    pub max_total_bytes: u64,
}

/// Canonical A/B/C ordering and quota semantics for v2 retrieval.
///
/// Comparator vectors are serialized in precedence order. Changing a field or
/// moving a comparator therefore creates a different policy identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalRankingPolicyV2 {
    pub a_primary_comparator: Vec<String>,
    pub a_version_comparator: Vec<String>,
    pub b_exact_comparator: Vec<String>,
    pub c_semantic_comparator: Vec<String>,
    pub quota_semantics_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalKeywordPolicyV2 {
    pub tokenizer: String,
    pub tokenizer_version: String,
    pub top_k: u32,
    /// Minimum accepted channel score in millionths (1_000_000 = 1.0).
    pub threshold_millionths: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalEmbeddingPolicyV2 {
    pub policy: String,
    pub policy_version: String,
    pub model_revision_sha256: String,
    pub top_k: u32,
    /// Minimum accepted channel score in millionths (1_000_000 = 1.0).
    pub threshold_millionths: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalRrfPolicyV2 {
    pub k: u32,
    /// Positive RRF channel coefficients in millionths.
    pub keyword_weight_millionths: u32,
    pub vector_weight_millionths: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalRerankPolicyV2 {
    pub provider_protocol_version: String,
    pub model_revision_sha256: String,
    pub config_revision_sha256: String,
    pub top_k: u32,
    pub timeout_ms: u32,
    pub score_normalization_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalRequestQuotasV2 {
    pub max_hits: u32,
    pub max_chunk_bytes: u32,
    pub max_total_bytes: u64,
}

/// Versioned immutable identity artifact for `knowledge-evidence-v2`.
///
/// The schema intentionally has no endpoint or secret fields. Model and
/// configuration identities are lowercase SHA-256 revisions rather than
/// mutable aliases. Struct field order and vector order are canonical JSON
/// order; maps and floating-point values are intentionally absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalPolicyV2 {
    pub schema_version: u16,
    pub contract_version: String,
    pub normalization_version: String,
    /// Canonical order: `text`, `parent_text`, `image_ocr`.
    pub trusted_source_types: Vec<String>,
    pub ranking: RetrievalRankingPolicyV2,
    pub keyword: RetrievalKeywordPolicyV2,
    pub embedding: RetrievalEmbeddingPolicyV2,
    pub rrf: RetrievalRrfPolicyV2,
    pub rerank: RetrievalRerankPolicyV2,
    pub request_quotas: RetrievalRequestQuotasV2,
}

#[derive(Debug, thiserror::Error)]
pub enum RetrievalPolicyV2Error {
    #[error("invalid retrieval policy v2: {0}")]
    Invalid(String),
    #[error("failed to serialize retrieval policy v2: {0}")]
    Serialization(#[from] serde_json::Error),
}

impl RetrievalPolicyV2 {
    pub fn validate(&self) -> Result<(), RetrievalPolicyV2Error> {
        if self.schema_version != RETRIEVAL_POLICY_SCHEMA_V2 {
            return Err(invalid_policy("schema_version must be 2"));
        }
        if self.contract_version != KNOWLEDGE_EVIDENCE_CONTRACT_V2 {
            return Err(invalid_policy(
                "contract_version must be knowledge-evidence-v2",
            ));
        }
        validate_supported_identity(
            "normalization_version",
            &self.normalization_version,
            RETRIEVAL_NORMALIZATION_VERSION_V2,
        )?;
        validate_supported_values(
            "trusted_source_types",
            &self.trusted_source_types,
            &RETRIEVAL_TRUSTED_SOURCE_TYPES_V2,
        )?;
        validate_supported_values(
            "ranking.a_primary_comparator",
            &self.ranking.a_primary_comparator,
            &RETRIEVAL_A_PRIMARY_COMPARATOR_V2,
        )?;
        validate_supported_values(
            "ranking.a_version_comparator",
            &self.ranking.a_version_comparator,
            &RETRIEVAL_A_VERSION_COMPARATOR_V2,
        )?;
        validate_supported_values(
            "ranking.b_exact_comparator",
            &self.ranking.b_exact_comparator,
            &RETRIEVAL_B_EXACT_COMPARATOR_V2,
        )?;
        validate_supported_values(
            "ranking.c_semantic_comparator",
            &self.ranking.c_semantic_comparator,
            &RETRIEVAL_C_SEMANTIC_COMPARATOR_V2,
        )?;
        validate_supported_identity(
            "ranking.quota_semantics_version",
            &self.ranking.quota_semantics_version,
            RETRIEVAL_QUOTA_SEMANTICS_VERSION_V2,
        )?;

        validate_supported_identity(
            "keyword.tokenizer",
            &self.keyword.tokenizer,
            RETRIEVAL_KEYWORD_TOKENIZER_V2,
        )?;
        validate_supported_identity(
            "keyword.tokenizer_version",
            &self.keyword.tokenizer_version,
            RETRIEVAL_KEYWORD_TOKENIZER_VERSION_V2,
        )?;
        validate_top_k("keyword.top_k", self.keyword.top_k)?;
        validate_threshold(
            "keyword.threshold_millionths",
            self.keyword.threshold_millionths,
        )?;

        validate_supported_identity(
            "embedding.policy",
            &self.embedding.policy,
            RETRIEVAL_EMBEDDING_POLICY_V2,
        )?;
        validate_supported_identity(
            "embedding.policy_version",
            &self.embedding.policy_version,
            RETRIEVAL_EMBEDDING_POLICY_VERSION_V2,
        )?;
        validate_sha256(
            "embedding.model_revision_sha256",
            &self.embedding.model_revision_sha256,
        )?;
        validate_top_k("embedding.top_k", self.embedding.top_k)?;
        validate_threshold(
            "embedding.threshold_millionths",
            self.embedding.threshold_millionths,
        )?;

        if self.rrf.k == 0 || self.rrf.k > RETRIEVAL_POLICY_MAX_RRF_K {
            return Err(invalid_policy("rrf.k is outside the supported range"));
        }
        validate_weight(
            "rrf.keyword_weight_millionths",
            self.rrf.keyword_weight_millionths,
        )?;
        validate_weight(
            "rrf.vector_weight_millionths",
            self.rrf.vector_weight_millionths,
        )?;

        validate_supported_identity(
            "rerank.provider_protocol_version",
            &self.rerank.provider_protocol_version,
            RETRIEVAL_RERANK_PROTOCOL_VERSION_V2,
        )?;
        validate_sha256(
            "rerank.model_revision_sha256",
            &self.rerank.model_revision_sha256,
        )?;
        validate_sha256(
            "rerank.config_revision_sha256",
            &self.rerank.config_revision_sha256,
        )?;
        validate_top_k("rerank.top_k", self.rerank.top_k)?;
        if self.rerank.timeout_ms == 0 || self.rerank.timeout_ms > RETRIEVAL_POLICY_MAX_TIMEOUT_MS {
            return Err(invalid_policy(
                "rerank.timeout_ms is outside the supported range",
            ));
        }
        validate_supported_identity(
            "rerank.score_normalization_version",
            &self.rerank.score_normalization_version,
            RETRIEVAL_RERANK_SCORE_NORMALIZATION_VERSION_V2,
        )?;

        if self.request_quotas.max_hits == 0
            || self.request_quotas.max_hits > RETRIEVAL_POLICY_MAX_HITS
            || self.request_quotas.max_chunk_bytes == 0
            || self.request_quotas.max_chunk_bytes > RETRIEVAL_POLICY_MAX_CHUNK_BYTES
            || self.request_quotas.max_total_bytes == 0
            || self.request_quotas.max_total_bytes > RETRIEVAL_POLICY_MAX_TOTAL_BYTES
        {
            return Err(invalid_policy(
                "request quotas are outside the supported range",
            ));
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, RetrievalPolicyV2Error> {
        self.validate()?;
        Ok(serde_json::to_vec(self)?)
    }

    pub fn sha256(&self) -> Result<String, RetrievalPolicyV2Error> {
        Ok(hex::encode(Sha256::digest(self.canonical_bytes()?)))
    }

    /// Derives the existing request DTO identity without changing its shape.
    pub fn request_identity(&self) -> Result<RetrievalPolicyIdentityV1, RetrievalPolicyV2Error> {
        Ok(RetrievalPolicyIdentityV1 {
            contract_version: self.contract_version.clone(),
            policy_sha256: self.sha256()?,
            max_hits: self.request_quotas.max_hits,
            max_chunk_bytes: self.request_quotas.max_chunk_bytes,
            max_total_bytes: self.request_quotas.max_total_bytes,
        })
    }
}

fn invalid_policy(message: impl Into<String>) -> RetrievalPolicyV2Error {
    RetrievalPolicyV2Error::Invalid(message.into())
}

fn validate_supported_identity(
    label: &str,
    value: &str,
    supported: &str,
) -> Result<(), RetrievalPolicyV2Error> {
    if value != supported {
        return Err(invalid_policy(format!(
            "{label} must use the supported identity {supported}"
        )));
    }
    Ok(())
}

fn validate_supported_values<const N: usize>(
    label: &str,
    values: &[String],
    supported: &[&str; N],
) -> Result<(), RetrievalPolicyV2Error> {
    if values.len() != N
        || values
            .iter()
            .map(String::as_str)
            .ne(supported.iter().copied())
    {
        return Err(invalid_policy(format!(
            "{label} must use the supported canonical values in order"
        )));
    }
    Ok(())
}

fn validate_sha256(label: &str, value: &str) -> Result<(), RetrievalPolicyV2Error> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(invalid_policy(format!(
            "{label} must be a lowercase 64-hex digest"
        )));
    }
    Ok(())
}

fn validate_top_k(label: &str, value: u32) -> Result<(), RetrievalPolicyV2Error> {
    if value == 0 || value > RETRIEVAL_POLICY_MAX_TOP_K {
        return Err(invalid_policy(format!(
            "{label} is outside the supported range"
        )));
    }
    Ok(())
}

fn validate_threshold(label: &str, value: u32) -> Result<(), RetrievalPolicyV2Error> {
    if value > MILLIONTHS_ONE {
        return Err(invalid_policy(format!(
            "{label} must be between 0 and 1_000_000"
        )));
    }
    Ok(())
}

fn validate_weight(label: &str, value: u32) -> Result<(), RetrievalPolicyV2Error> {
    if value == 0 || value > RETRIEVAL_POLICY_MAX_WEIGHT_MILLIONTHS {
        return Err(invalid_policy(format!(
            "{label} is outside the supported range"
        )));
    }
    Ok(())
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

/// Closed set of source snapshots trusted by the v2 exact-evidence contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeSourceTypeV2 {
    Text,
    ParentText,
    ImageOcr,
}

/// Fresh schema-2 snapshot. Its fields intentionally mirror, rather than
/// flatten or embed, v1 so either schema can evolve only through an explicit
/// contract change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeEvidenceHitV2 {
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
    pub retrieval_raw_score: String,
    pub retrieval_contract_version: String,
    pub source_type: KnowledgeSourceTypeV2,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeEvidenceBatchV2 {
    pub schema_version: u16,
    pub eligible_versions: Vec<EligibleEvidenceVersionV1>,
    pub hits: Vec<KnowledgeEvidenceHitV2>,
    pub exact_versions_truncated: u64,
    pub exact_hits_truncated: u64,
    pub semantic_hits_truncated: u64,
}

/// Explicitly tagged scope keeps product-line and company requests distinct on
/// the single deep v2 shadow seam.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "scope_type", content = "request", rename_all = "snake_case")]
pub enum KnowledgeEvidenceScopeV2 {
    ProductLine(ProductEvidenceRequestV1),
    Company(CompanyEvidenceRequestV1),
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

/// Validates the isolated schema-2 exact-prefix snapshot before a shadow
/// consumer observes it.
pub fn validate_evidence_batch_v2(
    expected_workspace_kind: &str,
    batch: &KnowledgeEvidenceBatchV2,
    policy: &RetrievalPolicyIdentityV1,
) -> Result<(), KnowledgeRetrievalError> {
    if batch.schema_version != KNOWLEDGE_EVIDENCE_SCHEMA_V2
        || policy.contract_version != KNOWLEDGE_EVIDENCE_CONTRACT_V2
        || !matches!(expected_workspace_kind, "product_line" | "company")
        || policy.max_hits == 0
        || policy.max_chunk_bytes == 0
        || policy.max_total_bytes == 0
        || batch.hits.len() > policy.max_hits as usize
        || batch.semantic_hits_truncated != 0
    {
        return Err(KnowledgeRetrievalError::InvalidHit(
            "invalid v2 evidence schema, contract, scope, quota, or metric".into(),
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
                "invalid or duplicate eligible v2 evidence version".into(),
            ));
        }
    }

    let mut identities = HashSet::new();
    let mut total_bytes = 0u64;
    for (index, hit) in batch.hits.iter().enumerate() {
        let chunk = hit.chunk_utf8.as_bytes();
        total_bytes = total_bytes.checked_add(chunk.len() as u64).ok_or_else(|| {
            KnowledgeRetrievalError::InvalidHit("v2 hit byte quota overflow".into())
        })?;
        if hit.schema_version != KNOWLEDGE_EVIDENCE_SCHEMA_V2
            || hit.workspace_kind != expected_workspace_kind
            || hit.document_id.is_nil()
            || hit.source_chunk_id.is_nil()
            || hit.product_id.is_nil()
            || hit.product_version_id.is_nil()
            || hit.frozen_document_display_name.is_empty()
            || hit.retrieval_contract_version != KNOWLEDGE_EVIDENCE_CONTRACT_V2
            || hit.offset_unit != UTF8_BYTE_OFFSET_UNIT
            || hit.chunk_byte_length != chunk.len() as u64
            || hit.chunk_byte_length > u64::from(policy.max_chunk_bytes)
            || hit.chunk_sha256 != hex::encode(Sha256::digest(chunk))
            || hit.quote_start_offset != 0
            || hit.quote_end_offset != chunk.len() as u64
            || !hit.chunk_utf8.is_char_boundary(chunk.len())
            || hit.retrieval_rank != index as u32 + 1
            || hit.retrieval_raw_score != "1.000000"
            || !eligible.contains(&(hit.product_id, hit.product_version_id))
            || !identities.insert((hit.document_id, hit.source_chunk_id))
        {
            return Err(KnowledgeRetrievalError::InvalidHit(
                "v2 evidence scope, source, bytes, digest, offset, rank, score, or identity is invalid"
                    .into(),
            ));
        }
    }
    if total_bytes > policy.max_total_bytes {
        return Err(KnowledgeRetrievalError::InvalidHit(
            "returned v2 hit byte quota exceeded".into(),
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
    #[error("knowledge retrieval quota exceeded: {0}")]
    QuotaExceeded(String),
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

/// Shadow/test-only schema-2 seam. It is deliberately separate from the v1
/// port so production callers cannot accidentally dispatch or fall back.
#[async_trait]
pub trait KnowledgeRetrievalPortV2: Send + Sync {
    async fn retrieve_evidence_v2(
        &self,
        scope: KnowledgeEvidenceScopeV2,
    ) -> Result<KnowledgeEvidenceBatchV2, KnowledgeRetrievalError>;
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
            retrieval_contract_version: KNOWLEDGE_EVIDENCE_CONTRACT_V1.into(),
        }
    }

    fn policy() -> RetrievalPolicyIdentityV1 {
        RetrievalPolicyIdentityV1 {
            contract_version: KNOWLEDGE_EVIDENCE_CONTRACT_V1.into(),
            policy_sha256: "a".repeat(64),
            max_hits: 2,
            max_chunk_bytes: 1024,
            max_total_bytes: 2048,
        }
    }

    fn policy_v2() -> RetrievalPolicyV2 {
        RetrievalPolicyV2 {
            schema_version: RETRIEVAL_POLICY_SCHEMA_V2,
            contract_version: KNOWLEDGE_EVIDENCE_CONTRACT_V2.into(),
            normalization_version: RETRIEVAL_NORMALIZATION_VERSION_V2.into(),
            trusted_source_types: RETRIEVAL_TRUSTED_SOURCE_TYPES_V2
                .iter()
                .map(|value| (*value).into())
                .collect(),
            ranking: RetrievalRankingPolicyV2 {
                a_primary_comparator: RETRIEVAL_A_PRIMARY_COMPARATOR_V2
                    .iter()
                    .map(|value| (*value).into())
                    .collect(),
                a_version_comparator: RETRIEVAL_A_VERSION_COMPARATOR_V2
                    .iter()
                    .map(|value| (*value).into())
                    .collect(),
                b_exact_comparator: RETRIEVAL_B_EXACT_COMPARATOR_V2
                    .iter()
                    .map(|value| (*value).into())
                    .collect(),
                c_semantic_comparator: RETRIEVAL_C_SEMANTIC_COMPARATOR_V2
                    .iter()
                    .map(|value| (*value).into())
                    .collect(),
                quota_semantics_version: RETRIEVAL_QUOTA_SEMANTICS_VERSION_V2.into(),
            },
            keyword: RetrievalKeywordPolicyV2 {
                tokenizer: RETRIEVAL_KEYWORD_TOKENIZER_V2.into(),
                tokenizer_version: RETRIEVAL_KEYWORD_TOKENIZER_VERSION_V2.into(),
                top_k: 128,
                threshold_millionths: 50_000,
            },
            embedding: RetrievalEmbeddingPolicyV2 {
                policy: RETRIEVAL_EMBEDDING_POLICY_V2.into(),
                policy_version: RETRIEVAL_EMBEDDING_POLICY_VERSION_V2.into(),
                model_revision_sha256: "1".repeat(64),
                top_k: 128,
                threshold_millionths: 100_000,
            },
            rrf: RetrievalRrfPolicyV2 {
                k: 60,
                keyword_weight_millionths: 1_000_000,
                vector_weight_millionths: 1_000_000,
            },
            rerank: RetrievalRerankPolicyV2 {
                provider_protocol_version: RETRIEVAL_RERANK_PROTOCOL_VERSION_V2.into(),
                model_revision_sha256: "2".repeat(64),
                config_revision_sha256: "a".repeat(64),
                top_k: 64,
                timeout_ms: 5_000,
                score_normalization_version: RETRIEVAL_RERANK_SCORE_NORMALIZATION_VERSION_V2.into(),
            },
            request_quotas: RetrievalRequestQuotasV2 {
                max_hits: 64,
                max_chunk_bytes: 256 * 1024,
                max_total_bytes: 8 * 1024 * 1024,
            },
        }
    }

    #[test]
    fn policy_v2_equal_artifacts_have_identical_canonical_bytes_and_digest() {
        let first = policy_v2();
        let second = first.clone();

        assert_eq!(
            first.canonical_bytes().unwrap(),
            second.canonical_bytes().unwrap()
        );
        assert_eq!(first.sha256().unwrap(), second.sha256().unwrap());
    }

    #[test]
    fn policy_v2_canonical_bytes_and_digest_are_golden() {
        // Intentional schema or serialization changes require a deliberate golden update.
        let expected = r#"{"schema_version":2,"contract_version":"knowledge-evidence-v2","normalization_version":"unicode-whitespace-lowercase-v1","trusted_source_types":["text","parent_text","image_ocr"],"ranking":{"a_primary_comparator":["chunk_byte_length ASC","document_id ASC","source_chunk_id ASC"],"a_version_comparator":["product_id ASC","product_version_id ASC"],"b_exact_comparator":["product_id ASC","product_version_id ASC","chunk_byte_length ASC","document_id ASC","source_chunk_id ASC"],"c_semantic_comparator":["normalized_rerank_score DESC","pre_rerank_rrf_rank ASC","complete_source_identity ASC"],"quota_semantics_version":"fair-exact-prefix-fail-closed-v1"},"keyword":{"tokenizer":"latin-numeric-cjk-bigram","tokenizer_version":"v1","top_k":128,"threshold_millionths":50000},"embedding":{"policy":"declared-version-model","policy_version":"v1","model_revision_sha256":"1111111111111111111111111111111111111111111111111111111111111111","top_k":128,"threshold_millionths":100000},"rrf":{"k":60,"keyword_weight_millionths":1000000,"vector_weight_millionths":1000000},"rerank":{"provider_protocol_version":"indexed-json-v1","model_revision_sha256":"2222222222222222222222222222222222222222222222222222222222222222","config_revision_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","top_k":64,"timeout_ms":5000,"score_normalization_version":"unit-interval-millionths-v1"},"request_quotas":{"max_hits":64,"max_chunk_bytes":262144,"max_total_bytes":8388608}}"#;
        let artifact = policy_v2();

        assert_eq!(artifact.canonical_bytes().unwrap(), expected.as_bytes());
        assert_eq!(
            artifact.sha256().unwrap(),
            "e39385ea62221cfab1bcd2f5d926714a76a742f0619bd4564f9850020cd2f135"
        );
    }

    #[test]
    fn policy_v2_valid_config_changes_change_digest() {
        let artifact = policy_v2();
        let digest = artifact.sha256().unwrap();
        let mut changes = Vec::new();

        let mut keyword = artifact.clone();
        keyword.keyword.top_k += 1;
        changes.push(keyword);

        let mut embedding = artifact.clone();
        embedding.embedding.model_revision_sha256 = "3".repeat(64);
        changes.push(embedding);

        let mut rrf = artifact.clone();
        rrf.rrf.vector_weight_millionths += 1;
        changes.push(rrf);

        let mut rerank = artifact.clone();
        rerank.rerank.timeout_ms += 1;
        changes.push(rerank);

        let mut quotas = artifact;
        quotas.request_quotas.max_hits += 1;
        changes.push(quotas);

        for changed in changes {
            assert_ne!(digest, changed.sha256().unwrap());
        }
    }

    #[test]
    fn policy_v2_rejects_wrong_trusted_source_types_or_order() {
        let mut missing = policy_v2();
        missing.trusted_source_types.pop();
        assert!(missing.validate().is_err());

        let mut reordered = policy_v2();
        reordered.trusted_source_types.swap(0, 1);
        assert!(reordered.validate().is_err());
    }

    #[test]
    fn policy_v2_rejects_mutable_or_noncanonical_revision_digests() {
        let mut mutable = policy_v2();
        mutable.rerank.model_revision_sha256 = "latest".into();
        assert!(mutable.validate().is_err());

        let mut uppercase = policy_v2();
        uppercase.rerank.config_revision_sha256 = "A".repeat(64);
        assert!(uppercase.validate().is_err());
    }

    #[test]
    fn policy_v2_rejects_unsupported_semantics_and_comparator_changes() {
        let mut direction = policy_v2();
        direction.ranking.a_primary_comparator[0] = "chunk_byte_length DESC".into();
        assert!(direction.validate().is_err());

        let mut order = policy_v2();
        order.ranking.b_exact_comparator.swap(0, 1);
        assert!(order.validate().is_err());

        let mut field = policy_v2();
        field.ranking.c_semantic_comparator[2] = "source_chunk_id ASC".into();
        assert!(field.validate().is_err());

        let mut unsupported = vec![];

        let mut normalization = policy_v2();
        normalization.normalization_version = "latest".into();
        unsupported.push(normalization);

        let mut quota = policy_v2();
        quota.ranking.quota_semantics_version = "latest".into();
        unsupported.push(quota);

        let mut tokenizer = policy_v2();
        tokenizer.keyword.tokenizer_version = "latest".into();
        unsupported.push(tokenizer);

        let mut embedding = policy_v2();
        embedding.embedding.policy_version = "latest".into();
        unsupported.push(embedding);

        let mut rerank_protocol = policy_v2();
        rerank_protocol.rerank.provider_protocol_version = "rerank.internal:443".into();
        unsupported.push(rerank_protocol);

        let mut score_normalization = policy_v2();
        score_normalization.rerank.score_normalization_version = "latest".into();
        unsupported.push(score_normalization);

        for artifact in unsupported {
            assert!(artifact.validate().is_err());
        }
    }

    #[test]
    fn policy_v2_rejects_zero_and_overflow_configuration() {
        let mut zero_top_k = policy_v2();
        zero_top_k.keyword.top_k = 0;
        assert!(zero_top_k.validate().is_err());

        let mut zero_weight = policy_v2();
        zero_weight.rrf.keyword_weight_millionths = 0;
        assert!(zero_weight.validate().is_err());

        let mut threshold_overflow = policy_v2();
        threshold_overflow.embedding.threshold_millionths = MILLIONTHS_ONE + 1;
        assert!(threshold_overflow.validate().is_err());

        let mut timeout_overflow = policy_v2();
        timeout_overflow.rerank.timeout_ms = RETRIEVAL_POLICY_MAX_TIMEOUT_MS + 1;
        assert!(timeout_overflow.validate().is_err());

        let mut quota_overflow = policy_v2();
        quota_overflow.request_quotas.max_total_bytes = RETRIEVAL_POLICY_MAX_TOTAL_BYTES + 1;
        assert!(quota_overflow.validate().is_err());
    }

    #[test]
    fn policy_v2_derives_existing_request_identity() {
        let artifact = policy_v2();
        let identity = artifact.request_identity().unwrap();

        assert_eq!(identity.contract_version, artifact.contract_version);
        assert_eq!(identity.policy_sha256, artifact.sha256().unwrap());
        assert_eq!(identity.max_hits, artifact.request_quotas.max_hits);
        assert_eq!(
            identity.max_chunk_bytes,
            artifact.request_quotas.max_chunk_bytes
        );
        assert_eq!(
            identity.max_total_bytes,
            artifact.request_quotas.max_total_bytes
        );
    }

    #[test]
    fn contract_constants_and_quota_error_are_explicit() {
        assert_eq!(KNOWLEDGE_EVIDENCE_CONTRACT_V1, "knowledge-evidence-v1");
        assert_eq!(KNOWLEDGE_EVIDENCE_CONTRACT_V2, "knowledge-evidence-v2");

        let error = KnowledgeRetrievalError::QuotaExceeded("max total bytes".into());
        assert!(matches!(&error, KnowledgeRetrievalError::QuotaExceeded(_)));
        assert_eq!(
            error.to_string(),
            "knowledge retrieval quota exceeded: max total bytes"
        );
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

    fn hit_v2(chunk: &str) -> KnowledgeEvidenceHitV2 {
        KnowledgeEvidenceHitV2 {
            schema_version: KNOWLEDGE_EVIDENCE_SCHEMA_V2,
            document_id: Uuid::from_u128(1),
            source_chunk_id: Uuid::from_u128(2),
            product_id: Uuid::from_u128(3),
            product_version_id: Uuid::from_u128(4),
            workspace_kind: "product_line".into(),
            frozen_document_display_name: "manual.pdf".into(),
            chunk_utf8: chunk.into(),
            chunk_sha256: hex::encode(Sha256::digest(chunk.as_bytes())),
            chunk_byte_length: chunk.len() as u64,
            quote_start_offset: 0,
            quote_end_offset: chunk.len() as u64,
            offset_unit: UTF8_BYTE_OFFSET_UNIT.into(),
            retrieval_rank: 1,
            retrieval_raw_score: "1.000000".into(),
            retrieval_contract_version: KNOWLEDGE_EVIDENCE_CONTRACT_V2.into(),
            source_type: KnowledgeSourceTypeV2::ParentText,
        }
    }

    fn batch_v2() -> KnowledgeEvidenceBatchV2 {
        KnowledgeEvidenceBatchV2 {
            schema_version: KNOWLEDGE_EVIDENCE_SCHEMA_V2,
            eligible_versions: vec![EligibleEvidenceVersionV1 {
                product_id: Uuid::from_u128(3),
                product_version_id: Uuid::from_u128(4),
                workspace_kind: "product_line".into(),
                frozen_display_name: "v2".into(),
            }],
            hits: vec![hit_v2("中A文")],
            exact_versions_truncated: 2,
            exact_hits_truncated: 3,
            semantic_hits_truncated: 0,
        }
    }

    fn identity_v2() -> RetrievalPolicyIdentityV1 {
        RetrievalPolicyIdentityV1 {
            contract_version: KNOWLEDGE_EVIDENCE_CONTRACT_V2.into(),
            policy_sha256: "b".repeat(64),
            max_hits: 2,
            max_chunk_bytes: 1024,
            max_total_bytes: 2048,
        }
    }

    #[test]
    fn v2_scope_tag_and_source_type_serialization_are_golden() {
        let scope = KnowledgeEvidenceScopeV2::ProductLine(ProductEvidenceRequestV1 {
            schema_version: 1,
            requirement_identity_sha256: "a".repeat(64),
            requirement_text: "条款".into(),
            product_version_ids: vec![Uuid::from_u128(1)],
            retrieval_policy: identity_v2(),
        });
        let expected = format!(
            "{{\"scope_type\":\"product_line\",\"request\":{{\"schema_version\":1,\"requirement_identity_sha256\":\"{}\",\"requirement_text\":\"条款\",\"product_version_ids\":[\"00000000-0000-0000-0000-000000000001\"],\"retrieval_policy\":{{\"contract_version\":\"knowledge-evidence-v2\",\"policy_sha256\":\"{}\",\"max_hits\":2,\"max_chunk_bytes\":1024,\"max_total_bytes\":2048}}}}}}",
            "a".repeat(64),
            "b".repeat(64)
        );
        assert_eq!(serde_json::to_string(&scope).unwrap(), expected);
        assert_eq!(
            serde_json::to_string(&KnowledgeSourceTypeV2::ImageOcr).unwrap(),
            "\"image_ocr\""
        );
    }

    #[test]
    fn v2_batch_serialization_and_validation_lock_schema_metrics_and_snapshot() {
        let batch = batch_v2();
        validate_evidence_batch_v2("product_line", &batch, &identity_v2()).unwrap();
        let value = serde_json::to_value(&batch).unwrap();
        assert_eq!(value["schema_version"], 2);
        assert_eq!(value["hits"][0]["source_type"], "parent_text");
        assert_eq!(value["exact_versions_truncated"], 2);
        assert_eq!(value["exact_hits_truncated"], 3);
        assert_eq!(value["semantic_hits_truncated"], 0);

        let mut bad_score = batch.clone();
        bad_score.hits[0].retrieval_raw_score = "0.999999".into();
        assert!(validate_evidence_batch_v2("product_line", &bad_score, &identity_v2()).is_err());
        let mut bad_offset = batch.clone();
        bad_offset.hits[0].quote_start_offset = 1;
        assert!(validate_evidence_batch_v2("product_line", &bad_offset, &identity_v2()).is_err());
        let mut semantic = batch;
        semantic.semantic_hits_truncated = 1;
        assert!(validate_evidence_batch_v2("product_line", &semantic, &identity_v2()).is_err());
    }

    #[test]
    fn v1_hit_serialization_bytes_remain_golden() {
        let expected = r#"{"schema_version":1,"document_id":"00000000-0000-0000-0000-000000000001","source_chunk_id":"00000000-0000-0000-0000-000000000002","product_id":"00000000-0000-0000-0000-000000000003","product_version_id":"00000000-0000-0000-0000-000000000004","workspace_kind":"product_line","frozen_document_display_name":"manual.pdf","chunk_utf8":"中A文","chunk_sha256":"3ead192f0e2117995036250588592ff765d9a48bc9f4d35a439a35e09ec23e99","chunk_byte_length":7,"quote_start_offset":3,"quote_end_offset":4,"offset_unit":"utf8_byte","retrieval_rank":1,"retrieval_raw_score":"0.500000","retrieval_contract_version":"knowledge-evidence-v1"}"#;
        assert_eq!(serde_json::to_string(&hit("中A文")).unwrap(), expected);
    }
}
