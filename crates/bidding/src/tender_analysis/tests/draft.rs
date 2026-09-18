use super::*;
use crate::agent_error::AgentError;
use crate::docx_composition::compiler;
use crate::tender_analysis::agent;
use crate::tender_analysis::draft::{
    BodyStatus, ChapterPurpose, DraftPlanItem, DraftStatus, OmitReason, bind_source_ids,
    draft_should_publish_partial, filled_templates_present, mark_window_coverage, merge_regions,
    plan_ready, reject_blank_erasing_source, split_windows,
};
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use knowledge::models::{ChatToolCall, ChatTurn};
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
fn outline_rejects_concatenated_list_title() {
    let input = draft_input();
    let mut limits = config().limits;
    limits.draft_path = true;
    let config = Config::with_provider(config().provider.clone(), limits).unwrap();
    let mut state = journal_state(&input, &config);
    enter_outline(&input, &mut state);
    mark_window_coverage(&input, &mut state.analysis.coverage, &["source".into()]);
    let end = input.source_units[0].text.len();
    let err = agent::apply(
        &input,
        &config,
        &mut state,
        "put_outline_item",
        &json!({
            "id":null,"parent":null,"order":0,
            "title":"投标函(附件 1A)、法定代表人身份证明(附件 1B)、授权委托书(附件 1C)",
            "prescribed":true,
            "grounds":[{"source_id":"source","start":0,"end":end,"view_id":null,"grid_cell":null}]
        }),
    )
    .unwrap_err();
    assert!(err.contains("multiple composition items"), "{err}");
    agent::apply(
        &input,
        &config,
        &mut state,
        "put_outline_item",
        &json!({
            "id":null,"parent":null,"order":0,
            "title":"法定代表人（单位负责人）身份证明(附件 1B)",
            "prescribed":true,
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
                "grounds":[{"source_id":"source","start":0,"end":end,"view_id":null,"grid_cell":null}]
            }),
        )
    };
    put(&mut state, Value::Null, Value::Null, 1, "商务文件").unwrap();
    put(&mut state, Value::Null, json!("outline-1"), 1, "投标函").unwrap();
    let err = put(&mut state, Value::Null, json!("outline-1"), 1, "授权委托书").unwrap_err();
    assert!(err.contains("sibling order"), "{err}");
    put(
        &mut state,
        json!("outline-2"),
        json!("outline-1"),
        1,
        "投标函",
    )
    .unwrap();
    put(&mut state, Value::Null, json!("outline-1"), 2, "授权委托书").unwrap();
    put(&mut state, Value::Null, Value::Null, 1, "技术文件").unwrap_err();
    put(&mut state, Value::Null, Value::Null, 2, "技术文件").unwrap();
}

#[test]
/// 根节点也要查子标题：招标原文的 `heading_path` 点了子结构，只写分册名不算写完。
fn outline_requires_children_where_source_headings_show_them() {
    let input = rooted_catalog_input();
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
        &json!({"id":"vol","parent":null,"order":0,"title":"商务文件","prescribed":true,"grounds":grounds(0)}),
    )
    .unwrap();
    crate::tender_analysis::draft::after_batch(&input, &mut state, false, false).unwrap();
    assert_eq!(
        state.draft_stage,
        crate::tender_analysis::draft::DraftStage::Outline,
        "只写分册名不算写完大纲"
    );
    assert!(
        state
            .main_work
            .as_ref()
            .unwrap()
            .note
            .contains("资格审查资料"),
        "{}",
        state.main_work.as_ref().unwrap().note
    );
    agent::apply(
        &input,
        &config,
        &mut state,
        "put_outline_item",
        &json!({"id":"qual","parent":"vol","order":0,"title":"资格审查资料","prescribed":true,"grounds":grounds(1)}),
    )
    .unwrap();
    crate::tender_analysis::draft::after_batch(&input, &mut state, false, false).unwrap();
    assert_eq!(
        state.draft_stage,
        crate::tender_analysis::draft::DraftStage::Published
    );
}

