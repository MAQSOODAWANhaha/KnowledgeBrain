#[tokio::test]
async fn correction_required_read_and_optional_capacity_recovery_use_distinct_windows() {
    for necessary in [true,false] {
        let (mut input,mut config,mut journal)=auto_support_fixture(3);
        if !necessary { for relation in &mut input.document_relations { relation.required=false; } }
        config.limits.max_correction_context_tokens=Some(262144);
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
        if necessary {
            assert_eq!(receipt["ok"],true,"{receipt}");
            assert_eq!(receipt["result"]["required_support"],true);
            assert!(!worker.related_read_frames.is_empty());
        } else {
            assert_eq!(receipt["ok"],false,"optional page cannot use correction allowance");
            assert!(receipt["error"].as_str().unwrap().contains("Optional source page"),"{receipt}");
            assert!(worker.related_read_frames.is_empty());
            assert!(matches!(&worker.pending,Some(Pending::Failed{code,..})if code=="BUSINESS_REJECTED"));
        }
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
    }
}

#[test]
fn optional_success_uses_ordinary_budget_and_claim_boundary_clears_correction() {
    let mut worker=Worker::default();
    for (id,name,result) in [("failed","submit_pack",json!({"ok":false})),
        ("read","read_evidence",json!({"ok":true,"result":{"status":"read_pending_delivery","required_support":false}}))] {
        worker.history.push(json!({"role":"assistant","tool_calls":[{"id":id,"function":{"name":name,"arguments":"{}"}}]}));
        worker.history.push(json!({"role":"tool","tool_call_id":id,"content":result.to_string()}));
    }
    assert!(!worker_correction_window(&worker));
    worker.history[3]["content"]=json!({"ok":true,"result":{"status":"read_pending_delivery","required_support":true}}).to_string().into();
    assert!(worker_correction_window(&worker));
    worker.history_revision_start=worker.history.len();
    assert!(!worker_correction_window(&worker));
}
