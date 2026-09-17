use super::*;

fn item(text: &str) -> serde_json::Value {
    json!({"id":"", "kind":"composition", "text":text, "condition":"", "grounds":[span()], "targets":[]})
}
fn rule(items: Vec<serde_json::Value>) -> serde_json::Value {
    json!({"id":null,"sources":[span()],"data":{"kind":"rule","text":"依原文组织投标稿","scope":"本文件", "applicability":{"state":"applicable","condition":"按原文", "scope":"本文件","grounds":[span()]},"items":items}})
}
fn put(analysis: &mut Analysis, args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let mut coverage = analysis.coverage.clone();
    tools::invoke(
        &input(),
        analysis,
        &mut coverage,
        false,
        "put_record",
        args,
        100000,
    )
}
fn saved(analysis: &Analysis, id: &str) -> serde_json::Value {
    serde_json::to_value(&analysis.records[id]).unwrap()
}

#[test]
fn insertion_and_reordering_preserve_explicit_saved_item_ids() {
    let mut analysis = read_analysis(&input());
    let output = put(&mut analysis, &rule(vec![item("甲"), item("乙")])).unwrap();
    let id = output["id"].as_str().unwrap();
    assert_eq!(output["item_ids"], json!(["i1", "i2"]));
    let mut args = saved(&analysis, id);
    let old = args["data"]["items"].as_array().unwrap().clone();
    args["data"]["items"] = json!([item("新增"), old[1], old[0]]);
    let output = put(&mut analysis, &args).unwrap();
    assert_eq!(output["item_ids"], json!(["i3", "i2", "i1"]));
    assert_eq!(analysis.rule_item_sequences[id], 3);
}

#[test]
fn deletion_restore_and_received_replay_never_reuse_item_identity() {
    let mut analysis = read_analysis(&input());
    let output = put(&mut analysis, &rule(vec![item("甲"), item("乙")])).unwrap();
    let id = output["id"].as_str().unwrap();
    let mut args = saved(&analysis, id);
    args["data"]["items"] = json!([]);
    put(&mut analysis, &args).unwrap();
    let checkpoint = serde_json::to_value(&analysis).unwrap();
    args["data"]["items"] = json!([item("新项")]);
    let mut replay: Analysis = serde_json::from_value(checkpoint.clone()).unwrap();
    let first = put(&mut replay, &args).unwrap();
    let mut replay_again: Analysis = serde_json::from_value(checkpoint).unwrap();
    let second = put(&mut replay_again, &args).unwrap();
    assert_eq!(first["item_ids"], json!(["i3"]));
    assert_eq!(first, second);
    assert_eq!(digest(&replay).unwrap(), digest(&replay_again).unwrap());
    let mut coverage = replay.coverage.clone();
    tools::invoke(
        &input(),
        &mut replay,
        &mut coverage,
        false,
        "delete_record",
        &json!({"id":id}),
        100000,
    )
    .unwrap();
    assert_eq!(replay.rule_item_sequences[id], 3);
}

#[test]
fn failed_rule_update_does_not_consume_ids_or_accept_forged_ids() {
    let mut analysis = read_analysis(&input());
    let output = put(&mut analysis, &rule(vec![item("甲")])).unwrap();
    let id = output["id"].as_str().unwrap();
    let mut args = saved(&analysis, id);
    args["data"]["items"].as_array_mut().unwrap().push(item(""));
    let before = digest(&analysis).unwrap();
    assert!(put(&mut analysis, &args).is_err());
    assert_eq!(digest(&analysis).unwrap(), before);
    args["data"]["items"][1]["text"] = json!("乙");
    args["data"]["items"][1]["id"] = json!("invented-id");
    assert!(
        put(&mut analysis, &args)
            .unwrap_err()
            .contains("/data/items/1/id")
    );
    args["data"]["items"][1]["id"] = json!("");
    assert_eq!(
        put(&mut analysis, &args).unwrap()["item_ids"],
        json!(["i1", "i2"])
    );
    args["id"] = serde_json::Value::Null;
    assert!(
        put(&mut analysis, &args).is_err(),
        "new records cannot import invented explicit identities"
    );
}

