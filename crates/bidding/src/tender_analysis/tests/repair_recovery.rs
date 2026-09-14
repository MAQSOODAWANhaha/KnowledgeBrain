use super::*;
use crate::agent_runtime::progress::{ExecutionBlocker, ProgressWatch, Recovery};

async fn blocked() -> (MemoryJournal, Config, String) {
    let (journal, config, _, sha, _) = super::repair_history::handled().await;
    let mut state = journal.load().await.unwrap().unwrap();
    let scope = vec!["source".to_string()];
    let values: Vec<_> = agent::context::scope_references(&state.analysis, &scope)
        .iter()
        .map(|key| agent::context::reference(&state.analysis, key).unwrap())
        .collect();
    state.main_progress.blockers = vec![ExecutionBlocker {
        scope,
        dependencies_sha256: digest(&values).unwrap(),
        watch: ProgressWatch {
            no_progress_turns: 6,
            focus_turns: 6,
            replans: 2,
            recovery: Recovery::Blocked,
        },
    }];
    state.main_progress.watch = ProgressWatch::default();
    state.main_work = None;
    *journal.state.lock().unwrap() = Some(state);
    (journal, config, sha)
}

fn work(scope: &str) -> Value {
    json!({"source_scope":[scope],"objective":"Resolve the original outstanding correction", "status":"active","note":"Compare the prior disposition and original finding"})
}

fn stale_candidate(state: &mut Checkpoint, config: &Config) {
    let id = state.findings_for_repair()[0].affected[0].id.clone();
    let mut record = json!(state.analysis.records[&id]);
    record["data"]["text"] = json!("待重新核对的提交格式要求");
    let mut coverage = state.analysis.coverage.clone();
    tools::invoke(
        &input(),
        &mut state.analysis,
        &mut coverage,
        false,
        "put_record",
        &record,
        config.limits.max_tool_result_bytes,
    )
    .unwrap();
}

async fn stale_blocked() -> (MemoryJournal, Config) {
    let (journal, config, _) = blocked().await;
    let mut state = journal.load().await.unwrap().unwrap();
    // Construct a failed-attempt snapshot after a normal candidate edit. The
    // review generation and prior receipt stay intact; only its dependency is stale.
    stale_candidate(&mut state, &config);
    let values: Vec<_> = agent::context::scope_references(&state.analysis, &["source".into()])
        .iter()
        .map(|key| agent::context::reference(&state.analysis, key).unwrap())
        .collect();
    state.main_progress.blockers[0].dependencies_sha256 = digest(&values).unwrap();
    *journal.state.lock().unwrap() = Some(state);
    (journal, config)
}

fn independent_input(state: &mut Checkpoint) -> FrozenInput {
    let mut extended = input();
    let mut independent = extended.source_units[0].clone();
    independent.source_unit_revision_id = "independent".into();
    independent.ordinal = 1;
    independent.text.clear();
    extended.source_units.push(independent);
    state.analysis.dispositions.insert(
        "independent".into(),
        Disposition {
            state: DispositionState::NonRequirement,
            reason: "Empty independent fixture source".into(),
        },
    );
    extended
}

#[tokio::test]
async fn completed_history_delivery_allows_one_retry_with_spent_replans() {
    let (journal, config, _) = blocked().await;
    let state = journal.load().await.unwrap().unwrap();
    let prior = state.main_progress.blockers[0].dependencies_sha256.clone();
    let state = super::repair::repair_steps(
        &journal,
        &config,
        vec![
            ("inspect_review", json!({"offset":0,"limit":1})),
            ("set_work_note", work("source")),
        ],
    )
    .await;
    assert!(
        state.main_work.is_some(),
        "delivered full prior history must open the bounded retry"
    );
    assert_eq!(state.main_progress.watch.replans, 2);
    assert_eq!(state.main_progress.blockers[0].dependencies_sha256, prior);
    assert!(state.main_progress.watch.focus_turns > 0);
}

#[tokio::test]
async fn history_page_without_completed_delivery_cannot_open_scope() {
    let (journal, config, _) = blocked().await;
    let mut state = journal.load().await.unwrap().unwrap();
    let original = digest(&state.main_progress.blockers).unwrap();
    agent::inspect_review(
        &state,
        &json!({"offset":0,"limit":1}),
        config.limits.max_tool_result_bytes,
    )
    .unwrap();
    assert!(
        agent::apply(
            &input(),
            &config,
            &mut state,
            "set_work_note",
            &work("source")
        )
        .is_err()
    );
    assert_eq!(digest(&state.main_progress.blockers).unwrap(), original);
}

