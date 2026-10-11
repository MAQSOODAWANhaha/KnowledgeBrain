// A host receipt failure is not feedback the model can repair by resubmitting.
#[tokio::test]
async fn source_confirmation_failure_stops_same_frame_after_reload_and_keeps_independent_check() {
    let (input, config, mut journal) = fixture(2);
    journal.stop_barrier = false;
    let mut state = journal.state.lock().unwrap().clone();
    let cancel = CancellationToken::new();
    let mut host = PackHost {
        input: &input,
        config: &config,
        state: &mut state,
        journal: &journal,
        cancel: &cancel,
    };
    let request = host.next(&BTreeSet::new()).await.unwrap().unwrap();
    host.reserve(&request).await.unwrap();
    let response = Delayed {
        delay_ms: 0,
        ..Default::default()
    }
    .turn(&config, &request.body)
    .await
    .unwrap();
    host.received(&request, &response).await.unwrap();
    // Host-owned frame has invalid immutable provenance. No response edit can
    // repair it, and failure must not be misclassified as a document issue.
    host.state
        .outline_run
        .discover_workers
        .get_mut(&request.scope.pack)
        .unwrap()
        .related_read_frames
        .push(coordinator::RelatedReadFrame {
            directory_page: None,
            call_id: "synthetic-frame".into(),
            pack_revision: request.scope.revision,
            input_digest: "wrong-input".into(),
            options: vec![],
            sealed_sha256: None,
        });
    host.commit(&request, response).await.unwrap();
    let failed = &host.state.outline_run.discover_workers[&request.scope.pack];
    assert!(
        matches!(&failed.pending, Some(Pending::Failed { request: saved, code })
        if saved == &request && code == "DISCOVER_SOURCE_CONFIRMATION_FAILED")
    );
    assert_eq!(
        failed.rejected_turns, 0,
        "host fault is not a model rejection"
    );
    assert_eq!(failed.related_read_frames.len(), 1);
    assert!(
        failed.history.last().unwrap()["content"]
            .as_str()
            .unwrap()
            .contains("DISCOVER_SOURCE_CONFIRMATION_FAILED")
    );
    let audit = serde_json::to_value(failed).unwrap();
    let accounting = serde_json::to_value(&host.state.journal.accounting).unwrap();
    let mut restored = journal.load().await.unwrap().unwrap();
    let mut host = PackHost {
        input: &input,
        config: &config,
        state: &mut restored,
        journal: &journal,
        cancel: &cancel,
    };
    host.replay_received().await.unwrap();
    let independent = host.next(&BTreeSet::new()).await.unwrap().unwrap();
    assert_ne!(independent.scope.pack, request.scope.pack);
    assert_eq!(
        serde_json::to_value(&host.state.journal.accounting).unwrap(),
        accounting
    );
    host.reserve(&independent).await.unwrap();
    let response = Delayed {
        delay_ms: 0,
        ..Default::default()
    }
    .turn(&config, &independent.body)
    .await
    .unwrap();
    host.received(&independent, &response).await.unwrap();
    host.commit(&independent, response).await.unwrap();
    assert!(host.next(&BTreeSet::new()).await.unwrap().is_none());
    assert_eq!(
        serde_json::to_value(&host.state.outline_run.discover_workers[&request.scope.pack])
            .unwrap(),
        audit
    );
    assert!(!host.all_committed());
    assert_eq!(
        crate::outline::partial_source::eligible(&input, host.state),
        Some(BTreeSet::from([independent.scope.pack.clone()]))
    );
    assert!(
        host.state
            .outline_run
            .tool_draft
            .check_reads
            .evidence
            .is_empty()
    );
    crate::outline::agent::synchronize_discovery(&input, host.state).unwrap();
    assert_eq!(
        crate::outline::agent::current(&input, host.state),
        crate::outline::agent::Duty::Check
    );
    let pack = independent.scope.pack.clone();
    let scope = host.state.outline_run.tool_draft.source_scopes[&pack].clone();
    let version = crate::outline::source_review::version(
        &input,
        &host.state.outline_run.tool_draft,
        &pack,
        &scope,
    )
    .unwrap();
    // Explicit receipt seam; actual Check submission/gates execute below.
    // This domain test does not claim a model saw a transport read response.
    host.state.outline_run.tool_draft.check_reads.evidence = scope.evidence.clone();
    host.state.outline_run.tool_draft.check_reads.structure_keys = scope.structure_keys.clone();
    let refs = scope
        .evidence
        .iter()
        .map(|reference| {
            json!({"review_evidence_key":
        crate::outline::agent::review_evidence_key(host.state, reference).unwrap()})
        })
        .collect::<Vec<_>>();
    let args = json!({"requirement_ids":[],"pack_ids":[pack],"inspected_evidence":refs,
        "issues":[],"template_reviews":[],"source_dispositions":[{"pack_id":pack,
        "version":version,"evidence":refs,"verdict":"no_response_obligation",
        "requirement_ids":[],"reason":"Synthetic protocol-only scope independently checked."}]});
    crate::outline::agent::apply_for_duty(
        &input,
        host.state,
        "submit_review",
        &args,
        crate::outline::agent::Duty::Check,
    )
    .unwrap();
    assert!(
        crate::outline::source_review::complete(&input, &host.state.outline_run.tool_draft, &pack)
            .unwrap()
    );
    host.save().await.unwrap();
    let reviewed = journal.load().await.unwrap().unwrap();
    assert!(
        crate::outline::source_review::complete(&input, &reviewed.outline_run.tool_draft, &pack)
            .unwrap()
    );
    assert!(!reviewed.done);
    assert!(
        !reviewed
            .outline_run
            .reading_packs
            .as_ref()
            .unwrap()
            .complete()
    );
}

