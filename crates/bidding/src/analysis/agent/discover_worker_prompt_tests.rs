// Real worker request preparation, synthetic frozen sources, no provider call.
fn discover_prompt_text(message: &Value) -> String {
    match &message["content"] {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .map(|part| {
                assert_eq!(
                    part["type"], "text",
                    "synthetic prompt fixture expects only Text parts"
                );
                part["text"].as_str().expect("Text part must contain text")
            })
            .collect::<String>(),
        other => panic!("unexpected synthetic prompt content shape: {other}"),
    }
}

fn discover_prompt_automatic_pages(request: &Request) -> Vec<Value> {
    let wire: Value = serde_json::from_slice(&request.body).unwrap();
    wire["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| message["role"] == "user")
        .map(discover_prompt_text)
        .filter_map(|text| serde_json::from_str::<Value>(&text).ok())
        .filter(|payload| payload.get("automatic_support_page").is_some())
        .collect()
}

fn discover_prompt_packet(request: &Request) -> (String, Value) {
    let wire: Value = serde_json::from_slice(&request.body).unwrap();
    let messages = wire["messages"].as_array().unwrap();
    let system = discover_prompt_text(messages.iter().find(|m| m["role"] == "system").unwrap());
    let packet = messages
        .iter()
        .filter(|m| m["role"] == "user")
        .map(discover_prompt_text)
        .filter_map(|text| serde_json::from_str::<Value>(&text).ok())
        .find(|value| value["reading_packs"].is_array())
        .unwrap();
    assert!(system.contains(crate::outline::agent::Duty::Discover.instructions()));
    assert!(system.contains("structural_context_candidate 是可选语义候选，不是统一的提交前置条件"));
    assert!(system.contains("按 next_required_read 指示继续读取"));
    assert!(system.contains("不能因为可选就忽略已发现的相关限定"));
    assert!(!system.contains("structural_context_candidate 必须经 read_evidence"));
    (system, packet)
}

#[tokio::test]
async fn discover_worker_prompt_matches_required_automatic_page_and_next_read() {
    let (input, config, journal) = auto_support_fixture(2);
    let mut state = journal.load().await.unwrap().unwrap();
    let cancel = CancellationToken::new();
    let request = PackHost {
        input: &input,
        config: &config,
        state: &mut state,
        journal: &journal,
        cancel: &cancel,
    }
    .next(&BTreeSet::new())
    .await
    .unwrap()
    .unwrap();
    let (system, packet) = discover_prompt_packet(&request);
    assert!(system.contains("必读支持由宿主选择并自动附在当前请求中"));
    assert_eq!(discover_prompt_automatic_pages(&request).len(), 1);
    assert_eq!(
        packet["reading_packs"][0]["pack"]["next_required_read"]["tool"],
        "read_evidence"
    );
    assert!(
        state.outline_run.discover_workers[&request.scope.pack]
            .delivered_related_evidence
            .is_empty(),
        "preparation and navigation grant no durable delivery credit"
    );
    assert_eq!(state.journal.accounting.physical_calls, 0);
}

#[tokio::test]
async fn discover_worker_prompt_does_not_require_optional_section_candidate() {
    let (mut input, config, _) = fixture(1);
    // Two native source units in one section; no required relation connects them.
    let mut owner = input.source_units[0].clone();
    owner.source_unit_revision_id.push_str("-following");
    owner.ordinal += 1;
    owner.locator["unit_id"] = json!("following");
    input.source_units.push(owner);
    let mut state = super::super::retirement::checkpoint(&input);
    state.turn = 0;
    state.config_sha256 = digest(&config).unwrap();
    let mut work = crate::outline::discover::DiscoverWork::plan_with_budget(&input, &|sessions| {
        Ok(sessions
            .iter()
            .all(|s| s["pack"]["atoms"].as_array().unwrap().len() == 1))
    });
    assert_eq!(work.pack_counts().total, 2);
    let pack = work
        .claim(2)
        .into_iter()
        .find(|p| {
            p.atoms
                .iter()
                .any(|a| a.source_unit_revision_id.ends_with("-following"))
        })
        .unwrap();
    assert!(
        work.required_support_refs(&input, &pack.id)
            .unwrap()
            .is_empty()
    );
    state.outline_run.reading_packs = Some(work);
    state
        .outline_run
        .discover_workers
        .insert(pack.id.clone(), Worker::default());
    let journal = Durable {
        state: Mutex::new(state.clone()),
        stop_barrier: false,
        crash_received: AtomicBool::new(false),
        call_limit: 0,
    };
    let cancel = CancellationToken::new();
    let request = PackHost {
        input: &input,
        config: &config,
        state: &mut state,
        journal: &journal,
        cancel: &cancel,
    }
    .prepare_inner(&pack.id, false)
    .await
    .unwrap();
    let (_, packet) = discover_prompt_packet(&request);
    let wire_pack = &packet["reading_packs"][0]["pack"];
    assert_eq!(
        wire_pack["read_dependencies"][0]["type"],
        "structural_context_candidate"
    );
    assert!(
        wire_pack["read_dependencies"][0]["read_policy"]
            .as_str()
            .unwrap()
            .contains("optional semantic candidate")
    );
    assert_eq!(
        wire_pack["read_dependencies"][0]["original_read_complete"],
        false
    );
    assert!(wire_pack["next_required_read"].is_null());
    assert!(discover_prompt_automatic_pages(&request).is_empty());
    assert!(
        state.outline_run.discover_workers[&pack.id]
            .delivered_related_evidence
            .is_empty()
    );
    assert_eq!(state.journal.accounting.physical_calls, 0);
}
