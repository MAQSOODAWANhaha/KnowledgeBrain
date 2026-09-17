use super::*;
use crate::docx_composition::agent_work::{self, Action, Status};

fn finish_and_install(input: &FrozenInput, result: &AnalysisResult, state: &mut agent::Checkpoint) {
    let reviewing = state.workspace.reviewing;
    assert!(agent_work::complete_assigned(input, result, state));
    agent_work::observe(state, reviewing, vec![], None, &config().limits).unwrap();
    agent_work::install_next(input, result, state, 100_000).unwrap();
}

#[test]
fn presentation_and_report_handoff_never_invent_section_work() {
    let (input, result) = fixture();
    let mut workspace = ready(&input, &result);
    let presentation = workspace.draft.presentation.take().unwrap();
    let omission = workspace.draft.omissions.values().next().unwrap().clone();
    let sections_before = digest(&workspace.draft.sections).unwrap();
    workspace.draft.omissions.clear();
    for item in [
        PlanItem {
            id: "presentation-plan".into(),
            kind: PlanItemKind::Presentation,
            parent: None,
            order: 0,
            title: presentation.title.clone(),
            placement: Default::default(),
            prescribed: true,
            grounds: presentation.grounds.clone(),
            obligation_refs: vec![],
            exception: None,
        },
        PlanItem {
            id: "report-plan".into(),
            kind: PlanItemKind::ReportNote,
            parent: None,
            order: 1,
            title: "Source-backed omission".into(),
            placement: Default::default(),
            prescribed: true,
            grounds: omission.grounds.clone(),
            obligation_refs: vec![reference_key(&omission.reference).unwrap()],
            exception: Some(omission.reason.clone()),
        },
    ] {
        workspace.draft.plan.insert(item.id.clone(), item);
    }
    let mut state = work_checkpoint(workspace);
    agent_work::install_next(&input, &result, &mut state, 100_000).unwrap();
    let first = state.main_work.as_ref().unwrap();
    assert_eq!(first.plan_item_id.as_deref(), Some("presentation-plan"));
    assert!(first.section_scope.is_empty());
    assert!(!agent_work::complete_assigned(&input, &result, &mut state));

    edit(
        &mut state.workspace,
        &input,
        &result,
        "set_presentation",
        json!(presentation),
    );
    finish_and_install(&input, &result, &mut state);
    let report = state.main_work.as_ref().unwrap();
    assert_eq!(report.plan_item_id.as_deref(), Some("report-plan"));
    assert!(report.section_scope.is_empty());
    assert_eq!(report.source_scope, ["s1"]);
    assert_eq!(report.action, Action::Compose);
    assert!(!agent_work::complete_assigned(&input, &result, &mut state));

    edit(
        &mut state.workspace,
        &input,
        &result,
        "put_omission",
        json!(omission),
    );
    finish_and_install(&input, &result, &mut state);
    assert_eq!(
        digest(&state.workspace.draft.sections).unwrap(),
        sections_before
    );
    assert!(agent_work::packet(&input, &result, &state)["assigned_plan_item"].is_null());
    assert!(!state.workspace.done);
}

fn parent_and_unsaved_child(input: &FrozenInput, result: &AnalysisResult) -> agent::Checkpoint {
    let mut workspace = ready(input, result);
    let parent = workspace.draft.plan.get_mut("fixture-section-0").unwrap();
    parent.order = 1;
    let child = workspace.draft.plan.get_mut("fixture-section-1").unwrap();
    child.parent = Some("fixture-section-0".into());
    child.order = 0;
    workspace.draft.sections.clear();
    work_checkpoint(workspace)
}

fn save_parent(input: &FrozenInput, result: &AnalysisResult, state: &mut agent::Checkpoint) {
    let plan = &state.workspace.draft.plan["fixture-section-0"];
    let mut args = section(input, 0);
    args["title"] = json!(plan.title);
    args["order"] = json!(plan.order);
    args["content"] = json!([]);
    args["expected_draft_sha256"] = json!(digest(&state.workspace.draft).unwrap());
    state
        .workspace
        .invoke(
            input,
            result,
            "put_section",
            &args,
            tool_limits(100_000, 1_000_000),
        )
        .unwrap();
}

