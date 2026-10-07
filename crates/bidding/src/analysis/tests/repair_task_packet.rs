use super::*;

fn packet(state: &Checkpoint) -> Value {
    agent::repair_feedback_packet(state, &[], &config().limits).unwrap()["repair"].clone()
}

#[tokio::test]
async fn assigned_repair_task_overrides_current_source_priority_without_mutating_state() {
    let (mut state, a, _) = super::repair_navigation::fixture().await;
    state.main_progress.blockers.clear();
    agent::repair::tasks::select(&mut state, "a", &config().limits).unwrap();
    let before = digest(&state).unwrap();
    let value = packet(&state);
    assert_eq!(value["next_finding"]["finding_sha256"], a);
    assert_eq!(value["next_finding"]["task_id"], "a");
    assert_eq!(
        value["next_finding"]["scope_handoff"]["source_scope"],
        json!(["source"])
    );
    assert_eq!(value["pending"], 2);
    assert_eq!(digest(&state).unwrap(), before);
}

#[tokio::test]
async fn missing_active_task_does_not_choose_an_independent_finding_or_claim_all_blocked() {
    let (mut state, _, _) = super::repair_navigation::fixture().await;
    state.repair.tasks.active = None;
    let before = digest(&state).unwrap();
    let value = packet(&state);
    assert!(value["next_finding"].is_null());
    assert_eq!(value["pending"], 2);
    assert_eq!(value["blocked_pending"], 1);
    assert!(
        value["instruction"]
            .as_str()
            .unwrap()
            .contains("No unfinished repair task is currently assigned")
    );
    assert_eq!(digest(&state).unwrap(), before);
}

#[tokio::test]
async fn task_allowance_exhaustion_stays_pending_without_a_source_blocker() {
    let (mut state, _, _) = super::repair_navigation::fixture().await;
    state.main_progress.blockers.clear();
    let cap = agent::repair::tasks::limit(&config().limits).unwrap();
    for task in state.repair.tasks.entries.values_mut() {
        task.committed_turns = cap;
    }
    let value = packet(&state);
    assert!(value["next_finding"].is_null());
    assert_eq!(value["blocked_pending"], 2);
    assert_eq!(value["pending"], 2);
    assert_eq!(value["handled"], 0);
}

#[tokio::test]
async fn current_dispositions_clear_task_navigation_but_do_not_clear_legacy_blockers() {
    let (mut state, _, _) = super::repair_navigation::fixture().await;
    state.repair.feedback_sha256 =
        Some(digest(&json!([state.review_rounds, state.findings_for_repair()])).unwrap());
    for finding in state.review_draft.values() {
        state.repair.results.insert(
            digest(finding).unwrap(),
            agent::repair::Receipt {
                conclusion: agent::repair::Conclusion::Disputed,
                summary: "Original source resolves this synthetic source-only question".into(),
                sources: finding.sources.clone(),
                candidate_versions: BTreeMap::new(),
            },
        );
    }
    let before = digest(&state).unwrap();
    let value = packet(&state);
    assert!(value["next_finding"].is_null());
    assert!(value["next_blocked_finding"].is_null());
    assert_eq!(value["handled"], 2);
    assert_eq!(value["pending"], 0);
    assert_eq!(state.main_progress.blockers.len(), 1);
    assert_eq!(digest(&state).unwrap(), before);
    assert!(
        value["instruction"]
            .as_str()
            .unwrap()
            .contains("does not grant independent approval")
    );
}

