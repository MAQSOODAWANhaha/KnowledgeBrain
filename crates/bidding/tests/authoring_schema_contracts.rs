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
            "outline-tools-v1.schema.json",
            "response-tools-v1.schema.json",
            "tender-analysis-tools-v1.schema.json",
        ]
    );
}

#[test]
fn outline_and_response_models_see_the_closed_tool_set() {
    let outline: Value =
        serde_json::from_str(include_str!("../schemas/outline-tools-v1.schema.json")).unwrap();
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
            "bind_forms_append",
            "put_slots",
            "put_slots_append",
            "read_outline",
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
    let mut chapter = json!({"chapters":[{"id":"letter","parent_id":null,"order":0,"title":"投标函","purpose":"response","requirement_ids":[]}]});
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
