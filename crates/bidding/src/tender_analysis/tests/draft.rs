use super::*;
use crate::agent_error::AgentError;
use crate::docx_composition::compiler;
use crate::tender_analysis::agent;
use crate::tender_analysis::draft::{
    DraftPlanItem, DraftStatus, OmitReason, bind_source_ids, draft_should_publish_partial,
    filled_templates_present, mark_window_coverage, merge_regions, omit_pending_deadline,
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
        omit_reason: None,
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
                cells: vec![],
                blank_ranges: vec![],
                instruction: String::new(),
            }],
        },
    }
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
        omit_reason: Some(OmitReason::BindFailed),
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
    crate::tender_analysis::draft::preload_outline_window(&input, &mut state);
    let windows = split_windows(&input, &["source".into(), "p2".into()]);
    assert_eq!(windows.len(), 2);
    state.analysis.draft_plan.push(DraftPlanItem {
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
        omit_reason: None,
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
    crate::tender_analysis::draft::preload_outline_window(&input, &mut state);
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
    assert!(
        err.contains("multiple composition items"),
        "{err}"
    );
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
    crate::tender_analysis::draft::preload_outline_window(&input, &mut state);
    mark_window_coverage(&input, &mut state.analysis.coverage, &["source".into()]);
    let end = input.source_units[0].text.len();
    let put = |state: &mut agent::Checkpoint, id: Value, parent: Value, order: usize, title: &str| {
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
fn large_outline_requires_children_under_every_live_volume() {
    let input = large_input_with_named_items();
    let mut limits = config().limits;
    limits.draft_path = true;
    let config =
        Config::with_provider_for(config().provider.clone(), limits, Some(&input)).unwrap();
    let mut state = journal_state(&input, &config);
    crate::tender_analysis::draft::preload_outline_window(&input, &mut state);
    let ids: Vec<String> = input
        .source_units
        .iter()
        .map(|source| source.source_unit_revision_id.clone())
        .collect();
    mark_window_coverage(&input, &mut state.analysis.coverage, &ids);
    put_named_outline(&input, &config, &mut state, 9);
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
        crate::tender_analysis::draft::DraftStage::Outline,
        "a childless live volume must block fill"
    );
}

#[test]
fn fill_prefers_format_page_over_composition_list() {
    let mut input = draft_input();
    input.source_units = vec![
        Source {
            source_unit_revision_id: "list".into(),
            document_id: "document".into(),
            text: "并由下列文件组成：(1) 投标函(附件 1A)、法定代表人身份证明(附件 1B)。"
                .into(),
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
    crate::tender_analysis::draft::preload_outline_window(&input, &mut state);
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
        &input,
        &mut state
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
    state.analysis.draft_plan.push(DraftPlanItem {
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
        omit_reason: None,
    });
    agent::apply(
        &input,
        &config,
        &mut state,
        "put_chapter_omission",
        &json!({
            "chapter_id":"outline-1",
            "reason":"bind_failed",
            "grounds":[{"source_id":"source","start":0,"end":1,"view_id":null,"grid_cell":null}]
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
        omit_reason: Some(OmitReason::Deadline),
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
            omit_reason: Some(OmitReason::WindowExceeded),
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

#[test]
fn assign_next_chapter_skips_volume_with_children() {
    let input = draft_input();
    let mut limits = config().limits;
    limits.draft_path = true;
    let config = Config::with_provider(config().provider.clone(), limits).unwrap();
    let mut state = journal_state(&input, &config);
    crate::tender_analysis::draft::preload_outline_window(&input, &mut state);
    mark_window_coverage(&input, &mut state.analysis.coverage, &["source".into()]);
    state.analysis.draft_plan = vec![
        DraftPlanItem {
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
            omit_reason: None,
        },
        DraftPlanItem {
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
            omit_reason: None,
        },
    ];
    assert!(crate::tender_analysis::draft::assign_next_chapter(
        &input,
        &mut state
    ));
    assert_eq!(state.draft_active_id.as_deref(), Some("biz-1c"));
    state.analysis.draft_plan[1].status = DraftStatus::Filled;
    assert!(!crate::tender_analysis::draft::assign_next_chapter(
        &input,
        &mut state
    ));
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
    let mut outline_read = crate::tender_analysis::draft::outline_schemas();
    outline_read.extend(crate::tender_analysis::draft::read_schemas());
    let mut fill_read = crate::tender_analysis::draft::fill_schemas();
    fill_read.extend(crate::tender_analysis::draft::read_schemas());
    assert_eq!(
        frozen.tools_read_sha256,
        digest(&outline_read).unwrap()
    );
    assert_eq!(
        frozen.fill_tools_read_sha256,
        digest(&fill_read).unwrap()
    );
    assert_ne!(frozen.tools_sha256, frozen.tools_read_sha256);
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
    assert!(body.contains("filled draft requires staged DOCX"));
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
        body.contains("tools_read_sha256") && body.contains("fill_tools_read_sha256"),
        "reserve must accept large-file read-tool hashes"
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
    let idle = body.find("draft_path}')::boolean,true) THEN").expect("draft dispatch idle");
    let owner = body.find("Main committed owner changed").expect("extract owner charge");
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
            locator: json!({"heading_path":"资格审查资料 > 8A 摘要表"}),
            ordinal: 1,
        },
    ];
    input
}

#[test]
fn large_outline_blocks_fill_until_heading_children_are_written() {
    let input = large_catalog_input();
    assert!(!crate::tender_analysis::draft::small_file(&input));
    let mut limits = config().limits;
    limits.draft_path = true;
    let config =
        Config::with_provider_for(config().provider.clone(), limits, Some(&input)).unwrap();
    let mut state = journal_state(&input, &config);
    crate::tender_analysis::draft::preload_outline_window(&input, &mut state);
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
        state
            .main_work
            .as_ref()
            .unwrap()
            .note
            .contains("8A 摘要表"),
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
        crate::tender_analysis::draft::DraftStage::Fill
    );
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

#[test]
fn large_draft_file_advertises_read_tools() {
    let mut input = draft_input();
    input.source_units[0].text = "投标函为固定格式。".repeat(400);
    assert!(!crate::tender_analysis::draft::small_file(&input));
    let names = |tools: Vec<serde_json::Value>| {
        tools
            .into_iter()
            .map(|tool| tool["function"]["name"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };
    let outline = names(crate::tender_analysis::draft::outline_schemas_for(&input));
    let fill = names(crate::tender_analysis::draft::fill_schemas_for(&input));
    assert!(outline.len() <= 6 && fill.len() <= 6);
    for required in [
        "read_source",
        "read_form",
        "search_sources",
        "read_source_view",
    ] {
        assert!(outline.iter().any(|name| name == required), "{required}");
        assert!(fill.iter().any(|name| name == required), "{required}");
    }
    for forbidden in [
        "put_record",
        "compile_docx",
        "put_source_review",
        "inspect_analysis",
    ] {
        assert!(!outline.iter().any(|name| name == forbidden), "{forbidden}");
        assert!(!fill.iter().any(|name| name == forbidden), "{forbidden}");
    }
    let mut limits = config().limits;
    limits.draft_path = true;
    Config::with_provider_for(config().provider.clone(), limits, Some(&input)).unwrap();
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
    crate::tender_analysis::draft::preload_outline_window(&input, &mut state);
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
        crate::tender_analysis::draft::DraftStage::Fill
    );
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

#[test]
fn large_outline_one_item_at_cap_waits_for_idle_turn() {
    let input = large_input_with_named_items();
    let mut limits = config().limits;
    limits.draft_path = true;
    let config =
        Config::with_provider_for(config().provider.clone(), limits, Some(&input)).unwrap();
    let mut state = journal_state(&input, &config);
    crate::tender_analysis::draft::preload_outline_window(&input, &mut state);
    let ids: Vec<String> = input
        .source_units
        .iter()
        .map(|source| source.source_unit_revision_id.clone())
        .collect();
    mark_window_coverage(&input, &mut state.analysis.coverage, &ids);
    put_named_outline(&input, &config, &mut state, 11);
    state.turn = 5;
    state.draft_stage = crate::tender_analysis::draft::DraftStage::Outline;
    crate::tender_analysis::draft::after_batch(&input, &mut state, true, false).unwrap();
    assert_eq!(
        state.draft_stage,
        crate::tender_analysis::draft::DraftStage::Outline,
        "a single composition item at the outline cap must not force fill"
    );
    crate::tender_analysis::draft::after_batch(&input, &mut state, false, false).unwrap();
    assert_eq!(
        state.draft_stage,
        crate::tender_analysis::draft::DraftStage::Outline,
        "large-file fill still requires a parent/child tree"
    );
}

#[test]
fn large_outline_volume_titles_without_children_stay_outline() {
    let input = large_input_with_named_items();
    let mut limits = config().limits;
    limits.draft_path = true;
    let config =
        Config::with_provider_for(config().provider.clone(), limits, Some(&input)).unwrap();
    let mut state = journal_state(&input, &config);
    crate::tender_analysis::draft::preload_outline_window(&input, &mut state);
    let ids: Vec<String> = input
        .source_units
        .iter()
        .map(|source| source.source_unit_revision_id.clone())
        .collect();
    mark_window_coverage(&input, &mut state.analysis.coverage, &ids);
    put_named_outline(&input, &config, &mut state, 10);
    put_named_outline(&input, &config, &mut state, 11);
    state.turn = 5;
    state.draft_stage = crate::tender_analysis::draft::DraftStage::Outline;
    crate::tender_analysis::draft::after_batch(&input, &mut state, false, false).unwrap();
    assert_eq!(
        state.draft_stage,
        crate::tender_analysis::draft::DraftStage::Outline
    );
}

#[test]
fn large_outline_two_items_at_cap_fills() {
    let input = large_input_with_named_items();
    let mut limits = config().limits;
    limits.draft_path = true;
    let config =
        Config::with_provider_for(config().provider.clone(), limits, Some(&input)).unwrap();
    let mut state = journal_state(&input, &config);
    crate::tender_analysis::draft::preload_outline_window(&input, &mut state);
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
    crate::tender_analysis::draft::after_batch(&input, &mut state, true, false).unwrap();
    assert_eq!(
        state.draft_stage,
        crate::tender_analysis::draft::DraftStage::Fill
    );
    assert_eq!(state.draft_active_id.as_deref(), Some("outline-2"));
}

#[tokio::test]
async fn draft_run_outline_then_fill_same_batch_preload() {
    let input = draft_input();
    let mut limits = config().limits;
    limits.draft_path = true;
    limits.max_turns = 80;
    let config = Config::with_provider(config().provider.clone(), limits).unwrap();
    let journal = MemoryJournal::default();
    let text = &input.source_units[0].text;
    let model = work_script(vec![
        (
            "put_outline_item",
            json!({
                "id":null,"parent":null,"order":0,"title":"投标函","prescribed":true,
                "grounds":[{"source_id":"source","start":0,"end":text.len(),"view_id":null,"grid_cell":null}]
            }),
        ),
        (
            "put_chapter_template",
            json!({
                "chapter_id":"outline-1","id":null,"title":"投标函","purpose":"格式",
                "regions":[{
                    "source":{"source_id":"source","start":0,"end":"投标函为固定格式。".len(),"view_id":null,"grid_cell":null},
                    "role":"fixed_text","form_id":null,"cells":[],"blank_ranges":[],"instruction":""
                }]
            }),
        ),
    ]);
    let result = agent::run(&input, &config, &journal, &model, &CancellationToken::new())
        .await
        .unwrap();
    assert!(result.review.draft);
    assert_eq!(result.quality, "needs_review");
    assert!(result.analysis.dispositions.is_empty());
    let state = journal.load().await.unwrap().unwrap();
    assert!(state.turn <= 8, "draft turns {}", state.turn);
    assert_draft_object_ref(&state);
    let xml = docx_xml_from_checkpoint(&state);
    assert!(
        xml.contains("投标函为固定格式"),
        "filled draft must persist unzippable DOCX bytes: {xml}"
    );
    let bodies = model.bodies.lock().unwrap();
    assert!(
        bodies.len() >= 2,
        "outline then fill need two prepared bodies"
    );
    let names = |body: &Value| {
        body["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["function"]["name"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };
    let outline_names = names(&bodies[0]);
    let fill_names = names(&bodies[1]);
    assert!(outline_names.contains(&"put_outline_item".into()));
    assert!(!outline_names.contains(&"put_chapter_template".into()));
    assert!(fill_names.contains(&"put_chapter_template".into()));
    assert!(!fill_names.contains(&"put_outline_item".into()));
    let outline_sha = digest(&bodies[0]["tools"]).unwrap();
    let fill_sha = digest(&bodies[1]["tools"]).unwrap();
    assert_ne!(outline_sha, fill_sha);
    let host = |body: &Value| {
        let messages = body["messages"].as_array().expect("messages");
        serde_json::from_str::<Value>(
            messages.last().expect("host packet")["content"]
                .as_str()
                .expect("host json"),
        )
        .unwrap()
    };
    let outline_host = host(&bodies[0]);
    let fill_host = host(&bodies[1]);
    for packet in [&outline_host, &fill_host] {
        assert!(packet.get("progress").is_some(), "{packet}");
        assert!(packet.get("work").is_some(), "{packet}");
        assert!(packet.get("source_review").is_none(), "{packet}");
        assert!(packet.get("main_dispatch").is_none(), "{packet}");
        assert!(packet.get("global_analysis_checks").is_none(), "{packet}");
        assert!(packet.get("review_findings").is_none(), "{packet}");
        assert!(packet.get("execution").is_none(), "{packet}");
        assert!(packet.get("work_state").is_none(), "{packet}");
    }
    assert_eq!(fill_host["progress"]["draft_active_id"], "outline-1");
    assert_eq!(fill_host["work"]["chapter_id"], "outline-1");
    for body in bodies.iter() {
        let tools = body["tools"].as_array().expect("tools");
        assert!(tools.len() <= 6 && tools.len() >= 2);
        for tool in tools {
            let name = tool["function"]["name"].as_str().unwrap();
            assert_ne!(name, "put_record");
            assert_ne!(name, "inspect_analysis");
            assert_ne!(name, "compile_docx");
            assert_ne!(name, "put_source_review");
        }
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
async fn run_budget_still_publishes_draft_docx_when_filled() {
    let input = draft_input();
    let mut limits = config().limits;
    limits.draft_path = true;
    limits.max_turns = 2;
    let config = Config::with_provider(config().provider.clone(), limits).unwrap();
    let journal = MemoryJournal::default();
    let model = outline_then_fill_one_chapter(&input);
    let result = agent::run(&input, &config, &journal, &model, &CancellationToken::new())
        .await
        .unwrap();
    assert_draft_filled_partial(&result, &journal.load().await.unwrap().unwrap());
}

#[tokio::test]
async fn run_cancel_still_publishes_draft_docx_when_filled() {
    let input = draft_input();
    let mut limits = config().limits;
    limits.draft_path = true;
    limits.max_turns = 80;
    let config = Config::with_provider(config().provider.clone(), limits).unwrap();
    let journal = MemoryJournal::default();
    // 大纲一回 + 填章一回后 turn=2；save 注入 INTERNAL，等同墙钟取消 drive。
    journal.interrupt_after.lock().unwrap().replace(2);
    let model = outline_then_fill_one_chapter(&input);
    let result = agent::run(&input, &config, &journal, &model, &CancellationToken::new())
        .await
        .unwrap();
    assert_draft_filled_partial(&result, &journal.load().await.unwrap().unwrap());
}

#[tokio::test]
async fn put_chapter_template_rejects_other_chapter() {
    let input = draft_input();
    let mut limits = config().limits;
    limits.draft_path = true;
    let config = Config::with_provider(config().provider.clone(), limits).unwrap();
    let mut state = journal_state(&input, &config);
    crate::tender_analysis::draft::preload_outline_window(&input, &mut state);
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
    crate::tender_analysis::draft::preload_outline_window(&input, &mut state);
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

struct BatchScript {
    turns: Mutex<VecDeque<Vec<(String, Value)>>>,
}

#[async_trait]
impl Model for BatchScript {
    async fn turn(&self, _: &Config, body: &[u8]) -> Result<ChatTurn, AgentError> {
        let _: Value = serde_json::from_slice(body).unwrap();
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

fn outline_then_fill_one_chapter(input: &FrozenInput) -> BatchScript {
    let text = &input.source_units[0].text;
    let grounds =
        json!([{"source_id":"source","start":0,"end":text.len(),"view_id":null,"grid_cell":null}]);
    let fixed_end = "投标函为固定格式。".len();
    BatchScript {
        turns: Mutex::new(VecDeque::from([
            vec![
                (
                    "put_outline_item".into(),
                    json!({
                        "id":"ch-letter","parent":null,"order":0,"title":"投标函","prescribed":true,
                        "grounds":grounds
                    }),
                ),
                (
                    "put_outline_item".into(),
                    json!({
                        "id":"ch-name","parent":null,"order":1,"title":"投标人名称","prescribed":true,
                        "grounds":grounds
                    }),
                ),
            ],
            vec![(
                "put_chapter_template".into(),
                json!({
                    "chapter_id":"ch-letter","id":null,"title":"投标函","purpose":"格式",
                    "regions":[{
                        "source":{"source_id":"source","start":0,"end":fixed_end,"view_id":null,"grid_cell":null},
                        "role":"fixed_text","form_id":null,"cells":[],"blank_ranges":[],"instruction":""
                    }]
                }),
            )],
        ])),
    }
}

fn assert_draft_filled_partial(result: &AnalysisResult, state: &Checkpoint) {
    assert!(result.review.draft);
    assert_eq!(result.quality, "needs_review");
    assert!(result.analysis.dispositions.is_empty());
    let letter = result
        .analysis
        .draft_plan
        .iter()
        .find(|item| item.id == "ch-letter")
        .expect("filled chapter");
    assert_eq!(letter.status, DraftStatus::Filled);
    let pending = result
        .analysis
        .draft_plan
        .iter()
        .find(|item| item.id == "ch-name")
        .expect("unfilled chapter");
    assert_eq!(pending.status, DraftStatus::Omitted);
    assert_eq!(pending.omit_reason, Some(OmitReason::Deadline));
    assert_draft_object_ref(state);
    let xml = docx_xml_from_checkpoint(state);
    assert!(
        xml.contains("投标函为固定格式"),
        "budget/cancel draft must keep tender fixed text: {xml}"
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
        draft_compile_object_id: None,
        draft_docx_base64: None,
        outline_config_sha256: None,
        fill_config_sha256: None,
    }
}

#[test]
fn omit_deadline_keeps_filled_chapters() {
    let mut plan = vec![
        filled_item("tpl"),
        DraftPlanItem {
            id: "ch2".into(),
            parent: None,
            order: 1,
            title: "授权".into(),
            prescribed: true,
            source_ids: vec![],
            windows: vec![],
            window_index: 0,
            template_id: None,
            status: DraftStatus::Pending,
            omit_reason: None,
        },
    ];
    omit_pending_deadline(&mut plan);
    assert_eq!(plan[0].status, DraftStatus::Filled);
    assert_eq!(plan[1].status, DraftStatus::Omitted);
    assert_eq!(plan[1].omit_reason, Some(OmitReason::Deadline));
}
