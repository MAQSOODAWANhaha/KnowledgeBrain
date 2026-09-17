use super::*;

#[test]
fn collection_queries_in_active_work_track_global_negative_dependencies() {
    let (input, _, mut state) = fixture();
    assert!(state.reviewer_work.is_some());
    let query =
        json!({"kind":"unresolved","scope":"collection","view":"index","offset":0,"limit":10});
    let coverage_before = digest(&state.reviewer_coverage).unwrap();
    let page = tools::inspect_analysis(
        &input,
        &state.analysis,
        &mut state.reviewer_coverage.clone(),
        &state.reviewer_coverage,
        &query,
        8192,
        Some(&state.reviewer_work.as_ref().unwrap().source_scope),
    )
    .unwrap();
    assert_eq!(page["total"], 0);
    record_query(&input, &mut state, "inspect_analysis", &query);
    let dependencies = state.source_review.as_ref().unwrap().dependencies.clone();
    assert!(dependencies.global);
    let before = version(&state, &dependencies).unwrap();
    state.analysis.records.insert(
        "elsewhere".into(),
        Record {
            id: "elsewhere".into(),
            sources: vec![Span {
                source_id: "other".into(),
                ..citation(&input)
            }],
            data: RecordData::Fact {
                name: "new outcome".into(),
                value: "added".into(),
                scope: "project".into(),
            },
        },
    );
    assert_ne!(version(&state, &dependencies).unwrap(), before);
    assert_eq!(digest(&state.reviewer_coverage).unwrap(), coverage_before);
}

#[test]
fn explicit_filters_keep_collection_query_dependencies_local() {
    for filter in [
        json!({"source_id":"source"}),
        json!({"ids":["source"],"kind":"disposition"}),
    ] {
        let (input, _, mut state) = fixture();
        let mut query = json!({"kind":"all","scope":"collection","offset":0,"limit":10});
        query
            .as_object_mut()
            .unwrap()
            .extend(filter.as_object().unwrap().clone());
        record_query(&input, &mut state, "inspect_analysis", &query);
        assert!(!state.source_review.as_ref().unwrap().dependencies.global);
    }
}

#[test]
fn put_analysis_check_stores_role_local_pass() {
    let (input, config, mut state) = fixture();
    state.role = Role::Reviewer;
    let err = apply(
        &input,
        &config,
        &mut state,
        "put_analysis_check",
        &json!({"key":"nope","conclusion":"pass","grounds":[]}),
    )
    .unwrap_err();
    assert!(err.contains("unknown global check"), "{err}");
    let args = check_args(&input, &state, "source_coverage");
    apply(&input, &config, &mut state, "put_analysis_check", &args).unwrap();
    let check = &state.analysis.review_global_checks["source_coverage"];
    assert_eq!(check.conclusion, GlobalCheckConclusion::Pass);
    assert_eq!(
        check.scope_sha256,
        crate::tender_analysis::rule_contract::scope_sha256(&input, &state.analysis).unwrap()
    );
    assert!(
        !state
            .analysis
            .main_global_checks
            .contains_key("source_coverage")
    );
}

fn check_args(input: &FrozenInput, state: &Checkpoint, key: &str) -> Value {
    json!({"key":key,"expected_scope_sha256":crate::tender_analysis::rule_contract::scope_sha256(input,&state.analysis).unwrap(),
        "conclusion":"pass","grounds":[citation(input)],"record_ids":[],"finding_ids":[]})
}

#[test]
fn global_check_rejects_an_unread_original() {
    let (input, config, mut state) = fixture();
    let args = check_args(&input, &state, "source_coverage");
    state.reviewer_coverage.text.clear();
    let error = apply(&input, &config, &mut state, "put_analysis_check", &args).unwrap_err();
    assert!(error.contains("read"), "{error}");
}

#[test]
fn global_check_rejects_a_stale_scope_instead_of_signing_current_graph() {
    let (input, config, mut state) = fixture();
    let args = check_args(&input, &state, "collection_consistency");
    state
        .analysis
        .dispositions
        .get_mut("source")
        .unwrap()
        .reason
        .push_str(" changed");
    let error = apply(&input, &config, &mut state, "put_analysis_check", &args).unwrap_err();
    assert!(error.contains("scope"), "{error}");
}

