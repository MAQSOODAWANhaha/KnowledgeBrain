use super::*;

#[test]
fn correction_window_configuration_and_receipt_authority() {
    let mut config = crate::analysis::tests::config();
    assert_eq!(config.limits.request_context_limit(true), 262144);
    config.limits.max_correction_context_tokens = Some(262144);
    config.validate().unwrap();
    assert_eq!(config.limits.request_context_limit(false), 131072);
    assert_eq!(config.limits.request_context_limit(true), 262144);
    for denied in [131071, 262145] {
        config.limits.max_correction_context_tokens = Some(denied);
        assert!(config.validate().is_err());
    }
    let mut history = vec![
        json!({"role":"assistant","tool_calls":[{"id":"a","function":{"name":"submit_pack","arguments":"{\"repair\":true}"}}]}),
        json!({"role":"tool","tool_call_id":"a","content":"{\"ok\":false,\"error\":\"BUSINESS_REJECTED\"}"}),
    ];
    assert!(correction_retry(&history));
    history[1]["tool_call_id"] = json!("foreign");
    assert!(!correction_retry(&history));
    history[1]["tool_call_id"] = json!("a");
    history.push(json!({"role":"assistant","tool_calls":[{"id":"b","function":{"name":"read_evidence","arguments":"{}"}}]}));
    history.push(json!({"role":"tool","tool_call_id":"b","content":"{\"ok\":true,\"result\":{}}"}));
    assert!(
        !correction_retry(&history),
        "old failure cannot widen ordinary reads"
    );
    assert!(!correction_retry(&[
        json!({"role":"user","content":"repair=true"})
    ]));
}

#[test]
fn sizing_placeholder_preserves_deferred_wire_reserve_without_failure_authority() {
    use crate::agent_runtime::chat::TokenEncoding;
    for encoding in [TokenEncoding::O200kBase, TokenEncoding::Cl100kBase] {
        let mut config = crate::analysis::tests::config();
        config.limits.tokenizer.encoding = encoding;
        for id in ["call-a", "call-长标识-123", "call_0000000000000000000000"] {
            let actual = deferred_message(id);
            let pending = sizing_message(id);
            assert!(
                serde_json::to_vec(&pending).unwrap().len()
                    >= serde_json::to_vec(&actual).unwrap().len()
            );
            let assistant = json!({"role":"assistant","tool_calls":[{"id":id,"type":"function","function":{"name":"put_chapters","arguments":"{}"}}]});
            let wire = |receipt: Value| json!({"model":config.provider.model_id,"messages":[{"role":"system","content":"fixture"},assistant.clone(),receipt],"tools":[]});
            assert!(
                crate::analysis::tests::request_tokens(&wire(pending.clone()), &config)
                    >= crate::analysis::tests::request_tokens(&wire(actual.clone()), &config)
            );
            assert!(!correction_retry(&[assistant.clone(), pending]));
            assert!(correction_retry(&[assistant, actual]));
        }
    }
}

#[tokio::test]
async fn fit_batch_pending_receipts_cannot_expand_ordinary_reads_but_real_failures_can() {
    let input = super::token_transport_validation::synthetic_input(1, 1);
    let mut config = crate::analysis::tests::config();
    config.limits.max_correction_context_tokens = Some(262144);
    let mut state = super::retirement::checkpoint(&input);
    let mut work = crate::outline::discover::DiscoverWork::plan(&input, 8000);
    for pack in work.claim(99) {
        work.submit(
            &input,
            &pack.id,
            crate::outline::discover::PackSubmit {
                call_id: format!("fixture-{}", pack.id),
                claim_token: pack.claim_token.clone(),
                pack_revision: pack.pack_revision,
                requirements: vec![],
                issues: vec![],
                no_requirement_reason: Some("Synthetic protocol fixture".into()),
                inspected_atom_ids: pack.atoms.iter().map(|atom| atom.id.clone()).collect(),
            },
        )
        .unwrap();
    }
    assert!(work.complete());
    state.outline_run.reading_packs = Some(work);
    state.transcript = vec![
        json!({"role":"assistant","tool_calls":[
            {"id":"read","type":"function","function":{"name":"read_evidence","arguments":"{}"}},
            {"id":"write","type":"function","function":{"name":"put_chapters","arguments":"{}"}}
        ]}),
        json!({"role":"tool","tool_call_id":"read","content":json!({"ok":true,"result":{"text":" token".repeat(140000)}}).to_string()}),
    ];
    let pending = [knowledge::models::ChatToolCall {
        id: "write".into(),
        name: "put_chapters".into(),
        arguments: "{}".into(),
    }];
    let before = serde_json::to_value(&state).unwrap();
    assert_eq!(
        fit_batch(&input, &config, &mut state, &pending, &[])
            .await
            .unwrap_err()
            .code,
        "AGENT_TURN_BUDGET_EXCEEDED"
    );
    assert_eq!(
        serde_json::to_value(&state).unwrap(),
        before,
        "sizing must not change receipts or registry"
    );

    // A completed, real deferral still authorizes the next correction. Measure
    // the complete production request, including prompt, tools and output.
    let mut real = state.clone();
    real.transcript.push(deferred_message("write"));
    fit_batch(&input, &config, &mut real, &[], &[])
        .await
        .unwrap();
    let wire = prepare_request(&input, &config, &mut real, false)
        .await
        .unwrap();
    let total = crate::analysis::tests::total_request_tokens(&wire, &config);
    assert!(
        total > 131072 && total <= 262144,
        "actual full request total={total}"
    );

    // An earlier completed failure in the current group is authoritative even
    // while another tool is still pending; only synthetic failure is excluded.
    let mut failed = state.clone();
    failed.transcript[1]["content"] = json!({"ok":false,"error":" token".repeat(140000)})
        .to_string()
        .into();
    fit_batch(&input, &config, &mut failed, &pending, &[])
        .await
        .unwrap();

    // The caller can admit a smaller page and finish both tools successfully
    // within the original window, without manufacturing a last-tool deferral.
    state.transcript[1]["content"] = json!({"ok":true,"result":{"text":" token".repeat(1000)}})
        .to_string()
        .into();
    fit_batch(&input, &config, &mut state, &pending, &[])
        .await
        .unwrap();
    state
        .transcript
        .push(json!({"role":"tool","tool_call_id":"write","content":"{\"ok\":true}"}));
    fit_batch(&input, &config, &mut state, &[], &[])
        .await
        .unwrap();
    assert!(!correction_retry(&state.transcript));
}

