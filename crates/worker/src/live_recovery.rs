//! Bounded producer and target-level dispatcher for open-operation recovery.

use std::{sync::Arc, time::Duration};

use runtime::{
    BidConversionV1Snapshots, BidExtractV1Snapshots, BidMatchRouteV1Snapshots,
    BidRenderSubmissionV1Snapshots, LiveRecoveryKind, LiveRecoveryObservationV1,
    LiveRecoveryObservedStage, LiveRecoveryOriginalSnapshotKind, LiveRecoveryOriginalSnapshotV1,
    LiveRecoverySnapshotRefsV1, LiveRecoveryTargetKind, LiveRecoveryV1Job,
};
use serde_json::json;
use sqlx::PgPool;
use storage::bid_recovery::{
    ClaimedRecovery, OriginalSnapshotKind, RecoveryAction, RecoveryCandidate, RecoveryKind,
    RecoveryTargetKind,
};
use tokio::sync::{Notify, oneshot};
use uuid::Uuid;

pub const DISCOVERY_INTERVAL: Duration = Duration::from_secs(5 * 60);
pub const DISCOVERY_LIMIT: u32 = 32;

pub async fn run_discovery_loop(pool: PgPool, stop: Arc<Notify>) -> Result<(), String> {
    loop {
        if let Err(error) = discover_once(&pool).await {
            tracing::warn!(%error, "bid live-recovery discovery failed");
        }
        tokio::select! {
            _ = stop.notified() => return Ok(()),
            _ = tokio::time::sleep(DISCOVERY_INTERVAL) => {}
        }
    }
}

pub async fn discover_once(pool: &PgPool) -> Result<usize, String> {
    let candidates = storage::bid_recovery::discover(pool, DISCOVERY_LIMIT)
        .await
        .map_err(|error| error.to_string())?;
    let mut enqueued = 0usize;
    let mut first_error = None;
    for candidate in candidates {
        let result = match job_from_candidate(&candidate) {
            Ok(job) => runtime::enqueue_live_recovery_v1(job).await,
            Err(error) => Err(error),
        };
        match result {
            Ok(Some(_)) => enqueued += 1,
            Ok(None) => {
                first_error.get_or_insert_with(|| "redis unavailable".to_string());
            }
            Err(error) => {
                tracing::warn!(
                    recovery_kind = ?candidate.recovery_kind,
                    durable_id = %candidate.durable_id,
                    %error,
                    "bid live-recovery candidate enqueue failed"
                );
                first_error.get_or_insert(error);
            }
        }
    }
    match first_error {
        Some(error) => Err(error),
        None => Ok(enqueued),
    }
}

pub async fn process(pool: &PgPool, job: LiveRecoveryV1Job) -> Result<(), String> {
    job.validate()?;
    let candidate = candidate_from_job(job);
    let claim_token = Uuid::new_v4();
    let Some(claim) =
        storage::bid_recovery::claim(pool, &candidate, claim_token, "system:live-recovery:v1")
            .await
            .map_err(|error| error.to_string())?
    else {
        return Ok(());
    };

    match dispatch_with_heartbeat(pool, &claim).await {
        Ok(enqueued_count) => {
            let receipt = json!({"enqueued_count": enqueued_count});
            storage::bid_recovery::complete(pool, &claim, &receipt)
                .await
                .map_err(|error| error.to_string())?;
            Ok(())
        }
        Err(DispatchError::Transport(error)) => {
            storage::bid_recovery::release(pool, &claim, "RECOVERY_ENQUEUE_FAILED")
                .await
                .map_err(|release_error| {
                    format!("{error}; live-recovery release failed: {release_error}")
                })?;
            Err(error)
        }
        Err(DispatchError::Terminal(error)) => {
            storage::bid_recovery::fail(pool, &claim, "RECOVERY_ACTION_INVALID")
                .await
                .map_err(|fail_error| {
                    format!("{error}; live-recovery terminal settlement failed: {fail_error}")
                })?;
            Ok(())
        }
        Err(DispatchError::LeaseLost(error)) => Err(error),
    }
}

#[derive(Debug)]
enum DispatchError {
    Transport(String),
    Terminal(String),
    LeaseLost(String),
}

