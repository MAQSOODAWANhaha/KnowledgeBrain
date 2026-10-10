//! Bounded SDK conversation. Dynamic host metadata belongs to each request,
//! while the SDK retains the actual model/tool history of the active window.
use super::invalid;
use crate::agent_error::AgentError;
use knowledge::models::ChatTurn;
use rig::{
    agent::{AgentRun, AgentRunStep, InvalidToolCallAction, ModelTurn, ModelTurnOutcome},
    completion::{FinishReason, Usage},
    message::{AssistantContent, Message, ToolChoice, UserContent},
    providers::openai::completion as wire,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Session {
    run: AgentRun,
    /// Host-owned messages surrounding the evidence transcript. The first
    /// system message remains the SDK anchor, including on an empty window.
    prefix: usize,
    suffix: usize,
    /// This turn's provider usage was added before `tools`, so the SDK call
    /// must not add it again. Absent on older checkpoints.
    #[serde(default, skip_serializing_if = "is_false")]
    usage_noted: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn apply_usage(run: &mut AgentRun, usage: Usage) -> Result<(), AgentError> {
    let mut value = serde_json::to_value(&*run).map_err(|_| failure())?;
    value["usage"] = serde_json::to_value(usage).map_err(|_| failure())?;
    *run = serde_json::from_value(value).map_err(|_| failure())?;
    Ok(())
}

fn failure() -> AgentError {
    invalid("SDK AgentRun state does not match the committed conversation")
}

fn message(value: Value) -> Result<Message, AgentError> {
    let wire: wire::Message = serde_json::from_value(value).map_err(|_| failure())?;
    // Rig's generic reverse conversion deliberately downgrades a Chat system
    // message to User. These are host instructions, so preserve their role.
    if let wire::Message::System { content, .. } = wire {
        let [text] = content.as_slice() else {
            return Err(failure());
        };
        return Ok(Message::system(&text.text));
    }
    Message::try_from(wire).map_err(|_| failure())
}

fn projected(messages: Vec<Message>) -> Result<Vec<Value>, AgentError> {
    messages
        .into_iter()
        .map(|mut message| {
            if let Message::Assistant { content, .. } = &mut message {
                content.retain(
                    |item| !matches!(item, AssistantContent::Text(text) if text.text.is_empty()),
                );
            }
            Vec::<wire::Message>::try_from(message).map_err(|_| failure())
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .map(|message| {
            let mut value = serde_json::to_value(message).map_err(|_| failure())?;
            // JSONB/JCS can reorder SDK Value objects, while the reserved Chat
            // body retains the original JSON string inside tool arguments.
            // Normalize only this comparison projection, never the wire body.
            if let Some(calls) = value.get_mut("tool_calls").and_then(Value::as_array_mut) {
                for call in calls {
                    if let Some(arguments) = call["function"]["arguments"].as_str()
                        && let Ok(parsed) = serde_json::from_str::<Value>(arguments)
                    {
                        call["function"]["arguments"] = json!(
                            String::from_utf8(
                                serde_json_canonicalizer::to_vec(&parsed).map_err(|_| failure())?
                            )
                            .map_err(|_| failure())?
                        );
                    }
                }
            }
            Ok(value)
        })
        .collect()
}

impl Session {
    pub fn turn(&self) -> usize {
        self.run.turn()
    }

    fn context(body: &Value, prefix: usize, suffix: usize) -> Result<Vec<Message>, AgentError> {
        let messages = body["messages"].as_array().ok_or_else(failure)?;
        if prefix == 0 || messages.first().is_none_or(|m| m["role"] != "system") {
            return Err(failure());
        }
        let end = messages.len().checked_sub(suffix).ok_or_else(failure)?;
        std::iter::once(&messages[0])
            .chain(messages.get(prefix..end).ok_or_else(failure)?)
            .cloned()
            .map(message)
            .collect()
    }

    pub(super) fn prepare(
        active: &mut Option<Self>,
        body: &[u8],
        prefix: usize,
        suffix: usize,
        max_bytes: usize,
    ) -> Result<(), AgentError> {
        let body: Value = serde_json::from_slice(body).map_err(|_| failure())?;
        let mut context = Self::context(&body, prefix, suffix)?;
        let expected = projected(context.clone())?;
        let prompt = context.pop().ok_or_else(failure)?;
        let carried = active.as_ref().map(|session| session.run.usage());
        // The host owns the business loop. Each SDK run serves one physical
        // request, retaining the admitted history and cumulative reported usage.
        // Build locally so a failed admission cannot replace the saved session.
        let mut run = AgentRun::new(prompt)
            .with_history(context)
            .with_tool_choice(ToolChoice::Required);
        if let Some(usage) = carried.filter(|usage| usage.has_values()) {
            apply_usage(&mut run, usage)?;
        }
        let mut session = Self {
            run,
            prefix,
            suffix,
            usage_noted: false,
        };
        let AgentRunStep::CallModel {
            prompt,
            mut history,
            ..
        } = session.run.next_step().map_err(|_| failure())?
        else {
            return Err(failure());
        };
        history.push(prompt);
        if projected(history)? != expected {
            return Err(failure());
        }
        if session.bytes()? <= max_bytes {
            *active = Some(session);
            return Ok(());
        }
        Err(AgentError::new(
            "AGENT_TURN_BUDGET_EXCEEDED",
            "selected SDK conversation exceeds the configured context byte budget",
        ))
    }

    pub(super) fn validate(&self, body: &Value) -> Result<(), AgentError> {
        if projected(self.run.full_history())?
            != projected(Self::context(body, self.prefix, self.suffix)?)?
        {
            return Err(failure());
        }
        Ok(())
    }

    pub(super) fn tools(
        &mut self,
        body: &[u8],
        response: &ChatTurn,
    ) -> Result<BTreeMap<String, String>, AgentError> {
        let body: Value = serde_json::from_slice(body).map_err(|_| failure())?;
        self.validate(&body)?;
        let names = body["tools"]
            .as_array()
            .ok_or_else(failure)?
            .iter()
            .map(|t| {
                t["function"]["name"]
                    .as_str()
                    .map(str::to_owned)
                    .ok_or_else(failure)
            })
            .collect::<Result<std::collections::BTreeSet<_>, _>>()?;
        let mut choice = Vec::new();
        if !response.content.is_empty() {
            choice.push(AssistantContent::text(&response.content));
        }
        for call in &response.tool_calls {
            // Preserve malformed arguments for the domain's field feedback;
            // they are never repaired into executable business arguments.
            choice.push(AssistantContent::tool_call(
                &call.id,
                &call.name,
                serde_json::from_str(&call.arguments).unwrap_or_else(|_| json!(call.arguments)),
            ));
        }
        let mut usage = Usage::new();
        if self.usage_noted {
            self.usage_noted = false;
        } else if let Some(reported) = &response.usage {
            usage.input_tokens = reported.prompt_tokens.unwrap_or(0);
            usage.output_tokens = reported.completion_tokens.unwrap_or(0);
            usage.total_tokens = reported.total_tokens.unwrap_or(0);
            usage.cached_input_tokens = reported.cached_tokens.unwrap_or(0);
            usage.reasoning_tokens = reported.reasoning_tokens.unwrap_or(0);
        }
        // Unknown usage remains explicit in the durable ChatTurn; SDK zero
        // sentinels are not used as provider measurements or global budgets.
        let mut outcome = self
            .run
            .model_response(
                ModelTurn::new(None, choice, usage, names.clone(), names)
                    .with_finish_reason(Some(FinishReason::ToolCalls)),
            )
            .map_err(|_| failure())?;
        loop {
            match outcome {
                ModelTurnOutcome::Continue { .. } => break,
                ModelTurnOutcome::NeedsResolution(_) => {
                    outcome = self.run.resolve_invalid_tool_call(InvalidToolCallAction::Skip {
                        reason: json!({"ok":false,"error":"Tool is not available in the current role; use an advertised tool."}).to_string(),
                    }).map_err(|_| failure())?;
                }
                ModelTurnOutcome::TurnRetried => return Err(failure()),
            }
        }
        let AgentRunStep::CallTools { calls } = self.run.next_step().map_err(|_| failure())? else {
            return Err(failure());
        };
        if calls.len() != response.tool_calls.len() {
            return Err(failure());
        }
        let mut suppressed = BTreeMap::new();
        for (pending, original) in calls.into_iter().zip(&response.tool_calls) {
            if pending.tool_call.id.as_str() != original.id
                || pending.tool_call.function.name != original.name
            {
                return Err(failure());
            }
            if let Some(result) = pending.preresolved_result {
                let results = projected(vec![Message::User {
                    content: vec![result],
                }])?;
                let content = results
                    .first()
                    .and_then(|v| v["content"].as_str())
                    .ok_or_else(failure)?;
                suppressed.insert(original.id.clone(), content.to_owned());
            }
        }
        Ok(suppressed)
    }

    pub(super) fn finish(&mut self, results: Vec<Value>) -> Result<(), AgentError> {
        let AgentRunStep::CallTools { calls } = self.run.next_step().map_err(|_| failure())? else {
            return Err(failure());
        };
        for call in calls {
            if let Some(result) = call.preresolved_result {
                let expected = projected(vec![Message::User {
                    content: vec![result],
                }])?;
                let actual = results
                    .iter()
                    .find(|v| v["tool_call_id"] == call.tool_call.id.as_str())
                    .cloned()
                    .ok_or_else(failure)?;
                if projected(vec![message(actual)?])? != expected {
                    return Err(failure());
                }
            }
        }
        let mut content = Vec::<UserContent>::new();
        for result in results {
            let Message::User { content: items } = message(result)? else {
                return Err(failure());
            };
            content.extend(items);
        }
        self.run.tool_results(content).map_err(|_| failure())
    }

    /// Add this turn's reported usage onto the run before the responded save.
    /// Missing usage stays missing: an absent object is not zero consumption.
    pub(super) fn note_provider_usage(&mut self, response: &ChatTurn) {
        let Some(reported) = &response.usage else {
            return;
        };
        if reported.prompt_tokens.is_none()
            && reported.completion_tokens.is_none()
            && reported.total_tokens.is_none()
            && reported.cached_tokens.is_none()
            && reported.reasoning_tokens.is_none()
        {
            return;
        }
        let addition = Usage {
            input_tokens: reported.prompt_tokens.unwrap_or(0),
            output_tokens: reported.completion_tokens.unwrap_or(0),
            total_tokens: reported.total_tokens.unwrap_or(0),
            cached_input_tokens: reported.cached_tokens.unwrap_or(0),
            cache_creation_input_tokens: 0,
            tool_use_prompt_tokens: 0,
            reasoning_tokens: reported.reasoning_tokens.unwrap_or(0),
        };
        let current = self.run.usage();
        let summed = Usage {
            input_tokens: current.input_tokens.saturating_add(addition.input_tokens),
            output_tokens: current.output_tokens.saturating_add(addition.output_tokens),
            total_tokens: current.total_tokens.saturating_add(addition.total_tokens),
            cached_input_tokens: current
                .cached_input_tokens
                .saturating_add(addition.cached_input_tokens),
            cache_creation_input_tokens: current.cache_creation_input_tokens,
            tool_use_prompt_tokens: current.tool_use_prompt_tokens,
            reasoning_tokens: current
                .reasoning_tokens
                .saturating_add(addition.reasoning_tokens),
        };
        if apply_usage(&mut self.run, summed).is_ok() {
            self.usage_noted = true;
        }
    }

    /// Serialized SDK AgentRun size, not the Chat Completions wire body.
    /// `drive` drops the activity window when this exceeds `max_context_bytes`.
    /// Domain adapters separately budget the reserved request from `chat::prepare`.
    pub(super) fn bytes(&self) -> Result<usize, AgentError> {
        serde_json::to_vec(self)
            .map(|bytes| bytes.len())
            .map_err(|_| failure())
    }

    #[cfg(test)]
    pub(crate) fn projected_history(&self) -> Result<Vec<Value>, AgentError> {
        projected(self.run.full_history())
    }
}

#[cfg(test)]
pub(crate) fn test_system_evidence(text: &str) -> Vec<Value> {
    projected(vec![Message::system(text)]).unwrap()
}

#[cfg(test)]
pub(crate) fn test_window_body(evidence: Vec<Value>, progress: &str) -> Vec<u8> {
    let mut messages = evidence;
    messages.insert(1, json!({"role":"user","content":"project metadata"}));
    messages.push(json!({"role":"user","content":progress}));
    serde_json_canonicalizer::to_vec(&json!({"messages":messages,
        "tools":[{"type":"function","function":{"name":"read_source"}}]}))
    .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use knowledge::models::ChatToolCall;

    fn request(history: Vec<Message>, progress: &str) -> Vec<u8> {
        let mut messages = projected(history).unwrap();
        messages.insert(1, json!({"role":"user","content":"project metadata"}));
        messages.push(json!({"role":"user","content":progress}));
        serde_json_canonicalizer::to_vec(&json!({"messages":messages,
            "tools":[{"type":"function","function":{"name":"read_source"}}]}))
        .unwrap()
    }

    fn response(names: &[&str]) -> ChatTurn {
        ChatTurn {
            tool_calls: names
                .iter()
                .enumerate()
                .map(|(i, name)| ChatToolCall {
                    id: format!("provider-{i}"),
                    name: (*name).into(),
                    arguments: "{}".into(),
                })
                .collect(),
            finish_reason: "tool_calls".into(),
            ..Default::default()
        }
    }

    fn result(id: &str, content: &str) -> Value {
        json!({"role":"tool","tool_call_id":id,"content":content})
    }

    #[test]
    fn canonical_checkpoint_roundtrip_preserves_argument_meaning_and_rejects_actual_edits() {
        let body = request(vec![Message::system("reviewer")], "progress");
        let mut active = None;
        Session::prepare(
            &mut active,
            &body,
            crate::agent_runtime::SESSION_PREFIX,
            crate::agent_runtime::ANALYSIS_SESSION_SUFFIX,
            16384,
        )
        .unwrap();
        let session = active.as_mut().unwrap();
        let mut turn = response(&["read_source"]);
        turn.tool_calls[0].arguments = r#"{"z":0,"a":{"second":2,"first":1}}"#.into();
        session.tools(&body, &turn).unwrap();
        session
            .finish(vec![result("provider-0", "fixture source")])
            .unwrap();
        let body = request(session.run.full_history(), "next progress");
        let mut raw: Value = serde_json::from_slice(&body).unwrap();
        raw["messages"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|m| m["tool_calls"].is_array())
            .unwrap()["tool_calls"][0]["function"]["arguments"] =
            json!(turn.tool_calls[0].arguments);
        let body = serde_json_canonicalizer::to_vec(&raw).unwrap();
        Session::prepare(
            &mut active,
            &body,
            crate::agent_runtime::SESSION_PREFIX,
            crate::agent_runtime::ANALYSIS_SESSION_SUFFIX,
            16384,
        )
        .unwrap();
        let original = body.clone();
        let mut restored: Session = serde_json::from_slice(
            &serde_json_canonicalizer::to_vec(active.as_ref().unwrap()).unwrap(),
        )
        .unwrap();
        restored
            .validate(&serde_json::from_slice(&body).unwrap())
            .unwrap();
        let mut changed: Value = serde_json::from_slice(&body).unwrap();
        let call = changed["messages"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|m| m["tool_calls"].is_array())
            .unwrap();
        call["tool_calls"][0]["function"]["arguments"] =
            json!(r#"{"z":1,"a":{"first":1,"second":2}}"#);
        assert!(
            restored.validate(&changed).is_err(),
            "key order is irrelevant; changed arguments are not"
        );
        restored.tools(&body, &response(&["read_source"])).unwrap();
        assert_eq!(
            body, original,
            "comparison must not rewrite reserved request bytes"
        );
    }

    #[test]
    fn request_scoped_sdk_preserves_history_and_restores_awaiting_model() {
        let mut active = None;
        let body = request(vec![Message::system("main")], "first progress");
        Session::prepare(
            &mut active,
            &body,
            crate::agent_runtime::SESSION_PREFIX,
            crate::agent_runtime::ANALYSIS_SESSION_SUFFIX,
            16384,
        )
        .unwrap();
        // A response saved by the host can be replayed into this persisted
        // AwaitingModel state without asking SDK for another model step.
        let mut saved: Session =
            serde_json::from_slice(&serde_json::to_vec(&active.unwrap()).unwrap()).unwrap();
        assert_eq!(saved.run.turn(), 1);
        assert!(
            saved
                .tools(&body, &response(&["read_source"]))
                .unwrap()
                .is_empty()
        );
        saved
            .finish(vec![result("provider-0", "原文证据")])
            .unwrap();
        let body = request(saved.run.full_history(), "updated progress");
        active = Some(saved);
        Session::prepare(
            &mut active,
            &body,
            crate::agent_runtime::SESSION_PREFIX,
            crate::agent_runtime::ANALYSIS_SESSION_SUFFIX,
            16384,
        )
        .unwrap();
        let session = active.as_mut().unwrap();
        assert_eq!(
            session.run.turn(),
            1,
            "the host owns business turns; each SDK run serves one request"
        );
        assert!(session.run.completion_calls().is_empty());
        let history = serde_json::to_string(&session.run.full_history()).unwrap();
        assert!(history.contains("原文证据"));
        assert!(
            !history.contains("progress"),
            "per-request metadata must not accumulate"
        );
        assert!(
            session
                .tools(&body, &response(&["read_source"]))
                .unwrap()
                .is_empty()
        );
        session
            .finish(vec![result("provider-0", "second result")])
            .unwrap();
        let body = request(session.run.full_history(), "third progress");
        Session::prepare(
            &mut active,
            &body,
            crate::agent_runtime::SESSION_PREFIX,
            crate::agent_runtime::ANALYSIS_SESSION_SUFFIX,
            16384,
        )
        .unwrap();
        let session = active.unwrap();
        assert_eq!(session.run.turn(), 1);
        let history = serde_json::to_string(&session.run.full_history()).unwrap();
        assert!(history.contains("原文证据"));
        assert!(history.contains("second result"));
        assert!(!history.contains("progress"));
    }

    #[test]
    fn suppressed_role_tools_keep_provider_ids_and_exact_sdk_feedback() {
        let body = request(vec![Message::system("reviewer")], "progress");
        let mut active = None;
        Session::prepare(
            &mut active,
            &body,
            crate::agent_runtime::SESSION_PREFIX,
            crate::agent_runtime::ANALYSIS_SESSION_SUFFIX,
            16384,
        )
        .unwrap();
        let session = active.as_mut().unwrap();
        let suppressed = session
            .tools(&body, &response(&["read_source", "put_record", "unknown"]))
            .unwrap();
        // SDK Skip suppresses the entire batch, including valid peers. None
        // may establish evidence or execute a business write on this turn.
        assert_eq!(suppressed.len(), 3);
        let results = vec![
            result("provider-0", &suppressed["provider-0"]),
            result("provider-1", &suppressed["provider-1"]),
            result("provider-2", &suppressed["provider-2"]),
        ];
        let mut corrupted = session.clone();
        let mut wrong = results.clone();
        wrong[1]["content"] = json!("tool was executed");
        assert!(corrupted.finish(wrong).is_err());
        session.finish(results).unwrap();
        assert_eq!(session.run.completion_calls().len(), 1);
    }

    #[test]
    fn window_and_role_handoffs_preserve_selected_evidence_and_remaining_budget() {
        let initial = request(vec![Message::system("main")], "progress");
        let mut active = None;
        Session::prepare(
            &mut active,
            &initial,
            crate::agent_runtime::SESSION_PREFIX,
            crate::agent_runtime::ANALYSIS_SESSION_SUFFIX,
            16384,
        )
        .unwrap();
        let session = active.as_mut().unwrap();
        session
            .tools(&initial, &response(&["read_source"]))
            .unwrap();
        session
            .finish(vec![result("provider-0", "old source")])
            .unwrap();
        let selected = vec![
            Message::system("reviewer"),
            Message::user("selected source and form anchors"),
        ];
        let body = request(selected.clone(), "new scope");
        Session::prepare(
            &mut active,
            &body,
            crate::agent_runtime::SESSION_PREFIX,
            crate::agent_runtime::ANALYSIS_SESSION_SUFFIX,
            16384,
        )
        .unwrap();
        let session = active.as_mut().unwrap();
        assert_eq!(session.run.turn(), 1);
        assert_eq!(
            projected(session.run.full_history()).unwrap(),
            projected(selected).unwrap()
        );
        session.tools(&body, &response(&["read_source"])).unwrap();
        session
            .finish(vec![result("provider-0", "new source")])
            .unwrap();
        assert!(
            session.run.next_step().is_err(),
            "handoff cannot reset remaining global budget"
        );
    }

    #[test]
    fn provider_usage_is_on_the_run_before_tools_and_survives_rebuild() {
        use knowledge::models::ChatUsage;
        let body = request(vec![Message::system("main")], "first progress");
        let mut active = None;
        Session::prepare(
            &mut active,
            &body,
            crate::agent_runtime::SESSION_PREFIX,
            crate::agent_runtime::ANALYSIS_SESSION_SUFFIX,
            16384,
        )
        .unwrap();
        let mut turn = response(&["read_source"]);
        turn.usage = Some(ChatUsage {
            prompt_tokens: Some(100),
            completion_tokens: Some(7),
            total_tokens: Some(107),
            cached_tokens: Some(3),
            reasoning_tokens: Some(1),
        });
        active.as_mut().unwrap().note_provider_usage(&turn);
        let usage = active.as_ref().unwrap().run.usage();
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 7);
        assert_eq!(usage.total_tokens, 107);
        assert_eq!(usage.cached_input_tokens, 3);
        assert_eq!(usage.reasoning_tokens, 1);
        active.as_mut().unwrap().tools(&body, &turn).unwrap();
        assert_eq!(active.as_ref().unwrap().run.usage().input_tokens, 100);
        active
            .as_mut()
            .unwrap()
            .finish(vec![result("provider-0", "原文")])
            .unwrap();
        let next = request(active.as_ref().unwrap().run.full_history(), "next");
        Session::prepare(
            &mut active,
            &next,
            crate::agent_runtime::SESSION_PREFIX,
            crate::agent_runtime::ANALYSIS_SESSION_SUFFIX,
            16384,
        )
        .unwrap();
        assert_eq!(active.as_ref().unwrap().run.usage().input_tokens, 100);
        let mut again = response(&["read_source"]);
        again.usage = Some(ChatUsage {
            prompt_tokens: Some(50),
            completion_tokens: Some(1),
            total_tokens: Some(51),
            ..ChatUsage::default()
        });
        active.as_mut().unwrap().note_provider_usage(&again);
        assert_eq!(active.as_ref().unwrap().run.usage().input_tokens, 150);
        let kept = active.as_ref().unwrap().run.usage().input_tokens;
        active
            .as_mut()
            .unwrap()
            .note_provider_usage(&response(&["read_source"]));
        assert_eq!(active.as_ref().unwrap().run.usage().input_tokens, kept);
    }
}
