use super::*;
use serde_json::json;

fn profile(encoding: TokenEncoding) -> TokenizerProfile {
    TokenizerProfile {
        model_id: match encoding {
            TokenEncoding::O200kBase => "gpt-4o-2024-08-06",
            TokenEncoding::Cl100kBase => "gpt-4-0613",
        }
        .into(),
        encoding,
        calibration: None,
    }
}

fn request() -> Value {
    json!({
        "model":"gpt-4o-2024-08-06", "max_tokens":8192,
        "messages":[
            {"role":"system","content":"按来源提取，不编造"},
            {"role":"user","content":"项目原文和约束"},
            {"role":"assistant","content":null,"tool_calls":[{"id":"call-1","type":"function","function":{"name":"read_source","arguments":"{\"source_id\":\"来源甲\"}"}}]},
            {"role":"tool","tool_call_id":"call-1","content":"完整原文结果"}
        ],
        "tools":[{"type":"function","function":{"name":"read_source","description":"读取完整来源","parameters":{"type":"object","properties":{"source_id":{"type":"string"}},"required":["source_id"],"additionalProperties":false}}}],
        "tool_choice":"required","stream":true,"stream_options":{"include_usage":true}
    })
}

#[test]
fn published_encoding_fixtures_count_tokens_not_bytes_or_characters() {
    // OpenAI Cookbook: How_to_count_tokens_with_tiktoken.ipynb.
    for (encoding, japanese_count) in [
        (TokenEncoding::Cl100kBase, 9),
        (TokenEncoding::O200kBase, 8),
    ] {
        let p = profile(encoding);
        assert_eq!(
            p.count_text_tokens("antidisestablishmentarianism").unwrap(),
            6
        );
        assert_eq!(p.count_text_tokens("2 + 2 = 4").unwrap(), 7);
        assert_eq!(
            p.count_text_tokens("お誕生日おめでとう").unwrap(),
            japanese_count
        );
        assert_eq!(p.count_text_tokens("hello world").unwrap(), 2);
        assert_eq!(p.count_text_tokens("").unwrap(), 0);
    }
}

#[test]
fn ordinary_special_token_spelling_is_not_a_reserved_token() {
    let p = profile(TokenEncoding::O200kBase);
    assert!(p.count_text_tokens("<|endoftext|>").unwrap() > 1);
}

#[test]
fn profile_rejects_unknown_models_wrong_encoding_and_model_drift() {
    let mut p = profile(TokenEncoding::O200kBase);
    assert!(p.validate_for_model("gpt-4o-mini").is_err());
    p.encoding = TokenEncoding::Cl100kBase;
    assert!(p.validate_for_model(&p.model_id).is_err());
    p.encoding = TokenEncoding::O200kBase;
    for name in [
        "vendor/custom-model",
        "gpt-5-unknown-vendor",
        "gpt-4o-unverified-suffix",
    ] {
        p.model_id = name.into();
        assert!(p.validate_for_model(name).is_err());
        assert!(p.count_text_tokens("hello").is_err());
    }
}

#[test]
fn unsupported_model_requires_explicit_nondiscounting_calibration() {
    let mut p = TokenizerProfile {
        model_id: "vendor/custom-model".into(),
        encoding: TokenEncoding::O200kBase,
        calibration: Some(TokenizerCalibration {
            multiplier_bps: 15_000,
            provenance: "synthetic test model double, 1.5x o200k_base".into(),
        }),
    };
    assert_eq!(p.count_text_tokens("hello world").unwrap(), 3);
    assert_eq!(p.count_text_tokens("hello").unwrap(), 2);
    p.calibration.as_mut().unwrap().multiplier_bps = 9_999;
    assert!(p.validate_for_model(&p.model_id).is_err());
    p.calibration.as_mut().unwrap().multiplier_bps = 10_000;
    p.calibration.as_mut().unwrap().provenance = "  ".into();
    assert!(p.validate_for_model(&p.model_id).is_err());
}

