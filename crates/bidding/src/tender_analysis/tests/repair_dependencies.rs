use super::*;

fn rule(state: &Checkpoint, id: &str, text: &str) -> Record {
    let mut record = state.analysis.records.values().next().unwrap().clone();
    record.id = id.into();
    record.data = serde_json::from_value(
        json!({"kind":"rule","text":text,"scope":"Synthetic rule scope",
        "applicability":requirement()["data"]["applicability"]}),
    )
    .unwrap();
    record
}

fn history(state: &Checkpoint, config: &Config, sha: &str) -> Value {
    agent::inspect_review(
        state,
        &json!({"offset":0,"limit":10}),
        config.limits.max_tool_result_bytes,
    )
    .unwrap()["repair_history"][sha]
        .clone()
}

fn local_version(state: &Checkpoint, key: &str) -> String {
    digest(&json!([
        "main-repair-local-v2",
        state.input_sha256,
        source_review::candidate_reference_values(state, key).unwrap()
    ]))
    .unwrap()
}

#[tokio::test]
async fn new_repairs_use_local_v2_and_ignore_unrelated_rules_while_review_still_invalidates() {
    let (journal, config, id, sha, _) = super::repair_history::handled().await;
    let mut state = journal.load().await.unwrap().unwrap();
    let key = format!("record:{id}");
    assert_eq!(
        state.repair.results[&sha].candidate_versions[&key],
        Some(local_version(&state, &key))
    );
    let review_before = source_review::candidate_version(&state, &key).unwrap();
    let receipt_before = json!(state.repair.results[&sha]);
    state.analysis.records.insert(
        "unrelated-rule".into(),
        rule(&state, "unrelated-rule", "An unrelated policy"),
    );
    assert_ne!(
        source_review::candidate_version(&state, &key).unwrap(),
        review_before
    );
    assert_eq!(history(&state, &config, &sha)["status"], "current");
    assert_eq!(
        agent::repair_feedback_packet(&state, &[], &crate::tender_analysis::tests::config().limits)
            .unwrap()["repair"]["pending"],
        0
    );
    assert_eq!(json!(state.repair.results[&sha]), receipt_before);
}

#[tokio::test]
async fn unchanged_legacy_is_valid_but_invalid_legacy_stays_stale_until_normal_model_write() {
    let (journal, config, id, sha, args) = super::repair_history::handled().await;
    let mut state = journal.load().await.unwrap().unwrap();
    let key = format!("record:{id}");
    let legacy = source_review::candidate_version(&state, &key).unwrap();
    state
        .repair
        .results
        .get_mut(&sha)
        .unwrap()
        .candidate_versions
        .insert(key.clone(), Some(legacy.clone()));
    let original = digest(&state).unwrap();
    assert_eq!(history(&state, &config, &sha)["status"], "current");
    assert_eq!(
        agent::repair_feedback_packet(&state, &[], &crate::tender_analysis::tests::config().limits)
            .unwrap()["repair"]["pending"],
        0
    );
    assert_eq!(digest(&state).unwrap(), original);
    state.analysis.records.insert(
        "unrelated-rule".into(),
        rule(
            &state,
            "unrelated-rule",
            "A newly recorded independent policy",
        ),
    );
    assert_eq!(history(&state, &config, &sha)["status"], "stale");
    assert_eq!(
        history(&state, &config, &sha)["reason"],
        "dependency_changed"
    );
    assert_eq!(
        state.repair.results[&sha].candidate_versions[&key],
        Some(legacy)
    );
    *journal.state.lock().unwrap() = Some(state);
    let saved =
        super::repair::repair_steps(&journal, &config, vec![("put_repair_result", args)]).await;
    assert_eq!(
        saved.repair.results[&sha].candidate_versions[&key],
        Some(local_version(&saved, &key))
    );
    assert_eq!(history(&saved, &config, &sha)["status"], "current");
}

fn edge(state: &Checkpoint, from: &str, to: &str) -> Relation {
    Relation {
        id: "fixture-edge".into(),
        from: from.into(),
        to: to.into(),
        from_target: RelationTarget::Record,
        to_target: RelationTarget::Record,
        from_record_sha256: digest(&state.analysis.records[from]).unwrap(),
        to_record_sha256: digest(&state.analysis.records[to]).unwrap(),
        kind: RelationKind::References,
        state: RelationState::Explicit,
        scope: "Synthetic local dependency".into(),
        explanation: "Explicit original relationship".into(),
        grounds: vec![span()],
    }
}

fn stamp_local(state: &mut Checkpoint, sha: &str, key: &str) {
    let version = local_version(state, key);
    state
        .repair
        .results
        .get_mut(sha)
        .unwrap()
        .candidate_versions
        .insert(key.into(), Some(version));
}

