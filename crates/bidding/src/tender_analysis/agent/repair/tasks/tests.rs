use super::*;

fn seed(id: &str, sha: &str) -> Seed {
    Seed {
        id: id.into(),
        sha: sha.into(),
        inherited: None,
    }
}

fn watch(replans: usize, recovery: Recovery) -> ProgressWatch {
    ProgressWatch {
        focus_turns: 1,
        no_progress_turns: 1,
        replans,
        recovery,
    }
}

#[test]
fn identical_findings_alias_one_budget_and_rewording_or_restore_cannot_refund_it() {
    let mut state = State::default();
    state
        .synchronize("first".into(), &[seed("a", "same"), seed("b", "same")])
        .unwrap();
    assert_eq!(state.aliases["a"], state.aliases["b"]);
    let id = state.aliases["a"].clone();
    state.enter(&id, "version-one", 3).unwrap();
    state.commit(&id, 1, watch(1, Recovery::Replan), 3).unwrap();
    state.commit(&id, 1, ProgressWatch::default(), 3).unwrap();
    assert_eq!(state.entries[&id].committed_turns, 1);
    assert_eq!(state.entries[&id].watch.replans, 1);
    let frozen = json!(state);
    state
        .synchronize("first".into(), &[seed("a", "changed in Main")])
        .unwrap();
    assert_eq!(json!(state), frozen, "Main phase identity is frozen");
    let mut state: State = serde_json::from_value(frozen).unwrap();
    state
        .synchronize(
            "second".into(),
            &[seed("a", "rewritten"), seed("b", "rewritten")],
        )
        .unwrap();
    assert_eq!(state.aliases["a"], id);
    assert_eq!(state.entries[&id].committed_turns, 1);
    assert_eq!(state.entries[&id].watch.replans, 1);
    assert!(
        state.entries[&id]
            .attempted_dependencies
            .contains("version-one")
    );
}

#[test]
fn replacing_draft_identity_with_the_same_claim_is_not_a_fresh_task() {
    let mut state = State::default();
    state
        .synchronize("first".into(), &[seed("a", "claim")])
        .unwrap();
    state.enter("a", "one", 3).unwrap();
    state
        .commit("a", 1, watch(1, Recovery::Blocked), 3)
        .unwrap();
    state
        .synchronize("second".into(), &[seed("replacement", "claim")])
        .unwrap();
    assert_eq!(state.aliases["replacement"], "a");
    assert_eq!(state.entries["a"].committed_turns, 1);
    assert!(state.enter("a", "one", 3).is_err());
}

#[test]
fn switching_tasks_preserves_each_watch_and_total_cap_bounds_new_versions() {
    let mut state = State::default();
    state
        .synchronize("first".into(), &[seed("a", "one"), seed("b", "two")])
        .unwrap();
    state.enter("a", "v1", 3).unwrap();
    state
        .commit("a", 1, watch(2, Recovery::Blocked), 3)
        .unwrap();
    let spent = json!(state.entries["a"]);
    state.enter("b", "b1", 3).unwrap();
    state
        .commit("b", 2, watch(0, Recovery::Running), 3)
        .unwrap();
    assert_eq!(json!(state.entries["a"]), spent);
    assert!(state.enter("a", "v1", 3).is_err());
    state.enter("a", "v2", 3).unwrap();
    assert_eq!(state.entries["a"].watch.replans, 2);
    state
        .commit("a", 3, watch(2, Recovery::Blocked), 3)
        .unwrap();
    assert!(state.enter("a", "v2", 3).is_err());
    state.enter("a", "v3", 3).unwrap();
    state
        .commit("a", 4, watch(2, Recovery::Running), 3)
        .unwrap();
    assert!(
        state.enter("a", "v4", 3).is_err(),
        "new dependencies cannot extend the absolute task cap"
    );
    assert_eq!(state.entries["a"].watch.recovery, Recovery::Blocked);
    assert_eq!(state.entries["b"].committed_turns, 1);
    assert!(
        state
            .commit("b", 3, watch(0, Recovery::Running), 3)
            .is_err()
    );
}

