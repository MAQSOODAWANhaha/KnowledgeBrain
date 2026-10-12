#[tokio::test]
async fn correction_window_applies_after_reload_until_pack_completes() {
    let (input, mut config, mut journal) = fixture(1);
    journal.stop_barrier = false;
    let mut state = journal.load().await.unwrap().unwrap();
    state.config_sha256 = digest(&config).unwrap();
    let cancel = CancellationToken::new();
    let mut host = PackHost { input: &input, config: &config, state: &mut state, journal: &journal, cancel: &cancel };
    let first = host.next(&BTreeSet::new()).await.unwrap().unwrap();
    assert!(first.input_reserve + first.output_reserve <= 131072);
    let mut response = stall_submission(&config, &first, &[true]).await;
    let mut args: Value = serde_json::from_str(&response.tool_calls[0].arguments).unwrap();
    args["requirements"][0]["description"] = json!("synthetic correction detail ".repeat(50000));
    args["requirements"][0]["evidence"][0]["evidence_key"] = json!("e-not-issued");
    response.tool_calls[0].arguments = args.to_string();
    host.reserve(&first).await.unwrap();
    host.received(&first, &response).await.unwrap();
    host.commit(&first, response).await.unwrap();
    let base = journal.load().await.unwrap().unwrap();
    assert!(matches!(&base.outline_run.discover_workers["pack-0"].pending,Some(Pending::Failed{code,..}) if code == "BUSINESS_REJECTED"));
    let mut retry_state = base.clone();
    let mut retry_host = PackHost { input: &input, config: &config, state: &mut retry_state, journal: &journal, cancel: &cancel };
    let retry_request = retry_host.next(&BTreeSet::new()).await.unwrap().unwrap();
    retry_host.reserve(&retry_request).await.unwrap();
    retry_host.failed(&retry_request, &error("AGENT_PROVIDER_TRANSIENT", "synthetic transport retry")).await.unwrap();
    let retry_saved = journal.load().await.unwrap().unwrap();
    let retry_worker = &retry_saved.outline_run.discover_workers["pack-0"];
    assert!(worker_correction_window(retry_worker), "physical retry and checkpoint reload retain pack correction");
    assert!(matches!(&retry_worker.pending, Some(Pending::Prepared { request }) if request == &retry_request),
        "retry keeps the exact expanded request instead of preparing an ordinary window");
    assert_eq!(retry_saved.journal.accounting.physical_calls, base.journal.accounting.physical_calls + 1);
    let mut restored = base.clone();
    let fitted = PackHost { input: &input, config: &config, state: &mut restored, journal: &journal, cancel: &cancel }.prepare_inner("pack-0", false).await.unwrap();
    let total = (fitted.input_reserve + fitted.output_reserve) as usize;
    assert!(total > 131072 && total <= 262144, "measured correction total={total}");
    let wire: Value = serde_json::from_slice(&fitted.body).unwrap();
    assert!(wire.get("max_tokens").is_none() && wire.get("max_completion_tokens").is_none());
    for (limit, allowed) in [(Some(total),true),(Some(total-1),false),(None,true)] {
        config.limits.max_correction_context_tokens = limit;
        let mut restored = base.clone();
        let result = PackHost { input: &input, config: &config, state: &mut restored, journal: &journal, cancel: &cancel }.prepare_inner("pack-0", false).await;
        assert_eq!(result.is_ok(), allowed, "full request exact boundary including output");
        assert_eq!(restored.journal.accounting.physical_calls, base.journal.accounting.physical_calls);
    }
    config.limits.max_correction_context_tokens = Some(262144);
    for (limit, allowed) in [(total, true), (total - 1, true), (131072, true)] {
        let mut restored = base.clone();
        restored.outline_run.discover_workers.get_mut("pack-0").unwrap()
            .reduced_context_tokens = Some(limit);
        let result = PackHost { input: &input, config: &config, state: &mut restored, journal: &journal, cancel: &cancel }
            .prepare_inner("pack-0", false).await;
        assert_eq!(result.is_ok(), allowed, "ordinary shrinking must not override the correction window");
        assert_eq!(restored.journal.accounting.physical_calls, base.journal.accounting.physical_calls);
        assert_eq!(restored.outline_run.discover_workers["pack-0"].reduced_context_tokens, Some(limit));
    }
    let mut oversized = base.clone();
    let args = &mut oversized.outline_run.discover_workers.get_mut("pack-0").unwrap().history[0]["tool_calls"][0]["function"]["arguments"];
    let mut payload: Value = serde_json::from_str(args.as_str().unwrap()).unwrap();
    payload["requirements"][0]["description"] = json!("synthetic correction detail ".repeat(90000));
    *args = json!(payload.to_string());
    let result = PackHost { input: &input, config: &config, state: &mut oversized, journal: &journal, cancel: &cancel }.prepare_inner("pack-0", false).await;
    assert!(result.is_err(), "correction cannot exceed 262144");
    let Some(Pending::Failed { request, .. }) = &oversized.outline_run.discover_workers["pack-0"].pending else { panic!("oversized request preserved") };
    assert!(request.input_reserve + request.output_reserve > 262144);
    let mut ordinary = base.clone();
    let worker = ordinary.outline_run.discover_workers.get_mut("pack-0").unwrap();
    worker.history.push(json!({"role":"assistant","tool_calls":[{"id":"successful-read","function":{"name":"read_evidence","arguments":"{}"}}]}));
    worker.history.push(json!({"role":"tool","tool_call_id":"successful-read","content":"{\"ok\":true,\"result\":{}}"}));
    let result = PackHost { input: &input, config: &config, state: &mut ordinary, journal: &journal, cancel: &cancel }.prepare_inner("pack-0", false).await;
    assert!(result.is_ok(), "successful reading keeps the correction window until the pack completes");
}
