use super::*;
use crate::docx_composition::agent_work::{self, Status};
use knowledge::models::{ChatToolCall, ChatTurn};

fn assign(input: &FrozenInput, result: &AnalysisResult, state: &mut agent::Checkpoint, i: usize) {
    let work = json!({"source_scope":[format!("s{i}")],
        "section_scope":[format!("fixture-section-{i}")],
        "plan_item_id":format!("fixture-section-{i}"),
        "action":if state.workspace.reviewing {"review"} else {"compose"},
        "objective":"Complete the assigned item", "note":"", "status":"active"});
    agent_work::set_work(input, result, state, &work, 100_000).unwrap();
}

async fn step(
    input: &FrozenInput,
    result: &AnalysisResult,
    state: &mut agent::Checkpoint,
    calls: Vec<(&str, Value)>,
) -> Vec<Value> {
    agent::execute_turn(
        input,
        result,
        &config(),
        state,
        ChatTurn {
            finish_reason: "tool_calls".into(),
            tool_calls: calls
                .into_iter()
                .enumerate()
                .map(|(i, (name, args))| ChatToolCall {
                    id: format!("local-{i}"),
                    name: name.into(),
                    arguments: args.to_string(),
                })
                .collect(),
            ..Default::default()
        },
        Default::default(),
        &tokio_util::sync::CancellationToken::new(),
    )
    .await
    .unwrap()
}

fn composing() -> (FrozenInput, AnalysisResult, agent::Checkpoint) {
    let (input, result) = fixture();
    let mut workspace = ready(&input, &result);
    workspace.draft.sections.clear();
    let mut state = work_checkpoint(workspace);
    assign(&input, &result, &mut state, 0);
    (input, result, state)
}

#[tokio::test]
async fn saved_item_advances_scope_after_charging_old_work_without_a_complete_call() {
    let (input, result, mut state) = composing();
    let coverage = digest(&state.workspace.source_coverage).unwrap();
    let mut args = section(&input, 0);
    args["expected_draft_sha256"] = json!(digest(&state.workspace.draft).unwrap());
    let outputs = step(&input, &result, &mut state, vec![("put_section", args)]).await;
    let output: Value = serde_json::from_str(outputs[0]["content"].as_str().unwrap()).unwrap();
    assert_eq!(output["ok"], true);
    let work = state.main_work.as_ref().unwrap();
    assert_eq!(work.plan_item_id.as_deref(), Some("fixture-section-1"));
    assert_eq!(work.source_scope, ["s1"]);
    assert_eq!(work.section_scope, ["fixture-section-1"]);
    assert_eq!(work.status, Status::Active);
    assert_eq!(state.main_progress.completions.len(), 1);
    assert_eq!(state.main_progress.watch.focus_turns, 0);
    assert_eq!(state.turn, 1);
    assert_eq!(state.tool_calls, 1);
    assert_eq!(digest(&state.workspace.source_coverage).unwrap(), coverage);
    assert!(!state.workspace.reviewing);
    agent_work::check_read_scope(&input, &state, "read_source", &json!({"source_id":"s1"}))
        .unwrap();
    assert!(
        agent_work::check_read_scope(&input, &state, "read_source", &json!({"source_id":"s0"}))
            .is_err()
    );
}

#[tokio::test]
async fn pending_evidence_or_a_tool_error_keeps_the_current_assignment() {
    for error in [false, true] {
        let (input, result, mut state) = composing();
        let mut args = section(&input, 0);
        args["expected_draft_sha256"] = json!(digest(&state.workspace.draft).unwrap());
        let second = if error {
            ("unknown_tool", json!({}))
        } else {
            (
                "inspect_analysis",
                json!({"kind":"all","view":"detail","ids":["r"],"offset":0,"limit":100}),
            )
        };
        step(
            &input,
            &result,
            &mut state,
            vec![("put_section", args), second],
        )
        .await;
        assert_eq!(
            state.main_work.as_ref().unwrap().plan_item_id.as_deref(),
            Some("fixture-section-0")
        );
        assert_eq!(state.main_work.as_ref().unwrap().status, Status::Active);
        assert_eq!(state.pending_delivery.is_some(), !error);
        assert!(!state.workspace.reviewing);
        if !error {
            let body = agent::request(&input, &state, &result, &config())
                .await
                .unwrap();
            state.journal.prepare(state.turn, "main", &body).unwrap();
            step(
                &input,
                &result,
                &mut state,
                vec![("composition_coverage", json!({"offset":0,"limit":100}))],
            )
            .await;
            assert!(state.pending_delivery.is_none());
            assert_eq!(
                state.main_work.as_ref().unwrap().plan_item_id.as_deref(),
                Some("fixture-section-1")
            );
        }
    }
}

#[tokio::test]
async fn later_deletion_invalidates_earlier_saved_completion() {
    let (input, result, mut state) = composing();
    let mut args = section(&input, 0);
    let mut after = state.workspace.draft.clone();
    after.sections.insert(
        "fixture-section-0".into(),
        serde_json::from_value(args.clone()).unwrap(),
    );
    args["expected_draft_sha256"] = json!(digest(&state.workspace.draft).unwrap());
    let remove = json!({"id":"fixture-section-0","expected_draft_sha256":digest(&after).unwrap()});
    step(
        &input,
        &result,
        &mut state,
        vec![("put_section", args), ("delete_section", remove)],
    )
    .await;
    assert!(
        !state
            .workspace
            .draft
            .sections
            .contains_key("fixture-section-0")
    );
    assert_eq!(state.main_work.as_ref().unwrap().status, Status::Active);
    assert_eq!(
        state.main_work.as_ref().unwrap().plan_item_id.as_deref(),
        Some("fixture-section-0")
    );
    assert!(
        state.main_progress.completions.is_empty(),
        "a deleted result cannot reset the execution watch"
    );
}

