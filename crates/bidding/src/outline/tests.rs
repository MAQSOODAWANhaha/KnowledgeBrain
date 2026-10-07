use super::*;
use crate::analysis::draft::{
    BodyStatus, ChapterPurpose as PlanPurpose, DraftPlanItem, DraftStatus,
};
use crate::analysis::outline_flow::{
    CoverStatus, IssueStatus, OutlineIssue, Phase, ProjectInfo, ProjectValue,
};
use crate::analysis::{
    Analysis, AnalysisResult, Applicability, ApplicabilityState, Cell, FrozenInput, Record,
    RecordData, RegionRole, Review, Source, Span, TemplateRegion,
};
use crate::response::{EvidenceHit, NO_EVIDENCE_TEXT, ResponseStatus, match_queries, respond};
use serde_json::json;
use std::collections::BTreeMap;

fn span(end: usize) -> Span {
    Span {
        source_id: "source".into(),
        start: 0,
        end,
        view_id: None,
        grid_cell: None,
    }
}

fn chapter(id: &str, purpose: PlanPurpose, template_id: Option<&str>) -> DraftPlanItem {
    DraftPlanItem {
        grounds: vec![],
        requirement_ids: vec!["requirement-1".into()],
        id: id.into(),
        parent: None,
        order: 0,
        title: "投标函".into(),
        prescribed: true,
        source_ids: vec!["source".into()],
        windows: vec![],
        window_index: 0,
        template_id: template_id.map(str::to_string),
        status: DraftStatus::Filled,
        purpose,
        format_refs: vec![],
        body_status: BodyStatus::Empty,
        omit_reason: None,
        preserved: vec![],
    }
}

fn region(role: RegionRole, end: usize, instruction: &str) -> TemplateRegion {
    TemplateRegion {
        source: span(end),
        role,
        form_id: None,
        cells: vec![],
        header_rows: None,
        blank_ranges: vec![],
        instruction: instruction.into(),
    }
}

fn fixture() -> (FrozenInput, AnalysisResult) {
    let text = "投标函为固定格式。投标人：";
    let input = FrozenInput {
        schema_version: 1,
        project_id: "project-1".into(),
        document_set_id: "set-1".into(),
        documents: vec![],
        document_relations: vec![],
        source_units: vec![Source {
            source_unit_revision_id: "source".into(),
            document_id: "doc-1".into(),
            text: text.into(),
            locator: json!({}),
            ordinal: 0,
        }],
        structured_forms: vec![json!({
            "form_definition_revision_id": "form-1",
            "definition": {
                "cells": [
                    {"row": 0, "column": 0, "text": "项目"},
                    {"row": 0, "column": 1, "text": "报价"}
                ]
            }
        })],
        decisions: vec![],
    };
    let fixed_end = "投标函为固定格式。".len();
    let applicability = Applicability {
        state: ApplicabilityState::Applicable,
        condition: "原文指定".into(),
        scope: "本项目".into(),
        grounds: vec![span(fixed_end)],
    };
    let mut records = BTreeMap::new();
    records.insert(
        "tpl".into(),
        Record {
            id: "tpl".into(),
            sources: vec![span(fixed_end)],
            data: RecordData::Template {
                label: "投标函".into(),
                title: "投标函".into(),
                parent: None,
                order: Some(0),
                purpose: "招标指定格式".into(),
                applicability,
                regions: vec![
                    region(RegionRole::FixedText, fixed_end, ""),
                    region(RegionRole::BidderBlank, text.len(), "填写投标人全称"),
                ],
            },
        },
    );
    let mut analysis = Analysis::default();
    analysis.outline.phase = Phase::Complete;
    analysis.outline.project_info = Some(ProjectInfo {
        cover_status: CoverStatus::Prescribed,
        cover_lines: vec![ProjectValue {
            value: "项目名称：示例工程".into(),
            grounds: vec![span(fixed_end)],
        }],
        cover_requirement_ids: vec!["requirement-cover".into()],
        ..ProjectInfo::default()
    });
    analysis.outline.issues.insert(
        "issue-1".into(),
        OutlineIssue {
            code: "notice".into(),
            description: "外部标准未解".into(),
            requirement_ids: vec![],
            chapter_ids: vec!["letter".into()],
            reference_ids: vec![],
            grounds: vec![span(fixed_end)],
            status: IssueStatus::Open,
            resolution_grounds: vec![],
        },
    );
    analysis.records = records;
    let mut letter = chapter("letter", PlanPurpose::Response, Some("tpl"));
    letter.order = 1;
    let mut group = chapter("volume", PlanPurpose::Group, None);
    group.title = "商务册".into();
    group.order = 0;
    group.requirement_ids.clear();
    let mut omitted = chapter("dropped", PlanPurpose::Response, None);
    omitted.status = DraftStatus::Omitted;
    analysis.draft_plan = vec![group, letter, omitted];
    let result = AnalysisResult {
        schema_version: 4,
        frozen_input_sha256: "a".repeat(64),
        analysis,
        review: Review::default(),
        quality: "draft".into(),
        source_views: BTreeMap::new(),
    };
    (input, result)
}