#[test]
fn order_sequence_and_kind_specific_fields_are_validated() {
    let mut analysis = read_analysis(&input());
    let output = put(&mut analysis, &rule(vec![item("甲"), item("乙")])).unwrap();
    let id = output["id"].as_str().unwrap();
    let mut args = saved(&analysis, id);
    let mut order = item("甲在乙前");
    order["kind"] = json!("order");
    for sequence in [json!(["missing"]), json!(["i1", "i1"]), json!(["i3"])] {
        order["sequence"] = sequence;
        args["data"]["items"] = json!([
            saved(&analysis, id)["data"]["items"][0],
            saved(&analysis, id)["data"]["items"][1],
            order
        ]);
        assert!(put(&mut analysis, &args).unwrap_err().contains("/sequence"));
    }
    order["sequence"] = json!(["i1", "i2"]);
    order["format_key"] = json!("font_size");
    args["data"]["items"][2] = order.clone();
    assert!(
        put(&mut analysis, &args)
            .unwrap_err()
            .contains("format fields")
    );
    order.as_object_mut().unwrap().remove("format_key");
    args["data"]["items"][2] = order;
    put(&mut analysis, &args).unwrap();
    let mut args = saved(&analysis, id);
    args["data"]["items"][2]["kind"] = json!("format");
    args["data"]["items"][2]["format_key"] = json!("font_size");
    args["data"]["items"][2]["format_value"] = json!("12");
    assert!(put(&mut analysis, &args).unwrap_err().contains("/sequence"));
}

#[test]
fn advertised_rule_schema_requires_the_same_kind_specific_fields_as_the_host() {
    let tools = tools::schemas(false);
    let writer = tools
        .iter()
        .find(|tool| tool["function"]["name"] == "put_record")
        .unwrap();
    let schema = jsonschema::JSONSchema::compile(&writer["function"]["parameters"]).unwrap();
    for kind in ["composition", "signature", "submission_hint"] {
        let mut ordinary = item("来源明确的编制要求");
        ordinary["kind"] = json!(kind);
        let args = rule(vec![ordinary]);
        assert!(
            schema.is_valid(&args),
            "{kind} must not require order or format fields"
        );
        put(&mut read_analysis(&input()), &args).unwrap();
    }
    for (kind, fields) in [
        ("order", json!({})),
        ("order", json!({"sequence":[]})),
        ("format", json!({})),
        ("format", json!({"format_key":"body_font_pt"})),
        ("format", json!({"format_key":"","format_value":"12"})),
        (
            "format",
            json!({"format_key":"body_font_pt","format_value":""}),
        ),
    ] {
        let mut constrained = item("来源中的顺序或格式要求");
        constrained["kind"] = json!(kind);
        constrained
            .as_object_mut()
            .unwrap()
            .extend(fields.as_object().unwrap().clone());
        let args = rule(vec![constrained]);
        assert!(
            !schema.is_valid(&args),
            "the provider must see the conditional requirement: {args}"
        );
        let mut analysis = read_analysis(&input());
        let before = digest(&analysis).unwrap();
        assert!(put(&mut analysis, &args).is_err());
        assert_eq!(digest(&analysis).unwrap(), before);
    }
    let mut analysis = read_analysis(&input());
    let created = put(&mut analysis, &rule(vec![item("甲"), item("乙")])).unwrap();
    let mut args = saved(&analysis, created["id"].as_str().unwrap());
    let mut order = item("甲排在乙前");
    order["kind"] = json!("order");
    order["sequence"] = created["item_ids"].clone();
    let mut format = item("原文规定字体字号");
    format["kind"] = json!("format");
    format["format_key"] = json!("body_font_pt");
    format["format_value"] = json!("12");
    args["data"]["items"]
        .as_array_mut()
        .unwrap()
        .extend([order, format]);
    assert!(schema.is_valid(&args));
    put(&mut analysis, &args).unwrap();
}