#[test]
fn global_check_rejects_unknown_findings_and_empty_source_limitation() {
    let (input, config, mut state) = fixture();
    let mut args = check_args(&input, &state, "cross_references");
    args["conclusion"] = json!("findings");
    args["finding_ids"] = json!(["invented"]);
    assert!(apply(&input, &config, &mut state, "put_analysis_check", &args).is_err());
    args["conclusion"] = json!("source_limited");
    args["finding_ids"] = json!([]);
    let error = apply(&input, &config, &mut state, "put_analysis_check", &args).unwrap_err();
    assert!(
        error.contains("source") || error.contains("unresolved"),
        "{error}"
    );
}

fn uncertain_record(input: &FrozenInput) -> Record {
    Record {
        id: "uncertain".into(),
        sources: vec![citation(input)],
        data: RecordData::Unresolved {
            problem: "The original does not determine the applicable option.".into(),
            affected: vec![],
            candidates: vec![],
        },
    }
}

#[test]
fn global_check_requires_the_current_candidate_delivery_and_preserves_real_uncertainty() {
    let (input, config, mut state) = fixture();
    let record = uncertain_record(&input);
    state
        .analysis
        .records
        .insert(record.id.clone(), record.clone());
    let mut args = check_args(&input, &state, "cross_references");
    args["conclusion"] = json!("source_limited");
    args["record_ids"] = json!([record.id]);
    let error = apply(&input, &config, &mut state, "put_analysis_check", &args).unwrap_err();
    assert!(
        error.contains("inspect") || error.contains("delivered"),
        "{error}"
    );
    state
        .reviewer_coverage
        .candidate
        .insert("record:uncertain".into(), digest(&record).unwrap());
    apply(&input, &config, &mut state, "put_analysis_check", &args).unwrap();
    assert_eq!(
        state.analysis.review_global_checks["cross_references"].conclusion,
        GlobalCheckConclusion::SourceLimited
    );
    state
        .analysis
        .records
        .get_mut("uncertain")
        .unwrap()
        .sources
        .clear();
    args["expected_scope_sha256"] = json!(
        crate::tender_analysis::rule_contract::scope_sha256(&input, &state.analysis).unwrap()
    );
    assert!(apply(&input, &config, &mut state, "put_analysis_check", &args).is_err());
}

#[test]
fn completed_source_review_does_not_bypass_missing_global_inventory() {
    let (input, config, mut state) = fixture();
    let args = judgment(&input, &config, &state);
    put(&input, &config, &mut state, &args).unwrap();
    assert!(!review_complete(&input, &config, &state).unwrap());
    for key in ANALYSIS_GLOBAL_CHECK_KEYS {
        let args = check_args(&input, &state, key);
        apply(&input, &config, &mut state, "put_analysis_check", &args).unwrap();
    }
    assert!(review_complete(&input, &config, &state).unwrap());
}

#[test]
fn global_inventory_rejects_unknown_finding_even_with_all_five_keys() {
    let (input, _, state) = fixture();
    let checks: Vec<_> = ANALYSIS_GLOBAL_CHECK_KEYS
        .iter()
        .map(|key| GlobalCheck {
            key: (*key).into(),
            scope_sha256: crate::tender_analysis::rule_contract::scope_sha256(
                &input,
                &state.analysis,
            )
            .unwrap(),
            conclusion: GlobalCheckConclusion::Findings,
            grounds: vec![citation(&input)],
            record_ids: vec![],
            finding_ids: vec!["made-up".into()],
        })
        .collect();
    assert!(
        crate::tender_analysis::rule_contract::validate_inventory(
            &input,
            &state.analysis,
            &checks,
            &[]
        )
        .is_err()
    );
}

#[test]
fn global_findings_freeze_real_finding_content_and_invalidate_after_edit() {
    let (input, config, mut state) = fixture();
    let finding = Finding {
        code: "global_issue".into(),
        message: "A source-backed inconsistency needs correction.".into(),
        correction: "Compare the retained source statement.".into(),
        affected: vec![],
        sources: vec![citation(&input)],
    };
    let saved = apply(
        &input,
        &config,
        &mut state,
        "put_review_finding",
        &json!({"id":null,"finding":finding}),
    )
    .unwrap();
    let id = saved["id"].as_str().unwrap();
    let mut args = check_args(&input, &state, "collection_consistency");
    args["conclusion"] = json!("findings");
    args["finding_ids"] = json!([id]);
    apply(&input, &config, &mut state, "put_analysis_check", &args).unwrap();
    let check = &state.analysis.review_global_checks["collection_consistency"];
    assert_eq!(check.finding_ids, vec![digest(&finding).unwrap()]);
    let mut changed = finding.clone();
    changed.correction.push_str(" Recheck the revision.");
    assert!(rule_contract::validate_check(&input, &state.analysis, check, &[changed]).is_err());
}

