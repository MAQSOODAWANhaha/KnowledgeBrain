use super::repair::{repair_fixture, repair_steps};
use super::*;

pub(super) async fn handled() -> (MemoryJournal, Config, String, String, Value) {
    let (journal, config, id, sha) = repair_fixture(true).await;
    let mut edit = requirement();
    edit["id"] = json!(id);
    edit["data"]["text"] = json!("按须知提交规定格式");
    let args = json!({"finding_sha256":sha,"conclusion":"revised",
        "summary":"The original requirement wording has been restored.","sources":[span()],
        "candidate_refs":[format!("record:{id}")]});
    let state = repair_steps(
        &journal,
        &config,
        vec![
            ("inspect_review", json!({"offset":0,"limit":1})),
            ("put_record", edit),
            (
                "inspect_analysis",
                json!({"kind":"record","view":"detail","ids":[id],"offset":0,"limit":1}),
            ),
            ("put_repair_result", args.clone()),
        ],
    )
    .await;
    assert!(state.repair.results.contains_key(&sha));
    (journal, config, id, sha, args)
}

fn page(state: &Checkpoint, budget: usize) -> Value {
    agent::inspect_review(state, &json!({"offset":0,"limit":1}), budget).unwrap()
}

fn incident_change(state: &mut Checkpoint, id: &str) {
    let subject = state.analysis.records[id].clone();
    let mut target = subject.clone();
    target.id = "fixture-target".into();
    let edge = Relation {
        id: "fixture-link".into(),
        from: id.into(),
        to: target.id.clone(),
        from_target: RelationTarget::Record,
        to_target: RelationTarget::Record,
        from_record_sha256: digest(&subject).unwrap(),
        to_record_sha256: digest(&target).unwrap(),
        kind: RelationKind::References,
        state: RelationState::Explicit,
        scope: "Synthetic source reference".into(),
        explanation: "A later source-backed reference changes incident dependencies.".into(),
        grounds: vec![span()],
    };
    state.analysis.records.insert(target.id.clone(), target);
    state.analysis.relations.insert(edge.id.clone(), edge);
}

#[tokio::test]
async fn main_history_survives_restore_without_replacing_finding_or_granting_reads() {
    let (journal, config, id, sha, _) = handled().await;
    let mut state = journal.load().await.unwrap().unwrap();
    state.transcript.clear();
    state.analysis.coverage.candidate.clear();
    let restored: Checkpoint = serde_json::from_value(json!(state)).unwrap();
    let before = digest(&restored).unwrap();
    let value = page(&restored, config.limits.max_tool_result_bytes);
    assert_eq!(value["items"], json!(restored.findings_for_repair()));
    let history = &value["repair_history"][&sha];
    assert_eq!(history["status"], "current");
    assert_eq!(
        history["previous"]["summary"],
        restored.repair.results[&sha].summary
    );
    assert_eq!(history["previous"]["sources"], json!([span()]));
    assert_eq!(
        history["previous"]["candidate_refs"],
        json!([format!("record:{id}")])
    );
    assert_eq!(history["current_candidate_read_receipt"], false);
    assert_eq!(history["independent_approval"], false);
    assert_eq!(digest(&restored).unwrap(), before);

    let mut unread = restored.clone();
    unread.main_progress.seen.clear();
    let original = agent::delivered_repair_feedback(&unread, &[
        json!({"role":"assistant","tool_calls":[{"id":"feedback","function":{"name":"inspect_review"}}]}),
        json!({"role":"tool","tool_call_id":"feedback","content":json!({"ok":true,"result":value}).to_string()}),
    ]).unwrap();
    assert_eq!(
        original,
        std::collections::BTreeSet::from([format!("repair_feedback:{sha}")])
    );
    assert!(unread.main_progress.seen.is_empty());
    let mut history_only = value;
    history_only["items"] = json!([]);
    assert!(agent::delivered_repair_feedback(&unread, &[
        json!({"role":"assistant","tool_calls":[{"id":"feedback","function":{"name":"inspect_review"}}]}),
        json!({"role":"tool","tool_call_id":"feedback","content":json!({"ok":true,"result":history_only}).to_string()}),
    ]).unwrap().is_empty());
}