fn messages(state: &Checkpoint, config: &Config) -> Vec<Value> {
    let result = agent::inspect_review(
        state,
        &json!({"offset":0,"limit":10}),
        config.limits.max_tool_result_bytes,
    )
    .unwrap();
    vec![
        json!({"role":"assistant","tool_calls":[{"id":"feedback","function":{"name":"inspect_review"}}]}),
        json!({"role":"tool","tool_call_id":"feedback","content":json!({"ok":true,"result":result}).to_string()}),
    ]
}

#[tokio::test]
async fn eligibility_does_not_change_blocker_until_consumed_and_never_replenishes() {
    let (journal, config, sha) = blocked().await;
    let mut state = journal.load().await.unwrap().unwrap();
    let scope = vec!["source".to_string()];
    let prior = state.main_progress.blockers[0].dependencies_sha256.clone();
    let delivery = agent::repair_recovery::delivered(&state, &messages(&state, &config)).unwrap();
    assert_eq!(
        delivery
            .iter()
            .filter(|token| token.contains(":eligible:"))
            .count(),
        1
    );
    let frozen = json!([
        state.analysis,
        state.reviewer_progress,
        state.reviewer_coverage,
        state.turn,
        state.tool_calls,
        state.read_bytes,
        state.journal
    ]);
    state.main_progress.seen.extend(delivery.clone());
    assert_eq!(
        agent::repair_recovery::dependencies(&state, &scope, prior.clone()).unwrap(),
        prior
    );
    assert!(agent::repair_recovery::available(&state, &scope).unwrap());
    let diagnostics = agent::context::execution_packet(&state, 16_384).unwrap();
    assert_eq!(
        diagnostics["blockers"]["items"][0]["history_recovery"]["tool"],
        "set_work_note"
    );

    state.main_progress.watch = state.main_progress.blockers[0].watch.clone();
    state.main_work = Some(serde_json::from_value(work("source")).unwrap());
    agent::context::observe_progress(&mut state, &Role::Main, None, &config.limits).unwrap();
    assert!(
        agent::repair_recovery::available(&state, &scope).unwrap(),
        "blocked handoff must not eat eligibility"
    );
    agent::apply(
        &input(),
        &config,
        &mut state,
        "set_work_note",
        &work("source"),
    )
    .unwrap();
    assert_eq!(state.main_progress.watch.replans, 2);
    assert!(!agent::repair_recovery::available(&state, &scope).unwrap());
    let effective = agent::repair_recovery::dependencies(&state, &scope, prior.clone()).unwrap();
    assert_ne!(effective, prior);
    state.main_progress.watch.no_progress_turns = 5;
    state.main_progress.watch.focus_turns = 5;
    // All existing domain versions have already been observed above.
    agent::context::observe_progress(&mut state, &Role::Main, None, &config.limits).unwrap();
    assert_eq!(state.main_progress.watch.recovery, Recovery::Blocked);
    assert_eq!(
        state.main_progress.blockers[0].dependencies_sha256,
        effective
    );
    state
        .repair
        .results
        .get_mut(&sha)
        .unwrap()
        .summary
        .push_str(" Reworded only.");
    assert!(
        agent::repair_recovery::delivered(&state, &messages(&state, &config))
            .unwrap()
            .is_subset(&delivery)
    );
    state.main_progress.seen.extend(delivery);
    let mut restored: Checkpoint = serde_json::from_value(json!(state)).unwrap();
    assert!(
        agent::apply(
            &input(),
            &config,
            &mut restored,
            "set_work_note",
            &work("source")
        )
        .is_err()
    );
    assert_eq!(
        json!([
            restored.analysis,
            restored.reviewer_progress,
            restored.reviewer_coverage,
            restored.turn,
            restored.tool_calls,
            restored.read_bytes,
            restored.journal
        ]),
        frozen
    );
}

