use super::*;

async fn fixture() -> (FrozenInput, Checkpoint, Config) {
    let input = input();
    let journal = fresh_review_journal().await;
    let mut state = journal.load().await.unwrap().unwrap();
    state.role = Role::Main;
    state.main_work = Some(serde_json::from_value(active_work("source")).unwrap());
    state.analysis.records.clear();
    for id in ["a-full-identity", "b-full-identity"] {
        let mut record = requirement();
        record["id"] = json!(id);
        state
            .analysis
            .records
            .insert(id.into(), serde_json::from_value(record).unwrap());
    }
    (input, state, config())
}

async fn inspect(
    input: &FrozenInput,
    state: &mut Checkpoint,
    config: &Config,
    args: &Value,
) -> Result<Value, String> {
    let committed = state.analysis.coverage.clone();
    agent::inspect_in_context(
        input,
        config,
        state,
        args,
        &[ChatToolCall {
            id: "candidate-navigation".into(),
            name: "inspect_analysis".into(),
            arguments: args.to_string(),
        }],
        &[],
        &committed,
    )
    .await
}

fn query(error: &str) -> Value {
    serde_json::from_str(
        error
            .split("Next inspect_analysis query: ")
            .nth(1)
            .expect("actionable index query"),
    )
    .unwrap()
}

#[tokio::test]
async fn fabricated_id_error_navigates_local_index_without_receipts_or_mutation() {
    let (input, mut state, config) = fixture().await;
    let before = digest(&state).unwrap();
    let error = inspect(
        &input,
        &mut state,
        &config,
        &json!({"kind":"all","ids":["b-????"],"offset":9,"limit":1}),
    )
    .await
    .unwrap_err();
    assert!(error.starts_with("INVALID_FIELD /ids: unknown candidate ID:"));
    assert_eq!(digest(&state).unwrap(), before);
    let mut navigation = query(&error);
    assert_eq!(
        navigation,
        json!({"kind":"all","offset":0,"limit":1,"view":"index"})
    );
    let schema = tools::schemas(false)
        .into_iter()
        .find(|t| t["function"]["name"] == "inspect_analysis")
        .unwrap();
    assert!(
        jsonschema::JSONSchema::compile(&schema["function"]["parameters"])
            .unwrap()
            .is_valid(&navigation)
    );
    let mut found = false;
    loop {
        let page = inspect(&input, &mut state, &config, &navigation)
            .await
            .unwrap();
        found |= page["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["id"] == "b-full-identity");
        if page["next"] == page["total"] {
            break;
        }
        navigation["offset"] = page["next"].clone();
    }
    assert!(found);
    assert_eq!(
        digest(&state).unwrap(),
        before,
        "index grants neither detail nor progress/recovery"
    );
}

#[tokio::test]
async fn wrong_category_navigation_preserves_explicit_source_and_empty_scope_fallback() {
    let (mut input, mut state, config) = fixture().await;
    let mut outside = input.source_units[0].clone();
    outside.source_unit_revision_id = "outside".into();
    input.source_units.push(outside);
    let mut record = state.analysis.records["b-full-identity"].clone();
    record.sources[0].source_id = "outside".into();
    state.analysis.records.insert(record.id.clone(), record);
    let error = inspect(&input, &mut state, &config, &json!({"kind":"relation","ids":["b-full-identity"],"source_id":"outside","offset":3,"limit":1})).await.unwrap_err();
    let navigation = query(&error);
    assert_eq!(navigation["source_id"], "outside");
    let page = inspect(&input, &mut state, &config, &navigation)
        .await
        .unwrap();
    assert_eq!(page["total"], 1);
    assert_eq!(page["items"][0]["id"], "b-full-identity");
    let local = inspect(
        &input,
        &mut state,
        &config,
        &json!({"kind":"record","view":"index","offset":0,"limit":10}),
    )
    .await
    .unwrap();
    assert_eq!(local["total"], 1);
    state
        .main_work
        .as_mut()
        .unwrap()
        .source_scope
        .push("outside".into());
    let expanded = inspect(
        &input,
        &mut state,
        &config,
        &json!({"kind":"record","view":"index","offset":0,"limit":10}),
    )
    .await
    .unwrap();
    assert_eq!(expanded["total"], 2);
    state.main_work = None;
    let error = inspect(
        &input,
        &mut state,
        &config,
        &json!({"kind":"template","ids":["b-full-identity"],"offset":0,"limit":10}),
    )
    .await
    .unwrap_err();
    assert!(query(&error).get("source_id").is_none());
    let global = inspect(&input, &mut state, &config, &query(&error))
        .await
        .unwrap();
    assert!(
        global["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["id"] == "b-full-identity")
    );
}

#[tokio::test]
async fn recovered_index_query_uses_existing_small_budget_pagination() {
    let (input, mut state, mut config) = fixture().await;
    config.limits.max_tool_result_bytes = 1024;
    for index in 0..12 {
        let mut record = state.analysis.records["a-full-identity"].clone();
        record.id = format!("extra-{index}");
        state.analysis.records.insert(record.id.clone(), record);
    }
    let before = digest(&state).unwrap();
    let error = inspect(
        &input,
        &mut state,
        &config,
        &json!({"kind":"all","ids":["missing"],"offset":0,"limit":100}),
    )
    .await
    .unwrap_err();
    assert!(json!({"ok":false,"error":error}).to_string().len() <= 1024);
    let mut navigation = query(&error);
    let first = inspect(&input, &mut state, &config, &navigation)
        .await
        .unwrap();
    assert!(first["next"].as_u64().unwrap() < first["total"].as_u64().unwrap());
    assert!(serde_json::to_vec(&first).unwrap().len() <= 1024);
    navigation["offset"] = first["next"].clone();
    let second = inspect(&input, &mut state, &config, &navigation)
        .await
        .unwrap();
    assert!(second["next"].as_u64().unwrap() > first["next"].as_u64().unwrap());
    assert_eq!(digest(&state).unwrap(), before);
}

#[tokio::test]
async fn guidance_respects_error_budget_and_other_failures_are_unchanged() {
    let (input, mut state, mut config) = fixture().await;
    let args = json!({"kind":"all","ids":["missing"],"offset":0,"limit":1});
    let full = inspect(&input, &mut state, &config, &args)
        .await
        .unwrap_err();
    config.limits.max_tool_result_bytes = json!({"ok":false,"error":full}).to_string().len() - 1;
    let bounded = inspect(&input, &mut state, &config, &args)
        .await
        .unwrap_err();
    assert!(!bounded.contains("Next inspect_analysis query:"));
    assert!(
        json!({"ok":false,"error":bounded}).to_string().len()
            <= config.limits.max_tool_result_bytes
    );
    for args in [
        json!({"kind":"nonsense","ids":["missing"],"offset":0,"limit":1}),
        json!({"kind":"all","ids":["missing"],"source_id":"foreign","offset":0,"limit":1}),
        json!({"kind":"all","ids":["missing"],"offset":0,"limit":0}),
    ] {
        let error = inspect(&input, &mut state, &config, &args)
            .await
            .unwrap_err();
        assert!(!error.contains("Next inspect_analysis query:"));
    }
}
