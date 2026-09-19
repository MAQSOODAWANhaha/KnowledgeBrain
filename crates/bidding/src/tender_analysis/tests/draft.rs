use super::*;
use crate::agent_error::AgentError;
use crate::docx_composition::compiler;
use crate::tender_analysis::agent::{self, Model, Role};
use crate::tender_analysis::draft::{
    BodyStatus, ChapterPurpose, DraftPlanItem, DraftStatus, OmitReason, bind_source_ids,
    draft_should_publish_partial, filled_templates_present, mark_window_coverage, merge_regions,
    plan_ready, reject_blank_erasing_source, split_windows,
};
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use knowledge::models::{ChatToolCall, ChatTurn};
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::io::Read;
use std::sync::Mutex;

fn draft_input() -> FrozenInput {
    FrozenInput {
        schema_version: 1,
        project_id: "project".into(),
        document_set_id: "set".into(),
        documents: vec![json!({"id":"document","file_name":"招标文件.pdf"})],
        document_relations: vec![],
        decisions: vec![],
        structured_forms: vec![],
        source_units: vec![Source {
            source_unit_revision_id: "source".into(),
            document_id: "document".into(),
            text: "投标函为固定格式。投标人名称：　　。".into(),
            locator: json!({"page_ordinal":1}),
            ordinal: 0,
        }],
    }
}

fn cover(input: &FrozenInput, analysis: &mut Analysis) {
    mark_window_coverage(
        input,
        &mut analysis.coverage,
        &input
            .source_units
            .iter()
            .map(|source| source.source_unit_revision_id.clone())
            .collect::<Vec<_>>(),
    );
}

fn outline_item_json(id: Value, parent: Value, order: usize, title: &str, grounds: Value) -> Value {
    json!({
        "id": id,
        "parent": parent,
        "order": order,
        "title": title,
        "prescribed": true,
        "grounds": grounds,
        "requirement_ids": [],
        "purpose": "response",
        "format_refs": []
    })
}

fn complete_discovery(input: &FrozenInput, state: &mut agent::Checkpoint) {
    let ids: Vec<String> = input
        .source_units
        .iter()
        .map(|source| source.source_unit_revision_id.clone())
        .collect();
    mark_window_coverage(input, &mut state.analysis.coverage, &ids);
    let mut text = serde_json::Map::new();
    let mut empty_sources = Vec::new();
    for source in &input.source_units {
        if source.text.is_empty() {
            empty_sources.push(source.source_unit_revision_id.clone());
        } else {
            text.insert(
                source.source_unit_revision_id.clone(),
                json!([[0, source.text.len()]]),
            );
        }
    }
    let mut forms = serde_json::Map::new();
    for form in &input.structured_forms {
        let Some(form_id) = form["form_definition_revision_id"].as_str() else {
            continue;
        };
        let n = form["definition"]["cells"]
            .as_array()
            .map(Vec::len)
            .unwrap_or(0);
        if n > 0 {
            forms.insert(form_id.to_string(), json!([[0, n]]));
        }
    }
    let mut metadata = serde_json::Map::new();
    for (kind, values) in [
        ("documents", &input.documents),
        ("document_relations", &input.document_relations),
        ("decisions", &input.decisions),
    ] {
        if !values.is_empty() {
            state
                .analysis
                .coverage
                .metadata
                .insert(kind.into(), vec![(0, values.len())]);
            metadata.insert(kind.into(), json!([[0, values.len()]]));
        }
    }
    let args = json!({
        "text": text,
        "forms": forms,
        "metadata":metadata,"empty_sources": empty_sources,
        "requirements": [],
        "references": [],
        "issues": [],
        "review_fragments": []
    });
    crate::tender_analysis::outline_flow::apply(input, state, "submit_outline_scan", &args, 64_000)
        .expect("submit_outline_scan");
    assert!(
        crate::tender_analysis::outline_flow::scan_complete(input, &state.analysis.outline),
        "discovery must cover every frozen source"
    );
    state.analysis.outline.phase = crate::tender_analysis::outline_flow::Phase::Outline;
    state.outline_run.phase = crate::tender_analysis::outline_flow::Phase::Outline;
}

fn finish_checked_outline(input: &FrozenInput, config: &Config, state: &mut agent::Checkpoint) {
    let mut finished = agent::apply(input, config, state, "finish_outline", &json!({})).unwrap();
    loop {
        let packet_id = finished["next_packet_id"]
            .as_str()
            .or_else(|| finished["packet_id"].as_str())
            .expect("check packet id")
            .to_string();
        let snapshot = finished
            .get("packet_snapshot_sha256")
            .cloned()
            .unwrap_or_else(|| finished["snapshot_sha256"].clone());
        crate::tender_analysis::outline_flow::read_check_evidence(input, state);
        finished = agent::apply(
            input,
            config,
            state,
            "submit_outline_check",
            &json!({
                "packet_id": packet_id,
                "packet_snapshot_sha256": snapshot,
                "issues": []
            }),
        )
        .unwrap();
        if finished["checked"] == true {
            break;
        }
    }
    crate::tender_analysis::draft::after_batch(input, state, false, false).unwrap();
}

fn save_required(
    state: &mut agent::Checkpoint,
    id: &str,
    description: &str,
    source_id: &str,
    end: usize,
) {
    state.analysis.outline.requirements.insert(
        id.into(),
        crate::tender_analysis::outline_flow::SubmissionNeed {
            description: description.into(),
            kind: crate::tender_analysis::outline_flow::NeedKind::Submission,
            submission_name: Some(description.into()),
            classification_reason: String::new(),
            format_required: false,
            applicability: crate::tender_analysis::outline_flow::Applicability::Required,
            condition: String::new(),
            grounds: vec![Span {
                source_id: source_id.into(),
                start: 0,
                end,
                view_id: None,
                grid_cell: None,
            }],
            format_grounds: vec![],
            order_constraints: vec![],
        },
    );
}

fn requirement_unmapped(input: &FrozenInput, state: &agent::Checkpoint) -> bool {
    crate::tender_analysis::outline_flow::blockers(input, state).contains(&"B_REQUIREMENT_UNMAPPED")
}

fn filled_item(template_id: &str) -> DraftPlanItem {
    DraftPlanItem {
        grounds: vec![],
        requirement_ids: vec![],
        id: "ch1".into(),
        parent: None,
        order: 0,
        title: "投标函".into(),
        prescribed: true,
        source_ids: vec!["source".into()],
        windows: vec![vec!["source".into()]],
        window_index: 0,
        template_id: Some(template_id.into()),
        status: DraftStatus::Filled,
        purpose: ChapterPurpose::Response,
        format_refs: vec![],
        body_status: BodyStatus::Empty,
        omit_reason: None,
        preserved: vec![],
    }
}

fn template_record(input: &FrozenInput) -> Record {
    let text = &input.source_units[0].text;
    let span = Span {
        source_id: "source".into(),
        start: 0,
        end: text.len(),
        view_id: None,
        grid_cell: None,
    };
    Record {
        id: "tpl".into(),
        sources: vec![span.clone()],
        data: RecordData::Template {
            label: "投标函".into(),
            title: "投标函".into(),
            parent: None,
            order: Some(0),
            purpose: "招标指定格式".into(),
            applicability: Applicability {
                state: ApplicabilityState::Applicable,
                scope: "本项目".into(),
                condition: "原文指定".into(),
                grounds: vec![span.clone()],
            },
            regions: vec![TemplateRegion {
                source: Span {
                    source_id: "source".into(),
                    start: 0,
                    end: "投标函为固定格式。".len(),
                    view_id: None,
                    grid_cell: None,
                },
                role: RegionRole::FixedText,
                form_id: None,
                header_rows: None,
                cells: vec![],
                blank_ranges: vec![],
                instruction: String::new(),
            }],
        },
    }
}

/// 一个填好了表格的章：grid 的 header 行数由 `header_rows` 声明。
fn grid_fill_fixture(header_rows: Option<usize>) -> (FrozenInput, AnalysisResult) {
    let text = "开标一览表 项目 报价 总价 壹万元";
    let mut input = draft_input();
    input.source_units[0].text = text.into();
    input.structured_forms = vec![json!({
        "form_definition_revision_id":"form-1",
        "source_unit_revision_id":"source",
        "definition":{
            "schema_version":3,"kind":"grid","title":"开标一览表",
            "row_count":2,"column_count":2,"widths_mm":[60,60],
            "cells":[
                {"row":0,"column":0,"row_span":1,"col_span":1,"text":"项目"},
                {"row":0,"column":1,"row_span":1,"col_span":1,"text":"报价"},
                {"row":1,"column":0,"row_span":1,"col_span":1,"text":"总价"},
                {"row":1,"column":1,"row_span":1,"col_span":1,"text":"壹万元"}
            ]
        }
    })];
    let mut result = draft_result(&input);
    let span = Span {
        source_id: "source".into(),
        start: 0,
        end: text.len(),
        view_id: None,
        grid_cell: None,
    };
    result
        .analysis
        .coverage
        .text
        .insert("source".into(), vec![(0, text.len())].into_iter().collect());
    let record = result.analysis.records.get_mut("tpl").unwrap();
    record.sources = vec![span.clone()];
    let RecordData::Template { regions, .. } = &mut record.data else {
        panic!("模板记录");
    };
    *regions = vec![TemplateRegion {
        source: span,
        role: RegionRole::FixedText,
        form_id: Some("form-1".into()),
        header_rows,
        cells: vec![
            Cell { row: 0, column: 0 },
            Cell { row: 0, column: 1 },
            Cell { row: 1, column: 0 },
            Cell { row: 1, column: 1 },
        ],
        blank_ranges: vec![],
        instruction: String::new(),
    }];
    result.review.analysis_sha256 = digest(&result.analysis).unwrap();
    (input, result)
}

fn draft_result(input: &FrozenInput) -> AnalysisResult {
    let mut analysis = Analysis::default();
    cover(input, &mut analysis);
    let record = template_record(input);
    analysis.records.insert(record.id.clone(), record);
    analysis.draft_plan = vec![filled_item("tpl")];
    let sha = digest(&analysis).unwrap();
    AnalysisResult {
        schema_version: 2,
        frozen_input_sha256: digest(input).unwrap(),
        analysis,
        review: Review {
            analysis_sha256: sha,
            coverage: Coverage::default(),
            findings: vec![],
            draft: true,
            ..Default::default()
        },
        quality: "needs_review".into(),
        source_views: BTreeMap::new(),
    }
}

/// 扫描 PL/pgSQL 的 IF / END IF（跳过注释和字符串），返回每次开合的字节区间。
fn plpgsql_if_spans(body: &str) -> Vec<(usize, usize)> {
    let bytes = body.as_bytes();
    let mut i = 0;
    let mut stack = Vec::new();
    let mut spans = Vec::new();
    let is_ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    while i < bytes.len() {
        match bytes[i] {
            b'-' if bytes.get(i + 1) == Some(&b'-') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i = i.saturating_add(2);
            }
            b'\'' => {
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\'' {
                        if bytes.get(i + 1) == Some(&b'\'') {
                            i += 2;
                            continue;
                        }
                        i += 1;
                        break;
                    }
                    i += 1;
                }
            }
            _ => {
                let rest = &bytes[i..];
                let keyword = |word: &[u8]| {
                    rest.len() >= word.len()
                        && rest[..word.len()].eq_ignore_ascii_case(word)
                        && (i == 0 || !is_ident(bytes[i - 1]))
                        && rest.get(word.len()).is_none_or(|c| !is_ident(*c))
                };
                if keyword(b"END") {
                    let mut j = i + 3;
                    while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                        j += 1;
                    }
                    let tail = &bytes[j..];
                    if tail.len() >= 2
                        && tail[..2].eq_ignore_ascii_case(b"IF")
                        && tail.get(2).is_none_or(|c| !is_ident(*c))
                    {
                        let start = stack
                            .pop()
                            .unwrap_or_else(|| panic!("unmatched END IF at {i}"));
                        spans.push((start, j + 2));
                        i = j + 2;
                        continue;
                    }
                }
                if keyword(b"ELSIF") {
                    i += 5;
                    continue;
                }
                if keyword(b"IF") {
                    stack.push(i);
                    i += 2;
                    continue;
                }
                i += 1;
            }
        }
    }
    assert!(stack.is_empty(), "publish_v4 unclosed IF at {:?}", stack);
    spans
}

#[test]
fn sql_draft_publish_requires_non_omitted_plan() {
    let sql = include_str!("../../../../../migrations/bidding_v2_baseline.sql");
    let start = sql
        .find("CREATE FUNCTION kb_bid_v2_publish_requirement_set_v4")
        .expect("publish v4");
    let rest = &sql[start + 1..];
    let end = rest
        .find("CREATE FUNCTION")
        .map(|i| start + 1 + i)
        .unwrap_or(sql.len());
    let body = &sql[start..end];
    let spans = plpgsql_if_spans(body);
    let draft_if = body
        .find("p_compiled#>'{analysis_result,review,draft}' = 'true'")
        .expect("draft IF");
    let (draft_start, draft_end) = spans
        .iter()
        .copied()
        .find(|(from, to)| *from <= draft_if && *to > draft_if)
        .expect("draft IF must have matching END IF");
    let draft_block = &body[draft_start..draft_end];
    assert!(
        draft_block.contains("END IF"),
        "draft review IF must close before shared INSERT"
    );
    assert!(
        !draft_block.contains("source partition incomplete"),
        "draft THEN/ELSE must not require analysis.dispositions coverage"
    );
    let disp = body
        .find("source partition incomplete")
        .expect("disposition gate");
    assert!(
        disp >= draft_end,
        "disposition completeness must sit after the draft/official END IF"
    );
    let gate = &body[disp.saturating_sub(500)..disp];
    assert!(
        gate.contains("review,draft") && gate.contains("IS DISTINCT FROM 'true'"),
        "disposition completeness must skip draft: {gate}"
    );
    let advance = body
        .find("IF can_publish AND p_compiled#>'{analysis_result,review,draft}' IS DISTINCT FROM 'true'")
        .expect("draft must skip disposition-set advance");
    assert!(advance > disp);
}

#[test]
fn plan_ready_requires_a_non_omitted_chapter() {
    assert!(!plan_ready(&[]));
    let omitted = DraftPlanItem {
        grounds: vec![],
        requirement_ids: vec![],
        id: "x".into(),
        parent: None,
        order: 0,
        title: "缺".into(),
        prescribed: false,
        source_ids: vec![],
        windows: vec![],
        window_index: 0,
        template_id: None,
        status: DraftStatus::Omitted,
        purpose: ChapterPurpose::Response,
        format_refs: vec![],
        body_status: BodyStatus::Empty,
        omit_reason: Some(OmitReason::BindFailed),
        preserved: vec![],
    };
    assert!(plan_ready(std::slice::from_ref(&omitted)));
    let mut pending = omitted;
    pending.status = DraftStatus::Pending;
    pending.omit_reason = None;
    assert!(plan_ready(&[pending]));
}

#[test]
fn two_windows_fill_only_on_last() {
    let mut input = draft_input();
    input.source_units[0].text = "字".repeat(5000);
    input.source_units.push(Source {
        source_unit_revision_id: "p2".into(),
        document_id: "document".into(),
        text: "字".repeat(5000),
        locator: json!({"page_ordinal":2}),
        ordinal: 1,
    });
    let mut limits = config().limits;
    limits.draft_path = true;
    let config = Config::with_provider(config().provider.clone(), limits).unwrap();
    let mut state = journal_state(&input, &config);
    enter_outline(&input, &mut state);
    let windows = split_windows(&input, &["source".into(), "p2".into()]);
    assert_eq!(windows.len(), 2);
    state.analysis.draft_plan.push(DraftPlanItem {
        grounds: vec![],
        requirement_ids: vec![],
        id: "ch".into(),
        parent: None,
        order: 0,
        title: "章".into(),
        prescribed: true,
        source_ids: vec!["source".into(), "p2".into()],
        windows: windows.clone(),
        window_index: 0,
        template_id: None,
        status: DraftStatus::Pending,
        purpose: ChapterPurpose::Response,
        format_refs: vec![],
        body_status: BodyStatus::Empty,
        omit_reason: None,
        preserved: vec![],
    });
    state.draft_active_id = Some("ch".into());
    mark_window_coverage(&input, &mut state.analysis.coverage, &windows[0]);
    mark_window_coverage(&input, &mut state.analysis.coverage, &windows[1]);
    let region = |id: &str, end: usize| {
        json!({
            "source":{"source_id":id,"start":0,"end":end,"view_id":null,"grid_cell":null},
            "role":"fixed_text","form_id":null,"cells":[],"blank_ranges":[],"instruction":""
        })
    };
    agent::apply(
        &input,
        &config,
        &mut state,
        "put_chapter_template",
        &json!({
            "chapter_id":"ch","id":null,"title":"章","purpose":"格式",
            "regions":[region("source", input.source_units[0].text.len())]
        }),
    )
    .unwrap();
    assert_eq!(state.analysis.draft_plan[0].status, DraftStatus::Pending);
    assert_eq!(state.analysis.draft_plan[0].window_index, 1);
    let template_id = state.analysis.draft_plan[0].template_id.clone();
    agent::apply(
        &input,
        &config,
        &mut state,
        "put_chapter_template",
        &json!({
            "chapter_id":"ch","id":template_id,"title":"章","purpose":"格式",
            "regions":[region("p2", input.source_units[1].text.len())]
        }),
    )
    .unwrap();
    assert_eq!(state.analysis.draft_plan[0].status, DraftStatus::Filled);
    assert_eq!(
        match &state.analysis.records.values().next().unwrap().data {
            RecordData::Template { regions, .. } => regions.len(),
            _ => 0,
        },
        2
    );
}

#[test]
fn bind_uses_title_containment_not_builtin_lexicon() {
    let input = draft_input();
    assert_eq!(
        bind_source_ids(&input, "投标函", &[]).unwrap(),
        vec!["source".to_string()]
    );
    assert_eq!(
        bind_source_ids(&input, "不存在的组成项", &[]).unwrap_err(),
        OmitReason::BindFailed
    );
    let extra = bind_source_ids(&input, "不存在的组成项", &["固定格式".into()]).unwrap();
    assert_eq!(extra, vec!["source".to_string()]);
}

#[test]
fn bind_matches_title_across_line_wrap() {
    let mut input = draft_input();
    input.source_units[0].text = "法定代表\n人授权委托书(附件 1C) 格式见后。".into();
    assert_eq!(
        bind_source_ids(&input, "法定代表人授权委托书(附件 1C)", &[]).unwrap(),
        vec!["source".to_string()]
    );
}

#[test]
fn outline_accepts_punctuation_in_material_titles() {
    let input = draft_input();
    let mut limits = config().limits;
    limits.draft_path = true;
    let config = Config::with_provider(config().provider.clone(), limits).unwrap();
    let mut state = journal_state(&input, &config);
    enter_outline(&input, &mut state);
    mark_window_coverage(&input, &mut state.analysis.coverage, &["source".into()]);
    let end = input.source_units[0].text.len();
    agent::apply(
        &input,
        &config,
        &mut state,
        "put_outline_item",
        &json!({
            "id":null,"parent":null,"order":0,
            "title":"设备安装、调试、培训方案",
            "prescribed":true,
            "requirement_ids":[],"purpose":"response","format_refs":[],
            "grounds":[{"source_id":"source","start":0,"end":end,"view_id":null,"grid_cell":null}]
        }),
    )
    .unwrap();
    agent::apply(
        &input,
        &config,
        &mut state,
        "put_outline_item",
        &json!({
            "id":null,"parent":null,"order":1,
            "title":"法定代表人（单位负责人）身份证明(附件 1B)",
            "prescribed":true,
            "requirement_ids":[],"purpose":"response","format_refs":[],
            "grounds":[{"source_id":"source","start":0,"end":end,"view_id":null,"grid_cell":null}]
        }),
    )
    .unwrap();
}

#[test]
fn outline_rejects_duplicate_sibling_order() {
    let input = draft_input();
    let mut limits = config().limits;
    limits.draft_path = true;
    let config = Config::with_provider(config().provider.clone(), limits).unwrap();
    let mut state = journal_state(&input, &config);
    enter_outline(&input, &mut state);
    mark_window_coverage(&input, &mut state.analysis.coverage, &["source".into()]);
    let end = input.source_units[0].text.len();
    let put = |state: &mut agent::Checkpoint,
               id: Value,
               parent: Value,
               order: usize,
               title: &str| {
        agent::apply(
            &input,
            &config,
            state,
            "put_outline_item",
            &json!({
                "id":id,"parent":parent,"order":order,"title":title,"prescribed":true,
                "requirement_ids":[],"purpose":"response","format_refs":[],
                "grounds":[{"source_id":"source","start":0,"end":end,"view_id":null,"grid_cell":null}]
            }),
        )
    };
    put(&mut state, Value::Null, Value::Null, 1, "商务文件").unwrap();
    let parent = state.analysis.draft_plan[0].id.clone();
    put(&mut state, Value::Null, json!(parent.clone()), 1, "投标函").unwrap();
    let err = put(
        &mut state,
        Value::Null,
        json!(parent.clone()),
        1,
        "授权委托书",
    )
    .unwrap_err();
    assert!(err.contains("sibling order"), "{err}");
    let child_id = state.analysis.draft_plan[1].id.clone();
    put(
        &mut state,
        json!(child_id),
        json!(parent.clone()),
        1,
        "投标函",
    )
    .unwrap();
    put(&mut state, Value::Null, json!(parent), 2, "授权委托书").unwrap();
    put(&mut state, Value::Null, Value::Null, 1, "技术文件").unwrap_err();
    put(&mut state, Value::Null, Value::Null, 2, "技术文件").unwrap();
}

#[test]
fn saved_requirement_stays_unmapped_until_a_chapter_cites_its_id() {
    let input = rooted_catalog_input();
    let mut limits = config().limits;
    limits.draft_path = true;
    let config =
        Config::with_provider_for(config().provider.clone(), limits, Some(&input)).unwrap();
    let mut state = journal_state(&input, &config);
    enter_outline(&input, &mut state);
    assert!(
        state.analysis.outline.requirements.is_empty(),
        "heading_path must not invent a submission list"
    );
    assert!(!requirement_unmapped(&input, &state));
    let qual = &input.source_units[1];
    save_required(
        &mut state,
        "qual",
        "资格审查资料",
        &qual.source_unit_revision_id,
        qual.text.len(),
    );
    let grounds = |index: usize| {
        let source = &input.source_units[index];
        json!([{
            "source_id":source.source_unit_revision_id,"start":0,"end":source.text.len(),
            "view_id":null,"grid_cell":null
        }])
    };
    agent::apply(
        &input,
        &config,
        &mut state,
        "put_outline_item",
        &json!({"id":"vol","parent":null,"order":0,"title":"商务文件","prescribed":true,"requirement_ids":[],"purpose":"response","format_refs":[],"grounds":grounds(0)}),
    )
    .unwrap();
    assert!(
        requirement_unmapped(&input, &state),
        "a parent title does not cover a saved requirement"
    );
    agent::apply(
        &input,
        &config,
        &mut state,
        "put_outline_item",
        &json!({"id":"qual","parent":"vol","order":0,"title":"投标人资格文件","prescribed":true,"requirement_ids":["qual"],"purpose":"response","format_refs":[],"grounds":grounds(1)}),
    )
    .unwrap();
    assert!(!requirement_unmapped(&input, &state));
    finish_checked_outline(&input, &config, &mut state);
    assert_eq!(
        state.draft_stage,
        crate::tender_analysis::draft::DraftStage::Published
    );
}

/// 组成句、投诉流程和附件号不会变成必备章。缺口只来自已保存且仍适用的要求。
#[test]
fn wording_does_not_invent_requirements_and_unmapped_ids_block() {
    let mut input = draft_input();
    input.source_units[0].text =
        "投标文件由下列文件组成：投标函、法定代表人身份证明、投标保证金。投诉人的名称、地址。附件 1A。"
            .into();
    let mut limits = config().limits;
    limits.draft_path = true;
    let config =
        Config::with_provider_for(config().provider.clone(), limits, Some(&input)).unwrap();
    let mut state = journal_state(&input, &config);
    enter_outline(&input, &mut state);
    assert!(state.analysis.outline.requirements.is_empty());
    assert!(!requirement_unmapped(&input, &state));
    let end = input.source_units[0].text.len();
    save_required(&mut state, "letter", "投标函", "source", end);
    save_required(&mut state, "identity", "法定代表人身份证明", "source", end);
    save_required(&mut state, "bond", "投标保证金", "source", end);
    let grounds = json!([{
        "source_id":"source","start":0,"end":end,
        "view_id":null,"grid_cell":null
    }]);
    let write = |state: &mut Checkpoint, id: &str, order: usize, title: &str, requirement: &str| {
        agent::apply(
            &input,
            &config,
            state,
            "put_outline_item",
            &json!({"id":id,"parent":null,"order":order,"title":title,"prescribed":true,"requirement_ids":[requirement],"purpose":"response","format_refs":[],"grounds":grounds}),
        )
        .unwrap();
    };
    write(&mut state, "ch-letter", 0, "投标函", "letter");
    write(&mut state, "ch-id", 1, "身份文件", "identity");
    assert!(requirement_unmapped(&input, &state));
    state
        .analysis
        .outline
        .requirements
        .get_mut("bond")
        .unwrap()
        .applicability = crate::tender_analysis::outline_flow::Applicability::NotApplicable;
    assert!(!requirement_unmapped(&input, &state));
    finish_checked_outline(&input, &config, &mut state);
    assert_eq!(
        state.draft_stage,
        crate::tender_analysis::draft::DraftStage::Published
    );
}