#[tokio::test]
async fn stale_dependency_navigation_does_not_claim_the_unchanged_record_was_edited() {
    let (journal, config, id, sha, _) = handled().await;
    let mut state = journal.load().await.unwrap().unwrap();
    let original = digest(&state.analysis.records[&id]).unwrap();
    incident_change(&mut state, &id);
    assert_eq!(digest(&state.analysis.records[&id]).unwrap(), original);
    agent::repair_task_host::schedule(&mut state, &config.limits).unwrap();
    let before = digest(&state).unwrap();
    let value = page(&state, config.limits.max_tool_result_bytes);
    let history = &value["repair_history"][&sha];
    assert_eq!(history["status"], "stale");
    assert_eq!(history["reason"], "dependency_changed");
    assert_eq!(
        history["invalidated_refs"][0]["reference"],
        format!("record:{id}")
    );
    assert_eq!(
        history["invalidated_refs"][0]["reason"],
        "dependency_changed"
    );
    assert_eq!(
        history["invalidated_refs"][0]["inspect_analysis"],
        json!({"kind":"record","view":"detail","ids":[id],"offset":0,"limit":1})
    );
    let packet =
        agent::repair_feedback_packet(&state, &[], &crate::tender_analysis::tests::config().limits)
            .unwrap();
    assert_eq!(packet["repair"]["next_finding"]["status"], "stale");
    assert_eq!(packet["repair"]["pending"], 1);
    assert_eq!(digest(&state).unwrap(), before);

    state.analysis.records.remove(&id);
    let deleted = page(&state, config.limits.max_tool_result_bytes);
    assert_eq!(
        deleted["repair_history"][&sha]["invalidated_refs"][0]["reason"],
        "deleted"
    );
    assert!(deleted["repair_history"][&sha]["invalidated_refs"][0]["inspect_analysis"].is_null());
    state.repair.feedback_sha256 = Some("different-generation".into());
    let older = page(&state, config.limits.max_tool_result_bytes);
    assert_eq!(older["repair_history"][&sha]["status"], "stale");
    assert_eq!(
        older["repair_history"][&sha]["reason"],
        "feedback_generation_changed"
    );
    assert!(!older["repair_history"][&sha]["previous"].is_null());
    state.repair.results.clear();
    let never = page(&state, config.limits.max_tool_result_bytes);
    assert_eq!(never["repair_history"][&sha]["status"], "never_handled");
    assert!(never["repair_history"][&sha]["previous"].is_null());
}

#[tokio::test]
async fn stale_repair_can_be_registered_after_current_reads_without_another_mutation() {
    let (journal, config, id, sha, args) = handled().await;
    let mut state = journal.load().await.unwrap().unwrap();
    incident_change(&mut state, &id);
    agent::main_dispatch::after_batch(&input(), &config, &mut state, None, false, false).unwrap();
    state.analysis.coverage.candidate.clear();
    let analysis = digest(&state.analysis.records).unwrap();
    let relations = digest(&state.analysis.relations).unwrap();
    *journal.state.lock().unwrap() = Some(state);
    let rejected = repair_steps(
        &journal,
        &config,
        vec![
            ("inspect_review", json!({"offset":0,"limit":1})),
            ("put_repair_result", args.clone()),
        ],
    )
    .await;
    assert_eq!(
        page(&rejected, config.limits.max_tool_result_bytes)["repair_history"][&sha]["status"],
        "stale"
    );
    let accepted = repair_steps(
        &journal,
        &config,
        vec![
            (
                "inspect_analysis",
                json!({"kind":"record","view":"detail","ids":[id],"offset":0,"limit":1}),
            ),
            ("put_repair_result", args),
        ],
    )
    .await;
    assert_eq!(
        page(&accepted, config.limits.max_tool_result_bytes)["repair_history"][&sha]["status"],
        "current"
    );
    assert_eq!(digest(&accepted.analysis.records).unwrap(), analysis);
    assert_eq!(digest(&accepted.analysis.relations).unwrap(), relations);
    assert!(!accepted.done);
    assert!(!accepted.review_draft.is_empty());
}

