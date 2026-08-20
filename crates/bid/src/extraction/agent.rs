use async_openai::Client;
use async_openai::config::OpenAIConfig;
#[cfg(test)]
use async_openai::types::chat::Role;
use async_openai::types::chat::{
    ChatCompletionMessageToolCall, ChatCompletionMessageToolCalls, ChatCompletionRequestMessage,
    ChatCompletionRequestSystemMessage, ChatCompletionRequestSystemMessageContent,
    ChatCompletionRequestToolMessage, ChatCompletionRequestToolMessageContent,
    ChatCompletionRequestUserMessage, ChatCompletionRequestUserMessageContent,
    ChatCompletionResponseMessage, ChatCompletionTool, ChatCompletionTools,
    CreateChatCompletionRequestArgs, FunctionCall, FunctionObject,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use super::policy::{ExtractionLimits, ExtractionPolicy, render_prompt};
use super::reconcile::{quote_in_body, quotes_overlap};
use super::types::{CandidateClause, ClauseFamily, ExtractedSection, ExtractedSpan};

#[derive(Debug, Clone, Default)]
pub struct AgentStats {
    pub rounds: usize,
    pub retries: usize,
    pub tool_calls: usize,
    pub rejected_invalid_quotes: usize,
    pub stopped_without_tool: bool,
    pub termination: String,
}

#[derive(Debug)]
pub struct AgentOutcome {
    pub candidates: Vec<CandidateClause>,
    pub stats: AgentStats,
}

#[derive(Debug)]
pub struct AgentFailure {
    pub category: String,
    pub stats: AgentStats,
}

#[derive(Debug, Clone, Copy)]
pub struct ToolChatOptions {
    pub temperature: f32,
    pub max_tokens: u32,
}

#[async_trait]
pub trait ToolChat: Send + Sync {
    async fn complete(
        &self,
        model: &str,
        messages: &[ChatCompletionRequestMessage],
        tools: &[ChatCompletionTools],
        options: ToolChatOptions,
    ) -> Result<ChatCompletionResponseMessage, String>;
}

#[derive(Default)]
pub struct OpenAiToolChat;

impl OpenAiToolChat {
    pub fn configured(model: &str) -> bool {
        !domain::chat_base_url().is_empty() && !model.trim().is_empty() && model != "stub-chat"
    }
}

#[async_trait]
impl ToolChat for OpenAiToolChat {
    async fn complete(
        &self,
        model: &str,
        messages: &[ChatCompletionRequestMessage],
        tools: &[ChatCompletionTools],
        options: ToolChatOptions,
    ) -> Result<ChatCompletionResponseMessage, String> {
        if !Self::configured(model) {
            return Err("bid extract tool model is not configured".into());
        }
        let cfg = OpenAIConfig::new()
            .with_api_key(domain::chat_api_key())
            .with_api_base(openai_api_base(&domain::chat_base_url()));
        let client = Client::with_config(cfg);
        let request = CreateChatCompletionRequestArgs::default()
            .model(model)
            .messages(messages.to_vec())
            .tools(tools.to_vec())
            .temperature(options.temperature)
            .max_tokens(options.max_tokens)
            .build()
            .map_err(|e| e.to_string())?;
        let response = client
            .chat()
            .create(request)
            .await
            .map_err(|e| e.to_string())?;
        response
            .choices
            .into_iter()
            .next()
            .map(|choice| choice.message)
            .ok_or_else(|| "empty chat choices".into())
    }
}

pub fn extract_model_id() -> String {
    let explicit = domain::first_env(&["BID_EXTRACT_MODEL_ID"]);
    if !explicit.is_empty() {
        return explicit;
    }
    let fallback = domain::chat_model();
    if fallback.is_empty() {
        "stub-chat".into()
    } else {
        fallback
    }
}

pub fn openai_api_base(raw: &str) -> String {
    let base = raw.trim().trim_end_matches('/');
    if base.ends_with("/chat/completions") {
        base.trim_end_matches("/chat/completions").to_string()
    } else if base.ends_with("/v1") {
        base.to_string()
    } else {
        format!("{base}/v1")
    }
}

pub async fn run_family_agent(
    policy: &ExtractionPolicy,
    family: ClauseFamily,
    sections: &[ExtractedSection],
    model: &str,
    chat: &dyn ToolChat,
) -> Result<AgentOutcome, AgentFailure> {
    let mut messages = vec![
        system_message(&render_prompt(policy, family)),
        user_message(&first_user(policy, family, sections)),
    ];
    let tools = tool_defs(policy, true);
    let mut candidates = Vec::new();
    let mut stats = AgentStats::default();
    'rounds: for round in 0..policy.limits.max_rounds {
        if candidates.len() >= policy.limits.max_file_clauses {
            stats.termination = "clause_cap".into();
            break;
        }
        stats.rounds = round + 1;
        let reply = complete_with_retry(chat, model, &messages, &tools, &policy.limits, &mut stats)
            .await
            .map_err(|category| AgentFailure {
                category,
                stats: stats.clone(),
            })?;
        let calls = function_calls(&reply);
        if calls.is_empty() {
            stats.stopped_without_tool = true;
            stats.termination = "no_tool_call".into();
            break;
        }
        messages.push(assistant_from_response(&reply));
        if stats.tool_calls.saturating_add(calls.len()) > policy.limits.max_tool_calls {
            stats.termination = "tool_call_cap".into();
            break 'rounds;
        }
        let mut stop = false;
        for (id, name, arguments) in calls {
            stats.tool_calls += 1;
            let args = serde_json::from_str::<Value>(&arguments).unwrap_or(Value::Null);
            let (body, done) = dispatch(DispatchCtx {
                policy,
                family,
                tool: &name,
                args: &args,
                sections,
                candidates: &mut candidates,
                stats: &mut stats,
                extractor: "agent",
            });
            messages.push(tool_message(
                &id,
                &bounded_tool_output(&body, policy.limits.max_tool_output_chars),
            ));
            stop |= done;
        }
        if stop {
            stats.termination = "done".into();
            break;
        }
    }
    if stats.termination.is_empty() {
        stats.termination = "round_cap".into();
    }
    Ok(AgentOutcome { candidates, stats })
}