/// 宿主不按组成句重排章节。模型写下的 order 保持不变，句子本身也不生成要求。
#[test]
fn host_keeps_written_chapter_order() {
    let mut input = draft_input();
    input.source_units[0].text = "投标文件由下列文件组成：投标函、法定代表人身份证明。".into();
    let mut limits = config().limits;
    limits.draft_path = true;
    let config =
        Config::with_provider_for(config().provider.clone(), limits, Some(&input)).unwrap();
    let mut state = journal_state(&input, &config);
    enter_outline(&input, &mut state);
    let grounds = json!([{
        "source_id":"source","start":0,"end":input.source_units[0].text.len(),
        "view_id":null,"grid_cell":null
    }]);
    for (id, order, title) in [
        ("ch-id", 0, "法定代表人身份证明"),
        ("ch-letter", 1, "投标函"),
    ] {
        agent::apply(
            &input,
            &config,
            &mut state,
            "put_outline_item",
            &json!({"id":id,"parent":null,"order":order,"title":title,"prescribed":true,"requirement_ids":[],"purpose":"response","format_refs":[],"grounds":grounds}),
        )
        .unwrap();
    }
    let order = |id: &str| {
        state
            .analysis
            .draft_plan
            .iter()
            .find(|item| item.id == id)
            .expect(id)
            .order
    };
    assert_eq!(order("ch-id"), 0);
    assert_eq!(order("ch-letter"), 1);
    assert!(state.analysis.outline.requirements.is_empty());
    assert!(!requirement_unmapped(&input, &state));
}

/// 解析出的表名不是指定格式，不能单独变成必备章。
#[test]
fn form_titles_do_not_become_required_chapters() {
    let mut input = draft_input();
    input.structured_forms = vec![json!({
        "source_unit_revision_id":"source",
        "form_definition_revision_id":"form-1",
        "definition":{"title":"开标一览表","row_count":1,"column_count":1,"cells":[{"row":0,"column":0,"text":"报价"}]}
    })];
    let mut limits = config().limits;
    limits.draft_path = true;
    let config =
        Config::with_provider_for(config().provider.clone(), limits, Some(&input)).unwrap();
    let mut state = journal_state(&input, &config);
    enter_outline(&input, &mut state);
    assert!(state.analysis.outline.requirements.is_empty());
    assert!(!requirement_unmapped(&input, &state));
    save_required(
        &mut state,
        "price-form",
        "开标一览表",
        "source",
        input.source_units[0].text.len(),
    );
    assert!(requirement_unmapped(&input, &state));
}

/// 原文没点名子结构就不强造三级：平铺大纲直接闭合。
#[test]
fn outline_without_substructure_evidence_publishes_flat() {
    let input = large_input_with_named_items();
    let mut limits = config().limits;
    limits.draft_path = true;
    let config =
        Config::with_provider_for(config().provider.clone(), limits, Some(&input)).unwrap();
    let mut state = journal_state(&input, &config);
    enter_outline(&input, &mut state);
    let ids: Vec<String> = input
        .source_units
        .iter()
        .map(|source| source.source_unit_revision_id.clone())
        .collect();
    mark_window_coverage(&input, &mut state.analysis.coverage, &ids);
    put_named_outline(&input, &config, &mut state, 10);
    put_named_outline(&input, &config, &mut state, 11);
    finish_checked_outline(&input, &config, &mut state);
    assert_eq!(
        state.draft_stage,
        crate::tender_analysis::draft::DraftStage::Published
    );
    assert!(
        state
            .analysis
            .draft_plan
            .iter()
            .all(|item| item.status == DraftStatus::Pending),
        "没有缺口就不该记 omitted"
    );
}

#[test]
fn fill_prefers_format_page_over_composition_list() {
    let mut input = draft_input();
    input.source_units = vec![
        Source {
            source_unit_revision_id: "list".into(),
            document_id: "document".into(),
            text: "并由下列文件组成：(1) 投标函(附件 1A)、法定代表人身份证明(附件 1B)。".into(),
            locator: json!({"page_ordinal": 1}),
            ordinal: 0,
        },
        Source {
            source_unit_revision_id: "form".into(),
            document_id: "document".into(),
            text: "投标函的格式(1A) 致：招标人。固定文字。".into(),
            locator: json!({"page_ordinal": 2}),
            ordinal: 1,
        },
    ];
    let mut limits = config().limits;
    limits.draft_path = true;
    let config = Config::with_provider(config().provider.clone(), limits).unwrap();
    let mut state = journal_state(&input, &config);
    enter_outline(&input, &mut state);
    mark_window_coverage(
        &input,
        &mut state.analysis.coverage,
        &["list".into(), "form".into()],
    );
    for (order, title, source_id) in [
        (0usize, "投标函(附件 1A)", "form"),
        (1usize, "法定代表人身份证明(附件 1B)", "list"),
    ] {
        let source = input
            .source_units
            .iter()
            .find(|s| s.source_unit_revision_id == source_id)
            .unwrap();
        agent::apply(
            &input,
            &config,
            &mut state,
            "put_outline_item",
            &json!({
                "id":null,"parent":null,"order":order,"title":title,"prescribed":true,
                "requirement_ids":[],"purpose":"response","format_refs":[],
                "grounds":[{"source_id":source_id,"start":0,"end":source.text.len(),"view_id":null,"grid_cell":null}]
            }),
        )
        .unwrap();
    }
    assert!(crate::tender_analysis::draft::assign_next_chapter(
        &input, &mut state
    ));
    assert_eq!(state.draft_active_id.as_deref(), Some("chapter-1"));
    assert_eq!(
        state.main_work.as_ref().unwrap().source_scope,
        vec!["form".to_string()],
        "fill window must be the form page, not the composition list"
    );
}

#[test]
fn fill_omission_does_not_relabel_bound_chapter_as_bind_failed() {
    let input = draft_input();
    let mut limits = config().limits;
    limits.draft_path = true;
    let config = Config::with_provider(config().provider.clone(), limits).unwrap();
    let mut state = journal_state(&input, &config);
    state.draft_stage = crate::tender_analysis::draft::DraftStage::Fill;
    state.draft_active_id = Some("outline-1".into());
    mark_window_coverage(&input, &mut state.analysis.coverage, &["source".into()]);
    state.analysis.draft_plan.push(DraftPlanItem {
        grounds: vec![],
        requirement_ids: vec![],
        id: "outline-1".into(),
        parent: None,
        order: 0,
        title: "投标函".into(),
        prescribed: true,
        source_ids: vec!["source".into()],
        windows: vec![vec!["source".into()]],
        window_index: 0,
        template_id: None,
        status: DraftStatus::Pending,
        purpose: ChapterPurpose::Response,
        format_refs: vec![],
        body_status: BodyStatus::Empty,
        omit_reason: None,
        preserved: vec![],
    });
    agent::apply(
        &input,
        &config,
        &mut state,
        "skip_chapter_content",
        &json!({
            "chapter_id":"outline-1",
            "reason":"bind_failed",
            "summary":"招标文件未给出该章格式",
            "grounds":[{"source_id":"source","start":0,"end":3,"view_id":null,"grid_cell":null}]
        }),
    )
    .unwrap();
    assert_eq!(state.analysis.draft_plan[0].status, DraftStatus::Pending);
    assert_eq!(
        state.analysis.draft_plan[0].omit_reason,
        Some(OmitReason::WindowExceeded)
    );
}

#[test]
fn long_source_and_form_chunks_continue_without_double_counting() {
    let text = "投标".repeat(3000);
    let ranges = crate::tender_analysis::draft::utf8_account_ranges(
        &text,
        crate::tender_analysis::draft::DRAFT_WINDOW_BYTES,
    );
    assert!(ranges.len() > 1, "{ranges:?}");
    let mut covered = 0usize;
    for (index, (start, end)) in ranges.iter().enumerate() {
        assert!(text.is_char_boundary(*start) && text.is_char_boundary(*end));
        assert!(end > start);
        if index == 0 {
            assert_eq!(*start, 0);
        } else {
            assert_eq!(*start, covered, "account ranges must not overlap");
        }
        covered = *end;
    }
    assert_eq!(covered, text.len());
    let definition = json!({
        "row_count": 20,
        "column_count": 2,
        "cells": []
    });
    let slices = crate::tender_analysis::draft::form_account_slices(&definition, 8);
    assert!(slices.len() > 1);
    assert_eq!(slices[0].2, 2, "continuation carries the header row");
    assert_eq!(
        slices[0].0, 0,
        "first chunk accounts for headers exactly once"
    );
    assert_eq!(slices.last().unwrap().1, 40);
}

#[test]
fn split_windows_keeps_tables_with_neighbors() {
    let mut input = draft_input();
    input.source_units.push(Source {
        source_unit_revision_id: "t1".into(),
        document_id: "document".into(),
        text: String::new(),
        locator: json!({"page_ordinal":1}),
        ordinal: 1,
    });
    let long = "字".repeat(5000);
    input.source_units[0].text = long.clone();
    input.source_units.push(Source {
        source_unit_revision_id: "p2".into(),
        document_id: "document".into(),
        text: long,
        locator: json!({"page_ordinal":2}),
        ordinal: 2,
    });
    let windows = split_windows(&input, &["source".into(), "t1".into(), "p2".into()]);
    assert_eq!(windows[0], vec!["source".to_string(), "t1".to_string()]);
    assert_eq!(windows[1], vec!["p2".to_string()]);
}

#[test]
fn merge_regions_overrides_same_span() {
    let span = Span {
        source_id: "source".into(),
        start: 0,
        end: 3,
        view_id: None,
        grid_cell: None,
    };
    let a = TemplateRegion {
        source: span.clone(),
        role: RegionRole::FixedText,
        form_id: None,
        header_rows: None,
        cells: vec![],
        blank_ranges: vec![],
        instruction: "a".into(),
    };
    let mut b = a.clone();
    b.instruction = "b".into();
    let merged = merge_regions(&[a], vec![b]);
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].instruction, "b");
}

#[test]
fn blank_cannot_erase_nonempty_source() {
    let input = draft_input();
    let region = TemplateRegion {
        source: Span {
            source_id: "source".into(),
            start: 0,
            end: input.source_units[0].text.len(),
            view_id: None,
            grid_cell: None,
        },
        role: RegionRole::BidderBlank,
        form_id: None,
        header_rows: None,
        cells: vec![],
        blank_ranges: vec![],
        instruction: String::new(),
    };
    assert!(reject_blank_erasing_source(&input, &[region]).is_err());
}

#[test]
fn partial_blank_keeps_unselected_source() {
    let input = draft_input();
    let text = &input.source_units[0].text;
    let start = text.find("：").unwrap() + "：".len();
    let region = TemplateRegion {
        source: Span {
            source_id: "source".into(),
            start: 0,
            end: text.len(),
            view_id: None,
            grid_cell: None,
        },
        role: RegionRole::BidderBlank,
        form_id: None,
        header_rows: None,
        cells: vec![],
        blank_ranges: vec![crate::template_grid::CellTextRange {
            row: 0,
            column: 0,
            start,
            end: text.len(),
        }],
        instruction: String::new(),
    };
    reject_blank_erasing_source(&input, &[region]).unwrap();
}

#[test]
fn blank_range_off_char_boundary_is_rejected() {
    let input = draft_input();
    let text = &input.source_units[0].text;
    assert!(text.is_char_boundary(0));
    assert!(!text.is_char_boundary(1));
    let region = TemplateRegion {
        source: Span {
            source_id: "source".into(),
            start: 0,
            end: text.len(),
            view_id: None,
            grid_cell: None,
        },
        role: RegionRole::BidderBlank,
        form_id: None,
        header_rows: None,
        cells: vec![],
        blank_ranges: vec![crate::template_grid::CellTextRange {
            row: 0,
            column: 0,
            start: 1,
            end: text.len(),
        }],
        instruction: String::new(),
    };
    let err = reject_blank_erasing_source(&input, &[region]).unwrap_err();
    assert!(
        err.contains("UTF-8"),
        "host must reject mid-rune blank ranges, not panic: {err}"
    );
}

#[test]
fn validate_draft_basis_accepts_plan_without_rechecker_or_disposition() {
    let input = draft_input();
    let result = draft_result(&input);
    assert!(result.analysis.dispositions.is_empty());
    assert!(result.review.findings.is_empty());
    crate::docx_composition::validate_draft_basis(&input, &result).unwrap();
    crate::docx_composition::validate_basis(&input, &result).unwrap_err();
}

#[test]
fn validate_draft_basis_rejects_empty_plan() {
    let input = draft_input();
    let mut result = draft_result(&input);
    result.analysis.draft_plan.clear();
    result.review.analysis_sha256 = digest(&result.analysis).unwrap();
    assert!(crate::docx_composition::validate_draft_basis(&input, &result).is_err());
}

#[test]
fn filled_templates_must_still_exist() {
    let input = draft_input();
    let mut result = draft_result(&input);
    result.analysis.records.clear();
    assert!(!filled_templates_present(&result.analysis));
}

#[test]
fn compile_draft_renders_fixed_source_text() {
    let input = draft_input();
    let result = draft_result(&input);
    let draft = crate::docx_composition::synthesize_draft_document(&input, &result).unwrap();
    let compiled = compiler::compile(&input, &result, &draft, 1_000_000).unwrap();
    use std::io::Read;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&compiled.docx)).unwrap();
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    assert!(
        xml.contains("投标函为固定格式"),
        "draft docx must keep tender fixed text: {xml}"
    );
}

#[test]
fn compile_draft_skips_plan_complete_for_unfilled_template() {
    let input = draft_input();
    let mut result = draft_result(&input);
    let leftover = Record {
        id: "leftover".into(),
        sources: vec![Span {
            source_id: "source".into(),
            start: 0,
            end: 3,
            view_id: None,
            grid_cell: None,
        }],
        data: RecordData::Template {
            label: "未填完".into(),
            title: "未填完".into(),
            parent: None,
            order: Some(1),
            purpose: "残留".into(),
            applicability: Applicability {
                state: ApplicabilityState::Applicable,
                scope: "ch2".into(),
                condition: "原文指定".into(),
                grounds: vec![],
            },
            regions: vec![],
        },
    };
    result
        .analysis
        .records
        .insert(leftover.id.clone(), leftover);
    result.analysis.draft_plan.push(DraftPlanItem {
        grounds: vec![],
        requirement_ids: vec![],
        id: "ch2".into(),
        parent: None,
        order: 1,
        title: "授权".into(),
        prescribed: true,
        source_ids: vec![],
        windows: vec![],
        window_index: 0,
        template_id: Some("leftover".into()),
        status: DraftStatus::Omitted,
        purpose: ChapterPurpose::Response,
        format_refs: vec![],
        body_status: BodyStatus::Empty,
        omit_reason: Some(OmitReason::Deadline),
        preserved: vec![],
    });
    result.review.analysis_sha256 = digest(&result.analysis).unwrap();
    let draft = crate::docx_composition::synthesize_draft_document(&input, &result).unwrap();
    assert!(
        crate::docx_composition::validate_plan(&result, &draft).is_err(),
        "leftover Template must fail official plan_complete"
    );
    let compiled = compiler::compile(&input, &result, &draft, 1_000_000).unwrap();
    let xml = docx_document_xml(&compiled.docx);
    assert!(
        xml.contains("投标函为固定格式"),
        "draft compile must skip plan_complete and keep filled text: {xml}"
    );
}

#[test]
fn synthesize_keeps_heading_for_omitted_parent() {
    let input = draft_input();
    let mut result = draft_result(&input);
    result.analysis.draft_plan = vec![
        DraftPlanItem {
            grounds: vec![],
            requirement_ids: vec![],
            id: "vol-price".into(),
            parent: None,
            order: 3,
            title: "报价文件".into(),
            prescribed: true,
            source_ids: vec![],
            windows: vec![],
            window_index: 0,
            template_id: None,
            status: DraftStatus::Omitted,
            purpose: ChapterPurpose::Response,
            format_refs: vec![],
            body_status: BodyStatus::Empty,
            omit_reason: Some(OmitReason::WindowExceeded),
            preserved: vec![],
        },
        {
            let mut child = filled_item("tpl");
            child.id = "price-9".into();
            child.parent = Some("vol-price".into());
            child.order = 1;
            child.title = "开标价格表".into();
            child
        },
    ];
    result.review.analysis_sha256 = digest(&result.analysis).unwrap();
    let draft = crate::docx_composition::synthesize_draft_document(&input, &result).unwrap();
    assert!(draft.sections.contains_key("vol-price"));
    assert_eq!(
        draft.sections["price-9"].parent.as_deref(),
        Some("vol-price")
    );
    assert!(draft.sections["vol-price"].content.is_empty());
    compiler::compile(&input, &result, &draft, 1_000_000).unwrap();
}

/// 阶段一止于骨架：零 Filled 章、且叶子章绑不上源，也必须编译出可编辑 Word。
#[test]
fn compile_draft_skeleton_without_any_filled_chapter() {
    let input = draft_input();
    let mut result = draft_result(&input);
    result.analysis.draft_plan = vec![
        DraftPlanItem {
            grounds: vec![],
            requirement_ids: vec![],
            id: "vol-tech".into(),
            parent: None,
            order: 0,
            title: "技术文件".into(),
            prescribed: true,
            source_ids: vec![],
            windows: vec![],
            window_index: 0,
            template_id: None,
            status: DraftStatus::Pending,
            purpose: ChapterPurpose::Response,
            format_refs: vec![],
            body_status: BodyStatus::Empty,
            omit_reason: None,
            preserved: vec![],
        },
        DraftPlanItem {
            grounds: vec![],
            requirement_ids: vec![],
            id: "tech-1".into(),
            parent: Some("vol-tech".into()),
            order: 0,
            title: "施工组织设计".into(),
            prescribed: true,
            source_ids: vec![],
            windows: vec![],
            window_index: 0,
            template_id: None,
            status: DraftStatus::Pending,
            purpose: ChapterPurpose::Response,
            format_refs: vec![],
            body_status: BodyStatus::Empty,
            omit_reason: None,
            preserved: vec![],
        },
    ];
    result.review.analysis_sha256 = digest(&result.analysis).unwrap();
    let draft = crate::docx_composition::synthesize_draft_document(&input, &result).unwrap();
    assert!(draft.sections["vol-tech"].content.is_empty());
    assert!(
        matches!(
            draft.sections["tech-1"].content.as_slice(),
            [crate::docx_composition::Content::BidderBlank]
        ),
        "空叶子章要显式声明由投标人书写，才能编译出 heading"
    );
    let compiled = compiler::compile(&input, &result, &draft, 1_000_000).unwrap();
    let xml = docx_document_xml(&compiled.docx);
    assert!(
        xml.contains("技术文件") && xml.contains("施工组织设计"),
        "骨架编译必须保留每一章的标题: {xml}"
    );
}

/// 填一部分不得让未填章从 Word 里消失（A28）。
#[test]
fn synthesize_keeps_heading_for_unfilled_sibling() {
    let input = draft_input();
    let mut result = draft_result(&input);
    result.analysis.draft_plan.push(DraftPlanItem {
        grounds: vec![],
        requirement_ids: vec![],
        id: "ch-pending".into(),
        parent: None,
        order: 1,
        title: "法定代表人身份证明".into(),
        prescribed: true,
        source_ids: vec!["source".into()],
        windows: vec![vec!["source".into()]],
        window_index: 0,
        template_id: None,
        status: DraftStatus::Pending,
        purpose: ChapterPurpose::Response,
        format_refs: vec![],
        body_status: BodyStatus::Empty,
        omit_reason: None,
        preserved: vec![],
    });
    result.review.analysis_sha256 = digest(&result.analysis).unwrap();
    let draft = crate::docx_composition::synthesize_draft_document(&input, &result).unwrap();
    assert!(draft.sections.contains_key("ch1"), "已填章照常出稿");
    assert!(
        draft.sections.contains_key("ch-pending"),
        "未填章必须留下空 heading，不能让用户以为章节被删了"
    );
    let compiled = compiler::compile(&input, &result, &draft, 1_000_000).unwrap();
    let xml = docx_document_xml(&compiled.docx);
    assert!(
        xml.contains("法定代表人身份证明"),
        "未填章的标题必须进文档: {xml}"
    );
}

/// 回读种子编译：用户已写的正文原样回去，没写的章留空 heading（A9/A28）。
#[test]
fn readback_seed_compiles_user_text_verbatim_and_keeps_pending_headings() {
    use crate::tender_analysis::readback::{Preserved, PreservedCell};
    let input = draft_input();
    let mut result = draft_result(&input);
    let prior = result.analysis.draft_plan.clone();
    result.analysis.draft_plan = vec![
        DraftPlanItem {
            grounds: vec![],
            requirement_ids: vec![],
            id: prior[0].id.clone(),
            parent: None,
            order: 0,
            title: prior[0].title.clone(),
            prescribed: true,
            source_ids: vec!["source".into()],
            windows: vec![vec!["source".into()]],
            window_index: 0,
            template_id: None,
            status: DraftStatus::Filled,
            purpose: ChapterPurpose::Response,
            format_refs: vec![],
            body_status: BodyStatus::Empty,
            omit_reason: None,
            preserved: vec![
                Preserved::Paragraphs {
                    unit_keys: vec!["story:3".into()],
                    paragraphs: vec!["我方承诺按招标文件要求履约。".into()],
                },
                Preserved::Table {
                    widths_twips: vec![2000, 2000],
                    header_rows: 0,
                    unit_key: "story:4".into(),
                    row_count: 1,
                    column_count: 2,
                    cells: vec![
                        PreservedCell {
                            row: 0,
                            column: 0,
                            row_span: 1,
                            col_span: 1,
                            text: "总价".into(),
                        },
                        PreservedCell {
                            row: 0,
                            column: 1,
                            row_span: 1,
                            col_span: 1,
                            text: "壹万元".into(),
                        },
                    ],
                },
            ],
        },
        DraftPlanItem {
            grounds: vec![],
            requirement_ids: vec![],
            id: "readback-1".into(),
            parent: None,
            order: 1,
            title: "用户新增的一章".into(),
            prescribed: false,
            source_ids: vec![],
            windows: vec![],
            window_index: 0,
            template_id: None,
            status: DraftStatus::Pending,
            purpose: ChapterPurpose::Response,
            format_refs: vec![],
            body_status: BodyStatus::Empty,
            omit_reason: None,
            preserved: vec![],
        },
    ];
    result.review.analysis_sha256 = digest(&result.analysis).unwrap();
    let draft = crate::docx_composition::synthesize_draft_document(&input, &result).unwrap();
    assert!(
        matches!(
            draft.sections[&prior[0].id].content.as_slice(),
            [crate::docx_composition::Content::Preserved { blocks }] if blocks.len() == 2
        ),
        "已有正文的章走保留原语，不重填"
    );
    let compiled = compiler::compile(&input, &result, &draft, 1_000_000).unwrap();
    let xml = docx_document_xml(&compiled.docx);
    assert!(
        xml.contains("我方承诺按招标文件要求履约。") && xml.contains("壹万元"),
        "用户正文与表格必须逐字回到新版本: {xml}"
    );
    assert!(
        xml.contains("用户新增的一章"),
        "没填的新增章要留空 heading: {xml}"
    );
}

/// §7.1 填章校验清单：引文非空、grid header policy、锚点不越界不重叠、弃章有据。
#[test]
fn fill_checklist_refuses_unfounded_regions_and_unfounded_omissions() {
    let (input, _) = grid_fill_fixture(Some(1));
    let mut limits = config().limits;
    limits.draft_path = true;
    let config = Config::with_provider(config().provider.clone(), limits).unwrap();
    let mut state = journal_state(&input, &config);
    state.draft_stage = crate::tender_analysis::draft::DraftStage::Fill;
    state.draft_active_id = Some("ch-form".into());
    mark_window_coverage(&input, &mut state.analysis.coverage, &["source".into()]);
    state.analysis.draft_plan.push(DraftPlanItem {
        grounds: vec![],
        requirement_ids: vec![],
        id: "ch-form".into(),
        parent: None,
        order: 0,
        title: "开标一览表".into(),
        prescribed: true,
        source_ids: vec!["source".into()],
        windows: vec![vec!["source".into()]],
        window_index: 0,
        template_id: None,
        status: DraftStatus::Pending,
        purpose: ChapterPurpose::Response,
        format_refs: vec![],
        body_status: BodyStatus::Empty,
        omit_reason: None,
        preserved: vec![],
    });
    let end = input.source_units[0].text.len();
    let region = |extra: Value| {
        let mut region = json!({
            "source":{"source_id":"source","start":0,"end":end,"view_id":null,"grid_cell":null},
            "role":"fixed_text","form_id":"form-1","cells":[{"row":0,"column":0}],
            "instruction":""
        });
        for (key, value) in extra.as_object().unwrap() {
            region[key] = value.clone();
        }
        json!({"chapter_id":"ch-form","id":"tpl-form","title":"开标一览表","purpose":"指定表",
               "regions":[region]})
    };
    let refuse = |args: Value| {
        let mut probe = state.clone();
        agent::apply(&input, &config, &mut probe, "put_chapter_template", &args).unwrap_err()
    };
    assert!(
        refuse(region(json!({}))).contains("header policy"),
        "grid 必须声明 header 行数"
    );
    assert!(
        refuse(region(json!({"header_rows":2}))).contains("whole form"),
        "header 不能吃掉整张表"
    );
    assert!(
        refuse(region(
            json!({"header_rows":1,"cells":[{"row":9,"column":9}]})
        ))
        .contains("foreign or covered"),
        "锚点不能越界"
    );
    assert!(
        refuse(region(
            json!({"header_rows":1,"source":{"source_id":"source","start":0,"end":0,"view_id":null,"grid_cell":null}})
        ))
        .contains("must be less than end"),
        "引文不能为空"
    );
    agent::apply(
        &input,
        &config,
        &mut state,
        "put_chapter_template",
        &region(json!({"header_rows":1})),
    )
    .expect("合规的 grid 区间应当写入");
    assert!(
        agent::apply(
            &input,
            &config,
            &mut state.clone(),
            "skip_chapter_content",
            &json!({"chapter_id":"ch-form","reason":"not_applicable","summary":"不适用","grounds":[]}),
        )
        .unwrap_err()
        .contains("grounds"),
        "弃章必须有据"
    );
}

