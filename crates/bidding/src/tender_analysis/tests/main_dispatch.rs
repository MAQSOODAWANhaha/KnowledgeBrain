use super::*;
use crate::agent_runtime::progress::Recovery;
use agent::main_dispatch::{self, Active};

async fn initial(input: &FrozenInput, config: &Config) -> (Checkpoint, Value) {
    let journal = MemoryJournal::default();
    *journal.fail_boundary_ack.lock().unwrap() = Some(1);
    let unused = work_script(vec![]);
    let error = agent::run(input, config, &journal, &unused, &CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(error.code, "INTERNAL");
    assert!(unused.bodies.lock().unwrap().is_empty());
    let state = journal.load().await.unwrap().unwrap();
    let body = serde_json::from_slice(state.journal.body().unwrap()).unwrap();
    (state, body)
}

fn two_sources() -> FrozenInput {
    let mut input = input();
    input.source_units.push(Source {
        source_unit_revision_id: "other".into(),
        ordinal: 1,
        ..input.source_units[0].clone()
    });
    input
}
fn complete_source(state: &mut Checkpoint, input: &FrozenInput, id: &str) {
    let source = input
        .source_units
        .iter()
        .find(|s| s.source_unit_revision_id == id)
        .unwrap();
    tools::cover(
        state.analysis.coverage.text.entry(id.into()).or_default(),
        0,
        source.text.len(),
    );
    state.analysis.dispositions.insert(
        id.into(),
        Disposition {
            state: DispositionState::NonRequirement,
            reason: "Source background has no submission obligation".into(),
        },
    );
}
fn install(input: &FrozenInput, config: &Config, state: &mut Checkpoint, body: &Value) {
    agent::evidence_delivery::confirm(input, config, state, body).unwrap();
    main_dispatch::confirm(input, config, state, body).unwrap();
}

#[tokio::test]
async fn first_reserved_owner_is_pure_and_received_installs_it_without_preload() {
    let input = input();
    let config = config();
    let (mut state, mut body) = initial(&input, &config).await;
    assert!(state.dispatch.entries.is_empty());
    assert!(state.dispatch.active.is_none());
    let original = digest(&state).unwrap();
    let projected = main_dispatch::projection(&input, &config, &state)
        .unwrap()
        .unwrap();
    assert_eq!(digest(&state).unwrap(), original);
    let last = body["messages"].as_array_mut().unwrap().last_mut().unwrap();
    let mut packet: Value = serde_json::from_str(last["content"].as_str().unwrap()).unwrap();
    packet["preloaded_evidence"] = Value::Null;
    last["content"] = json!(packet.to_string());
    main_dispatch::confirm(&input, &config, &mut state, &body).unwrap();
    assert_eq!(state.dispatch.active, Some(projected.owner.clone()));
    assert_eq!(
        state.main_work.as_ref().unwrap().source_scope,
        vec!["source"]
    );
    assert!(state.analysis.coverage.text.is_empty());
    main_dispatch::after_batch(
        &input,
        &config,
        &mut state,
        Some(&projected.owner),
        false,
        false,
    )
    .unwrap();
    let Active::Ordinary(id) = projected.owner else {
        panic!("ordinary first source")
    };
    assert_eq!(state.dispatch.entries[&id].spent_batches, 1);
    assert_eq!(state.dispatch.last_committed_turn, Some(1));
    assert!(
        main_dispatch::after_batch(
            &input,
            &config,
            &mut state,
            Some(&Active::Ordinary(id)),
            false,
            false
        )
        .is_err()
    );
}

#[tokio::test]
async fn reserved_owner_cannot_be_substituted_by_model_scope_or_another_root() {
    let input = two_sources();
    let config = config();
    let (mut state, mut body) = initial(&input, &config).await;
    let last = body["messages"].as_array_mut().unwrap().last_mut().unwrap();
    let mut packet: Value = serde_json::from_str(last["content"].as_str().unwrap()).unwrap();
    packet["main_dispatch"]["owner"]["id"] = json!("another-root");
    last["content"] = json!(packet.to_string());
    let before = digest(&state).unwrap();
    assert!(main_dispatch::confirm(&input, &config, &mut state, &body).is_err());
    assert_eq!(digest(&state).unwrap(), before);
    let (_, body) = initial(&input, &config).await;
    install(&input, &config, &mut state, &body);
    assert!(main_dispatch::check_scope(&input, &state, &config.limits, &["other".into()]).is_err());
    assert!(
        main_dispatch::check_scope(
            &input,
            &state,
            &config.limits,
            &["source".into(), "other".into()]
        )
        .is_ok()
    );
}

#[tokio::test]
async fn blocked_source_advances_independent_source_without_refunding_old_owner() {
    let input = two_sources();
    let config = config();
    let (mut state, body) = initial(&input, &config).await;
    install(&input, &config, &mut state, &body);
    let old = state.dispatch.active.clone().unwrap();
    state.main_progress.watch.recovery = Recovery::Blocked;
    state.main_progress.watch.replans = config.limits.max_focus_replans;
    main_dispatch::after_batch(&input, &config, &mut state, Some(&old), false, false).unwrap();
    state.turn += 1;
    assert_eq!(
        state.main_work.as_ref().unwrap().source_scope,
        vec!["other"]
    );
    let Active::Ordinary(id) = old else { panic!() };
    let saved = json!(state.dispatch.entries[&id]);
    let next = state.dispatch.active.clone().unwrap();
    complete_source(&mut state, &input, "other");
    main_dispatch::after_batch(&input, &config, &mut state, Some(&next), false, false).unwrap();
    assert_eq!(json!(state.dispatch.entries[&id]), saved);
    assert!(!state.done);
    assert_eq!(state.role, Role::Reviewer);
    assert!(state.dispatch.active.is_none());
    main_dispatch::check_ready(&input, &config, &state).unwrap();
}

#[tokio::test]
async fn reviewer_reserve_forces_handoff_while_another_source_is_still_open() {
    let input = two_sources();
    let mut config = config();
    config.limits.max_turns = 12;
    config.limits.reviewer_reserve = 4;
    let (mut state, body) = initial(&input, &config).await;
    install(&input, &config, &mut state, &body);
    let owner = state.dispatch.active.clone().unwrap();
    complete_source(&mut state, &input, "source");
    state.turn = 8;
    main_dispatch::after_batch(&input, &config, &mut state, Some(&owner), false, false).unwrap();
    assert_eq!(state.role, Role::Reviewer);
    main_dispatch::check_ready(&input, &config, &state).unwrap();
}

#[tokio::test]
async fn packed_work_drops_members_that_make_the_combined_scope_blocked() {
    let input = two_sources();
    let mut config = config();
    config.limits.pack_max_units = 8;
    config.limits.pack_max_chars = 1200;
    let (mut state, body) = initial(&input, &config).await;
    install(&input, &config, &mut state, &body);
    assert!(!state.main_work.as_ref().unwrap().source_scope.is_empty());
    let owner = state.dispatch.active.clone().unwrap();
    let scope = vec!["source".to_string()];
    let values: Vec<_> =
        crate::tender_analysis::agent::context::scope_references(&state.analysis, &scope)
            .iter()
            .map(|key| {
                crate::tender_analysis::agent::context::reference(&state.analysis, key).unwrap()
            })
            .collect();
    state
        .main_progress
        .blockers
        .push(crate::agent_runtime::progress::ExecutionBlocker {
            scope,
            dependencies_sha256: digest(&values).unwrap(),
            watch: crate::agent_runtime::progress::ProgressWatch {
                recovery: Recovery::Blocked,
                replans: 2,
                ..Default::default()
            },
        });
    main_dispatch::after_batch(&input, &config, &mut state, Some(&owner), false, false).unwrap();
    state.dispatch.active = None;
    let next = main_dispatch::projection(&input, &config, &state)
        .unwrap()
        .unwrap();
    assert!(
        !next.source_scope.contains(&"source".to_string()),
        "blocked pack member must not stay in the assigned work"
    );
    state.dispatch.active = Some(owner);
    state.main_work = Some(main_dispatch::work(&input, &config.limits, &state, &next));
    main_dispatch::check_scope(&input, &state, &config.limits, &next.source_scope).unwrap();
}

#[tokio::test]
async fn packed_owner_survives_completing_the_first_member() {
    let input = two_sources();
    let mut config = config();
    config.limits.pack_max_units = 8;
    config.limits.pack_max_chars = 1200;
    let (mut state, body) = initial(&input, &config).await;
    install(&input, &config, &mut state, &body);
    let owner = state.dispatch.active.clone().unwrap();
    assert_eq!(
        state.main_work.as_ref().unwrap().source_scope,
        vec!["source".to_string(), "other".to_string()]
    );
    complete_source(&mut state, &input, "source");
    main_dispatch::after_batch(&input, &config, &mut state, Some(&owner), false, false).unwrap();
    assert_eq!(state.dispatch.active.as_ref(), Some(&owner));
    let Active::Ordinary(id) = &owner else {
        panic!("ordinary pack owner")
    };
    assert_eq!(state.dispatch.entries[id].spent_batches, 1);
    state.dispatch.active = None;
    let next = main_dispatch::projection(&input, &config, &state)
        .unwrap()
        .unwrap();
    assert_eq!(next.owner, owner);
    assert_eq!(next.source_scope, vec!["other".to_string()]);
}

#[tokio::test]
async fn extract4_shape_enters_review_when_last_source_is_blocked() {
    let input = two_sources();
    let config = config();
    let (mut state, body) = initial(&input, &config).await;
    install(&input, &config, &mut state, &body);
    let owner = state.dispatch.active.clone().unwrap();
    complete_source(&mut state, &input, "source");
    fixture_global_checks(&input, &config, &mut state);
    let scope = vec!["other".to_string()];
    let values: Vec<_> =
        crate::tender_analysis::agent::context::scope_references(&state.analysis, &scope)
            .iter()
            .map(|key| {
                crate::tender_analysis::agent::context::reference(&state.analysis, key).unwrap()
            })
            .collect();
    state
        .main_progress
        .blockers
        .push(crate::agent_runtime::progress::ExecutionBlocker {
            scope,
            dependencies_sha256: digest(&values).unwrap(),
            watch: crate::agent_runtime::progress::ProgressWatch {
                recovery: Recovery::Blocked,
                replans: 2,
                ..Default::default()
            },
        });
    state.pending_coverage = Some(state.reviewer_coverage.clone());
    main_dispatch::after_batch(&input, &config, &mut state, Some(&owner), false, false).unwrap();
    assert_eq!(state.role, Role::Reviewer);
    assert!(
        !state.analysis.dispositions.contains_key("other"),
        "blocked leftover must stay an omission"
    );
    main_dispatch::check_ready(&input, &config, &state).unwrap();
}

#[tokio::test]
async fn pending_reads_do_not_block_review_when_remainder_is_host_closed() {
    let input = two_sources();
    let config = config();
    let (mut state, body) = initial(&input, &config).await;
    install(&input, &config, &mut state, &body);
    let old = state.dispatch.active.clone().unwrap();
    state.main_progress.watch.recovery = Recovery::Blocked;
    state.main_progress.watch.replans = config.limits.max_focus_replans;
    main_dispatch::after_batch(&input, &config, &mut state, Some(&old), false, false).unwrap();
    state.turn += 1;
    let next = state.dispatch.active.clone().unwrap();
    complete_source(&mut state, &input, "other");
    state.pending_coverage = Some(state.analysis.coverage.clone());
    main_dispatch::after_batch(&input, &config, &mut state, Some(&next), false, false).unwrap();
    assert_eq!(state.role, Role::Reviewer);
    assert!(state.pending_coverage.is_none());
}

#[tokio::test]
async fn disposition_without_required_records_or_delivered_grid_cannot_complete_root() {
    let mut input = grid_citation_input();
    input.structured_forms[0]["definition"]["schema_version"] = json!(3);
    let config = config();
    let (mut state, body) = initial(&input, &config).await;
    install(&input, &config, &mut state, &body);
    let owner = state.dispatch.active.clone().unwrap();
    state.analysis.coverage.form_cells.clear();
    state.analysis.dispositions.insert(
        "source".into(),
        Disposition {
            state: DispositionState::Requirement,
            reason: "Requires prescribed form".into(),
        },
    );
    main_dispatch::after_batch(&input, &config, &mut state, Some(&owner), false, false).unwrap();
    assert_eq!(state.dispatch.active, Some(owner.clone()));
    let Active::Ordinary(id) = owner else {
        panic!()
    };
    assert!(state.dispatch.entries[&id].completed_dependencies.is_none());
}

#[tokio::test]
async fn completed_source_moves_to_distinct_global_root_and_keeps_source_charge() {
    let input = input();
    let config = config();
    let (mut state, body) = initial(&input, &config).await;
    install(&input, &config, &mut state, &body);
    let owner = state.dispatch.active.clone().unwrap();
    complete_source(&mut state, &input, "source");
    main_dispatch::after_batch(&input, &config, &mut state, Some(&owner), false, false).unwrap();
    assert_ne!(state.dispatch.active, Some(owner.clone()));
    let Active::Ordinary(id) = owner else {
        panic!()
    };
    assert_eq!(state.dispatch.entries[&id].spent_batches, 1);
    assert!(state.dispatch.entries[&id].completed_dependencies.is_some());
    let Active::Ordinary(global) = state.dispatch.active.as_ref().unwrap() else {
        panic!()
    };
    assert!(state.dispatch.entries[global].source_id.is_none());
    assert_eq!(state.role, Role::Main);
}

#[tokio::test]
async fn hard_owner_cap_survives_watch_reset_scope_rename_and_new_dependencies() {
    let input = input();
    let mut config = config();
    config.limits.max_focus_turns = 1;
    config.limits.max_focus_replans = 1;
    let (mut state, body) = initial(&input, &config).await;
    install(&input, &config, &mut state, &body);
    let owner = state.dispatch.active.clone().unwrap();
    for _ in 0..2 {
        state.main_progress.watch = Default::default();
        state.main_work.as_mut().unwrap().objective =
            "New description is not a new allowance".into();
        main_dispatch::after_batch(&input, &config, &mut state, Some(&owner), false, false)
            .unwrap();
        state.turn += 1;
    }
    let Active::Ordinary(id) = owner else {
        panic!()
    };
    assert_eq!(state.dispatch.entries[&id].spent_batches, 2);
    assert!(state.dispatch.active.is_none());
    state.analysis.records.insert(
        "new-fact".into(),
        Record {
            id: "new-fact".into(),
            sources: vec![span()],
            data: RecordData::Fact {
                name: "Changed dependency".into(),
                value: "New evidence".into(),
                scope: "Original source".into(),
            },
        },
    );
    main_dispatch::after_batch(&input, &config, &mut state, None, false, false).unwrap();
    assert!(state.dispatch.active.is_none());
    assert_eq!(state.dispatch.entries[&id].spent_batches, 2);
}

fn linked_facts(state: &mut Checkpoint, input: &FrozenInput) {
    for (id, source) in [("left", "source"), ("right", "other")] {
        state.analysis.records.insert(
            id.into(),
            Record {
                id: id.into(),
                sources: vec![Span {
                    source_id: source.into(),
                    start: 0,
                    end: input.source_units[0].text.len(),
                    grid_cell: None,
                    view_id: None,
                }],
                data: RecordData::Fact {
                    name: "Source fact".into(),
                    value: "Original value".into(),
                    scope: "Original clause".into(),
                },
            },
        );
    }
    state.analysis.relations.insert(
        "link".into(),
        Relation {
            id: "link".into(),
            from: "left".into(),
            to: "right".into(),
            from_target: RelationTarget::Record,
            to_target: RelationTarget::Record,
            from_record_sha256: digest(&state.analysis.records["left"]).unwrap(),
            to_record_sha256: digest(&state.analysis.records["right"]).unwrap(),
            kind: RelationKind::References,
            state: RelationState::Explicit,
            scope: "Cross-source reference".into(),
            explanation: "Frozen original establishes the reference".into(),
            grounds: vec![span()],
        },
    );
}

#[tokio::test]
async fn distinct_source_with_saved_dependency_on_blocked_root_is_not_independent() {
    let input = two_sources();
    let config = config();
    let (mut state, body) = initial(&input, &config).await;
    install(&input, &config, &mut state, &body);
    linked_facts(&mut state, &input);
    let old = state.dispatch.active.clone().unwrap();
    state.main_progress.watch.recovery = Recovery::Blocked;
    main_dispatch::after_batch(&input, &config, &mut state, Some(&old), false, false).unwrap();
    assert!(
        state.dispatch.active.is_none(),
        "another source ID cannot bypass an actual saved semantic dependency"
    );
    assert!(main_dispatch::check_ready(&input, &config, &state).is_err());
}

#[tokio::test]
async fn stale_relation_keeps_root_open_and_last_relation_fix_can_enter_review() {
    let input = two_sources();
    let config = config();
    let (mut state, body) = initial(&input, &config).await;
    install(&input, &config, &mut state, &body);
    linked_facts(&mut state, &input);
    complete_source(&mut state, &input, "source");
    complete_source(&mut state, &input, "other");
    state
        .analysis
        .relations
        .get_mut("link")
        .unwrap()
        .to_record_sha256 = "0".repeat(64);
    let owner = state.dispatch.active.clone().unwrap();
    main_dispatch::after_batch(&input, &config, &mut state, Some(&owner), false, false).unwrap();
    state.turn += 1;
    assert_eq!(state.dispatch.active, Some(owner.clone()));
    let Active::Ordinary(id) = &owner else {
        panic!()
    };
    assert!(state.dispatch.entries[id].completed_dependencies.is_none());
    state
        .analysis
        .relations
        .get_mut("link")
        .unwrap()
        .to_record_sha256 = digest(&state.analysis.records["right"]).unwrap();
    fixture_global_checks(&input, &config, &mut state);
    main_dispatch::after_batch(&input, &config, &mut state, Some(&owner), false, false).unwrap();
    assert_eq!(state.role, Role::Reviewer);
    assert!(state.dispatch.active.is_none());
    assert!(!state.done);
    assert_eq!(state.dispatch.entries[id].spent_batches, 2);
}

#[tokio::test]
async fn handoff_ack_loss_charges_reserved_source_once_and_never_the_new_global_owner() {
    for boundary in [1, 2, 3] {
        let input = input();
        let config = config();
        let journal = MemoryJournal::default();
        *journal.fail_boundary_ack.lock().unwrap() = Some(boundary);
        let model = work_script(vec![(
            "set_disposition",
            json!({
                "source_id":"source","state":"non_requirement","reason":"Read source background has no submission obligation"
            }),
        )]);
        let error = agent::run(&input, &config, &journal, &model, &CancellationToken::new())
            .await
            .unwrap_err();
        assert_eq!(error.code, "INTERNAL");
        let saved = journal.load().await.unwrap().unwrap();
        if boundary < 3 {
            assert!(
                saved.dispatch.entries.is_empty(),
                "prepared and received do not install or charge an owner"
            );
            *journal.interrupt_after.lock().unwrap() = Some(1);
            let error = agent::run(&input, &config, &journal, &model, &CancellationToken::new())
                .await
                .unwrap_err();
            assert_eq!(error.code, "INTERNAL");
        }
        let committed = journal.load().await.unwrap().unwrap();
        assert_eq!(model.bodies.lock().unwrap().len(), 1);
        assert_eq!(committed.turn, 1);
        assert_eq!(committed.dispatch.last_committed_turn, Some(1));
        let source = committed
            .dispatch
            .entries
            .values()
            .find(|e| e.source_id.as_deref() == Some("source"))
            .unwrap();
        assert_eq!(source.spent_batches, 1);
        assert!(source.completed_dependencies.is_some());
        let Active::Ordinary(global) = committed.dispatch.active.as_ref().unwrap() else {
            panic!()
        };
        assert!(committed.dispatch.entries[global].source_id.is_none());
        assert_eq!(committed.dispatch.entries[global].spent_batches, 0);
        assert!(committed.journal.pending.is_none());
        assert_eq!(journal.reservations.lock().unwrap().len(), 1);
    }
}
