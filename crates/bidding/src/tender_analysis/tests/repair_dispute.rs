use super::*;

// Scripted semantics exercise the host protocol, never a real model's ability
// to detect an absent relation. This source is unrelated to acceptance PDFs.
fn source() -> FrozenInput {
    let mut input = input();
    input.source_units[0].text =
        "设备接口类型为光口。本条直接规定接口类型；本文件没有独立的接口参数表或其他参数引用行。"
            .into();
    input
}

fn original(input: &FrozenInput) -> Value {
    json!({"source_id":"source","start":0,"end":input.source_units[0].text.len()})
}

struct InitialExtraction(Value);
#[async_trait]
impl Model for InitialExtraction {
    async fn turn(&self, _: &Config, _: &[u8]) -> Result<ChatTurn, AgentError> {
        Ok(ChatTurn { content:String::new(), finish_reason:"tool_calls".into(), usage:None,
            tool_calls: vec![
                ChatToolCall {id:"initial-record".into(), name:"put_record".into(), arguments:self.0.to_string()},
                ChatToolCall {id:"initial-disposition".into(), name:"set_disposition".into(),
                    arguments:json!({"source_id":"source","state":"requirement","reason":"Direct interface requirement."}).to_string()},
            ] })
    }
}

async fn steps(
    input: &FrozenInput,
    config: &Config,
    journal: &MemoryJournal,
    calls: Vec<(&str, Value)>,
) -> (Checkpoint, Script) {
    let start = journal.load().await.unwrap().map_or(0, |s| s.turn);
    *journal.interrupt_after.lock().unwrap() = Some(start + calls.len());
    let model = work_script(calls);
    let error = agent::run(input, config, journal, &model, &CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(error.code, "INTERNAL", "{error:?}");
    (journal.load().await.unwrap().unwrap(), model)
}

#[tokio::test]
async fn absent_parameter_relation_dispute_reaches_independent_withdrawal_and_source_acceptance() {
    let input = source();
    let citation = original(&input);
    let mut config = config();
    config.limits.max_turns = 80;
    config.limits.max_tool_calls = 160;
    let journal = MemoryJournal::default();
    let requirement = json!({"id":null,"sources":[citation],"data":{
        "kind":"requirement","text":"设备接口类型为光口；本条直接规定，无独立参数引用行。",
        "categories":["technical"],"strength":"mandatory",
        "compliance":[{"policy":"must_comply","condition":"本条直接规定","grounds":[citation]}],
        "applicability":{"state":"applicable","condition":"本条直接规定","scope":"设备接口","grounds":[citation]},
        "response":[],"proofs":[],"criteria":[],"scoring_rule":null}});
    let read = json!({"source_id":"source","start":0,"max_bytes":1024});
    let records = json!({"kind":"all","view":"detail","offset":0,"limit":10});
    let dispositions = json!({"kind":"disposition","view":"detail","offset":0,"limit":10});
    let (before, _) = steps(
        &input,
        &config,
        &journal,
        vec![
            ("set_work_note", active_work("source")),
            ("read_source", read.clone()),
        ],
    )
    .await;
    // Creation and disposition share one response: the host-allocated record
    // detail has not been delivered back to Main when independent review starts.
    *journal.interrupt_after.lock().unwrap() = Some(before.turn + 1);
    let error = agent::run(
        &input,
        &config,
        &journal,
        &InitialExtraction(requirement),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "INTERNAL");
    let initial = journal.load().await.unwrap().unwrap();
    let record_id = initial.analysis.records.keys().next().unwrap().clone();
    let record_ref = format!("record:{record_id}");
    let finding = json!({"code":"MISSING_PARAMETER_RELATION",
        "message":"An interface-parameter row was not linked.",
        "correction":"Create a references edge to the separate interface-parameter row.",
        "affected":[{"id":record_id,"path":"/data"}],"sources":[citation]});
    let (saved, _) = steps(
        &input,
        &config,
        &journal,
        vec![
            ("set_work_note", active_work("source")),
            ("read_source", read.clone()),
            ("inspect_analysis", records.clone()),
            ("inspect_analysis", dispositions.clone()),
            ("put_review_finding", json!({"id":null,"finding":finding})),
            ("put_source_review", json!({"fixture_status":"findings"})),
        ],
    )
    .await;
    assert_eq!(saved.role, Role::Main);
    assert!(!saved.done);
    assert_eq!(saved.review.as_ref().unwrap().findings.len(), 1);
    let id = saved.review_draft.keys().next().unwrap().clone();
    let finding_sha = digest(&saved.review_draft[&id]).unwrap();
    let graph = json!([
        saved.analysis.records,
        saved.analysis.relations,
        saved.analysis.dispositions
    ]);
    let prior_review = json!(saved.review);
    let summary = "The complete original explicitly states there is no separate parameter row; the interface type is specified directly. Withdraw the unsupported relation request only after independent verification.";
    let dispute = json!({"finding_sha256":finding_sha,"conclusion":"disputed","summary":summary,"sources":[citation],"candidate_refs":[record_ref]});
    let (unread, _) = steps(
        &input,
        &config,
        &journal,
        vec![
            ("inspect_review", json!({"offset":0,"limit":10})),
            ("delete_review_finding", json!({"id":id})), // Main cannot withdraw its reviewer's finding.
            ("put_repair_result", dispute.clone()),
        ],
    )
    .await;
    assert_eq!(unread.role, Role::Main);
    assert!(
        unread.repair.results.is_empty(),
        "a referenced affected candidate requires its own current detail receipt"
    );
    assert!(unread.review_draft.contains_key(&id));
    let missing_detail: Value = serde_json::from_str(
        unread.transcript.last().unwrap()["content"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(missing_detail["ok"], false);
    assert!(
        missing_detail["error"]
            .as_str()
            .unwrap()
            .contains("inspect the current repaired candidate detail")
    );
    let (handoff, _) = steps(
        &input,
        &config,
        &journal,
        vec![
            (
                "inspect_analysis",
                json!({"kind":"record","view":"detail","ids":[record_id],"offset":0,"limit":1}),
            ),
            ("put_repair_result", dispute),
            ("request_review", json!({})),
        ],
    )
    .await;
    assert_eq!(handoff.role, Role::Reviewer);
    assert!(!handoff.done, "a dispute is not independent acceptance");
    assert_eq!(json!(handoff.review), prior_review);
    assert!(handoff.review_draft.contains_key(&id));
    assert_eq!(handoff.repair.results.len(), 1);
    assert!(
        !source_review::pending(&input, &config, &handoff)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        json!([
            handoff.analysis.records,
            handoff.analysis.relations,
            handoff.analysis.dispositions
        ]),
        graph
    );

    // Resume from the serialized handoff, retaining receipts and all counters.
    *journal.state.lock().unwrap() = Some(serde_json::from_value(json!(handoff)).unwrap());
    let (reviewing, model) = steps(
        &input,
        &config,
        &journal,
        vec![
            ("set_work_note", active_work("source")),
            ("read_source", read),
            ("inspect_analysis", records),
            ("inspect_analysis", dispositions),
            ("inspect_review", json!({"offset":0,"limit":10})),
            ("put_source_review", json!({})), // Negative: retained finding forbids a clean judgment.
        ],
    )
    .await;
    assert!(!reviewing.done);
    assert!(reviewing.review_draft.contains_key(&id));
    let omitted_finding: Value = serde_json::from_str(
        reviewing.transcript.last().unwrap()["content"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(omitted_finding["ok"], false);
    assert!(
        omitted_finding["error"]
            .as_str()
            .unwrap()
            .contains("missing_finding_ids")
    );
    let request = model.bodies.lock().unwrap().last().unwrap().clone();
    let delivered: Vec<Value> = request["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|m| serde_json::from_str(m["content"].as_str()?).ok())
        .collect();
    assert!(
        delivered.iter().any(
            |p| p["result"]["items"]
                .as_array()
                .is_some_and(|items| items.iter().any(|item| item["id"] == id
                    && item["finding"] == finding
                    && item["main_repair"]["conclusion"] == "disputed"
                    && item["main_repair"]["summary"] == summary
                    && item["main_repair"]["independent_approval"] == false))
        )
    );

    // Retaining the original concern cannot accept the unchanged graph, even
    // when the reviewer saves a findings judgment for the original source.
    let retained = MemoryJournal::default();
    *retained.state.lock().unwrap() = Some(serde_json::from_value(json!(reviewing)).unwrap());
    let (rejected, _) = steps(
        &input,
        &config,
        &retained,
        vec![
            ("put_review_finding", json!({"id":id,"finding":finding})),
            ("put_source_review", json!({"fixture_status":"findings"})),
        ],
    )
    .await;
    assert!(!rejected.done);
    assert!(rejected.review_draft.contains_key(&id));
    assert!(!rejected.review.as_ref().unwrap().findings.is_empty());
    assert!(
        rejected
            .source_review
            .as_ref()
            .unwrap()
            .results
            .values()
            .any(|result| json!(result.judgment.status) == "findings"
                && result.judgment.finding_ids.contains(&id))
    );

    // Positive: only the independent reviewer withdraws its unsupported claim,
    // then completes fresh candidate and source judgments through production tools.
    let result = agent::run(
        &input,
        &config,
        &journal,
        &work_script(vec![
            ("delete_review_finding", json!({"id":id})),
            ("put_source_review", json!({})),
        ]),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    let accepted = journal.load().await.unwrap().unwrap();
    assert!(accepted.done);
    assert!(accepted.review_draft.is_empty());
    assert!(result.review.findings.is_empty());
    assert!(agent::review_complete(&input, &config, &accepted).unwrap());
    assert_eq!(
        json!([
            accepted.analysis.records,
            accepted.analysis.relations,
            accepted.analysis.dispositions
        ]),
        graph
    );
    assert!(
        accepted.analysis.relations.is_empty(),
        "no invented target or edge was needed"
    );
    crate::docx_composition::validate_basis(&input, &result).unwrap();
}
