mod support;

use sqlx::{PgPool, Row};
use storage::bid_recovery::{OriginalSnapshotKind, RecoveryKind, RecoveryTargetKind};
use uuid::Uuid;

#[tokio::test]
async fn live_recovery_is_bounded_fenced_and_redeliverable() {
    let Some(pool) = support::connect_postgres_contract("LiveRecoveryV1").await else {
        return;
    };
    let ready: bool = sqlx::query_scalar(
        "SELECT to_regprocedure('kb_bid_live_recovery_discover(integer)') IS NOT NULL",
    )
    .fetch_one(&pool)
    .await
    .expect("probe live recovery schema");
    if !support::require_final_schema("LiveRecoveryV1", ready) {
        return;
    }

    let user_id = Uuid::new_v4();
    let project_id = Uuid::new_v4();
    sqlx::query("INSERT INTO users(id,email) VALUES($1,$2)")
        .bind(user_id)
        .bind(format!("{user_id}@live-recovery.test"))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO bid_projects(
           id,title,owner_user_id,ends_at,fact_sha256,ceiling_identity_sha256,created_by
         ) VALUES($1,'Live recovery contract',$2,clock_timestamp()+interval '1 day',
           repeat('0',64),repeat('0',64),$3)",
    )
    .bind(project_id)
    .bind(user_id)
    .bind(format!("user:{user_id}"))
    .execute(&pool)
    .await
    .unwrap();

    let expired_document_id = Uuid::from_u128(0x100);
    for ordinal in 0..12u128 {
        let document_id = Uuid::from_u128(0x100 + ordinal);
        let digest = format!("{:064x}", 0x100 + ordinal);
        sqlx::query(
            "INSERT INTO bid_documents(
               id,project_id,file_name,media_type,byte_length,original_object_ref,
               original_sha256,parse_status
             ) VALUES($1,$2,$3,'application/pdf',1,$4,$5,$6)",
        )
        .bind(document_id)
        .bind(project_id)
        .bind(format!("{ordinal}.pdf"))
        .bind(format!("objects/{digest}"))
        .bind(&digest)
        .bind(if document_id == expired_document_id {
            "processing"
        } else {
            "pending"
        })
        .execute(&pool)
        .await
        .unwrap();
    }
    let expired_owner = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO bid_document_conversion_attempts(
           document_id,conversion_generation,attempt,claim_token,claimed_by,
           claim_lease_ms,claimed_at,heartbeat_at,status
         ) VALUES($1,1,1,$2,'worker:test',1000,
           clock_timestamp()-interval '10 minutes',
           clock_timestamp()-interval '10 minutes','running')",
    )
    .bind(expired_document_id)
    .bind(expired_owner)
    .execute(&pool)
    .await
    .unwrap();

    let first = storage::bid_recovery::discover(&pool, 128).await.unwrap();
    assert_eq!(
        first.len(),
        4,
        "per-kind concurrency must bound orphan-target discovery"
    );
    assert!(first.iter().all(|candidate| {
        candidate.recovery_kind == RecoveryKind::OrphanTarget
            && candidate.target_kind == RecoveryTargetKind::DocumentConversion
    }));
    let duplicate = storage::bid_recovery::discover(&pool, 128).await.unwrap();
    let identities = |candidates: &[storage::bid_recovery::RecoveryCandidate]| {
        candidates
            .iter()
            .map(|candidate| {
                (
                    candidate.recovery_kind,
                    candidate.durable_id,
                    candidate.generation,
                    candidate.recovery_epoch,
                )
            })
            .collect::<std::collections::HashSet<_>>()
    };
    assert_eq!(
        identities(&duplicate),
        identities(&first),
        "undelivered candidates must keep one identity"
    );

    let expired = first
        .iter()
        .find(|candidate| candidate.durable_id == expired_document_id)
        .expect("expired owner candidate")
        .clone();
    let mut tampered = first
        .iter()
        .find(|candidate| candidate.durable_id != expired_document_id)
        .expect("pending candidate")
        .clone();
    tampered.original_snapshots[0].snapshot_id = Uuid::new_v4();
    assert!(
        storage::bid_recovery::claim(&pool, &tampered, Uuid::new_v4(), "worker:tampered")
            .await
            .unwrap()
            .is_none()
    );
    let tampered_status: (String, Option<String>) = sqlx::query_as(
        "SELECT status,terminal_code FROM system_live_recovery_claims
          WHERE recovery_kind='orphan_target' AND durable_id=$1",
    )
    .bind(tampered.durable_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(tampered_status.0, "failed");
    assert_eq!(tampered_status.1.as_deref(), Some("SNAPSHOT_MISSING"));

    let first_token = Uuid::new_v4();
    let claim = storage::bid_recovery::claim(&pool, &expired, first_token, "worker:first")
        .await
        .unwrap()
        .expect("expired owner is claimable");
    assert!(
        storage::bid_recovery::claim(&pool, &expired, Uuid::new_v4(), "worker:second")
            .await
            .unwrap()
            .is_none(),
        "one durable identity must have one active owner"
    );
    assert!(
        storage::bid_recovery::heartbeat(&pool, &claim)
            .await
            .unwrap()
    );
    assert!(
        storage::bid_recovery::release(&pool, &claim, "REDIS_UNAVAILABLE")
            .await
            .unwrap()
    );
    let second_claim =
        storage::bid_recovery::claim(&pool, &expired, Uuid::new_v4(), "worker:redelivery")
            .await
            .unwrap()
            .expect("released action is redeliverable");
    assert_eq!(second_claim.attempt, claim.attempt + 1);
    assert!(
        storage::bid_recovery::complete(
            &pool,
            &second_claim,
            &serde_json::json!({"enqueued_count":1}),
        )
        .await
        .unwrap()
    );
    let (document_status, attempt_status): (String, String) = sqlx::query_as(
        "SELECT document.parse_status,attempt.status
           FROM bid_documents document
           JOIN bid_document_conversion_attempts attempt
             ON attempt.document_id=document.id AND attempt.attempt=1
          WHERE document.id=$1",
    )
    .bind(expired_document_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(document_status, "pending");
    assert_eq!(attempt_status, "reaped");

    let gate_candidate = first
        .iter()
        .find(|candidate| {
            candidate.durable_id != expired_document_id
                && candidate.durable_id != tampered.durable_id
        })
        .unwrap();
    let gate_claim =
        storage::bid_recovery::claim(&pool, gate_candidate, Uuid::new_v4(), "worker:gate-race")
            .await
            .unwrap()
            .expect("gate race candidate is claimable");
    transition_gate(&pool, "maintenance").await;
    assert!(
        !storage::bid_recovery::complete(
            &pool,
            &gate_claim,
            &serde_json::json!({"enqueued_count":1}),
        )
        .await
        .unwrap(),
        "an old epoch owner cannot commit after a gate transition"
    );
    transition_gate(&pool, "open").await;

    let dirty_project_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO bid_projects(
           id,title,owner_user_id,ends_at,fact_sha256,ceiling_identity_sha256,
           matching_mutation_watermark,created_by
         ) VALUES($1,'Dirty manifest contract',$2,clock_timestamp()+interval '1 day',
           repeat('0',64),repeat('0',64),1,$3)",
    )
    .bind(dirty_project_id)
    .bind(user_id)
    .bind(format!("user:{user_id}"))
    .execute(&pool)
    .await
    .unwrap();
    let intent_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO bid_matching_schedule_intents(
           id,project_id,generation,mutation_watermark,matching_config_snapshot_id,
           feature_snapshot_id,score_policy_snapshot_id,verifier_policy_snapshot_id
         ) VALUES($1,$2,1,1,kb_bid_current_operation_snapshot('matching_config'),
           kb_bid_current_operation_snapshot('feature'),
           kb_bid_current_operation_snapshot('score_policy'),
           kb_bid_current_operation_snapshot('verifier_policy'))",
    )
    .bind(intent_id)
    .bind(dirty_project_id)
    .execute(&pool)
    .await
    .unwrap();
    let candidates = storage::bid_recovery::discover(&pool, 128).await.unwrap();
    let dirty = candidates
        .iter()
        .find(|candidate| candidate.durable_id == intent_id)
        .expect("frozen dirty manifest intent is discoverable");
    assert_eq!(dirty.recovery_kind, RecoveryKind::DirtyManifest);
    assert_eq!(dirty.observed_stage, "dirty");
    assert_eq!(dirty.generation, 1);
    assert_eq!(dirty.observed_watermark, 1);
    assert_eq!(
        dirty
            .original_snapshots
            .iter()
            .map(|snapshot| snapshot.snapshot_kind)
            .collect::<std::collections::HashSet<_>>(),
        std::collections::HashSet::from([
            OriginalSnapshotKind::MatchingConfig,
            OriginalSnapshotKind::Feature,
            OriginalSnapshotKind::ScorePolicy,
            OriginalSnapshotKind::VerifierPolicy,
        ])
    );

    assert_runtime_worker_acl().await;
}

