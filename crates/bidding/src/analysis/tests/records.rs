use super::*;

#[test]
fn string_encoded_record_is_rejected_without_echoing_or_decoding_its_contents() {
    let input = input();
    let mut analysis = Analysis::default();
    let mut coverage = Coverage::default();
    coverage.text.insert("source".into(), vec![(0, span().end)]);
    let original = requirement();
    let mut args = original.clone();
    args["data"] = json!(original["data"].to_string().repeat(256));
    let before = digest(&analysis).unwrap();
    let coverage_before = digest(&coverage).unwrap();
    let error = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "put_record",
        &args,
        8192,
    )
    .unwrap_err();
    assert!(error.starts_with("INVALID_FIELD /data:"), "{error}");
    assert!(error.contains("object directly"), "{error}");
    assert!(
        error.len() < 200,
        "bad payload must not be echoed into context"
    );
    assert_eq!(digest(&analysis).unwrap(), before);
    assert_eq!(digest(&coverage).unwrap(), coverage_before);
    // Correcting the shape still runs all original semantic/evidence guards.
    assert!(
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            "put_record",
            &original,
            8192,
        )
        .is_ok()
    );
}

#[test]
fn grid_grounded_obligations_publish_without_fabricating_text_quotes() {
    let input = grid_citation_input();
    let mut analysis = Analysis::default();
    let mut coverage = Coverage::default();
    let read = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "read_form",
        &json!({"form_id":"grid","offset":0,"limit":4}),
        8192,
    )
    .unwrap();
    let response = read["citations"][2].clone();
    let delivery = read["citations"][3].clone();
    let mut record = requirement();
    record["sources"] = json!([response, delivery]);
    record["data"]["response"][0]["grounds"] = json!([response]);
    record["data"]["applicability"]["grounds"] = json!([response]);
    record["data"]["compliance"][0]["grounds"] = json!([response]);
    record["data"]["proofs"] = json!([{"description":"供货时证书","subject":"产品","validity":"原文未规定",
        "issuer":"原文未规定","condition":"供货时提供","grounds":[delivery],"name_required":false,"page_required":false}]);
    let schema = tools::schemas(false)
        .into_iter()
        .find(|t| t["function"]["name"] == "put_record")
        .unwrap();
    assert!(
        jsonschema::JSONSchema::compile(&schema["function"]["parameters"])
            .unwrap()
            .is_valid(&record)
    );
    let id = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "put_record",
        &record,
        8192,
    )
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let result = AnalysisResult {
        schema_version: 1,
        frozen_input_sha256: digest(&input).unwrap(),
        review: Review {
            analysis_sha256: digest(&analysis).unwrap(),
            coverage: Coverage::default(),
            findings: vec![],
            ..Default::default()
        },
        analysis,
        quality: "needs_review".into(),
        source_views: BTreeMap::new(),
    };
    let publication = super::postgres::publication(&input, &result).unwrap();
    let req = &publication["requirements"][0];
    assert_eq!(req["source_spans"], json!([]));
    assert_eq!(req["structured_form_revision_ids"], json!(["grid"]));
    assert_eq!(req["response_needs"][0]["grounds"][0], response);
    assert_eq!(
        publication["analysis_result"]["analysis"]["records"][&id]["data"]["proofs"][0]["grounds"]
            [0],
        delivery
    );
    assert_eq!(
        publication["analysis_result"]["analysis"]["records"][&id]["data"]["proofs"][0]["condition"],
        "供货时提供"
    );
}

