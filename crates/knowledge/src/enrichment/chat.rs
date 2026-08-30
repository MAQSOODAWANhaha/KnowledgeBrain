//! OpenAI-compatible chat; `stub-chat` / missing URL stays local.

use serde_json::{Value, json};

pub fn chat_http_configured() -> bool {
    !crate::chat_base_url().is_empty()
}

fn resolve_chat_model(model_id: &str) -> String {
    if model_id.trim().is_empty() || model_id == "stub-chat" {
        let env = crate::chat_model();
        if env.is_empty() {
            "stub-chat".into()
        } else {
            env
        }
    } else {
        model_id.trim().to_string()
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".into(),
            content: content.into(),
        }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: content.into(),
        }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".into(),
            content: content.into(),
        }
    }
}

pub fn chat_complete(system: &str, user: &str, model_id: &str) -> Result<String, String> {
    chat_complete_limited(system, user, model_id, 2048)
}

pub fn chat_messages(messages: &[ChatMessage], model_id: &str) -> Result<String, String> {
    chat_messages_limited(messages, model_id, 2048)
}

/// Brain `wikiLLMMaxTokens` — large Chinese extracts otherwise truncate mid-JSON.
pub const WIKI_LLM_MAX_TOKENS: u32 = 32768;
/// Brain `wikiLLMMaxAttempts`.
pub const WIKI_LLM_MAX_ATTEMPTS: u32 = 3;
/// Brain `wikiLLMBackoffBase`.
pub const WIKI_LLM_BACKOFF_BASE: std::time::Duration = std::time::Duration::from_secs(2);

pub fn chat_complete_limited(
    system: &str,
    user: &str,
    model_id: &str,
    max_tokens: u32,
) -> Result<String, String> {
    match tokio::runtime::Handle::try_current() {
        Ok(h) if h.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(|| chat_complete_inner(system, user, model_id, max_tokens))
        }
        _ => chat_complete_inner(system, user, model_id, max_tokens),
    }
}

/// Brain `generateWithTemplate`: 32768 completion tokens, 3 attempts, 2s/4s/8s.
pub fn chat_complete_wiki(system: &str, user: &str, model_id: &str) -> Result<String, String> {
    let mut last = "chat returned empty".to_string();
    for attempt in 1..=WIKI_LLM_MAX_ATTEMPTS {
        match chat_complete_limited(system, user, model_id, WIKI_LLM_MAX_TOKENS) {
            Ok(text) if !text.trim().is_empty() => return Ok(text),
            Ok(_) => last = "chat returned empty".into(),
            Err(e) => last = e,
        }
        if attempt < WIKI_LLM_MAX_ATTEMPTS {
            std::thread::sleep(WIKI_LLM_BACKOFF_BASE * (1 << (attempt - 1)));
        }
    }
    Err(last)
}

fn chat_complete_inner(
    system: &str,
    user: &str,
    model_id: &str,
    max_tokens: u32,
) -> Result<String, String> {
    chat_messages_inner(
        &[ChatMessage::system(system), ChatMessage::user(user)],
        model_id,
        max_tokens,
    )
}

pub fn chat_complete_turn(
    system: &str,
    user: &str,
    model_id: &str,
    max_tokens: u32,
    timeout: std::time::Duration,
) -> Result<crate::models::ChatTurn, String> {
    chat_complete_turn_with_format(system, user, model_id, max_tokens, timeout, None)
}

pub fn chat_complete_turn_with_format(
    system: &str,
    user: &str,
    model_id: &str,
    max_tokens: u32,
    timeout: std::time::Duration,
    response_format: Option<&serde_json::Value>,
) -> Result<crate::models::ChatTurn, String> {
    chat_tools_turn_with_format(
        &[
            serde_json::json!({"role": "system", "content": system}),
            serde_json::json!({"role": "user", "content": user}),
        ],
        &serde_json::json!([]),
        model_id,
        max_tokens,
        timeout,
        response_format,
    )
}

/// One transport attempt. Domain orchestrators use this to own a single,
/// non-multiplicative retry budget across transport and structured-output errors.
pub fn chat_complete_turn_with_format_once(
    system: &str,
    user: &str,
    model_id: &str,
    max_tokens: u32,
    timeout: std::time::Duration,
    response_format: Option<&serde_json::Value>,
) -> Result<crate::models::ChatTurn, String> {
    chat_tools_turn_with_format_once(
        &[
            serde_json::json!({"role": "system", "content": system}),
            serde_json::json!({"role": "user", "content": user}),
        ],
        &serde_json::json!([]),
        model_id,
        max_tokens,
        timeout,
        response_format,
    )
}

