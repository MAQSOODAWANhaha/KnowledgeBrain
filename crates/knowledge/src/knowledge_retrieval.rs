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
pub const KNOWLEDGE_EVIDENCE_SCHEMA_V3: u16 = 3;
pub const KNOWLEDGE_EVIDENCE_CONTRACT_V1: &str = "knowledge-evidence-v1";
pub const KNOWLEDGE_EVIDENCE_CONTRACT_V2: &str = "knowledge-evidence-v2";
pub const RETRIEVAL_POLICY_SCHEMA_V2: u16 = 2;
pub const EMBEDDING_REVISION_SCHEMA_V2: u16 = 2;
pub const UTF8_BYTE_OFFSET_UNIT: &str = "utf8_byte";

pub const EMBEDDING_PROVIDER_PROTOCOL_VERSION_V2: &str = "openai-compatible-embeddings-json-v1";
pub const EMBEDDING_DIMENSION_V2: u32 = 1024;
pub const EMBEDDING_REQUEST_CONFIG_VERSION_V2: &str = "indexed-array-input-v1";
pub const EMBEDDING_REQUEST_CONFIG_SHA256_V2: &str =
    "a2ccbf02dc959b101e69f85df1b494ae0852065383e1e88e2a1c5a4bd09f40cb";
pub const EMBEDDING_OUTPUT_NORMALIZATION_VERSION_V2: &str =
    "finite-vector-no-client-normalization-v1";
pub const RERANK_REVISION_SCHEMA_V2: u16 = 2;
pub const RERANK_REQUEST_CONFIG_VERSION_V2: &str = "indexed-query-documents-v1";
pub const RERANK_REQUEST_CONFIG_SHA256_V2: &str =
    "21c0ee51fa4df1a5e436fab5e5df6ab851c2f6ebfcf115c86d77b40f40bf02f1";
const EMBEDDING_PROVIDER_MODEL_IDENTIFIER_MAX_BYTES: usize = 256;

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
pub const RETRIEVAL_SOURCE_FOLDING_VERSION_V2: &str = "unique-live-trusted-source-v1";
pub const RETRIEVAL_CHANNEL_SCORE_QUANTIZATION_VERSION_V2: &str =
    "floor-unit-interval-millionths-v1";
pub const RETRIEVAL_CHANNEL_RANK_COMPARATOR_V2: [&str; 2] =
    ["score_millionths DESC", "complete_signal_identity ASC"];
pub const RETRIEVAL_PRE_RERANK_RRF_COMPARATOR_V2: [&str; 7] = [
    "exact_rrf_score DESC",
    "vector_rank ASC NULLS LAST",
    "keyword_rank ASC NULLS LAST",
    "product_id ASC",
    "product_version_id ASC",
    "document_id ASC",
    "source_chunk_id ASC",
];
pub const RETRIEVAL_RRF_SCORE_REPRESENTATION_VERSION_V2: &str = "reduced-u128-rational-v1";
pub const RETRIEVAL_QUOTA_SEMANTICS_VERSION_V2: &str = "fair-exact-prefix-fail-closed-v1";
pub const RETRIEVAL_KEYWORD_TOKENIZER_V2: &str = "latin-numeric-cjk-bigram";
pub const RETRIEVAL_KEYWORD_TOKENIZER_VERSION_V2: &str = "v1";
pub const RETRIEVAL_KEYWORD_SCORE_VERSION_V2: &str =
    "postgres-ts-rank-cd-normalization32-millionths-v1";
pub const RETRIEVAL_EMBEDDING_POLICY_V2: &str = "declared-version-model";
pub const RETRIEVAL_EMBEDDING_POLICY_VERSION_V2: &str = "v1";
pub const RETRIEVAL_EMBEDDING_SIMILARITY_VERSION_V2: &str =
    "pgvector-cosine-clamp-zero-one-millionths-v1";
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

/// Canonical immutable identity for the embedding service behavior used by v2.
///
/// Credentials are deliberately registry metadata rather than part of these
/// canonical bytes. Field order is the canonical JSON order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingRevisionV2 {
    pub schema_version: u16,
    pub provider_protocol_version: String,
    pub provider_model_identifier: String,
    pub provider_model_revision_sha256: String,
    pub endpoint_config_sha256: String,
    pub endpoint_identity: String,
    pub dimension: u32,
    pub request_config_sha256: String,
    pub output_normalization_version: String,
}

#[derive(Debug, thiserror::Error)]
pub enum EmbeddingRevisionV2Error {
    #[error("invalid embedding revision v2: {0}")]
    Invalid(String),
    #[error("failed to serialize embedding revision v2: {0}")]
    Serialization(#[from] serde_json::Error),
}

impl EmbeddingRevisionV2 {
    pub fn canonical_request_config_sha256() -> String {
        debug_assert_eq!(
            hex::encode(Sha256::digest(
                EMBEDDING_REQUEST_CONFIG_VERSION_V2.as_bytes()
            )),
            EMBEDDING_REQUEST_CONFIG_SHA256_V2,
        );
        EMBEDDING_REQUEST_CONFIG_SHA256_V2.into()
    }

