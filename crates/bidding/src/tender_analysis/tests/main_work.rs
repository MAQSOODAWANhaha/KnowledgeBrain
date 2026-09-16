use super::*;

fn packet(body: &Value) -> Value {
    serde_json::from_str(
        body["messages"].as_array().unwrap().last().unwrap()["content"]
            .as_str()
            .unwrap(),
    )
    .unwrap()
}

async fn prepared(input: &FrozenInput, config: &Config) -> (MemoryJournal, Checkpoint, Value) {
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
    (journal, state, body)
}

struct ExtractPackage {
    bodies: Mutex<Vec<Vec<u8>>>,
}

#[async_trait]
impl Model for ExtractPackage {
    async fn turn(&self, _: &Config, bytes: &[u8]) -> Result<ChatTurn, AgentError> {
        self.bodies.lock().unwrap().push(bytes.to_vec());
        let body: Value = serde_json::from_slice(bytes).unwrap();
        let sent = packet(&body)["preloaded_evidence"].clone();
        let mut calls = vec![];
        for item in sent["assigned_evidence"]["boundary_evidence"]
            .as_array()
            .unwrap()
        {
            if item["tool"] != "read_source" {
                continue;
            }
            let source = &item["source"];
            assert_eq!(source["end"], source["total_bytes"]);
            let source_id = source["source_id"].as_str().unwrap();
            assert!(
                sent["main_work"]["source_scope"]
                    .as_array()
                    .unwrap()
                    .contains(&json!(source_id))
            );
            calls.push(("put_record", json!({"id":null,"sources":[{
                "source_id":source_id,"start":source["start"],"end":source["end"]
            }],"data":{"kind":"fact","name":"Background","value":source["text"],"scope":"Source background"}})));
            calls.push(("set_disposition", json!({"source_id":source_id,
                "state":"non_requirement","reason":"Background source has no submission obligation."})));
        }
        assert!(!calls.is_empty());
        Ok(ChatTurn {
            content: String::new(),
            finish_reason: "tool_calls".into(),
            usage: None,
            tool_calls: calls
                .into_iter()
                .enumerate()
                .map(|(index, (name, args))| ChatToolCall {
                    id: format!("package-{index}"),
                    name: name.into(),
                    arguments: args.to_string(),
                })
                .collect(),
        })
    }
}

fn background() -> FrozenInput {
    let mut input = input();
    input.source_units[0].text = "Project background information.".into();
    input
}