async fn dispatch_with_heartbeat(
    pool: &PgPool,
    claim: &ClaimedRecovery,
) -> Result<usize, DispatchError> {
    let heartbeat_pool = pool.clone();
    let heartbeat_claim = claim.clone();
    let period = Duration::from_millis((i64::from(claim.claim_lease_ms) / 3).max(1_000) as u64);
    let (lost_tx, mut lost_rx) = oneshot::channel();
    let heartbeat_task = tokio::spawn(async move {
        loop {
            tokio::time::sleep(period).await;
            match storage::bid_recovery::heartbeat(&heartbeat_pool, &heartbeat_claim).await {
                Ok(true) => {}
                Ok(false) => {
                    let _ = lost_tx.send("live-recovery lease lost".to_string());
                    return;
                }
                Err(error) => {
                    let _ = lost_tx.send(format!("live-recovery heartbeat failed: {error}"));
                    return;
                }
            }
        }
    });

    let result = tokio::select! {
        result = dispatch_action(pool, claim) => result,
        reason = &mut lost_rx => Err(DispatchError::LeaseLost(
            reason.unwrap_or_else(|_| "live-recovery heartbeat stopped".to_string())
        )),
    };
    heartbeat_task.abort();
    result
}

async fn dispatch_action(pool: &PgPool, claim: &ClaimedRecovery) -> Result<usize, DispatchError> {
    match &claim.action {
        RecoveryAction::ScheduleMatchingManifest {
            schedule_intent_id,
            project_id,
            mutation_watermark,
            matching_config_snapshot_id,
            feature_snapshot_id,
            score_policy_snapshot_id,
            verifier_policy_snapshot_id,
        } => {
            let expected_snapshots = storage::bid_matching::EnvelopeSnapshotIdentity {
                config_snapshot_id: *matching_config_snapshot_id,
                feature_snapshot_id: *feature_snapshot_id,
                score_policy_snapshot_id: *score_policy_snapshot_id,
                verifier_policy_snapshot_id: *verifier_policy_snapshot_id,
            };
            let receipt = storage::bid_matching::schedule_recovery_intent(
                pool,
                *schedule_intent_id,
                *project_id,
                *mutation_watermark,
                expected_snapshots,
                bid::matching_schedule_environment(),
                &storage::bid_matching::ScheduleMutationContext::system(),
            )
            .await
            .map_err(|error| DispatchError::Terminal(error.to_string()))?;
            let Some(receipt) = receipt else {
                return Err(DispatchError::Terminal(
                    "matching recovery intent changed".to_string(),
                ));
            };
            let mut enqueued = 0usize;
            for job in receipt.jobs {
                require_enqueue(
                    runtime::enqueue_bid_match_route_v1(
                        job.id,
                        runtime::BidMatchRouteV1Snapshots {
                            config_snapshot_id: job.snapshots.config_snapshot_id,
                            feature_snapshot_id: job.snapshots.feature_snapshot_id,
                            score_policy_snapshot_id: job.snapshots.score_policy_snapshot_id,
                            verifier_policy_snapshot_id: job.snapshots.verifier_policy_snapshot_id,
                        },
                        None,
                    )
                    .await,
                )?;
                enqueued += 1;
            }
            Ok(enqueued)
        }
        RecoveryAction::ReenqueueDocumentConversion { document_id, .. } => {
            let snapshots = BidConversionV1Snapshots {
                conversion_snapshot_id: snapshot_id(claim, OriginalSnapshotKind::ConversionConfig)?,
                feature_snapshot_id: snapshot_id(claim, OriginalSnapshotKind::Feature)?,
            };
            require_enqueue(
                runtime::enqueue_bid_convert_with_snapshots(*document_id, snapshots).await,
            )?;
            Ok(1)
        }
        RecoveryAction::ReenqueueExtractionTarget {
            target_id,
            project_id,
            document_id,
            ..
        } => {
            let snapshots = BidExtractV1Snapshots {
                target_config_snapshot_id: snapshot_id(claim, OriginalSnapshotKind::TargetConfig)?,
                feature_snapshot_id: snapshot_id(claim, OriginalSnapshotKind::Feature)?,
            };
            require_enqueue(
                runtime::enqueue_bid_extract_with_snapshots(
                    *target_id,
                    *project_id,
                    Some(*document_id),
                    snapshots,
                )
                .await,
            )?;
            Ok(1)
        }
        RecoveryAction::ReenqueueAttachmentPreparation { preparation_job_id } => {
            let snapshots = BidConversionV1Snapshots {
                conversion_snapshot_id: snapshot_id(claim, OriginalSnapshotKind::ConversionConfig)?,
                feature_snapshot_id: snapshot_id(claim, OriginalSnapshotKind::Feature)?,
            };
            require_enqueue(
                runtime::enqueue_bid_prepare_attachment_v1_with_snapshots(
                    *preparation_job_id,
                    snapshots,
                )
                .await,
            )?;
            Ok(1)
        }
        RecoveryAction::ReenqueueSubmissionRender { render_job_id } => {
            let snapshots = BidRenderSubmissionV1Snapshots {
                submission_render_job_snapshot_id: snapshot_id(
                    claim,
                    OriginalSnapshotKind::SubmissionRenderJob,
                )?,
            };
            require_enqueue(
                runtime::enqueue_bid_render_submission_v1_with_snapshots(*render_job_id, snapshots)
                    .await,
            )?;
            Ok(1)
        }
        RecoveryAction::ReenqueueMatchingJob {
            job_id,
            matching_config_snapshot_id,
            feature_snapshot_id,
            score_policy_snapshot_id,
            verifier_policy_snapshot_id,
        } => {
            require_enqueue(
                runtime::enqueue_bid_match_route_v1(
                    *job_id,
                    BidMatchRouteV1Snapshots {
                        config_snapshot_id: *matching_config_snapshot_id,
                        feature_snapshot_id: *feature_snapshot_id,
                        score_policy_snapshot_id: *score_policy_snapshot_id,
                        verifier_policy_snapshot_id: *verifier_policy_snapshot_id,
                    },
                    None,
                )
                .await,
            )?;
            Ok(1)
        }
    }
}