#[test]
fn template_text_policies_cannot_copy_bidder_slots_or_duplicate_fixed_wording() {
    let input = input();
    let mut analysis = Analysis::default();
    analysis
        .coverage
        .text
        .insert("source".into(), vec![(0, span().end)]);
    let mut record: Record = serde_json::from_value(json!({"id":"template","sources":[span()],"data":{
        "kind":"template","label":"甲","title":"格式","parent":null,"order":null,"purpose":"投标格式",
        "applicability":{"state":"applicable","condition":"本项目","scope":"投标文件","grounds":[span()]},
        "regions":[{"source":span(),"role":"fixed_text","form_id":null,"cells":[],"instruction":"保留固定文字"}]}})).unwrap();
    assert!(tools::validate_record(&input, &analysis, &record).is_ok());
    for role in [
        RegionRole::FixedText,
        RegionRole::BidderBlank,
        RegionRole::Signature,
    ] {
        let mut overlapping = record.clone();
        let RecordData::Template { regions, .. } = &mut overlapping.data else {
            unreachable!()
        };
        let mut region = regions[0].clone();
        region.role = role;
        // Partial overlap is also unsafe, not only identical whole-page ranges.
        region.source.start = 3;
        regions.push(region);
        let before = digest(&analysis).unwrap();
        let error = tools::validate_record(&input, &analysis, &overlapping).unwrap_err();
        assert!(
            error.starts_with("INVALID_FIELD /data/regions/1/source:"),
            "{error}"
        );
        assert!(error.contains("regions 0 and 1 overlap"));
        let mut args = serde_json::to_value(&overlapping).unwrap();
        args["id"] = Value::Null;
        let mut coverage = analysis.coverage.clone();
        assert!(
            tools::invoke(
                &input,
                &mut analysis,
                &mut coverage,
                false,
                "put_record",
                &args,
                16000
            )
            .unwrap_err()
            .contains("regions 0 and 1 overlap")
        );
        assert_eq!(digest(&analysis).unwrap(), before);
    }
    let RecordData::Template { regions, .. } = &mut record.data else {
        unreachable!()
    };
    let mut blank = regions[0].clone();
    regions[0].source.end = 3;
    blank.source.start = 3;
    blank.role = RegionRole::BidderBlank;
    regions.push(blank);
    assert!(
        tools::validate_record(&input, &analysis, &record).is_ok(),
        "adjacent byte ranges remain valid"
    );
}

#[test]
fn template_grid_regions_must_be_contiguous_before_review() {
    let (input, mut analysis, _) = field_relation_fixture();
    let mut record = analysis.records["template-a"].clone();
    let RecordData::Template { regions, .. } = &mut record.data else {
        unreachable!()
    };
    let mut note = regions[0].clone();
    note.form_id = None;
    note.cells.clear();
    regions.insert(1, note);
    let before = digest(&analysis).unwrap();
    let mut coverage = analysis.coverage.clone();
    let coverage_before = digest(&coverage).unwrap();
    assert!(
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            "put_record",
            &serde_json::to_value(&record).unwrap(),
            16000
        )
        .unwrap_err()
        .contains("contiguous")
    );
    assert_eq!(digest(&analysis).unwrap(), before);
    assert_eq!(digest(&coverage).unwrap(), coverage_before);
    let RecordData::Template { regions, .. } = &mut record.data else {
        unreachable!()
    };
    let note = regions.remove(1);
    regions.push(note);
    assert!(
        tools::validate_record(&input, &analysis, &record).is_ok(),
        "an explicitly placed note after a complete grid remains valid"
    );
}

#[test]
fn visual_evidence_cannot_be_mistaken_for_editable_template_wording() {
    let (input, mut analysis, _) = field_relation_fixture();
    let view = test_view();
    let view_id = digest(&view.identity).unwrap();
    analysis
        .coverage
        .views
        .insert(view_id.clone(), view.identity);
    let visual = Span {
        source_id: "source".into(),
        start: 0,
        end: 0,
        view_id: Some(view_id),
        grid_cell: None,
    };
    assert!(tools::validate_span(&input, &analysis.coverage, &visual).is_ok());
    let mut record = analysis.records["template-a"].clone();
    let RecordData::Template { regions, .. } = &mut record.data else {
        unreachable!()
    };
    let mut wording = regions[0].clone();
    wording.source = visual.clone();
    wording.form_id = None;
    wording.cells.clear();
    *regions = vec![wording];
    let before = digest(&analysis).unwrap();
    let mut coverage = analysis.coverage.clone();
    let coverage_before = digest(&coverage).unwrap();
    assert!(
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            "put_record",
            &serde_json::to_value(&record).unwrap(),
            16000
        )
        .unwrap_err()
        .contains("editable")
    );
    assert_eq!(digest(&analysis).unwrap(), before);
    assert_eq!(digest(&coverage).unwrap(), coverage_before);
    record.sources = vec![visual];
    record.data = RecordData::Unresolved {
        problem: "原页可读，仍需统一解析服务提供可编辑文字".into(),
        affected: vec![],
        candidates: vec![],
    };
    assert!(tools::validate_record(&input, &analysis, &record).is_ok());
}

