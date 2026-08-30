use sqlx::{PgPool, Row};
use std::time::Duration;
use uuid::Uuid;

const RETENTION_LEASE_MS: i32 = 60_000;
const RETENTION_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetentionClaim {
    pub object_ref: String,
    pub digest: String,
    pub byte_length: i64,
    pub attempt: i32,
    pub claim_token: Uuid,
}

pub async fn stage_object_upload(
    pool: &PgPool,
    staging_id: Uuid,
    object_ref: &str,
    digest: &str,
    media_type: &str,
    byte_length: i64,
    actor_identity: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "SELECT kb_object_upload_stage(
            $1,$2::kb_object_ref,$3::kb_sha256,$4,$5,$6::kb_actor_identity
        )",
    )
    .bind(staging_id)
    .bind(object_ref)
    .bind(digest)
    .bind(media_type)
    .bind(byte_length)
    .bind(actor_identity)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn abandon_object_upload(
    pool: &PgPool,
    staging_id: Uuid,
    actor_identity: &str,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_object_upload_abandon($1,$2::kb_actor_identity)")
        .bind(staging_id)
        .bind(actor_identity)
        .fetch_one(pool)
        .await
}

pub async fn expire_object_uploads(pool: &PgPool) -> Result<i32, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_object_upload_expire()")
        .fetch_one(pool)
        .await
}

pub async fn register_knowledge_document_object(
    pool: &PgPool,
    document_id: Uuid,
    media_type: &str,
    actor_identity: &str,
    idempotency_key: &str,
) -> Result<String, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT kb_register_knowledge_document_object($1,$2,$3::kb_actor_identity,$4,$5)",
    )
    .bind(document_id)
    .bind(media_type)
    .bind(actor_identity)
    .bind(idempotency_key)
    .bind(Uuid::new_v4())
    .fetch_one(pool)
    .await
}

pub async fn release_knowledge_document_object(
    pool: &PgPool,
    document_id: Uuid,
    actor_identity: &str,
    idempotency_key: &str,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT kb_release_knowledge_document_object($1,$2::kb_actor_identity,$3,$4)",
    )
    .bind(document_id)
    .bind(actor_identity)
    .bind(idempotency_key)
    .bind(Uuid::new_v4())
    .fetch_one(pool)
    .await
}

async fn claim_retention(
    pool: &PgPool,
    worker_name: &str,
) -> Result<Option<RetentionClaim>, sqlx::Error> {
    let claim_token = Uuid::new_v4();
    let row = sqlx::query(
        "SELECT object_ref::text AS object_ref, digest::text AS digest, byte_length, attempt FROM kb_retention_claim($1,$2,$3)",
    )
        .bind(claim_token)
        .bind(worker_name)
        .bind(RETENTION_LEASE_MS)
        .fetch_optional(pool)
        .await?;
    row.map(|row| {
        Ok(RetentionClaim {
            object_ref: row.try_get("object_ref")?,
            digest: row.try_get("digest")?,
            byte_length: row.try_get("byte_length")?,
            attempt: row.try_get("attempt")?,
            claim_token,
        })
    })
    .transpose()
}

async fn delete_claimed_blob_with_heartbeat(
    pool: &PgPool,
    claim: &RetentionClaim,
) -> Result<Result<(), String>, sqlx::Error> {
    let deletion = crate::delete_claimed_blob(&claim.digest);
    tokio::pin!(deletion);
    let mut heartbeat = tokio::time::interval(RETENTION_HEARTBEAT_INTERVAL);
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    heartbeat.tick().await;

    loop {
        tokio::select! {
            result = &mut deletion => return Ok(result),
            _ = heartbeat.tick() => {
                let renewal: Result<bool, sqlx::Error> = sqlx::query_scalar(
                    "SELECT kb_retention_heartbeat($1::kb_object_ref,$2,$3)",
                )
                .bind(&claim.object_ref)
                .bind(claim.claim_token)
                .bind(RETENTION_LEASE_MS)
                .fetch_one(pool)
                .await;
                match renewal {
                    Ok(true) => {}
                    Ok(false) => {
                        let _ = deletion.await;
                        return Err(sqlx::Error::Protocol(
                            "retention heartbeat lost the current claim".into(),
                        ));
                    }
                    Err(error) => {
                        let _ = deletion.await;
                        return Err(error);
                    }
                }
            }
        }
    }
}

fn bounded_error_code(error: &str) -> &'static str {
    if error.contains("timeout") {
        "OBJECT_DELETE_TIMEOUT"
    } else if error.contains("s3") {
        "OBJECT_STORE_DELETE_FAILED"
    } else {
        "OBJECT_DELETE_FAILED"
    }
}

/// Finish one current claim. Keeping this whole future in an owned Tokio task
/// makes cancellation of the polling loop detach, rather than split, the
/// heartbeat/delete/receipt lifecycle.
async fn process_claimed_retention_item(
    pool: &PgPool,
    claim: &RetentionClaim,
) -> Result<bool, sqlx::Error> {
    match delete_claimed_blob_with_heartbeat(pool, claim).await? {
        Ok(()) => {
            let completed: bool =
                sqlx::query_scalar("SELECT kb_retention_complete($1::kb_object_ref,$2)")
                    .bind(&claim.object_ref)
                    .bind(claim.claim_token)
                    .fetch_one(pool)
                    .await?;
            if !completed {
                return Err(sqlx::Error::Protocol(
                    "retention completion did not acknowledge the claim".into(),
                ));
            }
            Ok(true)
        }
        Err(error) => {
            let failed: bool =
                sqlx::query_scalar("SELECT kb_retention_fail($1::kb_object_ref,$2,$3)")
                    .bind(&claim.object_ref)
                    .bind(claim.claim_token)
                    .bind(bounded_error_code(&error))
                    .fetch_one(pool)
                    .await?;
            if !failed {
                return Err(sqlx::Error::Protocol(
                    "retention retry did not acknowledge the claim".into(),
                ));
            }
            Ok(true)
        }
    }
}

/// Claim and process at most one retention item. Physical deletion is hidden
/// inside this module and is impossible without a current database claim.
pub async fn process_one_retention_item(
    pool: &PgPool,
    worker_name: &str,
) -> Result<bool, sqlx::Error> {
    let Some(claim) = claim_retention(pool, worker_name).await? else {
        return Ok(false);
    };
    let claimed_pool = pool.clone();
    tokio::spawn(async move { process_claimed_retention_item(&claimed_pool, &claim).await })
        .await
        .map_err(|error| sqlx::Error::Protocol(format!("retention task join failed: {error}")))?
}
