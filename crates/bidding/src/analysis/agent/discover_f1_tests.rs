// F1 scheduling regression: settled failures must not starve unclaimed scopes.
// All responses are synthetic and use the existing in-process transport oracle.
#[tokio::test]
async fn f1_terminal_failures_release_slots_and_preserve_fresh_check_after_reload() {
    let (input, config, mut journal) = fixture(4);
    journal.stop_barrier = false;
    let mut state = journal.state.lock().unwrap().clone();
    let cancel = CancellationToken::new();
    {
        let mut host = PackHost { input: &input, config: &config, state: &mut state, journal: &journal, cancel: &cancel };
        let a = host.next(&BTreeSet::new()).await.unwrap().unwrap();
        host.reserve(&a).await.unwrap();
        let b = host.next(&BTreeSet::from([a.scope.request.clone()])).await.unwrap().unwrap();
        host.reserve(&b).await.unwrap();
        assert_eq!(a.scope.pack, "pack-0");
        assert_eq!(b.scope.pack, "pack-1");
        assert!(host.next(&BTreeSet::from([a.scope.request.clone(), b.scope.request.clone()])).await.unwrap().is_none(),
            "two active workers still exhaust the concurrency allowance");
        let response = Delayed { delay_ms: 0, ..Default::default() }.turn(&config, &a.body).await.unwrap();
        host.received(&a, &response).await.unwrap();
        host.commit(&a, response).await.unwrap();
        let c = host.next(&BTreeSet::from([b.scope.request.clone()])).await.unwrap().unwrap();
        host.reserve(&c).await.unwrap();
        assert_eq!(c.scope.pack, "pack-2");
        let failure = error("TEST_PROVIDER_FAILED", "synthetic terminal worker failure");
        host.failed(&b, &failure).await.unwrap();
        host.failed(&c, &failure).await.unwrap();
        host.save().await.unwrap();
    }
    // Durable serialisation is part of this reproduction: there is no hidden
    // active task after reload that might eventually release the occupied slots.
    let mut restored = journal.load().await.unwrap().unwrap();
    assert!(matches!(restored.outline_run.discover_workers["pack-0"].pending, Some(Pending::Committed { .. })));
    for id in ["pack-1", "pack-2"] {
        assert!(matches!(restored.outline_run.discover_workers[id].pending, Some(Pending::Failed { .. })));
    }
    assert!(!restored.outline_run.discover_workers.contains_key("pack-3"));
    let work = restored.outline_run.reading_packs.as_mut().unwrap();
    assert_eq!(work.status("pack-0"), Some(PackStatus::Committed));
    assert_eq!(work.status("pack-3"), Some(PackStatus::Pending));
    let failed_audit: Vec<_> = ["pack-1", "pack-2"].iter().map(|id|
        serde_json::to_value(&restored.outline_run.discover_workers[*id]).unwrap()).collect();
    let accounting = serde_json::to_value(&restored.journal.accounting).unwrap();
    let mut host = PackHost { input: &input, config: &config, state: &mut restored, journal: &journal, cancel: &cancel };
    let d = host.next(&BTreeSet::new()).await.unwrap().expect("terminal B/C must release a slot for D");
    assert_eq!(d.scope.pack, "pack-3");
    assert_eq!(serde_json::to_value(&host.state.journal.accounting).unwrap(), accounting,
        "claiming D must not reset or charge historical accounting");
    host.reserve(&d).await.unwrap();
    let response = Delayed { delay_ms: 0, ..Default::default() }.turn(&config, &d.body).await.unwrap();
    host.received(&d, &response).await.unwrap();
    host.commit(&d, response).await.unwrap();
    assert!(host.next(&BTreeSet::new()).await.unwrap().is_none(), "failed B/C are not retried");
    for (index, id) in ["pack-1", "pack-2"].iter().enumerate() {
        assert_eq!(serde_json::to_value(&host.state.outline_run.discover_workers[*id]).unwrap(), failed_audit[index]);
    }
    assert!(!host.all_committed());
    host.save().await.unwrap();
    let mut checked = journal.load().await.unwrap().unwrap();
    assert_eq!(crate::outline::partial_source::eligible(&input, &checked), Some(BTreeSet::from(["pack-0".into(), "pack-3".into()])));
    crate::outline::agent::synchronize_discovery(&input, &mut checked).unwrap();
    assert_eq!(crate::outline::agent::current(&input, &checked), crate::outline::agent::Duty::Check);
    assert!(checked.outline_run.tool_draft.check_reads.evidence.is_empty());
    assert!(!crate::outline::source_review::complete(&input, &checked.outline_run.tool_draft, "pack-0").unwrap(),
        "A is eligible only for fresh Check, not credited by its Discover commit");
    assert_eq!(checked.outline_run.tool_draft.required_pack_ids.len(), 4);
    assert!(crate::outline::tools::template_ready(&input, &checked.outline_run.tool_draft).is_err());
    assert!(!checked.done);
}
