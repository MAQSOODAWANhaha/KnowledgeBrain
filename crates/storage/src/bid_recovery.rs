//! Durable, target-level Bid live recovery.
//!
//! Callers discover bounded candidates, claim one exact observed identity,
//! enqueue the returned action, then complete or release the claim. All gate,
//! snapshot, generation, watermark, owner, lease and redelivery fencing stays
//! inside the PostgreSQL implementation.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryKind {
    DirtyManifest,
    OrphanTarget,
    OrphanMatchJob,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryTargetKind {
    MatchingManifest,
    DocumentConversion,
    ExtractionTarget,
    AttachmentPreparation,
    SubmissionRender,
    MatchingJob,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OriginalSnapshotKind {
    ConversionConfig,
    Feature,
    SourceArtifact,
    TargetConfig,
    SubmissionRenderJob,
    MatchingManifest,
    MatchingConfig,
    ScorePolicy,
    VerifierPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OriginalSnapshot {
    pub snapshot_kind: OriginalSnapshotKind,
    pub snapshot_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryCandidate {
    pub recovery_kind: RecoveryKind,
    pub target_kind: RecoveryTargetKind,
    pub durable_id: Uuid,
    pub generation: i64,
    pub observed_watermark: i64,
    pub observed_stage: String,
    pub observed_heartbeat_at: Option<DateTime<Utc>>,
    pub observed_owner_token: Option<Uuid>,
    pub observed_attempt: Option<i32>,
    pub recovery_epoch: i64,
    pub recovery_policy_snapshot_id: Uuid,
    pub feature_snapshot_id: Uuid,
    pub original_snapshots: Vec<OriginalSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RecoveryAction {
    ScheduleMatchingManifest {
        schedule_intent_id: Uuid,
        project_id: Uuid,
        mutation_watermark: i64,
        matching_config_snapshot_id: Uuid,
        feature_snapshot_id: Uuid,
        score_policy_snapshot_id: Uuid,
        verifier_policy_snapshot_id: Uuid,
    },
    ReenqueueDocumentConversion {
        document_id: Uuid,
        conversion_generation: i64,
    },
    ReenqueueExtractionTarget {
        target_id: Uuid,
        project_id: Uuid,
        document_id: Uuid,
        extraction_generation: i64,
    },
    ReenqueueAttachmentPreparation {
        preparation_job_id: Uuid,
    },
    ReenqueueSubmissionRender {
        render_job_id: Uuid,
    },
    ReenqueueMatchingJob {
        job_id: Uuid,
        matching_config_snapshot_id: Uuid,
        feature_snapshot_id: Uuid,
        score_policy_snapshot_id: Uuid,
        verifier_policy_snapshot_id: Uuid,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimedRecovery {
    pub candidate: RecoveryCandidate,
    pub claim_token: Uuid,
    pub attempt: i32,
    pub claim_lease_ms: i32,
    pub action: RecoveryAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConversionSnapshotRefs {
    pub conversion_snapshot_id: Uuid,
    pub feature_snapshot_id: Uuid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExtractionSnapshotRefs {
    pub target_config_snapshot_id: Uuid,
    pub feature_snapshot_id: Uuid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubmissionRenderSnapshotRefs {
    pub submission_render_job_snapshot_id: Uuid,
}

pub async fn conversion_snapshots(
    pool: &PgPool,
    document_id: Uuid,
) -> Result<Option<ConversionSnapshotRefs>, sqlx::Error> {
    sqlx::query_as::<_, (Uuid, Uuid)>(
        "SELECT conversion_snapshot_id,feature_snapshot_id
           FROM bid_documents WHERE id=$1",
    )
    .bind(document_id)
    .fetch_optional(pool)
    .await
    .map(|row| {
        row.map(
            |(conversion_snapshot_id, feature_snapshot_id)| ConversionSnapshotRefs {
                conversion_snapshot_id,
                feature_snapshot_id,
            },
        )
    })
}

pub async fn attachment_preparation_snapshots(
    pool: &PgPool,
    preparation_job_id: Uuid,
) -> Result<Option<ConversionSnapshotRefs>, sqlx::Error> {
    sqlx::query_as::<_, (Uuid, Uuid)>(
        "SELECT conversion_snapshot_id,feature_snapshot_id
           FROM bid_attachment_preparation_jobs WHERE id=$1",
    )
    .bind(preparation_job_id)
    .fetch_optional(pool)
    .await
    .map(|row| {
        row.map(
            |(conversion_snapshot_id, feature_snapshot_id)| ConversionSnapshotRefs {
                conversion_snapshot_id,
                feature_snapshot_id,
            },
        )
    })
}

pub async fn extraction_snapshots(
    pool: &PgPool,
    target_id: Uuid,
) -> Result<Option<ExtractionSnapshotRefs>, sqlx::Error> {
    sqlx::query_as::<_, (Uuid, Uuid)>(
        "SELECT target_config_snapshot_id,feature_snapshot_id
           FROM bid_extraction_targets WHERE id=$1",
    )
    .bind(target_id)
    .fetch_optional(pool)
    .await
    .map(|row| {
        row.map(
            |(target_config_snapshot_id, feature_snapshot_id)| ExtractionSnapshotRefs {
                target_config_snapshot_id,
                feature_snapshot_id,
            },
        )
    })
}

pub async fn submission_render_snapshots(
    pool: &PgPool,
    render_job_id: Uuid,
) -> Result<Option<SubmissionRenderSnapshotRefs>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT submission_render_job_snapshot_id
           FROM bid_submission_render_jobs WHERE id=$1",
    )
    .bind(render_job_id)
    .fetch_optional(pool)
    .await
    .map(|row| {
        row.map(
            |submission_render_job_snapshot_id| SubmissionRenderSnapshotRefs {
                submission_render_job_snapshot_id,
            },
        )
    })
}

pub async fn discover(pool: &PgPool, limit: u32) -> Result<Vec<RecoveryCandidate>, sqlx::Error> {
    let limit = i32::try_from(limit).map_err(|_| protocol("live recovery limit overflow"))?;
    let values: Vec<Value> =
        sqlx::query_scalar("SELECT candidate FROM kb_bid_live_recovery_discover($1) candidate")
            .bind(limit)
            .fetch_all(pool)
            .await?;
    values.into_iter().map(decode).collect()
}

pub async fn claim(
    pool: &PgPool,
    candidate: &RecoveryCandidate,
    claim_token: Uuid,
    claimed_by: &str,
) -> Result<Option<ClaimedRecovery>, sqlx::Error> {
    let candidate = serde_json::to_value(candidate)
        .map_err(|error| protocol(format!("live recovery candidate encode failed: {error}")))?;
    let value: Option<Value> = sqlx::query_scalar("SELECT kb_bid_live_recovery_claim($1,$2,$3)")
        .bind(candidate)
        .bind(claim_token)
        .bind(claimed_by)
        .fetch_one(pool)
        .await?;
    value.map(decode).transpose()
}

pub async fn heartbeat(pool: &PgPool, claim: &ClaimedRecovery) -> Result<bool, sqlx::Error> {
    let candidate = &claim.candidate;
    sqlx::query_scalar("SELECT kb_bid_live_recovery_heartbeat($1,$2,$3,$4,$5,$6)")
        .bind(kind_name(candidate.recovery_kind))
        .bind(candidate.durable_id)
        .bind(candidate.generation)
        .bind(candidate.recovery_epoch)
        .bind(claim.claim_token)
        .bind(claim.attempt)
        .fetch_one(pool)
        .await
}

pub async fn complete(
    pool: &PgPool,
    claim: &ClaimedRecovery,
    receipt: &Value,
) -> Result<bool, sqlx::Error> {
    let candidate = &claim.candidate;
    sqlx::query_scalar("SELECT kb_bid_live_recovery_complete($1,$2,$3,$4,$5,$6,$7)")
        .bind(kind_name(candidate.recovery_kind))
        .bind(candidate.durable_id)
        .bind(candidate.generation)
        .bind(candidate.recovery_epoch)
        .bind(claim.claim_token)
        .bind(claim.attempt)
        .bind(receipt)
        .fetch_one(pool)
        .await
}

/// Return a claimed action to `pending` after a transport enqueue failure.
/// The already-applied exact domain reap is retained, so redelivery cannot
/// resurrect or re-run the old owner.
pub async fn release(
    pool: &PgPool,
    claim: &ClaimedRecovery,
    error_code: &str,
) -> Result<bool, sqlx::Error> {
    let candidate = &claim.candidate;
    sqlx::query_scalar("SELECT kb_bid_live_recovery_release($1,$2,$3,$4,$5,$6,$7)")
        .bind(kind_name(candidate.recovery_kind))
        .bind(candidate.durable_id)
        .bind(candidate.generation)
        .bind(candidate.recovery_epoch)
        .bind(claim.claim_token)
        .bind(claim.attempt)
        .bind(error_code)
        .fetch_one(pool)
        .await
}

pub async fn fail(
    pool: &PgPool,
    claim: &ClaimedRecovery,
    error_code: &str,
) -> Result<bool, sqlx::Error> {
    let candidate = &claim.candidate;
    sqlx::query_scalar("SELECT kb_bid_live_recovery_fail($1,$2,$3,$4,$5,$6,$7)")
        .bind(kind_name(candidate.recovery_kind))
        .bind(candidate.durable_id)
        .bind(candidate.generation)
        .bind(candidate.recovery_epoch)
        .bind(claim.claim_token)
        .bind(claim.attempt)
        .bind(error_code)
        .fetch_one(pool)
        .await
}

fn kind_name(kind: RecoveryKind) -> &'static str {
    match kind {
        RecoveryKind::DirtyManifest => "dirty_manifest",
        RecoveryKind::OrphanTarget => "orphan_target",
        RecoveryKind::OrphanMatchJob => "orphan_match_job",
    }
}

fn decode<T: for<'de> Deserialize<'de>>(value: Value) -> Result<T, sqlx::Error> {
    serde_json::from_value(value)
        .map_err(|error| protocol(format!("live recovery response decode failed: {error}")))
}

fn protocol(message: impl Into<String>) -> sqlx::Error {
    sqlx::Error::Protocol(message.into())
}
