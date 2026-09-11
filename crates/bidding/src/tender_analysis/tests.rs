use super::agent::{Checkpoint, Config, Journal, Limits, Model, Role};
use super::*;
use crate::{agent_error::AgentError, authoring_runtime::AuthoringRuntimeContractV1};
use async_trait::async_trait;
use knowledge::models::{ChatToolCall, ChatTurn};
use serde_json::json;
use std::{
    collections::{BTreeMap, VecDeque},
    sync::Mutex,
};
use tokio_util::sync::CancellationToken;

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
fn span() -> Span {
    Span {
        view_id: None,
        grid_cell: None,
        source_id: "source".into(),
        start: 0,
        end: input().source_units[0].text.len(),
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

#[test]
fn form_literal_matches_return_original_utf8_offsets_without_selecting_a_blank_policy() {
    let mut input = grid_citation_input();
    input.structured_forms[0]["definition"]["cells"][1]["text"] = json!("名称：甲甲甲；备注：甲甲");
    let mut analysis = Analysis::default();
    let mut coverage = Coverage::default();
    let args = json!({"form_id":"grid","offset":0,"limit":4,"find_text":"甲甲"});
    let schema = tools::schemas(false)
        .into_iter()
        .find(|t| t["function"]["name"] == "read_form")
        .unwrap();
    assert!(
        jsonschema::JSONSchema::compile(&schema["function"]["parameters"])
            .unwrap()
            .is_valid(&args)
    );
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
fn grid_grounded_obligations_publish_without_fabricating_text_quotes() {
    let input = grid_citation_input();
    let mut analysis = Analysis::default();
    let mut coverage = Coverage::default();
    let read = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "read_form",
        &json!({"form_id":"grid","offset":0,"limit":4}),
        8192,
    )
    .unwrap();
    let response = read["citations"][2].clone();
    let delivery = read["citations"][3].clone();
    let mut record = requirement();
    record["sources"] = json!([response, delivery]);
    record["data"]["response"][0]["grounds"] = json!([response]);
    record["data"]["applicability"]["grounds"] = json!([response]);
    record["data"]["compliance"][0]["grounds"] = json!([response]);
    record["data"]["proofs"] = json!([{"description":"供货时证书","subject":"产品","validity":"原文未规定",
        "issuer":"原文未规定","condition":"供货时提供","grounds":[delivery],"name_required":false,"page_required":false}]);
    let schema = tools::schemas(false)
        .into_iter()
        .find(|t| t["function"]["name"] == "put_record")
        .unwrap();
    assert!(
        jsonschema::JSONSchema::compile(&schema["function"]["parameters"])
            .unwrap()
            .is_valid(&record)
    );
    let id = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "put_record",
        &record,
        8192,
    )
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let result = AnalysisResult {
        schema_version: 1,
        frozen_input_sha256: digest(&input).unwrap(),
        review: Review {
            analysis_sha256: digest(&analysis).unwrap(),
            coverage: Coverage::default(),
            findings: vec![],
        },
        analysis,
        quality: "needs_review".into(),
        source_views: BTreeMap::new(),
    };
    let publication = super::postgres::publication(&input, &result).unwrap();
    let req = &publication["requirements"][0];
    assert_eq!(req["source_spans"], json!([]));
    assert_eq!(req["structured_form_revision_ids"], json!(["grid"]));
    assert_eq!(req["response_needs"][0]["grounds"][0], response);
    assert_eq!(
        publication["analysis_result"]["analysis"]["records"][&id]["data"]["proofs"][0]["grounds"]
            [0],
        delivery
    );
    assert_eq!(
        publication["analysis_result"]["analysis"]["records"][&id]["data"]["proofs"][0]["condition"],
        "供货时提供"
    );
}

#[test]
fn template_text_policies_cannot_copy_bidder_slots_or_duplicate_fixed_wording() {
    let input = input();
    let mut analysis = Analysis::default();
    analysis
        .coverage
        .text
        .insert("source".into(), vec![(0, span().end)]);
    let mut record: Record = serde_json::from_value(json!({"id":"template","sources":[span()],"data":{
        "kind":"template","label":"甲","title":"格式","parent":null,"order":null,"purpose":"投标格式",
        "applicability":{"state":"applicable","condition":"本项目","scope":"投标文件","grounds":[span()]},
        "regions":[{"source":span(),"role":"fixed_text","form_id":null,"cells":[],"instruction":"保留固定文字"}]}})).unwrap();
    assert!(tools::validate_record(&input, &analysis, &record).is_ok());
    for role in [
        RegionRole::FixedText,
        RegionRole::BidderBlank,
        RegionRole::Signature,
    ] {
        let mut overlapping = record.clone();
        let RecordData::Template { regions, .. } = &mut overlapping.data else {
            unreachable!()
        };
        let mut region = regions[0].clone();
        region.role = role;
        // Partial overlap is also unsafe, not only identical whole-page ranges.
        region.source.start = 3;
        regions.push(region);
        let before = digest(&analysis).unwrap();
        let error = tools::validate_record(&input, &analysis, &overlapping).unwrap_err();
        assert!(
            error.starts_with("INVALID_FIELD /data/regions/1/source:"),
            "{error}"
        );
        assert!(error.contains("regions 0 and 1 overlap"));
        let mut args = serde_json::to_value(&overlapping).unwrap();
        args["id"] = Value::Null;
        let mut coverage = analysis.coverage.clone();
        assert!(
            tools::invoke(
                &input,
                &mut analysis,
                &mut coverage,
                false,
                "put_record",
                &args,
                16000
            )
            .unwrap_err()
            .contains("regions 0 and 1 overlap")
        );
        assert_eq!(digest(&analysis).unwrap(), before);
    }
    let RecordData::Template { regions, .. } = &mut record.data else {
        unreachable!()
    };
    let mut blank = regions[0].clone();
    regions[0].source.end = 3;
    blank.source.start = 3;
    blank.role = RegionRole::BidderBlank;
    regions.push(blank);
    assert!(
        tools::validate_record(&input, &analysis, &record).is_ok(),
        "adjacent byte ranges remain valid"
    );
}

#[test]
fn template_grid_regions_must_be_contiguous_before_review() {
    let (input, mut analysis, _) = field_relation_fixture();
    let mut record = analysis.records["template-a"].clone();
    let RecordData::Template { regions, .. } = &mut record.data else {
        unreachable!()
    };
    let mut note = regions[0].clone();
    note.form_id = None;
    note.cells.clear();
    regions.insert(1, note);
    let before = digest(&analysis).unwrap();
    let mut coverage = analysis.coverage.clone();
    let coverage_before = digest(&coverage).unwrap();
    assert!(
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            "put_record",
            &serde_json::to_value(&record).unwrap(),
            16000
        )
        .unwrap_err()
        .contains("contiguous")
    );
    assert_eq!(digest(&analysis).unwrap(), before);
    assert_eq!(digest(&coverage).unwrap(), coverage_before);
    let RecordData::Template { regions, .. } = &mut record.data else {
        unreachable!()
    };
    let note = regions.remove(1);
    regions.push(note);
    assert!(
        tools::validate_record(&input, &analysis, &record).is_ok(),
        "an explicitly placed note after a complete grid remains valid"
    );
}

#[test]
fn visual_evidence_cannot_be_mistaken_for_editable_template_wording() {
    let (input, mut analysis, _) = field_relation_fixture();
    let view = test_view();
    let view_id = digest(&view.identity).unwrap();
    analysis
        .coverage
        .views
        .insert(view_id.clone(), view.identity);
    let visual = Span {
        source_id: "source".into(),
        start: 0,
        end: 0,
        view_id: Some(view_id),
        grid_cell: None,
    };
    assert!(tools::validate_span(&input, &analysis.coverage, &visual).is_ok());
    let mut record = analysis.records["template-a"].clone();
    let RecordData::Template { regions, .. } = &mut record.data else {
        unreachable!()
    };
    let mut wording = regions[0].clone();
    wording.source = visual.clone();
    wording.form_id = None;
    wording.cells.clear();
    *regions = vec![wording];
    let before = digest(&analysis).unwrap();
    let mut coverage = analysis.coverage.clone();
    let coverage_before = digest(&coverage).unwrap();
    assert!(
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            "put_record",
            &serde_json::to_value(&record).unwrap(),
            16000
        )
        .unwrap_err()
        .contains("editable")
    );
    assert_eq!(digest(&analysis).unwrap(), before);
    assert_eq!(digest(&coverage).unwrap(), coverage_before);
    record.sources = vec![visual];
    record.data = RecordData::Unresolved {
        problem: "原页可读，仍需统一解析服务提供可编辑文字".into(),
        affected: vec![],
        candidates: vec![],
    };
    assert!(tools::validate_record(&input, &analysis, &record).is_ok());
}

#[test]
fn empty_grid_anchor_requires_an_explicit_role_without_defaulting_to_bidder_blank() {
    let (mut input, analysis, _) = field_relation_fixture();
    input.structured_forms[0]["definition"]["cells"][2]["text"] = json!("");
    let mut record = analysis.records["template-a"].clone();
    let RecordData::Template { regions, .. } = &mut record.data else {
        unreachable!()
    };
    let empty_cell = regions[1].cells.pop().unwrap();
    assert!(
        tools::validate_record(&input, &analysis, &record)
            .unwrap_err()
            .contains("every output grid anchor")
    );
    let RecordData::Template { regions, .. } = &mut record.data else {
        unreachable!()
    };
    let mut policy = regions[0].clone();
    policy.cells = vec![empty_cell];
    policy.instruction = "保留原表空格".into();
    regions.push(policy);
    let before = digest(&record).unwrap();
    assert!(tools::validate_record(&input, &analysis, &record).is_ok());
    assert_eq!(digest(&record).unwrap(), before);
    let RecordData::Template { regions, .. } = &record.data else {
        unreachable!()
    };
    assert_eq!(regions.last().unwrap().role, RegionRole::FixedText);
}

#[test]
fn template_grid_requires_every_anchor_role() {
    let (input, mut analysis, _) = field_relation_fixture();
    let record = analysis.records["template-a"].clone();
    assert!(
        tools::validate_record(&input, &analysis, &record).is_ok(),
        "fixture assigns every actual anchor"
    );
    let mut incomplete = record.clone();
    let RecordData::Template { regions, .. } = &mut incomplete.data else {
        unreachable!()
    };
    regions[1].cells.pop();
    let error = tools::validate_record(&input, &analysis, &incomplete).unwrap_err();
    assert!(error.starts_with("INVALID_FIELD /data/regions:"), "{error}");
    assert!(error.contains("every output grid anchor"));
    assert!(error.contains("first missing anchor Some("));
    let mut args = serde_json::to_value(&incomplete).unwrap();
    args["id"] = Value::Null;
    let before = digest(&analysis).unwrap();
    let mut coverage = analysis.coverage.clone();
    assert!(
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            "put_record",
            &args,
            16000,
        )
        .unwrap_err()
        .contains("every output grid anchor")
    );
    assert_eq!(digest(&analysis).unwrap(), before);
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
    assert!(tools::validate_span(&input, &coverage, &span()).is_err());
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
fn reading_ranges_merge_but_do_not_hide_internal_holes() {
    let mut ranges = vec![];
    tools::cover(&mut ranges, 5, 8);
    tools::cover(&mut ranges, 0, 3);
    assert_eq!(ranges, vec![(0, 3), (5, 8)]);
    tools::cover(&mut ranges, 2, 6);
    assert_eq!(ranges, vec![(0, 8)]);
}

#[test]
fn foreign_spans_and_reviewer_mutations_are_rejected() {
    let input = input();
    let mut analysis = Analysis::default();
    let mut coverage = Coverage::default();
    let foreign = Span {
        view_id: None,
        grid_cell: None,
        source_id: "other-project".into(),
        start: 0,
        end: 3,
    };
    assert!(tools::validate_span(&input, &coverage, &foreign).is_err());
    assert!(
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            true,
            "set_disposition",
            &json!({"source_id":"source","state":"unresolved","reason":"x"}),
            1024
        )
        .is_err()
    );
    assert!(analysis.dispositions.is_empty());
}

fn requirement() -> Value {
    json!({"id":null,"sources":[span()],"data":{"kind":"requirement","text":"提交规定格式",
        "categories":["format"],"strength":"mandatory","compliance":[{"policy":"explicit_response","condition":"按须知提交","grounds":[span()]}],"applicability":{"state":"applicable","condition":"按须知提交","scope":"本次投标","grounds":[span()]},
        "response":[{"channel":"structured_form","description":"按附表编制","condition":"按须知提交","grounds":[span()]}],"scoring_rule":null,"proofs":[],"criteria":[]}})
}

#[test]
fn compliance_without_a_submission_requirement_does_not_invent_a_response() {
    let mut input = input();
    input.source_units[0].text = "费用由投标人自行承担。".into();
    let citation = Span {
        end: input.source_units[0].text.len(),
        ..span()
    };
    let mut analysis = read_analysis(&input);
    let mut coverage = analysis.coverage.clone();
    let mut args = requirement();
    args["sources"] = json!([citation]);
    args["data"]["text"] = json!(input.source_units[0].text);
    args["data"]["categories"] = json!(["commercial"]);
    args["data"]["compliance"] =
        json!([{"policy":"must_comply","condition":"投标活动","grounds":[citation]}]);
    args["data"]["applicability"] = json!({"state":"applicable","condition":"投标活动","scope":"费用承担","grounds":[citation]});
    args["data"]["response"] = json!([]);
    let result = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "put_record",
        &args,
        16000,
    )
    .unwrap();
    let record = &analysis.records[result["id"].as_str().unwrap()];
    assert_eq!(json!(record.data)["response"], json!([]));
    assert_eq!(json!(record.data)["compliance"], args["data"]["compliance"]);
}

#[test]
fn same_compliance_policy_preserves_distinct_conditions_and_ground_ranges() {
    let mut input = input();
    let first = "第一阶段自行承担费用。";
    input.source_units[0].text = format!("{first}第二阶段自行承担费用。");
    let all = Span {
        end: input.source_units[0].text.len(),
        ..span()
    };
    let grounds = [
        Span {
            end: first.len(),
            ..span()
        },
        Span {
            start: first.len(),
            ..all.clone()
        },
    ];
    let mut args = requirement();
    args["sources"] = json!([all]);
    args["data"]["text"] = json!(input.source_units[0].text);
    args["data"]["categories"] = json!(["commercial"]);
    args["data"]["applicability"]["grounds"] = json!([all]);
    args["data"]["response"] = json!([]);
    args["data"]["compliance"] = json!([
        {"policy":"must_comply","condition":"第一阶段","grounds":[grounds[0]]},
        {"policy":"must_comply","condition":"第二阶段","grounds":[grounds[1]]}
    ]);
    let mut analysis = read_analysis(&input);
    let mut coverage = analysis.coverage.clone();
    let saved = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "put_record",
        &args,
        16000,
    )
    .unwrap();
    assert_eq!(
        json!(analysis.records[saved["id"].as_str().unwrap()].data)["compliance"],
        args["data"]["compliance"]
    );
    let result = AnalysisResult {
        schema_version: 1,
        frozen_input_sha256: digest(&input).unwrap(),
        review: Review {
            analysis_sha256: digest(&analysis).unwrap(),
            coverage: Coverage::default(),
            findings: vec![],
        },
        analysis,
        quality: "needs_review".into(),
        source_views: BTreeMap::new(),
    };
    let publication = super::postgres::publication(&input, &result).unwrap();
    assert_eq!(
        publication["requirements"][0]["compliance_policy"],
        "must_comply"
    );
}

#[test]
fn record_validation_identifies_the_rejected_field_without_partial_writes() {
    let input = input();
    let mut analysis = read_analysis(&input);
    let mut coverage = analysis.coverage.clone();
    let mut valid = requirement();
    valid["data"]["criteria"] = json!([{"subject":"设备","aspect":"数量","operator":"=","value":"1","unit":"","condition":"本次交付","grounds":[span()]}]);
    valid["data"]["proofs"] = json!([{"description":"证明材料","subject":"投标人","validity":"","issuer":"","name_required":false,"page_required":false,"condition":"按原文提交","grounds":[span()]}]);
    for (path, replacement) in [
        ("/data/text", json!(" ")),
        ("/data/categories", json!([])),
        ("/data/response", json!([])),
        ("/data/response/0/condition", json!("")),
        (
            "/data/response/0/grounds/0",
            json!({"source_id":"source","start":1,"end":2}),
        ),
        ("/data/applicability/scope", json!("")),
        ("/data/criteria/0/operator", json!("")),
        ("/data/proofs/0/subject", json!("")),
        (
            "/sources/0",
            json!({"source_id":"missing","start":0,"end":1}),
        ),
    ] {
        let mut args = valid.clone();
        *args.pointer_mut(path).unwrap() = replacement;
        let before = (digest(&analysis).unwrap(), digest(&coverage).unwrap());
        let error = tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            "put_record",
            &args,
            16000,
        )
        .unwrap_err();
        assert!(
            error.starts_with(&format!("INVALID_FIELD {path}:")),
            "{error}"
        );
        assert_eq!(
            before,
            (digest(&analysis).unwrap(), digest(&coverage).unwrap())
        );
    }
    let mut duplicate = valid.clone();
    let policy = duplicate["data"]["compliance"][0].clone();
    duplicate["data"]["compliance"]
        .as_array_mut()
        .unwrap()
        .push(policy);
    let error = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "put_record",
        &duplicate,
        16000,
    )
    .unwrap_err();
    assert!(
        error.starts_with("INVALID_FIELD /data/compliance/1:"),
        "{error}"
    );
    assert!(error.contains("/data/compliance/0"), "{error}");
    assert!(analysis.records.is_empty());
    tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "put_record",
        &valid,
        16000,
    )
    .unwrap();
    assert_eq!(
        analysis.records.len(),
        1,
        "an unspecified numeric unit stays empty"
    );
}

#[test]
fn missing_strength_or_applicability_cannot_default_to_mandatory() {
    let mut value = requirement()["data"].clone();
    value.as_object_mut().unwrap().remove("strength");
    assert!(serde_json::from_value::<RecordData>(value).is_err());
    let mut value = requirement()["data"].clone();
    value["strength"] = json!("unknown");
    value["applicability"]["state"] = json!("unknown");
    let r: RecordData = serde_json::from_value(value).unwrap();
    assert!(matches!(
        r,
        RecordData::Requirement {
            strength: Strength::Unknown,
            ..
        }
    ));
}

fn read_analysis(input: &FrozenInput) -> Analysis {
    let mut analysis = Analysis::default();
    for source in &input.source_units {
        tools::cover(
            analysis
                .coverage
                .text
                .entry(source.source_unit_revision_id.clone())
                .or_default(),
            0,
            source.text.len(),
        );
    }
    analysis
}

#[test]
fn concurrent_policies_and_criteria_preserve_evidence_and_units() {
    let mut input = input();
    // Synthetic protocol fixture; real sample semantic accuracy needs model evaluation.
    input.source_units[0].text =
        "★网络处理能力≥20Gbps；不响应否决投标，优于要求加分，并提供证明材料。".into();
    let span = json!({"source_id":"source","start":0,"end":input.source_units[0].text.len()});
    let mut value = requirement();
    value["sources"] = json!([span]);
    value["data"]["compliance"] = json!([
        {"policy":"must_comply","condition":"不响应否决","grounds":[span]},
        {"policy":"scored","condition":"优于要求","grounds":[span]},
        {"policy":"explicit_response","condition":"提交证明","grounds":[span]}
    ]);
    value["data"]["criteria"] = json!([{"subject":"所投设备","aspect":"网络处理能力",
        "operator":"≥","value":"20","unit":"Gbps","condition":"★条款","grounds":[span]}]);
    let mut analysis = read_analysis(&input);
    let mut coverage = analysis.coverage.clone();
    let result = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "put_record",
        &value,
        16000,
    )
    .unwrap();
    let record = &analysis.records[result["id"].as_str().unwrap()];
    let RecordData::Requirement {
        compliance,
        criteria,
        ..
    } = &record.data
    else {
        panic!("requirement expected")
    };
    assert_eq!(compliance.len(), 3);
    assert_eq!(criteria[0].value, "20");
    assert_eq!(criteria[0].unit, "Gbps");
    value["data"]["criteria"][0]["grounds"][0]["source_id"] = json!("foreign-source");
    assert!(
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            "put_record",
            &value,
            16000
        )
        .is_err()
    );
    assert_eq!(
        analysis.records.len(),
        1,
        "invalid criterion must not leave a partial record"
    );
}

#[test]
fn reviewer_must_inspect_current_candidate_and_oversize_is_not_coverage() {
    let input = input();
    let mut analysis = read_analysis(&input);
    let mut main = analysis.coverage.clone();
    let result = tools::invoke(
        &input,
        &mut analysis,
        &mut main,
        false,
        "put_record",
        &requirement(),
        16000,
    )
    .unwrap();
    let mut review = main.clone();
    assert_eq!(tools::review_gaps(&input, &analysis, &review).len(), 1);
    let query = json!({"view":"detail","kind":"all","offset":0,"limit":10});
    assert!(
        tools::invoke(
            &input,
            &mut analysis,
            &mut review,
            true,
            "inspect_analysis",
            &query,
            100
        )
        .is_err()
    );
    assert!(review.candidate.is_empty());
    tools::invoke(
        &input,
        &mut analysis,
        &mut review,
        true,
        "inspect_analysis",
        &query,
        16000,
    )
    .unwrap();
    assert!(tools::review_gaps(&input, &analysis, &review).is_empty());
    let mut change = requirement();
    change["id"] = result["id"].clone();
    change["data"]["text"] = json!("修订后的要求");
    tools::invoke(
        &input,
        &mut analysis,
        &mut main,
        false,
        "put_record",
        &change,
        16000,
    )
    .unwrap();
    assert_eq!(
        tools::review_gaps(&input, &analysis, &review).len(),
        1,
        "old inspection cannot approve changed content"
    );
}

fn analysis_query_fixture() -> (FrozenInput, Analysis, Vec<String>) {
    let mut input = input();
    input.source_units.push(Source {
        source_unit_revision_id: "other-source".into(),
        document_id: "other-document".into(),
        ordinal: 1,
        ..input.source_units[0].clone()
    });
    let mut analysis = Analysis::default();
    let mut coverage = Coverage::default();
    let mut ids = Vec::new();
    for source_id in ["source", "other-source", "source"] {
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            "read_source",
            &json!({"source_id":source_id,"start":0,"max_bytes":1024}),
            16000,
        )
        .unwrap();
        let record = tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            "put_record",
            &json!({"id":null,"sources":[Span {source_id:source_id.into(),..span()}],
                "data":{"kind":"fact","name":"同名格式","value":source_id,"scope":"本文件"}}),
            16000,
        )
        .unwrap();
        ids.push(record["id"].as_str().unwrap().to_owned());
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            "set_disposition",
            &json!({"source_id":source_id,"state":"non_requirement","reason":"测试事实来源"}),
            16000,
        )
        .unwrap();
    }
    let relation = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "put_relation",
        &json!({"id":null,"from":ids[0],"to":ids[1],
            "from_target":{"kind":"record"},"to_target":{"kind":"record"},
            "kind":"references","state":"explicit","scope":"跨文件引用",
            "explanation":"依据原文引用","grounds":[span()]}),
        16000,
    )
    .unwrap();
    ids.push(relation["id"].as_str().unwrap().to_owned());
    (input, analysis, ids)
}