#[tokio::test]
async fn active_retry_keeps_focus_when_leaving_and_reentering_after_restart() {
    let (journal, config, _) = blocked().await;
    let mut state = journal.load().await.unwrap().unwrap();
    let delivery = agent::repair_recovery::delivered(&state, &messages(&state, &config)).unwrap();
    state.main_progress.seen.extend(delivery);
    agent::apply(
        &input(),
        &config,
        &mut state,
        "set_work_note",
        &work("source"),
    )
    .unwrap();
    agent::context::observe_progress(&mut state, &Role::Main, None, &config.limits).unwrap();
    state.main_progress.watch.focus_turns = 5;
    state.main_progress.watch.no_progress_turns = 3;
    agent::context::observe_progress(&mut state, &Role::Main, None, &config.limits).unwrap();
    let spent = json!(state.main_progress.watch);
    let mut extended = input();
    let mut independent = extended.source_units[0].clone();
    independent.source_unit_revision_id = "independent".into();
    independent.ordinal = 1;
    extended.source_units.push(independent);
    let mut independent_work = work("independent");
    independent_work["deferred_sources"] = json!(["source"]);
    agent::apply(
        &extended,
        &config,
        &mut state,
        "set_work_note",
        &independent_work,
    )
    .unwrap();
    // Even a completed independent task cannot erase the target's spent watch.
    state.main_progress.watch = ProgressWatch::default();
    let mut state: Checkpoint = serde_json::from_value(json!(state)).unwrap();
    let mut returning = work("source");
    returning["deferred_sources"] = json!(["independent"]);
    agent::apply(&extended, &config, &mut state, "set_work_note", &returning).unwrap();
    assert_eq!(json!(state.main_progress.watch), spent);
}

#[tokio::test]
async fn expanded_scope_completion_cannot_erase_pending_recovery_target() {
    let (journal, config) = stale_blocked().await;
    let mut state = journal.load().await.unwrap().unwrap();
    let feedback_generation = state.repair.feedback_sha256.clone();
    assert_eq!(
        feedback_generation,
        Some(digest(&json!([state.review_rounds, state.findings_for_repair()])).unwrap())
    );
    let scope = vec!["source".to_string()];
    let dependencies = |state: &Checkpoint| {
        digest(
            &agent::context::scope_references(&state.analysis, &scope)
                .iter()
                .map(|key| agent::context::reference(&state.analysis, key).unwrap())
                .collect::<Vec<_>>(),
        )
        .unwrap()
    };
    let original_dependencies = dependencies(&state);
    let pending_before =
        agent::repair_feedback_packet(&state, &[], &crate::tender_analysis::tests::config().limits)
            .unwrap()["repair"]["pending"]
            .clone();
    assert!(pending_before.as_u64().unwrap() > 0);
    let delivery = agent::repair_recovery::delivered(&state, &messages(&state, &config)).unwrap();
    state.main_progress.seen.extend(delivery);
    agent::apply(
        &input(),
        &config,
        &mut state,
        "set_work_note",
        &work("source"),
    )
    .unwrap();
    agent::context::observe_progress(&mut state, &Role::Main, None, &config.limits).unwrap();
    assert!(!agent::repair_recovery::available(&state, &scope).unwrap());

    let extended = independent_input(&mut state);
    let mut expanded = work("source");
    expanded["source_scope"] = json!(["source", "independent"]);
    agent::apply(&extended, &config, &mut state, "set_work_note", &expanded).unwrap();
    agent::context::observe_progress(&mut state, &Role::Main, None, &config.limits).unwrap();
    let target_watch = json!(state.main_progress.blockers[0].watch);
    assert!(state.main_progress.blockers[0].watch.focus_turns > 0);
    expanded["status"] = json!("complete");
    let completion = agent::apply(&extended, &config, &mut state, "set_work_note", &expanded);
    if completion.is_ok() {
        agent::context::observe_progress(&mut state, &Role::Main, None, &config.limits).unwrap();
    }
    assert_eq!(dependencies(&state), original_dependencies);
    assert_eq!(state.repair.feedback_sha256, feedback_generation);
    assert_eq!(
        agent::repair_feedback_packet(&state, &[], &crate::tender_analysis::tests::config().limits)
            .unwrap()["repair"]["pending"],
        pending_before
    );
    let target = state
        .main_progress
        .blockers
        .iter()
        .find(|blocker| blocker.scope == scope);
    assert!(
        target.is_some(),
        "completing an expanded scope erased the unchanged recovery target while its finding remains pending"
    );
    assert_eq!(json!(target.unwrap().watch), target_watch);
    assert!(
        completion.is_err(),
        "pending recovery target must prevent this completion"
    );
}

