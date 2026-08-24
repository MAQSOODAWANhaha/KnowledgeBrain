//! Worker handler for one frozen matching route.

use super::{
    CandidateBusinessValue, EvidenceVerifier, LexicalEvidenceVerifier, MatchError,
    MatchingReportV1, MatchingWorkflow, QualityStatus, SystemDecision, VerifierSupport,
};
use runtime::BidMatchRouteV1Job;
use sqlx::PgPool;
use std::time::Duration;
use storage::bid_matching::{
    ClaimedMatchingRequest, EnvelopeSnapshotIdentity, PublishReceipt, PublishRouteV2,
    StagedCandidateGroupV1, StagedCandidateV1, StagedDecisionV1, StagedEvidenceV1,
    StagedSourceArtifactV1,
};
use tokio::sync::watch;
use uuid::Uuid;

pub async fn run_match_route_v1(pool: &PgPool, job: BidMatchRouteV1Job) -> Result<(), String> {
    job.validate()?;
    dispatch(pool, job, MatchingWorkflow::new(LexicalEvidenceVerifier)).await
}

async fn dispatch<V>(
    pool: &PgPool,
    job: BidMatchRouteV1Job,
    workflow: MatchingWorkflow<V>,
) -> Result<(), String>
where
    V: EvidenceVerifier,
{
    let snapshots = EnvelopeSnapshotIdentity {
        config_snapshot_id: job.config_snapshot_id,
        feature_snapshot_id: job.feature_snapshot_id,
        score_policy_snapshot_id: job.score_policy_snapshot_id,
        verifier_policy_snapshot_id: job.verifier_policy_snapshot_id,
    };
    let Some(claimed) = storage::bid_matching::claim_and_load(pool, job.job_id, snapshots)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Ok(());
    };

    let (stop_tx, stop_rx) = watch::channel(false);
    let heartbeat = tokio::spawn(heartbeat_loop(pool.clone(), claimed.clone(), stop_rx));
    let result = match workflow.execute(&claimed, Uuid::new_v4()).await {
        Ok(report) => publish_report(pool, &claimed, report).await,
        Err(error) if error.is_retryable() => retry(pool, &claimed, &error).await,
        Err(error) => fail(pool, &claimed, error.code(), error.detail()).await,
    };
    let _ = stop_tx.send(true);
    let heartbeat_result = heartbeat
        .await
        .map_err(|error| format!("matching heartbeat task failed: {error}"))?;
    result?;
    heartbeat_result
}

async fn heartbeat_loop(
    pool: PgPool,
    claimed: ClaimedMatchingRequest,
    mut stop: watch::Receiver<bool>,
) -> Result<(), String> {
    let interval_ms = (i64::from(claimed.claim.claim_lease_ms) / 3)
        .min(900_000 / 3)
        .clamp(1_000, 5 * 60 * 1000);
    loop {
        tokio::select! {
            changed = stop.changed() => {
                if changed.is_err() || *stop.borrow() {
                    return Ok(());
                }
            },
            _ = tokio::time::sleep(Duration::from_millis(interval_ms as u64)) => {
                match storage::bid_matching::heartbeat_claim(&pool, &claimed).await {
                    Ok(true) => {}
                    Ok(false) => return Err("matching claim lease lost".into()),
                    Err(error) => return Err(format!("matching heartbeat failed: {error}")),
                }
            }
        }
    }
}

async fn publish_report(
    pool: &PgPool,
    claimed: &ClaimedMatchingRequest,
    report: MatchingReportV1,
) -> Result<(), String> {
    let transport = report_transport(&report)?;
    match storage::bid_matching::publish_route(pool, claimed, transport)
        .await
        .map_err(|error| error.to_string())?
    {
        PublishReceipt::Committed { .. }
        | PublishReceipt::Replayed { .. }
        | PublishReceipt::Stale => Ok(()),
    }
}

fn report_transport(report: &MatchingReportV1) -> Result<PublishRouteV2, String> {
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
    Ok(PublishRouteV2 {
        report_id: report.payload.report_id,
        report_nonce: stable_child_id(report.payload.report_id, "nonce", 0),
        canonical_payload: report.canonical_bytes(),
        sources,
        candidates,
        evidences,
        decisions,
        candidate_groups,
        reason_codes: report.payload.reason_codes.clone(),
    })
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

async fn retry(
    pool: &PgPool,
    claimed: &ClaimedMatchingRequest,
    error: &MatchError,
) -> Result<(), String> {
    let updated = storage::bid_matching::retry_claim(pool, claimed, error.code(), error.detail())
        .await
        .map_err(|db| db.to_string())?;
    updated
        .then_some(())
        .ok_or_else(|| "matching retry claim fence lost".into())
}

async fn fail(
    pool: &PgPool,
    claimed: &ClaimedMatchingRequest,
    code: &str,
    detail: &str,
) -> Result<(), String> {
    let updated = storage::bid_matching::fail_claim(pool, claimed, code, detail)
        .await
        .map_err(|error| error.to_string())?;
    updated
        .then_some(())
        .ok_or_else(|| "matching failure claim fence lost".into())
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
        let transport = report_transport(&report).unwrap();
        assert!(
            !String::from_utf8(transport.canonical_payload)
                .unwrap()
                .contains("1.000000")
        );
        assert_eq!(CanonicalDecimal::new(Decimal::ONE).decimal(), Decimal::ONE);
    }
}
