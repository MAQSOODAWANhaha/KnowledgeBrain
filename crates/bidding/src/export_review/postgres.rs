//! Durable export-review checkpoints on the existing agent ledger.
//! Does not reuse composition workspace.done or publish a triad.
use super::agent::{Checkpoint, Journal};
use crate::agent_error::AgentError;
use crate::bid_authoring_v2::AgentRunLease;
use platform::BidAuthoringRequestIdentityV2;
use sqlx::PgPool;

pub struct PgJournal<'a> {
    pub pool: &'a PgPool,
    pub request: &'a BidAuthoringRequestIdentityV2,
    pub owner: &'a AgentRunLease,
}

fn db_error(error: sqlx::Error) -> AgentError {
    AgentError::new("INTERNAL", error.to_string())
}

#[async_trait::async_trait]
impl Journal for PgJournal<'_> {
    async fn load(&self) -> Result<Option<Checkpoint>, AgentError> {
        let value: Option<sqlx::types::Json<Checkpoint>> = sqlx::query_scalar(
            "SELECT kb_bid_v2_export_review_checkpoint_get($1,$2::kb_sha256)",
        )
        .bind(self.request.request_artifact_id)
        .bind(&self.request.frozen_input_sha256)
        .fetch_one(self.pool)
        .await
        .map_err(db_error)?;
        Ok(value.map(|v| v.0))
    }

    async fn reserve(&self, state: &Checkpoint, body: &[u8]) -> Result<usize, AgentError> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let count: i64 = sqlx::query_scalar(
            "SELECT kb_bid_v2_export_review_reserve($1,$2::kb_sha256,$3,$4,$5,$6::kb_sha256,$7)",
        )
        .bind(self.request.request_artifact_id)
        .bind(&self.request.frozen_input_sha256)
        .bind(self.owner.attempt)
        .bind(self.owner.execution_owner_token)
        .bind(i32::try_from(state.turn).map_err(|e| AgentError::new("INTERNAL", e.to_string()))?)
        .bind(&state.contract_sha256)
        .bind(body)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_error)?;
        sqlx::query("SELECT kb_bid_v2_export_review_checkpoint_put($1,$2::kb_sha256,$3,$4,$5)")
            .bind(self.request.request_artifact_id)
            .bind(&self.request.frozen_input_sha256)
            .bind(self.owner.attempt)
            .bind(self.owner.execution_owner_token)
            .bind(sqlx::types::Json(state))
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        usize::try_from(count).map_err(|e| AgentError::new("INTERNAL", e.to_string()))
    }

    async fn save(&self, state: &Checkpoint) -> Result<(), AgentError> {
        sqlx::query("SELECT kb_bid_v2_export_review_checkpoint_put($1,$2::kb_sha256,$3,$4,$5)")
            .bind(self.request.request_artifact_id)
            .bind(&self.request.frozen_input_sha256)
            .bind(self.owner.attempt)
            .bind(self.owner.execution_owner_token)
            .bind(sqlx::types::Json(state))
            .execute(self.pool)
            .await
            .map_err(db_error)?;
        Ok(())
    }
}

pub const CHECKPOINT_SQL_KEYS: &[&str] = &[
    "journal",
    "contract_sha256",
    "inventory",
    "analysis",
    "tender_coverage",
    "output_coverage",
    "reviews",
    "turn",
    "tool_calls",
    "read_bytes",
    "transcript",
    "progress",
    "done",
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_runtime::progress::Progress;
    use crate::export_review::{Inventory, OutputCoverage};
    use crate::tender_analysis::{Analysis, Coverage};
    use serde_json::Value;
    use std::collections::BTreeMap;

    #[test]
    fn checkpoint_object_keys_match_sql_exact_list() {
        let state = Checkpoint {
            journal: Default::default(),
            contract_sha256: "a".repeat(64),
            inventory: Inventory::default(),
            analysis: Analysis::default(),
            tender_coverage: Coverage::default(),
            output_coverage: OutputCoverage::default(),
            reviews: BTreeMap::new(),
            turn: 0,
            tool_calls: 0,
            read_bytes: 0,
            transcript: vec![],
            progress: Progress::default(),
            done: false,
        };
        let value = serde_json::to_value(&state).unwrap();
        let mut keys: Vec<_> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort();
        let mut expected = CHECKPOINT_SQL_KEYS.to_vec();
        expected.sort();
        assert_eq!(keys, expected);
        assert!(matches!(value["done"], Value::Bool(false)));
    }
}