#[tokio::test]
async fn initial_package_is_pure_and_its_first_response_can_write_and_handoff() {
    let input = background();
    let config = config();
    let (journal, before, body) = prepared(&input, &config).await;
    assert!(before.main_work.is_none());
    assert_eq!(before.read_bytes, 0);
    assert!(before.analysis.coverage.text.is_empty());
    assert!(before.analysis.records.is_empty());
    assert!(packet(&body)["preloaded_evidence"]["main_work"].is_object());
    *journal.interrupt_after.lock().unwrap() = Some(1);
    let model = ExtractPackage {
        bodies: Mutex::new(vec![]),
    };
    let error = agent::run(&input, &config, &journal, &model, &CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(error.code, "INTERNAL");
    let state = journal.load().await.unwrap().unwrap();
    assert_eq!(state.role, Role::Reviewer);
    assert_eq!(state.turn, 1);
    assert_eq!(state.tool_calls, 2);
    assert_eq!(state.analysis.records.len(), 1);
    assert!(tools::gaps(&input, &state.analysis).is_empty());
    assert!(state.reviewer_coverage.text.is_empty());
    assert_eq!(
        model.bodies.lock().unwrap().as_slice(),
        &[before.journal.body().unwrap().to_vec()]
    );
    assert!(!state.done);
}

#[tokio::test]
async fn received_main_package_recovers_without_recall_or_double_delivery() {
    let input = background();
    let config = config();
    let (journal, before, _) = prepared(&input, &config).await;
    let model = ExtractPackage {
        bodies: Mutex::new(vec![]),
    };
    *journal.fail_boundary_ack.lock().unwrap() = Some(2);
    let error = agent::run(&input, &config, &journal, &model, &CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(error.code, "INTERNAL");
    let received = journal.load().await.unwrap().unwrap();
    assert!(received.journal.response().is_some());
    assert!(received.main_work.is_none());
    assert!(received.analysis.coverage.text.is_empty());
    assert_eq!(received.read_bytes, before.read_bytes);
    let reservations = journal.reservations.lock().unwrap().clone();
    *journal.state.lock().unwrap() = Some(serde_json::from_value(json!(received)).unwrap());
    *journal.interrupt_after.lock().unwrap() = Some(1);
    let unused = work_script(vec![]);
    let error = agent::run(
        &input,
        &config,
        &journal,
        &unused,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "INTERNAL");
    assert!(unused.bodies.lock().unwrap().is_empty());
    assert_eq!(*journal.reservations.lock().unwrap(), reservations);
    let saved = journal.load().await.unwrap().unwrap();
    assert_eq!(saved.turn, 1);
    assert_eq!(saved.tool_calls, 2);
    assert_eq!(saved.role, Role::Reviewer);
    assert_eq!(saved.analysis.records.len(), 1);
    assert_eq!(model.bodies.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn completed_packages_collectively_handoff_without_global_scope_declaration() {
    let mut input = background();
    input.source_units.push(Source {
        source_unit_revision_id: "other".into(),
        document_id: "another-document".into(),
        ordinal: 1,
        ..input.source_units[0].clone()
    });
    let config = config();
    let journal = MemoryJournal::default();
    let model = ExtractPackage {
        bodies: Mutex::new(vec![]),
    };
    for turn in [1, 2] {
        *journal.interrupt_after.lock().unwrap() = Some(turn);
        let error = agent::run(&input, &config, &journal, &model, &CancellationToken::new())
            .await
            .unwrap_err();
        assert_eq!(error.code, "INTERNAL");
        let state = journal.load().await.unwrap().unwrap();
        assert_eq!(state.turn, turn);
        assert_eq!(state.main_work.as_ref().unwrap().source_scope.len(), 1);
        assert_eq!(state.analysis.dispositions.len(), turn);
        assert_eq!(
            state.role,
            if turn == 1 {
                Role::Main
            } else {
                Role::Reviewer
            }
        );
    }
    assert_eq!(model.bodies.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn partial_package_confirms_only_its_actual_text_and_grid_ranges() {
    let mut input = grid_citation_input();
    input.structured_forms[0]["definition"]["schema_version"] = json!(3);
    input.source_units[0].text = "Long source text. ".repeat(4000);
    let mut config = config();
    config.limits.max_tool_result_bytes = 4000;
    let (_, mut state, body) = prepared(&input, &config).await;
    let sent = packet(&body)["preloaded_evidence"].clone();
    assert!(serde_json::to_vec(&sent).unwrap().len() <= config.limits.max_tool_result_bytes);
    agent::evidence_delivery::confirm(&input, &config, &mut state, &body).unwrap();
    assert!(!tools::reading_gaps(&input, &state.analysis.coverage).is_empty());
    assert!(!tools::contains(
        state.analysis.coverage.text.get("source"),
        0,
        input.source_units[0].text.len()
    ));
    assert!(state.analysis.dispositions.is_empty());
    assert_eq!(
        state.main_work.as_ref().unwrap().status,
        agent::context::WorkStatus::Active
    );
    assert!(state.reviewer_coverage.text.is_empty());
}

#[tokio::test]
async fn changed_package_cannot_install_scope_or_receipts() {
    let input = background();
    let config = config();
    let (_, mut state, mut body) = prepared(&input, &config).await;
    let before = digest(&state).unwrap();
    let mut changed = packet(&body);
    changed["preloaded_evidence"]["main_work"]["source_scope"] = json!(["not-frozen"]);
    body["messages"].as_array_mut().unwrap().last_mut().unwrap()["content"] =
        json!(changed.to_string());
    assert!(agent::evidence_delivery::confirm(&input, &config, &mut state, &body).is_err());
    assert_eq!(digest(&state).unwrap(), before);
}

#[tokio::test]
async fn delivered_grid_and_metadata_are_real_receipts_without_semantic_approval() {
    let mut input = grid_citation_input();
    input.structured_forms[0]["definition"]["schema_version"] = json!(3);
    input
        .documents
        .push(json!({"document_id":"document","title":"Fixture source"}));
    let config = config();
    let (_, mut state, body) = prepared(&input, &config).await;
    agent::evidence_delivery::confirm(&input, &config, &mut state, &body).unwrap();
    assert!(tools::reading_gaps(&input, &state.analysis.coverage).is_empty());
    assert!(tools::contains(
        state.analysis.coverage.form_cells.get("grid"),
        0,
        4
    ));
    assert!(tools::contains(
        state.analysis.coverage.metadata.get("documents"),
        0,
        1
    ));
    assert!(state.analysis.dispositions.is_empty());
    assert!(state.source_review.is_none());
    assert!(state.reviewer_coverage.form_cells.is_empty());
}

#[tokio::test]
async fn deferred_scope_and_repair_cannot_be_replaced_by_a_normal_package() {
    let mut input = background();
    input.source_units.push(Source {
        source_unit_revision_id: "other".into(),
        document_id: "another-document".into(),
        ordinal: 1,
        ..input.source_units[0].clone()
    });
    let config = config();
    let (_, mut state, body) = prepared(&input, &config).await;
    agent::evidence_delivery::confirm(&input, &config, &mut state, &body).unwrap();
    state
        .main_work
        .as_mut()
        .unwrap()
        .deferred_sources
        .push("other".into());
    assert!(
        agent::evidence_delivery::select(&input, &config, &state)
            .unwrap()
            .is_none()
    );
    state.main_work = None;
    state.source_review = Some(source_review::initialize(&input, &config).unwrap());
    assert!(
        agent::evidence_delivery::select(&input, &config, &state)
            .unwrap()
            .is_none()
    );
}

struct FailedPackage(ExtractPackage);
#[async_trait]
impl Model for FailedPackage {
    async fn turn(&self, config: &Config, bytes: &[u8]) -> Result<ChatTurn, AgentError> {
        let mut response = self.0.turn(config, bytes).await?;
        response.tool_calls.push(ChatToolCall {
            id: "failed-after-disposition".into(),
            name: "put_record".into(),
            arguments: "{}".into(),
        });
        Ok(response)
    }
}

#[tokio::test]
async fn a_failed_later_tool_cannot_complete_the_package() {
    let input = background();
    let config = config();
    let journal = MemoryJournal::default();
    *journal.interrupt_after.lock().unwrap() = Some(1);
    let model = FailedPackage(ExtractPackage {
        bodies: Mutex::new(vec![]),
    });
    let error = agent::run(&input, &config, &journal, &model, &CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(error.code, "INTERNAL");
    let saved = journal.load().await.unwrap().unwrap();
    assert_eq!(saved.role, Role::Main);
    assert_eq!(
        saved.main_work.as_ref().unwrap().status,
        agent::context::WorkStatus::Active
    );
    assert_eq!(
        saved.analysis.records.len(),
        1,
        "valid incremental writes survive"
    );
    assert_eq!(saved.analysis.dispositions.len(), 1);
    assert!(saved.source_review.is_none());
}

#[tokio::test]
async fn output_ceiling_also_bounds_main_evidence_workload() {
    let mut input = background();
    input.source_units[0].text = "Dense original clause. ".repeat(4000);
    let mut config = config();
    config.limits.max_tool_result_bytes = 48_000;
    let (_, state, body) = prepared(&input, &config).await;
    let sent = packet(&body)["preloaded_evidence"].clone();
    assert!(sent.is_object());
    assert!(serde_json::to_vec(&sent).unwrap().len() <= config.provider.max_tokens as usize);
    assert_eq!(
        sent["assigned_evidence"]["workload_bytes_limit"],
        config.provider.max_tokens
    );
    assert!(state.analysis.coverage.text.is_empty());
}

#[tokio::test]
async fn remaining_metadata_continues_after_all_sources_are_disposed() {
    let mut input = background();
    input.documents = (0..24)
        .map(|index| {
            json!({"document_id":format!("metadata-{index}"),
        "description":"Original frozen metadata. ".repeat(24)})
        })
        .collect();
    let config = config();
    let (_, mut state, body) = prepared(&input, &config).await;
    agent::evidence_delivery::confirm(&input, &config, &mut state, &body).unwrap();
    let mut coverage = state.analysis.coverage.clone();
    tools::invoke(
        &input,
        &mut state.analysis,
        &mut coverage,
        false,
        "set_disposition",
        &json!({"source_id":"source","state":"non_requirement","reason":"Background only."}),
        config.limits.max_tool_result_bytes,
    )
    .unwrap();
    state.main_work.as_mut().unwrap().status = agent::context::WorkStatus::Complete;
    let work = json!(state.main_work);
    assert!(!tools::contains(
        state.analysis.coverage.metadata.get("documents"),
        0,
        input.documents.len()
    ));
    for _ in 0..input.documents.len() {
        if tools::reading_gaps(&input, &state.analysis.coverage).is_empty() {
            break;
        }
        let before = json!(state.analysis.coverage);
        let body: Value =
            serde_json::from_slice(&agent::request(&input, &config, &mut state).await.unwrap())
                .unwrap();
        let sent = packet(&body)["preloaded_evidence"].clone();
        assert!(sent.is_object());
        assert!(sent.get("main_work").is_none());
        assert_eq!(json!(state.main_work), work);
        assert_eq!(
            json!(state.analysis.coverage),
            before,
            "preparation is not delivery"
        );
        agent::evidence_delivery::confirm(&input, &config, &mut state, &body).unwrap();
        assert_eq!(json!(state.main_work), work);
        assert_ne!(json!(state.analysis.coverage), before);
    }
    assert!(tools::reading_gaps(&input, &state.analysis.coverage).is_empty());
    assert_eq!(
        state.role,
        Role::Main,
        "metadata delivery is not semantic approval"
    );
}

async fn existing_candidates() -> (FrozenInput, Config, Checkpoint, Vec<String>) {
    let (input, analysis, ids) = analysis_query_fixture();
    let mut config = config();
    config.provider.max_tokens = 16_000;
    let (_, mut state, _) = prepared(&input, &config).await;
    state.analysis = analysis;
    state.analysis.coverage = Coverage::default();
    state.main_work = Some(serde_json::from_value(active_work("source")).unwrap());
    state.transcript.clear();
    (input, config, state, ids)
}

#[tokio::test]
async fn main_candidates_include_complete_relation_endpoints_with_exact_receipts() {
    let (input, config, mut state, ids) = existing_candidates().await;
    let body: Value =
        serde_json::from_slice(&agent::request(&input, &config, &mut state).await.unwrap())
            .unwrap();
    let sent = packet(&body)["preloaded_evidence"].clone();
    let candidates = sent["assigned_evidence"]["candidates"].as_array().unwrap();
    for reference in [
        format!("record:{}", ids[0]),
        format!("record:{}", ids[1]),
        format!("relation:{}", ids[3]),
    ] {
        let candidate = candidates
            .iter()
            .find(|candidate| candidate["reference"] == reference)
            .unwrap();
        let value = agent::context::reference(&state.analysis, &reference).unwrap();
        assert_eq!(candidate["value"], value);
        assert_eq!(candidate["sha256"], digest(&value).unwrap());
        assert!(!state.analysis.coverage.candidate.contains_key(&reference));
    }
    agent::evidence_delivery::confirm(&input, &config, &mut state, &body).unwrap();
    assert!(
        state
            .analysis
            .coverage
            .candidate
            .contains_key(&format!("record:{}", ids[1]))
    );
    assert!(
        !state.analysis.coverage.text.contains_key("other-source"),
        "endpoint detail is not remote original evidence"
    );
    assert!(state.reviewer_coverage.candidate.is_empty());
}

#[tokio::test]
async fn oversized_candidate_group_is_explicit_and_has_legal_exact_lookup_ids() {
    let (input, config, mut state, ids) = existing_candidates().await;
    let record = state.analysis.records.get_mut(&ids[1]).unwrap();
    let RecordData::Fact { value, .. } = &mut record.data else {
        panic!("fact fixture");
    };
    *value = "Large retained candidate. ".repeat(3000);
    let body: Value =
        serde_json::from_slice(&agent::request(&input, &config, &mut state).await.unwrap())
            .unwrap();
    let sent = packet(&body)["preloaded_evidence"].clone();
    let delivery = &sent["assigned_evidence"]["candidate_delivery"];
    assert_eq!(delivery["complete"], false);
    let query = &delivery["next_inspection"];
    assert!(query["ids"].as_array().unwrap().contains(&json!(ids[1])));
    assert!(query["ids"].as_array().unwrap().contains(&json!(ids[3])));
    let candidates = sent["assigned_evidence"]["candidates"].as_array().unwrap();
    assert!(
        !candidates
            .iter()
            .any(|candidate| candidate["reference"] == format!("relation:{}", ids[3]))
    );
    assert!(
        !candidates
            .iter()
            .any(|candidate| candidate["reference"] == format!("record:{}", ids[1]))
    );
    let mut coverage = state.analysis.coverage.clone();
    tools::inspect_analysis(
        &input,
        &state.analysis,
        &mut coverage,
        &state.analysis.coverage,
        query,
        200_000,
        None,
    )
    .unwrap();
    agent::evidence_delivery::confirm(&input, &config, &mut state, &body).unwrap();
    assert!(
        !state
            .analysis
            .coverage
            .candidate
            .contains_key(&format!("record:{}", ids[1]))
    );
    assert!(
        !state
            .analysis
            .coverage
            .candidate
            .contains_key(&format!("relation:{}", ids[3]))
    );
}

#[tokio::test]
async fn retained_candidates_do_not_suppress_new_metadata_in_an_active_package() {
    let (mut input, config, mut state, _) = existing_candidates().await;
    input.documents = (0..40)
        .map(|index| {
            json!({"document_id":format!("metadata-{index}"),
        "description":"Original frozen metadata. ".repeat(100)})
        })
        .collect();
    let first: Value =
        serde_json::from_slice(&agent::request(&input, &config, &mut state).await.unwrap())
            .unwrap();
    agent::evidence_delivery::confirm(&input, &config, &mut state, &first).unwrap();
    assert!(!tools::contains(
        state.analysis.coverage.metadata.get("documents"),
        0,
        input.documents.len()
    ));
    assert!(!state.analysis.coverage.candidate.is_empty());
    let before = json!(state.analysis.coverage);
    let second: Value =
        serde_json::from_slice(&agent::request(&input, &config, &mut state).await.unwrap())
            .unwrap();
    let sent = packet(&second)["preloaded_evidence"].clone();
    assert!(
        sent.is_object(),
        "retained candidates must not hide unread collection pages"
    );
    assert!(
        sent["assigned_evidence"]["boundary_evidence"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["tool"] == "collection_index"
                && item["arguments"]["offset"].as_u64().unwrap() > 0)
    );
    assert_eq!(
        json!(state.analysis.coverage),
        before,
        "preparing the second page grants no receipt"
    );
    let mut restored: Checkpoint = serde_json::from_value(json!(state)).unwrap();
    agent::evidence_delivery::confirm(&input, &config, &mut restored, &second).unwrap();
    agent::evidence_delivery::confirm(&input, &config, &mut state, &second).unwrap();
    assert_eq!(
        digest(&restored).unwrap(),
        digest(&state).unwrap(),
        "received replay confirms the same reserved page"
    );
    assert_ne!(json!(state.analysis.coverage.metadata), before["metadata"]);
    assert_eq!(
        json!(state.analysis.coverage.candidate),
        before["candidate"]
    );
    assert!(state.reviewer_coverage.metadata.is_empty());
}
