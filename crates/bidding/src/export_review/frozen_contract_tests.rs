use super::{FrozenContext, FrozenExecution, agent::Config};
use serde_json::json;

fn configured() -> Config {
    serde_json::from_value(json!({
        "provider": {"schema_version":1,"base_url":"https://example.invalid/v1",
            "endpoint":"https://example.invalid/v1/chat/completions",
            "protocol":"openai_chat_completions_sse","model_id":"frozen-fixture",
            "credential_ref":"env:LLM_API_KEY","stream":true,"max_tokens":8192,
            "timeout_ms":180000,"response_mode":"tool_calls","transport_retries":0,
            "temperature":null,"reasoning_effort":null},
        "limits":{"max_turns":8,"max_tool_calls":20,"max_physical_calls":20,
            "max_read_bytes":200000,"max_context_bytes":200000,"max_tool_result_bytes":20000}
    }))
    .unwrap()
}

#[test]
fn frozen_execution_round_trip_preserves_provider_budgets_and_credential_reference() {
    let config = configured();
    let frozen = FrozenExecution::freeze(&config).unwrap();
    let restored = frozen.config().unwrap();
    assert_eq!(
        serde_json::to_value(restored).unwrap(),
        serde_json::to_value(config).unwrap()
    );
    assert_eq!(
        frozen.definition["config"]["provider"]["credential_ref"],
        "env:LLM_API_KEY"
    );
}

#[test]
fn frozen_execution_rejects_mutated_config_and_rehashed_obsolete_prompt_or_tools() {
    let frozen = FrozenExecution::freeze(&configured()).unwrap();
    let mut changed = frozen.clone();
    changed.definition["config"]["provider"]["model_id"] = json!("replacement");
    assert_eq!(
        changed.config().unwrap_err().code,
        "FROZEN_INPUT_DIGEST_MISMATCH"
    );
    for key in ["reviewer", "tools"] {
        let mut changed = frozen.clone();
        changed.definition[key] = json!([]);
        changed.contract_sha256 = crate::tender_analysis::digest(&changed.definition).unwrap();
        assert_eq!(
            changed.config().unwrap_err().code,
            "FROZEN_INPUT_DIGEST_MISMATCH"
        );
    }
}

#[test]
fn analysis_identity_alone_does_not_claim_a_frozen_semantic_review() {
    let mut context = FrozenContext {
        analysis_identity: Some(json!({"schema_version":2})),
        execution_contract: None,
        layout_result: None,
    };
    assert!(!context.allows_semantic_export_review());
    context.execution_contract = Some(FrozenExecution::freeze(&configured()).unwrap());
    assert!(context.allows_semantic_export_review());
    context.analysis_identity = Some(json!({"schema_version":1}));
    assert!(!context.allows_semantic_export_review());
}

#[test]
fn omitted_limits_freeze_stably_and_explicit_invalid_limits_fail() {
    let mut config = configured();
    config.limits = super::agent::Limits::default();
    let default = FrozenExecution::freeze(&config).unwrap();
    config.limits = serde_json::from_value(json!({})).unwrap();
    assert_eq!(FrozenExecution::freeze(&config).unwrap(), default);
    config.limits = serde_json::from_value(json!({"max_turns": 0})).unwrap();
    assert!(FrozenExecution::freeze(&config).is_err());
    assert!(serde_json::from_value::<super::agent::Limits>(json!({"typo": 1})).is_err());
}
