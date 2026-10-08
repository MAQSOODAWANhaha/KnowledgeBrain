use super::agent::{Config, Limits};
use super::*;
use crate::authoring_runtime::AuthoringRuntimeContractV1;
use serde_json::json;

mod input;

fn input() -> FrozenInput {
    FrozenInput {
        schema_version: 1,
        project_id: "project".into(),
        document_set_id: "set".into(),
        documents: vec![],
        document_relations: vec![],
        decisions: vec![],
        structured_forms: vec![],
        source_units: vec![Source {
            source_unit_revision_id: "source".into(),
            document_id: "document".into(),
            text: "提交格式见附表甲。".into(),
            locator: json!({"heading_path":"须知"}),
            ordinal: 0,
        }],
    }
}

fn grid_citation_input() -> FrozenInput {
    let mut input = input();
    input.source_units[0].text.clear();
    input.structured_forms.push(
        json!({"form_definition_revision_id":"grid","source_unit_revision_id":"source",
        "definition":{"kind":"grid","row_count":2,"column_count":2,"widths_mm":[40,40],
            "cells":[{"row":0,"column":0,"row_span":1,"col_span":2,"text":"产品条件"},
                {"row":1,"column":0,"row_span":1,"col_span":1,"text":"投标时逐项响应"},
                {"row":1,"column":1,"row_span":1,"col_span":1,"text":"供货时提供证书"}]}}),
    );
    input
}

pub(super) fn config() -> Config {
    // Test provider never makes network calls; production resolves this identity
    // from configuration and freezes it before execution.
    let provider: AuthoringRuntimeContractV1 = serde_json::from_value(json!({
        "schema_version":1,"base_url":"https://llm.example/v1",
        "endpoint":"https://llm.example/v1/chat/completions","protocol":"openai_chat_completions_sse","model_id":"test-frozen-model",
        "credential_ref":"env:LLM_API_KEY","stream":true,"max_tokens":8192,"timeout_ms":180000,"response_mode":"tool_calls",
        "transport_retries":0,"temperature":null,"reasoning_effort":null
    }))
    .unwrap();
    Config::with_provider(
        provider,
        Limits {
            max_no_progress_turns: 6,
            max_focus_turns: 24,
            max_focus_replans: 2,
            max_turns: 40,
            max_tool_calls: 80,
            max_read_bytes: 1000000,
            max_context_bytes: 100000,
            max_history_bytes: 32000,
            max_context_tokens: 1_000_000,
            image_token_reserve: 16000,
            token_safety_margin: 2048,
            max_tool_result_bytes: 16000,
            max_review_rounds: 3,
            max_source_view_edge: 1600,
            max_source_view_bytes: 16000,
            reviewer_reserve: 0,
            pack_max_units: 1,
            pack_max_chars: 0,
            pack_max_turns: 0,
            draft_bind_terms: vec![],
        },
    )
    .unwrap()
}

#[test]
fn at_least_for_does_not_scale_tool_or_read_caps() {
    let input = input();
    let mut limits = config().limits;
    let configured = limits.max_turns;
    limits.max_tool_calls = 1;
    limits.max_read_bytes = 1;
    limits.reviewer_reserve = 3;
    let applied = limits.clone().at_least_for(&input).unwrap();
    assert_eq!(applied.max_turns, configured);
    assert_eq!(applied.max_tool_calls, 1);
    assert_eq!(applied.max_read_bytes, 1);
    assert_eq!(applied.reviewer_reserve, 0);
    limits.max_turns = 0;
    let applied = limits.at_least_for(&input).unwrap();
    assert_eq!(applied.max_turns, 0);
    assert_eq!(applied.max_tool_calls, 1);
    assert_eq!(applied.max_read_bytes, 1);
}

#[test]
fn max_turns_zero_is_not_a_contract_mismatch() {
    let mut limits = config().limits;
    limits.max_turns = 0;
    let provider = config().provider;
    let built = Config::with_provider(provider, limits).unwrap();
    assert_eq!(built.limits.max_turns, 0);
    built.validate().unwrap();
    let focus = built.limits.max_focus_turns * (built.limits.max_focus_replans + 1);
    assert_eq!(
        super::agent::repair::tasks::limit(&built.limits).unwrap(),
        focus
    );
    let mut broken = built.clone();
    broken.limits.max_tool_calls = 0;
    let error = broken.validate().unwrap_err();
    assert_ne!(error.code, "FROZEN_INPUT_DIGEST_MISMATCH");
    assert!(
        error.message.contains("max_tool_calls"),
        "{}",
        error.message
    );
}

#[test]
fn analysis_result_writes_usage_and_the_finished_outline() {
    let outline = crate::outline::tools::Draft {
        chapters: vec![crate::outline::ChapterOutline {
            id: "letter".into(),
            parent_id: None,
            order: 0,
            title: "投标函".into(),
            purpose: crate::outline::ChapterPurpose::Response,
            requirement_ids: vec![],
        }],
        bindings: vec![crate::outline::chapters::AttachmentBinding {
            form_id: "form-1".into(),
            chapter_id: "letter".into(),
        }],
        slots_submitted: true,
        finished: true,
        ..Default::default()
    };
    let result = AnalysisResult {
        schema_version: 2,
        frozen_input_sha256: "sha".into(),
        analysis: Analysis::default(),
        review: Review::default(),
        quality: "needs_review".into(),
        source_views: Default::default(),
        usage: crate::agent_runtime::TokenUsage {
            input_tokens: 11,
            output_tokens: 2,
            total_tokens: 13,
            cached_input_tokens: 1,
            reasoning_tokens: 0,
        },
        outline: Some(outline),
    };
    let value = serde_json::to_value(&result).unwrap();
    assert_eq!(value["usage"]["input_tokens"], 11);
    assert_eq!(value["usage"]["output_tokens"], 2);
    assert_eq!(value["usage"]["total_tokens"], 13);
    assert_eq!(value["usage"]["cached_input_tokens"], 1);
    assert_eq!(value["outline"]["finished"], true);
    assert_eq!(value["outline"]["chapters"][0]["id"], "letter");
    assert_eq!(value["outline"]["bindings"][0]["form_id"], "form-1");
    assert_eq!(value["outline"]["slots_submitted"], true);
    let bare = serde_json::json!({
        "schema_version": 2,
        "frozen_input_sha256": "sha",
        "analysis": serde_json::to_value(Analysis::default()).unwrap(),
        "review": serde_json::to_value(Review::default()).unwrap(),
        "quality": "needs_review",
        "source_views": {}
    });
    let loaded: AnalysisResult = serde_json::from_value(bare).unwrap();
    assert!(loaded.usage.is_empty());
    assert!(loaded.outline.is_none());
    let mut empty = result;
    empty.usage = Default::default();
    empty.outline = None;
    let omitted = serde_json::to_value(&empty).unwrap();
    assert!(omitted.get("usage").is_none());
    assert!(omitted.get("outline").is_none());
}
