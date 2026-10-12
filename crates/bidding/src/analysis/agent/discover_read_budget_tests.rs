#[tokio::test]
async fn correction_window_keeps_required_and_optional_reads_until_pack_completion() {
    for necessary in [true,false] {
        let (mut input,config,mut journal)=auto_support_fixture(3);
        if !necessary { for relation in &mut input.document_relations { relation.required=false; } }
        journal.stop_barrier=false;
        let mut state=super::super::retirement::checkpoint(&input);
        state.turn=0;state.config_sha256=digest(&config).unwrap();
        state.outline_run.reading_packs=Some(crate::outline::discover::DiscoverWork::plan_with_budget(&input,&|sessions|
            Ok(sessions.iter().all(|s|s["pack"]["atoms"].as_array().unwrap().len()==1))));
        let cancel=CancellationToken::new();
        let mut host=PackHost{input:&input,config:&config,state:&mut state,journal:&journal,cancel:&cancel};
        let first=host.next(&BTreeSet::new()).await.unwrap().unwrap();
        assert!(first.input_reserve+first.output_reserve<=131072);
        let mut failed=stall_submission(&config,&first,&[true]).await;
        let mut args:Value=serde_json::from_str(&failed.tool_calls[0].arguments).unwrap();
        args["requirements"][0]["description"]=json!("synthetic correction detail ".repeat(50000));
        args["requirements"][0]["evidence"][0]["evidence_key"]=json!("e-unissued");
        failed.tool_calls[0].arguments=args.to_string();
        host.reserve(&first).await.unwrap();host.received(&first,&failed).await.unwrap();host.commit(&first,failed).await.unwrap();
        let mut restored=journal.load().await.unwrap().unwrap();
        let mut host=PackHost{input:&input,config:&config,state:&mut restored,journal:&journal,cancel:&cancel};
        let correction=host.next(&BTreeSet::new()).await.unwrap().unwrap();
        assert!(correction.input_reserve+correction.output_reserve>131072);
        let mut read=Delayed{delay_ms:0,..Default::default()}.turn(&config,&correction.body).await.unwrap();
        let wire:Value=serde_json::from_str(&read.tool_calls[0].arguments).unwrap();
        read.tool_calls[0].name="read_evidence".into();
        read.tool_calls[0].arguments=json!({"wire_scope":wire["wire_scope"],"dependency_id":"synthetic-support-3"}).to_string();
        host.reserve(&correction).await.unwrap();host.received(&correction,&read).await.unwrap();host.commit(&correction,read).await.unwrap();
        let worker=&host.state.outline_run.discover_workers["pack-0"];
        let receipt:Value=serde_json::from_str(worker.history.last().unwrap()["content"].as_str().unwrap()).unwrap();
        assert_eq!(receipt["ok"],true,"both required and optional reads retain correction: {receipt}");
        assert_eq!(receipt["result"]["required_support"],necessary);
        assert!(!worker.related_read_frames.is_empty());
        assert!(worker_correction_window(worker));
        let mut restored=journal.load().await.unwrap().unwrap();
        let mut host=PackHost{input:&input,config:&config,state:&mut restored,journal:&journal,cancel:&cancel};
        let corrected=host.next(&BTreeSet::new()).await.unwrap().unwrap();
        assert_eq!(corrected.scope.pack,"pack-0");
        assert!(corrected.input_reserve+corrected.output_reserve>131072);
        assert!(corrected.input_reserve+corrected.output_reserve<=262144);
        let response=Delayed{delay_ms:0,..Default::default()}.turn(&config,&corrected.body).await.unwrap();
        host.reserve(&corrected).await.unwrap();host.received(&corrected,&response).await.unwrap();host.commit(&corrected,response).await.unwrap();
        assert_eq!(host.state.outline_run.reading_packs.as_ref().unwrap().status("pack-0"),Some(PackStatus::Committed));
        assert!(!worker_correction_window(&host.state.outline_run.discover_workers["pack-0"]));
        let peer=host.next(&BTreeSet::new()).await.unwrap().unwrap();
        assert_ne!(peer.scope.pack,"pack-0");
        assert!(peer.input_reserve+peer.output_reserve<=config.limits.max_context_tokens as u64);
        assert!(!worker_correction_window(&host.state.outline_run.discover_workers[&peer.scope.pack]));
    }
}

#[test]
fn pack_correction_requires_host_state_and_survives_checkpoint_reload() {
    let mut worker=Worker::default();
    for (id,name,result) in [("failed","submit_pack",json!({"ok":false})),
        ("read","read_evidence",json!({"ok":true,"result":{"status":"read_pending_delivery","required_support":false}}))] {
        worker.history.push(json!({"role":"assistant","tool_calls":[{"id":id,"function":{"name":name,"arguments":"{}"}}]}));
        worker.history.push(json!({"role":"tool","tool_call_id":id,"content":result.to_string()}));
    }
    assert!(!worker_correction_window(&worker), "history alone cannot manufacture host correction state");
    worker.correction_active=true;
    worker.history_revision_start=worker.history.len();
    let restored: Worker=serde_json::from_value(serde_json::to_value(&worker).unwrap()).unwrap();
    assert!(worker_correction_window(&restored), "optional reads and audit projection do not reset host correction");
    assert!(!worker_correction_window(&Worker::default()), "another pack has its own ordinary window");
}