#[tokio::test]
async fn complete_history_and_original_share_the_page_budget_and_failed_writes_are_atomic() {
    let (journal, config, _, sha, mut args) = handled().await;
    let mut state = journal.load().await.unwrap().unwrap();
    // Reconstruct the still-active task inside a batch, after its first
    // valid receipt and before the boundary dispatcher clears the assignment.
    state.repair.tasks.active = state
        .repair
        .tasks
        .entries
        .iter()
        .find(|(_, entry)| entry.finding_sha256 == sha)
        .map(|(id, _)| id.clone());
    let full = page(&state, config.limits.max_tool_result_bytes);
    let bytes = serde_json::to_vec(&full).unwrap().len();
    assert_eq!(page(&state, bytes), full);
    let before = digest(&state).unwrap();
    assert!(agent::inspect_review(&state, &json!({"offset":0,"limit":1}), bytes - 1).is_err());
    assert_eq!(digest(&state).unwrap(), before);
    let mut small = config.clone();
    small.limits.max_tool_result_bytes = bytes;
    args["summary"] = json!("x".repeat(bytes / 2));
    assert!(serde_json::to_vec(&args).unwrap().len() <= bytes);
    let failure =
        agent::apply(&input(), &small, &mut state, "put_repair_result", &args).unwrap_err();
    assert!(failure.contains("main history budget"), "{failure}");
    assert_eq!(digest(&state).unwrap(), before);
    assert_eq!(
        page(&state, bytes)["repair_history"][&sha],
        full["repair_history"][&sha]
    );
}

#[test]
#[ignore = "requires KB_REPAIR_HISTORY_CHECKPOINT, KB_REPAIR_HISTORY_RUNTIME and KB_REPAIR_HISTORY_REPORT; offline model-state inspection only"]
fn archived_repair_history_is_retrievable_under_the_unchanged_frozen_config() {
    use std::path::PathBuf;
    let checkpoint = PathBuf::from(std::env::var("KB_REPAIR_HISTORY_CHECKPOINT").unwrap());
    let original = std::fs::read(&checkpoint).unwrap();
    let state: Checkpoint = serde_json::from_slice(&original).unwrap();
    assert_eq!(state.role, Role::Main);
    let runtime: Config = serde_json::from_slice(
        &std::fs::read(std::env::var("KB_REPAIR_HISTORY_RUNTIME").unwrap()).unwrap(),
    )
    .unwrap();
    runtime.validate().unwrap();
    let current = Config::with_provider(runtime.provider.clone(), runtime.limits.clone()).unwrap();
    assert_eq!(digest(&runtime).unwrap(), digest(&current).unwrap());
    assert_eq!(state.config_sha256, digest(&runtime).unwrap());
    let before = digest(&state).unwrap();
    let mut observations = Vec::new();
    for (offset, finding) in state.findings_for_repair().into_iter().enumerate() {
        let sha = digest(finding).unwrap();
        let value = agent::inspect_review(
            &state,
            &json!({"offset":offset,"limit":1}),
            runtime.limits.max_tool_result_bytes,
        )
        .unwrap();
        assert_eq!(value["items"], json!([finding]));
        let history = &value["repair_history"][&sha];
        if let Some(receipt) = state.repair.results.get(&sha) {
            assert_eq!(history["previous"]["summary"], receipt.summary);
            assert_eq!(history["previous"]["sources"], json!(receipt.sources));
            assert_eq!(
                history["previous"]["candidate_refs"],
                json!(receipt.candidate_versions.keys().collect::<Vec<_>>())
            );
        }
        observations.push(
            json!({"finding_sha256":sha,"offset":offset,"status":history["status"],
            "reason":history["reason"],"invalidated_refs":history["invalidated_refs"],
            "page_bytes":serde_json::to_vec(&value).unwrap().len()}),
        );
    }
    assert!(observations.iter().any(|value| value["status"] == "stale"));
    assert_eq!(digest(&state).unwrap(), before);
    assert_eq!(std::fs::read(checkpoint).unwrap(), original);
    let report = PathBuf::from(std::env::var("KB_REPAIR_HISTORY_REPORT").unwrap());
    assert!(!report.exists(), "preserve prior evidence");
    std::fs::write(report, serde_json::to_vec_pretty(&json!({
        "scope":"Offline read of archived model findings and repair receipts; no human answers, model requests, candidate repairs or independent approval",
        "config_sha256":digest(&runtime).unwrap(),"config_unchanged":true,"checkpoint_unchanged":true,
        "max_tool_result_bytes":runtime.limits.max_tool_result_bytes,"findings":observations,
        "full_acceptance":false,"pending_request_replay_validated":false
    })).unwrap()).unwrap();
}

