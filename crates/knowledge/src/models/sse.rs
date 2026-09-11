//! OpenAI-compatible SSE (`text/event-stream`) for every LLM HTTP call.

use std::collections::BTreeMap;
use std::io::Read;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChatToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChatTurn {
    pub content: String,
    pub tool_calls: Vec<ChatToolCall>,
    pub finish_reason: String,
    pub usage: Option<ChatUsage>,
}

/// Provider-reported counts. Missing values stay unknown; cache/reasoning counts
/// are subsets of prompt/completion counts and must not be added to them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChatUsage {
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub cached_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
}

fn usage(value: &Value) -> Option<ChatUsage> {
    value.is_object().then(|| ChatUsage {
        prompt_tokens: value["prompt_tokens"].as_u64(),
        completion_tokens: value["completion_tokens"].as_u64(),
        total_tokens: value["total_tokens"].as_u64(),
        cached_tokens: value["prompt_tokens_details"]["cached_tokens"].as_u64(),
        reasoning_tokens: value["completion_tokens_details"]["reasoning_tokens"].as_u64(),
    })
}

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

pub fn collect_chat_turn(body: &str) -> Result<ChatTurn, String> {
    if looks_like_sse(body) {
        let mut turn = ChatTurn::default();
        let mut calls: BTreeMap<u64, ChatToolCall> = BTreeMap::new();
        for data in sse_data_payloads(body) {
            if data == "[DONE]" {
                break;
            }
            apply_sse_data(&mut turn, &mut calls, &data)?;
        }
        turn.tool_calls = calls.into_values().collect();
        return Ok(turn);
    }
    let v: Value = serde_json::from_str(body.trim()).map_err(|e| format!("chat json: {e}"))?;
    unary_turn(&v)
}

pub fn consume_sse_read(reader: &mut impl Read) -> Result<ChatTurn, String> {
    let mut pending = Vec::new();
    let mut buf = [0u8; 4096];
    let mut turn = ChatTurn::default();
    let mut calls: BTreeMap<u64, ChatToolCall> = BTreeMap::new();
    let mut bytes_read = 0usize;
    let mut complete_events = 0usize;
    loop {
        let n = reader.read(&mut buf).map_err(|error| {
            format!("sse read: {error}; bytes_read={bytes_read}; complete_events={complete_events}")
        })?;
        if n == 0 {
            break;
        }
        bytes_read += n;
        pending.extend_from_slice(&buf[..n]);
        while let Some(idx) = find_event_break(&pending) {
            complete_events += 1;
            let raw = pending.drain(..=idx).collect::<Vec<_>>();
            let text = String::from_utf8_lossy(&raw);
            for data in sse_data_payloads(&text) {
                if data == "[DONE]" {
                    turn.tool_calls = calls.into_values().collect();
                    return Ok(turn);
                }
                apply_sse_data(&mut turn, &mut calls, &data)?;
            }
        }
    }
    turn.tool_calls = calls.into_values().collect();
    Ok(turn)
}

fn find_event_break(buf: &[u8]) -> Option<usize> {
    buf.windows(2).position(|w| w == b"\n\n").map(|idx| idx + 1)
}

