//! Blocking OpenAI-compatible POST. Chat/VLM always request SSE.

use std::sync::OnceLock;
use std::time::Duration;

use reqwest::blocking::Client;
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde_json::{Value, json};

use crate::sse;
use crate::sse::truncate;

pub const CHAT_TIMEOUT: Duration = Duration::from_secs(300);
pub const EMBED_TIMEOUT: Duration = Duration::from_secs(300);
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
pub const HTTP_ATTEMPTS: u32 = 3;

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
        assert!(!is_retryable("llm http 400 https://example: bad model"));
        assert!(!is_retryable("embed missing vector"));
        assert!(!is_retryable("chat returned empty"));
    }
}