#[test]
fn empty_grid_anchor_requires_an_explicit_role_without_defaulting_to_bidder_blank() {
    let (mut input, analysis, _) = field_relation_fixture();
    input.structured_forms[0]["definition"]["cells"][2]["text"] = json!("");
    let mut record = analysis.records["template-a"].clone();
    let RecordData::Template { regions, .. } = &mut record.data else {
        unreachable!()
    };
    let empty_cell = regions[1].cells.pop().unwrap();
    assert!(
        tools::validate_record(&input, &analysis, &record)
            .unwrap_err()
            .contains("every output grid anchor")
    );
    let RecordData::Template { regions, .. } = &mut record.data else {
        unreachable!()
    };
    let mut policy = regions[0].clone();
    policy.cells = vec![empty_cell];
    policy.instruction = "保留原表空格".into();
    regions.push(policy);
    let before = digest(&record).unwrap();
    assert!(tools::validate_record(&input, &analysis, &record).is_ok());
    assert_eq!(digest(&record).unwrap(), before);
    let RecordData::Template { regions, .. } = &record.data else {
        unreachable!()
    };
    assert_eq!(regions.last().unwrap().role, RegionRole::FixedText);
}

#[test]
fn template_grid_requires_every_anchor_role() {
    let (input, mut analysis, _) = field_relation_fixture();
    let record = analysis.records["template-a"].clone();
    assert!(
        tools::validate_record(&input, &analysis, &record).is_ok(),
        "fixture assigns every actual anchor"
    );
    let mut incomplete = record.clone();
    let RecordData::Template { regions, .. } = &mut incomplete.data else {
        unreachable!()
    };
    regions[1].cells.pop();
    let error = tools::validate_record(&input, &analysis, &incomplete).unwrap_err();
    assert!(error.starts_with("INVALID_FIELD /data/regions:"), "{error}");
    assert!(error.contains("every output grid anchor"));
    assert!(error.contains("first missing anchor Some("));
    let mut args = serde_json::to_value(&incomplete).unwrap();
    args["id"] = Value::Null;
    let before = digest(&analysis).unwrap();
    let mut coverage = analysis.coverage.clone();
    assert!(
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            "put_record",
            &args,
            16000,
        )
        .unwrap_err()
        .contains("every output grid anchor")
    );
    assert_eq!(digest(&analysis).unwrap(), before);
}

#[test]
fn foreign_spans_and_reviewer_mutations_are_rejected() {
    let input = input();
    let mut analysis = Analysis::default();
    let mut coverage = Coverage::default();
    let foreign = Span {
        view_id: None,
        grid_cell: None,
        source_id: "other-project".into(),
        start: 0,
        end: 3,
    };
    assert!(tools::validate_span(&input, &coverage, &foreign).is_err());
    assert!(
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            true,
            "set_disposition",
            &json!({"source_id":"source","state":"unresolved","reason":"x"}),
            1024
        )
        .is_err()
    );
    assert!(analysis.dispositions.is_empty());
}

#[test]
fn compliance_without_a_submission_requirement_does_not_invent_a_response() {
    let mut input = input();
    input.source_units[0].text = "费用由投标人自行承担。".into();
    let citation = Span {
        end: input.source_units[0].text.len(),
        ..span()
    };
    let mut analysis = read_analysis(&input);
    let mut coverage = analysis.coverage.clone();
    let mut args = requirement();
    args["sources"] = json!([citation]);
    args["data"]["text"] = json!(input.source_units[0].text);
    args["data"]["categories"] = json!(["commercial"]);
    args["data"]["compliance"] =
        json!([{"policy":"must_comply","condition":"投标活动","grounds":[citation]}]);
    args["data"]["applicability"] = json!({"state":"applicable","condition":"投标活动","scope":"费用承担","grounds":[citation]});
    args["data"]["response"] = json!([]);
    let result = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "put_record",
        &args,
        16000,
    )
    .unwrap();
    let record = &analysis.records[result["id"].as_str().unwrap()];
    assert_eq!(json!(record.data)["response"], json!([]));
    assert_eq!(json!(record.data)["compliance"], args["data"]["compliance"]);
}

