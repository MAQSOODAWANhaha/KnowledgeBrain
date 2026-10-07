use super::*;

struct Batch(Vec<(&'static str, Value)>);
#[async_trait]
impl Model for Batch {
    async fn turn(&self, _: &Config, _: &[u8]) -> Result<ChatTurn, AgentError> {
        Ok(ChatTurn {
            content: String::new(),
            tool_calls: self
                .0
                .iter()
                .enumerate()
                .map(|(i, (name, args))| ChatToolCall {
                    id: format!("handoff-{i}"),
                    name: (*name).into(),
                    arguments: args.to_string(),
                })
                .collect(),
            usage: None,
            finish_reason: "tool_calls".into(),
        })
    }
}

async fn ready() -> (MemoryJournal, Checkpoint) {
    let journal = MemoryJournal::default();
    *journal.interrupt_after.lock().unwrap() = Some(3);
    let model = work_script(vec![
        ("set_work_note", active_work("source")),
        (
            "read_source",
            json!({"source_id":"source","start":0,"max_bytes":1024}),
        ),
        (
            "put_record",
            json!({"id":null,"sources":[span()],"data":{"kind":"fact","name":"fixture title","value":"fixture","scope":"source"}}),
        ),
    ]);
    let error = agent::run(
        &input(),
        &config(),
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "INTERNAL", "{error:?}");
    let mut state = journal.load().await.unwrap().unwrap();
    let mut coverage = state.analysis.coverage.clone();
    tools::invoke(
        &input(),
        &mut state.analysis,
        &mut coverage,
        false,
        "set_disposition",
        &json!({"source_id":"source","state":"non_requirement","reason":"fixture title"}),
        config().limits.max_tool_result_bytes,
    )
    .unwrap();
    fixture_global_checks(&input(), &config(), &mut state);
    *journal.state.lock().unwrap() = Some(state.clone());
    assert!(tools::gaps(&input(), &state.analysis).is_empty());
    (journal, state)
}

// These tests extend a synthetic frozen input, not a production checkpoint.
// Re-key its existing source/global roots and add the new untouched source.
fn extend_fixture_roots(input: &FrozenInput, state: &mut Checkpoint) {
    let old = std::mem::take(&mut state.dispatch.entries);
    let active_source = state
        .dispatch
        .active
        .as_ref()
        .and_then(|active| match active {
            agent::main_dispatch::Active::Ordinary(id) => old.get(id).map(|e| e.source_id.clone()),
            _ => None,
        });
    for source_id in input
        .source_units
        .iter()
        .map(|s| Some(s.source_unit_revision_id.clone()))
        .chain(std::iter::once(None))
    {
        let entry = old
            .values()
            .find(|e| e.source_id == source_id)
            .cloned()
            .unwrap_or_else(|| agent::main_dispatch::Entry {
                source_id: source_id.clone(),
                ..Default::default()
            });
        let id = digest(&json!([
            "main-dispatch-v1",
            digest(input).unwrap(),
            source_id
        ]))
        .unwrap();
        if active_source.as_ref() == Some(&source_id) {
            state.dispatch.active = Some(agent::main_dispatch::Active::Ordinary(id.clone()));
        }
        state.dispatch.entries.insert(id, entry);
    }
}

fn complete() -> Value {
    let mut work = active_work("source");
    work["status"] = json!("complete");
    work
}

async fn batch(
    journal: &MemoryJournal,
    before: &Checkpoint,
    calls: Vec<(&'static str, Value)>,
) -> Checkpoint {
    *journal.interrupt_after.lock().unwrap() = Some(before.turn + 1);
    let error = agent::run(
        &input(),
        &config(),
        journal,
        &Batch(calls),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "INTERNAL", "{error:?}");
    journal.load().await.unwrap().unwrap()
}

#[tokio::test]
async fn whole_collection_completion_hands_off_without_another_model_call() {
    let (journal, before) = ready().await;
    let saved = batch(&journal, &before, vec![("set_work_note", complete())]).await;
    assert_eq!(saved.role, Role::Reviewer);
    assert_eq!(saved.turn, before.turn + 1);
    assert_eq!(saved.tool_calls, before.tool_calls + 1);
    assert_eq!(journal.reservations.lock().unwrap().len(), before.turn + 1);
    assert!(saved.transcript.is_empty());
    assert!(saved.journal.session.is_none());
    assert!(saved.source_review.as_ref().unwrap().active_task.is_some());
    assert!(saved.reviewer_coverage.text.is_empty());
    assert!(saved.reviewer_progress.seen.is_empty());
    assert_eq!(saved.review_rounds, 0);
    assert!(!saved.done);
}

#[tokio::test]
async fn failed_or_mixed_transition_batch_cannot_auto_handoff() {
    for calls in [
        vec![("set_work_note", complete()), ("put_record", json!({}))],
        vec![("put_record", json!({})), ("set_work_note", complete())],
        vec![("set_work_note", complete()), ("request_review", json!({}))],
    ] {
        let (journal, before) = ready().await;
        let saved = batch(&journal, &before, calls).await;
        assert_eq!(saved.role, Role::Main);
        assert!(saved.source_review.is_none());
        assert_eq!(saved.turn, before.turn + 1);
        assert!(
            saved.transcript.iter().filter(|m| m["role"] == "tool").any(
                |m| serde_json::from_str::<Value>(m["content"].as_str().unwrap()).unwrap()["ok"]
                    == false
            )
        );
    }
}

#[tokio::test]
async fn later_candidate_write_invalidates_explicit_completion_for_auto_handoff() {
    let (journal, before) = ready().await;
    let mut record = json!(before.analysis.records.values().next().unwrap());
    record["data"]["value"] = json!("changed after completion");
    let saved = batch(
        &journal,
        &before,
        vec![("set_work_note", complete()), ("put_record", record)],
    )
    .await;
    assert_eq!(saved.role, Role::Main);
    assert_ne!(
        digest(&saved.analysis).unwrap(),
        digest(&before.analysis).unwrap()
    );
    assert!(tools::gaps(&input(), &saved.analysis).is_empty());
    assert!(saved.source_review.is_none());
}

#[tokio::test]
async fn local_completion_uses_global_saved_coverage_instead_of_redeclaring_all_sources() {
    let (journal, mut before) = ready().await;
    let mut larger = input();
    larger.source_units.push(Source {
        source_unit_revision_id: "other".into(),
        ordinal: 1,
        ..larger.source_units[0].clone()
    });
    // A previously extracted independent source makes the global structural
    // check empty, while this response explicitly completes only one scope.
    before.input_sha256 = digest(&larger).unwrap();
    extend_fixture_roots(&larger, &mut before);
    tools::cover(
        before
            .analysis
            .coverage
            .text
            .entry("other".into())
            .or_default(),
        0,
        larger.source_units[1].text.len(),
    );
    let mut coverage = before.analysis.coverage.clone();
    tools::invoke(&larger, &mut before.analysis, &mut coverage, false, "set_disposition", &json!({"source_id":"other","state":"non_requirement","reason":"independent fixture title"}), config().limits.max_tool_result_bytes).unwrap();
    assert!(tools::gaps(&larger, &before.analysis).is_empty());
    fixture_global_checks(&larger, &config(), &mut before);
    *journal.state.lock().unwrap() = Some(before.clone());
    *journal.interrupt_after.lock().unwrap() = Some(before.turn + 1);
    let error = agent::run(
        &larger,
        &config(),
        &journal,
        &Batch(vec![("set_work_note", complete())]),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "INTERNAL", "{error:?}");
    let saved = journal.load().await.unwrap().unwrap();
    assert_eq!(
        saved.main_work.as_ref().unwrap().status,
        agent::context::WorkStatus::Complete
    );
    assert_eq!(saved.role, Role::Reviewer);
}

#[tokio::test]
async fn local_completion_cannot_skip_an_unread_or_undisposed_source() {
    let (journal, mut before) = ready().await;
    let mut larger = input();
    larger.source_units.push(Source {
        source_unit_revision_id: "other".into(),
        ordinal: 1,
        ..larger.source_units[0].clone()
    });
    before.input_sha256 = digest(&larger).unwrap();
    extend_fixture_roots(&larger, &mut before);
    *journal.state.lock().unwrap() = Some(before.clone());
    *journal.interrupt_after.lock().unwrap() = Some(before.turn + 1);
    let error = agent::run(
        &larger,
        &config(),
        &journal,
        &Batch(vec![("set_work_note", complete())]),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "INTERNAL", "{error:?}");
    let saved = journal.load().await.unwrap().unwrap();
    assert_eq!(saved.role, Role::Main);
    assert!(saved.source_review.is_none());
    assert!(!saved.analysis.dispositions.contains_key("other"));
}

#[tokio::test]
async fn received_completion_recovers_without_model_recall_or_double_charge() {
    let (journal, before) = ready().await;
    *journal.fail_boundary_ack.lock().unwrap() = Some(before.journal.sequence + 2);
    let error = agent::run(
        &input(),
        &config(),
        &journal,
        &Batch(vec![("set_work_note", complete())]),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "INTERNAL", "{error:?}");
    let received = journal.load().await.unwrap().unwrap();
    assert!(received.journal.response().is_some());
    assert_eq!(received.role, Role::Main);
    assert_eq!(received.turn, before.turn);
    let reservations = journal.reservations.lock().unwrap().clone();
    *journal.state.lock().unwrap() = Some(serde_json::from_value(json!(received)).unwrap());
    *journal.interrupt_after.lock().unwrap() = Some(before.turn + 1);
    let unused_model = work_script(vec![]);
    let error = agent::run(
        &input(),
        &config(),
        &journal,
        &unused_model,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "INTERNAL", "{error:?}");
    let saved = journal.load().await.unwrap().unwrap();
    assert_eq!(saved.role, Role::Reviewer);
    assert_eq!(saved.tool_calls, before.tool_calls + 1);
    assert_eq!(saved.turn, before.turn + 1);
    assert_eq!(*journal.reservations.lock().unwrap(), reservations);
    assert!(unused_model.bodies.lock().unwrap().is_empty());
    assert_eq!(saved.journal.sequence, before.journal.sequence + 3);
}

#[tokio::test]
async fn same_batch_main_read_receipts_do_not_cross_into_reviewer() {
    let (journal, before) = ready().await;
    let saved = batch(
        &journal,
        &before,
        vec![
            ("set_work_note", complete()),
            (
                "inspect_analysis",
                json!({"kind":"all","view":"detail","offset":0,"limit":10}),
            ),
        ],
    )
    .await;
    assert_eq!(saved.role, Role::Main);
    assert!(saved.pending_coverage.is_some());
    assert!(saved.reviewer_coverage.text.is_empty());
    assert!(saved.source_review.is_none());
    assert!(
        saved
            .transcript
            .iter()
            .filter(|m| m["role"] == "tool")
            .all(
                |m| serde_json::from_str::<Value>(m["content"].as_str().unwrap()).unwrap()["ok"]
                    == true
            )
    );
    // A subsequent Main response confirms its own reads before a legal
    // separate handoff; the reviewer still inherits none of those receipts.
    let reviewed = batch(&journal, &saved, vec![("request_review", json!({}))]).await;
    assert_eq!(reviewed.role, Role::Reviewer);
    assert!(reviewed.pending_coverage.is_none());
    assert!(reviewed.reviewer_coverage.text.is_empty());
    assert!(reviewed.reviewer_coverage.candidate.is_empty());
}

#[tokio::test]
async fn source_uncertainty_survives_handoff_for_independent_judgment() {
    let (journal, before) = ready().await;
    let saved = batch(&journal, &before, vec![
        ("set_disposition", json!({"source_id":"source","state":"unresolved","reason":"The referenced target requires independent source interpretation."})),
        ("set_work_note", complete()),
    ]).await;
    assert_eq!(
        saved.role,
        Role::Main,
        "changed disposition invalidates earlier global checks"
    );
    let mut updated = saved;
    fixture_global_checks(&input(), &config(), &mut updated);
    *journal.state.lock().unwrap() = Some(updated.clone());
    let saved = batch(&journal, &updated, vec![("request_review", json!({}))]).await;
    assert_eq!(saved.role, Role::Reviewer);
    assert_eq!(
        saved.main_work.as_ref().unwrap().pending_refs,
        vec!["disposition:source"]
    );
    assert_eq!(
        saved.analysis.dispositions["source"].state,
        DispositionState::Unresolved
    );
    assert!(saved.review.is_none());
    assert_eq!(saved.review_rounds, 0);
    assert!(!saved.done);
}
