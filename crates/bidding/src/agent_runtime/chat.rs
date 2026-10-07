//! Chat transport for one reserved request. The provider response is parsed
//! with eventsource-stream; the body is not rebuilt into another SSE stream.
use crate::agent_error::AgentError;
use eventsource_stream::Eventsource;
use futures::StreamExt;
use knowledge::models::{ChatToolCall, ChatTurn, ChatUsage};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

mod request;
pub(crate) use request::{estimate_input_tokens, prepare, system_content};

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

fn interrupted() -> AgentError {
    AgentError::new(
        "AGENT_TRANSPORT_INTERRUPTED",
        "provider HTTP response stream interrupted; incomplete tool calls were not accepted",
    )
}

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const WAIT_LOG: Duration = Duration::from_secs(15);

/// Heartbeat ticks must not cancel the SSE task, so it is spawned. Dropping
/// this wrapper (cancel, timeout, or return) aborts that task; JoinHandle
/// drop alone would leave the HTTP request running.
struct AbortOnDrop<T> {
    handle: tokio::task::JoinHandle<T>,
}

impl<T> AbortOnDrop<T> {
    fn abort(&self) {
        self.handle.abort();
    }
}

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

fn response_type(headers: &reqwest::header::HeaderMap) -> &'static str {
    let Some(value) = headers.get(reqwest::header::CONTENT_TYPE) else {
        return "missing";
    };
    let Ok(value) = value.to_str() else {
        return "other";
    };
    let mime = value.split(';').next().unwrap_or_default().trim();
    if mime.eq_ignore_ascii_case("text/event-stream") {
        "sse"
    } else if mime.eq_ignore_ascii_case("application/json") {
        "json"
    } else if mime.eq_ignore_ascii_case("text/html") {
        "html"
    } else {
        "other"
    }
}

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

#[derive(Clone, Default)]
struct StreamStats {
    events: u64,
    text_delta_bytes: u64,
    tool_events: u64,
    last_event_ms: Option<u64>,
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
        events = stats.events,
        text_delta_bytes = stats.text_delta_bytes,
        tool_events = stats.tool_events,
        last_event_ms = stats.last_event_ms,
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
        Arc::default(),
    )
    .await
}

