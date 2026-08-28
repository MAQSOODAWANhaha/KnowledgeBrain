//! Worker handler for one frozen matching route.

use super::{
    CandidateBusinessValue, EvidenceVerifier, LexicalEvidenceVerifier, MatchingReportV1,
    MatchingWorkflow, QualityStatus, SystemDecision, VerifierSupport,
};
use sqlx::PgPool;
use storage::bid_matching::{
    MatchingRequest, PublishReceipt, PublishRouteV2, StagedCandidateGroupV1, StagedCandidateV1,
    StagedDecisionV1, StagedEvidenceV1, StagedSourceArtifactV1,
};
use uuid::Uuid;

pub async fn run_match_route_v1(
    pool: &PgPool,
    job_id: Uuid,
    target_revision: i64,
) -> Result<(), String> {
    dispatch(
        pool,
        job_id,
        target_revision,
        MatchingWorkflow::new(LexicalEvidenceVerifier),
    )
    .await
}

async fn dispatch<V>(
    pool: &PgPool,
    job_id: Uuid,
    target_revision: i64,
    workflow: MatchingWorkflow<V>,
) -> Result<(), String>
where
    V: EvidenceVerifier,
{
    let Some(request) =
        storage::bid_matching::start_matching_execution(pool, job_id, target_revision)
            .await
            .map_err(|error| error.to_string())?
    else {
        return Ok(());
    };
    match workflow.execute(&request, Uuid::new_v4()).await {
        Ok(report) => publish_report(pool, &request, report).await,
        Err(error) => {
            settle(
                pool,
                &request,
                error.code(),
                error.detail(),
                error.is_retryable(),
            )
            .await
        }
    }
}

async fn publish_report(
    pool: &PgPool,
    request: &MatchingRequest,
    report: MatchingReportV1,
) -> Result<(), String> {
    let transport = report_transport(&report);
    let receipt = match storage::bid_matching::publish_route(pool, request, transport).await {
        Ok(receipt) => receipt,
        Err(error) => {
            return settle_retryable(
                pool,
                request,
                "MATCHING_REPORT_PUBLISH_FAILED",
                &error.to_string(),
            )
            .await;
        }
    };
    match receipt {
        PublishReceipt::Committed { .. }
        | PublishReceipt::Replayed { .. }
        | PublishReceipt::Stale => Ok(()),
    }
}

fn report_transport(report: &MatchingReportV1) -> PublishRouteV2 {
    let sources = report
        .source_artifacts
        .iter()
        .map(|row| StagedSourceArtifactV1 {
            id: row.id,
            product_version_artifact_id: row.product_version_artifact_id,
            document_id: row.document_id,
            source_chunk_id: row.source_chunk_id,
            frozen_document_display_name: row.frozen_document_display_name.clone(),
            chunk_utf8: row.chunk_utf8.clone(),
            chunk_sha256: row.chunk_sha256.clone(),
            chunk_byte_length: row.chunk_byte_length,
            retrieval_rank: row.retrieval_rank,
            retrieval_raw_score: format!("{:.6}", row.retrieval_raw_score.decimal()),
            retrieval_contract_version: row.retrieval_contract_version.clone(),
        })
        .collect::<Vec<_>>();
    let mut evidences = Vec::new();
    let mut candidates = Vec::new();
    for candidate in &report.payload.candidates {
        let (business_value_status, business_value) = match &candidate.business_value {
            CandidateBusinessValue::Scored { value, .. } => (
                "scored".to_string(),
                Some(format!("{:.6}", value.decimal())),
            ),
            CandidateBusinessValue::NotScored { .. } => ("not_scored".to_string(), None),
        };
        candidates.push(StagedCandidateV1 {
            id: candidate.id,
            requirement_artifact_id: candidate.requirement_artifact_id,
            product_version_artifact_id: candidate.product_version_artifact_id,
            route_product_ordinal: candidate.route_product_ordinal,
            retrieval_rank: candidate.retrieval_rank,
            retrieval_raw_score: format!("{:.6}", candidate.retrieval_raw_score.decimal()),
            candidate_identity_sha256: candidate.candidate_identity_sha256.clone(),
            evidence_v1_sha256: candidate.evidence_v1_sha256.clone(),
            support: support(candidate.support).into(),
            business_value_status,
            business_value,
            recommended: candidate.recommended,
        });
        for (ordinal, evidence) in candidate.evidence.items.iter().enumerate() {
            evidences.push(StagedEvidenceV1 {
                id: stable_child_id(candidate.id, "evidence", ordinal),
                candidate_artifact_id: candidate.id,
                source_chunk_artifact_id: evidence.source_chunk_artifact_id,
                document_id: evidence.document_id,
                document_display_name: evidence.document_display_name.clone(),
                source_chunk_id: evidence.source_chunk_id,
                source_chunk_sha256: evidence.source_chunk_sha256.clone(),
                quote: evidence.quote.clone(),
                start_offset: evidence.start_offset,
                end_offset: evidence.end_offset,
                offset_unit: evidence.offset_unit.clone(),
                ordinal: ordinal as u32,
            });
        }
    }
    let decisions = report
        .payload
        .requirement_decisions
        .iter()
        .enumerate()
        .map(|(ordinal, row)| StagedDecisionV1 {
            id: stable_child_id(report.payload.report_id, "decision", ordinal),
            requirement_artifact_id: row.requirement_artifact_id,
            final_support: support(row.final_support).into(),
            system_decision: decision(row.system_decision).into(),
            quality_status: quality(row.quality_status).into(),
            reason_code: row.reason_code.clone(),
            selected_candidate_artifact_id: row.selected_candidate_artifact_id,
            ordinal: ordinal as u32,
        })
        .collect();
    let candidate_groups = report
        .payload
        .candidate_groups
        .iter()
        .enumerate()
        .map(|(ordinal, row)| {
            let canonical_payload = serde_json::to_vec(row).expect("candidate group serializes");
            StagedCandidateGroupV1 {
                id: stable_child_id(report.payload.report_id, "candidate-group", ordinal),
                requirement_artifact_id: row.requirement_artifact_id,
                support: support(row.support).into(),
                ordinal: ordinal as u32,
                content_sha256: super::sha256_hex(&canonical_payload),
                canonical_payload,
            }
        })
        .collect();
    PublishRouteV2 {
        report_id: report.payload.report_id,
        report_nonce: stable_child_id(report.payload.report_id, "nonce", 0),
        canonical_payload: report.canonical_bytes(),
        sources,
        candidates,
        evidences,
        decisions,
        candidate_groups,
        reason_codes: report.payload.reason_codes.clone(),
    }
}

