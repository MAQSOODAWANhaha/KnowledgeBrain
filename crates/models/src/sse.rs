//! OpenAI-compatible SSE (`text/event-stream`) for every LLM HTTP call.

use serde_json::Value;

pub fn looks_like_sse(body: &str) -> bool {
    body.lines().any(|l| l.trim_start().starts_with("data:"))
}

/// Concatenate `choices[0].delta.content` from an SSE body. Ignores
/// `reasoning_content` (thinking tokens). Falls back to a unary JSON body.
pub fn collect_chat_content(body: &str) -> Result<String, String> {
    if looks_like_sse(body) {
        let mut content = String::new();
        for data in sse_data_payloads(body) {
            if data == "[DONE]" {
                break;
            }
            let v: Value = serde_json::from_str(&data)
                .map_err(|e| format!("sse chat json: {e}: {}", truncate(&data, 180)))?;
            if let Some(t) = v["choices"][0]["delta"]["content"].as_str() {
                content.push_str(t);
            } else if let Some(t) = v["choices"][0]["message"]["content"].as_str()
                && content.is_empty()
            {
                content.push_str(t);
            }
        }
        return Ok(content);
    }
    let v: Value = serde_json::from_str(body.trim()).map_err(|e| format!("chat json: {e}"))?;
    Ok(v["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .to_string())
}

/// Last JSON object from an SSE stream, or the unary JSON body.
pub fn last_json_value(body: &str) -> Result<Value, String> {
    if looks_like_sse(body) {
        let mut last: Option<Value> = None;
        for data in sse_data_payloads(body) {
            if data == "[DONE]" {
                break;
            }
            match serde_json::from_str::<Value>(&data) {
                Ok(v) => last = Some(v),
                Err(e) => {
                    return Err(format!("sse json: {e}: {}", truncate(&data, 180)));
                }
            }
        }
        return last.ok_or_else(|| "sse stream had no json data".into());
    }
    serde_json::from_str(body.trim()).map_err(|e| format!("json: {e}"))
}

fn sse_data_payloads(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in body.lines() {
        let Some(rest) = line.strip_prefix("data:") else {
            continue;
        };
        let data = rest.trim();
        if !data.is_empty() {
            out.push(data.to_string());
        }
    }
    out
}

pub(crate) fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_sse_keeps_content_drops_reasoning() {
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"think\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"OK\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n",
        );
        assert_eq!(collect_chat_content(body).unwrap(), "OK");
    }

    #[test]
    fn chat_sse_concatenates_deltas() {
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"hel\"}}]}\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n",
            "data: [DONE]\n",
        );
        assert_eq!(collect_chat_content(body).unwrap(), "hello");
    }

    #[test]
    fn unary_json_still_reads_message() {
        let body = r#"{"choices":[{"message":{"content":"hi"}}]}"#;
        assert_eq!(collect_chat_content(body).unwrap(), "hi");
    }

    #[test]
    fn last_json_from_sse_or_plain() {
        let sse = concat!(
            "data: {\"data\":[{\"embedding\":[1.0]}]}\n\n",
            "data: [DONE]\n",
        );
        let v = last_json_value(sse).unwrap();
        assert_eq!(v["data"][0]["embedding"][0], 1.0);
        let plain = r#"{"data":[{"embedding":[2.0]}]}"#;
        let v = last_json_value(plain).unwrap();
        assert_eq!(v["data"][0]["embedding"][0], 2.0);
    }
}