async fn send(
    endpoint: &str,
    key: &str,
    body: &[u8],
    timeout: Duration,
    stats: Arc<Mutex<StreamStats>>,
) -> Result<ChatTurn, AgentError> {
    let received_bytes = Arc::new(AtomicU64::new(0));
    let received_chunks = Arc::new(AtomicU64::new(0));
    let started = Instant::now();
    let mut heartbeat = tokio::time::interval(WAIT_LOG);
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    heartbeat.tick().await;
    let mut deadline = std::pin::pin!(tokio::time::sleep(timeout));
    let mut handle = AbortOnDrop {
        handle: tokio::spawn(read_provider_events(
            endpoint.to_owned(),
            key.to_owned(),
            body.to_vec(),
            received_bytes.clone(),
            received_chunks.clone(),
            started,
            stats.clone(),
        )),
    };
    loop {
        tokio::select! {
            biased;
            joined = &mut handle.handle => {
                let outcome = joined.unwrap_or_else(|_| Err(unavailable()));
                log_stream(
                    &stats.lock().expect("stream statistics"),
                    &received_bytes,
                    &received_chunks,
                    false,
                    started,
                );
                return outcome;
            }
            _ = &mut deadline => {
                handle.abort();
                log_stream(
                    &stats.lock().expect("stream statistics"),
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

async fn read_provider_events(
    endpoint: String,
    key: String,
    body: Vec<u8>,
    received_bytes: Arc<AtomicU64>,
    received_chunks: Arc<AtomicU64>,
    started: Instant,
    stats: Arc<Mutex<StreamStats>>,
) -> Result<ChatTurn, AgentError> {
    let response = client()?
        .post(&endpoint)
        .header("content-type", "application/json")
        .header("accept", "text/event-stream")
        .bearer_auth(key)
        .body(body)
        .send()
        .await
        .map_err(|_| unavailable())?;
    tracing::info!(
        event = "llm_response_headers",
        status = response.status().as_u16(),
        content_type = response_type(response.headers()),
        elapsed_ms = started.elapsed().as_millis() as u64
    );
    if !response.status().is_success() {
        return Err(AgentError::new(
            "AGENT_PROVIDER_UNAVAILABLE",
            format!("configured provider returned HTTP {}", response.status()),
        ));
    }
    let stream = response.bytes_stream().map(move |chunk| {
        if let Ok(bytes) = &chunk {
            received_bytes.fetch_add(bytes.len() as u64, Ordering::Relaxed);
            received_chunks.fetch_add(1, Ordering::Relaxed);
        }
        chunk.map_err(|error| {
            tracing::warn!(
                event = "llm_stream_failure",
                stage = "http_body",
                timeout = error.is_timeout(),
                decode = error.is_decode(),
                body = error.is_body()
            );
            std::io::Error::other("provider transport failed")
        })
    });
    let mut events = stream.eventsource();
    let mut turn = ChatTurn::default();
    let mut calls = BTreeMap::new();
    let mut done = false;
    while let Some(event) = events.next().await {
        let event = event.map_err(|_| {
            tracing::warn!(event = "llm_stream_failure", stage = "sse_frame");
            interrupted()
        })?;
        if event.data == "[DONE]" {
            tracing::info!(
                event = "llm_sse_done",
                elapsed_ms = started.elapsed().as_millis() as u64
            );
            done = true;
            break;
        }
        apply_event(&mut turn, &mut calls, &event.data, &stats, started)?;
    }
    if !done {
        return Err(interrupted());
    }
    turn.tool_calls = calls.into_values().collect();
    if turn.finish_reason != "tool_calls" {
        return Err(invalid());
    }
    for call in &turn.tool_calls {
        serde_json::from_str::<Value>(&call.arguments).map_err(|_| invalid())?;
    }
    if !super::valid_tool_turn(&turn) {
        return Err(invalid());
    }
    Ok(turn)
}

fn apply_event(
    turn: &mut ChatTurn,
    calls: &mut BTreeMap<u64, ChatToolCall>,
    data: &str,
    stats: &Arc<Mutex<StreamStats>>,
    started: Instant,
) -> Result<(), AgentError> {
    let value: Value = serde_json::from_str(data).map_err(|_| invalid())?;
    if value.get("error").is_some() {
        return Err(invalid());
    }
    if value["usage"].is_object() {
        turn.usage = Some(ChatUsage {
            prompt_tokens: value["usage"]["prompt_tokens"].as_u64(),
            completion_tokens: value["usage"]["completion_tokens"].as_u64(),
            total_tokens: value["usage"]["total_tokens"].as_u64(),
            cached_tokens: value["usage"]["prompt_tokens_details"]["cached_tokens"]
                .as_u64()
                .filter(|&count| count != 0),
            reasoning_tokens: value["usage"]["completion_tokens_details"]["reasoning_tokens"]
                .as_u64()
                .filter(|&count| count != 0),
        });
    }
    if let Some(reason) = value["choices"][0]["finish_reason"].as_str() {
        turn.finish_reason = reason.to_owned();
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
    if let Some(text) = value["choices"][0]["delta"]["content"].as_str() {
        turn.content.push_str(text);
        let mut stats = stats.lock().expect("stream statistics");
        stats.text_delta_bytes = stats.text_delta_bytes.saturating_add(text.len() as u64);
    }
    if let Some(items) = value["choices"][0]["delta"]["tool_calls"].as_array() {
        let mut stats = stats.lock().expect("stream statistics");
        stats.tool_events = stats.tool_events.saturating_add(items.len() as u64);
        drop(stats);
        for item in items {
            let index = item["index"].as_u64().unwrap_or(0);
            let slot = calls.entry(index).or_default();
            if let Some(id) = item["id"].as_str() {
                slot.id = id.to_owned();
            }
            if let Some(name) = item["function"]["name"].as_str() {
                slot.name.push_str(name);
            }
            if let Some(arguments) = item["function"]["arguments"].as_str() {
                slot.arguments.push_str(arguments);
            }
        }
    }
    let mut stats = stats.lock().expect("stream statistics");
    if stats.events == 0 {
        tracing::info!(
            event = "llm_first_sse_event",
            elapsed_ms = started.elapsed().as_millis() as u64
        );
    }
    stats.events += 1;
    stats.last_event_ms = Some(started.elapsed().as_millis() as u64);
    Ok(())
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

    #[tokio::test]
    async fn truncated_http_body_is_transport_failure_without_accepting_partial_calls() {
        let body = event(
            json!({"role":"assistant","tool_calls":[{"index":0,"id":"partial","type":"function","function":{"name":"inspect_analysis","arguments":"{"}}]}),
            Value::Null,
        );
        let (result, requests, _) = exchange_body(200, body, false, true).await;
        assert_eq!(result.unwrap_err().code, "AGENT_TRANSPORT_INTERRUPTED");
        assert_eq!(requests.len(), 1);
    }

    #[test]
    fn response_type_never_exposes_arbitrary_header_values() {
        let mut headers = reqwest::header::HeaderMap::new();
        assert_eq!(response_type(&headers), "missing");
        for (value, expected) in [
            ("Text/Event-Stream; charset=utf-8", "sse"),
            ("application/json", "json"),
            ("text/html; detail=private-provider-value", "html"),
            ("private-provider-value", "other"),
        ] {
            headers.insert(reqwest::header::CONTENT_TYPE, value.parse().unwrap());
            assert_eq!(response_type(&headers), expected);
        }
    }

    async fn exchange(
        status: u16,
        response: String,
        hold: bool,
    ) -> (Result<ChatTurn, AgentError>, Vec<Vec<u8>>, StreamStats) {
        exchange_body(status, response, hold, false).await
    }

    async fn exchange_body(
        status: u16,
        response: String,
        hold: bool,
        truncated: bool,
    ) -> (Result<ChatTurn, AgentError>, Vec<Vec<u8>>, StreamStats) {
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
                    let size = response.len()+usize::from(hold || truncated)*10000;
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
        let stats = Arc::default();
        let result = send(
            &endpoint,
            "local-fixture",
            &request,
            Duration::from_millis(if hold { 200 } else { 2000 }),
            Arc::clone(&stats),
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
        let observed = stats.lock().expect("stream statistics").clone();
        (result, bodies, observed)
    }

    #[tokio::test]
    async fn eventsource_turn_accepts_done_without_waiting_for_http_eof() {
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
        let (held, _, _) = exchange(200, complete.clone(), true).await;
        assert!(
            held.is_ok(),
            "[DONE] must complete without waiting for HTTP EOF"
        );
        let (result, _, _) = exchange(200, complete, false).await;
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
        let (result, _, _) = exchange(200, format!("{tool}{finish}data: [DONE]\n\n"), false).await;
        assert_eq!(result.unwrap().usage, None);
        let partial_usage = format!(
            "data: {}\n\n",
            json!({"choices":[],"usage":{"prompt_tokens":120,"total_tokens":120,"prompt_tokens_details":{},"completion_tokens_details":{}}})
        );
        let (result, _, _) = exchange(
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
        let (result, _, _) = exchange(
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
            let (result, _, _) = exchange(200, body, false).await;
            assert!(result.is_err(), "{name} must not execute tools: {result:?}");
        }
        for status in [400, 413, 429, 503] {
            let (result, _, _) =
                exchange(status, "truncated provider error body".into(), true).await;
            let error = result.unwrap_err();
            assert_eq!(error.code, "AGENT_PROVIDER_UNAVAILABLE");
            assert!(error.message.contains(&format!("HTTP {status}")));
            assert!(!error.message.contains("truncated provider"));
        }
        let (result, _, _) = exchange(200, tool, true).await;
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
            Arc::default(),
        )
        .await;
        server.abort();
        assert_eq!(result.unwrap_err().code, "AGENT_TURN_TIMEOUT");
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "timeout must not wait for the hung connection drop"
        );
    }

    #[tokio::test]
    async fn dropping_send_aborts_the_in_flight_provider_stream() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!(
            "http://{}/v1/chat/completions",
            listener.local_addr().unwrap()
        );
        let (accepted, mut accepted_rx) = mpsc::unbounded_channel();
        let (closed, mut closed_rx) = mpsc::unbounded_channel();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            let mut block = [0; 4096];
            let end = loop {
                let n = socket.read(&mut block).await.unwrap();
                if n == 0 {
                    return;
                }
                bytes.extend_from_slice(&block[..n]);
                if let Some(i) = bytes.windows(4).position(|b| b == b"\r\n\r\n") {
                    break i + 4;
                }
            };
            let headers = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
            let size = headers
                .lines()
                .find_map(|s| s.strip_prefix("content-length:"))
                .unwrap()
                .trim()
                .parse::<usize>()
                .unwrap();
            while bytes.len() < end + size {
                let n = socket.read(&mut block).await.unwrap();
                if n == 0 {
                    return;
                }
                bytes.extend_from_slice(&block[..n]);
            }
            accepted.send(()).unwrap();
            loop {
                match socket.read(&mut block).await {
                    Ok(0) | Err(_) => {
                        let _ = closed.send(());
                        return;
                    }
                    Ok(_) => {}
                }
            }
        });
        let mut send = Some(Box::pin(send(
            &endpoint,
            "local-fixture",
            b"{}",
            Duration::from_secs(30),
            Arc::default(),
        )));
        tokio::select! {
            biased;
            result = send.as_mut().unwrap().as_mut() => {
                panic!("in-flight send completed before drop: {result:?}")
            }
            _ = accepted_rx.recv() => {}
        }
        let started = Instant::now();
        send.take();
        tokio::time::timeout(Duration::from_secs(2), closed_rx.recv())
            .await
            .expect("dropping send must abort the spawned HTTP stream")
            .expect("server closed");
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "abort must not wait for the unused send timeout"
        );
        server.abort();
    }
}