#[test]
fn relation_validation_identifies_endpoint_and_ground_fields_without_partial_writes() {
    let (input, mut analysis, ids) = analysis_query_fixture();
    let mut args = json!(analysis.relations[&ids[3]]);
    args.as_object_mut().unwrap().remove("from_record_sha256");
    args.as_object_mut().unwrap().remove("to_record_sha256");
    let mut coverage = analysis.coverage.clone();
    for (path, replacement) in [
        ("/from", json!("missing")),
        ("/from_target", json!({"kind":"proof","index":0})),
        ("/to_target", json!({"kind":"response","index":0})),
        ("/scope", json!("")),
        ("/explanation", json!("")),
        (
            "/grounds/0",
            json!({"source_id":"source","start":1,"end":2}),
        ),
    ] {
        let mut invalid = args.clone();
        *invalid.pointer_mut(path).unwrap() = replacement;
        let before = (digest(&analysis).unwrap(), digest(&coverage).unwrap());
        let error = tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            "put_relation",
            &invalid,
            16000,
        )
        .unwrap_err();
        assert!(
            error.starts_with(&format!("INVALID_FIELD {path}:")),
            "{error}"
        );
        assert_eq!(
            before,
            (digest(&analysis).unwrap(), digest(&coverage).unwrap())
        );
    }
    tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "put_relation",
        &args,
        16000,
    )
    .unwrap();
}

#[test]
fn inspect_analysis_exact_ids_and_source_filter_do_not_review_neighbours() {
    let (input, mut analysis, ids) = analysis_query_fixture();
    let mut coverage = Coverage::default();
    let query = json!({"view":"detail","kind":"all","offset":0,"limit":1,"ids":[ids[1]]});
    for reviewer in [false, true] {
        let schema = tools::schemas(reviewer)
            .into_iter()
            .find(|t| t["function"]["name"] == "inspect_analysis")
            .unwrap();
        assert!(
            jsonschema::JSONSchema::compile(&schema["function"]["parameters"])
                .unwrap()
                .is_valid(&query)
        );
    }
    let out = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        true,
        "inspect_analysis",
        &query,
        16000,
    )
    .unwrap();
    assert_eq!(out["total"], 1);
    assert_eq!(out["items"][0]["id"], ids[1]);
    assert_eq!(coverage.candidate.len(), 1);
    assert!(
        coverage
            .candidate
            .contains_key(&format!("record:{}", ids[1]))
    );
    assert!(
        coverage.text.is_empty(),
        "candidate inspection is not source reading"
    );

    let mut found = Vec::new();
    for offset in 0..2 {
        let out = tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            true,
            "inspect_analysis",
            &json!({"view":"detail","kind":"fact","source_id":"source","offset":offset,"limit":1}),
            16000,
        )
        .unwrap();
        assert_eq!(out["total"], 2);
        assert_eq!(out["next"], offset + 1);
        found.push(out["items"][0]["id"].as_str().unwrap().to_owned());
    }
    found.sort();
    let mut expected = vec![ids[0].clone(), ids[2].clone()];
    expected.sort();
    assert_eq!(
        found, expected,
        "same labels in another document must not leak into this source"
    );
    let out = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        true,
        "inspect_analysis",
        &json!({"view":"detail","kind":"all","source_id":"source","ids":[ids[1]],"offset":0,"limit":1}),
        16000,
    )
    .unwrap();
    assert_eq!(
        out["total"], 0,
        "filters intersect rather than broaden the query"
    );
}

#[tokio::test]
async fn active_scope_queries_exclude_unrelated_outcomes_but_allow_exact_cross_references() {
    let (input, analysis, ids) = analysis_query_fixture();
    for role in [Role::Main, Role::Reviewer] {
        let journal = MemoryJournal::default();
        *journal.interrupt_after.lock().unwrap() = Some(1);
        agent::run(
            &input,
            &config(),
            &journal,
            &work_script(vec![("set_work_note", active_work("source"))]),
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        let mut state = journal.load().await.unwrap().unwrap();
        state.analysis = analysis.clone();
        state.role = role.clone();
        if role == Role::Reviewer {
            state.source_review =
                Some(agent::source_review::initialize(&input, &config()).unwrap());
            agent::source_review::select_next(&input, &config(), &mut state).unwrap();
            state.reviewer_work = Some(serde_json::from_value(active_work("source")).unwrap());
        }
        *journal.state.lock().unwrap() = Some(state);
        *journal.interrupt_after.lock().unwrap() = Some(5);
        let model = work_script(vec![
            (
                "inspect_analysis",
                json!({"view":"detail","kind":"all","offset":0,"limit":50}),
            ),
            (
                "inspect_analysis",
                json!({"view":"detail","kind":"relation","offset":0,"limit":50}),
            ),
            (
                "inspect_analysis",
                json!({"view":"detail","kind":"disposition","offset":0,"limit":50}),
            ),
            (
                "inspect_analysis",
                json!({"view":"detail","kind":"all","offset":0,"limit":50,"ids":[ids[1]]}),
            ),
        ]);
        agent::run(
            &input,
            &config(),
            &journal,
            &model,
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        let state = journal.load().await.unwrap().unwrap();
        let pages: Vec<Value> = state
            .transcript
            .iter()
            .filter(|m| m["role"] == "tool")
            .map(|m| {
                serde_json::from_str::<Value>(m["content"].as_str().unwrap()).unwrap()["result"]
                    .clone()
            })
            .filter(|v| v["items"].is_array())
            .collect();
        assert_eq!(pages.len(), 4);
        assert_eq!(pages[0]["total"], 4);
        assert!(
            pages[0]["items"]
                .as_array()
                .unwrap()
                .iter()
                .all(|r| r["id"] != ids[1])
        );
        assert_eq!(pages[1]["total"], 1);
        assert_eq!(pages[1]["items"][0]["id"], ids[3]);
        assert_eq!(pages[2]["total"], 1);
        assert_eq!(pages[2]["items"][0]["source_id"], "source");
        assert_eq!(pages[3]["total"], 1);
        assert_eq!(pages[3]["items"][0]["id"], ids[1]);
        if role == Role::Reviewer {
            assert!(
                !state
                    .reviewer_coverage
                    .candidate
                    .contains_key(&format!("record:{}", ids[1]))
            );
            assert!(
                state
                    .pending_coverage
                    .as_ref()
                    .unwrap()
                    .candidate
                    .contains_key(&format!("record:{}", ids[1]))
            );
        }
    }
}

#[test]
fn inspect_all_resolves_mixed_candidate_ids_and_tracks_only_delivered_details() {
    let (input, mut analysis, ids) = analysis_query_fixture();
    let mut coverage = Coverage::default();
    let mut query =
        json!({"kind":"all","ids":[ids[0],ids[3],"source"],"view":"index","offset":0,"limit":10});
    let index = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        true,
        "inspect_analysis",
        &query,
        16000,
    )
    .unwrap();
    assert_eq!(index["total"], 3);
    assert!(coverage.candidate.is_empty());
    query["view"] = json!("detail");
    query["limit"] = json!(1);
    let mut offset = 0;
    while offset < 3 {
        query["offset"] = json!(offset);
        let page = tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            true,
            "inspect_analysis",
            &query,
            16000,
        )
        .unwrap();
        offset = page["next"].as_u64().unwrap();
        assert_eq!(coverage.candidate.len(), offset as usize);
    }
    assert!(
        coverage
            .candidate
            .contains_key(&format!("record:{}", ids[0]))
    );
    assert!(
        coverage
            .candidate
            .contains_key(&format!("relation:{}", ids[3]))
    );
    assert!(coverage.candidate.contains_key("disposition:source"));
    let delivered = coverage.candidate.clone();
    query["offset"] = json!(0);
    query["ids"] = json!([ids[0], "missing"]);
    assert!(
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            true,
            "inspect_analysis",
            &query,
            16000
        )
        .is_err()
    );
    assert_eq!(
        coverage.candidate, delivered,
        "invalid mixed queries cannot partially receipt objects"
    );
    let mut collision = analysis.records[&ids[0]].clone();
    collision.id = "source".into();
    analysis.records.insert("source".into(), collision);
    query["ids"] = json!(["source"]);
    assert!(
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            true,
            "inspect_analysis",
            &query,
            16000
        )
        .unwrap_err()
        .contains("ambiguous")
    );
    assert_eq!(coverage.candidate, delivered);
    query["kind"] = json!("record");
    tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        true,
        "inspect_analysis",
        &query,
        16000,
    )
    .unwrap();
    assert!(coverage.candidate.contains_key("record:source"));
}

#[test]
fn inspect_analysis_category_errors_explain_reference_mapping_without_receipting_a_partial_batch() {
    let (input, mut analysis, ids) = analysis_query_fixture();
    let mut coverage = Coverage::default();
    for query_ids in [json!([ids[0], ids[3]]), json!(["disposition:source"])] {
        let error = tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            true,
            "inspect_analysis",
            &json!({"kind":"record","ids":query_ids,"offset":0,"limit":10}),
            16000,
        )
        .unwrap_err();
        assert!(error.contains("/ids"));
        assert!(error.contains("kind=all for mixed categories"));
        assert!(error.contains("kind=relation"));
        assert!(error.contains("kind=disposition"));
        assert!(error.contains("without the reference prefix"));
        assert!(coverage.candidate.is_empty());
    }
}

#[test]
fn inspect_analysis_filtered_relations_and_dispositions_keep_exact_review_identity() {
    let (input, mut analysis, ids) = analysis_query_fixture();
    let mut coverage = Coverage::default();
    let out = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        true,
        "inspect_analysis",
        &json!({"view":"detail","kind":"relation","source_id":"other-source","ids":[ids[3]],"offset":0,"limit":1}),
        16000,
    )
    .unwrap();
    assert_eq!(
        out["items"][0]["id"], ids[3],
        "endpoint provenance must locate cross-source links"
    );
    assert_eq!(coverage.candidate.len(), 1);
    assert!(
        coverage
            .candidate
            .contains_key(&format!("relation:{}", ids[3]))
    );
    let out = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        true,
        "inspect_analysis",
        &json!({"view":"detail","kind":"disposition","ids":["other-source"],"offset":0,"limit":1}),
        16000,
    )
    .unwrap();
    assert_eq!(out["items"][0]["source_id"], "other-source");
    assert!(coverage.candidate.contains_key("disposition:other-source"));
    assert!(!coverage.candidate.contains_key("disposition:source"));
    assert!(
        tools::review_gaps(&input, &analysis, &coverage)
            .iter()
            .any(|g| g["kind"] == "unreviewed_candidate")
    );
}

#[test]
fn inspect_analysis_budgeted_pages_preserve_records_and_only_receipt_returned_items() {
    let (input, analysis, _) = analysis_query_fixture();
    for reviewer in [false, true] {
        let mut analysis = analysis.clone();
        let mut coverage = Coverage::default();
        let expected = tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            "inspect_analysis",
            &json!({"view":"detail","kind":"all","offset":0,"limit":100}),
            16000,
        )
        .unwrap();
        let records = expected["items"].as_array().unwrap();
        coverage = Coverage::default();
        assert!(records.len() > 1);
        let budget = records
            .iter()
            .enumerate()
            .map(|(index, record)| {
                serde_json::to_vec(
                    &json!({"view":"detail","total":records.len(),"next":index+1,"items":[record]}),
                )
                .unwrap()
                .len()
            })
            .max()
            .unwrap();
        let mut offset = 0;
        let mut actual = Vec::new();
        let mut pages = 0;
        while offset < records.len() {
            let page = tools::invoke(
                &input,
                &mut analysis,
                &mut coverage,
                reviewer,
                "inspect_analysis",
                &json!({"view":"detail","kind":"all","offset":offset,"limit":100}),
                budget,
            )
            .unwrap();
            assert!(serde_json::to_vec(&page).unwrap().len() <= budget);
            assert_eq!(page["total"], records.len());
            let next = page["next"].as_u64().unwrap() as usize;
            assert!(next > offset);
            actual.extend(page["items"].as_array().unwrap().iter().cloned());
            assert_eq!(coverage.candidate.len(), actual.len());
            offset = next;
            pages += 1;
        }
        assert!(
            pages > 1,
            "fixture must exercise a page too large for one response"
        );
        assert_eq!(
            &actual, records,
            "every record must survive pagination intact and in order"
        );
    }
}

#[test]
fn candidate_detail_receipts_are_role_local_and_invalidated_by_edits() {
    let (input, mut analysis, ids) = analysis_query_fixture();
    let query = json!({"kind":"all","ids":[ids[0]],"offset":0,"limit":1});
    let mut index_query = query.clone();
    index_query["view"] = json!("index");
    for reviewer in [false, true] {
        let mut coverage = Coverage::default();
        let index = tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            reviewer,
            "inspect_analysis",
            &index_query,
            16000,
        )
        .unwrap();
        assert_eq!(index["items"][0]["detail_received"], false);
        assert!(coverage.candidate.is_empty());
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            reviewer,
            "inspect_analysis",
            &query,
            16000,
        )
        .unwrap();
        let restored: Coverage = serde_json::from_value(json!(coverage)).unwrap();
        assert_eq!(restored.candidate.len(), 1);
        let index = tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            reviewer,
            "inspect_analysis",
            &index_query,
            16000,
        )
        .unwrap();
        assert_eq!(index["items"][0]["detail_received"], true);
        assert_eq!(coverage.candidate, restored.candidate);
        let other_role_index = tools::invoke(
            &input,
            &mut analysis,
            &mut Coverage::default(),
            !reviewer,
            "inspect_analysis",
            &index_query,
            16000,
        )
        .unwrap();
        assert_eq!(other_role_index["items"][0]["detail_received"], false);
        let RecordData::Fact { value, .. } = &mut analysis.records.get_mut(&ids[0]).unwrap().data
        else {
            panic!("fixture fact")
        };
        value.push_str(" changed");
        let index = tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            reviewer,
            "inspect_analysis",
            &index_query,
            16000,
        )
        .unwrap();
        assert_eq!(index["items"][0]["detail_received"], false);
        assert_eq!(
            coverage.candidate, restored.candidate,
            "index cannot refresh a stale receipt"
        );
    }
}

#[tokio::test]
async fn candidate_receipt_is_not_reported_received_before_model_delivery() {
    struct DetailThenIndex(String);
    #[async_trait]
    impl Model for DetailThenIndex {
        async fn turn(&self, _: &Config, _: &[u8]) -> Result<ChatTurn, AgentError> {
            Ok(ChatTurn {
                usage: None,
                content: String::new(),
                finish_reason: "tool_calls".into(),
                tool_calls: ["detail", "index"]
                    .into_iter()
                    .map(|view| ChatToolCall {
                        id: format!("inspect-{view}"),
                        name: "inspect_analysis".into(),
                        arguments:
                            json!({"kind":"all","ids":[self.0],"view":view,"offset":0,"limit":1})
                                .to_string(),
                    })
                    .collect(),
            })
        }
    }
    let (input, analysis, ids) = analysis_query_fixture();
    for role in [Role::Main, Role::Reviewer] {
        let journal = MemoryJournal::default();
        *journal.interrupt_after.lock().unwrap() = Some(1);
        agent::run(
            &input,
            &config(),
            &journal,
            &work_script(vec![("set_work_note", active_work("source"))]),
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        let mut state = journal.load().await.unwrap().unwrap();
        state.analysis = analysis.clone();
        state.role = role.clone();
        if role == Role::Reviewer {
            state.source_review =
                Some(agent::source_review::initialize(&input, &config()).unwrap());
            agent::source_review::select_next(&input, &config(), &mut state).unwrap();
            state.reviewer_work = Some(serde_json::from_value(active_work("source")).unwrap());
        }
        *journal.state.lock().unwrap() = Some(state);
        *journal.interrupt_after.lock().unwrap() = Some(2);
        agent::run(
            &input,
            &config(),
            &journal,
            &DetailThenIndex(ids[0].clone()),
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        let saved = journal.load().await.unwrap().unwrap();
        assert!(
            if role == Role::Main {
                &saved.analysis.coverage
            } else {
                &saved.reviewer_coverage
            }
            .candidate
            .is_empty()
        );
        assert_eq!(saved.pending_coverage.as_ref().unwrap().candidate.len(), 1);
        let index: Value = serde_json::from_str(
            saved.transcript.last().unwrap()["content"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(index["result"]["items"][0]["detail_received"], false);
        let restored: Checkpoint = serde_json::from_value(json!(saved)).unwrap();
        *journal.state.lock().unwrap() = Some(restored);
        *journal.interrupt_after.lock().unwrap() = Some(3);
        agent::run(
            &input,
            &config(),
            &journal,
            &work_script(vec![(
                "inspect_analysis",
                json!({"kind":"all","ids":[ids[0]],"view":"index","offset":0,"limit":1}),
            )]),
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        let received = journal.load().await.unwrap().unwrap();
        assert_eq!(
            if role == Role::Main {
                &received.analysis.coverage
            } else {
                &received.reviewer_coverage
            }
            .candidate
            .len(),
            1
        );
        let index: Value = serde_json::from_str(
            received.transcript.last().unwrap()["content"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(index["result"]["items"][0]["detail_received"], true);
        let other = if role == Role::Main {
            &received.reviewer_coverage
        } else {
            &received.analysis.coverage
        };
        assert!(other.candidate.is_empty());
    }
}

#[test]
fn candidate_index_is_navigation_and_never_reviews_complete_objects() {
    let (input, mut analysis, ids) = analysis_query_fixture();
    let mut coverage = Coverage::default();
    for kind in ["all", "relation", "disposition"] {
        let index = tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            true,
            "inspect_analysis",
            &json!({"kind":kind,"offset":0,"limit":100}),
            16000,
        )
        .unwrap();
        assert_eq!(index["view"], "index");
        assert!(index["total"].as_u64().unwrap() > 0);
        assert!(coverage.candidate.is_empty());
        for row in index["items"].as_array().unwrap() {
            assert!(row["ref"].is_string());
            assert!(row.get("data").is_none());
            assert!(row.get("grounds").is_none());
            assert!(row.get("disposition").is_none());
        }
    }
    let query = json!({"kind":"all","ids":[ids[0]],"offset":0,"limit":1});
    let detail = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        true,
        "inspect_analysis",
        &query,
        16000,
    )
    .unwrap();
    assert_eq!(detail["view"], "detail");
    assert_eq!(detail["items"][0], json!(analysis.records[&ids[0]]));
    assert_eq!(coverage.candidate.len(), 1);
    let prior = coverage.candidate.clone();
    let mut indexed = query;
    indexed["view"] = json!("index");
    tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        true,
        "inspect_analysis",
        &indexed,
        16000,
    )
    .unwrap();
    assert_eq!(coverage.candidate, prior);
    assert!(
        tools::review_gaps(&input, &analysis, &coverage)
            .iter()
            .any(|g| g["kind"] == "unreviewed_candidate")
    );
}

#[test]
fn candidate_index_pages_fit_without_returning_full_candidate_content() {
    let (input, mut analysis, ids) = analysis_query_fixture();
    let record = analysis.records.get_mut(&ids[0]).unwrap();
    let RecordData::Fact { value, .. } = &mut record.data else {
        panic!("fixture fact")
    };
    *value = "large candidate body".repeat(1000);
    let mut coverage = Coverage::default();
    let full = json!({"kind":"all","ids":[ids[0]],"offset":0,"limit":1});
    assert!(
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            true,
            "inspect_analysis",
            &full,
            4096
        )
        .is_err()
    );
    let mut query = full.clone();
    query["view"] = json!("index");
    let index = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        true,
        "inspect_analysis",
        &query,
        4096,
    )
    .unwrap();
    assert_eq!(index["items"][0]["id"], ids[0]);
    assert!(coverage.candidate.is_empty());
    // The envelope itself participates in the byte cap; pagination must
    // neither drop nor duplicate an ID, including the last short page.
    let all = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        true,
        "inspect_analysis",
        &json!({"kind":"all","offset":0,"limit":100}),
        16000,
    )
    .unwrap();
    let budget = all["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            serde_json::to_vec(
                &json!({"view":"index","total":all["total"],"next":all["total"],"items":[row]}),
            )
            .unwrap()
            .len()
        })
        .max()
        .unwrap();
    let mut offset = 0;
    let mut found = std::collections::BTreeSet::new();
    loop {
        let page = tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            true,
            "inspect_analysis",
            &json!({"kind":"all","offset":offset,"limit":100}),
            budget,
        )
        .unwrap();
        assert!(serde_json::to_vec(&page).unwrap().len() <= budget);
        for row in page["items"].as_array().unwrap() {
            assert!(found.insert(row["ref"].as_str().unwrap().to_owned()));
        }
        let next = page["next"].as_u64().unwrap();
        assert!(next > offset);
        if next == page["total"].as_u64().unwrap() {
            break;
        }
        offset = next;
    }
    assert_eq!(found.len(), all["total"].as_u64().unwrap() as usize);
    assert!(found.iter().any(|key| key.starts_with("relation:")));
    assert!(found.iter().any(|key| key.starts_with("disposition:")));
    assert!(coverage.candidate.is_empty());
}

#[test]
fn inspect_analysis_rejects_invalid_selectors_and_oversize_without_partial_coverage() {
    let (input, mut analysis, ids) = analysis_query_fixture();
    let mut coverage = Coverage::default();
    for extra in [
        json!({"ids":[]}),
        json!({"ids":[ids[0],ids[0]]}),
        json!({"ids":[ids[0],"missing"]}),
        json!({"kind":"record","ids":[ids[3]]}),
        json!({"ids":null}),
        json!({"source_id":"missing"}),
        json!({"source_id":null}),
        json!({"view":null}),
        json!({"view":"summary"}),
    ] {
        let mut query = json!({"view":"detail","kind":"all","offset":0,"limit":10});
        query
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let before = digest(&analysis).unwrap();
        assert!(
            tools::invoke(
                &input,
                &mut analysis,
                &mut coverage,
                true,
                "inspect_analysis",
                &query,
                16000
            )
            .is_err(),
            "{query}"
        );
        assert!(coverage.candidate.is_empty());
        assert_eq!(before, digest(&analysis).unwrap());
    }
    let query = json!({"view":"detail","kind":"all","offset":0,"limit":1,"ids":[ids[0]]});
    assert!(
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            true,
            "inspect_analysis",
            &query,
            64
        )
        .is_err()
    );
    assert!(coverage.candidate.is_empty());
}

#[test]
fn requirement_disposition_cannot_be_satisfied_by_an_unrelated_fact() {
    let input = input();
    let mut analysis = read_analysis(&input);
    let mut coverage = analysis.coverage.clone();
    tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "put_record",
        &json!({"id":null,"sources":[span()],
        "data":{"kind":"fact","name":"附件名称","value":"甲","scope":"本标"}}),
        16000,
    )
    .unwrap();
    tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "set_disposition",
        &json!({"source_id":"source",
        "state":"requirement","reason":"有事实记录不等于提取了提交义务"}),
        16000,
    )
    .unwrap();
    assert!(
        tools::gaps(&input, &analysis)
            .iter()
            .any(|gap| gap["kind"] == "disposition_without_record")
    );
}

