//! Rig Chat transport for an already reserved, canonical request. The SDK's
//! public raw-request entry point does not serialize or rebuild its body.
use crate::agent_error::AgentError;
use bytes::Bytes;
use eventsource_stream::Eventsource;
use futures::StreamExt;
use knowledge::models::{ChatToolCall, ChatTurn, ChatUsage};
use rig::{
    completion::{CompletionError, FinishReason},
    http_client::{
        self, HttpClientExt, LazyBody, MultipartForm, Request, Response, StreamingResponse,
    },
    message::AssistantContent,
    providers::openai::completion::streaming::send_compatible_streaming_request,
    streaming::{StreamedAssistantContent, ToolCallDeltaContent},
};
use std::{
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

mod request;
pub(crate) use request::{
    default_context_tokens, default_image_token_reserve, default_token_safety_margin,
    estimate_input_tokens, prepare, system_content,
};

fn invalid() -> AgentError {
    AgentError::new(
        "AGENT_OUTPUT_INVALID",
        "provider response invalid or incomplete",
    )
}

fn unavailable() -> AgentError {
    AgentError::new(
        "AGENT_PROVIDER_UNAVAILABLE",
        "configured provider unavailable",
    )
}

fn http_unavailable() -> http_client::Error {
    // Never pass a reqwest URL or provider error body into SDK diagnostics.
    http_client::Error::Instance(std::io::Error::other("provider transport failed").into())
}

fn response_error(error: CompletionError) -> AgentError {
    match error {
        CompletionError::HttpError(http_client::Error::InvalidStatusCode(status)) => {
            AgentError::new(
                "AGENT_PROVIDER_UNAVAILABLE",
                format!("configured provider returned HTTP {status}"),
            )
        }
        CompletionError::HttpError(_) => unavailable(),
        _ => invalid(),
    }
}

/// One physical reservation permits one HTTP request, including when an SDK
/// event source attempts to reconnect. Retry ownership stays with the Journal.
#[derive(Clone)]
struct OnceHttp {
    client: reqwest::Client,
    sent: Arc<AtomicBool>,
    received_bytes: Arc<AtomicU64>,
    received_chunks: Arc<AtomicU64>,
}

impl HttpClientExt for OnceHttp {
    fn send<T, U>(
        &self,
        _: Request<T>,
    ) -> impl Future<Output = http_client::Result<Response<LazyBody<U>>>> + Send + 'static
    where
        T: Into<Bytes> + Send,
        U: From<Bytes> + Send + 'static,
    {
        std::future::ready(Err(http_unavailable()))
    }

    fn send_multipart<U>(
        &self,
        _: Request<MultipartForm>,
    ) -> impl Future<Output = http_client::Result<Response<LazyBody<U>>>> + Send + 'static
    where
        U: From<Bytes> + Send + 'static,
    {
        std::future::ready(Err(http_unavailable()))
    }

    async fn send_streaming<T>(&self, request: Request<T>) -> http_client::Result<StreamingResponse>
    where
        T: Into<Bytes> + Send,
    {
        if self.sent.swap(true, Ordering::SeqCst) {
            return Err(http_unavailable());
        }
        let request = reqwest::Request::try_from(request.map(Into::<Bytes>::into))
            .map_err(|_| http_unavailable())?;
        let started = Instant::now();
        let response = self
            .client
            .execute(request)
            .await
            .map_err(|_| http_unavailable())?;
        tracing::info!(
            event = "llm_response_headers",
            status = response.status().as_u16(),
            elapsed_ms = started.elapsed().as_millis() as u64
        );
        if !response.status().is_success() {
            // Capture status before touching a potentially truncated error body.
            return Err(http_client::Error::InvalidStatusCode(response.status()));
        }
        let mut output = Response::builder().status(response.status());
        *output.headers_mut().ok_or_else(http_unavailable)? = response.headers().clone();
        let mut first = true;
        let received_bytes = self.received_bytes.clone();
        let received_chunks = self.received_chunks.clone();
        let stream = response.bytes_stream().map(move |chunk| {
            if let Ok(bytes) = &chunk {
                received_bytes.fetch_add(bytes.len() as u64, Ordering::Relaxed);
                received_chunks.fetch_add(1, Ordering::Relaxed);
            }
            if first && chunk.is_ok() {
                first = false;
                tracing::info!(
                    event = "llm_first_body_chunk",
                    elapsed_ms = started.elapsed().as_millis() as u64
                );
            }
            chunk.map_err(|_| http_unavailable())
        });
        // Rig 0.42 records [DONE] but waits for HTTP EOF before flushing its
        // final response. Bound the transport at the actual SSE sentinel.
        // Reuse Rig's SSE parser for split frames/UTF-8; Rig still owns all
        // JSON, tool-call assembly, finish-reason and usage interpretation.
        let stream = futures::stream::unfold(
            (Box::pin(stream.eventsource()), false),
            move |(mut stream, done)| async move {
                if done {
                    return None;
                }
                let event = stream.next().await?;
                let mut done = false;
                let item = event.map_err(|_| http_unavailable()).map(|event| {
                    done = event.data == "[DONE]";
                    if done {
                        tracing::info!(
                            event = "llm_sse_done",
                            elapsed_ms = started.elapsed().as_millis() as u64
                        );
                    } else if let Ok(frame) = serde_json::from_str::<serde_json::Value>(&event.data)
                    {
                        // Protocol labels only: never log provider text or
                        // arbitrary finish-reason strings as diagnostics.
                        for reason in frame["choices"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|choice| choice["finish_reason"].as_str())
                        {
                            let reason = match reason {
                                "tool_calls" | "stop" | "length" | "content_filter" => reason,
                                _ => "other",
                            };
                            tracing::info!(
                                event = "llm_sse_finish_reason",
                                finish_reason = reason,
                                elapsed_ms = started.elapsed().as_millis() as u64
                            );
                        }
                    }
                    let data = event.data.replace('\n', "\ndata: ");
                    Bytes::from(format!("data: {data}\n\n"))
                });
                Some((item, (stream, done)))
            },
        );
        output
            .headers_mut()
            .ok_or_else(http_unavailable)?
            .remove("content-length");
        output.body(Box::pin(stream) as _).map_err(Into::into)
    }
}

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const WAIT_LOG: Duration = Duration::from_secs(15);

fn client() -> Result<reqwest::Client, AgentError> {
    static CLIENT: OnceLock<Result<reqwest::Client, ()>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .retry(reqwest::retry::never())
                .connect_timeout(CONNECT_TIMEOUT)
                .build()
                .map_err(|_| ())
        })
        .as_ref()
        .cloned()
        .map_err(|_| unavailable())
}