#[test]
fn same_compliance_policy_preserves_distinct_conditions_and_ground_ranges() {
    let mut input = input();
    let first = "第一阶段自行承担费用。";
    input.source_units[0].text = format!("{first}第二阶段自行承担费用。");
    let all = Span {
        end: input.source_units[0].text.len(),
        ..span()
    };
    let grounds = [
        Span {
            end: first.len(),
            ..span()
        },
        Span {
            start: first.len(),
            ..all.clone()
        },
    ];
    let mut args = requirement();
    args["sources"] = json!([all]);
    args["data"]["text"] = json!(input.source_units[0].text);
    args["data"]["categories"] = json!(["commercial"]);
    args["data"]["applicability"]["grounds"] = json!([all]);
    args["data"]["response"] = json!([]);
    args["data"]["compliance"] = json!([
        {"policy":"must_comply","condition":"第一阶段","grounds":[grounds[0]]},
        {"policy":"must_comply","condition":"第二阶段","grounds":[grounds[1]]}
    ]);
    let mut analysis = read_analysis(&input);
    let mut coverage = analysis.coverage.clone();
    let saved = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "put_record",
        &args,
        16000,
    )
    .unwrap();
    assert_eq!(
        json!(analysis.records[saved["id"].as_str().unwrap()].data)["compliance"],
        args["data"]["compliance"]
    );
    let result = AnalysisResult {
        schema_version: 1,
        frozen_input_sha256: digest(&input).unwrap(),
        review: Review {
            analysis_sha256: digest(&analysis).unwrap(),
            coverage: Coverage::default(),
            findings: vec![],
            ..Default::default()
        },
        analysis,
        quality: "needs_review".into(),
        source_views: BTreeMap::new(),
    };
    let publication = super::postgres::publication(&input, &result).unwrap();
    assert_eq!(
        publication["requirements"][0]["compliance_policy"],
        "must_comply"
    );
}

#[test]
fn record_validation_identifies_the_rejected_field_without_partial_writes() {
    let input = input();
    let mut analysis = read_analysis(&input);
    let mut coverage = analysis.coverage.clone();
    let mut valid = requirement();
    valid["data"]["criteria"] = json!([{"subject":"设备","aspect":"数量","operator":"=","value":"1","unit":"","condition":"本次交付","grounds":[span()]}]);
    valid["data"]["proofs"] = json!([{"description":"证明材料","subject":"投标人","validity":"","issuer":"","name_required":false,"page_required":false,"condition":"按原文提交","grounds":[span()]}]);
    for (path, replacement) in [
        ("/data/text", json!(" ")),
        ("/data/categories", json!([])),
        ("/data/response", json!([])),
        ("/data/response/0/condition", json!(" ")),
        ("/data/response/0/description", json!("")),
        ("/data/response/0/grounds", json!([])),
        (
            "/data/response/0/grounds/0",
            json!({"source_id":"source","start":1,"end":2}),
        ),
        ("/data/applicability/scope", json!("")),
        ("/data/criteria/0/operator", json!("")),
        ("/data/proofs/0/subject", json!("")),
        (
            "/sources/0",
            json!({"source_id":"missing","start":0,"end":1}),
        ),
    ] {
        let mut args = valid.clone();
        *args.pointer_mut(path).unwrap() = replacement;
        let before = (digest(&analysis).unwrap(), digest(&coverage).unwrap());
        let error = tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            "put_record",
            &args,
            16000,
        )
        .unwrap_err();
        assert!(
            error.starts_with(&format!("INVALID_FIELD {path}:")),
            "{error}"
        );
        assert_eq!(
            before,
            (digest(&analysis).unwrap(), digest(&coverage).unwrap())
        );
    }
    let mut duplicate = valid.clone();
    let policy = duplicate["data"]["compliance"][0].clone();
    duplicate["data"]["compliance"]
        .as_array_mut()
        .unwrap()
        .push(policy);
    let error = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "put_record",
        &duplicate,
        16000,
    )
    .unwrap_err();
    assert!(
        error.starts_with("INVALID_FIELD /data/compliance/1:"),
        "{error}"
    );
    assert!(error.contains("/data/compliance/0"), "{error}");
    assert!(analysis.records.is_empty());
    tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "put_record",
        &valid,
        16000,
    )
    .unwrap();
    assert_eq!(
        analysis.records.len(),
        1,
        "an unspecified numeric unit stays empty"
    );
}

