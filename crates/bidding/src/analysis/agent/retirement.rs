//! Product outline requests stay on the one-shot tools. Old outline tool names
//! are not registered, and they do not publish an `OutlineArtifact`.

use super::{Checkpoint, Config, Journal, Role, finalize_run};
use crate::analysis::draft::{
    BodyStatus, ChapterPurpose as PlanPurpose, DraftPlanItem, DraftStatus,
};
use crate::analysis::outline_flow::{OutlineCheck, Phase};
use crate::analysis::{Analysis, FrozenInput, Source, Span, digest};
use crate::outline::discover::{DiscoverWork, PackSubmit};
use crate::outline::tools::Draft;
use crate::outline::{ChapterOutline, ChapterPurpose, SlotKind, TemplateContent};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::Mutex;

const RETIRED: &[&str] = &[
    "submit_outline_scan",
    "put_outline_items",
    "submit_outline_check",
];

fn input() -> FrozenInput {
    FrozenInput {
        schema_version: 2,
        project_id: "project".into(),
        document_set_id: "set".into(),
        documents: vec![],
        document_relations: vec![],
        source_units: vec![Source {
            source_unit_revision_id: "source".into(),
            document_id: "document".into(),
            text: "投标函".into(),
            locator: json!({"heading_path": "第一章 > 投标函"}),
            ordinal: 0,
        }],
        structured_forms: vec![],
        decisions: vec![],
    }
}

pub(super) fn checkpoint(input: &FrozenInput) -> Checkpoint {
    Checkpoint {
        journal: Default::default(),
        input_sha256: digest(input).unwrap(),
        config_sha256: String::new(),
        turn: 1,
        tool_calls: 0,
        read_bytes: 0,
        review_rounds: 0,
        role: Role::Main,
        analysis: Analysis::default(),
        review: None,
        pending_coverage: None,
        transcript: vec![],
        main_progress: Default::default(),
        main_work: None,
        done: false,
        source_views: BTreeMap::new(),
        draft_stage: crate::analysis::draft::DraftStage::Outline,
        draft_outline_gaps: None,
        draft_outline_stalls: 0,
        draft_outline_window: 0,
        outline_config_sha256: None,

        outline_run: Default::default(),
    }
}

fn committed_packs(input: &FrozenInput) -> DiscoverWork {
    let mut work = DiscoverWork::plan(input, 8000);
    loop {
        let claimed = work.claim(100);
        if claimed.is_empty() {
            break;
        }
        for pack in claimed {
            work.submit(
                input,
                &pack.id,
                PackSubmit {
                    call_id: format!("call:{}", pack.id),
                    claim_token: pack.claim_token.clone(),
                    pack_revision: pack.pack_revision,
                    requirements: vec![],
                    no_requirement_reason: Some("No requirements in this fixture".into()),
                    inspected_atom_ids: pack.atoms.iter().map(|atom| atom.id.clone()).collect(),
                },
            )
            .unwrap();
        }
    }
    assert!(work.complete());
    work
}

fn chapter() -> ChapterOutline {
    ChapterOutline {
        id: "letter".into(),
        parent_id: None,
        order: 0,
        title: "投标函".into(),
        purpose: ChapterPurpose::Response,
        requirement_ids: vec![],
    }
}

fn finished_draft() -> Draft {
    Draft {
        chapters: vec![chapter()],
        bindings: vec![],
        slots: vec![TemplateContent {
            slot_id: "letter:bidder".into(),
            chapter_id: "letter".into(),
            kind: SlotKind::BidderBlank,
            content: crate::outline::TemplateBody::EditableBlank,
            text: String::new(),
            response_required: true,
            match_query: "投标人名称".into(),
        }],
        slots_submitted: true,
        finished: true,
        ..Default::default()
    }
}

fn tool_names(body: &Value) -> Vec<String> {
    body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["function"]["name"].as_str().unwrap().to_string())
        .collect()
}

struct RecordingJournal {
    published: Mutex<usize>,
}

#[async_trait]
impl Journal for RecordingJournal {
    async fn load(&self) -> Result<Option<Checkpoint>, crate::agent_error::AgentError> {
        Ok(None)
    }

    async fn reserve(
        &self,
        _: &Checkpoint,
        _: &[u8],
    ) -> Result<Option<usize>, crate::agent_error::AgentError> {
        Ok(Some(0))
    }

