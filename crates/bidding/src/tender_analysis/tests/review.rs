use super::*;

#[tokio::test]
async fn overflowing_review_writes_are_rejected_atomically_and_fitted_checks_commit() {
    struct BatchChecks(Vec<String>);
    #[async_trait]
    impl Model for BatchChecks {
        async fn turn(&self, _: &Config, _: &[u8]) -> Result<ChatTurn, AgentError> {
            Ok(ChatTurn {
                usage: None,
                content: String::new(),
                finish_reason: "tool_calls".into(),
                tool_calls: self.0.iter().enumerate().map(|(i, reference)| ChatToolCall {
                    id: format!("check-{i}"),
                    name: "complete_review_check".into(),
                    arguments: json!({"reference":reference,"summary":"comparison ".repeat(700),"sources":[span()]}).to_string(),
                }).collect(),
            })
        }
    }
    let mut config = config();
    let journal = fresh_review_journal().await;
    let mut state = journal.load().await.unwrap().unwrap();
    let record = state.analysis.records.values().next().unwrap().clone();
    let mut references = Vec::new();
    state.reviewer_coverage = state.analysis.coverage.clone();
    for i in 0..6 {
        let mut record = record.clone();
        record.id = format!("overflow-{i}");
        let reference = format!("record:{}", record.id);
        state
            .reviewer_coverage
            .candidate
            .insert(reference.clone(), digest(&record).unwrap());
        state.analysis.records.insert(record.id.clone(), record);
        references.push(reference);
    }
    state.reviewer_work.as_mut().unwrap().focus.references = references.clone();
    // Size the boundary from the actual request and batch, so evolving tool
    // descriptions do not turn this partial-admission test into all-rejected.
    let baseline = agent::request(&input(), &config, &mut state.clone())
        .await
        .unwrap()
        .len();
    let batch = BatchChecks(references.clone())
        .turn(&config, &[])
        .await
        .unwrap();
    let arguments_bytes: usize = batch
        .tool_calls
        .iter()
        .map(|call| call.arguments.len())
        .sum();
    config.limits.max_context_bytes = baseline + arguments_bytes + arguments_bytes / 2;
    state.config_sha256 = digest(&config).unwrap();
    let before = state.clone();
    *journal.state.lock().unwrap() = Some(state);
    *journal.interrupt_after.lock().unwrap() = Some(before.turn + 1);
    let error = agent::run(
        &input(),
        &config,
        &journal,
        &BatchChecks(references),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(
        error.code, "INTERNAL",
        "the batch must commit before the injected stop"
    );
    let saved = journal.load().await.unwrap().unwrap();
    assert_eq!(saved.turn, before.turn + 1);
    assert_eq!(saved.tool_calls, before.tool_calls + 6);
    assert_eq!(json!(saved.analysis), json!(before.analysis));
    let mut accepted = 0;
    let mut rejected = 0;
    let mut projected = saved.clone();
    let wire: Value = serde_json::from_slice(
        &agent::request(&input(), &config, &mut projected)
            .await
            .unwrap(),
    )
    .unwrap();
    let packet: Value = serde_json::from_str(
        wire["messages"].as_array().unwrap().last().unwrap()["content"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let pending = packet["source_review"]["current"]["pending_candidate_refs"]["items"]
        .as_array()
        .unwrap();
    for message in saved.transcript.iter().filter(|m| m["role"] == "tool") {
        let output: Value = serde_json::from_str(message["content"].as_str().unwrap()).unwrap();
        if output["ok"] == true {
            accepted += 1;
            assert!(
                saved
                    .reviewer_progress
                    .seen
                    .contains(output["result"]["completion_sha256"].as_str().unwrap())
            );
        } else {
            rejected += 1;
            assert!(
                output["error"]
                    .as_str()
                    .unwrap()
                    .contains("remaining batch context")
            );
            let index = message["tool_call_id"]
                .as_str()
                .unwrap()
                .strip_prefix("check-")
                .unwrap();
            assert!(
                pending.contains(&json!(format!("record:overflow-{index}"))),
                "rejected write must remain pending in the next model request"
            );
        }
    }
    assert!(
        accepted > 0 && rejected > 0,
        "accepted={accepted}, rejected={rejected}"
    );
    assert_eq!(accepted + rejected, 6);
    assert_eq!(saved.reviewer_progress.watch.no_progress_turns, 0);
}

#[tokio::test]
async fn review_findings_survive_handoff_ack_loss_and_are_finalized_from_storage() {
    let mut config = config();
    // This fixture includes an earlier full repair cycle and then an ACK-loss
    // review; freeze enough test calls for both before creating the journal.
    config.limits.max_turns = 40;
    config.limits.max_tool_calls = 80;
    let journal = fresh_review_journal_config(&config).await;
    let start = journal.load().await.unwrap().unwrap().turn;
    let finding = json!({"code":"FIELD_MISMATCH","message":"earlier saved field issue",
        "correction":"按所引原文补全并重新核对该字段", "affected":[],"sources":[span()]});
    *journal.interrupt_after.lock().unwrap() = Some(start + 3);
    let model = work_script(vec![
        ("set_work_note", active_work("source")),
        (
            "read_source",
            json!({"source_id":"source","start":0,"max_bytes":1024}),
        ),
        ("put_review_finding", json!({"id":null,"finding":finding})),
    ]);
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
    let id = saved.review_draft.keys().next().unwrap().clone();
    let restored: Checkpoint =
        serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
    *journal.state.lock().unwrap() = Some(restored);
    assert!(saved.analysis.relations.is_empty());
    let mut complete = active_work("source");
    complete["status"] = json!("complete");
    *journal.interrupt_after.lock().unwrap() = Some(start + 6);
    let model = work_script(vec![
        (
            "inspect_analysis",
            json!({"view":"detail","kind":"all","offset":0,"limit":10}),
        ),
        (
            "inspect_analysis",
            json!({"view":"detail","kind":"disposition","offset":0,"limit":10}),
        ),
        ("set_work_note", complete),
    ]);
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
    let handed_off = journal.load().await.unwrap().unwrap();
    assert_eq!(
        handed_off.review_draft[&id].message,
        "earlier saved field issue"
    );
    assert!(
        !serde_json::to_string(&handed_off.transcript)
            .unwrap()
            .contains("earlier saved field issue"),
        "handoff was not completed: {:?}",
        handed_off.transcript.last()
    );
    assert!(handed_off.read_bytes >= saved.read_bytes);
    let mut revised = finding;
    revised["message"] = json!("revised field correction");
    let model = work_script(vec![
        ("set_work_note", active_work("source")),
        ("inspect_review", json!({"offset":0,"limit":1})),
        // The retired bulk payload must not clear a persisted finding.
        ("put_source_review", json!({"findings":[]})),
        ("put_review_finding", json!({"id":id,"finding":revised})),
        ("put_source_review", json!({"fixture_status":"findings"})),
    ]);
    let result = agent::run(
        &input(),
        &config,
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(result.review.findings.len(), 1);
    assert_eq!(
        result.review.findings[0].message,
        "revised field correction"
    );
    assert_ne!(result.quality, "verified");
    assert!(journal.load().await.unwrap().unwrap().review_draft.len() == 1);
    let bodies = model.bodies.lock().unwrap();
    assert!(bodies[2].to_string().contains(&id));
    assert!(bodies[3].to_string().contains("unknown field"));
}

#[tokio::test]
async fn read_receipts_and_empty_submission_cannot_attest_source_omissions() {
    let journal = fresh_review_journal().await;
    let start = journal.load().await.unwrap().unwrap().turn;
    let calls = vec![
        ("set_work_note", active_work("source")),
        (
            "read_source",
            json!({"source_id":"source","start":0,"max_bytes":1024}),
        ),
        (
            "inspect_analysis",
            json!({"kind":"all","view":"detail","offset":0,"limit":10}),
        ),
        (
            "inspect_analysis",
            json!({"kind":"disposition","view":"detail","offset":0,"limit":10}),
        ),
        ("submit_review", json!({})),
    ];
    *journal.interrupt_after.lock().unwrap() = Some(start + calls.len());
    let _ = agent::run(
        &input(),
        &config(),
        &journal,
        &work_script(calls),
        &CancellationToken::new(),
    )
    .await;
    let saved = journal.load().await.unwrap().unwrap();
    assert!(
        !saved.done,
        "reading coverage is not an independent omission judgment"
    );
    assert!(saved.review.is_none());
}

#[tokio::test]
async fn finding_page_shrinks_before_evicting_the_original_under_review() {
    let journal = fresh_review_journal().await;
    let mut state = journal.load().await.unwrap().unwrap();
    let mut config = config();
    state.reviewer_work = Some(serde_json::from_value(active_work("source")).unwrap());
    let evidence = tools::invoke(
        &input(),
        &mut state.analysis,
        &mut state.reviewer_coverage,
        true,
        "read_source",
        &json!({"source_id":"source","start":0,"max_bytes":1024}),
        config.limits.max_tool_result_bytes,
    )
    .unwrap();
    for index in 0..10 {
        state.review_draft.insert(
            format!("finding-{index}"),
            serde_json::from_value(json!({
                "code":"field_issue", "message":"Source mismatch. ".repeat(60),
                "correction":"Compare the source", "affected":[], "sources":[span()]
            }))
            .unwrap(),
        );
    }
    let args = json!({"offset":0,"limit":10});
    let call = ChatToolCall {
        id: "recall".into(),
        name: "inspect_review".into(),
        arguments: args.to_string(),
    };
    state.transcript = vec![
        json!({"role":"assistant","tool_calls":[{"id":"source-read","type":"function","function":{"name":"read_source","arguments":json!({"source_id":"source","start":0,"max_bytes":1024}).to_string()}}]}),
        json!({"role":"tool","tool_call_id":"source-read","content":json!({"ok":true,"result":evidence}).to_string()}),
        json!({"role":"assistant","tool_calls":[{"id":call.id,"type":"function","function":{"name":call.name,"arguments":call.arguments}}]}),
    ];
    let full_page =
        agent::inspect_review(&state, &args, config.limits.max_tool_result_bytes).unwrap();
    let mut probe = state.clone();
    probe
        .transcript
        .push(json!({"role":"tool","tool_call_id":call.id,
        "content":json!({"ok":true,"result":full_page}).to_string()}));
    let full_request = agent::request(&input(), &config, &mut probe).await.unwrap();
    // Leave enough room for only part of this complete-finding page, measured
    // from the actual request instead of depending on prompt/schema lengths.
    config.limits.max_context_bytes =
        full_request.len() - serde_json::to_vec(&full_page).unwrap().len() / 2;
    let original = serde_json::to_value(&state).unwrap();
    let coverage = state.reviewer_coverage.clone();
    let page = agent::inspect_in_context(
        &input(),
        &config,
        &mut state,
        &args,
        std::slice::from_ref(&call),
        &[],
        &coverage,
    )
    .await
    .unwrap();
    assert_eq!(page["total"], 10);
    let count = page["items"].as_array().unwrap().len();
    assert!(count > 0 && count < 10);
    assert_eq!(page["next"], count);
    assert_eq!(
        page["items"][0]["finding"],
        json!(state.review_draft["finding-0"])
    );
    assert_eq!(
        serde_json::to_value(&state).unwrap(),
        original,
        "projection cannot mutate findings, counters, reading receipts or source judgments"
    );
}

#[tokio::test]
async fn inspect_review_returns_known_draft_ids_without_scanning_or_mutating_state() {
    let journal = fresh_review_journal().await;
    let mut state = journal.load().await.unwrap().unwrap();
    for id in ["first", "middle", "last"] {
        state.review_draft.insert(
            id.into(),
            serde_json::from_value(json!({
                "code":"missing_mapping", "message":format!("{id} issue"),
                "correction":"Compare the prescribed form and its requirement", "affected":[],
                "sources":[span()]
            }))
            .unwrap(),
        );
    }
    let args = json!({"ids":["last","first"],"offset":0,"limit":10});
    let schema = tools::schemas(true)
        .into_iter()
        .find(|tool| tool["function"]["name"] == "inspect_review")
        .unwrap();
    assert!(
        jsonschema::JSONSchema::compile(&schema["function"]["parameters"])
            .unwrap()
            .is_valid(&args)
    );
    let before = serde_json::to_value(&state).unwrap();
    let out = agent::inspect_review(&state, &args, 16000).unwrap();
    assert_eq!(out["total"], 2);
    assert_eq!(out["next"], 2);
    assert_eq!(out["items"][0]["id"], "last");
    assert_eq!(out["items"][1]["id"], "first");
    let page = agent::inspect_review(
        &state,
        &json!({"ids":["last","first"],"offset":1,"limit":1}),
        16000,
    )
    .unwrap();
    assert_eq!(page["items"][0], out["items"][1]);
    for ids in [
        json!(["missing"]),
        json!(["first", "first"]),
        json!([]),
        Value::Null,
    ] {
        assert!(
            agent::inspect_review(&state, &json!({"ids":ids,"offset":0,"limit":1}), 16000).is_err()
        );
    }
    assert_eq!(serde_json::to_value(&state).unwrap(), before);
    state.role = Role::Main;
    assert!(agent::inspect_review(&state, &args, 16000).is_err());
}

#[tokio::test]
async fn inspect_review_pages_complete_findings_by_bytes_for_both_roles() {
    let journal = fresh_review_journal().await;
    let mut state = journal.load().await.unwrap().unwrap();
    for id in ["a", "b", "c"] {
        state.review_draft.insert(
            id.into(),
            serde_json::from_value(json!({
                "code":"field_issue", "message":id.repeat(300), "correction":"Compare source",
                "affected":[], "sources":[span()]
            }))
            .unwrap(),
        );
    }
    for role in [Role::Reviewer, Role::Main] {
        state.role = role;
        if matches!(state.role, Role::Main) {
            state.review = Some(Review {
                analysis_sha256: digest(&state.analysis).unwrap(),
                coverage: Coverage::default(),
                findings: state.review_draft.values().cloned().collect(),
            ..Default::default()
            });
        }
        let all = agent::inspect_review(&state, &json!({"offset":0,"limit":100}), 16000).unwrap();
        let one = agent::inspect_review(&state, &json!({"offset":0,"limit":1}), 16000).unwrap();
        let budget = serde_json::to_vec(&one).unwrap().len();
        let before = serde_json::to_value(&state).unwrap();
        let mut items = Vec::new();
        let mut offset = 0;
        while offset < 3 {
            let page = agent::inspect_review(&state, &json!({"offset":offset,"limit":100}), budget)
                .unwrap();
            assert!(serde_json::to_vec(&page).unwrap().len() <= budget);
            assert_eq!(page["total"], 3);
            assert_eq!(page["items"].as_array().unwrap().len(), 1);
            let next = page["next"].as_u64().unwrap();
            assert!(next > offset);
            items.extend(page["items"].as_array().unwrap().iter().cloned());
            offset = next;
        }
        assert_eq!(json!(items), all["items"]);
        let end = agent::inspect_review(&state, &json!({"offset":3,"limit":100}), budget).unwrap();
        assert_eq!(end["items"], json!([]));
        assert_eq!(end["next"], 3);
        assert!(
            agent::inspect_review(&state, &json!({"offset":0,"limit":100}), budget - 1)
                .unwrap_err()
                .contains("single complete review finding")
        );
        assert_eq!(serde_json::to_value(&state).unwrap(), before);
    }
}

#[tokio::test]
async fn review_finding_must_fit_its_retrieval_envelope_before_persistence() {
    let journal = fresh_review_journal().await;
    let start = journal.load().await.unwrap().unwrap().turn;
    let mut finding = json!({"code":"FIELD_MISMATCH","message":"",
        "correction":"按所引原文补全并重新核对该字段", "affected":[],"sources":[span()]});
    let overhead = serde_json::to_vec(&finding).unwrap().len();
    finding["message"] = json!("x".repeat(config().limits.max_tool_result_bytes - overhead));
    assert_eq!(
        serde_json::to_vec(&finding).unwrap().len(),
        config().limits.max_tool_result_bytes
    );
    *journal.interrupt_after.lock().unwrap() = Some(start + 3);
    let model = work_script(vec![
        ("set_work_note", active_work("source")),
        (
            "read_source",
            json!({"source_id":"source","start":0,"max_bytes":1024}),
        ),
        ("put_review_finding", json!({"id":null,"finding":finding})),
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
    assert!(
        saved.review_draft.is_empty(),
        "an unqueryable finding must not be saved"
    );
    assert!(
        saved.transcript.last().unwrap()["content"]
            .as_str()
            .unwrap()
            .contains("finding exceeds budget")
    );
}

#[tokio::test]
async fn review_findings_require_current_field_paths_and_concrete_corrections() {
    let journal = fresh_review_journal().await;
    let saved = journal.load().await.unwrap().unwrap();
    let id = saved.analysis.records.keys().next().unwrap();
    let finding = json!({"code":"FIELD_MISMATCH","message":"所引条款与响应条件不一致",
        "correction":"依据引用条款核对并修正条件，保留原文适用范围",
        "affected":[{"id":id,"path":"/data"}],"sources":[span()]});
    let mut bad_path = finding.clone();
    bad_path["affected"][0]["path"] = json!("/data/nonexistent");
    let mut missing_correction = finding.clone();
    missing_correction["correction"] = json!(" ");
    let mut duplicate = finding.clone();
    duplicate["affected"]
        .as_array_mut()
        .unwrap()
        .push(finding["affected"][0].clone());
    *journal.interrupt_after.lock().unwrap() = Some(saved.turn + 7);
    let model = work_script(vec![
        ("set_work_note", active_work("source")),
        (
            "read_source",
            json!({"source_id":"source","start":0,"max_bytes":1024}),
        ),
        (
            "inspect_analysis",
            json!({"view":"detail","kind":"all","ids":[id],"offset":0,"limit":1}),
        ),
        ("put_review_finding", json!({"id":null,"finding":bad_path})),
        (
            "put_review_finding",
            json!({"id":null,"finding":missing_correction}),
        ),
        ("put_review_finding", json!({"id":null,"finding":duplicate})),
        ("put_review_finding", json!({"id":null,"finding":finding})),
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
    assert_eq!(saved.review_draft.len(), 1);
    assert_eq!(
        saved.review_draft.values().next().unwrap().affected[0].path,
        "/data"
    );
    let bodies = model.bodies.lock().unwrap();
    for (index, message) in [
        (4, "unknown affected field"),
        (5, "concrete correction"),
        (6, "duplicate affected field"),
    ] {
        assert!(bodies[index].to_string().contains(message), "{message}");
    }
}

#[tokio::test]
async fn review_draft_rejects_unseen_evidence_and_main_agent_mutations() {
    let journal = fresh_review_journal().await;
    let mut state = journal.load().await.unwrap().unwrap();
    let mut source = input();
    let padding = "Independent neighboring text. ".repeat(1024);
    let tail = Span {
        source_id: "unseen".into(),
        start: padding.len(),
        end: padding.len() + "Final evidence.".len(),
        grid_cell: None,
        view_id: None,
    };
    source.source_units.push(Source {
        source_unit_revision_id: "unseen".into(),
        ordinal: 1,
        text: format!("{padding}Final evidence."),
        ..source.source_units[0].clone()
    });
    state.input_sha256 = digest(&source).unwrap();
    tools::cover(
        state
            .analysis
            .coverage
            .text
            .entry("unseen".into())
            .or_default(),
        0,
        tail.end,
    );
    state.analysis.dispositions.insert(
        "unseen".into(),
        Disposition {
            state: DispositionState::NonRequirement,
            reason: "Independent synthetic fact source".into(),
        },
    );
    let record: Record = serde_json::from_value(json!({"id":"unseen-fact","sources":[tail],"data":{"kind":"fact","name":"independent fact","value":"Final evidence.","scope":"unseen"}})).unwrap();
    let record_id = record.id.clone();
    state.analysis.records.insert(record_id.clone(), record);
    state.source_review = Some(source_review::initialize(&source, &config()).unwrap());
    source_review::select_next(&source, &config(), &mut state).unwrap();
    let projected = source_review::evidence(&source, &config(), &state)
        .unwrap()
        .unwrap();
    assert!(tools::validate_span(&source, &projected.coverage, &tail).is_err());
    assert!(
        !projected
            .coverage
            .candidate
            .contains_key(&format!("record:{record_id}"))
    );
    *journal.state.lock().unwrap() = Some(state.clone());
    let finding = json!({"code":"FIELD_MISMATCH","message":"specific field issue",
        "correction":"按所引原文补全并重新核对该字段", "affected":[],"sources":[tail]});
    let unseen_candidate = json!({"code":"FIELD_MISMATCH","message":"specific field issue",
        "correction":"核对原文后修正该字段", "affected":[{"id":record_id,"path":"/data"}],"sources":[]});
    *journal.interrupt_after.lock().unwrap() = Some(state.turn + 3);
    let model = work_script(vec![
        ("put_review_finding", json!({"id":null,"finding":finding})),
        (
            "put_review_finding",
            json!({"id":null,"finding":unseen_candidate}),
        ),
        ("delete_review_finding", json!({"id":"foreign"})),
    ]);
    agent::run(
        &source,
        &config(),
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let mut saved = journal.load().await.unwrap().unwrap();
    let packet: Value = {
        let wire = model.bodies.lock().unwrap();
        serde_json::from_str(
            wire[0]["messages"].as_array().unwrap().last().unwrap()["content"]
                .as_str()
                .unwrap(),
        )
        .unwrap()
    };
    assert!(
        !packet["preloaded_evidence"]
            .to_string()
            .contains("Final evidence.")
    );
    assert!(
        !packet["preloaded_evidence"]
            .to_string()
            .contains(&record_id)
    );
    assert!(saved.review_draft.is_empty());
    assert!(
        model.bodies.lock().unwrap()[2]
            .to_string()
            .contains("independently inspect current affected outcome")
    );
    // Fixture with a previously delivered reviewer finding; main cannot edit it.
    saved.review_draft.insert(
        "existing".into(),
        serde_json::from_value(finding.clone()).unwrap(),
    );
    saved.reviewer_coverage = saved.analysis.coverage.clone();
    saved.role = Role::Main;
    let start = saved.turn;
    *journal.state.lock().unwrap() = Some(saved);
    *journal.interrupt_after.lock().unwrap() = Some(start + 2);
    let model = work_script(vec![
        ("put_review_finding", json!({"id":null,"finding":finding})),
        ("delete_review_finding", json!({"id":"existing"})),
    ]);
    agent::run(
        &source,
        &config(),
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let mut saved = journal.load().await.unwrap().unwrap();
    assert_eq!(saved.review_draft.len(), 1);
    assert!(saved.review_draft.contains_key("existing"));
    saved.role = Role::Reviewer;
    let next = saved.turn + 1;
    *journal.state.lock().unwrap() = Some(saved);
    *journal.interrupt_after.lock().unwrap() = Some(next);
    agent::run(
        &source,
        &config(),
        &journal,
        &work_script(vec![("delete_review_finding", json!({"id":"existing"}))]),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert!(
        journal
            .load()
            .await
            .unwrap()
            .unwrap()
            .review_draft
            .is_empty()
    );
    let main = tools::schemas(false);
    let reviewer = tools::schemas(true);
    for name in [
        "read_review_task",
        "put_review_finding",
        "delete_review_finding",
    ] {
        assert!(!main.iter().any(|t| t["function"]["name"] == name));
        assert!(reviewer.iter().any(|t| t["function"]["name"] == name));
    }
}

#[tokio::test]
async fn source_judgment_can_pipeline_next_read_without_granting_same_batch_evidence() {
    struct Pipeline {
        calls: Mutex<usize>,
        status: &'static str,
    }
    #[async_trait]
    impl Model for Pipeline {
        async fn turn(&self, _: &Config, body: &[u8]) -> Result<ChatTurn, AgentError> {
            let mut turn = self.calls.lock().unwrap();
            let mut calls = if *turn == 0 {
                Vec::new()
            } else {
                let body = serde_json::from_slice(body).unwrap();
                let mut calls =
                    fixture_review_calls(&body, "put_source_review", &json!({})).unwrap();
                if self.status != "checked" {
                    let last = calls.last_mut().unwrap();
                    let mut args: Value = serde_json::from_str(&last.arguments).unwrap();
                    if self.status == "stale" {
                        args["expected_version"] = json!("stale");
                    } else {
                        args["status"] = json!("needs_evidence");
                        args["evidence_requests"] =
                            json!([{"question":"Check the next paragraph.","source_ids":["next"]}]);
                    }
                    last.arguments = args.to_string();
                }
                calls
            };
            calls.push(ChatToolCall {
                id: format!("read-{turn}"),
                name: "read_review_task".into(),
                arguments: "{}".into(),
            });
            if *turn == 1 {
                calls.push(ChatToolCall {
                    id: "premature-next".into(),
                    name: "complete_review_check".into(),
                    arguments:
                        json!({"reference":"disposition:next", "summary":"Unseen next source.",
                        "sources":[{"source_id":"next","start":0,"end":"另一个背景段落。".len()}]})
                        .to_string(),
                });
            }
            *turn += 1;
            Ok(ChatTurn {
                finish_reason: "tool_calls".into(),
                tool_calls: calls,
                ..Default::default()
            })
        }
    }
    for status in ["checked", "stale", "needs_evidence"] {
        let journal = fresh_review_journal().await;
        let mut input = input();
        input.source_units.push(Source {
            source_unit_revision_id: "next".into(),
            document_id: "document".into(),
            text: "另一个背景段落。".into(),
            locator: json!({}),
            ordinal: 1,
        });
        let mut state = journal.load().await.unwrap().unwrap();
        state.input_sha256 = digest(&input).unwrap();
        state.journal = Default::default();
        state.turn = 0;
        state.tool_calls = 0;
        state.read_bytes = 0;
        state.review_rounds = 0;
        state.analysis.records.clear();
        state.analysis.relations.clear();
        for source in &input.source_units {
            state.analysis.dispositions.insert(
                source.source_unit_revision_id.clone(),
                Disposition {
                    state: DispositionState::NonRequirement,
                    reason: "Synthetic background.".into(),
                },
            );
            tools::cover(
                state
                    .analysis
                    .coverage
                    .text
                    .entry(source.source_unit_revision_id.clone())
                    .or_default(),
                0,
                source.text.len(),
            );
        }
        // Metadata is already delivered; source/candidate evidence remains unread.
        state.reviewer_coverage.metadata = state.analysis.coverage.metadata.clone();
        state.source_review = Some(source_review::initialize(&input, &config()).unwrap());
        source_review::select_next(&input, &config(), &mut state).unwrap();
        let start = state.turn;
        let analysis_before = digest(&state.analysis).unwrap();
        *journal.state.lock().unwrap() = Some(state);
        journal.reservations.lock().unwrap().clear();
        *journal.interrupt_after.lock().unwrap() = Some(start + 2);
        let stopped = agent::run(
            &input,
            &config(),
            &journal,
            &Pipeline {
                calls: Mutex::new(0),
                status,
            },
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        let saved = journal.load().await.unwrap().unwrap();
        let outputs: BTreeMap<String, Value> = saved
            .transcript
            .iter()
            .filter(|m| m["role"] == "tool")
            .map(|m| {
                (
                    m["tool_call_id"].as_str().unwrap().into(),
                    serde_json::from_str(m["content"].as_str().unwrap()).unwrap(),
                )
            })
            .collect();
        assert!(
            outputs.contains_key("read-1"),
            "{stopped:?}; turn={}; {outputs:?}",
            saved.turn
        );
        assert_eq!(outputs["read-1"]["ok"], true);
        assert_eq!(
            outputs["read-1"]["result"]["assigned_evidence"]["source"]["source_id"],
            if status == "checked" {
                "next"
            } else {
                "source"
            }
        );
        assert_eq!(outputs["premature-next"]["ok"], false);
        // The first packet delivered adjacent text, but never that task's
        // candidates. The handoff still cannot authorize same-batch comparison.
        assert!(saved.reviewer_coverage.text.contains_key("next"));
        assert!(
            !saved
                .reviewer_coverage
                .candidate
                .contains_key("disposition:next")
        );
        assert_eq!(
            saved
                .pending_coverage
                .as_ref()
                .unwrap()
                .candidate
                .contains_key("disposition:next"),
            status == "checked"
        );
        assert_eq!(
            saved.source_review.as_ref().unwrap().results.len(),
            usize::from(status != "stale")
        );
        assert_eq!(digest(&saved.analysis).unwrap(), analysis_before);
        assert_eq!(saved.turn, start + 2);
    }
}

#[tokio::test]
async fn preloaded_review_evidence_allows_comparison_without_an_extra_read_round() {
    struct ReadAndCompare(String);
    #[async_trait]
    impl Model for ReadAndCompare {
        async fn turn(&self, _: &Config, body: &[u8]) -> Result<ChatTurn, AgentError> {
            let body: Value = serde_json::from_slice(body).unwrap();
            let packet: Value = serde_json::from_str(
                body["messages"].as_array().unwrap().last().unwrap()["content"]
                    .as_str()
                    .unwrap(),
            )
            .unwrap();
            let evidence = &packet["preloaded_evidence"]["assigned_evidence"];
            assert_eq!(evidence["source"]["text"], input().source_units[0].text);
            assert!(
                evidence
                    .to_string()
                    .contains(self.0.strip_prefix("record:").unwrap())
            );
            Ok(ChatTurn {
                finish_reason: "tool_calls".into(),
                tool_calls: vec![
                    ChatToolCall { id:"bundle".into(), name:"read_review_task".into(), arguments:"{}".into() },
                    ChatToolCall { id:"premature".into(), name:"complete_review_check".into(),
                        arguments:json!({"reference":self.0,"summary":"Compared original and current candidate.","sources":[span()]}).to_string() },
                ],
                ..Default::default()
            })
        }
    }
    let journal = fresh_review_journal().await;
    let initial = journal.load().await.unwrap().unwrap();
    assert!(initial.reviewer_coverage.text.is_empty());
    let delivered = source_review::evidence(&input(), &config(), &initial)
        .unwrap()
        .unwrap()
        .coverage;
    let key = format!("record:{}", initial.analysis.records.keys().next().unwrap());
    *journal.interrupt_after.lock().unwrap() = Some(initial.turn + 1);
    agent::run(
        &input(),
        &config(),
        &journal,
        &ReadAndCompare(key.clone()),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let saved = journal.load().await.unwrap().unwrap();
    assert_eq!(json!(saved.reviewer_coverage), json!(delivered));
    let receipt = digest(&json!([
        "review_check",
        key,
        source_review::candidate_version(&initial, &key).unwrap()
    ]))
    .unwrap();
    let pending = saved.pending_coverage.as_ref().unwrap();
    assert_eq!(pending.text["source"], vec![(0, span().end)]);
    assert_eq!(
        pending.candidate[&key],
        digest(&initial.analysis.records.values().next().unwrap()).unwrap()
    );
    assert!(saved.reviewer_progress.seen.contains(&receipt));
    let results: Vec<Value> = saved
        .transcript
        .iter()
        .filter(|m| m["role"] == "tool")
        .map(|m| serde_json::from_str(m["content"].as_str().unwrap()).unwrap())
        .collect();
    assert_eq!(results[0]["ok"], true);
    assert_eq!(
        results[0]["result"]["assigned_evidence"]["source"]["text"],
        input().source_units[0].text
    );
    assert!(
        serde_json::to_vec(&results[0]["result"]).unwrap().len()
            <= config().limits.max_tool_result_bytes
    );
    assert_eq!(
        results[1]["ok"], true,
        "the reserved request already delivered the independent evidence before any tool executed"
    );
    let before = digest(&saved.analysis).unwrap();
    let mut projected = saved.clone();
    let body: Value = serde_json::from_slice(
        &agent::request(&input(), &config(), &mut projected)
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(body["messages"].as_array().unwrap().iter().any(|m| {
        m["content"]
            .as_str()
            .is_some_and(|raw| raw.contains("assigned_evidence") && raw.contains("citation_ref"))
    }));
    assert_eq!(json!(projected.reviewer_coverage), json!(delivered));
    assert_eq!(
        projected.read_bytes, saved.read_bytes,
        "request projection is not a second tool read"
    );

    *journal.interrupt_after.lock().unwrap() = Some(initial.turn + 2);
    let model = work_script(vec![(
        "complete_review_check",
        json!({"reference":key,
        "summary":"Compared original and current candidate.","sources":[span()]}),
    )]);
    agent::run(
        &input(),
        &config(),
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let after = journal.load().await.unwrap().unwrap();
    assert!(after.reviewer_progress.seen.contains(&receipt));
    assert_eq!(
        after.reviewer_progress.completions, saved.reviewer_progress.completions,
        "a repeated comparison after the redundant read cannot earn completion twice"
    );
    assert_eq!(digest(&after.analysis).unwrap(), before);
    assert!(
        !after.done,
        "reading and a candidate comparison cannot replace source review"
    );
    assert_eq!(after.review_rounds, initial.review_rounds);
}

#[tokio::test]
async fn independent_review_must_read_and_drives_repair() {
    let journal = MemoryJournal::default();
    let model = script();
    let expected_calls = model.calls.lock().unwrap().len();
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
    assert_eq!(result.analysis.records.len(), 1);
    assert!(result.review.findings.is_empty());
    let state = journal.state.lock().unwrap();
    let state = state.as_ref().unwrap();
    assert_eq!(state.review_rounds, 2);
    assert_eq!(state.turn, expected_calls);
    // The reviewer now receives its own evidence packet before judging.
    // Its grounded omission still forces a Main repair and a second review;
    // preloading does not itself accept the initial non-requirement decision.
    let bodies = model.bodies.lock().unwrap();
    assert!(
        bodies[4]["messages"]
            .to_string()
            .contains("assigned_evidence")
    );
    assert!(
        bodies
            .iter()
            .any(|body| body["messages"].to_string().contains("OMITTED_REQUIREMENT"))
    );
    for body in bodies.iter().filter(|b| {
        b["messages"][0]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("INDEPENDENT")
    }) {
        assert!(
            body["tools"]
                .as_array()
                .unwrap()
                .iter()
                .all(|t| t["function"]["name"] != "put_record")
        );
    }
}

#[tokio::test]
async fn review_navigation_finishes_current_focus_before_other_pending_candidates() {
    let journal = fresh_review_journal().await;
    let mut state = journal.load().await.unwrap().unwrap();
    let mut record = state.analysis.records.values().next().unwrap().clone();
    record.id = "zz-focused".into();
    state.analysis.records.insert(record.id.clone(), record);
    state.reviewer_work.as_mut().unwrap().focus.references = vec!["record:zz-focused".into()];
    let before = json!([
        state.analysis,
        state.reviewer_progress,
        state.reviewer_coverage
    ]);
    let body: Value = serde_json::from_slice(
        &agent::request(&input(), &config(), &mut state)
            .await
            .unwrap(),
    )
    .unwrap();
    let packet: Value = serde_json::from_str(
        body["messages"].as_array().unwrap().last().unwrap()["content"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        packet["work_state"]["comparison_progress"]["focus_remaining"],
        1
    );
    assert_eq!(
        packet["work_state"]["comparison_progress"]["next_reference"],
        "record:zz-focused"
    );
    assert_eq!(
        json!([
            state.analysis,
            state.reviewer_progress,
            state.reviewer_coverage
        ]),
        before
    );
}

#[tokio::test]
async fn clean_review_checks_require_independent_evidence_and_only_complete_a_version_once() {
    let journal = fresh_review_journal().await;
    let initial = journal.load().await.unwrap().unwrap();
    let record = initial.analysis.records.values().next().unwrap();
    let reference = format!("record:{}", record.id);
    let receipt = digest(&json!([
        "review_check",
        reference,
        source_review::candidate_version(&initial, &reference).unwrap()
    ]))
    .unwrap();
    let start = initial.turn;
    let mut focus = active_work("source");
    focus["focus"] = json!({"action":"review","source_spans":[],"references":[reference]});
    let check = json!({"reference":reference,"summary":"The recorded submission requirement matches the cited original clause.","sources":[span()]});
    let calls = vec![
        ("set_work_note", focus),
        ("check_gaps", json!({"scope":"work","offset":0,"limit":10})), // preparation grants no semantic comparison
        (
            "read_source",
            json!({"source_id":"source","start":0,"max_bytes":1024}),
        ),
        (
            "inspect_analysis",
            json!({"kind":"all","view":"detail","offset":0,"limit":10}),
        ),
        ("complete_review_check", check.clone()),
    ];
    *journal.interrupt_after.lock().unwrap() = Some(start + calls.len());
    agent::run(
        &input(),
        &config(),
        &journal,
        &work_script(calls),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let saved = journal.load().await.unwrap().unwrap();
    assert!(saved.reviewer_progress.seen.contains(&receipt));
    assert!(saved.reviewer_progress.completions.contains(&receipt));
    assert_eq!(saved.reviewer_progress.watch.focus_turns, 0);
    assert!(!saved.done);
    assert!(saved.review.is_none());
    assert!(saved.review_draft.is_empty());
    assert_eq!(json!(saved.analysis), json!(initial.analysis));
    // A completed focus must lead to the remaining disposition, even though
    // the source scope still has work. Repeating the focused record is not it.
    let mut projected = saved.clone();
    let before = digest(&projected.reviewer_progress).unwrap();
    let body: Value = serde_json::from_slice(
        &agent::request(&input(), &config(), &mut projected)
            .await
            .unwrap(),
    )
    .unwrap();
    let packet: Value = serde_json::from_str(
        body["messages"].as_array().unwrap().last().unwrap()["content"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        packet["work_state"]["next_action"],
        "select_next_review_focus"
    );
    assert_eq!(
        packet["work_state"]["comparison_progress"]["focus_remaining"],
        0
    );
    assert_eq!(
        packet["work_state"]["comparison_progress"]["next_reference"],
        "disposition:source"
    );
    assert!(
        packet["execution"]["next_action"]
            .as_str()
            .unwrap()
            .contains("already has recorded outcomes")
    );
    assert_eq!(digest(&projected.reviewer_progress).unwrap(), before);
    assert!(!projected.done);
    assert_eq!(
        saved.reviewer_coverage.candidate.get(&reference),
        Some(&digest(record).unwrap()),
        "a successful clean comparison still requires this reviewer's exact current candidate receipt"
    );

    // A new summary (or a restart) cannot turn the same comparison into progress.
    let mut repeated = check.clone();
    repeated["summary"] = json!("Reworded conclusion about the same candidate.");
    *journal.interrupt_after.lock().unwrap() = Some(saved.turn + 1);
    agent::run(
        &input(),
        &config(),
        &journal,
        &work_script(vec![("complete_review_check", repeated)]),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let repeated = journal.load().await.unwrap().unwrap();
    assert_eq!(
        repeated.reviewer_progress.completions,
        saved.reviewer_progress.completions
    );
    assert_eq!(repeated.reviewer_progress.watch.no_progress_turns, 1);

    // A repaired candidate must be independently delivered again. The next
    // request can preload its current value rather than spend a reading turn.
    let mut changed = repeated;
    let record = changed.analysis.records.values_mut().next().unwrap();
    let RecordData::Requirement { text, .. } = &mut record.data else {
        panic!("requirement fixture")
    };
    text.push('。');
    let changed_receipt = digest(&json!([
        "review_check",
        reference,
        source_review::candidate_version(&changed, &reference).unwrap()
    ]))
    .unwrap();
    *journal.interrupt_after.lock().unwrap() = Some(changed.turn + 1);
    *journal.state.lock().unwrap() = Some(changed);
    let model = work_script(vec![("complete_review_check", check)]);
    agent::run(
        &input(),
        &config(),
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let changed = journal.load().await.unwrap().unwrap();
    let wire = model.bodies.lock().unwrap();
    let packet: Value = serde_json::from_str(
        wire[0]["messages"].as_array().unwrap().last().unwrap()["content"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert!(
        packet["preloaded_evidence"]
            .to_string()
            .contains("提交规定格式。")
    );
    assert!(changed.reviewer_progress.seen.contains(&changed_receipt));
    assert!(!changed.done);
    assert!(
        !tools::schemas(false)
            .iter()
            .any(|t| t["function"]["name"] == "complete_review_check")
    );
}

#[tokio::test]
async fn source_review_advances_without_pulling_in_deferred_unknowns() {
    let mut source = input();
    source.source_units.push(Source {
        source_unit_revision_id: "later".into(),
        ordinal: 1,
        ..source.source_units[0].clone()
    });
    let mut main = active_work("source");
    main["source_scope"] = json!(["source", "later"]);
    let mut local = active_work("source");
    local["deferred_sources"] = json!(["later"]);
    let journal = MemoryJournal::default();
    let read = |id| json!({"source_id":id,"start":0,"max_bytes":1024});
    let disposition = |id| json!({"source_id":id,"state":"non_requirement","reason":"fixture source accounted for"});
    let inspect =
        |id, kind| json!({"source_id":id,"kind":kind,"view":"detail","offset":0,"limit":10});
    let calls = vec![
        ("set_work_note", main),
        ("read_source", read("source")),
        ("read_source", read("later")),
        (
            "put_record",
            json!({"id":null,"sources":[Span {source_id:"later".into(), ..span()}],
            "data":{"kind":"unresolved","problem":"Deferred source refers to unavailable evidence","affected":[],"candidates":[]}}),
        ),
        ("set_disposition", disposition("source")),
        ("set_disposition", disposition("later")),
        ("request_review", json!({})),
        ("set_work_note", local),
        ("read_source", read("source")),
        ("inspect_analysis", inspect("source", "disposition")),
        ("put_source_review", json!({})),
    ];
    *journal.interrupt_after.lock().unwrap() = Some(calls.len());
    agent::run(
        &source,
        &config(),
        &journal,
        &work_script(calls),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let saved = journal.load().await.unwrap().unwrap();
    assert_eq!(
        json!(saved.reviewer_work.as_ref().unwrap().status),
        "active"
    );
    assert_eq!(
        saved.reviewer_work.as_ref().unwrap().source_scope,
        vec!["later"]
    );
    assert_eq!(saved.reviewer_work.as_ref().unwrap().pending_refs.len(), 1);
    assert!(
        source_review::pending(&source, &config(), &saved)
            .unwrap()
            .iter()
            .any(|task| task.source_id == "later"),
        "neighbor evidence may be delivered, but the deferred source still requires its own semantic judgment"
    );
    assert!(
        saved.review.is_none(),
        "local completion cannot approve the deferred source"
    );
    assert!(!saved.done);
    *journal.interrupt_after.lock().unwrap() = None;

    let result = agent::run(
        &source,
        &config(),
        &journal,
        &work_script(vec![
            ("set_work_note", active_work("later")),
            ("read_source", read("later")),
            ("inspect_analysis", inspect("later", "all")),
            ("inspect_analysis", inspect("later", "disposition")),
            ("put_source_review", json!({"fixture_status":"checked","fixture_relationship_status":"source_limited","fixture_unresolved":true})),
        ]),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(
        result.analysis.records.len(),
        1,
        "the unresolved outcome was retained"
    );
    assert!(result.review.coverage.text.contains_key("later"));
}

#[tokio::test]
async fn reviewer_recovery_exposes_clean_completion_without_granting_approval() {
    use crate::agent_runtime::progress::Recovery;
    let journal = MemoryJournal::default();
    agent::run(
        &input(),
        &config(),
        &journal,
        &script(),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    let mut state = journal.load().await.unwrap().unwrap();
    state.role = Role::Reviewer;
    state.done = false;
    state.review = None;
    state.transcript.clear();
    state.pending_coverage = None;
    state.reviewer_work = Some(serde_json::from_value(active_work("source")).unwrap());
    state.reviewer_progress.watch.recovery = Recovery::Replan;
    let before = digest(&state.reviewer_coverage).unwrap();
    let body: Value = serde_json::from_slice(
        &agent::request(&input(), &config(), &mut state)
            .await
            .unwrap(),
    )
    .unwrap();
    let packet: Value = serde_json::from_str(
        body["messages"].as_array().unwrap().last().unwrap()["content"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(packet["work_state"]["gap_counts"], json!({}));
    assert_eq!(
        packet["work_state"]["next_action"],
        "compare_then_judge_source"
    );
    let recovery = packet["execution"]["next_action"].as_str().unwrap();
    assert!(recovery.contains("put_source_review"));
    assert!(recovery.contains("host advances tasks"));
    assert_eq!(digest(&state.reviewer_coverage).unwrap(), before);
    assert!(!state.done);
    assert!(state.review.is_none());
    assert!(state.review_draft.is_empty());

    // The other role's failure remains visible, without replacing the active
    // review's local next action or granting whole-analysis acceptance.
    state.main_progress.watch.recovery = Recovery::Blocked;
    state
        .main_progress
        .block(vec!["source".into()], "dependencies".into());
    let body: Value = serde_json::from_slice(
        &agent::request(&input(), &config(), &mut state)
            .await
            .unwrap(),
    )
    .unwrap();
    let packet: Value = serde_json::from_str(
        body["messages"].as_array().unwrap().last().unwrap()["content"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        packet["work_state"]["next_action"],
        "compare_then_judge_source"
    );
    assert_eq!(packet["progress"]["execution_blockers"], 1);
    assert_eq!(state.main_progress.blockers.len(), 1);
    assert!(!state.done);
}