#[test]
fn response_without_an_extra_trigger_preserves_parent_conditions_and_required_evidence() {
    let mut input = input();
    input.source_units[0].text =
        "提供A类设备时提交证明文件；若采用替代产品，还须附型号对照表。".into();
    let citation = Span {
        end: input.source_units[0].text.len(),
        ..span()
    };
    let mut analysis = read_analysis(&input);
    let mut coverage = analysis.coverage.clone();
    let args = json!({"id":null,"sources":[citation],"data":{
        "kind":"requirement","text":input.source_units[0].text,
        "categories":["technical"],"strength":"mandatory",
        "applicability":{"state":"conditional","condition":"提供A类设备时","scope":"A类设备","grounds":[citation]},
        "compliance":[{"policy":"explicit_response","condition":"提供A类设备时","grounds":[citation]}],
        "response":[
            {"channel":"evidence_attachment","description":"提交证明文件","condition":"","grounds":[citation]},
            {"channel":"structured_form","description":"附型号对照表","condition":"若采用替代产品","grounds":[citation]}
        ],
        "proofs":[],"criteria":[],"scoring_rule":null
    }});
    let schemas = tools::schemas(false);
    let writer = schemas
        .iter()
        .find(|tool| tool["function"]["name"] == "put_record")
        .unwrap();
    let schema = jsonschema::JSONSchema::compile(&writer["function"]["parameters"]).unwrap();
    assert!(
        schema.is_valid(&args),
        "empty additional condition with a grounded parent must satisfy the real tool schema"
    );
    // Some compatible Chat gateways corrupt nested object output when given
    // nonblank regex constraints. Advertise minLength plus a description;
    // whitespace semantics remain a strict host check, never a fallback value.
    let mut whitespace = args.clone();
    whitespace["data"]["applicability"]["condition"] = json!(" \t\n");
    assert!(schema.is_valid(&whitespace));
    let before = (digest(&analysis).unwrap(), digest(&coverage).unwrap());
    let error = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "put_record",
        &whitespace,
        16000,
    )
    .unwrap_err();
    assert!(
        error.starts_with("INVALID_FIELD /data/applicability/condition:"),
        "{error}"
    );
    assert_eq!(
        before,
        (digest(&analysis).unwrap(), digest(&coverage).unwrap())
    );
    for (path, replacement) in [
        ("/data/response/0/condition", json!(" ")),
        ("/data/response/0/condition", json!("\t\n")),
        ("/data/response/0/description", json!("")),
        ("/data/response/0/grounds", json!([])),
        ("/data/applicability/condition", json!("")),
        ("/data/compliance/0/condition", json!("")),
    ] {
        let mut invalid = args.clone();
        *invalid.pointer_mut(path).unwrap() = replacement;
        if path == "/data/response/0/condition" {
            assert!(
                schema.is_valid(&invalid),
                "whitespace is rejected by the host"
            );
        } else {
            assert!(!schema.is_valid(&invalid), "tool schema must reject {path}");
        }
        let before = (digest(&analysis).unwrap(), digest(&coverage).unwrap());
        let error = tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            "put_record",
            &invalid,
            16000,
        )
        .unwrap_err();
        assert!(
            error.starts_with(&format!("INVALID_FIELD {path}:")),
            "{error}"
        );
        assert_eq!(
            before,
            (digest(&analysis).unwrap(), digest(&coverage).unwrap())
        );
    }
    let mut missing_key = args.clone();
    missing_key["data"]["response"][0]
        .as_object_mut()
        .unwrap()
        .remove("condition");
    assert!(
        !schema.is_valid(&missing_key),
        "optional value does not make the condition key optional"
    );
    assert!(serde_json::from_value::<RecordData>(missing_key["data"].clone()).is_err());
    let result = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "put_record",
        &args,
        16000,
    )
    .unwrap();
    let record = &analysis.records[result["id"].as_str().unwrap()];
    let RecordData::Requirement {
        applicability,
        compliance,
        response,
        ..
    } = &record.data
    else {
        panic!("requirement expected")
    };
    assert_eq!(applicability.state, ApplicabilityState::Conditional);
    assert_eq!(applicability.condition, "提供A类设备时");
    assert_eq!(compliance[0].condition, "提供A类设备时");
    assert_eq!(
        response[0].condition, "",
        "no fabricated fallback or copied parent condition"
    );
    assert_eq!(response[1].condition, "若采用替代产品");
    assert_eq!(json!(response[0].grounds), json!([citation]));
}