/// 带表格的章必须能编译：header policy 由填章时声明，宿主搬运（A29）。
#[test]
fn filled_grid_chapter_compiles_with_the_declared_header_policy() {
    let (input, result) = grid_fill_fixture(Some(1));
    let draft = crate::docx_composition::synthesize_draft_document(&input, &result).unwrap();
    let [crate::docx_composition::Content::Template { headers, .. }] =
        draft.sections["ch1"].content.as_slice()
    else {
        panic!("填好的章应当是模板正文");
    };
    assert_eq!(headers.len(), 1, "grid 必须带 header policy");
    assert_eq!(headers[0].header_rows, 1);
    let compiled = compiler::compile(&input, &result, &draft, 1_000_000).unwrap();
    assert!(docx_document_xml(&compiled.docx).contains("报价"));
}

/// 没有 header policy 的 grid 章不能悄悄按默认值出稿，只能明确报错。
#[test]
fn grid_chapter_without_a_header_policy_refuses_to_compile() {
    let (input, result) = grid_fill_fixture(None);
    let error = crate::docx_composition::synthesize_draft_document(&input, &result).unwrap_err();
    assert!(error.contains("header policy"), "{error}");
}

/// Over-budget compile fails without dropping filled bodies (§6 / T5).
#[test]
fn oversized_draft_refuses_to_drop_filled_bodies() {
    let input = draft_input();
    let mut result = draft_result(&input);
    result.analysis.draft_plan.push(DraftPlanItem {
        grounds: vec![],
        requirement_ids: vec![],
        id: "ch2".into(),
        parent: None,
        order: 1,
        title: "法定代表人身份证明".into(),
        prescribed: true,
        source_ids: vec!["source".into()],
        windows: vec![vec!["source".into()]],
        window_index: 0,
        template_id: Some("tpl".into()),
        status: DraftStatus::Filled,
        purpose: ChapterPurpose::Response,
        format_refs: vec![],
        body_status: BodyStatus::Empty,
        omit_reason: None,
        preserved: vec![],
    });
    result.review.analysis_sha256 = digest(&result.analysis).unwrap();
    let full = crate::tender_analysis::draft::compile_draft(&input, &result, 1_000_000).unwrap();
    assert!(full.degraded.is_empty());
    let budget = full.compiled.docx.len() - 1;
    match crate::tender_analysis::draft::compile_draft(&input, &result, budget) {
        Ok(_) => panic!("over-budget compile must fail without dropping filled bodies"),
        Err(error) => assert!(error.contains("exceeds configured byte budget"), "{error}"),
    }
    assert_eq!(
        result
            .analysis
            .draft_plan
            .iter()
            .filter(|item| item.status == DraftStatus::Filled)
            .count(),
        2,
        "failed compile must leave the filled plan untouched"
    );
}

/// 换章不清零 Job 级停滞账本，只重置本章回合数（A4）。
#[test]
fn rotating_chapters_keeps_the_job_level_stall_account() {
    let input = draft_input();
    let mut limits = config().limits;
    limits.draft_path = true;
    let config = Config::with_provider(config().provider.clone(), limits).unwrap();
    let mut state = journal_state(&input, &config);
    state.draft_stage = crate::tender_analysis::draft::DraftStage::Fill;
    state.analysis.draft_plan = vec![DraftPlanItem {
        grounds: vec![],
        requirement_ids: vec![],
        id: "ch-next".into(),
        parent: None,
        order: 0,
        title: "法定代表人身份证明".into(),
        prescribed: true,
        source_ids: vec!["source".into()],
        windows: vec![vec!["source".into()]],
        window_index: 0,
        template_id: None,
        status: DraftStatus::Pending,
        purpose: ChapterPurpose::Response,
        format_refs: vec![],
        body_status: BodyStatus::Empty,
        omit_reason: None,
        preserved: vec![],
    }];
    state.main_progress.watch.no_progress_turns = 4;
    state.main_progress.watch.replans = 2;
    state.main_progress.watch.focus_turns = 9;
    assert!(crate::tender_analysis::draft::assign_next_chapter(
        &input, &mut state
    ));
    assert_eq!(
        state.main_progress.watch.focus_turns, 0,
        "本章回合数随换章清零"
    );
    assert_eq!(
        (
            state.main_progress.watch.no_progress_turns,
            state.main_progress.watch.replans
        ),
        (4, 2),
        "全局停滞保护必须跨章累计，否则卡住的 Job 能烧满预算"
    );
}

/// 「停止填充」在章界生效：当前章写完就不再派下一章，剩下的章仍是 Pending——在稿
/// 子里是空 heading，用户再发一次填充能接着写，而不是被标成放弃。
#[test]
fn stop_request_ends_the_fill_run_at_the_chapter_boundary() {
    let input = draft_input();
    let mut limits = config().limits;
    limits.draft_path = true;
    let config = Config::with_provider(config().provider.clone(), limits).unwrap();
    let mut state = journal_state(&input, &config);
    state.draft_stage = crate::tender_analysis::draft::DraftStage::Fill;
    state.analysis.draft_plan = (0..2)
        .map(|i| DraftPlanItem {
            grounds: vec![],
            requirement_ids: vec![],
            id: format!("ch-{i}"),
            parent: None,
            order: i,
            title: format!("第 {i} 章"),
            prescribed: true,
            source_ids: vec!["source".into()],
            windows: vec![vec!["source".into()]],
            window_index: 0,
            template_id: None,
            status: DraftStatus::Pending,
            purpose: ChapterPurpose::Response,
            format_refs: vec![],
            body_status: BodyStatus::Empty,
            omit_reason: None,
            preserved: vec![],
        })
        .collect();
    assert!(crate::tender_analysis::draft::assign_next_chapter(
        &input, &mut state
    ));
    assert_eq!(state.draft_active_id.as_deref(), Some("ch-0"));
    // 本章还没写完时按停：不打断本章，仍等它收尾。
    crate::tender_analysis::draft::after_batch(&input, &mut state, false, true).unwrap();
    assert_eq!(
        state.draft_stage,
        crate::tender_analysis::draft::DraftStage::Fill,
        "停不是中途掐断：当前章仍按预算写完"
    );
    state.analysis.draft_plan[0].status = DraftStatus::Filled;
    state.analysis.draft_plan[0].template_id = Some("tpl".into());
    crate::tender_analysis::draft::after_batch(&input, &mut state, false, true).unwrap();
    assert_eq!(
        state.draft_stage,
        crate::tender_analysis::draft::DraftStage::Published,
        "本章收尾后即停，不再派新章"
    );
    assert!(state.done && state.draft_stopped);
    assert_eq!(
        state.analysis.draft_plan[1].status,
        DraftStatus::Pending,
        "没填的章不能被当成放弃，否则空 heading 会从稿子里消失"
    );
}

/// Response parents with children still need a body; only Group nodes are skipped.
#[test]
fn assign_next_chapter_fills_response_parent_after_children() {
    let input = draft_input();
    let mut limits = config().limits;
    limits.draft_path = true;
    let config = Config::with_provider(config().provider.clone(), limits).unwrap();
    let mut state = journal_state(&input, &config);
    enter_outline(&input, &mut state);
    mark_window_coverage(&input, &mut state.analysis.coverage, &["source".into()]);
    state.analysis.draft_plan = vec![
        DraftPlanItem {
            grounds: vec![],
            requirement_ids: vec![],
            id: "vol-biz".into(),
            parent: None,
            order: 0,
            title: "商务文件".into(),
            prescribed: true,
            source_ids: vec!["source".into()],
            windows: vec![vec!["source".into()]],
            window_index: 0,
            template_id: None,
            status: DraftStatus::Pending,
            purpose: ChapterPurpose::Response,
            format_refs: vec![],
            body_status: BodyStatus::Empty,
            omit_reason: None,
            preserved: vec![],
        },
        DraftPlanItem {
            grounds: vec![],
            requirement_ids: vec![],
            id: "biz-1c".into(),
            parent: Some("vol-biz".into()),
            order: 1,
            title: "授权委托书".into(),
            prescribed: true,
            source_ids: vec!["source".into()],
            windows: vec![vec!["source".into()]],
            window_index: 0,
            template_id: None,
            status: DraftStatus::Pending,
            purpose: ChapterPurpose::Response,
            format_refs: vec![],
            body_status: BodyStatus::Empty,
            omit_reason: None,
            preserved: vec![],
        },
        DraftPlanItem {
            grounds: vec![],
            requirement_ids: vec![],
            id: "group-only".into(),
            parent: None,
            order: 2,
            title: "附件".into(),
            prescribed: true,
            source_ids: vec!["source".into()],
            windows: vec![vec!["source".into()]],
            window_index: 0,
            template_id: None,
            status: DraftStatus::Pending,
            purpose: ChapterPurpose::Group,
            format_refs: vec![],
            body_status: BodyStatus::Empty,
            omit_reason: None,
            preserved: vec![],
        },
    ];
    assert!(crate::tender_analysis::draft::assign_next_chapter(
        &input, &mut state
    ));
    assert_eq!(state.draft_active_id.as_deref(), Some("biz-1c"));
    state.analysis.draft_plan[1].status = DraftStatus::Filled;
    assert!(crate::tender_analysis::draft::assign_next_chapter(
        &input, &mut state
    ));
    assert_eq!(state.draft_active_id.as_deref(), Some("vol-biz"));
    state.analysis.draft_plan[0].status = DraftStatus::Filled;
    assert!(
        !crate::tender_analysis::draft::assign_next_chapter(&input, &mut state),
        "group nodes must not be filled"
    );
}

/// 阶段一的快是**设计出来的**：按窗数推导的帽在目标回合数内就闭合，兜底上限只防
/// 跑飞。所以「20 回合」是目标而不是闸门——难文档宁可多跑几轮把大纲写全，也不为凑
/// 数字砍成半张大纲；整体填充另记一套账，不受这个目标约束。
#[test]
fn outline_meets_the_turn_target_by_design_not_by_truncation() {
    use crate::tender_analysis::draft::{
        FILL_MAX_TURNS, OUTLINE_MAX_TURNS, fill_turn_cap, outline_turn_cap,
    };
    for input in [
        draft_input(),
        rooted_catalog_input(),
        large_input_with_named_items(),
    ] {
        let cap = outline_turn_cap(&input);
        assert_eq!(
            cap,
            crate::tender_analysis::draft::outline_chunks(&input).len() * 4 + 12
        );
    }
    // 「最坏窗数仍在目标内」「兜底宽于目标」两条口径写在常量旁边的编译期断言里。
    // 整体填充按待填章数记账：章多就该给更多回合，不受阶段一目标压制。
    assert!(fill_turn_cap(40) > OUTLINE_MAX_TURNS);
    assert_eq!(fill_turn_cap(1_000), FILL_MAX_TURNS, "再多章也有兜底");
    assert!(fill_turn_cap(0) >= 4, "补填一章也要够读表、写模板、返工");
}

/// 填章的额度与墙钟都独立记账：回合随章数放开时工具与读字节同步放开（否则回合没用
/// 完就先撞工具帽），编译上限按整份正文放大；写作窗必须短于信封窗，信封窗必须短于
/// SQL 的 46 分钟硬租约——这样到期时还有时间把已填的章编译出稿。
#[test]
fn fill_accounts_its_own_budget_and_leaves_time_to_publish() {
    use crate::tender_analysis::draft::{
        DRAFT_MAX_DOCX_BYTES, FILL_MAX_DOCX_BYTES, fill_limits, fill_turn_cap,
    };
    let mut limits = config().limits;
    limits.draft_path = true;
    limits.max_turns = 20;
    limits.max_tool_calls = 30;
    limits.max_draft_docx_bytes = DRAFT_MAX_DOCX_BYTES;
    let filling = fill_limits(limits.clone(), 30);
    assert_eq!(filling.max_turns, fill_turn_cap(30));
    assert!(
        filling.max_tool_calls >= filling.max_turns * 3,
        "回合放开而工具没放开，等于没放开"
    );
    assert!(filling.max_read_bytes > limits.max_read_bytes);
    assert_eq!(filling.max_draft_docx_bytes, FILL_MAX_DOCX_BYTES);
}

#[test]
fn draft_path_keeps_turn_ceiling() {
    let input = draft_input();
    let mut limits = config().limits;
    limits.draft_path = true;
    limits.max_turns = 80;
    let limited = limits.at_least_for(&input).unwrap();
    assert_eq!(limited.max_turns, 80);
    assert_eq!(limited.reviewer_reserve, 0);
}

#[test]
fn draft_runtime_freezes_outline_and_fill_contracts() {
    let mut limits = config().limits;
    limits.draft_path = true;
    let frozen = Config::with_provider(config().provider.clone(), limits).unwrap();
    assert_eq!(
        frozen.tools_sha256,
        digest(&crate::tender_analysis::draft::outline_schemas()).unwrap()
    );
    assert_eq!(
        frozen.fill_tools_sha256,
        digest(&crate::tender_analysis::draft::fill_schemas()).unwrap()
    );
    assert_ne!(frozen.tools_sha256, frozen.fill_tools_sha256);
    assert_ne!(frozen.main_prompt_sha256, frozen.fill_prompt_sha256);
    // 读工具常驻在两份合同里，所以冻结 hash 只有两份，没有「读工具版」。
    let outline = crate::tender_analysis::draft::outline_schemas();
    for read in crate::tender_analysis::draft::read_schemas() {
        assert!(outline.contains(&read), "{read}");
    }
    let contract = serde_json::to_string(&json!(frozen)).unwrap();
    assert!(
        !contract.contains("tools_read_sha256"),
        "统一读取路径后不该再有第二份读工具 hash"
    );
}

#[test]
fn sql_draft_publish_writes_current_docx_from_staged_bytes() {
    let sql = include_str!("../../../../../migrations/bidding_v2_baseline.sql");
    let start = sql
        .find("CREATE FUNCTION kb_bid_v2_publish_requirement_set_v4")
        .expect("publish v4");
    let rest = &sql[start + 1..];
    let end = rest
        .find("CREATE FUNCTION")
        .map(|i| start + 1 + i)
        .unwrap_or(sql.len());
    let body = &sql[start..end];
    assert!(body.contains("p_docx_staging uuid"));
    assert!(body.contains("draft chapters require staged DOCX"));
    assert!(body.contains("official analysis cannot stage draft DOCX"));
    assert!(body.contains("INSERT INTO bid_docx_current"));
    assert!(body.contains("'draft_docx'"));
}

#[test]
fn sql_draft_reserve_skips_extract_owner_and_allows_fill_hash() {
    let sql = include_str!("../../../../../migrations/bidding_v2_baseline.sql");
    let start = sql
        .find("CREATE FUNCTION kb_bid_v2_tender_agent_reserve")
        .expect("reserve");
    let rest = &sql[start + 1..];
    let end = rest
        .find("CREATE FUNCTION")
        .map(|i| start + 1 + i)
        .unwrap_or(sql.len());
    let body = &sql[start..end];
    assert!(
        body.contains("coalesce((runtime#>'{limits,draft_path}')::boolean,true)"),
        "missing draft_path must reserve as draft, not extract"
    );
    assert!(
        body.contains("fill_tools_sha256"),
        "reserve must accept frozen fill tools"
    );
    assert!(
        !body.contains("tools_read_sha256"),
        "unified read path leaves exactly two draft tool hashes"
    );
    assert!(
        body.contains("draft outline/fill contract changed"),
        "draft tools/prompt mismatch must stay a digest error"
    );
    let draft_if = body.find("limits,draft_path").expect("draft_path");
    let owner = body
        .find("initial Main source owner changed")
        .expect("official owner gate");
    assert!(
        owner > draft_if,
        "official first-source owner must sit in the non-draft branch"
    );
}

#[test]
fn sql_draft_checkpoint_allows_outline_host_assignment() {
    let sql = include_str!("../../../../../migrations/bidding_v2_baseline.sql");
    let start = sql
        .find("CREATE FUNCTION kb_bid_v2_tender_agent_checkpoint_put")
        .expect("checkpoint_put");
    let rest = &sql[start + 1..];
    let end = rest
        .find("CREATE FUNCTION")
        .map(|i| start + 1 + i)
        .unwrap_or(sql.len());
    let body = &sql[start..end];
    assert!(
        body.contains("initial checkpoint must be empty"),
        "empty first checkpoint remains a digest gate"
    );
    let empty = body
        .find("initial checkpoint must be empty")
        .expect("empty gate");
    let draft = &body[..empty];
    assert!(
        draft.contains("draft_stage") && draft.contains("{main_work,status}"),
        "draft first checkpoint may host-assign outline work"
    );
    let idle = body
        .find("draft_path}')::boolean,true) THEN")
        .expect("draft dispatch idle");
    let owner = body
        .find("Main committed owner changed")
        .expect("extract owner charge");
    assert!(
        owner > idle,
        "extract main_dispatch owner charge must sit in the non-draft branch"
    );
}

#[test]
fn sql_tender_outline_returns_draft_plan() {
    let sql = include_str!("../../../../../migrations/bidding_v2_baseline.sql");
    let start = sql
        .find("CREATE FUNCTION kb_bid_v2_get_tender_outline")
        .expect("outline");
    let rest = &sql[start + 1..];
    let end = rest
        .find("CREATE FUNCTION")
        .map(|i| start + 1 + i)
        .unwrap_or(sql.len());
    let body = &sql[start..end];
    assert!(
        body.contains("'draft_plan'") && body.contains("{analysis,draft_plan}"),
        "outline preview must expose draft_plan parent/child tree"
    );
}

#[test]
fn numbered_heading_allows_paraphrased_children() {
    let input = draft_input();
    let config = config();
    let mut state = journal_state(&input, &config);
    complete_discovery(&input, &mut state);
    let grounds = json!([{"source_id":"source","start":0,"end":input.source_units[0].text.len(),"view_id":null,"grid_cell":null}]);
    agent::apply(
        &input,
        &config,
        &mut state,
        "put_outline_item",
        &json!({"id":"ch8","parent":null,"order":8,"title":"8. 包装及运输","prescribed":true,"requirement_ids":[],"purpose":"response","format_refs":[],"grounds":grounds}),
    )
    .unwrap();
    agent::apply(
        &input,
        &config,
        &mut state,
        "put_outline_item",
        &json!({"id":"ch81","parent":"ch8","order":1,"title":"8.1 大件运输","prescribed":true,"requirement_ids":[],"purpose":"response","format_refs":[],"grounds":grounds}),
    )
    .unwrap();
    assert_eq!(state.analysis.draft_plan.len(), 2);
}

#[test]
fn pdf_without_heading_path_binds_technical_qualification_and_price() {
    let mut input = draft_input();
    input.source_units = vec![
        Source {
            source_unit_revision_id: "tech".into(),
            document_id: "pdf".into(),
            text: "技术文件第二卷应当逐项响应。".into(),
            locator: json!({"page_ordinal": 12}),
            ordinal: 0,
        },
        Source {
            source_unit_revision_id: "qual".into(),
            document_id: "pdf".into(),
            text: "资格审查资料包括营业执照。".into(),
            locator: json!({"page_ordinal": 3}),
            ordinal: 1,
        },
        Source {
            source_unit_revision_id: "price".into(),
            document_id: "pdf".into(),
            text: "投标报价表见本页。".into(),
            locator: json!({"page_ordinal": 20}),
            ordinal: 2,
        },
    ];
    assert!(
        input
            .source_units
            .iter()
            .all(|source| source.locator.get("heading_path").is_none())
    );
    assert_eq!(
        crate::tender_analysis::draft::bind_source_ids(&input, "技术文件", &[]).unwrap(),
        vec!["tech".to_string()]
    );
    assert_eq!(
        crate::tender_analysis::draft::bind_source_ids(&input, "资格审查资料", &[]).unwrap(),
        vec!["qual".to_string()]
    );
    assert_eq!(
        crate::tender_analysis::draft::bind_source_ids(&input, "投标报价表", &[]).unwrap(),
        vec!["price".to_string()]
    );
    let covered: std::collections::BTreeSet<_> =
        crate::tender_analysis::draft::outline_windows(&input)
            .into_iter()
            .flatten()
            .collect();
    assert_eq!(covered.len(), 3);
}

const BIDDINGFILE_PDF_SHA256: &str =
    "4c80edd6f570fe107ac1d5b3d0d224c20748f2379fe71a8ae05c8315df058fdc";

fn biddingfile_frozen_input() -> FrozenInput {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/bid/biddingfile/frozen-input.json");
    let bytes = std::fs::read(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
    serde_json::from_slice(&bytes).unwrap_or_else(|err| panic!("parse {}: {err}", path.display()))
}

fn bound_sources<'a>(input: &'a FrozenInput, title: &str) -> Vec<&'a Source> {
    let ids = crate::tender_analysis::draft::bind_source_ids(input, title, &[]).unwrap();
    assert!(!ids.is_empty(), "bind {title} returned empty");
    ids.into_iter()
        .map(|id| {
            input
                .source_units
                .iter()
                .find(|source| source.source_unit_revision_id == id)
                .unwrap_or_else(|| panic!("missing bound source {id} for {title}"))
        })
        .collect()
}

fn bound_text_contains(sources: &[&Source], needle: &str) -> bool {
    sources.iter().any(|source| source.text.contains(needle))
}

fn window_index_for_page(input: &FrozenInput, page: u64) -> Option<usize> {
    let windows = crate::tender_analysis::draft::outline_windows(input);
    windows.iter().position(|window| {
        window.iter().any(|id| {
            input
                .source_units
                .iter()
                .find(|source| source.source_unit_revision_id == *id)
                .and_then(|source| source.locator.get("page_ordinal").and_then(|v| v.as_u64()))
                == Some(page)
        })
    })
}

/// A02：真实 BiddingFile.pdf 冻结样本无 heading_path，技术第二卷／资格／价格按正文绑定。
#[test]
fn biddingfile_pdf_without_heading_path_maps_tech_qualification_and_price() {
    let input = biddingfile_frozen_input();
    let pdf_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/bid/BiddingFile.pdf");
    let pdf =
        std::fs::read(&pdf_path).unwrap_or_else(|err| panic!("read {}: {err}", pdf_path.display()));
    assert_eq!(hex::encode(Sha256::digest(&pdf)), BIDDINGFILE_PDF_SHA256);
    let doc = input
        .documents
        .iter()
        .find(|doc| doc.get("file_name").and_then(|v| v.as_str()) == Some("BiddingFile.pdf"))
        .expect("BiddingFile.pdf document row");
    assert_eq!(
        doc.get("sha256").and_then(|v| v.as_str()),
        Some(BIDDINGFILE_PDF_SHA256)
    );
    assert_eq!(input.source_units.len(), 148);
    assert_eq!(input.structured_forms.len(), 42);
    assert!(
        input
            .source_units
            .iter()
            .all(|source| source.locator.get("heading_path").is_none()),
        "A02 requires page locators without heading_path"
    );

    let tech = bound_sources(&input, "第二卷 技术规范");
    assert!(
        bound_text_contains(&tech, "第二卷") && bound_text_contains(&tech, "技术规范"),
        "tech bind missed volume-2 body/catalog: {:?}",
        tech.iter()
            .map(|s| s.locator.get("page_ordinal"))
            .collect::<Vec<_>>()
    );

    let qual = bound_sources(&input, "资格审查资料");
    assert!(
        bound_text_contains(&qual, "资格审查资料"),
        "qualification bind missed attachment/clause: {:?}",
        qual.iter()
            .map(|s| s.locator.get("page_ordinal"))
            .collect::<Vec<_>>()
    );

    let price = bound_sources(&input, "投标价格表");
    assert!(
        bound_text_contains(&price, "投标价格表"),
        "price bind missed price table: {:?}",
        price
            .iter()
            .map(|s| s.locator.get("page_ordinal"))
            .collect::<Vec<_>>()
    );

    // 组成条款页（约 p22）应能按正文命中，不依赖 heading_path。
    let composition = bound_sources(&input, "技术文件");
    assert!(
        composition.iter().any(|source| {
            source.locator.get("page_ordinal").and_then(|v| v.as_u64()) == Some(21)
                || source.locator.get("page_ordinal").and_then(|v| v.as_u64()) == Some(22)
        }),
        "composition clause for 技术文件 should bind near pages 21–22: {:?}",
        composition
            .iter()
            .map(|s| s.locator.get("page_ordinal"))
            .collect::<Vec<_>>()
    );
}

