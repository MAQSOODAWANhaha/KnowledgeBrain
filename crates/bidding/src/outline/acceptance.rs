//! One-shot outline acceptance.
//!
//! Discover, organize (chapters, bindings, and slots), and finish run against the frozen input.
//! The checkpoint is the memory: clearing the transcript must not drop requirements.

use super::agent::{Duty, apply, current, deny, host_packet};
use super::discover::{DiscoverWork, PackRequirement, PackSubmit, claim_turn};
use super::tools::Draft;
use super::{project_draft, validate_artifact};
use crate::analysis::agent::Checkpoint;
use crate::analysis::draft::{
    BodyStatus, ChapterPurpose as PlanPurpose, DraftPlanItem, DraftStatus,
};
use crate::analysis::outline_flow::{OutlineCheck, Phase};
use crate::analysis::{Analysis, FrozenInput, Source};
use serde_json::json;
use std::collections::BTreeMap;

fn source(id: &str, ordinal: usize, text: &str, heading: &str) -> Source {
    Source {
        source_unit_revision_id: id.into(),
        document_id: "doc".into(),
        text: text.into(),
        locator: json!({"heading_path": heading}),
        ordinal,
    }
}

fn frozen() -> FrozenInput {
    FrozenInput {
        schema_version: 1,
        project_id: "project-1".into(),
        document_set_id: "set-1".into(),
        documents: vec![],
        document_relations: vec![],
        source_units: vec![
            source("b", 1, "B", "第二章 > 技术方案"),
            source("a", 0, "A", "第一章 > 投标函"),
            source("form-source", 2, "", "附件"),
        ],
        structured_forms: vec![json!({
            "form_definition_revision_id": "form-1",
            "source_unit_revision_id": "form-source",
            "definition": {
                "title": "附件一 报价表",
                "row_count": 1,
                "column_count": 1,
                "cells": [{"row": 0, "column": 0, "text": "报价"}]
            }
        })],
        decisions: vec![],
    }
}

