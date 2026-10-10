use super::agent::{Config, Limits};
use super::*;
use crate::authoring_runtime::AuthoringRuntimeContractV1;
use serde_json::json;

mod input;

fn input() -> FrozenInput {
    FrozenInput {
        schema_version: 2,
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
        "credential_ref":"env:LLM_API_KEY","stream":true,"output_token_reserve":8192,"timeout_ms":180000,"response_mode":"tool_calls",
        "transport_retries":0,"temperature":null,"reasoning_effort":null
    }))
    .unwrap();
    Config::with_provider(
        provider,
        Limits {
            vision_enabled: true,
            max_no_progress_turns: 6,
            max_focus_turns: 24,
            max_focus_replans: 2,

            max_context_tokens: 131_072,
            tokenizer: crate::agent_runtime::chat::TokenizerProfile {
                model_id: "test-frozen-model".into(),
                encoding: crate::agent_runtime::chat::TokenEncoding::O200kBase,
                calibration: Some(crate::agent_runtime::chat::TokenizerCalibration {
                    multiplier_bps: 10_000,
                    provenance: "synthetic test model double uses o200k_base".into(),
                }),
            },
            image_token_reserve: 16000,
            token_safety_margin: 2048,

            max_source_view_edge: 1600,
            max_source_view_bytes: 16000,
            draft_bind_terms: vec![],
        },
    )
    .unwrap()
}

/// Complete request token cost, including the frozen output reservation.
pub(super) fn request_tokens(body: &Value, config: &Config) -> usize {
    crate::agent_runtime::chat::estimate_input_tokens(
        body,
        &config.limits.tokenizer,
        config.limits.image_token_reserve,
        config.limits.token_safety_margin,
    )
    .unwrap()
        + config.provider.output_token_reserve as usize
}

pub(super) fn total_request_tokens(bytes: &[u8], config: &Config) -> usize {
    request_tokens(&serde_json::from_slice(bytes).unwrap(), config)
}

#[test]
fn production_configuration_has_no_operation_quotas() {
    let value = serde_json::to_value(config().limits).unwrap();
    for field in [
        "run_budget",
        "max_turns",
        "max_tool_calls",
        "max_read_bytes",
        "max_review_rounds",
    ] {
        assert!(
            value.get(field).is_none(),
            "removed operation quota: {field}"
        );
    }
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

#[test]
fn removed_byte_context_limits_and_missing_tokenizer_are_rejected() {
    for field in [
        "max_context_bytes",
        "max_history_bytes",
        "pack_max_chars",
        "max_tool_result_bytes",
        "run_budget",
        "max_turns",
        "max_read_bytes",
    ] {
        let mut value = json!(config().limits);
        value[field] = json!(131_072);
        assert!(
            serde_json::from_value::<Limits>(value).is_err(),
            "obsolete field {field} must not reintroduce a second context cap"
        );
    }
    let mut value = json!(config().limits);
    value.as_object_mut().unwrap().remove("tokenizer");
    assert!(serde_json::from_value::<Limits>(value).is_err());
}

#[test]
fn form_literal_matches_return_original_utf8_offsets_without_selecting_a_blank_policy() {
    let mut input = grid_citation_input();
    input.structured_forms[0]["definition"]["cells"][1]["text"] = json!("名称：甲甲甲；备注：甲甲");
    let mut analysis = Analysis::default();
    let mut coverage = Coverage::default();
    let args = json!({"form_id":"grid","offset":0,"limit":4,"find_text":"甲甲"});
    let read = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "read_form",
        &args,
        8192,
    )
    .unwrap();
    assert_eq!(
        read["matches"],
        json!([[],null,[
        {"row":1,"column":0,"start":9,"end":15},
        {"row":1,"column":0,"start":12,"end":18},
        {"row":1,"column":0,"start":30,"end":36}
    ],[]])
    );
    assert!(analysis.records.is_empty());
    let no_match = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        true,
        "read_form",
        &json!({"form_id":"grid","offset":2,"limit":1,"find_text":"名称:甲甲"}),
        8192,
    )
    .unwrap();
    assert_eq!(no_match["matches"], json!([[]]));
    let original = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        true,
        "read_form",
        &json!({"form_id":"grid","offset":2,"limit":1}),
        8192,
    )
    .unwrap();
    assert!(original.get("matches").is_none());
    assert_eq!(original["cells"][0], read["cells"][2]);
}