#[derive(Default)]
struct MemoryJournal {
    sdk_turns: Mutex<Vec<usize>>,
    fail_boundary_ack: Mutex<Option<usize>>,
    cancel_boundary: Mutex<Option<(usize, CancellationToken)>>,
    reject_reservation: Mutex<bool>,
    state: Mutex<Option<Checkpoint>>,
    reservations: Mutex<BTreeMap<usize, (Vec<u8>, usize)>>,
    interrupt_after: Mutex<Option<usize>>,
    view: Mutex<Option<views::SourceView>>,
    view_calls: Mutex<usize>,
}
#[async_trait]
impl Journal for MemoryJournal {
    async fn source_view(
        &self,
        _: &str,
        _: &Limits,
        _: &CancellationToken,
    ) -> Result<views::SourceView, AgentError> {
        *self.view_calls.lock().unwrap() += 1;
        self.view
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| AgentError::new("SOURCE_VIEW_UNAVAILABLE", "injected unavailable view"))
    }
    async fn load(&self) -> Result<Option<Checkpoint>, AgentError> {
        Ok(self.state.lock().unwrap().clone())
    }
    async fn reserve(&self, state: &Checkpoint, body: &[u8]) -> Result<Option<usize>, AgentError> {
        if *self.reject_reservation.lock().unwrap() {
            return Err(crate::agent_error::AgentError::new(
                "INTERNAL",
                "reservation transaction rejected",
            ));
        }
        self.sdk_turns
            .lock()
            .unwrap()
            .push(state.journal.session.as_ref().unwrap().turn());
        let mut rows = self.reservations.lock().unwrap();
        let row = rows.entry(state.turn).or_insert_with(|| (body.to_vec(), 0));
        assert_eq!(
            row.0, body,
            "same boundary must preserve exact provider body"
        );
        if row.1 == 3 {
            return Ok(None);
        }
        row.1 += 1;
        *self.state.lock().unwrap() = Some(state.clone());
        if let Some((sequence, token)) = &*self.cancel_boundary.lock().unwrap()
            && *sequence == state.journal.sequence
        {
            token.cancel();
        }
        let mut boundary = self.fail_boundary_ack.lock().unwrap();
        if *boundary == Some(state.journal.sequence) {
            *boundary = None;
            return Err(crate::agent_error::AgentError::new(
                "INTERNAL",
                "lost boundary acknowledgement",
            ));
        }
        Ok(Some(row.1))
    }
    async fn save(&self, state: &Checkpoint, _: &Value) -> Result<(), AgentError> {
        *self.state.lock().unwrap() = Some(state.clone());
        if let Some((sequence, token)) = &*self.cancel_boundary.lock().unwrap()
            && *sequence == state.journal.sequence
        {
            token.cancel();
        }
        let mut boundary = self.fail_boundary_ack.lock().unwrap();
        if *boundary == Some(state.journal.sequence) {
            *boundary = None;
            return Err(crate::agent_error::AgentError::new(
                "INTERNAL",
                "lost boundary acknowledgement",
            ));
        }
        let mut interrupt = self.interrupt_after.lock().unwrap();
        if *interrupt == Some(state.turn) && state.journal.pending.is_none() {
            *interrupt = None;
            return Err(AgentError::new(
                "INTERNAL",
                "simulated lost checkpoint acknowledgement",
            ));
        }
        Ok(())
    }
}

fn test_view() -> views::SourceView {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use sha2::{Digest, Sha256};
    let image = image::RgbImage::from_pixel(8, 8, image::Rgb([255, 255, 255]));
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new(&mut bytes)
        .encode_image(&image)
        .unwrap();
    views::SourceView {
        identity: views::ViewIdentity {
            source_id: "source".into(),
            original_sha256: "a".repeat(64),
            image_sha256: hex::encode(Sha256::digest(&bytes)),
            page_ordinal: 0,
            width: 8,
            height: 8,
            renderer: "docreader-source-view-v1/test-fixture".into(),
        },
        jpeg_base64: STANDARD.encode(bytes),
    }
}

#[tokio::test]
async fn requested_original_evidence_takes_priority_over_optional_assigned_candidate_recall() {
    let mut config = config();
    let journal = fresh_review_journal_config(&config).await;
    let mut state = journal.load().await.unwrap().unwrap();
    state.transcript.clear();
    for index in 0..6 {
        let id = format!("saved-{index}");
        let record = Record {
            id: id.clone(),
            sources: vec![span()],
            data: RecordData::Fact {
                name: "saved fact".into(),
                value: "already delivered content ".repeat(40),
                scope: "source".into(),
            },
        };
        let version = digest(&record).unwrap();
        state.analysis.records.insert(id.clone(), record);
        state
            .reviewer_coverage
            .candidate
            .insert(format!("record:{id}"), version);
    }
    state.reviewer_work.as_mut().unwrap().focus.references = vec!["record:saved-0".into()];
    let text = input().source_units[0].text.clone();
    let original =
        json!({"ok":true,"result":{"source_id":"source","start":0,"end":text.len(),"text":text}});
    state.transcript = vec![
        json!({"role":"assistant","tool_calls":[{"id":"read","type":"function","function":{"name":"read_source","arguments":"{}"}}]}),
        json!({"role":"tool","tool_call_id":"read","content":original.to_string()}),
    ];
    let mut pending = state.reviewer_coverage.clone();
    tools::cover(
        pending.text.entry("source".into()).or_default(),
        0,
        text.len(),
    );
    state.pending_coverage = Some(pending);
    let checkpoint = json!(state);
    let full = agent::request(&input(), &config, &mut state).await.unwrap();
    let recalled = |bytes: &[u8]| {
        let body: Value = serde_json::from_slice(bytes).unwrap();
        body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|message| {
                let content: Value = serde_json::from_str(message["content"].as_str()?).ok()?;
                content["retained_candidate_details"]["items"]
                    .as_array()
                    .cloned()
            })
            .flatten()
            .collect::<Vec<_>>()
    };
    let full_count = recalled(&full).len();
    assert!(full_count > 1);
    config.limits.max_context_bytes = full.len() - 1;
    let bounded = agent::request(&input(), &config, &mut state).await.unwrap();
    assert!(bounded.len() <= config.limits.max_context_bytes);
    let items = recalled(&bounded);
    assert!(items.len() > 1 && items.len() < full_count);
    assert_eq!(items[0]["reference"], "record:saved-0");
    let body: Value = serde_json::from_slice(&bounded).unwrap();
    let original_content = original.to_string();
    assert!(
        body["messages"].as_array().unwrap().iter().any(|message| {
            message["tool_call_id"] == "read"
                && message["content"].as_str() == Some(original_content.as_str())
        }),
        "new original result must be delivered verbatim"
    );
    assert_eq!(
        json!(state),
        checkpoint,
        "context selection cannot commit pending reads or alter counters"
    );
    state.reviewer_coverage = state.pending_coverage.clone().unwrap();
    let reread_checkpoint = json!(state);
    let reread = agent::request(&input(), &config, &mut state).await.unwrap();
    assert_eq!(recalled(&reread), items);
    assert_eq!(json!(state), reread_checkpoint);

    // Exact candidate results also take precedence over optional recall.
    // The cache must not make the very tool for fetching evidence unavailable.
    let result =
        json!({"ok":true,"result":{"view":"detail","items":[state.analysis.records["saved-5"]]}})
            .to_string();
    state.transcript[0]["tool_calls"][0]["function"]["name"] = json!("inspect_analysis");
    state.transcript[1]["content"] = json!(result);
    let candidate_checkpoint = json!(state);
    let lookup = agent::request(&input(), &config, &mut state).await.unwrap();
    assert!(recalled(&lookup).len() > 1);
    let body: Value = serde_json::from_slice(&lookup).unwrap();
    assert!(body["messages"].as_array().unwrap().iter().any(|m| {
        m["tool_call_id"] == "read" && m["content"].as_str() == Some(result.as_str())
    }));
    assert_eq!(json!(state), candidate_checkpoint);

    let mut mandatory: Checkpoint = serde_json::from_value(checkpoint.clone()).unwrap();
    mandatory.reviewer_work.as_mut().unwrap().focus.references =
        (0..6).map(|i| format!("record:saved-{i}")).collect();
    assert!(
        agent::request(&input(), &config, &mut mandatory)
            .await
            .is_err()
    );
    // A subsequent request can use the full cache again; exclusions are local
    // to request construction, not persisted reading or recovery state.
    let mut restored: Checkpoint = serde_json::from_value(checkpoint).unwrap();
    config.limits.max_context_bytes = full.len();
    let restored = agent::request(&input(), &config, &mut restored)
        .await
        .unwrap();
    assert_eq!(recalled(&restored).len(), full_count);
}

#[tokio::test]
async fn active_source_review_keeps_its_own_pixels_after_history_eviction_within_total_budget() {
    let mut config = config();
    config.limits.max_history_bytes = 128;
    let journal = fresh_review_journal_config(&config).await;
    let mut state = journal.load().await.unwrap().unwrap();
    let view = test_view();
    let id = view.id().unwrap();
    state.source_views.insert(id.clone(), view.clone());
    state
        .reviewer_coverage
        .views
        .insert(id.clone(), view.identity.clone());
    state
        .analysis
        .coverage
        .views
        .insert(id.clone(), view.identity.clone());
    state
        .reviewer_work
        .as_mut()
        .unwrap()
        .focus
        .source_spans
        .push(Span {
            source_id: view.identity.source_id.clone(),
            start: 0,
            end: 0,
            view_id: Some(id.clone()),
            grid_cell: None,
        });
    let assistant = |id: &str, name: &str| {
        json!({"role":"assistant","tool_calls":[
        {"id":id,"type":"function","function":{"name":name,"arguments":"{}"}}]})
    };
    let result =
        |id: &str| json!({"role":"tool","tool_call_id":id,"content":"{\"ok\":true,\"result\":{}}"});
    state.transcript = vec![
        assistant("view", "read_source_view"),
        result("view"),
        json!({"role":"user","source_view_refs":[id]}),
        assistant("latest", "inspect_analysis"),
        result("latest"),
    ];
    let before = state.clone();
    let body = agent::request(&input(), &config, &mut state).await.unwrap();
    let image_count = |bytes: &[u8]| {
        let body: Value = serde_json::from_slice(bytes).unwrap();
        body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|m| m["content"].as_array().into_iter().flatten())
            .filter(|item| item["type"] == "image_url")
            .count()
    };
    assert_eq!(
        image_count(&body),
        1,
        "active original must survive losing its old transcript group"
    );
    assert!(
        state
            .transcript
            .iter()
            .any(|m| m["tool_call_id"] == "latest")
    );
    assert_eq!(
        json!(state.reviewer_coverage),
        json!(before.reviewer_coverage)
    );
    assert_eq!(json!(state.source_views), json!(before.source_views));
    assert_eq!(
        json!(state.reviewer_progress),
        json!(before.reviewer_progress)
    );
    assert_eq!(state.read_bytes, before.read_bytes);
    let mut primary = before.clone();
    primary.role = Role::Main;
    primary.main_work = primary.reviewer_work.clone();
    primary.transcript.clear();
    primary.reviewer_coverage.views.clear();
    let primary_body = agent::request(&input(), &config, &mut primary)
        .await
        .unwrap();
    assert_eq!(
        image_count(&primary_body),
        1,
        "the main role also retains its own explicitly focused original"
    );
    let mut probe = before.clone();
    probe.transcript.pop(); // The latest candidate call has not returned yet.
    let pending_probe = probe.clone();
    let args = json!({"kind":"all","view":"detail","offset":0,"limit":1});
    let call = ChatToolCall {
        id: "latest".into(),
        name: "inspect_analysis".into(),
        arguments: args.to_string(),
    };
    let committed = probe.reviewer_coverage.clone();
    let page = agent::inspect_in_context(
        &input(),
        &config,
        &mut probe,
        &args,
        &[call],
        &[],
        &committed,
    )
    .await
    .unwrap();
    assert_eq!(
        page["items"].as_array().unwrap().len(),
        1,
        "moving the active pixels out of history must not falsely reject an exact candidate lookup"
    );
    assert_eq!(probe.transcript, pending_probe.transcript);
    assert_eq!(probe.read_bytes, pending_probe.read_bytes);
    assert_eq!(probe.turn, pending_probe.turn);
    let mut cache_only = pending_probe;
    cache_only.reviewer_coverage.views.clear();
    let before_failed_probe = cache_only.clone();
    let call = ChatToolCall {
        id: "latest".into(),
        name: "inspect_analysis".into(),
        arguments: args.to_string(),
    };
    let committed = cache_only.reviewer_coverage.clone();
    assert!(
        agent::inspect_in_context(
            &input(),
            &config,
            &mut cache_only,
            &args,
            &[call],
            &[],
            &committed
        )
        .await
        .unwrap_err()
        .contains("cannot fit together"),
        "cached pixels absent from the final request cannot satisfy retention"
    );
    assert_eq!(
        json!(cache_only),
        json!(before_failed_probe),
        "a failed sizing probe must restore all tentative state"
    );
    let mut unseen = state.clone();
    unseen.reviewer_coverage.views.clear();
    let without_image = agent::request(&input(), &config, &mut unseen)
        .await
        .unwrap();
    assert_eq!(
        image_count(&without_image),
        0,
        "the primary receipt and shared cache cannot grant independent reading"
    );
    let mut cross_input = input();
    let mut other_source = cross_input.source_units[0].clone();
    other_source.source_unit_revision_id = "other".into();
    other_source.ordinal += 1;
    cross_input.source_units.push(other_source);
    let mut cross = state.clone();
    cross.transcript.clear();
    cross.source_review = Some(agent::source_review::initialize(&cross_input, &config).unwrap());
    agent::source_review::select_next(&cross_input, &config, &mut cross).unwrap();
    cross
        .reviewer_work
        .as_mut()
        .unwrap()
        .source_scope
        .push("other".into());
    let mut other_view = view.clone();
    other_view.identity.source_id = "other".into();
    let other_id = other_view.id().unwrap();
    cross
        .reviewer_coverage
        .views
        .insert(other_id.clone(), other_view.identity.clone());
    cross.source_views.insert(other_id.clone(), other_view);
    cross
        .reviewer_work
        .as_mut()
        .unwrap()
        .focus
        .source_spans
        .push(Span {
            source_id: "other".into(),
            start: 0,
            end: 0,
            view_id: Some(other_id),
            grid_cell: None,
        });
    let coverage_before = json!(cross.reviewer_coverage);
    let both = agent::request(&cross_input, &config, &mut cross)
        .await
        .unwrap();
    assert_eq!(
        image_count(&both),
        2,
        "a cross-source original in the declared comparison scope must survive history eviction"
    );
    let cited_view = cross
        .reviewer_work
        .as_mut()
        .unwrap()
        .focus
        .source_spans
        .pop()
        .unwrap();
    cross.analysis.records.insert(
        "visual-claim".into(),
        Record {
            id: "visual-claim".into(),
            sources: vec![cited_view],
            data: RecordData::Fact {
                name: "visual claim".into(),
                value: "previously extracted value".into(),
                scope: "source".into(),
            },
        },
    );
    cross.reviewer_work.as_mut().unwrap().focus.references = vec!["record:visual-claim".into()];
    let implicit = agent::request(&cross_input, &config, &mut cross)
        .await
        .unwrap();
    assert_eq!(
        image_count(&implicit),
        1,
        "candidate citations alone must not pin every previously visited image; select a visual comparison explicitly"
    );
    cross
        .reviewer_work
        .as_mut()
        .unwrap()
        .source_scope
        .retain(|id| id != "other");
    cross
        .reviewer_work
        .as_mut()
        .unwrap()
        .focus
        .source_spans
        .retain(|span| span.source_id != "other");
    cross
        .reviewer_work
        .as_mut()
        .unwrap()
        .focus
        .references
        .clear();
    let one = agent::request(&cross_input, &config, &mut cross)
        .await
        .unwrap();
    assert_eq!(
        image_count(&one),
        1,
        "leaving the scope releases its pixels without erasing prior receipts"
    );
    assert_eq!(json!(cross.reviewer_coverage), coverage_before);
    config.limits.max_context_bytes = without_image.len() + view.jpeg_base64.len() / 2;
    assert!(
        agent::request(&input(), &config, &mut state).await.is_err(),
        "working images still obey the total ceiling; do not silently omit the active original"
    );
}