#[test]
fn outline_publishes_chapters_and_template_without_bidder_facts() {
    let (input, result) = fixture();
    let artifact = project(&input, &result).unwrap();
    assert_eq!(
        artifact
            .chapters
            .iter()
            .map(|chapter| chapter.id.as_str())
            .collect::<Vec<_>>(),
        vec!["volume", "letter"]
    );
    assert!(
        artifact
            .chapters
            .iter()
            .all(|chapter| chapter.id != "dropped")
    );
    let cover = artifact
        .templates
        .iter()
        .find(|slot| slot.slot_id == "cover:0")
        .unwrap();
    assert_eq!(cover.text, "项目名称：示例工程");
    assert!(!cover.response_required);
    let fixed = artifact
        .templates
        .iter()
        .find(|slot| slot.slot_id == "letter:region:0")
        .unwrap();
    assert_eq!(fixed.text, "投标函为固定格式。");
    assert!(fixed.match_query.is_empty());
    let blank = artifact
        .templates
        .iter()
        .find(|slot| slot.slot_id == "letter:region:1")
        .unwrap();
    assert!(blank.text.is_empty());
    assert!(blank.response_required);
    assert_eq!(blank.match_query, "投标函\n填写投标人全称");
    assert_eq!(artifact.open_issues[0].description, "外部标准未解");
    let encoded = serde_json::to_value(&artifact).unwrap();
    assert!(encoded.get("evidence").is_none());
    assert!(encoded.get("responses").is_none());
}

#[test]
fn outline_rejects_an_unfinished_tree_and_image_template_text() {
    let (input, mut result) = fixture();
    result.analysis.outline.phase = Phase::Check;
    assert_eq!(
        project(&input, &result).unwrap_err(),
        "outline is not complete"
    );
    result.analysis.outline.phase = Phase::Complete;
    let RecordData::Template { regions, .. } =
        &mut result.analysis.records.get_mut("tpl").unwrap().data
    else {
        panic!("template");
    };
    regions[0].source.view_id = Some("view-1".into());
    assert!(
        project(&input, &result)
            .unwrap_err()
            .contains("tender text span")
    );
}

#[test]
fn outline_copies_grid_wording_and_protects_user_chapters() {
    let (input, mut result) = fixture();
    let RecordData::Template { regions, .. } =
        &mut result.analysis.records.get_mut("tpl").unwrap().data
    else {
        panic!("template");
    };
    regions[0].form_id = Some("form-1".into());
    regions[0].cells = vec![Cell { row: 0, column: 0 }, Cell { row: 0, column: 1 }];
    let artifact = project(&input, &result).unwrap();
    assert_eq!(
        artifact
            .templates
            .iter()
            .find(|slot| slot.slot_id == "letter:region:0")
            .unwrap()
            .text,
        "项目\n报价"
    );

    result.analysis.draft_plan[1].body_status = BodyStatus::User;
    result.analysis.draft_plan[1].preserved.push(
        crate::analysis::readback::Preserved::Paragraphs {
            unit_keys: vec!["unit-1".into()],
            paragraphs: vec!["用户已写正文".into()],
        },
    );
    let protected = project(&input, &result).unwrap();
    assert!(
        protected
            .templates
            .iter()
            .any(|slot| { slot.slot_id == "letter:preserved" && !slot.response_required })
    );
    assert!(
        protected
            .templates
            .iter()
            .all(|slot| slot.chapter_id != "letter" || !slot.response_required)
    );
}

