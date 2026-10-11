// Offline production-host regressions for state-based Discover stagnation.
// Responses below are scripted protocol fixtures, never model acceptance evidence.

fn stall_packet(request: &Request) -> Value {
    let body: Value = serde_json::from_slice(&request.body).unwrap();
    body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|message| message["content"].as_str())
        .filter_map(|text| serde_json::from_str::<Value>(text).ok())
        .find(|payload| payload["reading_packs"].is_array())
        .unwrap()
}

async fn stall_submission(config: &Config, request: &Request, descriptions: &[bool]) -> ChatTurn {
    let mut response = Delayed {
        delay_ms: 0,
        ..Default::default()
    }
    .turn(config, &request.body)
    .await
    .unwrap();
    let packet = stall_packet(request);
    let atom = &packet["reading_packs"][0]["pack"]["atoms"][0];
    let mut args: Value = serde_json::from_str(&response.tool_calls[0].arguments).unwrap();
    args["requirements"] = json!(descriptions.iter().enumerate().map(|(i, valid)| {
        json!({"description":if *valid { format!("Synthetic supported response requirement {i}.") } else { " ".to_owned() },
            "kind":"technical","obligation_strength":"mandatory","extraction_quality":"explicit",
            "source_section_id":{"atom_key":atom["atom_key"]},
            "evidence":[{"evidence_key":atom["excerpts"][0]["evidence_key"]}]})
    }).collect::<Vec<_>>());
    args["no_requirement_reason"] = Value::Null;
    response.tool_calls[0].arguments = args.to_string();
    response.tool_calls[0].id = format!("synthetic-generation-{}", request.scope.generation);
    response
}

#[tokio::test]
async fn discover_stall_nine_effective_corrections_survive_old_six_rejection_limit() {
    let (input, config, mut journal) = fixture(1);
    journal.stop_barrier = false;
    assert_eq!(config.limits.max_no_progress_turns, 6);
    let mut state = journal.load().await.unwrap().unwrap();
    let cancel = CancellationToken::new();
    for corrected in 0..=9 {
        let mut host = PackHost {
            input: &input,
            config: &config,
            state: &mut state,
            journal: &journal,
            cancel: &cancel,
        };
        let request = host.next(&BTreeSet::new()).await.unwrap().expect(
            "a different effective correction must remain schedulable beyond six rejected turns",
        );
        assert_eq!(request.scope.pack, "pack-0");
        let descriptions = (0..9).map(|i| i < corrected).collect::<Vec<_>>();
        let response = stall_submission(&config, &request, &descriptions).await;
        host.reserve(&request).await.unwrap();
        host.received(&request, &response).await.unwrap();
        host.commit(&request, response).await.unwrap();
        let worker = &host.state.outline_run.discover_workers["pack-0"];
        if corrected < 9 {
            assert!(
                matches!(&worker.pending, Some(Pending::Failed { code, .. }) if code == "BUSINESS_REJECTED")
            );
            assert_eq!(worker.rejected_turns, corrected + 1);
            let receipt: Value =
                serde_json::from_str(worker.history.last().unwrap()["content"].as_str().unwrap())
                    .unwrap();
            assert_eq!(receipt["ok"], false);
        } else {
            assert!(matches!(&worker.pending, Some(Pending::Committed { .. })));
            assert_eq!(
                worker.rejected_turns, 9,
                "the count is an audit, not an admission limit"
            );
        }
        let saved = journal.load().await.unwrap().unwrap();
        assert_eq!(json!(saved), json!(host.state));
        state = saved;
    }
    assert!(state.outline_run.reading_packs.as_ref().unwrap().complete());
    assert_eq!(
        state
            .outline_run
            .reading_packs
            .as_ref()
            .unwrap()
            .requirement_records()
            .len(),
        9
    );
    assert_eq!(state.journal.accounting.physical_calls, 10);
}

