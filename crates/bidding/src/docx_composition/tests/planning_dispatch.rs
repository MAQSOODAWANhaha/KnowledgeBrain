use super::*;
use crate::docx_composition::agent_work::{self, Action, Status};
use knowledge::models::{ChatToolCall, ChatTurn};

fn partially_planned(input: &FrozenInput, result: &AnalysisResult) -> agent::Checkpoint {
    let mut workspace = ready(input, result);
    workspace.draft.sections.clear();
    workspace.draft.plan.remove("fixture-section-1");
    assert!(!plan_complete(result, &workspace.draft).unwrap());
    work_checkpoint(workspace)
}

async fn call(
    input: &FrozenInput,
    result: &AnalysisResult,
    state: &mut agent::Checkpoint,
    name: &str,
    args: Value,
) -> Value {
    let call_id = format!("planning-{}", state.turn);
    let outputs = agent::execute_turn(
        input,
        result,
        &config(),
        state,
        ChatTurn {
            finish_reason: "tool_calls".into(),
            tool_calls: vec![ChatToolCall {
                id: call_id,
                name: name.into(),
                arguments: args.to_string(),
            }],
            ..Default::default()
        },
        Default::default(),
        &tokio_util::sync::CancellationToken::new(),
    )
    .await
    .unwrap();
    serde_json::from_str(outputs[0]["content"].as_str().unwrap()).unwrap()
}

#[test]
fn incomplete_plan_cannot_assign_or_claim_a_section() {
    let (input, result) = fixture();
    let mut state = partially_planned(&input, &result);
    assert!(agent_work::packet(&input, &result, &state)["assigned_plan_item"].is_null());
    let planning = json!({
        "source_scope":["s0","s1"], "section_scope":[], "plan_item_id":null,
        "action":"locate", "objective":"Plan the remaining tender obligations",
        "note":"", "status":"active"
    });
    for (item, sections) in [
        (json!("fixture-section-0"), json!([])),
        (Value::Null, json!(["fixture-section-0"])),
        (json!("fixture-section-0"), json!(["fixture-section-0"])),
    ] {
        let before = digest(&state).unwrap();
        let mut attempted = planning.clone();
        attempted["plan_item_id"] = item;
        attempted["section_scope"] = sections;
        assert!(
            agent_work::set_work(&input, &result, &mut state, &attempted, 100_000).is_err(),
            "partial planning must not grant chapter implementation authority"
        );
        assert_eq!(digest(&state).unwrap(), before);
    }
    agent_work::set_work(&input, &result, &mut state, &planning, 100_000).unwrap();
    assert!(state.main_work.as_ref().unwrap().plan_item_id.is_none());
    assert!(state.main_work.as_ref().unwrap().section_scope.is_empty());
    assert!(agent_work::packet(&input, &result, &state)["assigned_plan_item"].is_null());
}

#[tokio::test]
async fn last_plan_write_installs_first_section_without_granting_reading_receipts() {
    let (input, result) = fixture();
    let mut state = partially_planned(&input, &result);
    let before_main = digest(&state.workspace.source_coverage).unwrap();
    let before_review = digest(&state.workspace.review_coverage).unwrap();
    let before_inspected = state.workspace.inspected.clone();
    let mut plan = plan_for_section(&result, &section(&input, 1));
    plan["expected_draft_sha256"] = json!(digest(&state.workspace.draft).unwrap());
    let output = call(
        &input,
        &result,
        &mut state,
        "put_composition_plan_item",
        plan,
    )
    .await;
    assert_eq!(output["ok"], true, "{output}");
    assert!(plan_complete(&result, &state.workspace.draft).unwrap());
    let work = state
        .main_work
        .as_ref()
        .expect("host must save its assignment");
    assert_eq!(work.plan_item_id.as_deref(), Some("fixture-section-0"));
    assert_eq!(work.section_scope, ["fixture-section-0"]);
    assert_eq!(work.source_scope, ["s0"]);
    assert_eq!(work.action, Action::Compose);
    assert_eq!(work.status, Status::Active);
    assert_eq!(
        digest(&state.workspace.source_coverage).unwrap(),
        before_main
    );
    assert_eq!(
        digest(&state.workspace.review_coverage).unwrap(),
        before_review
    );
    assert_eq!(state.workspace.inspected, before_inspected);
    assert!(state.pending_delivery.is_none());
    assert!(state.workspace.draft.sections.is_empty());
    assert_eq!(state.tool_calls, 1);
    assert!(!state.workspace.reviewing);
    assert!(!state.workspace.done);
}

#[tokio::test]
async fn entering_reviewer_saves_the_first_review_work_without_inheriting_main_evidence() {
    let (input, result) = fixture();
    let mut state = work_checkpoint(ready(&input, &result));
    let before_main = digest(&state.workspace.source_coverage).unwrap();
    let before_review = digest(&state.workspace.review_coverage).unwrap();
    let output = call(
        &input,
        &result,
        &mut state,
        "composition_coverage",
        json!({"offset":0,"limit":100}),
    )
    .await;
    assert_eq!(output["ok"], true, "{output}");
    assert!(state.workspace.artifact.is_some());
    assert!(state.workspace.reviewing);
    let work = state
        .review_work
        .as_ref()
        .expect("review work must be durable");
    assert_eq!(work.plan_item_id.as_deref(), Some("fixture-section-0"));
    assert_eq!(work.section_scope, ["fixture-section-0"]);
    assert_eq!(work.source_scope, ["s0"]);
    assert_eq!(work.action, Action::Review);
    assert_eq!(work.status, Status::Active);
    assert_eq!(
        digest(&state.workspace.source_coverage).unwrap(),
        before_main
    );
    assert_eq!(
        digest(&state.workspace.review_coverage).unwrap(),
        before_review
    );
    assert!(state.workspace.inspected.is_empty());
    assert!(state.workspace.plan_reviews.is_empty());
    assert!(!state.workspace.done);
}

#[tokio::test]
async fn generated_plan_identity_replays_from_the_same_received_baseline() {
    let (input, result) = fixture();
    let mut first = partially_planned(&input, &result);
    let mut replay: agent::Checkpoint = serde_json::from_value(json!(first)).unwrap();
    let mut plan = plan_for_section(&result, &section(&input, 1));
    plan["id"] = Value::Null;
    plan["expected_draft_sha256"] = json!(digest(&first.workspace.draft).unwrap());
    let original = call(
        &input,
        &result,
        &mut first,
        "put_composition_plan_item",
        plan.clone(),
    )
    .await;
    let repeated = call(
        &input,
        &result,
        &mut replay,
        "put_composition_plan_item",
        plan,
    )
    .await;
    assert_eq!(original["ok"], true, "{original}");
    assert!(original["result"]["id"].is_string());
    assert_eq!(
        original, repeated,
        "a replay cannot allocate a new chapter identity"
    );
    assert_eq!(
        json!(first),
        json!(replay),
        "all committed business and dispatch state must match"
    );
}
