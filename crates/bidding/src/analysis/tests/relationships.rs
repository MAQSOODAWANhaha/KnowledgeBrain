use super::*;

#[test]
fn relation_validation_identifies_endpoint_and_ground_fields_without_partial_writes() {
    let (input, mut analysis, ids) = analysis_query_fixture();
    let mut args = json!(analysis.relations[&ids[3]]);
    args.as_object_mut().unwrap().remove("from_record_sha256");
    args.as_object_mut().unwrap().remove("to_record_sha256");
    let mut coverage = analysis.coverage.clone();
    for (path, replacement) in [
        ("/from", json!("missing")),
        ("/from_target", json!({"kind":"proof","index":0})),
        ("/to_target", json!({"kind":"response","index":0})),
        ("/scope", json!("")),
        ("/explanation", json!("")),
        (
            "/grounds/0",
            json!({"source_id":"source","start":1,"end":2}),
        ),
    ] {
        let mut invalid = args.clone();
        *invalid.pointer_mut(path).unwrap() = replacement;
        let before = (digest(&analysis).unwrap(), digest(&coverage).unwrap());
        let error = tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            "put_relation",
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
    tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "put_relation",
        &args,
        16000,
    )
    .unwrap();
}

#[test]
fn identical_new_relation_calls_reuse_identity_without_changing_the_analysis() {
    let (input, mut analysis, args) = field_relation_fixture();
    let mut coverage = analysis.coverage.clone();
    let first = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "put_relation",
        &args,
        16000,
    )
    .unwrap();
    let before = serde_json::to_value(&analysis).unwrap();
    let repeated = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "put_relation",
        &args,
        16000,
    )
    .unwrap();
    assert_eq!(repeated["id"], first["id"]);
    assert_eq!(repeated["unchanged"], true);
    assert_eq!(serde_json::to_value(&analysis).unwrap(), before);
    // Different claims are not semantically merged by a structural guard.
    let mut distinct = args.clone();
    distinct["scope"] = json!("Another source scope requiring independent comparison");
    let second = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "put_relation",
        &distinct,
        16000,
    )
    .unwrap();
    assert_ne!(second["id"], first["id"]);
    assert_eq!(analysis.relations.len(), 2);
    // An existing copy cannot bypass evidence admission.
    let relations_before = serde_json::to_value(&analysis.relations).unwrap();
    let mut unread = Coverage::default();
    assert!(
        tools::invoke(
            &input,
            &mut analysis,
            &mut unread,
            false,
            "put_relation",
            &args,
            16000
        )
        .is_err()
    );
    assert_eq!(
        serde_json::to_value(&analysis.relations).unwrap(),
        relations_before
    );
}

#[test]
fn field_relations_distinguish_cells_and_reject_whole_appendix_value_claims() {
    let (input, mut analysis, args) = field_relation_fixture();
    let mut coverage = analysis.coverage.clone();
    let id = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "put_relation",
        &args,
        16000,
    )
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let relation = &analysis.relations[&id];
    assert_eq!(
        relation.from_record_sha256,
        digest(&analysis.records["template-a"]).unwrap()
    );
    assert!(tools::gaps(&input, &analysis).is_empty());
    for change in [
        json!({"kind":"template_cell","form_id":"form-a","row":0,"column":1}), // covered part of merged cell, not anchor
        json!({"kind":"template_cell","form_id":"other-form","row":1,"column":1}),
        json!({"kind":"template_region","index":2}),
        json!({"kind":"proof","index":0}),
        json!({"kind":"record"}),
        args["from_target"].clone(),
    ] {
        let mut wrong = args.clone();
        wrong["to_target"] = change;
        assert!(
            tools::invoke(
                &input,
                &mut analysis,
                &mut coverage,
                false,
                "put_relation",
                &wrong,
                16000
            )
            .is_err()
        );
    }
    let mut unresolved = args.clone();
    unresolved["to_target"] = json!({"kind":"record"});
    unresolved["state"] = json!("unresolved");
    tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "put_relation",
        &unresolved,
        16000,
    )
    .unwrap();
}