#[derive(Default)]
struct StreamStats {
    sdk_events: u64,
    text_events: u64,
    text_delta_bytes: u64,
    reasoning_events: u64,
    reasoning_delta_bytes: u64,
    tool_delta_events: u64,
    tool_argument_delta_bytes: u64,
    completed_tool_events: u64,
    final_events: u64,
    last_sdk_event_ms: Option<u64>,
}

fn log_stream(
    stats: &StreamStats,
    received_bytes: &Arc<AtomicU64>,
    received_chunks: &Arc<AtomicU64>,
    timed_out: bool,
    started: Instant,
) {
    tracing::info!(
        event = "llm_stream_completed",
        received_bytes = received_bytes.load(Ordering::Relaxed),
        received_chunks = received_chunks.load(Ordering::Relaxed),
        sdk_events = stats.sdk_events,
        text_events = stats.text_events,
        text_delta_bytes = stats.text_delta_bytes,
        reasoning_events = stats.reasoning_events,
        reasoning_delta_bytes = stats.reasoning_delta_bytes,
        tool_delta_events = stats.tool_delta_events,
        tool_argument_delta_bytes = stats.tool_argument_delta_bytes,
        completed_tool_events = stats.completed_tool_events,
        final_events = stats.final_events,
        last_sdk_event_ms = stats.last_sdk_event_ms,
        timed_out,
        elapsed_ms = started.elapsed().as_millis() as u64
    );
}

