//! Blocking OpenAI-compatible POST. Chat/VLM always request SSE.

use std::sync::OnceLock;
use std::time::Duration;

use reqwest::blocking::Client;
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde_json::{Value, json};

use crate::sse;
use crate::sse::truncate;

pub const CHAT_TIMEOUT: Duration = Duration::from_secs(180);
pub const EMBED_TIMEOUT: Duration = Duration::from_secs(120);

fn client() -> Result<&'static Client, String> {
    static CLIENT: OnceLock<Result<Client, String>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            Client::builder()
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

/// POST JSON with `Accept: text/event-stream`. `stream: true` when `stream` is set.
pub fn post_llm(
    url: &str,
    api_key: &str,
    mut body: Value,
    stream: bool,
    timeout: Duration,
) -> Result<String, String> {
    if stream && let Some(obj) = body.as_object_mut() {
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

pub fn chat_sse(url: &str, api_key: &str, body: Value) -> Result<String, String> {
    let raw = post_llm(url, api_key, body, true, CHAT_TIMEOUT)?;
    let text = sse::collect_chat_content(&raw)?;
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("chat returned empty".into());
    }
    Ok(text)
}

pub fn json_sse(url: &str, api_key: &str, body: Value, stream: bool) -> Result<Value, String> {
    let raw = post_llm(url, api_key, body, stream, EMBED_TIMEOUT)?;
    sse::last_json_value(&raw)
}