#[test]
fn relation_prose_rejects_encoded_text_without_decoding_sources_or_literal_examples() {
    let (input, analysis, original) = field_relation_fixture();
    let source_before = digest(&input).unwrap();
    for field in ["scope", "explanation"] {
        let mut args = original.clone();
        args[field] = json!(r"1.4.1 \u8d44\u8d28\u8981\u6c42");
        let mut next = analysis.clone();
        let mut coverage = analysis.coverage.clone();
        let before = digest(&next).unwrap();
        let error = tools::invoke(
            &input,
            &mut next,
            &mut coverage,
            false,
            "put_relation",
            &args,
            16000,
        )
        .unwrap_err();
        assert!(
            error.contains(&format!("INVALID_FIELD /{field}")),
            "{error}"
        );
        assert_eq!(
            digest(&next).unwrap(),
            before,
            "bad prose cannot partially write a relation"
        );
    }
    for prose in [
        "原文条件对应证明位置。",
        r"路径 C:\users\招标\材料",
        r"匹配正则 \u[0-9a-f]{4}",
        r"保留字面示例 \u8d44\u8d28",
        r"literal example: \u8d44\u8d28",
    ] {
        let mut args = original.clone();
        args["explanation"] = json!(prose);
        let mut next = analysis.clone();
        let mut coverage = analysis.coverage.clone();
        let out = tools::invoke(
            &input,
            &mut next,
            &mut coverage,
            false,
            "put_relation",
            &args,
            16000,
        )
        .unwrap();
        assert_eq!(
            next.relations[out["id"].as_str().unwrap()].explanation,
            prose
        );
    }
    let decoded: String = serde_json::from_str(r#""\u8d44\u8d28\u8981\u6c42""#).unwrap();
    let mut args = original;
    args["explanation"] = json!(decoded);
    let mut next = analysis.clone();
    let mut coverage = analysis.coverage.clone();
    assert!(
        tools::invoke(
            &input,
            &mut next,
            &mut coverage,
            false,
            "put_relation",
            &args,
            16000
        )
        .is_ok()
    );
    assert_eq!(
        digest(&input).unwrap(),
        source_before,
        "source bytes are never rewritten"
    );
}

#[test]
fn identical_appendix_labels_do_not_authorize_foreign_template_cells() {
    let (mut input, mut analysis, mut args) = field_relation_fixture();
    let mut other_source = input.source_units[0].clone();
    other_source.source_unit_revision_id = "other-source".into();
    other_source.document_id = "other-document".into();
    input.source_units.push(other_source);
    let mut form = input.structured_forms[0].clone();
    form["form_definition_revision_id"] = json!("other-form");
    form["source_unit_revision_id"] = json!("other-source");
    input.structured_forms.push(form);
    let mut other = analysis.records["template-a"].clone();
    other.id = "template-b".into();
    other.sources[0].source_id = "other-source".into();
    if let RecordData::Template { regions, .. } = &mut other.data {
        for region in regions {
            region.source.source_id = "other-source".into();
            region.form_id = Some("other-form".into());
        }
    }
    analysis.records.insert(other.id.clone(), other);
    let mut coverage = analysis.coverage.clone();
    args["to"] = json!("template-b");
    assert!(
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            "put_relation",
            &args,
            16000
        )
        .is_err()
    );
    args["to_target"]["form_id"] = json!("other-form");
    tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "put_relation",
        &args,
        16000,
    )
    .unwrap();
    // The grid exists, but a different region of the same template owns this cell.
    if let RecordData::Template { regions, .. } =
        &mut analysis.records.get_mut("template-b").unwrap().data
    {
        regions.pop();
    }
    assert!(
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            "put_relation",
            &args,
            16000
        )
        .is_err()
    );
}

