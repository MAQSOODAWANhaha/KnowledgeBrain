use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

mod knowledge;
mod lifecycle;
pub use knowledge::*;
pub use lifecycle::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "text", rename_all = "snake_case")]
pub enum ObjectState {
    Available,
    Deleting,
    Deleted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectDeletionIdentity {
    pub deletion_id: Uuid,
    pub object_ref: String,
    pub digest: String,
    pub byte_length: i64,
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
    pool: PgPool,
    staging_ids: std::sync::Arc<std::sync::Mutex<Vec<Uuid>>>,
}

impl StagedObjectCleanupTracker {
    pub fn new(pool: &PgPool) -> Self {
        Self {
            pool: pool.clone(),
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
            // The supervisor calls this only after its producer has stopped.
            // Persist cancellation before queue handoff; failure or cancellation
            // keeps the staging row eligible for the independent expiry sweep.
            request_object_upload_expiry(&self.pool, staging_id, None)
                .await
                .map_err(|error| {
                    format!("staged object expiry request failed for {staging_id}: {error}")
                })?;
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
    pub fn new(pool: &PgPool) -> Self {
        StagedObjectCleanupTracker::new(pool).guard()
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
    let staging_ids = expired_object_upload_candidates(pool).await?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_state_preserves_wire_values_and_rejects_unknown_values() {
        for (state, wire) in [
            (ObjectState::Available, "available"),
            (ObjectState::Deleting, "deleting"),
            (ObjectState::Deleted, "deleted"),
        ] {
            assert_eq!(serde_json::to_value(state).unwrap(), wire);
            assert_eq!(
                serde_json::from_value::<ObjectState>(serde_json::json!(wire)).unwrap(),
                state
            );
        }
        assert!(serde_json::from_str::<ObjectState>("\"pending\"").is_err());
    }

    #[tokio::test]
    async fn staging_guard_registration_and_disarm_are_closed() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://unused:unused@127.0.0.1:1/unused")
            .unwrap();
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        let mut guard = StagedObjectCleanupGuard::new(&pool);
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