    async fn save(&self, _: &Checkpoint, _: &Value) -> Result<(), crate::agent_error::AgentError> {
        Ok(())
    }

    async fn publish_outline(
        &self,
        _: &crate::outline::OutlineArtifact,
        _: &[crate::outline::chapters::AttachmentBinding],
    ) -> Result<(), crate::agent_error::AgentError> {
        *self.published.lock().expect("publish count") += 1;
        Ok(())
    }
}

#[test]
fn production_constructor_uses_the_one_shot_contract() {
    let config = super::super::tests::config();
    assert_eq!(
        config.tools_sha256,
        digest(&crate::outline::agent::schemas()).unwrap()
    );
}

#[tokio::test]
async fn product_requests_do_not_register_retired_outline_tools() {
    let config = super::super::tests::config();
    let input = input();
    let mut discover = checkpoint(&input);
    let discover_body = request_body(&input, &config, &mut discover).await;
    assert_eq!(
        tool_names(&discover_body),
        crate::outline::agent::schemas_for(crate::outline::agent::Duty::Discover)
            .iter()
            .map(|t| t["function"]["name"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    );

    let mut organize = checkpoint(&input);
    organize.outline_run.reading_packs = Some(committed_packs(&input));
    let organize_body = request_body(&input, &config, &mut organize).await;
    let organize_tools = crate::outline::agent::schemas_for(crate::outline::agent::Duty::Organize)
        .iter()
        .map(|t| t["function"]["name"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert_eq!(tool_names(&organize_body), organize_tools);

    let mut organize_with_chapters = organize.clone();
    organize_with_chapters.outline_run.tool_draft.chapters = vec![chapter()];
    let organize_with_chapters_body =
        request_body(&input, &config, &mut organize_with_chapters).await;
    assert_eq!(tool_names(&organize_with_chapters_body), organize_tools);

    let mut check = organize_with_chapters.clone();
    check.outline_run.tool_draft.slots_submitted = true;
    check.outline_run.tool_draft.slots = vec![TemplateContent {
        slot_id: "letter:fixed".into(),
        chapter_id: "letter".into(),
        kind: SlotKind::BidderBlank,
        content: crate::outline::TemplateBody::EditableBlank,
        text: String::new(),
        response_required: true,
        match_query: "投标函".into(),
    }];
    let check_body = request_body(&input, &config, &mut check).await;
    assert_eq!(
        tool_names(&check_body),
        crate::outline::agent::schemas_for(crate::outline::agent::Duty::Check)
            .iter()
            .map(|t| t["function"]["name"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    );

    for body in [
        discover_body,
        organize_body,
        organize_with_chapters_body,
        check_body,
    ] {
        let text = body.to_string();
        for name in RETIRED {
            assert!(!text.contains(name), "{name} leaked into {text}");
        }
    }
}

async fn request_body(input: &FrozenInput, config: &Config, state: &mut Checkpoint) -> Value {
    let bytes = super::request(input, config, state).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[test]
fn product_dispatch_rejects_retired_outline_tools_without_old_checks() {
    let config = super::super::tests::config();
    let input = input();
    let mut state = checkpoint(&input);
    for name in RETIRED {
        let error = super::apply(&input, &config, &mut state, name, &json!({})).unwrap_err();
        assert!(
            error.contains("tool is not allowed in the current outline duty"),
            "{name} reached outline_flow: {error}"
        );
    }
    assert!(state.analysis.outline.checks.is_empty());
    assert_eq!(state.analysis.outline.phase, Phase::Discover);
    assert!(!state.outline_run.tool_draft.finished);
}

#[tokio::test]
async fn finished_tool_draft_publishes_an_outline_artifact() {
    let config = super::super::tests::config();
    let input = input();
    let sha = digest(&input).unwrap();
    let mut state = checkpoint(&input);
    state.journal.sequence = 1;
    state.outline_run.tool_draft = finished_draft();
    state.analysis.outline.phase = Phase::Complete;
    state.analysis.outline.checks.insert(
        "composition".into(),
        OutlineCheck {
            scope: "composition".into(),
            fragment_ids: vec![],
            snapshot_sha256: "ab".repeat(32),
            finding_ids: vec![],
            status: "pass".into(),
        },
    );
    assert!(!crate::analysis::outline_flow::checked(&input, &state));

    let product = RecordingJournal {
        published: Mutex::new(0),
    };
    finalize_run(&input, &config, &product, &mut state, sha)
        .await
        .unwrap();
    assert_eq!(*product.published.lock().unwrap(), 1);
}

fn span(end: usize) -> Span {
    Span {
        source_id: "source".into(),
        start: 0,
        end,
        view_id: None,
        grid_cell: None,
    }
}

/// A scanned outline whose old `finish_outline` would move phase to `check`.
fn finishable_old_outline() -> (FrozenInput, Checkpoint) {
    let input = FrozenInput {
        schema_version: 2,
        project_id: "p".into(),
        document_set_id: "d".into(),
        documents: vec![],
        document_relations: vec![],
        source_units: vec![Source {
            source_unit_revision_id: "source".into(),
            document_id: "doc".into(),
            text: "投标函格式".into(),
            locator: json!({}),
            ordinal: 0,
        }],
        structured_forms: vec![],
        decisions: vec![],
    };
    let mut state = checkpoint(&input);
    let end = input.source_units[0].text.len();
    crate::analysis::tools::cover(
        state
            .analysis
            .coverage
            .text
            .entry("source".into())
            .or_default(),
        0,
        end,
    );
    let scan = json!({
        "text":{"source":[[0,end]]},
        "forms":{},
        "metadata":{},"empty_sources":[],
        "requirements":[{
            "id":"","description":"投标函","kind":"submission","submission_name":"投标函","classification_reason":"","format_required":false,"applicability":"required","condition":"",
            "grounds":[{"source_id":"source","start":0,"end":end,"view_id":null,"grid_cell":null}],
            "format_grounds":[],"order_constraints":[]
        }],
        "references":[],
        "issues":[],
        "review_fragments":[{
            "id":"","kind":"composition","document_id":"doc","volume_ids":[],
            "span":{"source_id":"source","start":0,"end":end,"view_id":null,"grid_cell":null}
        }]
    });
    crate::analysis::outline_flow::apply(&input, &mut state, "submit_outline_scan", &scan, 64_000)
        .unwrap();
    state.analysis.outline.phase = Phase::Outline;
    state.outline_run.phase = Phase::Outline;
    state.analysis.draft_plan.push(DraftPlanItem {
        grounds: vec![span(end)],
        requirement_ids: state
            .analysis
            .outline
            .requirements
            .keys()
            .cloned()
            .collect(),
        id: "chapter-1".into(),
        parent: None,
        order: 0,
        title: "投标函".into(),
        prescribed: true,
        source_ids: vec!["source".into()],
        windows: vec![],
        window_index: 0,
        template_id: None,
        status: DraftStatus::Pending,
        purpose: PlanPurpose::Response,
        format_refs: vec![],
        body_status: BodyStatus::Empty,
        omit_reason: None,
        preserved: vec![],
    });
    state.analysis.outline.project_info = Some(crate::analysis::outline_flow::ProjectInfo {
        cover_status: crate::analysis::outline_flow::CoverStatus::NotPrescribed,
        ..Default::default()
    });
    state.analysis.outline.checks.insert(
        "composition".into(),
        OutlineCheck {
            scope: "composition".into(),
            fragment_ids: vec![],
            snapshot_sha256: "ab".repeat(32),
            finding_ids: vec![],
            status: "pass".into(),
        },
    );
    (input, state)
}

#[test]
fn product_after_batch_does_not_finish_through_old_checks() {
    let (input, state) = finishable_old_outline();
    let mut via_old_tool = state.clone();
    crate::analysis::outline_flow::apply(
        &input,
        &mut via_old_tool,
        "finish_outline",
        &json!({}),
        64_000,
    )
    .unwrap();
    assert_eq!(via_old_tool.analysis.outline.phase, Phase::Check);

    let mut product = state.clone();
    crate::analysis::draft::after_batch(&input, &mut product, false, false).unwrap();
    assert_eq!(product.analysis.outline.phase, Phase::Outline);
    assert!(!product.outline_run.tool_draft.finished);
    assert!(!product.done);
    assert!(!crate::analysis::outline_flow::checked(&input, &product));
}
