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
            max_draft_docx_bytes: crate::analysis::draft::DRAFT_MAX_DOCX_BYTES,
        },
    )
    .unwrap()
}

#[test]
fn configured_max_turns_is_not_raised_to_the_outline_estimate() {
    let mut input = input();
    input.source_units = (0..20)
        .map(|ordinal| Source {
            source_unit_revision_id: format!("s{ordinal}"),
            document_id: "document".into(),
            text: String::new(),
            locator: json!({}),
            ordinal,
        })
        .collect();
    let estimate = crate::analysis::draft::outline_turn_cap(&input);
    assert!(estimate > crate::analysis::draft::OUTLINE_MAX_TURNS);
    let mut limits = config().limits;
    let cap = crate::analysis::draft::OUTLINE_MAX_TURNS;
    limits.max_turns = cap;
    limits.max_tool_calls = 1;
    limits.max_read_bytes = 1;
    let applied = limits.clone().at_least_for(&input).unwrap();
    assert_eq!(applied.max_turns, cap);
    assert_eq!(applied.max_tool_calls, cap * 12);
    assert!(applied.max_read_bytes >= cap * applied.max_tool_result_bytes * 4);
    limits.max_turns = 0;
    let applied = limits.at_least_for(&input).unwrap();
    assert_eq!(applied.max_turns, crate::analysis::draft::OUTLINE_MAX_TURNS);
    assert_ne!(applied.max_turns, estimate);
}