fn require_enqueue(result: Result<Option<String>, String>) -> Result<(), DispatchError> {
    match result {
        Ok(Some(_)) => Ok(()),
        Ok(None) => Err(DispatchError::Transport("redis unavailable".to_string())),
        Err(error) => Err(DispatchError::Transport(error)),
    }
}

fn snapshot_id(claim: &ClaimedRecovery, kind: OriginalSnapshotKind) -> Result<Uuid, DispatchError> {
    claim
        .candidate
        .original_snapshots
        .iter()
        .find(|snapshot| snapshot.snapshot_kind == kind)
        .map(|snapshot| snapshot.snapshot_id)
        .ok_or_else(|| DispatchError::Terminal(format!("missing {kind:?} snapshot")))
}

fn job_from_candidate(candidate: &RecoveryCandidate) -> Result<LiveRecoveryV1Job, String> {
    LiveRecoveryV1Job::new(
        map_recovery_kind(candidate.recovery_kind),
        map_target_kind(candidate.target_kind),
        candidate.durable_id,
        LiveRecoveryObservationV1 {
            generation: candidate.generation,
            watermark: candidate.observed_watermark,
            stage: map_observed_stage(&candidate.observed_stage)?,
            heartbeat_at: candidate.observed_heartbeat_at,
            owner_token: candidate.observed_owner_token,
            attempt: candidate.observed_attempt,
        },
        candidate.recovery_epoch,
        LiveRecoverySnapshotRefsV1 {
            recovery_policy_snapshot_id: candidate.recovery_policy_snapshot_id,
            feature_snapshot_id: candidate.feature_snapshot_id,
            original_snapshots: candidate
                .original_snapshots
                .iter()
                .map(|snapshot| LiveRecoveryOriginalSnapshotV1 {
                    snapshot_kind: map_snapshot_kind(snapshot.snapshot_kind),
                    snapshot_id: snapshot.snapshot_id,
                })
                .collect(),
        },
    )
}

