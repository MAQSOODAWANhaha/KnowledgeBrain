use super::*;

struct ReviewFromReservedEvidence {
    original: String,
    bodies: Mutex<Vec<Vec<u8>>>,
}

#[async_trait]
impl Model for ReviewFromReservedEvidence {
    async fn turn(&self, _: &Config, bytes: &[u8]) -> Result<ChatTurn, AgentError> {
        let mut bodies = self.bodies.lock().unwrap();
        assert!(
            bodies.is_empty(),
            "received/committed recovery must not call the model again"
        );
        bodies.push(bytes.to_vec());
        let body: Value = serde_json::from_slice(bytes).unwrap();
        let packet: Value = serde_json::from_str(
            body["messages"].as_array().unwrap().last().unwrap()["content"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            packet["preloaded_evidence"]["assigned_evidence"]["source"]["text"],
            self.original
        );
        assert_eq!(
            packet["preloaded_evidence"]["assigned_evidence"]["candidate_delivery"]["complete"],
            true
        );
        Ok(ChatTurn {
            content: String::new(),
            finish_reason: "tool_calls".into(),
            usage: None,
            tool_calls: fixture_review_calls(
                &body,
                "fixture_global_checks",
                &json!({"grounds":[{"source_id":"source","start":0,"end":self.original.len()}]}),
            )
            .unwrap()
            .into_iter()
            .chain(fixture_review_calls(&body, "put_source_review", &json!({})).unwrap())
            .collect(),
        })
    }
}

async fn ready() -> (FrozenInput, MemoryJournal, Checkpoint) {
    let mut input = input();
    input.source_units[0].text = "这是项目背景介绍，没有投标响应要求。".into();
    let journal = MemoryJournal::default();
    *journal.interrupt_after.lock().unwrap() = Some(4);
    let main = work_script(vec![
        ("set_work_note", active_work("source")),
        (
            "read_source",
            json!({"source_id":"source","start":0,"max_bytes":1024}),
        ),
        (
            "set_disposition",
            json!({"source_id":"source","state":"non_requirement","reason":"Synthetic background paragraph."}),
        ),
        (
            "fixture_global_checks",
            json!({"grounds":[{"source_id":"source","start":0,"end":input.source_units[0].text.len()}]}),
        ),
    ]);
    let error = agent::run(
        &input,
        &config(),
        &journal,
        &main,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "INTERNAL");
    let state = journal.load().await.unwrap().unwrap();
    assert_eq!(state.role, Role::Reviewer);
    assert!(state.reviewer_coverage.text.is_empty());
    assert!(state.reviewer_coverage.candidate.is_empty());
    assert!(state.pending_coverage.is_none());
    assert!(
        !state.analysis.coverage.text.is_empty(),
        "Main reading must not confer independent reviewer delivery"
    );
    (input, journal, state)
}

#[tokio::test]
async fn preloaded_evidence_survives_all_three_journal_boundaries_without_double_delivery() {
    for boundary in [1, 2, 3] {
        let (input, journal, before) = ready().await;
        let config = config();
        let model = ReviewFromReservedEvidence {
            original: input.source_units[0].text.clone(),
            bodies: Mutex::new(vec![]),
        };
        *journal.fail_boundary_ack.lock().unwrap() = Some(before.journal.sequence + boundary);
        let error = agent::run(&input, &config, &journal, &model, &CancellationToken::new())
            .await
            .unwrap_err();
        assert_eq!(error.code, "INTERNAL", "boundary {boundary}: {error:?}");
        let interrupted = journal.load().await.unwrap().unwrap();
        assert_eq!(
            interrupted.journal.sequence,
            before.journal.sequence + boundary
        );
        let reserved = journal.reservations.lock().unwrap()[&before.turn].0.clone();
        let body: Value = serde_json::from_slice(&reserved).unwrap();
        let packet: Value = serde_json::from_str(
            body["messages"].as_array().unwrap().last().unwrap()["content"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        let sent = &packet["preloaded_evidence"];
        assert_eq!(
            sent["assigned_evidence"]["source"]["text"],
            input.source_units[0].text
        );
        if boundary < 3 {
            assert_eq!(interrupted.read_bytes, before.read_bytes);
            assert_eq!(
                json!(interrupted.reviewer_coverage),
                json!(before.reviewer_coverage)
            );
            assert_eq!(interrupted.tool_calls, before.tool_calls);
            assert_eq!(interrupted.journal.response().is_some(), boundary == 2);
            assert_eq!(interrupted.journal.body().unwrap(), reserved);
            assert!(!interrupted.done);
        } else {
            assert!(interrupted.done);
            assert!(interrupted.journal.pending.is_none());
        }
        assert_eq!(
            model.bodies.lock().unwrap().len(),
            usize::from(boundary != 1)
        );
        let result = agent::run(&input, &config, &journal, &model, &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(result.quality, "verified");
        let finished = journal.load().await.unwrap().unwrap();
        assert!(finished.done);
        assert_eq!(model.bodies.lock().unwrap().as_slice(), &[reserved]);
        assert_eq!(
            json!(finished.analysis.coverage),
            json!(before.analysis.coverage)
        );
        assert!(tools::reading_gaps(&input, &finished.reviewer_coverage).is_empty());
        assert!(
            finished
                .reviewer_coverage
                .candidate
                .contains_key("disposition:source")
        );
        assert!(finished.reviewer_coverage.views.is_empty());
        let output_bytes: usize = finished
            .transcript
            .iter()
            .filter(|m| m["role"] == "tool")
            .map(|m| m["content"].as_str().unwrap().len())
            .sum();
        assert_eq!(
            finished.read_bytes,
            before.read_bytes + serde_json::to_vec(sent).unwrap().len() + output_bytes,
            "exactly one evidence delivery plus actual tool outputs must be charged"
        );
        let final_digest = digest(&finished).unwrap();
        agent::run(&input, &config, &journal, &model, &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(
            digest(&journal.load().await.unwrap().unwrap()).unwrap(),
            final_digest
        );
        assert_eq!(model.bodies.lock().unwrap().len(), 1);
    }
}
