use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectDeletionIdentity {
    pub deletion_id: Uuid,
    pub object_ref: String,
    pub digest: String,
    pub byte_length: i64,
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
) -> Result<Option<ObjectDeletionIdentity>, sqlx::Error> {
    let value: Option<serde_json::Value> =
        sqlx::query_scalar("SELECT kb_object_upload_abandon($1,$2::kb_actor_identity)")
            .bind(staging_id)
            .bind(actor_identity)
            .fetch_one(pool)
            .await?;
    value
        .map(serde_json::from_value)
        .transpose()
        .map_err(|error| sqlx::Error::Decode(Box::new(error)))
}

pub async fn schedule_object_upload_cleanup(staging_id: Uuid) -> Result<(), String> {
    crate::enqueue_object_upload_expire(staging_id)
        .await?
        .map(|_| ())
        .ok_or_else(|| "Oxana Redis is not configured for upload cleanup".to_string())
}

pub async fn dispatch_object_deletion(
    _pool: &PgPool,
    deletion: ObjectDeletionIdentity,
) -> Result<(), String> {
    let queued = crate::enqueue_object_retention(crate::ObjectRetentionJob {
        deletion_id: deletion.deletion_id,
        object_ref: deletion.object_ref.clone(),
        digest: deletion.digest.clone(),
        byte_length: deletion.byte_length,
    })
    .await?;
    queued
        .map(|_| ())
        .ok_or_else(|| "Oxana Redis is not configured for object retention".to_string())
}

/// Shared ownership of staged uploads created by one handler. The handler's
/// supervisor retains this tracker and explicitly awaits cleanup after joining
/// cancelled work; expiry is only a fallback for process crashes.
#[derive(Clone)]
pub struct StagedObjectCleanupTracker {
    staging_ids: std::sync::Arc<std::sync::Mutex<Vec<Uuid>>>,
}

impl StagedObjectCleanupTracker {
    pub fn new(_pool: &PgPool, _actor: &str) -> Self {
        Self {
            staging_ids: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }

    pub fn guard(&self) -> StagedObjectCleanupGuard {
        StagedObjectCleanupGuard {
            tracker: self.clone(),
        }
    }

    pub fn register(&self, staging_id: Uuid) {
        let mut ids = self
            .staging_ids
            .lock()
            .expect("staged cleanup lock poisoned");
        if !ids.contains(&staging_id) {
            ids.push(staging_id);
        }
    }

    pub fn disarm(&self, staging_id: Uuid) {
        self.staging_ids
            .lock()
            .expect("staged cleanup lock poisoned")
            .retain(|value| *value != staging_id);
    }

    pub async fn cleanup_pending(&self) -> Result<(), String> {
        loop {
            let staging_id = self
                .staging_ids
                .lock()
                .map_err(|_| "staged cleanup lock poisoned".to_string())?
                .first()
                .copied();
            let Some(staging_id) = staging_id else {
                return Ok(());
            };
            schedule_object_upload_cleanup(staging_id)
                .await
                .map_err(|error| {
                    format!("staged object cleanup scheduling failed for {staging_id}: {error}")
                })?;
            let mut ids = self
                .staging_ids
                .lock()
                .map_err(|_| "staged cleanup lock poisoned".to_string())?;
            if ids.first() == Some(&staging_id) {
                ids.remove(0);
            } else {
                ids.retain(|value| *value != staging_id);
            }
        }
    }

    /// Recovery identities in the order in which handoff will be attempted.
    pub fn pending_staging_ids(&self) -> Vec<Uuid> {
        self.staging_ids
            .lock()
            .expect("staged cleanup lock poisoned")
            .clone()
    }

    pub fn pending_count(&self) -> usize {
        self.staging_ids
            .lock()
            .expect("staged cleanup lock poisoned")
            .len()
    }

    pub fn has_pending(&self) -> bool {
        self.pending_count() != 0
    }
}

/// Synchronous registration facade used inside a handler pipeline. Dropping it
/// never detaches work; the retained tracker is the only cleanup executor.
pub struct StagedObjectCleanupGuard {
    tracker: StagedObjectCleanupTracker,
}

impl StagedObjectCleanupGuard {
    pub fn new(pool: &PgPool, actor: &str) -> Self {
        StagedObjectCleanupTracker::new(pool, actor).guard()
    }

    pub fn tracker(&self) -> StagedObjectCleanupTracker {
        self.tracker.clone()
    }

    pub fn register(&mut self, staging_id: Uuid) {
        self.tracker.register(staging_id);
    }