#[test]
fn targets_use_current_local_items_or_explicit_unresolved() {
    let mut analysis = read_analysis(&input());
    let output = put(&mut analysis, &rule(vec![item("甲"), item("乙")])).unwrap();
    let id = output["id"].as_str().unwrap();
    let other = put(&mut analysis, &rule(vec![item("其他规则")])).unwrap();
    let mut args = saved(&analysis, id);
    for target in [
        json!({"kind":"rule_item","record_id":other["id"],"item_id":"i1"}),
        json!({"kind":"rule_item","record_id":id,"item_id":"i1"}),
        json!({"kind":"rule_item","record_id":id,"item_id":"missing"}),
        json!({"kind":"record","id":"future-chapter"}),
        json!({"kind":"unresolved","reason":""}),
    ] {
        args["data"]["items"][0]["targets"] = json!([target]);
        assert!(
            put(&mut analysis, &args)
                .unwrap_err()
                .contains("/targets/0")
        );
    }
    args["data"]["items"][0]["targets"] = json!([{"kind":"rule_item","record_id":id,"item_id":"i2"},{"kind":"unresolved","reason":"原文跨引用尚待核对"}]);
    put(&mut analysis, &args).unwrap();
    args["data"]["items"].as_array_mut().unwrap().remove(1);
    assert!(
        put(&mut analysis, &args)
            .unwrap_err()
            .contains("/targets/0")
    );
}

#[test]
fn allocation_history_changes_analysis_identity_but_not_semantic_scope() {
    let analysis = read_analysis(&input());
    let mut with_history = analysis.clone();
    with_history
        .rule_item_sequences
        .insert("deleted-rule".into(), 9);
    assert_ne!(digest(&analysis).unwrap(), digest(&with_history).unwrap());
    assert_eq!(
        rule_contract::scope_sha256(&input(), &analysis).unwrap(),
        rule_contract::scope_sha256(&input(), &with_history).unwrap()
    );
}

#[tokio::test]
async fn rule_item_allocation_replays_received_response_without_another_model_call() {
    let input = input();
    let config = config();
    let journal = MemoryJournal::default();
    *journal.interrupt_after.lock().unwrap() = Some(1);
    let initial = work_script(vec![("put_record", rule(vec![item("甲")]))]);
    let error = agent::run(
        &input,
        &config,
        &journal,
        &initial,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "INTERNAL");
    let before = journal.load().await.unwrap().unwrap();
    let id = before.analysis.records.keys().next().unwrap().clone();
    assert_eq!(before.analysis.rule_item_sequences[&id], 1);
    let mut args = saved(&before.analysis, &id);
    args["data"]["items"]
        .as_array_mut()
        .unwrap()
        .insert(0, item("新增"));
    let update = work_script(vec![("put_record", args)]);
    *journal.fail_boundary_ack.lock().unwrap() = Some(before.journal.sequence + 2);
    let error = agent::run(
        &input,
        &config,
        &journal,
        &update,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "INTERNAL");
    let received = journal.load().await.unwrap().unwrap();
    assert!(received.journal.response().is_some());
    assert_eq!(received.analysis.rule_item_sequences[&id], 1);
    let mut output = None;
    for _ in 0..2 {
        *journal.state.lock().unwrap() = Some(serde_json::from_value(json!(received)).unwrap());
        *journal.interrupt_after.lock().unwrap() = Some(before.turn + 1);
        let error = agent::run(
            &input,
            &config,
            &journal,
            &update,
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "INTERNAL");
        let committed = journal.load().await.unwrap().unwrap();
        assert_eq!(committed.analysis.rule_item_sequences[&id], 2);
        let ids = saved(&committed.analysis, &id)["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["id"].clone())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec![json!("i2"), json!("i1")]);
        let current = digest(&committed.analysis).unwrap();
        if let Some(previous) = &output {
            assert_eq!(previous, &current);
        }
        output = Some(current);
    }
    assert_eq!(update.bodies.lock().unwrap().len(), 1);
}
