//! Blocking OpenAI-compatible POST. Chat/VLM always request SSE.

use std::io::Read;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use reqwest::Client as AsyncClient;
use reqwest::blocking::Client;
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde_json::{Value, json};

use crate::models::sse;
use crate::models::sse::{ChatTurn, truncate};

pub const CHAT_TIMEOUT: Duration = Duration::from_secs(300);
pub const AGENT_TURN_TIMEOUT: Duration = Duration::from_secs(300);
pub const EMBED_TIMEOUT: Duration = Duration::from_secs(300);
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
pub const HTTP_ATTEMPTS: u32 = 3;
pub const AGENT_HTTP_ATTEMPTS: u32 = 2;

fn async_client() -> Result<&'static AsyncClient, String> {
    static CLIENT: OnceLock<Result<AsyncClient, String>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            AsyncClient::builder()
                .connect_timeout(CONNECT_TIMEOUT)
                .pool_max_idle_per_host(8)
                .build()
                .map_err(|error| format!("async llm http client: {error}"))
        })
        .as_ref()
        .map_err(Clone::clone)
}

fn client() -> Result<&'static Client, String> {
    static CLIENT: OnceLock<Result<Client, String>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            Client::builder()
                .connect_timeout(CONNECT_TIMEOUT)
                .timeout(CHAT_TIMEOUT)
                .pool_max_idle_per_host(8)
                .build()
                .map_err(|e| format!("llm http client: {e}"))
        })
        .as_ref()
        .map_err(|e| e.clone())
}

fn format_reqwest(err: reqwest::Error) -> String {
    let mut s = err.to_string();
    let mut src = std::error::Error::source(&err);
    while let Some(cur) = src {
        s.push_str(": ");
        s.push_str(&cur.to_string());
        src = cur.source();
    }
    s
}

pub fn is_retryable(err: &str) -> bool {
    let e = err.to_ascii_lowercase();
    e.contains("timeout")
        || e.contains("timed out")
        || e.contains("connection")
        || e.contains("connect")
        || e.contains("sendrequest")
        || e.contains("sse read:")
        || e.contains("request or response body error")
        || e.contains("error decoding response body")
        || e.contains("unexpected eof")
        || e.contains("os error 110")
        || e.contains("llm http 429")
        || e.contains("llm http 502")
        || e.contains("llm http 503")
        || e.contains("llm http 504")
}

fn with_retry<T>(mut op: impl FnMut() -> Result<T, String>) -> Result<T, String> {
    let mut last = String::new();
    for attempt in 1..=HTTP_ATTEMPTS {
        match op() {
            Ok(v) => return Ok(v),
            Err(e) if attempt < HTTP_ATTEMPTS && is_retryable(&e) => {
                last = e;
                std::thread::sleep(Duration::from_millis(400 * (1 << (attempt - 1))));
            }
            Err(e) => return Err(e),
        }
    }
    Err(last)
}

/// POST JSON with `Accept: text/event-stream`. LLM calls always set `stream: true`.
pub fn post_llm(
    url: &str,
    api_key: &str,
    mut body: Value,
    _stream: bool,
    timeout: Duration,
) -> Result<String, String> {
    if let Some(obj) = body.as_object_mut() {
        obj.insert("stream".into(), json!(true));
    }
    let mut req = client()?
        .post(url)
        .timeout(timeout)
        .header(ACCEPT, "text/event-stream")
        .header(CONTENT_TYPE, "application/json")
        .json(&body);
    if !api_key.is_empty() {
        req = req.bearer_auth(api_key);
    }
    let resp = req.send().map_err(format_reqwest)?;
    let status = resp.status();
    let text = resp.text().map_err(format_reqwest)?;
    if !status.is_success() {
        return Err(format!(
            "llm http {status} {}: {}",
            url,
            truncate(&text, 240)
        ));
    }
    Ok(text)
}

fn post_json(url: &str, api_key: &str, body: &Value, timeout: Duration) -> Result<String, String> {
    let mut req = client()?
        .post(url)
        .timeout(timeout)
        .header(ACCEPT, "application/json")
        .header(CONTENT_TYPE, "application/json")
        .json(body);
    if !api_key.is_empty() {
        req = req.bearer_auth(api_key);
    }
    let resp = req.send().map_err(format_reqwest)?;
    let status = resp.status();
    let text = resp.text().map_err(format_reqwest)?;
    if !status.is_success() {
        return Err(format!(
            "llm http {status} {}: {}",
            url,
            truncate(&text, 240)
        ));
    }
    Ok(text)
}