#[tokio::test]
async fn repeated_pending_related_frame_is_not_new_host_coverage() {
    use crate::outline::discover::{ConditionSupport, ConditionSupportOrigin, DiscoverWork};
    let input = crate::outline::frozen::tests::related_native_input();
    let mut config = crate::analysis::tests::config();
    config.limits.max_no_progress_turns = 2;
    let mut state = super::super::retirement::checkpoint(&input);
    let mut work = DiscoverWork::plan_with_budget(&input, &|sessions| {
        Ok(sessions
            .iter()
            .all(|s| s["pack"]["document_ids"].as_array().unwrap().len() == 1))
    });
    work.claim(2);
    let refs = work
        .dependency_selection(&input, "pack-0", "pricing-reference")
        .unwrap();
    work.bind_condition_support(
        "pack-0",
        vec![ConditionSupport {
            origin: ConditionSupportOrigin::ReviewedRequirement {
                requirement_id: "previously-reviewed".into(),
            },
            review_version: "version".into(),
            evidence: refs,
        }],
    )
    .unwrap();
    state.outline_run.reading_packs = Some(work);
    state
        .outline_run
        .discover_workers
        .insert("pack-0".into(), Worker::default());
    let journal = Durable {
        state: Mutex::new(state.clone()),
        stop_barrier: false,
        crash_received: AtomicBool::new(false),
        call_limit: 99,
    };
    let cancel = CancellationToken::new();
    let mut host = PackHost {
        input: &input,
        config: &config,
        state: &mut state,
        journal: &journal,
        cancel: &cancel,
    };
    let request = host.prepare_inner("pack-0", false).await.unwrap();
    let response = ChatTurn {
        content: String::new(),
        finish_reason: "tool_calls".into(),
        usage: None,
        tool_calls: vec![knowledge::models::ChatToolCall {
            id: "read-current-original".into(),
            name: "read_evidence".into(),
            arguments: "{}".into(),
        }],
    };
    let key = host
        .state
        .outline_run
        .reading_packs
        .as_ref()
        .unwrap()
        .session(&input, "pack-0")
        .unwrap()["pack"]["condition_support_options"][0]["support_key"]
        .as_str()
        .unwrap()
        .to_owned();
    let first = host
        .related_page(&request, &response, &json!({"dependency_id":key}))
        .await
        .unwrap();
    assert_eq!(
        first["read"]["new_coverage"], true,
        "first queue is new work"
    );
    let pending_before = host.state.outline_run.discover_workers["pack-0"]
        .related_read_frames
        .clone();
    let repeated = host
        .related_page(&request, &response, &json!({"dependency_id":key}))
        .await
        .unwrap();
    assert_eq!(
        repeated["read"]["new_coverage"], false,
        "same not-yet-delivered frame cannot reset the no-progress counter"
    );
    assert!(
        host.state.outline_run.discover_workers["pack-0"]
            .delivered_related_evidence
            .is_empty()
    );
    assert!(!pending_before.is_empty());
    // One selected range is covered jointly, neither half suffices alone.
    let worker = host
        .state
        .outline_run
        .discover_workers
        .get_mut("pack-0")
        .unwrap();
    worker.related_read_frames = pending_before;
    let full = worker.related_read_frames[0].options[0].evidence[0].clone();
    let mut left = serde_json::to_value(&full).unwrap();
    let mut right = left.clone();
    let start = left["start_byte"].as_u64().unwrap();
    let end = left["end_byte"].as_u64().unwrap();
    assert!(end > start + 1);
    let middle = start + 1; // Fixture is ASCII; preserve a valid UTF-8 boundary.
    left["end_byte"] = json!(middle);
    right["start_byte"] = json!(middle);
    worker
        .delivered_related_evidence
        .push(serde_json::from_value(left).unwrap());
    worker.related_read_frames[0].options[0].evidence[0] = serde_json::from_value(right).unwrap();
    let mixed = host
        .related_page(&request, &response, &json!({"dependency_id":key}))
        .await
        .unwrap();
    assert_eq!(
        mixed["read"]["new_coverage"], false,
        "delivered prefix plus pending suffix jointly cover the selected range"
    );
}

