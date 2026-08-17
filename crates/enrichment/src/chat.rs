//! OpenAI-compatible chat; `stub-chat` / missing URL stays local.

use serde_json::json;

pub fn chat_http_configured() -> bool {
    !std::env::var("KNOWLEDGEBRAIN_CHAT_BASE_URL")
        .unwrap_or_default()
        .trim()
        .is_empty()
}

pub fn chat_complete(system: &str, user: &str, model_id: &str) -> Result<String, String> {
    if model_id == "stub-chat"
        || std::env::var("KNOWLEDGEBRAIN_CHAT_BASE_URL")
            .unwrap_or_default()
            .is_empty()
    {
        return Ok(stub_complete(user));
    }
    let base = std::env::var("KNOWLEDGEBRAIN_CHAT_BASE_URL").unwrap_or_default();
    let key = std::env::var("KNOWLEDGEBRAIN_CHAT_API_KEY").unwrap_or_default();
    let url = completions_url(&base);
    let model = if model_id.is_empty() {
        "stub-chat"
    } else {
        model_id
    };
    let body = json!({
        "model": model,
        "temperature": 0.3,
        "max_tokens": 2048,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user}
        ]
    });
    let mut req = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| e.to_string())?
        .post(url)
        .json(&body);
    if !key.is_empty() {
        req = req.bearer_auth(key);
    }
    let resp = req.send().map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("chat failed: {}", resp.status()));
    }
    let v: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
    let text = v["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();
    if text.is_empty() {
        return Err("chat returned empty".into());
    }
    Ok(text)
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
