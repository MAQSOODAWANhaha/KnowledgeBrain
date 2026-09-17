use super::*;
use crate::tender_analysis::{Applicability, ApplicabilityState, Record, RecordData};

fn obligation_fixture() -> (FrozenInput, Checkpoint, Value, Value) {
    let (input, mut state, output) = evidence_fixture();
    let grounds: Vec<Span> = serde_json::from_value(output["grounds"].clone()).unwrap();
    let record = Record {
        id: "template".into(),
        sources: grounds.clone(),
        data: RecordData::Template {
            label: "附表".into(),
            title: "投标函".into(),
            parent: None,
            order: Some(0),
            purpose: "投标文件应有此表".into(),
            applicability: Applicability {
                state: ApplicabilityState::Applicable,
                condition: "所有投标人".into(),
                scope: "投标文件".into(),
                grounds,
            },
            regions: vec![],
        },
    };
    state.analysis.records.insert(record.id.clone(), record);
    state.obligations = obligation_inventory(&state.analysis).unwrap();
    let obligation = json!({
        "item_id":state.obligations.keys().next().unwrap(),
        "conclusion":"pass", "grounds":output["grounds"],
        "output_ids":[output["item_id"]]
    });
    (input, state, output, obligation)
}

fn deliver_candidate(state: &mut Checkpoint) {
    let record = &state.analysis.records["template"];
    state
        .tender_coverage
        .candidate
        .insert(format!("record:{}", record.id), digest(record).unwrap());
}

#[test]
fn existing_output_passes_cannot_hide_an_unreviewed_tender_obligation() {
    let (input, mut state, output, obligation) = obligation_fixture();
    deliver_output(&mut state);
    deliver_tender(&input, &mut state);
    deliver_candidate(&mut state);
    put_review(&input, &mut state, &output).unwrap();
    state.done = true;
    assert!(state.completed_report(&input).is_none());
    put_review(&input, &mut state, &obligation).unwrap();
    assert_eq!(state.completed_report(&input).unwrap().status, "reviewed");
    // Removing both the missing check and the denominator entry must not pass.
    state
        .reviews
        .remove(obligation["item_id"].as_str().unwrap());
    state.obligations.clear();
    assert!(state.completed_report(&input).is_none());
}

#[test]
fn obligation_pass_requires_current_candidate_and_read_actual_output() {
    let (input, mut state, _, obligation) = obligation_fixture();
    deliver_tender(&input, &mut state);
    deliver_output(&mut state);
    assert!(put_review(&input, &mut state, &obligation).is_err());
    deliver_candidate(&mut state);
    for locations in [json!([]), json!(["invented-output"])] {
        let mut changed = obligation.clone();
        changed["output_ids"] = locations;
        assert!(put_review(&input, &mut state, &changed).is_err());
    }
    state.output_coverage.units.clear();
    assert!(put_review(&input, &mut state, &obligation).is_err());
    deliver_output(&mut state);
    put_review(&input, &mut state, &obligation).unwrap();
    state
        .analysis
        .records
        .get_mut("template")
        .unwrap()
        .sources
        .clear();
    assert!(put_review(&input, &mut state, &obligation).is_err());
}

#[test]
fn absent_obligation_needs_complete_search_and_retains_actual_finding() {
    let (input, mut state, output, mut obligation) = obligation_fixture();
    deliver_tender(&input, &mut state);
    deliver_candidate(&mut state);
    obligation["conclusion"] = json!("findings");
    obligation["output_ids"] = json!([]);
    obligation["findings"] = json!(["实际文件缺少要求的投标函附表"]);
    assert!(put_review(&input, &mut state, &obligation).is_err());
    deliver_output(&mut state);
    put_review(&input, &mut state, &output).unwrap();
    put_review(&input, &mut state, &obligation).unwrap();
    state.done = true;
    let report = state.completed_report(&input).unwrap();
    let id = obligation["item_id"].as_str().unwrap();
    assert_eq!(report.status, "reviewed_with_findings");
    assert_eq!(
        report.reviews[id].findings,
        ["实际文件缺少要求的投标函附表"]
    );
    assert_eq!(report.reviews[id].finding_ids.len(), 1);
    state.reviews.get_mut(id).unwrap().findings[0] = "篡改结论".into();
    assert!(state.completed_report(&input).is_none());
}