#[tokio::test]
async fn worker_wire_deduplicates_only_adjacent_identical_plain_observations() {
    let (input, config, mut journal) = fixture(1);
    journal.stop_barrier = false;
    let mut state = journal.state.lock().unwrap().clone();
    let cancel = CancellationToken::new();
    let mut host = PackHost {
        input: &input,
        config: &config,
        state: &mut state,
        journal: &journal,
        cancel: &cancel,
    };
    let first = host.next(&BTreeSet::new()).await.unwrap().unwrap();
    let plain = json!({"role":"assistant","content":"Synthetic identical observation."});
    let unique = json!({"role":"assistant","content":"Synthetic distinct observation."});
    let group = json!({"role":"assistant","content":"Synthetic identical observation.","tool_calls":[{"id":"unconsumed","type":"function","function":{"name":"read_evidence","arguments":"{}"}}]});
    let result = json!({"role":"tool","tool_call_id":"unconsumed","content":"{\"ok\":true}"});
    let spaced = json!({"role":"assistant","content":"Synthetic identical observation. "});
    let composed = json!({"role":"assistant","content":"Synthetic café"});
    let decomposed = json!({"role":"assistant","content":"Synthetic cafe\u{301}"});
    let history = vec![
        plain.clone(),
        plain.clone(),
        unique.clone(),
        plain.clone(),
        spaced.clone(),
        composed.clone(),
        decomposed.clone(),
        group.clone(),
        result.clone(),
    ];
    host.state
        .outline_run
        .discover_workers
        .get_mut(&first.scope.pack)
        .unwrap()
        .history = history.clone();
    let request = host.prepare_inner(&first.scope.pack, false).await.unwrap();
    let wire: Value = serde_json::from_slice(&request.body).unwrap();
    let messages = wire["messages"].as_array().unwrap();
    let observations = messages
        .iter()
        .filter_map(synthetic_plain_assistant_text)
        .collect::<Vec<_>>();
    assert_eq!(
        observations,
        vec![
            "Synthetic identical observation.",
            "Synthetic distinct observation.",
            "Synthetic identical observation.",
            "Synthetic identical observation. ",
            "Synthetic café",
            "Synthetic cafe\u{301}",
        ],
        "complete text and order, including Unicode and whitespace distinctions, must survive final wire"
    );
    assert!(messages.iter().any(|m| {
        m["tool_calls"]
            .as_array()
            .is_some_and(|calls| calls.iter().any(|call| call["id"] == "unconsumed"))
    }));
    assert!(messages.iter().any(|m| m["tool_call_id"] == "unconsumed"));
    assert_eq!(
        host.state.outline_run.discover_workers[&first.scope.pack].history,
        history
    );
    assert_eq!(
        project_settled_submission_history(&history),
        history,
        "shared main projection semantics are unchanged"
    );
}

// SDK wire may represent text as typed content blocks and emit empty tool_calls.
// Fail loudly on unexpected content instead of silently dropping observations.
fn synthetic_plain_assistant_text(message: &Value) -> Option<String> {
    if message["role"] != "assistant" {
        return None;
    }
    if message
        .get("tool_calls")
        .is_some_and(|calls| !calls.is_null() && !calls.as_array().is_some_and(Vec::is_empty))
    {
        return None;
    }
    match &message["content"] {
        Value::String(text) => Some(text.clone()),
        Value::Array(blocks) => Some(
            blocks
                .iter()
                .map(|block| {
                    assert_eq!(
                        block["type"], "text",
                        "synthetic observation must contain only text blocks"
                    );
                    block["text"]
                        .as_str()
                        .expect("text block string")
                        .to_owned()
                })
                .collect::<String>(),
        ),
        other => panic!("unexpected synthetic assistant content shape: {other}"),
    }
}
