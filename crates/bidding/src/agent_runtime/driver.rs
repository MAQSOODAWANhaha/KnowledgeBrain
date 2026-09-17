//! Shared I/O loop. Domain adapters own tool semantics and checkpoint storage;
//! this layer owns the prepared/received/committed ordering for both products.
use super::{TurnJournal, valid_tool_turn};
use crate::agent_error::AgentError;
use async_trait::async_trait;
use knowledge::models::ChatTurn;
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

// Three attempts already bound the boundary. Space its two retries by 1s/2s;
// keep retry ownership here, never in the SDK or HTTP client.
const PROVIDER_RETRY_INTERVAL: Duration = Duration::from_secs(1);

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

pub(crate) fn check_cancel(cancel: &CancellationToken) -> Result<(), AgentError> {
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
        let recovered = host.status().journal.response().is_some();
        if recovered {
            // A received response already belongs to its saved reservation.
            // Validate it before replay; these gates authorize only a NEW call.
            let status = host.status();
            status.journal.validate(status.turn, status.role)?;
        } else {
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
                let delay = PROVIDER_RETRY_INTERVAL * attempt as u32;
                tracing::info!(
                    event = "agent_provider_retry_wait",
                    turn = host.status().turn,
                    role = host.status().role,
                    attempt,
                    delay_ms = delay.as_millis() as u64
                );
                tokio::select! {
                    biased;
                    _ = cancel.cancelled() => check_cancel(cancel)?,
                    _ = tokio::time::sleep(delay) => {}
                }
            };
            host.journal_mut().responded(response.clone())?;
            host.save().await?;
            response
        };
        // Saved response recovery must never reserve/call the model again.
        // Yield so a concurrent cancel can land before tools; the recovered
        // path has no other await between loop entry and execute.
        if recovered {
            tokio::task::yield_now().await;
        }
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
        // session.bytes() is serialized AgentRun state, not the Chat wire body.
        // Adapters separately enforce the reserved request against byte/token ceilings.
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use std::{
        sync::{Arc, Mutex},
        time::Duration,
    };

    #[derive(Default)]
    struct FailedProvider {
        journal: TurnJournal,
        reserved: Mutex<usize>,
        sent: Mutex<Vec<(Instant, Vec<u8>)>>,
        first_send: Arc<tokio::sync::Notify>,
    }

    #[async_trait]
    impl Driver for FailedProvider {
        fn status(&self) -> Status<'_> {
            Status {
                journal: &self.journal,
                turn: 0,
                role: "main",
                done: false,
                budget_exhausted: false,
                execution_blocked: false,
                max_context_bytes: 4096,
            }
        }

        fn journal_mut(&mut self) -> &mut TurnJournal {
            &mut self.journal
        }

        async fn prepare_request(&mut self) -> Result<Vec<u8>, AgentError> {
            Ok(br#"{"messages":[{"content":"synthetic","role":"system"}]}"#.to_vec())
        }

        async fn reserve(&self, body: &[u8], _: usize) -> Result<usize, AgentError> {
            assert_eq!(body, self.journal.body().unwrap());
            let mut reserved = self.reserved.lock().unwrap();
            assert!(
                *reserved < 3,
                "exhausted boundary must not be reserved again"
            );
            *reserved += 1;
            Ok(*reserved)
        }

        async fn call_model(&self, body: &[u8]) -> Result<ChatTurn, AgentError> {
            self.sent
                .lock()
                .unwrap()
                .push((Instant::now(), body.to_vec()));
            self.first_send.notify_one();
            Err(AgentError::new(
                "AGENT_PROVIDER_UNAVAILABLE",
                "synthetic failure",
            ))
        }

        async fn execute(
            &mut self,
            _: ChatTurn,
            _: BTreeMap<String, String>,
            _: &CancellationToken,
        ) -> Result<Vec<Value>, AgentError> {
            panic!("partial or failed provider responses cannot execute tools")
        }

        async fn save(&self) -> Result<(), AgentError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn provider_retry_waits_between_exact_reserved_requests_and_keeps_exhaustion() {
        let mut host = FailedProvider::default();
        let error = drive(&mut host, &CancellationToken::new())
            .await
            .unwrap_err();
        assert_eq!(error.code, "AGENT_PROVIDER_UNAVAILABLE");
        {
            let sent = host.sent.lock().unwrap();
            assert_eq!(sent.len(), 3);
            assert!(
                sent.iter()
                    .all(|(_, body)| body == host.journal.body().unwrap())
            );
            assert!(sent[1].0.duration_since(sent[0].0) >= Duration::from_secs(1));
            assert!(sent[2].0.duration_since(sent[1].0) >= Duration::from_secs(2));
            assert!(host.journal.response().is_none());
            assert_eq!(host.journal.sequence, 1);
        }

        let mut resumed = FailedProvider {
            reserved: Mutex::new(2),
            ..Default::default()
        };
        drive(&mut resumed, &CancellationToken::new())
            .await
            .unwrap_err();
        assert_eq!(resumed.sent.lock().unwrap().len(), 1);
        assert_eq!(*resumed.reserved.lock().unwrap(), 3);
    }

    #[tokio::test]
    async fn cancellation_during_retry_wait_does_not_reserve_another_call() {
        let mut host = FailedProvider::default();
        let cancel = CancellationToken::new();
        let first_send = host.first_send.clone();
        let cancellation = async {
            first_send.notified().await;
            tokio::time::sleep(Duration::from_millis(20)).await;
            cancel.cancel();
        };
        let (result, ()) = tokio::join!(drive(&mut host, &cancel), cancellation);
        assert_eq!(result.unwrap_err().code, "INTERNAL");
        assert_eq!(*host.reserved.lock().unwrap(), 1);
        assert_eq!(host.sent.lock().unwrap().len(), 1);
        assert!(host.journal.response().is_none());
    }

    struct ScriptedHost {
        journal: TurnJournal,
        turn: usize,
        done: bool,
        complete_after: usize,
        budget_exhausted: bool,
        execution_blocked: bool,
        reserved: Mutex<usize>,
        calls: Mutex<usize>,
        executes: Mutex<usize>,
        sdk_turns: Mutex<Vec<usize>>,
    }

    impl Default for ScriptedHost {
        fn default() -> Self {
            Self {
                journal: TurnJournal::default(),
                turn: 0,
                done: false,
                complete_after: 2,
                budget_exhausted: false,
                execution_blocked: false,
                reserved: Mutex::new(0),
                calls: Mutex::new(0),
                executes: Mutex::new(0),
                sdk_turns: Mutex::new(Vec::new()),
            }
        }
    }

    fn tool_response(id: &str) -> ChatTurn {
        ChatTurn {
            tool_calls: vec![knowledge::models::ChatToolCall {
                id: id.into(),
                name: "read_source".into(),
                arguments: "{}".into(),
            }],
            finish_reason: "tool_calls".into(),
            ..Default::default()
        }
    }

    fn tool_result(id: &str) -> Value {
        json!({"role":"tool","tool_call_id":id,"content":"fixture source"})
    }

    #[async_trait]
    impl Driver for ScriptedHost {
        fn status(&self) -> Status<'_> {
            Status {
                journal: &self.journal,
                turn: self.turn,
                role: "main",
                done: self.done,
                budget_exhausted: self.budget_exhausted,
                execution_blocked: self.execution_blocked,
                max_context_bytes: 16384,
            }
        }

        fn journal_mut(&mut self) -> &mut TurnJournal {
            &mut self.journal
        }

        async fn prepare_request(&mut self) -> Result<Vec<u8>, AgentError> {
            let evidence = if let Some(session) = &self.journal.session {
                session.projected_history()?
            } else {
                super::super::session::test_system_evidence("main")
            };
            let body = super::super::session::test_window_body(evidence, "progress");
            self.journal.prepare_session(
                &body,
                crate::agent_runtime::SESSION_PREFIX,
                crate::agent_runtime::ANALYSIS_SESSION_SUFFIX,
                4 - self.turn,
                16384,
            )?;
            self.sdk_turns.lock().unwrap().push(
                self.journal
                    .session
                    .as_ref()
                    .map(|session| session.turn())
                    .unwrap_or(0),
            );
            Ok(body)
        }

        async fn reserve(&self, body: &[u8], local_attempt: usize) -> Result<usize, AgentError> {
            assert_eq!(body, self.journal.body().unwrap());
            *self.reserved.lock().unwrap() += 1;
            Ok(local_attempt)
        }

        async fn call_model(&self, _: &[u8]) -> Result<ChatTurn, AgentError> {
            let mut calls = self.calls.lock().unwrap();
            *calls += 1;
            Ok(tool_response(&format!("call-{calls}")))
        }

        async fn execute(
            &mut self,
            response: ChatTurn,
            _: BTreeMap<String, String>,
            _: &CancellationToken,
        ) -> Result<Vec<Value>, AgentError> {
            *self.executes.lock().unwrap() += 1;
            self.turn += 1;
            self.done = self.turn >= self.complete_after;
            Ok(vec![tool_result(&response.tool_calls[0].id)])
        }

        async fn save(&self) -> Result<(), AgentError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn drive_commits_two_turns_and_reuses_the_sdk_session() {
        let mut host = ScriptedHost::default();
        drive(&mut host, &CancellationToken::new()).await.unwrap();
        assert_eq!(*host.calls.lock().unwrap(), 2);
        assert_eq!(*host.reserved.lock().unwrap(), 2);
        assert_eq!(*host.executes.lock().unwrap(), 2);
        assert_eq!(*host.sdk_turns.lock().unwrap(), vec![1, 2]);
        assert!(host.journal.pending.is_none());
        assert!(host.done);
    }

    #[tokio::test]
    async fn saved_response_resumes_without_reserving_or_calling_the_model() {
        let mut host = ScriptedHost {
            complete_after: 1,
            ..Default::default()
        };
        let body = host.prepare_request().await.unwrap();
        host.journal.prepare(0, "main", &body).unwrap();
        host.journal.responded(tool_response("saved-0")).unwrap();
        drive(&mut host, &CancellationToken::new()).await.unwrap();
        assert_eq!(*host.calls.lock().unwrap(), 0);
        assert_eq!(*host.reserved.lock().unwrap(), 0);
        assert_eq!(*host.executes.lock().unwrap(), 1);
        assert!(host.journal.pending.is_none());
    }

    async fn stopped_boundary(received: bool, budget: bool, blocked: bool) -> ScriptedHost {
        let mut host = ScriptedHost {
            budget_exhausted: budget,
            execution_blocked: blocked,
            ..Default::default()
        };
        let body = host.prepare_request().await.unwrap();
        host.journal.prepare(0, "main", &body).unwrap();
        if received {
            host.journal
                .responded(tool_response("saved-at-limit"))
                .unwrap();
        }
        host
    }

    #[tokio::test]
    async fn received_response_finishes_before_next_call_budget_or_execution_gate() {
        for (budget, blocked) in [(true, false), (false, true), (true, true)] {
            let mut host = stopped_boundary(true, budget, blocked).await;
            host.complete_after = 1;
            drive(&mut host, &CancellationToken::new()).await.unwrap();
            assert!(host.done);
            assert!(host.journal.pending.is_none());
            assert_eq!(host.journal.sequence, 3);
            assert_eq!(*host.executes.lock().unwrap(), 1);
            assert_eq!(*host.reserved.lock().unwrap(), 0);
            assert_eq!(*host.calls.lock().unwrap(), 0);
            assert_eq!(host.sdk_turns.lock().unwrap().len(), 1);
        }
    }

    #[tokio::test]
    async fn received_replay_cannot_authorize_another_preparation_or_reservation() {
        for (budget, blocked) in [(true, false), (false, true)] {
            let mut host = stopped_boundary(true, budget, blocked).await;
            let error = drive(&mut host, &CancellationToken::new())
                .await
                .unwrap_err();
            assert_eq!(error.code, "AGENT_TURN_BUDGET_EXCEEDED");
            assert!(!host.done);
            assert!(host.journal.pending.is_none());
            assert_eq!(host.journal.sequence, 3);
            assert_eq!(host.turn, 1);
            assert_eq!(*host.executes.lock().unwrap(), 1);
            assert_eq!(*host.reserved.lock().unwrap(), 0);
            assert_eq!(*host.calls.lock().unwrap(), 0);
            assert_eq!(host.sdk_turns.lock().unwrap().len(), 1);
        }
    }

    #[tokio::test]
    async fn prepared_boundary_still_obeys_next_call_budget_and_execution_gates() {
        for (budget, blocked) in [(true, false), (false, true)] {
            let mut host = stopped_boundary(false, budget, blocked).await;
            let before = serde_json::to_value(&host.journal).unwrap();
            let error = drive(&mut host, &CancellationToken::new())
                .await
                .unwrap_err();
            assert_eq!(error.code, "AGENT_TURN_BUDGET_EXCEEDED");
            assert_eq!(serde_json::to_value(&host.journal).unwrap(), before);
            assert_eq!(*host.executes.lock().unwrap(), 0);
            assert_eq!(*host.reserved.lock().unwrap(), 0);
            assert_eq!(*host.calls.lock().unwrap(), 0);
            assert_eq!(host.sdk_turns.lock().unwrap().len(), 1);
        }
    }

    #[tokio::test]
    async fn received_replay_at_limit_keeps_cancellation_and_frozen_boundary_validation() {
        let mut host = stopped_boundary(true, true, true).await;
        let cancel = CancellationToken::new();
        cancel.cancel();
        let error = drive(&mut host, &cancel).await.unwrap_err();
        assert_eq!(error.message, "Agent run cancelled");
        assert_eq!(*host.executes.lock().unwrap(), 0);
        assert!(host.journal.response().is_some());

        for corruption in ["role", "turn", "body", "response", "session"] {
            let mut host = stopped_boundary(true, true, true).await;
            let pending = host.journal.pending.as_mut().unwrap();
            match corruption {
                "role" => pending.role = "reviewer".into(),
                "turn" => pending.turn += 1,
                "body" => pending.body = "{}".into(),
                "response" => pending.response = Some(ChatTurn::default()),
                "session" => host.journal.session = None,
                _ => unreachable!(),
            }
            let error = drive(&mut host, &CancellationToken::new())
                .await
                .unwrap_err();
            assert_eq!(error.code, "FROZEN_INPUT_DIGEST_MISMATCH", "{corruption}");
            assert_eq!(*host.executes.lock().unwrap(), 0);
            assert_eq!(*host.reserved.lock().unwrap(), 0);
            assert_eq!(*host.calls.lock().unwrap(), 0);
        }
    }

    #[tokio::test]
    async fn cancellation_after_saved_response_does_not_execute_tools() {
        let mut host = ScriptedHost {
            complete_after: 1,
            ..Default::default()
        };
        let body = host.prepare_request().await.unwrap();
        host.journal.prepare(0, "main", &body).unwrap();
        host.journal.responded(tool_response("saved-0")).unwrap();
        let cancel = CancellationToken::new();
        let error = {
            let mut run = std::pin::pin!(drive(&mut host, &cancel));
            tokio::select! {
                biased;
                result = &mut run => panic!(
                    "drive finished before the recovered-response yield: {result:?}"
                ),
                _ = std::future::ready(()) => {}
            }
            cancel.cancel();
            run.await.unwrap_err()
        };
        assert_eq!(error.code, "INTERNAL");
        assert_eq!(error.message, "Agent run cancelled");
        assert_eq!(*host.calls.lock().unwrap(), 0);
        assert_eq!(*host.reserved.lock().unwrap(), 0);
        assert_eq!(*host.executes.lock().unwrap(), 0);
        assert!(host.journal.response().is_some());
        assert!(host.journal.pending.is_some());
    }
}