    pub fn validate(&self) -> Result<(), EmbeddingRevisionV2Error> {
        if self.schema_version != EMBEDDING_REVISION_SCHEMA_V2 {
            return Err(invalid_embedding_revision("schema_version must be 2"));
        }
        if self.provider_protocol_version != EMBEDDING_PROVIDER_PROTOCOL_VERSION_V2 {
            return Err(invalid_embedding_revision(
                "provider_protocol_version is unsupported",
            ));
        }
        validate_provider_model_identifier(&self.provider_model_identifier)?;
        validate_embedding_sha256(
            "provider_model_revision_sha256",
            &self.provider_model_revision_sha256,
        )?;
        validate_embedding_sha256("endpoint_config_sha256", &self.endpoint_config_sha256)?;
        validate_endpoint_identity(&self.endpoint_identity)?;
        if self.dimension != EMBEDDING_DIMENSION_V2 {
            return Err(invalid_embedding_revision("dimension must be 1024"));
        }
        if self.request_config_sha256 != Self::canonical_request_config_sha256() {
            return Err(invalid_embedding_revision(
                "request_config_sha256 must identify indexed-array-input-v1",
            ));
        }
        if self.output_normalization_version != EMBEDDING_OUTPUT_NORMALIZATION_VERSION_V2 {
            return Err(invalid_embedding_revision(
                "output_normalization_version is unsupported",
            ));
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, EmbeddingRevisionV2Error> {
        self.validate()?;
        Ok(serde_json::to_vec(self)?)
    }

    pub fn sha256(&self) -> Result<String, EmbeddingRevisionV2Error> {
        Ok(hex::encode(Sha256::digest(self.canonical_bytes()?)))
    }
}

/// Immutable registry artifact for the dedicated cross-encoder reranker.
/// Credentials are operational metadata and deliberately excluded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RerankRevisionV2 {
    pub schema_version: u16,
    pub provider_protocol_version: String,
    pub provider_model_identifier: String,
    pub provider_model_revision_sha256: String,
    pub config_revision_sha256: String,
    pub endpoint_identity: String,
    pub request_config_sha256: String,
    pub score_normalization_version: String,
}

#[derive(Debug, thiserror::Error)]
pub enum RerankRevisionV2Error {
    #[error("invalid rerank revision v2: {0}")]
    Invalid(String),
    #[error("failed to serialize rerank revision v2: {0}")]
    Serialization(#[from] serde_json::Error),
}

impl RerankRevisionV2 {
    pub fn canonical_request_config_sha256() -> String {
        debug_assert_eq!(
            hex::encode(Sha256::digest(RERANK_REQUEST_CONFIG_VERSION_V2.as_bytes())),
            RERANK_REQUEST_CONFIG_SHA256_V2,
        );
        RERANK_REQUEST_CONFIG_SHA256_V2.into()
    }