#[test]
fn parser_limitations_are_not_source_uncertainty_or_a_passing_review() {
    let (input, mut state, mut args) = evidence_fixture();
    state.inventory.units[0].kind = "not_checked".into();
    state.inventory.units[0].not_checked_reason = Some("unsupported image content".into());
    deliver_output(&mut state);
    deliver_tender(&input, &mut state);
    assert!(put_review(&input, &mut state, &args).is_err());
    args["conclusion"] = json!("source_limited");
    args["findings"] = json!(["未能检查图片"]);
    assert!(put_review(&input, &mut state, &args).is_err());
    args["conclusion"] = json!("not_checked");
    args["grounds"] = json!([]);
    put_review(&input, &mut state, &args).unwrap();
    state.done = true;
    assert_eq!(
        state.completed_report(&input).unwrap().status,
        "not_checked"
    );
}

#[test]
fn a_known_docx_hash_cannot_be_used_as_pdf_unit_identity() {
    let (input, mut state, mut args) = evidence_fixture();
    state.inventory.pdf_sha256 = Some("b".repeat(64));
    state.inventory.units[0].id = format!("pdf:{}:page:1", state.inventory.docx_sha256);
    state.inventory.units[0].kind = "pdf_page".into();
    args["item_id"] = json!(state.inventory.units[0].id);
    deliver_output(&mut state);
    deliver_tender(&input, &mut state);
    assert!(put_review(&input, &mut state, &args).is_err());
}

#[test]
fn cached_review_map_key_must_match_the_reviewed_item() {
    let (input, mut state, output, obligation) = obligation_fixture();
    deliver_output(&mut state);
    deliver_tender(&input, &mut state);
    deliver_candidate(&mut state);
    put_review(&input, &mut state, &output).unwrap();
    // A duplicated valid output judgment cannot satisfy the obligation key.
    state.reviews.insert(
        obligation["item_id"].as_str().unwrap().into(),
        state.reviews[output["item_id"].as_str().unwrap()].clone(),
    );
    state.done = true;
    assert!(state.completed_report(&input).is_none());
}

#[test]
fn an_exclusion_requires_frozen_inapplicability_and_does_not_exempt_actual_content() {
    let (input, mut state, mut output, mut obligation) = obligation_fixture();
    deliver_tender(&input, &mut state);
    deliver_candidate(&mut state);
    obligation["conclusion"] = json!("not_applicable");
    obligation["output_ids"] = json!([]);
    obligation["findings"] = json!(["冻结来源明确该附表不适用于本项目"]);
    assert!(put_review(&input, &mut state, &obligation).is_err());
    let RecordData::Template { applicability, .. } =
        &mut state.analysis.records.get_mut("template").unwrap().data
    else {
        unreachable!()
    };
    applicability.state = ApplicabilityState::NotApplicable;
    deliver_candidate(&mut state);
    put_review(&input, &mut state, &obligation).unwrap();
    assert!(!complete(&input, &state));
    deliver_output(&mut state);
    output["conclusion"] = json!("not_applicable");
    output["findings"] = json!(["不能以来源不适用跳过已有内容"]);
    assert!(put_review(&input, &mut state, &output).is_err());
    output["conclusion"] = json!("pass");
    output["findings"] = json!([]);
    put_review(&input, &mut state, &output).unwrap();
    state.done = true;
    assert_eq!(state.completed_report(&input).unwrap().status, "reviewed");
}

#[test]
fn a_source_limitation_requires_an_actual_frozen_uncertainty() {
    let (input, mut state, _, mut obligation) = obligation_fixture();
    deliver_tender(&input, &mut state);
    deliver_output(&mut state);
    deliver_candidate(&mut state);
    obligation["conclusion"] = json!("source_limited");
    obligation["findings"] = json!(["原文对附表适用对象存在歧义"]);
    assert!(put_review(&input, &mut state, &obligation).is_err());
    let RecordData::Template { applicability, .. } =
        &mut state.analysis.records.get_mut("template").unwrap().data
    else {
        unreachable!()
    };
    applicability.state = ApplicabilityState::Unknown;
    deliver_candidate(&mut state);
    put_review(&input, &mut state, &obligation).unwrap();
}

