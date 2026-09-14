use super::*;
use crate::agent_runtime::progress::{ExecutionBlocker, ProgressWatch, Recovery};

pub(super) async fn fixture() -> (Checkpoint, String, String) {
    let (journal, _, _, _) = super::repair::repair_fixture(false).await;
    let mut state = journal.load().await.unwrap().unwrap();
    state.review = None;
    state.review_draft.clear();
    let make = |source: &str| {
        let mut source_span = span();
        source_span.source_id = source.into();
        serde_json::from_value::<Finding>(json!({"code":"fixture_issue", "message":format!("Resolve original issue from {source}"),
            "correction":"Compare the original evidence and saved candidate", "affected":[], "sources":[source_span]})).unwrap()
    };
    let a = make("source");
    let b = make("independent");
    let a_id = digest(&a).unwrap();
    let b_id = digest(&b).unwrap();
    state.review_draft.insert("a".into(), a);
    state.review_draft.insert("b".into(), b);
    state.repair.tasks = Default::default();
    agent::repair::tasks::sync(&mut state, &config().limits).unwrap();
    let values: Vec<_> = agent::context::scope_references(&state.analysis, &["source".into()])
        .iter()
        .map(|key| agent::context::reference(&state.analysis, key).unwrap())
        .collect();
    state.main_progress.blockers = vec![ExecutionBlocker {
        scope: vec!["source".into()],
        dependencies_sha256: digest(&values).unwrap(),
        watch: ProgressWatch {
            recovery: Recovery::Blocked,
            replans: 2,
            ..Default::default()
        },
    }];
    state.main_work = Some(serde_json::from_value(json!({"source_scope":["independent"],"objective":"Resolve this source issue", "status":"active","note":"Compare this source"})).unwrap());
    agent::repair::tasks::select(&mut state, "b", &config().limits).unwrap();
    (state, a_id, b_id)
}

fn packet(state: &Checkpoint) -> Value {
    agent::repair_feedback_packet(state, &[], &crate::tender_analysis::tests::config().limits)
        .unwrap()["repair"]
        .clone()
}

#[tokio::test]
async fn repair_navigation_uses_assigned_task_before_first_blocked_finding() {
    let (state, a, b) = fixture().await;
    let before = digest(&state).unwrap();
    let value = packet(&state);
    assert_eq!(value["next_finding"]["finding_sha256"], b);
    assert_eq!(value["next_finding"]["inspect_review"]["offset"], 1);
    assert_eq!(
        value["next_finding"]["source_scope"],
        json!(["independent"])
    );
    assert_eq!(value["blocked_pending"], 1);
    assert_eq!(value["next_blocked_finding"]["finding_sha256"], a);
    assert_eq!(value["pending"], 2);
    assert_eq!(value["handled"], 0);
    assert_eq!(digest(&state).unwrap(), before);
}

#[tokio::test]
async fn all_blocked_findings_stay_pending_and_never_appear_completed() {
    let (mut state, a, _) = fixture().await;
    state.review_draft.remove("b");
    let value = packet(&state);
    assert!(value["next_finding"].is_null());
    assert_eq!(value["blocked_pending"], 1);
    assert_eq!(value["pending"], 1);
    assert_eq!(value["handled"], 0);
    assert_eq!(value["next_blocked_finding"]["finding_sha256"], a);
}

#[tokio::test]
async fn assigned_task_does_not_reopen_legacy_scope_after_dependency_change() {
    let (mut state, _, b) = fixture().await;
    // Fixed-task execution does not grant a new attempt for a legacy blocker.
    state.main_progress.blockers[0].dependencies_sha256 = "prior-business-version".into();
    let value = packet(&state);
    assert_eq!(value["next_finding"]["finding_sha256"], b);
    assert_eq!(value["blocked_pending"], 1);
    assert_eq!(value["pending"], 2);
    assert!(value["next_finding"].get("scope_handoff").is_none());
}

#[tokio::test]
async fn independent_finding_navigation_requires_explicit_scope_handoff() {
    let (mut state, _, b) = fixture().await;
    state.main_work.as_mut().unwrap().source_scope = vec!["source".into()];
    let before = digest(&state).unwrap();
    let value = packet(&state);
    assert_eq!(value["next_finding"]["finding_sha256"], b);
    assert_eq!(
        value["next_finding"]["scope_handoff"]["tool"],
        "set_work_note"
    );
    assert_eq!(
        value["next_finding"]["scope_handoff"]["source_scope"],
        json!(["independent"])
    );
    assert_eq!(digest(&state).unwrap(), before);
}