fn candidate_from_job(job: LiveRecoveryV1Job) -> RecoveryCandidate {
    RecoveryCandidate {
        recovery_kind: match job.recovery_kind {
            LiveRecoveryKind::DirtyManifest => RecoveryKind::DirtyManifest,
            LiveRecoveryKind::OrphanTarget => RecoveryKind::OrphanTarget,
            LiveRecoveryKind::OrphanMatchJob => RecoveryKind::OrphanMatchJob,
        },
        target_kind: match job.target_kind {
            LiveRecoveryTargetKind::MatchingManifest => RecoveryTargetKind::MatchingManifest,
            LiveRecoveryTargetKind::DocumentConversion => RecoveryTargetKind::DocumentConversion,
            LiveRecoveryTargetKind::ExtractionTarget => RecoveryTargetKind::ExtractionTarget,
            LiveRecoveryTargetKind::AttachmentPreparation => {
                RecoveryTargetKind::AttachmentPreparation
            }
            LiveRecoveryTargetKind::SubmissionRender => RecoveryTargetKind::SubmissionRender,
            LiveRecoveryTargetKind::MatchingJob => RecoveryTargetKind::MatchingJob,
        },
        durable_id: job.durable_id,
        generation: job.generation,
        observed_watermark: job.observed_watermark,
        observed_stage: match job.observed_stage {
            LiveRecoveryObservedStage::Dirty => "dirty",
            LiveRecoveryObservedStage::Pending => "pending",
            LiveRecoveryObservedStage::Processing => "processing",
            LiveRecoveryObservedStage::Running => "running",
            LiveRecoveryObservedStage::Publishing => "publishing",
            LiveRecoveryObservedStage::Terminal => "terminal",
        }
        .to_string(),
        observed_heartbeat_at: job.observed_heartbeat_at,
        observed_owner_token: job.observed_owner_token,
        observed_attempt: job.observed_attempt,
        recovery_epoch: job.recovery_epoch,
        recovery_policy_snapshot_id: job.recovery_policy_snapshot_id,
        feature_snapshot_id: job.feature_snapshot_id,
        original_snapshots: job
            .original_snapshots
            .into_iter()
            .map(|snapshot| storage::bid_recovery::OriginalSnapshot {
                snapshot_kind: unmap_snapshot_kind(snapshot.snapshot_kind),
                snapshot_id: snapshot.snapshot_id,
            })
            .collect(),
    }
}

fn map_recovery_kind(kind: RecoveryKind) -> LiveRecoveryKind {
    match kind {
        RecoveryKind::DirtyManifest => LiveRecoveryKind::DirtyManifest,
        RecoveryKind::OrphanTarget => LiveRecoveryKind::OrphanTarget,
        RecoveryKind::OrphanMatchJob => LiveRecoveryKind::OrphanMatchJob,
    }
}

fn map_target_kind(kind: RecoveryTargetKind) -> LiveRecoveryTargetKind {
    match kind {
        RecoveryTargetKind::MatchingManifest => LiveRecoveryTargetKind::MatchingManifest,
        RecoveryTargetKind::DocumentConversion => LiveRecoveryTargetKind::DocumentConversion,
        RecoveryTargetKind::ExtractionTarget => LiveRecoveryTargetKind::ExtractionTarget,
        RecoveryTargetKind::AttachmentPreparation => LiveRecoveryTargetKind::AttachmentPreparation,
        RecoveryTargetKind::SubmissionRender => LiveRecoveryTargetKind::SubmissionRender,
        RecoveryTargetKind::MatchingJob => LiveRecoveryTargetKind::MatchingJob,
    }
}

fn map_observed_stage(stage: &str) -> Result<LiveRecoveryObservedStage, String> {
    match stage {
        "dirty" => Ok(LiveRecoveryObservedStage::Dirty),
        "pending" => Ok(LiveRecoveryObservedStage::Pending),
        "processing" => Ok(LiveRecoveryObservedStage::Processing),
        "running" => Ok(LiveRecoveryObservedStage::Running),
        "publishing" => Ok(LiveRecoveryObservedStage::Publishing),
        "terminal" => Ok(LiveRecoveryObservedStage::Terminal),
        _ => Err(format!("rejected live-recovery observed stage {stage}")),
    }
}

fn map_snapshot_kind(kind: OriginalSnapshotKind) -> LiveRecoveryOriginalSnapshotKind {
    match kind {
        OriginalSnapshotKind::ConversionConfig => {
            LiveRecoveryOriginalSnapshotKind::ConversionConfig
        }
        OriginalSnapshotKind::Feature => LiveRecoveryOriginalSnapshotKind::Feature,
        OriginalSnapshotKind::SourceArtifact => LiveRecoveryOriginalSnapshotKind::SourceArtifact,
        OriginalSnapshotKind::TargetConfig => LiveRecoveryOriginalSnapshotKind::TargetConfig,
        OriginalSnapshotKind::SubmissionRenderJob => {
            LiveRecoveryOriginalSnapshotKind::SubmissionRenderJob
        }
        OriginalSnapshotKind::MatchingManifest => {
            LiveRecoveryOriginalSnapshotKind::MatchingManifest
        }
        OriginalSnapshotKind::MatchingConfig => LiveRecoveryOriginalSnapshotKind::MatchingConfig,
        OriginalSnapshotKind::ScorePolicy => LiveRecoveryOriginalSnapshotKind::ScorePolicy,
        OriginalSnapshotKind::VerifierPolicy => LiveRecoveryOriginalSnapshotKind::VerifierPolicy,
    }
}