#[test]
fn form_literal_match_failures_do_not_acknowledge_reading() {
    let mut input = grid_citation_input();
    input.structured_forms[0]["definition"]["cells"][1]["text"] = json!("甲".repeat(100));
    let mut analysis = Analysis::default();
    let mut coverage = Coverage::default();
    for (query, budget) in [
        (json!(""), 8192),
        (Value::Null, 8192),
        (json!(7), 8192),
        (json!("甲"), 512),
    ] {
        assert!(
            tools::invoke(
                &input,
                &mut analysis,
                &mut coverage,
                false,
                "read_form",
                &json!({"form_id":"grid","offset":2,"limit":1,"find_text":query}),
                budget
            )
            .is_err()
        );
        assert!(coverage.form_cells.is_empty());
        assert!(analysis.records.is_empty());
    }
}

#[test]
fn source_line_spans_preserve_original_bytes_and_support_direct_citations() {
    let mut input = input();
    input.source_units[0].text = "标题\r\n第一项需\n要跨行保留。😀\n\n末行".into();
    let mut analysis = Analysis::default();
    let mut coverage = Coverage::default();
    let out = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "read_source",
        &json!({"source_id":"source","start":0,"max_bytes":4096}),
        8192,
    )
    .unwrap();
    let lines = out["line_spans"]
        .as_array()
        .expect("read source needs directly usable line spans");
    assert_eq!(
        lines
            .iter()
            .map(|line| line["text"].as_str().unwrap())
            .collect::<String>(),
        input.source_units[0].text
    );
    for line in lines {
        let start = line["start"].as_u64().unwrap() as usize;
        let end = line["end"].as_u64().unwrap() as usize;
        assert_eq!(&input.source_units[0].text[start..end], line["text"]);
        let expanded = evidence_refs::expand(&input, &line["citation_ref"]).unwrap();
        assert_eq!(
            expanded,
            json!({"source_id":"source","start":start,"end":end})
        );
        tools::validate_span(
            &input,
            &coverage,
            &Span {
                source_id: "source".into(),
                start,
                end,
                view_id: None,
                grid_cell: None,
            },
        )
        .unwrap();
    }
    assert_eq!(lines[0]["text"], "标题\r\n");
    let start = lines[1]["start"].as_u64().unwrap() as usize;
    let end = lines[2]["end"].as_u64().unwrap() as usize;
    assert_eq!(
        &input.source_units[0].text[start..end],
        "第一项需\n要跨行保留。😀\n"
    );
    tools::validate_span(
        &input,
        &coverage,
        &Span {
            source_id: "source".into(),
            start,
            end,
            view_id: None,
            grid_cell: None,
        },
    )
    .unwrap();
}

#[test]
fn reading_ranges_merge_but_do_not_hide_internal_holes() {
    let mut ranges = vec![];
    tools::cover(&mut ranges, 5, 8);
    tools::cover(&mut ranges, 0, 3);
    assert_eq!(ranges, vec![(0, 3), (5, 8)]);
    tools::cover(&mut ranges, 2, 6);
    assert_eq!(ranges, vec![(0, 8)]);
}

#[test]
fn grid_citations_require_actual_cell_reading_and_preserve_exact_source_identity() {
    let input = grid_citation_input();
    let citation: Span = serde_json::from_value(json!({"source_id":"source","start":0,"end":0,
        "grid_cell":{"form_id":"grid","row":1,"column":1}}))
    .unwrap();
    let mut analysis = Analysis::default();
    let mut coverage = Coverage::default();
    assert!(tools::validate_span(&input, &coverage, &citation).is_err());
    let first = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "read_form",
        &json!({"form_id":"grid","offset":0,"limit":2}),
        8192,
    )
    .unwrap();
    assert!(first["citations"][1].is_null());
    assert!(tools::validate_span(&input, &coverage, &citation).is_err());
    let args = json!({"source_id":"source","state":"requirement","reason":"原始网格明确约定义务"});
    assert!(
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            "set_disposition",
            &args,
            8192
        )
        .is_err()
    );
    let before = digest(&coverage).unwrap();
    assert!(
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            "read_form",
            &json!({"form_id":"grid","offset":2,"limit":2}),
            40
        )
        .is_err()
    );
    assert_eq!(digest(&coverage).unwrap(), before);
    let second = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "read_form",
        &json!({"form_id":"grid","offset":2,"limit":2}),
        8192,
    )
    .unwrap();
    assert_eq!(second["citations"][1], json!(citation));
    tools::validate_span(&input, &coverage, &citation).unwrap();
    tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "set_disposition",
        &args,
        8192,
    )
    .unwrap();
    // Reading grid evidence never establishes text or independent reviewer coverage.
    assert!(coverage.text.is_empty());
    assert!(tools::validate_span(&input, &Coverage::default(), &citation).is_err());
    let mut malformed = input.clone();
    malformed.structured_forms[0]["definition"]["row_count"] = json!(1);
    assert!(tools::validate_span(&malformed, &coverage, &citation).is_err());
    for patch in [
        json!({"source_id":"foreign"}),
        json!({"end":1}),
        json!({"view_id":"view"}),
        json!({"grid_cell":{"form_id":"grid","row":0,"column":1}}),
        json!({"grid_cell":{"form_id":"foreign","row":1,"column":1}}),
    ] {
        let mut bad = json!(citation);
        for (k, v) in patch.as_object().unwrap() {
            bad[k] = v.clone();
        }
        let bad: Span = serde_json::from_value(bad).unwrap();
        assert!(tools::validate_span(&input, &coverage, &bad).is_err());
    }
}