pub async fn run_span_sweep(
    policy: &ExtractionPolicy,
    family: ClauseFamily,
    span: &ExtractedSpan,
    model: &str,
    chat: &dyn ToolChat,
) -> Result<AgentOutcome, AgentFailure> {
    let sections = [ExtractedSection {
        key: span.section_key.clone(),
        heading_path: span.heading_path.clone(),
        hint_family: "unknown".into(),
        body: span.body.clone(),
        spans: vec![span.clone()],
        extract_status: "pending".into(),
        error_message: String::new(),
    }];
    let context = if span.context.is_empty() {
        String::new()
    } else {
        format!("\nnon_quotable_context:\n{}\n", span.context)
    };
    let messages = vec![
        system_message(&render_prompt(policy, family)),
        user_message(&format!(
            "这是覆盖补扫。请直接检查下面 span；有当前 family 条款就调用 emit_clauses，没有就调用 done。只能逐字引用 quotable_text，禁止把 context 拼进 quote。\nspan_id: {}\nheading_path: {}\n{}\nquotable_text:\n{}",
            span.id, span.heading_path, context, span.body
        )),
    ];
    let tools = tool_defs(policy, false);
    let mut stats = AgentStats {
        rounds: 1,
        ..Default::default()
    };
    let reply = complete_with_retry(chat, model, &messages, &tools, &policy.limits, &mut stats)
        .await
        .map_err(|category| AgentFailure {
            category,
            stats: stats.clone(),
        })?;
    let calls = function_calls(&reply);
    if calls.is_empty() {
        stats.stopped_without_tool = true;
        stats.termination = "no_tool_call".into();
        return Ok(AgentOutcome {
            candidates: Vec::new(),
            stats,
        });
    }
    if calls.len() > policy.limits.max_tool_calls {
        stats.termination = "tool_call_cap".into();
        return Ok(AgentOutcome {
            candidates: Vec::new(),
            stats,
        });
    }
    let mut candidates = Vec::new();
    let mut done = false;
    for (_, name, arguments) in calls {
        stats.tool_calls += 1;
        let args = serde_json::from_str::<Value>(&arguments).unwrap_or(Value::Null);
        let (_, tool_done) = dispatch(DispatchCtx {
            policy,
            family,
            tool: &name,
            args: &args,
            sections: &sections,
            candidates: &mut candidates,
            stats: &mut stats,
            extractor: "span_sweep",
        });
        done |= tool_done;
    }
    stats.termination = if done { "done" } else { "single_pass" }.into();
    Ok(AgentOutcome { candidates, stats })
}

