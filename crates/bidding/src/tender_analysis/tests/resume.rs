use super::*;

#[tokio::test]
async fn pending_reads_are_not_evidence_until_a_successful_model_turn_after_resume() {
    let journal = MemoryJournal::default();
    *journal.interrupt_after.lock().unwrap() = Some(2);
    let model = script();
    let err = agent::run(
        &input(),
        &config(),
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(err.code, "INTERNAL");
    let saved = journal.load().await.unwrap().unwrap();
    assert!(
        saved.analysis.coverage.text.is_empty(),
        "a tool receipt is not model delivery"
    );
    assert!(saved.reviewer_coverage.text.is_empty());
    let json = serde_json::to_value(&saved).unwrap();
    assert_eq!(
        json["pending_coverage"]["text"]["source"],
        json!([[0, span().end]])
    );
    assert!(tools::validate_span(&input(), &saved.analysis.coverage, &span()).is_err());
    let result = agent::run(
        &input(),
        &config(),
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(result.quality, "verified");
    assert!(tools::validate_span(&input(), &result.analysis.coverage, &span()).is_ok());
    assert!(tools::validate_span(&input(), &result.review.coverage, &span()).is_ok());
}

#[tokio::test]
async fn failed_delivery_retains_pending_reads_without_committing_coverage() {
    struct Unavailable;
    #[async_trait]
    impl Model for Unavailable {
        async fn turn(&self, _: &Config, _: &[u8]) -> Result<ChatTurn, AgentError> {
            Err(AgentError::new(
                "AGENT_PROVIDER_UNAVAILABLE",
                "scripted HTTP 503",
            ))
        }
    }
    let journal = MemoryJournal::default();
    *journal.interrupt_after.lock().unwrap() = Some(2);
    agent::run(
        &input(),
        &config(),
        &journal,
        &script(),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let before = journal.load().await.unwrap().unwrap();
    let err = agent::run(
        &input(),
        &config(),
        &journal,
        &Unavailable,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(err.code, "AGENT_PROVIDER_UNAVAILABLE");
    let after = journal.load().await.unwrap().unwrap();
    assert!(after.analysis.coverage.text.is_empty());
    assert_eq!(after.turn, before.turn);
    assert_eq!(after.tool_calls, before.tool_calls);
    assert_eq!(after.read_bytes, before.read_bytes);
    assert_eq!(
        digest(&after.analysis).unwrap(),
        digest(&before.analysis).unwrap()
    );
    assert_eq!(
        digest(&after.pending_coverage).unwrap(),
        digest(&before.pending_coverage).unwrap()
    );
    assert_eq!(after.journal.sequence, before.journal.sequence + 1);
    assert!(after.journal.response().is_none());
    let reservations = journal.reservations.lock().unwrap();
    assert_eq!(after.journal.body().unwrap(), reservations[&2].0);
    assert_eq!(reservations[&2].1, 3);
}

#[tokio::test]
async fn resume_reuses_saved_turns_and_frozen_config() {
    let journal = MemoryJournal::default();
    *journal.interrupt_after.lock().unwrap() = Some(6);
    let model = script();
    let expected_calls = model.calls.lock().unwrap().len();
    let failed = agent::run(
        &input(),
        &config(),
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(failed.code, "INTERNAL");
    let mut changed = config();
    changed.limits.max_turns += 1;
    let failed = agent::run(
        &input(),
        &changed,
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(failed.code, "FROZEN_INPUT_DIGEST_MISMATCH");
    let calls_before = model.bodies.lock().unwrap().len();
    let mut changed_adapter = config();
    changed_adapter.runtime_adapter.push_str("-changed");
    let failed = agent::run(
        &input(),
        &changed_adapter,
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(failed.code, "FROZEN_INPUT_DIGEST_MISMATCH");
    assert_eq!(model.bodies.lock().unwrap().len(), calls_before);
    let mut changed_policy = config();
    changed_policy.repair_task_policy.push_str("-changed");
    let failure = agent::run(
        &input(),
        &changed_policy,
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(failure.code, "FROZEN_INPUT_DIGEST_MISMATCH");
    assert_eq!(model.bodies.lock().unwrap().len(), calls_before);
    let mut no_task_policy = serde_json::to_value(config()).unwrap();
    no_task_policy
        .as_object_mut()
        .unwrap()
        .remove("repair_task_policy");
    assert!(serde_json::from_value::<agent::Config>(no_task_policy).is_err());
    let mut old_contract = serde_json::to_value(config()).unwrap();
    old_contract
        .as_object_mut()
        .unwrap()
        .remove("runtime_adapter");
    assert!(serde_json::from_value::<agent::Config>(old_contract).is_err());
    let result = agent::run(
        &input(),
        &config(),
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(result.quality, "verified");
    assert_eq!(model.bodies.lock().unwrap().len(), expected_calls);
    assert!(
        journal
            .sdk_turns
            .lock()
            .unwrap()
            .iter()
            .any(|turn| *turn > 1),
        "production extraction must reuse SDK multi-turn state"
    );
    assert!(
        journal
            .reservations
            .lock()
            .unwrap()
            .values()
            .all(|(_, n)| *n == 1)
    );
}

#[tokio::test]
async fn prepared_and_received_boundaries_resume_without_losing_or_repeating_model_turns() {
    for boundary in [1, 2] {
        let journal = MemoryJournal::default();
        *journal.fail_boundary_ack.lock().unwrap() = Some(boundary);
        let model = script();
        let expected_calls = model.calls.lock().unwrap().len();
        let cancel = CancellationToken::new();
        let err = agent::run(&input(), &config(), &journal, &model, &cancel)
            .await
            .unwrap_err();
        assert_eq!(err.code, "INTERNAL");
        let saved = journal.load().await.unwrap().unwrap();
        assert_eq!(saved.turn, 0);
        assert_eq!(saved.tool_calls, 0, "no tools before durable response ACK");
        assert!(saved.main_work.is_none());
        assert_eq!(saved.journal.sequence, boundary);
        assert_eq!(saved.journal.response().is_some(), boundary == 2);
        assert_eq!(
            model.bodies.lock().unwrap().len(),
            usize::from(boundary == 2)
        );
        assert_eq!(
            saved.journal.body().unwrap(),
            journal.reservations.lock().unwrap()[&0].0
        );
        // Cancellation at recovery must leave the saved response/tools untouched.
        let cancelled = CancellationToken::new();
        cancelled.cancel();
        assert!(
            agent::run(&input(), &config(), &journal, &model, &cancelled)
                .await
                .is_err()
        );
        assert_eq!(
            digest(&saved).unwrap(),
            digest(&journal.load().await.unwrap().unwrap()).unwrap()
        );
        let result = agent::run(&input(), &config(), &journal, &model, &cancel)
            .await
            .unwrap();
        assert_eq!(result.quality, "verified");
        assert_eq!(model.bodies.lock().unwrap().len(), expected_calls);
        assert!(model.calls.lock().unwrap().is_empty());
        assert_eq!(
            journal.reservations.lock().unwrap()[&0].1,
            if boundary == 1 { 2 } else { 1 }
        );
        let completed = journal.load().await.unwrap().unwrap();
        assert!(completed.journal.pending.is_none());
        assert!(completed.journal.sequence > completed.turn);
    }
}

#[tokio::test]
async fn cancellation_during_durable_boundary_stops_the_next_io_and_resumes_exactly() {
    for boundary in [1, 2] {
        let journal = MemoryJournal::default();
        let cancel = CancellationToken::new();
        *journal.cancel_boundary.lock().unwrap() = Some((boundary, cancel.clone()));
        let model = script();
        let expected_calls = model.calls.lock().unwrap().len();
        let error = agent::run(&input(), &config(), &journal, &model, &cancel)
            .await
            .unwrap_err();
        assert_eq!(error.code, "INTERNAL");
        let saved = journal.load().await.unwrap().unwrap();
        assert_eq!(saved.journal.sequence, boundary);
        assert_eq!(saved.journal.response().is_some(), boundary == 2);
        assert_eq!(saved.turn, 0);
        assert_eq!(saved.tool_calls, 0);
        assert!(saved.main_work.is_none());
        assert_eq!(
            model.bodies.lock().unwrap().len(),
            usize::from(boundary == 2)
        );
        let result = agent::run(
            &input(),
            &config(),
            &journal,
            &model,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(result.quality, "verified");
        assert_eq!(model.bodies.lock().unwrap().len(), expected_calls);
        assert_eq!(
            journal.reservations.lock().unwrap()[&0].1,
            if boundary == 1 { 2 } else { 1 }
        );
    }
}

#[tokio::test]
async fn rejected_prepared_transaction_never_calls_the_extraction_model() {
    let journal = MemoryJournal::default();
    *journal.reject_reservation.lock().unwrap() = true;
    let model = script();
    assert!(
        agent::run(
            &input(),
            &config(),
            &journal,
            &model,
            &CancellationToken::new()
        )
        .await
        .is_err()
    );
    assert!(model.bodies.lock().unwrap().is_empty());
    assert!(journal.load().await.unwrap().is_none());
    assert!(journal.reservations.lock().unwrap().is_empty());
}