#[tokio::test]
async fn local_repairs_still_invalidate_explicit_rules_endpoints_new_deleted_edges_and_template_parents()
 {
    let (journal, config, id, sha, _) = super::repair_history::handled().await;
    let initial = journal.load().await.unwrap().unwrap();
    let key = format!("record:{id}");
    for case in [
        "explicit_rule",
        "endpoint",
        "new_edge",
        "deleted_edge",
        "template_parent",
    ] {
        let mut state = initial.clone();
        let mut target = state.analysis.records[&id].clone();
        target.id = "fixture-target".into();
        if case == "explicit_rule" {
            target = rule(&state, "fixture-target", "A directly linked policy");
        }
        if case == "template_parent" {
            let mut parent = target.clone();
            parent.id = "fixture-parent".into();
            state.analysis.records.insert(parent.id.clone(), parent);
            target.data = serde_json::from_value(json!({"kind":"template","label":"Synthetic form","title":"Synthetic format",
                "parent":"fixture-parent","order":null,"purpose":"Original response form","applicability":requirement()["data"]["applicability"],
                "regions":[{"source":span(),"role":"instruction","form_id":null,"cells":[],"instruction":"Keep original fields"}]})).unwrap();
        }
        state.analysis.records.insert(target.id.clone(), target);
        let relation = edge(&state, &id, "fixture-target");
        if case != "new_edge" {
            state
                .analysis
                .relations
                .insert(relation.id.clone(), relation.clone());
        }
        stamp_local(&mut state, &sha, &key);
        assert_eq!(
            history(&state, &config, &sha)["status"],
            "current",
            "{case}"
        );
        match case {
            "new_edge" => {
                state
                    .analysis
                    .relations
                    .insert(relation.id.clone(), relation);
            }
            "deleted_edge" => {
                state.analysis.relations.remove(&relation.id);
            }
            "template_parent" => {
                let mut parent =
                    serde_json::to_value(&state.analysis.records["fixture-parent"]).unwrap();
                parent["data"]["text"] = json!("The explicitly referenced parent changed");
                state.analysis.records.insert(
                    "fixture-parent".into(),
                    serde_json::from_value(parent).unwrap(),
                );
            }
            _ => {
                let mut target =
                    serde_json::to_value(&state.analysis.records["fixture-target"]).unwrap();
                target["data"]["text"] =
                    json!("The directly related original interpretation changed");
                state.analysis.records.insert(
                    "fixture-target".into(),
                    serde_json::from_value(target).unwrap(),
                );
            }
        }
        assert_eq!(history(&state, &config, &sha)["status"], "stale", "{case}");
        assert_eq!(
            history(&state, &config, &sha)["reason"],
            "dependency_changed",
            "{case}"
        );
    }
}

#[tokio::test]
async fn deletion_receipts_remain_none_and_restoration_invalidates_them() {
    let (journal, config, id, sha, _) = super::repair_history::handled().await;
    let mut state = journal.load().await.unwrap().unwrap();
    let key = format!("record:{id}");
    let deleted = state.analysis.records.remove(&id).unwrap();
    state
        .repair
        .results
        .get_mut(&sha)
        .unwrap()
        .candidate_versions
        .insert(key.clone(), None);
    assert_eq!(history(&state, &config, &sha)["status"], "current");
    state.analysis.records.insert(id, deleted);
    let value = history(&state, &config, &sha);
    assert_eq!(value["status"], "stale");
    assert_eq!(value["invalidated_refs"][0]["reason"], "restored");
    assert!(state.repair.results[&sha].candidate_versions[&key].is_none());
}