#[test]
fn whole_request_counts_tools_history_prompt_metadata_and_output_separately() {
    let p = profile(TokenEncoding::O200kBase);
    let body = request();
    let estimate = estimate_request_tokens(&body, &p, 16_000, 2048).unwrap();
    let canonical = serde_json_canonicalizer::to_string(&body).unwrap();
    assert_eq!(
        estimate.text_tokens,
        p.count_text_tokens(&canonical).unwrap()
    );
    assert_eq!(estimate.calibrated_text_tokens, estimate.text_tokens);
    assert_eq!(estimate.framing_tokens, 16 + 4 * 8 + 16);
    assert_eq!(
        estimate.total_input_tokens,
        estimate.text_tokens + estimate.framing_tokens + 2048
    );
    assert_eq!(estimate.output_reserved_tokens, 8192);
    assert_eq!(
        estimate.total_context_tokens,
        estimate.total_input_tokens + 8192
    );
    assert_eq!(
        estimate_input_tokens(&body, &p, 16_000, 2048).unwrap(),
        estimate.total_input_tokens
    );
    assert!(!estimate.calibrated);
    assert!(estimate.provenance.contains("tiktoken-rs 0.12.1"));
    let mut expanded = body;
    expanded["tools"][0]["function"]["description"] = json!("完整原文和约束".repeat(3000));
    assert!(
        estimate_input_tokens(&expanded, &p, 16_000, 2048).unwrap()
            > estimate.total_input_tokens + 3000
    );
    expanded["messages"][3]["content"] = json!("历史工具原文".repeat(3000));
    assert!(
        estimate_input_tokens(&expanded, &p, 16_000, 2048).unwrap()
            > estimate.total_input_tokens + 6000
    );
}

#[test]
fn image_payload_size_does_not_turn_into_text_tokens() {
    let p = profile(TokenEncoding::O200kBase);
    let mut body = request();
    body["messages"]
        .as_array_mut()
        .unwrap()
        .push(json!({"role":"user","content":[
            {"type":"text","text":"原页"},
            {"type":"image_url","image_url":{"url":"data:image/jpeg;base64,AAAA","detail":"high"}}
        ]}));
    let one = estimate_request_tokens(&body, &p, 16_000, 2048).unwrap();
    body["messages"][4]["content"][1]["image_url"]["url"] = json!(format!(
        "data:image/jpeg;base64,{}",
        "1234567890+/".repeat(300_000)
    ));
    assert_eq!(
        estimate_request_tokens(&body, &p, 16_000, 2048).unwrap(),
        one
    );
    body["messages"][4]["content"].as_array_mut().unwrap().push(json!({"type":"image_url","image_url":{"url":"https://example.invalid/another-image","detail":"low"}}));
    let two = estimate_request_tokens(&body, &p, 16_000, 2048).unwrap();
    assert_eq!(one.image_tokens, 16_000);
    assert_eq!(two.image_tokens, 32_000);
    assert!(two.total_input_tokens >= one.total_input_tokens + 16_000);
    assert!(estimate_request_tokens(&body, &p, 0, 2048).is_err());
}

#[test]
fn ordinary_base64_looking_tool_text_is_still_counted() {
    let p = profile(TokenEncoding::O200kBase);
    let mut body = request();
    let base = estimate_input_tokens(&body, &p, 16_000, 2048).unwrap();
    body["messages"][3]["content"] = json!(format!(
        "data:image/jpeg;base64,{}",
        "1234567890+/".repeat(1000)
    ));
    assert!(estimate_input_tokens(&body, &p, 16_000, 2048).unwrap() > base + 1000);
}