#[test]
fn annotated_source_pages_remain_bounded_and_never_skip_utf8_fragments() {
    let mut input = input();
    input.source_units[0].text = "甲😀\r\n\n乙\n".repeat(200);
    let mut analysis = Analysis::default();
    let mut coverage = Coverage::default();
    let mut start = 0;
    let mut read = String::new();
    while start < input.source_units[0].text.len() {
        let out = tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            true,
            "read_source",
            &json!({"source_id":"source","start":start,"max_bytes":4096}),
            1024,
        )
        .unwrap();
        assert!(serde_json::to_vec(&out).unwrap().len() <= 1024);
        assert_eq!(out["start"], start);
        let end = out["end"].as_u64().unwrap() as usize;
        assert!(end > start);
        let joined: String = out["line_spans"]
            .as_array()
            .unwrap()
            .iter()
            .map(|line| line["text"].as_str().unwrap())
            .collect();
        assert_eq!(joined, out["text"]);
        assert_eq!(joined, input.source_units[0].text[start..end]);
        read.push_str(&joined);
        start = end;
    }
    assert_eq!(read, input.source_units[0].text);
    assert!(
        analysis.coverage.text.is_empty(),
        "reviewer annotations cannot mark main evidence read"
    );
    assert!(tools::contains(coverage.text.get("source"), 0, start));
    let before = digest(&coverage).unwrap();
    assert!(
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            true,
            "read_source",
            &json!({"source_id":"source","start":0,"max_bytes":4096}),
            1
        )
        .is_err()
    );
    assert_eq!(digest(&coverage).unwrap(), before);
    let eof = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        true,
        "read_source",
        &json!({"source_id":"source","start":start,"max_bytes":4096}),
        1024,
    )
    .unwrap();
    assert_eq!(eof["line_spans"], json!([]));
}

#[test]
fn read_coverage_is_exact_utf8_and_oversized_results_do_not_mark_read() {
    let input = input();
    let mut analysis = Analysis::default();
    let mut coverage = Coverage::default();
    let out = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "read_source",
        &json!({"source_id":"source","start":0,"max_bytes":4}),
        1024,
    )
    .unwrap();
    assert_eq!(out["end"], 3);
    assert_eq!(out["text"], "提");
    assert!(
        tools::validate_span(
            &input,
            &coverage,
            &Span {
                source_id: "source".into(),
                start: 0,
                end: input.source_units[0].text.len(),
                view_id: None,
                grid_cell: None
            }
        )
        .is_err()
    );
    let prior = digest(&coverage).unwrap();
    assert!(
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            "read_source",
            &json!({"source_id":"source","start":1,"max_bytes":10}),
            1024
        )
        .is_err()
    );
    assert_eq!(digest(&coverage).unwrap(), prior);
    assert!(
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            "read_source",
            &json!({"source_id":"source","start":3,"max_bytes":10}),
            5
        )
        .is_err()
    );
    assert_eq!(digest(&coverage).unwrap(), prior);
    assert!(
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            "set_disposition",
            &json!({"source_id":"source","state":"non_requirement","reason":"read only prefix"}),
            1024
        )
        .is_err()
    );
}

#[test]
fn search_and_index_do_not_establish_reading_coverage() {
    let input = input();
    let mut analysis = Analysis::default();
    let mut coverage = Coverage::default();
    for (name, args) in [
        ("source_index", json!({"offset":0,"limit":10})),
        (
            "search_sources",
            json!({"query":"附表甲","offset":0,"limit":10}),
        ),
    ] {
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            name,
            &args,
            4096,
        )
        .unwrap();
    }
    assert_eq!(tools::reading_gaps(&input, &coverage).len(), 1);
    assert!(coverage.text.is_empty());
}