#[test]
fn response_fills_only_response_slots_from_knowledge_hits() {
    let (input, result) = fixture();
    let artifact = project(&input, &result).unwrap();
    let queries = match_queries(&artifact).unwrap();
    assert_eq!(queries.len(), 1);
    assert_eq!(queries[0].slot_id, "letter:region:1");

    let unmatched = respond(&artifact, &[]).unwrap();
    assert_eq!(unmatched.responses.len(), 1);
    assert_eq!(unmatched.responses[0].status, ResponseStatus::NoEvidence);
    assert_eq!(unmatched.responses[0].text, NO_EVIDENCE_TEXT);
    assert_eq!(
        unmatched.outline_sha256,
        canonical_sha256(&artifact).unwrap()
    );

    let matched = respond(
        &artifact,
        &[EvidenceHit {
            evidence_id: "evidence-1".into(),
            document_id: "company-doc".into(),
            slot_id: "letter:region:1".into(),
            text: "某某科技有限公司".into(),
        }],
    )
    .unwrap();
    assert_eq!(matched.responses[0].status, ResponseStatus::Matched);
    assert_eq!(matched.responses[0].text, "某某科技有限公司");
    assert_eq!(matched.responses[0].evidence_ids, vec!["evidence-1"]);
    assert!(
        matched
            .responses
            .iter()
            .all(|response| response.slot_id != "letter:region:0")
    );

    let rejected = respond(
        &artifact,
        &[EvidenceHit {
            evidence_id: "evidence-2".into(),
            document_id: "company-doc".into(),
            slot_id: "letter:region:0".into(),
            text: "伪造的固定格式".into(),
        }],
    );
    assert!(rejected.unwrap_err().contains("not a response slot"));
}

#[test]
fn phase_contracts_reject_unknown_fields_and_placeholder_hits() {
    let (input, result) = fixture();
    let artifact = project(&input, &result).unwrap();
    let mut value = serde_json::to_value(&artifact).unwrap();
    value["company_fact"] = json!("不应出现");
    assert!(serde_json::from_value::<OutlineArtifact>(value).is_err());

    let placeholder = respond(
        &artifact,
        &[EvidenceHit {
            evidence_id: "evidence-1".into(),
            document_id: "company-doc".into(),
            slot_id: "letter:region:1".into(),
            text: NO_EVIDENCE_TEXT.into(),
        }],
    );
    assert!(placeholder.unwrap_err().contains("no evidence text"));

    let mut changed = artifact.clone();
    changed.chapters[1].title = "授权委托书".into();
    assert_ne!(
        canonical_sha256(&artifact).unwrap(),
        canonical_sha256(&changed).unwrap()
    );
}

#[test]
fn attachment_table_mapping_follows_chapter_id_not_title() {
    let (mut input, mut result) = fixture();
    input.structured_forms[0]["definition"]["title"] = json!("附件1 报价表");
    input.structured_forms[0]["source_unit_revision_id"] = json!("source");
    let RecordData::Template { regions, .. } =
        &mut result.analysis.records.get_mut("tpl").unwrap().data
    else {
        panic!("template");
    };
    regions[0].form_id = Some("form-1".into());
    let mapped = chapters::map_attachment_tables(
        &input,
        &result.analysis.draft_plan,
        &result.analysis.records,
    )
    .unwrap();
    assert_eq!(mapped.len(), 1);
    assert_eq!(mapped[0].form_id, "form-1");
    assert_eq!(mapped[0].chapter_id, "letter");
    result.analysis.draft_plan[1].title = "报价响应".into();
    let renamed = chapters::map_attachment_tables(
        &input,
        &result.analysis.draft_plan,
        &result.analysis.records,
    )
    .unwrap();
    assert_eq!(renamed, mapped);
    assert!(project(&input, &result).is_ok());

    let RecordData::Template { regions, .. } =
        &mut result.analysis.records.get_mut("tpl").unwrap().data
    else {
        panic!("template");
    };
    regions[0].form_id = None;
    assert_eq!(
        chapters::unmapped_attachment_forms(
            &input,
            &result.analysis.draft_plan,
            &result.analysis.records
        ),
        vec!["form-1".to_string()]
    );
    assert!(
        project(&input, &result)
            .unwrap_err()
            .contains("attachment table form-1")
    );
}