pub fn chat_sse(url: &str, api_key: &str, body: Value) -> Result<String, String> {
    with_retry(|| {
        let raw = post_llm(url, api_key, body.clone(), true, CHAT_TIMEOUT)?;
        let text = sse::collect_chat_content(&raw)?;
        let text = text.trim().to_string();
        if text.is_empty() {
            return Err("chat returned empty".into());
        }
        Ok(text)
    })
}

fn with_retry_n<T>(attempts: u32, mut op: impl FnMut() -> Result<T, String>) -> Result<T, String> {
    let mut last = String::new();
    for attempt in 1..=attempts {
        match op() {
            Ok(v) => return Ok(v),
            Err(e) if attempt < attempts && is_retryable(&e) => {
                last = e;
                std::thread::sleep(Duration::from_millis(400 * (1 << (attempt - 1))));
            }
            Err(e) => return Err(e),
        }
    }
    Err(last)
}

fn post_llm_turn(
    url: &str,
    api_key: &str,
    mut body: Value,
    timeout: Duration,
) -> Result<ChatTurn, String> {
    if let Some(obj) = body.as_object_mut() {
        obj.insert("stream".into(), json!(true));
    }
    let mut req = client()?
        .post(url)
        .timeout(timeout)
        .header(ACCEPT, "text/event-stream")
        .header(CONTENT_TYPE, "application/json")
        .json(&body);
    if !api_key.is_empty() {
        req = req.bearer_auth(api_key);
    }
    let mut resp = req.send().map_err(format_reqwest)?;
    let status = resp.status();
    if !status.is_success() {
        let mut text = String::new();
        resp.read_to_string(&mut text).ok();
        return Err(format!(
            "llm http {status} {}: {}",
            url,
            truncate(&text, 240)
        ));
    }
    sse::consume_sse_read(&mut resp)
}

pub fn chat_sse_turn_once(
    url: &str,
    api_key: &str,
    body: Value,
    timeout: Duration,
) -> Result<ChatTurn, String> {
    let turn = post_llm_turn(url, api_key, body, timeout)?;
    if turn.content.trim().is_empty() && turn.tool_calls.is_empty() {
        return Err("chat returned empty".into());
    }
    Ok(turn)
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ChatTransportError {
    #[error("chat transport timeout: {0}")]
    Timeout(String),
    #[error("chat provider unavailable: {0}")]
    Unavailable(String),
    #[error("chat provider returned HTTP {0}")]
    HttpStatus(reqwest::StatusCode),
    #[error("chat response invalid: {0}")]
    Response(String),
}

/// One cancellation-safe async transport attempt. Dropping this future drops
/// the in-flight reqwest request; no internal retry or response-format fallback
/// is performed.
pub async fn chat_sse_turn_once_async(
    url: &str,
    api_key: &str,
    mut body: Value,
    timeout: Duration,
) -> Result<ChatTurn, ChatTransportError> {
    if let Some(object) = body.as_object_mut() {
        object.insert("stream".into(), json!(true));
    }
    let body = serde_json::to_vec(&body).map_err(|error| {
        ChatTransportError::Response(format!("chat request JSON invalid: {error}"))
    })?;
    chat_sse_turn_once_async_bytes(url, api_key, &body, timeout).await
}

/// Send the caller-frozen provider request bytes without another serialization.
#[tracing::instrument(name = "llm_http_turn", skip_all, fields(request_bytes = body.len()))]
pub async fn chat_sse_turn_once_async_bytes(
    url: &str,
    api_key: &str,
    body: &[u8],
    timeout: Duration,
) -> Result<ChatTurn, ChatTransportError> {
    let started = Instant::now();
    let mut request = async_client()
        .map_err(ChatTransportError::Unavailable)?
        .post(url)
        .timeout(timeout)
        .header(ACCEPT, "text/event-stream")
        .header(CONTENT_TYPE, "application/json")
        .body(body.to_vec());
    if !api_key.is_empty() {
        request = request.bearer_auth(api_key);
    }
    let mut response = request.send().await.map_err(|error| {
        let timed_out = error.is_timeout();
        let message = format_reqwest(error);
        if timed_out {
            ChatTransportError::Timeout(message)
        } else {
            ChatTransportError::Unavailable(message)
        }
    })?;
    let status = response.status();
    tracing::info!(
        event = "llm_response_headers",
        status = status.as_u16(),
        elapsed_ms = started.elapsed().as_millis() as u64,
    );
    if !status.is_success() {
        // Keep the typed status even if the error body is incomplete. Provider
        // bodies may echo request text or credentials, so do not include them.
        return Err(ChatTransportError::HttpStatus(status));
    }
    let mut bytes = Vec::new();
    let mut line_start = 0;
    let mut event_data_lines = 0;
    let mut event_done = false;
    let mut first_chunk_received = false;
    'body: while let Some(chunk) = response.chunk().await.map_err(|error| {
        let timed_out = error.is_timeout();
        let message = format_reqwest(error);
        if timed_out {
            ChatTransportError::Timeout(message)
        } else {
            ChatTransportError::Unavailable(message)
        }
    })? {
        if !first_chunk_received {
            first_chunk_received = true;
            // This is the first HTTP body chunk, not necessarily a model token:
            // providers may send SSE comments or metadata before output.
            tracing::info!(
                event = "llm_first_body_chunk",
                elapsed_ms = started.elapsed().as_millis() as u64,
            );
        }
        let offset = bytes.len();
        bytes.extend_from_slice(&chunk);
        for (index, byte) in chunk.iter().enumerate() {
            if *byte != b'\n' {
                continue;
            }
            let end = offset + index;
            let line = bytes[line_start..end]
                .strip_suffix(b"\r")
                .unwrap_or(&bytes[line_start..end]);
            if line.is_empty() {
                // SSE completion is independent of HTTP EOF. Require the
                // complete terminal event, not a marker inside JSON or an
                // unfinished data line. Preserve the existing turn parser.
                if event_data_lines == 1 && event_done {
                    bytes.truncate(end + 1);
                    break 'body;
                }
                event_data_lines = 0;
                event_done = false;
            } else if let Some(data) = line.strip_prefix(b"data:") {
                event_data_lines += 1;
                event_done = data.trim_ascii() == b"[DONE]";
            }
            line_start = end + 1;
        }
    }
    let text = String::from_utf8_lossy(&bytes);
    let turn = sse::collect_chat_turn(&text).map_err(ChatTransportError::Response)?;
    if turn.content.trim().is_empty() && turn.tool_calls.is_empty() {
        return Err(ChatTransportError::Response("chat returned empty".into()));
    }
    tracing::info!(
        event = "llm_response_completed",
        usage_reported = turn.usage.is_some(),
        prompt_tokens = turn.usage.as_ref().and_then(|u| u.prompt_tokens),
        completion_tokens = turn.usage.as_ref().and_then(|u| u.completion_tokens),
        cached_tokens = turn.usage.as_ref().and_then(|u| u.cached_tokens),
        reasoning_tokens = turn.usage.as_ref().and_then(|u| u.reasoning_tokens),
        elapsed_ms = started.elapsed().as_millis() as u64,
        response_bytes = bytes.len(),
        tool_calls = turn.tool_calls.len(),
    );
    Ok(turn)
}