#[tokio::test]
async fn real_evidence_pages_keep_pending_batch_in_ordinary_window_and_preserve_last_success() {
    use crate::outline::agent::Duty;
    let text = " token".repeat(140_000);
    let input = super::token_transport_validation::synthetic_input_with_text(text.clone(), 1);
    let mut config = crate::analysis::tests::config();
    config.limits.max_correction_context_tokens = Some(262144);
    let mut state = super::retirement::checkpoint(&input);
    // Discovery is already complete: exercise ordinary Organize source reads,
    // without building a large project fixture or using an external service.
    let mut work = crate::outline::discover::DiscoverWork::plan_with_budget(&input, &|_| Ok(true));
    for pack in work.claim(99) {
        work.submit(
            &input,
            &pack.id,
            crate::outline::discover::PackSubmit {
                call_id: format!("fixture-{}", pack.id),
                claim_token: pack.claim_token.clone(),
                pack_revision: pack.pack_revision,
                requirements: vec![],
                issues: vec![],
                no_requirement_reason: Some("Synthetic pagination fixture".into()),
                inspected_atom_ids: pack.atoms.iter().map(|atom| atom.id.clone()).collect(),
            },
        )
        .unwrap();
    }
    assert!(work.complete());
    state.outline_run.reading_packs = Some(work);
    let mut args = json!({"refs":[{
        "kind":"text", "input_digest":crate::outline::evidence::input_digest(&input).unwrap(),
        "unit_id":input.source_units[0].source_unit_revision_id,
        "start_byte":0, "end_byte":text.len()
    }]});
    let mut restored = String::new();
    let mut pages = 0;
    loop {
        pages += 1;
        assert!(pages <= 8, "pagination must make bounded progress");
        let calls = [
            knowledge::models::ChatToolCall {
                id: format!("read-{pages}"),
                name: "read_evidence".into(),
                arguments: args.to_string(),
            },
            knowledge::models::ChatToolCall {
                id: format!("last-{pages}"),
                name: "read_outline".into(),
                arguments: json!({"mode":"chapters"}).to_string(),
            },
        ];
        state.transcript.push(json!({"role":"assistant","tool_calls":calls.iter().map(|call|
            json!({"id":call.id,"type":"function","function":{"name":call.name,"arguments":call.arguments}})
        ).collect::<Vec<_>>()}));
        let before = state.outline_run.tool_draft.clone();
        let page = read_evidence_in_context(
            &input,
            &config,
            &mut state,
            &args,
            &calls,
            &[],
            Duty::Organize,
        )
        .await
        .unwrap();
        if pages == 1 {
            assert_eq!(
                page["selection_complete"], false,
                "the real pagination loop must shrink the oversized source even with a pending tool"
            );
        }
        if let Some(consumed) = args["cursor"].as_str() {
            assert!(
                !state
                    .outline_run
                    .tool_draft
                    .evidence_continuations
                    .contains_key(consumed)
            );
        }
        let excerpts = page["excerpts"].as_array().unwrap();
        assert_eq!(excerpts.len(), 1);
        let excerpt = &excerpts[0];
        assert_eq!(
            excerpt["evidence"]["start_byte"].as_u64().unwrap() as usize,
            restored.len()
        );
        restored.push_str(excerpt["quote"].as_str().unwrap());
        assert_eq!(
            excerpt["evidence"]["end_byte"].as_u64().unwrap() as usize,
            restored.len()
        );
        state.transcript.push(json!({"role":"tool","tool_call_id":calls[0].id,"content":json!({"ok":true,"result":page}).to_string()}));
        crate::outline::read_receipts::queue(&mut state, &before, &calls[0].id, false);

        // Execute the final tool's actual projection/admission path. Neither
        // receipt may be replaced by the deferral used during sizing.
        let before = state.outline_run.tool_draft.clone();
        let last = read_projection_in_context(
            &input,
            &config,
            &mut state,
            &json!({"mode":"chapters"}),
            &calls[1..],
            &[],
            Duty::Organize,
        )
        .await
        .unwrap();
        state.transcript.push(json!({"role":"tool","tool_call_id":calls[1].id,"content":json!({"ok":true,"result":last}).to_string()}));
        crate::outline::read_receipts::queue(&mut state, &before, &calls[1].id, false);
        fit_batch(&input, &config, &mut state, &[], &[])
            .await
            .unwrap();
        assert!(!correction_retry(&state.transcript));
        let bytes = prepare_request(&input, &config, &mut state, false)
            .await
            .unwrap();
        let total = crate::analysis::tests::total_request_tokens(&bytes, &config);
        assert!(
            total <= 131072,
            "ordinary full-wire budget exceeded: {total}"
        );
        let wire: Value = serde_json::from_slice(&bytes).unwrap();
        for call in &calls {
            let receipt = wire["messages"]
                .as_array()
                .unwrap()
                .iter()
                .find(|m| m["tool_call_id"] == call.id)
                .expect("both successful tools survive in the actual request");
            let content: Value =
                serde_json::from_str(receipt["content"].as_str().unwrap()).unwrap();
            assert_eq!(content["ok"], true);
        }
        crate::outline::read_receipts::seal(&mut state, &wire).unwrap();
        crate::outline::read_receipts::confirm(&mut state, &wire).unwrap();
        if page["selection_complete"] == true {
            break;
        }
        let cursor = page["next_cursor"]
            .as_str()
            .expect("incomplete page must supply a continuation");
        assert!(
            state
                .outline_run
                .tool_draft
                .evidence_continuations
                .contains_key(cursor)
        );
        args = json!({"cursor":cursor});
        // Isolate the next delivered-page turn, retaining the real continuation
        // and confirmed read registry. History retention is tested elsewhere.
        state.transcript.clear();
    }
    assert!(pages > 1);
    assert_eq!(
        restored, text,
        "real cursor traversal must neither lose nor duplicate source bytes"
    );
}