async fn complete_with_retry(
    chat: &dyn ToolChat,
    model: &str,
    messages: &[ChatCompletionRequestMessage],
    tools: &[ChatCompletionTools],
    limits: &ExtractionLimits,
    stats: &mut AgentStats,
) -> Result<ChatCompletionResponseMessage, String> {
    let mut last = "chat failed".to_string();
    for attempt in 0..limits.max_retries {
        if attempt > 0 {
            stats.retries += 1;
        }
        match tokio::time::timeout(
            std::time::Duration::from_secs(limits.request_timeout_secs.max(1)),
            chat.complete(
                model,
                messages,
                tools,
                ToolChatOptions {
                    temperature: limits.temperature,
                    max_tokens: limits.max_tokens,
                },
            ),
        )
        .await
        {
            Ok(Ok(reply)) => return Ok(reply),
            Ok(Err(error)) => last = error,
            Err(_) => last = "provider request timeout".into(),
        }
    }
    Err(last)
}

fn bounded_tool_output(output: &str, max_chars: usize) -> String {
    if output.chars().count() <= max_chars {
        output.to_string()
    } else {
        json!({
            "error": "tool_output_too_large",
            "max_chars": max_chars,
            "actual_chars": output.chars().count()
        })
        .to_string()
    }
}

fn bounded_line(output: &str, max_chars: usize) -> String {
    output.chars().take(max_chars).collect()
}

struct DispatchCtx<'a> {
    policy: &'a ExtractionPolicy,
    family: ClauseFamily,
    tool: &'a str,
    args: &'a Value,
    sections: &'a [ExtractedSection],
    candidates: &'a mut Vec<CandidateClause>,
    stats: &'a mut AgentStats,
    extractor: &'a str,
}