#[tokio::test]
async fn current_repair_receipts_do_not_bypass_exhausted_handoff_on_resume() {
    let (journal, config, _, _, _) = super::repair_history::handled().await;
    let mut state = journal.load().await.unwrap().unwrap();
    let tasks = agent::repair::tasks::current(&state, &config.limits).unwrap();
    assert!(!tasks.is_empty());
    assert!(tasks.iter().all(|task| task.complete));
    assert!(state.repair.tasks.active.is_none());
    assert!(state.journal.pending.is_none());
    assert_eq!(state.role, Role::Main);
    assert!(state.turn < config.limits.max_turns);
    assert!(state.tool_calls < config.limits.max_tool_calls);
    assert!(state.read_bytes < config.limits.max_read_bytes);
    state.main_progress.watch = crate::agent_runtime::progress::ProgressWatch {
        recovery: crate::agent_runtime::progress::Recovery::Blocked,
        replans: config.limits.max_focus_replans,
        no_progress_turns: config.limits.max_no_progress_turns * 2,
        focus_turns: config.limits.max_focus_turns,
    };
    let agent::main_dispatch::Active::Ordinary(root) = state.dispatch.active.clone().unwrap()
    else {
        panic!("global root after repair");
    };
    let entry = state.dispatch.entries.get_mut(&root).unwrap();
    entry.watch = state.main_progress.watch.clone();
    entry.spent_replans = config.limits.max_focus_replans;
    entry.spent_batches = agent::repair::tasks::limit(&config.limits).unwrap();
    let before = digest(&state).unwrap();
    *journal.state.lock().unwrap() = Some(serde_json::from_value(json!(state)).unwrap());
    let reservations = journal.reservations.lock().unwrap().clone();
    let model = work_script(vec![]);
    let error = agent::run(
        &input(),
        &config,
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "AGENT_TURN_BUDGET_EXCEEDED");
    assert!(model.bodies.lock().unwrap().is_empty());
    assert_eq!(*journal.reservations.lock().unwrap(), reservations);
    assert_eq!(
        digest(&journal.load().await.unwrap().unwrap()).unwrap(),
        before
    );
}

#[tokio::test]
async fn task_scope_expansion_and_scheduling_cannot_reopen_changed_legacy_blocker() {
    let (mut state, _, _) = super::repair_navigation::fixture().await;
    let config = config();
    let mut input = input();
    let mut independent = input.source_units[0].clone();
    independent.source_unit_revision_id = "independent".into();
    independent.ordinal += 1;
    input.source_units.push(independent);
    // Changed business dependencies allow the old source recovery path, but
    // must not grant that recovery to an independently assigned repair task.
    state.main_progress.blockers[0].dependencies_sha256 =
        digest(&json!(["prior synthetic source dependencies"])).unwrap();
    let expanded = vec!["independent".into(), "source".into()];
    agent::context::check_blocked_scope(&state, &expanded).unwrap();
    let mut work = active_work("independent");
    work["source_scope"] = json!(expanded);
    let before = digest(&state).unwrap();
    let error = agent::apply(&input, &config, &mut state, "set_work_note", &work).unwrap_err();
    assert!(error.contains("legacy blocked source"), "{error}");
    assert_eq!(digest(&state).unwrap(), before);

    let markers = state.main_progress.seen.clone();
    let blockers = digest(&state.main_progress.blockers).unwrap();
    state.repair.tasks.active = None;
    agent::repair_task_host::schedule(&mut state, &config.limits).unwrap();
    assert_eq!(state.repair.tasks.active.as_deref(), Some("b"));
    state
        .repair
        .tasks
        .entries
        .get_mut("b")
        .unwrap()
        .committed_turns = agent::repair::tasks::limit(&config.limits).unwrap();
    let ledger = digest(&state.repair.tasks.entries).unwrap();
    assert!(
        agent::repair::tasks::current(&state, &config.limits)
            .unwrap()
            .iter()
            .any(|task| task.id == "a" && !task.complete && !task.exhausted)
    );
    agent::repair_task_host::schedule(&mut state, &config.limits).unwrap();
    assert!(state.repair.tasks.active.is_none());
    assert_eq!(digest(&state.repair.tasks.entries).unwrap(), ledger);
    assert_eq!(state.main_progress.seen, markers);
    assert_eq!(digest(&state.main_progress.blockers).unwrap(), blockers);
}