/// 组成条款枚举覆盖：条款列了三项，只写两项不算写完大纲。
#[test]
fn outline_requires_every_composition_clause_item() {
    let mut input = draft_input();
    input.source_units[0].text =
        "投标文件由下列文件组成：投标函、法定代表人身份证明、投标保证金。".into();
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
    let grounds = json!([{
        "source_id":"source","start":0,"end":input.source_units[0].text.len(),
        "view_id":null,"grid_cell":null
    }]);
    let write = |state: &mut Checkpoint, id: &str, order: usize, title: &str| {
        agent::apply(
            &input,
            &config,
            state,
            "put_outline_item",
            &json!({"id":id,"parent":null,"order":order,"title":title,"prescribed":true,"grounds":grounds}),
        )
        .unwrap();
    };
    write(&mut state, "ch-letter", 0, "投标函");
    write(&mut state, "ch-id", 1, "法定代表人身份证明");
    crate::tender_analysis::draft::after_batch(&input, &mut state, false, false).unwrap();
    assert_eq!(
        state.draft_stage,
        crate::tender_analysis::draft::DraftStage::Outline,
        "组成条款还有一项没落树"
    );
    assert!(
        state
            .main_work
            .as_ref()
            .unwrap()
            .note
            .contains("投标保证金"),
        "{}",
        state.main_work.as_ref().unwrap().note
    );
    // 有据 omitted 也算映射到了，不必非写成活节点。
    agent::apply(
        &input,
        &config,
        &mut state,
        "omit_outline_item",
        &json!({"id":null,"title":"投标保证金","reason":"not_applicable","grounds":grounds}),
    )
    .unwrap();
    crate::tender_analysis::draft::after_batch(&input, &mut state, false, false).unwrap();
    assert_eq!(
        state.draft_stage,
        crate::tender_analysis::draft::DraftStage::Published
    );
}

/// 顺序正确性：宿主按条款枚举顺序重排同级 order，模型写反了也纠回来。
#[test]
fn outline_order_follows_composition_clause() {
    let mut input = draft_input();
    input.source_units[0].text = "投标文件由下列文件组成：投标函、法定代表人身份证明。".into();
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
            &json!({"id":id,"parent":null,"order":order,"title":title,"prescribed":true,"grounds":grounds}),
        )
        .unwrap();
    }
    crate::tender_analysis::draft::after_batch(&input, &mut state, false, false).unwrap();
    let order = |id: &str| {
        state
            .analysis
            .draft_plan
            .iter()
            .find(|item| item.id == id)
            .expect(id)
            .order
    };
    assert_eq!(order("ch-letter"), 0, "条款先列投标函");
    assert_eq!(order("ch-id"), 1);
}

/// 指定格式/附表覆盖：被点名的表必须有对应节点。
#[test]
fn outline_requires_prescribed_form_titles() {
    let mut input = draft_input();
    input.structured_forms = vec![json!({
        "source_unit_revision_id":"source",
        "definition":{"title":"开标一览表"}
    })];
    let plan = vec![filled_item("tpl")];
    let gaps = crate::tender_analysis::draft::outline_gaps(&input, &plan);
    assert!(
        gaps.iter()
            .any(|gap| gap.missing.iter().any(|m| m == "开标一览表")),
        "点名的表没落树必须算缺口"
    );
    assert!(!crate::tender_analysis::draft::outline_ready(&input, &plan));
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
    crate::tender_analysis::draft::after_batch(&input, &mut state, false, false).unwrap();
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
    let list_end = input.source_units[0].text.len();
    for (order, title) in [
        (0usize, "投标函(附件 1A)"),
        (1usize, "法定代表人身份证明(附件 1B)"),
    ] {
        agent::apply(
            &input,
            &config,
            &mut state,
            "put_outline_item",
            &json!({
                "id":null,"parent":null,"order":order,"title":title,"prescribed":true,
                "grounds":[{"source_id":"list","start":0,"end":list_end,"view_id":null,"grid_cell":null}]
            }),
        )
        .unwrap();
    }
    assert!(crate::tender_analysis::draft::assign_next_chapter(
        &input, &mut state
    ));
    assert_eq!(state.draft_active_id.as_deref(), Some("outline-1"));
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
        "put_chapter_omission",
        &json!({
            "chapter_id":"outline-1",
            "reason":"bind_failed",
            "summary":"招标文件未给出该章格式",
            "grounds":[{"source_id":"source","start":0,"end":3,"view_id":null,"grid_cell":null}]
        }),
    )
    .unwrap();
    assert_eq!(state.analysis.draft_plan[0].status, DraftStatus::Omitted);
    assert_eq!(
        state.analysis.draft_plan[0].omit_reason,
        Some(OmitReason::WindowExceeded)
    );
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
            "put_chapter_omission",
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