pub fn chat_tools_turn(
    messages: &[serde_json::Value],
    tools: &serde_json::Value,
    model_id: &str,
    max_tokens: u32,
    timeout: std::time::Duration,
) -> Result<crate::models::ChatTurn, String> {
    chat_tools_turn_with_format(messages, tools, model_id, max_tokens, timeout, None)
}

pub fn chat_tools_turn_with_format(
    messages: &[serde_json::Value],
    tools: &serde_json::Value,
    model_id: &str,
    max_tokens: u32,
    timeout: std::time::Duration,
    response_format: Option<&serde_json::Value>,
) -> Result<crate::models::ChatTurn, String> {
    chat_tools_turn_with_format_mode(
        messages,
        tools,
        model_id,
        max_tokens,
        timeout,
        response_format,
        true,
    )
}

pub fn chat_tools_turn_with_format_once(
    messages: &[serde_json::Value],
    tools: &serde_json::Value,
    model_id: &str,
    max_tokens: u32,
    timeout: std::time::Duration,
    response_format: Option<&serde_json::Value>,
) -> Result<crate::models::ChatTurn, String> {
    chat_tools_turn_with_format_mode(
        messages,
        tools,
        model_id,
        max_tokens,
        timeout,
        response_format,
        false,
    )
}

fn chat_tools_turn_with_format_mode(
    messages: &[serde_json::Value],
    tools: &serde_json::Value,
    model_id: &str,
    max_tokens: u32,
    timeout: std::time::Duration,
    response_format: Option<&serde_json::Value>,
    retry_transport: bool,
) -> Result<crate::models::ChatTurn, String> {
    match tokio::runtime::Handle::try_current() {
        Ok(h) if h.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(|| {
                chat_tools_turn_inner(
                    messages,
                    tools,
                    model_id,
                    max_tokens,
                    timeout,
                    response_format,
                    retry_transport,
                )
            })
        }
        _ => chat_tools_turn_inner(
            messages,
            tools,
            model_id,
            max_tokens,
            timeout,
            response_format,
            retry_transport,
        ),
    }
}

fn chat_tools_turn_inner(
    messages: &[serde_json::Value],
    tools: &serde_json::Value,
    model_id: &str,
    max_tokens: u32,
    timeout: std::time::Duration,
    response_format: Option<&serde_json::Value>,
    retry_transport: bool,
) -> Result<crate::models::ChatTurn, String> {
    let base = crate::chat_base_url();
    let model = resolve_chat_model(model_id);
    if base.is_empty() || model == "stub-chat" {
        let last = messages
            .iter()
            .rev()
            .find(|m| m.get("role").and_then(|v| v.as_str()) == Some("user"))
            .and_then(|m| m.get("content").and_then(|v| v.as_str()))
            .unwrap_or("");
        return Ok(crate::models::ChatTurn {
            content: stub_complete(last),
            tool_calls: Vec::new(),
            finish_reason: "stop".into(),
        });
    }
    let key = crate::chat_api_key();
    let url = completions_url(&base);
    let mut body = json!({
        "model": model,
        "temperature": 0.3,
        "max_tokens": max_tokens,
        "messages": messages
    });
    if let Some(obj) = body.as_object_mut() {
        if let Ok(effort) = std::env::var("KNOWLEDGEBRAIN_CHAT_REASONING_EFFORT") {
            let effort = effort.trim().to_ascii_lowercase();
            if matches!(effort.as_str(), "low" | "medium" | "high") {
                obj.insert("reasoning_effort".into(), json!(effort));
            }
        }
        if tools
            .as_array()
            .map(|items| !items.is_empty())
            .unwrap_or(false)
        {
            obj.insert("tools".into(), tools.clone());
        }
        if let Some(format) = response_format {
            obj.insert("response_format".into(), format.clone());
        }
    }
    let call = |payload| {
        if retry_transport {
            crate::models::chat_sse_turn(&url, &key, payload, timeout)
        } else {
            crate::models::chat_sse_turn_once(&url, &key, payload, timeout)
        }
    };
    let result = call(body.clone());
    if let Err(error) = &result
        && let Some(fallback) = response_format_fallback(&body, response_format, error)
    {
        return call(fallback);
    }
    result
}

fn response_format_fallback(
    body: &serde_json::Value,
    response_format: Option<&serde_json::Value>,
    error: &str,
) -> Option<serde_json::Value> {
    if response_format
        .and_then(|format| format.get("type"))
        .and_then(Value::as_str)
        != Some("json_schema")
        || !format_unsupported(error)
    {
        return None;
    }
    let mut fallback = body.clone();
    fallback
        .as_object_mut()?
        .insert("response_format".into(), json!({"type": "json_object"}));
    Some(fallback)
}

fn format_unsupported(error: &str) -> bool {
    let e = error.to_ascii_lowercase();
    e.contains("llm http 400")
        && (e.contains("response_format")
            || e.contains("json_schema")
            || e.contains("json_object")
            || e.contains("unknown")
            || e.contains("unsupported"))
}

