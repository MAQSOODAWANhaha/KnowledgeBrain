use super::*;
use crate::tender_analysis::agent::context::{self, WorkState, WorkStatus};

#[test]
fn unrelated_candidate_coverage_does_not_reset_reviewer_no_progress() {
    let (_input, mut config, mut state) = fixture();
    config.limits.max_no_progress_turns = 2;
    config.limits.max_focus_turns = 24;
    config.limits.max_focus_replans = 2;
    state.reviewer_work = Some(WorkState {
        source_scope: vec!["source".into()],
        deferred_sources: vec![],
        objective: "review assigned source".into(),
        focus: Default::default(),
        output_refs: vec!["disposition:source".into()],
        pending_refs: vec![],
        status: WorkStatus::Active,
        note: String::new(),
    });
    context::observe_progress(&mut state, &Role::Reviewer, None, &config.limits).unwrap();
    let before = state.reviewer_progress.watch.no_progress_turns;
    state
        .reviewer_coverage
        .candidate
        .insert("record:unrelated".into(), "b".repeat(64));
    context::observe_progress(&mut state, &Role::Reviewer, None, &config.limits).unwrap();
    assert_eq!(
        state.reviewer_progress.watch.no_progress_turns,
        before + 1,
        "inspecting an unassigned candidate must not count as packet progress"
    );
}

#[test]
fn assigned_source_coverage_counts_as_reviewer_progress() {
    let (_input, mut config, mut state) = fixture();
    config.limits.max_no_progress_turns = 2;
    state.reviewer_work = Some(WorkState {
        source_scope: vec!["source".into()],
        deferred_sources: vec![],
        objective: "review assigned source".into(),
        focus: Default::default(),
        output_refs: vec!["disposition:source".into()],
        pending_refs: vec![],
        status: WorkStatus::Active,
        note: String::new(),
    });
    context::observe_progress(&mut state, &Role::Reviewer, None, &config.limits).unwrap();
    state.reviewer_progress.watch.no_progress_turns = 1;
    state
        .reviewer_coverage
        .text
        .entry("source".into())
        .or_default()
        .push((0, 1));
    context::observe_progress(&mut state, &Role::Reviewer, None, &config.limits).unwrap();
    assert_eq!(
        state.reviewer_progress.watch.no_progress_turns, 0,
        "new assigned-source coverage is packet progress"
    );
}
