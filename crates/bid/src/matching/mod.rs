//! Final V1 `MatchingPublication` domain model.
//!
//! The public workflow consumes only a frozen route claim. Open/Stage/Commit
//! remains private to the storage adapter.

mod handler;
mod workflow;

pub use handler::run_match_route_v1;
pub use workflow::{
    EvidenceVerifier, FakeVerifier, LexicalEvidenceVerifier, MatchError, MatchingWorkflow,
    VerifyOutcome,
};

use rust_decimal::{Decimal, RoundingStrategy};
use serde::{Deserialize, Serialize, Serializer};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use uuid::Uuid;

pub const MATCHING_REPORT_SCHEMA_V1: u16 = 1;
pub const EVIDENCE_SCHEMA_V1: u16 = 1;
pub const UTF8_BYTE_OFFSET_UNIT: &str = "utf8_byte";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CanonicalDecimal(Decimal);

impl CanonicalDecimal {
    pub fn new(value: Decimal) -> Self {
        let mut value = value.round_dp_with_strategy(6, RoundingStrategy::MidpointNearestEven);
        value.rescale(6);
        Self(value)
    }

    pub fn decimal(self) -> Decimal {
        self.0
    }
}

impl Serialize for CanonicalDecimal {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&format!("{:.6}", self.0))
    }
}

