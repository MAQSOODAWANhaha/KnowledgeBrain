//! Deterministic report builder over an already-frozen claim.

use super::{
    CandidateBusinessValue, CandidateGroupV1, CanonicalDecimal, CoverageCountsV1,
    EVIDENCE_SCHEMA_V1, EmptyDisposition, EvidenceItemV1, EvidenceV1, FrozenRetrievedHitV1,
    MATCHING_REPORT_SCHEMA_V1, MatchRoute, MatchingCandidateV1, MatchingReportPayloadV1,
    MatchingReportV1, QualityStatus, ReportScoreV1, RequirementDecisionV1, SourceChunkArtifactV1,
    SourceChunkProjectionV1, SystemDecision, UTF8_BYTE_OFFSET_UNIT, VerifierSupport,
    aggregate_report_quality, aggregate_report_reasons, deterministic_uuid, sha256_hex,
};
use async_trait::async_trait;
use rust_decimal::Decimal;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use storage::bid_matching::{ClaimedMatchingRequest, LoadedFrozenHit, LoadedRequirement};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct VerifyOutcome {
    pub support: VerifierSupport,
    /// A verifier may return a typed frozen value. Absence is never replaced by
    /// a policy-fabricated `1.0`.
    pub business_value: Option<CanonicalDecimal>,
}

#[derive(Debug, thiserror::Error)]
pub enum MatchError {
    #[error("{code}: {detail}")]
    Retryable { code: &'static str, detail: String },
    #[error("{code}: {detail}")]
    Fatal { code: &'static str, detail: String },
}

impl MatchError {
    pub fn retryable(code: &'static str, detail: impl Into<String>) -> Self {
        Self::Retryable {
            code,
            detail: bounded(detail.into()),
        }
    }

    pub fn fatal(code: &'static str, detail: impl Into<String>) -> Self {
        Self::Fatal {
            code,
            detail: bounded(detail.into()),
        }
    }

    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Retryable { .. })
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::Retryable { code, .. } | Self::Fatal { code, .. } => code,
        }
    }

    pub fn detail(&self) -> &str {
        match self {
            Self::Retryable { detail, .. } | Self::Fatal { detail, .. } => detail,
        }
    }
}

fn bounded(mut value: String) -> String {
    if value.is_empty() {
        value.push_str("matching_error");
    }
    value.truncate(1024);
    value
}

#[async_trait]
pub trait EvidenceVerifier: Send + Sync {
    async fn verify(
        &self,
        requirement: &LoadedRequirement,
        hit: &FrozenRetrievedHitV1,
    ) -> Result<VerifyOutcome, MatchError>;
}

/// Deterministic verifier used only by tests.
#[cfg(test)]
pub struct FakeVerifier;

#[cfg(test)]
#[async_trait]
impl EvidenceVerifier for FakeVerifier {
    async fn verify(
        &self,
        _requirement: &LoadedRequirement,
        _hit: &FrozenRetrievedHitV1,
    ) -> Result<VerifyOutcome, MatchError> {
        Ok(VerifyOutcome {
            support: VerifierSupport::Supported,
            business_value: None,
        })
    }
}

/// Production V1 verifier. Retrieval and verification remain separate seams;
/// this implementation is deliberately deterministic and never reads storage.
pub struct LexicalEvidenceVerifier;

#[async_trait]
impl EvidenceVerifier for LexicalEvidenceVerifier {
    async fn verify(
        &self,
        requirement: &LoadedRequirement,
        hit: &FrozenRetrievedHitV1,
    ) -> Result<VerifyOutcome, MatchError> {
        let quote = hit
            .quote()
            .map_err(|error| MatchError::fatal("INVALID_EVIDENCE", error.to_string()))?;
        let support = if normalized_contains(&requirement.text, quote) {
            VerifierSupport::Supported
        } else {
            VerifierSupport::Insufficient
        };
        Ok(VerifyOutcome {
            support,
            business_value: None,
        })
    }
}

fn normalized_contains(requirement: &str, quote: &str) -> bool {
    let needle: String = requirement
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    let haystack: String = quote
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    !needle.is_empty() && haystack.contains(&needle)
}