#[tokio::test]
async fn subset_scope_completion_cannot_reset_pending_recovery_target_watch() {
    let (journal, config) = stale_blocked().await;
    let mut state = journal.load().await.unwrap().unwrap();
    let extended = independent_input(&mut state);
    let scope = vec!["source".to_string(), "independent".to_string()];
    let values: Vec<_> = agent::context::scope_references(&state.analysis, &scope)
        .iter()
        .map(|key| agent::context::reference(&state.analysis, key).unwrap())
        .collect();
    // This initial failed snapshot covered both sources. No target is removed
    // or edited during the completion path exercised below.
    state.main_progress.blockers[0].scope = scope.clone();
    state.main_progress.blockers[0].dependencies_sha256 = digest(&values).unwrap();
    let delivery = agent::repair_recovery::delivered(&state, &messages(&state, &config)).unwrap();
    state.main_progress.seen.extend(delivery);
    let mut expanded = work("source");
    expanded["source_scope"] = json!(scope);
    agent::apply(&extended, &config, &mut state, "set_work_note", &expanded).unwrap();
    agent::context::observe_progress(&mut state, &Role::Main, None, &config.limits).unwrap();
    let mut subset = work("source");
    subset["deferred_sources"] = json!(["independent"]);
    agent::apply(&extended, &config, &mut state, "set_work_note", &subset).unwrap();
    agent::context::observe_progress(&mut state, &Role::Main, None, &config.limits).unwrap();
    let before = json!(state.main_progress.blockers);
    assert!(state.main_progress.blockers[0].watch.focus_turns > 0);
    assert!(
        agent::repair_feedback_packet(&state, &[], &crate::tender_analysis::tests::config().limits)
            .unwrap()["repair"]["pending"]
            .as_u64()
            .unwrap()
            > 0
    );
    subset["status"] = json!("complete");
    let completion = agent::apply(&extended, &config, &mut state, "set_work_note", &subset);
    if completion.is_ok() {
        agent::context::observe_progress(&mut state, &Role::Main, None, &config.limits).unwrap();
    }
    assert_eq!(
        json!(state.main_progress.blockers),
        before,
        "subset completion reset the pending recovery target's spent watch"
    );
    assert!(completion.is_err());
}

#[tokio::test]
async fn expanded_scope_completion_accepts_target_with_valid_repair_receipt() {
    let (journal, config, _) = blocked().await;
    let mut state = journal.load().await.unwrap().unwrap();
    assert_eq!(
        agent::repair_feedback_packet(&state, &[], &crate::tender_analysis::tests::config().limits)
            .unwrap()["repair"]["pending"],
        0
    );
    let delivery = agent::repair_recovery::delivered(&state, &messages(&state, &config)).unwrap();
    state.main_progress.seen.extend(delivery);
    agent::apply(
        &input(),
        &config,
        &mut state,
        "set_work_note",
        &work("source"),
    )
    .unwrap();
    agent::context::observe_progress(&mut state, &Role::Main, None, &config.limits).unwrap();
    let extended = independent_input(&mut state);
    let mut expanded = work("source");
    expanded["source_scope"] = json!(["source", "independent"]);
    agent::apply(&extended, &config, &mut state, "set_work_note", &expanded).unwrap();
    agent::context::observe_progress(&mut state, &Role::Main, None, &config.limits).unwrap();
    expanded["status"] = json!("complete");
    agent::apply(&extended, &config, &mut state, "set_work_note", &expanded).unwrap();
    agent::context::observe_progress(&mut state, &Role::Main, None, &config.limits).unwrap();
    assert!(state.main_progress.blockers.is_empty());
    assert_eq!(
        agent::repair_feedback_packet(&state, &[], &crate::tender_analysis::tests::config().limits)
            .unwrap()["repair"]["pending"],
        0
    );
}

