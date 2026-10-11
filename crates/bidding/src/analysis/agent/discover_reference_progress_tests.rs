// Offline production PackHost rejection/restore path, no provider calls.
#[tokio::test]
async fn reference_error_paths_allow_partial_repairs_but_changed_garbage_is_not_progress() {
    let (input, config, mut journal) = fixture(1);
    journal.stop_barrier = false;
    let mut state = journal.load().await.unwrap().unwrap();
    let cancel = CancellationToken::new();
    for step in 0..3 {
        let mut host = PackHost {
            input: &input,
            config: &config,
            state: &mut state,
            journal: &journal,
            cancel: &cancel,
        };
        let request = host.next(&BTreeSet::new()).await.unwrap().unwrap();
        let mut response = stall_submission(&config, &request, &[true, true, true]).await;
        let mut args: Value = serde_json::from_str(&response.tool_calls[0].arguments).unwrap();
        let broken = if step == 0 { 3 } else { 2 };
        for index in 0..broken {
            args["requirements"][index]["evidence"][0]["evidence_key"] =
                json!(format!("e-unissued-{step}-{index}"));
        }
        response.tool_calls[0].arguments = args.to_string();
        host.reserve(&request).await.unwrap();
        host.received(&request, &response).await.unwrap();
        host.commit(&request, response).await.unwrap();
        let worker = &host.state.outline_run.discover_workers["pack-0"];
        assert_eq!(
            matches!(&worker.pending,Some(Pending::Failed{code,..}) if code=="DISCOVER_NO_STATE_PROGRESS"),
            step == 2,
            "fewer failed paths permit correction; new spelling at identical paths is not progress"
        );
        let receipt: Value =
            serde_json::from_str(worker.history.last().unwrap()["content"].as_str().unwrap())
                .unwrap();
        assert_eq!(
            receipt["invalid_reference_diagnostics"]
                .as_array()
                .unwrap()
                .len(),
            broken
        );
        assert!(
            receipt["invalid_reference_diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .all(|e| e["code"] == "unknown_reference")
        );
        state = journal.load().await.unwrap().unwrap();
    }
}

#[tokio::test]
async fn valid_source_change_survives_another_invalid_reference_without_garbage_progress() {
    let (mut input, config, mut journal) = fixture(1);
    let mut second = input.source_units[0].clone();
    second.source_unit_revision_id.push_str("-second");
    second.ordinal += 1;
    second.locator["unit_id"] = json!("second");
    input.source_units.push(second);
    journal.stop_barrier = false;
    let mut state = super::super::retirement::checkpoint(&input);
    state.turn = 0;
    state.config_sha256 = digest(&config).unwrap();
    state.outline_run.reading_packs = Some(
        crate::outline::discover::DiscoverWork::plan_with_budget(&input, &|_| Ok(true)),
    );
    assert_eq!(
        state
            .outline_run
            .reading_packs
            .as_ref()
            .unwrap()
            .pack_counts()
            .total,
        1
    );
    let cancel = CancellationToken::new();
    for step in 0..3 {
        let mut host = PackHost {
            input: &input,
            config: &config,
            state: &mut state,
            journal: &journal,
            cancel: &cancel,
        };
        let request = host.next(&BTreeSet::new()).await.unwrap().unwrap();
        let mut response = stall_submission(&config, &request, &[true, true]).await;
        let packet = stall_packet(&request);
        let atoms = packet["reading_packs"][0]["pack"]["atoms"]
            .as_array()
            .unwrap();
        assert_eq!(atoms.len(), 2);
        let mut args: Value = serde_json::from_str(&response.tool_calls[0].arguments).unwrap();
        args["requirements"][0]["evidence"][0]["evidence_key"] = json!(format!("e-unknown-{step}"));
        args["requirements"][1]["evidence"][0]["evidence_key"] =
            atoms[usize::from(step > 0)]["excerpts"][0]["evidence_key"].clone();
        response.tool_calls[0].arguments = args.to_string();
        host.reserve(&request).await.unwrap();
        host.received(&request, &response).await.unwrap();
        host.commit(&request, response).await.unwrap();
        let worker = &host.state.outline_run.discover_workers["pack-0"];
        assert_eq!(
            matches!(&worker.pending,Some(Pending::Failed{code,..})if code=="DISCOVER_NO_STATE_PROGRESS"),
            step == 2,
            "unchanged error path does not hide a different valid source; changing only garbage is not progress"
        );
        assert_eq!(
            host.state
                .outline_run
                .reading_packs
                .as_ref()
                .unwrap()
                .requirement_records()
                .len(),
            0,
            "signature-only partial canonicalization never executes a partially valid proposal"
        );
        state = journal.load().await.unwrap().unwrap();
    }
}
