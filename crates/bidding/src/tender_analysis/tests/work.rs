use super::*;

#[tokio::test]
async fn active_scope_queries_exclude_unrelated_outcomes_but_allow_exact_cross_references() {
    let (input, analysis, ids) = analysis_query_fixture();
    for role in [Role::Main, Role::Reviewer] {
        let journal = MemoryJournal::default();
        *journal.interrupt_after.lock().unwrap() = Some(1);
        agent::run(
            &input,
            &config(),
            &journal,
            &work_script(vec![("set_work_note", active_work("source"))]),
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        let mut state = journal.load().await.unwrap().unwrap();
        state.analysis = analysis.clone();
        state.role = role.clone();
        if role == Role::Reviewer {
            state.source_review = Some(source_review::initialize(&input, &config()).unwrap());
            source_review::select_next(&input, &config(), &mut state).unwrap();
            state.reviewer_work = Some(serde_json::from_value(active_work("source")).unwrap());
        }
        *journal.state.lock().unwrap() = Some(state);
        *journal.interrupt_after.lock().unwrap() = Some(5);
        let model = work_script(vec![
            (
                "inspect_analysis",
                json!({"view":"detail","kind":"all","offset":0,"limit":50}),
            ),
            (
                "inspect_analysis",
                json!({"view":"detail","kind":"relation","offset":0,"limit":50}),
            ),
            (
                "inspect_analysis",
                json!({"view":"detail","kind":"disposition","offset":0,"limit":50}),
            ),
            (
                "inspect_analysis",
                json!({"view":"detail","kind":"all","offset":0,"limit":50,"ids":[ids[1]]}),
            ),
        ]);
        agent::run(
            &input,
            &config(),
            &journal,
            &model,
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        let state = journal.load().await.unwrap().unwrap();
        let pages: Vec<Value> = state
            .transcript
            .iter()
            .filter(|m| m["role"] == "tool")
            .map(|m| {
                serde_json::from_str::<Value>(m["content"].as_str().unwrap()).unwrap()["result"]
                    .clone()
            })
            .filter(|v| v["items"].is_array())
            .collect();
        assert_eq!(pages.len(), 4);
        assert_eq!(pages[0]["total"], 4);
        assert!(
            pages[0]["items"]
                .as_array()
                .unwrap()
                .iter()
                .all(|r| r["id"] != ids[1])
        );
        assert_eq!(pages[1]["total"], 1);
        assert_eq!(pages[1]["items"][0]["id"], ids[3]);
        assert_eq!(pages[2]["total"], 1);
        assert_eq!(pages[2]["items"][0]["source_id"], "source");
        assert_eq!(pages[3]["total"], 1);
        assert_eq!(pages[3]["items"][0]["id"], ids[1]);
        if role == Role::Reviewer {
            assert!(
                !state
                    .reviewer_coverage
                    .candidate
                    .contains_key(&format!("record:{}", ids[1]))
            );
            assert!(
                state
                    .pending_coverage
                    .as_ref()
                    .unwrap()
                    .candidate
                    .contains_key(&format!("record:{}", ids[1]))
            );
        }
    }
}

#[tokio::test]
async fn work_actions_can_plan_unread_text_without_authorizing_a_citation() {
    for (role, action) in [Role::Main, Role::Reviewer].into_iter().flat_map(|role| {
        ["locate", "extract", "review"]
            .into_iter()
            .map(move |action| (role.clone(), action))
    }) {
        let journal = if role == Role::Reviewer {
            fresh_review_journal().await
        } else {
            MemoryJournal::default()
        };
        let start = journal.load().await.unwrap().map_or(0, |s| s.turn);
        *journal.interrupt_after.lock().unwrap() = Some(start + 1);
        let mut work = active_work("source");
        work["focus"]["action"] = json!(action);
        work["focus"]["source_spans"] = json!([span()]);
        agent::run(
            &input(),
            &config(),
            &journal,
            &work_script(vec![("set_work_note", work)]),
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        let saved = journal.load().await.unwrap().unwrap();
        let work = if role == Role::Main {
            saved.main_work.as_ref()
        } else {
            saved.reviewer_work.as_ref()
        };
        assert!(
            work.is_some(),
            "planning a valid unread range for {action} must be possible"
        );
        let coverage = if role == Role::Main {
            &saved.analysis.coverage
        } else {
            &saved.reviewer_coverage
        };
        assert!(
            tools::validate_span(&input(), coverage, &span()).is_err(),
            "planning is not delivery, even if the other role has read the source"
        );
    }
}

#[tokio::test]
async fn completed_scope_needs_coverage_and_outcomes_but_no_new_focus() {
    for ready in [false, true] {
        let journal = MemoryJournal::default();
        let mut calls = vec![("set_work_note", active_work("source"))];
        if ready {
            calls.extend([
                ("read_source", json!({"source_id":"source","start":0,"max_bytes":1024})),
                ("set_disposition", json!({"source_id":"source","state":"non_requirement","reason":"diagnostic disposition"})),
            ]);
        }
        let mut complete = active_work("source");
        complete["status"] = json!("complete");
        complete["focus"]["action"] = json!("extract");
        calls.push(("set_work_note", complete));
        *journal.interrupt_after.lock().unwrap() = Some(calls.len());
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
        assert_eq!(json!(saved.main_work.unwrap().status) == "complete", ready);
    }
}

#[tokio::test]
async fn work_split_retains_deferred_sources_outcomes_and_review_barriers() {
    let (input, analysis, ids) = analysis_query_fixture();
    for role in [Role::Main, Role::Reviewer] {
        let journal = MemoryJournal::default();
        *journal.interrupt_after.lock().unwrap() = Some(1);
        let mut broad = active_work("source");
        broad["source_scope"] = json!(["source", "other-source"]);
        agent::run(
            &input,
            &config(),
            &journal,
            &work_script(vec![("set_work_note", broad.clone())]),
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        let mut state = journal.load().await.unwrap().unwrap();
        state.analysis = analysis.clone();
        state.role = role.clone();
        if role == Role::Reviewer {
            state.source_review = Some(source_review::initialize(&input, &config()).unwrap());
            source_review::select_next(&input, &config(), &mut state).unwrap();
            state.reviewer_work = Some(serde_json::from_value(broad).unwrap());
        }
        let analysis_before = digest(&state.analysis).unwrap();
        *journal.state.lock().unwrap() = Some(state);
        let mut split = active_work("source");
        split["deferred_sources"] = json!(["other-source"]);
        let mut manual_refs = split.clone();
        manual_refs["output_refs"] = json!([]);
        let mut foreign = split.clone();
        foreign["deferred_sources"] = json!(["foreign"]);
        let transition = if role == Role::Main {
            "request_review"
        } else {
            "put_source_review"
        };
        *journal.interrupt_after.lock().unwrap() = Some(7);
        let model = work_script(vec![
            ("set_work_note", active_work("source")),
            ("set_work_note", foreign),
            ("set_work_note", manual_refs),
            ("set_work_note", split.clone()),
            ("set_work_note", active_work("source")),
            (transition, json!({})),
        ]);
        agent::run(
            &input,
            &config(),
            &journal,
            &model,
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        let state = journal.load().await.unwrap().unwrap();
        let work = if role == Role::Main {
            state.main_work.as_ref()
        } else {
            state.reviewer_work.as_ref()
        }
        .unwrap();
        assert_eq!(work.source_scope, vec!["source"]);
        assert_eq!(work.deferred_sources, vec!["other-source"]);
        assert!(work.output_refs.contains(&format!("record:{}", ids[1])));
        assert!(work.output_refs.contains(&format!("relation:{}", ids[3])));
        assert_eq!(state.role, role);
        assert!(!state.done);
        assert_eq!(digest(&state.analysis).unwrap(), analysis_before);
        let outputs: Vec<Value> = model
            .bodies
            .lock()
            .unwrap()
            .iter()
            .flat_map(|body| {
                body["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|m| m["role"] == "tool")
                    .map(|m| serde_json::from_str(m["content"].as_str().unwrap()).unwrap())
            })
            .collect();
        for message in [
            "finish the active scope",
            "unknown work source",
            "maintained by the host",
            "resume the deferred source",
        ] {
            assert!(
                outputs
                    .iter()
                    .any(|out| out["error"].as_str().is_some_and(|e| e.contains(message))),
                "missing rejection: {message}"
            );
        }
        let last: Value = serde_json::from_str(
            state.transcript.last().unwrap()["content"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        let error = last["error"].as_str().unwrap();
        assert!(
            if role == Role::Main {
                error.contains("resume deferred_sources")
            } else {
                error.contains("independently") || error.contains("read the cited source")
            },
            "{error}"
        );
        assert!(outputs.iter().any(|out| out["result"]["handoff"] == true));
        // Reconstruct the checkpoint as durable JSON, then explicitly resume
        // each deferred source. Switching focus must retain the other source.
        let restored: agent::Checkpoint = serde_json::from_value(json!(state)).unwrap();
        *journal.state.lock().unwrap() = Some(restored);
        let mut resume = active_work("other-source");
        resume["deferred_sources"] = json!(["source"]);
        let mut expanded = resume.clone();
        expanded["source_scope"] = json!(["other-source", "source"]);
        expanded["deferred_sources"] = json!([]);
        *journal.interrupt_after.lock().unwrap() = Some(9);
        agent::run(
            &input,
            &config(),
            &journal,
            &work_script(vec![
                ("set_work_note", resume),
                ("set_work_note", expanded.clone()),
            ]),
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        let resumed = journal.load().await.unwrap().unwrap();
        let work = if role == Role::Main {
            resumed.main_work.as_ref()
        } else {
            resumed.reviewer_work.as_ref()
        }
        .unwrap();
        assert_eq!(json!(work.source_scope), expanded["source_scope"]);
        assert!(work.deferred_sources.is_empty());
        assert_eq!(digest(&resumed.analysis).unwrap(), analysis_before);
    }
}

#[tokio::test]
async fn work_references_cannot_be_fabricated_or_erased_by_deletion() {
    let journal = MemoryJournal::default();
    *journal.interrupt_after.lock().unwrap() = Some(1);
    agent::run(
        &input(),
        &config(),
        &journal,
        &work_script(vec![("set_work_note", active_work("source"))]),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let mut state = journal.load().await.unwrap().unwrap();
    let record = Record {
        id: "pending".into(),
        sources: vec![span()],
        data: RecordData::Unresolved {
            problem: "引用目标仍待核查".into(),
            affected: vec![],
            candidates: vec![],
        },
    };
    state.analysis.records.insert(record.id.clone(), record);
    let mut pending = active_work("source");
    pending["pending_refs"] = json!(["record:pending"]);
    // A main-agent deletion must also preserve unresolved reviewer handoffs.
    state.reviewer_work = Some(serde_json::from_value(pending).unwrap());
    *journal.state.lock().unwrap() = Some(state);
    *journal.interrupt_after.lock().unwrap() = Some(4);
    let mut fabricated = active_work("source");
    fabricated["output_refs"] = json!(["record:invented"]);
    let model = work_script(vec![
        ("set_work_note", active_work("foreign")),
        ("set_work_note", fabricated),
        ("delete_record", json!({"id":"pending"})),
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
    let state = journal.load().await.unwrap().unwrap();
    assert!(state.analysis.records.contains_key("pending"));
    assert_eq!(state.main_work.unwrap().source_scope, vec!["source"]);
    let errors: Vec<Value> = state
        .transcript
        .iter()
        .filter(|m| m["role"] == "tool")
        .map(|m| serde_json::from_str(m["content"].as_str().unwrap()).unwrap())
        .filter(|out: &Value| out["ok"] == false)
        .collect();
    assert_eq!(errors.len(), 3);
    for (out, reason) in errors.iter().zip([
        "unknown work source",
        "maintained by the host",
        "resolve the pending outcome before deleting",
    ]) {
        assert!(out["error"].as_str().unwrap().contains(reason));
    }
}

#[tokio::test]
async fn work_gaps_find_local_blockers_hidden_behind_global_unread_pages() {
    let (mut input, mut analysis, _) = analysis_query_fixture();
    for ordinal in 2..62 {
        input.source_units.push(Source {
            source_unit_revision_id: format!("unrelated-{ordinal}"),
            ordinal,
            ..input.source_units[0].clone()
        });
    }
    analysis.dispositions.remove("source");
    let journal = MemoryJournal::default();
    *journal.interrupt_after.lock().unwrap() = Some(1);
    agent::run(
        &input,
        &config(),
        &journal,
        &work_script(vec![("set_work_note", active_work("source"))]),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let mut saved = journal.load().await.unwrap().unwrap();
    saved.analysis = analysis;
    let before = digest(&saved.analysis).unwrap();
    *journal.state.lock().unwrap() = Some(saved);
    *journal.interrupt_after.lock().unwrap() = Some(3);
    agent::run(
        &input,
        &config(),
        &journal,
        &work_script(vec![
            (
                "check_gaps",
                json!({"scope":"analysis","offset":0,"limit":50}),
            ),
            ("check_gaps", json!({"scope":"work","offset":0,"limit":100})),
        ]),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let saved = journal.load().await.unwrap().unwrap();
    assert_eq!(
        digest(&saved.analysis).unwrap(),
        before,
        "diagnostics cannot establish evidence"
    );
    assert!(saved.pending_coverage.is_none());
    let pages: Vec<Value> = saved
        .transcript
        .iter()
        .filter(|m| m["role"] == "tool")
        .map(|m| {
            serde_json::from_str::<Value>(m["content"].as_str().unwrap()).unwrap()["result"].clone()
        })
        .filter(|v| v["items"].is_array())
        .collect();
    assert_eq!(pages.len(), 2);
    assert!(
        pages[0]["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|gap| gap["kind"] == "unread_source" && gap["source_id"] != "source")
    );
    let local = pages[1]["items"].as_array().unwrap();
    assert_eq!(pages[1]["next"], pages[1]["total"]);
    assert_eq!(
        local.len(),
        1,
        "only the missing disposition is a blocker; outcome references are host-maintained"
    );
    assert_eq!(local[0]["kind"], "missing_disposition");
    assert_eq!(local[0]["source_id"], "source");
    let mut complete = active_work("source");
    complete["status"] = json!("complete");
    *journal.interrupt_after.lock().unwrap() = Some(8);
    let model = work_script(vec![
        (
            "set_disposition",
            json!({"source_id":"source","state":"non_requirement","reason":"已读事实来源"}),
        ),
        ("set_work_note", complete),
        ("check_gaps", json!({"scope":"work","offset":0,"limit":10})),
        ("request_review", json!({})),
        ("set_work_note", active_work("other-source")),
    ]);
    agent::run(
        &input,
        &config(),
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let saved = journal.load().await.unwrap().unwrap();
    assert_eq!(saved.main_work.unwrap().source_scope, vec!["other-source"]);
    let bodies = model.bodies.lock().unwrap();
    let tool = bodies[3]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|m| m["role"] == "tool")
        .unwrap();
    let result: Value = serde_json::from_str(tool["content"].as_str().unwrap()).unwrap();
    assert_eq!(result["result"]["total"], 0);
    assert!(
        bodies[4]
            .to_string()
            .contains("structural/reading gaps remain"),
        "zero local blockers cannot authorize global review"
    );
}

#[test]
fn gap_queries_require_explicit_scope_and_reject_invalid_pages_without_evidence() {
    for reviewer in [false, true] {
        let mut analysis = Analysis::default();
        let mut coverage = Coverage::default();
        let before = digest(&coverage).unwrap();
        for query in [
            json!({"offset":0,"limit":10}),
            json!({"scope":"unknown","offset":0,"limit":10}),
            json!({"scope":"analysis","offset":0,"limit":0}),
            json!({"scope":"analysis","offset":usize::MAX,"limit":10}),
        ] {
            assert!(
                tools::invoke(
                    &input(),
                    &mut analysis,
                    &mut coverage,
                    reviewer,
                    "check_gaps",
                    &query,
                    16000
                )
                .is_err()
            );
            assert_eq!(digest(&coverage).unwrap(), before);
        }
        let schema = tools::schemas(reviewer)
            .into_iter()
            .find(|tool| tool["function"]["name"] == "check_gaps")
            .unwrap();
        assert!(
            schema["function"]["parameters"]["required"]
                .as_array()
                .unwrap()
                .contains(&json!("scope"))
        );
    }
}

#[tokio::test]
async fn rereading_completed_evidence_does_not_reopen_work_gaps_or_block_handoff() {
    struct InspectAndComplete;
    #[async_trait]
    impl Model for InspectAndComplete {
        async fn turn(&self, _: &Config, _: &[u8]) -> Result<ChatTurn, AgentError> {
            let mut complete = active_work("source");
            complete["status"] = json!("complete");
            complete["focus"]["action"] = json!("handoff");
            let calls = [
                (
                    "read_source",
                    json!({"source_id":"source","start":0,"max_bytes":1024}),
                ),
                (
                    "inspect_analysis",
                    json!({"view":"detail","kind":"all","offset":0,"limit":100}),
                ),
                ("check_gaps", json!({"scope":"work","offset":0,"limit":100})),
                ("set_work_note", complete),
            ];
            Ok(ChatTurn {
                finish_reason: "tool_calls".into(),
                tool_calls: calls
                    .into_iter()
                    .enumerate()
                    .map(|(i, (name, args))| ChatToolCall {
                        id: format!("repeat-{i}"),
                        name: name.into(),
                        arguments: args.to_string(),
                    })
                    .collect(),
                ..Default::default()
            })
        }
    }
    for role in [Role::Main, Role::Reviewer] {
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
        state.role = role;
        state.done = false;
        state.review = None;
        state.transcript.clear();
        state.pending_coverage = None;
        state.main_work = Some(serde_json::from_value(active_work("source")).unwrap());
        state.reviewer_work = state.main_work.clone();
        *journal.interrupt_after.lock().unwrap() = Some(state.turn + 1);
        *journal.state.lock().unwrap() = Some(state);
        agent::run(
            &input(),
            &config(),
            &journal,
            &InspectAndComplete,
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        let saved = journal.load().await.unwrap().unwrap();
        let work = if saved.role == Role::Main {
            saved.main_work.as_ref()
        } else {
            saved.reviewer_work.as_ref()
        };
        assert_eq!(
            json!(work.unwrap().status),
            "complete",
            "duplicate reads must not create a new completion obligation"
        );
    }
}

#[tokio::test]
async fn work_gap_diagnostics_cannot_count_same_batch_reads_as_reviewer_evidence() {
    struct ReadAndDiagnose;
    #[async_trait]
    impl Model for ReadAndDiagnose {
        async fn turn(&self, _: &Config, _: &[u8]) -> Result<ChatTurn, AgentError> {
            let calls = [
                (
                    "read_source",
                    json!({"source_id":"source","start":0,"max_bytes":1024}),
                ),
                (
                    "inspect_analysis",
                    json!({"view":"detail","kind":"all","offset":0,"limit":100}),
                ),
                ("check_gaps", json!({"scope":"work","offset":0,"limit":100})),
            ];
            Ok(ChatTurn {
                usage: None,
                content: String::new(),
                finish_reason: "tool_calls".into(),
                tool_calls: calls
                    .into_iter()
                    .enumerate()
                    .map(|(i, (name, args))| ChatToolCall {
                        id: format!("probe-{i}"),
                        name: name.into(),
                        arguments: args.to_string(),
                    })
                    .collect(),
            })
        }
    }
    let journal = fresh_review_journal().await;
    let mut saved = journal.load().await.unwrap().unwrap();
    saved.reviewer_work = Some(serde_json::from_value(active_work("source")).unwrap());
    let before = digest(&saved.reviewer_coverage).unwrap();
    let turn = saved.turn;
    *journal.state.lock().unwrap() = Some(saved);
    *journal.interrupt_after.lock().unwrap() = Some(turn + 1);
    agent::run(
        &input(),
        &config(),
        &journal,
        &ReadAndDiagnose,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let saved = journal.load().await.unwrap().unwrap();
    assert_eq!(digest(&saved.reviewer_coverage).unwrap(), before);
    assert!(
        saved
            .pending_coverage
            .as_ref()
            .unwrap()
            .text
            .contains_key("source")
    );
    let result: Value = serde_json::from_str(
        saved.transcript.last().unwrap()["content"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(result["ok"], true);
    let gaps = result["result"]["items"].as_array().unwrap();
    for kind in ["unread_source", "unreviewed_scope_outcome"] {
        assert!(
            gaps.iter().any(|g| g["kind"] == kind),
            "missing blocker {kind}"
        );
    }
}

#[tokio::test]
async fn work_handoff_cannot_drop_an_unresolved_outcome() {
    let mut input = input();
    input.source_units.push(Source {
        source_unit_revision_id: "other".into(),
        ordinal: 1,
        ..input.source_units[0].clone()
    });
    let mut complete = active_work("source");
    complete["status"] = json!("complete");
    let model = work_script(vec![
        ("set_work_note", active_work("source")),
        (
            "read_source",
            json!({"source_id":"source","start":0,"max_bytes":1024}),
        ),
        (
            "set_disposition",
            json!({"source_id":"source","state":"unresolved","reason":"引用目标尚未解析"}),
        ),
        ("set_work_note", complete),
        ("set_work_note", active_work("other")),
        (
            "check_gaps",
            json!({"scope":"pending","offset":0,"limit":10}),
        ),
    ]);
    let journal = MemoryJournal::default();
    *journal.interrupt_after.lock().unwrap() = Some(6);
    agent::run(
        &input,
        &config(),
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let state = journal.load().await.unwrap().unwrap();
    assert_eq!(
        state.main_work.as_ref().unwrap().source_scope,
        vec!["other"]
    );
    assert_eq!(
        state.main_work.as_ref().unwrap().pending_refs,
        vec!["disposition:source"]
    );
    let pending: Value = serde_json::from_str(
        state.transcript.last().unwrap()["content"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        pending["result"]["items"][0]["reference"],
        "disposition:source"
    );
}

#[tokio::test]
async fn reviewer_scope_completion_requires_its_own_source_and_candidate_delivery() {
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
    // Start a fresh review fixture over actual stored outcomes, with no inherited evidence.
    state.role = Role::Reviewer;
    state.done = false;
    state.transcript.clear();
    state.reviewer_work = None;
    state.reviewer_coverage = Coverage::default();
    state.pending_coverage = None;
    let turn = state.turn;
    *journal.state.lock().unwrap() = Some(state);
    *journal.interrupt_after.lock().unwrap() = Some(turn + 4);
    let mut complete = active_work("source");
    complete["status"] = json!("complete");
    let model = work_script(vec![
        ("set_work_note", active_work("source")),
        ("set_work_note", complete.clone()),
        (
            "read_source",
            json!({"source_id":"source","start":0,"max_bytes":1024}),
        ),
        ("set_work_note", complete),
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
    let state = journal.load().await.unwrap().unwrap();
    let last: Value = serde_json::from_str(
        state.transcript.last().unwrap()["content"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(last["ok"], false);
    assert!(
        last["error"]
            .as_str()
            .unwrap()
            .contains("independently inspect current scope outcome")
    );
    assert!(
        model.bodies.lock().unwrap()[2]
            .to_string()
            .contains("undelivered source ranges")
    );
    assert!(state.reviewer_coverage.candidate.is_empty());
}

#[tokio::test]
async fn source_reads_require_a_known_active_scope_and_explicit_cross_reference_expansion() {
    let mut input = input();
    input.source_units.push(Source {
        source_unit_revision_id: "other".into(),
        ordinal: 1,
        ..input.source_units[0].clone()
    });
    let mut expanded = active_work("source");
    expanded["source_scope"] = json!(["source", "other"]);
    let model = work_script(vec![
        (
            "read_source",
            json!({"source_id":"source","start":0,"max_bytes":1024}),
        ),
        ("set_work_note", active_work("source")),
        (
            "read_source",
            json!({"source_id":"other","start":0,"max_bytes":1024}),
        ),
        ("set_work_note", expanded),
        (
            "read_source",
            json!({"source_id":"other","start":0,"max_bytes":1024}),
        ),
    ]);
    let journal = MemoryJournal::default();
    *journal.interrupt_after.lock().unwrap() = Some(5);
    agent::run(
        &input,
        &config(),
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let state = journal.load().await.unwrap().unwrap();
    assert!(state.analysis.coverage.text.is_empty());
    assert_eq!(state.pending_coverage.as_ref().unwrap().text.len(), 1);
    assert!(
        state
            .pending_coverage
            .as_ref()
            .unwrap()
            .text
            .contains_key("other")
    );
    let bodies = model.bodies.lock().unwrap();
    for (index, reason) in [(1, "declare an active"), (3, "outside active")] {
        assert!(bodies[index]["messages"].to_string().contains(reason));
    }
}

#[tokio::test]
async fn completed_work_releases_old_source_below_the_context_ceiling_and_keeps_outcomes() {
    let mut input = input();
    input.source_units[0].text = "EARLIER_SOURCE_TEXT_甲乙".repeat(100);
    input.source_units.push(Source {
        source_unit_revision_id: "other".into(),
        ordinal: 1,
        text: "NEXT_SOURCE_TEXT_丙丁".into(),
        ..input.source_units[0].clone()
    });
    let mut complete = active_work("source");
    complete["status"] = json!("complete");
    let model = work_script(vec![
        ("set_work_note", active_work("source")),
        (
            "read_source",
            json!({"source_id":"source","start":0,"max_bytes":10000}),
        ),
        (
            "set_disposition",
            json!({"source_id":"source","state":"non_requirement","reason":"来源说明"}),
        ),
        ("set_work_note", complete),
        ("set_work_note", active_work("other")),
        (
            "read_source",
            json!({"source_id":"other","start":0,"max_bytes":10000}),
        ),
        (
            "check_gaps",
            json!({"scope":"analysis","offset":0,"limit":10}),
        ),
    ]);
    let journal = MemoryJournal::default();
    *journal.interrupt_after.lock().unwrap() = Some(7);
    agent::run(
        &input,
        &config(),
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    {
        let bodies = model.bodies.lock().unwrap();
        assert!(bodies[3].to_string().contains("EARLIER_SOURCE_TEXT_甲乙"));
        assert!(
            !bodies[4].to_string().contains("EARLIER_SOURCE_TEXT_甲乙"),
            "handoff must release text before the emergency ceiling"
        );
        assert!(bodies[6].to_string().contains("NEXT_SOURCE_TEXT_丙丁"));
    }
    let state = journal.load().await.unwrap().unwrap();
    assert!(state.analysis.dispositions.contains_key("source"));
    assert_eq!(state.analysis.coverage.text.len(), 2);
    assert_eq!(state.main_work.unwrap().source_scope, vec!["other"]);
}

#[tokio::test]
async fn request_work_checklist_projects_current_receipts_without_committing_them() {
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
    let mut state = journal.load().await.unwrap().unwrap();
    assert!(state.analysis.coverage.text.is_empty());
    assert!(
        state
            .pending_coverage
            .as_ref()
            .unwrap()
            .text
            .contains_key("source")
    );
    let before = digest(&state.analysis).unwrap();
    let receipts_before = digest(&state.pending_coverage).unwrap();
    let body: Value = serde_json::from_slice(
        &agent::request(&input(), &config(), &mut state)
            .await
            .unwrap(),
    )
    .unwrap();
    let tail: Value = serde_json::from_str(
        body["messages"].as_array().unwrap().last().unwrap()["content"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let checklist = &tail["work_state"];
    assert_eq!(checklist["gap_counts"]["missing_disposition"], 1);
    assert_eq!(checklist["next_action"], "resolve_work_gaps");
    assert!(
        checklist["gap_counts"].get("unread_source").is_none(),
        "already delivered request text should not invite another identical read"
    );
    assert!(checklist["gap_counts"].get("pending_delivery").is_none());
    assert!(serde_json::to_vec(checklist).unwrap().len() <= config().limits.max_tool_result_bytes);
    assert_eq!(digest(&state.analysis).unwrap(), before);
    assert_eq!(digest(&state.pending_coverage).unwrap(), receipts_before);
    assert!(
        tools::validate_span(&input(), &state.analysis.coverage, &span()).is_err(),
        "request metadata is not evidence authorization"
    );
    // The other role still needs its own reading, even when main's pending
    // receipts have subsequently been confirmed.
    state.analysis.coverage = state.pending_coverage.take().unwrap();
    state.role = Role::Reviewer;
    state.source_review = Some(source_review::initialize(&input(), &config()).unwrap());
    state.reviewer_work = state.main_work.clone();
    state.transcript.clear();
    let body: Value = serde_json::from_slice(
        &agent::request(&input(), &config(), &mut state)
            .await
            .unwrap(),
    )
    .unwrap();
    let tail: Value = serde_json::from_str(
        body["messages"].as_array().unwrap().last().unwrap()["content"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(tail["work_state"]["gap_counts"]["unread_source"], 1);
    assert_eq!(tail["work_state"]["next_action"], "resolve_work_gaps");
    assert!(state.reviewer_coverage.text.is_empty());
}

#[tokio::test]
async fn stalled_work_is_persisted_and_independent_sources_continue_after_restart() {
    use crate::agent_runtime::progress::Recovery;
    let mut source = input();
    source.source_units.push(Source {
        source_unit_revision_id: "independent".into(),
        ordinal: 1,
        ..source.source_units[0].clone()
    });
    let mut config = config();
    config.limits.max_no_progress_turns = 2;
    config.limits.max_focus_replans = 1;
    let journal = MemoryJournal::default();
    *journal.interrupt_after.lock().unwrap() = Some(4);
    let mut rename = active_work("source");
    rename["note"] = json!("same work, new note and objective");
    rename["objective"] = json!("renamed work");
    let calls = vec![
        ("set_work_note", active_work("source")),
        (
            "inspect_analysis",
            json!({"view":"index","kind":"all","offset":0,"limit":10}),
        ),
        ("set_work_note", rename),
        (
            "search_sources",
            json!({"query":"absent","offset":0,"limit":10}),
        ),
    ];
    agent::run(
        &source,
        &config,
        &journal,
        &work_script(calls),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let state = journal.load().await.unwrap().unwrap();
    assert_eq!(state.main_progress.watch.recovery, Recovery::Blocked);
    assert_eq!(state.main_progress.blockers.len(), 1);
    assert_eq!(state.main_progress.blockers[0].scope, vec!["source"]);
    assert!(
        state.analysis.dispositions.is_empty(),
        "runtime failures cannot become source uncertainty"
    );
    let restored = serde_json::from_value(json!(state)).unwrap();
    *journal.state.lock().unwrap() = Some(restored);
    *journal.interrupt_after.lock().unwrap() = Some(9);
    let model = work_script(vec![
        ("set_work_note", active_work("source")),
        ("set_work_note", active_work("independent")),
        (
            "read_source",
            json!({"source_id":"independent","start":0,"max_bytes":1024}),
        ),
        ("check_gaps", json!({"scope":"work","offset":0,"limit":10})),
        ("request_review", json!({})),
    ]);
    agent::run(
        &source,
        &config,
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let state = journal.load().await.unwrap().unwrap();
    assert_eq!(
        state.main_work.as_ref().unwrap().source_scope,
        vec!["independent"]
    );
    assert!(state.analysis.coverage.text.contains_key("independent"));
    assert_eq!(state.main_progress.blockers[0].watch.replans, 1);
    assert_eq!(state.main_progress.blockers.len(), 1);
    assert!(!state.done);
    assert_eq!(state.role, Role::Main);
    let last: Value = serde_json::from_str(
        state.transcript.last().unwrap()["content"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert!(
        last["error"]
            .as_str()
            .unwrap()
            .contains("execution blockers")
    );
    assert!(
        model
            .bodies
            .lock()
            .unwrap()
            .iter()
            .any(|body| body.to_string().contains("dependencies are unchanged"))
    );
}

#[tokio::test]
async fn all_blocked_sources_stop_before_reserving_a_pointless_handoff() {
    let config = config();
    let journal = MemoryJournal::default();
    let mut calls = vec![("set_work_note", active_work("source"))];
    calls.extend((0..30).map(|_| ("check_gaps", json!({"scope":"work","offset":0,"limit":10}))));
    let model = work_script(calls);
    let error = agent::run(
        &input(),
        &config,
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert!(
        error
            .message
            .contains("no independent source scope remains")
    );
    let state = journal.load().await.unwrap().unwrap();
    assert!(state.turn < config.limits.max_turns);
    assert_eq!(state.turn, 18);
    assert_eq!(model.bodies.lock().unwrap().len(), state.turn);
    assert_eq!(journal.reservations.lock().unwrap().len(), state.turn);
    assert_eq!(state.main_progress.blockers.len(), 1);
    assert!(
        state.journal.pending.is_none(),
        "stop at committed boundary, before another reservation"
    );
    assert!(!state.done);

    let mut changed = state.clone();
    changed.analysis.dispositions.insert(
        "source".into(),
        serde_json::from_value(json!({"state":"requirement","reason":"newly saved outcome"}))
            .unwrap(),
    );
    let changed_journal = MemoryJournal::default();
    *changed_journal.state.lock().unwrap() = Some(changed);
    *changed_journal.interrupt_after.lock().unwrap() = Some(state.turn + 1);
    let resume = work_script(vec![("set_work_note", active_work("source"))]);
    let error = agent::run(
        &input(),
        &config,
        &changed_journal,
        &resume,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "INTERNAL");
    assert_eq!(resume.bodies.lock().unwrap().len(), 1);
    let mut reviewer = state.clone();
    reviewer.role = Role::Reviewer;
    reviewer.reviewer_progress = state.main_progress.clone();
    *journal.state.lock().unwrap() = Some(reviewer);
    let no_model = work_script(vec![]);
    let error = agent::run(
        &input(),
        &config,
        &journal,
        &no_model,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert!(
        error
            .message
            .contains("no independent source scope remains")
    );
    assert!(no_model.bodies.lock().unwrap().is_empty());

    // An older compatible checkpoint can already contain a received response.
    // Commit it normally, then stop before creating any additional reservation.
    let mut received = state.clone();
    let body = agent::request(&input(), &config, &mut received)
        .await
        .unwrap();
    received
        .journal
        .prepare_session(
            &body,
            crate::agent_runtime::SESSION_PREFIX,
            crate::agent_runtime::ANALYSIS_SESSION_SUFFIX,
            config.limits.max_turns - received.turn,
            config.limits.max_context_bytes,
        )
        .unwrap();
    received
        .journal
        .prepare(received.turn, "main", &body)
        .unwrap();
    received
        .journal
        .responded(ChatTurn {
            content: String::new(),
            finish_reason: "tool_calls".into(),
            usage: None,
            tool_calls: vec![ChatToolCall {
                id: "saved-gap-check".into(),
                name: "check_gaps".into(),
                arguments: json!({"scope":"execution","offset":0,"limit":10}).to_string(),
            }],
        })
        .unwrap();
    *journal.state.lock().unwrap() = Some(received);
    let no_model = work_script(vec![]);
    let error = agent::run(
        &input(),
        &config,
        &journal,
        &no_model,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert!(
        error
            .message
            .contains("no independent source scope remains")
    );
    let committed = journal.load().await.unwrap().unwrap();
    assert_eq!(committed.turn, state.turn + 1);
    assert!(committed.journal.pending.is_none());
    assert!(no_model.bodies.lock().unwrap().is_empty());
    assert_eq!(journal.reservations.lock().unwrap().len(), state.turn);
    assert_eq!(json!(committed.analysis), json!(state.analysis));
    assert_eq!(
        json!(committed.main_progress.blockers),
        json!(state.main_progress.blockers)
    );
}