pub struct MatchingWorkflow<V> {
    verifier: V,
}

impl<V> MatchingWorkflow<V>
where
    V: EvidenceVerifier,
{
    pub fn new(verifier: V) -> Self {
        Self { verifier }
    }

    pub async fn execute(
        &self,
        claimed: &ClaimedMatchingRequest,
        report_id: Uuid,
    ) -> Result<MatchingReportV1, MatchError> {
        let route = match claimed.route {
            storage::bid_matching::MatchRoute::Technical { unit_id } => {
                MatchRoute::Technical { unit_id }
            }
            storage::bid_matching::MatchRoute::Commercial => MatchRoute::Commercial,
        };
        let empty_disposition =
            claimed
                .requirements
                .is_empty()
                .then_some(if claimed.empty_policy == "skip_unit" {
                    EmptyDisposition::SkipUnit
                } else {
                    EmptyDisposition::ClearRoute
                });

        let requirements: HashMap<Uuid, &LoadedRequirement> = claimed
            .requirements
            .iter()
            .map(|row| (row.id, row))
            .collect();
        if requirements.len() != claimed.requirements.len() {
            return Err(MatchError::fatal(
                "INVALID_FROZEN_SCOPE",
                "duplicate requirement artifact identity",
            ));
        }

        let mut source_by_identity: BTreeMap<
            (Uuid, Uuid, Uuid, String, String, String),
            SourceChunkArtifactV1,
        > = BTreeMap::new();
        let mut candidates = Vec::new();
        let mut dedup = HashSet::new();
        for loaded in &claimed.frozen_hits {
            let Some(requirement) = requirements.get(&loaded.requirement_artifact_id).copied()
            else {
                return Err(MatchError::fatal(
                    "INVALID_FROZEN_SCOPE",
                    "hit requirement is outside the claimed route",
                ));
            };
            let hit = frozen_hit(loaded)?;
            let quote = hit
                .quote()
                .map_err(|error| MatchError::fatal("INVALID_EVIDENCE", error.to_string()))?;
            let dedup_key = (
                requirement.id,
                hit.product_version_artifact_id,
                hit.document_id,
                hit.source_chunk_id,
                hit.quote_start_offset,
                hit.quote_end_offset,
                hit.chunk_sha256.clone(),
            );
            if !dedup.insert(dedup_key) {
                continue;
            }
            let source_identity = (
                hit.product_version_artifact_id,
                hit.document_id,
                hit.source_chunk_id,
                hit.chunk_sha256.clone(),
                hit.frozen_document_display_name.clone(),
                hit.retrieval_contract_version.clone(),
            );
            let source_id = deterministic_uuid(
                "MatchingSourceChunkV1",
                format!(
                    "{}:{}:{}:{}:{}:{}:{}",
                    report_id,
                    source_identity.0,
                    source_identity.1,
                    source_identity.2,
                    source_identity.3,
                    source_identity.4,
                    source_identity.5
                )
                .as_bytes(),
            );
            source_by_identity
                .entry(source_identity)
                .or_insert_with(|| SourceChunkArtifactV1 {
                    id: source_id,
                    product_version_artifact_id: hit.product_version_artifact_id,
                    document_id: hit.document_id,
                    source_chunk_id: hit.source_chunk_id,
                    frozen_document_display_name: hit.frozen_document_display_name.clone(),
                    chunk_utf8: hit.chunk_utf8.clone(),
                    chunk_sha256: hit.chunk_sha256.clone(),
                    chunk_byte_length: hit.chunk_byte_length,
                    retrieval_rank: hit.retrieval_rank,
                    retrieval_raw_score: hit.retrieval_raw_score,
                    retrieval_contract_version: hit.retrieval_contract_version.clone(),
                });
            let evidence = EvidenceV1 {
                schema_version: EVIDENCE_SCHEMA_V1,
                items: vec![EvidenceItemV1 {
                    source_chunk_artifact_id: source_id,
                    document_id: hit.document_id,
                    document_display_name: hit.frozen_document_display_name.clone(),
                    source_chunk_id: hit.source_chunk_id,
                    source_chunk_sha256: hit.chunk_sha256.clone(),
                    quote: quote.to_string(),
                    start_offset: hit.quote_start_offset,
                    end_offset: hit.quote_end_offset,
                    offset_unit: UTF8_BYTE_OFFSET_UNIT.into(),
                }],
            };
            let evidence_sha256 = evidence.sha256();
            let candidate_identity_sha256 = sha256_hex(
                format!(
                    "CandidateV1\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}",
                    requirement.id,
                    hit.product_version_artifact_id,
                    hit.source_chunk_id,
                    hit.quote_start_offset,
                    hit.quote_end_offset
                )
                .as_bytes(),
            );
            let candidate_id = deterministic_uuid(
                "MatchingCandidateV1",
                format!("{candidate_identity_sha256}:{evidence_sha256}").as_bytes(),
            );
            let outcome = self.verifier.verify(requirement, &hit).await?;
            let business_value = outcome.business_value.map_or_else(
                || CandidateBusinessValue::NotScored {
                    reason: "NO_EVIDENCE".into(),
                },
                |value| CandidateBusinessValue::Scored {
                    value,
                    source: "verifier".into(),
                },
            );
            candidates.push(MatchingCandidateV1 {
                id: candidate_id,
                requirement_artifact_id: requirement.id,
                product_version_artifact_id: hit.product_version_artifact_id,
                route_product_ordinal: hit.route_product_ordinal,
                retrieval_rank: hit.retrieval_rank,
                retrieval_raw_score: hit.retrieval_raw_score,
                candidate_identity_sha256,
                evidence_v1_sha256: evidence_sha256,
                evidence,
                support: outcome.support,
                business_value,
                recommended: false,
            });
        }

        candidates.sort_by(candidate_order);
        let mut decisions = Vec::with_capacity(claimed.requirements.len());
        for requirement in &claimed.requirements {
            let rows: Vec<usize> = candidates
                .iter()
                .enumerate()
                .filter_map(|(index, row)| {
                    (row.requirement_artifact_id == requirement.id).then_some(index)
                })
                .collect();
            let best_support = rows
                .iter()
                .map(|index| candidates[*index].support)
                .max_by_key(|support| support.priority());
            let (final_support, system_decision, quality_status, reason_code, selected) =
                match best_support {
                    Some(VerifierSupport::Supported) => {
                        let selected_index = rows
                            .iter()
                            .copied()
                            .filter(|index| {
                                candidates[*index].support == VerifierSupport::Supported
                            })
                            .min_by(|left, right| {
                                candidate_recommendation_order(
                                    &candidates[*left],
                                    &candidates[*right],
                                )
                            })
                            .expect("supported set is non-empty");
                        candidates[selected_index].recommended = true;
                        (
                            VerifierSupport::Supported,
                            SystemDecision::Select,
                            QualityStatus::Pass,
                            "SUPPORTED",
                            Some(selected_index),
                        )
                    }
                    Some(VerifierSupport::Unresolved) => (
                        VerifierSupport::Unresolved,
                        SystemDecision::Review,
                        QualityStatus::Review,
                        "UNRESOLVED",
                        None,
                    ),
                    Some(VerifierSupport::Insufficient) => (
                        VerifierSupport::Insufficient,
                        SystemDecision::Review,
                        QualityStatus::Review,
                        "INSUFFICIENT",
                        None,
                    ),
                    Some(VerifierSupport::Contradicted) => (
                        VerifierSupport::Contradicted,
                        SystemDecision::Reject,
                        QualityStatus::Block,
                        "CONTRADICTED",
                        None,
                    ),
                    None => (
                        VerifierSupport::Insufficient,
                        SystemDecision::Review,
                        QualityStatus::Review,
                        "NO_EVIDENCE",
                        None,
                    ),
                };
            let selected_candidate_artifact_id = selected.map(|index| candidates[index].id);
            let business_value = selected.map_or_else(
                || CandidateBusinessValue::NotScored {
                    reason: "NO_EVIDENCE".into(),
                },
                |index| candidates[index].business_value.clone(),
            );
            decisions.push(RequirementDecisionV1 {
                requirement_artifact_id: requirement.id,
                final_support,
                system_decision,
                quality_status,
                reason_code: reason_code.into(),
                selected_candidate_artifact_id,
                business_value,
            });
        }

        let coverage = coverage(&decisions)?;
        let quality_status = aggregate_report_quality(&decisions);
        let reason_codes = aggregate_report_reasons(&decisions, empty_disposition);
        let mut candidate_groups = candidate_groups(&candidates);
        candidate_groups.sort_by(|left, right| {
            left.requirement_artifact_id
                .cmp(&right.requirement_artifact_id)
                .then(left.support.cmp(&right.support))
        });
        let mut source_artifacts: Vec<_> = source_by_identity.into_values().collect();
        source_artifacts.sort_by_key(|row| row.id);
        let source_projections = source_artifacts
            .iter()
            .map(SourceChunkProjectionV1::from)
            .collect();
        let report = MatchingReportV1 {
            project_id: claimed.project_id,
            payload: MatchingReportPayloadV1 {
                schema_version: MATCHING_REPORT_SCHEMA_V1,
                report_id,
                manifest_id: claimed.manifest_id,
                job_id: claimed.job_id,
                route_id: claimed.route_id,
                route,
                generation: claimed.generation,
                mutation_watermark: claimed.mutation_watermark,
                empty_disposition,
                coverage,
                quality_status,
                degraded: quality_status != QualityStatus::Pass,
                reason_codes,
                score: ReportScoreV1::NotScored {
                    reason: "NO_EVIDENCE".into(),
                },
                requirement_decisions: decisions,
                candidates,
                candidate_groups,
                source_artifacts: source_projections,
                ai_run_id: None,
                ai_span_id: None,
            },
            source_artifacts,
        };
        report
            .validate_header()
            .map_err(|error| MatchError::fatal("INVALID_REPORT", error.to_string()))?;
        Ok(report)
    }
}