#[tokio::test]
async fn reviewer_findings_finish_the_judgment_without_erasing_the_problem() {
    let (input, result) = fixture();
    let mut state = work_checkpoint(review_workspace(&input, &result));
    assign(&input, &result, &mut state, 0);
    let mut args = plan_judgment(&input, 0);
    args["conclusion"] = json!("findings");
    args["findings"] = json!([{"message":"The prescribed field needs repair", "section_ids":["fixture-section-0"], "record_ids":["t0"], "sources":args["grounds"]}]);
    step(
        &input,
        &result,
        &mut state,
        vec![("put_composition_review", args)],
    )
    .await;
    assert_eq!(
        state.review_work.as_ref().unwrap().plan_item_id.as_deref(),
        Some("fixture-section-1")
    );
    assert_eq!(state.review_work.as_ref().unwrap().source_scope, ["s1"]);
    assert_eq!(state.workspace.findings.len(), 1);
    assert_eq!(
        state.workspace.plan_reviews["fixture-section-0"].conclusion,
        tools::PlanReviewConclusion::Findings
    );
    assert!(state.workspace.reviewing);
    assert!(!state.workspace.done);
}

#[test]
fn saved_old_implementations_do_not_skip_main_repair_or_compile_feedback() {
    for feedback in [false, true] {
        let (input, result, mut state) = composing();
        state.workspace.draft.sections.insert(
            "fixture-section-0".into(),
            serde_json::from_value(section(&input, 0)).unwrap(),
        );
        if feedback {
            state.workspace.compile_feedback = Some(tools::CompileFeedback {
                draft_sha256: digest(&state.workspace.draft).unwrap(),
                error: "uncovered obligation".into(),
            });
        } else {
            state.workspace.findings.push(serde_json::from_value(json!({"message":"repair required", "section_ids":["fixture-section-0"], "record_ids":["t0"], "sources":section(&input, 0)["grounds"]})).unwrap());
        }
        assert!(!agent_work::complete_assigned(&input, &result, &mut state));
        assert_eq!(state.main_work.as_ref().unwrap().status, Status::Active);
    }
}

#[test]
fn writing_a_different_item_in_the_same_source_is_not_focused_completion() {
    let (input, _, mut state) = composing();
    let mut other: Section = serde_json::from_value(section(&input, 0)).unwrap();
    other.id = "other-item".into();
    state
        .workspace
        .draft
        .sections
        .insert(other.id.clone(), other);
    assert!(
        agent_work::focused_completion(&state, "put_section", &json!({"id":"other-item"}))
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn received_and_committed_ack_loss_preserve_one_completed_item_and_its_successor() {
    for boundary in [2, 3] {
        let (input, result, state) = composing();
        let journal = Journal::default();
        *journal.state.lock().unwrap() = Some(json!(state));
        *journal.fail_boundary_ack.lock().unwrap() = Some(boundary);
        let model = Model {
            turns: Mutex::new(VecDeque::from([("put_section", section(&input, 0))])),
            invalid_replies: Mutex::new(0),
        };
        let cancel = tokio_util::sync::CancellationToken::new();
        assert!(
            agent::run(&input, &result, &config(), &journal, &model, &cancel)
                .await
                .is_err()
        );
        let saved: agent::Checkpoint =
            serde_json::from_value(journal.state.lock().unwrap().clone().unwrap()).unwrap();
        assert_eq!(saved.turn, usize::from(boundary == 3));
        assert_eq!(saved.journal.response().is_some(), boundary == 2);
        assert_eq!(
            saved.main_work.as_ref().unwrap().plan_item_id.as_deref(),
            Some(if boundary == 2 {
                "fixture-section-0"
            } else {
                "fixture-section-1"
            })
        );
        assert!(model.turns.lock().unwrap().is_empty());
        *journal.reject_reservation.lock().unwrap() = true;
        for _ in 0..2 {
            let error = agent::run(&input, &result, &config(), &journal, &model, &cancel)
                .await
                .unwrap_err();
            assert_eq!(error.message, "reservation transaction rejected");
            let committed: agent::Checkpoint =
                serde_json::from_value(journal.state.lock().unwrap().clone().unwrap()).unwrap();
            assert_eq!(committed.turn, 1);
            assert_eq!(committed.tool_calls, 1);
            assert_eq!(committed.main_progress.completions.len(), 1);
            assert_eq!(
                committed
                    .main_work
                    .as_ref()
                    .unwrap()
                    .plan_item_id
                    .as_deref(),
                Some("fixture-section-1")
            );
            assert_eq!(committed.main_work.as_ref().unwrap().source_scope, ["s1"]);
            assert_eq!(*journal.calls.lock().unwrap(), 1);
            assert!(!committed.workspace.done);
        }
    }
}
