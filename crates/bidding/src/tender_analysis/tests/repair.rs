use super::*;

#[test]
#[ignore = "requires KB_REPAIR_FEEDBACK_RUN and KB_REPAIR_FEEDBACK_REPORT; archived request projection only, no model calls"]
fn archived_repair_feedback_projects_unread_pages_without_changing_candidates() {
    use std::path::PathBuf;
    let root = PathBuf::from(std::env::var("KB_REPAIR_FEEDBACK_RUN").unwrap());
    let original = std::fs::read(root.join("source/repair-seed.json")).unwrap();
    let mut state: Checkpoint = serde_json::from_slice(&original).unwrap();
    state.role = Role::Main;
    let analysis = digest(&state.analysis).unwrap();
    let reviewer = digest(&state.reviewer_coverage).unwrap();
    let watch = digest(&state.main_progress.watch).unwrap();
    let limits: Value =
        serde_json::from_slice(&std::fs::read(root.join("limits.json")).unwrap()).unwrap();
    let max_bytes = limits["extraction"]["max_tool_result_bytes"]
        .as_u64()
        .unwrap() as usize;
    let mut requests = 0;
    for entry in std::fs::read_dir(root.join("extraction")).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_str().unwrap();
        if !name.starts_with("turn-") || !name.ends_with("-main.json") {
            continue;
        }
        let body: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let receipts =
            agent::delivered_repair_feedback(&state, body["messages"].as_array().unwrap()).unwrap();
        state.main_progress.seen.extend(receipts);
        requests += 1;
    }
    let before =
        agent::repair_feedback_packet(&state, &[], &crate::tender_analysis::tests::config().limits)
            .unwrap();
    assert!(before["unread"].as_u64().unwrap() > 0);
    let mut pages = Vec::new();
    while let Some(query) =
        agent::repair_feedback_packet(&state, &[], &crate::tender_analysis::tests::config().limits)
            .unwrap()["next_query"]
            .as_object()
    {
        let query = json!(query);
        let result = agent::inspect_review(&state, &query, max_bytes).unwrap();
        assert!(result["next"].as_u64().unwrap() > query["offset"].as_u64().unwrap());
        let messages = vec![
            json!({"role":"assistant","tool_calls":[{"id":"page","function":{"name":"inspect_review","arguments":query.to_string()}}]}),
            json!({"role":"tool","tool_call_id":"page","content":json!({"ok":true,"result":result}).to_string()}),
        ];
        let receipts = agent::delivered_repair_feedback(&state, &messages).unwrap();
        pages.push(json!({"query":query,"next":result["next"],"whole_findings":receipts.len()}));
        state.main_progress.seen.extend(receipts);
    }
    let after =
        agent::repair_feedback_packet(&state, &[], &crate::tender_analysis::tests::config().limits)
            .unwrap();
    assert_eq!(after["unread"], 0);
    assert_eq!(digest(&state.analysis).unwrap(), analysis);
    assert_eq!(digest(&state.reviewer_coverage).unwrap(), reviewer);
    assert_eq!(digest(&state.main_progress.watch).unwrap(), watch);
    assert_eq!(
        std::fs::read(root.join("source/repair-seed.json")).unwrap(),
        original
    );
    let restored: Checkpoint = serde_json::from_value(json!(state)).unwrap();
    assert_eq!(
        agent::repair_feedback_packet(
            &restored,
            &[],
            &crate::tender_analysis::tests::config().limits
        )
        .unwrap(),
        after
    );
    let report = PathBuf::from(std::env::var("KB_REPAIR_FEEDBACK_REPORT").unwrap());
    assert!(!report.exists(), "preserve prior replay evidence");
    std::fs::write(report, serde_json::to_vec_pretty(&json!({
        "scope":"Archived main request visibility and hypothetical remaining page delivery, not original checkpoint recovery, model execution or semantic acceptance",
        "requests":requests,"before":before,"remaining_pages":pages,"after":after,
        "candidate_analysis_unchanged":true,"reviewer_receipts_unchanged":true,
        "progress_watch_unchanged":true,"original_seed_unchanged":true
    })).unwrap()).unwrap();
}