#[tokio::test]
async fn pagination_keeps_history_for_an_included_finding_when_the_next_item_does_not_fit() {
    let (journal, _, _, sha, _) = handled().await;
    let mut state = journal.load().await.unwrap().unwrap();
    let finding = state.review_draft.values().next().unwrap().clone();
    state.review_draft.insert("zz-same-finding".into(), finding);
    let one = page(&state, usize::MAX);
    let bytes = serde_json::to_vec(&one).unwrap().len();
    let bounded = agent::inspect_review(&state, &json!({"offset":0,"limit":2}), bytes).unwrap();
    assert_eq!(bounded, one);
    assert_eq!(bounded["next"], 1);
    assert!(bounded["repair_history"].get(&sha).is_some());
    let next = agent::inspect_review(&state, &json!({"offset":1,"limit":2}), bytes).unwrap();
    assert_eq!(next["items"], one["items"]);
    assert_eq!(next["repair_history"], one["repair_history"]);
    assert_eq!(next["next"], 2);
}

#[tokio::test]
async fn newly_saved_limit_sized_finding_is_retrievable_by_both_roles() {
    let journal = fresh_review_journal().await;
    let original_config = config();
    let prepared = repair_steps(
        &journal,
        &original_config,
        vec![
            ("set_work_note", active_work("source")),
            (
                "read_source",
                json!({"source_id":"source","start":0,"max_bytes":1024}),
            ),
            (
                "inspect_analysis",
                json!({"kind":"record","view":"index","offset":0,"limit":1}),
            ),
        ],
    )
    .await;
    let finding: Finding = serde_json::from_value(json!({"code":"SOURCE_OMISSION",
        "message":"x".repeat(1024),"correction":"Compare the complete cited original.",
        "affected":[],"sources":[span()]}))
    .unwrap();
    let mut projected = prepared.clone();
    projected.role = Role::Main;
    projected
        .review_draft
        .insert("projected-id".into(), finding.clone());
    let full = page(&projected, usize::MAX);
    let budget = serde_json::to_vec(&full).unwrap().len();
    let mut limited = original_config;
    limited.limits.max_tool_result_bytes = budget;
    limited.limits.max_tool_calls = 1;
    let mut saved = prepared.clone();
    let result = agent::apply(
        &input(),
        &limited,
        &mut saved,
        "put_review_finding",
        &json!({"id":null,"finding":finding}),
    )
    .unwrap();
    let review = agent::inspect_review(&saved, &json!({"offset":0,"limit":1}), budget).unwrap();
    assert_eq!(review["items"][0]["id"], result["id"]);
    assert_eq!(review["items"][0]["finding"], json!(finding));
    saved.role = Role::Main;
    assert_eq!(page(&saved, budget)["items"], json!([finding]));
    limited.limits.max_tool_result_bytes -= 1;
    let mut rejected = prepared;
    let before = digest(&rejected).unwrap();
    let error = agent::apply(
        &input(),
        &limited,
        &mut rejected,
        "put_review_finding",
        &json!({"id":null,"finding":finding}),
    )
    .unwrap_err();
    assert!(error.contains("main history budget"), "{error}");
    assert_eq!(digest(&rejected).unwrap(), before);
}