#[test]
fn calibrated_estimate_exposes_provenance_and_applies_ratio_once() {
    let mut p = profile(TokenEncoding::O200kBase);
    p.model_id = "test-model".into();
    p.calibration = Some(TokenizerCalibration {
        multiplier_bps: 12_500,
        provenance: "synthetic test model double measured fixture".into(),
    });
    let mut body = request();
    body["model"] = json!(p.model_id);
    let estimate = estimate_request_tokens(&body, &p, 16_000, 2048).unwrap();
    assert!(estimate.calibrated);
    assert_eq!(
        estimate.provenance,
        p.calibration.as_ref().unwrap().provenance
    );
    assert_eq!(
        estimate.calibrated_text_tokens,
        (estimate.text_tokens * 5).div_ceil(4)
    );
}

#[test]
fn missing_model_and_mismatched_request_fail_closed() {
    let p = profile(TokenEncoding::O200kBase);
    let mut body = request();
    body["model"] = json!("gpt-4o-mini");
    assert!(estimate_input_tokens(&body, &p, 16_000, 2048).is_err());
    body.as_object_mut().unwrap().remove("model");
    assert!(estimate_input_tokens(&body, &p, 16_000, 2048).is_err());
}

#[test]
fn fixed_byte_threshold_does_not_reject_valid_128k_token_context() {
    let p = profile(TokenEncoding::O200kBase);
    let mut body = request();
    body["messages"][1]["content"] = json!("采购项目的技术要求。".repeat(9000));
    let bytes = serde_json_canonicalizer::to_vec(&body).unwrap().len();
    let estimate = estimate_request_tokens(&body, &p, 16_000, 2048).unwrap();
    assert!(bytes > 128_000);
    assert!(estimate.total_context_tokens < 128_000);
    // Boundary test includes output exactly once and all input reserves.
    let remaining = 128_000 - estimate.total_context_tokens;
    assert_eq!(
        estimate_request_tokens(&body, &p, 16_000, 2048 + remaining)
            .unwrap()
            .total_context_tokens,
        128_000
    );
    assert_eq!(
        estimate_request_tokens(&body, &p, 16_000, 2049 + remaining)
            .unwrap()
            .total_context_tokens,
        128_001
    );
}

#[test]
fn text_requests_over_two_megabytes_are_admitted_by_tokens_only() {
    let p = profile(TokenEncoding::O200kBase);
    let mut body = request();
    // Fixed-width source rows can be byte-heavy but have few BPE tokens.
    let row = format!("{}a\n", " ".repeat(128));
    body["messages"][1]["content"] = json!(row.repeat(18_000));
    assert!(serde_json_canonicalizer::to_vec(&body).unwrap().len() > 2 * 1024 * 1024);
    assert!(
        estimate_request_tokens(&body, &p, 16_000, 2048)
            .unwrap()
            .total_context_tokens
            < 128_000
    );
}

#[test]
fn overflow_is_rejected_instead_of_wrapping_to_fit() {
    let p = profile(TokenEncoding::O200kBase);
    assert!(estimate_input_tokens(&request(), &p, 16_000, usize::MAX).is_err());
    let mut body = request();
    body["max_tokens"] = json!(u64::MAX);
    assert!(estimate_request_tokens(&body, &p, 16_000, 2048).is_err());
}

#[test]
fn maximum_piece_bound_matches_every_embedded_regular_token() {
    for (encoding, vocabulary_size) in [
        (TokenEncoding::Cl100kBase, 100_256),
        (TokenEncoding::O200kBase, 199_998),
    ] {
        let maximum = (0..vocabulary_size)
            .map(|rank| encoding.bpe().decode_bytes(&[rank]).unwrap().len())
            .max()
            .unwrap();
        assert_eq!(profile(encoding).max_piece_bytes(), maximum);
    }
}

#[test]
fn profile_serialization_is_explicit_and_has_no_silent_default() {
    let p = profile(TokenEncoding::O200kBase);
    let value = serde_json::to_value(&p).unwrap();
    assert_eq!(value["encoding"], "o200k_base");
    assert_eq!(
        serde_json::from_value::<TokenizerProfile>(value).unwrap(),
        p
    );
    assert!(serde_json::from_value::<TokenizerProfile>(json!({"model_id":"test"})).is_err());
}