fn apply_sse_data(
    turn: &mut ChatTurn,
    calls: &mut BTreeMap<u64, ChatToolCall>,
    data: &str,
) -> Result<(), String> {
    let v: Value = serde_json::from_str(data)
        .map_err(|e| format!("sse chat json: {e}: {}", truncate(data, 180)))?;
    if let Some(usage) = usage(&v["usage"]) {
        // Streaming counts are cumulative snapshots, not increments per event.
        turn.usage = Some(usage);
    }
    if let Some(reason) = v["choices"][0]["finish_reason"].as_str() {
        turn.finish_reason = reason.to_owned();
    }
    if let Some(text) = v["choices"][0]["delta"]["content"].as_str() {
        turn.content.push_str(text);
    }
    if let Some(items) = v["choices"][0]["delta"]["tool_calls"].as_array() {
        for item in items {
            let index = item.get("index").and_then(Value::as_u64).unwrap_or(0);
            let slot = calls.entry(index).or_default();
            if let Some(id) = item.get("id").and_then(Value::as_str) {
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
    if turn.content.is_empty()
        && let Some(text) = v["choices"][0]["message"]["content"].as_str()
    {
        turn.content.push_str(text);
    }
    if let Some(items) = v["choices"][0]["message"]["tool_calls"].as_array() {
        for (index, item) in items.iter().enumerate() {
            calls.insert(
                index as u64,
                ChatToolCall {
                    id: item
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned(),
                    name: item["function"]["name"].as_str().unwrap_or("").to_owned(),
                    arguments: item["function"]["arguments"]
                        .as_str()
                        .unwrap_or("")
                        .to_owned(),
                },
            );
        }
    }
    Ok(())
}

fn unary_turn(v: &Value) -> Result<ChatTurn, String> {
    let mut turn = ChatTurn {
        usage: usage(&v["usage"]),
        content: v["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_owned(),
        finish_reason: v["choices"][0]["finish_reason"]
            .as_str()
            .unwrap_or("stop")
            .to_owned(),
        tool_calls: Vec::new(),
    };
    if let Some(items) = v["choices"][0]["message"]["tool_calls"].as_array() {
        for item in items {
            turn.tool_calls.push(ChatToolCall {
                id: item
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
                name: item["function"]["name"].as_str().unwrap_or("").to_owned(),
                arguments: item["function"]["arguments"]
                    .as_str()
                    .unwrap_or("")
                    .to_owned(),
            });
        }
    }
    Ok(turn)
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
    fn streaming_usage_survives_empty_choices_without_double_counting() {
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\",\"function\":{\"name\":\"read_source\",\"arguments\":\"{}\"}}]},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":5}}\n\n",
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":10,\"total_tokens\":110,\"prompt_tokens_details\":{\"cached_tokens\":60},\"completion_tokens_details\":{\"reasoning_tokens\":4}}}\n\n",
            "data: [DONE]\n\n",
        );
        let turn = collect_chat_turn(body).unwrap();
        assert_eq!(turn.finish_reason, "tool_calls");
        assert_eq!(turn.tool_calls.len(), 1);
        assert_eq!(
            turn.usage,
            Some(ChatUsage {
                prompt_tokens: Some(100),
                completion_tokens: Some(10),
                total_tokens: Some(110),
                cached_tokens: Some(60),
                reasoning_tokens: Some(4),
            })
        );
        assert_eq!(consume_sse_read(&mut body.as_bytes()).unwrap(), turn);
    }

    #[test]
    fn missing_usage_remains_unknown_and_unary_counts_are_preserved() {
        let body = r#"{"choices":[{"message":{"content":"ok"},"finish_reason":"stop"}]}"#;
        assert_eq!(collect_chat_turn(body).unwrap().usage, None);
        let mut value: Value = serde_json::from_str(body).unwrap();
        value["usage"] = serde_json::json!({"prompt_tokens":0,"completion_tokens":2});
        let counts = collect_chat_turn(&value.to_string())
            .unwrap()
            .usage
            .unwrap();
        assert_eq!(counts.prompt_tokens, Some(0));
        assert_eq!(counts.completion_tokens, Some(2));
        assert_eq!(counts.cached_tokens, None);
        assert_eq!(counts.reasoning_tokens, None);
        assert_eq!(counts.total_tokens, None);
    }

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
    fn chat_sse_concatenates_tool_call_argument_deltas() {
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\",\"function\":{\"name\":\"submit_outline\",\"arguments\":\"{\"}}]}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"}}\"}}]}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n",
        );
        let turn = collect_chat_turn(body).unwrap();
        assert_eq!(turn.tool_calls.len(), 1);
        assert_eq!(turn.tool_calls[0].name, "submit_outline");
        assert_eq!(turn.tool_calls[0].arguments, "{}}");
        assert_eq!(turn.finish_reason, "tool_calls");
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