impl<'de> Deserialize<'de> for CanonicalDecimal {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value
            .parse::<Decimal>()
            .map(Self::new)
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MatchRoute {
    Technical { unit_id: Uuid },
    Commercial,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmptyDisposition {
    ClearRoute,
    SkipUnit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerifierSupport {
    Contradicted,
    Insufficient,
    Unresolved,
    Supported,
}

impl VerifierSupport {
    pub fn priority(self) -> u8 {
        match self {
            Self::Supported => 4,
            Self::Unresolved => 3,
            Self::Insufficient => 2,
            Self::Contradicted => 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemDecision {
    Select,
    Review,
    Reject,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityStatus {
    Pass,
    Review,
    Block,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrozenRetrievedHitV1 {
    pub product_version_artifact_id: Uuid,
    pub route_product_ordinal: u32,
    pub document_id: Uuid,
    pub source_chunk_id: Uuid,
    pub frozen_document_display_name: String,
    pub chunk_utf8: String,
    pub chunk_sha256: String,
    pub chunk_byte_length: u64,
    pub retrieval_rank: u32,
    pub retrieval_raw_score: CanonicalDecimal,
    pub quote_start_offset: u64,
    pub quote_end_offset: u64,
    pub offset_unit: String,
    pub retrieval_contract_version: String,
}

impl FrozenRetrievedHitV1 {
    pub fn quote(&self) -> Result<&str, MatchValidationError> {
        let bytes = self.chunk_utf8.as_bytes();
        let start = usize::try_from(self.quote_start_offset)
            .map_err(|_| MatchValidationError::InvalidEvidence("offset overflow".into()))?;
        let end = usize::try_from(self.quote_end_offset)
            .map_err(|_| MatchValidationError::InvalidEvidence("offset overflow".into()))?;
        if self.offset_unit != UTF8_BYTE_OFFSET_UNIT
            || self.chunk_byte_length != bytes.len() as u64
            || self.chunk_sha256 != sha256_hex(bytes)
            || start >= end
            || end > bytes.len()
            || !self.chunk_utf8.is_char_boundary(start)
            || !self.chunk_utf8.is_char_boundary(end)
        {
            return Err(MatchValidationError::InvalidEvidence(
                "chunk digest, byte length, or UTF-8 byte offset is invalid".into(),
            ));
        }
        self.chunk_utf8
            .get(start..end)
            .ok_or_else(|| MatchValidationError::InvalidEvidence("quote is not UTF-8".into()))
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum MatchValidationError {
    #[error("invalid evidence: {0}")]
    InvalidEvidence(String),
    #[error("invalid report: {0}")]
    InvalidReport(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceChunkArtifactV1 {
    pub id: Uuid,
    pub product_version_artifact_id: Uuid,
    pub document_id: Uuid,
    pub source_chunk_id: Uuid,
    pub frozen_document_display_name: String,
    pub chunk_utf8: String,
    pub chunk_sha256: String,
    pub chunk_byte_length: u64,
    pub retrieval_rank: u32,
    pub retrieval_raw_score: CanonicalDecimal,
    pub retrieval_contract_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceChunkProjectionV1 {
    pub id: Uuid,
    pub product_version_artifact_id: Uuid,
    pub document_id: Uuid,
    pub source_chunk_id: Uuid,
    pub frozen_document_display_name: String,
    pub chunk_sha256: String,
    pub chunk_byte_length: u64,
    pub retrieval_rank: u32,
    pub retrieval_raw_score: CanonicalDecimal,
    pub retrieval_contract_version: String,
}

impl From<&SourceChunkArtifactV1> for SourceChunkProjectionV1 {
    fn from(value: &SourceChunkArtifactV1) -> Self {
        Self {
            id: value.id,
            product_version_artifact_id: value.product_version_artifact_id,
            document_id: value.document_id,
            source_chunk_id: value.source_chunk_id,
            frozen_document_display_name: value.frozen_document_display_name.clone(),
            chunk_sha256: value.chunk_sha256.clone(),
            chunk_byte_length: value.chunk_byte_length,
            retrieval_rank: value.retrieval_rank,
            retrieval_raw_score: value.retrieval_raw_score,
            retrieval_contract_version: value.retrieval_contract_version.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceItemV1 {
    pub source_chunk_artifact_id: Uuid,
    pub document_id: Uuid,
    pub document_display_name: String,
    pub source_chunk_id: Uuid,
    pub source_chunk_sha256: String,
    pub quote: String,
    pub start_offset: u64,
    pub end_offset: u64,
    pub offset_unit: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceV1 {
    pub schema_version: u16,
    pub items: Vec<EvidenceItemV1>,
}

impl EvidenceV1 {
    pub fn canonical_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("EvidenceV1 has only infallible values")
    }

    pub fn sha256(&self) -> String {
        sha256_hex(&self.canonical_bytes())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum CandidateBusinessValue {
    Scored {
        value: CanonicalDecimal,
        source: String,
    },
    NotScored {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatchingCandidateV1 {
    pub id: Uuid,
    pub requirement_artifact_id: Uuid,
    pub product_version_artifact_id: Uuid,
    pub route_product_ordinal: u32,
    pub retrieval_rank: u32,
    pub retrieval_raw_score: CanonicalDecimal,
    pub candidate_identity_sha256: String,
    pub evidence_v1_sha256: String,
    pub evidence: EvidenceV1,
    pub support: VerifierSupport,
    pub business_value: CandidateBusinessValue,
    pub recommended: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequirementDecisionV1 {
    pub requirement_artifact_id: Uuid,
    pub final_support: VerifierSupport,
    pub system_decision: SystemDecision,
    pub quality_status: QualityStatus,
    pub reason_code: String,
    pub selected_candidate_artifact_id: Option<Uuid>,
    pub business_value: CandidateBusinessValue,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateGroupV1 {
    pub requirement_artifact_id: Uuid,
    pub support: VerifierSupport,
    pub candidate_artifact_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoverageCountsV1 {
    pub total: u32,
    pub eligible: u32,
    pub supported: u32,
    pub contradicted: u32,
    pub insufficient: u32,
    pub unresolved: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReportScoreV1 {
    Scored { value: CanonicalDecimal },
    NotScored { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatchingReportPayloadV1 {
    pub schema_version: u16,
    pub report_id: Uuid,
    pub manifest_id: Uuid,
    pub job_id: Uuid,
    pub route_id: Uuid,
    pub route: MatchRoute,
    pub generation: i64,
    pub mutation_watermark: i64,
    pub empty_disposition: Option<EmptyDisposition>,
    pub coverage: CoverageCountsV1,
    pub quality_status: QualityStatus,
    pub degraded: bool,
    pub reason_codes: Vec<String>,
    pub score: ReportScoreV1,
    pub requirement_decisions: Vec<RequirementDecisionV1>,
    pub candidates: Vec<MatchingCandidateV1>,
    pub candidate_groups: Vec<CandidateGroupV1>,
    pub source_artifacts: Vec<SourceChunkProjectionV1>,
    pub ai_run_id: Option<Uuid>,
    pub ai_span_id: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchingReportV1 {
    pub project_id: Uuid,
    pub payload: MatchingReportPayloadV1,
    pub source_artifacts: Vec<SourceChunkArtifactV1>,
}

impl MatchingReportV1 {
    pub fn canonical_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(&self.payload).expect("MatchingReportV1 has only infallible values")
    }

    pub fn content_sha256(&self) -> String {
        sha256_hex(&self.canonical_bytes())
    }

    pub fn validate_header(&self) -> Result<(), MatchValidationError> {
        let coverage = &self.payload.coverage;
        if coverage.total != coverage.eligible
            || coverage.total
                != coverage.supported
                    + coverage.contradicted
                    + coverage.insufficient
                    + coverage.unresolved
            || coverage.total != self.payload.requirement_decisions.len() as u32
            || self.payload.degraded != (self.payload.quality_status != QualityStatus::Pass)
        {
            return Err(MatchValidationError::InvalidReport(
                "coverage or quality header differs from decisions".into(),
            ));
        }
        let expected_quality = aggregate_report_quality(&self.payload.requirement_decisions);
        if self.payload.quality_status != expected_quality {
            return Err(MatchValidationError::InvalidReport(
                "report quality differs from decision aggregation".into(),
            ));
        }
        let expected_reasons = aggregate_report_reasons(
            &self.payload.requirement_decisions,
            self.payload.empty_disposition,
        );
        if self.payload.reason_codes != expected_reasons {
            return Err(MatchValidationError::InvalidReport(
                "report reasons differ from decision aggregation".into(),
            ));
        }
        Ok(())
    }
}

pub fn aggregate_report_quality(decisions: &[RequirementDecisionV1]) -> QualityStatus {
    if decisions
        .iter()
        .any(|row| row.quality_status == QualityStatus::Block)
    {
        QualityStatus::Block
    } else if decisions.is_empty()
        || decisions
            .iter()
            .any(|row| row.quality_status == QualityStatus::Review)
    {
        QualityStatus::Review
    } else {
        QualityStatus::Pass
    }
}

pub fn aggregate_report_reasons(
    decisions: &[RequirementDecisionV1],
    empty_disposition: Option<EmptyDisposition>,
) -> Vec<String> {
    let mut reasons = BTreeSet::from(["FROZEN_SCOPE".to_string()]);
    reasons.extend(decisions.iter().map(|row| row.reason_code.clone()));
    if decisions.is_empty() {
        reasons.insert("EMPTY_ROUTE".into());
        if empty_disposition == Some(EmptyDisposition::SkipUnit) {
            reasons.insert("SKIP_UNIT".into());
        }
    }
    reasons.into_iter().collect()
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub(crate) fn deterministic_uuid(tag: &str, identity: &[u8]) -> Uuid {
    let mut digest = Sha256::new();
    digest.update(tag.as_bytes());
    digest.update([0]);
    digest.update(identity);
    let mut bytes: [u8; 16] = digest.finalize()[..16]
        .try_into()
        .expect("SHA-256 prefix is 16 bytes");
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_utf8_slice_uses_bytes_not_characters() {
        let text = "甲A乙";
        let hit = FrozenRetrievedHitV1 {
            product_version_artifact_id: Uuid::new_v4(),
            route_product_ordinal: 0,
            document_id: Uuid::new_v4(),
            source_chunk_id: Uuid::new_v4(),
            frozen_document_display_name: "规格.pdf".into(),
            chunk_utf8: text.into(),
            chunk_sha256: sha256_hex(text.as_bytes()),
            chunk_byte_length: text.len() as u64,
            retrieval_rank: 1,
            retrieval_raw_score: CanonicalDecimal::new(Decimal::ONE),
            quote_start_offset: 3,
            quote_end_offset: 4,
            offset_unit: UTF8_BYTE_OFFSET_UNIT.into(),
            retrieval_contract_version: "knowledge-evidence-v1".into(),
        };
        assert_eq!(hit.quote().unwrap(), "A");
        let mut invalid = hit;
        invalid.quote_start_offset = 1;
        assert!(invalid.quote().is_err());
    }

    #[test]
    fn report_reasons_always_include_frozen_scope() {
        assert_eq!(
            aggregate_report_reasons(&[], Some(EmptyDisposition::SkipUnit)),
            vec!["EMPTY_ROUTE", "FROZEN_SCOPE", "SKIP_UNIT"]
        );
    }
}