#[tokio::test]
async fn direct_completion_cannot_consume_eligible_pending_recovery() {
    let (journal, config) = stale_blocked().await;
    let mut state = journal.load().await.unwrap().unwrap();
    let scope = vec!["source".to_string()];
    let delivery = agent::repair_recovery::delivered(&state, &messages(&state, &config)).unwrap();
    state.main_progress.seen.extend(delivery);
    assert_eq!(
        state.main_progress.blockers[0].watch.recovery,
        Recovery::Blocked
    );
    assert!(agent::repair_recovery::available(&state, &scope).unwrap());
    assert!(
        agent::repair_feedback_packet(&state, &[], &crate::tender_analysis::tests::config().limits)
            .unwrap()["repair"]["pending"]
            .as_u64()
            .unwrap()
            > 0
    );
    let before = digest(&state).unwrap();
    let mut complete = work("source");
    complete["status"] = json!("complete");
    assert!(agent::apply(&input(), &config, &mut state, "set_work_note", &complete).is_err());
    assert_eq!(
        digest(&state).unwrap(),
        before,
        "rejected completion must preserve eligibility, consumed markers, watches and analysis"
    );
    assert!(agent::repair_recovery::available(&state, &scope).unwrap());
    agent::apply(
        &input(),
        &config,
        &mut state,
        "set_work_note",
        &work("source"),
    )
    .unwrap();
    assert!(!agent::repair_recovery::available(&state, &scope).unwrap());
    assert_eq!(
        state.main_progress.watch.replans,
        config.limits.max_focus_replans
    );
    assert_ne!(
        state.main_progress.blockers[0].watch.recovery,
        Recovery::Blocked
    );
    assert_eq!(state.main_progress.blockers.len(), 1);
}

#[tokio::test]
async fn ordinary_scope_completion_keeps_existing_local_semantics_without_blocker() {
    let (journal, config, _, _, _) = super::repair_history::handled().await;
    let mut state = journal.load().await.unwrap().unwrap();
    stale_candidate(&mut state, &config);
    assert!(state.main_progress.blockers.is_empty());
    let pending =
        agent::repair_feedback_packet(&state, &[], &crate::tender_analysis::tests::config().limits)
            .unwrap()["repair"]["pending"]
            .clone();
    assert!(pending.as_u64().unwrap() > 0);
    let mut complete = work("source");
    complete["status"] = json!("complete");
    agent::apply(&input(), &config, &mut state, "set_work_note", &complete).unwrap();
    agent::context::observe_progress(&mut state, &Role::Main, None, &config.limits).unwrap();
    assert!(state.main_progress.blockers.is_empty());
    assert_eq!(
        agent::repair_feedback_packet(&state, &[], &crate::tender_analysis::tests::config().limits)
            .unwrap()["repair"]["pending"],
        pending
    );
}