#[tokio::test]
async fn partial_repair_feedback_read_cannot_start_another_review() {
    let journal = fresh_review_journal().await;
    let mut state = journal.load().await.unwrap().unwrap();
    state.role = Role::Main;
    state.review = None;
    for index in 0..2 {
        state.review_draft.insert(
            format!("prior-{index}"),
            serde_json::from_value(json!({
                "code":"source_issue", "message":format!("Prior model finding {index}"),
                "correction":"Compare the original before deciding a correction",
                "affected":[], "sources":[span()]
            }))
            .unwrap(),
        );
    }
    let start = state.turn;
    *journal.state.lock().unwrap() = Some(state);
    *journal.interrupt_after.lock().unwrap() = Some(start + 2);
    let model = work_script(vec![
        ("inspect_review", json!({"offset":0,"limit":1})),
        ("request_review", json!({})),
    ]);
    agent::run(
        &input(),
        &config(),
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let saved = journal.load().await.unwrap().unwrap();
    assert_eq!(
        saved.role,
        Role::Main,
        "one feedback page is not the whole repair report"
    );
    assert_eq!(saved.review_draft.len(), 2);
    assert!(!saved.done);
    let packet =
        agent::repair_feedback_packet(&saved, &[], &crate::tender_analysis::tests::config().limits)
            .unwrap();
    assert_eq!(packet["received"], 1);
    assert_eq!(packet["next_query"], json!({"offset":1,"limit":1}));

    // Delivery survives serialization and history eviction. An offline query
    // alone cannot advance it, nor can modifying the stored finding reuse it.
    let mut resumed: Checkpoint =
        serde_json::from_value(serde_json::to_value(&saved).unwrap()).unwrap();
    resumed.transcript.clear();
    let before = digest(&resumed.main_progress).unwrap();
    agent::inspect_review(&resumed, &json!({"offset":1,"limit":1}), 16000).unwrap();
    assert_eq!(digest(&resumed.main_progress).unwrap(), before);
    let mut revised = resumed.clone();
    revised
        .review_draft
        .get_mut("prior-0")
        .unwrap()
        .correction
        .push_str(" with new evidence");
    assert_eq!(
        agent::repair_feedback_packet(
            &revised,
            &[],
            &crate::tender_analysis::tests::config().limits
        )
        .unwrap()["unread"],
        2
    );
    assert!(
        saved
            .reviewer_progress
            .seen
            .iter()
            .all(|key| !key.starts_with("repair_feedback:"))
    );

    *journal.state.lock().unwrap() = Some(resumed);
    *journal.interrupt_after.lock().unwrap() = Some(saved.turn + 2);
    let model = work_script(vec![
        ("inspect_review", json!({"offset":1,"limit":1})),
        ("request_review", json!({})),
    ]);
    agent::run(
        &input(),
        &config(),
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let complete = journal.load().await.unwrap().unwrap();
    assert_eq!(complete.role, Role::Main);
    assert_eq!(
        agent::repair_feedback_packet(
            &complete,
            &[],
            &crate::tender_analysis::tests::config().limits
        )
        .unwrap()["repair"]["pending"],
        2
    );
    assert_eq!(
        agent::repair_feedback_packet(
            &complete,
            &[],
            &crate::tender_analysis::tests::config().limits
        )
        .unwrap()["unread"],
        0
    );
    assert_eq!(
        complete.review_draft.len(),
        2,
        "reading feedback must not clear any finding"
    );
    assert!(!complete.done);
}

#[tokio::test]
async fn complete_feedback_delivery_cannot_replace_per_finding_repair_disposition() {
    let journal = fresh_review_journal().await;
    let mut state = journal.load().await.unwrap().unwrap();
    state.role = Role::Main;
    state.review = None;
    state.review_draft.insert("unhandled".into(), serde_json::from_value(json!({
        "code":"source_issue", "message":"A saved source-backed problem still needs repair or a grounded disagreement.",
        "correction":"Reconcile this finding against its actual original evidence.",
        "affected":[], "sources":[span()]
    })).unwrap());
    let start = state.turn;
    *journal.state.lock().unwrap() = Some(state);
    *journal.interrupt_after.lock().unwrap() = Some(start + 2);
    let model = work_script(vec![
        ("inspect_review", json!({"offset":0,"limit":1})),
        ("request_review", json!({})),
    ]);
    agent::run(
        &input(),
        &config(),
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let saved = journal.load().await.unwrap().unwrap();
    assert_eq!(
        agent::repair_feedback_packet(&saved, &[], &crate::tender_analysis::tests::config().limits)
            .unwrap()["unread"],
        0
    );
    assert_eq!(
        saved.role,
        Role::Main,
        "Reading the entire report cannot replace a source-backed disposition for each finding"
    );
    assert_eq!(saved.review_draft.len(), 1);
    assert!(!saved.done);
}

// Run the disposition scenarios through completed model responses so read
// receipts, checkpoint restoration and handoff use the production boundaries.
pub(super) async fn repair_fixture(affected: bool) -> (MemoryJournal, Config, String, String) {
    let mut config = config();
    config.limits.max_turns = 100;
    config.limits.max_tool_calls = 200;
    let journal = fresh_review_journal_config(&config).await;
    let mut state = journal.load().await.unwrap().unwrap();
    state.role = Role::Main;
    let id = state.analysis.records.keys().next().unwrap().clone();
    let finding: Finding = serde_json::from_value(json!({
        "code":"fixture_issue", "message":"Check the submitted requirement against its original wording",
        "correction":"Reconcile the requirement with the original", "sources":[span()],
        "affected":if affected { json!([{"id":id,"path":"/data/text"}]) } else {json!([])}
    })).unwrap();
    let sha = digest(&finding).unwrap();
    state.review_draft.insert("fixture-issue".into(), finding);
    *journal.state.lock().unwrap() = Some(state);
    (journal, config, id, sha)
}

pub(super) async fn repair_steps(
    journal: &MemoryJournal,
    config: &Config,
    calls: Vec<(&str, Value)>,
) -> Checkpoint {
    let start = journal.load().await.unwrap().unwrap().turn;
    *journal.interrupt_after.lock().unwrap() = Some(start + calls.len());
    let error = agent::run(
        &input(),
        config,
        journal,
        &work_script(calls),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "INTERNAL", "{error:?}");
    journal.load().await.unwrap().unwrap()
}

#[tokio::test]
async fn repair_disposition_requires_relevant_change_and_current_details_and_survives_restore() {
    let (journal, config, id, sha) = repair_fixture(true).await;
    let detail = json!({"view":"detail","kind":"record","ids":[id],"offset":0,"limit":1});
    let receipt = json!({"finding_sha256":sha,"conclusion":"revised","summary":"Reconciled the requirement wording with the cited original.",
        "sources":[span()],"candidate_refs":[format!("record:{id}")]});
    let saved = repair_steps(
        &journal,
        &config,
        vec![
            ("inspect_review", json!({"offset":0,"limit":1})),
            ("inspect_analysis", detail.clone()),
            ("put_repair_result", receipt.clone()),
        ],
    )
    .await;
    assert!(
        saved.repair.results.is_empty(),
        "unchanged candidate cannot be revised"
    );
    // A changed unrelated candidate on the same page is not this field's repair.
    let mut unrelated = requirement();
    unrelated["data"]["text"] = json!("Another fixture requirement");
    let saved = repair_steps(
        &journal,
        &config,
        vec![
            ("put_record", unrelated),
            (
                "inspect_analysis",
                json!({"view":"detail","kind":"record","offset":0,"limit":10}),
            ),
        ],
    )
    .await;
    let extra = saved
        .analysis
        .records
        .keys()
        .find(|other| *other != &id)
        .unwrap();
    let mut borrowed = receipt.clone();
    borrowed["candidate_refs"] = json!([format!("record:{id}"), format!("record:{extra}")]);
    let saved = repair_steps(&journal, &config, vec![("put_repair_result", borrowed)]).await;
    assert!(
        saved.repair.results.is_empty(),
        "unrelated edit must not discharge this issue"
    );
    let mut edit = requirement();
    edit["id"] = json!(id);
    edit["data"]["text"] = json!("按须知提交规定格式");
    let saved = repair_steps(
        &journal,
        &config,
        vec![
            ("put_record", edit.clone()),
            ("put_repair_result", receipt.clone()),
        ],
    )
    .await;
    assert!(
        saved.repair.results.is_empty(),
        "write result is not a current detail read"
    );
    let saved = repair_steps(
        &journal,
        &config,
        vec![("inspect_analysis", detail), ("put_repair_result", receipt)],
    )
    .await;
    assert_eq!(saved.repair.results.len(), 1);
    assert_eq!(
        agent::repair_feedback_packet(&saved, &[], &crate::tender_analysis::tests::config().limits)
            .unwrap()["repair"]["pending"],
        0
    );
    let mut restored: Checkpoint = serde_json::from_value(json!(saved)).unwrap();
    restored.transcript.clear();
    assert_eq!(
        agent::repair_feedback_packet(
            &restored,
            &[],
            &crate::tender_analysis::tests::config().limits
        )
        .unwrap()["repair"]["pending"],
        0
    );
    *journal.state.lock().unwrap() = Some(restored);
    edit["data"]["text"] = json!("Later changed wording requiring another check");
    let saved = repair_steps(&journal, &config, vec![("put_record", edit)]).await;
    assert_eq!(
        agent::repair_feedback_packet(&saved, &[], &crate::tender_analysis::tests::config().limits)
            .unwrap()["repair"]["pending"],
        1
    );
    assert_eq!(
        saved.review_draft.len(),
        1,
        "main cannot withdraw independent findings"
    );
}

#[tokio::test]
async fn repair_disposition_tracks_real_deletions_and_rejects_invented_deleted_candidates() {
    let (journal, config, id, sha) = repair_fixture(true).await;
    let receipt = json!({"finding_sha256":sha,"conclusion":"revised","summary":"Removed the unsupported fixture candidate after checking the source.",
        "sources":[span()],"candidate_refs":[format!("record:{id}")]});
    let mut invented = receipt.clone();
    invented["candidate_refs"] = json!([format!("record:{id}"), "record:never-existed"]);
    let rejected = repair_steps(
        &journal,
        &config,
        vec![
            ("inspect_review", json!({"offset":0,"limit":1})),
            ("delete_record", json!({"id":id})),
            ("put_repair_result", invented.clone()),
        ],
    )
    .await;
    assert!(rejected.repair.results.is_empty());
    assert!(
        rejected.transcript.last().unwrap()["content"]
            .as_str()
            .unwrap()
            .contains("unknown repair candidate")
    );
    let saved = repair_steps(
        &journal,
        &config,
        vec![("put_repair_result", receipt.clone())],
    )
    .await;
    assert_eq!(
        saved.repair.results[&sha].candidate_versions[&format!("record:{id}")],
        None
    );
    assert_eq!(
        agent::repair_feedback_packet(&saved, &[], &crate::tender_analysis::tests::config().limits)
            .unwrap()["repair"]["pending"],
        0
    );
    // Once committed, a complete task has no active write allowance.

    let saved = repair_steps(&journal, &config, vec![("put_repair_result", invented)]).await;
    assert_eq!(saved.repair.results[&sha].candidate_versions.len(), 1);
    assert!(
        saved.transcript.last().unwrap()["content"]
            .as_str()
            .unwrap()
            .contains("select a repair task")
    );
}

#[tokio::test]
async fn repair_disposition_cannot_use_a_candidate_read_from_the_same_response() {
    struct Batch(Vec<ChatToolCall>);
    #[async_trait]
    impl Model for Batch {
        async fn turn(&self, _: &Config, _: &[u8]) -> Result<ChatTurn, AgentError> {
            Ok(ChatTurn {
                tool_calls: self.0.clone(),
                finish_reason: "tool_calls".into(),
                ..Default::default()
            })
        }
    }
    let (journal, config, id, sha) = repair_fixture(true).await;
    let mut edit = requirement();
    edit["id"] = json!(id);
    edit["data"]["text"] = json!("按须知提交规定格式");
    let state = repair_steps(
        &journal,
        &config,
        vec![
            ("inspect_review", json!({"offset":0,"limit":1})),
            ("put_record", edit),
        ],
    )
    .await;
    let receipt = json!({"finding_sha256":sha,"conclusion":"revised","summary":"Corrected the requirement from its original.",
        "sources":[span()],"candidate_refs":[format!("record:{id}")]});
    let model = Batch(vec![
        ChatToolCall {
            id: "detail".into(),
            name: "inspect_analysis".into(),
            arguments: json!({"view":"detail","kind":"record","ids":[id],"offset":0,"limit":1})
                .to_string(),
        },
        ChatToolCall {
            id: "receipt".into(),
            name: "put_repair_result".into(),
            arguments: receipt.to_string(),
        },
    ]);
    *journal.interrupt_after.lock().unwrap() = Some(state.turn + 1);
    assert_eq!(
        agent::run(
            &input(),
            &config,
            &journal,
            &model,
            &CancellationToken::new()
        )
        .await
        .unwrap_err()
        .code,
        "INTERNAL"
    );
    let saved = journal.load().await.unwrap().unwrap();
    assert!(saved.repair.results.is_empty());
    let saved = repair_steps(
        &journal,
        &config,
        vec![("put_repair_result", receipt.clone())],
    )
    .await;
    assert_eq!(
        saved.repair.results.len(),
        1,
        "next completed response delivers the detail"
    );
    let seen = saved.main_progress.seen.clone();
    let mut rewritten = receipt;
    rewritten["summary"] = json!("Same correction, merely reworded summary.");
    let saved = repair_steps(&journal, &config, vec![("put_repair_result", rewritten)]).await;
    assert_eq!(
        saved.main_progress.seen, seen,
        "rewriting a summary cannot renew progress"
    );
}

#[tokio::test]
async fn source_backed_dispute_requires_original_read_and_allows_only_independent_handoff() {
    let (journal, config, _, sha) = repair_fixture(false).await;
    journal
        .state
        .lock()
        .unwrap()
        .as_mut()
        .unwrap()
        .analysis
        .coverage
        .text
        .clear();
    let receipt = json!({"finding_sha256":sha,"conclusion":"disputed",
        "summary":"The original already supports the submitted requirement; ask the independent reviewer to check the cited span.",
        "sources":[span()],"candidate_refs":[]});
    let saved = repair_steps(
        &journal,
        &config,
        vec![
            ("inspect_review", json!({"offset":0,"limit":1})),
            ("put_repair_result", receipt.clone()),
        ],
    )
    .await;
    assert!(
        saved.repair.results.is_empty(),
        "feedback is not original reading coverage"
    );
    let saved = repair_steps(
        &journal,
        &config,
        vec![
            ("set_work_note", active_work("source")),
            (
                "read_source",
                json!({"source_id":"source","start":0,"max_bytes":1024}),
            ),
            ("put_repair_result", receipt.clone()),
            ("request_review", json!({})),
        ],
    )
    .await;
    assert_eq!(saved.repair.results.len(), 1);
    assert_eq!(saved.role, Role::Reviewer);
    assert!(!saved.done);
    assert_eq!(saved.review_draft.len(), 1);
    let before = digest(&saved.repair).unwrap();
    let saved = repair_steps(&journal, &config, vec![("put_repair_result", receipt)]).await;
    assert_eq!(
        digest(&saved.repair).unwrap(),
        before,
        "reviewer cannot write main dispositions"
    );
}

#[tokio::test]
async fn prior_draft_is_readable_repair_feedback_without_review_completion_or_receipts() {
    let journal = fresh_review_journal().await;
    let mut state = journal.load().await.unwrap().unwrap();
    state.role = Role::Main;
    state.review = None;
    state.review_rounds = 0;
    let finding: Finding = serde_json::from_value(json!({
        "code":"source_issue", "message":"Prior model claims a missing relation",
        "correction":"Compare the original requirement and form before changing their mapping",
        "affected":[], "sources":[span()]
    }))
    .unwrap();
    state
        .review_draft
        .insert("prior-model-finding".into(), finding.clone());
    let before = serde_json::to_value(&state).unwrap();
    let page = agent::inspect_review(&state, &json!({"offset":0,"limit":10}), 16000).unwrap();
    assert_eq!(page["items"], json!([finding]));
    assert_eq!(serde_json::to_value(&state).unwrap(), before);
    assert!(state.review.is_none());
    assert_eq!(state.review_rounds, 0);
    assert!(state.reviewer_coverage.text.is_empty());
    assert!(state.reviewer_coverage.candidate.is_empty());
    assert!(
        state
            .source_review
            .as_ref()
            .unwrap()
            .completed_analysis_sha256
            .is_none()
    );
    *journal.state.lock().unwrap() = Some(state);
    let start = journal.load().await.unwrap().unwrap().turn;
    *journal.interrupt_after.lock().unwrap() = Some(start + 3);
    let model = work_script(vec![
        ("delete_review_finding", json!({"id":"prior-model-finding"})),
        ("inspect_review", json!({"offset":0,"limit":10})),
        ("request_review", json!({})),
    ]);
    agent::run(
        &input(),
        &config(),
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let saved = journal.load().await.unwrap().unwrap();
    assert_eq!(saved.role, Role::Main);
    assert_eq!(
        agent::repair_feedback_packet(&saved, &[], &crate::tender_analysis::tests::config().limits)
            .unwrap()["repair"]["pending"],
        1
    );
    assert!(saved.review_draft.contains_key("prior-model-finding"));
    assert_eq!(saved.review_rounds, 0);
    assert!(!saved.done);
    assert!(saved.review.is_none());
    assert!(
        saved
            .source_review
            .as_ref()
            .unwrap()
            .completed_analysis_sha256
            .is_none()
    );
}
