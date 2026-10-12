// Sanitized reproduction shape: 74 synthetic proposals, 32 wrong typed handles.
// Production prepare/reserve/Received/commit/reload only; no provider network.
#[tokio::test]
async fn batch_74_candidates_32_typed_errors_repair_atomically_through_worker_reload() {
    let (input, config, mut journal) = fixture(1);
    journal.stop_barrier = false;
    let mut state = journal.load().await.unwrap().unwrap();
    let cancel = CancellationToken::new();
    let mut original_failed_arguments = Vec::<String>::new();
    for (turn, bad_count) in [32usize, 24, 16, 8, 0].into_iter().enumerate() {
        let mut host = PackHost {
            input: &input,
            config: &config,
            state: &mut state,
            journal: &journal,
            cancel: &cancel,
        };
        let request = host
            .next(&BTreeSet::new())
            .await
            .unwrap()
            .expect("effective typed-reference corrections remain schedulable");
        assert_eq!(request.scope.pack, "pack-0");
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        let payloads = body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|m| m["content"].as_str())
            .filter_map(|s| serde_json::from_str::<Value>(s).ok())
            .collect::<Vec<_>>();
        if turn > 0 {
            let audits = payloads.iter().filter(|p| p["rejected_submission_audit"].is_object()).collect::<Vec<_>>();
            assert_eq!(audits.len(), 1, "wire retains only the latest complete failed proposal");
            assert_eq!(audits[0]["rejected_submission_audit"]["original_arguments_text"],
                *original_failed_arguments.last().unwrap());
            let latest = payloads
                .iter()
                .rev()
                .find(|p| p["rejected_submission_audit"].is_object())
                .expect("audit is an inert host user packet");
            assert!(
                !latest["rejected_submission_audit"]["current_reference_mappings"]
                    .as_array()
                    .expect("verified current mappings")
                    .is_empty()
            );
            assert_eq!(
                latest["rejected_submission_audit"]["invalid_reference_diagnostics"]
                    .as_array()
                    .expect("all invalid references remain diagnostics")
                    .len(),
                bad_count + 8
            );
        }
        let packet = stall_packet(&request);
        let atom = &packet["reading_packs"][0]["pack"]["atoms"][0];
        let mut response = stall_submission(&config, &request, &[true; 74]).await;
        let mut args: Value = serde_json::from_str(&response.tool_calls[0].arguments).unwrap();
        for (index, requirement) in args["requirements"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .enumerate()
        {
            requirement["description"] = json!(format!(
                "Synthetic candidate {index:02}: preserve this independent response clause."
            ));
            if index < bad_count {
                // Issued current atom handle deliberately used in evidence namespace.
                requirement["evidence"][0]["evidence_key"] = atom["atom_key"].clone();
            }
        }
        assert_eq!(args["requirements"].as_array().unwrap().len(), 74);
        let arguments = args.to_string();
        response.tool_calls[0].arguments = arguments.clone();
        response.tool_calls[0].id = format!("synthetic-batch-correction-{turn}");
        host.reserve(&request).await.unwrap();
        host.received(&request, &response).await.unwrap();
        host.commit(&request, response).await.unwrap();
        let worker = &host.state.outline_run.discover_workers["pack-0"];
        if bad_count > 0 {
            original_failed_arguments.push(arguments);
            assert!(
                matches!(&worker.pending,Some(Pending::Failed{code,..}) if code=="BUSINESS_REJECTED")
            );
            assert_eq!(
                host.state
                    .outline_run
                    .reading_packs
                    .as_ref()
                    .unwrap()
                    .requirement_count(),
                0,
                "partial batch must never commit"
            );
            let receipt: Value =
                serde_json::from_str(worker.history.last().unwrap()["content"].as_str().unwrap())
                    .unwrap();
            assert_eq!(receipt["ok"], false);
            let serialized = receipt.to_string();
            for index in 0..bad_count {
                assert!(
                    serialized.contains(&format!("/requirements/{index}/evidence/0/evidence_key")),
                    "all bad paths must be returned together: {serialized}"
                );
            }
        } else {
            assert!(matches!(&worker.pending, Some(Pending::Committed { .. })));
            assert_eq!(
                host.state
                    .outline_run
                    .reading_packs
                    .as_ref()
                    .unwrap()
                    .requirement_count(),
                74
            );
        }
        let saved = journal.load().await.unwrap().unwrap();
        assert_eq!(json!(saved), json!(host.state));
        state = serde_json::from_value(json!(saved)).unwrap();
    }
    let work = state.outline_run.reading_packs.as_ref().unwrap();
    assert!(work.complete());
    assert_eq!(work.requirement_count(), 74);
    assert_eq!(
        work.pack_counts().committed,
        1,
        "one successful atomic submission, no earlier partial success"
    );
    assert_eq!(state.journal.accounting.physical_calls, 5);
    let history = &state.outline_run.discover_workers["pack-0"].history;
    for original in original_failed_arguments {
        assert!(
            history.iter().any(|m| m["tool_calls"]
                .as_array()
                .is_some_and(|calls| calls.iter().any(|c| c["function"]["arguments"] == original))),
            "durable raw rejected arguments survive final success"
        );
    }
}