fn checkpoint(input: &FrozenInput) -> Checkpoint {
    Checkpoint {
        journal: Default::default(),
        input_sha256: crate::analysis::digest(input).unwrap(),
        config_sha256: String::new(),
        turn: 1,
        tool_calls: 0,
        read_bytes: 0,
        review_rounds: 0,
        role: crate::analysis::agent::Role::Main,
        analysis: Analysis::default(),
        review: None,
        review_draft: BTreeMap::new(),
        source_review: None,
        repair: Default::default(),
        dispatch: Default::default(),
        reviewer_coverage: Default::default(),
        pending_coverage: None,
        transcript: vec![json!({"role":"assistant","content":"发现对话里的要求：投标函"})],
        main_progress: Default::default(),
        reviewer_progress: Default::default(),
        main_work: None,
        reviewer_work: None,
        done: false,
        source_views: BTreeMap::new(),
        draft_stage: crate::analysis::draft::DraftStage::Outline,
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

fn plan_item() -> DraftPlanItem {
    DraftPlanItem {
        grounds: vec![],
        requirement_ids: vec![],
        id: "draft-plan-only".into(),
        parent: None,
        order: 0,
        title: "不应计为产品章数".into(),
        prescribed: true,
        source_ids: vec![],
        windows: vec![],
        window_index: 0,
        template_id: None,
        status: DraftStatus::Pending,
        purpose: PlanPurpose::Response,
        format_refs: vec![],
        body_status: BodyStatus::Empty,
        omit_reason: None,
        preserved: vec![],
    }
}

fn submit(work: &mut DiscoverWork, pack_id: &str, requirements: Vec<PackRequirement>) {
    work.submit(
        pack_id,
        PackSubmit {
            call_id: pack_id.into(),
            requirements,
        },
    )
    .unwrap();
}

#[test]
fn one_shot_acceptance_publishes_from_the_tool_draft() {
    let input = frozen();
    let mut slot = None;
    let first = claim_turn(&mut slot, &input, 1, 1);
    assert_eq!(first[0]["pack"]["id"], "pack-0");
    assert_eq!(first[0]["pack"]["text"][0]["source_id"], "a");
    assert_eq!(first[0]["pack"]["text"][0]["text"], "A");
    assert_eq!(first[0]["pack"]["heading"], "第一章 > 投标函");
    let replay = claim_turn(&mut slot, &input, 1, 1);
    assert_eq!(replay[0]["pack"]["id"], "pack-0");
    assert_eq!(replay[0]["status"], "running");
    assert_eq!(replay[0]["pack"]["text"][0]["text"], "A");
    assert_eq!(replay[1]["pack"]["text"][0]["text"], "B");

    let mut work = slot.take().unwrap();
    work.claim(10);
    let outside = work
        .submit(
            "pack-1",
            PackSubmit {
                call_id: "bad-pack".into(),
                requirements: vec![PackRequirement {
                    description: "别的包".into(),
                    source_id: "a".into(),
                    start: 0,
                    end: 1,
                }],
            },
        )
        .unwrap_err();
    assert_eq!(outside.errors[0].code, "outside_pack");
    assert!(work.requirement("pack-1:0").is_none());
    assert_eq!(
        work.status("pack-1"),
        Some(super::discover::PackStatus::Failed)
    );
    let outside_slice = work
        .submit(
            "pack-0",
            PackSubmit {
                call_id: "bad-slice".into(),
                requirements: vec![PackRequirement {
                    description: "越界".into(),
                    source_id: "a".into(),
                    start: 0,
                    end: 2,
                }],
            },
        )
        .unwrap_err();
    assert_eq!(outside_slice.errors[0].code, "outside_slice");
    assert_eq!(work.requirement_count(), 0);

    let failed_replay = work.inflight_sessions(&input);
    assert!(failed_replay.iter().any(|session| {
        session["pack"]["id"] == "pack-0"
            && session["status"] == "failed"
            && session["pack"]["text"][0]["text"] == "A"
    }));

    submit(
        &mut work,
        "pack-0",
        vec![PackRequirement {
            description: "投标函".into(),
            source_id: "a".into(),
            start: 0,
            end: 1,
        }],
    );
    submit(
        &mut work,
        "pack-1",
        vec![PackRequirement {
            description: "技术方案".into(),
            source_id: "b".into(),
            start: 0,
            end: 1,
        }],
    );
    submit(&mut work, "pack-2", vec![]);
    assert!(work.complete());
    let letter = work.requirement("pack-0:0").unwrap();
    assert_eq!(
        (letter.source_id.as_str(), letter.start, letter.end),
        ("a", 0, 1)
    );
    assert_eq!(
        work.requirement("pack-1:0").unwrap().description,
        "技术方案"
    );

    let mut state = checkpoint(&input);
    assert_eq!(current(&input, &state), Duty::Discover);
    assert!(deny(Duty::Discover, "put_slots", false).is_some());
    assert!(deny(Duty::Discover, "put_chapters", false).is_some());
    state.outline_run.reading_packs = Some(work);
    state.analysis.draft_plan = vec![plan_item()];
    crate::analysis::draft::after_batch(&input, &mut state, false, false).unwrap();
    assert!(state.transcript.is_empty());
    assert_eq!(state.analysis.outline.phase, Phase::Outline);
    assert_eq!(state.outline_run.phase, Phase::Outline);
    assert_eq!(current(&input, &state), Duty::Organize);
    assert!(deny(Duty::Discover, "put_chapters", false).is_some());

    let packet = host_packet(&input, &state, 8_000, json!({}), json!({}), None);
    let requirements = packet["requirements"].as_array().unwrap();
    assert_eq!(requirements.len(), 2);
    assert_eq!(requirements[0]["id"], "pack-0:0");
    assert_eq!(requirements[0]["description"], "投标函");
    assert_eq!(requirements[0]["source_id"], "a");
    assert_eq!(requirements[0]["start"], 0);
    assert_eq!(requirements[0]["end"], 1);
    assert_eq!(requirements[1]["source_id"], "b");
    assert!(!packet.to_string().contains("发现对话"));

    let missing = apply(
        &input,
        0,
        &mut state,
        "put_chapters",
        &json!({"chapters":[
            {"id":"group","parent_id":null,"order":0,"title":"投标文件","purpose":"group"},
            {"id":"letter","parent_id":"group","order":0,"title":"投标函","purpose":"response"}
        ]}),
    )
    .unwrap_err();
    assert!(missing.contains("requirement_ids"));
    let unknown = apply(
        &input,
        0,
        &mut state,
        "put_chapters",
        &json!({"chapters":[
            {"id":"letter","parent_id":null,"order":0,"title":"投标函","purpose":"response","requirement_ids":["missing"]}
        ]}),
    )
    .unwrap_err();
    assert!(unknown.contains("unknown requirement"));
    let partial = apply(
        &input,
        0,
        &mut state,
        "put_chapters",
        &json!({"chapters":[
            {"id":"letter","parent_id":null,"order":0,"title":"投标函","purpose":"response","requirement_ids":["pack-0:0"]}
        ]}),
    )
    .unwrap_err();
    assert!(partial.contains("pack-1:0"));
    let duplicated = apply(
        &input,
        0,
        &mut state,
        "put_chapters",
        &json!({"chapters":[
            {"id":"group","parent_id":null,"order":0,"title":"投标文件","purpose":"group","requirement_ids":["pack-0:0"]},
            {"id":"letter","parent_id":"group","order":0,"title":"投标函","purpose":"response","requirement_ids":["pack-0:0","pack-1:0"]}
        ]}),
    )
    .unwrap_err();
    assert!(duplicated.contains("more than once"));
    apply(
        &input,
        0,
        &mut state,
        "put_chapters",
        &json!({"chapters":[
            {"id":"group","parent_id":null,"order":0,"title":"投标文件","purpose":"group","requirement_ids":[]},
            {"id":"letter","parent_id":"group","order":0,"title":"投标函","purpose":"response","requirement_ids":["pack-0:0","pack-1:0"]}
        ]}),
    )
    .unwrap();
    let unbound = apply(&input, 0, &mut state, "finish_outline", &json!({})).unwrap_err();
    assert!(unbound.contains("form-1"));
    apply(
        &input,
        0,
        &mut state,
        "bind_forms",
        &json!({"bindings":[{"form_id":"form-1","chapter_id":"letter"}]}),
    )
    .unwrap();
    assert_eq!(current(&input, &state), Duty::Organize);
    assert!(deny(Duty::Organize, "put_slots", false).is_none());
    assert!(deny(Duty::Organize, "put_chapters", false).is_none());
    assert!(deny(Duty::Organize, "bind_forms", false).is_none());
    assert_eq!(
        host_packet(&input, &state, 8_000, json!({}), json!({}), None)["requirements"]
            .as_array()
            .map(|rows| rows.len()),
        Some(2)
    );
    let no_slots = apply(&input, 0, &mut state, "finish_outline", &json!({})).unwrap_err();
    assert!(no_slots.contains("put_slots"));
    let grouped = apply(
        &input,
        0,
        &mut state,
        "put_slots",
        &json!({"slots":[
            {"slot_id":"group:bidder","chapter_id":"group","kind":"bidder_blank","text":"","match_query":"名称"}
        ]}),
    )
    .unwrap_err();
    assert!(grouped.contains("group chapter"));
    apply(
        &input,
        0,
        &mut state,
        "put_slots",
        &json!({"slots":[
            {"slot_id":"letter:fixed","chapter_id":"letter","kind":"fixed_text","text":"投标函","match_query":""},
            {"slot_id":"letter:bidder","chapter_id":"letter","kind":"bidder_blank","text":"","match_query":"投标人名称"},
            {"slot_id":"letter:sign","chapter_id":"letter","kind":"signature","text":"","match_query":"授权签字"}
        ]}),
    )
    .unwrap();
    assert_eq!(current(&input, &state), Duty::Check);
    assert!(deny(Duty::Check, "put_chapters", false).is_some());
    assert!(deny(Duty::Check, "bind_forms", false).is_some());
    assert!(deny(Duty::Check, "put_slots", false).is_some());
    assert!(deny(Duty::Check, "finish_outline", false).is_none());
    apply(&input, 0, &mut state, "finish_outline", &json!({})).unwrap();
    assert!(state.outline_run.tool_draft.finished);
    assert_eq!(state.outline_run.phase, Phase::Complete);
    assert_eq!(state.analysis.outline.phase, Phase::Complete);
    assert!(state.analysis.outline.checks.is_empty());

    let sha = state.input_sha256.clone();
    let projected = project_draft(&input, &sha, &state.outline_run.tool_draft).unwrap();
    validate_artifact(&projected.artifact).unwrap();
    assert_eq!(projected.artifact.schema_version, super::SCHEMA_VERSION);
    assert_eq!(projected.artifact.project_id, "project-1");
    assert_eq!(projected.artifact.frozen_input_sha256, sha);
    assert_eq!(
        projected
            .artifact
            .chapters
            .iter()
            .map(|chapter| chapter.id.as_str())
            .collect::<Vec<_>>(),
        vec!["group", "letter"]
    );
    assert_eq!(
        projected.artifact.chapters[1].requirement_ids,
        vec!["pack-0:0".to_string(), "pack-1:0".to_string()]
    );
    assert_eq!(projected.bindings, state.outline_run.tool_draft.bindings);
    assert_eq!(
        projected.artifact.templates,
        state.outline_run.tool_draft.slots
    );
    let bidder = projected
        .artifact
        .templates
        .iter()
        .find(|slot| slot.slot_id == "letter:bidder")
        .unwrap();
    assert!(bidder.text.is_empty());
    assert!(bidder.response_required);
    assert!(!bidder.match_query.is_empty());
    let signature = projected
        .artifact
        .templates
        .iter()
        .find(|slot| slot.slot_id == "letter:sign")
        .unwrap();
    assert!(signature.text.is_empty() && signature.response_required);
    let fixed = projected
        .artifact
        .templates
        .iter()
        .find(|slot| slot.slot_id == "letter:fixed")
        .unwrap();
    assert!(fixed.match_query.is_empty());
    assert!(
        projected
            .artifact
            .templates
            .iter()
            .all(|slot| { slot.chapter_id != "group" || !slot.response_required })
    );

    assert!(state.analysis.outline.checks.is_empty());
    assert!(!crate::analysis::outline_flow::checked(&input, &state));
    crate::analysis::draft::after_batch(&input, &mut state, false, false).unwrap();
    assert!(state.done);

    let progress = state.progress(&input);
    assert_eq!(progress["outline_chapters"], 2);
    assert_eq!(state.analysis.draft_plan.len(), 1);
    assert_eq!(progress["outline_requirements"], 2);
    assert_eq!(progress["outline_pack_total"], 3);
    assert_eq!(progress["outline_pack_committed"], 3);
    assert_eq!(progress["outline_unmapped_forms"], 0);
    assert_eq!(progress["outline_slots_submitted"], true);
    assert_eq!(progress["outline_finished"], true);

    let mut checks_only = state.clone();
    checks_only.draft_stage = crate::analysis::draft::DraftStage::Outline;
    checks_only.outline_run.tool_draft.finished = false;
    checks_only.done = false;
    checks_only.analysis.outline.checks.insert(
        "composition".into(),
        OutlineCheck {
            scope: "composition".into(),
            fragment_ids: vec![],
            snapshot_sha256: "ab".repeat(32),
            finding_ids: vec![],
            status: "pass".into(),
        },
    );
    assert!(project_draft(&input, &sha, &checks_only.outline_run.tool_draft).is_err());
    crate::analysis::draft::after_batch(&input, &mut checks_only, false, false).unwrap();
    assert!(!checks_only.done);

    let empty = Draft {
        chapters: state.outline_run.tool_draft.chapters.clone(),
        slots: state.outline_run.tool_draft.slots.clone(),
        bindings: state.outline_run.tool_draft.bindings.clone(),
        slots_submitted: true,
        finished: false,
    };
    assert!(project_draft(&input, &sha, &empty).is_err());
}

#[test]
fn chapters_may_carry_empty_requirement_lists_when_nothing_was_submitted() {
    let input = frozen();
    let mut state = checkpoint(&input);
    let mut work = DiscoverWork::plan(&input, 1);
    work.claim(10);
    for pack in ["pack-0", "pack-1", "pack-2"] {
        submit(&mut work, pack, vec![]);
    }
    state.outline_run.reading_packs = Some(work);
    apply(
        &input,
        0,
        &mut state,
        "put_chapters",
        &json!({"chapters":[
            {"id":"letter","parent_id":null,"order":0,"title":"投标函","purpose":"response","requirement_ids":[]}
        ]}),
    )
    .unwrap();
    assert!(
        state.outline_run.tool_draft.chapters[0]
            .requirement_ids
            .is_empty()
    );
}

#[test]
fn fill_and_published_stay_slot_only() {
    let input = frozen();
    for stage in [
        crate::analysis::draft::DraftStage::Fill,
        crate::analysis::draft::DraftStage::Published,
    ] {
        let mut state = checkpoint(&input);
        state.draft_stage = stage;
        state.outline_run.tool_draft.chapters = vec![super::ChapterOutline {
            id: "letter".into(),
            parent_id: None,
            order: 0,
            title: "投标函".into(),
            purpose: super::ChapterPurpose::Response,
            requirement_ids: vec![],
        }];
        state.outline_run.tool_draft.slots_submitted = true;
        assert_eq!(current(&input, &state), Duty::Template);
        assert!(deny(current(&input, &state), "put_chapters", false).is_some());
        assert!(deny(current(&input, &state), "bind_forms", false).is_some());
        assert!(deny(current(&input, &state), "put_slots", false).is_none());
        let schemas = super::agent::schemas_for(current(&input, &state));
        let names: Vec<_> = schemas
            .iter()
            .map(|tool| tool["function"]["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["put_slots", "read_outline"]);
    }
}