#[test]
fn missing_strength_or_applicability_cannot_default_to_mandatory() {
    let mut value = requirement()["data"].clone();
    value.as_object_mut().unwrap().remove("strength");
    assert!(serde_json::from_value::<RecordData>(value).is_err());
    let mut value = requirement()["data"].clone();
    value["strength"] = json!("unknown");
    value["applicability"]["state"] = json!("unknown");
    let r: RecordData = serde_json::from_value(value).unwrap();
    assert!(matches!(
        r,
        RecordData::Requirement {
            strength: Strength::Unknown,
            ..
        }
    ));
}

#[test]
fn concurrent_policies_and_criteria_preserve_evidence_and_units() {
    let mut input = input();
    // Synthetic protocol fixture; real sample semantic accuracy needs model evaluation.
    input.source_units[0].text =
        "★网络处理能力≥20Gbps；不响应否决投标，优于要求加分，并提供证明材料。".into();
    let span = json!({"source_id":"source","start":0,"end":input.source_units[0].text.len()});
    let mut value = requirement();
    value["sources"] = json!([span]);
    value["data"]["compliance"] = json!([
        {"policy":"must_comply","condition":"不响应否决","grounds":[span]},
        {"policy":"scored","condition":"优于要求","grounds":[span]},
        {"policy":"explicit_response","condition":"提交证明","grounds":[span]}
    ]);
    value["data"]["criteria"] = json!([{"subject":"所投设备","aspect":"网络处理能力",
        "operator":"≥","value":"20","unit":"Gbps","condition":"★条款","grounds":[span]}]);
    let mut analysis = read_analysis(&input);
    let mut coverage = analysis.coverage.clone();
    let result = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "put_record",
        &value,
        16000,
    )
    .unwrap();
    let record = &analysis.records[result["id"].as_str().unwrap()];
    let RecordData::Requirement {
        compliance,
        criteria,
        ..
    } = &record.data
    else {
        panic!("requirement expected")
    };
    assert_eq!(compliance.len(), 3);
    assert_eq!(criteria[0].value, "20");
    assert_eq!(criteria[0].unit, "Gbps");
    value["data"]["criteria"][0]["grounds"][0]["source_id"] = json!("foreign-source");
    assert!(
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            "put_record",
            &value,
            16000
        )
        .is_err()
    );
    assert_eq!(
        analysis.records.len(),
        1,
        "invalid criterion must not leave a partial record"
    );
}

#[test]
fn requirement_disposition_cannot_be_satisfied_by_an_unrelated_fact() {
    let input = input();
    let mut analysis = read_analysis(&input);
    let mut coverage = analysis.coverage.clone();
    tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "put_record",
        &json!({"id":null,"sources":[span()],
        "data":{"kind":"fact","name":"附件名称","value":"甲","scope":"本标"}}),
        16000,
    )
    .unwrap();
    tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "set_disposition",
        &json!({"source_id":"source",
        "state":"requirement","reason":"有事实记录不等于提取了提交义务"}),
        16000,
    )
    .unwrap();
    assert!(
        tools::gaps(&input, &analysis)
            .iter()
            .any(|gap| gap["kind"] == "disposition_without_record")
    );
}