    pub fn disarm(&mut self, staging_id: Uuid) {
        self.tracker.disarm(staging_id);
    }

    pub fn disarm_all(&mut self) {
        let mut ids = self
            .tracker
            .staging_ids
            .lock()
            .expect("staged cleanup lock poisoned");
        ids.clear();
    }

    #[cfg(test)]
    fn pending(&self) -> Vec<Uuid> {
        self.tracker
            .staging_ids
            .lock()
            .expect("staged cleanup lock poisoned")
            .clone()
    }
}

pub async fn enqueue_expired_object_uploads(pool: &PgPool) -> Result<i32, sqlx::Error> {
    enqueue_expired_object_uploads_with(pool, |staging_id| async move {
        crate::enqueue_object_upload_expire(staging_id).await
    })
    .await
}

pub async fn enqueue_expired_object_uploads_with<F, Fut>(
    pool: &PgPool,
    enqueue: F,
) -> Result<i32, sqlx::Error>
where
    F: Fn(Uuid) -> Fut,
    Fut: std::future::Future<Output = Result<Option<String>, String>>,
{
    let staging_ids: Vec<Uuid> =
        sqlx::query_scalar("SELECT * FROM kb_object_upload_expiry_candidates()")
            .fetch_all(pool)
            .await?;
    for staging_id in &staging_ids {
        enqueue(*staging_id)
            .await
            .map_err(sqlx::Error::Protocol)?
            .ok_or_else(|| {
                sqlx::Error::Protocol("Oxana Redis is not configured for upload expiry".into())
            })?;
    }
    i32::try_from(staging_ids.len()).map_err(|error| sqlx::Error::Decode(Box::new(error)))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ObjectUploadExpiryResult {
    state: String,
    staging_id: Uuid,
    deletion: Option<ObjectDeletionIdentity>,
}

pub async fn expire_one_object_upload(
    pool: &PgPool,
    staging_id: Uuid,
) -> Result<Option<ObjectDeletionIdentity>, String> {
    let value: serde_json::Value = sqlx::query_scalar("SELECT kb_object_upload_expire_one($1)")
        .bind(staging_id)
        .fetch_one(pool)
        .await
        .map_err(|error| error.to_string())?;
    let result: ObjectUploadExpiryResult =
        serde_json::from_value(value).map_err(|error| error.to_string())?;
    if result.staging_id != staging_id
        || !matches!(result.state.as_str(), "expired" | "not_current")
    {
        return Err("upload expiry returned an invalid closed result".into());
    }
    Ok(result.deletion)
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
) -> Result<Option<ObjectDeletionIdentity>, sqlx::Error> {
    let value: Option<serde_json::Value> = sqlx::query_scalar(
        "SELECT kb_release_knowledge_document_object($1,$2::kb_actor_identity,$3,$4)",
    )
    .bind(document_id)
    .bind(actor_identity)
    .bind(idempotency_key)
    .bind(Uuid::new_v4())
    .fetch_one(pool)
    .await?;
    value
        .map(serde_json::from_value)
        .transpose()
        .map_err(|error| sqlx::Error::Decode(Box::new(error)))
}

pub async fn process_object_deletion(
    pool: &PgPool,
    deletion: &ObjectDeletionIdentity,
) -> Result<(), String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Preflight {
        state: String,
    }
    let value: serde_json::Value =
        sqlx::query_scalar("SELECT kb_retention_preflight($1,$2::kb_object_ref,$3::kb_sha256,$4)")
            .bind(deletion.deletion_id)
            .bind(&deletion.object_ref)
            .bind(&deletion.digest)
            .bind(deletion.byte_length)
            .fetch_one(pool)
            .await
            .map_err(|error| error.to_string())?;
    let preflight: Preflight = serde_json::from_value(value).map_err(|error| error.to_string())?;
    match preflight.state.as_str() {
        "completed" => return Ok(()),
        "current" => {}
        "mismatch" => return Err("object deletion business fence is not current".into()),
        _ => return Err("retention preflight returned an invalid closed state".into()),
    }
    crate::delete_retained_blob(&deletion.digest).await?;
    let completed: bool =
        sqlx::query_scalar("SELECT kb_retention_complete($1,$2::kb_object_ref,$3::kb_sha256)")
            .bind(deletion.deletion_id)
            .bind(&deletion.object_ref)
            .bind(&deletion.digest)
            .fetch_one(pool)
            .await
            .map_err(|error| error.to_string())?;
    if completed {
        Ok(())
    } else {
        Err("object deletion completion was not acknowledged".into())
    }
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
