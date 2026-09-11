//! Shared I/O loop. Domain adapters own tool semantics and checkpoint storage;
//! this layer owns the prepared/received/committed ordering for both products.
use super::{TurnJournal, valid_tool_turn};
use crate::agent_error::AgentError;
use async_trait::async_trait;
use knowledge::models::ChatTurn;
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::Instant;
use tokio_util::sync::CancellationToken;

/// Read-only view of an existing domain checkpoint, not another saved state.
pub(crate) struct Status<'a> {
    pub journal: &'a TurnJournal,
    pub turn: usize,
    pub role: &'static str,
    pub done: bool,
    pub budget_exhausted: bool,
    pub execution_blocked: bool,
    pub max_context_bytes: usize,
}

#[async_trait]
pub(crate) trait Driver: Send + Sync {
    fn status(&self) -> Status<'_>;
    fn journal_mut(&mut self) -> &mut TurnJournal;
    async fn prepare_request(&mut self) -> Result<Vec<u8>, AgentError>;
    /// Persist the prepared checkpoint and reserve this exact body before
    /// returning the boundary attempt. Adapters retain their durable budgets.
    async fn reserve(&self, body: &[u8], local_attempt: usize) -> Result<usize, AgentError>;
    async fn call_model(&self, body: &[u8]) -> Result<ChatTurn, AgentError>;
    /// Apply a fully saved response, including domain turn/role transitions.
    /// This does not save or advance the shared Journal boundary itself.
    async fn execute(
        &mut self,
        response: ChatTurn,
        suppressed: BTreeMap<String, String>,
        cancel: &CancellationToken,
    ) -> Result<Vec<Value>, AgentError>;
    async fn save(&self) -> Result<(), AgentError>;
}

fn check_cancel(cancel: &CancellationToken) -> Result<(), AgentError> {
    if cancel.is_cancelled() {
        return Err(AgentError::new("INTERNAL", "Agent run cancelled"));
    }
    Ok(())
}

pub(crate) async fn drive<D: Driver>(
    host: &mut D,
    cancel: &CancellationToken,
) -> Result<(), AgentError> {
    while !host.status().done {
        check_cancel(cancel)?;
        if host.status().execution_blocked {
            return Err(AgentError::new(
                "AGENT_TURN_BUDGET_EXCEEDED",
                "local execution and independent-work handoff allowances exhausted; blockers and checkpoint retained",
            ));
        }
        if host.status().budget_exhausted {
            return Err(AgentError::new(
                "AGENT_TURN_BUDGET_EXCEEDED",
                "global Agent budget exhausted; checkpoint retained",
            ));
        }
        let started = Instant::now();
        if host.status().journal.pending.is_none() {
            let body = host.prepare_request().await?;
            let (turn, role) = (host.status().turn, host.status().role);
            host.journal_mut().prepare(turn, role, &body)?;
        }
        let body = host.status().journal.body()?.to_vec();
        tracing::info!(
            event = "agent_request_prepared",
            turn = host.status().turn,
            role = host.status().role,
            sdk_turn = host.status().journal.session.as_ref().map(|s| s.turn()),
            request_bytes = body.len(),
            elapsed_ms = started.elapsed().as_millis() as u64
        );
        let response = if let Some(response) = host.status().journal.response() {
            response.clone()
        } else {
            let mut local_attempt = 0;
            let response = loop {
                check_cancel(cancel)?;
                local_attempt += 1;
                let started = Instant::now();
                let attempt = host.reserve(&body, local_attempt).await?;
                let reserve_ms = started.elapsed().as_millis() as u64;
                check_cancel(cancel)?;
                let started = Instant::now();
                let result = tokio::select! {
                    biased;
                    _ = cancel.cancelled() => return Err(AgentError::new("INTERNAL", "Agent run cancelled")),
                    result = host.call_model(&body) => result,
                };
                tracing::info!(
                    event = "agent_provider_completed",
                    turn = host.status().turn,
                    role = host.status().role,
                    attempt,
                    reserve_ms,
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    error_code = result.as_ref().err().map(|e| e.code.as_str()),
                    valid_tool_turn = result.as_ref().is_ok_and(valid_tool_turn)
                );
                match result {
                    Ok(response) if valid_tool_turn(&response) => break response,
                    Ok(_) if attempt >= 3 => {
                        return Err(AgentError::new(
                            "AGENT_OUTPUT_INVALID",
                            "Agent must return complete unique tool calls",
                        ));
                    }
                    Err(error) if attempt >= 3 => return Err(error),
                    _ => {}
                }
            };
            host.journal_mut().responded(response.clone())?;
            host.save().await?;
            response
        };
        // Saved response recovery must never reserve/call the model again.
        // Cancellation here retains that response without executing its tools.
        check_cancel(cancel)?;
        let role = host.status().role;
        let suppressed = host
            .journal_mut()
            .session
            .as_mut()
            .ok_or_else(|| super::invalid("pending SDK session missing"))?
            .tools(&body, &response)?;
        let results = host.execute(response, suppressed, cancel).await?;
        let session = host
            .journal_mut()
            .session
            .as_mut()
            .ok_or_else(|| super::invalid("pending SDK session missing"))?;
        session.finish(results)?;
        let session_bytes = session.bytes()?;
        // Drop SDK state only at a committed boundary. Global domain budgets
        // and durable evidence survive role/window handoffs unchanged.
        if host.status().done
            || host.status().role != role
            || session_bytes > host.status().max_context_bytes
        {
            host.journal_mut().session = None;
        }
        host.journal_mut().committed()?;
        let started = Instant::now();
        host.save().await?;
        tracing::info!(
            event = "agent_checkpoint_saved",
            completed_turns = host.status().turn,
            elapsed_ms = started.elapsed().as_millis() as u64
        );
    }
    Ok(())
}