pub(crate) async fn provider_turn(
    runtime: &crate::authoring_runtime::AuthoringRuntimeContractV1,
    body: &[u8],
) -> Result<ChatTurn, AgentError> {
    let key = runtime.resolve_api_key().map_err(|_| unavailable())?;
    send(
        &runtime.endpoint,
        &key,
        body,
        Duration::from_millis(runtime.timeout_ms),
    )
    .await
}

async fn send(
    endpoint: &str,
    key: &str,
    body: &[u8],
    timeout: Duration,
) -> Result<ChatTurn, AgentError> {
    let mut request = Request::builder()
        .method("POST")
        .uri(endpoint)
        .header("content-type", "application/json")
        .header("accept", "text/event-stream")
        .body(body.to_vec())
        .map_err(|_| unavailable())?;
    let mut auth =
        http_client::HeaderValue::from_str(&format!("Bearer {key}")).map_err(|_| unavailable())?;
    auth.set_sensitive(true);
    request.headers_mut().insert("authorization", auth);
    let received_bytes = Arc::new(AtomicU64::new(0));
    let received_chunks = Arc::new(AtomicU64::new(0));
    let http = OnceHttp {
        client: client()?,
        sent: Arc::new(AtomicBool::new(false)),
        received_bytes: received_bytes.clone(),
        received_chunks: received_chunks.clone(),
    };
    let started = Instant::now();
    let mut heartbeat = tokio::time::interval(WAIT_LOG);
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    heartbeat.tick().await;
    let mut deadline = std::pin::pin!(tokio::time::sleep(timeout));
    let mut handle = tokio::spawn(complete_provider_stream(http, request, started));
    loop {
        tokio::select! {
            biased;
            joined = &mut handle => {
                let outcome = joined.unwrap_or_else(|_| Err(unavailable()));
                let empty = StreamStats::default();
                log_stream(
                    outcome.as_ref().ok().map(|(_, stats)| stats).unwrap_or(&empty),
                    &received_bytes,
                    &received_chunks,
                    false,
                    started,
                );
                return outcome.map(|(turn, _)| turn);
            }
            _ = &mut deadline => {
                handle.abort();
                log_stream(
                    &StreamStats::default(),
                    &received_bytes,
                    &received_chunks,
                    true,
                    started,
                );
                return Err(AgentError::new(
                    "AGENT_TURN_TIMEOUT",
                    "configured provider timed out",
                ));
            }
            _ = heartbeat.tick() => {
                tracing::info!(
                    event = "llm_waiting",
                    elapsed_ms = started.elapsed().as_millis() as u64
                );
            }
        }
    }
}

