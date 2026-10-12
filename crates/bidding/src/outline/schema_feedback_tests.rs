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
    let mut args = schema_feedback_submission();
    assert!(validate_arguments("submit_pack", &args).is_ok());
    args["requirements"][0]["kind"] = json!("informational");
    args["requirements"][0]["obligation_strength"] = json!("technical");
    let original = args.clone();
    let error = validate_arguments("submit_pack", &args).unwrap_err();
    let feedback: Value =
        serde_json::from_str(error.strip_prefix("SCHEMA/validation ").unwrap()).unwrap();
    let errors = feedback["errors"].as_array().unwrap();
    assert_eq!(errors.len(), 2);
    let kind = errors
        .iter()
        .find(|e| e["path"] == "/requirements/0/kind")
        .unwrap();
    assert_eq!(kind["code"], "enum");
    assert_eq!(kind["received"], "informational");
    assert!(
        kind["allowed"]
            .as_array()
            .unwrap()
            .contains(&json!("technical"))
    );
    assert!(
        !kind["allowed"]
            .as_array()
            .unwrap()
            .contains(&json!("informational"))
    );
    let strength = errors
        .iter()
        .find(|e| e["path"] == "/requirements/0/obligation_strength")
        .unwrap();
    assert_eq!(strength["received"], "technical");
    assert!(
        strength["allowed"]
            .as_array()
            .unwrap()
            .contains(&json!("informational"))
    );
    assert_eq!(feedback["state_changed"], false);
    assert_eq!(args, original);
    // Host reports legal values but does not choose or move either field.
    args["requirements"][0]["kind"] = json!("unknown");
    assert!(validate_arguments("submit_pack", &args).is_err());
    args["requirements"][0]["obligation_strength"] = json!("informational");
    assert!(validate_arguments("submit_pack", &args).is_ok());
}

#[test]
fn schema_feedback_collects_missing_and_extra_paths_without_echoing_large_text() {
    let mut args = schema_feedback_submission();
    args["requirements"][0]
        .as_object_mut()
        .unwrap()
        .remove("kind");
    args["requirements"][0]["invented_field"] = json!("synthetic-private-value".repeat(1000));
    let error = validate_arguments("submit_pack", &args).unwrap_err();
    assert!(!error.contains("synthetic-private-value"));
    let feedback: Value =
        serde_json::from_str(error.strip_prefix("SCHEMA/validation ").unwrap()).unwrap();
    let errors = feedback["errors"].as_array().unwrap();
    let missing = errors
        .iter()
        .find(|e| e["path"] == "/requirements/0/kind")
        .unwrap();
    assert_eq!(missing["received"], json!({"missing":true}));
    let extra = errors
        .iter()
        .find(|e| e["path"] == "/requirements/0/invented_field")
        .unwrap();
    assert_eq!(extra["received"]["omitted"], true);
    assert_eq!(extra["allowed"]["additional_property"], false);
}

#[test]
fn schema_feedback_groups_many_errors_without_changing_the_complete_candidate() {
    let mut args = schema_feedback_submission();
    args["requirements"] = json!(vec![json!({}); 1000]);
    let audit_arguments = args.to_string();
    let error = validate_arguments("submit_pack", &args).unwrap_err();
    assert!(error.len() <= SCHEMA_FEEDBACK_MAX_BYTES);
    let feedback: Value =
        serde_json::from_str(error.strip_prefix("SCHEMA/validation ").unwrap()).unwrap();
    assert_eq!(feedback["total_errors"], 6000);
    assert_eq!(feedback["shown_errors"], 18);
    assert_eq!(feedback["omitted_errors"], 5982);
    let groups = feedback["errors"].as_array().unwrap();
    assert_eq!(groups.len(), 6);
    for group in groups {
        assert_eq!(group["count"], 1000);
        assert_eq!(group["other_paths"].as_array().unwrap().len(), 2);
    }
    // Durable raw arguments are unchanged and can reproduce the same diagnostics
    // with the frozen schema. No fabricated repair or item truncation is applied.
    assert_eq!(args.to_string(), audit_arguments);
    let reloaded: Value = serde_json::from_str(&audit_arguments).unwrap();
    assert_eq!(
        validate_arguments("submit_pack", &reloaded).unwrap_err(),
        error
    );
    assert!(validate_arguments("submit_pack", &schema_feedback_submission()).is_ok());

    // Match the nested string envelope of the failed-submission audit. Retain
    // the entire original candidate; only the host diagnostic is compacted.
    let receipt = json!({"ok":false,"error":error});
    let body = json!({"model":"gpt-4o","messages":[{"role":"user","content":json!({
        "rejected_submission_audit":{"original_arguments_text":audit_arguments,
        "settled_submission_failure":receipt.to_string()}
    }).to_string()}],"tools":[]});
    let count = crate::agent_runtime::chat::estimate_request_tokens_with_reserve(
        &body,
        &crate::agent_runtime::chat::TokenizerProfile {
            model_id: "gpt-4o".into(),
            encoding: crate::agent_runtime::chat::TokenEncoding::O200kBase,
            calibration: None,
        },
        0,
        4096,
        16384,
    )
    .unwrap();
    assert!(
        count.total_context_tokens < 32768,
        "diagnostics must leave a usable correction window"
    );
}

#[test]
fn schema_feedback_bounds_distinct_errors_and_escaped_unicode_paths() {
    let mut args = schema_feedback_submission();
    for index in 0..100 {
        let field = format!("{}-{index}", "😀\"\\\n/".repeat(200));
        args["requirements"][0][field] = json!(index);
    }
    let original = args.clone();
    let error = validate_arguments("submit_pack", &args).unwrap_err();
    assert!(error.len() <= SCHEMA_FEEDBACK_MAX_BYTES);
    let feedback: Value =
        serde_json::from_str(error.strip_prefix("SCHEMA/validation ").unwrap()).unwrap();
    assert_eq!(feedback["total_errors"], 100);
    assert!(feedback["omitted_errors"].as_u64().unwrap() > 0);
    assert!(!feedback["errors"].as_array().unwrap().is_empty());
    assert!(feedback["errors"].as_array().unwrap().len() <= SCHEMA_FEEDBACK_MAX_GROUPS);
    for group in feedback["errors"].as_array().unwrap() {
        assert_eq!(group["path_truncated"], true);
        assert!(group["path"].as_str().unwrap().len() <= 259);
    }
    assert_eq!(args, original);
}
