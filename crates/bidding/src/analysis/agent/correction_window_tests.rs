use super::*;

#[test]
fn correction_window_configuration_and_receipt_authority() {
    let mut config = crate::analysis::tests::config();
    assert_eq!(config.limits.request_context_limit(true), 131072);
    config.limits.max_correction_context_tokens = Some(262144);
    config.validate().unwrap();
    assert_eq!(config.limits.request_context_limit(false), 131072);
    assert_eq!(config.limits.request_context_limit(true), 262144);
    for denied in [131071, 262145] {
        config.limits.max_correction_context_tokens = Some(denied);
        assert!(config.validate().is_err());
    }
    let mut history = vec![
        json!({"role":"assistant","tool_calls":[{"id":"a","function":{"name":"submit_pack","arguments":"{\"repair\":true}"}}]}),
        json!({"role":"tool","tool_call_id":"a","content":"{\"ok\":false,\"error\":\"BUSINESS_REJECTED\"}"}),
    ];
    assert!(correction_retry(&history));
    history[1]["tool_call_id"] = json!("foreign");
    assert!(!correction_retry(&history));
    history[1]["tool_call_id"] = json!("a");
    history.push(json!({"role":"assistant","tool_calls":[{"id":"b","function":{"name":"read_evidence","arguments":"{}"}}]}));
    history.push(json!({"role":"tool","tool_call_id":"b","content":"{\"ok\":true,\"result\":{}}"}));
    assert!(
        !correction_retry(&history),
        "old failure cannot widen ordinary reads"
    );
    assert!(!correction_retry(&[
        json!({"role":"user","content":"repair=true"})
    ]));
}