#[test]
fn frozen_missing_document_has_a_metadata_grounded_source_limited_path() {
    let (mut input, config, mut state) = fixture();
    input.documents.push(json!({"document_id":"missing","disposition":"unresolved","reason":"Original pages unavailable"}));
    let mut args = check_args(&input, &state, "collection_consistency");
    args["grounds"] = json!([]);
    args["conclusion"] = json!("source_limited");
    assert!(apply(&input, &config, &mut state, "put_analysis_check", &args).is_err());
    state
        .reviewer_coverage
        .metadata
        .insert("documents".into(), vec![(0, 1)]);
    apply(&input, &config, &mut state, "put_analysis_check", &args).unwrap();
}

#[test]
fn published_global_review_rejects_wrong_contract_duplicate_key_and_changed_graph() {
    let (input, config, mut state) = fixture();
    for key in ANALYSIS_GLOBAL_CHECK_KEYS {
        let args = check_args(&input, &state, key);
        apply(&input, &config, &mut state, "put_analysis_check", &args).unwrap();
    }
    let mut review = Review {
        analysis_sha256: digest(&state.analysis).unwrap(),
        coverage: state.reviewer_coverage.clone(),
        findings: vec![],
        contract_sha256: rule_contract::contract_sha256().unwrap(),
        global_checks: state
            .analysis
            .review_global_checks
            .values()
            .cloned()
            .collect(),
        omitted_sources: BTreeMap::new(),
        draft: false,
    };
    rule_contract::validate_review(&input, &state.analysis, &review).unwrap();
    review.contract_sha256.clear();
    assert!(rule_contract::validate_review(&input, &state.analysis, &review).is_err());
    review.contract_sha256 = rule_contract::contract_sha256().unwrap();
    let check = review.global_checks.pop().unwrap();
    review.global_checks.push(review.global_checks[0].clone());
    assert!(rule_contract::validate_review(&input, &state.analysis, &review).is_err());
    review.global_checks.pop();
    review.global_checks.push(check);
    state
        .analysis
        .dispositions
        .get_mut("source")
        .unwrap()
        .reason
        .push_str(" changed");
    review.analysis_sha256 = digest(&state.analysis).unwrap();
    assert!(rule_contract::validate_review(&input, &state.analysis, &review).is_err());
}

#[test]
fn v2_publication_rejects_wrong_contract_and_forged_post_review_grounds() {
    let (input, config, mut state) = fixture();
    for key in ANALYSIS_GLOBAL_CHECK_KEYS {
        let args = check_args(&input, &state, key);
        apply(&input, &config, &mut state, "put_analysis_check", &args).unwrap();
    }
    let review = Review {
        analysis_sha256: digest(&state.analysis).unwrap(),
        coverage: state.reviewer_coverage.clone(),
        findings: vec![],
        contract_sha256: rule_contract::contract_sha256().unwrap(),
        global_checks: state
            .analysis
            .review_global_checks
            .values()
            .cloned()
            .collect(),
        omitted_sources: BTreeMap::new(),
        draft: false,
    };
    let mut result = AnalysisResult {
        schema_version: 2,
        frozen_input_sha256: digest(&input).unwrap(),
        analysis: state.analysis,
        review,
        quality: "verified".into(),
        source_views: BTreeMap::new(),
    };
    crate::tender_analysis::postgres::publication(&input, &result).unwrap();
    result.review.contract_sha256.clear();
    assert!(crate::tender_analysis::postgres::publication(&input, &result).is_err());
    result.review.contract_sha256 = rule_contract::contract_sha256().unwrap();
    result.review.global_checks[0].grounds[0].end += 3;
    let forged = result.review.global_checks[0].clone();
    result
        .analysis
        .review_global_checks
        .insert(forged.key.clone(), forged);
    result.review.analysis_sha256 = digest(&result.analysis).unwrap();
    assert!(crate::tender_analysis::postgres::publication(&input, &result).is_err());
}