#[test]
fn old_source_spend_is_conservatively_blocked_and_never_reopened_by_a_new_version() {
    let mut state = State::default();
    let mut inherited = seed("a", "claim");
    inherited.inherited = Some(watch(2, Recovery::Blocked));
    state.synchronize("first".into(), &[inherited]).unwrap();
    assert!(state.entries["a"].inherited_blocked);
    assert!(state.enter("a", "unseen-version", 100).is_err());
    state
        .synchronize("second".into(), &[seed("a", "changed-title")])
        .unwrap();
    assert!(state.enter("a", "another-version", 100).is_err());
    assert_eq!(state.entries["a"].watch.replans, 2);
}

#[test]
fn merging_and_splitting_aliases_preserve_all_prior_spend_without_repeated_sums() {
    let mut state = State::default();
    state
        .synchronize("first".into(), &[seed("a", "one"), seed("b", "two")])
        .unwrap();
    state.enter("a", "a1", 10).unwrap();
    state
        .commit("a", 1, watch(1, Recovery::Replan), 10)
        .unwrap();
    state.enter("b", "b1", 10).unwrap();
    state
        .commit("b", 2, watch(2, Recovery::Blocked), 10)
        .unwrap();
    let retired = json!(state.entries["b"]);
    state
        .synchronize("merged".into(), &[seed("a", "same"), seed("b", "same")])
        .unwrap();
    assert_eq!(state.aliases["a"], "a");
    assert_eq!(state.aliases["b"], "a");
    assert_eq!(state.entries["a"].committed_turns, 2);
    assert_eq!(
        json!(state.entries["b"]),
        retired,
        "historical entry remains recorded"
    );
    state
        .synchronize(
            "merged-again".into(),
            &[seed("a", "same"), seed("b", "same")],
        )
        .unwrap();
    assert_eq!(state.entries["a"].committed_turns, 2);
    state
        .synchronize("split".into(), &[seed("a", "different"), seed("b", "same")])
        .unwrap();
    assert_ne!(state.aliases["a"], state.aliases["b"]);
    for id in ["a", "b"] {
        assert_eq!(state.entries[id].committed_turns, 2);
        assert_eq!(state.entries[id].watch.replans, 2);
        assert!(state.entries[id].attempted_dependencies.contains("a1"));
        assert!(state.entries[id].attempted_dependencies.contains("b1"));
    }
}

#[test]
fn rejected_refunds_and_legacy_deserialization_do_not_create_fresh_allowance() {
    let old: super::super::State =
        serde_json::from_value(json!({"feedback_sha256":null,"baseline":{},"results":{}})).unwrap();
    assert_eq!(
        json!(old.tasks),
        json!({"active":null,"aliases":{},"entries":{},"feedback_sha256":null,"last_committed_turn":null})
    );
    let mut state = State::default();
    state
        .synchronize("first".into(), &[seed("a", "claim")])
        .unwrap();
    state.enter("a", "one", 3).unwrap();
    state.commit("a", 1, watch(2, Recovery::Replan), 3).unwrap();
    let prior = json!(state);
    assert!(state.commit("a", 2, ProgressWatch::default(), 3).is_err());
    assert_eq!(json!(state), prior);
}

#[test]
fn task_limit_is_derived_checked_and_bounded_by_global_turns() {
    let mut limits = crate::tender_analysis::tests::config().limits;
    limits.max_turns = 100;
    limits.max_focus_turns = 7;
    limits.max_focus_replans = 2;
    assert_eq!(limit(&limits).unwrap(), 21);
    limits.max_turns = 5;
    assert_eq!(limit(&limits).unwrap(), 5);
    limits.max_focus_replans = usize::MAX;
    assert!(limit(&limits).is_err());
    limits.max_focus_replans = 1;
    limits.max_focus_turns = usize::MAX;
    assert!(limit(&limits).is_err());
}