#[tokio::test]
async fn legacy_source_spend_cannot_be_reissued_as_a_fresh_repair_task() {
    use crate::agent_runtime::progress::{ExecutionBlocker, ProgressWatch, Recovery};
    let (journal, config, id, sha, args) = super::repair_history::handled().await;
    let mut state = journal.load().await.unwrap().unwrap();
    let key = format!("record:{id}");
    let legacy = source_review::candidate_version(&state, &key).unwrap();
    state
        .repair
        .results
        .get_mut(&sha)
        .unwrap()
        .candidate_versions
        .insert(key, Some(legacy));
    // Construct a pre-installation checkpoint: only legacy disposition novelty
    // has been seen, and this source exhausted its existing replan allowance.
    state.main_progress.seen.clear();
    state
        .main_progress
        .seen
        .insert(format!("repair_feedback:{sha}"));
    let scope = vec!["source".to_string()];
    let values: Vec<_> = agent::context::scope_references(&state.analysis, &scope)
        .iter()
        .map(|key| agent::context::reference(&state.analysis, key).unwrap())
        .collect();
    state.main_progress.blockers = vec![ExecutionBlocker {
        scope: scope.clone(),
        dependencies_sha256: digest(&values).unwrap(),
        watch: ProgressWatch {
            replans: 2,
            no_progress_turns: 6,
            focus_turns: 6,
            recovery: Recovery::Blocked,
        },
    }];
    state.main_progress.watch = ProgressWatch::default();
    state.main_work = None;
    let page = agent::inspect_review(
        &state,
        &json!({"offset":0,"limit":1}),
        config.limits.max_tool_result_bytes,
    )
    .unwrap();
    let messages = vec![
        json!({"role":"assistant","tool_calls":[{"id":"history","function":{"name":"inspect_review"}}]}),
        json!({"role":"tool","tool_call_id":"history","content":json!({"ok":true,"result":page}).to_string()}),
    ];
    let delivered = agent::repair_recovery::delivered(&state, &messages).unwrap();
    state.main_progress.seen.extend(delivered);
    agent::apply(&input(),&config,&mut state,"set_work_note",&json!({"source_scope":scope,"status":"active",
        "objective":"Reconcile the source finding","note":"Compare the existing repair and original"})).unwrap();
    agent::context::observe_progress(&mut state, &Role::Main, None, &config.limits).unwrap();
    state.main_progress.watch.focus_turns = 4;
    state.main_progress.watch.no_progress_turns = 2;
    // This is explicitly a pre-task-ledger archive specimen. Reading its old
    // three-field repair object must not manufacture fresh task allowance.
    let mut legacy_repair = json!(state.repair);
    legacy_repair.as_object_mut().unwrap().remove("tasks");
    state.repair = serde_json::from_value(legacy_repair).unwrap();
    let original_progress = json!(state.main_progress);
    let original_receipt = json!(state.repair.results);
    let original_graph = json!(state.analysis);
    agent::repair::tasks::sync(&mut state, &config.limits).unwrap();
    let tasks = agent::repair::tasks::current(&state, &config.limits).unwrap();
    assert_eq!(tasks.len(), 1);
    assert!(tasks[0].exhausted);
    assert!(state.repair.tasks.entries[&tasks[0].id].inherited_blocked);
    assert!(agent::repair::tasks::select(&mut state, &tasks[0].id, &config.limits).is_err());
    assert!(agent::apply(&input(), &config, &mut state, "put_repair_result", &args).is_err());
    assert!(agent::apply(&input(), &config, &mut state, "request_review", &json!({})).is_err());
    assert_eq!(json!(state.main_progress), original_progress);
    assert_eq!(json!(state.repair.results), original_receipt);
    assert_eq!(json!(state.analysis), original_graph);
    assert!(!agent::repair_recovery::available(&state, &scope).unwrap());
}

fn context_missing(state: &Checkpoint, key: &str) -> bool {
    agent::context::reference(&state.analysis, key).is_err()
        && state.repair.baseline.contains_key(key)
}

