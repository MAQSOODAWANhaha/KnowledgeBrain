use jsonschema::{Draft, JSONSchema};
use serde_json::{Value, json};

fn validator_from(root: Value) -> JSONSchema {
    JSONSchema::options()
        .with_draft(Draft::Draft202012)
        .compile(&root)
        .unwrap_or_else(|error| panic!("schema did not compile: {error}"))
}

#[test]
fn checked_in_schemas_are_only_the_live_contracts() {
    let mut names: Vec<_> = std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/schemas"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| name.ends_with(".schema.json"))
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "outline-tools-v2.schema.json",
            "response-tools-v1.schema.json",
            "tender-analysis-tools-v1.schema.json",
        ]
    );
}

#[test]
fn outline_and_response_models_see_the_closed_tool_set() {
    let outline: Value =
        serde_json::from_str(include_str!("../schemas/outline-tools-v2.schema.json")).unwrap();
    let response: Value =
        serde_json::from_str(include_str!("../schemas/response-tools-v1.schema.json")).unwrap();
    let names = |tools: &Value| -> Vec<_> {
        tools
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["function"]["name"].as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(
        names(&outline),
        [
            "submit_pack",
            "put_chapters",
            "bind_forms",
            "put_slots",
            "put_fulfillments",
            "read_outline",
            "read_requirements",
            "read_evidence",
            "read_source_view",
            "submit_review",
            "read_claim_evidence",
            "submit_claim_comparison",
            "finish_outline"
        ]
    );
    assert_eq!(names(&response), ["read_outline", "put_responses"]);
    for tool in outline
        .as_array()
        .unwrap()
        .iter()
        .chain(response.as_array().unwrap())
    {
        assert_eq!(
            tool["function"]["parameters"]["additionalProperties"],
            json!(false)
        );
    }
    let chapters = validator_from(outline[1]["function"]["parameters"].clone());
    let mut chapter = json!({"mode":"replace","chapters":[{"id":"letter","parent_id":null,"order":0,"title":"投标函","purpose":"response","requirement_ids":[]}]});
    assert!(chapters.is_valid(&chapter));
    chapter["chapters"][0]
        .as_object_mut()
        .unwrap()
        .remove("requirement_ids");
    assert!(!chapters.is_valid(&chapter));
    chapter["chapters"][0]["requirement_ids"] = json!([]);
    chapter["chapters"][0]["grounds"] = json!([]);
    assert!(!chapters.is_valid(&chapter));
    let responses = validator_from(response[1]["function"]["parameters"].clone());
    let mut row =
        json!({"responses":[{"slot_id":"bidder","status":"no_evidence","text":"【待人工补充】"}]});
    assert!(responses.is_valid(&row));
    row["responses"][0]["status"] = json!("invented");
    assert!(!responses.is_valid(&row));
}

#[test]
fn registry_parameters_match_the_single_checked_in_schema() {
    let checked: Vec<Value> =
        serde_json::from_str(include_str!("../schemas/outline-tools-v2.schema.json")).unwrap();
    for spec in bidding::outline::agent::registry() {
        let schema = checked
            .iter()
            .find(|schema| schema["function"]["name"] == spec.name)
            .unwrap();
        assert_eq!(spec.schema, *schema);
        validator_from(spec.schema["function"]["parameters"].clone());
    }
}

#[test]
fn collection_write_modes_are_explicit_and_closed() {
    let tools: Value =
        serde_json::from_str(include_str!("../schemas/outline-tools-v2.schema.json")).unwrap();
    for (name, field) in [
        ("put_chapters", "chapters"),
        ("bind_forms", "bindings"),
        ("put_slots", "slots"),
    ] {
        let tool = tools
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["function"]["name"] == name)
            .unwrap();
        let schema = validator_from(tool["function"]["parameters"].clone());
        let mut args = json!({field:[]});
        if field == "chapters" {
            args[field] = json!([{ "id":"response", "parent_id":null, "order":0, "title":"Response", "purpose":"response", "requirement_ids":[] }]);
        }
        assert!(!schema.is_valid(&args), "missing mode: {name}");
        args["mode"] = json!("append");
        assert!(!schema.is_valid(&args), "unknown mode: {name}");
        for mode in ["replace", "upsert"] {
            args["mode"] = json!(mode);
            assert!(schema.is_valid(&args), "valid mode: {name} {mode}");
        }
        assert!(!bidding::outline::agent::handles(&format!("{name}_append")));
    }
}
