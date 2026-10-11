// Schema diagnostics only: synthetic model proposals, no semantic correction.
fn schema_feedback_submission() -> Value {
    json!({"pack_id":"pack-0","call_id":"op_0000000000000000","claim_token":"cl_0000000000000000","pack_revision":1,
        "repair":false,"requirements":[{"description":"Synthetic requirement",
        "kind":"technical","obligation_strength":"mandatory","extraction_quality":"explicit",
        "source_section_id":{"atom_key":format!("atom_{}","0".repeat(64))},"evidence":[{"evidence_key":format!("ev_{}","0".repeat(24))}]}],
        "issues":[],"no_requirement_reason":null,"inspected_atom_ids":[{"atom_key":format!("atom_{}","0".repeat(64))}]})
}

#[test]
fn schema_feedback_reports_all_enum_paths_received_and_allowed_without_coercion() {
    let mut args=schema_feedback_submission();
    assert!(validate_arguments("submit_pack",&args).is_ok());
    args["requirements"][0]["kind"]=json!("informational");
    args["requirements"][0]["obligation_strength"]=json!("technical");
    let original=args.clone();
    let error=validate_arguments("submit_pack",&args).unwrap_err();
    let feedback:Value=serde_json::from_str(error.strip_prefix("SCHEMA/validation ").unwrap()).unwrap();
    let errors=feedback["errors"].as_array().unwrap();
    assert_eq!(errors.len(),2);
    let kind=errors.iter().find(|e|e["path"]=="/requirements/0/kind").unwrap();
    assert_eq!(kind["code"],"enum"); assert_eq!(kind["received"],"informational");
    assert!(kind["allowed"].as_array().unwrap().contains(&json!("technical")));
    assert!(!kind["allowed"].as_array().unwrap().contains(&json!("informational")));
    let strength=errors.iter().find(|e|e["path"]=="/requirements/0/obligation_strength").unwrap();
    assert_eq!(strength["received"],"technical");
    assert!(strength["allowed"].as_array().unwrap().contains(&json!("informational")));
    assert_eq!(feedback["state_changed"],false); assert_eq!(args,original);
    // Host reports legal values but does not choose or move either field.
    args["requirements"][0]["kind"]=json!("unknown");
    assert!(validate_arguments("submit_pack",&args).is_err());
    args["requirements"][0]["obligation_strength"]=json!("informational");
    assert!(validate_arguments("submit_pack",&args).is_ok());
}

#[test]
fn schema_feedback_collects_missing_and_extra_paths_without_echoing_large_text() {
    let mut args=schema_feedback_submission();
    args["requirements"][0].as_object_mut().unwrap().remove("kind");
    args["requirements"][0]["invented_field"]=json!("synthetic-private-value".repeat(1000));
    let error=validate_arguments("submit_pack",&args).unwrap_err();
    assert!(!error.contains("synthetic-private-value"));
    let feedback:Value=serde_json::from_str(error.strip_prefix("SCHEMA/validation ").unwrap()).unwrap();
    let errors=feedback["errors"].as_array().unwrap();
    let missing=errors.iter().find(|e|e["path"]=="/requirements/0/kind").unwrap();
    assert_eq!(missing["received"],json!({"missing":true}));
    let extra=errors.iter().find(|e|e["path"]=="/requirements/0/invented_field").unwrap();
    assert_eq!(extra["received"]["omitted"],true);
    assert_eq!(extra["allowed"]["additional_property"],false);
}