fn unmap_snapshot_kind(kind: LiveRecoveryOriginalSnapshotKind) -> OriginalSnapshotKind {
    match kind {
        LiveRecoveryOriginalSnapshotKind::ConversionConfig => {
            OriginalSnapshotKind::ConversionConfig
        }
        LiveRecoveryOriginalSnapshotKind::Feature => OriginalSnapshotKind::Feature,
        LiveRecoveryOriginalSnapshotKind::SourceArtifact => OriginalSnapshotKind::SourceArtifact,
        LiveRecoveryOriginalSnapshotKind::TargetConfig => OriginalSnapshotKind::TargetConfig,
        LiveRecoveryOriginalSnapshotKind::SubmissionRenderJob => {
            OriginalSnapshotKind::SubmissionRenderJob
        }
        LiveRecoveryOriginalSnapshotKind::MatchingManifest => {
            OriginalSnapshotKind::MatchingManifest
        }
        LiveRecoveryOriginalSnapshotKind::MatchingConfig => OriginalSnapshotKind::MatchingConfig,
        LiveRecoveryOriginalSnapshotKind::ScorePolicy => OriginalSnapshotKind::ScorePolicy,
        LiveRecoveryOriginalSnapshotKind::VerifierPolicy => OriginalSnapshotKind::VerifierPolicy,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use storage::bid_recovery::OriginalSnapshot;

    #[test]
    fn candidate_job_roundtrip_preserves_every_fence() {
        let candidate = RecoveryCandidate {
            recovery_kind: RecoveryKind::OrphanMatchJob,
            target_kind: RecoveryTargetKind::MatchingJob,
            durable_id: Uuid::from_u128(1),
            generation: 7,
            observed_watermark: 19,
            observed_stage: "running".to_string(),
            observed_heartbeat_at: Some("1970-01-01T00:00:00Z".parse().unwrap()),
            observed_owner_token: Some(Uuid::from_u128(2)),
            observed_attempt: Some(3),
            recovery_epoch: 11,
            recovery_policy_snapshot_id: Uuid::from_u128(4),
            feature_snapshot_id: Uuid::from_u128(5),
            original_snapshots: vec![
                snapshot(OriginalSnapshotKind::MatchingManifest, 6),
                snapshot(OriginalSnapshotKind::MatchingConfig, 7),
                snapshot(OriginalSnapshotKind::Feature, 8),
                snapshot(OriginalSnapshotKind::ScorePolicy, 9),
                snapshot(OriginalSnapshotKind::VerifierPolicy, 10),
            ],
        };

        let job = job_from_candidate(&candidate).unwrap();
        job.validate().unwrap();
        assert_eq!(candidate_from_job(job), candidate);
    }

    #[test]
    fn candidate_job_mapping_rejects_unknown_stage() {
        let candidate = RecoveryCandidate {
            recovery_kind: RecoveryKind::DirtyManifest,
            target_kind: RecoveryTargetKind::MatchingManifest,
            durable_id: Uuid::from_u128(1),
            generation: 1,
            observed_watermark: 1,
            observed_stage: "queued_somewhere".to_string(),
            observed_heartbeat_at: None,
            observed_owner_token: None,
            observed_attempt: None,
            recovery_epoch: 1,
            recovery_policy_snapshot_id: Uuid::from_u128(2),
            feature_snapshot_id: Uuid::from_u128(3),
            original_snapshots: vec![
                snapshot(OriginalSnapshotKind::MatchingConfig, 4),
                snapshot(OriginalSnapshotKind::Feature, 5),
                snapshot(OriginalSnapshotKind::ScorePolicy, 6),
                snapshot(OriginalSnapshotKind::VerifierPolicy, 7),
            ],
        };

        assert!(job_from_candidate(&candidate).is_err());
    }

    fn snapshot(snapshot_kind: OriginalSnapshotKind, id: u128) -> OriginalSnapshot {
        OriginalSnapshot {
            snapshot_kind,
            snapshot_id: Uuid::from_u128(id),
        }
    }
}
