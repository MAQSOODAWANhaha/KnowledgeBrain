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
    async fn admit(&mut self, _body: &[u8]) -> Result<(), AgentError> {
        Ok(())
    }
    async fn reserve(&mut self, body: &[u8], local_attempt: usize) -> Result<usize, AgentError>;
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
    /// Why `status().execution_blocked` is stopping a new model call.
    fn block_message(&self) -> String {
        "local execution and independent-work handoff allowances exhausted; blockers and checkpoint retained"
            .into()
    }
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
        let started = Instant::now();
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
                    host.block_message(),
                ));
            }
            if host.status().journal.pending.is_some() {
                let status = host.status();
                status.journal.validate(status.turn, status.role)?;
            } else {
                let body = host.prepare_request().await?;
                let (turn, role) = (host.status().turn, host.status().role);
                host.journal_mut().prepare(turn, role, &body)?;
            }
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
        let request_id = format!("serial:{}", host.status().journal.sequence);
        let response = if let Some(response) = host.status().journal.response() {
            response.clone()
        } else {
            let mut local_attempt = 0;
            let response = loop {
                check_cancel(cancel)?;
                let charged = host
                    .status()
                    .journal
                    .pending
                    .as_ref()
                    .ok_or_else(|| super::invalid("pending request missing"))?
                    .physical_attempts;
                host.journal_mut().retry.recover(&request_id, charged);
                if charged >= super::retry::MAX_ATTEMPTS {
                    let code = host
                        .status()
                        .journal
                        .retry
                        .last_outcome(&request_id)
                        .unwrap_or("AGENT_RETRY_EXHAUSTED")
                        .to_owned();
                    return Err(AgentError::new(
                        &code,
                        "pending request physical attempts exhausted; possible prior charges and checkpoint retained",
                    ));
                }
                if charged > 0 {
                    let last = host
                        .status()
                        .journal
                        .retry
                        .last_outcome(&request_id)
                        .unwrap_or("AGENT_SEND_OUTCOME_UNKNOWN")
                        .to_owned();
                    if !super::retry::retryable(&last) {
                        return Err(AgentError::new(
                            &last,
                            "saved provider failure is not automatically retryable",
                        ));
                    }
                    if host.status().journal.retry.not_before_ms == 0 {
                        host.journal_mut().retry.schedule(
                            &request_id,
                            &AgentError::new(&last, "resume reserved request"),
                        );
                        host.save().await?;
                    }
                    let delay = host.status().journal.retry.remaining();
                    tokio::select! {
                        biased;
                        _ = cancel.cancelled() => check_cancel(cancel)?,
                        _ = tokio::time::sleep(delay) => {}
                    }
                }
                local_attempt += 1;
                host.admit(&body).await?;
                host.journal_mut()
                    .pending
                    .as_mut()
                    .unwrap()
                    .physical_attempts += 1;
                host.journal_mut().retry.reserve(&request_id);
                host.reserve(&body, local_attempt).await?;
                check_cancel(cancel)?;
                let result = tokio::select! {
                    biased;
                    _ = cancel.cancelled() => return Err(AgentError::new("INTERNAL", "Agent run cancelled; reserved provider outcome and possible charge retained")),
                    result = host.call_model(&body) => result,
                };
                match result {
                    Ok(response) if valid_tool_turn(&response) => {
                        host.journal_mut().retry.complete(
                            &request_id,
                            "received",
                            crate::agent_runtime::retry::usage_known(response.usage.as_ref()),
                        );
                        break response;
                    }
                    result => {
                        let error = match result {
                            Err(error) => error,
                            Ok(response) => AgentError::new(
                                "AGENT_RESPONSE_INCOMPLETE",
                                "provider did not return complete unique tool calls",
                            )
                            .with_usage(response.usage),
                        };
                        if let Some(usage) = error.usage.as_deref() {
                            let observed = ChatTurn {
                                usage: Some(usage.clone()),
                                ..Default::default()
                            };
                            if let Some(session) = host.journal_mut().session.as_mut() {
                                session.note_provider_usage(&observed);
                            }
                            host.journal_mut().note_usage(&observed);
                        }
                        host.journal_mut().retry.complete(
                            &request_id,
                            &error.code,
                            crate::agent_runtime::retry::usage_known(error.usage.as_deref()),
                        );
                        host.journal_mut().retry.schedule(&request_id, &error);
                        host.save().await?;
                        // A stop observed alongside provider failure wins after
                        // persisting its actual outcome and any reported usage.
                        check_cancel(cancel)?;
                        if !super::retry::retryable(&error.code)
                            || host
                                .status()
                                .journal
                                .pending
                                .as_ref()
                                .unwrap()
                                .physical_attempts
                                >= super::retry::MAX_ATTEMPTS
                        {
                            return Err(error);
                        }
                    }
                }
            };
            if let Some(session) = host.journal_mut().session.as_mut() {
                session.note_provider_usage(&response);
            }
            host.journal_mut().note_usage(&response);
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
        // Session::tools advances the SDK run before domain execution. A
        // paused/failed batch must retain the received boundary, not a SDK
        // CallTools state that cannot replay the saved provider response.
        let received_session = host.status().journal.session.clone();
        let suppressed = host
            .journal_mut()
            .session
            .as_mut()
            .ok_or_else(|| super::invalid("pending SDK session missing"))?
            .tools(&body, &response)?;
        let results = match host.execute(response, suppressed, cancel).await {
            Ok(results) => results,
            Err(error) => {
                host.journal_mut().session = received_session;
                return Err(error);
            }
        };
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
        error_code: Option<&'static str>,
        cancel_on_send: Option<CancellationToken>,
    }

    #[async_trait]
    impl Driver for FailedProvider {
        fn status(&self) -> Status<'_> {
            Status {
                journal: &self.journal,
                turn: 0,
                role: "main",
                done: false,
                execution_blocked: false,
                max_context_bytes: 4096,
            }
        }

        fn journal_mut(&mut self) -> &mut TurnJournal {
            &mut self.journal
        }

        async fn prepare_request(&mut self) -> Result<Vec<u8>, AgentError> {
            let body = super::super::session::test_window_body(
                super::super::session::test_system_evidence("main"),
                "progress",
            );
            self.journal.prepare_session(
                &body,
                crate::agent_runtime::SESSION_PREFIX,
                crate::agent_runtime::ANALYSIS_SESSION_SUFFIX,
                16384,
            )?;
            Ok(body)
        }

        async fn reserve(&mut self, body: &[u8], _: usize) -> Result<usize, AgentError> {
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
            if let Some(cancel) = &self.cancel_on_send {
                cancel.cancel();
            }
            Err(AgentError::new(
                self.error_code.unwrap_or("AGENT_PROVIDER_TRANSIENT"),
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
        assert_eq!(error.code, "AGENT_PROVIDER_TRANSIENT");
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
        resumed.journal = host.journal.clone();
        resumed.journal.pending.as_mut().unwrap().physical_attempts = 2;
        resumed.journal.retry.attempts.truncate(2);
        resumed.journal.retry.not_before_ms = 0;
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

    #[tokio::test]
    async fn cancellation_with_provider_failure_preserves_outcome_but_returns_stop() {
        let cancel = CancellationToken::new();
        let mut host = FailedProvider {
            error_code: Some("AGENT_PROVIDER_UNAVAILABLE"),
            cancel_on_send: Some(cancel.clone()),
            ..Default::default()
        };
        let error = drive(&mut host, &cancel).await.unwrap_err();
        assert_eq!(error.code, "INTERNAL");
        assert_eq!(error.message, "Agent run cancelled");
        assert_eq!(host.sent.lock().unwrap().len(), 1);
        assert_eq!(host.journal.retry.attempts.len(), 1);
        assert_eq!(
            host.journal.retry.attempts[0].outcome,
            "AGENT_PROVIDER_UNAVAILABLE"
        );
        assert!(host.journal.retry.attempts[0].usage_unknown);
    }

    #[tokio::test]
    async fn permanent_provider_failure_is_not_retried_even_after_reload() {
        let mut host = FailedProvider {
            error_code: Some("AGENT_PROVIDER_UNAVAILABLE"),
            ..Default::default()
        };
        assert!(drive(&mut host, &CancellationToken::new()).await.is_err());
        host.journal =
            serde_json::from_value(serde_json::to_value(&host.journal).unwrap()).unwrap();
        assert_eq!(
            drive(&mut host, &CancellationToken::new())
                .await
                .unwrap_err()
                .code,
            "AGENT_PROVIDER_UNAVAILABLE"
        );
        assert_eq!(host.sent.lock().unwrap().len(), 1);
        assert_eq!(*host.reserved.lock().unwrap(), 1);
    }

    struct ScriptedHost {
        http_endpoint: Option<String>,
        operator_paused: bool,
        journal: TurnJournal,
        interrupt_call: Option<usize>,
        turn: usize,
        done: bool,
        complete_after: usize,
        execution_blocked: bool,
        reserved: Mutex<usize>,
        calls: Mutex<usize>,
        executes: Mutex<usize>,
        sdk_turns: Mutex<Vec<usize>>,
    }

    impl Default for ScriptedHost {
        fn default() -> Self {
            Self {
                http_endpoint: None,
                operator_paused: false,
                journal: TurnJournal::default(),
                interrupt_call: None,
                turn: 0,
                done: false,
                complete_after: 2,
                execution_blocked: false,
                reserved: Mutex::new(0),
                calls: Mutex::new(0),
                executes: Mutex::new(0),
                sdk_turns: Mutex::new(Vec::new()),
            }
        }
    }

    #[tokio::test]
    async fn operator_pause_before_send_is_idempotent_and_unknown_send_stays_counted() {
        let cancel = CancellationToken::new();
        let mut host = ScriptedHost {
            operator_paused: true,
            complete_after: 1,
            interrupt_call: Some(1),
            ..Default::default()
        };
        assert_eq!(
            drive(&mut host, &cancel).await.unwrap_err().code,
            "AGENT_PAUSED_OPERATOR"
        );
        let prepared = serde_json::to_value(&host.journal).unwrap();
        assert_eq!(host.journal.pending.as_ref().unwrap().physical_attempts, 0);
        assert_eq!(host.journal.accounting.physical_calls, 0);
        assert_eq!(
            drive(&mut host, &cancel).await.unwrap_err().code,
            "AGENT_PAUSED_OPERATOR"
        );
        assert_eq!(serde_json::to_value(&host.journal).unwrap(), prepared);
        host.operator_paused = false;
        drive(&mut host, &cancel).await.unwrap();
        assert_eq!(*host.calls.lock().unwrap(), 2);
        assert_eq!(host.journal.accounting.physical_calls, 2);
        assert_eq!(*host.executes.lock().unwrap(), 1);
        assert_eq!(
            host.journal.retry.attempts[0].outcome,
            "AGENT_TRANSPORT_INTERRUPTED"
        );
        assert!(host.journal.retry.attempts[0].usage_unknown);
    }

    fn tool_response(id: &str) -> ChatTurn {
        ChatTurn {
            tool_calls: vec![knowledge::models::ChatToolCall {
                id: id.into(),
                name: "read_source".into(),
                arguments: "{}".into(),
            }],
            finish_reason: "tool_calls".into(),
            usage: Some(knowledge::models::ChatUsage {
                prompt_tokens: Some(10),
                completion_tokens: Some(2),
                total_tokens: Some(12),
                cached_tokens: Some(1),
                reasoning_tokens: Some(0),
            }),
            ..Default::default()
        }
    }

    fn tool_result(id: &str) -> Value {
        json!({"role":"tool","tool_call_id":id,"content":"fixture source"})
    }

    #[async_trait]
    impl Driver for ScriptedHost {
        async fn admit(&mut self, _body: &[u8]) -> Result<(), AgentError> {
            if self.operator_paused {
                Err(AgentError::new(
                    "AGENT_PAUSED_OPERATOR",
                    "test pause before send",
                ))
            } else {
                Ok(())
            }
        }

        fn status(&self) -> Status<'_> {
            Status {
                journal: &self.journal,
                turn: self.turn,
                role: "main",
                done: self.done,
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

        async fn reserve(
            &mut self,
            body: &[u8],
            local_attempt: usize,
        ) -> Result<usize, AgentError> {
            assert_eq!(body, self.journal.body().unwrap());
            self.journal.accounting.record(10, 2, 1)?;
            *self.reserved.lock().unwrap() += 1;
            Ok(local_attempt)
        }

        async fn call_model(&self, body: &[u8]) -> Result<ChatTurn, AgentError> {
            if let Some(endpoint) = &self.http_endpoint {
                return super::super::chat::fixture_send(endpoint, body).await;
            }
            let mut calls = self.calls.lock().unwrap();
            *calls += 1;
            if self.interrupt_call == Some(*calls) {
                return Err(AgentError::new(
                    "AGENT_TRANSPORT_INTERRUPTED",
                    "synthetic stream disconnect",
                ));
            }
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
    async fn actual_http_retry_after_empty_tools_then_success_preserves_exact_wire() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!(
            "http://{}/v1/chat/completions",
            listener.local_addr().unwrap()
        );
        let server = tokio::spawn(async move {
            let mut requests = Vec::new();
            for index in 0..3 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                let mut block = [0u8; 4096];
                let header_end = loop {
                    let read = socket.read(&mut block).await.unwrap();
                    assert!(read > 0);
                    bytes.extend_from_slice(&block[..read]);
                    if let Some(offset) = bytes.windows(4).position(|b| b == b"\r\n\r\n") {
                        break offset + 4;
                    }
                };
                let headers = String::from_utf8_lossy(&bytes[..header_end]).to_ascii_lowercase();
                let length: usize = headers
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length:"))
                    .unwrap()
                    .trim()
                    .parse()
                    .unwrap();
                while bytes.len() < header_end + length {
                    let read = socket.read(&mut block).await.unwrap();
                    assert!(read > 0);
                    bytes.extend_from_slice(&block[..read]);
                }
                requests.push((
                    Instant::now(),
                    bytes[header_end..header_end + length].to_vec(),
                ));
                let (status, retry_after, response) = match index {
                    0 => (429, "Retry-After: 2\r\n", String::new()),
                    1 => (
                        200,
                        "",
                        format!(
                            "data: {}\n\ndata: [DONE]\n\n",
                            json!({"choices":[{"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":5,"completion_tokens":2,"total_tokens":7}})
                        ),
                    ),
                    _ => (
                        200,
                        "",
                        format!(
                            "data: {}\n\ndata: [DONE]\n\n",
                            json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"accepted","function":{"name":"read_source","arguments":"{}"}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":10,"completion_tokens":2,"total_tokens":12}})
                        ),
                    ),
                };
                let header = format!(
                    "HTTP/1.1 {status} Fixture\r\n{retry_after}Content-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    response.len()
                );
                socket.write_all(header.as_bytes()).await.unwrap();
                socket.write_all(response.as_bytes()).await.unwrap();
            }
            requests
        });
        let mut host = ScriptedHost {
            http_endpoint: Some(endpoint),
            complete_after: 1,
            ..Default::default()
        };
        tokio::time::timeout(
            Duration::from_secs(30),
            drive(&mut host, &CancellationToken::new()),
        )
        .await
        .unwrap()
        .unwrap();
        let requests = server.await.unwrap();
        assert_eq!(requests.len(), 3);
        assert!(requests.iter().all(|(_, body)| body == &requests[0].1));
        assert!(requests[1].0.duration_since(requests[0].0) >= Duration::from_secs(2));
        assert_eq!(*host.reserved.lock().unwrap(), 3);
        assert_eq!(host.journal.accounting.physical_calls, 3);
        assert_eq!(*host.executes.lock().unwrap(), 1);
        assert_eq!(host.journal.usage.total_tokens, 19);
        assert_eq!(
            host.journal
                .retry
                .attempts
                .iter()
                .map(|a| a.outcome.as_str())
                .collect::<Vec<_>>(),
            vec![
                "AGENT_PROVIDER_TRANSIENT",
                "AGENT_RESPONSE_INCOMPLETE",
                "received"
            ]
        );
        assert!(host.journal.retry.attempts[0].usage_unknown);
        assert!(!host.journal.retry.attempts[1].usage_unknown);
        assert!(!host.journal.retry.attempts[2].usage_unknown);
        assert!(host.journal.pending.is_none());
        drive(&mut host, &CancellationToken::new()).await.unwrap();
        assert_eq!(
            *host.executes.lock().unwrap(),
            1,
            "completed tools must not execute again"
        );
    }

    #[tokio::test]
    async fn drive_commits_two_turns_with_request_scoped_sdk_and_cumulative_usage() {
        let mut host = ScriptedHost::default();
        drive(&mut host, &CancellationToken::new()).await.unwrap();
        assert_eq!(*host.calls.lock().unwrap(), 2);
        assert_eq!(*host.reserved.lock().unwrap(), 2);
        assert_eq!(*host.executes.lock().unwrap(), 2);
        assert_eq!(*host.sdk_turns.lock().unwrap(), vec![1, 1]);
        assert!(host.journal.pending.is_none());
        assert!(host.done);
        assert!(host.journal.session.is_none());
        assert_eq!(host.journal.usage.input_tokens, 20);
        assert_eq!(host.journal.usage.output_tokens, 4);
        assert_eq!(host.journal.usage.total_tokens, 24);
        assert_eq!(host.journal.usage.cached_input_tokens, 2);
    }

    #[tokio::test]
    async fn interrupted_stream_retries_exact_boundary_without_reexecuting() {
        let mut host = ScriptedHost {
            interrupt_call: Some(2),
            ..Default::default()
        };
        drive(&mut host, &CancellationToken::new()).await.unwrap();
        assert_eq!(host.turn, 2);
        assert_eq!(*host.executes.lock().unwrap(), 2);
        assert_eq!(*host.calls.lock().unwrap(), 3);
        let restored: TurnJournal =
            serde_json::from_value(serde_json::to_value(&host.journal).unwrap()).unwrap();
        assert_eq!(restored.retry.attempts.len(), 3);
        assert_eq!(
            restored.retry.attempts[1].outcome,
            "AGENT_TRANSPORT_INTERRUPTED"
        );
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

    async fn stopped_boundary(received: bool, blocked: bool) -> ScriptedHost {
        let mut host = ScriptedHost {
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
    async fn received_response_finishes_before_next_call_execution_gate() {
        for blocked in [false, true] {
            let mut host = stopped_boundary(true, blocked).await;
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
        for blocked in [true] {
            let mut host = stopped_boundary(true, blocked).await;
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
    async fn prepared_boundary_still_obeys_execution_gate() {
        for blocked in [true] {
            let mut host = stopped_boundary(false, blocked).await;
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
        let mut host = stopped_boundary(true, true).await;
        let cancel = CancellationToken::new();
        cancel.cancel();
        let error = drive(&mut host, &cancel).await.unwrap_err();
        assert_eq!(error.message, "Agent run cancelled");
        assert_eq!(*host.executes.lock().unwrap(), 0);
        assert!(host.journal.response().is_some());

        for corruption in ["role", "turn", "body", "response", "session"] {
            let mut host = stopped_boundary(true, true).await;
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
            assert_eq!(error.code, "AGENT_CHECKPOINT_INVALID", "{corruption}");
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