#[tokio::test]
async fn context_trims_whole_old_groups_but_never_discards_pending_evidence() {
    let (_, mut state, _) = evidence_fixture();
    let config = config();
    state.transcript = vec![
        json!({"role":"assistant","content":"old", "tool_calls":[{"id":"old","type":"function","function":{"name":"read_source","arguments":"{}"}}]}),
        json!({"role":"tool","tool_call_id":"old","content":"x".repeat(config.limits.max_context_bytes)}),
        json!({"role":"assistant","content":"current", "tool_calls":[{"id":"current","type":"function","function":{"name":"read_output_evidence","arguments":"{}"}}]}),
        json!({"role":"tool","tool_call_id":"current","content":"current evidence"}),
    ];
    state.pending_delivery = Some(delivery::PendingDelivery {
        view_refs: Vec::new(),
        output_coverage: OutputCoverage::default(),
        tender_coverage: Coverage::default(),
        messages: BTreeMap::from([("current".into(), digest(&"current evidence").unwrap())]),
    });
    let body: Value = serde_json::from_slice(&request(&mut state, &config).await.unwrap()).unwrap();
    assert_eq!(state.transcript.len(), 2);
    assert_eq!(body["messages"][3]["tool_call_id"], "current");
    assert!(state.pending_delivery.is_some());
    state.transcript[1]["content"] = json!("x".repeat(config.limits.max_context_bytes));
    assert_eq!(
        request(&mut state, &config).await.unwrap_err().code,
        "AGENT_TURN_BUDGET_EXCEEDED"
    );
    assert_eq!(state.transcript.len(), 2);
    assert!(state.pending_delivery.is_some());
}

#[tokio::test]
async fn token_limit_is_enforced_even_when_the_body_fits_byte_limit() {
    let (_, mut state, _) = evidence_fixture();
    let mut config = config();
    let body = request(&mut state, &config).await.unwrap();
    assert!(body.len() < config.limits.max_context_bytes);
    config.limits.max_context_tokens = config.provider.max_tokens as usize;
    assert_eq!(
        request(&mut state, &config).await.unwrap_err().code,
        "AGENT_TURN_BUDGET_EXCEEDED"
    );
}

#[tokio::test]
async fn oversized_read_cannot_grant_coverage_or_bypass_read_budget() {
    let (input, mut state, _) = evidence_fixture();
    let mut config = config();
    config.limits.max_read_bytes = 1;
    let response = ChatTurn {
        usage: None,
        content: String::new(),
        finish_reason: "tool_calls".into(),
        tool_calls: vec![knowledge::models::ChatToolCall {
            id: "read".into(),
            name: "read_output_evidence".into(),
            arguments: json!({"offset":0,"limit":8}).to_string(),
        }],
    };
    let result = execute_turn(
        &input,
        &config,
        &mut state,
        response.clone(),
        BTreeMap::new(),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(
        result[0]["content"]
            .as_str()
            .unwrap()
            .contains("remaining read budget")
    );
    assert!(state.output_coverage.units.is_empty());
    assert!(state.pending_delivery.is_none());
    assert_eq!(state.read_bytes, 0);
    execute_turn(
        &input,
        &super::config(),
        &mut state,
        response,
        BTreeMap::new(),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(state.read_bytes > 1);
    assert!(state.output_coverage.units.is_empty());
    assert!(state.pending_delivery.is_some());
}

#[tokio::test]
async fn repeated_navigation_is_charged_and_eventually_stops_without_progress() {
    let (input, mut state, _) = evidence_fixture();
    let mut config = config();
    config.limits.max_no_progress_turns = 1;
    config.limits.max_focus_turns = 1;
    config.limits.max_focus_replans = 1;
    for index in 0..4 {
        execute_turn(
            &input,
            &config,
            &mut state,
            ChatTurn {
                usage: None,
                content: String::new(),
                finish_reason: "tool_calls".into(),
                tool_calls: vec![knowledge::models::ChatToolCall {
                    id: format!("navigation-{index}"),
                    name: "read_review_obligations".into(),
                    arguments: json!({"offset":0,"limit":8}).to_string(),
                }],
            },
            BTreeMap::new(),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    }
    assert!(state.progress.handoff_exhausted(&config.limits.progress()));
    assert_eq!(state.tool_calls, 4);
    assert!(state.read_bytes > 0);
    assert!(!state.done);
}
