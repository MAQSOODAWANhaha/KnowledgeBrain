//! OpenAI-compatible chat; `stub-chat` / missing URL stays local.

use serde_json::json;

pub fn chat_http_configured() -> bool {
    !domain::chat_base_url().is_empty()
}

fn resolve_chat_model(model_id: &str) -> String {
    if model_id.trim().is_empty() || model_id == "stub-chat" {
        let env = domain::chat_model();
        if env.is_empty() {
            "stub-chat".into()
        } else {
            env
        }
    } else {
        model_id.trim().to_string()
    }
}

pub fn chat_complete(system: &str, user: &str, model_id: &str) -> Result<String, String> {
    chat_complete_limited(system, user, model_id, 2048)
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
    if tokio::runtime::Handle::try_current().is_ok() {
        tokio::task::block_in_place(|| chat_complete_inner(system, user, model_id, max_tokens))
    } else {
        chat_complete_inner(system, user, model_id, max_tokens)
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
    let base = domain::chat_base_url();
    let model = resolve_chat_model(model_id);
    if base.is_empty() || model == "stub-chat" {
        return Ok(stub_complete(user));
    }
    let key = domain::chat_api_key();
    let url = completions_url(&base);
    let body = json!({
        "model": model,
        "temperature": 0.3,
        "max_tokens": max_tokens,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user}
        ]
    });
    models::chat_sse(&url, &key, body)
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
}