fn stable_child_id(parent: Uuid, tag: &str, ordinal: usize) -> Uuid {
    super::deterministic_uuid(tag, format!("{parent}:{ordinal}").as_bytes())
}

fn support(value: VerifierSupport) -> &'static str {
    match value {
        VerifierSupport::Supported => "supported",
        VerifierSupport::Unresolved => "unresolved",
        VerifierSupport::Insufficient => "insufficient",
        VerifierSupport::Contradicted => "contradicted",
    }
}

fn decision(value: SystemDecision) -> &'static str {
    match value {
        SystemDecision::Select => "select",
        SystemDecision::Review => "review",
        SystemDecision::Reject => "reject",
    }
}

fn quality(value: QualityStatus) -> &'static str {
    match value {
        QualityStatus::Pass => "pass",
        QualityStatus::Review => "review",
        QualityStatus::Block => "block",
    }
}

async fn settle_retryable(
    pool: &PgPool,
    request: &MatchingRequest,
    code: &str,
    detail: &str,
) -> Result<(), String> {
    let updated = storage::bid_matching::fail_matching(pool, request, code, detail, true)
        .await
        .map_err(|db| db.to_string())?;
    retryable_settlement_result(updated, code, detail)
}

fn retryable_settlement_result(updated: bool, code: &str, detail: &str) -> Result<(), String> {
    if updated {
        Err(format!("{code}: {detail}"))
    } else {
        Ok(())
    }
}

async fn settle(
    pool: &PgPool,
    request: &MatchingRequest,
    code: &str,
    detail: &str,
    retryable: bool,
) -> Result<(), String> {
    let updated = storage::bid_matching::fail_matching(pool, request, code, detail, retryable)
        .await
        .map_err(|error| error.to_string())?;
    if retryable {
        retryable_settlement_result(updated, code, detail)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matching::{
        CandidateGroupV1, CanonicalDecimal, CoverageCountsV1, MatchRoute, MatchingReportPayloadV1,
        ReportScoreV1,
    };
    use rust_decimal::Decimal;

    #[test]
    fn transport_does_not_turn_decimals_into_json_numbers() {
        let report = MatchingReportV1 {
            project_id: Uuid::from_u128(1),
            source_artifacts: Vec::new(),
            payload: MatchingReportPayloadV1 {
                schema_version: 1,
                report_id: Uuid::from_u128(2),
                manifest_id: Uuid::from_u128(3),
                job_id: Uuid::from_u128(4),
                route_id: Uuid::from_u128(5),
                route: MatchRoute::Commercial,
                generation: 1,
                mutation_watermark: 1,
                empty_disposition: None,
                coverage: CoverageCountsV1 {
                    total: 0,
                    eligible: 0,
                    supported: 0,
                    contradicted: 0,
                    insufficient: 0,
                    unresolved: 0,
                },
                quality_status: QualityStatus::Review,
                degraded: true,
                reason_codes: vec!["EMPTY_ROUTE".into(), "FROZEN_SCOPE".into()],
                score: ReportScoreV1::NotScored {
                    reason: "NO_EVIDENCE".into(),
                },
                requirement_decisions: Vec::new(),
                candidates: Vec::new(),
                candidate_groups: Vec::<CandidateGroupV1>::new(),
                source_artifacts: Vec::new(),
                ai_run_id: None,
                ai_span_id: None,
            },
        };
        let transport = report_transport(&report);
        assert!(
            !String::from_utf8(transport.canonical_payload)
                .unwrap()
                .contains("1.000000")
        );
        assert_eq!(CanonicalDecimal::new(Decimal::ONE).decimal(), Decimal::ONE);
    }

    #[test]
    fn retryable_settlement_retries_only_after_releasing_the_claim() {
        assert_eq!(
            retryable_settlement_result(true, "MATCHING_PUBLISH_FAILED", "database unavailable"),
            Err("MATCHING_PUBLISH_FAILED: database unavailable".into())
        );
        assert_eq!(
            retryable_settlement_result(
                false,
                "MATCHING_PUBLISH_FAILED",
                "matching claim fence lost"
            ),
            Ok(())
        );
    }
}