#[test]
#[ignore = "requires KB_REPAIR_LOCAL_BEFORE, KB_REPAIR_LOCAL_AFTER and KB_REPAIR_LOCAL_REPORT; hypothetical digest comparison only, no model calls"]
fn archived_single_rule_change_preserves_only_prospective_unrelated_local_repairs() {
    use std::path::PathBuf;
    let before_path = PathBuf::from(std::env::var("KB_REPAIR_LOCAL_BEFORE").unwrap());
    let after_path = PathBuf::from(std::env::var("KB_REPAIR_LOCAL_AFTER").unwrap());
    let report = PathBuf::from(std::env::var("KB_REPAIR_LOCAL_REPORT").unwrap());
    let original_before = std::fs::read(&before_path).unwrap();
    let original_after = std::fs::read(&after_path).unwrap();
    let before: Checkpoint = serde_json::from_slice(&original_before).unwrap();
    let after: Checkpoint = serde_json::from_slice(&original_after).unwrap();
    let changed: Vec<_> = after
        .analysis
        .records
        .iter()
        .filter(|(id, record)| {
            matches!(record.data, RecordData::Rule { .. })
                && json!(before.analysis.records.get(*id)) != json!(record)
        })
        .collect();
    assert_eq!(
        changed.len(),
        1,
        "this archived experiment isolates one actual Rule change"
    );
    let (rule_id, changed_rule) = changed[0];
    let mut projected = before.clone();
    projected
        .analysis
        .records
        .insert(rule_id.clone(), changed_rule.clone());
    let original_repair = json!(before.repair);
    let old_packet = agent::repair_feedback_packet(
        &before,
        &[],
        &crate::tender_analysis::tests::config().limits,
    )
    .unwrap()["repair"]
        .clone();
    let changed_legacy_packet = agent::repair_feedback_packet(
        &projected,
        &[],
        &crate::tender_analysis::tests::config().limits,
    )
    .unwrap()["repair"]
        .clone();
    assert_eq!(old_packet["handled"], 22);
    assert_eq!(
        changed_legacy_packet["handled"], 0,
        "old saved hashes must remain stale; installation does not upgrade them"
    );
    assert_eq!(json!(projected.repair), original_repair);
    let mut original_current = std::collections::BTreeSet::new();
    let mut prior_statuses = Vec::new();
    for (offset, finding) in before.findings_for_repair().into_iter().enumerate() {
        let id = digest(finding).unwrap();
        if !before.repair.results.contains_key(&id) {
            continue;
        }
        let detail = agent::inspect_review(
            &before,
            &json!({"offset":offset,"limit":1}),
            original_before.len(),
        )
        .unwrap();
        let status = &detail["repair_history"][&id];
        if status["status"] == "current" {
            original_current.insert(id.clone());
        }
        prior_statuses
            .push(json!({"finding_sha256":id,"status":status["status"],"reason":status["reason"]}));
    }
    assert_eq!(original_current.len(), 22);
    let mut hash_matched = 0;
    let mut prospective = before.clone();
    let mut unrelated = 0;
    let mut related = 0;
    let mut details = Vec::new();
    for (id, receipt) in &before.repair.results {
        let matches_original =
            receipt
                .candidate_versions
                .iter()
                .all(|(key, version)| match version {
                    Some(saved) => source_review::candidate_version(&before, key)
                        .is_ok_and(|current| &current == saved),
                    None => context_missing(&before, key),
                });
        if !matches_original {
            continue;
        }
        hash_matched += 1;
        if !original_current.contains(id) {
            continue;
        }
        let depends_on_rule = receipt
            .candidate_versions
            .iter()
            .filter(|(_, saved)| saved.is_some())
            .any(|(key, _)| {
                source_review::candidate_reference_values(&before, key)
                    .unwrap()
                    .contains_key(&format!("record:{rule_id}"))
            });
        let mut versions = BTreeMap::new();
        for (key, saved) in &receipt.candidate_versions {
            if saved.is_none() {
                versions.insert(key.clone(), None);
                continue;
            }
            let old_review = source_review::candidate_version(&before, key).unwrap();
            let new_review = source_review::candidate_version(&projected, key).unwrap();
            assert_ne!(
                old_review, new_review,
                "independent review retains global Rule sensitivity"
            );
            versions.insert(key.clone(), Some(local_version(&before, key)));
        }
        // This is a hypothetical fresh v2 receipt for comparison, never an
        // automatic migration or a claim that the model wrote this receipt.
        prospective
            .repair
            .results
            .get_mut(id)
            .unwrap()
            .candidate_versions = versions;
        let local_matches = prospective.repair.results[id]
            .candidate_versions
            .iter()
            .all(|(key, saved)| match saved {
                Some(saved) => saved == &local_version(&projected, key),
                None => context_missing(&projected, key),
            });
        assert_eq!(local_matches, !depends_on_rule);
        if depends_on_rule {
            related += 1;
        } else {
            unrelated += 1;
        }
        details.push(
            json!({"finding_sha256":id,"explicitly_depends_on_changed_rule":depends_on_rule,
            "legacy_invalidates":true,"prospective_local_stays_current":local_matches}),
        );
    }
    prospective
        .analysis
        .records
        .insert(rule_id.clone(), changed_rule.clone());
    let local_packet = agent::repair_feedback_packet(
        &prospective,
        &[],
        &crate::tender_analysis::tests::config().limits,
    )
    .unwrap()["repair"]
        .clone();
    assert_eq!(local_packet["handled"], unrelated);
    assert_eq!(hash_matched, 22);
    assert_eq!(unrelated + related, 22);
    assert_eq!(json!(projected.main_progress), json!(before.main_progress));
    assert_eq!(
        json!(projected.reviewer_progress),
        json!(before.reviewer_progress)
    );
    assert_eq!(json!(projected.journal), json!(before.journal));
    assert_eq!(std::fs::read(before_path).unwrap(), original_before);
    assert_eq!(std::fs::read(after_path).unwrap(), original_after);
    assert!(!report.exists());
    std::fs::write(report,serde_json::to_vec_pretty(&json!({"human_diagnostic_only":true,
        "scope":"Single actual Rule replacement in memory; hypothetical v2 receipts are comparison only, not migrated checkpoints, model repairs or independent approval",
        "before_turn":before.turn,"rule_from_turn":after.turn,"changed_rule_id":rule_id,
        "legacy_current_before":old_packet["handled"],"legacy_current_after":changed_legacy_packet["handled"],
        "prospective_local_unrelated_current":unrelated,"prospective_local_related_stale":related,
        "preexisting_stale_receipts_not_upgraded":before.repair.results.len()-unrelated-related,
        "old_dependency_hash_matched":hash_matched,"prior_disposition_statuses":prior_statuses,"receipt_results":details,"original_archives_unchanged":true,"no_model_calls":true})).unwrap()).unwrap();
}