fn frozen_hit(value: &LoadedFrozenHit) -> Result<FrozenRetrievedHitV1, MatchError> {
    let raw_score = value
        .retrieval_raw_score
        .parse::<Decimal>()
        .map_err(|_| MatchError::fatal("INVALID_EVIDENCE", "retrieval score is not decimal"))?;
    Ok(FrozenRetrievedHitV1 {
        product_version_artifact_id: value.product_version_artifact_id,
        route_product_ordinal: value.route_product_ordinal,
        document_id: value.document_id,
        source_chunk_id: value.source_chunk_id,
        frozen_document_display_name: value.frozen_document_display_name.clone(),
        chunk_utf8: value.chunk_utf8.clone(),
        chunk_sha256: value.chunk_sha256.clone(),
        chunk_byte_length: value.chunk_byte_length,
        retrieval_rank: value.retrieval_rank,
        retrieval_raw_score: CanonicalDecimal::new(raw_score),
        quote_start_offset: value.quote_start_offset,
        quote_end_offset: value.quote_end_offset,
        offset_unit: value.offset_unit.clone(),
        retrieval_contract_version: value.retrieval_contract_version.clone(),
    })
}

fn candidate_order(left: &MatchingCandidateV1, right: &MatchingCandidateV1) -> std::cmp::Ordering {
    left.requirement_artifact_id
        .cmp(&right.requirement_artifact_id)
        .then(candidate_recommendation_order(left, right))
        .then(left.id.cmp(&right.id))
}