pub fn chat_messages_limited(
    messages: &[ChatMessage],
    model_id: &str,
    max_tokens: u32,
) -> Result<String, String> {
    match tokio::runtime::Handle::try_current() {
        Ok(h) if h.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(|| chat_messages_inner(messages, model_id, max_tokens))
        }
        _ => chat_messages_inner(messages, model_id, max_tokens),
    }
}

fn chat_messages_inner(
    messages: &[ChatMessage],
    model_id: &str,
    max_tokens: u32,
) -> Result<String, String> {
    let base = crate::chat_base_url();
    let model = resolve_chat_model(model_id);
    if base.is_empty() || model == "stub-chat" {
        let last = messages
            .iter()
            .rev()
            .find(|m| m.role == "user")
            .map(|m| m.content.as_str())
            .unwrap_or("");
        return Ok(stub_complete(last));
    }
    let key = crate::chat_api_key();
    let url = completions_url(&base);
    let body = json!({
        "model": model,
        "temperature": 0.3,
        "max_tokens": max_tokens,
        "messages": messages
    });
    crate::models::chat_sse(&url, &key, body)
}

pub(crate) fn completions_url_for_vlm(base: &str) -> String {
    completions_url(base)
}

fn completions_url(base: &str) -> String {
    let b = base.trim_end_matches('/');
    if b.ends_with("/v1") {
        format!("{b}/chat/completions")
    } else if b.ends_with("/chat/completions") {
        b.to_string()
    } else {
        format!("{b}/v1/chat/completions")
    }
}

fn stub_complete(user: &str) -> String {
    let runes: Vec<char> = user.chars().collect();
    if runes.len() > 280 {
        format!("{}…", runes.into_iter().take(280).collect::<String>())
    } else {
        user.to_string()
    }
}

/// Brain `sampleLongContent`: head 60% / middle 20% / tail 20%.
pub fn sample_long_content(content: &str, max_chars: usize) -> String {
    let runes: Vec<char> = content.chars().collect();
    if runes.len() <= max_chars {
        return content.to_string();
    }
    const OMIT: &str = "\n\n[...content omitted...]\n\n";
    let omit_n = OMIT.chars().count();
    let usable = max_chars.saturating_sub(2 * omit_n);
    if usable < 100 {
        return runes.into_iter().take(max_chars).collect();
    }
    let head_len = usable * 60 / 100;
    let tail_len = usable * 20 / 100;
    let mid_len = usable - head_len - tail_len;
    let head: String = runes[..head_len].iter().collect();
    let tail: String = runes[runes.len() - tail_len..].iter().collect();
    let mut mid_start = runes.len() / 2 - mid_len / 2;
    if mid_start < head_len {
        mid_start = head_len;
    }
    let mut mid_end = mid_start + mid_len;
    if mid_end > runes.len() - tail_len {
        mid_end = runes.len() - tail_len;
        mid_start = mid_end.saturating_sub(mid_len).max(head_len);
    }
    let middle: String = runes[mid_start..mid_end].iter().collect();
    format!("{head}{OMIT}{middle}{OMIT}{tail}")
}

pub fn attempt_superseded(current: i32, job_attempt: i32) -> bool {
    job_attempt > 0 && current > job_attempt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_schema_unsupported_falls_back_to_json_object() {
        let body = json!({"model":"m","response_format":{"type":"json_schema"}});
        let strict = json!({"type":"json_schema"});
        let fallback = response_format_fallback(
            &body,
            Some(&strict),
            "LLM HTTP 400: response_format json_schema unsupported",
        )
        .expect("fallback payload");
        assert_eq!(fallback["response_format"], json!({"type":"json_object"}));
        assert!(response_format_fallback(&body, Some(&strict), "LLM HTTP 500").is_none());
    }

    #[test]
    fn sample_keeps_short() {
        assert_eq!(sample_long_content("hello", 100), "hello");
    }

    #[test]
    fn sample_marks_omit_on_long() {
        let s: String = (0..800).map(|_| '字').collect();
        let out = sample_long_content(&s, 200);
        assert!(out.contains("[...content omitted...]"));
        assert!(out.chars().count() <= 200);
    }

    #[test]
    fn superseded_only_when_newer_attempt() {
        assert!(!attempt_superseded(1, 0));
        assert!(!attempt_superseded(1, 1));
        assert!(attempt_superseded(2, 1));
    }

    #[test]
    fn stub_messages_echo_last_user() {
        let out = chat_messages(
            &[ChatMessage::system("sys"), ChatMessage::user("hello-user")],
            "stub-chat",
        )
        .unwrap();
        assert_eq!(out, "hello-user");
    }
}