async fn complete_provider_stream(
    http: OnceHttp,
    request: Request<Vec<u8>>,
    started: Instant,
) -> Result<(ChatTurn, StreamStats), AgentError> {
    let mut stats = StreamStats {
        sdk_events: 0,
        text_events: 0,
        text_delta_bytes: 0,
        reasoning_events: 0,
        reasoning_delta_bytes: 0,
        tool_delta_events: 0,
        tool_argument_delta_bytes: 0,
        completed_tool_events: 0,
        final_events: 0,
        last_sdk_event_ms: None,
    };
    let mut stream = send_compatible_streaming_request(http, request, "openai-chat-compatible")
        .await
        .map_err(response_error)?;
    while let Some(event) = stream.next().await {
        match event.map_err(response_error)? {
            StreamedAssistantContent::Text(text) => {
                stats.text_events += 1;
                stats.text_delta_bytes = stats
                    .text_delta_bytes
                    .saturating_add(text.text.len() as u64);
            }
            StreamedAssistantContent::Reasoning { .. } => stats.reasoning_events += 1,
            StreamedAssistantContent::ReasoningDelta { reasoning, .. } => {
                stats.reasoning_events += 1;
                stats.reasoning_delta_bytes = stats
                    .reasoning_delta_bytes
                    .saturating_add(reasoning.len() as u64);
            }
            StreamedAssistantContent::ToolCallDelta { content, .. } => {
                stats.tool_delta_events += 1;
                if let ToolCallDeltaContent::Delta(arguments) = content {
                    stats.tool_argument_delta_bytes = stats
                        .tool_argument_delta_bytes
                        .saturating_add(arguments.len() as u64);
                }
            }
            StreamedAssistantContent::ToolCall { .. } => stats.completed_tool_events += 1,
            StreamedAssistantContent::Final(_) => stats.final_events += 1,
            _ => {}
        }
        stats.last_sdk_event_ms = Some(started.elapsed().as_millis() as u64);
        if stats.sdk_events == 0 {
            tracing::info!(
                event = "llm_first_sdk_event",
                elapsed_ms = started.elapsed().as_millis() as u64
            );
        }
        stats.sdk_events += 1;
    }
    let response = stream.response.as_ref().ok_or_else(invalid)?;
    // Rig normalizes `stop` with tools to ToolCalls. The frozen provider
    // contract is stricter: require its preserved wire reason as well.
    if response.finish_reason != Some(FinishReason::ToolCalls)
        || response.raw["finish_reason"] != "tool_calls"
    {
        return Err(invalid());
    }
    let mut result = ChatTurn {
        finish_reason: "tool_calls".into(),
        ..Default::default()
    };
    for item in &stream.choice {
        match item {
            AssistantContent::Text(text) => result.content.push_str(&text.text),
            AssistantContent::ToolCall(call) => {
                // Rig can mint correlation IDs for id-less providers. Our
                // frozen Chat contract requires a provider-issued call ID.
                let provider = call.provider.as_ref().ok_or_else(invalid)?;
                result.tool_calls.push(ChatToolCall {
                    id: provider.call_id.as_str().into(),
                    name: call.function.name.clone(),
                    arguments: serde_json::to_string(&call.function.arguments)
                        .map_err(|_| invalid())?,
                });
            }
            _ => {}
        }
    }
    // The SDK uses zero as the missing-usage sentinel. Never record that
    // as observed zero consumption. Optional detail counts stay unknown.
    if response.usage.has_values() {
        let raw = &response.raw["usage"];
        result.usage = Some(ChatUsage {
            prompt_tokens: raw["prompt_tokens"].as_u64(),
            completion_tokens: raw["completion_tokens"].as_u64(),
            total_tokens: raw["total_tokens"].as_u64(),
            cached_tokens: raw["prompt_tokens_details"]["cached_tokens"]
                .as_u64()
                .filter(|&n| n != 0),
            reasoning_tokens: raw["completion_tokens_details"]["reasoning_tokens"]
                .as_u64()
                .filter(|&n| n != 0),
        });
    }
    if !super::valid_tool_turn(&result) {
        return Err(invalid());
    }
    Ok((result, stats))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        sync::mpsc,
    };

    fn event(delta: Value, finish: Value) -> String {
        format!(
            "data: {}\n\n",
            json!({"id":"fixture-response","model":"fixture-model","choices":[{"index":0,"delta":delta,"finish_reason":finish}]})
        )
    }

    async fn exchange(
        status: u16,
        response: String,
        hold: bool,
    ) -> (Result<ChatTurn, AgentError>, Vec<Vec<u8>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!(
            "http://{}/v1/chat/completions",
            listener.local_addr().unwrap()
        );
        let (tx, mut received) = mpsc::unbounded_channel();
        let server = tokio::spawn(async move {
            // Accept every attempt so the assertion can detect extra sends.
            let mut sockets = tokio::task::JoinSet::new();
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let tx = tx.clone();
                let response = response.clone();
                sockets.spawn(async move {
                    let mut bytes = Vec::new();
                    let mut block = [0;4096];
                    let end = loop {
                        let n = socket.read(&mut block).await.unwrap();
                        if n == 0 { return; }
                        bytes.extend_from_slice(&block[..n]);
                        if let Some(i) = bytes.windows(4).position(|b| b == b"\r\n\r\n") { break i+4; }
                    };
                    let headers = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
                    let size = headers.lines().find_map(|s| s.strip_prefix("content-length:")).unwrap().trim().parse::<usize>().unwrap();
                    while bytes.len() < end+size {
                        let n = socket.read(&mut block).await.unwrap();
                        if n == 0 { return; }
                        bytes.extend_from_slice(&block[..n]);
                    }
                    tx.send(bytes[end..end+size].to_vec()).unwrap();
                    let size = response.len()+usize::from(hold)*10000;
                    let header = format!("HTTP/1.1 {status} Fixture\r\nContent-Type: text/event-stream\r\nContent-Length: {size}\r\nConnection: close\r\n\r\n");
                    if socket.write_all(header.as_bytes()).await.is_ok() {
                        let _ = socket.write_all(response.as_bytes()).await;
                    }
                    if hold { std::future::pending::<()>().await; }
                });
            }
        });
        let runtime = serde_json::from_value(json!({
            "schema_version":1,"base_url":endpoint.strip_suffix("/chat/completions").unwrap(),
            "endpoint":endpoint,"protocol":"openai_chat_completions_sse","model_id":"fixture-model",
            "credential_ref":"env:LLM_API_KEY","stream":true,"max_tokens":8192,"timeout_ms":90000,
            "response_mode":"tool_calls","transport_retries":0,"temperature":null,"reasoning_effort":null
        })).unwrap();
        let request = prepare(
            &runtime,
            vec![json!({"role":"system","content":"合成来源"}),json!({"role":"user","content":"读取已冻结来源"})],
            vec![json!({"type":"function","function":{"name":"inspect_analysis","description":"Read fixture evidence","parameters":{"type":"object"}}})],
        ).await.unwrap();
        let result = send(
            &endpoint,
            "local-fixture",
            &request,
            Duration::from_millis(if hold { 200 } else { 2000 }),
        )
        .await;
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
        let mut bodies = Vec::new();
        while let Ok(body) = received.try_recv() {
            bodies.push(body);
        }
        assert_eq!(
            bodies,
            vec![request],
            "one reserved body must produce exactly one unchanged physical request"
        );
        (result, bodies)
    }

    #[tokio::test]
    #[ignore = "owned loopback HTTP server; no external provider or credentials"]
    async fn reserved_chat_bytes_use_rig_and_reject_invalid_terminals() {
        let tool = event(
            json!({"tool_calls":[{"index":0,"id":"call-a","type":"function","function":{"name":"inspect_analysis","arguments":"{\"kind\":\"all\"}"}}]}),
            Value::Null,
        );
        let finish = event(json!({}), json!("tool_calls"));
        let usage = format!(
            "data: {}\n\n",
            json!({"choices":[],"usage":{"prompt_tokens":120,"completion_tokens":8,"total_tokens":128,"prompt_tokens_details":{"cached_tokens":70},"completion_tokens_details":{"reasoning_tokens":3}}})
        );
        let complete = format!("{tool}{finish}{usage}data: [DONE]\n\n");
        let (held, _) = exchange(200, complete.clone(), true).await;
        assert!(
            held.is_ok(),
            "[DONE] must complete without waiting for HTTP EOF"
        );
        let (result, _) = exchange(200, complete, false).await;
        let result = result.unwrap();
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].id, "call-a");
        assert_eq!(result.tool_calls[0].name, "inspect_analysis");
        assert_eq!(
            serde_json::from_str::<Value>(&result.tool_calls[0].arguments).unwrap(),
            json!({"kind":"all"})
        );
        assert_eq!(
            result.usage,
            Some(ChatUsage {
                prompt_tokens: Some(120),
                completion_tokens: Some(8),
                total_tokens: Some(128),
                cached_tokens: Some(70),
                reasoning_tokens: Some(3)
            })
        );
        let (result, _) = exchange(200, format!("{tool}{finish}data: [DONE]\n\n"), false).await;
        assert_eq!(result.unwrap().usage, None);
        let partial_usage = format!(
            "data: {}\n\n",
            json!({"choices":[],"usage":{"prompt_tokens":120,"total_tokens":120,"prompt_tokens_details":{},"completion_tokens_details":{}}})
        );
        let (result, _) = exchange(
            200,
            format!("{tool}{finish}{partial_usage}data: [DONE]\n\n"),
            false,
        )
        .await;
        let reported = result.unwrap().usage.unwrap();
        assert_eq!(reported.prompt_tokens, Some(120));
        assert_eq!(reported.completion_tokens, None);
        assert_eq!(reported.cached_tokens, None);
        assert_eq!(reported.reasoning_tokens, None);
        let first = event(
            json!({"tool_calls":[{"index":0,"id":"first","function":{"name":"search_sources","arguments":"{\"query\":\"中"}},{"index":1,"id":"second","function":{"name":"check_gaps","arguments":"{"}}]}),
            Value::Null,
        );
        let second = event(
            json!({"tool_calls":[{"index":1,"function":{"arguments":"}"}},{"index":0,"function":{"arguments":"文\"}"}}]}),
            Value::Null,
        );
        let (result, _) = exchange(
            200,
            format!("{first}{second}{finish}data: [DONE]\n\n"),
            false,
        )
        .await;
        let calls = result.unwrap().tool_calls;
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].id, "first");
        assert_eq!(calls[1].id, "second");
        assert_eq!(
            serde_json::from_str::<Value>(&calls[0].arguments).unwrap(),
            json!({"query":"中文"})
        );
        assert_eq!(calls[1].arguments, "{}");
        let idless = event(
            json!({"tool_calls":[{"index":0,"function":{"name":"inspect_analysis","arguments":"{}"}}]}),
            json!("tool_calls"),
        );
        let malformed = event(
            json!({"tool_calls":[{"index":0,"id":"call-a","function":{"name":"inspect_analysis","arguments":"{\"unfinished\":"}}]}),
            json!("tool_calls"),
        );
        let duplicate = event(
            json!({"tool_calls":[{"index":0,"id":"same","function":{"name":"one","arguments":"{}"}},{"index":1,"id":"same","function":{"name":"two","arguments":"{}"}}]}),
            json!("tool_calls"),
        );
        for (name, body) in [
            ("premature_eof", tool.clone()),
            ("done_without_finish", format!("{tool}data: [DONE]\n\n")),
            ("idless", format!("{idless}data: [DONE]\n\n")),
            (
                "malformed_arguments",
                format!("{malformed}data: [DONE]\n\n"),
            ),
            ("duplicate_ids", format!("{duplicate}data: [DONE]\n\n")),
            (
                "length",
                format!(
                    "{tool}{}data: [DONE]\n\n",
                    event(json!({}), json!("length"))
                ),
            ),
            (
                "stop",
                format!("{tool}{}data: [DONE]\n\n", event(json!({}), json!("stop"))),
            ),
            (
                "inband_error",
                format!(
                    "{tool}{finish}data: {{\"error\":{{\"message\":\"do not log this provider body\"}}}}\n\n"
                ),
            ),
            (
                "invalid_json_frame",
                format!("{tool}data: {{corrupt\n\n{finish}data: [DONE]\n\n"),
            ),
        ] {
            let (result, _) = exchange(200, body, false).await;
            assert!(result.is_err(), "{name} must not execute tools: {result:?}");
        }
        for status in [400, 413, 429, 503] {
            let (result, _) = exchange(status, "truncated provider error body".into(), true).await;
            let error = result.unwrap_err();
            assert_eq!(error.code, "AGENT_PROVIDER_UNAVAILABLE");
            assert!(error.message.contains(&format!("HTTP {status}")));
            assert!(!error.message.contains("truncated provider"));
        }
        let (result, _) = exchange(200, tool, true).await;
        assert_eq!(result.unwrap_err().code, "AGENT_TURN_TIMEOUT");
    }

    #[tokio::test]
    async fn headerless_connection_times_out() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!(
            "http://{}/v1/chat/completions",
            listener.local_addr().unwrap()
        );
        let server = tokio::spawn(async move {
            loop {
                let (_socket, _) = listener.accept().await.unwrap();
                std::future::pending::<()>().await;
            }
        });
        let started = Instant::now();
        let result = send(
            &endpoint,
            "local-fixture",
            b"{}",
            Duration::from_millis(200),
        )
        .await;
        server.abort();
        assert_eq!(result.unwrap_err().code, "AGENT_TURN_TIMEOUT");
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "timeout must not wait for the hung connection drop"
        );
    }
}