#[tokio::test]
async fn original_pixels_are_independently_viewed_and_survive_resume_without_rerender() {
    let journal = MemoryJournal::default();
    *journal.view.lock().unwrap() = Some(test_view());
    *journal.interrupt_after.lock().unwrap() = Some(2);
    let model = script();
    {
        let mut calls = model.calls.lock().unwrap();
        calls.insert(
            1,
            ("read_source_view".into(), json!({"source_id":"source"})),
        );
        // Both independent review rounds must actually receive the same pixels.
        for i in (0..calls.len()).rev().collect::<Vec<_>>() {
            if calls[i].0 == "request_review" {
                calls.insert(
                    i + 2,
                    ("read_source_view".into(), json!({"source_id":"source"})),
                );
            }
        }
    }
    assert_eq!(
        agent::run(
            &input(),
            &config(),
            &journal,
            &model,
            &CancellationToken::new()
        )
        .await
        .unwrap_err()
        .code,
        "INTERNAL"
    );
    let result = agent::run(
        &input(),
        &config(),
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(result.quality, "verified");
    assert_eq!(
        *journal.view_calls.lock().unwrap(),
        1,
        "resume/reviewer must reuse frozen pixels"
    );
    assert_eq!(result.source_views.len(), 1);
    assert_eq!(result.review.coverage.views, result.analysis.coverage.views);
    let bodies = model.bodies.lock().unwrap();
    assert!(
        bodies
            .iter()
            .filter(|b| b["messages"][0]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("INDEPENDENT"))
            .any(|b| b["messages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|m| m["content"]
                    .as_array()
                    .is_some_and(|parts| parts.iter().any(|p| p["type"] == "image_url"))))
    );
    let id = result.source_views.keys().next().unwrap().clone();
    let mut span = Span {
        source_id: "source".into(),
        start: 0,
        end: 0,
        view_id: Some(id),
        grid_cell: None,
    };
    assert!(tools::validate_span(&input(), &result.review.coverage, &span).is_ok());
    assert!(tools::validate_span(&input(), &Coverage::default(), &span).is_err());
    span.end = 3;
    assert!(tools::validate_span(&input(), &result.review.coverage, &span).is_err());
}

#[tokio::test]
async fn a_failed_original_view_cannot_be_published_as_verified() {
    let journal = MemoryJournal::default();
    let model = script();
    model.calls.lock().unwrap().insert(
        1,
        ("read_source_view".into(), json!({"source_id":"source"})),
    );
    let result = agent::run(
        &input(),
        &config(),
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(result.quality, "needs_review");
    assert!(!result.analysis.coverage.view_failures.is_empty());
    assert!(result.source_views.is_empty());
}

#[tokio::test]
async fn invalid_or_oversized_pixels_never_establish_visual_coverage() {
    for oversize in [false, true] {
        let journal = MemoryJournal::default();
        let model = script();
        let mut config = config();
        let mut view = test_view();
        if oversize {
            config.limits.max_source_view_bytes = 1;
        } else {
            view.identity.image_sha256 = "0".repeat(64);
        }
        *journal.view.lock().unwrap() = Some(view);
        model.calls.lock().unwrap().insert(
            1,
            ("read_source_view".into(), json!({"source_id":"source"})),
        );
        let result = agent::run(
            &input(),
            &config,
            &journal,
            &model,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(result.quality, "needs_review");
        assert!(result.analysis.coverage.views.is_empty());
        assert!(result.source_views.is_empty());
        assert!(model.bodies.lock().unwrap().iter().all(|b| {
            !b["messages"]
                .to_string()
                .contains("data:image/jpeg;base64,")
        }));
    }
}

#[test]
fn primary_visual_coverage_is_not_reviewer_visual_coverage() {
    let input = input();
    let mut analysis = read_analysis(&input);
    let view = test_view();
    analysis
        .coverage
        .views
        .insert(view.id().unwrap(), view.identity);
    let mut reviewer = analysis.coverage.clone();
    reviewer.views.clear();
    assert!(tools::reading_gaps(&input, &reviewer).is_empty());
    assert!(
        tools::review_gaps(&input, &analysis, &reviewer)
            .iter()
            .any(|g| g["kind"] == "unreviewed_source_view")
    );
}
#[tokio::test]
async fn oversized_pending_image_batch_resumes_with_only_delivered_visual_coverage() {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use sha2::{Digest, Sha256};
    let journal = MemoryJournal::default();
    agent::run(
        &input(),
        &config(),
        &journal,
        &script(),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    let mut state = journal.state.lock().unwrap().clone().unwrap();
    state.role = Role::Reviewer;
    state.done = false;
    state.transcript.clear();
    state.pending_coverage = Some(state.reviewer_coverage.clone());
    let image = image::RgbImage::from_fn(160, 160, |x, y| {
        image::Rgb([
            (x.wrapping_mul(37) ^ y.wrapping_mul(71)) as u8,
            (x.wrapping_mul(17) ^ y.wrapping_mul(31)) as u8,
            (x ^ y) as u8,
        ])
    });
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 90)
        .encode_image(&image)
        .unwrap();
    let mut calls = vec![];
    let mut results = vec![];
    let mut refs = vec![];
    for i in 0..12 {
        let mut view = test_view();
        view.identity.source_id = format!("image-source-{i}");
        view.identity.page_ordinal = i;
        view.identity.width = 160;
        view.identity.height = 160;
        view.identity.image_sha256 = hex::encode(Sha256::digest(&bytes));
        view.jpeg_base64 = STANDARD.encode(&bytes);
        view.validate(&view.identity.source_id, 160, bytes.len())
            .unwrap();
        let id = view.id().unwrap();
        calls.push(
            json!({"id":format!("view-{i}"),"type":"function","function":{"name":"read_source_view",
            "arguments":json!({"source_id":view.identity.source_id}).to_string()}}),
        );
        results.push(
            json!({"role":"tool","tool_call_id":format!("view-{i}"),"content":json!({"ok":true,
            "result":{"view_id":id,"identity":view.identity}}).to_string()}),
        );
        state
            .pending_coverage
            .as_mut()
            .unwrap()
            .views
            .insert(id.clone(), view.identity.clone());
        state
            .analysis
            .coverage
            .views
            .insert(id.clone(), view.identity.clone());
        state.source_views.insert(id.clone(), view);
        refs.push(id);
    }
    state
        .transcript
        .push(json!({"role":"assistant","content":null,"tool_calls":calls}));
    state.transcript.extend(results);
    state
        .transcript
        .push(json!({"role":"user","source_view_refs":refs}));
    let before = state.clone();
    // Derive a partial-admission budget from the actual prompt and image
    // payloads. A fixed byte/token cap eventually fits no image as tools grow.
    let mut sizing_config = config();
    sizing_config.limits.max_context_bytes = 2_000_000;
    let mut sizing_state = before.clone();
    let mut sized: Value = serde_json::from_slice(
        &agent::request(&input(), &sizing_config, &mut sizing_state)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        sizing_state.pending_coverage.as_ref().unwrap().views.len(),
        refs.len()
    );
    let mut images = 0;
    sized["messages"].as_array_mut().unwrap().retain(|message| {
        let has_image = message["content"]
            .as_array()
            .is_some_and(|parts| parts.iter().any(|part| part["type"] == "image_url"));
        if has_image {
            images += 1;
        }
        !has_image || images <= refs.len() / 2
    });
    let byte_budget = serde_json::to_vec(&sized).unwrap().len();
    let token_budget = crate::agent_runtime::chat::estimate_input_tokens(
        &sized,
        sizing_config.limits.image_token_reserve,
        sizing_config.limits.token_safety_margin,
    )
    .unwrap()
        + sizing_config.provider.max_tokens as usize;
    for token_limited in [false, true] {
        let mut state = before.clone();
        let mut config = config();
        if token_limited {
            config.limits.max_context_tokens = token_budget;
            config.limits.max_context_bytes = sizing_config.limits.max_context_bytes;
        } else {
            config.limits.max_context_bytes = byte_budget;
        }
        let body = agent::request(&input(), &config, &mut state).await.unwrap();
        assert!(body.len() <= config.limits.max_context_bytes);
        let pending = state.pending_coverage.as_ref().unwrap();
        assert!(pending.views.len() < refs.len());
        assert!(!pending.views.is_empty());
        assert!(
            state.reviewer_coverage.views.is_empty(),
            "preparing a request is not delivery"
        );
        assert_eq!(
            state.analysis.coverage.views,
            before.analysis.coverage.views
        );
        assert_eq!(
            digest(&state.source_views).unwrap(),
            digest(&before.source_views).unwrap()
        );
        assert_eq!(state.read_bytes, before.read_bytes);
        assert_eq!(state.tool_calls, before.tool_calls);
        let request: Value = serde_json::from_slice(&body).unwrap();
        let messages = request["messages"].as_array().unwrap();
        let images = messages
            .iter()
            .filter(|m| {
                m["content"]
                    .as_array()
                    .is_some_and(|parts| parts.iter().any(|p| p["type"] == "image_url"))
            })
            .count();
        assert_eq!(images, pending.views.len());
        let errors = messages
            .iter()
            .filter(|m| m["role"] == "tool")
            .filter(|m| {
                let output: Value = serde_json::from_str(m["content"].as_str().unwrap()).unwrap();
                output["ok"] == false && output["error"].as_str().unwrap().contains("NOT delivered")
            })
            .count();
        assert_eq!(errors + images, refs.len());
        assert!(
            tools::review_gaps(&input(), &state.analysis, &state.reviewer_coverage)
                .iter()
                .any(|g| g["kind"] == "unreviewed_source_view")
        );
        // Resume must reserve identical provider bytes without resetting budgets.
        let mut replay: Checkpoint =
            serde_json::from_value(serde_json::to_value(&before).unwrap()).unwrap();
        assert_eq!(
            body,
            agent::request(&input(), &config, &mut replay)
                .await
                .unwrap()
        );
        assert_eq!(digest(&state).unwrap(), digest(&replay).unwrap());
    }
}

struct Script {
    calls: Mutex<VecDeque<(String, Value)>>,
    bodies: Mutex<Vec<Value>>,
}
#[async_trait]
impl Model for Script {
    async fn turn(&self, _: &Config, body: &[u8]) -> Result<ChatTurn, AgentError> {
        let body: Value = serde_json::from_slice(body).unwrap();
        self.bodies.lock().unwrap().push(body.clone());
        let (name, mut args) = self
            .calls
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected model call");
        if name == "delete_review_finding" && args["id"] == "$fixture_finding" {
            args["id"] = body["messages"]
                .as_array()
                .unwrap()
                .iter()
                .rev()
                .find_map(|message| {
                    let result: Value = serde_json::from_str(message["content"].as_str()?).ok()?;
                    result["result"]["items"]
                        .as_array()?
                        .first()?
                        .get("id")
                        .cloned()
                })
                .expect("fixture explicitly retrieved its finding");
        }
        let tools = fixture_review_calls(&body, &name, &args);
        Ok(ChatTurn {
            usage: None,
            content: String::new(),
            finish_reason: "tool_calls".into(),
            tool_calls: tools.unwrap_or_else(|| {
                vec![ChatToolCall {
                    id: format!("call-{}", self.calls.lock().unwrap().len()),
                    name,
                    arguments: args.to_string(),
                }]
            }),
        })
    }
}

/// Scripted semantic assertions for synthetic fixtures, using the real task
/// identity from the request. This does not run the completion reducer or
/// manufacture durable receipts; every assertion still traverses the tools.
fn fixture_review_calls(body: &Value, name: &str, args: &Value) -> Option<Vec<ChatToolCall>> {
    if name != "put_source_review" || !(args == &json!({}) || args.get("fixture_status").is_some())
    {
        return None;
    }
    let packet: Value =
        serde_json::from_str(body["messages"].as_array()?.last()?["content"].as_str()?).ok()?;
    let current = &packet["source_review"]["current"];
    let task = &current["task"];
    let source = task["source_id"].as_str()?;
    let region = &task["region"];
    if region["kind"] != "text" {
        return None;
    }
    let sources = json!([{"source_id":source,"start":region["start"],"end":region["end"]}]);
    let mut calls = Vec::new();
    let mut emit = |name: &str, args: Value| {
        calls.push(ChatToolCall {
            id: format!("fixture-{}", calls.len()),
            name: name.into(),
            arguments: args.to_string(),
        })
    };
    for reference in current["pending_candidate_refs"]["items"].as_array()? {
        emit(
            "complete_review_check",
            json!({"reference":reference,"summary":"Synthetic fixture comparison against the cited original.","sources":sources}),
        );
    }
    let mut finding_ids = vec![];
    if args["fixture_status"] == "findings" {
        for message in body["messages"].as_array()? {
            let Some(content) = message["content"].as_str() else {
                continue;
            };
            let Ok(value) = serde_json::from_str::<Value>(content) else {
                continue;
            };
            if value["result"]["saved"] == true
                && let Some(id) = value["result"]["id"].as_str()
            {
                finding_ids.push(id.to_owned());
            }
        }
    }
    let boundary = json!({"state":"complete","reason":"The synthetic paragraph is complete at this boundary.","sources":sources});
    let template_mappings = if args["fixture_template_mappings"] == true {
        current["templates_requiring_mapping_judgment"]["items"].as_array()?.iter()
            .map(|id| json!({"template_id":id,"requirement_ids":[],"relation_ids":[],
                "finding_ids":[],"sources":sources,
                "reason":"Synthetic applicability-only fixture contains one standalone template and no separate requirement record."}))
            .collect::<Vec<_>>()
    } else {
        vec![]
    };
    let relationship_checks = current["records_requiring_relationship_judgment"]["items"]
        .as_array()?.iter().map(|id| {
            let unresolved = args["fixture_unresolved"] == true;
            json!({"record_id":id,"status":args.get("fixture_relationship_status").unwrap_or(&json!("not_required")),
                "related_record_ids":[],"relation_ids":[],"unresolved_record_ids":if unresolved {json!([id])} else {json!([])},
                "finding_ids":[],"reason":"Synthetic fixture: standalone bidder condition or documented unavailable evidence, without an external selected condition or continuation target.","sources":sources})
        }).collect::<Vec<_>>();
    emit(
        "put_source_review",
        json!({"task_id":task["id"],"expected_version":current["expected_version"],
        "status":if finding_ids.is_empty(){"checked"}else{"findings"},"summary":"Synthetic fixture source-to-result judgment, including omissions and boundaries.",
        "sources":sources,"candidate_refs":current["pending_candidate_refs"]["items"],"template_mappings":template_mappings,"relationship_checks":relationship_checks,"boundaries":{"before":boundary,"after":boundary},
        "finding_ids":finding_ids,"evidence_requests":[]}),
    );
    Some(calls)
}
pub(super) fn config() -> Config {
    // Test provider never makes network calls; production resolves this identity
    // from configuration and freezes it before execution.
    let provider:AuthoringRuntimeContractV1=serde_json::from_value(json!({"schema_version":1,"base_url":"https://llm.example/v1",
        "endpoint":"https://llm.example/v1/chat/completions","protocol":"openai_chat_completions_sse","model_id":"test-frozen-model",
        "credential_ref":"env:LLM_API_KEY","stream":true,"max_tokens":8192,"timeout_ms":180000,"response_mode":"tool_calls",
        "transport_retries":0,"temperature":null,"reasoning_effort":null})).unwrap();
    Config::with_provider(
        provider,
        Limits {
            max_no_progress_turns: 6,
            max_focus_turns: 24,
            max_focus_replans: 2,
            max_turns: 30,
            max_tool_calls: 40,
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
        },
    )
    .unwrap()
}

#[tokio::test]
async fn pending_reads_are_not_evidence_until_a_successful_model_turn_after_resume() {
    let journal = MemoryJournal::default();
    *journal.interrupt_after.lock().unwrap() = Some(2);
    let model = script();
    let err = agent::run(
        &input(),
        &config(),
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(err.code, "INTERNAL");
    let saved = journal.load().await.unwrap().unwrap();
    assert!(
        saved.analysis.coverage.text.is_empty(),
        "a tool receipt is not model delivery"
    );
    assert!(saved.reviewer_coverage.text.is_empty());
    let json = serde_json::to_value(&saved).unwrap();
    assert_eq!(
        json["pending_coverage"]["text"]["source"],
        json!([[0, span().end]])
    );
    assert!(tools::validate_span(&input(), &saved.analysis.coverage, &span()).is_err());
    let result = agent::run(
        &input(),
        &config(),
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(result.quality, "verified");
    assert!(tools::validate_span(&input(), &result.analysis.coverage, &span()).is_ok());
    assert!(tools::validate_span(&input(), &result.review.coverage, &span()).is_ok());
}

#[tokio::test]
async fn failed_delivery_retains_pending_reads_without_committing_coverage() {
    struct Unavailable;
    #[async_trait]
    impl Model for Unavailable {
        async fn turn(&self, _: &Config, _: &[u8]) -> Result<ChatTurn, AgentError> {
            Err(AgentError::new(
                "AGENT_PROVIDER_UNAVAILABLE",
                "scripted HTTP 503",
            ))
        }
    }
    let journal = MemoryJournal::default();
    *journal.interrupt_after.lock().unwrap() = Some(2);
    agent::run(
        &input(),
        &config(),
        &journal,
        &script(),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let before = journal.load().await.unwrap().unwrap();
    let err = agent::run(
        &input(),
        &config(),
        &journal,
        &Unavailable,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(err.code, "AGENT_PROVIDER_UNAVAILABLE");
    let after = journal.load().await.unwrap().unwrap();
    assert!(after.analysis.coverage.text.is_empty());
    assert_eq!(after.turn, before.turn);
    assert_eq!(after.tool_calls, before.tool_calls);
    assert_eq!(after.read_bytes, before.read_bytes);
    assert_eq!(
        digest(&after.analysis).unwrap(),
        digest(&before.analysis).unwrap()
    );
    assert_eq!(
        digest(&after.pending_coverage).unwrap(),
        digest(&before.pending_coverage).unwrap()
    );
    assert_eq!(after.journal.sequence, before.journal.sequence + 1);
    assert!(after.journal.response().is_none());
    let reservations = journal.reservations.lock().unwrap();
    assert_eq!(after.journal.body().unwrap(), reservations[&2].0);
    assert_eq!(reservations[&2].1, 3);
}

#[tokio::test]
async fn a_same_turn_read_cannot_authorize_a_write_before_the_model_sees_it() {
    struct ReadThenWrite;
    #[async_trait]
    impl Model for ReadThenWrite {
        async fn turn(&self, _: &Config, _: &[u8]) -> Result<ChatTurn, AgentError> {
            Ok(ChatTurn {
                usage: None,
                content: String::new(),
                finish_reason: "tool_calls".into(),
                tool_calls: vec![
                    ChatToolCall {
                        id: "work".into(),
                        name: "set_work_note".into(),
                        arguments: active_work("source").to_string(),
                    },
                    ChatToolCall {
                        id: "read".into(),
                        name: "read_source".into(),
                        arguments: json!({"source_id":"source","start":0,"max_bytes":1024})
                            .to_string(),
                    },
                    ChatToolCall {
                        id: "write".into(),
                        name: "put_record".into(),
                        arguments: requirement().to_string(),
                    },
                ],
            })
        }
    }
    let journal = MemoryJournal::default();
    *journal.interrupt_after.lock().unwrap() = Some(1);
    agent::run(
        &input(),
        &config(),
        &journal,
        &ReadThenWrite,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let saved = journal.load().await.unwrap().unwrap();
    assert!(
        saved.analysis.records.is_empty(),
        "unseen read results cannot ground a record"
    );
    assert!(saved.analysis.coverage.text.is_empty());
    let result: Value = serde_json::from_str(
        saved.transcript.last().unwrap()["content"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(result["ok"], false);
}
fn active_work(source_id: &str) -> Value {
    json!({"source_scope":[source_id],"objective":"核对当前来源及其响应要求",
        "focus":{"action":"locate","source_spans":[],"references":[]},"status":"active","note":"从来源提取，保留跨范围引用"})
}

#[tokio::test]
async fn work_actions_can_plan_unread_text_without_authorizing_a_citation() {
    for (role, action) in [Role::Main, Role::Reviewer].into_iter().flat_map(|role| {
        ["locate", "extract", "review"]
            .into_iter()
            .map(move |action| (role.clone(), action))
    }) {
        let journal = if role == Role::Reviewer {
            fresh_review_journal().await
        } else {
            MemoryJournal::default()
        };
        let start = journal.load().await.unwrap().map_or(0, |s| s.turn);
        *journal.interrupt_after.lock().unwrap() = Some(start + 1);
        let mut work = active_work("source");
        work["focus"]["action"] = json!(action);
        work["focus"]["source_spans"] = json!([span()]);
        agent::run(
            &input(),
            &config(),
            &journal,
            &work_script(vec![("set_work_note", work)]),
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        let saved = journal.load().await.unwrap().unwrap();
        let work = if role == Role::Main {
            saved.main_work.as_ref()
        } else {
            saved.reviewer_work.as_ref()
        };
        assert!(
            work.is_some(),
            "planning a valid unread range for {action} must be possible"
        );
        let coverage = if role == Role::Main {
            &saved.analysis.coverage
        } else {
            &saved.reviewer_coverage
        };
        assert!(
            tools::validate_span(&input(), coverage, &span()).is_err(),
            "planning is not delivery, even if the other role has read the source"
        );
    }
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
            ..span()
        };
        let error = tools::validate_span(&input, &coverage, &citation).unwrap_err();
        assert!(error.contains(expected), "{error}");
        assert_eq!((citation.start, citation.end), (start, end));
    }
    for (start, end) in [(0, 3), (3, 6)] {
        let citation = Span {
            start,
            end,
            ..span()
        };
        tools::validate_text_span(&input, &citation).unwrap();
        assert_eq!(
            tools::validate_span(&input, &coverage, &citation).unwrap_err(),
            "read the cited source range before using it",
            "choosing a suggested boundary must not establish delivery"
        );
    }
}

#[tokio::test]
async fn completed_scope_needs_coverage_and_outcomes_but_no_new_focus() {
    for ready in [false, true] {
        let journal = MemoryJournal::default();
        let mut calls = vec![("set_work_note", active_work("source"))];
        if ready {
            calls.extend([
                ("read_source", json!({"source_id":"source","start":0,"max_bytes":1024})),
                ("set_disposition", json!({"source_id":"source","state":"non_requirement","reason":"diagnostic disposition"})),
            ]);
        }
        let mut complete = active_work("source");
        complete["status"] = json!("complete");
        complete["focus"]["action"] = json!("extract");
        calls.push(("set_work_note", complete));
        *journal.interrupt_after.lock().unwrap() = Some(calls.len());
        agent::run(
            &input(),
            &config(),
            &journal,
            &work_script(calls),
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        let saved = journal.load().await.unwrap().unwrap();
        assert_eq!(json!(saved.main_work.unwrap().status) == "complete", ready);
    }
}

fn work_script(calls: Vec<(&str, Value)>) -> Script {
    Script {
        calls: Mutex::new(calls.into_iter().map(|(n, v)| (n.into(), v)).collect()),
        bodies: Mutex::new(vec![]),
    }
}

#[tokio::test]
async fn overflowing_review_writes_are_rejected_atomically_and_fitted_checks_commit() {
    struct BatchChecks(Vec<String>);
    #[async_trait]
    impl Model for BatchChecks {
        async fn turn(&self, _: &Config, _: &[u8]) -> Result<ChatTurn, AgentError> {
            Ok(ChatTurn {
                usage: None,
                content: String::new(),
                finish_reason: "tool_calls".into(),
                tool_calls: self.0.iter().enumerate().map(|(i, reference)| ChatToolCall {
                    id: format!("check-{i}"),
                    name: "complete_review_check".into(),
                    arguments: json!({"reference":reference,"summary":"comparison ".repeat(700),"sources":[span()]}).to_string(),
                }).collect(),
            })
        }
    }
    let mut config = config();
    let journal = fresh_review_journal().await;
    let mut state = journal.load().await.unwrap().unwrap();
    let record = state.analysis.records.values().next().unwrap().clone();
    let mut references = Vec::new();
    state.reviewer_coverage = state.analysis.coverage.clone();
    for i in 0..6 {
        let mut record = record.clone();
        record.id = format!("overflow-{i}");
        let reference = format!("record:{}", record.id);
        state
            .reviewer_coverage
            .candidate
            .insert(reference.clone(), digest(&record).unwrap());
        state.analysis.records.insert(record.id.clone(), record);
        references.push(reference);
    }
    state.reviewer_work.as_mut().unwrap().focus.references = references.clone();
    // Size the boundary from the actual request and batch, so evolving tool
    // descriptions do not turn this partial-admission test into all-rejected.
    let baseline = agent::request(&input(), &config, &mut state.clone())
        .await
        .unwrap()
        .len();
    let batch = BatchChecks(references.clone())
        .turn(&config, &[])
        .await
        .unwrap();
    let arguments_bytes: usize = batch
        .tool_calls
        .iter()
        .map(|call| call.arguments.len())
        .sum();
    config.limits.max_context_bytes = baseline + arguments_bytes + arguments_bytes / 2;
    state.config_sha256 = digest(&config).unwrap();
    let before = state.clone();
    *journal.state.lock().unwrap() = Some(state);
    *journal.interrupt_after.lock().unwrap() = Some(before.turn + 1);
    let error = agent::run(
        &input(),
        &config,
        &journal,
        &BatchChecks(references),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(
        error.code, "INTERNAL",
        "the batch must commit before the injected stop"
    );
    let saved = journal.load().await.unwrap().unwrap();
    assert_eq!(saved.turn, before.turn + 1);
    assert_eq!(saved.tool_calls, before.tool_calls + 6);
    assert_eq!(json!(saved.analysis), json!(before.analysis));
    let mut accepted = 0;
    let mut rejected = 0;
    let mut projected = saved.clone();
    let wire: Value = serde_json::from_slice(
        &agent::request(&input(), &config, &mut projected)
            .await
            .unwrap(),
    )
    .unwrap();
    let packet: Value = serde_json::from_str(
        wire["messages"].as_array().unwrap().last().unwrap()["content"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let pending = packet["source_review"]["current"]["pending_candidate_refs"]["items"]
        .as_array()
        .unwrap();
    for message in saved.transcript.iter().filter(|m| m["role"] == "tool") {
        let output: Value = serde_json::from_str(message["content"].as_str().unwrap()).unwrap();
        if output["ok"] == true {
            accepted += 1;
            assert!(
                saved
                    .reviewer_progress
                    .seen
                    .contains(output["result"]["completion_sha256"].as_str().unwrap())
            );
        } else {
            rejected += 1;
            assert!(
                output["error"]
                    .as_str()
                    .unwrap()
                    .contains("remaining batch context")
            );
            let index = message["tool_call_id"]
                .as_str()
                .unwrap()
                .strip_prefix("check-")
                .unwrap();
            assert!(
                pending.contains(&json!(format!("record:overflow-{index}"))),
                "rejected write must remain pending in the next model request"
            );
        }
    }
    assert!(
        accepted > 0 && rejected > 0,
        "accepted={accepted}, rejected={rejected}"
    );
    assert_eq!(accepted + rejected, 6);
    assert_eq!(saved.reviewer_progress.watch.no_progress_turns, 0);
}

#[tokio::test]
async fn tool_batch_overflow_returns_feedback_and_only_retains_fitting_read_receipts() {
    struct BatchRead;
    #[async_trait]
    impl Model for BatchRead {
        async fn turn(&self, _: &Config, _: &[u8]) -> Result<ChatTurn, AgentError> {
            let mut calls = vec![ChatToolCall {
                id: "work".into(),
                name: "set_work_note".into(),
                arguments: active_work("source").to_string(),
            }];
            for i in 0..6 {
                calls.push(ChatToolCall {
                    id: format!("read-{i}"),
                    name: "read_source".into(),
                    arguments: json!({"source_id":"source","start":i*12000,"max_bytes":12000})
                        .to_string(),
                });
            }
            Ok(ChatTurn {
                usage: None,
                content: String::new(),
                finish_reason: "tool_calls".into(),
                tool_calls: calls,
            })
        }
    }
    for token_limited in [false, true] {
        let mut input = input();
        input.source_units[0].text = "x".repeat(72000);
        let mut config = config();
        config.limits.max_context_bytes = if token_limited { 200000 } else { 80000 };
        if token_limited {
            config.limits.max_context_tokens = 80000;
        }
        let journal = MemoryJournal::default();
        *journal.interrupt_after.lock().unwrap() = Some(1);
        let err = agent::run(
            &input,
            &config,
            &journal,
            &BatchRead,
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        assert_eq!(
            err.code, "INTERNAL",
            "a read overflow should save feedback, not strand the whole batch"
        );
        let saved = journal.load().await.unwrap().unwrap();
        assert!(saved.analysis.coverage.text.is_empty());
        let mut successes = 0;
        let mut rejected = 0;
        let mut delivered_ranges = Vec::new();
        for message in &saved.transcript {
            if message["role"] != "tool"
                || !message["tool_call_id"]
                    .as_str()
                    .unwrap()
                    .starts_with("read-")
            {
                continue;
            }
            let output: Value = serde_json::from_str(message["content"].as_str().unwrap()).unwrap();
            if output["ok"] == true {
                successes += 1;
                let start = output["result"]["start"].as_u64().unwrap() as usize;
                let end = output["result"]["end"].as_u64().unwrap() as usize;
                assert_eq!(
                    output["result"]["text"],
                    input.source_units[0].text[start..end]
                );
                delivered_ranges.push((start, end));
            } else {
                rejected += 1;
                assert!(
                    output["error"]
                        .as_str()
                        .unwrap()
                        .contains("remaining batch context")
                );
            }
        }
        assert!(successes > 0 && rejected > 0);
        assert_eq!(successes + rejected, 6);
        assert_eq!(
            saved.pending_coverage.as_ref().unwrap().text["source"],
            delivered_ranges
        );
        let mut state = saved.clone();
        let body = agent::request(&input, &config, &mut state).await.unwrap();
        assert!(body.len() <= config.limits.max_context_bytes);
        assert_eq!(
            digest(&saved.pending_coverage).unwrap(),
            digest(&state.pending_coverage).unwrap()
        );
        assert_eq!(saved.tool_calls, 7);
        assert_eq!(saved.turn, 1);
    }
}

#[tokio::test]
async fn review_findings_survive_handoff_ack_loss_and_are_finalized_from_storage() {
    let mut config = config();
    // This fixture includes an earlier full repair cycle and then an ACK-loss
    // review; freeze enough test calls for both before creating the journal.
    config.limits.max_turns = 40;
    config.limits.max_tool_calls = 80;
    let journal = fresh_review_journal_config(&config).await;
    let start = journal.load().await.unwrap().unwrap().turn;
    let finding = json!({"code":"FIELD_MISMATCH","message":"earlier saved field issue",
        "correction":"按所引原文补全并重新核对该字段", "affected":[],"sources":[span()]});
    *journal.interrupt_after.lock().unwrap() = Some(start + 3);
    let model = work_script(vec![
        ("set_work_note", active_work("source")),
        (
            "read_source",
            json!({"source_id":"source","start":0,"max_bytes":1024}),
        ),
        ("put_review_finding", json!({"id":null,"finding":finding})),
    ]);
    assert_eq!(
        agent::run(
            &input(),
            &config,
            &journal,
            &model,
            &CancellationToken::new()
        )
        .await
        .unwrap_err()
        .code,
        "INTERNAL"
    );
    let saved = journal.load().await.unwrap().unwrap();
    let id = saved.review_draft.keys().next().unwrap().clone();
    let restored: Checkpoint =
        serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
    *journal.state.lock().unwrap() = Some(restored);
    assert!(saved.analysis.relations.is_empty());
    let mut complete = active_work("source");
    complete["status"] = json!("complete");
    *journal.interrupt_after.lock().unwrap() = Some(start + 6);
    let model = work_script(vec![
        (
            "inspect_analysis",
            json!({"view":"detail","kind":"all","offset":0,"limit":10}),
        ),
        (
            "inspect_analysis",
            json!({"view":"detail","kind":"disposition","offset":0,"limit":10}),
        ),
        ("set_work_note", complete),
    ]);
    assert_eq!(
        agent::run(
            &input(),
            &config,
            &journal,
            &model,
            &CancellationToken::new()
        )
        .await
        .unwrap_err()
        .code,
        "INTERNAL"
    );
    let handed_off = journal.load().await.unwrap().unwrap();
    assert_eq!(
        handed_off.review_draft[&id].message,
        "earlier saved field issue"
    );
    assert!(
        !serde_json::to_string(&handed_off.transcript)
            .unwrap()
            .contains("earlier saved field issue"),
        "handoff was not completed: {:?}",
        handed_off.transcript.last()
    );
    assert!(handed_off.read_bytes >= saved.read_bytes);
    let mut revised = finding;
    revised["message"] = json!("revised field correction");
    let model = work_script(vec![
        ("set_work_note", active_work("source")),
        ("inspect_review", json!({"offset":0,"limit":1})),
        // The retired bulk payload must not clear a persisted finding.
        ("put_source_review", json!({"findings":[]})),
        ("put_review_finding", json!({"id":id,"finding":revised})),
        ("put_source_review", json!({"fixture_status":"findings"})),
    ]);
    let result = agent::run(
        &input(),
        &config,
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(result.review.findings.len(), 1);
    assert_eq!(
        result.review.findings[0].message,
        "revised field correction"
    );
    assert_ne!(result.quality, "verified");
    assert!(journal.load().await.unwrap().unwrap().review_draft.len() == 1);
    let bodies = model.bodies.lock().unwrap();
    assert!(bodies[2].to_string().contains(&id));
    assert!(bodies[3].to_string().contains("unknown field"));
}

async fn fresh_review_journal() -> MemoryJournal {
    fresh_review_journal_config(&config()).await
}

async fn fresh_review_journal_config(config: &Config) -> MemoryJournal {
    let journal = MemoryJournal::default();
    agent::run(
        &input(),
        config,
        &journal,
        &script(),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    let mut state = journal.load().await.unwrap().unwrap();
    state.role = Role::Reviewer;
    state.done = false;
    state.review = None;
    state.reviewer_coverage = Coverage::default();
    state.reviewer_progress = Default::default();
    state.pending_coverage = None;
    state.reviewer_work = None;
    state.transcript.clear();
    state.source_review = Some(agent::source_review::initialize(&input(), config).unwrap());
    agent::source_review::select_next(&input(), config, &mut state).unwrap();
    *journal.state.lock().unwrap() = Some(state);
    journal
}

#[tokio::test]
async fn read_receipts_and_empty_submission_cannot_attest_source_omissions() {
    let journal = fresh_review_journal().await;
    let start = journal.load().await.unwrap().unwrap().turn;
    let calls = vec![
        ("set_work_note", active_work("source")),
        (
            "read_source",
            json!({"source_id":"source","start":0,"max_bytes":1024}),
        ),
        (
            "inspect_analysis",
            json!({"kind":"all","view":"detail","offset":0,"limit":10}),
        ),
        (
            "inspect_analysis",
            json!({"kind":"disposition","view":"detail","offset":0,"limit":10}),
        ),
        ("submit_review", json!({})),
    ];
    *journal.interrupt_after.lock().unwrap() = Some(start + calls.len());
    let _ = agent::run(
        &input(),
        &config(),
        &journal,
        &work_script(calls),
        &CancellationToken::new(),
    )
    .await;
    let saved = journal.load().await.unwrap().unwrap();
    assert!(
        !saved.done,
        "reading coverage is not an independent omission judgment"
    );
    assert!(saved.review.is_none());
}

#[tokio::test]
async fn review_finding_must_fit_its_retrieval_envelope_before_persistence() {
    let journal = fresh_review_journal().await;
    let start = journal.load().await.unwrap().unwrap().turn;
    let mut finding = json!({"code":"FIELD_MISMATCH","message":"",
        "correction":"按所引原文补全并重新核对该字段", "affected":[],"sources":[span()]});
    let overhead = serde_json::to_vec(&finding).unwrap().len();
    finding["message"] = json!("x".repeat(config().limits.max_tool_result_bytes - overhead));
    assert_eq!(
        serde_json::to_vec(&finding).unwrap().len(),
        config().limits.max_tool_result_bytes
    );
    *journal.interrupt_after.lock().unwrap() = Some(start + 3);
    let model = work_script(vec![
        ("set_work_note", active_work("source")),
        (
            "read_source",
            json!({"source_id":"source","start":0,"max_bytes":1024}),
        ),
        ("put_review_finding", json!({"id":null,"finding":finding})),
    ]);
    agent::run(
        &input(),
        &config(),
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let saved = journal.load().await.unwrap().unwrap();
    assert!(
        saved.review_draft.is_empty(),
        "an unqueryable finding must not be saved"
    );
    assert!(
        saved.transcript.last().unwrap()["content"]
            .as_str()
            .unwrap()
            .contains("finding exceeds budget")
    );
}

#[tokio::test]
async fn review_findings_require_current_field_paths_and_concrete_corrections() {
    let journal = fresh_review_journal().await;
    let saved = journal.load().await.unwrap().unwrap();
    let id = saved.analysis.records.keys().next().unwrap();
    let finding = json!({"code":"FIELD_MISMATCH","message":"所引条款与响应条件不一致",
        "correction":"依据引用条款核对并修正条件，保留原文适用范围",
        "affected":[{"id":id,"path":"/data"}],"sources":[span()]});
    let mut bad_path = finding.clone();
    bad_path["affected"][0]["path"] = json!("/data/nonexistent");
    let mut missing_correction = finding.clone();
    missing_correction["correction"] = json!(" ");
    let mut duplicate = finding.clone();
    duplicate["affected"]
        .as_array_mut()
        .unwrap()
        .push(finding["affected"][0].clone());
    *journal.interrupt_after.lock().unwrap() = Some(saved.turn + 7);
    let model = work_script(vec![
        ("set_work_note", active_work("source")),
        (
            "read_source",
            json!({"source_id":"source","start":0,"max_bytes":1024}),
        ),
        (
            "inspect_analysis",
            json!({"view":"detail","kind":"all","ids":[id],"offset":0,"limit":1}),
        ),
        ("put_review_finding", json!({"id":null,"finding":bad_path})),
        (
            "put_review_finding",
            json!({"id":null,"finding":missing_correction}),
        ),
        ("put_review_finding", json!({"id":null,"finding":duplicate})),
        ("put_review_finding", json!({"id":null,"finding":finding})),
    ]);
    agent::run(
        &input(),
        &config(),
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let saved = journal.load().await.unwrap().unwrap();
    assert_eq!(saved.review_draft.len(), 1);
    assert_eq!(
        saved.review_draft.values().next().unwrap().affected[0].path,
        "/data"
    );
    let bodies = model.bodies.lock().unwrap();
    for (index, message) in [
        (4, "unknown affected field"),
        (5, "concrete correction"),
        (6, "duplicate affected field"),
    ] {
        assert!(bodies[index].to_string().contains(message), "{message}");
    }
}

#[tokio::test]
async fn review_draft_rejects_unseen_evidence_and_main_agent_mutations() {
    let journal = fresh_review_journal().await;
    let state = journal.load().await.unwrap().unwrap();
    let record_id = state.analysis.records.keys().next().unwrap().clone();
    let finding = json!({"code":"FIELD_MISMATCH","message":"specific field issue",
        "correction":"按所引原文补全并重新核对该字段", "affected":[],"sources":[span()]});
    let unseen_candidate = json!({"code":"FIELD_MISMATCH","message":"specific field issue",
        "correction":"核对原文后修正该字段", "affected":[{"id":record_id,"path":"/data"}],"sources":[]});
    *journal.interrupt_after.lock().unwrap() = Some(state.turn + 3);
    let model = work_script(vec![
        ("put_review_finding", json!({"id":null,"finding":finding})),
        (
            "put_review_finding",
            json!({"id":null,"finding":unseen_candidate}),
        ),
        ("delete_review_finding", json!({"id":"foreign"})),
    ]);
    agent::run(
        &input(),
        &config(),
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let mut saved = journal.load().await.unwrap().unwrap();
    assert!(saved.review_draft.is_empty());
    assert!(
        model.bodies.lock().unwrap()[2]
            .to_string()
            .contains("independently inspect current affected outcome")
    );
    // Fixture with a previously delivered reviewer finding; main cannot edit it.
    saved.review_draft.insert(
        "existing".into(),
        serde_json::from_value(finding.clone()).unwrap(),
    );
    saved.reviewer_coverage = saved.analysis.coverage.clone();
    saved.role = Role::Main;
    let start = saved.turn;
    *journal.state.lock().unwrap() = Some(saved);
    *journal.interrupt_after.lock().unwrap() = Some(start + 2);
    let model = work_script(vec![
        ("put_review_finding", json!({"id":null,"finding":finding})),
        ("delete_review_finding", json!({"id":"existing"})),
    ]);
    agent::run(
        &input(),
        &config(),
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let mut saved = journal.load().await.unwrap().unwrap();
    assert_eq!(saved.review_draft.len(), 1);
    assert!(saved.review_draft.contains_key("existing"));
    saved.role = Role::Reviewer;
    let next = saved.turn + 1;
    *journal.state.lock().unwrap() = Some(saved);
    *journal.interrupt_after.lock().unwrap() = Some(next);
    agent::run(
        &input(),
        &config(),
        &journal,
        &work_script(vec![("delete_review_finding", json!({"id":"existing"}))]),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert!(
        journal
            .load()
            .await
            .unwrap()
            .unwrap()
            .review_draft
            .is_empty()
    );
    let main = tools::schemas(false);
    let reviewer = tools::schemas(true);
    for name in ["put_review_finding", "delete_review_finding"] {
        assert!(!main.iter().any(|t| t["function"]["name"] == name));
        assert!(reviewer.iter().any(|t| t["function"]["name"] == name));
    }
}

#[tokio::test]
async fn invalid_token_configuration_cannot_reserve_or_call_a_model() {
    for invalid_case in 0..4 {
        let mut config = config();
        match invalid_case {
            0 => {
                config.limits.max_context_tokens =
                    config.provider.max_tokens as usize + config.limits.token_safety_margin
            }
            1 => config.limits.token_safety_margin = 0,
            2 => config.limits.image_token_reserve = 0,
            _ => config.limits.token_safety_margin = usize::MAX,
        }
        let journal = MemoryJournal::default();
        let model = script();
        assert!(
            agent::run(
                &input(),
                &config,
                &journal,
                &model,
                &CancellationToken::new()
            )
            .await
            .is_err()
        );
        assert!(journal.reservations.lock().unwrap().is_empty());
        assert!(model.bodies.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn token_budget_trims_both_roles_and_reserves_output_below_the_byte_ceiling() {
    let journal = MemoryJournal::default();
    agent::run(
        &input(),
        &config(),
        &journal,
        &script(),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    let saved = journal.load().await.unwrap().unwrap();
    for role in [Role::Main, Role::Reviewer] {
        let mut state = saved.clone();
        state.role = role;
        state.transcript.clear();
        let mut config = config();
        let base_bytes = agent::request(&input(), &config, &mut state)
            .await
            .unwrap()
            .len();
        config.limits.max_context_tokens = base_bytes
            + config.provider.max_tokens as usize
            + config.limits.token_safety_margin
            + 6000;
        config.limits.max_history_bytes = 90000;
        for i in 0..8 {
            state.transcript.extend([
                json!({"role":"assistant","content":null,"tool_calls":[{"id":format!("read-{i}"),
                    "type":"function","function":{"name":"read_source","arguments":"{}"}}]}),
                json!({"role":"tool","tool_call_id":format!("read-{i}"),
                    "content":format!("range-{i}:{}", "中文😀".repeat(300))}),
            ]);
        }
        state.pending_coverage = Some(Coverage::default());
        let before = state.clone();
        let bytes = agent::request(&input(), &config, &mut state).await.unwrap();
        assert!(
            bytes.len() + config.limits.token_safety_margin + config.provider.max_tokens as usize
                <= config.limits.max_context_tokens
        );
        assert!(bytes.len() < config.limits.max_context_bytes);
        let text = std::str::from_utf8(&bytes).unwrap();
        assert!(!text.contains("range-0:"));
        assert!(text.contains("range-7:"));
        assert_eq!(
            digest(&before.pending_coverage).unwrap(),
            digest(&state.pending_coverage).unwrap()
        );
        let mut replay = before;
        assert_eq!(
            bytes,
            agent::request(&input(), &config, &mut replay)
                .await
                .unwrap()
        );
        // Increasing output reservation can make even the last group impossible.
        config.provider.max_tokens =
            (config.limits.max_context_tokens - config.limits.token_safety_margin - base_bytes + 1)
                as u32;
        assert_eq!(
            agent::request(&input(), &config, &mut state)
                .await
                .unwrap_err()
                .code,
            "AGENT_TURN_BUDGET_EXCEEDED"
        );
        assert!(
            state.transcript.last().unwrap()["content"]
                .as_str()
                .unwrap()
                .contains("range-7:")
        );
        assert!(state.pending_coverage.is_some());
    }
}

#[tokio::test]
async fn both_extraction_roles_send_only_frozen_optional_reasoning() {
    for effort in [None, Some("high")] {
        let mut config = config();
        config.provider.reasoning_effort = effort.map(String::from);
        config.provider.max_tokens = 4096;
        config.provider.timeout_ms = 90000;
        let model = script();
        let journal = MemoryJournal::default();
        agent::run(
            &input(),
            &config,
            &journal,
            &model,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        let bodies = model.bodies.lock().unwrap();
        assert!(
            bodies
                .iter()
                .any(|b| b["messages"][0]["content"] != bodies[0]["messages"][0]["content"])
        );
        for body in bodies.iter() {
            assert_eq!(body["max_tokens"], 4096);
            assert_eq!(body.get("reasoning_effort").and_then(Value::as_str), effort);
            assert_eq!(body.get("reasoning_effort").is_some(), effort.is_some());
        }
    }
}

#[tokio::test]
async fn work_split_retains_deferred_sources_outcomes_and_review_barriers() {
    let (input, analysis, ids) = analysis_query_fixture();
    for role in [Role::Main, Role::Reviewer] {
        let journal = MemoryJournal::default();
        *journal.interrupt_after.lock().unwrap() = Some(1);
        let mut broad = active_work("source");
        broad["source_scope"] = json!(["source", "other-source"]);
        agent::run(
            &input,
            &config(),
            &journal,
            &work_script(vec![("set_work_note", broad.clone())]),
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        let mut state = journal.load().await.unwrap().unwrap();
        state.analysis = analysis.clone();
        state.role = role.clone();
        if role == Role::Reviewer {
            state.source_review =
                Some(agent::source_review::initialize(&input, &config()).unwrap());
            agent::source_review::select_next(&input, &config(), &mut state).unwrap();
            state.reviewer_work = Some(serde_json::from_value(broad).unwrap());
        }
        let analysis_before = digest(&state.analysis).unwrap();
        *journal.state.lock().unwrap() = Some(state);
        let mut split = active_work("source");
        split["deferred_sources"] = json!(["other-source"]);
        let mut manual_refs = split.clone();
        manual_refs["output_refs"] = json!([]);
        let mut foreign = split.clone();
        foreign["deferred_sources"] = json!(["foreign"]);
        let transition = if role == Role::Main {
            "request_review"
        } else {
            "put_source_review"
        };
        *journal.interrupt_after.lock().unwrap() = Some(7);
        let model = work_script(vec![
            ("set_work_note", active_work("source")),
            ("set_work_note", foreign),
            ("set_work_note", manual_refs),
            ("set_work_note", split.clone()),
            ("set_work_note", active_work("source")),
            (transition, json!({})),
        ]);
        agent::run(
            &input,
            &config(),
            &journal,
            &model,
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        let state = journal.load().await.unwrap().unwrap();
        let work = if role == Role::Main {
            state.main_work.as_ref()
        } else {
            state.reviewer_work.as_ref()
        }
        .unwrap();
        assert_eq!(work.source_scope, vec!["source"]);
        assert_eq!(work.deferred_sources, vec!["other-source"]);
        assert!(work.output_refs.contains(&format!("record:{}", ids[1])));
        assert!(work.output_refs.contains(&format!("relation:{}", ids[3])));
        assert_eq!(state.role, role);
        assert!(!state.done);
        assert_eq!(digest(&state.analysis).unwrap(), analysis_before);
        let outputs: Vec<Value> = model
            .bodies
            .lock()
            .unwrap()
            .iter()
            .flat_map(|body| {
                body["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|m| m["role"] == "tool")
                    .map(|m| serde_json::from_str(m["content"].as_str().unwrap()).unwrap())
            })
            .collect();
        for message in [
            "finish the active scope",
            "unknown work source",
            "maintained by the host",
            "resume the deferred source",
        ] {
            assert!(
                outputs
                    .iter()
                    .any(|out| out["error"].as_str().is_some_and(|e| e.contains(message))),
                "missing rejection: {message}"
            );
        }
        let last: Value = serde_json::from_str(
            state.transcript.last().unwrap()["content"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        let error = last["error"].as_str().unwrap();
        assert!(
            if role == Role::Main {
                error.contains("resume deferred_sources")
            } else {
                error.contains("independently") || error.contains("read the cited source")
            },
            "{error}"
        );
        assert!(outputs.iter().any(|out| out["result"]["handoff"] == true));
        // Reconstruct the checkpoint as durable JSON, then explicitly resume
        // each deferred source. Switching focus must retain the other source.
        let restored: agent::Checkpoint = serde_json::from_value(json!(state)).unwrap();
        *journal.state.lock().unwrap() = Some(restored);
        let mut resume = active_work("other-source");
        resume["deferred_sources"] = json!(["source"]);
        let mut expanded = resume.clone();
        expanded["source_scope"] = json!(["other-source", "source"]);
        expanded["deferred_sources"] = json!([]);
        *journal.interrupt_after.lock().unwrap() = Some(9);
        agent::run(
            &input,
            &config(),
            &journal,
            &work_script(vec![
                ("set_work_note", resume),
                ("set_work_note", expanded.clone()),
            ]),
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        let resumed = journal.load().await.unwrap().unwrap();
        let work = if role == Role::Main {
            resumed.main_work.as_ref()
        } else {
            resumed.reviewer_work.as_ref()
        }
        .unwrap();
        assert_eq!(json!(work.source_scope), expanded["source_scope"]);
        assert!(work.deferred_sources.is_empty());
        assert_eq!(digest(&resumed.analysis).unwrap(), analysis_before);
    }
}

#[tokio::test]
async fn work_references_cannot_be_fabricated_or_erased_by_deletion() {
    let journal = MemoryJournal::default();
    *journal.interrupt_after.lock().unwrap() = Some(1);
    agent::run(
        &input(),
        &config(),
        &journal,
        &work_script(vec![("set_work_note", active_work("source"))]),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let mut state = journal.load().await.unwrap().unwrap();
    let record = Record {
        id: "pending".into(),
        sources: vec![span()],
        data: RecordData::Unresolved {
            problem: "引用目标仍待核查".into(),
            affected: vec![],
            candidates: vec![],
        },
    };
    state.analysis.records.insert(record.id.clone(), record);
    let mut pending = active_work("source");
    pending["pending_refs"] = json!(["record:pending"]);
    // A main-agent deletion must also preserve unresolved reviewer handoffs.
    state.reviewer_work = Some(serde_json::from_value(pending).unwrap());
    *journal.state.lock().unwrap() = Some(state);
    *journal.interrupt_after.lock().unwrap() = Some(4);
    let mut fabricated = active_work("source");
    fabricated["output_refs"] = json!(["record:invented"]);
    let model = work_script(vec![
        ("set_work_note", active_work("foreign")),
        ("set_work_note", fabricated),
        ("delete_record", json!({"id":"pending"})),
    ]);
    agent::run(
        &input(),
        &config(),
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let state = journal.load().await.unwrap().unwrap();
    assert!(state.analysis.records.contains_key("pending"));
    assert_eq!(state.main_work.unwrap().source_scope, vec!["source"]);
    let errors: Vec<Value> = state
        .transcript
        .iter()
        .filter(|m| m["role"] == "tool")
        .map(|m| serde_json::from_str(m["content"].as_str().unwrap()).unwrap())
        .filter(|out: &Value| out["ok"] == false)
        .collect();
    assert_eq!(errors.len(), 3);
    for (out, reason) in errors.iter().zip([
        "unknown work source",
        "maintained by the host",
        "resolve the pending outcome before deleting",
    ]) {
        assert!(out["error"].as_str().unwrap().contains(reason));
    }
}

#[tokio::test]
async fn work_gaps_find_local_blockers_hidden_behind_global_unread_pages() {
    let (mut input, mut analysis, _) = analysis_query_fixture();
    for ordinal in 2..62 {
        input.source_units.push(Source {
            source_unit_revision_id: format!("unrelated-{ordinal}"),
            ordinal,
            ..input.source_units[0].clone()
        });
    }
    analysis.dispositions.remove("source");
    let journal = MemoryJournal::default();
    *journal.interrupt_after.lock().unwrap() = Some(1);
    agent::run(
        &input,
        &config(),
        &journal,
        &work_script(vec![("set_work_note", active_work("source"))]),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let mut saved = journal.load().await.unwrap().unwrap();
    saved.analysis = analysis;
    let before = digest(&saved.analysis).unwrap();
    *journal.state.lock().unwrap() = Some(saved);
    *journal.interrupt_after.lock().unwrap() = Some(3);
    agent::run(
        &input,
        &config(),
        &journal,
        &work_script(vec![
            (
                "check_gaps",
                json!({"scope":"analysis","offset":0,"limit":50}),
            ),
            ("check_gaps", json!({"scope":"work","offset":0,"limit":100})),
        ]),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let saved = journal.load().await.unwrap().unwrap();
    assert_eq!(
        digest(&saved.analysis).unwrap(),
        before,
        "diagnostics cannot establish evidence"
    );
    assert!(saved.pending_coverage.is_none());
    let pages: Vec<Value> = saved
        .transcript
        .iter()
        .filter(|m| m["role"] == "tool")
        .map(|m| {
            serde_json::from_str::<Value>(m["content"].as_str().unwrap()).unwrap()["result"].clone()
        })
        .filter(|v| v["items"].is_array())
        .collect();
    assert_eq!(pages.len(), 2);
    assert!(
        pages[0]["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|gap| gap["kind"] == "unread_source" && gap["source_id"] != "source")
    );
    let local = pages[1]["items"].as_array().unwrap();
    assert_eq!(pages[1]["next"], pages[1]["total"]);
    assert_eq!(
        local.len(),
        1,
        "only the missing disposition is a blocker; outcome references are host-maintained"
    );
    assert_eq!(local[0]["kind"], "missing_disposition");
    assert_eq!(local[0]["source_id"], "source");
    let mut complete = active_work("source");
    complete["status"] = json!("complete");
    *journal.interrupt_after.lock().unwrap() = Some(8);
    let model = work_script(vec![
        (
            "set_disposition",
            json!({"source_id":"source","state":"non_requirement","reason":"已读事实来源"}),
        ),
        ("set_work_note", complete),
        ("check_gaps", json!({"scope":"work","offset":0,"limit":10})),
        ("request_review", json!({})),
        ("set_work_note", active_work("other-source")),
    ]);
    agent::run(
        &input,
        &config(),
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let saved = journal.load().await.unwrap().unwrap();
    assert_eq!(saved.main_work.unwrap().source_scope, vec!["other-source"]);
    let bodies = model.bodies.lock().unwrap();
    let tool = bodies[3]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|m| m["role"] == "tool")
        .unwrap();
    let result: Value = serde_json::from_str(tool["content"].as_str().unwrap()).unwrap();
    assert_eq!(result["result"]["total"], 0);
    assert!(
        bodies[4]
            .to_string()
            .contains("structural/reading gaps remain"),
        "zero local blockers cannot authorize global review"
    );
}

#[test]
fn gap_queries_require_explicit_scope_and_reject_invalid_pages_without_evidence() {
    for reviewer in [false, true] {
        let mut analysis = Analysis::default();
        let mut coverage = Coverage::default();
        let before = digest(&coverage).unwrap();
        for query in [
            json!({"offset":0,"limit":10}),
            json!({"scope":"unknown","offset":0,"limit":10}),
            json!({"scope":"analysis","offset":0,"limit":0}),
            json!({"scope":"analysis","offset":usize::MAX,"limit":10}),
        ] {
            assert!(
                tools::invoke(
                    &input(),
                    &mut analysis,
                    &mut coverage,
                    reviewer,
                    "check_gaps",
                    &query,
                    16000
                )
                .is_err()
            );
            assert_eq!(digest(&coverage).unwrap(), before);
        }
        let schema = tools::schemas(reviewer)
            .into_iter()
            .find(|tool| tool["function"]["name"] == "check_gaps")
            .unwrap();
        assert!(
            schema["function"]["parameters"]["required"]
                .as_array()
                .unwrap()
                .contains(&json!("scope"))
        );
    }
}

#[tokio::test]
async fn rereading_completed_evidence_does_not_reopen_work_gaps_or_block_handoff() {
    struct InspectAndComplete;
    #[async_trait]
    impl Model for InspectAndComplete {
        async fn turn(&self, _: &Config, _: &[u8]) -> Result<ChatTurn, AgentError> {
            let mut complete = active_work("source");
            complete["status"] = json!("complete");
            complete["focus"]["action"] = json!("handoff");
            let calls = [
                (
                    "read_source",
                    json!({"source_id":"source","start":0,"max_bytes":1024}),
                ),
                (
                    "inspect_analysis",
                    json!({"view":"detail","kind":"all","offset":0,"limit":100}),
                ),
                ("check_gaps", json!({"scope":"work","offset":0,"limit":100})),
                ("set_work_note", complete),
            ];
            Ok(ChatTurn {
                finish_reason: "tool_calls".into(),
                tool_calls: calls
                    .into_iter()
                    .enumerate()
                    .map(|(i, (name, args))| ChatToolCall {
                        id: format!("repeat-{i}"),
                        name: name.into(),
                        arguments: args.to_string(),
                    })
                    .collect(),
                ..Default::default()
            })
        }
    }
    for role in [Role::Main, Role::Reviewer] {
        let journal = MemoryJournal::default();
        agent::run(
            &input(),
            &config(),
            &journal,
            &script(),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        let mut state = journal.load().await.unwrap().unwrap();
        state.role = role;
        state.done = false;
        state.review = None;
        state.transcript.clear();
        state.pending_coverage = None;
        state.main_work = Some(serde_json::from_value(active_work("source")).unwrap());
        state.reviewer_work = state.main_work.clone();
        *journal.interrupt_after.lock().unwrap() = Some(state.turn + 1);
        *journal.state.lock().unwrap() = Some(state);
        agent::run(
            &input(),
            &config(),
            &journal,
            &InspectAndComplete,
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        let saved = journal.load().await.unwrap().unwrap();
        let work = if saved.role == Role::Main {
            saved.main_work.as_ref()
        } else {
            saved.reviewer_work.as_ref()
        };
        assert_eq!(
            json!(work.unwrap().status),
            "complete",
            "duplicate reads must not create a new completion obligation"
        );
    }
}

#[tokio::test]
async fn work_gap_diagnostics_cannot_count_same_batch_reads_as_reviewer_evidence() {
    struct ReadAndDiagnose;
    #[async_trait]
    impl Model for ReadAndDiagnose {
        async fn turn(&self, _: &Config, _: &[u8]) -> Result<ChatTurn, AgentError> {
            let calls = [
                (
                    "read_source",
                    json!({"source_id":"source","start":0,"max_bytes":1024}),
                ),
                (
                    "inspect_analysis",
                    json!({"view":"detail","kind":"all","offset":0,"limit":100}),
                ),
                ("check_gaps", json!({"scope":"work","offset":0,"limit":100})),
            ];
            Ok(ChatTurn {
                usage: None,
                content: String::new(),
                finish_reason: "tool_calls".into(),
                tool_calls: calls
                    .into_iter()
                    .enumerate()
                    .map(|(i, (name, args))| ChatToolCall {
                        id: format!("probe-{i}"),
                        name: name.into(),
                        arguments: args.to_string(),
                    })
                    .collect(),
            })
        }
    }
    let journal = fresh_review_journal().await;
    let mut saved = journal.load().await.unwrap().unwrap();
    saved.reviewer_work = Some(serde_json::from_value(active_work("source")).unwrap());
    let before = digest(&saved.reviewer_coverage).unwrap();
    let turn = saved.turn;
    *journal.state.lock().unwrap() = Some(saved);
    *journal.interrupt_after.lock().unwrap() = Some(turn + 1);
    agent::run(
        &input(),
        &config(),
        &journal,
        &ReadAndDiagnose,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let saved = journal.load().await.unwrap().unwrap();
    assert_eq!(digest(&saved.reviewer_coverage).unwrap(), before);
    assert!(
        saved
            .pending_coverage
            .as_ref()
            .unwrap()
            .text
            .contains_key("source")
    );
    let result: Value = serde_json::from_str(
        saved.transcript.last().unwrap()["content"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(result["ok"], true);
    let gaps = result["result"]["items"].as_array().unwrap();
    for kind in ["unread_source", "unreviewed_scope_outcome"] {
        assert!(
            gaps.iter().any(|g| g["kind"] == kind),
            "missing blocker {kind}"
        );
    }
}

#[tokio::test]
async fn work_handoff_cannot_drop_an_unresolved_outcome() {
    let mut input = input();
    input.source_units.push(Source {
        source_unit_revision_id: "other".into(),
        ordinal: 1,
        ..input.source_units[0].clone()
    });
    let mut complete = active_work("source");
    complete["status"] = json!("complete");
    let model = work_script(vec![
        ("set_work_note", active_work("source")),
        (
            "read_source",
            json!({"source_id":"source","start":0,"max_bytes":1024}),
        ),
        (
            "set_disposition",
            json!({"source_id":"source","state":"unresolved","reason":"引用目标尚未解析"}),
        ),
        ("set_work_note", complete),
        ("set_work_note", active_work("other")),
        (
            "check_gaps",
            json!({"scope":"pending","offset":0,"limit":10}),
        ),
    ]);
    let journal = MemoryJournal::default();
    *journal.interrupt_after.lock().unwrap() = Some(6);
    agent::run(
        &input,
        &config(),
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let state = journal.load().await.unwrap().unwrap();
    assert_eq!(
        state.main_work.as_ref().unwrap().source_scope,
        vec!["other"]
    );
    assert_eq!(
        state.main_work.as_ref().unwrap().pending_refs,
        vec!["disposition:source"]
    );
    let pending: Value = serde_json::from_str(
        state.transcript.last().unwrap()["content"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        pending["result"]["items"][0]["reference"],
        "disposition:source"
    );
}

#[tokio::test]
async fn reviewer_scope_completion_requires_its_own_source_and_candidate_delivery() {
    let journal = MemoryJournal::default();
    agent::run(
        &input(),
        &config(),
        &journal,
        &script(),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    let mut state = journal.load().await.unwrap().unwrap();
    // Start a fresh review fixture over actual stored outcomes, with no inherited evidence.
    state.role = Role::Reviewer;
    state.done = false;
    state.transcript.clear();
    state.reviewer_work = None;
    state.reviewer_coverage = Coverage::default();
    state.pending_coverage = None;
    let turn = state.turn;
    *journal.state.lock().unwrap() = Some(state);
    *journal.interrupt_after.lock().unwrap() = Some(turn + 4);
    let mut complete = active_work("source");
    complete["status"] = json!("complete");
    let model = work_script(vec![
        ("set_work_note", active_work("source")),
        ("set_work_note", complete.clone()),
        (
            "read_source",
            json!({"source_id":"source","start":0,"max_bytes":1024}),
        ),
        ("set_work_note", complete),
    ]);
    agent::run(
        &input(),
        &config(),
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let state = journal.load().await.unwrap().unwrap();
    let last: Value = serde_json::from_str(
        state.transcript.last().unwrap()["content"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(last["ok"], false);
    assert!(
        last["error"]
            .as_str()
            .unwrap()
            .contains("independently inspect current scope outcome")
    );
    assert!(
        model.bodies.lock().unwrap()[2]
            .to_string()
            .contains("undelivered source ranges")
    );
    assert!(state.reviewer_coverage.candidate.is_empty());
}

#[tokio::test]
async fn source_reads_require_a_known_active_scope_and_explicit_cross_reference_expansion() {
    let mut input = input();
    input.source_units.push(Source {
        source_unit_revision_id: "other".into(),
        ordinal: 1,
        ..input.source_units[0].clone()
    });
    let mut expanded = active_work("source");
    expanded["source_scope"] = json!(["source", "other"]);
    let model = work_script(vec![
        (
            "read_source",
            json!({"source_id":"source","start":0,"max_bytes":1024}),
        ),
        ("set_work_note", active_work("source")),
        (
            "read_source",
            json!({"source_id":"other","start":0,"max_bytes":1024}),
        ),
        ("set_work_note", expanded),
        (
            "read_source",
            json!({"source_id":"other","start":0,"max_bytes":1024}),
        ),
    ]);
    let journal = MemoryJournal::default();
    *journal.interrupt_after.lock().unwrap() = Some(5);
    agent::run(
        &input,
        &config(),
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let state = journal.load().await.unwrap().unwrap();
    assert!(state.analysis.coverage.text.is_empty());
    assert_eq!(state.pending_coverage.as_ref().unwrap().text.len(), 1);
    assert!(
        state
            .pending_coverage
            .as_ref()
            .unwrap()
            .text
            .contains_key("other")
    );
    let bodies = model.bodies.lock().unwrap();
    for (index, reason) in [(1, "declare an active"), (3, "outside active")] {
        assert!(bodies[index]["messages"].to_string().contains(reason));
    }
}

#[tokio::test]
async fn completed_work_releases_old_source_below_the_context_ceiling_and_keeps_outcomes() {
    let mut input = input();
    input.source_units[0].text = "EARLIER_SOURCE_TEXT_甲乙".repeat(100);
    input.source_units.push(Source {
        source_unit_revision_id: "other".into(),
        ordinal: 1,
        text: "NEXT_SOURCE_TEXT_丙丁".into(),
        ..input.source_units[0].clone()
    });
    let mut complete = active_work("source");
    complete["status"] = json!("complete");
    let model = work_script(vec![
        ("set_work_note", active_work("source")),
        (
            "read_source",
            json!({"source_id":"source","start":0,"max_bytes":10000}),
        ),
        (
            "set_disposition",
            json!({"source_id":"source","state":"non_requirement","reason":"来源说明"}),
        ),
        ("set_work_note", complete),
        ("set_work_note", active_work("other")),
        (
            "read_source",
            json!({"source_id":"other","start":0,"max_bytes":10000}),
        ),
        (
            "check_gaps",
            json!({"scope":"analysis","offset":0,"limit":10}),
        ),
    ]);
    let journal = MemoryJournal::default();
    *journal.interrupt_after.lock().unwrap() = Some(7);
    agent::run(
        &input,
        &config(),
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    {
        let bodies = model.bodies.lock().unwrap();
        assert!(bodies[3].to_string().contains("EARLIER_SOURCE_TEXT_甲乙"));
        assert!(
            !bodies[4].to_string().contains("EARLIER_SOURCE_TEXT_甲乙"),
            "handoff must release text before the emergency ceiling"
        );
        assert!(bodies[6].to_string().contains("NEXT_SOURCE_TEXT_丙丁"));
    }
    let state = journal.load().await.unwrap().unwrap();
    assert!(state.analysis.dispositions.contains_key("source"));
    assert_eq!(state.analysis.coverage.text.len(), 2);
    assert_eq!(state.main_work.unwrap().source_scope, vec!["other"]);
}

#[tokio::test]
async fn bounded_history_keeps_latest_unseen_group_and_replay_bytes_for_both_roles() {
    let mut input = input();
    let chunks: Vec<_> = (0..12)
        .map(|i| format!("range-{i:02}:中文😀证据。").repeat(16))
        .collect();
    input.source_units[0].text = chunks.concat();
    let mut calls = vec![("set_work_note", active_work("source"))];
    let mut start = 0;
    for chunk in &chunks {
        calls.push((
            "read_source",
            json!({"source_id":"source","start":start,"max_bytes":chunk.len()}),
        ));
        start += chunk.len();
    }
    let mut config = config();
    config.limits.max_history_bytes = 256;
    let model = work_script(calls);
    let journal = MemoryJournal::default();
    *journal.interrupt_after.lock().unwrap() = Some(13);
    agent::run(&input, &config, &journal, &model, &CancellationToken::new())
        .await
        .unwrap_err();
    let saved = journal.load().await.unwrap().unwrap();
    {
        let bodies = model.bodies.lock().unwrap();
        assert!(bodies.last().unwrap().to_string().contains("range-10:"));
        assert!(!bodies.last().unwrap().to_string().contains("range-00:"));
        let stable = &bodies[0]["messages"][1];
        assert!(
            bodies.iter().all(|b| &b["messages"][1] == stable),
            "dynamic progress must follow the stable prefix"
        );
    }
    for role in [Role::Main, Role::Reviewer] {
        let mut state = saved.clone();
        state.role = role;
        state.source_review = Some(agent::source_review::initialize(&input, &config).unwrap());
        state.reviewer_work = state.main_work.clone();
        state.reviewer_coverage = state.analysis.coverage.clone();
        let before = state.clone();
        let bytes = agent::request(&input, &config, &mut state).await.unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert!(
            body.to_string().contains("range-11:"),
            "latest undelivered result cannot be dropped"
        );
        assert!(!body.to_string().contains("range-00:"));
        assert_eq!(
            digest(&state.pending_coverage).unwrap(),
            digest(&before.pending_coverage).unwrap()
        );
        assert_eq!(state.turn, before.turn);
        assert_eq!(state.read_bytes, before.read_bytes);
        let mut replay: Checkpoint =
            serde_json::from_value(serde_json::to_value(&before).unwrap()).unwrap();
        assert_eq!(
            bytes,
            agent::request(&input, &config, &mut replay).await.unwrap()
        );
        let messages = body["messages"].as_array().unwrap();
        let calls: Vec<_> = messages
            .iter()
            .filter_map(|m| m["tool_calls"].as_array())
            .flatten()
            .map(|c| c["id"].clone())
            .collect();
        let results: Vec<_> = messages
            .iter()
            .filter(|m| m["role"] == "tool")
            .map(|m| m["tool_call_id"].clone())
            .collect();
        assert_eq!(calls, results, "trim only complete protocol groups");
    }
}

#[tokio::test]
#[ignore = "requires explicit frozen input, recorded requests, report path and configured runtime; no model calls"]
async fn replay_recorded_requests_with_bounded_history() {
    use std::{fs, path::PathBuf};
    let path = |key| PathBuf::from(std::env::var(key).expect("explicit replay path required"));
    let input: FrozenInput =
        serde_json::from_slice(&fs::read(path("KB_TENDER_REPLAY_INPUT")).unwrap()).unwrap();
    tools::validate_input(&input).unwrap();
    let config = Config::from_environment().unwrap();
    let mut requests: Vec<_> = fs::read_dir(path("KB_TENDER_REPLAY_REQUESTS"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter_map(|file| {
            let name = file.file_name()?.to_str()?;
            let turn = name
                .strip_prefix("turn-")?
                .split('-')
                .next()?
                .parse::<usize>()
                .ok()?;
            name.ends_with(".json").then_some((turn, file))
        })
        .collect();
    requests.sort_by_key(|(turn, _)| *turn);
    assert!(requests.len() > 1, "a trajectory needs multiple requests");
    let mut report = Vec::new();
    for (turn, file) in requests {
        let original = fs::read(&file).unwrap();
        let recorded: Value = serde_json::from_slice(&original).unwrap();
        let messages = recorded["messages"].as_array().unwrap();
        let first = messages.iter().position(|m| m["role"] == "assistant");
        let transcript = first.map_or_else(Vec::new, |i| messages[i..].to_vec());
        for role in [Role::Main, Role::Reviewer] {
            // Project historical protocol groups into the new request builder.
            // This is not a resumed old contract or a semantic extraction run.
            let mut state = Checkpoint {
                journal: Default::default(),
                input_sha256: digest(&input).unwrap(),
                config_sha256: digest(&config).unwrap(),
                turn,
                tool_calls: 0,
                read_bytes: 0,
                review_rounds: 0,
                role: role.clone(),
                analysis: Analysis::default(),
                review: None,
                review_draft: BTreeMap::new(),
                source_review: None,
                reviewer_coverage: Coverage::default(),
                pending_coverage: None,
                transcript: transcript.clone(),
                main_progress: Default::default(),
                reviewer_progress: Default::default(),
                main_work: None,
                reviewer_work: None,
                done: false,
                source_views: BTreeMap::new(),
            };
            let before = state.clone();
            let bytes = agent::request(&input, &config, &mut state).await.unwrap();
            assert!(bytes.len() <= config.limits.max_context_bytes);
            assert_eq!(
                digest(&state.analysis).unwrap(),
                digest(&before.analysis).unwrap()
            );
            assert!(state.pending_coverage.is_none());
            let mut replay = before.clone();
            assert_eq!(
                bytes,
                agent::request(&input, &config, &mut replay).await.unwrap()
            );
            let body: Value = serde_json::from_slice(&bytes).unwrap();
            let sent = body["messages"].as_array().unwrap();
            if let Some(last) = transcript.iter().rposition(|m| m["role"] == "assistant") {
                let latest = &transcript[last..];
                assert_eq!(
                    &sent[sent.len() - 1 - latest.len()..sent.len() - 1],
                    latest,
                    "the latest pending group must survive unchanged"
                );
            }
            let call_ids: Vec<_> = sent
                .iter()
                .filter_map(|m| m["tool_calls"].as_array())
                .flatten()
                .map(|c| c["id"].clone())
                .collect();
            let result_ids: Vec<_> = sent
                .iter()
                .filter(|m| m["role"] == "tool")
                .map(|m| m["tool_call_id"].clone())
                .collect();
            assert_eq!(
                call_ids, result_ids,
                "no orphan tool results after trimming"
            );
            report.push(
                json!({"turn":turn,"role":role,"original_bytes":original.len(),
                "bounded_bytes":bytes.len(),"original_messages":transcript.len(),
                "retained_messages":state.transcript.len()}),
            );
        }
    }
    assert!(
        report
            .iter()
            .any(|r| r["retained_messages"].as_u64() < r["original_messages"].as_u64())
    );
    fs::write(
        path("KB_TENDER_REPLAY_REPORT"),
        serde_json::to_vec_pretty(&json!({
            "input_sha256":digest(&input).unwrap(),"config_sha256":digest(&config).unwrap(),
            "scope":"recorded protocol projection only; no model or semantic acceptance",
            "requests":report
        }))
        .unwrap(),
    )
    .unwrap();
}

fn script() -> Script {
    let read = (
        "read_source",
        json!({"source_id":"source","start":0,"max_bytes":1024}),
    );
    let calls = vec![
        ("set_work_note", active_work("source")),
        read.clone(),
        (
            "set_disposition",
            json!({"source_id":"source","state":"non_requirement","reason":"incorrect initial interpretation"}),
        ),
        ("request_review", json!({})),
        ("set_work_note", active_work("source")),
        ("put_source_review", json!({})),
        read.clone(),
        (
            "inspect_analysis",
            json!({"view":"detail","kind":"disposition","offset":0,"limit":10}),
        ),
        (
            "put_review_finding",
            json!({"id":null,"finding":{"code":"OMITTED_REQUIREMENT","message":"遗漏附表提交义务","correction":"按所引原文补全并重新核对该字段", "affected":[],"sources":[span()]}}),
        ),
        ("put_source_review", json!({"fixture_status":"findings"})),
        ("put_record", requirement()),
        (
            "set_disposition",
            json!({"source_id":"source","state":"requirement","reason":"须知要求按附表提交"}),
        ),
        ("request_review", json!({})),
        ("set_work_note", active_work("source")),
        read,
        ("put_source_review", json!({})),
        (
            "inspect_analysis",
            json!({"view":"detail","kind":"disposition","offset":0,"limit":10}),
        ),
        (
            "inspect_analysis",
            json!({"view":"detail","kind":"all","offset":0,"limit":10}),
        ),
        ("inspect_review", json!({"offset":0,"limit":10})),
        ("delete_review_finding", json!({"id":"$fixture_finding"})),
        ("put_source_review", json!({})),
    ];
    Script {
        calls: Mutex::new(calls.into_iter().map(|(n, v)| (n.into(), v)).collect()),
        bodies: Mutex::new(vec![]),
    }
}

#[tokio::test]
async fn reviewed_conditions_do_not_assert_bidder_applicability_or_hide_unknowns() {
    for kind in ["requirement", "rule", "template"] {
        for state in ["conditional", "unknown"] {
            let mut source = input();
            source.source_units[0].text = "代理商投标时须提供授权书。".into();
            let grounds = Span {
                end: source.source_units[0].text.len(),
                ..span()
            };
            let applicability = json!({"state":state,
                "condition":"代理商投标时", "scope":"本次投标", "grounds":[grounds]});
            let mut record = requirement();
            record["sources"] = json!([grounds]);
            record["data"] = match kind {
                "rule" => json!({"kind":"rule","text":"代理商提供授权书", "scope":"本次投标",
                    "applicability":applicability}),
                "template" => json!({"kind":"template","label":"附表甲","title":"授权书",
                    "parent":null,"order":null,"purpose":"代理商投标授权格式",
                    "applicability":applicability,"regions":[{"source":grounds,
                    "role":"instruction","form_id":null,"cells":[],"instruction":"保留适用条件"}]}),
                _ => {
                    let mut data = record["data"].clone();
                    data["applicability"] = applicability.clone();
                    data
                }
            };
            let model = script();
            for (name, args) in model.calls.lock().unwrap().iter_mut() {
                if name == "put_record" {
                    *args = record.clone();
                }
                if name == "put_source_review"
                    && (args == &json!({}) || args.get("fixture_status").is_some())
                {
                    if args.get("fixture_status").is_none() {
                        args["fixture_status"] = json!("checked");
                    }
                    args["fixture_template_mappings"] = json!(kind == "template");
                    args["fixture_relationship_status"] = json!(if state == "unknown" {
                        "source_limited"
                    } else {
                        "not_required"
                    });
                }
            }
            let result = agent::run(
                &source,
                &config(),
                &MemoryJournal::default(),
                &model,
                &CancellationToken::new(),
            )
            .await
            .unwrap();
            assert_eq!(
                result.quality,
                if state == "conditional" {
                    "verified"
                } else {
                    "needs_review"
                },
                "{kind}/{state}"
            );
            assert!(result.review.findings.is_empty());
            let saved =
                serde_json::to_value(&result.analysis.records.values().next().unwrap().data)
                    .unwrap();
            assert_eq!(
                saved["applicability"], applicability,
                "review cannot resolve bidder identity or discard the source condition"
            );
        }
    }
}

#[test]
fn a_conditional_claim_still_requires_read_source_grounds_and_a_condition() {
    let source = input();
    let analysis = read_analysis(&source);
    for missing in ["condition", "scope", "grounds"] {
        let mut record = requirement()["data"].clone();
        record["applicability"]["state"] = json!("conditional");
        record["applicability"][missing] = if missing == "grounds" {
            json!([])
        } else {
            json!("")
        };
        let record = Record {
            id: "conditional-record".into(),
            sources: vec![span()],
            data: serde_json::from_value(record).unwrap(),
        };
        assert!(
            tools::validate_record(&source, &analysis, &record).is_err(),
            "{missing}"
        );
    }
}

#[tokio::test]
async fn independent_review_must_read_and_drives_repair() {
    let journal = MemoryJournal::default();
    let model = script();
    let result = agent::run(
        &input(),
        &config(),
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(result.quality, "verified");
    assert_eq!(result.analysis.records.len(), 1);
    assert!(result.review.findings.is_empty());
    let state = journal.state.lock().unwrap();
    let state = state.as_ref().unwrap();
    assert_eq!(state.review_rounds, 2);
    assert_eq!(state.turn, 21);
    // The review's premature submit was rejected, despite complete main coverage.
    let bodies = model.bodies.lock().unwrap();
    assert!(
        bodies[6]["messages"]
            .to_string()
            .contains("independently inspect")
    );
    for body in bodies.iter().filter(|b| {
        b["messages"][0]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("INDEPENDENT")
    }) {
        assert!(
            body["tools"]
                .as_array()
                .unwrap()
                .iter()
                .all(|t| t["function"]["name"] != "put_record")
        );
    }
}

#[tokio::test]
async fn resume_reuses_saved_turns_and_frozen_config() {
    let journal = MemoryJournal::default();
    *journal.interrupt_after.lock().unwrap() = Some(6);
    let model = script();
    let failed = agent::run(
        &input(),
        &config(),
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(failed.code, "INTERNAL");
    let mut changed = config();
    changed.limits.max_turns += 1;
    let failed = agent::run(
        &input(),
        &changed,
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(failed.code, "FROZEN_INPUT_DIGEST_MISMATCH");
    let calls_before = model.bodies.lock().unwrap().len();
    let mut changed_adapter = config();
    changed_adapter.runtime_adapter.push_str("-changed");
    let failed = agent::run(
        &input(),
        &changed_adapter,
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(failed.code, "FROZEN_INPUT_DIGEST_MISMATCH");
    assert_eq!(model.bodies.lock().unwrap().len(), calls_before);
    let mut old_contract = serde_json::to_value(config()).unwrap();
    old_contract
        .as_object_mut()
        .unwrap()
        .remove("runtime_adapter");
    assert!(serde_json::from_value::<agent::Config>(old_contract).is_err());
    let result = agent::run(
        &input(),
        &config(),
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(result.quality, "verified");
    assert_eq!(model.bodies.lock().unwrap().len(), 21);
    assert!(
        journal
            .sdk_turns
            .lock()
            .unwrap()
            .iter()
            .any(|turn| *turn > 1),
        "production extraction must reuse SDK multi-turn state"
    );
    assert!(
        journal
            .reservations
            .lock()
            .unwrap()
            .values()
            .all(|(_, n)| *n == 1)
    );
}

#[tokio::test]
async fn global_budget_exhaustion_retains_partial_result_without_approval() {
    let journal = MemoryJournal::default();
    let model = script();
    let mut config = config();
    config.limits.max_turns = 2;
    let error = agent::run(
        &input(),
        &config,
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "AGENT_TURN_BUDGET_EXCEEDED");
    let state = journal.state.lock().unwrap();
    let state = state.as_ref().unwrap();
    assert_eq!(state.turn, 2);
    assert!(!state.done);
    assert!(state.review.is_none());
}

fn field_relation_fixture() -> (FrozenInput, Analysis, Value) {
    let mut input = input();
    input.structured_forms = vec![json!({
        "form_definition_revision_id":"form-a", "source_unit_revision_id":"source",
        "definition":{"kind":"grid","row_count":2,"column_count":2,"widths_mm":[40,40],"cells":[
            {"row":0,"column":0,"row_span":1,"col_span":2,"text":"同号附表"},
            {"row":1,"column":0,"row_span":1,"col_span":1,"text":"分项"},
            {"row":1,"column":1,"row_span":1,"col_span":1,"text":"合计"}
        ]}
    })];
    let mut analysis = read_analysis(&input);
    analysis
        .coverage
        .form_cells
        .insert("form-a".into(), vec![(0, 4)]);
    analysis.dispositions.insert(
        "source".into(),
        Disposition {
            state: DispositionState::Requirement,
            reason: "表格字段".into(),
        },
    );
    let record:Record=serde_json::from_value(json!({"id":"template-a","sources":[span()],"data":{
        "kind":"template","label":"附表","title":"同号附表","parent":null,"order":null,"purpose":"投标格式",
        "applicability":{"state":"applicable","scope":"本项目","condition":"原文指定","grounds":[span()]},
        "regions":[{"source":span(),"role":"fixed_text","form_id":"form-a","cells":[{"row":0,"column":0}],"instruction":"保留"},
                   {"source":span(),"role":"bidder_blank","form_id":"form-a","cells":[{"row":1,"column":0},{"row":1,"column":1}],"instruction":"后续填写"}]
    }})).unwrap();
    analysis.records.insert(record.id.clone(), record);
    let args = json!({"id":null,"from":"template-a","to":"template-a",
        "from_target":{"kind":"template_cell","form_id":"form-a","row":1,"column":0},
        "to_target":{"kind":"template_cell","form_id":"form-a","row":1,"column":1},
        "kind":"aggregates","state":"explicit","scope":"当前附件","explanation":"分项汇总到合计，保留原文条件，不执行计算","grounds":[span()]});
    (input, analysis, args)
}

#[test]
fn field_relations_distinguish_cells_and_reject_whole_appendix_value_claims() {
    let (input, mut analysis, args) = field_relation_fixture();
    let mut coverage = analysis.coverage.clone();
    let id = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "put_relation",
        &args,
        16000,
    )
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let relation = &analysis.relations[&id];
    assert_eq!(
        relation.from_record_sha256,
        digest(&analysis.records["template-a"]).unwrap()
    );
    assert!(tools::gaps(&input, &analysis).is_empty());
    for change in [
        json!({"kind":"template_cell","form_id":"form-a","row":0,"column":1}), // covered part of merged cell, not anchor
        json!({"kind":"template_cell","form_id":"other-form","row":1,"column":1}),
        json!({"kind":"template_region","index":2}),
        json!({"kind":"proof","index":0}),
        json!({"kind":"record"}),
        args["from_target"].clone(),
    ] {
        let mut wrong = args.clone();
        wrong["to_target"] = change;
        assert!(
            tools::invoke(
                &input,
                &mut analysis,
                &mut coverage,
                false,
                "put_relation",
                &wrong,
                16000
            )
            .is_err()
        );
    }
    let mut unresolved = args.clone();
    unresolved["to_target"] = json!({"kind":"record"});
    unresolved["state"] = json!("unresolved");
    tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "put_relation",
        &unresolved,
        16000,
    )
    .unwrap();
}

#[test]
fn relation_prose_rejects_encoded_text_without_decoding_sources_or_literal_examples() {
    let (input, analysis, original) = field_relation_fixture();
    let source_before = digest(&input).unwrap();
    for field in ["scope", "explanation"] {
        let mut args = original.clone();
        args[field] = json!(r"1.4.1 \u8d44\u8d28\u8981\u6c42");
        let mut next = analysis.clone();
        let mut coverage = analysis.coverage.clone();
        let before = digest(&next).unwrap();
        let error = tools::invoke(
            &input,
            &mut next,
            &mut coverage,
            false,
            "put_relation",
            &args,
            16000,
        )
        .unwrap_err();
        assert!(
            error.contains(&format!("INVALID_FIELD /{field}")),
            "{error}"
        );
        assert_eq!(
            digest(&next).unwrap(),
            before,
            "bad prose cannot partially write a relation"
        );
    }
    for prose in [
        "原文条件对应证明位置。",
        r"路径 C:\users\招标\材料",
        r"匹配正则 \u[0-9a-f]{4}",
        r"保留字面示例 \u8d44\u8d28",
        r"literal example: \u8d44\u8d28",
    ] {
        let mut args = original.clone();
        args["explanation"] = json!(prose);
        let mut next = analysis.clone();
        let mut coverage = analysis.coverage.clone();
        let out = tools::invoke(
            &input,
            &mut next,
            &mut coverage,
            false,
            "put_relation",
            &args,
            16000,
        )
        .unwrap();
        assert_eq!(
            next.relations[out["id"].as_str().unwrap()].explanation,
            prose
        );
    }
    let decoded: String = serde_json::from_str(r#""\u8d44\u8d28\u8981\u6c42""#).unwrap();
    let mut args = original;
    args["explanation"] = json!(decoded);
    let mut next = analysis.clone();
    let mut coverage = analysis.coverage.clone();
    assert!(
        tools::invoke(
            &input,
            &mut next,
            &mut coverage,
            false,
            "put_relation",
            &args,
            16000
        )
        .is_ok()
    );
    assert_eq!(
        digest(&input).unwrap(),
        source_before,
        "source bytes are never rewritten"
    );
}

#[test]
fn identical_appendix_labels_do_not_authorize_foreign_template_cells() {
    let (mut input, mut analysis, mut args) = field_relation_fixture();
    let mut other_source = input.source_units[0].clone();
    other_source.source_unit_revision_id = "other-source".into();
    other_source.document_id = "other-document".into();
    input.source_units.push(other_source);
    let mut form = input.structured_forms[0].clone();
    form["form_definition_revision_id"] = json!("other-form");
    form["source_unit_revision_id"] = json!("other-source");
    input.structured_forms.push(form);
    let mut other = analysis.records["template-a"].clone();
    other.id = "template-b".into();
    other.sources[0].source_id = "other-source".into();
    if let RecordData::Template { regions, .. } = &mut other.data {
        for region in regions {
            region.source.source_id = "other-source".into();
            region.form_id = Some("other-form".into());
        }
    }
    analysis.records.insert(other.id.clone(), other);
    let mut coverage = analysis.coverage.clone();
    args["to"] = json!("template-b");
    assert!(
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            "put_relation",
            &args,
            16000
        )
        .is_err()
    );
    args["to_target"]["form_id"] = json!("other-form");
    tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "put_relation",
        &args,
        16000,
    )
    .unwrap();
    // The grid exists, but a different region of the same template owns this cell.
    if let RecordData::Template { regions, .. } =
        &mut analysis.records.get_mut("template-b").unwrap().data
    {
        regions.pop();
    }
    assert!(
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            "put_relation",
            &args,
            16000
        )
        .is_err()
    );
}

#[test]
fn endpoint_edits_invalidate_relations_and_independent_review_until_rebound() {
    let (input, mut analysis, mut args) = field_relation_fixture();
    let mut coverage = analysis.coverage.clone();
    let id = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "put_relation",
        &args,
        16000,
    )
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut review = coverage.clone();
    for kind in ["all", "relation", "disposition"] {
        tools::invoke(
            &input,
            &mut analysis,
            &mut review,
            true,
            "inspect_analysis",
            &json!({"view":"detail","kind":kind,"offset":0,"limit":100}),
            16000,
        )
        .unwrap();
    }
    assert!(tools::review_gaps(&input, &analysis, &review).is_empty());
    let mut update = serde_json::to_value(&analysis.records["template-a"]).unwrap();
    update["data"]["regions"].as_array_mut().unwrap().reverse();
    let changed = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "put_record",
        &update,
        16000,
    )
    .unwrap();
    assert_eq!(changed["relations_to_recheck"], json!([id]));
    assert!(
        tools::gaps(&input, &analysis)
            .iter()
            .any(|gap| gap["kind"] == "invalid_relation")
    );
    args["id"] = json!(id);
    tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "put_relation",
        &args,
        16000,
    )
    .unwrap();
    assert!(tools::gaps(&input, &analysis).is_empty());
    assert!(
        tools::review_gaps(&input, &analysis, &review)
            .iter()
            .any(|gap| gap["key"] == format!("relation:{id}"))
    );
    // A persisted checkpoint retains endpoint digests and does not make stale review valid.
    let restored: Analysis =
        serde_json::from_value(serde_json::to_value(&analysis).unwrap()).unwrap();
    assert!(tools::gaps(&input, &restored).is_empty());
    assert!(!tools::review_gaps(&input, &restored, &review).is_empty());
    let mut forged = args;
    forged["from_record_sha256"] = json!("forged");
    assert!(
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            "put_relation",
            &forged,
            16000
        )
        .is_err()
    );
}

#[test]
fn relation_schema_and_runtime_share_exact_endpoint_variants() {
    let (_, _, mut args) = field_relation_fixture();
    let schema = tools::schemas(false)
        .into_iter()
        .find(|v| v["function"]["name"] == "put_relation")
        .unwrap();
    let compiled = jsonschema::JSONSchema::compile(&schema["function"]["parameters"]).unwrap();
    for target in [
        json!({"kind":"record"}),
        json!({"kind":"template_region","index":0}),
        json!({"kind":"template_cell","form_id":"f","row":0,"column":0}),
        json!({"kind":"response","index":0}),
        json!({"kind":"proof","index":0}),
        json!({"kind":"criterion","index":0}),
    ] {
        args["from_target"] = target.clone();
        assert!(compiled.is_valid(&args));
        serde_json::from_value::<RelationTarget>(target).unwrap();
    }
    args["from_target"] =
        json!({"kind":"template_cell","form_id":"f","row":0,"column":0,"index":0});
    assert!(!compiled.is_valid(&args));
    assert!(serde_json::from_value::<RelationTarget>(args["from_target"].clone()).is_err());
}

#[test]
fn field_aliases_and_multi_cell_regions_cannot_fake_value_relationships() {
    let (input, mut analysis, mut args) = field_relation_fixture();
    let mut coverage = analysis.coverage.clone();
    args["to_target"] = json!({"kind":"template_region","index":1});
    assert!(
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            "put_relation",
            &args,
            16000
        )
        .is_err()
    );
    args["from_target"] = json!({"kind":"template_cell","form_id":"form-a","row":0,"column":0});
    args["to_target"] = json!({"kind":"template_region","index":0});
    assert!(
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            "put_relation",
            &args,
            16000
        )
        .is_err()
    );
}

#[test]
fn requirement_targets_keep_response_proof_and_criterion_indices_separate() {
    let input = input();
    let mut raw = requirement();
    raw["id"] = json!("r");
    raw["data"]["proofs"] = json!([{"description":"检测报告","subject":"产品","validity":"有效期内","issuer":"检测机构","condition":"原文条件","grounds":[span()],"name_required":true,"page_required":true}]);
    raw["data"]["criteria"] = json!([{"subject":"产品","aspect":"性能","operator":"≥","value":"20","unit":"Gbps","condition":"原文条件","grounds":[span()]}]);
    let record: Record = serde_json::from_value(raw).unwrap();
    for target in [
        RelationTarget::Response { index: 0 },
        RelationTarget::Proof { index: 0 },
        RelationTarget::Criterion { index: 0 },
    ] {
        relations::validate_target(&input, &record, &target).unwrap();
    }
    for target in [
        RelationTarget::Response { index: 1 },
        RelationTarget::Proof { index: 1 },
        RelationTarget::Criterion { index: 1 },
        RelationTarget::TemplateRegion { index: 0 },
    ] {
        assert!(relations::validate_target(&input, &record, &target).is_err());
    }
}

#[tokio::test]
async fn prepared_and_received_boundaries_resume_without_losing_or_repeating_model_turns() {
    for boundary in [1, 2] {
        let journal = MemoryJournal::default();
        *journal.fail_boundary_ack.lock().unwrap() = Some(boundary);
        let model = script();
        let expected_calls = model.calls.lock().unwrap().len();
        let cancel = CancellationToken::new();
        let err = agent::run(&input(), &config(), &journal, &model, &cancel)
            .await
            .unwrap_err();
        assert_eq!(err.code, "INTERNAL");
        let saved = journal.load().await.unwrap().unwrap();
        assert_eq!(saved.turn, 0);
        assert_eq!(saved.tool_calls, 0, "no tools before durable response ACK");
        assert!(saved.main_work.is_none());
        assert_eq!(saved.journal.sequence, boundary);
        assert_eq!(saved.journal.response().is_some(), boundary == 2);
        assert_eq!(
            model.bodies.lock().unwrap().len(),
            usize::from(boundary == 2)
        );
        assert_eq!(
            saved.journal.body().unwrap(),
            journal.reservations.lock().unwrap()[&0].0
        );
        // Cancellation at recovery must leave the saved response/tools untouched.
        let cancelled = CancellationToken::new();
        cancelled.cancel();
        assert!(
            agent::run(&input(), &config(), &journal, &model, &cancelled)
                .await
                .is_err()
        );
        assert_eq!(
            digest(&saved).unwrap(),
            digest(&journal.load().await.unwrap().unwrap()).unwrap()
        );
        let result = agent::run(&input(), &config(), &journal, &model, &cancel)
            .await
            .unwrap();
        assert_eq!(result.quality, "verified");
        assert_eq!(model.bodies.lock().unwrap().len(), expected_calls);
        assert!(model.calls.lock().unwrap().is_empty());
        assert_eq!(
            journal.reservations.lock().unwrap()[&0].1,
            if boundary == 1 { 2 } else { 1 }
        );
        let completed = journal.load().await.unwrap().unwrap();
        assert!(completed.journal.pending.is_none());
        assert!(completed.journal.sequence > completed.turn);
    }
}

#[tokio::test]
async fn cancellation_during_durable_boundary_stops_the_next_io_and_resumes_exactly() {
    for boundary in [1, 2] {
        let journal = MemoryJournal::default();
        let cancel = CancellationToken::new();
        *journal.cancel_boundary.lock().unwrap() = Some((boundary, cancel.clone()));
        let model = script();
        let expected_calls = model.calls.lock().unwrap().len();
        let error = agent::run(&input(), &config(), &journal, &model, &cancel)
            .await
            .unwrap_err();
        assert_eq!(error.code, "INTERNAL");
        let saved = journal.load().await.unwrap().unwrap();
        assert_eq!(saved.journal.sequence, boundary);
        assert_eq!(saved.journal.response().is_some(), boundary == 2);
        assert_eq!(saved.turn, 0);
        assert_eq!(saved.tool_calls, 0);
        assert!(saved.main_work.is_none());
        assert_eq!(
            model.bodies.lock().unwrap().len(),
            usize::from(boundary == 2)
        );
        let result = agent::run(
            &input(),
            &config(),
            &journal,
            &model,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(result.quality, "verified");
        assert_eq!(model.bodies.lock().unwrap().len(), expected_calls);
        assert_eq!(
            journal.reservations.lock().unwrap()[&0].1,
            if boundary == 1 { 2 } else { 1 }
        );
    }
}

#[tokio::test]
async fn rejected_prepared_transaction_never_calls_the_extraction_model() {
    let journal = MemoryJournal::default();
    *journal.reject_reservation.lock().unwrap() = true;
    let model = script();
    assert!(
        agent::run(
            &input(),
            &config(),
            &journal,
            &model,
            &CancellationToken::new()
        )
        .await
        .is_err()
    );
    assert!(model.bodies.lock().unwrap().is_empty());
    assert!(journal.load().await.unwrap().is_none());
    assert!(journal.reservations.lock().unwrap().is_empty());
}

#[tokio::test]
async fn review_replan_compacts_redundant_history_before_the_budget_is_full() {
    let config = config();
    let journal = fresh_review_journal().await;
    let mut state = journal.load().await.unwrap().unwrap();
    let record = state.analysis.records.values().next().unwrap().clone();
    let reference = format!("record:{}", record.id);
    state.reviewer_work.as_mut().unwrap().focus.references = vec![reference.clone()];
    state
        .reviewer_coverage
        .candidate
        .insert(reference.clone(), digest(&record).unwrap());
    let text = input().source_units[0].text.clone();
    tools::cover(
        state
            .reviewer_coverage
            .text
            .entry("source".into())
            .or_default(),
        0,
        text.len(),
    );
    let group = |id: &str, name: &str, result: serde_json::Value| {
        vec![
            json!({"role":"assistant","content":"I will inventory the remaining items before comparing them.","tool_calls":[{"id":id,"type":"function","function":{"name":name,"arguments":"{}"}}]}),
            json!({"role":"tool","tool_call_id":id,"content":json!({"ok":true,"result":result}).to_string()}),
        ]
    };
    let latest = group(
        "pending",
        "check_gaps",
        json!({"items":[],"next":0,"total":0}),
    );
    state.transcript = [
        group(
            "old-index",
            "source_index",
            json!({"items":[{"id":"source"}]}),
        ),
        group(
            "original",
            "read_source",
            json!({"source_id":"source","start":0,"end":text.len(),"text":text}),
        ),
        group(
            "candidate",
            "inspect_analysis",
            json!({"view":"detail","items":[record]}),
        ),
        latest.clone(),
    ]
    .concat();
    let normal: serde_json::Value =
        serde_json::from_slice(&agent::request(&input(), &config, &mut state).await.unwrap())
            .unwrap();
    assert!(
        normal["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["tool_call_id"] == "old-index")
    );
    let original = normal["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["tool_call_id"] == "original")
        .unwrap()
        .clone();
    state.reviewer_progress.watch.recovery = crate::agent_runtime::progress::Recovery::Replan;
    state.reviewer_progress.watch.replans = 1;
    let mut newly_read = state.clone();
    newly_read.reviewer_progress.watch.recovery = crate::agent_runtime::progress::Recovery::Running;
    let before = json!([
        state.analysis,
        state.reviewer_coverage,
        state.main_progress,
        state.reviewer_progress,
        state.pending_coverage,
        state.journal,
        state.turn,
        state.tool_calls,
        state.read_bytes
    ]);
    let recovered: serde_json::Value =
        serde_json::from_slice(&agent::request(&input(), &config, &mut state).await.unwrap())
            .unwrap();
    let messages = recovered["messages"].as_array().unwrap();
    assert!(!messages.iter().any(|m| m["tool_call_id"] == "old-index"));
    assert!(messages.contains(&original));
    assert!(state.transcript.ends_with(&latest));
    assert!(
        messages
            .iter()
            .any(|m| m["content"].as_str().is_some_and(|content| {
                serde_json::from_str::<serde_json::Value>(content)
                    .ok()
                    .is_some_and(|value| {
                        value["retained_candidate_details"]["items"]
                            .as_array()
                            .is_some_and(|items| {
                                items.iter().any(|item| {
                                    item["reference"] == reference && item["value"] == json!(record)
                                })
                            })
                    })
            }))
    );
    assert_eq!(
        json!([
            state.analysis,
            state.reviewer_coverage,
            state.main_progress,
            state.reviewer_progress,
            state.pending_coverage,
            state.journal,
            state.turn,
            state.tool_calls,
            state.read_bytes
        ]),
        before
    );
    let packet: serde_json::Value =
        serde_json::from_str(messages.last().unwrap()["content"].as_str().unwrap()).unwrap();
    assert!(
        packet["execution"]["next_action"]
            .as_str()
            .unwrap()
            .contains("single unfinished")
    );
    let stable = json!(state);
    agent::request(&input(), &config, &mut state).await.unwrap();
    assert_eq!(
        json!(state),
        stable,
        "repeated construction must not spend or reset recovery state"
    );
    let watch = json!(newly_read.reviewer_progress.watch);
    let renewed: Value = serde_json::from_slice(
        &agent::request(&input(), &config, &mut newly_read)
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(
        !renewed["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["tool_call_id"] == "old-index")
    );
    assert_eq!(json!(newly_read.reviewer_progress.watch), watch);
}

#[tokio::test]
async fn review_navigation_finishes_current_focus_before_other_pending_candidates() {
    let journal = fresh_review_journal().await;
    let mut state = journal.load().await.unwrap().unwrap();
    let mut record = state.analysis.records.values().next().unwrap().clone();
    record.id = "zz-focused".into();
    state.analysis.records.insert(record.id.clone(), record);
    state.reviewer_work.as_mut().unwrap().focus.references = vec!["record:zz-focused".into()];
    let before = json!([
        state.analysis,
        state.reviewer_progress,
        state.reviewer_coverage
    ]);
    let body: Value = serde_json::from_slice(
        &agent::request(&input(), &config(), &mut state)
            .await
            .unwrap(),
    )
    .unwrap();
    let packet: Value = serde_json::from_str(
        body["messages"].as_array().unwrap().last().unwrap()["content"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        packet["work_state"]["comparison_progress"]["focus_remaining"],
        1
    );
    assert_eq!(
        packet["work_state"]["comparison_progress"]["next_reference"],
        "record:zz-focused"
    );
    assert_eq!(
        json!([
            state.analysis,
            state.reviewer_progress,
            state.reviewer_coverage
        ]),
        before
    );
}

#[tokio::test]
async fn clean_review_checks_require_independent_evidence_and_only_complete_a_version_once() {
    let journal = fresh_review_journal().await;
    let initial = journal.load().await.unwrap().unwrap();
    let record = initial.analysis.records.values().next().unwrap();
    let reference = format!("record:{}", record.id);
    let receipt = digest(&json!([
        "review_check",
        reference,
        agent::source_review::candidate_version(&initial, &reference).unwrap()
    ]))
    .unwrap();
    let start = initial.turn;
    let mut focus = active_work("source");
    focus["focus"] = json!({"action":"review","source_spans":[],"references":[reference]});
    let check = json!({"reference":reference,"summary":"The recorded submission requirement matches the cited original clause.","sources":[span()]});
    let calls = vec![
        ("set_work_note", focus),
        ("complete_review_check", check.clone()), // no independent receipts yet
        (
            "read_source",
            json!({"source_id":"source","start":0,"max_bytes":1024}),
        ),
        (
            "inspect_analysis",
            json!({"kind":"all","view":"detail","offset":0,"limit":10}),
        ),
        ("complete_review_check", check.clone()),
    ];
    *journal.interrupt_after.lock().unwrap() = Some(start + calls.len());
    agent::run(
        &input(),
        &config(),
        &journal,
        &work_script(calls),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let saved = journal.load().await.unwrap().unwrap();
    assert!(saved.reviewer_progress.seen.contains(&receipt));
    assert!(saved.reviewer_progress.completions.contains(&receipt));
    assert_eq!(saved.reviewer_progress.watch.focus_turns, 0);
    assert!(!saved.done);
    assert!(saved.review.is_none());
    assert!(saved.review_draft.is_empty());
    assert_eq!(json!(saved.analysis), json!(initial.analysis));
    // A completed focus must lead to the remaining disposition, even though
    // the source scope still has work. Repeating the focused record is not it.
    let mut projected = saved.clone();
    let before = digest(&projected.reviewer_progress).unwrap();
    let body: Value = serde_json::from_slice(
        &agent::request(&input(), &config(), &mut projected)
            .await
            .unwrap(),
    )
    .unwrap();
    let packet: Value = serde_json::from_str(
        body["messages"].as_array().unwrap().last().unwrap()["content"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        packet["work_state"]["next_action"],
        "select_next_review_focus"
    );
    assert_eq!(
        packet["work_state"]["comparison_progress"]["focus_remaining"],
        0
    );
    assert_eq!(
        packet["work_state"]["comparison_progress"]["next_reference"],
        "disposition:source"
    );
    assert!(
        packet["execution"]["next_action"]
            .as_str()
            .unwrap()
            .contains("already has recorded outcomes")
    );
    assert_eq!(digest(&projected.reviewer_progress).unwrap(), before);
    assert!(!projected.done);
    assert!(saved.transcript.iter().any(|m| {
        m["content"]
            .as_str()
            .is_some_and(|s| s.contains("independently inspect the current candidate"))
    }));

    // A new summary (or a restart) cannot turn the same comparison into progress.
    let mut repeated = check.clone();
    repeated["summary"] = json!("Reworded conclusion about the same candidate.");
    *journal.interrupt_after.lock().unwrap() = Some(saved.turn + 1);
    agent::run(
        &input(),
        &config(),
        &journal,
        &work_script(vec![("complete_review_check", repeated)]),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let repeated = journal.load().await.unwrap().unwrap();
    assert_eq!(
        repeated.reviewer_progress.completions,
        saved.reviewer_progress.completions
    );
    assert_eq!(repeated.reviewer_progress.watch.no_progress_turns, 1);

    // A repaired candidate must be independently retrieved again before its
    // new version can get a clean comparison receipt.
    let mut changed = repeated;
    let record = changed.analysis.records.values_mut().next().unwrap();
    let RecordData::Requirement { text, .. } = &mut record.data else {
        panic!("requirement fixture")
    };
    text.push('。');
    let changed_receipt = digest(&json!([
        "review_check",
        reference,
        agent::source_review::candidate_version(&changed, &reference).unwrap()
    ]))
    .unwrap();
    *journal.interrupt_after.lock().unwrap() = Some(changed.turn + 1);
    *journal.state.lock().unwrap() = Some(changed);
    agent::run(
        &input(),
        &config(),
        &journal,
        &work_script(vec![("complete_review_check", check)]),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let changed = journal.load().await.unwrap().unwrap();
    assert!(!changed.reviewer_progress.seen.contains(&changed_receipt));
    assert!(!changed.done);
    assert!(
        !tools::schemas(false)
            .iter()
            .any(|t| t["function"]["name"] == "complete_review_check")
    );
}

#[tokio::test]
async fn source_review_advances_without_pulling_in_deferred_unknowns() {
    let mut source = input();
    source.source_units.push(Source {
        source_unit_revision_id: "later".into(),
        ordinal: 1,
        ..source.source_units[0].clone()
    });
    let mut main = active_work("source");
    main["source_scope"] = json!(["source", "later"]);
    let mut local = active_work("source");
    local["deferred_sources"] = json!(["later"]);
    let journal = MemoryJournal::default();
    let read = |id| json!({"source_id":id,"start":0,"max_bytes":1024});
    let disposition = |id| json!({"source_id":id,"state":"non_requirement","reason":"fixture source accounted for"});
    let inspect =
        |id, kind| json!({"source_id":id,"kind":kind,"view":"detail","offset":0,"limit":10});
    let calls = vec![
        ("set_work_note", main),
        ("read_source", read("source")),
        ("read_source", read("later")),
        (
            "put_record",
            json!({"id":null,"sources":[Span {source_id:"later".into(), ..span()}],
            "data":{"kind":"unresolved","problem":"Deferred source refers to unavailable evidence","affected":[],"candidates":[]}}),
        ),
        ("set_disposition", disposition("source")),
        ("set_disposition", disposition("later")),
        ("request_review", json!({})),
        ("set_work_note", local),
        ("read_source", read("source")),
        ("inspect_analysis", inspect("source", "disposition")),
        ("put_source_review", json!({})),
    ];
    *journal.interrupt_after.lock().unwrap() = Some(calls.len());
    agent::run(
        &source,
        &config(),
        &journal,
        &work_script(calls),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let saved = journal.load().await.unwrap().unwrap();
    assert_eq!(
        json!(saved.reviewer_work.as_ref().unwrap().status),
        "active"
    );
    assert_eq!(
        saved.reviewer_work.as_ref().unwrap().source_scope,
        vec!["later"]
    );
    assert_eq!(saved.reviewer_work.as_ref().unwrap().pending_refs.len(), 1);
    assert!(!saved.reviewer_coverage.text.contains_key("later"));
    assert!(
        saved.review.is_none(),
        "local completion cannot approve the deferred source"
    );
    assert!(!saved.done);
    *journal.interrupt_after.lock().unwrap() = None;

    let result = agent::run(
        &source,
        &config(),
        &journal,
        &work_script(vec![
            ("set_work_note", active_work("later")),
            ("read_source", read("later")),
            ("inspect_analysis", inspect("later", "all")),
            ("inspect_analysis", inspect("later", "disposition")),
            ("put_source_review", json!({"fixture_status":"checked","fixture_relationship_status":"source_limited","fixture_unresolved":true})),
        ]),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(
        result.analysis.records.len(),
        1,
        "the unresolved outcome was retained"
    );
    assert!(result.review.coverage.text.contains_key("later"));
}

#[tokio::test]
async fn reviewer_recovery_exposes_clean_completion_without_granting_approval() {
    use crate::agent_runtime::progress::Recovery;
    let journal = MemoryJournal::default();
    agent::run(
        &input(),
        &config(),
        &journal,
        &script(),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    let mut state = journal.load().await.unwrap().unwrap();
    state.role = Role::Reviewer;
    state.done = false;
    state.review = None;
    state.transcript.clear();
    state.pending_coverage = None;
    state.reviewer_work = Some(serde_json::from_value(active_work("source")).unwrap());
    state.reviewer_progress.watch.recovery = Recovery::Replan;
    let before = digest(&state.reviewer_coverage).unwrap();
    let body: Value = serde_json::from_slice(
        &agent::request(&input(), &config(), &mut state)
            .await
            .unwrap(),
    )
    .unwrap();
    let packet: Value = serde_json::from_str(
        body["messages"].as_array().unwrap().last().unwrap()["content"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(packet["work_state"]["gap_counts"], json!({}));
    assert_eq!(
        packet["work_state"]["next_action"],
        "compare_then_judge_source"
    );
    let recovery = packet["execution"]["next_action"].as_str().unwrap();
    assert!(recovery.contains("put_source_review"));
    assert!(recovery.contains("host advances tasks"));
    assert_eq!(digest(&state.reviewer_coverage).unwrap(), before);
    assert!(!state.done);
    assert!(state.review.is_none());
    assert!(state.review_draft.is_empty());

    // An empty structural checklist cannot hide the other role's execution
    // blocker or turn a failed run into a clean review.
    state.main_progress.watch.recovery = Recovery::Blocked;
    state
        .main_progress
        .block(vec!["source".into()], "dependencies".into());
    let body: Value = serde_json::from_slice(
        &agent::request(&input(), &config(), &mut state)
            .await
            .unwrap(),
    )
    .unwrap();
    let packet: Value = serde_json::from_str(
        body["messages"].as_array().unwrap().last().unwrap()["content"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        packet["work_state"]["next_action"],
        "resolve_execution_blockers"
    );
    assert!(!state.done);
}

#[tokio::test]
async fn request_work_checklist_projects_current_receipts_without_committing_them() {
    let journal = MemoryJournal::default();
    *journal.interrupt_after.lock().unwrap() = Some(2);
    agent::run(
        &input(),
        &config(),
        &journal,
        &script(),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let mut state = journal.load().await.unwrap().unwrap();
    assert!(state.analysis.coverage.text.is_empty());
    assert!(
        state
            .pending_coverage
            .as_ref()
            .unwrap()
            .text
            .contains_key("source")
    );
    let before = digest(&state.analysis).unwrap();
    let receipts_before = digest(&state.pending_coverage).unwrap();
    let body: Value = serde_json::from_slice(
        &agent::request(&input(), &config(), &mut state)
            .await
            .unwrap(),
    )
    .unwrap();
    let tail: Value = serde_json::from_str(
        body["messages"].as_array().unwrap().last().unwrap()["content"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let checklist = &tail["work_state"];
    assert_eq!(checklist["gap_counts"]["missing_disposition"], 1);
    assert_eq!(checklist["next_action"], "resolve_work_gaps");
    assert!(
        checklist["gap_counts"].get("unread_source").is_none(),
        "already delivered request text should not invite another identical read"
    );
    assert!(checklist["gap_counts"].get("pending_delivery").is_none());
    assert!(serde_json::to_vec(checklist).unwrap().len() <= config().limits.max_tool_result_bytes);
    assert_eq!(digest(&state.analysis).unwrap(), before);
    assert_eq!(digest(&state.pending_coverage).unwrap(), receipts_before);
    assert!(
        tools::validate_span(&input(), &state.analysis.coverage, &span()).is_err(),
        "request metadata is not evidence authorization"
    );
    // The other role still needs its own reading, even when main's pending
    // receipts have subsequently been confirmed.
    state.analysis.coverage = state.pending_coverage.take().unwrap();
    state.role = Role::Reviewer;
    state.source_review = Some(agent::source_review::initialize(&input(), &config()).unwrap());
    state.reviewer_work = state.main_work.clone();
    state.transcript.clear();
    let body: Value = serde_json::from_slice(
        &agent::request(&input(), &config(), &mut state)
            .await
            .unwrap(),
    )
    .unwrap();
    let tail: Value = serde_json::from_str(
        body["messages"].as_array().unwrap().last().unwrap()["content"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(tail["work_state"]["gap_counts"]["unread_source"], 1);
    assert_eq!(tail["work_state"]["next_action"], "resolve_work_gaps");
    assert!(state.reviewer_coverage.text.is_empty());
}

#[tokio::test]
async fn stalled_work_is_persisted_and_independent_sources_continue_after_restart() {
    use crate::agent_runtime::progress::Recovery;
    let mut source = input();
    source.source_units.push(Source {
        source_unit_revision_id: "independent".into(),
        ordinal: 1,
        ..source.source_units[0].clone()
    });
    let mut config = config();
    config.limits.max_no_progress_turns = 2;
    config.limits.max_focus_replans = 1;
    let journal = MemoryJournal::default();
    *journal.interrupt_after.lock().unwrap() = Some(4);
    let mut rename = active_work("source");
    rename["note"] = json!("same work, new note and objective");
    rename["objective"] = json!("renamed work");
    let calls = vec![
        ("set_work_note", active_work("source")),
        (
            "inspect_analysis",
            json!({"view":"index","kind":"all","offset":0,"limit":10}),
        ),
        ("set_work_note", rename),
        (
            "search_sources",
            json!({"query":"absent","offset":0,"limit":10}),
        ),
    ];
    agent::run(
        &source,
        &config,
        &journal,
        &work_script(calls),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let state = journal.load().await.unwrap().unwrap();
    assert_eq!(state.main_progress.watch.recovery, Recovery::Blocked);
    assert_eq!(state.main_progress.blockers.len(), 1);
    assert_eq!(state.main_progress.blockers[0].scope, vec!["source"]);
    assert!(
        state.analysis.dispositions.is_empty(),
        "runtime failures cannot become source uncertainty"
    );
    let restored = serde_json::from_value(json!(state)).unwrap();
    *journal.state.lock().unwrap() = Some(restored);
    *journal.interrupt_after.lock().unwrap() = Some(9);
    let model = work_script(vec![
        ("set_work_note", active_work("source")),
        ("set_work_note", active_work("independent")),
        (
            "read_source",
            json!({"source_id":"independent","start":0,"max_bytes":1024}),
        ),
        ("check_gaps", json!({"scope":"work","offset":0,"limit":10})),
        ("request_review", json!({})),
    ]);
    agent::run(
        &source,
        &config,
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let state = journal.load().await.unwrap().unwrap();
    assert_eq!(
        state.main_work.as_ref().unwrap().source_scope,
        vec!["independent"]
    );
    assert!(state.analysis.coverage.text.contains_key("independent"));
    assert_eq!(state.main_progress.blockers[0].watch.replans, 1);
    assert_eq!(state.main_progress.blockers.len(), 1);
    assert!(!state.done);
    assert_eq!(state.role, Role::Main);
    let last: Value = serde_json::from_str(
        state.transcript.last().unwrap()["content"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert!(
        last["error"]
            .as_str()
            .unwrap()
            .contains("execution blockers")
    );
    assert!(
        model
            .bodies
            .lock()
            .unwrap()
            .iter()
            .any(|body| body.to_string().contains("dependencies are unchanged"))
    );
}

#[test]
fn omitted_progress_limits_freeze_central_defaults() {
    let config = config();
    let mut value = json!(config.limits);
    for key in [
        "max_no_progress_turns",
        "max_focus_turns",
        "max_focus_replans",
    ] {
        value.as_object_mut().unwrap().remove(key);
    }
    let limits: Limits = serde_json::from_value(value).unwrap();
    assert_eq!(limits.max_no_progress_turns, 6);
    assert_eq!(limits.max_focus_turns, 24);
    assert_eq!(limits.max_focus_replans, 2);
    let frozen = Config::with_provider(config.provider, limits).unwrap();
    assert!(json!(frozen)["limits"]["max_no_progress_turns"].is_number());
}

#[tokio::test]
async fn all_blocked_sources_stop_before_reserving_a_pointless_handoff() {
    let config = config();
    let journal = MemoryJournal::default();
    let mut calls = vec![("set_work_note", active_work("source"))];
    calls.extend((0..30).map(|_| ("check_gaps", json!({"scope":"work","offset":0,"limit":10}))));
    let model = work_script(calls);
    let error = agent::run(
        &input(),
        &config,
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert!(
        error
            .message
            .contains("no independent source scope remains")
    );
    let state = journal.load().await.unwrap().unwrap();
    assert!(state.turn < config.limits.max_turns);
    assert_eq!(state.turn, 18);
    assert_eq!(model.bodies.lock().unwrap().len(), state.turn);
    assert_eq!(journal.reservations.lock().unwrap().len(), state.turn);
    assert_eq!(state.main_progress.blockers.len(), 1);
    assert!(
        state.journal.pending.is_none(),
        "stop at committed boundary, before another reservation"
    );
    assert!(!state.done);

    let mut changed = state.clone();
    changed.analysis.dispositions.insert(
        "source".into(),
        serde_json::from_value(json!({"state":"requirement","reason":"newly saved outcome"}))
            .unwrap(),
    );
    let changed_journal = MemoryJournal::default();
    *changed_journal.state.lock().unwrap() = Some(changed);
    *changed_journal.interrupt_after.lock().unwrap() = Some(state.turn + 1);
    let resume = work_script(vec![("set_work_note", active_work("source"))]);
    let error = agent::run(
        &input(),
        &config,
        &changed_journal,
        &resume,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "INTERNAL");
    assert_eq!(resume.bodies.lock().unwrap().len(), 1);
    let mut reviewer = state.clone();
    reviewer.role = Role::Reviewer;
    reviewer.reviewer_progress = state.main_progress.clone();
    *journal.state.lock().unwrap() = Some(reviewer);
    let no_model = work_script(vec![]);
    let error = agent::run(
        &input(),
        &config,
        &journal,
        &no_model,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert!(
        error
            .message
            .contains("no independent source scope remains")
    );
    assert!(no_model.bodies.lock().unwrap().is_empty());

    // An older compatible checkpoint can already contain a received response.
    // Commit it normally, then stop before creating any additional reservation.
    let mut received = state.clone();
    let body = agent::request(&input(), &config, &mut received)
        .await
        .unwrap();
    received
        .journal
        .prepare_session(
            &body,
            2,
            1,
            config.limits.max_turns - received.turn,
            config.limits.max_context_bytes,
        )
        .unwrap();
    received
        .journal
        .prepare(received.turn, "main", &body)
        .unwrap();
    received
        .journal
        .responded(ChatTurn {
            content: String::new(),
            finish_reason: "tool_calls".into(),
            usage: None,
            tool_calls: vec![ChatToolCall {
                id: "saved-gap-check".into(),
                name: "check_gaps".into(),
                arguments: json!({"scope":"execution","offset":0,"limit":10}).to_string(),
            }],
        })
        .unwrap();
    *journal.state.lock().unwrap() = Some(received);
    let no_model = work_script(vec![]);
    let error = agent::run(
        &input(),
        &config,
        &journal,
        &no_model,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert!(
        error
            .message
            .contains("no independent source scope remains")
    );
    let committed = journal.load().await.unwrap().unwrap();
    assert_eq!(committed.turn, state.turn + 1);
    assert!(committed.journal.pending.is_none());
    assert!(no_model.bodies.lock().unwrap().is_empty());
    assert_eq!(journal.reservations.lock().unwrap().len(), state.turn);
    assert_eq!(json!(committed.analysis), json!(state.analysis));
    assert_eq!(
        json!(committed.main_progress.blockers),
        json!(state.main_progress.blockers)
    );
}
