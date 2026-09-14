use super::*;
use crate::agent_runtime::progress::{ExecutionBlocker, ProgressWatch, Recovery};

async fn fixture() -> (Checkpoint, Config) {
    let (journal, config, _, _, _) = super::repair_history::handled().await;
    let mut state = journal.load().await.unwrap().unwrap();
    let values: Vec<_> = agent::context::scope_references(&state.analysis, &["source".into()])
        .iter()
        .map(|key| agent::context::reference(&state.analysis, key).unwrap())
        .collect();
    state.main_progress.blockers.push(ExecutionBlocker {
        scope: vec!["source".into()],
        dependencies_sha256: digest(&values).unwrap(),
        watch: ProgressWatch {
            no_progress_turns: 6,
            focus_turns: 6,
            replans: 2,
            recovery: Recovery::Blocked,
        },
    });
    state.main_work = None;
    state.main_progress.watch = ProgressWatch::default();
    (state, config)
}

fn row(state: &Checkpoint) -> Value {
    agent::context::execution_packet(state, 16_384).unwrap()["blockers"]["items"][0].clone()
}

fn messages(state: &Checkpoint, config: &Config) -> Vec<Value> {
    let result = agent::inspect_review(
        state,
        &json!({"offset":0,"limit":1}),
        config.limits.max_tool_result_bytes,
    )
    .unwrap();
    vec![
        json!({"role":"assistant","tool_calls":[{"id":"history","function":{"name":"inspect_review"}}]}),
        json!({"role":"tool","tool_call_id":"history","content":json!({"ok":true,"result":result}).to_string()}),
    ]
}

#[tokio::test]
async fn current_undelivered_history_is_discoverable_without_granting_recovery() {
    let (state, _) = fixture().await;
    let before = digest(&state).unwrap();
    assert_eq!(
        agent::repair_feedback_packet(&state, &[], &crate::tender_analysis::tests::config().limits)
            .unwrap()["repair"]["pending"],
        0
    );
    let value = row(&state);
    assert_eq!(
        value["history_to_read"]["inspect_review"],
        json!({"offset":0,"limit":1})
    );
    assert!(value.get("history_recovery").is_none());
    assert!(!agent::repair_recovery::available(&state, &["source".into()]).unwrap());
    assert!(agent::context::check_blocked_scope(&state, &["source".into()]).is_err());
    assert_eq!(digest(&state).unwrap(), before);
    assert!(agent::context::execution_packet(&state, 256).is_err());
}

#[tokio::test]
async fn history_navigation_becomes_only_the_existing_one_time_handoff_after_delivery() {
    let (mut state, config) = fixture().await;
    let original = digest(&state).unwrap();
    let page = messages(&state, &config);
    // Constructing/reading the page is not confirmation that the model received it.
    assert!(agent::context::check_blocked_scope(&state, &["source".into()]).is_err());
    assert_eq!(digest(&state).unwrap(), original);
    let confirmed = agent::repair_recovery::delivered(&state, &page).unwrap();
    state.main_progress.seen.extend(confirmed);
    let value = row(&state);
    assert!(value.get("history_to_read").is_none());
    assert!(value.get("history_recovery").is_some());
    agent::apply(&input(), &config, &mut state, "set_work_note", &json!({
        "source_scope":["source"],"objective":"Reconcile the saved correction with original evidence",
        "status":"active","note":"Use the existing one-time history opportunity"
    })).unwrap();
    assert_eq!(state.main_progress.watch.replans, 2);
    let value = row(&state);
    assert!(value.get("history_to_read").is_none());
    assert!(value.get("history_recovery").is_none());
    let repeated = agent::repair_recovery::delivered(&state, &page).unwrap();
    state.main_progress.seen.extend(repeated);
    assert!(!agent::repair_recovery::available(&state, &["source".into()]).unwrap());
    assert!(row(&state).get("history_to_read").is_none());
}

#[tokio::test]
async fn known_history_never_handled_and_reviewer_role_have_no_reading_invitation() {
    let (mut state, config) = fixture().await;
    let blockers = std::mem::take(&mut state.main_progress.blockers);
    let confirmed = agent::repair_recovery::delivered(&state, &messages(&state, &config)).unwrap();
    state.main_progress.seen.extend(confirmed);
    state.main_progress.blockers = blockers;
    assert!(row(&state).get("history_to_read").is_none());
    assert!(!agent::repair_recovery::available(&state, &["source".into()]).unwrap());
    let (mut never_handled, _) = fixture().await;
    never_handled.repair.results.clear();
    assert!(row(&never_handled).get("history_to_read").is_none());
    let (mut reviewer, _) = fixture().await;
    reviewer.role = Role::Reviewer;
    reviewer.reviewer_progress.blockers = reviewer.main_progress.blockers.clone();
    assert!(row(&reviewer).get("history_to_read").is_none());
}

#[tokio::test]
async fn navigation_skips_missing_history_and_preserves_the_original_offset() {
    let (mut state, _) = fixture().await;
    let original = state.findings_for_repair()[0].clone();
    let mut unhandled = original.clone();
    unhandled.message.push_str(" Separate outstanding concern.");
    state.review_draft = BTreeMap::from([("first".into(), unhandled), ("second".into(), original)]);
    assert_eq!(
        row(&state)["history_to_read"]["inspect_review"],
        json!({"offset":1,"limit":1})
    );
    let original = state.review_draft["second"].clone();
    let unrelated = state.review_draft["first"].clone();
    state.review = Some(Review {
        analysis_sha256: digest(&state.analysis).unwrap(),
        coverage: Coverage::default(),
        findings: vec![unrelated.clone(), unrelated, original.clone()],
    });
    let query = row(&state)["history_to_read"]["inspect_review"].clone();
    assert_eq!(query, json!({"offset":2,"limit":1}));
    assert_eq!(
        agent::inspect_review(&state, &query, 16_384).unwrap()["items"],
        json!([original])
    );
    state.main_progress.blockers[0].scope = vec!["unrelated".into()];
    assert!(row(&state).get("history_to_read").is_none());
}