#[test]
fn reading_gaps_keep_interleaved_source_order_without_acknowledging_evidence() {
    let mut input = grid_citation_input();
    input.documents.push(json!({"id":"document"}));
    input.source_units.push(Source {
        source_unit_revision_id: "later-text".into(),
        document_id: "document".into(),
        text: "后续正文".into(),
        locator: json!({}),
        ordinal: 1,
    });
    let mut later_grid = input.source_units[0].clone();
    later_grid.source_unit_revision_id = "later-grid".into();
    later_grid.ordinal = 2;
    input.source_units.push(later_grid);
    let mut form = input.structured_forms[0].clone();
    form["form_definition_revision_id"] = json!("grid-later");
    form["source_unit_revision_id"] = json!("later-grid");
    // The form collection's order must not override the frozen source order.
    input.structured_forms.insert(0, form);
    let mut coverage = Coverage::default();
    coverage.form_cells.insert("grid".into(), vec![(0, 2)]);
    let before = digest(&coverage).unwrap();
    let gaps = tools::reading_gaps(&input, &coverage);
    assert_eq!(gaps.len(), 4);
    assert_eq!(gaps[0]["kind"], "unread_metadata");
    assert_eq!(gaps[1]["form_id"], "grid");
    assert_eq!(gaps[1]["read_ranges"], json!([[0, 2]]));
    assert_eq!(gaps[2]["source_id"], "later-text");
    assert_eq!(gaps[3]["form_id"], "grid-later");
    assert_eq!(digest(&coverage).unwrap(), before);
    tools::cover(coverage.form_cells.get_mut("grid").unwrap(), 2, 4);
    let gaps = tools::reading_gaps(&input, &coverage);
    assert_eq!(gaps.len(), 3);
    assert_eq!(gaps[1]["source_id"], "later-text");
    assert_eq!(gaps[2]["form_id"], "grid-later");
}

#[test]
fn search_finds_sparse_grid_text_with_dense_read_offsets_and_no_evidence() {
    let mut input = grid_citation_input();
    input.source_units[0].text = "证书".into();
    input.structured_forms[0]["definition"]["cells"][0]["text"] = json!("证书");
    let mut analysis = Analysis::default();
    let mut coverage = Coverage::default();
    let before = digest(&coverage).unwrap();
    let mut hits = vec![];
    let mut offset = 0;
    loop {
        let page = tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            "search_sources",
            &json!({"query":"证书","offset":offset,"limit":100}),
            220,
        )
        .unwrap();
        assert!(serde_json::to_vec(&page).unwrap().len() <= 220);
        assert_eq!(page["total"], 3);
        let next = page["next"].as_u64().unwrap();
        assert!(next > offset);
        hits.extend(page["items"].as_array().unwrap().clone());
        offset = next;
        if page["next"] == page["total"] {
            break;
        }
    }
    assert_eq!(
        hits,
        vec![
            json!({"source_id":"source","start":0,"end":6}),
            json!({"source_id":"source","grid_cell":{"form_id":"grid","row":0,"column":0},"form_offset":0,"cell_match":{"start":0,"end":6}}),
            json!({"source_id":"source","grid_cell":{"form_id":"grid","row":1,"column":1},"form_offset":3,"cell_match":{"start":15,"end":21}}),
        ]
    );
    assert_eq!(digest(&coverage).unwrap(), before);
    let citation: Span = serde_json::from_value(
        json!({"source_id":"source","start":0,"end":0,"grid_cell":hits[2]["grid_cell"]}),
    )
    .unwrap();
    assert!(tools::validate_span(&input, &coverage, &citation).is_err());
    let read = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "read_form",
        &json!({"form_id":"grid","offset":hits[2]["form_offset"],"limit":1}),
        4096,
    )
    .unwrap();
    assert_eq!(read["cells"][0]["text"], "供货时提供证书");
    tools::validate_span(&input, &coverage, &citation).unwrap();
}