/// 正文超出字节上限时降级出稿：保留全部标题，退回部分正文，不报错（A13）。
#[test]
fn oversized_draft_degrades_to_fewer_bodies_instead_of_failing() {
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
    assert!(full.degraded.is_empty(), "宽裕预算不该降级");
    let budget = full.compiled.docx.len() - 1;
    let tight = crate::tender_analysis::draft::compile_draft(&input, &result, budget).unwrap();
    assert!(!tight.degraded.is_empty(), "超限必须降级而不是报错");
    assert!(tight.compiled.docx.len() <= budget);
    let xml = docx_document_xml(&tight.compiled.docx);
    assert!(
        xml.contains("投标函") && xml.contains("法定代表人身份证明"),
        "降级只退正文，标题一章都不能少: {xml}"
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

#[test]
fn assign_next_chapter_skips_volume_with_children() {
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
    ];
    assert!(crate::tender_analysis::draft::assign_next_chapter(
        &input, &mut state
    ));
    assert_eq!(state.draft_active_id.as_deref(), Some("biz-1c"));
    state.analysis.draft_plan[1].status = DraftStatus::Filled;
    assert!(!crate::tender_analysis::draft::assign_next_chapter(
        &input, &mut state
    ));
}

/// 阶段一的快是**设计出来的**：按窗数推导的帽在目标回合数内就闭合，兜底上限只防
/// 跑飞。所以「20 回合」是目标而不是闸门——难文档宁可多跑几轮把大纲写全，也不为凑
/// 数字砍成半张大纲；整体填充另记一套账，不受这个目标约束。
#[test]
fn outline_meets_the_turn_target_by_design_not_by_truncation() {
    use crate::tender_analysis::draft::{
        FILL_MAX_TURNS, OUTLINE_MAX_TURNS, OUTLINE_TURN_TARGET, fill_turn_cap, outline_turn_cap,
    };
    for input in [
        draft_input(),
        rooted_catalog_input(),
        large_input_with_named_items(),
    ] {
        let cap = outline_turn_cap(&input);
        assert!(
            cap <= OUTLINE_TURN_TARGET,
            "按窗数推导的帽应当落在目标内：{cap} > {OUTLINE_TURN_TARGET}"
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
fn numbered_heading_rejects_unrelated_child() {
    let input = draft_input();
    let config = config();
    let mut state = journal_state(&input, &config);
    cover(&input, &mut state.analysis);
    let grounds = json!([{"source_id":"source","start":0,"end":input.source_units[0].text.len(),"view_id":null,"grid_cell":null}]);
    agent::apply(
        &input,
        &config,
        &mut state,
        "put_outline_item",
        &json!({"id":"ch8","parent":null,"order":8,"title":"8. 包装及运输","prescribed":true,"grounds":grounds}),
    )
    .unwrap();
    let err = agent::apply(
        &input,
        &config,
        &mut state,
        "put_outline_item",
        &json!({"id":"ex","parent":"ch8","order":1,"title":"业绩要求","prescribed":true,"grounds":grounds}),
    )
    .unwrap_err();
    assert!(
        err.contains("child title must belong under that parent"),
        "{err}"
    );
    agent::apply(
        &input,
        &config,
        &mut state,
        "put_outline_item",
        &json!({"id":"ch81","parent":"ch8","order":1,"title":"8.1 大件运输","prescribed":true,"grounds":grounds}),
    )
    .unwrap();
}

/// 招标原文在**根**一级就点了子结构：`heading_path` 写着「商务文件 > 资格审查资料」。
/// 与 `agent::run` 的入口一致：进入大纲阶段并投第一窗。
fn enter_outline(input: &FrozenInput, state: &mut agent::Checkpoint) {
    state.draft_stage = crate::tender_analysis::draft::DraftStage::Outline;
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
    let grounds_biz = json!([{"source_id":"biz","start":0,"end":input.source_units[0].text.len(),"view_id":null,"grid_cell":null}]);
    let grounds_qual = json!([{"source_id":"qual","start":0,"end":input.source_units[1].text.len(),"view_id":null,"grid_cell":null}]);
    agent::apply(
        &input,
        &config,
        &mut state,
        "put_outline_item",
        &json!({"id":"vol","parent":null,"order":0,"title":"商务文件","prescribed":true,"grounds":grounds_biz}),
    )
    .unwrap();
    agent::apply(
        &input,
        &config,
        &mut state,
        "put_outline_item",
        &json!({"id":"qual","parent":"vol","order":1,"title":"资格审查资料","prescribed":true,"grounds":grounds_qual}),
    )
    .unwrap();
    crate::tender_analysis::draft::after_batch(&input, &mut state, false, false).unwrap();
    assert_eq!(
        state.draft_stage,
        crate::tender_analysis::draft::DraftStage::Outline
    );
    assert!(
        state.main_work.as_ref().unwrap().note.contains("8A 摘要表"),
        "{}",
        state.main_work.as_ref().unwrap().note
    );
    agent::apply(
        &input,
        &config,
        &mut state,
        "put_outline_item",
        &json!({"id":"a8","parent":"qual","order":1,"title":"8A 摘要表","prescribed":true,"grounds":grounds_qual}),
    )
    .unwrap();
    crate::tender_analysis::draft::after_batch(&input, &mut state, false, false).unwrap();
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
    assert!(outline.len() <= 6 && fill.len() <= 6);
    for forbidden in [
        "put_record",
        "compile_docx",
        "put_source_review",
        "inspect_analysis",
        "search_sources",
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

/// 一条路径只差窗数：2800 字样本一窗，大样本多窗，帽随窗数推导。
#[test]
fn outline_windows_scale_with_bytes_not_with_a_size_class() {
    let small = draft_input();
    assert_eq!(
        crate::tender_analysis::draft::outline_windows(&small).len(),
        1
    );
    assert_eq!(crate::tender_analysis::draft::outline_turn_cap(&small), 5);
    let large = large_catalog_input();
    let windows = crate::tender_analysis::draft::outline_windows(&large);
    assert_eq!(windows.len(), 2, "{windows:?}");
    assert_eq!(crate::tender_analysis::draft::outline_turn_cap(&large), 6);
}

/// 锚点窗优先：能当目录依据的来源排在顺序全扫 fallback 之前，且窗数有上限。
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
    assert_eq!(windows[0], vec!["format".to_string()], "{windows:?}");
    assert_eq!(
        windows.len(),
        crate::tender_analysis::draft::OUTLINE_MAX_WINDOWS,
        "顺序全扫必须有窗数上限，超限带缺口发布"
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
    agent::apply(
        &input,
        &config,
        &mut state,
        "put_outline_item",
        &json!({
            "id":null,"parent":"outline-1","order":1,"title":"组成项11","prescribed":true,
            "grounds":[{"source_id":child.source_unit_revision_id,"start":0,"end":child.text.len(),"view_id":null,"grid_cell":null}]
        }),
    )
    .unwrap();
    crate::tender_analysis::draft::after_batch(&input, &mut state, false, false).unwrap();
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
        &json!({
            "id":null,"parent":null,"order":i,"title":format!("组成项{i}"),"prescribed":true,
            "grounds":[{"source_id":source.source_unit_revision_id,"start":0,"end":source.text.len(),"view_id":null,"grid_cell":null}]
        }),
    )
    .unwrap();
}

/// 大纲帽由窗数推导；帽到期不是「算完成」，缺口要记成 `omitted(bind_failed)`。
#[test]
fn outline_cap_publishes_and_records_gaps() {
    let input = rooted_catalog_input();
    let cap = crate::tender_analysis::draft::outline_turn_cap(&input);
    assert_eq!(cap, 6, "两窗 + 4");
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
    let source = &input.source_units[0];
    agent::apply(
        &input,
        &config,
        &mut state,
        "put_outline_item",
        &json!({
            "id":"vol","parent":null,"order":0,"title":"商务文件","prescribed":true,
            "grounds":[{"source_id":source.source_unit_revision_id,"start":0,"end":source.text.len(),"view_id":null,"grid_cell":null}]
        }),
    )
    .unwrap();
    state.turn = cap - 1;
    state.draft_stage = crate::tender_analysis::draft::DraftStage::Outline;
    crate::tender_analysis::draft::after_batch(&input, &mut state, false, false).unwrap();
    assert_eq!(
        state.draft_stage,
        crate::tender_analysis::draft::DraftStage::Published,
        "帽到期必须带缺口发布，不能继续空转"
    );
    let gap = state
        .analysis
        .draft_plan
        .iter()
        .find(|item| item.title == "资格审查资料")
        .expect("缺口必须留在报告里");
    assert_eq!(gap.status, DraftStatus::Omitted);
    assert_eq!(gap.omit_reason, Some(OmitReason::BindFailed));
    assert_eq!(gap.parent.as_deref(), Some("vol"));
}

/// 修补轮连续不减少缺口就收尾，不烧满 job 级回合。
#[test]
fn outline_stalled_repair_rounds_stop_spinning() {
    let input = rooted_catalog_input();
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
    let source = &input.source_units[0];
    agent::apply(
        &input,
        &config,
        &mut state,
        "put_outline_item",
        &json!({
            "id":"vol","parent":null,"order":0,"title":"商务文件","prescribed":true,
            "grounds":[{"source_id":source.source_unit_revision_id,"start":0,"end":source.text.len(),"view_id":null,"grid_cell":null}]
        }),
    )
    .unwrap();
    for round in 0..crate::tender_analysis::draft::OUTLINE_MAX_STALLED_ROUNDS {
        crate::tender_analysis::draft::after_batch(&input, &mut state, false, false).unwrap();
        assert_eq!(
            state.draft_stage,
            crate::tender_analysis::draft::DraftStage::Outline,
            "第 {round} 轮还该派修补"
        );
    }
    crate::tender_analysis::draft::after_batch(&input, &mut state, false, false).unwrap();
    assert_eq!(
        state.draft_stage,
        crate::tender_analysis::draft::DraftStage::Published,
        "缺口连续不减就收尾"
    );
}

/// 宿主逐窗推进：一批写不完就换下一窗，模型不需要检索也不需要自己翻页。
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
    assert_eq!(
        state.main_work.as_ref().unwrap().source_scope,
        vec!["biz".to_string()],
        "第一窗只投锚点窗，不投全书"
    );
    crate::tender_analysis::draft::after_batch(&input, &mut state, false, false).unwrap();
    assert_eq!(state.draft_outline_window, 1, "有缺口且窗未走完就前进一窗");
    assert_eq!(
        state.main_work.as_ref().unwrap().source_scope,
        vec!["qual".to_string()]
    );
    assert!(
        state.analysis.coverage.text.contains_key("qual"),
        "投递即已读：窗内原文必须计入覆盖，否则写树时引用不上"
    );
}

/// S1 索引取代自由检索：请求里带全书索引投影（标出目录依据），工具集里没有 `search_sources`。
#[tokio::test]
async fn outline_request_delivers_source_index_instead_of_search() {
    let input = rooted_catalog_input();
    let mut limits = config().limits;
    limits.draft_path = true;
    limits.max_turns = 2;
    let config =
        Config::with_provider_for(config().provider.clone(), limits, Some(&input)).unwrap();
    let journal = MemoryJournal::default();
    let model = outline_volume_without_children(&input);
    agent::run(&input, &config, &journal, &model, &CancellationToken::new())
        .await
        .unwrap();
    let request = model.requests.lock().unwrap()[0].clone();
    let tools: Vec<String> = request["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["function"]["name"].as_str().unwrap().to_string())
        .collect();
    assert!(
        !tools.iter().any(|name| name == "search_sources"),
        "{tools:?}"
    );
    assert!(tools.iter().any(|name| name == "read_source"));
    let packet: Value = serde_json::from_str(
        request["messages"].as_array().unwrap().last().unwrap()["content"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let index = &packet["source_index"];
    assert_eq!(index["windows"], json!(2));
    assert_eq!(index["current_window"], json!(1));
    assert_eq!(index["total_sources"], json!(2));
    let rows = index["sources"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert!(rows.iter().all(|row| row["catalog"] == json!(true)));
    assert!(
        rows.iter().all(|row| row.get("text").is_none()),
        "索引不含正文"
    );
}

/// 定向补读：索引里列出的 id 不在当前窗也能读，读完才能当依据写树。
#[test]
fn targeted_read_outside_the_current_window_is_allowed() {
    let input = rooted_catalog_input();
    let mut limits = config().limits;
    limits.draft_path = true;
    let config =
        Config::with_provider_for(config().provider.clone(), limits, Some(&input)).unwrap();
    let mut state = journal_state(&input, &config);
    enter_outline(&input, &mut state);
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

/// 目录分母只取投标文件格式/组成条款：评标办法等章节的标题树是要求，不是投标目录。
#[test]
fn catalog_denominator_ignores_requirement_chapters() {
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
            text: "投标文件格式。".into(),
            locator: json!({"heading_path":"投标文件格式 > 商务文件"}),
            ordinal: 1,
        },
    ];
    let plan = vec![DraftPlanItem {
            grounds: vec![],
            requirement_ids: vec![],
        id: "vol".into(),
        parent: None,
        order: 0,
        title: "商务文件".into(),
        prescribed: true,
        source_ids: vec!["fmt".into()],
        windows: vec![vec!["fmt".into()]],
        window_index: 0,
        template_id: None,
        status: DraftStatus::Pending,
        purpose: ChapterPurpose::Response,
        format_refs: vec![],
        body_status: BodyStatus::Empty,
        omit_reason: None,
        preserved: vec![],
    }];
    assert!(
        crate::tender_analysis::draft::outline_ready(&input, &plan),
        "{:?}",
        crate::tender_analysis::draft::outline_gaps(&input, &plan)
            .iter()
            .map(|gap| gap.missing.clone())
            .collect::<Vec<_>>()
    );
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
    let ids: Vec<String> = input
        .source_units
        .iter()
        .map(|source| source.source_unit_revision_id.clone())
        .collect();
    mark_window_coverage(&input, &mut state.analysis.coverage, &ids);
    put_named_outline(&input, &config, &mut state, 10);
    let child = &input.source_units[11];
    agent::apply(
        &input,
        &config,
        &mut state,
        "put_outline_item",
        &json!({
            "id":null,"parent":"outline-1","order":1,"title":"组成项11","prescribed":true,
            "grounds":[{"source_id":child.source_unit_revision_id,"start":0,"end":child.text.len(),"view_id":null,"grid_cell":null}]
        }),
    )
    .unwrap();
    state.turn = 5;
    state.draft_stage = crate::tender_analysis::draft::DraftStage::Outline;
    crate::tender_analysis::draft::after_batch(&input, &mut state, false, false).unwrap();
    assert_eq!(
        state.draft_stage,
        crate::tender_analysis::draft::DraftStage::Published
    );
    assert!(state.done);
    assert!(state.draft_active_id.is_none(), "到帽也只出骨架，不填章");
}

/// 阶段一：大纲一闭合就编译骨架 Word 并结束，本 job 不进 Fill。
#[tokio::test]
async fn draft_run_stops_at_skeleton_without_filling() {
    let input = draft_input();
    let mut limits = config().limits;
    limits.draft_path = true;
    limits.max_turns = 80;
    let config = Config::with_provider(config().provider.clone(), limits).unwrap();
    let journal = MemoryJournal::default();
    let text = &input.source_units[0].text;
    let model = work_script(vec![(
        "put_outline_item",
        json!({
            "id":null,"parent":null,"order":0,"title":"投标函","prescribed":true,
            "grounds":[{"source_id":"source","start":0,"end":text.len(),"view_id":null,"grid_cell":null}]
        }),
    )]);
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
    assert!(state.turn <= 8, "draft turns {}", state.turn);
    assert_draft_object_ref(&state);
    let xml = docx_xml_from_checkpoint(&state);
    assert!(
        xml.contains("投标函"),
        "阶段一必须产出带章节标题的可编辑 DOCX: {xml}"
    );
    let bodies = model.bodies.lock().unwrap();
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
            tools.contains(&"put_outline_item".into()),
            "阶段一只跑大纲合同: {tools:?}"
        );
        assert!(
            !tools.contains(&"put_chapter_template".into()),
            "阶段一不得给出填章工具: {tools:?}"
        );
        assert!(tools.len() <= 6 && tools.len() >= 2);
        for name in tools {
            assert_ne!(name, "put_record");
            assert_ne!(name, "inspect_analysis");
            assert_ne!(name, "compile_docx");
            assert_ne!(name, "put_source_review");
        }
    }
    let host = |body: &Value| {
        let messages = body["messages"].as_array().expect("messages");
        serde_json::from_str::<Value>(
            messages.last().expect("host packet")["content"]
                .as_str()
                .expect("host json"),
        )
        .unwrap()
    };
    for body in bodies.iter() {
        let packet = host(body);
        assert!(packet.get("progress").is_some(), "{packet}");
        assert!(packet.get("work").is_some(), "{packet}");
        assert!(packet.get("source_review").is_none(), "{packet}");
        assert!(packet.get("main_dispatch").is_none(), "{packet}");
        assert!(packet.get("global_analysis_checks").is_none(), "{packet}");
        assert!(packet.get("review_findings").is_none(), "{packet}");
        assert!(packet.get("execution").is_none(), "{packet}");
        assert!(packet.get("work_state").is_none(), "{packet}");
    }
}

#[test]
fn draft_attempt_and_deadline_caps() {
    assert!(!crate::tender_analysis::draft::draft_claim_exhausted(
        false, 4
    ));
    assert!(!crate::tender_analysis::draft::draft_claim_exhausted(
        true, 2
    ));
    assert!(crate::tender_analysis::draft::draft_claim_exhausted(
        true, 3
    ));
    assert_eq!(
        crate::tender_analysis::draft::analysis_deadline_secs(true),
        20 * 60
    );
    assert_eq!(
        crate::tender_analysis::draft::analysis_deadline_secs(false),
        45 * 60
    );
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
fn sql_composition_and_export_reject_draft_analysis() {
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
    assert!(
        load.contains(
            "DOCX_COMPOSITION_INPUT_INVALID: official composition rejects draft analysis"
        )
    );
    assert!(
        prepare.contains(
            "DOCX_COMPOSITION_INPUT_INVALID: official composition rejects draft analysis"
        )
    );
    assert!(submit.contains("kb_bid_v2_create_docx_composition_request"));
    assert!(
        export
            .contains("SUBMISSION_EXPORT_CONTEXT_INVALID: official export rejects draft analysis")
    );
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
    let config =
        Config::with_provider_for(config().provider.clone(), limits, Some(&input)).unwrap();
    let journal = MemoryJournal::default();
    let model = outline_volume_without_children(&input);
    let result = agent::run(&input, &config, &journal, &model, &CancellationToken::new())
        .await
        .unwrap();
    assert_draft_skeleton_partial(&result, &journal.load().await.unwrap().unwrap());
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
    // 大纲写了两回还没闭合；save 注入 INTERNAL，等同墙钟取消 drive。
    journal.interrupt_after.lock().unwrap().replace(2);
    let model = outline_volume_without_children(&input);
    let result = agent::run(&input, &config, &journal, &model, &CancellationToken::new())
        .await
        .unwrap();
    assert_draft_skeleton_partial(&result, &journal.load().await.unwrap().unwrap());
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
    agent::apply(
        &input,
        &config,
        &mut state,
        "put_outline_item",
        &json!({
            "id":null,"parent":null,"order":0,"title":"投标函","prescribed":true,
            "grounds":[{"source_id":"source","start":0,"end":text.len(),"view_id":null,"grid_cell":null}]
        }),
    )
    .unwrap();
    state.draft_active_id = Some("outline-1".into());
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
    assert!(err.contains("draft_active_id \"outline-1\""), "{err}");
    assert!(err.contains("do not send the title"), "{err}");
}

fn outlined_letter(input: &FrozenInput) -> (agent::Config, agent::Checkpoint) {
    let mut limits = config().limits;
    limits.draft_path = true;
    let config = Config::with_provider(config().provider.clone(), limits).unwrap();
    let mut state = journal_state(input, &config);
    crate::tender_analysis::draft::preload_outline_window(input, &mut state);
    let text = input.source_units[0].text.clone();
    agent::apply(
        input,
        &config,
        &mut state,
        "put_outline_item",
        &json!({
            "id":null,"parent":null,"order":0,"title":"投标函","prescribed":true,
            "grounds":[{"source_id":"source","start":0,"end":text.len(),"view_id":null,"grid_cell":null}]
        }),
    )
    .unwrap();
    state.draft_active_id = Some("outline-1".into());
    (config, state)
}

fn letter_region(source: Value) -> Value {
    json!({
        "chapter_id":"outline-1","id":null,"title":"投标函","purpose":"格式",
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
        let saved = agent::apply(
            &input,
            &config,
            &mut state,
            "put_chapter_template",
            &letter_region(source.clone()),
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
    let saved = agent::apply(
        &input,
        &config,
        &mut state,
        "put_chapter_template",
        &letter_region(json!({
            "source_id":"source","start":0,"end":end,"view_id":null,"grid_cell":null
        })),
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

fn assert_draft_skeleton_partial(result: &AnalysisResult, state: &Checkpoint) {
    assert!(result.review.draft);
    assert_eq!(result.quality, "needs_review");
    assert!(result.analysis.dispositions.is_empty());
    let volume = result
        .analysis
        .draft_plan
        .iter()
        .find(|item| item.id == "vol-biz")
        .expect("written chapter");
    assert_eq!(
        volume.status,
        DraftStatus::Pending,
        "带缺口收尾的章仍待用户触发填充，不是 omitted"
    );
    assert_draft_object_ref(state);
    let xml = docx_xml_from_checkpoint(state);
    assert!(
        xml.contains("商务文件"),
        "到期/取消也必须留下带章节的骨架 DOCX: {xml}"
    );
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
fn outline_flow_assignment_does_not_grant_reading_receipts() {
    let input = draft_input();
    let config = config();
    let mut state = journal_state(&input, &config);
    crate::tender_analysis::draft::preload_outline_window(&input, &mut state);
    assert!(state.analysis.coverage.text.is_empty());
    assert!(state.analysis.coverage.form_cells.is_empty());
    assert!(!crate::tender_analysis::outline_flow::scan_complete(&input, &state.analysis.outline));
}

#[test]
fn outline_flow_rejects_unseen_scan_ranges() {
    let input = draft_input();
    let config = config();
    let mut state = journal_state(&input, &config);
    let args = json!({"text":{"source":[[0,input.source_units[0].text.len()]]},"forms":{},"empty_sources":[],"requirements":[],"references":[],"issues":[]});
    let before = digest(&state.analysis).unwrap();
    assert!(crate::tender_analysis::outline_flow::apply(&input, &mut state, "submit_outline_scan", &args, 8000).is_err());
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
    assert_eq!(state.analysis.outline.phase, crate::tender_analysis::outline_flow::Phase::Discover);
}

#[test]
fn outline_flow_rejects_self_and_ancestor_cycles() {
    let mut first = filled_item("unused");
    first.parent = Some(first.id.clone());
    assert!(crate::tender_analysis::outline_flow::tree_valid(&[first.clone()]).unwrap_err().contains("cycle"));
    let mut second = first.clone();
    second.id = "child".into();
    first.parent = Some(second.id.clone());
    second.parent = Some(first.id.clone());
    assert!(crate::tender_analysis::outline_flow::tree_valid(&[first, second]).unwrap_err().contains("cycle"));
}

#[test]
fn outline_flow_windows_do_not_drop_tail_sources() {
    let mut input = draft_input();
    let source = input.source_units[0].clone();
    input.source_units = (0..20).map(|i| {
        let mut s = source.clone();
        s.source_unit_revision_id = format!("source-{i}");
        s.ordinal = i;
        s.text = "正文".repeat(2000);
        s
    }).collect();
    let windows = crate::tender_analysis::draft::outline_windows(&input);
    assert_eq!(windows.len(), 20);
    assert_eq!(windows.last().unwrap(), &["source-19".to_string()]);
}