fn dispatch(ctx: DispatchCtx<'_>) -> (String, bool) {
    let spans: Vec<_> = ctx
        .sections
        .iter()
        .flat_map(|section| &section.spans)
        .collect();
    match ctx.tool {
        "done" => match serde_json::from_value::<EmptyArgs>(ctx.args.clone()) {
            Ok(_) => ("ok".into(), true),
            Err(error) => (format!("invalid done args: {error}"), false),
        },
        "list_outline" => {
            if let Err(error) = serde_json::from_value::<EmptyArgs>(ctx.args.clone()) {
                return (format!("invalid list_outline args: {error}"), false);
            }
            let mut remaining = ctx.policy.limits.max_outline_spans;
            let rows: Vec<Value> = ctx
                .sections
                .iter()
                .filter_map(|section| {
                    if remaining == 0 {
                        return None;
                    }
                    let spans: Vec<_> = section
                        .spans
                        .iter()
                        .take(remaining)
                        .map(|span| {
                            json!({
                                "span_id": span.id,
                                "chars": span.body.chars().count(),
                                "candidate": span.candidate
                            })
                        })
                        .collect();
                    remaining = remaining.saturating_sub(spans.len());
                    Some(json!({
                        "heading_path": section.heading_path,
                        "hint_family": section.hint_family,
                        "spans": spans
                    }))
                })
                .collect();
            (
                serde_json::to_string(&rows).unwrap_or_else(|_| "[]".into()),
                false,
            )
        }
        "read_span" => {
            let args = match serde_json::from_value::<SpanArgs>(ctx.args.clone()) {
                Ok(args) => args,
                Err(error) => return (format!("invalid read_span args: {error}"), false),
            };
            let span_id = args.span_id;
            match spans.iter().find(|span| span.id == span_id) {
                Some(span) => (
                    json!({
                        "span_id": span.id,
                        "heading_path": span.heading_path,
                        "non_quotable_context": span.context,
                        "quotable_text": span.body
                    })
                    .to_string(),
                    false,
                ),
                None => (format!("unknown span_id: {span_id}"), false),
            }
        }
        "grep" => {
            let args = match serde_json::from_value::<GrepArgs>(ctx.args.clone()) {
                Ok(args) => args,
                Err(error) => return (format!("invalid grep args: {error}"), false),
            };
            if args.pattern.chars().count() > ctx.policy.limits.max_grep_pattern_chars {
                return ("grep pattern too long".into(), false);
            }
            let regex = match regex::RegexBuilder::new(&args.pattern)
                .case_insensitive(true)
                .build()
            {
                Ok(regex) => regex,
                Err(error) => return (format!("bad regex: {error}"), false),
            };
            let mut hits = Vec::new();
            for span in spans {
                for line in span.body.lines() {
                    if regex.is_match(line) {
                        hits.push(json!({
                            "span_id": span.id,
                            "heading_path": span.heading_path,
                            "line": bounded_line(line, 1000)
                        }));
                        if hits.len() >= ctx.policy.limits.grep_hits {
                            break;
                        }
                    }
                }
                if hits.len() >= ctx.policy.limits.grep_hits {
                    break;
                }
            }
            (
                serde_json::to_string(&hits).unwrap_or_else(|_| "[]".into()),
                false,
            )
        }
        "emit_clauses" => {
            let batch = match serde_json::from_value::<EmitBatch>(ctx.args.clone()) {
                Ok(batch) => batch,
                Err(error) => return (format!("invalid emit_clauses args: {error}"), false),
            };
            if batch.clauses.len() > ctx.policy.limits.max_emit {
                return (
                    format!(
                        "emit batch too large: {} > {}",
                        batch.clauses.len(),
                        ctx.policy.limits.max_emit
                    ),
                    false,
                );
            }
            let mut kept = 0;
            for item in batch.clauses {
                if ctx.candidates.len() >= ctx.policy.limits.max_file_clauses {
                    break;
                }
                let Some(span) = spans.iter().find(|span| span.id == item.span_id) else {
                    ctx.stats.rejected_invalid_quotes += 1;
                    continue;
                };
                if item.text.trim().is_empty() || !quote_in_body(&item.quote, &span.body) {
                    ctx.stats.rejected_invalid_quotes += 1;
                    continue;
                }
                if ctx.candidates.iter().any(|candidate| {
                    candidate.span_id == item.span_id
                        && quotes_overlap(&candidate.quote, &item.quote)
                }) {
                    continue;
                }
                ctx.candidates.push(CandidateClause {
                    span_id: item.span_id,
                    quote: item.quote.clone(),
                    text: item.quote,
                    must: item.must,
                    proposed_family: ctx.family,
                    extractor: format!(
                        "{family_name}_{extractor}",
                        family_name = ctx.family.as_str(),
                        extractor = ctx.extractor
                    ),
                });
                kept += 1;
            }
            (format!("emitted {kept}"), false)
        }
        other => (format!("unknown tool: {other}"), false),
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyArgs {}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SpanArgs {
    span_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GrepArgs {
    pattern: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EmitBatch {
    clauses: Vec<EmitItem>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EmitItem {
    span_id: String,
    quote: String,
    text: String,
    must: bool,
}

fn tool_defs(policy: &ExtractionPolicy, browsing: bool) -> Vec<ChatCompletionTools> {
    let mut tools = Vec::new();
    if browsing {
        tools.push(function_tool(
            "list_outline",
            "列出标题、family hint 和可读取的 span_id。",
            json!({"type":"object","properties":{},"additionalProperties":false}),
        ));
        tools.push(function_tool(
            "read_span",
            "按 span_id 读取完整原文。",
            json!({
                "type":"object",
                "properties":{"span_id":{"type":"string"}},
                "required":["span_id"],
                "additionalProperties":false
            }),
        ));
        tools.push(function_tool(
            "grep",
            "仅在当前招标文件内做大小写不敏感正则搜索。",
            json!({
                "type":"object",
                "properties":{"pattern":{"type":"string"}},
                "required":["pattern"],
                "additionalProperties":false
            }),
        ));
    }
    tools.push(function_tool(
        "emit_clauses",
        "提交当前 family 的草稿条款；quote 必须来自指定 span。",
        json!({
            "type":"object",
            "properties":{
                "clauses":{
                    "type":"array",
                    "maxItems":policy.limits.max_emit,
                    "items":{
                        "type":"object",
                        "properties":{
                            "span_id":{"type":"string"},
                            "quote":{"type":"string"},
                            "text":{"type":"string"},
                            "must":{"type":"boolean"}
                        },
                        "required":["span_id","quote","text","must"],
                        "additionalProperties":false
                    }
                }
            },
            "required":["clauses"],
            "additionalProperties":false
        }),
    ));
    tools.push(function_tool(
        "done",
        "当前 family 没有更多条款时结束。",
        json!({"type":"object","properties":{},"additionalProperties":false}),
    ));
    tools
}

fn function_tool(name: &str, description: &str, parameters: Value) -> ChatCompletionTools {
    ChatCompletionTools::Function(ChatCompletionTool {
        function: FunctionObject {
            name: name.into(),
            description: Some(description.into()),
            parameters: Some(parameters),
            strict: Some(true),
        },
    })
}

fn first_user(
    policy: &ExtractionPolicy,
    family: ClauseFamily,
    sections: &[ExtractedSection],
) -> String {
    let outline = sections
        .iter()
        .flat_map(|section| {
            section.spans.iter().map(|span| {
                format!(
                    "- {} [{}] {}(chars={},candidate={})",
                    section.heading_path,
                    section.hint_family,
                    span.id,
                    span.body.chars().count(),
                    span.candidate
                )
            })
        })
        .take(policy.limits.max_outline_spans)
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "抽取 {} 条款。请先浏览大纲，再读取和搜索原文；不要仅凭标题生成条款。\nOutline:\n{}",
        family.as_str(),
        outline
    )
}

fn function_calls(message: &ChatCompletionResponseMessage) -> Vec<(String, String, String)> {
    let Some(calls) = &message.tool_calls else {
        return Vec::new();
    };
    calls
        .iter()
        .filter_map(|call| match call {
            ChatCompletionMessageToolCalls::Function(ChatCompletionMessageToolCall {
                id,
                function: FunctionCall { name, arguments },
            }) => Some((id.clone(), name.clone(), arguments.clone())),
            #[allow(unreachable_patterns)]
            _ => None,
        })
        .collect()
}

fn system_message(content: &str) -> ChatCompletionRequestMessage {
    ChatCompletionRequestSystemMessage {
        content: ChatCompletionRequestSystemMessageContent::Text(content.into()),
        name: None,
    }
    .into()
}

fn user_message(content: &str) -> ChatCompletionRequestMessage {
    ChatCompletionRequestUserMessage {
        content: ChatCompletionRequestUserMessageContent::Text(content.into()),
        name: None,
    }
    .into()
}

fn tool_message(id: &str, content: &str) -> ChatCompletionRequestMessage {
    ChatCompletionRequestToolMessage {
        content: ChatCompletionRequestToolMessageContent::Text(content.into()),
        tool_call_id: id.into(),
    }
    .into()
}

fn assistant_from_response(
    message: &ChatCompletionResponseMessage,
) -> ChatCompletionRequestMessage {
    use async_openai::types::chat::{
        ChatCompletionRequestAssistantMessage, ChatCompletionRequestAssistantMessageContent,
    };
    #[allow(deprecated)]
    ChatCompletionRequestAssistantMessage {
        content: message
            .content
            .clone()
            .map(ChatCompletionRequestAssistantMessageContent::Text),
        refusal: message.refusal.clone(),
        name: None,
        audio: None,
        tool_calls: message.tool_calls.clone(),
        function_call: None,
    }
    .into()
}

#[cfg(test)]
pub struct ScriptedToolChat {
    replies: tokio::sync::Mutex<std::collections::VecDeque<ChatCompletionResponseMessage>>,
}

#[cfg(test)]
impl ScriptedToolChat {
    pub fn new(replies: Vec<ChatCompletionResponseMessage>) -> Self {
        Self {
            replies: tokio::sync::Mutex::new(replies.into()),
        }
    }
}

#[cfg(test)]
#[async_trait]
impl ToolChat for ScriptedToolChat {
    async fn complete(
        &self,
        _model: &str,
        _messages: &[ChatCompletionRequestMessage],
        _tools: &[ChatCompletionTools],
        _options: ToolChatOptions,
    ) -> Result<ChatCompletionResponseMessage, String> {
        self.replies
            .lock()
            .await
            .pop_front()
            .ok_or_else(|| "script exhausted".into())
    }
}

#[cfg(test)]
pub fn scripted_text_message(content: &str) -> ChatCompletionResponseMessage {
    #[allow(deprecated)]
    ChatCompletionResponseMessage {
        content: Some(content.into()),
        refusal: None,
        tool_calls: None,
        annotations: None,
        role: Role::Assistant,
        function_call: None,
        audio: None,
    }
}

#[cfg(test)]
pub fn scripted_tool_message(
    id: &str,
    name: &str,
    arguments: Value,
) -> ChatCompletionResponseMessage {
    #[allow(deprecated)]
    ChatCompletionResponseMessage {
        content: None,
        refusal: None,
        tool_calls: Some(vec![ChatCompletionMessageToolCalls::Function(
            ChatCompletionMessageToolCall {
                id: id.into(),
                function: FunctionCall {
                    name: name.into(),
                    arguments: arguments.to_string(),
                },
            },
        )]),
        annotations: None,
        role: Role::Assistant,
        function_call: None,
        audio: None,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::extraction::outline::build_sections;
    use crate::extraction::policy::default_policy;
    use crate::extraction::types::ExtractionScope;

    struct FlakyChat {
        attempts: AtomicUsize,
        reply: ChatCompletionResponseMessage,
    }

    #[async_trait]
    impl ToolChat for FlakyChat {
        async fn complete(
            &self,
            _model: &str,
            _messages: &[ChatCompletionRequestMessage],
            _tools: &[ChatCompletionTools],
            _options: ToolChatOptions,
        ) -> Result<ChatCompletionResponseMessage, String> {
            if self.attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                Err("429 rate limited".into())
            } else {
                Ok(self.reply.clone())
            }
        }
    }

    #[tokio::test]
    async fn transient_model_error_is_retried_and_counted() {
        let policy = default_policy().unwrap();
        let sections = build_sections(
            "# 技术要求\n吞吐量不得低于40G。",
            &ExtractionScope::Document,
            policy,
        );
        let chat = FlakyChat {
            attempts: AtomicUsize::new(0),
            reply: scripted_tool_message("done", "done", json!({})),
        };
        let outcome = run_family_agent(
            policy,
            ClauseFamily::Technical,
            &sections,
            "test-model",
            &chat,
        )
        .await
        .unwrap();
        assert_eq!(outcome.stats.retries, 1);
        assert_eq!(chat.attempts.load(Ordering::SeqCst), 2);
    }

    struct FailingChat {
        attempts: AtomicUsize,
    }

    #[async_trait]
    impl ToolChat for FailingChat {
        async fn complete(
            &self,
            _model: &str,
            _messages: &[ChatCompletionRequestMessage],
            _tools: &[ChatCompletionTools],
            _options: ToolChatOptions,
        ) -> Result<ChatCompletionResponseMessage, String> {
            self.attempts.fetch_add(1, Ordering::SeqCst);
            Err("provider secret detail".into())
        }
    }

    #[tokio::test]
    async fn terminal_provider_failure_carries_attempt_stats_without_message() {
        let policy = default_policy().unwrap();
        let sections = build_sections(
            "# 技术要求\n吞吐量不得低于40G。",
            &ExtractionScope::Document,
            policy,
        );
        let chat = FailingChat {
            attempts: AtomicUsize::new(0),
        };
        let failure = run_family_agent(
            policy,
            ClauseFamily::Technical,
            &sections,
            "test-model",
            &chat,
        )
        .await
        .unwrap_err();
        assert_eq!(failure.stats.rounds, 1);
        assert_eq!(failure.stats.retries, policy.limits.max_retries - 1);
        assert!(!failure.category.is_empty());
    }

    #[tokio::test]
    async fn strict_emit_uses_span_and_locked_family() {
        let policy = default_policy().unwrap();
        let sections = build_sections(
            "# 技术要求\n吞吐量不得低于40G。",
            &ExtractionScope::Document,
            policy,
        );
        let span = &sections[0].spans[0];
        let chat = ScriptedToolChat::new(vec![
            scripted_tool_message(
                "1",
                "emit_clauses",
                json!({"clauses":[{
                    "span_id":span.id,
                    "quote":"吞吐量不得低于40G",
                    "text":"吞吐量不低于40G",
                    "must":true
                }]}),
            ),
            scripted_tool_message("2", "done", json!({})),
        ]);
        let output = run_family_agent(
            policy,
            ClauseFamily::Technical,
            &sections,
            "test-model",
            &chat,
        )
        .await
        .unwrap();
        assert_eq!(output.candidates.len(), 1);
        assert_eq!(
            output.candidates[0].proposed_family,
            ClauseFamily::Technical
        );
        assert_eq!(output.candidates[0].text, output.candidates[0].quote);
    }

    #[tokio::test]
    async fn malformed_emit_arguments_are_not_accepted() {
        let policy = default_policy().unwrap();
        let sections = build_sections(
            "# 技术要求\n吞吐量不得低于40G。",
            &ExtractionScope::Document,
            policy,
        );
        let span = &sections[0].spans[0];
        let chat = ScriptedToolChat::new(vec![
            scripted_tool_message(
                "1",
                "emit_clauses",
                json!({"clauses":[{
                    "span_id":span.id,
                    "quote":"吞吐量不得低于40G",
                    "text":"吞吐量不低于40G"
                }]}),
            ),
            scripted_tool_message("2", "done", json!({})),
        ]);
        let outcome = run_family_agent(
            policy,
            ClauseFamily::Technical,
            &sections,
            "test-model",
            &chat,
        )
        .await
        .unwrap();
        assert!(outcome.candidates.is_empty());
    }

    #[tokio::test]
    async fn long_section_can_read_and_emit_from_later_span() {
        let policy = default_policy().unwrap();
        let markdown = format!(
            "# 技术要求\n{}\n目标设备必须支持万兆接口。",
            "普通说明".repeat(2500)
        );
        let sections = build_sections(&markdown, &ExtractionScope::Document, policy);
        let later = sections[0].spans.last().unwrap();
        assert!(sections[0].spans.len() > 1);
        let chat = ScriptedToolChat::new(vec![
            scripted_tool_message("1", "read_span", json!({"span_id": later.id})),
            scripted_tool_message(
                "2",
                "emit_clauses",
                json!({"clauses":[{
                    "span_id":later.id,
                    "quote":"目标设备必须支持万兆接口",
                    "text":"支持万兆接口",
                    "must":true
                }]}),
            ),
            scripted_tool_message("3", "done", json!({})),
        ]);
        let outcome = run_family_agent(
            policy,
            ClauseFamily::Technical,
            &sections,
            "test-model",
            &chat,
        )
        .await
        .unwrap();
        assert_eq!(outcome.candidates[0].span_id, later.id);
    }

    #[test]
    fn model_id_falls_back_to_main_chat_model_contract() {
        // Pure env mutation is process-global; verify the no-value terminal behavior here.
        if domain::chat_model().is_empty()
            && std::env::var("BID_EXTRACT_MODEL_ID")
                .unwrap_or_default()
                .is_empty()
        {
            assert_eq!(extract_model_id(), "stub-chat");
        }
    }

    #[test]
    fn all_tools_reject_extra_or_malformed_arguments_server_side() {
        let policy = default_policy().unwrap();
        let sections = build_sections(
            "# 技术要求\n设备必须支持万兆接口。",
            &ExtractionScope::Document,
            policy,
        );
        for (tool, args) in [
            ("done", json!({"extra": true})),
            ("list_outline", Value::Null),
            (
                "read_span",
                json!({"span_id": sections[0].spans[0].id, "extra": 1}),
            ),
            ("grep", json!({"pattern": "支持", "extra": 1})),
        ] {
            let mut candidates = Vec::new();
            let mut stats = AgentStats::default();
            let (message, done) = dispatch(DispatchCtx {
                policy,
                family: ClauseFamily::Technical,
                tool,
                args: &args,
                sections: &sections,
                candidates: &mut candidates,
                stats: &mut stats,
                extractor: "test",
            });
            assert!(message.starts_with("invalid"), "{tool}: {message}");
            assert!(!done);
        }
    }

    #[test]
    fn tool_output_is_valid_structured_error_and_oversized_emit_is_rejected() {
        let policy = default_policy().unwrap();
        let mut sections = build_sections(
            "# 技术要求\n设备必须支持万兆接口。",
            &ExtractionScope::Document,
            policy,
        );
        sections[0].spans[0].heading_path = "\\\"长标题".repeat(4000);
        let mut candidates = Vec::new();
        let mut stats = AgentStats::default();
        let (body, _) = dispatch(DispatchCtx {
            policy,
            family: ClauseFamily::Technical,
            tool: "read_span",
            args: &json!({"span_id": sections[0].spans[0].id}),
            sections: &sections,
            candidates: &mut candidates,
            stats: &mut stats,
            extractor: "test",
        });
        let bounded = bounded_tool_output(&body, policy.limits.max_tool_output_chars);
        let parsed: Value = serde_json::from_str(&bounded).unwrap();
        assert_eq!(parsed["error"], "tool_output_too_large");

        let clauses: Vec<_> = (0..=policy.limits.max_emit)
            .map(|_| {
                json!({
                    "span_id": sections[0].spans[0].id,
                    "quote": "设备必须支持万兆接口",
                    "text": "设备必须支持万兆接口",
                    "must": true
                })
            })
            .collect();
        let (message, _) = dispatch(DispatchCtx {
            policy,
            family: ClauseFamily::Technical,
            tool: "emit_clauses",
            args: &json!({"clauses": clauses}),
            sections: &sections,
            candidates: &mut candidates,
            stats: &mut stats,
            extractor: "test",
        });
        assert!(message.starts_with("emit batch too large"));
        assert!(candidates.is_empty());
    }

    #[test]
    fn emit_schema_requires_every_server_accepted_field() {
        let policy = default_policy().unwrap();
        let tools = tool_defs(policy, true);
        let emit = tools
            .iter()
            .find_map(|tool| match tool {
                ChatCompletionTools::Function(tool) if tool.function.name == "emit_clauses" => {
                    tool.function.parameters.as_ref()
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(
            emit["properties"]["clauses"]["items"]["required"],
            json!(["span_id", "quote", "text", "must"])
        );
        assert_eq!(emit["additionalProperties"], false);
        assert_eq!(
            emit["properties"]["clauses"]["items"]["additionalProperties"],
            false
        );
    }
}