#[test]
fn empty_parent_finishes_locally_before_its_lower_order_unsaved_child() {
    let (input, result) = fixture();
    let mut state = parent_and_unsaved_child(&input, &result);
    agent_work::install_next(&input, &result, &mut state, 100_000).unwrap();
    assert_eq!(
        state.main_work.as_ref().unwrap().plan_item_id.as_deref(),
        Some("fixture-section-0"),
        "a child's order cannot bypass its missing parent"
    );
    save_parent(&input, &result, &mut state);
    finish_and_install(&input, &result, &mut state);
    let child = state.main_work.as_ref().unwrap();
    assert_eq!(child.plan_item_id.as_deref(), Some("fixture-section-1"));
    assert_eq!(child.source_scope, ["s1"]);
    assert_eq!(child.section_scope, ["fixture-section-1"]);
    assert_eq!(child.status, Status::Active);
    assert!(!agent_work::complete_assigned(&input, &result, &mut state));
    assert!(!state.workspace.done);
}

#[test]
fn existing_parent_with_stale_metadata_must_be_repaired_before_child_dispatch() {
    let (input, result) = fixture();
    let mut state = parent_and_unsaved_child(&input, &result);
    save_parent(&input, &result, &mut state);
    state
        .workspace
        .draft
        .plan
        .get_mut("fixture-section-0")
        .unwrap()
        .title = "Revised parent heading".into();
    agent_work::install_next(&input, &result, &mut state, 100_000).unwrap();
    assert_eq!(
        state.main_work.as_ref().unwrap().plan_item_id.as_deref(),
        Some("fixture-section-0"),
        "parent existence is not a valid dependency when its plan changed"
    );
    assert!(!agent_work::complete_assigned(&input, &result, &mut state));
    save_parent(&input, &result, &mut state);
    finish_and_install(&input, &result, &mut state);
    assert_eq!(
        state.main_work.as_ref().unwrap().plan_item_id.as_deref(),
        Some("fixture-section-1")
    );
}

#[test]
fn stale_reviewer_judgment_keeps_its_assignment_active() {
    for stale_artifact in [false, true] {
        let (input, result) = fixture();
        let mut state = work_checkpoint(review_workspace(&input, &result));
        agent_work::install_next(&input, &result, &mut state, 100_000).unwrap();
        state
            .workspace
            .invoke(
                &input,
                &result,
                "put_composition_review",
                &plan_judgment(&input, 0),
                tool_limits(100_000, 1_000_000),
            )
            .unwrap();
        let judgment = state
            .workspace
            .plan_reviews
            .get_mut("fixture-section-0")
            .unwrap();
        if stale_artifact {
            judgment.artifact_sha256 = "stale-artifact".into();
        } else {
            judgment.draft_sha256 = "stale-draft".into();
        }
        assert!(!agent_work::complete_assigned(&input, &result, &mut state));
        agent_work::install_next(&input, &result, &mut state, 100_000).unwrap();
        let work = state.review_work.as_ref().unwrap();
        assert_eq!(work.plan_item_id.as_deref(), Some("fixture-section-0"));
        assert_eq!(work.status, Status::Active);
        assert!(state.workspace.reviewing);
        assert!(!state.workspace.done);
    }
}

#[test]
fn no_next_local_item_cannot_hide_unplanned_global_obligations() {
    let (input, result) = fixture();
    let mut workspace = ready(&input, &result);
    workspace.draft.plan.remove("fixture-section-1");
    workspace.draft.sections.clear();
    workspace.draft.omissions.clear();
    let mut state = work_checkpoint(workspace);
    agent_work::install_next(&input, &result, &mut state, 100_000).unwrap();
    assert!(
        state.main_work.is_none(),
        "unplanned obligations keep the host in planning"
    );
    assert!(!agent_work::complete_assigned(&input, &result, &mut state));
    assert!(agent_work::packet(&input, &result, &state)["assigned_plan_item"].is_null());
    assert!(!plan_complete(&result, &state.workspace.draft).unwrap());
    agent_work::advance_main(&input, &result, &mut state, &config().limits).unwrap();
    assert!(state.workspace.artifact.is_none());
    assert!(!state.workspace.reviewing);
    assert!(!state.workspace.done);
}
