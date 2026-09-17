use super::*;
use crate::docx_composition::agent_scope::WriteAuthority;
use crate::docx_composition::agent_work::{Action, Status, Work};
use crate::docx_composition::tools::{
    CompileFeedback, CompositionFinding, PlanReview, PlanReviewConclusion,
};

fn assigned(input: &FrozenInput, result: &AnalysisResult) -> agent::Checkpoint {
    let mut state = work_checkpoint(ready(input, result));
    state.main_work = Some(Work {
        source_scope: vec!["s0".into()],
        section_scope: vec!["fixture-section-0".into()],
        plan_item_id: Some("fixture-section-0".into()),
        action: Action::Compose,
        objective: "Compose the assigned plan item".into(),
        note: String::new(),
        status: Status::Active,
    });
    state
}

#[test]
fn shared_source_and_complete_coverage_do_not_authorize_another_section() {
    let (input, result) = fixture();
    let state = assigned(&input, &result);
    let authority = WriteAuthority::capture(&input, &result, &state).unwrap();
    assert!(
        authority
            .check(&input, &result, &state, "put_section", &section(&input, 0))
            .is_ok()
    );
    let mut other = section(&input, 1);
    other["grounds"] = section(&input, 0)["grounds"].clone();
    assert!(
        authority
            .check(&input, &result, &state, "put_section", &other)
            .is_err()
    );
    assert!(
        authority
            .check(
                &input,
                &result,
                &state,
                "delete_section",
                &json!({"id":"fixture-section-1"})
            )
            .is_err()
    );
    assert!(
        authority
            .check(
                &input,
                &result,
                &state,
                "set_presentation",
                &presentation(&input)
            )
            .is_err()
    );
    let mut own = section(&input, 0);
    own["grounds"] = section(&input, 1)["grounds"].clone();
    assert!(
        authority
            .check(&input, &result, &state, "put_section", &own)
            .is_err()
    );
}

#[test]
fn response_indices_and_stored_omissions_are_checked_exactly() {
    let (input, result) = fixture();
    let mut state = assigned(&input, &result);
    let authority = WriteAuthority::capture(&input, &result, &state).unwrap();
    let mut own = section(&input, 0);
    own["content"] = json!([{"kind":"placeholder","needs":[{"record_id":"r","target":{"kind":"response","index":1}}]}]);
    assert!(
        authority
            .check(&input, &result, &state, "put_section", &own)
            .is_err()
    );
    let reference = Reference {
        record_id: "r".into(),
        target: RelationTarget::Response { index: 1 },
    };
    let key = reference_key(&reference).unwrap();
    state.workspace.draft.omissions.insert(
        key.clone(),
        Omission {
            reference,
            reason: "Source-backed condition".into(),
            grounds: state.workspace.draft.sections["fixture-section-0"]
                .grounds
                .clone(),
        },
    );
    assert!(
        authority
            .check(
                &input,
                &result,
                &state,
                "delete_omission",
                &json!({"id":key})
            )
            .is_err()
    );
}

#[test]
fn planning_completion_cannot_acquire_section_authority_in_the_same_batch() {
    let (input, result) = fixture();
    let mut state = assigned(&input, &result);
    state.main_work = None;
    let removed = state
        .workspace
        .draft
        .plan
        .remove("fixture-section-1")
        .unwrap();
    let authority = WriteAuthority::capture(&input, &result, &state).unwrap();
    assert!(
        authority
            .check(
                &input,
                &result,
                &state,
                "put_composition_plan_item",
                &json!(removed)
            )
            .is_ok()
    );
    state
        .workspace
        .draft
        .plan
        .insert(removed.id.clone(), removed);
    assert!(plan_complete(&result, &state.workspace.draft).unwrap());
    assert!(
        authority
            .check(&input, &result, &state, "put_section", &section(&input, 0))
            .is_err()
    );
}

#[test]
fn assigned_plan_updates_cannot_expand_references_kind_or_batch_source_scope() {
    let (input, result) = fixture();
    let mut state = assigned(&input, &result);
    let authority = WriteAuthority::capture(&input, &result, &state).unwrap();
    let mut plan = json!(state.workspace.draft.plan["fixture-section-0"]);
    plan["kind"] = json!("presentation");
    assert!(
        authority
            .check(&input, &result, &state, "put_composition_plan_item", &plan)
            .is_err()
    );
    plan["kind"] = json!("section");
    plan["obligation_refs"] =
        json!(state.workspace.draft.plan["fixture-section-1"].obligation_refs);
    assert!(
        authority
            .check(&input, &result, &state, "put_composition_plan_item", &plan)
            .is_err()
    );
    state
        .main_work
        .as_mut()
        .unwrap()
        .source_scope
        .push("s1".into());
    let mut own = section(&input, 0);
    own["grounds"] = section(&input, 1)["grounds"].clone();
    assert!(
        authority
            .check(&input, &result, &state, "put_section", &own)
            .is_err()
    );
}

#[test]
fn newly_acquired_reading_does_not_authorize_same_batch_evidence() {
    let (input, result) = fixture();
    let mut state = assigned(&input, &result);
    let coverage = state.workspace.source_coverage.clone();
    state.workspace.source_coverage = Coverage::default();
    let authority = WriteAuthority::capture(&input, &result, &state).unwrap();
    state.workspace.source_coverage = coverage;
    assert!(
        authority
            .check(&input, &result, &state, "put_section", &section(&input, 0))
            .is_err()
    );
}

