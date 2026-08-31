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

/// Cancellation-safe best-effort cleanup for the small set of object uploads
/// staged by one publication attempt. Database expiry remains the final fallback.
pub struct StagedObjectCleanupGuard {
    pool: PgPool,
    actor: String,
    staging_ids: Vec<Uuid>,
}

impl StagedObjectCleanupGuard {
    pub fn new(pool: &PgPool, actor: &str) -> Self {
        Self {
            pool: pool.clone(),
            actor: actor.to_owned(),
            staging_ids: Vec::new(),
        }
    }

    /// Register before awaiting the stage/write future so cancellation cannot
    /// lose an upload that committed immediately before the future was dropped.
    pub fn register(&mut self, staging_id: Uuid) {
        if !self.staging_ids.contains(&staging_id) {
            self.staging_ids.push(staging_id);
        }
    }

    pub fn disarm(&mut self, staging_id: Uuid) {
        self.staging_ids.retain(|value| *value != staging_id);
    }

    pub fn disarm_all(&mut self) {
        self.staging_ids.clear();
    }

    #[cfg(test)]
    fn pending(&self) -> &[Uuid] {
        &self.staging_ids
    }
}

impl Drop for StagedObjectCleanupGuard {
    fn drop(&mut self) {
        if self.staging_ids.is_empty() {
            return;
        }
        let ids = std::mem::take(&mut self.staging_ids);
        let pool = self.pool.clone();
        let actor = self.actor.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                for staging_id in ids {
                    let _ = abandon_object_upload(&pool, staging_id, &actor).await;
                }
            });
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn staging_guard_registration_and_disarm_are_closed() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://unused:unused@127.0.0.1:1/unused")
            .unwrap();
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        let mut guard = StagedObjectCleanupGuard::new(&pool, "system:test");
        guard.register(first);
        guard.register(first);
        guard.register(second);
        assert_eq!(guard.pending(), &[first, second]);
        guard.disarm(first);
        assert_eq!(guard.pending(), &[second]);
        guard.disarm_all();
        assert!(guard.pending().is_empty());
    }
}