/// A01 离线切片：百页样本首窗偏商务，技术卷与报价附件落在后续窗。
#[test]
fn biddingfile_outline_windows_place_business_before_tech_and_price() {
    let input = biddingfile_frozen_input();
    let windows = crate::tender_analysis::draft::outline_windows(&input);
    assert!(
        windows.len() > 8,
        "expected >8 windows, got {}",
        windows.len()
    );
    let business = window_index_for_page(&input, 8).expect("商务卷正文 page 8");
    let tech = window_index_for_page(&input, 47).expect("第二卷 技术规范 page 47");
    let price = window_index_for_page(&input, 81).expect("附件 2 投标价格表 page 81");
    assert!(
        business < tech,
        "business window {business} should precede tech {tech}"
    );
    assert!(
        tech < price,
        "tech window {tech} should precede price {price}"
    );
    assert_eq!(
        windows.iter().flatten().count(),
        input.source_units.len(),
        "tail sources must remain in the scan plan"
    );
}

/// A04：真实样本里的组成句、投诉材料和附件号不会被宿主收成必备章。
#[test]
fn biddingfile_wording_does_not_invent_requirements() {
    let full = biddingfile_frozen_input();
    let mut picked: Vec<_> = full
        .source_units
        .into_iter()
        .filter(|source| {
            source.text.contains("投诉")
                || source.text.contains("投标文件应包括")
                || source.text.contains("附件 1A")
                || source.text.contains("由下列")
        })
        .take(4)
        .collect();
    assert!(
        picked.len() >= 3,
        "BiddingFile freeze should contain the rejected phrases"
    );
    for (ordinal, source) in picked.iter_mut().enumerate() {
        source.ordinal = ordinal;
        source.document_id = "document".into();
    }
    let mut input = draft_input();
    input.source_units = picked;
    input.structured_forms.clear();
    let mut limits = config().limits;
    limits.draft_path = true;
    let config =
        Config::with_provider_for(config().provider.clone(), limits, Some(&input)).unwrap();
    let mut state = journal_state(&input, &config);
    enter_outline(&input, &mut state);
    assert!(state.analysis.outline.requirements.is_empty());
    assert!(state.analysis.draft_plan.is_empty());
    assert!(!requirement_unmapped(&input, &state));
}

/// T1 校准：真实 PDF 分块 UTF-8 不重叠；长表续段携带表头但不重复记账。
#[test]
fn biddingfile_outline_chunks_are_non_overlapping_with_form_headers() {
    let input = biddingfile_frozen_input();
    let chunks = crate::tender_analysis::draft::outline_chunks(&input);
    // BiddingFile.pdf 冻结样本校准（2026-09-18）：148 sources／42 forms／20 windows／161 chunks。
    assert_eq!(input.source_units.len(), 148);
    assert_eq!(input.structured_forms.len(), 42);
    assert_eq!(
        crate::tender_analysis::draft::outline_windows(&input).len(),
        20
    );
    assert_eq!(chunks.len(), 204);

    let mut text_ranges: std::collections::BTreeMap<&str, Vec<(usize, usize)>> =
        std::collections::BTreeMap::new();
    for chunk in &chunks {
        if matches!(chunk.kind.as_str(), "metadata" | "empty") {
            continue;
        }
        assert!(chunk.account_start < chunk.account_end);
        if chunk.kind == "text" {
            assert!(chunk.deliver_start <= chunk.account_start);
            if chunk.account_start > 0 {
                assert!(chunk.deliver_start < chunk.account_start);
            }
            text_ranges
                .entry(chunk.source_id.as_str())
                .or_default()
                .push((chunk.account_start, chunk.account_end));
        } else {
            assert_eq!(chunk.kind, "form");
            if chunk.account_start > 0 {
                assert!(
                    chunk.header_cells > 0,
                    "continuation form chunk missing header redelivery: {chunk:?}"
                );
                assert_eq!(chunk.deliver_start, 0);
            }
        }
    }
    for (source_id, mut ranges) in text_ranges {
        ranges.sort_unstable();
        for window in ranges.windows(2) {
            assert!(
                window[0].1 <= window[1].0,
                "overlapping text accounts on {source_id}: {ranges:?}"
            );
        }
    }
}

#[test]
fn paraphrased_title_with_grounds_is_kept() {
    let mut input = draft_input();
    input.source_units[0].text = "资格审查资料应当提交。".into();
    input.source_units[0].locator =
        json!({"heading_path":"投标文件格式 > 商务文件 > 资格审查资料"});
    let config = config();
    let mut state = journal_state(&input, &config);
    complete_discovery(&input, &mut state);
    save_required(
        &mut state,
        "qual",
        "资格审查资料",
        "source",
        input.source_units[0].text.len(),
    );
    let grounds = json!([{
        "source_id":"source",
        "start":0,
        "end":input.source_units[0].text.len(),
        "view_id":null,
        "grid_cell":null
    }]);
    agent::apply(
        &input,
        &config,
        &mut state,
        "put_outline_item",
        &json!({"id":"biz","parent":null,"order":0,"title":"商务文件","prescribed":true,"requirement_ids":[],"purpose":"group","format_refs":[],"grounds":grounds}),
    )
    .unwrap();
    agent::apply(
        &input,
        &config,
        &mut state,
        "put_outline_item",
        &json!({"id":"qual","parent":"biz","order":0,"title":"投标人资格文件","prescribed":true,"requirement_ids":["qual"],"purpose":"response","format_refs":[],"grounds":grounds}),
    )
    .unwrap();
    assert!(
        !requirement_unmapped(&input, &state),
        "requirement id maps the chapter even when the title is paraphrased"
    );
    state.analysis.draft_plan[1].source_ids.clear();
    crate::tender_analysis::draft::assign_next_chapter(&input, &mut state);
    assert_eq!(state.analysis.draft_plan[1].status, DraftStatus::Pending);
    assert_eq!(
        state.analysis.draft_plan[1].source_ids,
        vec!["source".to_string()]
    );
    assert!(state.analysis.draft_plan[1].omit_reason.is_none());
}

/// 招标原文在**根**一级就点了子结构：`heading_path` 写着「商务文件 > 资格审查资料」。
/// 与组织阶段一致：发现完成后进入大纲阶段。
fn enter_outline(input: &FrozenInput, state: &mut agent::Checkpoint) {
    state.draft_stage = crate::tender_analysis::draft::DraftStage::Outline;
    complete_discovery(input, state);
    crate::tender_analysis::draft::preload_outline_window(input, state);
}

fn rooted_catalog_input() -> FrozenInput {
    let pad = "正文内容".repeat(700);
    let mut input = draft_input();
    input.source_units = vec![
        Source {
            source_unit_revision_id: "biz".into(),
            document_id: "document".into(),
            text: format!("商务文件。{pad}"),
            locator: json!({"heading_path":"投标文件格式 > 商务文件"}),
            ordinal: 0,
        },
        Source {
            source_unit_revision_id: "qual".into(),
            document_id: "document".into(),
            text: format!("资格审查资料。{pad}"),
            locator: json!({"heading_path":"投标文件格式 > 商务文件 > 资格审查资料"}),
            ordinal: 1,
        },
    ];
    input
}

fn large_catalog_input() -> FrozenInput {
    let pad = "正文内容".repeat(700);
    let mut input = draft_input();
    input.source_units = vec![
        Source {
            source_unit_revision_id: "biz".into(),
            document_id: "document".into(),
            text: format!("商务文件由下列文件组成。{pad}"),
            locator: json!({"heading_path":"商务文件"}),
            ordinal: 0,
        },
        Source {
            source_unit_revision_id: "qual".into(),
            document_id: "document".into(),
            text: format!("资格审查资料。8A 摘要表。{pad}"),
            locator: json!({"heading_path":"投标文件格式 > 资格审查资料 > 8A 摘要表"}),
            ordinal: 1,
        },
    ];
    input
}

#[test]
fn large_outline_blocks_publish_until_heading_children_are_written() {
    let input = large_catalog_input();
    let mut limits = config().limits;
    limits.draft_path = true;
    let config =
        Config::with_provider_for(config().provider.clone(), limits, Some(&input)).unwrap();
    let mut state = journal_state(&input, &config);
    enter_outline(&input, &mut state);
    let ids: Vec<String> = input
        .source_units
        .iter()
        .map(|source| source.source_unit_revision_id.clone())
        .collect();
    mark_window_coverage(&input, &mut state.analysis.coverage, &ids);
    save_required(
        &mut state,
        "summary",
        "摘要表",
        "qual",
        input.source_units[1].text.len(),
    );
    let grounds_biz = json!([{"source_id":"biz","start":0,"end":input.source_units[0].text.len(),"view_id":null,"grid_cell":null}]);
    let grounds_qual = json!([{"source_id":"qual","start":0,"end":input.source_units[1].text.len(),"view_id":null,"grid_cell":null}]);
    agent::apply(
        &input,
        &config,
        &mut state,
        "put_outline_item",
        &json!({"id":"vol","parent":null,"order":0,"title":"商务文件","prescribed":true,"requirement_ids":[],"purpose":"response","format_refs":[],"grounds":grounds_biz}),
    )
    .unwrap();
    agent::apply(
        &input,
        &config,
        &mut state,
        "put_outline_item",
        &json!({"id":"qual","parent":"vol","order":1,"title":"资格审查资料","prescribed":true,"requirement_ids":[],"purpose":"response","format_refs":[],"grounds":grounds_qual}),
    )
    .unwrap();
    assert!(
        requirement_unmapped(&input, &state),
        "heading_path children are not a denominator; the saved requirement is"
    );
    agent::apply(
        &input,
        &config,
        &mut state,
        "put_outline_item",
        &json!({"id":"a8","parent":"qual","order":1,"title":"摘要表","prescribed":true,"requirement_ids":["summary"],"purpose":"response","format_refs":[],"grounds":grounds_qual}),
    )
    .unwrap();
    assert!(!requirement_unmapped(&input, &state));
    finish_checked_outline(&input, &config, &mut state);
    assert_eq!(
        state.draft_stage,
        crate::tender_analysis::draft::DraftStage::Published,
        "大纲闭合即结束阶段一，不在同一个 job 里接着填章"
    );
    assert!(state.done);
    assert!(state.draft_active_id.is_none(), "阶段一不派发填章");
}

