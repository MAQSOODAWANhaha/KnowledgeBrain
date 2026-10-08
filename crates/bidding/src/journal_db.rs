//! Production [`Journal`](crate::analysis::agent::Journal) backed by PostgreSQL.
//!
//! Checkpoints live in `bid_outline_runs.checkpoint`; publishing a finished
//! outline delegates to [`outline::store::publish`](crate::outline::store::publish).
//! This is the B4 wiring that connects a finished agent run to the database —
//! previously `Journal::publish_outline` was a no-op default and nothing
//! persisted the outline.

use crate::agent_error::AgentError;
use crate::analysis::agent::{Checkpoint, Journal};
use crate::outline::chapters::AttachmentBinding;
use crate::outline::{self, OutlineArtifact};
use async_trait::async_trait;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

/// Production journal: checkpoints go to `bid_outline_runs`, publishing goes
/// through `outline::store::publish` (which calls `kb_bid_v2_publish_outline`).
pub struct DbJournal {
    pub pool: PgPool,
    pub project_id: Uuid,
    pub run_id: Uuid,
}

impl DbJournal {
    pub fn new(pool: PgPool, project_id: Uuid, run_id: Uuid) -> Self {
        Self {
            pool,
            project_id,
            run_id,
        }
    }

    fn error(code: &str, message: impl Into<String>) -> AgentError {
        AgentError::new(code, message)
    }
}

#[async_trait]
impl Journal for DbJournal {
    async fn load(&self) -> Result<Option<Checkpoint>, AgentError> {
        let row: Option<(Value,)> =
            sqlx::query_as("SELECT checkpoint FROM bid_outline_runs WHERE id = $1")
                .bind(self.run_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| Self::error("JOURNAL_LOAD_FAILED", format!("journal load: {error}")))?;
        match row {
            None => Ok(None),
            Some((value,)) => {
                // `kb_bid_v2_publish_outline` inserts `'{}'::jsonb` on its own
                // insert path; that is not a valid Checkpoint — treat it (and
                // only it) as "no checkpoint yet".
                if value == serde_json::json!({}) {
                    return Ok(None);
                }
                serde_json::from_value::<Checkpoint>(value)
                    .map(Some)
                    .map_err(|error| {
                        Self::error(
                            "JOURNAL_CHECKPOINT_INVALID",
                            format!("journal checkpoint decode: {error}"),
                        )
                    })
            }
        }
    }

    async fn reserve(
        &self,
        _state: &Checkpoint,
        body: &[u8],
    ) -> Result<Option<usize>, AgentError> {
        // Minimal production semantic: the attempt-independent budget is not
        // tracked in the database here; the agent's own limits (max turns,
        // provider budgets in Config) still apply.
        //
        // NOTE: the exact request bytes only become durable once `save`
        // persists the checkpoint after the turn completes. A crash between
        // `reserve` and `save` loses the pending turn; closing that window is
        // future work.
        Ok(Some(body.len()))
    }

    async fn save(&self, state: &Checkpoint, _progress: &Value) -> Result<(), AgentError> {
        let checkpoint = serde_json::to_value(state).map_err(|error| {
            Self::error(
                "JOURNAL_ENCODE_FAILED",
                format!("journal checkpoint encode: {error}"),
            )
        })?;
        // Upsert: the row may not exist yet (first save of a run), or it may
        // have been created by `kb_bid_v2_publish_outline`'s
        // `ON CONFLICT DO NOTHING` insert. Never touch `status` here — the
        // publish function owns the status transitions.
        sqlx::query(
            "INSERT INTO bid_outline_runs (id, project_id, status, checkpoint)
             VALUES ($1, $2, 'running', $3)
             ON CONFLICT (id) DO UPDATE
             SET checkpoint = EXCLUDED.checkpoint, updated_at = now()",
        )
        .bind(self.run_id)
        .bind(self.project_id)
        .bind(checkpoint)
        .execute(&self.pool)
        .await
        .map_err(|error| Self::error("JOURNAL_SAVE_FAILED", format!("journal save: {error}")))?;
        Ok(())
    }

    async fn publish_outline(
        &self,
        artifact: &OutlineArtifact,
        bindings: &[AttachmentBinding],
    ) -> Result<(), AgentError> {
        outline::store::publish(&self.pool, self.project_id, self.run_id, artifact, bindings)
            .await
            .map_err(|error| {
                Self::error(
                    "OUTLINE_PUBLISH_FAILED",
                    format!("publish_outline: {error}"),
                )
            })?;
        Ok(())
    }
}