#[tokio::test]
async fn reviewed_conditions_do_not_assert_bidder_applicability_or_hide_unknowns() {
    for kind in ["requirement", "rule", "template"] {
        for state in ["conditional", "unknown"] {
            let mut source = input();
            source.source_units[0].text = "代理商投标时须提供授权书。".into();
            let grounds = Span {
                end: source.source_units[0].text.len(),
                ..span()
            };
            let applicability = json!({"state":state,
                "condition":"代理商投标时", "scope":"本次投标", "grounds":[grounds]});
            let mut record = requirement();
            record["sources"] = json!([grounds]);
            record["data"] = match kind {
                "rule" => json!({"kind":"rule","text":"代理商提供授权书", "scope":"本次投标",
                    "applicability":applicability}),
                "template" => json!({"kind":"template","label":"附表甲","title":"授权书",
                    "parent":null,"order":null,"purpose":"代理商投标授权格式",
                    "applicability":applicability,"regions":[{"source":grounds,
                    "role":"instruction","form_id":null,"cells":[],"instruction":"保留适用条件"}]}),
                _ => {
                    let mut data = record["data"].clone();
                    data["applicability"] = applicability.clone();
                    data
                }
            };
            let model = script();
            for (name, args) in model.calls.lock().unwrap().iter_mut() {
                if name == "put_record" {
                    *args = record.clone();
                }
                if name == "put_source_review"
                    && (args == &json!({}) || args.get("fixture_status").is_some())
                {
                    if args.get("fixture_status").is_none() {
                        args["fixture_status"] = json!("checked");
                    }
                    args["fixture_template_mappings"] = json!(kind == "template");
                    args["fixture_relationship_status"] = json!(if state == "unknown" {
                        "source_limited"
                    } else {
                        "not_required"
                    });
                }
            }
            let result = agent::run(
                &source,
                &config(),
                &MemoryJournal::default(),
                &model,
                &CancellationToken::new(),
            )
            .await
            .unwrap();
            assert_eq!(
                result.quality,
                if state == "conditional" {
                    "verified"
                } else {
                    "needs_review"
                },
                "{kind}/{state}"
            );
            assert!(result.review.findings.is_empty());
            let saved =
                serde_json::to_value(&result.analysis.records.values().next().unwrap().data)
                    .unwrap();
            assert_eq!(
                saved["applicability"], applicability,
                "review cannot resolve bidder identity or discard the source condition"
            );
        }
    }
}

#[test]
fn a_conditional_claim_still_requires_read_source_grounds_and_a_condition() {
    let source = input();
    let analysis = read_analysis(&source);
    for missing in ["condition", "scope", "grounds"] {
        let mut record = requirement()["data"].clone();
        record["applicability"]["state"] = json!("conditional");
        record["applicability"][missing] = if missing == "grounds" {
            json!([])
        } else {
            json!("")
        };
        let record = Record {
            id: "conditional-record".into(),
            sources: vec![span()],
            data: serde_json::from_value(record).unwrap(),
        };
        assert!(
            tools::validate_record(&source, &analysis, &record).is_err(),
            "{missing}"
        );
    }
}

#[test]
fn put_record_assigns_stable_rule_item_ids() {
    let input = input();
    let mut analysis = read_analysis(&input);
    let mut coverage = analysis.coverage.clone();
    let args = json!({
        "id":null,
        "sources":[span()],
        "data":{
            "kind":"rule",
            "text":"投标函、授权书依次排列并分别签章",
            "scope":"本次投标",
            "applicability":{
                "state":"applicable",
                "condition":"按须知提交",
                "scope":"本次投标",
                "grounds":[span()]
            },
            "items":[
                {"id":"","kind":"composition","text":"投标函","condition":"","grounds":[span()],"targets":[]},
                {"id":"","kind":"composition","text":"授权书","condition":"","grounds":[span()],"targets":[]},
                {"id":"","kind":"order","text":"依次排列","condition":"","grounds":[span()],"targets":[],"sequence":["i1","i2"]}
            ]
        }
    });
    let schema = tools::schemas(false)
        .into_iter()
        .find(|t| t["function"]["name"] == "put_record")
        .unwrap();
    assert!(
        jsonschema::JSONSchema::compile(&schema["function"]["parameters"])
            .unwrap()
            .is_valid(&args)
    );
    let id = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "put_record",
        &args,
        16000,
    )
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let RecordData::Rule { items, .. } = &analysis.records[&id].data else {
        panic!("rule expected");
    };
    assert_eq!(items.len(), 3);
    assert_eq!(items[0].id, "i1");
    assert_eq!(items[1].id, "i2");
    assert_eq!(items[2].kind, RuleItemKind::Order);
    assert_eq!(items[2].sequence, ["i1", "i2"]);
}