#[test]
fn endpoint_edits_invalidate_relations_and_independent_review_until_rebound() {
    let (input, mut analysis, mut args) = field_relation_fixture();
    let mut coverage = analysis.coverage.clone();
    let id = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "put_relation",
        &args,
        16000,
    )
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut review = coverage.clone();
    for kind in ["all", "relation", "disposition"] {
        tools::invoke(
            &input,
            &mut analysis,
            &mut review,
            true,
            "inspect_analysis",
            &json!({"view":"detail","kind":kind,"offset":0,"limit":100}),
            16000,
        )
        .unwrap();
    }
    assert!(tools::review_gaps(&input, &analysis, &review).is_empty());
    let mut update = serde_json::to_value(&analysis.records["template-a"]).unwrap();
    update["data"]["regions"].as_array_mut().unwrap().reverse();
    let changed = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "put_record",
        &update,
        16000,
    )
    .unwrap();
    assert_eq!(changed["relations_to_recheck"], json!([id]));
    assert!(
        tools::gaps(&input, &analysis)
            .iter()
            .any(|gap| gap["kind"] == "invalid_relation")
    );
    args["id"] = json!(id);
    tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "put_relation",
        &args,
        16000,
    )
    .unwrap();
    assert!(tools::gaps(&input, &analysis).is_empty());
    assert!(
        tools::review_gaps(&input, &analysis, &review)
            .iter()
            .any(|gap| gap["key"] == format!("relation:{id}"))
    );
    // A persisted checkpoint retains endpoint digests and does not make stale review valid.
    let restored: Analysis =
        serde_json::from_value(serde_json::to_value(&analysis).unwrap()).unwrap();
    assert!(tools::gaps(&input, &restored).is_empty());
    assert!(!tools::review_gaps(&input, &restored, &review).is_empty());
    let mut forged = args;
    forged["from_record_sha256"] = json!("forged");
    assert!(
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            "put_relation",
            &forged,
            16000
        )
        .is_err()
    );
}

#[test]
fn relation_schema_and_runtime_share_exact_endpoint_variants() {
    let (_, _, mut args) = field_relation_fixture();
    let schema = tools::schemas(false)
        .into_iter()
        .find(|v| v["function"]["name"] == "put_relation")
        .unwrap();
    let compiled = jsonschema::JSONSchema::compile(&schema["function"]["parameters"]).unwrap();
    for target in [
        json!({"kind":"record"}),
        json!({"kind":"template_region","index":0}),
        json!({"kind":"template_cell","form_id":"f","row":0,"column":0}),
        json!({"kind":"response","index":0}),
        json!({"kind":"proof","index":0}),
        json!({"kind":"criterion","index":0}),
    ] {
        args["from_target"] = target.clone();
        assert!(compiled.is_valid(&args));
        serde_json::from_value::<RelationTarget>(target).unwrap();
    }
    args["from_target"] =
        json!({"kind":"template_cell","form_id":"f","row":0,"column":0,"index":0});
    assert!(!compiled.is_valid(&args));
    assert!(serde_json::from_value::<RelationTarget>(args["from_target"].clone()).is_err());
}

#[test]
fn field_aliases_and_multi_cell_regions_cannot_fake_value_relationships() {
    let (input, mut analysis, mut args) = field_relation_fixture();
    let mut coverage = analysis.coverage.clone();
    args["to_target"] = json!({"kind":"template_region","index":1});
    assert!(
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            "put_relation",
            &args,
            16000
        )
        .is_err()
    );
    args["from_target"] = json!({"kind":"template_cell","form_id":"form-a","row":0,"column":0});
    args["to_target"] = json!({"kind":"template_region","index":0});
    assert!(
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            "put_relation",
            &args,
            16000
        )
        .is_err()
    );
}

#[test]
fn requirement_targets_keep_response_proof_and_criterion_indices_separate() {
    let input = input();
    let mut raw = requirement();
    raw["id"] = json!("r");
    raw["data"]["proofs"] = json!([{"description":"检测报告","subject":"产品","validity":"有效期内","issuer":"检测机构","condition":"原文条件","grounds":[span()],"name_required":true,"page_required":true}]);
    raw["data"]["criteria"] = json!([{"subject":"产品","aspect":"性能","operator":"≥","value":"20","unit":"Gbps","condition":"原文条件","grounds":[span()]}]);
    let record: Record = serde_json::from_value(raw).unwrap();
    for target in [
        RelationTarget::Response { index: 0 },
        RelationTarget::Proof { index: 0 },
        RelationTarget::Criterion { index: 0 },
    ] {
        relations::validate_target(&input, &record, &target).unwrap();
    }
    for target in [
        RelationTarget::Response { index: 1 },
        RelationTarget::Proof { index: 1 },
        RelationTarget::Criterion { index: 1 },
        RelationTarget::TemplateRegion { index: 0 },
    ] {
        assert!(relations::validate_target(&input, &record, &target).is_err());
    }
}
