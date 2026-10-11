// Synthetic final-wire contract tests; no provider calls or private documents.
fn slot_wire_fixture() -> (FrozenInput, Checkpoint, Value, Value) {
    let (input, mut state, _, refs) = super::super::fixture_discovered(true);
    synchronize_discovery(&input, &mut state).unwrap();
    apply_for_duty(&input, &mut state, "put_chapters", &json!({"mode":"upsert","chapters":[{
        "id":"leaf-demo","parent_id":null,"title":"Synthetic response","purpose":"response",
        "order":0,"requirement_ids":[]
    }]}), Duty::Organize).unwrap();
    // Explicit delivered-evidence seam; wire issuance alone is not read credit.
    state.outline_run.tool_draft.delivered_evidence = refs.clone();
    let mut body = json!({"messages":[{"role":"user","content":json!({"evidence":refs[0]}).to_string()}],"tools":schemas_for(Duty::Organize)});
    state.outline_run.tool_draft.source_keys = super::super::source_wire::request(&state, &mut body).unwrap();
    state.outline_run.tool_draft.model_wire = super::super::model_wire::project(&state, &mut body).unwrap();
    let host: Value = serde_json::from_str(body["messages"][0]["content"].as_str().unwrap()).unwrap();
    let parameters = body["tools"].as_array().unwrap().iter()
        .find(|tool|tool["function"]["name"]=="put_slots").unwrap()["function"]["parameters"].clone();
    (input, state, host, parameters)
}

fn dispatch_slot_wire(input: &FrozenInput, state: &mut Checkpoint, args: Value) -> Result<Value, String> {
    let args = state.outline_run.tool_draft.model_wire.decode(args, false)?;
    let args = super::super::source_wire::resolve(state, "put_slots", args)?;
    apply_for_duty(input, state, "put_slots", &args, Duty::Organize)
}

#[test]
fn slot_prompt_examples_match_final_projected_wire_and_strict_business_gate() {
    let (input, state, host, parameters) = slot_wire_fixture();
    let validator = jsonschema::JSONSchema::compile(&parameters).unwrap();
    let examples: Vec<Value> = OUTLINE_PROMPT.lines()
        .filter(|line|line.starts_with("{\"mode\":\"upsert\""))
        .map(|line|serde_json::from_str(line).unwrap()).collect();
    assert_eq!(examples.len(), 2, "both complete generated and source-copy examples are executable shapes");
    for mut example in examples {
        assert!(validator.is_valid(&example), "published synthetic shape must match final schema");
        example["wire_scope"] = host["wire_scope"].clone();
        let content = &mut example["slots"][0]["content"];
        let field = if content["type"]=="source_copy" {"refs"} else {"supporting_refs"};
        content[field] = json!([host["evidence"]]);
        let mut candidate = state.clone();
        assert!(validator.is_valid(&example));
        if example["slots"][0]["content"]["type"] == "source_copy" {
            let mut missing_purpose = example.clone();
            missing_purpose["slots"][0]["content"].as_object_mut().unwrap().remove("purpose");
            assert!(!validator.is_valid(&missing_purpose));
            let before = serde_json::to_value(&candidate).unwrap();
            assert!(dispatch_slot_wire(&input, &mut candidate, missing_purpose).is_err());
            assert_eq!(serde_json::to_value(&candidate).unwrap(), before);
        }
        dispatch_slot_wire(&input, &mut candidate, example.clone()).unwrap();
        assert_eq!(candidate.outline_run.tool_draft.slots.len(), 1);
        assert!(candidate.outline_run.tool_draft.reviewed_requirement_ids.is_empty());
        // Even a syntactically valid example cannot bypass actual read delivery.
        let mut unread = state.clone();
        unread.outline_run.tool_draft.delivered_evidence.clear();
        let before = serde_json::to_value(&unread).unwrap();
        assert!(dispatch_slot_wire(&input, &mut unread, example).is_err());
        assert_eq!(serde_json::to_value(&unread).unwrap(), before);
    }
}

#[test]
fn misplaced_generated_text_reports_exact_paths_and_preserves_state() {
    let (input, state, host, parameters) = slot_wire_fixture();
    let validator = jsonschema::JSONSchema::compile(&parameters).unwrap();
    let valid = json!({"mode":"upsert","wire_scope":host["wire_scope"],"slots":[{
        "slot_id":"slot-demo","chapter_id":"leaf-demo","text":"Synthetic instruction",
        "content":{"type":"generated_explanation","supporting_refs":[host["evidence"]]}
    }]});
    assert!(validator.is_valid(&valid));
    for malformed in ["nested_text", "missing_text", "integer_ref", "unknown_ref"] {
        let mut args = valid.clone();
        match malformed {
            "nested_text" => {
                let text = args["slots"][0].as_object_mut().unwrap().remove("text").unwrap();
                args["slots"][0]["content"]["text"] = text;
            }
            "missing_text" => { args["slots"][0].as_object_mut().unwrap().remove("text"); }
            "integer_ref" => args["slots"][0]["content"]["supporting_refs"] = json!([2]),
            "unknown_ref" => args["slots"][0]["content"]["supporting_refs"] = json!([{"source_key":"s999999"}]),
            _ => unreachable!(),
        }
        if malformed != "unknown_ref" { assert!(!validator.is_valid(&args)); }
        let mut candidate = state.clone();
        let before = serde_json::to_value(&candidate).unwrap();
        let error = dispatch_slot_wire(&input, &mut candidate, args).unwrap_err();
        if malformed == "nested_text" {
            assert!(error.contains("/slots/0/content/text"));
            assert!(error.contains("/slots/0/text"));
            assert!(error.contains("supporting_refs inside content"));
            assert!(error.len() <= 160);
        }
        if malformed == "missing_text" { assert!(error.contains("SCHEMA/required /slots/0/text")); }
        assert_eq!(serde_json::to_value(&candidate).unwrap(), before, "{malformed}");
    }
}