#[test]
fn advertised_draft_tools_omit_extract_contract() {
    let names = |tools: Vec<serde_json::Value>| {
        tools
            .into_iter()
            .map(|tool| tool["function"]["name"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };
    let outline = names(crate::tender_analysis::draft::outline_schemas());
    let fill = names(crate::tender_analysis::draft::fill_schemas());
    assert!(
        outline.len() >= 8 && fill.len() >= 5,
        "outline={} fill={}",
        outline.len(),
        fill.len()
    );
    for forbidden in [
        "put_record",
        "compile_docx",
        "put_source_review",
        "inspect_analysis",
    ] {
        assert!(!outline.iter().any(|name| name == forbidden), "{forbidden}");
        assert!(!fill.iter().any(|name| name == forbidden), "{forbidden}");
    }
}

/// 工具集固定、不随文件大小变：小文件是「全书正好一窗」的退化情形，不是第二套语义。
#[test]
fn draft_tools_do_not_change_with_file_size() {
    let names = |tools: Vec<serde_json::Value>| {
        tools
            .into_iter()
            .map(|tool| tool["function"]["name"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };
    let small = draft_input();
    let mut large = draft_input();
    large.source_units[0].text = "投标函为固定格式。".repeat(400);
    let outline = names(crate::tender_analysis::draft::outline_schemas());
    let fill = names(crate::tender_analysis::draft::fill_schemas());
    for required in ["read_source", "read_form", "read_source_view"] {
        assert!(outline.iter().any(|name| name == required), "{required}");
        assert!(fill.iter().any(|name| name == required), "{required}");
    }
    let mut limits = config().limits;
    limits.draft_path = true;
    let small_frozen =
        Config::with_provider_for(config().provider.clone(), limits.clone(), Some(&small)).unwrap();
    let large_frozen =
        Config::with_provider_for(config().provider.clone(), limits, Some(&large)).unwrap();
    assert_eq!(small_frozen.tools_sha256, large_frozen.tools_sha256);
    assert_eq!(
        small_frozen.fill_tools_sha256,
        large_frozen.fill_tools_sha256
    );
}

/// 一条路径只差窗数：小样本一窗，大样本按字节切窗；帽按全量窗数推导，不截断。
#[test]
fn outline_windows_scale_with_bytes_not_with_a_size_class() {
    let small = draft_input();
    assert_eq!(
        crate::tender_analysis::draft::outline_windows(&small).len(),
        1
    );
    assert_eq!(
        crate::tender_analysis::draft::outline_turn_cap(&small),
        crate::tender_analysis::draft::outline_chunks(&small).len() * 4 + 12
    );
    let large = large_catalog_input();
    let windows = crate::tender_analysis::draft::outline_windows(&large);
    assert_eq!(windows.len(), 2, "{windows:?}");
    assert_eq!(
        crate::tender_analysis::draft::outline_turn_cap(&large),
        crate::tender_analysis::draft::outline_chunks(&large).len() * 4 + 12
    );
}

/// 全量分窗：每个来源都会进入扫描计划，不因窗数上限丢掉尾部。
#[test]
fn outline_windows_put_catalog_sources_first_and_cap_the_scan() {
    let mut input = draft_input();
    let pad = "正文内容".repeat(700);
    input.structured_forms = vec![];
    input.source_units = (0..12)
        .map(|index| Source {
            source_unit_revision_id: format!("s{index}"),
            document_id: "document".into(),
            text: format!("技术规格条款。{pad}"),
            locator: json!({"heading_path":format!("技术规格 > 第 {index} 节")}),
            ordinal: index,
        })
        .collect();
    input.source_units.push(Source {
        source_unit_revision_id: "format".into(),
        document_id: "document".into(),
        text: format!("投标文件格式。{pad}"),
        locator: json!({"heading_path":"投标文件格式"}),
        ordinal: 12,
    });
    let windows = crate::tender_analysis::draft::outline_windows(&input);
    let covered: std::collections::BTreeSet<_> = windows.iter().flatten().cloned().collect();
    assert_eq!(covered.len(), input.source_units.len(), "{windows:?}");
    assert!(covered.contains("format"));
    assert!(
        windows.len() > crate::tender_analysis::draft::OUTLINE_MAX_WINDOWS,
        "must not truncate to the old 8-window cap: {windows:?}"
    );
}

#[test]
fn large_draft_fill_skips_official_dispatch_scope_gate() {
    let mut input = draft_input();
    input.source_units = (0..12)
        .map(|i| Source {
            source_unit_revision_id: format!("s{i}"),
            document_id: "document".into(),
            text: format!("组成项{i}。{}", "正文内容".repeat(80)),
            locator: json!({"page_ordinal": i + 1}),
            ordinal: i,
        })
        .collect();
    let mut limits = config().limits;
    limits.draft_path = true;
    let config =
        Config::with_provider_for(config().provider.clone(), limits, Some(&input)).unwrap();
    let mut state = journal_state(&input, &config);
    enter_outline(&input, &mut state);
    let ids: Vec<String> = input
        .source_units
        .iter()
        .map(|source| source.source_unit_revision_id.clone())
        .collect();
    mark_window_coverage(&input, &mut state.analysis.coverage, &ids);
    put_named_outline(&input, &config, &mut state, 10);
    let child = &input.source_units[11];
    let parent_id = state.analysis.draft_plan[0].id.clone();
    agent::apply(
        &input,
        &config,
        &mut state,
        "put_outline_item",
        &json!({
            "id":null,"parent":parent_id,"order":1,"title":"组成项11","prescribed":true,
            "requirement_ids":[],"purpose":"response","format_refs":[],
            "grounds":[{"source_id":child.source_unit_revision_id,"start":0,"end":child.text.len(),"view_id":null,"grid_cell":null}]
        }),
    )
    .unwrap();
    finish_checked_outline(&input, &config, &mut state);
    assert_eq!(
        state.draft_stage,
        crate::tender_analysis::draft::DraftStage::Published
    );
    // 填章由用户另起一次请求触发，进来时 checkpoint 已在 Fill 段。
    state.draft_stage = crate::tender_analysis::draft::DraftStage::Fill;
    state.done = false;
    assert!(crate::tender_analysis::draft::assign_next_chapter(
        &input, &mut state
    ));
    assert_eq!(
        state.main_work.as_ref().unwrap().source_scope,
        vec!["s11".to_string()]
    );
    // Fill preload only stages unread window bytes; discovery coverage must not
    // suppress the chapter window package.
    state.analysis.coverage.text.remove("s11");
    let evidence =
        crate::tender_analysis::agent::evidence_delivery::select(&input, &config, &state)
            .expect("draft fill must not hit official work-scope replacement")
            .expect("draft fill must preload the chapter window");
    assert!(
        !evidence.content["assigned_evidence"]["boundary_evidence"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    state.transcript.push(json!({
        "role":"user",
        "content": json!({"preloaded_evidence":evidence.content}).to_string()
    }));
    let again = crate::tender_analysis::agent::evidence_delivery::select(&input, &config, &state)
        .expect("retained outline reads must not hide fill preload")
        .expect("draft fill still preloads after retained evidence");
    assert!(
        !again.content["assigned_evidence"]["boundary_evidence"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

fn large_input_with_named_items() -> FrozenInput {
    let mut input = draft_input();
    input.source_units = (0..12)
        .map(|i| Source {
            source_unit_revision_id: format!("s{i}"),
            document_id: "document".into(),
            text: format!("组成项{i}。{}", "正文内容".repeat(80)),
            locator: json!({"page_ordinal": i + 1}),
            ordinal: i,
        })
        .collect();
    input
}

fn put_named_outline(
    input: &FrozenInput,
    config: &Config,
    state: &mut agent::Checkpoint,
    i: usize,
) {
    let source = &input.source_units[i];
    agent::apply(
        input,
        config,
        state,
        "put_outline_item",
        &outline_item_json(
            Value::Null,
            Value::Null,
            i,
            &format!("组成项{i}"),
            json!([{
                "source_id":source.source_unit_revision_id,
                "start":0,
                "end":source.text.len(),
                "view_id":null,
                "grid_cell":null
            }]),
        ),
    )
    .unwrap();
}

/// 保护帽到期不得发布部分骨架：保留未完成与阻断。
#[test]
fn outline_cap_publishes_and_records_gaps() {
    let input = rooted_catalog_input();
    let cap = crate::tender_analysis::draft::outline_turn_cap(&input);
    assert_eq!(
        cap,
        crate::tender_analysis::draft::outline_chunks(&input).len() * 4 + 12
    );
    let mut limits = config().limits;
    limits.draft_path = true;
    let config =
        Config::with_provider_for(config().provider.clone(), limits, Some(&input)).unwrap();
    let mut state = journal_state(&input, &config);
    enter_outline(&input, &mut state);
    let source = &input.source_units[0];
    agent::apply(
        &input,
        &config,
        &mut state,
        "put_outline_item",
        &json!({
            "id":"vol","parent":null,"order":0,"title":"商务文件","prescribed":true,
            "requirement_ids":[],"purpose":"response","format_refs":[],
            "grounds":[{"source_id":source.source_unit_revision_id,"start":0,"end":source.text.len(),"view_id":null,"grid_cell":null}]
        }),
    )
    .unwrap();
    state.turn = cap - 1;
    state.draft_stage = crate::tender_analysis::draft::DraftStage::Outline;
    crate::tender_analysis::draft::after_batch(&input, &mut state, false, false).unwrap();
    assert_ne!(
        state.draft_stage,
        crate::tender_analysis::draft::DraftStage::Published,
        "帽到期不得发布部分骨架"
    );
    assert!(!state.done);
    assert!(
        !crate::tender_analysis::outline_flow::checked(&input, &state)
            || !crate::tender_analysis::outline_flow::blockers(&input, &state).is_empty()
    );
}

/// 修补无进展则失败保留，不得带缺口发布。
#[test]
fn outline_stalled_repair_rounds_stop_spinning() {
    let input = rooted_catalog_input();
    let mut limits = config().limits;
    limits.draft_path = true;
    let config =
        Config::with_provider_for(config().provider.clone(), limits, Some(&input)).unwrap();
    let mut state = journal_state(&input, &config);
    enter_outline(&input, &mut state);
    let source = &input.source_units[0];
    agent::apply(
        &input,
        &config,
        &mut state,
        "put_outline_item",
        &json!({
            "id":"vol","parent":null,"order":0,"title":"商务文件","prescribed":true,
            "requirement_ids":[],"purpose":"response","format_refs":[],
            "grounds":[{"source_id":source.source_unit_revision_id,"start":0,"end":source.text.len(),"view_id":null,"grid_cell":null}]
        }),
    )
    .unwrap();
    state.outline_run.no_progress_rounds = 3;
    let err =
        crate::tender_analysis::draft::after_batch(&input, &mut state, false, false).unwrap_err();
    assert!(
        err.contains("repair exhausted") || err.contains("checkpoint retained"),
        "{err}"
    );
    assert_ne!(
        state.draft_stage,
        crate::tender_analysis::draft::DraftStage::Published
    );
}

/// 宿主逐窗推进：已扫描窗推进后投递下一窗未扫描来源。
#[test]
fn outline_advances_the_window_when_gaps_remain() {
    let input = rooted_catalog_input();
    let mut limits = config().limits;
    limits.draft_path = true;
    let config =
        Config::with_provider_for(config().provider.clone(), limits, Some(&input)).unwrap();
    let mut state = journal_state(&input, &config);
    state.draft_stage = crate::tender_analysis::draft::DraftStage::None;
    crate::tender_analysis::draft::after_batch(&input, &mut state, false, false).unwrap();
    assert_eq!(state.draft_outline_window, 0);
    assert_eq!(state.outline_run.chunk_cursor, 0);
    assert_eq!(
        state.outline_run.chunk_plan_sha256.len(),
        64,
        "first preload freezes the chunk plan digest"
    );
    let frozen = state.outline_run.chunk_plan_sha256.clone();
    assert_eq!(
        state.main_work.as_ref().unwrap().source_scope,
        vec!["biz".to_string()],
        "长来源单独占阅读包"
    );
    mark_window_coverage(&input, &mut state.analysis.coverage, &["biz".into()]);
    let args = json!({
        "text":{"biz":[[0,input.source_units[0].text.len()]]},
        "forms":{},
        "metadata":{},"empty_sources":[],
        "requirements":[],
        "references":[],
        "issues":[],
        "review_fragments":[]
    });
    crate::tender_analysis::outline_flow::apply(
        &input,
        &mut state,
        "submit_outline_scan",
        &args,
        64_000,
    )
    .unwrap();
    crate::tender_analysis::draft::after_batch(&input, &mut state, false, false).unwrap();
    assert_eq!(state.draft_outline_window, 1, "扫完首窗后前进一窗");
    let chunks = crate::tender_analysis::draft::outline_chunks(&input);
    assert!(
        chunks
            .iter()
            .take(state.outline_run.chunk_cursor)
            .all(|chunk| chunk.source_id == "biz"),
        "cursor stays on biz until its ranges are accounted"
    );
    assert_eq!(chunks[state.outline_run.chunk_cursor].source_id, "qual");
    assert_eq!(state.outline_run.chunk_plan_sha256, frozen);
    assert_eq!(
        state.main_work.as_ref().unwrap().source_scope,
        vec!["qual".to_string()]
    );
}

/// Complete source navigation includes bounded search.
#[tokio::test]
async fn outline_request_delivers_source_index_and_bounded_search() {
    let input = rooted_catalog_input();
    let mut limits = config().limits;
    limits.draft_path = true;
    limits.max_turns = 2;
    let mut config =
        Config::with_provider_for(config().provider.clone(), limits, Some(&input)).unwrap();
    config.limits.max_turns = 2;
    let journal = MemoryJournal::default();
    let model = outline_volume_without_children(&input);
    let _ = agent::run(&input, &config, &journal, &model, &CancellationToken::new()).await;
    let request = model.requests.lock().unwrap()[0].clone();
    let tools: Vec<String> = request["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["function"]["name"].as_str().unwrap().to_string())
        .collect();
    assert!(
        tools.iter().any(|name| name == "search_sources"),
        "{tools:?}"
    );
    assert!(tools.iter().any(|name| name == "read_source"));
    let packet = request["messages"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find_map(|message| {
            let content = message["content"].as_str()?;
            let value: Value = serde_json::from_str(content).ok()?;
            value.get("source_index")?;
            Some(value)
        })
        .expect("host packet with source_index");
    let index = &packet["source_index"];
    assert!(
        index["total_sources"].as_u64().unwrap_or(0) >= 2
            || index["sources"]["total"].as_u64().unwrap_or(0) >= 2,
        "{index}"
    );
    let rows = index["sources"]["items"]
        .as_array()
        .or_else(|| index["sources"].as_array())
        .expect("source index page");
    assert!(!rows.is_empty(), "{rows:?}");
    assert!(
        rows.iter().all(|row| row.get("text").is_none()),
        "索引不含正文"
    );
}

/// 定向补读：索引里列出的 id 不在当前窗也能读，读完才能当依据写树。
#[test]
fn targeted_read_outside_the_current_window_is_allowed() {
    let mut input = rooted_catalog_input();
    input.source_units[0].text = "x".repeat(8000);
    let mut limits = config().limits;
    limits.draft_path = true;
    let config =
        Config::with_provider_for(config().provider.clone(), limits, Some(&input)).unwrap();
    let mut state = journal_state(&input, &config);
    state.draft_stage = crate::tender_analysis::draft::DraftStage::Outline;
    crate::tender_analysis::draft::preload_outline_window(&input, &mut state);
    assert_eq!(
        state.main_work.as_ref().unwrap().source_scope,
        vec!["biz".to_string()]
    );
    agent::apply(
        &input,
        &config,
        &mut state,
        "read_source",
        &json!({"source_id":"qual","start":0,"max_bytes":36}),
    )
    .expect("窗外但在索引里的来源必须能定向补读");
}

/// 绑源有界：命中全书也只绑前 N 个，目录依据章优先，其余留给定向补读。
#[test]
fn bind_source_ids_is_bounded_and_prefers_catalog_sources() {
    let mut input = draft_input();
    input.structured_forms = vec![];
    input.source_units = (0..20)
        .map(|index| Source {
            source_unit_revision_id: format!("s{index}"),
            document_id: "document".into(),
            text: "投标函内容".into(),
            locator: json!({"heading_path":format!("技术规格 > 第 {index} 节")}),
            ordinal: index + 1,
        })
        .collect();
    input.source_units.push(Source {
        source_unit_revision_id: "format".into(),
        document_id: "document".into(),
        text: "投标函格式如下。".into(),
        locator: json!({"heading_path":"投标文件格式 > 投标函"}),
        ordinal: 99,
    });
    let bound = crate::tender_analysis::draft::bind_source_ids(&input, "投标函", &[]).unwrap();
    assert_eq!(
        bound.len(),
        crate::tender_analysis::draft::BIND_MAX_SOURCES,
        "{bound:?}"
    );
    assert_eq!(bound[0], "format", "目录依据章排在正文命中之前");
}

/// 标题路径不会生成必备章。已保存的要求无论来自评标办法还是格式章，未映射都阻断。
#[test]
fn heading_paths_do_not_invent_required_chapters() {
    let mut input = draft_input();
    input.structured_forms = vec![];
    input.source_units = vec![
        Source {
            source_unit_revision_id: "rule".into(),
            document_id: "document".into(),
            text: "评标办法。评标细则。".into(),
            locator: json!({"heading_path":"评标办法 > 商务文件 > 评标细则"}),
            ordinal: 0,
        },
        Source {
            source_unit_revision_id: "fmt".into(),
            document_id: "document".into(),
            text: "投标文件格式。商务文件。".into(),
            locator: json!({"heading_path":"投标文件格式 > 商务文件"}),
            ordinal: 1,
        },
    ];
    let mut limits = config().limits;
    limits.draft_path = true;
    let config =
        Config::with_provider_for(config().provider.clone(), limits, Some(&input)).unwrap();
    let mut state = journal_state(&input, &config);
    enter_outline(&input, &mut state);
    assert!(state.analysis.outline.requirements.is_empty());
    assert!(!requirement_unmapped(&input, &state));
    save_required(
        &mut state,
        "rule-item",
        "评标细则",
        "rule",
        input.source_units[0].text.len(),
    );
    assert!(requirement_unmapped(&input, &state));
}

#[test]
fn large_outline_two_items_at_cap_publishes_skeleton() {
    let input = large_input_with_named_items();
    let mut limits = config().limits;
    limits.draft_path = true;
    let config =
        Config::with_provider_for(config().provider.clone(), limits, Some(&input)).unwrap();
    let mut state = journal_state(&input, &config);
    enter_outline(&input, &mut state);
    put_named_outline(&input, &config, &mut state, 10);
    let child = &input.source_units[11];
    let parent_id = state.analysis.draft_plan[0].id.clone();
    agent::apply(
        &input,
        &config,
        &mut state,
        "put_outline_item",
        &json!({
            "id":null,"parent":parent_id,"order":1,"title":"组成项11","prescribed":true,
            "requirement_ids":[],"purpose":"response","format_refs":[],
            "grounds":[{"source_id":child.source_unit_revision_id,"start":0,"end":child.text.len(),"view_id":null,"grid_cell":null}]
        }),
    )
    .unwrap();
    finish_checked_outline(&input, &config, &mut state);
    assert_eq!(
        state.draft_stage,
        crate::tender_analysis::draft::DraftStage::Published
    );
    assert!(state.done);
    assert!(state.draft_active_id.is_none(), "到帽也只出骨架，不填章");
}

/// 阶段一：发现→组织→核对通过后编译骨架 Word 并结束，本 job 不进 Fill。
#[tokio::test]
async fn draft_run_stops_at_skeleton_without_filling() {
    let input = draft_input();
    let mut limits = config().limits;
    limits.draft_path = true;
    limits.max_turns = 80;
    let config = Config::with_provider(config().provider.clone(), limits).unwrap();
    let journal = MemoryJournal::default();
    let model = outline_publish_script(&input);
    let result = agent::run(&input, &config, &journal, &model, &CancellationToken::new())
        .await
        .unwrap();
    assert!(result.review.draft);
    assert_eq!(result.quality, "needs_review");
    assert!(result.analysis.dispositions.is_empty());
    assert!(
        result
            .analysis
            .draft_plan
            .iter()
            .all(|item| item.status == DraftStatus::Pending),
        "阶段一结束时章节仍待填，不能被判 omitted"
    );
    let state = journal.load().await.unwrap().unwrap();
    assert!(state.turn <= 12, "draft turns {}", state.turn);
    assert_draft_object_ref(&state);
    let xml = docx_xml_from_checkpoint(&state);
    assert!(
        xml.contains("投标函"),
        "阶段一必须产出带章节标题的可编辑 DOCX: {xml}"
    );
    let bodies = model.requests.lock().unwrap();
    let names = |body: &Value| {
        body["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["function"]["name"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };
    for body in bodies.iter() {
        let tools = names(body);
        assert!(
            tools.iter().any(|name| {
                matches!(
                    name.as_str(),
                    "put_outline_items"
                        | "put_outline_item"
                        | "submit_outline_scan"
                        | "finish_outline"
                        | "submit_outline_check"
                )
            }),
            "阶段一只跑大纲合同: {tools:?}"
        );
        assert!(
            !tools.contains(&"put_chapter_template".into()),
            "阶段一不得给出填章工具: {tools:?}"
        );
        assert!(tools.len() >= 2);
        for name in &tools {
            assert_ne!(name, "put_record");
            assert_ne!(name, "inspect_analysis");
            assert_ne!(name, "compile_docx");
            assert_ne!(name, "put_source_review");
            // Search is bounded and available during discovery.
        }
    }
}

#[test]
fn draft_attempt_and_deadline_caps() {
    assert!(!crate::tender_analysis::draft::draft_claim_exhausted(
        false, 4
    ));
    assert!(!crate::tender_analysis::draft::draft_claim_exhausted(
        true, 3
    ));
    assert!(crate::tender_analysis::draft::draft_claim_exhausted(
        true, 4
    ));
    assert_eq!(
        crate::tender_analysis::draft::analysis_deadline_secs(true),
        46 * 60
    );
    assert_eq!(
        crate::tender_analysis::draft::analysis_deadline_secs(false),
        45 * 60
    );
}

#[test]
fn retry_uses_the_same_absolute_deadline() {
    let first = 1_700_000_000;
    let budget = crate::tender_analysis::draft::handler_budget_secs(None, first, 46 * 60);
    assert_eq!(budget, 46 * 60);
    let deadline = first + budget as i64;
    let later = first + 20 * 60;
    assert_eq!(
        crate::tender_analysis::draft::handler_budget_secs(Some(deadline), later, 46 * 60),
        26 * 60
    );
    assert_eq!(
        crate::tender_analysis::draft::handler_budget_secs(Some(deadline), deadline + 5, 46 * 60),
        0
    );
    let sql = include_str!("../../../../../migrations/bidding_v2_baseline.sql");
    assert!(sql.contains("later AgentRun must copy the first deadline"));
    assert!(sql.contains("IF first_deadline<=claimed_at THEN"));
    assert!(!sql.contains("first_deadline<=claimed_at+interval '300 seconds'"));
    assert!(sql.contains("kb_bid_v2_tender_agent_frozen_deadline"));
    assert_eq!(crate::tender_analysis::draft::PUBLISH_RESERVE_SECS, 300);
}

#[test]
fn official_basis_and_composition_entry_reject_draft() {
    let input = draft_input();
    let result = draft_result(&input);
    let err = crate::docx_composition::validate_basis(&input, &result).unwrap_err();
    assert!(err.contains("official composition rejects draft"), "{err}");
    crate::docx_composition::Draft::new(&input, &result).unwrap();
}

#[test]
fn sql_composition_rejects_official_and_export_allows_saved_docx() {
    let sql = include_str!("../../../../../migrations/bidding_v2_baseline.sql");
    let function_body = |name: &str| {
        let start = sql.find(&format!("CREATE FUNCTION {name}")).expect(name);
        let rest = &sql[start + 1..];
        let end = rest
            .find("CREATE FUNCTION")
            .map(|i| start + 1 + i)
            .unwrap_or(sql.len());
        &sql[start..end]
    };
    let load = function_body("kb_bid_v2_load_docx_composition_source");
    let prepare = function_body("kb_bid_v2_prepare_docx_composition_source");
    let submit = function_body("kb_bid_v2_submit_docx_composition_request");
    let export = function_body("kb_bid_v2_create_submission_export_request");
    let publish = function_body("kb_bid_v2_publish_submission_export");
    assert!(!load.contains("p_mode"));
    assert!(!prepare.contains("p_mode"));
    assert!(submit.contains("kb_bid_v2_create_docx_composition_request"));
    assert!(!export.contains("official export rejects draft analysis"));
    assert!(export.contains("DOCX_VERSION_CAS_MISMATCH"));
    assert!(export.contains("DOCX_SAVE_PENDING"));
    assert!(publish.contains("review_checkpoint->'done' = 'true'::jsonb"));
    assert!(publish.contains("unfrozen semantic review cannot be published"));
}

#[test]
fn draft_partial_publish_accepts_deadline_and_cancel() {
    assert!(draft_should_publish_partial(
        "AGENT_TURN_BUDGET_EXCEEDED",
        ""
    ));
    assert!(draft_should_publish_partial(
        "AGENT_DEADLINE_EXCEEDED",
        "analysis deadline reached"
    ));
    assert!(draft_should_publish_partial(
        "INTERNAL",
        "Agent run cancelled"
    ));
    assert!(!draft_should_publish_partial(
        "FROZEN_INPUT_DIGEST_MISMATCH",
        ""
    ));
    assert!(!draft_should_publish_partial("AGENT_OUTPUT_INVALID", ""));
    assert!(!draft_should_publish_partial(
        "AGENT_PROVIDER_UNAVAILABLE",
        ""
    ));
}

#[tokio::test]
async fn run_budget_still_publishes_skeleton_docx() {
    let input = rooted_catalog_input();
    let mut limits = config().limits;
    limits.draft_path = true;
    limits.max_turns = 2;
    let mut config =
        Config::with_provider_for(config().provider.clone(), limits, Some(&input)).unwrap();
    config.limits.max_turns = 2;
    let journal = MemoryJournal::default();
    let model = outline_volume_without_children(&input);
    let err = agent::run(&input, &config, &journal, &model, &CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(err.code, "AGENT_TURN_BUDGET_EXCEEDED");
    let state = journal.load().await.unwrap().unwrap();
    assert!(
        state.draft_docx_base64.is_none(),
        "budget must not publish a skeleton DOCX"
    );
    assert_ne!(
        state.draft_stage,
        crate::tender_analysis::draft::DraftStage::Published
    );
}

#[tokio::test]
async fn run_cancel_still_publishes_skeleton_docx() {
    let input = rooted_catalog_input();
    let mut limits = config().limits;
    limits.draft_path = true;
    limits.max_turns = 80;
    let config =
        Config::with_provider_for(config().provider.clone(), limits, Some(&input)).unwrap();
    let journal = MemoryJournal::default();
    journal.interrupt_after.lock().unwrap().replace(2);
    let model = outline_volume_without_children(&input);
    let err = agent::run(&input, &config, &journal, &model, &CancellationToken::new())
        .await
        .unwrap_err();
    assert!(
        err.code == "INTERNAL" || err.message.to_lowercase().contains("cancel"),
        "{err:?}"
    );
    let state = journal.load().await.unwrap().unwrap();
    assert!(
        state.draft_docx_base64.is_none(),
        "cancel must not publish a skeleton DOCX"
    );
    assert_ne!(
        state.draft_stage,
        crate::tender_analysis::draft::DraftStage::Published
    );
}

#[tokio::test]
async fn put_chapter_template_rejects_other_chapter() {
    let input = draft_input();
    let mut limits = config().limits;
    limits.draft_path = true;
    let config = Config::with_provider(config().provider.clone(), limits).unwrap();
    let mut state = journal_state(&input, &config);
    enter_outline(&input, &mut state);
    let text = input.source_units[0].text.clone();
    let saved = agent::apply(
        &input,
        &config,
        &mut state,
        "put_outline_item",
        &json!({
            "id":null,"parent":null,"order":0,"title":"投标函","prescribed":true,
            "requirement_ids":[],"purpose":"response","format_refs":[],
            "grounds":[{"source_id":"source","start":0,"end":text.len(),"view_id":null,"grid_cell":null}]
        }),
    )
    .unwrap();
    let chapter_id = saved["id"].as_str().unwrap().to_string();
    state.draft_active_id = Some(chapter_id.clone());
    let err = agent::apply(
        &input,
        &config,
        &mut state,
        "put_chapter_template",
        &json!({
            "chapter_id":"other","id":null,"title":"x","purpose":"y",
            "regions":[]
        }),
    )
    .unwrap_err();
    assert!(
        err.contains(&format!("draft_active_id \"{chapter_id}\"")),
        "{err}"
    );
    assert!(err.contains("do not send the title"), "{err}");
}

fn outlined_letter(input: &FrozenInput) -> (agent::Config, agent::Checkpoint) {
    let mut limits = config().limits;
    limits.draft_path = true;
    let config = Config::with_provider(config().provider.clone(), limits).unwrap();
    let mut state = journal_state(input, &config);
    complete_discovery(input, &mut state);
    let text = input.source_units[0].text.clone();
    let saved = agent::apply(
        input,
        &config,
        &mut state,
        "put_outline_item",
        &json!({
            "id":null,"parent":null,"order":0,"title":"投标函","prescribed":true,
            "requirement_ids":[],"purpose":"response","format_refs":[],
            "grounds":[{"source_id":"source","start":0,"end":text.len(),"view_id":null,"grid_cell":null}]
        }),
    )
    .unwrap();
    let chapter_id = saved["id"].as_str().unwrap().to_string();
    state.draft_active_id = Some(chapter_id.clone());
    (config, state)
}

fn letter_region(chapter_id: &str, source: Value) -> Value {
    json!({
        "chapter_id":chapter_id,"id":null,"title":"投标函","purpose":"格式",
        "regions":[{
            "source":source,
            "role":"fixed_text","form_id":null,"cells":[],"blank_ranges":[],"instruction":""
        }]
    })
}

#[test]
fn put_chapter_template_accepts_citation_ref_and_compact_ref() {
    let input = draft_input();
    let end = "投标函为固定格式。".len();
    for source in [
        json!({"ref":format!("t:0:0:{end}")}),
        json!({"citation_ref":{"ref":format!("t:0:0:{end}")}}),
        json!({"source_id":"source","start":0,"end":end,"view_id":null,"grid_cell":null}),
    ] {
        let (config, mut state) = outlined_letter(&input);
        let chapter_id = state.draft_active_id.clone().unwrap();
        let saved = agent::apply(
            &input,
            &config,
            &mut state,
            "put_chapter_template",
            &letter_region(&chapter_id, source.clone()),
        )
        .unwrap();
        assert_eq!(saved["saved"], true, "{source}");
        assert_eq!(saved["status"], "filled", "{source}");
    }
}

#[test]
fn draft_fill_ignores_official_execution_block() {
    let input = draft_input();
    let (config, mut state) = outlined_letter(&input);
    state.main_progress.watch.recovery = crate::agent_runtime::progress::Recovery::Blocked;
    let end = "投标函为固定格式。".len();
    let chapter_id = state.draft_active_id.clone().unwrap();
    let saved = agent::apply(
        &input,
        &config,
        &mut state,
        "put_chapter_template",
        &letter_region(
            &chapter_id,
            json!({
                "source_id":"source","start":0,"end":end,"view_id":null,"grid_cell":null
            }),
        ),
    )
    .unwrap();
    assert_eq!(saved["saved"], true);
}

#[test]
fn draft_apply_rejects_official_extract_tools() {
    let input = draft_input();
    let mut limits = config().limits;
    limits.draft_path = true;
    let config = Config::with_provider(config().provider.clone(), limits).unwrap();
    let mut state = journal_state(&input, &config);
    enter_outline(&input, &mut state);
    for name in [
        "inspect_analysis",
        "put_record",
        "set_work_note",
        "request_review",
        "check_gaps",
    ] {
        let err = agent::apply(&input, &config, &mut state, name, &json!({})).unwrap_err();
        assert!(err.contains("unknown or role-forbidden"), "{name}: {err}");
    }
}

#[derive(Default)]
struct BatchScript {
    turns: Mutex<VecDeque<Vec<(String, Value)>>>,
    requests: Mutex<Vec<Value>>,
}

#[async_trait]
impl Model for BatchScript {
    async fn turn(&self, _: &Config, body: &[u8]) -> Result<ChatTurn, AgentError> {
        let body: Value = serde_json::from_slice(body).unwrap();
        self.requests.lock().unwrap().push(body);
        let calls = self
            .turns
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected model call");
        Ok(ChatTurn {
            usage: None,
            content: String::new(),
            finish_reason: "tool_calls".into(),
            tool_calls: calls
                .into_iter()
                .enumerate()
                .map(|(index, (name, args))| ChatToolCall {
                    id: format!("call-{index}"),
                    name,
                    arguments: args.to_string(),
                })
                .collect(),
        })
    }
}

struct OutlinePublishScript {
    end: usize,
    step: Mutex<usize>,
    requests: Mutex<Vec<Value>>,
}

fn outline_publish_script(input: &FrozenInput) -> OutlinePublishScript {
    OutlinePublishScript {
        end: input.source_units[0].text.len(),
        step: Mutex::new(0),
        requests: Mutex::new(Vec::new()),
    }
}

#[async_trait]
impl Model for OutlinePublishScript {
    async fn turn(&self, _: &Config, body: &[u8]) -> Result<ChatTurn, AgentError> {
        let body: Value = serde_json::from_slice(body).unwrap();
        self.requests.lock().unwrap().push(body.clone());
        let step = {
            let mut step = self.step.lock().unwrap();
            let current = *step;
            *step += 1;
            current
        };
        let end = self.end;
        let calls = match step {
            0 => vec![(
                "read_source".into(),
                json!({"source_id":"source","start":0,"max_bytes":end}),
            )],
            1 => vec![(
                "submit_outline_scan".into(),
                json!({
                    "text":{"source":[[0,end]]},
                    "forms":{},
                    "metadata":{"documents":[[0,1]]},"empty_sources":[],
                    "requirements":[{"id":"","description":"投标函","kind":"submission","submission_name":"投标函","classification_reason":"","format_required":false,"applicability":"required","condition":"","grounds":[{"source_id":"source","start":0,"end":end}],"format_grounds":[],"order_constraints":[]}],
                    "references":[],
                    "issues":[],
                    "review_fragments":[]
                }),
            )],
            2 => vec![(
                "put_outline_items".into(),
                json!({
                    "items":[{
                        "id":"tmp-letter","parent":null,"order":0,"title":"投标函","prescribed":true,
                        "requirement_ids":["requirement-1"],"purpose":"response"
                    }],
                    "remove_ids":[]
                }),
            )],
            3 => vec![("finish_outline".into(), json!({}))],
            step if step >= 4 && step % 2 == 0 => vec![(
                "read_outline".into(),
                json!({"kind":"fragments","offset":0,"limit":1000}),
            )],
            step if step >= 4 => {
                let packet = body["messages"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .rev()
                    .find_map(|message| {
                        let content = message.get("content")?;
                        let value = match content {
                            Value::String(text) => serde_json::from_str(text).ok()?,
                            Value::Object(_) => content.clone(),
                            _ => return None,
                        };
                        let value = value
                            .pointer("/result")
                            .or_else(|| value.pointer("/outline_state"))
                            .cloned()
                            .unwrap_or(value);
                        if value.get("checked") == Some(&json!(true)) {
                            return None;
                        }
                        let packet_id = value
                            .get("next_packet_id")
                            .or_else(|| value.get("packet_id"))
                            .and_then(Value::as_str)?
                            .to_string();
                        let snapshot = value.get("packet_snapshot_sha256").cloned()?;
                        Some((packet_id, snapshot))
                    })
                    .unwrap_or_else(|| {
                        panic!(
                            "check packet identity missing in messages: {}",
                            body["messages"]
                        )
                    });
                vec![(
                    "submit_outline_check".into(),
                    json!({
                        "packet_id": packet.0,
                        "packet_snapshot_sha256": packet.1,
                        "issues": []
                    }),
                )]
            }
            _ => panic!("unexpected model call at step {step}"),
        };
        Ok(ChatTurn {
            usage: None,
            content: String::new(),
            finish_reason: "tool_calls".into(),
            tool_calls: calls
                .into_iter()
                .enumerate()
                .map(|(index, (name, args))| ChatToolCall {
                    id: format!("call-{index}"),
                    name,
                    arguments: args.to_string(),
                })
                .collect(),
        })
    }
}

/// 只写分册名、始终不写子章：大纲清单闭不上，阶段一只能带缺口收尾。
fn outline_volume_without_children(input: &FrozenInput) -> BatchScript {
    let volume = &input.source_units[0];
    let head = "商务文件由下列文件组成。".len();
    let grounds = json!([{
        "source_id":volume.source_unit_revision_id,"start":0,"end":head,
        "view_id":null,"grid_cell":null
    }]);
    let put = vec![(
        "put_outline_item".into(),
        json!({
            "id":"vol-biz","parent":null,"order":0,"title":"商务文件","prescribed":true,
            "requirement_ids":[],"purpose":"response","format_refs":[],
            "grounds":grounds
        }),
    )];
    BatchScript {
        requests: Mutex::new(Vec::new()),
        turns: Mutex::new(VecDeque::from([
            vec![(
                "read_source".into(),
                json!({"source_id":volume.source_unit_revision_id,"start":0,"max_bytes":head}),
            )],
            put.clone(),
            put.clone(),
            put,
        ])),
    }
}

fn assert_draft_object_ref(state: &Checkpoint) {
    let bytes = STANDARD
        .decode(state.draft_docx_base64.as_ref().expect("draft DOCX bytes"))
        .unwrap();
    let sha = {
        use sha2::{Digest, Sha256};
        hex::encode(Sha256::digest(&bytes))
    };
    let object_ref = format!("objects/{sha}");
    assert_eq!(
        state.draft_compile_object_id.as_deref(),
        Some(object_ref.as_str()),
        "object_ref must be the raw DOCX digest, not a JCS hash"
    );
}

fn docx_xml_from_checkpoint(state: &Checkpoint) -> String {
    let bytes = STANDARD
        .decode(state.draft_docx_base64.as_ref().expect("draft DOCX bytes"))
        .unwrap();
    docx_document_xml(&bytes)
}

fn docx_document_xml(docx: &[u8]) -> String {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(docx)).unwrap();
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    xml
}

fn journal_state(input: &FrozenInput, config: &Config) -> Checkpoint {
    Checkpoint {
        journal: Default::default(),
        input_sha256: digest(input).unwrap(),
        config_sha256: digest(config).unwrap(),
        turn: 0,
        tool_calls: 0,
        read_bytes: 0,
        review_rounds: 0,
        role: Role::Main,
        analysis: Analysis::default(),
        review: None,
        review_draft: BTreeMap::new(),
        source_review: None,
        repair: Default::default(),
        dispatch: Default::default(),
        reviewer_coverage: Coverage::default(),
        pending_coverage: None,
        transcript: vec![],
        main_progress: Default::default(),
        reviewer_progress: Default::default(),
        main_work: None,
        reviewer_work: None,
        done: false,
        source_views: BTreeMap::new(),
        draft_stage: Default::default(),
        draft_active_id: None,
        draft_outline_gaps: None,
        draft_outline_stalls: 0,
        draft_outline_window: 0,
        draft_degraded: Vec::new(),
        draft_stopped: false,
        draft_compile_object_id: None,
        draft_docx_base64: None,
        outline_config_sha256: None,
        fill_config_sha256: None,
        outline_run: Default::default(),
    }
}

#[test]
fn check_phase_rejects_source_index_through_agent_gate() {
    let input = draft_input();
    let mut limits = config().limits;
    limits.draft_path = true;
    let config =
        Config::with_provider_for(config().provider.clone(), limits, Some(&input)).unwrap();
    let mut state = journal_state(&input, &config);
    complete_discovery(&input, &mut state);
    state.analysis.outline.phase = crate::tender_analysis::outline_flow::Phase::Check;
    state.outline_run.phase = crate::tender_analysis::outline_flow::Phase::Check;
    let err = agent::apply(
        &input,
        &config,
        &mut state,
        "source_index",
        &json!({"offset":0,"limit":10}),
    )
    .expect_err("check phase must reject source_index");
    assert!(
        err.contains("check phase allows only read_outline"),
        "{err}"
    );
    agent::apply(
        &input,
        &config,
        &mut state,
        "read_outline",
        &json!({"kind":"fragments","offset":0,"limit":10}),
    )
    .expect("read_outline fragments remains available");
}

#[test]
fn outline_flow_rejects_unseen_scan_ranges() {
    let input = draft_input();
    let config = config();
    let mut state = journal_state(&input, &config);
    let args = json!({"text":{"source":[[0,input.source_units[0].text.len()]]},"forms":{},"metadata":{},"empty_sources":[],"requirements":[],"references":[],"issues":[],"review_fragments":[]});
    let before = digest(&state.analysis).unwrap();
    assert!(
        crate::tender_analysis::outline_flow::apply(
            &input,
            &mut state,
            "submit_outline_scan",
            &args,
            8000
        )
        .is_err()
    );
    assert_eq!(digest(&state.analysis).unwrap(), before);
}

#[test]
fn outline_flow_does_not_publish_an_unscanned_flat_outline() {
    let input = draft_input();
    let config = config();
    let mut state = journal_state(&input, &config);
    state.draft_stage = crate::tender_analysis::draft::DraftStage::Outline;
    state.analysis.draft_plan.push(filled_item("unused"));
    for _ in 0..4 {
        crate::tender_analysis::draft::after_batch(&input, &mut state, false, false).unwrap();
    }
    assert!(!state.done);
    assert_eq!(
        state.analysis.outline.phase,
        crate::tender_analysis::outline_flow::Phase::Discover
    );
}

#[test]
fn outline_flow_rejects_self_and_ancestor_cycles() {
    let mut first = filled_item("unused");
    first.parent = Some(first.id.clone());
    assert!(
        crate::tender_analysis::outline_flow::tree_valid(&[first.clone()])
            .unwrap_err()
            .contains("cycle")
    );
    let mut second = first.clone();
    second.id = "child".into();
    first.parent = Some(second.id.clone());
    second.parent = Some(first.id.clone());
    assert!(
        crate::tender_analysis::outline_flow::tree_valid(&[first, second])
            .unwrap_err()
            .contains("cycle")
    );
}

#[test]
fn outline_flow_windows_do_not_drop_tail_sources() {
    let mut input = draft_input();
    let source = input.source_units[0].clone();
    input.source_units = (0..20)
        .map(|i| {
            let mut s = source.clone();
            s.source_unit_revision_id = format!("source-{i}");
            s.ordinal = i;
            s.text = "正文".repeat(2000);
            s
        })
        .collect();
    let windows = crate::tender_analysis::draft::outline_windows(&input);
    assert_eq!(windows.len(), 20);
    assert_eq!(windows.last().unwrap(), &["source-19".to_string()]);
}

#[tokio::test]
async fn publish_reserve_allows_checked_outline_compilation_without_model_work() {
    let input = draft_input();
    let mut limits = config().limits;
    limits.draft_path = true;
    limits.max_turns = 80;
    let config = Config::with_provider(config().provider.clone(), limits).unwrap();
    let journal = MemoryJournal::default();
    let model = outline_publish_script(&input);
    let error = agent::run_with_model_budget(
        &input,
        &config,
        &journal,
        &model,
        &CancellationToken::new(),
        std::time::Duration::ZERO,
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "AGENT_DEADLINE_EXCEEDED");
    assert!(model.requests.lock().unwrap().is_empty());
    agent::run(&input, &config, &journal, &model, &CancellationToken::new())
        .await
        .unwrap();
    let calls = model.requests.lock().unwrap().len();
    {
        let mut saved = journal.state.lock().unwrap();
        let state = saved.as_mut().unwrap();
        state.draft_docx_base64 = None;
        state.draft_compile_object_id = None;
        state.done = false;
    }
    agent::run_with_model_budget(
        &input,
        &config,
        &journal,
        &model,
        &CancellationToken::new(),
        std::time::Duration::ZERO,
    )
    .await
    .unwrap();
    assert_eq!(model.requests.lock().unwrap().len(), calls);
    assert_draft_object_ref(&journal.load().await.unwrap().unwrap());
}

#[test]
fn chapters_can_be_organized_incrementally_and_reordered_during_repair() {
    use crate::tender_analysis::outline_flow::{self, Phase};
    let input = draft_input();
    let config = config();
    let mut state = journal_state(&input, &config);
    enter_outline(&input, &mut state);
    let source = &input.source_units[0];
    for id in ["need-a", "need-b"] {
        save_required(
            &mut state,
            id,
            id,
            &source.source_unit_revision_id,
            source.text.len(),
        );
    }
    let chapter = |id: &str, order: usize, need: &str| {
        json!({
            "id":id,"parent":null,"order":order,"title":need,"prescribed":false,
            "requirement_ids":[need],"purpose":"response"
        })
    };
    let first = agent::apply(
        &input,
        &config,
        &mut state,
        "put_outline_items",
        &json!({"items":[chapter("tmp-a",0,"need-a")],"remove_ids":[]}),
    )
    .unwrap();
    let first_id = first["id_map"]["tmp-a"].as_str().unwrap().to_string();
    assert!(outline_flow::blockers(&input, &state).contains(&"B_REQUIREMENT_UNMAPPED"));
    let second = agent::apply(
        &input,
        &config,
        &mut state,
        "put_outline_items",
        &json!({"items":[chapter("tmp-b",1,"need-b")],"remove_ids":[]}),
    )
    .unwrap();
    let second_id = second["id_map"]["tmp-b"].as_str().unwrap().to_string();
    outline_flow::compose_check_packets(&input, &mut state);
    state.analysis.outline.phase = Phase::Discover;
    agent::apply(&input,&config,&mut state,"put_outline_items",
        &json!({"items":[chapter(&first_id,1,"need-a"),chapter(&second_id,0,"need-b")],"remove_ids":[]})).unwrap();
    // Replacing a removed sibling at the same order is valid in the final tree.
    let replacement = agent::apply(
        &input,
        &config,
        &mut state,
        "put_outline_items",
        &json!({"items":[chapter("tmp-replacement",0,"need-b")],"remove_ids":[second_id]}),
    )
    .unwrap();
    assert!(replacement["id_map"]["tmp-replacement"].is_string());
    assert!(outline_flow::blockers(&input, &state).is_empty());
    assert_eq!(
        state
            .analysis
            .draft_plan
            .iter()
            .find(|n| n.id == first_id)
            .unwrap()
            .order,
        1
    );
    assert!(
        agent::apply(
            &input,
            &config,
            &mut state,
            "put_outline_items",
            &json!({"items":[],"remove_ids":[first_id]})
        )
        .is_err()
    );
}

#[tokio::test]
async fn fill_preserves_outline_warnings_in_recompiled_document() {
    use crate::tender_analysis::{
        outline_flow::{IssueStatus, OutlineIssue, OutlineState},
        readback::Preserved,
    };
    let input = draft_input();
    let config = config();
    let mut state = journal_state(&input, &config);
    enter_outline(&input, &mut state);
    let mut item = filled_item("unused");
    item.template_id = None;
    item.preserved = vec![Preserved::Paragraphs {
        unit_keys: vec!["body".into()],
        paragraphs: vec!["用户报价100万元".into()],
    }];
    let mut outline = OutlineState::default();
    outline.issues.insert(
        "warning-1".into(),
        OutlineIssue {
            code: "W_EXTERNAL_CONTENT".into(),
            description: "外部标准待确认".into(),
            requirement_ids: vec![],
            chapter_ids: vec![],
            reference_ids: vec![],
            grounds: vec![],
            status: IssueStatus::Open,
            resolution_grounds: vec![],
        },
    );
    // Resume a fill that already received source evidence, before compiling its saved body.
    state.draft_stage = crate::tender_analysis::draft::DraftStage::Fill;
    state.analysis.draft_plan = vec![item.clone()];
    state.analysis.outline = outline.clone();
    state.analysis.fill_seed_chapters =
        crate::tender_analysis::readback::seed_identities(&state.analysis.draft_plan).unwrap();
    let journal = MemoryJournal::default();
    *journal.state.lock().unwrap() = Some(state);
    let model = outline_publish_script(&input);
    let cancel = CancellationToken::new();
    cancel.cancel();
    let result = agent::run_fill(
        &input,
        &config,
        &journal,
        &model,
        &cancel,
        vec![item],
        outline,
    )
    .await
    .unwrap_err();
    assert!(result.message.contains("no new content"));
    let saved = journal.load().await.unwrap().unwrap();
    assert_eq!(saved.analysis.outline.issues.len(), 1);
    let mut compilation = draft_result(&input);
    compilation.analysis = saved.analysis;
    compilation.review.analysis_sha256 = digest(&compilation.analysis).unwrap();
    let docx =
        crate::tender_analysis::draft::compile_draft(&input, &compilation, 1_000_000).unwrap();
    let xml = docx_document_xml(&docx.compiled.docx);
    assert!(xml.contains("外部标准待确认"));
    assert!(xml.contains("用户报价100万元"));
}

#[test]
fn short_discovery_sources_share_scope_without_claiming_coverage() {
    let mut input = rooted_catalog_input();
    for source in &mut input.source_units {
        source.text = "投标须提供资格材料".into();
    }
    let config = config();
    let mut state = journal_state(&input, &config);
    let before = serde_json::to_value(&state.analysis.coverage).unwrap();
    crate::tender_analysis::draft::preload_outline_window(&input, &mut state);
    assert_eq!(
        crate::tender_analysis::draft::outline_reading_scope(&input, &state, 32_000).len(),
        input.source_units.len()
    );
    assert_eq!(
        serde_json::to_value(&state.analysis.coverage).unwrap(),
        before
    );
    assert!(!crate::tender_analysis::outline_flow::scan_complete(
        &input,
        &state.analysis.outline
    ));
}

#[tokio::test]
async fn discovery_package_crosses_accounting_chunks_and_replays_exact_receipts() {
    let mut input = rooted_catalog_input();
    input.structured_forms.clear();
    for source in &mut input.source_units {
        source.text = "技".repeat(3000);
    }
    let mut config = config();
    config.limits.draft_path = true;
    config.limits.max_tool_result_bytes = 64_000;
    config.limits.max_context_bytes = 256_000;
    config.limits.max_context_tokens = 256_000;
    config.provider.max_tokens = 16384;
    let mut state = journal_state(&input, &config);
    crate::tender_analysis::draft::preload_outline_window(&input, &mut state);
    let saved = serde_json::to_vec(&state).unwrap();
    let large = agent::evidence_delivery::select(&input, &config, &state)
        .unwrap()
        .unwrap();
    assert!(serde_json::to_vec(&large.content).unwrap().len() <= 64_000);
    for source in &input.source_units {
        assert!(crate::tender_analysis::tools::contains(
            large.coverage.text.get(&source.source_unit_revision_id),
            0,
            9000
        ));
    }
    assert!(
        state.analysis.coverage.text.is_empty(),
        "preparation grants no receipt"
    );
    let mut small_config = config.clone();
    small_config.provider.max_tokens = 2048;
    let small = agent::evidence_delivery::select(&input, &small_config, &state)
        .unwrap()
        .unwrap();
    assert!(
        serde_json::to_vec(&small.content).unwrap().len()
            < serde_json::to_vec(&large.content).unwrap().len()
    );
    for package in [&large, &small] {
        let ranges = &package.content["assigned_evidence"]["scan_ranges"];
        for evidence in package.content["assigned_evidence"]["boundary_evidence"]
            .as_array()
            .unwrap()
        {
            if evidence["tool"] == "read_source" {
                let source = &evidence["source"];
                let id = source["source_id"].as_str().unwrap();
                if source["start"] != source["end"] {
                    assert!(
                        ranges["text"][id]
                            .as_array()
                            .unwrap()
                            .contains(&json!([source["start"], source["end"]]))
                    );
                }
            }
        }
        let mut probe = state.clone();
        let mut batch = ranges.clone();
        for field in ["requirements", "references", "issues", "review_fragments"] {
            batch[field] = json!([]);
        }
        assert!(
            agent::apply(&input, &config, &mut probe, "submit_outline_scan", &batch).is_err(),
            "package preparation is not confirmation"
        );
        probe.analysis.coverage = package.coverage.clone();
        agent::apply(&input, &config, &mut probe, "submit_outline_scan", &batch).unwrap();
    }
    let mut restored: Checkpoint = serde_json::from_slice(&saved).unwrap();
    let replay = agent::evidence_delivery::select(&input, &config, &restored)
        .unwrap()
        .unwrap();
    assert_eq!(large.content, replay.content);
    let request = agent::request(&input, &config, &mut restored)
        .await
        .unwrap();
    assert!(request.len() <= config.limits.max_context_bytes);
    let body: Value = serde_json::from_slice(&request).unwrap();
    agent::evidence_delivery::confirm(&input, &config, &mut restored, &body).unwrap();
    for source in &input.source_units {
        assert!(crate::tender_analysis::tools::contains(
            restored
                .analysis
                .coverage
                .text
                .get(&source.source_unit_revision_id),
            0,
            9000
        ));
    }
    assert!(
        !crate::tender_analysis::outline_flow::scan_complete(&input, &restored.analysis.outline),
        "receipt is not semantic scan completion"
    );
}

#[tokio::test]
async fn discovery_shrinks_late_request_and_replays_only_delivered_ranges() {
    check_late_discovery_budget(false).await;
}

#[tokio::test]
async fn discovery_shrinks_on_token_admission_while_bytes_still_fit() {
    check_late_discovery_budget(true).await;
}

async fn check_late_discovery_budget(token_only: bool) {
    let mut input = rooted_catalog_input();
    input.structured_forms.clear();
    for source in &mut input.source_units {
        source.text = "技".repeat(20_000);
    }
    let mut config = config();
    config.limits.draft_path = true;
    config.limits.max_tool_result_bytes = 64_000;
    config.limits.max_context_bytes = 256_000;
    config.limits.max_history_bytes = 480_000;
    config.limits.max_context_tokens = 1_000_000;
    config.provider.max_tokens = 16_384;
    if token_only {
        config.limits.max_context_bytes = 512_000;
        config.limits.max_context_tokens = 200_000;
    }
    let mut state = journal_state(&input, &config);
    crate::tender_analysis::draft::preload_outline_window(&input, &mut state);
    let first = agent::request(&input, &config, &mut state).await.unwrap();
    let full = agent::evidence_delivery::select(&input, &config, &state)
        .unwrap()
        .unwrap();
    // A latest response is mandatory context, even after older groups are evicted.
    let first_body: Value = serde_json::from_slice(&first).unwrap();
    let first_tokens = crate::agent_runtime::chat::estimate_input_tokens(
        &first_body,
        config.limits.image_token_reserve,
        config.limits.token_safety_margin,
    )
    .unwrap();
    let padding = if token_only {
        (config.limits.max_context_tokens - first_tokens - config.provider.max_tokens as usize
            + 4_000)
            * 2
    } else {
        config.limits.max_context_bytes - first.len() + 4_000
    };
    if token_only {
        assert!(first.len() + padding + 1000 < config.limits.max_context_bytes);
        assert!(
            first_tokens + padding / 2 + config.provider.max_tokens as usize
                > config.limits.max_context_tokens
        );
    }
    state
        .transcript
        .push(json!({"role":"assistant","content":"x".repeat(padding)}));
    let request = agent::request(&input, &config, &mut state).await.unwrap();
    assert!(request.len() <= config.limits.max_context_bytes);
    let body: Value = serde_json::from_slice(&request).unwrap();
    assert!(
        crate::agent_runtime::chat::estimate_input_tokens(
            &body,
            config.limits.image_token_reserve,
            config.limits.token_safety_margin
        )
        .unwrap()
            + config.provider.max_tokens as usize
            <= config.limits.max_context_tokens
    );
    let host: Value = serde_json::from_str(
        body["messages"].as_array().unwrap().last().unwrap()["content"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let sent = &host["preloaded_evidence"];
    assert!(sent.is_object(), "must shrink rather than drop evidence");
    assert!(
        sent["assigned_evidence"]["workload_bytes_limit"]
            .as_u64()
            .unwrap()
            < full.content["assigned_evidence"]["workload_bytes_limit"]
                .as_u64()
                .unwrap()
    );
    assert!(state.analysis.coverage.text.is_empty());
    let mut recovered: Checkpoint =
        serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
    agent::evidence_delivery::confirm(&input, &config, &mut recovered, &body).unwrap();
    let delivered = recovered
        .analysis
        .coverage
        .text
        .values()
        .flatten()
        .map(|(a, b)| b - a)
        .sum::<usize>();
    let original = full
        .coverage
        .text
        .values()
        .flatten()
        .map(|(a, b)| b - a)
        .sum::<usize>();
    assert!(delivered > 0 && delivered < original);
    state.transcript =
        vec![json!({"role":"assistant","content":"x".repeat(config.limits.max_context_bytes)})];
    assert!(
        agent::request(&input, &config, &mut state).await.is_err(),
        "no room for minimum evidence must fail explicitly"
    );
    assert!(state.analysis.coverage.text.is_empty());
}

#[test]
fn completed_discovery_history_can_yield_without_losing_scan_state() {
    let input = rooted_catalog_input();
    let config = config();
    let mut state = journal_state(&input, &config);
    crate::tender_analysis::draft::preload_outline_window(&input, &mut state);
    let id = &input.source_units[0].source_unit_revision_id;
    state
        .analysis
        .coverage
        .text
        .insert(id.clone(), vec![(0, 10)]);
    state.transcript = vec![
        json!({"role":"assistant","content":"old result"}),
        json!({"role":"tool","content":json!({"ok":true,"result":{"source_id":id,"start":0,"end":10,"text":"old unique"}}).to_string()}),
        json!({"role":"assistant","content":"latest pending"}),
    ];
    assert!(!agent::context::evict_completed_discovery_history(
        &mut state, 1000
    ));
    state
        .analysis
        .outline
        .scanned
        .text
        .insert(id.clone(), vec![(0, 10)]);
    let before = serde_json::to_value(&state.analysis).unwrap();
    assert!(agent::context::evict_completed_discovery_history(
        &mut state, 1000
    ));
    assert_eq!(state.transcript.len(), 1);
    assert_eq!(serde_json::to_value(&state.analysis).unwrap(), before);
}

#[test]
fn mostly_read_large_source_does_not_exclude_following_small_source() {
    let mut input = rooted_catalog_input();
    input.source_units[0].text = "x".repeat(60_000);
    input.source_units[1].text = "资格要求".into();
    let config = config();
    let mut state = journal_state(&input, &config);
    let id = input.source_units[0].source_unit_revision_id.clone();
    state
        .analysis
        .coverage
        .text
        .insert(id.clone(), vec![(0, 59_990)]);
    state
        .analysis
        .outline
        .scanned
        .text
        .insert(id, vec![(0, 59_990)]);
    assert_eq!(
        crate::tender_analysis::draft::outline_reading_scope(&input, &state, 4096).len(),
        2
    );
}

#[tokio::test]
#[ignore = "requires KB_DISCOVERY_LOOP_DIR archived input and checkpoint; no live model or DB"]
async fn archived_discovery_loop_projects_pending_ranges_under_frozen_token_budget() {
    let root = std::path::PathBuf::from(std::env::var("KB_DISCOVERY_LOOP_DIR").unwrap());
    let frozen: Value =
        serde_json::from_slice(&std::fs::read(root.join("frozen.json")).unwrap()).unwrap();
    let input: FrozenInput = serde_json::from_value(frozen["input"].clone()).unwrap();
    let config: Config = serde_json::from_value(frozen["runtime"].clone()).unwrap();
    let mut state: Checkpoint =
        serde_json::from_slice(&std::fs::read(root.join("loop-checkpoint.json")).unwrap()).unwrap();
    let request = agent::request(&input, &config, &mut state).await.unwrap();
    let body: Value = serde_json::from_slice(&request).unwrap();
    let tokens = crate::agent_runtime::chat::estimate_input_tokens(
        &body,
        config.limits.image_token_reserve,
        config.limits.token_safety_margin,
    )
    .unwrap();
    assert!(tokens + config.provider.max_tokens as usize <= config.limits.max_context_tokens);
    let pending = agent::apply(
        &input,
        &config,
        &mut state,
        "read_outline",
        &json!({"kind":"pending_scan","offset":0,"limit":1000}),
    )
    .unwrap();
    assert!(
        pending["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["id"] == "2bf533bf-17f8-4724-aab7-006d67787506"
                && r["start"] == 0
                && r["end"] == 36)
    );
    let source = input
        .source_units
        .iter()
        .find(|s| s.source_unit_revision_id == "74715b3f-23a3-512a-823f-020b9337ded7")
        .unwrap();
    let before = state.outline_run.chunk_cursor;
    agent::apply(&input, &config, &mut state, "submit_outline_scan", &json!({
        "text":{source.source_unit_revision_id.clone():[[0,source.text.len()]]},
        "forms":{"2bf533bf-17f8-4724-aab7-006d67787506":[[0,36]]},
        "metadata":{},"empty_sources":[],"requirements":[],"references":[],"issues":[],"review_fragments":[]
    })).unwrap();
    crate::tender_analysis::draft::after_batch(&input, &mut state, false, false).unwrap();
    assert!(state.outline_run.chunk_cursor > before);
    assert!(!crate::tender_analysis::outline_flow::checked(
        &input, &state
    ));
}

#[test]
fn discovery_repeated_reads_exhaust_recovery_without_resetting_replans() {
    let input = rooted_catalog_input();
    let mut config = config();
    config.limits.draft_path = true;
    let mut state = journal_state(&input, &config);
    state.draft_stage = crate::tender_analysis::draft::DraftStage::Outline;
    crate::tender_analysis::draft::preload_outline_window(&input, &mut state);
    let source = &input.source_units[0];
    for _ in 0..30 {
        if state.main_progress.watch.recovery == crate::agent_runtime::progress::Recovery::Blocked {
            break;
        }
        agent::apply(
            &input,
            &config,
            &mut state,
            "read_source",
            &json!({
            "source_id":source.source_unit_revision_id,"start":0,"max_bytes":100}),
        )
        .unwrap();
        agent::context::observe_progress(&mut state, &Role::Main, None, &config.limits).unwrap();
    }
    assert_eq!(
        state.main_progress.watch.recovery,
        crate::agent_runtime::progress::Recovery::Blocked
    );
    assert_eq!(
        state.main_progress.watch.replans,
        config.limits.max_focus_replans
    );
    assert!(!crate::tender_analysis::outline_flow::scan_complete(
        &input,
        &state.analysis.outline
    ));
}

#[test]
fn discovery_reoffers_evicted_unsubmitted_prefix_and_keeps_hot_state_small() {
    let input = rooted_catalog_input();
    let mut config = config();
    config.limits.draft_path = true;
    let mut state = journal_state(&input, &config);
    crate::tender_analysis::draft::preload_outline_window(&input, &mut state);
    let id = &input.source_units[0].source_unit_revision_id;
    let len = input.source_units[0].text.len();
    state
        .analysis
        .coverage
        .text
        .insert(id.clone(), vec![(0, len)]);
    state
        .analysis
        .outline
        .scanned
        .text
        .insert(id.clone(), vec![(3, len)]);
    let evidence = agent::evidence_delivery::select(&input, &config, &state)
        .unwrap()
        .unwrap();
    let visible = agent::context::visible_work_evidence(
        &state,
        &[
            json!({"role":"user","content":json!({"preloaded_evidence":evidence.content}).to_string()}),
        ],
    );
    assert!(crate::tender_analysis::tools::contains(
        visible.get(&format!("text:{id}")),
        0,
        3
    ));
    let packet = crate::tender_analysis::outline_flow::packet(&input, &state, 48000).unwrap();
    assert!(packet.get("requirements").is_none());
    assert!(packet.get("requirement_count").is_some());
    assert_eq!(packet["cursor_chunk"]["source_id"], *id);
    assert!(!crate::tender_analysis::tools::contains(
        state.analysis.outline.scanned.text.get(id),
        0,
        3
    ));
}

#[test]
fn discovery_duplicate_receipts_require_actual_visible_complete_ranges() {
    let input = rooted_catalog_input();
    let config = config();
    let mut state = journal_state(&input, &config);
    crate::tender_analysis::draft::preload_outline_window(&input, &mut state);
    let id = &input.source_units[0].source_unit_revision_id;
    for (name, result) in [
        (
            "read_source",
            json!({"source_id":id,"start":0,"end":100,"text":"visible original"}),
        ),
        (
            "read_form",
            json!({"source_id":id,"form_id":"f","offset":0,"next":36,"cells":["original"]}),
        ),
        (
            "collection_index",
            json!({"kind":"documents","offset":0,"next":2,"items":["original","original"]}),
        ),
    ] {
        let messages =
            vec![json!({"role":"tool","content":json!({"ok":true,"result":result}).to_string()})];
        let visible = agent::context::visible_work_evidence(&state, &messages);
        assert!(!visible.is_empty());
        let receipt = agent::context::visible_read_receipt(&state, &visible, name, result.clone());
        assert_eq!(receipt["already_visible"], true);
        assert!(
            agent::context::visible_work_evidence(
                &state,
                &[json!({"role":"tool","content":json!({"ok":true,"result":receipt}).to_string()})]
            )
            .is_empty(),
            "short receipt must not masquerade as original evidence"
        );
        assert_eq!(
            agent::context::visible_read_receipt(&state, &Default::default(), name, result.clone()),
            result,
            "evicted and not-yet-delivered evidence must be readable"
        );
        let mut partial = visible.clone();
        partial.values_mut().next().unwrap()[0].1 -= 1;
        assert_eq!(
            agent::context::visible_read_receipt(&state, &partial, name, result.clone()),
            result,
            "partial overlap cannot suppress a missing range"
        );
    }
    assert!(state.analysis.outline.scanned.text.is_empty());
}

#[tokio::test]
#[ignore = "requires KB_DISCOVERY_LOOP_DIR cursor-118 archive; no live model or DB"]
async fn archived_cursor_118_reoffers_missing_grid_tail_without_advancing_scan() {
    let root = std::path::PathBuf::from(std::env::var("KB_DISCOVERY_LOOP_DIR").unwrap());
    let frozen: Value =
        serde_json::from_slice(&std::fs::read(root.join("frozen.json")).unwrap()).unwrap();
    let input: FrozenInput = serde_json::from_value(frozen["input"].clone()).unwrap();
    let config: Config = serde_json::from_value(frozen["runtime"].clone()).unwrap();
    let mut state: Checkpoint =
        serde_json::from_slice(&std::fs::read(root.join("latest-checkpoint.json")).unwrap())
            .unwrap();
    assert_eq!(state.outline_run.chunk_cursor, 118);
    // Archive is awaiting a response to its tool results. Simulate that delivery
    // boundary before preparing the next request; do not invent scan conclusions.
    if let Some(delivered) = state.pending_coverage.take() {
        state.replace_coverage(delivered);
    }
    crate::tender_analysis::draft::preload_outline_window(&input, &mut state);
    let before = serde_json::to_value(&state.analysis.outline.scanned).unwrap();
    let request = agent::request(&input, &config, &mut state).await.unwrap();
    let body: Value = serde_json::from_slice(&request).unwrap();
    let messages = body["messages"].as_array().unwrap();
    let visible = agent::context::visible_work_evidence(&state, messages);
    assert!(crate::tender_analysis::tools::contains(
        visible.get("form:7c726851-7e75-4055-8a15-14b69401ad45"),
        50,
        63
    ));
    let host: Value =
        serde_json::from_str(messages.last().unwrap()["content"].as_str().unwrap()).unwrap();
    assert!(host.to_string().contains("cursor_chunk"));
    let tokens = crate::agent_runtime::chat::estimate_input_tokens(
        &body,
        config.limits.image_token_reserve,
        config.limits.token_safety_margin,
    )
    .unwrap();
    assert!(tokens + config.provider.max_tokens as usize <= config.limits.max_context_tokens);
    eprintln!(
        "cursor118 request bytes={} estimated_tokens={} package_limit={}",
        request.len(),
        tokens,
        host["preloaded_evidence"]["assigned_evidence"]["workload_bytes_limit"]
    );
    agent::evidence_delivery::confirm(&input, &config, &mut state, &body).unwrap();
    assert_eq!(
        serde_json::to_value(&state.analysis.outline.scanned).unwrap(),
        before
    );
    agent::apply(&input, &config, &mut state, "submit_outline_scan", &json!({
        "text":{},
        "forms":{"7c726851-7e75-4055-8a15-14b69401ad45":[[50,63]]},"metadata":{},"empty_sources":[],"requirements":[],"references":[],"issues":[],"review_fragments":[]
    })).unwrap();
    crate::tender_analysis::draft::after_batch(&input, &mut state, false, false).unwrap();
    assert!(state.outline_run.chunk_cursor > 118);
    assert!(!crate::tender_analysis::outline_flow::checked(
        &input, &state
    ));
}

#[test]
fn duplicate_extraction_does_not_reset_scan_stall() {
    let input = draft_input();
    let mut config = config();
    config.limits.draft_path = true;
    let mut state = journal_state(&input, &config);
    state.draft_stage = crate::tender_analysis::draft::DraftStage::Outline;
    mark_window_coverage(&input, &mut state.analysis.coverage, &["source".into()]);
    for _ in 0..30 {
        agent::apply(&input,&config,&mut state,"submit_outline_scan",&json!({"text":{},"forms":{},"metadata":{},"empty_sources":[],"references":[],"issues":[],"review_fragments":[],
   "requirements":[{"id":"","description":"投标函","kind":"submission","submission_name":"投标函","classification_reason":"","format_required":false,"applicability":"required","condition":"","grounds":[{"source_id":"source","start":0,"end":3,"view_id":null,"grid_cell":null}],"format_grounds":[],"order_constraints":[]}]})).unwrap();
        let _ = agent::context::observe_progress(&mut state, &Role::Main, None, &config.limits);
    }
    assert_eq!(state.analysis.outline.requirements.len(), 1);
    assert!(state.analysis.outline.scanned.text.is_empty());
    assert!(
        state.main_progress.watch.no_progress_turns > 0 || state.main_progress.watch.replans > 0
    );
}
#[test]
fn pending_range_does_not_pin_completed_old_history() {
    let input = draft_input();
    let config = config();
    let mut state = journal_state(&input, &config);
    state.draft_stage = crate::tender_analysis::draft::DraftStage::Outline;
    state
        .analysis
        .coverage
        .text
        .insert("source".into(), vec![(0, 3)]);
    state
        .analysis
        .coverage
        .text
        .insert("pending".into(), vec![(0, 3)]);
    state
        .analysis
        .outline
        .scanned
        .text
        .insert("source".into(), vec![(0, 3)]);
    state.transcript = vec![
        json!({"role":"assistant","content":"old"}),
        json!({"role":"tool","content":json!({"ok":true,"result":{"source_id":"source","start":0,"end":3,"text":"投"}}).to_string()}),
        json!({"role":"assistant","content":"latest"}),
        json!({"role":"tool","content":json!({"ok":true,"result":{"source_id":"pending","start":0,"end":3,"text":"标"}}).to_string()}),
    ];
    assert!(agent::context::evict_completed_discovery_history(
        &mut state, 1
    ));
    assert_eq!(state.transcript.len(), 2);
    state
        .analysis
        .outline
        .scanned
        .text
        .insert("pending".into(), vec![(0, 3)]);
    assert!(!agent::context::evict_completed_discovery_history(
        &mut state, 1
    ));
}
#[test]
fn oversized_grid_cell_supports_exact_continuation() {
    let mut input = draft_input();
    input.structured_forms = vec![
        json!({"form_definition_revision_id":"f","source_unit_revision_id":"source","definition":{"kind":"grid","row_count":1,"column_count":1,"cells":[{"row":0,"column":0,"row_span":1,"col_span":1,"text":"技".repeat(20000)}]}}),
    ];
    let mut analysis = Analysis::default();
    let mut coverage = Coverage::default();
    let result = crate::tender_analysis::tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        true,
        "read_form",
        &json!({"form_id":"f","offset":0,"limit":1}),
        48000,
    );
    assert!(result.is_err());
    assert!(coverage.form_cells.is_empty());
    let mut start = 0;
    while start < 60000 {
        let out = crate::tender_analysis::tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            true,
            "read_form_cell",
            &json!({"form_id":"f","offset":0,"start":start,"max_bytes":12000}),
            16000,
        )
        .unwrap();
        start = out["end"].as_u64().unwrap() as usize;
        assert_eq!(coverage.form_cells.contains_key("f"), start == 60000);
    }
    assert_eq!(coverage.form_cells["f"], vec![(0, 1)]);
}
#[test]
fn fill_deadline_preserves_existing_heading() {
    let input = draft_input();
    let mut result = draft_result(&input);
    let mut missing = result.analysis.draft_plan[0].clone();
    missing.id = "unfilled".into();
    missing.title = "必须保留的空章".into();
    missing.order = 10;
    missing.status = DraftStatus::Omitted;
    missing.omit_reason = Some(OmitReason::Deadline);
    missing.template_id = None;
    result.analysis.draft_plan.push(missing);
    result.analysis.fill_seed_chapters =
        crate::tender_analysis::readback::seed_identities(&result.analysis.draft_plan).unwrap();
    result.review.analysis_sha256 = digest(&result.analysis).unwrap();
    let draft = crate::docx_composition::synthesize_draft_document(&input, &result).unwrap();
    let compiled = compiler::compile(&input, &result, &draft, 1_000_000).unwrap();
    assert!(docx_document_xml(&compiled.docx).contains("必须保留的空章"));
}
#[test]
fn regression_preserved_body_does_not_require_old_source_reread() {
    let mut input = draft_input();
    input.source_units.push(Source {
        source_unit_revision_id: "old-source".into(),
        document_id: "document".into(),
        text: "资格证明".into(),
        locator: json!({}),
        ordinal: 1,
    });
    let mut result = draft_result(&input);
    result.analysis.coverage.text.remove("old-source");
    let mut preserved = result.analysis.draft_plan[0].clone();
    preserved.id = "user-body".into();
    preserved.order = 1;
    preserved.title = "资格证明".into();
    preserved.template_id = None;
    preserved.grounds = vec![Span {
        source_id: "old-source".into(),
        start: 0,
        end: 12,
        view_id: None,
        grid_cell: None,
    }];
    preserved.source_ids = vec!["old-source".into()];
    preserved.preserved = vec![crate::tender_analysis::readback::Preserved::Paragraphs {
        unit_keys: vec!["u".into()],
        paragraphs: vec!["用户已经保存的证明正文".into()],
    }];
    result.analysis.draft_plan.push(preserved);
    result.analysis.fill_seed_chapters =
        crate::tender_analysis::readback::seed_identities(&result.analysis.draft_plan).unwrap();
    result.review.analysis_sha256 = digest(&result.analysis).unwrap();
    let draft = crate::docx_composition::synthesize_draft_document(&input, &result).unwrap();
    let compiled = compiler::compile(&input, &result, &draft, 1_000_000).unwrap();
    assert!(docx_document_xml(&compiled.docx).contains("用户已经保存的证明正文"));
    assert!(!result.analysis.coverage.text.contains_key("old-source"));
    result.analysis.draft_plan.last_mut().unwrap().title = "篡改".into();
    assert!(crate::docx_composition::synthesize_draft_document(&input, &result).is_err());
}

#[tokio::test]
async fn oversized_cell_auto_delivery_replays_and_only_completes_after_last_byte() {
    let mut input = draft_input();
    input.structured_forms = vec![
        json!({"form_definition_revision_id":"f","source_unit_revision_id":"source","definition":{"kind":"grid","row_count":1,"column_count":1,"cells":[{"row":0,"column":0,"row_span":1,"col_span":1,"text":"技".repeat(20000)}]}}),
    ];
    let mut config = config();
    config.limits.draft_path = true;
    config.limits.max_tool_result_bytes = 16000;
    config.limits.max_context_bytes = 256000;
    config.limits.max_context_tokens = 256000;
    config.provider.max_tokens = 8192;
    let mut state = journal_state(&input, &config);
    crate::tender_analysis::draft::preload_outline_window(&input, &mut state);
    for _ in 0..30 {
        if state.analysis.coverage.form_cells.contains_key("f") {
            break;
        }
        let before = serde_json::to_value(&state.analysis.coverage).unwrap();
        let raw = agent::request(&input, &config, &mut state).await.unwrap();
        assert_eq!(
            serde_json::to_value(&state.analysis.coverage).unwrap(),
            before
        );
        let body: Value = serde_json::from_slice(&raw).unwrap();
        let mut replay: Checkpoint =
            serde_json::from_value(serde_json::to_value(&state).unwrap()).unwrap();
        agent::evidence_delivery::confirm(&input, &config, &mut replay, &body).unwrap();
        agent::evidence_delivery::confirm(&input, &config, &mut state, &body).unwrap();
        assert_eq!(
            serde_json::to_value(&state.analysis.coverage).unwrap(),
            serde_json::to_value(&replay.analysis.coverage).unwrap()
        );
        // Simulate already submitted ordinary text; cell completion must still wait for all bytes.
        state.analysis.outline.scanned.text = state.analysis.coverage.text.clone();
        state.analysis.outline.scanned.metadata = state
            .analysis
            .coverage
            .metadata
            .iter()
            .filter(|(key, _)| !key.starts_with("form-cell:"))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        state.transcript.clear();
    }
    assert_eq!(
        state.analysis.coverage.form_cells.get("f"),
        Some(&vec![(0, 1)])
    );
    assert!(state.analysis.outline.scanned.form_cells.is_empty());
    assert!(crate::tender_analysis::tools::contains(
        state.analysis.coverage.metadata.get("form-cell:f:0"),
        0,
        60000
    ));
}

#[test]
fn discovery_completion_synchronizes_phase_cursor_and_clears_old_work() {
    let input = draft_input();
    let config = config();
    let mut state = journal_state(&input, &config);
    crate::tender_analysis::draft::preload_outline_window(&input, &mut state);
    complete_discovery(&input, &mut state);
    state.analysis.outline.phase = crate::tender_analysis::outline_flow::Phase::Discover;
    state.outline_run.phase = crate::tender_analysis::outline_flow::Phase::Discover;
    crate::tender_analysis::draft::after_batch(&input, &mut state, false, false).unwrap();
    assert_eq!(
        state.analysis.outline.phase,
        crate::tender_analysis::outline_flow::Phase::Outline
    );
    assert_eq!(state.outline_run.phase, state.analysis.outline.phase);
    assert_eq!(
        state.outline_run.chunk_cursor,
        crate::tender_analysis::draft::outline_chunks(&input).len()
    );
    assert!(state.main_work.is_none());
    assert!(
        agent::evidence_delivery::select(&input, &config, &state)
            .unwrap()
            .is_none()
    );
}

#[test]
#[ignore = "requires locally archived cursor-181 snapshot, no live model"]
fn archived_cursor181_exposes_reference_and_format_repairs_without_rescanning() {
    let dir = std::env::var("KB_CURSOR181_DIR").expect("archive directory");
    let frozen: Value =
        serde_json::from_slice(&std::fs::read(format!("{dir}/kb-cursor181-frozen.json")).unwrap())
            .unwrap();
    let input: FrozenInput = serde_json::from_value(frozen["input"].clone()).unwrap();
    let archived: Value =
        serde_json::from_slice(&std::fs::read(format!("{dir}/kb-cursor181-current.json")).unwrap())
            .unwrap();
    // Offline test of a new contract, never an upgrade of the live checkpoint.
    let mut raw = archived["state"].clone();
    raw["analysis"]["fill_seed_chapters"] = json!({});
    let mut state: Checkpoint = serde_json::from_value(raw).unwrap();
    assert_eq!(
        state.analysis.outline.phase,
        crate::tender_analysis::outline_flow::Phase::Outline
    );
    let details = crate::tender_analysis::outline_flow::apply(
        &input,
        &mut state,
        "read_outline",
        &json!({"kind":"blockers","offset":0,"limit":200}),
        48000,
    )
    .unwrap();
    assert!(
        details["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["reference_id"] == "reference-1")
    );
    assert!(
        details["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["required_format_ref"].is_object())
    );
    let before = serde_json::to_value(&state.analysis.outline.scanned).unwrap();
    let mut saved =
        serde_json::to_value(&state.analysis.outline.requirements["requirement-1"]).unwrap();
    saved["id"] = json!("requirement-1");
    crate::tender_analysis::outline_flow::apply(&input,&mut state,"submit_outline_scan",&json!({"text":{},"forms":{},"metadata":{},"empty_sources":[],"requirements":[saved],"references":[],"issues":[],"review_fragments":[]}),48000).unwrap();
    assert_eq!(
        serde_json::to_value(&state.analysis.outline.scanned).unwrap(),
        before
    );
    assert!(!crate::tender_analysis::outline_flow::checked(
        &input, &state
    ));
}

#[tokio::test]
async fn organization_support_reads_survive_cleared_work_without_widening_other_phases() {
    use crate::tender_analysis::{agent::context, draft::DraftStage, outline_flow::Phase};
    let mut input = draft_input();
    input.structured_forms.push(json!({
        "form_definition_revision_id":"form", "source_unit_revision_id":"source",
        "definition":{"schema_version":3,"kind":"grid","row_count":1,"column_count":1,
        "cells":[{"row":0,"column":0,"row_span":1,"col_span":1,"text":"报价"}]}
    }));
    let config = config();
    let mut state = journal_state(&input, &config);
    enter_outline(&input, &mut state);
    state.analysis.outline.phase = Phase::Discover;
    crate::tender_analysis::draft::after_batch(&input, &mut state, false, false).unwrap();
    assert_eq!(state.analysis.outline.phase, Phase::Outline);
    assert!(state.main_work.is_none());
    let journal = MemoryJournal::default();
    let body = agent::request(&input, &config, &mut state).await.unwrap();
    state
        .journal
        .prepare_session(
            &body,
            crate::agent_runtime::SESSION_PREFIX,
            crate::agent_runtime::ANALYSIS_SESSION_SUFFIX,
            config.limits.max_turns,
            config.limits.max_context_bytes,
        )
        .unwrap();
    state.journal.prepare(state.turn, "main", &body).unwrap();
    agent::execute_turn(
        &input,
        &config,
        &mut state,
        &journal,
        ChatTurn {
            tool_calls: vec![ChatToolCall {
                id: "organization-read".into(),
                name: "read_source".into(),
                arguments: json!({"source_id":"source","start":0,"max_bytes":1024}).to_string(),
            }],
            finish_reason: "tool_calls".into(),
            ..Default::default()
        },
        Default::default(),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    let output: Value = state
        .transcript
        .iter()
        .find(|message| message["role"] == "tool" && message["tool_call_id"] == "organization-read")
        .and_then(|message| message["content"].as_str())
        .map(|content| serde_json::from_str(content).unwrap())
        .unwrap();
    assert_eq!(output["ok"], true, "{output}");
    for (name, args) in [
        ("read_source", json!({"source_id":"source"})),
        ("read_source_view", json!({"source_id":"source"})),
        ("read_form", json!({"form_id":"form"})),
        ("read_form_cell", json!({"form_id":"form"})),
    ] {
        context::check_read_scope(&input, &state, name, &args).unwrap();
        let mut checking = state.clone();
        checking.analysis.outline.phase = Phase::Check;
        assert!(context::check_read_scope(&input, &checking, name, &args).is_err());
        let mut fill = state.clone();
        fill.draft_stage = DraftStage::Fill;
        assert!(context::check_read_scope(&input, &fill, name, &args).is_err());
    }
    assert!(
        context::check_read_scope(
            &input,
            &state,
            "read_source",
            &json!({"source_id":"missing"})
        )
        .is_err()
    );
}

#[test]
fn unknown_stable_chapter_id_rejects_entire_batch() {
    let input = draft_input();
    let config = config();
    let mut state = journal_state(&input, &config);
    enter_outline(&input, &mut state);
    let before = json!(state);
    let error = agent::apply(
        &input,
        &config,
        &mut state,
        "put_outline_items",
        &json!({
            "items":[{"id":"tmp-valid","purpose":"group","requirement_ids":[]},{"id":"chapter-999","purpose":"group","requirement_ids":[]}],"remove_ids":[]
        }),
    )
    .unwrap_err();
    assert!(error.contains("/items/1/id"), "{error}");
    assert_eq!(json!(state), before);
}

#[test]
fn frozen_wall_clock_budget_does_not_grow_with_turn_cap_or_provider_timeout() {
    let mut limits = config().limits;
    limits.max_turns = 828;
    let mut provider = config().provider.clone();
    provider.timeout_ms = 180_000;
    let config = Config::with_provider(provider, limits).unwrap();
    let frozen = serde_json::to_value(config).unwrap();
    assert_eq!(frozen["budget"]["total_timeout_secs"], 3600);
    assert_eq!(frozen["budget"]["publish_reserve_secs"], 300);
}

#[test]
fn cosmetic_organization_edits_do_not_reset_completion_watch() {
    let input = draft_input();
    let config = config();
    let mut state = journal_state(&input, &config);
    enter_outline(&input, &mut state);
    for id in ["need-a", "need-b"] {
        save_required(
            &mut state,
            id,
            id,
            "source",
            input.source_units[0].text.len(),
        );
    }
    agent::apply(
        &input,
        &config,
        &mut state,
        "put_outline_items",
        &json!({
            "items":[{"id":"tmp-first","parent":null,"order":0,"title":"投标函", "prescribed":false,
            "requirement_ids":["need-a","need-b"],"purpose":"response"}],
            "remove_ids":[]
        }),
    )
    .unwrap();
    agent::context::observe_progress(&mut state, &Role::Main, None, &config.limits).unwrap();
    let completions = state.main_progress.completions.clone();
    state.analysis.draft_plan[0].requirement_ids.reverse();
    state.analysis.draft_plan[0].id = "chapter-100".into();
    state.analysis.draft_plan[0].title = "投标响应函".into();
    agent::context::observe_progress(&mut state, &Role::Main, None, &config.limits).unwrap();
    assert_eq!(state.main_progress.completions, completions);
    assert_eq!(state.main_progress.watch.focus_turns, 1);
}

#[tokio::test]
async fn compilation_uses_a_distinct_checkpoint_and_resume_reuses_it() {
    let input = draft_input();
    let config = Config::with_provider(config().provider, config().limits).unwrap();
    let journal = MemoryJournal {
        strict_checkpoint_identity: true,
        ..Default::default()
    };
    let model = outline_publish_script(&input);
    let result = agent::run(&input, &config, &journal, &model, &CancellationToken::new())
        .await
        .unwrap();
    let saved = journal.load().await.unwrap().unwrap();
    assert_eq!(json!(saved.review), json!(result.review));
    assert_draft_object_ref(&saved);
    let calls = model.requests.lock().unwrap().len();
    let resumed = agent::run(&input, &config, &journal, &model, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(json!(result), json!(resumed));
    assert_eq!(calls, model.requests.lock().unwrap().len());
    assert_eq!(json!(saved), json!(journal.load().await.unwrap().unwrap()));
}

#[tokio::test]
async fn rejected_compilation_checkpoint_cannot_return_a_publishable_result() {
    let input = draft_input();
    let config = Config::with_provider(config().provider, config().limits).unwrap();
    let journal = MemoryJournal {
        reject_compilation_checkpoint: true,
        ..Default::default()
    };
    let error = agent::run(
        &input,
        &config,
        &journal,
        &outline_publish_script(&input),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "FROZEN_INPUT_DIGEST_MISMATCH");
    assert!(
        journal
            .load()
            .await
            .unwrap()
            .unwrap()
            .draft_docx_base64
            .is_none()
    );
}

#[tokio::test]
async fn lost_compilation_save_ack_resumes_without_a_second_compile() {
    let input = draft_input();
    let config = Config::with_provider(config().provider, config().limits).unwrap();
    let failed = MemoryJournal {
        reject_compilation_checkpoint: true,
        ..Default::default()
    };
    let model = outline_publish_script(&input);
    agent::run(&input, &config, &failed, &model, &CancellationToken::new())
        .await
        .unwrap_err();
    let committed = failed.load().await.unwrap().unwrap();
    let journal = MemoryJournal {
        strict_checkpoint_identity: true,
        fail_boundary_ack: Mutex::new(Some(committed.journal.sequence + 1)),
        state: Mutex::new(Some(committed)),
        ..Default::default()
    };
    let error = agent::run(&input, &config, &journal, &model, &CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(error.message, "lost boundary acknowledgement");
    let saved = journal.load().await.unwrap().unwrap();
    assert_draft_object_ref(&saved);
    let calls = model.requests.lock().unwrap().len();
    agent::run(&input, &config, &journal, &model, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(json!(saved), json!(journal.load().await.unwrap().unwrap()));
    assert_eq!(calls, model.requests.lock().unwrap().len());
}

#[test]
fn compact_material_batch_derives_and_refreshes_exact_basis() {
    use crate::tender_analysis::outline_flow;
    let input = draft_input();
    let config = config();
    let mut state = journal_state(&input, &config);
    enter_outline(&input, &mut state);
    save_required(&mut state, "need", "投标函", "source", 3);
    let request = json!({"items":[
        {"id":"tmp-volume","parent":null,"order":0,"title":"商务文件","purpose":"group","prescribed":false,"requirement_ids":[]},
        {"id":"tmp-letter","parent":"tmp-volume","order":0,"title":"投标函","purpose":"response","prescribed":true,"requirement_ids":["need","need"]}
    ],"remove_ids":[]});
    agent::apply(&input, &config, &mut state, "put_outline_items", &request).unwrap();
    assert_eq!(state.analysis.draft_plan[1].requirement_ids, vec!["need"]);
    let old = state.analysis.outline.requirements["need"].grounds.clone();
    for node in &state.analysis.draft_plan {
        assert_eq!(node.grounds, old);
    }
    let mut revised = json!(state.analysis.outline.requirements["need"]);
    revised["id"] = json!("need");
    revised["grounds"][0]["end"] = json!(6);
    revised["format_required"] = json!(true);
    revised["format_grounds"] = revised["grounds"].clone();
    agent::apply(
        &input,
        &config,
        &mut state,
        "submit_outline_scan",
        &json!({
            "text":{},"forms":{},"metadata":{},"empty_sources":[],"requirements":[revised],
            "references":[],"issues":[],"review_fragments":[]
        }),
    )
    .unwrap();
    for node in &state.analysis.draft_plan {
        assert_eq!(node.grounds[0].end, 6);
        assert_eq!(node.format_refs, node.grounds);
        assert!(!node.grounds.contains(&old[0]));
    }
    assert!(!outline_flow::blockers(&input, &state).contains(&"B_FORMAT_EVIDENCE_MISSING"));
}

#[test]
fn non_document_and_structure_conclusions_need_review_not_chapters() {
    use crate::tender_analysis::outline_flow::{self, NeedKind};
    let input = draft_input();
    let config = config();
    let mut state = journal_state(&input, &config);
    enter_outline(&input, &mut state);
    for (id, kind) in [
        ("upload", NeedKind::NonDocument),
        ("language", NeedKind::StructureConstraint),
    ] {
        save_required(&mut state, id, id, "source", 3);
        let need = state.analysis.outline.requirements.get_mut(id).unwrap();
        need.kind = kind;
        need.submission_name = None;
        need.classification_reason = "原文规定操作或全局编制规则".into();
    }
    assert!(!outline_flow::blockers(&input, &state).contains(&"B_REQUIREMENT_UNMAPPED"));
    outline_flow::compose_check_packets(&input, &mut state);
    assert!(
        !state.analysis.outline.checks["composition"]
            .fragment_ids
            .is_empty()
    );
    let before = outline_flow::packet_snapshot(&input, &state, "composition").unwrap();
    state
        .analysis
        .outline
        .requirements
        .get_mut("upload")
        .unwrap()
        .classification_reason = "排除理由改变".into();
    assert_ne!(
        before,
        outline_flow::packet_snapshot(&input, &state, "composition").unwrap()
    );
    let error = agent::apply(&input,&config,&mut state,"put_outline_items",&json!({"items":[
        {"id":"tmp-upload","parent":null,"order":0,"title":"电子上传","purpose":"response","prescribed":false,"requirement_ids":["upload"]}
    ],"remove_ids":[]})).unwrap_err();
    assert!(error.contains("REQUIREMENT_DESTINATION"), "{error}");
}

#[test]
fn prescribed_format_flag_and_response_location_are_real_blockers() {
    use crate::tender_analysis::outline_flow;
    let input = draft_input();
    let config = config();
    let mut state = journal_state(&input, &config);
    enter_outline(&input, &mut state);
    save_required(&mut state, "letter", "投标函", "source", 3);
    state
        .analysis
        .outline
        .requirements
        .get_mut("letter")
        .unwrap()
        .format_required = true;
    assert!(outline_flow::blockers(&input, &state).contains(&"B_FORMAT_EVIDENCE_MISSING"));
    let before = json!(state);
    assert!(agent::apply(&input,&config,&mut state,"put_outline_items",&json!({"items":[
        {"id":"tmp-root","parent":null,"order":0,"title":"商务文件","purpose":"group","prescribed":false,"requirement_ids":["letter"]}
    ],"remove_ids":[]})).is_err());
    assert_eq!(json!(state), before);
}

#[test]
fn splitting_mixed_requirements_uses_scanned_basis_and_strict_categories() {
    let input = draft_input();
    let config = config();
    let mut state = journal_state(&input, &config);
    enter_outline(&input, &mut state);
    let mut batch = json!({"text":{},"forms":{},"metadata":{},"empty_sources":[],"references":[],"issues":[],"review_fragments":[],
        "requirements":[{"id":"","description":"提交授权书","kind":"submission","submission_name":"授权书","classification_reason":"","format_required":false,
        "applicability":"required","condition":"","grounds":[{"source_id":"source","start":0,"end":3}],"format_grounds":[],"order_constraints":[]}]});
    let result = agent::apply(&input, &config, &mut state, "submit_outline_scan", &batch).unwrap();
    assert_eq!(result["saved"][0], "requirement-1");
    batch["requirements"][0]["id"] = json!("requirement-1");
    batch["requirements"][0]["kind"] = json!("non_document");
    batch["requirements"][0]["submission_name"] = Value::Null;
    assert!(
        agent::apply(&input, &config, &mut state, "submit_outline_scan", &batch)
            .unwrap_err()
            .contains("classification_reason")
    );
    batch["requirements"][0]["kind"] = json!("qualification");
    assert!(
        agent::apply(&input, &config, &mut state, "submit_outline_scan", &batch)
            .unwrap_err()
            .contains("unknown variant")
    );
}

#[test]
fn requirement_replacement_is_atomic_and_reopens_affected_conclusions() {
    use crate::tender_analysis::outline_flow::{
        IssueStatus, OutlineIssue, OutlineReference, ReferenceImpact, ReferenceStatus,
    };
    let input = draft_input();
    let config = config();
    let mut state = journal_state(&input, &config);
    enter_outline(&input, &mut state);
    save_required(&mut state, "mixed", "投标函与授权书", "source", 3);
    agent::apply(&input,&config,&mut state,"put_outline_items",&json!({"items":[
        {"id":"tmp-response","parent":null,"order":0,"title":"投标文件","purpose":"response","prescribed":false,"requirement_ids":["mixed"]}
    ],"remove_ids":[]})).unwrap();
    let grounds = state.analysis.outline.requirements["mixed"].grounds.clone();
    state.analysis.outline.references.insert(
        "ref".into(),
        OutlineReference {
            grounds: grounds.clone(),
            target_description: "核对材料".into(),
            target_ids: vec!["source".into()],
            requirement_ids: vec!["mixed".into()],
            impact: ReferenceImpact::Structure,
            status: ReferenceStatus::Resolved,
            resolution_grounds: grounds.clone(),
        },
    );
    state.analysis.outline.issues.insert(
        "issue".into(),
        OutlineIssue {
            code: "material".into(),
            description: "材料拆分".into(),
            requirement_ids: vec!["mixed".into()],
            chapter_ids: vec![],
            reference_ids: vec![],
            grounds: grounds.clone(),
            status: IssueStatus::Resolved,
            resolution_grounds: grounds,
        },
    );
    let mut new = json!(state.analysis.outline.requirements["mixed"]);
    new["id"] = json!("tmp-letter");
    new["description"] = json!("投标函");
    new["submission_name"] = json!("投标函");
    let mut second = new.clone();
    second["id"] = json!("tmp-authority");
    second["description"] = json!("授权书");
    second["submission_name"] = json!("授权书");
    let mut batch = json!({"text":{},"forms":{},"metadata":{},"empty_sources":[],"requirements":[new,second],
        "references":[],"issues":[],"review_fragments":[],"requirement_replacements":{"mixed":["tmp-letter","unknown"]}});
    let before = json!(state);
    assert!(agent::apply(&input, &config, &mut state, "submit_outline_scan", &batch).is_err());
    assert_eq!(json!(state), before);
    batch["requirement_replacements"]["mixed"] = json!(["tmp-letter", "tmp-authority"]);
    let result = agent::apply(&input, &config, &mut state, "submit_outline_scan", &batch).unwrap();
    assert!(!state.analysis.outline.requirements.contains_key("mixed"));
    let ids = vec![
        result["id_map"]["requirements"]["tmp-letter"]
            .as_str()
            .unwrap()
            .to_string(),
        result["id_map"]["requirements"]["tmp-authority"]
            .as_str()
            .unwrap()
            .to_string(),
    ];
    assert_eq!(state.analysis.draft_plan[0].requirement_ids, ids);
    assert_eq!(
        state.analysis.outline.references["ref"].status,
        ReferenceStatus::Unresolved
    );
    assert_eq!(
        state.analysis.outline.references["ref"].requirement_ids,
        ids
    );
    assert_eq!(
        state.analysis.outline.issues["issue"].status,
        IssueStatus::Open
    );
    assert!(
        state.analysis.outline.issues["issue"]
            .resolution_grounds
            .is_empty()
    );
    assert!(
        state
            .analysis
            .outline
            .review_fragments
            .keys()
            .any(|id| id.starts_with("replacement-"))
    );
}

fn scan_repair_fixture() -> (FrozenInput, Config, agent::Checkpoint, Value) {
    let input = draft_input();
    let config = config();
    let mut state = journal_state(&input, &config);
    enter_outline(&input, &mut state);
    let batch = json!({"text":{},"forms":{},"metadata":{},"empty_sources":[],"requirements":[
        {"id":"tmp-structure","description":"编排顺序","kind":"structure_constraint","submission_name":null,"classification_reason":"目录规则","format_required":true,
        "applicability":"required","condition":"","grounds":[{"source_id":"source","start":0,"end":3}],"format_grounds":[],"order_constraints":[]},
        {"id":"tmp-letter","description":"授权材料","kind":"submission","submission_name":"授权书","classification_reason":"","format_required":false,
        "applicability":"conditional","condition":"","grounds":[{"source_id":"source","start":0,"end":3}],"format_grounds":[],"order_constraints":[]}
    ],"references":[],"issues":[],"review_fragments":[
        {"id":"tmp-fragment","kind":"composition","document_id":"document","volume_ids":[],"span":{"source_id":"source","start":0,"end":3}}
    ]});
    (input, config, state, batch)
}

fn remember_scan(state: &mut agent::Checkpoint, id: &str, args: &Value, output: Value) {
    state.transcript.push(json!({"role":"assistant","tool_calls":[{"id":id,"type":"function","function":{"name":"submit_outline_scan","arguments":args.to_string()}}]}));
    state
        .transcript
        .push(json!({"role":"tool","tool_call_id":id,"content":output.to_string()}));
}

#[test]
fn scan_returns_all_field_errors_and_compact_repair_survives_restore() {
    let (input, config, mut state, batch) = scan_repair_fixture();
    let before = json!(state.analysis);
    let message =
        agent::apply(&input, &config, &mut state, "submit_outline_scan", &batch).unwrap_err();
    let details: Value = serde_json::from_str(&message).unwrap();
    assert_eq!(details["error_count"], 2);
    assert_eq!(
        details["errors"][0]["path"],
        "/requirements/0/format_required"
    );
    assert_eq!(details["errors"][1]["path"], "/requirements/1/condition");
    assert_eq!(json!(state.analysis), before);
    remember_scan(
        &mut state,
        "failed-scan",
        &batch,
        json!({"ok":false,"details":details}),
    );
    let mut restored: agent::Checkpoint = serde_json::from_value(json!(state)).unwrap();
    let repair = json!({"repair":{"call_id":"failed-scan","arguments_sha256":details["arguments_sha256"],"changes":[
        {"path":"/requirements/0/format_required","value":false},
        {"path":"/requirements/1/condition","value":"委托代理人签字时"}
    ]}});
    assert!(repair.to_string().len() < batch.to_string().len() / 2);
    let output = agent::apply(
        &input,
        &config,
        &mut restored,
        "submit_outline_scan",
        &repair,
    )
    .unwrap();
    assert!(output["id_map"]["review_fragments"]["tmp-fragment"].is_string());
    assert_eq!(restored.analysis.outline.requirements.len(), 2);
    remember_scan(
        &mut restored,
        "repair-success",
        &repair,
        json!({"ok":true,"result":output}),
    );
    assert!(
        agent::apply(
            &input,
            &config,
            &mut restored,
            "submit_outline_scan",
            &repair
        )
        .unwrap_err()
        .contains("no failed scan")
    );
}

#[test]
fn scan_repair_rejects_stale_identity_digest_and_outside_paths_atomically() {
    let (input, config, mut state, batch) = scan_repair_fixture();
    remember_scan(&mut state, "failed", &batch, json!({"ok":false}));
    let before = json!(state);
    let valid = json!({"repair":{"call_id":"failed","arguments_sha256":digest(&batch).unwrap(),"changes":[{"path":"/requirements/0/format_required","value":false}]}});
    for (path, value) in [
        ("/repair/call_id", json!("old")),
        ("/repair/arguments_sha256", json!("bad")),
        ("/repair/changes/0/path", json!("/analysis/coverage")),
    ] {
        let mut bad = valid.clone();
        *bad.pointer_mut(path).unwrap() = value;
        assert!(agent::apply(&input, &config, &mut state, "submit_outline_scan", &bad).is_err());
        assert_eq!(json!(state), before);
    }
}

#[test]
fn latest_failed_scan_chain_is_pinned_until_success() {
    use crate::tender_analysis::{agent::context, outline_flow};
    let (input, config, mut state, batch) = scan_repair_fixture();
    remember_scan(&mut state, "base", &batch, json!({"ok":false}));
    let repair = json!({"repair":{"call_id":"base","arguments_sha256":digest(&batch).unwrap(),"changes":[{"path":"/requirements/0/format_required","value":false}]}});
    let message =
        agent::apply(&input, &config, &mut state, "submit_outline_scan", &repair).unwrap_err();
    let details: Value = serde_json::from_str(&message).unwrap();
    assert_eq!(details["error_count"], 1);
    remember_scan(
        &mut state,
        "second",
        &repair,
        json!({"ok":false,"details":details}),
    );
    state
        .transcript
        .push(json!({"role":"assistant","tool_calls":[]}));
    assert!(outline_flow::protects_scan_repair(
        &state,
        &state.transcript
    ));
    let before = json!(state.transcript);
    context::evict_completed_discovery_history(&mut state, 1);
    context::evict_delivered_group(&mut state, 1, true);
    assert_eq!(json!(state.transcript), before);
    let final_repair = json!({"repair":{"call_id":"second","arguments_sha256":details["arguments_sha256"],"changes":[{"path":"/requirements/1/condition","value":"代理人签字时"}]}});
    let result = agent::apply(
        &input,
        &config,
        &mut state,
        "submit_outline_scan",
        &final_repair,
    )
    .unwrap();
    remember_scan(
        &mut state,
        "done",
        &final_repair,
        json!({"ok":true,"result":result}),
    );
    assert!(!outline_flow::protects_scan_repair(
        &state,
        &state.transcript
    ));
}

#[test]
fn all_scan_categories_share_alias_rules_and_keep_associations() {
    let (input, config, mut state, mut batch) = scan_repair_fixture();
    batch["requirements"][0]["format_required"] = json!(false);
    batch["requirements"][1]["condition"] = json!("代理签字");
    batch["references"] = json!([{"id":"tmp-ref","grounds":[{"source_id":"source","start":0,"end":3}],"target_description":"格式","target_ids":["source"],"requirement_ids":["tmp-letter"],"impact":"content","status":"unresolved","resolution_grounds":[]}]);
    batch["issues"] = json!([{"id":"tmp-issue","code":"W_CONTENT_REFERENCE","description":"待确认","requirement_ids":["tmp-letter"],"chapter_ids":[],"reference_ids":["tmp-ref"],"grounds":[],"status":"open","resolution_grounds":[]}]);
    let result = agent::apply(&input, &config, &mut state, "submit_outline_scan", &batch).unwrap();
    let map = &result["id_map"];
    let issue = &state.analysis.outline.issues[map["issues"]["tmp-issue"].as_str().unwrap()];
    assert_eq!(
        issue.reference_ids[0],
        map["references"]["tmp-ref"].as_str().unwrap()
    );
    assert_eq!(
        issue.requirement_ids[0],
        map["requirements"]["tmp-letter"].as_str().unwrap()
    );
}

#[tokio::test]
async fn scan_turn_reports_repair_identity_and_progress_without_committing() {
    let (input, config, mut state, batch) = scan_repair_fixture();
    let before = json!(state.analysis.outline.scanned);
    let body = agent::request(&input, &config, &mut state).await.unwrap();
    state
        .journal
        .prepare_session(
            &body,
            crate::agent_runtime::SESSION_PREFIX,
            crate::agent_runtime::ANALYSIS_SESSION_SUFFIX,
            config.limits.max_turns,
            config.limits.max_context_bytes,
        )
        .unwrap();
    state.journal.prepare(state.turn, "main", &body).unwrap();
    agent::execute_turn(
        &input,
        &config,
        &mut state,
        &MemoryJournal::default(),
        ChatTurn {
            tool_calls: vec![ChatToolCall {
                id: "failed-scan".into(),
                name: "submit_outline_scan".into(),
                arguments: batch.to_string(),
            }],
            finish_reason: "tool_calls".into(),
            ..Default::default()
        },
        Default::default(),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    let output: Value = state
        .transcript
        .iter()
        .find(|m| m["role"] == "tool" && m["tool_call_id"] == "failed-scan")
        .and_then(|m| m["content"].as_str())
        .map(|s| serde_json::from_str(s).unwrap())
        .unwrap();
    assert_eq!(output["error"], "SCAN_BATCH_INVALID");
    assert_eq!(output["details"]["call_id"], "failed-scan");
    assert_eq!(output["details"]["error_count"], 2);
    assert_eq!(json!(state.analysis.outline.scanned), before);
    assert_eq!(state.progress(&input)["outline_scan_repair"], true);
    assert_eq!(
        state.progress(&input)["outline_scan_cursor"],
        state.outline_run.chunk_cursor
    );
}

#[test]
fn scan_error_feedback_is_bounded_and_does_not_commit() {
    let (input, mut config, mut state, mut batch) = scan_repair_fixture();
    config.limits.max_tool_result_bytes = 1024;
    let item = batch["requirements"][0].clone();
    batch["requirements"] = json!(
        (0..40)
            .map(|i| {
                let mut item = item.clone();
                item["id"] = json!(format!("tmp-{i}"));
                item
            })
            .collect::<Vec<_>>()
    );
    let before = json!(state);
    let error =
        agent::apply(&input, &config, &mut state, "submit_outline_scan", &batch).unwrap_err();
    assert!(
        error.len() <= config.limits.max_tool_result_bytes,
        "{}",
        error.len()
    );
    let feedback: Value = serde_json::from_str(&error).unwrap();
    assert_eq!(feedback["error_count"], 40);
    assert_eq!(feedback["truncated"], true);
    assert!(!feedback["errors"].as_array().unwrap().is_empty());
    assert_eq!(json!(state), before);
}

#[test]
fn chapter_batch_aggregates_destinations_and_repairs_after_restore() {
    let input = draft_input();
    let config = config();
    let mut state = journal_state(&input, &config);
    enter_outline(&input, &mut state);
    save_required(&mut state, "letter", "投标函", "source", 3);
    save_required(&mut state, "excluded", "中标后保函", "source", 3);
    state
        .analysis
        .outline
        .requirements
        .get_mut("excluded")
        .unwrap()
        .applicability = crate::tender_analysis::outline_flow::Applicability::NotApplicable;
    let batch = json!({"items":[
        {"id":"tmp-letter","parent":null,"order":0,"title":"投标函","purpose":"group","prescribed":false,"requirement_ids":["letter"]},
        {"id":"tmp-volume","parent":null,"order":1,"title":"商务文件","purpose":"response","prescribed":false,"requirement_ids":["excluded"]}
    ],"remove_ids":[]});
    let before = json!(state);
    let error = agent::apply(&input, &config, &mut state, "put_outline_items", &batch).unwrap_err();
    let details: Value = serde_json::from_str(&error).unwrap();
    assert_eq!(details["error_count"], 2);
    assert_eq!(json!(state), before);
    remember_scan(
        &mut state,
        "chapter-failure",
        &batch,
        json!({"ok":false,"details":details}),
    );
    state
        .transcript
        .iter_mut()
        .find(|m| m["role"] == "assistant")
        .unwrap()["tool_calls"][0]["function"]["name"] = json!("put_outline_items");
    let mut state: agent::Checkpoint = serde_json::from_value(json!(state)).unwrap();
    assert!(crate::tender_analysis::outline_flow::protects_scan_repair(
        &state,
        &state.transcript
    ));
    let repair = json!({"repair":{"call_id":"chapter-failure","arguments_sha256":details["arguments_sha256"],"changes":[
        {"path":"/items/0/purpose","value":"response"},
        {"path":"/items/1/purpose","value":"group"},
        {"path":"/items/1/requirement_ids","value":[]}
    ]}});
    agent::apply(&input, &config, &mut state, "put_outline_items", &repair).unwrap();
    assert_eq!(state.analysis.draft_plan.len(), 2);
    assert!(state.analysis.outline.requirements.contains_key("excluded"));
    let page = agent::apply(
        &input,
        &config,
        &mut state,
        "read_outline",
        &json!({"kind":"organization","offset":0,"limit":20}),
    )
    .unwrap();
    assert!(page.to_string().contains("review_exclusion"));
    assert!(!page.to_string().contains("grounds"));
}

#[test]
fn chapter_final_candidate_preserves_only_unchanged_check_snapshots() {
    use crate::tender_analysis::outline_flow;
    let input = draft_input();
    let config = config();
    let mut state = journal_state(&input, &config);
    enter_outline(&input, &mut state);
    save_required(&mut state, "letter", "投标函", "source", 3);
    let mut batch = json!({"items":[{"id":"tmp-letter","parent":null,"order":0,"title":"投标函","purpose":"response","prescribed":false,"requirement_ids":["letter"]}],"remove_ids":[]});
    let result = agent::apply(&input, &config, &mut state, "put_outline_items", &batch).unwrap();
    batch["items"][0]["id"] = result["id_map"]["tmp-letter"].clone();
    outline_flow::compose_check_packets(&input, &mut state);
    for id in state
        .analysis
        .outline
        .checks
        .keys()
        .cloned()
        .collect::<Vec<_>>()
    {
        let sha = outline_flow::packet_snapshot(&input, &state, &id).unwrap();
        let check = state.analysis.outline.checks.get_mut(&id).unwrap();
        check.status = "pass".into();
        check.snapshot_sha256 = sha;
    }
    let saved = json!(state.analysis.outline.checks);
    agent::apply(&input, &config, &mut state, "put_outline_items", &batch).unwrap();
    assert_eq!(json!(state.analysis.outline.checks), saved);
    batch["items"][0]["title"] = json!("投标函及说明");
    agent::apply(&input, &config, &mut state, "put_outline_items", &batch).unwrap();
    assert!(
        state
            .analysis
            .outline
            .checks
            .values()
            .any(|c| c.status != "pass")
    );
}

#[test]
fn scan_preflight_reports_metadata_and_empty_ranges_together() {
    let (input, config, mut state, mut batch) = scan_repair_fixture();
    batch["requirements"] = json!([]);
    batch["review_fragments"] = json!([]);
    batch["metadata"] = json!({"source_units":[[0,1]]});
    batch["text"] = json!({"source":[[0,0]]});
    let before = json!(state);
    let failure =
        agent::apply(&input, &config, &mut state, "submit_outline_scan", &batch).unwrap_err();
    let feedback: Value = serde_json::from_str(&failure).unwrap();
    assert_eq!(feedback["error_count"], 2);
    assert!(
        feedback["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["path"] == "/metadata/source_units")
    );
    assert!(
        feedback["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["path"] == "/text/source/0")
    );
    assert_eq!(json!(state), before);
}

#[test]
fn repaired_discovery_returns_to_checks_without_erasing_scan_or_approving() {
    use crate::tender_analysis::{draft, outline_flow};
    let input = draft_input();
    let config = config();
    let mut state = journal_state(&input, &config);
    enter_outline(&input, &mut state);
    save_required(&mut state, "letter", "投标函", "source", 3);
    agent::apply(&input,&config,&mut state,"put_outline_items",&json!({"items":[{"id":"tmp-letter","parent":null,"order":0,"title":"投标函","purpose":"response","prescribed":false,"requirement_ids":["letter"]}],"remove_ids":[]})).unwrap();
    agent::apply(&input, &config, &mut state, "finish_outline", &json!({})).unwrap();
    let scanned = json!(state.analysis.outline.scanned);
    state.analysis.outline.phase = outline_flow::Phase::Discover;
    state.outline_run.phase = outline_flow::Phase::Discover;
    let mut blocked = state.clone();
    blocked.analysis.draft_plan.clear();
    draft::after_batch(&input, &mut blocked, false, false).unwrap();
    assert_eq!(blocked.analysis.outline.phase, outline_flow::Phase::Outline);
    assert!(!blocked.analysis.outline.checks.is_empty());
    assert_eq!(blocked.progress(&input)["outline_repairing"], true);
    draft::after_batch(&input, &mut state, false, false).unwrap();
    assert_eq!(state.analysis.outline.phase, outline_flow::Phase::Check);
    assert_eq!(json!(state.analysis.outline.scanned), scanned);
    assert!(
        state
            .analysis
            .outline
            .checks
            .values()
            .any(|check| check.status != "pass")
    );
    assert!(!state.done);
    let mut restored: agent::Checkpoint = serde_json::from_value(json!(state)).unwrap();
    draft::after_batch(&input, &mut restored, false, false).unwrap();
    assert_eq!(restored.analysis.outline.phase, outline_flow::Phase::Check);
    assert!(!restored.done);
}
