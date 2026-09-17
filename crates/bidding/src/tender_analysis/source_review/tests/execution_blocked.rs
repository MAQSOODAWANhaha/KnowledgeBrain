use super::*;
use crate::agent_runtime::progress::{ExecutionBlocker, ProgressWatch};
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

fn blocked_remaining() -> (FrozenInput, Config, Checkpoint) {
    blocked_remaining_with_independent(false)
}

fn blocked_remaining_with_independent(independent: bool) -> (FrozenInput, Config, Checkpoint) {
    let (mut input, config, mut state) = fixture();
    input.source_units.push(Source {
        source_unit_revision_id: "pending".into(),
        document_id: "another-document".into(),
        text: "Another original needs independent review.".into(),
        locator: json!({}),
        ordinal: 1,
    });
    if independent {
        input.source_units.push(Source {
            source_unit_revision_id: "independent".into(),
            document_id: "independent-document".into(),
            text: "An independent original paragraph.".into(),
            locator: json!({}),
            ordinal: 2,
        });
    }
    state.input_sha256 = digest(&input).unwrap();
    state.analysis.dispositions.insert(
        "pending".into(),
        Disposition {
            state: DispositionState::NonRequirement,
            reason: "Background paragraph".into(),
        },
    );
    state.source_review = Some(initialize(&input, &config).unwrap());
    select_next(&input, &config, &mut state).unwrap();
    compare(&input, &config, &mut state);
    let args = judgment(&input, &config, &state);
    put(&input, &config, &mut state, &args).unwrap();
    state.reviewer_work.as_mut().unwrap().status = WorkStatus::Complete;
    let scope = vec!["pending".into()];
    state.reviewer_progress.blockers.push(ExecutionBlocker {
        dependencies_sha256: digest(&vec![
            context::reference(&state.analysis, "disposition:pending").unwrap(),
        ])
        .unwrap(),
        scope,
        watch: ProgressWatch {
            recovery: Recovery::Blocked,
            ..Default::default()
        },
    });
    select_next(&input, &config, &mut state).unwrap();
    assert_eq!(
        pending(&input, &config, &state).unwrap().len(),
        if independent { 2 } else { 1 }
    );
    assert_eq!(
        state.source_review.as_ref().unwrap().active_task.is_some(),
        independent
    );
    // This is the live failure shape: an old completed scope and running watch
    // do not make the remaining blocked source executable.
    assert_ne!(state.reviewer_progress.watch.recovery, Recovery::Blocked);
    (input, config, state)
}

#[test]
fn navigation_never_presents_an_unassigned_blocked_task_as_current() {
    let (input, config, state) = blocked_remaining();
    let before = digest(&state).unwrap();
    let navigation = packet(&input, &config, &state).unwrap();
    assert_eq!(navigation["remaining"], 1);
    assert!(navigation["current"].is_null());
    assert_eq!(navigation["execution_blocked"], true);
    assert_eq!(digest(&state).unwrap(), before);
    assert!(!state.done);
}

#[test]
fn an_independent_pending_source_remains_executable_without_clearing_blockers() {
    let (input, config, mut state) = blocked_remaining_with_independent(true);
    let blockers = json!(state.reviewer_progress.blockers);
    assert!(!execution_blocked(&input, &config, &state).unwrap());
    select_next(&input, &config, &mut state).unwrap();
    let navigation = packet(&input, &config, &state).unwrap();
    assert_eq!(navigation["remaining"], 2);
    assert_eq!(navigation["current"]["task"]["source_id"], "independent");
    assert_eq!(navigation["execution_blocked"], false);
    assert_eq!(json!(state.reviewer_progress.blockers), blockers);
    assert!(!state.done);
}

#[test]
fn a_changed_semantic_dependency_can_resume_without_refunding_prior_costs() {
    let (input, config, mut state) = blocked_remaining();
    state.turn = 7;
    state.tool_calls = 13;
    state.read_bytes = 97;
    state.reviewer_progress.blockers[0].watch.replans = 2;
    state.reviewer_progress.watch = state.reviewer_progress.blockers[0].watch.clone();
    let blockers = json!(state.reviewer_progress.blockers);
    assert!(execution_blocked(&input, &config, &state).unwrap());
    state
        .analysis
        .dispositions
        .get_mut("pending")
        .unwrap()
        .reason = "The corrected background classification needs independent comparison.".into();
    assert!(!execution_blocked(&input, &config, &state).unwrap());
    select_next(&input, &config, &mut state).unwrap();
    assert_eq!(
        packet(&input, &config, &state).unwrap()["current"]["task"]["source_id"],
        "pending"
    );
    assert_eq!(state.reviewer_progress.watch.replans, 2);
    assert_eq!(
        (state.turn, state.tool_calls, state.read_bytes),
        (7, 13, 97)
    );
    assert_eq!(json!(state.reviewer_progress.blockers), blockers);
    assert!(!state.done);
}