#[tokio::test]
async fn borrowed_input_digest_keeps_wire_and_budget_checks_current() {
    let input = super::token_transport_validation::synthetic_input(1, 1);
    let config = crate::analysis::tests::config();
    let mut state = super::retirement::checkpoint(&input);
    // Fix discovery claim identity before comparing bytes; fresh claims
    // legitimately generate different wire scopes even without digest reuse.
    let mut work = crate::outline::discover::DiscoverWork::plan(&input, 8000);
    for pack in work.claim(99) {
        work.submit(
            &input,
            &pack.id,
            crate::outline::discover::PackSubmit {
                call_id: format!("fixture-{}", pack.id),
                claim_token: pack.claim_token.clone(),
                pack_revision: pack.pack_revision,
                requirements: vec![],
                issues: vec![],
                no_requirement_reason: Some("Synthetic protocol fixture".into()),
                inspected_atom_ids: pack.atoms.iter().map(|atom| atom.id.clone()).collect(),
            },
        )
        .unwrap();
    }
    assert!(work.complete());
    state.outline_run.reading_packs = Some(work);
    let mut plain = state.clone();
    let expected = prepare_request(&input, &config, &mut plain, false)
        .await
        .unwrap();
    crate::outline::evidence::with_input_digest(&input, async {
        let mut scoped = state.clone();
        let actual = prepare_request(&input, &config, &mut scoped, false)
            .await
            .unwrap();
        assert!(
            actual == expected,
            "input digest reuse cannot change final wire bytes"
        );
        assert_eq!(
            serde_json::to_value(scoped).unwrap(),
            serde_json::to_value(plain).unwrap()
        );

        let mut changed = state.clone();
        changed
            .transcript
            .push(json!({"role":"user","content":"new request content"}));
        let messages = prepare_request(&input, &config, &mut changed, false)
            .await
            .unwrap();
        assert_ne!(messages, expected);
        assert!(
            String::from_utf8(messages)
                .unwrap()
                .contains("new request content")
        );

        let mut other_model = config.clone();
        other_model.provider.model_id = "test-other-model".into();
        other_model.limits.tokenizer.model_id = other_model.provider.model_id.clone();
        let wire = prepare_request(&input, &other_model, &mut state.clone(), false)
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&wire).unwrap()["model"],
            "test-other-model"
        );
        assert_ne!(wire, expected);

        let mut tight = config.clone();
        tight.limits.max_context_tokens = config.provider.output_token_reserve as usize;
        let rejected = prepare_request(&input, &tight, &mut state.clone(), false)
            .await
            .unwrap_err();
        assert_eq!(rejected.code, "AGENT_TURN_BUDGET_EXCEEDED");
    })
    .await
    .unwrap();
}