#[tokio::test]
async fn wrong_partial_and_other_role_history_never_grants_recovery() {
    let (journal, config, sha) = blocked().await;
    let state = journal.load().await.unwrap().unwrap();
    let original = messages(&state, &config);
    for path in ["items", "repair_history"] {
        let mut partial = original.clone();
        let mut content: Value =
            serde_json::from_str(partial[1]["content"].as_str().unwrap()).unwrap();
        content["result"][path] = json!([]);
        partial[1]["content"] = json!(content.to_string());
        assert!(
            agent::repair_recovery::delivered(&state, &partial)
                .unwrap()
                .is_empty()
        );
    }
    let mut unrelated = state.clone();
    unrelated.main_progress.blockers[0].scope = vec!["unrelated".into()];
    assert!(
        !agent::repair_recovery::delivered(&unrelated, &original)
            .unwrap()
            .iter()
            .any(|token| token.contains(":eligible:"))
    );
    let mut reviewer = state.clone();
    reviewer.role = Role::Reviewer;
    assert!(
        agent::repair_recovery::delivered(&reviewer, &original)
            .unwrap()
            .is_empty()
    );
    let mut changed = state;
    changed
        .repair
        .results
        .get_mut(&sha)
        .unwrap()
        .summary
        .push_str(" Changed after request.");
    assert!(
        agent::repair_recovery::delivered(&changed, &original)
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn two_histories_in_one_scope_share_the_same_single_allowance() {
    let (journal, config, sha) = blocked().await;
    let mut state = journal.load().await.unwrap().unwrap();
    let initial = agent::repair_recovery::delivered(&state, &messages(&state, &config)).unwrap();
    let mut second = state.findings_for_repair()[0].clone();
    second.message.push_str(" Another original concern.");
    state
        .repair
        .results
        .insert(digest(&second).unwrap(), state.repair.results[&sha].clone());
    state.review_draft.insert("second".into(), second);
    let eligible = |tokens: std::collections::BTreeSet<String>| {
        tokens
            .into_iter()
            .filter(|token| token.contains(":eligible:"))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        eligible(agent::repair_recovery::delivered(&state, &messages(&state, &config)).unwrap()),
        eligible(initial)
    );
}

#[test]
#[ignore = "requires KB_HISTORY_RECOVERY_RUN and KB_HISTORY_RECOVERY_REPORT; offline recovery projection only, no model calls"]
fn archived_blocked_scopes_have_deliverable_history_with_one_bounded_retry() {
    let root = std::path::PathBuf::from(std::env::var("KB_HISTORY_RECOVERY_RUN").unwrap());
    let report = std::path::PathBuf::from(std::env::var("KB_HISTORY_RECOVERY_REPORT").unwrap());
    let original = std::fs::read(root.join("extraction/checkpoint.json")).unwrap();
    let state: Checkpoint = serde_json::from_slice(&original).unwrap();
    let frozen: FrozenInput =
        serde_json::from_slice(&std::fs::read(root.join("source/frozen-input.json")).unwrap())
            .unwrap();
    let config: Config =
        serde_json::from_slice(&std::fs::read(root.join("extraction/runtime.json")).unwrap())
            .unwrap();
    let current = Config::with_provider(config.provider.clone(), config.limits.clone()).unwrap();
    assert_eq!(json!(config), json!(current));
    let mut pages = Vec::new();
    let mut offset = 0;
    loop {
        let result = agent::inspect_review(
            &state,
            &json!({"offset":offset,"limit":1}),
            config.limits.max_tool_result_bytes,
        )
        .unwrap();
        let count = result["items"].as_array().unwrap().len();
        if count == 0 {
            break;
        }
        pages.extend([
            json!({"role":"assistant","tool_calls":[{"id":format!("page-{offset}"),"function":{"name":"inspect_review"}}]}),
            json!({"role":"tool","tool_call_id":format!("page-{offset}"),"content":json!({"ok":true,"result":result}).to_string()}),
        ]);
        offset += count;
        if offset >= state.findings_for_repair().len() {
            break;
        }
    }
    let receipts = agent::repair_recovery::delivered(&state, &pages).unwrap();
    let mut results = Vec::new();
    for blocker in &state.main_progress.blockers {
        let values: Vec<_> = agent::context::scope_references(&state.analysis, &blocker.scope)
            .iter()
            .map(|key| agent::context::reference(&state.analysis, key).unwrap())
            .collect();
        let raw = digest(&values).unwrap();
        assert_eq!(raw, blocker.dependencies_sha256);
        assert_eq!(
            agent::repair_recovery::dependencies(&state, &blocker.scope, raw.clone()).unwrap(),
            raw
        );
        let mut projected = state.clone();
        projected.main_progress.seen.extend(receipts.clone());
        assert!(agent::repair_recovery::available(&projected, &blocker.scope).unwrap());
        assert_eq!(
            agent::repair_recovery::dependencies(&projected, &blocker.scope, raw.clone()).unwrap(),
            raw
        );
        let mut next = work(&blocker.scope[0]);
        next["source_scope"] = json!(blocker.scope);
        let mut deferred = projected
            .main_work
            .as_ref()
            .map(|w| w.deferred_sources.clone())
            .unwrap_or_default();
        if let Some(work) = projected.main_work.as_ref() {
            deferred.extend(work.source_scope.clone());
        }
        deferred.retain(|id| !blocker.scope.contains(id));
        deferred.sort();
        deferred.dedup();
        next["deferred_sources"] = json!(deferred);
        agent::apply(&frozen, &config, &mut projected, "set_work_note", &next).unwrap();
        assert_eq!(projected.main_progress.watch.replans, blocker.watch.replans);
        let effective =
            agent::repair_recovery::dependencies(&projected, &blocker.scope, raw.clone()).unwrap();
        assert_ne!(effective, raw);
        let domain_before = digest(&state.analysis).unwrap();
        assert_eq!(digest(&projected.analysis).unwrap(), domain_before);
        assert_eq!(
            json!(projected.reviewer_progress),
            json!(state.reviewer_progress)
        );
        assert_eq!(json!(projected.journal), json!(state.journal));
        assert_eq!(
            (projected.turn, projected.tool_calls, projected.read_bytes),
            (state.turn, state.tool_calls, state.read_bytes)
        );
        results.push(json!({"scope":blocker.scope,"raw_dependencies_sha256":raw,"consumed_dependencies_sha256":effective,"replans":projected.main_progress.watch.replans,"full_history_deliverable":true}));
    }
    assert_eq!(
        std::fs::read(root.join("extraction/checkpoint.json")).unwrap(),
        original
    );
    assert!(!report.exists());
    std::fs::write(report, serde_json::to_vec_pretty(&json!({"scope":"Offline hypothetical full-history delivery and explicit handoff; no request sent, no original checkpoint edited, no semantic approval", "blocked_scopes":results, "stable_receipts":receipts.iter().filter(|token| token.contains(":eligible:")).count(),"original_turn":state.turn,"candidate_and_reviewer_state_unchanged":true,"journal_and_counters_unchanged":true})).unwrap()).unwrap();
}

#[tokio::test]
async fn history_already_delivered_before_blocking_cannot_become_a_recovery() {
    let (journal, config, _) = blocked().await;
    let mut state = journal.load().await.unwrap().unwrap();
    let blockers = std::mem::take(&mut state.main_progress.blockers);
    let delivered = agent::repair_recovery::delivered(&state, &messages(&state, &config)).unwrap();
    state.main_progress.seen.extend(delivered);
    // A same-source candidate outside the saved receipt changes the domain,
    // but the already delivered repair history is still the same information.
    let mut extra = state.analysis.records.values().next().unwrap().clone();
    extra.id = "unrelated-history-candidate".into();
    state.analysis.records.insert(extra.id.clone(), extra);
    state.main_progress.blockers = blockers;
    let values: Vec<_> = agent::context::scope_references(&state.analysis, &["source".into()])
        .iter()
        .map(|key| agent::context::reference(&state.analysis, key).unwrap())
        .collect();
    state.main_progress.blockers[0].dependencies_sha256 = digest(&values).unwrap();
    let delivered = agent::repair_recovery::delivered(&state, &messages(&state, &config)).unwrap();
    state.main_progress.seen.extend(delivered);
    assert!(!agent::repair_recovery::available(&state, &["source".into()]).unwrap());
    let diagnostics = agent::context::execution_packet(&state, 16_384).unwrap();
    assert!(
        diagnostics["blockers"]["items"][0]
            .get("history_recovery")
            .is_none()
    );
}

#[tokio::test]
async fn expanded_retry_failure_blocks_each_original_target_before_unrelated_change() {
    let (journal, config, _) = blocked().await;
    let mut state = journal.load().await.unwrap().unwrap();
    let delivered = agent::repair_recovery::delivered(&state, &messages(&state, &config)).unwrap();
    state.main_progress.seen.extend(delivered);
    agent::apply(
        &input(),
        &config,
        &mut state,
        "set_work_note",
        &work("source"),
    )
    .unwrap();
    let mut extended = input();
    let mut independent = extended.source_units[0].clone();
    independent.source_unit_revision_id = "independent".into();
    independent.ordinal = 1;
    extended.source_units.push(independent);
    let mut expanded = work("source");
    expanded["source_scope"] = json!(["source", "independent"]);
    agent::apply(&extended, &config, &mut state, "set_work_note", &expanded).unwrap();
    agent::context::observe_progress(&mut state, &Role::Main, None, &config.limits).unwrap();
    state.main_progress.watch.no_progress_turns = 5;
    agent::context::observe_progress(&mut state, &Role::Main, None, &config.limits).unwrap();
    assert_eq!(state.main_progress.watch.recovery, Recovery::Blocked);
    // Synthetic independent candidate changes only the expanded scope's domain.
    let mut independent_record = state.analysis.records.values().next().unwrap().clone();
    independent_record.id = "independent-record".into();
    for source in &mut independent_record.sources {
        source.source_id = "independent".into();
    }
    state
        .analysis
        .records
        .insert(independent_record.id.clone(), independent_record);
    let mut returning = work("source");
    returning["deferred_sources"] = json!(["independent"]);
    assert!(
        agent::apply(&extended, &config, &mut state, "set_work_note", &returning).is_err(),
        "an unrelated change must not renew the failed original source attempt"
    );
}