#[test]
fn no_pending_sources_does_not_skip_required_global_review() {
    let (input, config, mut state) = fixture();
    let args = judgment(&input, &config, &state);
    put(&input, &config, &mut state, &args).unwrap();
    select_next(&input, &config, &mut state).unwrap();
    assert!(pending(&input, &config, &state).unwrap().is_empty());
    assert!(!execution_blocked(&input, &config, &state).unwrap());
    assert!(packet(&input, &config, &state).unwrap()["current"].is_null());
    assert!(!review_complete(&input, &config, &state).unwrap());
    assert!(!state.done);
    crate::tender_analysis::tests::fixture_global_checks(&input, &config, &mut state);
    assert!(review_complete(&input, &config, &state).unwrap());
}

#[test]
fn invalid_task_inventory_is_not_disguised_as_execution_blockage() {
    let (input, config, mut state) = blocked_remaining();
    state.source_review.as_mut().unwrap().manifest_sha256 = "invalid".into();
    let error = execution_blocked(&input, &config, &state).unwrap_err();
    assert_eq!(error, "independent source task manifest changed");
    assert_eq!(select_next(&input, &config, &mut state).unwrap_err(), error);
}

struct Stored {
    state: Mutex<Checkpoint>,
    reservations: AtomicUsize,
}
#[async_trait]
impl Journal for Stored {
    async fn load(&self) -> Result<Option<Checkpoint>, AgentError> {
        Ok(Some(self.state.lock().unwrap().clone()))
    }
    async fn reserve(&self, _: &Checkpoint, _: &[u8]) -> Result<Option<usize>, AgentError> {
        self.reservations.fetch_add(1, Ordering::SeqCst);
        Err(AgentError::new(
            "INTERNAL",
            "unexpected reservation for blocked work",
        ))
    }
    async fn save(&self, state: &Checkpoint, _: &Value) -> Result<(), AgentError> {
        *self.state.lock().unwrap() = state.clone();
        Ok(())
    }
}
struct NoModel;
#[async_trait]
impl agent::Model for NoModel {
    async fn turn(&self, _: &Config, _: &[u8]) -> Result<ChatTurn, AgentError> {
        panic!("no provider call for wholly blocked remaining work")
    }
}

async fn freeze(input: &FrozenInput, config: &Config, state: &mut Checkpoint) {
    let body = request(input, config, state).await.unwrap();
    state
        .journal
        .prepare_session(
            &body,
            crate::agent_runtime::SESSION_PREFIX,
            crate::agent_runtime::ANALYSIS_SESSION_SUFFIX,
            config.limits.max_turns - state.turn,
            config.limits.max_context_bytes,
        )
        .unwrap();
    state
        .journal
        .prepare(state.turn, "reviewer", &body)
        .unwrap();
}

#[tokio::test]
async fn completed_sources_cannot_authorize_new_or_prepared_blocked_review_calls() {
    for prepared in [false, true] {
        let (input, config, mut state) = blocked_remaining();
        if prepared {
            freeze(&input, &config, &mut state).await;
        }
        let before = digest(&state).unwrap();
        let journal = Stored {
            state: Mutex::new(state),
            reservations: AtomicUsize::new(0),
        };
        let error = agent::run(
            &input,
            &config,
            &journal,
            &NoModel,
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "AGENT_OUTPUT_INVALID", "{error:?}");
        assert!(
            error.message.contains("no_executable_source_review_tasks"),
            "{error:?}"
        );
        assert_eq!(journal.reservations.load(Ordering::SeqCst), 0);
        assert_eq!(digest(&*journal.state.lock().unwrap()).unwrap(), before);
    }
}

#[tokio::test]
async fn saved_response_commits_before_stopping_wholly_blocked_review() {
    let (input, config, mut state) = blocked_remaining();
    freeze(&input, &config, &mut state).await;
    state
        .journal
        .responded(ChatTurn {
            finish_reason: "tool_calls".into(),
            tool_calls: vec![knowledge::models::ChatToolCall {
                id: "saved-navigation".into(),
                name: "source_index".into(),
                arguments: json!({"offset":0,"limit":10}).to_string(),
            }],
            ..Default::default()
        })
        .unwrap();
    let original = state.clone();
    let journal = Stored {
        state: Mutex::new(state),
        reservations: AtomicUsize::new(0),
    };
    let error = agent::run(
        &input,
        &config,
        &journal,
        &NoModel,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "AGENT_OUTPUT_INVALID", "{error:?}");
    let saved = journal.state.lock().unwrap();
    assert_eq!(saved.turn, original.turn + 1);
    assert_eq!(saved.tool_calls, original.tool_calls + 1);
    assert!(saved.journal.pending.is_none());
    assert_eq!(
        json!(saved.reviewer_progress.blockers),
        json!(original.reviewer_progress.blockers)
    );
    assert_eq!(json!(saved.analysis), json!(original.analysis));
    assert!(!saved.done);
    assert_eq!(journal.reservations.load(Ordering::SeqCst), 0);
}
