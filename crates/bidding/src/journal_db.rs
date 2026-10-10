//! PostgreSQL journal. Every checkpoint and publication is fenced by the exact
//! worker token and monotonically increasing acquisition epoch.

use crate::agent_error::AgentError;
use crate::analysis::FrozenInput;
use crate::analysis::agent::{Checkpoint, Journal};
use crate::outline::chapters::AttachmentBinding;
use crate::outline::store::OutlineLease;
use crate::outline::{self, OutlineArtifact};
use async_trait::async_trait;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

pub struct DbJournal {
    pub pool: PgPool,
    pub project_id: Uuid,
    pub run_id: Uuid,
    pub lease: OutlineLease,
    input: FrozenInput,
}

impl DbJournal {
    /// Completed or budget-paused jobs are acknowledged without new model work.
    /// A live or expired acquisition never reuses another worker's fence.
    pub async fn acquire(
        pool: PgPool,
        project_id: Uuid,
        run_id: Uuid,
        input: &FrozenInput,
        input_sha256: &str,
    ) -> Result<Option<Self>, String> {
        let terminal: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM bid_outline_runs WHERE id=$1 AND project_id=$2
             AND frozen_input_sha256=$3::kb_sha256 AND status IN ('published','paused_budget'))",
        )
        .bind(run_id)
        .bind(project_id)
        .bind(input_sha256)
        .fetch_one(&pool)
        .await
        .map_err(|error| format!("journal status: {error}"))?;
        if terminal {
            return Ok(None);
        }
        let value: Value =
            sqlx::query_scalar("SELECT kb_bid_v2_outline_claim($1,$2,$3::kb_sha256,$4,120)")
                .bind(run_id)
                .bind(project_id)
                .bind(input_sha256)
                .bind(Uuid::new_v4())
                .fetch_one(&pool)
                .await
                .map_err(|error| format!("outline claim: {error}"))?;
        let lease = serde_json::from_value(value)
            .map_err(|error| format!("outline claim receipt: {error}"))?;
        Ok(Some(Self {
            pool,
            project_id,
            run_id,
            lease,
            input: input.clone(),
        }))
    }

    pub async fn renew(&self) -> Result<(), String> {
        sqlx::query("SELECT kb_bid_v2_outline_renew($1,$2,$3,120)")
            .bind(self.run_id)
            .bind(self.lease.lease_token)
            .bind(self.lease.lease_epoch)
            .execute(&self.pool)
            .await
            .map_err(|error| format!("outline lease renewal: {error}"))?;
        Ok(())
    }

    pub async fn release(&self) -> Result<(), String> {
        sqlx::query("SELECT kb_bid_v2_outline_release($1,$2,$3)")
            .bind(self.run_id)
            .bind(self.lease.lease_token)
            .bind(self.lease.lease_epoch)
            .execute(&self.pool)
            .await
            .map_err(|error| format!("outline lease release: {error}"))?;
        Ok(())
    }

    fn error(code: &str, message: impl Into<String>) -> AgentError {
        AgentError::new(code, message)
    }
}

#[async_trait]
impl Journal for DbJournal {
    async fn load(&self) -> Result<Option<Checkpoint>, AgentError> {
        let value: Value = sqlx::query_scalar(
            "SELECT checkpoint FROM bid_outline_runs WHERE id=$1 AND project_id=$2
             AND lease_token=$3 AND lease_epoch=$4 AND status='running'
             AND lease_until > clock_timestamp()",
        )
        .bind(self.run_id)
        .bind(self.project_id)
        .bind(self.lease.lease_token)
        .bind(self.lease.lease_epoch)
        .fetch_one(&self.pool)
        .await
        .map_err(|error| Self::error("JOURNAL_LOAD_FAILED", format!("journal load: {error}")))?;
        if value == serde_json::json!({}) {
            return Ok(None);
        }
        serde_json::from_value(value).map(Some).map_err(|error| {
            Self::error(
                "JOURNAL_CHECKPOINT_INVALID",
                format!("journal checkpoint decode: {error}"),
            )
        })
    }

    async fn reserve(&self, state: &Checkpoint, body: &[u8]) -> Result<Option<usize>, AgentError> {
        let pending = state.journal.pending.as_ref().ok_or_else(|| {
            Self::error(
                "JOURNAL_RESERVATION_INVALID",
                "reservation has no prepared request",
            )
        })?;
        let saved_body = state.journal.body()?;
        if saved_body != body || pending.physical_attempts == 0 {
            return Err(Self::error(
                "JOURNAL_RESERVATION_INVALID",
                "reservation request identity mismatch",
            ));
        }
        // Commit exact pending bytes, physical attempt count and cumulative run
        // budget under the lease before any network call. A lost ACK may cost a
        // reservation but cannot buy another provider call for free.
        self.save(state, &serde_json::json!({})).await?;
        Ok(Some(pending.physical_attempts))
    }

    async fn save(&self, state: &Checkpoint, _progress: &Value) -> Result<(), AgentError> {
        let checkpoint = serde_json::to_value(state).map_err(|error| {
            Self::error(
                "JOURNAL_ENCODE_FAILED",
                format!("journal checkpoint encode: {error}"),
            )
        })?;
        sqlx::query("SELECT kb_bid_v2_outline_checkpoint($1,$2,$3,$4)")
            .bind(self.run_id)
            .bind(self.lease.lease_token)
            .bind(self.lease.lease_epoch)
            .bind(checkpoint)
            .execute(&self.pool)
            .await
            .map_err(|error| {
                Self::error("JOURNAL_SAVE_FAILED", format!("journal save: {error}"))
            })?;
        Ok(())
    }

    async fn publish_outline(
        &self,
        artifact: &OutlineArtifact,
        bindings: &[AttachmentBinding],
    ) -> Result<(), AgentError> {
        outline::store::publish(
            &self.pool,
            self.project_id,
            self.run_id,
            self.lease,
            &self.input,
            artifact,
            bindings,
        )
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