fn candidate_recommendation_order(
    left: &MatchingCandidateV1,
    right: &MatchingCandidateV1,
) -> std::cmp::Ordering {
    left.route_product_ordinal
        .cmp(&right.route_product_ordinal)
        .then(left.retrieval_rank.cmp(&right.retrieval_rank))
        .then(
            left.candidate_identity_sha256
                .cmp(&right.candidate_identity_sha256),
        )
        .then(left.evidence_v1_sha256.cmp(&right.evidence_v1_sha256))
}

fn coverage(decisions: &[RequirementDecisionV1]) -> Result<CoverageCountsV1, MatchError> {
    let total = u32::try_from(decisions.len())
        .map_err(|_| MatchError::fatal("REPORT_QUOTA_EXCEEDED", "too many requirements"))?;
    let mut counts = CoverageCountsV1 {
        total,
        eligible: total,
        supported: 0,
        contradicted: 0,
        insufficient: 0,
        unresolved: 0,
    };
    for decision in decisions {
        match decision.final_support {
            VerifierSupport::Supported => counts.supported += 1,
            VerifierSupport::Contradicted => counts.contradicted += 1,
            VerifierSupport::Insufficient => counts.insufficient += 1,
            VerifierSupport::Unresolved => counts.unresolved += 1,
        }
    }
    Ok(counts)
}