#[tokio::test]
async fn discover_stall_same_failure_and_a_b_a_stop_across_wire_rotation_reload_and_isolate_pack() {
    for cycle in [false, true] {
        let (input, config, mut journal) = fixture(2);
        journal.stop_barrier = false;
        let mut state = journal.load().await.unwrap().unwrap();
        let cancel = CancellationToken::new();
        let mut wrappers = BTreeSet::new();
        let mut revisions = BTreeSet::new();
        let turns = if cycle { 3 } else { 2 };
        for step in 0..turns {
            let mut host = PackHost {
                input: &input,
                config: &config,
                state: &mut state,
                journal: &journal,
                cancel: &cancel,
            };
            let request = host.next(&BTreeSet::new()).await.unwrap().unwrap();
            assert_eq!(request.scope.pack, "pack-0");
            revisions.insert(request.scope.revision);
            let is_b = cycle && step == 1;
            let descriptions = if is_b {
                vec![false, false]
            } else {
                vec![false]
            };
            let response = stall_submission(&config, &request, &descriptions).await;
            let args: Value = serde_json::from_str(&response.tool_calls[0].arguments).unwrap();
            wrappers.insert(args["wire_scope"].as_str().unwrap().to_owned());
            host.reserve(&request).await.unwrap();
            host.received(&request, &response).await.unwrap();
            host.commit(&request, response).await.unwrap();
            let worker = &host.state.outline_run.discover_workers["pack-0"];
            let expected = if step + 1 == turns {
                "DISCOVER_NO_STATE_PROGRESS"
            } else {
                "BUSINESS_REJECTED"
            };
            assert!(
                matches!(&worker.pending, Some(Pending::Failed { code, .. }) if code == expected),
                "cycle {cycle}, step {step}: {:?}",
                worker.pending
            );
            assert_eq!(worker.rejected_turns, step + 1);
            let saved = journal.load().await.unwrap().unwrap();
            assert_eq!(json!(saved), json!(host.state));
            state = saved;
        }
        assert_eq!(
            wrappers.len(),
            turns,
            "repeat is detected despite actual fresh wire scopes"
        );
        if cycle {
            assert!(
                revisions.len() > 1,
                "A-B-A must cross actual failed repair revision rotation"
            );
        }
        let audit = json!(state.outline_run.discover_workers["pack-0"]);
        let accounting = json!(state.journal.accounting);
        let mut host = PackHost {
            input: &input,
            config: &config,
            state: &mut state,
            journal: &journal,
            cancel: &cancel,
        };
        host.replay_received().await.unwrap();
        let independent = host.next(&BTreeSet::new()).await.unwrap().unwrap();
        assert_eq!(
            independent.scope.pack, "pack-1",
            "terminal failure must free the slot for an independent scope"
        );
        assert_eq!(
            json!(host.state.journal.accounting),
            accounting,
            "reload/prepare never purchases another response to the stopped worker"
        );
        let response = Delayed {
            delay_ms: 0,
            ..Default::default()
        }
        .turn(&config, &independent.body)
        .await
        .unwrap();
        host.reserve(&independent).await.unwrap();
        host.received(&independent, &response).await.unwrap();
        host.commit(&independent, response).await.unwrap();
        assert!(host.next(&BTreeSet::new()).await.unwrap().is_none());
        assert_eq!(
            json!(host.state.outline_run.discover_workers["pack-0"]),
            audit
        );
        assert!(!host.all_committed());
        assert_eq!(
            host.state.journal.accounting.physical_calls,
            turns as u64 + 1
        );
        assert_eq!(
            host.state
                .outline_run
                .reading_packs
                .as_ref()
                .unwrap()
                .status("pack-1"),
            Some(PackStatus::Committed)
        );
        let saved = journal.load().await.unwrap().unwrap();
        assert!(
            matches!(&saved.outline_run.discover_workers["pack-0"].pending, Some(Pending::Failed { code, .. }) if code == "DISCOVER_NO_STATE_PROGRESS")
        );
    }
}