    pub fn validate(&self) -> Result<(), RerankRevisionV2Error> {
        if self.schema_version != RERANK_REVISION_SCHEMA_V2 {
            return Err(RerankRevisionV2Error::Invalid(
                "schema_version must be 2".into(),
            ));
        }
        if self.provider_protocol_version != RETRIEVAL_RERANK_PROTOCOL_VERSION_V2 {
            return Err(RerankRevisionV2Error::Invalid(
                "provider_protocol_version is unsupported".into(),
            ));
        }
        validate_provider_model_identifier(&self.provider_model_identifier)
            .map_err(|error| RerankRevisionV2Error::Invalid(error.to_string()))?;
        for (label, value) in [
            (
                "provider_model_revision_sha256",
                &self.provider_model_revision_sha256,
            ),
            ("config_revision_sha256", &self.config_revision_sha256),
        ] {
            validate_embedding_sha256(label, value)
                .map_err(|error| RerankRevisionV2Error::Invalid(error.to_string()))?;
        }
        validate_endpoint_identity(&self.endpoint_identity)
            .map_err(|error| RerankRevisionV2Error::Invalid(error.to_string()))?;
        if self.request_config_sha256 != Self::canonical_request_config_sha256() {
            return Err(RerankRevisionV2Error::Invalid(
                "request_config_sha256 must identify indexed-query-documents-v1".into(),
            ));
        }
        if self.score_normalization_version != RETRIEVAL_RERANK_SCORE_NORMALIZATION_VERSION_V2 {
            return Err(RerankRevisionV2Error::Invalid(
                "score_normalization_version is unsupported".into(),
            ));
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, RerankRevisionV2Error> {
        self.validate()?;
        Ok(serde_json::to_vec(self)?)
    }

    pub fn sha256(&self) -> Result<String, RerankRevisionV2Error> {
        Ok(hex::encode(Sha256::digest(self.canonical_bytes()?)))
    }
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
    pub source_folding_version: String,
    pub channel_score_quantization_version: String,
    pub channel_rank_comparator: Vec<String>,
    pub pre_rerank_rrf_comparator: Vec<String>,
    pub quota_semantics_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalKeywordPolicyV2 {
    pub tokenizer: String,
    pub tokenizer_version: String,
    pub score_version: String,
    pub top_k: u32,
    /// Minimum accepted channel score in millionths (1_000_000 = 1.0).
    pub threshold_millionths: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalEmbeddingPolicyV2 {
    pub policy: String,
    pub policy_version: String,
    pub similarity_version: String,
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
    pub score_representation_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalRerankPolicyV2 {
    pub provider_protocol_version: String,
    /// SHA-256 of the complete canonical immutable rerank revision artifact.
    pub revision_sha256: String,
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
            "ranking.source_folding_version",
            &self.ranking.source_folding_version,
            RETRIEVAL_SOURCE_FOLDING_VERSION_V2,
        )?;
        validate_supported_identity(
            "ranking.channel_score_quantization_version",
            &self.ranking.channel_score_quantization_version,
            RETRIEVAL_CHANNEL_SCORE_QUANTIZATION_VERSION_V2,
        )?;
        validate_supported_values(
            "ranking.channel_rank_comparator",
            &self.ranking.channel_rank_comparator,
            &RETRIEVAL_CHANNEL_RANK_COMPARATOR_V2,
        )?;
        validate_supported_values(
            "ranking.pre_rerank_rrf_comparator",
            &self.ranking.pre_rerank_rrf_comparator,
            &RETRIEVAL_PRE_RERANK_RRF_COMPARATOR_V2,
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
        validate_supported_identity(
            "keyword.score_version",
            &self.keyword.score_version,
            RETRIEVAL_KEYWORD_SCORE_VERSION_V2,
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
        validate_supported_identity(
            "embedding.similarity_version",
            &self.embedding.similarity_version,
            RETRIEVAL_EMBEDDING_SIMILARITY_VERSION_V2,
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
            "rrf.score_representation_version",
            &self.rrf.score_representation_version,
            RETRIEVAL_RRF_SCORE_REPRESENTATION_VERSION_V2,
        )?;

        validate_supported_identity(
            "rerank.provider_protocol_version",
            &self.rerank.provider_protocol_version,
            RETRIEVAL_RERANK_PROTOCOL_VERSION_V2,
        )?;
        validate_sha256("rerank.revision_sha256", &self.rerank.revision_sha256)?;
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

fn invalid_embedding_revision(message: impl Into<String>) -> EmbeddingRevisionV2Error {
    EmbeddingRevisionV2Error::Invalid(message.into())
}

fn validate_provider_model_identifier(value: &str) -> Result<(), EmbeddingRevisionV2Error> {
    if value.is_empty() || value.len() > EMBEDDING_PROVIDER_MODEL_IDENTIFIER_MAX_BYTES {
        return Err(invalid_embedding_revision(
            "provider_model_identifier must be a bounded pinned identifier",
        ));
    }
    let Some((model_base, revision)) = value.split_once('@') else {
        return Err(invalid_embedding_revision(
            "provider_model_identifier must use <model-base>@<revision>",
        ));
    };
    if model_base.is_empty()
        || revision.is_empty()
        || model_base
            .bytes()
            .any(|byte| !byte.is_ascii_alphanumeric() && !matches!(byte, b'.' | b'_' | b'/' | b'-'))
    {
        return Err(invalid_embedding_revision(
            "provider_model_identifier has an invalid model base",
        ));
    }
    if let Some(digest) = revision.strip_prefix("sha256:") {
        validate_embedding_sha256("provider_model_identifier revision", digest)?;
        return Ok(());
    }
    if !is_real_iso_date(revision) {
        return Err(invalid_embedding_revision(
            "provider_model_identifier revision must be a real YYYY-MM-DD date or sha256 digest",
        ));
    }
    Ok(())
}

fn validate_embedding_sha256(label: &str, value: &str) -> Result<(), EmbeddingRevisionV2Error> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(invalid_embedding_revision(format!(
            "{label} must be a lowercase SHA-256 digest"
        )));
    }
    Ok(())
}

fn is_real_iso_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes
            .iter()
            .enumerate()
            .any(|(index, byte)| index != 4 && index != 7 && !byte.is_ascii_digit())
    {
        return false;
    }
    let year = value[0..4].parse::<u32>().unwrap_or_default();
    let month = value[5..7].parse::<u32>().unwrap_or_default();
    let day = value[8..10].parse::<u32>().unwrap_or_default();
    let leap_year =
        year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap_year => 29,
        2 => 28,
        _ => return false,
    };
    year != 0 && (1..=days_in_month).contains(&day)
}

fn validate_endpoint_identity(value: &str) -> Result<(), EmbeddingRevisionV2Error> {
    let remainder = value
        .strip_prefix("https://")
        .ok_or_else(|| invalid_embedding_revision("endpoint_identity must use lowercase https"))?;
    let (authority, path) = remainder.split_once('/').unwrap_or((remainder, ""));
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (authority, None),
    };

    let valid_dns_label = |label: &str| {
        let bytes = label.as_bytes();
        !bytes.is_empty()
            && bytes
                .first()
                .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
            && bytes
                .last()
                .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
            && bytes
                .iter()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
    };
    if host.is_empty()
        || authority.bytes().filter(|byte| *byte == b':').count() > 1
        || !host.split('.').all(valid_dns_label)
    {
        return Err(invalid_embedding_revision(
            "endpoint_identity must use a canonical lowercase DNS host",
        ));
    }

    if let Some(port) = port {
        if port.is_empty()
            || port.starts_with('0')
            || !port.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(invalid_embedding_revision(
                "endpoint_identity has an invalid port",
            ));
        }
        let parsed = port
            .parse::<u16>()
            .map_err(|_| invalid_embedding_revision("endpoint_identity has an invalid port"))?;
        if parsed == 443 {
            return Err(invalid_embedding_revision(
                "endpoint_identity has a noncanonical port",
            ));
        }
    }

    if !path.is_empty()
        && path.split('/').any(|segment| {
            segment.is_empty()
                || matches!(segment, "." | "..")
                || !segment.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'~' | b'-')
                })
        })
    {
        return Err(invalid_embedding_revision(
            "endpoint_identity has a noncanonical path",
        ));
    }
    if remainder.ends_with('/') {
        return Err(invalid_embedding_revision(
            "endpoint_identity must not have a trailing slash",
        ));
    }
    Ok(())
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeEvidenceMediaV1 {
    pub image_artifact_revision_id: Uuid,
    pub object_ref: String,
    pub sha256: String,
    pub media_type: String,
    pub width: u32,
    pub height: u32,
    pub page_ordinal: Option<u32>,
    pub bounding_region: Option<serde_json::Value>,
    pub frozen_document_display_name: String,
}

/// Sole schema-3 retrieval snapshot. Exact/semantic V2 ranking policy remains
/// frozen, while image OCR sources now carry an immutable media identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeEvidenceHitV3 {
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
    /// Present only for the reranked C suffix. This freezes the stable RRF
    /// tie-break provenance without exposing the pre-rerank candidate DTO.
    pub pre_rerank_rrf_rank: Option<u32>,
    pub retrieval_contract_version: String,
    pub source_type: KnowledgeSourceTypeV2,
    pub media: Option<KnowledgeEvidenceMediaV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeEvidenceBatchV3 {
    pub schema_version: u16,
    pub eligible_versions: Vec<EligibleEvidenceVersionV1>,
    pub hits: Vec<KnowledgeEvidenceHitV3>,
    pub exact_prefix_hit_count: u32,
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
pub fn validate_evidence_batch_v3(
    expected_workspace_kind: &str,
    batch: &KnowledgeEvidenceBatchV3,
    policy: &RetrievalPolicyIdentityV1,
) -> Result<(), KnowledgeRetrievalError> {
    let max_hits = usize::try_from(policy.max_hits).map_err(|_| {
        KnowledgeRetrievalError::InvalidHit("v2 max_hits cannot be represented".into())
    })?;
    if batch.schema_version != KNOWLEDGE_EVIDENCE_SCHEMA_V3
        || policy.contract_version != KNOWLEDGE_EVIDENCE_CONTRACT_V2
        || !matches!(expected_workspace_kind, "product_line" | "company")
        || policy.max_hits == 0
        || policy.max_chunk_bytes == 0
        || policy.max_total_bytes == 0
        || batch.hits.len() > max_hits
        || usize::try_from(batch.exact_prefix_hit_count)
            .ok()
            .is_none_or(|count| count > batch.hits.len())
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
    let exact_versions_truncated =
        usize::try_from(batch.exact_versions_truncated).map_err(|_| {
            KnowledgeRetrievalError::InvalidHit(
                "v2 exact_versions_truncated cannot be represented".into(),
            )
        })?;
    if exact_versions_truncated > eligible.len().saturating_sub(max_hits)
        || batch.exact_hits_truncated < batch.exact_versions_truncated
        || (batch.exact_prefix_hit_count == 0
            && (batch.exact_versions_truncated != 0 || batch.exact_hits_truncated != 0))
        || (batch.exact_versions_truncated > 0
            && usize::try_from(batch.exact_prefix_hit_count).ok() != Some(max_hits))
    {
        return Err(KnowledgeRetrievalError::InvalidHit(
            "inconsistent v2 exact truncation metrics".into(),
        ));
    }

    let mut identities = HashSet::new();
    let mut hit_versions = HashSet::new();
    let mut total_bytes = 0u64;
    for (index, hit) in batch.hits.iter().enumerate() {
        let chunk = hit.chunk_utf8.as_bytes();
        total_bytes = total_bytes.checked_add(chunk.len() as u64).ok_or_else(|| {
            KnowledgeRetrievalError::InvalidHit("v2 hit byte quota overflow".into())
        })?;
        let media_valid = match (&hit.source_type, &hit.media) {
            (KnowledgeSourceTypeV2::ImageOcr, Some(media)) => {
                !media.image_artifact_revision_id.is_nil()
                    && media.object_ref == format!("objects/{}", media.sha256)
                    && media.sha256.len() == 64
                    && media
                        .sha256
                        .bytes()
                        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
                    && matches!(
                        media.media_type.as_str(),
                        "image/png" | "image/jpeg" | "image/webp"
                    )
                    && media.width > 0
                    && media.height > 0
                    && media.frozen_document_display_name == hit.frozen_document_display_name
                    && media
                        .bounding_region
                        .as_ref()
                        .is_none_or(|value| value.is_object())
            }
            (KnowledgeSourceTypeV2::ImageOcr, None) => false,
            (_, None) => true,
            (_, Some(_)) => false,
        };
        if hit.schema_version != KNOWLEDGE_EVIDENCE_SCHEMA_V3
            || !media_valid
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
            || (index < batch.exact_prefix_hit_count as usize
                && (hit.retrieval_raw_score != "1.000000" || hit.pre_rerank_rrf_rank.is_some()))
            || (index >= batch.exact_prefix_hit_count as usize
                && (!is_unit_interval_millionths(&hit.retrieval_raw_score)
                    || hit.pre_rerank_rrf_rank.is_none_or(|rank| rank == 0)))
            || !eligible.contains(&(hit.product_id, hit.product_version_id))
            || !identities.insert((hit.document_id, hit.source_chunk_id))
        {
            return Err(KnowledgeRetrievalError::InvalidHit(
                "v2 evidence scope, source, bytes, digest, offset, rank, score, or identity is invalid"
                    .into(),
            ));
        }
        if index < batch.exact_prefix_hit_count as usize {
            hit_versions.insert((hit.product_id, hit.product_version_id));
        }
    }
    let semantic_start = batch.exact_prefix_hit_count as usize;
    let semantic = &batch.hits[semantic_start..];
    let mut semantic_rrf_ranks = HashSet::new();
    for (index, hit) in semantic.iter().enumerate() {
        let rrf_rank = hit
            .pre_rerank_rrf_rank
            .expect("semantic provenance was validated above");
        if !semantic_rrf_ranks.insert(rrf_rank) {
            return Err(KnowledgeRetrievalError::InvalidHit(
                "duplicate v2 semantic pre-rerank rank".into(),
            ));
        }
        if let Some(previous) = index.checked_sub(1).and_then(|i| semantic.get(i)) {
            let incorrectly_ordered = previous.retrieval_raw_score < hit.retrieval_raw_score
                || (previous.retrieval_raw_score == hit.retrieval_raw_score
                    && previous.pre_rerank_rrf_rank > hit.pre_rerank_rrf_rank);
            if incorrectly_ordered {
                return Err(KnowledgeRetrievalError::InvalidHit(
                    "v2 semantic suffix violates the rerank total order".into(),
                ));
            }
        }
    }
    if total_bytes > policy.max_total_bytes {
        return Err(KnowledgeRetrievalError::InvalidHit(
            "returned v2 hit byte quota exceeded".into(),
        ));
    }
    if batch.exact_versions_truncated > 0 && hit_versions.len() != max_hits {
        return Err(KnowledgeRetrievalError::InvalidHit(
            "v2 truncated exact versions require a complete distinct primary prefix".into(),
        ));
    }
    Ok(())
}

fn is_unit_interval_millionths(value: &str) -> bool {
    value == "1.000000"
        || (value.len() == 8
            && value.starts_with("0.")
            && value[2..].bytes().all(|byte| byte.is_ascii_digit()))
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

/// Sole schema-3 evidence retrieval seam. Ranking and policy remain frozen
/// at the V2 revision, while each image OCR hit carries immutable media.
#[async_trait]
pub trait KnowledgeRetrievalPortV3: Send + Sync {
    async fn retrieve_evidence_v3(
        &self,
        scope: KnowledgeEvidenceScopeV2,
    ) -> Result<KnowledgeEvidenceBatchV3, KnowledgeRetrievalError>;
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
                source_folding_version: RETRIEVAL_SOURCE_FOLDING_VERSION_V2.into(),
                channel_score_quantization_version: RETRIEVAL_CHANNEL_SCORE_QUANTIZATION_VERSION_V2
                    .into(),
                channel_rank_comparator: RETRIEVAL_CHANNEL_RANK_COMPARATOR_V2
                    .iter()
                    .map(|value| (*value).into())
                    .collect(),
                pre_rerank_rrf_comparator: RETRIEVAL_PRE_RERANK_RRF_COMPARATOR_V2
                    .iter()
                    .map(|value| (*value).into())
                    .collect(),
                quota_semantics_version: RETRIEVAL_QUOTA_SEMANTICS_VERSION_V2.into(),
            },
            keyword: RetrievalKeywordPolicyV2 {
                tokenizer: RETRIEVAL_KEYWORD_TOKENIZER_V2.into(),
                tokenizer_version: RETRIEVAL_KEYWORD_TOKENIZER_VERSION_V2.into(),
                score_version: RETRIEVAL_KEYWORD_SCORE_VERSION_V2.into(),
                top_k: 128,
                threshold_millionths: 50_000,
            },
            embedding: RetrievalEmbeddingPolicyV2 {
                policy: RETRIEVAL_EMBEDDING_POLICY_V2.into(),
                policy_version: RETRIEVAL_EMBEDDING_POLICY_VERSION_V2.into(),
                similarity_version: RETRIEVAL_EMBEDDING_SIMILARITY_VERSION_V2.into(),
                model_revision_sha256: embedding_revision_v2().sha256().unwrap(),
                top_k: 128,
                threshold_millionths: 100_000,
            },
            rrf: RetrievalRrfPolicyV2 {
                k: 60,
                keyword_weight_millionths: 1_000_000,
                vector_weight_millionths: 1_000_000,
                score_representation_version: RETRIEVAL_RRF_SCORE_REPRESENTATION_VERSION_V2.into(),
            },
            rerank: RetrievalRerankPolicyV2 {
                provider_protocol_version: RETRIEVAL_RERANK_PROTOCOL_VERSION_V2.into(),
                revision_sha256: "3".repeat(64),
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

    fn embedding_revision_v2() -> EmbeddingRevisionV2 {
        EmbeddingRevisionV2 {
            schema_version: EMBEDDING_REVISION_SCHEMA_V2,
            provider_protocol_version: EMBEDDING_PROVIDER_PROTOCOL_VERSION_V2.into(),
            provider_model_identifier: "text-embedding-model@2025-01-15".into(),
            provider_model_revision_sha256: "b".repeat(64),
            endpoint_config_sha256: "c".repeat(64),
            endpoint_identity: "https://embeddings.example.test/v1/embeddings".into(),
            dimension: EMBEDDING_DIMENSION_V2,
            request_config_sha256: EmbeddingRevisionV2::canonical_request_config_sha256(),
            output_normalization_version: EMBEDDING_OUTPUT_NORMALIZATION_VERSION_V2.into(),
        }
    }

    #[test]
    fn embedding_revision_v2_canonical_bytes_and_digest_are_golden() {
        let artifact = embedding_revision_v2();
        let expected = r#"{"schema_version":2,"provider_protocol_version":"openai-compatible-embeddings-json-v1","provider_model_identifier":"text-embedding-model@2025-01-15","provider_model_revision_sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","endpoint_config_sha256":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","endpoint_identity":"https://embeddings.example.test/v1/embeddings","dimension":1024,"request_config_sha256":"a2ccbf02dc959b101e69f85df1b494ae0852065383e1e88e2a1c5a4bd09f40cb","output_normalization_version":"finite-vector-no-client-normalization-v1"}"#;
        assert_eq!(artifact.canonical_bytes().unwrap(), expected.as_bytes());
        assert_eq!(
            artifact.sha256().unwrap(),
            "4ec0b20218c6b97230fddadd5cc821c9aa2d1489cd0cb0ffc1f0dbca0efa9ca3"
        );
        assert!(!expected.contains("credential"));
        assert!(!expected.contains("secret"));
    }

    fn rerank_revision_v2() -> RerankRevisionV2 {
        RerankRevisionV2 {
            schema_version: RERANK_REVISION_SCHEMA_V2,
            provider_protocol_version: RETRIEVAL_RERANK_PROTOCOL_VERSION_V2.into(),
            provider_model_identifier: "cross-encoder@2025-01-15".into(),
            provider_model_revision_sha256: "d".repeat(64),
            config_revision_sha256: "e".repeat(64),
            endpoint_identity: "https://rerank.example.test/v1/rerank".into(),
            request_config_sha256: RerankRevisionV2::canonical_request_config_sha256(),
            score_normalization_version: RETRIEVAL_RERANK_SCORE_NORMALIZATION_VERSION_V2.into(),
        }
    }

    #[test]
    fn rerank_revision_v2_is_canonical_and_https_only() {
        let artifact = rerank_revision_v2();
        let bytes = artifact.canonical_bytes().unwrap();
        assert_eq!(
            serde_json::from_slice::<RerankRevisionV2>(&bytes).unwrap(),
            artifact
        );
        assert_eq!(artifact.sha256().unwrap().len(), 64);
        let mut http = artifact;
        http.endpoint_identity = "http://rerank.example.test/v1/rerank".into();
        assert!(http.validate().is_err());
    }

    #[test]
    fn embedding_revision_v2_endpoint_identity_uses_canonical_dns_grammar() {
        for accepted in [
            "https://localhost:8443",
            "https://embedding-api.example.test/v1/embeddings",
            "https://a-b.c9:8080/path_1/~model.v2",
        ] {
            let mut value = embedding_revision_v2();
            value.endpoint_identity = accepted.into();
            assert!(value.validate().is_ok(), "accepted endpoint {accepted}");
        }

        for rejected in [
            "ftp://localhost",
            "http://localhost",
            "http://a-b.c9:8080/path_1/~model.v2",
            "HTTP://localhost",
            "https://[::1]",
            "https://[]",
            "https://[not-an-ip]",
            "https://-host.example",
            "https://host-.example",
            "https://host_name.example",
            "https://Host.example",
            "http://localhost:80",
            "https://localhost:443",
            "https://localhost:0",
            "https://localhost:01",
            "https://localhost:65536",
            "https://localhost/",
            "https://localhost/a//b",
            "https://localhost/.",
            "https://localhost/..",
            "https://localhost/a/b%20c",
            "https://localhost/a+b",
            "https://user@localhost/path",
            "https://localhost/path?query",
            "https://localhost/path#fragment",
            "https://localhost/white space",
        ] {
            let mut value = embedding_revision_v2();
            value.endpoint_identity = rejected.into();
            assert!(value.validate().is_err(), "rejected endpoint {rejected}");
        }
    }

    #[test]
    fn embedding_revision_v2_rejects_unsupported_or_mutable_provenance() {
        let mut mutations = Vec::new();
        let mut value = embedding_revision_v2();
        value.schema_version = 1;
        mutations.push(value);
        let mut value = embedding_revision_v2();
        value.provider_protocol_version = "latest".into();
        mutations.push(value);
        for unpinned_or_mutable in [
            "text-embedding-model",
            "bare",
            "unversioned",
            "prod",
            "stable",
            "latest",
            "current",
            "default",
            "text-embedding-model@prod",
            "text-embedding-model@stable",
            "text-embedding-model@latest",
            "text-embedding-model@current",
            "text-embedding-model@default",
            "text model@2025-01-15",
            "text-embedding-model@2025-02-29",
            "text-embedding-model@2024-13-01",
            "text-embedding-model@sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        ] {
            let mut value = embedding_revision_v2();
            value.provider_model_identifier = unpinned_or_mutable.into();
            mutations.push(value);
        }
        let mut value = embedding_revision_v2();
        value.provider_model_revision_sha256 = "A".repeat(64);
        mutations.push(value);
        let mut value = embedding_revision_v2();
        value.endpoint_config_sha256 = "latest".into();
        mutations.push(value);
        let mut value = embedding_revision_v2();
        value.endpoint_identity = "https://user@example.test/v1".into();
        mutations.push(value);
        let mut value = embedding_revision_v2();
        value.endpoint_identity = "https://example.test/v1/".into();
        mutations.push(value);
        let mut value = embedding_revision_v2();
        value.endpoint_identity = "https://example.test/v1?key=secret".into();
        mutations.push(value);
        let mut value = embedding_revision_v2();
        value.endpoint_identity = "https://Example.test/v1".into();
        mutations.push(value);
        let mut value = embedding_revision_v2();
        value.endpoint_identity = "https://example.test:443/v1".into();
        mutations.push(value);
        let mut value = embedding_revision_v2();
        value.dimension = 1536;
        mutations.push(value);
        let mut value = embedding_revision_v2();
        value.request_config_sha256 = "A".repeat(64);
        mutations.push(value);
        let mut value = embedding_revision_v2();
        value.output_normalization_version = "latest".into();
        mutations.push(value);
        for mutation in mutations {
            assert!(mutation.validate().is_err());
        }
    }

    #[test]
    fn embedding_revision_v2_semantic_changes_change_digest() {
        let artifact = embedding_revision_v2();
        let digest = artifact.sha256().unwrap();
        let mut model = artifact.clone();
        model.provider_model_identifier = "text-embedding-model@2025-01-16".into();
        let mut digest_pinned_model = artifact.clone();
        digest_pinned_model.provider_model_identifier =
            format!("text-embedding-model@sha256:{}", "d".repeat(64));
        let mut endpoint = artifact;
        endpoint.endpoint_identity = "https://embeddings-2.example.test/v1/embeddings".into();
        assert_ne!(model.sha256().unwrap(), digest);
        assert_ne!(digest_pinned_model.sha256().unwrap(), digest);
        assert_ne!(endpoint.sha256().unwrap(), digest);
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
        let expected = r#"{"schema_version":2,"contract_version":"knowledge-evidence-v2","normalization_version":"unicode-whitespace-lowercase-v1","trusted_source_types":["text","parent_text","image_ocr"],"ranking":{"a_primary_comparator":["chunk_byte_length ASC","document_id ASC","source_chunk_id ASC"],"a_version_comparator":["product_id ASC","product_version_id ASC"],"b_exact_comparator":["product_id ASC","product_version_id ASC","chunk_byte_length ASC","document_id ASC","source_chunk_id ASC"],"c_semantic_comparator":["normalized_rerank_score DESC","pre_rerank_rrf_rank ASC","complete_source_identity ASC"],"source_folding_version":"unique-live-trusted-source-v1","channel_score_quantization_version":"floor-unit-interval-millionths-v1","channel_rank_comparator":["score_millionths DESC","complete_signal_identity ASC"],"pre_rerank_rrf_comparator":["exact_rrf_score DESC","vector_rank ASC NULLS LAST","keyword_rank ASC NULLS LAST","product_id ASC","product_version_id ASC","document_id ASC","source_chunk_id ASC"],"quota_semantics_version":"fair-exact-prefix-fail-closed-v1"},"keyword":{"tokenizer":"latin-numeric-cjk-bigram","tokenizer_version":"v1","score_version":"postgres-ts-rank-cd-normalization32-millionths-v1","top_k":128,"threshold_millionths":50000},"embedding":{"policy":"declared-version-model","policy_version":"v1","similarity_version":"pgvector-cosine-clamp-zero-one-millionths-v1","model_revision_sha256":"4ec0b20218c6b97230fddadd5cc821c9aa2d1489cd0cb0ffc1f0dbca0efa9ca3","top_k":128,"threshold_millionths":100000},"rrf":{"k":60,"keyword_weight_millionths":1000000,"vector_weight_millionths":1000000,"score_representation_version":"reduced-u128-rational-v1"},"rerank":{"provider_protocol_version":"indexed-json-v1","revision_sha256":"3333333333333333333333333333333333333333333333333333333333333333","model_revision_sha256":"2222222222222222222222222222222222222222222222222222222222222222","config_revision_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","top_k":64,"timeout_ms":5000,"score_normalization_version":"unit-interval-millionths-v1"},"request_quotas":{"max_hits":64,"max_chunk_bytes":262144,"max_total_bytes":8388608}}"#;
        let artifact = policy_v2();

        assert_eq!(artifact.canonical_bytes().unwrap(), expected.as_bytes());
        assert_eq!(
            artifact.sha256().unwrap(),
            "c877bcc06766d7060879579a07fdc851c6266c57654cd93cc32f87b456073964"
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
    fn policy_v2_semantic_fields_are_digest_bearing() {
        let artifact = policy_v2();
        let digest = |value: &RetrievalPolicyV2| {
            hex::encode(Sha256::digest(serde_json::to_vec(value).unwrap()))
        };
        let expected = digest(&artifact);
        let mut changes = Vec::new();
        let mut value = artifact.clone();
        value.ranking.source_folding_version.push_str("-changed");
        changes.push(value);
        let mut value = artifact.clone();
        value
            .ranking
            .channel_score_quantization_version
            .push_str("-changed");
        changes.push(value);
        let mut value = artifact.clone();
        value.ranking.channel_rank_comparator.swap(0, 1);
        changes.push(value);
        let mut value = artifact.clone();
        value.ranking.pre_rerank_rrf_comparator.swap(1, 2);
        changes.push(value);
        let mut value = artifact;
        value.rrf.score_representation_version.push_str("-changed");
        changes.push(value);
        for changed in changes {
            assert_ne!(digest(&changed), expected);
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

        let mut source_folding = policy_v2();
        source_folding.ranking.source_folding_version = "latest".into();
        unsupported.push(source_folding);

        let mut quantization = policy_v2();
        quantization.ranking.channel_score_quantization_version = "latest".into();
        unsupported.push(quantization);

        let mut channel_rank = policy_v2();
        channel_rank.ranking.channel_rank_comparator.swap(0, 1);
        unsupported.push(channel_rank);

        let mut pre_rerank = policy_v2();
        pre_rerank.ranking.pre_rerank_rrf_comparator.pop();
        unsupported.push(pre_rerank);

        let mut quota = policy_v2();
        quota.ranking.quota_semantics_version = "latest".into();
        unsupported.push(quota);

        let mut score_representation = policy_v2();
        score_representation.rrf.score_representation_version = "latest".into();
        unsupported.push(score_representation);

        let mut tokenizer = policy_v2();
        tokenizer.keyword.tokenizer_version = "latest".into();
        unsupported.push(tokenizer);

        let mut keyword_score = policy_v2();
        keyword_score.keyword.score_version = "latest".into();
        unsupported.push(keyword_score);

        let mut embedding = policy_v2();
        embedding.embedding.policy_version = "latest".into();
        unsupported.push(embedding);

        let mut embedding_similarity = policy_v2();
        embedding_similarity.embedding.similarity_version = "latest".into();
        unsupported.push(embedding_similarity);

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

    fn hit_v3(chunk: &str) -> KnowledgeEvidenceHitV3 {
        KnowledgeEvidenceHitV3 {
            schema_version: KNOWLEDGE_EVIDENCE_SCHEMA_V3,
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
            pre_rerank_rrf_rank: None,
            retrieval_contract_version: KNOWLEDGE_EVIDENCE_CONTRACT_V2.into(),
            source_type: KnowledgeSourceTypeV2::ParentText,
            media: None,
        }
    }

    fn batch_v3() -> KnowledgeEvidenceBatchV3 {
        KnowledgeEvidenceBatchV3 {
            schema_version: KNOWLEDGE_EVIDENCE_SCHEMA_V3,
            eligible_versions: vec![EligibleEvidenceVersionV1 {
                product_id: Uuid::from_u128(3),
                product_version_id: Uuid::from_u128(4),
                workspace_kind: "product_line".into(),
                frozen_display_name: "v2".into(),
            }],
            hits: vec![hit_v3("中A文")],
            exact_prefix_hit_count: 1,
            exact_versions_truncated: 0,
            exact_hits_truncated: 0,
            semantic_hits_truncated: 0,
        }
    }

    fn truncated_batch_v3() -> KnowledgeEvidenceBatchV3 {
        let mut batch = batch_v3();
        batch
            .eligible_versions
            .extend([(5, 6), (7, 8)].map(|(product_id, product_version_id)| {
                EligibleEvidenceVersionV1 {
                    product_id: Uuid::from_u128(product_id),
                    product_version_id: Uuid::from_u128(product_version_id),
                    workspace_kind: "product_line".into(),
                    frozen_display_name: format!("v{product_version_id}"),
                }
            }));
        let mut second_hit = hit_v3("第二");
        second_hit.document_id = Uuid::from_u128(9);
        second_hit.source_chunk_id = Uuid::from_u128(10);
        second_hit.product_id = Uuid::from_u128(5);
        second_hit.product_version_id = Uuid::from_u128(6);
        second_hit.retrieval_rank = 2;
        batch.hits.push(second_hit);
        batch.exact_prefix_hit_count = 2;
        batch.exact_versions_truncated = 1;
        batch.exact_hits_truncated = 1;
        batch
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
    fn v3_batch_serialization_and_validation_lock_schema_metrics_and_snapshot() {
        let batch = batch_v3();
        validate_evidence_batch_v3("product_line", &batch, &identity_v2()).unwrap();
        let expected = r#"{"schema_version":3,"eligible_versions":[{"product_id":"00000000-0000-0000-0000-000000000003","product_version_id":"00000000-0000-0000-0000-000000000004","workspace_kind":"product_line","frozen_display_name":"v2"}],"hits":[{"schema_version":3,"document_id":"00000000-0000-0000-0000-000000000001","source_chunk_id":"00000000-0000-0000-0000-000000000002","product_id":"00000000-0000-0000-0000-000000000003","product_version_id":"00000000-0000-0000-0000-000000000004","workspace_kind":"product_line","frozen_document_display_name":"manual.pdf","chunk_utf8":"中A文","chunk_sha256":"3ead192f0e2117995036250588592ff765d9a48bc9f4d35a439a35e09ec23e99","chunk_byte_length":7,"quote_start_offset":0,"quote_end_offset":7,"offset_unit":"utf8_byte","retrieval_rank":1,"retrieval_raw_score":"1.000000","pre_rerank_rrf_rank":null,"retrieval_contract_version":"knowledge-evidence-v2","source_type":"parent_text","media":null}],"exact_prefix_hit_count":1,"exact_versions_truncated":0,"exact_hits_truncated":0,"semantic_hits_truncated":0}"#;
        assert_eq!(serde_json::to_string(&batch).unwrap(), expected);
        assert_eq!(
            serde_json::from_str::<KnowledgeEvidenceBatchV3>(expected).unwrap(),
            batch
        );

        let mut missing_prefix = serde_json::to_value(&batch).unwrap();
        missing_prefix
            .as_object_mut()
            .unwrap()
            .remove("exact_prefix_hit_count");
        assert!(serde_json::from_value::<KnowledgeEvidenceBatchV3>(missing_prefix).is_err());

        let mut bad_score = batch.clone();
        bad_score.hits[0].retrieval_raw_score = "0.999999".into();
        assert!(validate_evidence_batch_v3("product_line", &bad_score, &identity_v2()).is_err());
        let mut bad_offset = batch.clone();
        bad_offset.hits[0].quote_start_offset = 1;
        assert!(validate_evidence_batch_v3("product_line", &bad_offset, &identity_v2()).is_err());
        let mut semantic = batch;
        semantic.exact_prefix_hit_count = 0;
        semantic.hits[0].retrieval_raw_score = "0.999999".into();
        semantic.hits[0].pre_rerank_rrf_rank = Some(1);
        semantic.semantic_hits_truncated = 1;
        validate_evidence_batch_v3("product_line", &semantic, &identity_v2()).unwrap();
    }

    #[test]
    fn v2_batch_validation_rejects_impossible_exact_metrics() {
        let policy = identity_v2();
        let valid_truncated = truncated_batch_v3();
        validate_evidence_batch_v3("product_line", &valid_truncated, &policy).unwrap();

        let mut beyond_eligible_bound = batch_v3();
        beyond_eligible_bound.exact_versions_truncated = 1;
        beyond_eligible_bound.exact_hits_truncated = 1;
        assert!(
            validate_evidence_batch_v3("product_line", &beyond_eligible_bound, &policy).is_err()
        );

        let mut fewer_hits_than_versions = valid_truncated.clone();
        fewer_hits_than_versions.exact_hits_truncated = 0;
        assert!(
            validate_evidence_batch_v3("product_line", &fewer_hits_than_versions, &policy).is_err()
        );

        let mut empty_with_truncated_versions = batch_v3();
        empty_with_truncated_versions.hits.clear();
        empty_with_truncated_versions.exact_prefix_hit_count = 0;
        empty_with_truncated_versions
            .eligible_versions
            .extend(valid_truncated.eligible_versions[1..].iter().cloned());
        empty_with_truncated_versions.exact_versions_truncated = 1;
        empty_with_truncated_versions.exact_hits_truncated = 1;
        assert!(
            validate_evidence_batch_v3("product_line", &empty_with_truncated_versions, &policy)
                .is_err()
        );

        let mut empty_with_truncated_hits = batch_v3();
        empty_with_truncated_hits.hits.clear();
        empty_with_truncated_hits.exact_prefix_hit_count = 0;
        empty_with_truncated_hits.exact_hits_truncated = 1;
        assert!(
            validate_evidence_batch_v3("product_line", &empty_with_truncated_hits, &policy)
                .is_err()
        );

        let mut incomplete_primary_prefix = valid_truncated.clone();
        incomplete_primary_prefix.hits.pop();
        incomplete_primary_prefix.exact_prefix_hit_count = 1;
        assert!(
            validate_evidence_batch_v3("product_line", &incomplete_primary_prefix, &policy)
                .is_err()
        );

        let mut duplicate_version_prefix = valid_truncated.clone();
        duplicate_version_prefix.hits[1].product_id = duplicate_version_prefix.hits[0].product_id;
        duplicate_version_prefix.hits[1].product_version_id =
            duplicate_version_prefix.hits[0].product_version_id;
        assert!(
            validate_evidence_batch_v3("product_line", &duplicate_version_prefix, &policy).is_err()
        );

        let mut semantic_only = batch_v3();
        semantic_only.exact_prefix_hit_count = 0;
        semantic_only.hits[0].retrieval_raw_score = "0.750000".into();
        semantic_only.hits[0].pre_rerank_rrf_rank = Some(1);
        validate_evidence_batch_v3("product_line", &semantic_only, &policy).unwrap();

        semantic_only.hits[0].retrieval_raw_score = "0.75000".into();
        assert!(validate_evidence_batch_v3("product_line", &semantic_only, &policy).is_err());
    }

    #[test]
    fn v1_hit_serialization_bytes_remain_golden() {
        let expected = r#"{"schema_version":1,"document_id":"00000000-0000-0000-0000-000000000001","source_chunk_id":"00000000-0000-0000-0000-000000000002","product_id":"00000000-0000-0000-0000-000000000003","product_version_id":"00000000-0000-0000-0000-000000000004","workspace_kind":"product_line","frozen_document_display_name":"manual.pdf","chunk_utf8":"中A文","chunk_sha256":"3ead192f0e2117995036250588592ff765d9a48bc9f4d35a439a35e09ec23e99","chunk_byte_length":7,"quote_start_offset":3,"quote_end_offset":4,"offset_unit":"utf8_byte","retrieval_rank":1,"retrieval_raw_score":"0.500000","retrieval_contract_version":"knowledge-evidence-v1"}"#;
        assert_eq!(serde_json::to_string(&hit("中A文")).unwrap(), expected);
    }
}