pub fn chat_sse_turn(
    url: &str,
    api_key: &str,
    body: Value,
    timeout: Duration,
) -> Result<ChatTurn, String> {
    with_retry_n(AGENT_HTTP_ATTEMPTS, || {
        chat_sse_turn_once(url, api_key, body.clone(), timeout)
    })
}

/// Embeddings: unary JSON (not SSE). Transient failures retry 3 times.
pub fn json_sse(url: &str, api_key: &str, body: Value, _stream: bool) -> Result<Value, String> {
    with_retry(|| {
        let raw = post_json(url, api_key, &body, EMBED_TIMEOUT)?;
        sse::last_json_value(&raw)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retries_timeouts_and_gateway_errors() {
        assert!(is_retryable(
            "error sending request: connection error: Connection timed out (os error 110)"
        ));
        assert!(is_retryable(
            "llm http 503 https://example/embeddings: busy"
        ));
        assert!(is_retryable("llm http 429"));
        assert!(is_retryable("sse read: request or response body error"));
        assert!(is_retryable("error decoding response body: unexpected EOF"));
        assert!(!is_retryable("llm http 400 https://example: bad model"));
        assert!(!is_retryable("embed missing vector"));
        assert!(!is_retryable("chat returned empty"));
    }

    #[tokio::test]
    async fn async_byte_transport_sends_exact_reserved_body_without_reserialization() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            let body_start = loop {
                let mut chunk = [0; 1024];
                let count = socket.read(&mut chunk).await.unwrap();
                assert!(count > 0);
                bytes.extend_from_slice(&chunk[..count]);
                if let Some(index) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                    break index + 4;
                }
            };
            let headers = String::from_utf8(bytes[..body_start].to_vec())
                .unwrap()
                .to_ascii_lowercase();
            assert!(headers.contains("content-type: application/json\r\n"));
            let length: usize = headers
                .lines()
                .find_map(|line| line.strip_prefix("content-length: "))
                .unwrap()
                .parse()
                .unwrap();
            while bytes.len() < body_start + length {
                let mut chunk = [0; 1024];
                let count = socket.read(&mut chunk).await.unwrap();
                assert!(count > 0);
                bytes.extend_from_slice(&chunk[..count]);
            }
            let response = "data: {\"choices\":[{\"delta\":{\"content\":\"{}\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
            socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).as_bytes()).await.unwrap();
            bytes[body_start..body_start + length].to_vec()
        });
        let body = "{\"max_tokens\":8192,\"messages\":[{\"content\":\"𠀀\",\"role\":\"user\"}],\"model\":\"frozen\",\"stream\":true}".as_bytes();
        let turn = chat_sse_turn_once_async_bytes(
            &format!("http://{address}/v1/chat/completions"),
            "",
            body,
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        assert_eq!(turn.content, "{}");
        assert_eq!(server.await.unwrap(), body);
    }

    #[tokio::test]
    async fn async_http_status_survives_a_truncated_error_body_without_exposing_it() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        for code in [400, 413, 429, 503] {
            let status = reqwest::StatusCode::from_u16(code).unwrap();
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = [0; 4096];
                assert!(socket.read(&mut request).await.unwrap() > 0);
                // Error bodies can echo credentials/source text or fail during
                // download. Neither should hide the received status code.
                socket.write_all(format!("HTTP/1.1 {status}\r\nContent-Length: 1000\r\nConnection: close\r\n\r\nechoed-secret-and-tender-text").as_bytes()).await.unwrap();
            });
            let error = chat_sse_turn_once_async_bytes(
                &format!("http://{address}/v1/chat/completions"),
                "request-secret",
                b"{}",
                Duration::from_secs(5),
            )
            .await
            .unwrap_err();
            server.await.unwrap();
            assert_eq!(
                error.to_string(),
                format!("chat provider returned HTTP {status}")
            );
            assert!(!format!("{error:?}").contains("secret"));
        }
    }

    async fn unfinished_http_stream(
        fragments: Vec<Vec<u8>>,
        timeout: Duration,
    ) -> Result<ChatTurn, ChatTransportError> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (release, released) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 4096];
            assert!(socket.read(&mut request).await.unwrap() > 0);
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n").await.unwrap();
            for fragment in fragments {
                let mut frame = format!("{:x}\r\n", fragment.len()).into_bytes();
                frame.extend_from_slice(&fragment);
                frame.extend_from_slice(b"\r\n");
                socket.write_all(&frame).await.unwrap();
            }
            // No terminating HTTP chunk. Keep the socket open until the client
            // returns, including on the old transport's request timeout.
            let _ = released.await;
        });
        let result = chat_sse_turn_once_async_bytes(
            &format!("http://{address}/v1/chat/completions"),
            "",
            b"{}",
            timeout,
        )
        .await;
        let _ = release.send(());
        server.await.unwrap();
        result
    }

    #[tokio::test]
    async fn async_sse_done_returns_before_http_eof_across_byte_boundaries() {
        for newline in ["\n", "\r\n"] {
            let event = json!({"choices":[{"delta":{"tool_calls":[{
                "index":0,"id":"call-check","type":"function",
                "function":{"name":"check","arguments":"{\"label\":\"原页𠀀\"}"}
            }]},"finish_reason":"tool_calls"}]});
            let response = format!("data: {event}{newline}{newline}data: [DONE]{newline}{newline}");
            let turn = unfinished_http_stream(
                response.bytes().map(|byte| vec![byte]).collect(),
                Duration::from_secs(2),
            )
            .await
            .expect("a complete SSE response must not wait for HTTP EOF");
            assert_eq!(turn.finish_reason, "tool_calls");
            assert_eq!(turn.tool_calls.len(), 1);
            assert_eq!(turn.tool_calls[0].name, "check");
            assert_eq!(turn.tool_calls[0].arguments, "{\"label\":\"原页𠀀\"}");
        }
    }

    #[tokio::test]
    async fn async_sse_incomplete_terminal_event_does_not_complete_the_turn() {
        let response = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"[DONE]\"}}]}\n\n",
            "data: [DONE]\n"
        );
        let result = unfinished_http_stream(
            vec![response.as_bytes().to_vec()],
            Duration::from_millis(100),
        )
        .await;
        assert!(matches!(result, Err(ChatTransportError::Timeout(_))));
    }

    #[tokio::test]
    async fn async_one_shot_preserves_reqwest_timeout_type() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (_socket, _) = listener.accept().await.unwrap();
            tokio::time::sleep(Duration::from_secs(1)).await;
        });
        let error = chat_sse_turn_once_async(
            &format!("http://{address}/v1/chat/completions"),
            "test-key",
            json!({"model":"frozen","messages":[]}),
            Duration::from_millis(20),
        )
        .await
        .expect_err("hanging transport must time out");
        assert!(matches!(error, ChatTransportError::Timeout(_)));
        server.abort();
    }
}