#[test]
fn relation_permissions_follow_exact_endpoints_and_resolve_stored_deletions() {
    let (input, result) = fixture();
    let mut state = assigned(&input, &result);
    let authority = WriteAuthority::capture(&input, &result, &state).unwrap();
    let grounds = state.workspace.draft.sections["fixture-section-0"]
        .grounds
        .clone();
    let mut args =
        json!({"relation_id":"link0","reason":"An evidenced alternative","grounds":grounds});
    assert!(
        authority
            .check(&input, &result, &state, "omit_template_relation", &args)
            .is_ok()
    );
    args["relation_id"] = json!("link1");
    assert!(
        authority
            .check(&input, &result, &state, "omit_template_relation", &args)
            .is_err()
    );
    state.workspace.draft.relation_omissions.insert(
        "link1".into(),
        RelationOmission {
            relation_id: "link1".into(),
            reason: "Another item's alternative".into(),
            grounds,
        },
    );
    assert!(
        authority
            .check(
                &input,
                &result,
                &state,
                "delete_relation_omission",
                &json!({"id":"link1"})
            )
            .is_err()
    );
}

#[test]
fn reviewer_can_cite_cross_source_findings_but_only_judge_its_item() {
    let (input, result) = fixture();
    let mut state = assigned(&input, &result);
    state.workspace.reviewing = true;
    state.workspace.review_coverage = state.workspace.source_coverage.clone();
    state.review_work = state.main_work.take();
    let authority = WriteAuthority::capture(&input, &result, &state).unwrap();
    let mut judgment = plan_judgment(&input, 0);
    judgment["findings"] = json!([{
        "message":"Cross-source requirement changes this item",
        "section_ids":["fixture-section-0","fixture-section-1"],
        "record_ids":["r"],"sources":section(&input, 1)["grounds"]
    }]);
    assert!(
        authority
            .check(&input, &result, &state, "put_composition_review", &judgment)
            .is_ok()
    );
    judgment["item_id"] = json!("fixture-section-1");
    assert!(
        authority
            .check(&input, &result, &state, "put_composition_review", &judgment)
            .is_err()
    );
    state.workspace.reviewing = false;
    assert!(
        authority
            .check(&input, &result, &state, "put_section", &section(&input, 0))
            .is_err()
    );
}

#[test]
fn repair_ownership_comes_from_saved_judgments_even_without_section_ids() {
    let (input, result) = fixture();
    let mut state = assigned(&input, &result);
    state.main_work = None;
    let finding = CompositionFinding {
        message: "Repair the reviewed item".into(),
        section_ids: vec![],
        record_ids: vec!["r".into()],
        sources: state.workspace.draft.sections["fixture-section-0"]
            .grounds
            .clone(),
    };
    state.workspace.plan_reviews.insert(
        "fixture-section-0".into(),
        PlanReview {
            item_id: "fixture-section-0".into(),
            conclusion: PlanReviewConclusion::Findings,
            finding_ids: vec![digest(&finding).unwrap()],
            grounds: finding.sources.clone(),
            artifact_sha256: "reviewed artifact".into(),
            draft_sha256: digest(&state.workspace.draft).unwrap(),
        },
    );
    state.workspace.findings.push(finding);
    let authority = WriteAuthority::capture(&input, &result, &state).unwrap();
    assert!(
        authority
            .check(&input, &result, &state, "put_section", &section(&input, 0))
            .is_ok()
    );
    assert!(
        authority
            .check(&input, &result, &state, "put_section", &section(&input, 1))
            .is_err()
    );
    assert!(
        authority
            .check(
                &input,
                &result,
                &state,
                "set_presentation",
                &presentation(&input)
            )
            .is_err()
    );
}

#[test]
fn only_exact_current_compile_feedback_authorizes_root_repair() {
    let (input, result) = fixture();
    let mut state = assigned(&input, &result);
    state.main_work = None;
    state.workspace.compile_feedback = Some(CompileFeedback {
        draft_sha256: digest(&state.workspace.draft).unwrap(),
        error: "compile failed".into(),
    });
    let authority = WriteAuthority::capture(&input, &result, &state).unwrap();
    assert!(
        authority
            .check(&input, &result, &state, "put_section", &section(&input, 1))
            .is_ok()
    );
    state
        .workspace
        .compile_feedback
        .as_mut()
        .unwrap()
        .draft_sha256 = "stale".into();
    let stale = WriteAuthority::capture(&input, &result, &state).unwrap();
    assert!(
        stale
            .check(&input, &result, &state, "put_section", &section(&input, 1))
            .is_err()
    );
    assert!(
        stale
            .check(
                &input,
                &result,
                &state,
                "set_presentation",
                &presentation(&input)
            )
            .is_ok()
    );
    state.workspace.reviewing = true;
    let global = WriteAuthority::capture(&input, &result, &state).unwrap();
    assert!(
        global
            .check(
                &input,
                &result,
                &state,
                "put_composition_review",
                &plan_judgment(&input, 0)
            )
            .is_err()
    );
    assert!(
        global
            .check(
                &input,
                &result,
                &state,
                "submit_composition_review",
                &json!({})
            )
            .is_ok()
    );
}