fn candidate_groups(candidates: &[MatchingCandidateV1]) -> Vec<CandidateGroupV1> {
    let mut groups: BTreeMap<(Uuid, VerifierSupport), BTreeSet<Uuid>> = BTreeMap::new();
    for candidate in candidates {
        groups
            .entry((candidate.requirement_artifact_id, candidate.support))
            .or_default()
            .insert(candidate.id);
    }
    groups
        .into_iter()
        .map(
            |((requirement_artifact_id, support), ids)| CandidateGroupV1 {
                requirement_artifact_id,
                support,
                candidate_artifact_ids: ids.into_iter().collect(),
            },
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use storage::bid_matching::{MatchClaim, MatchRoute as StorageRoute};

    fn requirement(id: u128, ordinal: u32) -> LoadedRequirement {
        LoadedRequirement {
            id: Uuid::from_u128(id),
            ordinal,
            text: format!("requirement-{id}"),
            requirement_sha256: sha256_hex(format!("requirement-{id}").as_bytes()),
        }
    }

    fn hit(requirement_id: Uuid, ordinal: u32, rank: u32, suffix: u128) -> LoadedFrozenHit {
        let chunk = format!("evidence requirement-{}", requirement_id.as_u128());
        LoadedFrozenHit {
            requirement_artifact_id: requirement_id,
            product_version_artifact_id: Uuid::from_u128(100 + u128::from(ordinal)),
            route_product_ordinal: ordinal,
            document_id: Uuid::from_u128(200 + suffix),
            source_chunk_id: Uuid::from_u128(300 + suffix),
            frozen_document_display_name: "冻结手册.pdf".into(),
            chunk_sha256: sha256_hex(chunk.as_bytes()),
            chunk_byte_length: chunk.len() as u64,
            chunk_utf8: chunk,
            retrieval_rank: rank,
            retrieval_raw_score: "0.900000".into(),
            quote_start_offset: 0,
            quote_end_offset: format!("evidence requirement-{}", requirement_id.as_u128()).len()
                as u64,
            offset_unit: UTF8_BYTE_OFFSET_UNIT.into(),
            retrieval_contract_version: "knowledge-evidence-v1".into(),
        }
    }

    fn claim(
        requirements: Vec<LoadedRequirement>,
        hits: Vec<LoadedFrozenHit>,
    ) -> ClaimedMatchingRequest {
        ClaimedMatchingRequest {
            job_id: Uuid::from_u128(1),
            manifest_id: Uuid::from_u128(2),
            project_id: Uuid::from_u128(3),
            generation: 1,
            mutation_watermark: 1,
            route_id: Uuid::from_u128(4),
            route: StorageRoute::Technical {
                unit_id: Uuid::nil(),
            },
            empty_policy: "clear_route".into(),
            claim: MatchClaim {
                token: Uuid::from_u128(5),
                attempt: 1,
                claim_lease_ms: 300_000,
                lease_policy_generation: 1,
            },
            requirements,
            frozen_hits: hits,
        }
    }

    struct ScriptedVerifier(HashMap<Uuid, VerifierSupport>);

    #[async_trait]
    impl EvidenceVerifier for ScriptedVerifier {
        async fn verify(
            &self,
            _requirement: &LoadedRequirement,
            hit: &FrozenRetrievedHitV1,
        ) -> Result<VerifyOutcome, MatchError> {
            Ok(VerifyOutcome {
                support: self
                    .0
                    .get(&hit.source_chunk_id)
                    .copied()
                    .unwrap_or(VerifierSupport::Insufficient),
                business_value: None,
            })
        }
    }

    #[tokio::test]
    async fn aggregate_priority_and_recommended_tuple_are_exact() {
        let req = requirement(10, 0);
        let later_product = hit(req.id, 4, 1, 1);
        let earlier_product = hit(req.id, 1, 8, 2);
        let unresolved = hit(req.id, 0, 1, 3);
        let verifier = ScriptedVerifier(HashMap::from([
            (later_product.source_chunk_id, VerifierSupport::Supported),
            (earlier_product.source_chunk_id, VerifierSupport::Supported),
            (unresolved.source_chunk_id, VerifierSupport::Unresolved),
        ]));
        let report = MatchingWorkflow::new(verifier)
            .execute(
                &claim(vec![req], vec![later_product, earlier_product, unresolved]),
                Uuid::from_u128(9),
            )
            .await
            .unwrap();
        let decision = &report.payload.requirement_decisions[0];
        assert_eq!(decision.final_support, VerifierSupport::Supported);
        assert_eq!(decision.reason_code, "SUPPORTED");
        let recommended: Vec<_> = report
            .payload
            .candidates
            .iter()
            .filter(|row| row.recommended)
            .collect();
        assert_eq!(
            report
                .payload
                .candidates
                .iter()
                .filter(|row| row.support == VerifierSupport::Supported)
                .count(),
            2,
            "all supported candidates remain visible for a 1..N human selection"
        );
        assert_eq!(recommended.len(), 1);
        assert_eq!(recommended[0].route_product_ordinal, 1);
        assert_eq!(
            decision.selected_candidate_artifact_id,
            Some(recommended[0].id)
        );
        assert_eq!(report.payload.quality_status, QualityStatus::Pass);
        assert_eq!(
            report.payload.reason_codes,
            vec!["FROZEN_SCOPE", "SUPPORTED"]
        );
    }

    #[tokio::test]
    async fn select_and_reject_makes_report_block_and_degraded() {
        let first = requirement(10, 0);
        let second = requirement(11, 1);
        let first_hit = hit(first.id, 0, 1, 1);
        let second_hit = hit(second.id, 0, 1, 2);
        let verifier = ScriptedVerifier(HashMap::from([
            (first_hit.source_chunk_id, VerifierSupport::Supported),
            (second_hit.source_chunk_id, VerifierSupport::Contradicted),
        ]));
        let report = MatchingWorkflow::new(verifier)
            .execute(
                &claim(vec![first, second], vec![first_hit, second_hit]),
                Uuid::from_u128(9),
            )
            .await
            .unwrap();
        assert_eq!(report.payload.quality_status, QualityStatus::Block);
        assert!(report.payload.degraded);
        assert_eq!(report.payload.coverage.supported, 1);
        assert_eq!(report.payload.coverage.contradicted, 1);
    }

    #[tokio::test]
    async fn no_hit_is_insufficient_review_with_no_evidence_reason() {
        let report = MatchingWorkflow::new(FakeVerifier)
            .execute(&claim(vec![requirement(10, 0)], vec![]), Uuid::from_u128(9))
            .await
            .unwrap();
        let decision = &report.payload.requirement_decisions[0];
        assert_eq!(decision.final_support, VerifierSupport::Insufficient);
        assert_eq!(decision.reason_code, "NO_EVIDENCE");
        assert_eq!(report.payload.quality_status, QualityStatus::Review);
        assert_eq!(
            report.payload.reason_codes,
            vec!["FROZEN_SCOPE", "NO_EVIDENCE"]
        );
    }

    #[tokio::test]
    async fn report_quality_matrix_covers_empty_select_review_and_reject() {
        let empty = MatchingWorkflow::new(FakeVerifier)
            .execute(&claim(Vec::new(), Vec::new()), Uuid::from_u128(9))
            .await
            .unwrap();
        assert_eq!(empty.payload.quality_status, QualityStatus::Review);
        assert_eq!(empty.payload.coverage.total, 0);

        let first = requirement(10, 0);
        let second = requirement(11, 1);
        let first_hit = hit(first.id, 0, 1, 1);
        let second_hit = hit(second.id, 1, 1, 2);
        let all_select = MatchingWorkflow::new(FakeVerifier)
            .execute(
                &claim(
                    vec![first.clone(), second.clone()],
                    vec![first_hit.clone(), second_hit.clone()],
                ),
                Uuid::from_u128(9),
            )
            .await
            .unwrap();
        assert_eq!(all_select.payload.quality_status, QualityStatus::Pass);
        assert_eq!(all_select.payload.coverage.supported, 2);

        let select_review = MatchingWorkflow::new(ScriptedVerifier(HashMap::from([
            (first_hit.source_chunk_id, VerifierSupport::Supported),
            (second_hit.source_chunk_id, VerifierSupport::Unresolved),
        ])))
        .execute(
            &claim(
                vec![first.clone(), second.clone()],
                vec![first_hit.clone(), second_hit.clone()],
            ),
            Uuid::from_u128(9),
        )
        .await
        .unwrap();
        assert_eq!(select_review.payload.quality_status, QualityStatus::Review);

        let review_reject = MatchingWorkflow::new(ScriptedVerifier(HashMap::from([
            (first_hit.source_chunk_id, VerifierSupport::Insufficient),
            (second_hit.source_chunk_id, VerifierSupport::Contradicted),
        ])))
        .execute(
            &claim(vec![first, second], vec![first_hit, second_hit]),
            Uuid::from_u128(9),
        )
        .await
        .unwrap();
        assert_eq!(review_reject.payload.quality_status, QualityStatus::Block);
        assert_eq!(review_reject.payload.coverage.insufficient, 1);
        assert_eq!(review_reject.payload.coverage.contradicted, 1);
    }

    #[tokio::test]
    async fn frozen_source_identity_keeps_distinct_display_name_snapshots() {
        let first = requirement(10, 0);
        let second = requirement(11, 1);
        let first_hit = hit(first.id, 0, 1, 1);
        let mut renamed_hit = hit(second.id, 0, 1, 2);
        renamed_hit.document_id = first_hit.document_id;
        renamed_hit.source_chunk_id = first_hit.source_chunk_id;
        renamed_hit.chunk_utf8 = first_hit.chunk_utf8.clone();
        renamed_hit.chunk_sha256 = first_hit.chunk_sha256.clone();
        renamed_hit.chunk_byte_length = first_hit.chunk_byte_length;
        renamed_hit.quote_end_offset = first_hit.quote_end_offset;
        renamed_hit.frozen_document_display_name = "renamed-manual.pdf".into();

        let report = MatchingWorkflow::new(FakeVerifier)
            .execute(
                &claim(vec![first, second], vec![first_hit, renamed_hit]),
                Uuid::from_u128(9),
            )
            .await
            .unwrap();
        assert_eq!(report.source_artifacts.len(), 2);
    }

    #[tokio::test]
    async fn frozen_source_artifact_identity_is_scoped_to_its_report() {
        let requirement = requirement(10, 0);
        let frozen_hit = hit(requirement.id, 0, 1, 1);
        let claimed = claim(vec![requirement], vec![frozen_hit]);

        let first = MatchingWorkflow::new(FakeVerifier)
            .execute(&claimed, Uuid::from_u128(9))
            .await
            .unwrap();
        let second = MatchingWorkflow::new(FakeVerifier)
            .execute(&claimed, Uuid::from_u128(10))
            .await
            .unwrap();

        assert_ne!(first.source_artifacts[0].id, second.source_artifacts[0].id);
    }

    #[tokio::test]
    async fn report_bytes_use_decimal_strings_and_are_stable() {
        let req = requirement(10, 0);
        let frozen = hit(req.id, 0, 1, 1);
        let claimed = claim(vec![req], vec![frozen]);
        let first = MatchingWorkflow::new(FakeVerifier)
            .execute(&claimed, Uuid::from_u128(9))
            .await
            .unwrap();
        let second = MatchingWorkflow::new(FakeVerifier)
            .execute(&claimed, Uuid::from_u128(9))
            .await
            .unwrap();
        assert_eq!(first.canonical_bytes(), second.canonical_bytes());
        let text = String::from_utf8(first.canonical_bytes()).unwrap();
        assert!(text.contains("\"retrieval_raw_score\":\"0.900000\""));
        assert_eq!(first.content_sha256(), second.content_sha256());
    }
}