async fn transition_gate(pool: &PgPool, mode: &str) {
    let row = sqlx::query(
        "UPDATE application_maintenance_gate
            SET mode=$1,generation=generation+1,updated_by='system:first-launch',
                updated_at=clock_timestamp()
          WHERE singleton_key
          RETURNING generation",
    )
    .bind(mode)
    .fetch_one(pool)
    .await
    .unwrap();
    let generation: i64 = row.get("generation");
    assert!(generation > 0);
}

async fn assert_runtime_worker_acl() {
    let worker_url =
        std::env::var("KNOWLEDGEBRAIN_TEST_WORKER_DATABASE_URL").unwrap_or_else(|_| {
            "postgres://kb_runtime_worker:worker-test@127.0.0.1:55452/knowledgebrain".to_string()
        });
    let pool = PgPool::connect(&worker_url)
        .await
        .expect("connect runtime worker role");
    storage::bid_recovery::discover(&pool, 1)
        .await
        .expect("runtime worker may discover through the fenced function");
    assert!(
        sqlx::query("DELETE FROM system_live_recovery_claims")
            .execute(&pool)
            .await
            .is_err(),
        "runtime worker must not have direct recovery-ledger DML"
    );
    assert!(
        sqlx::query(
            "UPDATE application_maintenance_gate SET mode='maintenance' WHERE singleton_key"
        )
        .execute(&pool)
        .await
        .is_err(),
        "live recovery must not mutate the control plane"
    );
}