#[tokio::test]
async fn aggregated_candidate_provenance_does_not_override_explicit_finding_scope() {
    let (mut state, _, b) = fixture().await;
    let affected = state.analysis.records.keys().next().unwrap().clone();
    let mut cited = span();
    cited.source_id = "independent".into();
    state
        .analysis
        .records
        .get_mut(&affected)
        .unwrap()
        .sources
        .push(cited);
    state.review_draft.get_mut("b").unwrap().affected =
        serde_json::from_value(json!([{"id":affected,"path":"/data/text"}])).unwrap();
    // Keep the A blocker at the current dependency version after fixture setup.
    let values: Vec<_> = agent::context::scope_references(&state.analysis, &["source".into()])
        .iter()
        .map(|key| agent::context::reference(&state.analysis, key).unwrap())
        .collect();
    state.main_progress.blockers[0].dependencies_sha256 = digest(&values).unwrap();
    agent::repair::tasks::sync(&mut state, &config().limits).unwrap();
    agent::repair::tasks::select(&mut state, "b", &config().limits).unwrap();
    let value = packet(&state);
    assert_eq!(
        value["next_finding"]["finding_sha256"],
        digest(state.review_draft.get("b").unwrap()).unwrap()
    );
    assert_ne!(value["next_finding"]["finding_sha256"], b);
    assert_eq!(
        value["next_finding"]["source_scope"],
        json!(["independent"])
    );
    assert_eq!(value["blocked_pending"], 1);
    assert_eq!(value["pending"], 2);
}

#[tokio::test]
async fn explicitly_multi_source_finding_keeps_its_blocked_source_pending() {
    let (mut state, _, _) = fixture().await;
    state
        .review_draft
        .get_mut("b")
        .unwrap()
        .sources
        .push(span());
    let value = packet(&state);
    assert!(value["next_finding"].is_null());
    assert_eq!(value["blocked_pending"], 2);
    assert_eq!(value["pending"], 2);
    assert_eq!(value["handled"], 0);
}

#[test]
#[ignore = "requires KB_REPAIR_NAVIGATION_CHECKPOINT and KB_REPAIR_NAVIGATION_REPORT; offline packet projection only"]
fn archived_navigation_does_not_send_active_work_back_into_exhausted_scope() {
    let path = std::path::PathBuf::from(std::env::var("KB_REPAIR_NAVIGATION_CHECKPOINT").unwrap());
    let report = std::path::PathBuf::from(std::env::var("KB_REPAIR_NAVIGATION_REPORT").unwrap());
    let original = std::fs::read(&path).unwrap();
    let state: Checkpoint = serde_json::from_slice(&original).unwrap();
    let before = digest(&state).unwrap();
    let value = packet(&state);
    assert!(value["pending"].as_u64().unwrap() > 0);
    assert!(value["blocked_pending"].as_u64().unwrap() > 0);
    if !value["next_finding"].is_null() {
        let scope: Vec<String> =
            serde_json::from_value(value["next_finding"]["source_scope"].clone()).unwrap();
        agent::context::check_blocked_scope(&state, &scope).unwrap();
        if value["next_finding"].get("scope_handoff").is_none() {
            assert!(scope.iter().all(|source| {
                state
                    .main_work
                    .as_ref()
                    .unwrap()
                    .source_scope
                    .contains(source)
            }));
        }
    } else {
        assert_eq!(value["blocked_pending"], value["pending"]);
    }
    assert_eq!(digest(&state).unwrap(), before);
    assert_eq!(std::fs::read(path).unwrap(), original);
    assert!(!report.exists());
    std::fs::write(report, serde_json::to_vec_pretty(&json!({"human_diagnostic_only":true,
        "scope":"Read-only navigation projection, no model calls, no state mutations or semantic acceptance",
        "turn":state.turn,"active_scope":state.main_work.as_ref().map(|work| &work.source_scope),"repair_packet":value,
        "state_unchanged":true})).unwrap()).unwrap();
}

#[tokio::test]
async fn deleted_affected_candidate_uses_saved_original_sources_for_navigation() {
    let (mut state, _, _) = fixture().await;
    let finding = state.review_draft.get_mut("b").unwrap();
    finding.sources.clear();
    finding.affected =
        serde_json::from_value(json!([{"id":"deleted-candidate","path":"/data/text"}])).unwrap();
    let id = digest(finding).unwrap();
    let mut original = span();
    original.source_id = "independent".into();
    state.repair.results.insert(
        id.clone(),
        agent::repair::Receipt {
            conclusion: agent::repair::Conclusion::Disputed,
            summary: "The original evidence explains the deleted candidate".into(),
            sources: vec![original],
            candidate_versions: BTreeMap::from([("record:deleted-candidate".into(), None)]),
        },
    );
    agent::repair::tasks::sync(&mut state, &config().limits).unwrap();
    agent::repair::tasks::select(&mut state, "b", &config().limits).unwrap();
    let value = packet(&state);
    assert_eq!(value["next_finding"]["finding_sha256"], id);
    assert_eq!(
        value["next_finding"]["source_scope"],
        json!(["independent"])
    );
    assert_eq!(value["blocked_pending"], 1);
    assert_eq!(value["pending"], 2);
}