#[test]
#[ignore = "requires KB_TENDER_FROZEN_SOURCE from the shared parser; no model calls"]
fn frozen_parser_grids_remain_searchable_and_readable_as_exact_evidence() {
    let bytes = std::fs::read(std::env::var("KB_TENDER_FROZEN_SOURCE").unwrap()).unwrap();
    let input: FrozenInput = serde_json::from_slice(&bytes).unwrap();
    tools::validate_input(&input).unwrap();
    assert!(!input.structured_forms.is_empty());
    let mut analysis = Analysis::default();
    let mut coverage = Coverage::default();
    let mut searched = 0;
    let mut read_cells = 0;
    for f in &input.structured_forms {
        let definition = &f["definition"];
        assert_eq!(definition["schema_version"], 3);
        let columns = definition["column_count"].as_u64().unwrap();
        for cell in definition["cells"].as_array().unwrap() {
            let coordinate = json!({"form_id":f["form_definition_revision_id"],"row":cell["row"],"column":cell["column"]});
            let index = cell["row"].as_u64().unwrap() * columns + cell["column"].as_u64().unwrap();
            let text = cell["text"].as_str().unwrap();
            if !text.trim().is_empty() {
                let before = digest(&coverage).unwrap();
                let mut offset = 0;
                let mut found = false;
                loop {
                    let page = tools::invoke(
                        &input,
                        &mut analysis,
                        &mut coverage,
                        false,
                        "search_sources",
                        &json!({"query":text,"offset":offset,"limit":100}),
                        2048,
                    )
                    .unwrap();
                    found |= page["items"].as_array().unwrap().iter().any(|hit| {
                        hit["source_id"] == f["source_unit_revision_id"]
                            && hit["grid_cell"] == coordinate
                            && hit["form_offset"] == index
                            && hit["cell_match"] == json!({"start":0,"end":text.len()})
                    });
                    if page["next"] == page["total"] {
                        break;
                    }
                    offset = page["next"].as_u64().unwrap();
                }
                assert!(found, "missing grid match: {coordinate}");
                assert_eq!(digest(&coverage).unwrap(), before);
                searched += 1;
            }
            let read = tools::invoke(
                &input,
                &mut analysis,
                &mut coverage,
                false,
                "read_form",
                &json!({"form_id":f["form_definition_revision_id"],"offset":index,"limit":1}),
                65536,
            )
            .unwrap();
            assert_eq!(read["cells"][0], *cell);
            let citation: Span = serde_json::from_value(read["citations"][0].clone()).unwrap();
            tools::validate_span(&input, &coverage, &citation).unwrap();
            read_cells += 1;
        }
    }
    println!(
        "{}",
        json!({"sources":input.source_units.len(),"forms":input.structured_forms.len(),"searched_nonempty_anchors":searched,"read_anchors":read_cells})
    );
    assert!(analysis.records.is_empty());
}

#[test]
fn read_source_errors_identify_repairable_offsets_without_claiming_coverage() {
    let input = input();
    let mut analysis = Analysis::default();
    let mut coverage = Coverage::default();
    let end = input.source_units[0].text.len();
    for (start, max_bytes, expected) in [
        (
            1,
            100,
            "INVALID_FIELD /start: byte 1 is inside a UTF-8 character; adjacent boundaries are 0 and 3",
        ),
        (end + 1, 100, "INVALID_FIELD /start: byte"),
        (0, 0, "INVALID_FIELD /max_bytes:"),
    ] {
        let error = tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            "read_source",
            &json!({"source_id":"source","start":start,"max_bytes":max_bytes}),
            4096,
        )
        .unwrap_err();
        assert!(error.contains(expected), "{error}");
        assert!(
            coverage.text.is_empty(),
            "a suggested boundary is not delivered text"
        );
    }
    // EOF remains a valid empty read. A caller can explicitly use a suggested
    // boundary; the tool never silently relocates the requested start.
    for start in [0, 3, end] {
        let result = tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            "read_source",
            &json!({"source_id":"source","start":start,"max_bytes":100}),
            4096,
        )
        .unwrap();
        assert_eq!(result["start"], start);
    }
}

#[test]
fn text_span_errors_identify_the_bad_offset_without_adjusting_the_citation() {
    let input = input();
    let coverage = Coverage::default();
    for (start, end, expected) in [
        (0, 0, "start 0 must be less than end 0"),
        (
            0,
            input.source_units[0].text.len() + 1,
            "exceeds source length",
        ),
        (
            1,
            6,
            "start 1 is not a UTF-8 boundary; adjacent boundaries are 0 and 3",
        ),
        (
            0,
            4,
            "end 4 is not a UTF-8 boundary; adjacent boundaries are 3 and 6",
        ),
    ] {
        let citation = Span {
            start,
            end,
            source_id: "source".into(),
            view_id: None,
            grid_cell: None,
        };
        let error = tools::validate_span(&input, &coverage, &citation).unwrap_err();
        assert!(error.contains(expected), "{error}");
        assert_eq!((citation.start, citation.end), (start, end));
    }
    for (start, end) in [(0, 3), (3, 6)] {
        let citation = Span {
            start,
            end,
            source_id: "source".into(),
            view_id: None,
            grid_cell: None,
        };
        tools::validate_text_span(&input, &citation).unwrap();
        assert_eq!(
            tools::validate_span(&input, &coverage, &citation).unwrap_err(),
            "read the cited source range before using it",
            "choosing a suggested boundary must not establish delivery"
        );
    }
}
