use super::*;

fn parent_fixture() -> (FrozenInput, Config, Checkpoint, Judgment, Dependencies) {
    let (input, config, mut state) = fixture();
    for (id, parent) in [("parent", None), ("child", Some("parent"))] {
        state.analysis.records.insert(
            id.into(),
            Record {
                id: id.into(),
                sources: vec![citation(&input)],
                data: RecordData::Template {
                    label: id.into(),
                    title: id.into(),
                    parent: parent.map(str::to_owned),
                    order: None,
                    purpose: "Prescribed format".into(),
                    applicability: Applicability {
                        state: ApplicabilityState::Applicable,
                        condition: String::new(),
                        scope: "project".into(),
                        grounds: vec![citation(&input)],
                    },
                    regions: vec![],
                },
            },
        );
        let key = format!("record:{id}");
        state.reviewer_coverage.candidate.insert(
            key.clone(),
            digest(&context::reference(&state.analysis, &key).unwrap()).unwrap(),
        );
    }
    let mut check: Judgment = serde_json::from_value(judgment(&input, &config, &state)).unwrap();
    check.relationship_checks = vec![RelationshipCheck {
        record_id: "child".into(),
        status: RelationshipStatus::Resolved,
        related_record_ids: vec!["parent".into()],
        relation_ids: vec![],
        unresolved_record_ids: vec![],
        finding_ids: vec![],
        reason: "The original makes this format a child of the listed parent.".into(),
        sources: vec![citation(&input)],
    }];
    let deps = state.source_review.as_ref().unwrap().dependencies.clone();
    (input, config, state, check, deps)
}

#[test]
fn applicable_template_parent_requires_explicit_relationship_judgment() {
    let (input, config, state, mut check, mut deps) = parent_fixture();
    let navigation = packet(&input, &config, &state).unwrap();
    assert_eq!(
        navigation["current"]["records_requiring_relationship_judgment"]["items"],
        json!(["child"])
    );
    assert_eq!(
        relationship_records(&state.analysis, &references(&state.analysis, &deps)),
        BTreeSet::from(["child".into()])
    );
    check.relationship_checks.clear();
    let error = validate_relationship_checks(&input, &state, &check, &mut deps).unwrap_err();
    assert!(
        error.contains("/relationship_checks") && error.contains("record:child"),
        "{error}"
    );
}

#[test]
fn template_parent_resolution_uses_existing_parent_without_duplicate_edge() {
    let (input, _, state, check, mut deps) = parent_fixture();
    validate_relationship_checks(&input, &state, &check, &mut deps).unwrap();
    assert!(deps.references.contains("record:parent"));
    assert!(state.analysis.relations.is_empty());
}

#[test]
fn template_parent_resolution_requires_actual_current_independently_read_parent() {
    let (input, _, mut state, mut check, deps) = parent_fixture();
    check.relationship_checks[0].related_record_ids.clear();
    assert!(validate_relationship_checks(&input, &state, &check, &mut deps.clone()).is_err());
    check.relationship_checks[0].related_record_ids = vec!["parent".into()];
    state.reviewer_coverage.candidate.remove("record:parent");
    assert!(
        validate_relationship_checks(&input, &state, &check, &mut deps.clone())
            .unwrap_err()
            .contains("independently inspect")
    );
    state.reviewer_coverage.candidate.insert(
        "record:parent".into(),
        digest(&json!(state.analysis.records["parent"])).unwrap(),
    );
    if let RecordData::Template { title, .. } =
        &mut state.analysis.records.get_mut("parent").unwrap().data
    {
        *title = "Changed parent".into();
    }
    assert!(
        validate_relationship_checks(&input, &state, &check, &mut deps.clone())
            .unwrap_err()
            .contains("independently inspect")
    );
    state.analysis.records.remove("parent");
    assert!(validate_relationship_checks(&input, &state, &check, &mut deps.clone()).is_err());
}

#[test]
fn template_parent_cannot_be_dismissed_as_not_required() {
    let (input, _, state, mut check, mut deps) = parent_fixture();
    check.relationship_checks[0].status = RelationshipStatus::NotRequired;
    check.relationship_checks[0].related_record_ids.clear();
    assert!(
        validate_relationship_checks(&input, &state, &check, &mut deps)
            .unwrap_err()
            .contains("not_required")
    );
}

#[test]
fn template_parent_does_not_resolve_unrelated_targets() {
    let (input, _, mut state, mut check, mut deps) = parent_fixture();
    let mut other = state.analysis.records["parent"].clone();
    other.id = "other".into();
    state
        .reviewer_coverage
        .candidate
        .insert("record:other".into(), digest(&json!(other)).unwrap());
    state.analysis.records.insert("other".into(), other);
    check.relationship_checks[0]
        .related_record_ids
        .push("other".into());
    assert!(
        validate_relationship_checks(&input, &state, &check, &mut deps)
            .unwrap_err()
            .contains("resolved requires")
    );
}

#[test]
fn template_parent_can_retain_genuine_source_uncertainty() {
    let (input, _, mut state, mut check, mut deps) = parent_fixture();
    if let RecordData::Template { applicability, .. } =
        &mut state.analysis.records.get_mut("child").unwrap().data
    {
        applicability.state = ApplicabilityState::Unknown;
        applicability.condition = "Source leaves the applicable branch unspecified.".into();
    }
    state.reviewer_coverage.candidate.insert(
        "record:child".into(),
        digest(&json!(state.analysis.records["child"])).unwrap(),
    );
    check.relationship_checks[0].status = RelationshipStatus::SourceLimited;
    validate_relationship_checks(&input, &state, &check, &mut deps).unwrap();
}

#[test]
fn applicable_template_can_retain_saved_parent_ambiguity() {
    let (input, _, mut state, mut check, mut deps) = parent_fixture();
    let unresolved = Record {
        id: "uncertain".into(),
        sources: vec![citation(&input)],
        data: RecordData::Unresolved {
            problem: "The source prescribes this format but leaves its parent ambiguous.".into(),
            affected: vec!["child".into()],
            candidates: vec!["source".into()],
        },
    };
    state.reviewer_coverage.candidate.insert(
        "record:uncertain".into(),
        digest(&json!(unresolved)).unwrap(),
    );
    state
        .analysis
        .records
        .insert("uncertain".into(), unresolved);
    check.relationship_checks[0].status = RelationshipStatus::SourceLimited;
    check.relationship_checks[0].unresolved_record_ids = vec!["uncertain".into()];
    let mut uncertainty = check.relationship_checks[0].clone();
    uncertainty.record_id = "uncertain".into();
    check.relationship_checks.push(uncertainty);
    validate_relationship_checks(&input, &state, &check, &mut deps).unwrap();
}

#[test]
fn template_parent_resolution_requires_original_parent_evidence() {
    let (mut input, _, mut state, mut check, mut deps) = parent_fixture();
    input.source_units.push(Source {
        source_unit_revision_id: "parent-source".into(),
        document_id: "document".into(),
        text: "Composition and prescribed child format.".into(),
        locator: json!({}),
        ordinal: 1,
    });
    let parent_source = Span {
        source_id: "parent-source".into(),
        start: 0,
        end: input.source_units[1].text.len(),
        view_id: None,
        grid_cell: None,
    };
    state.analysis.records.get_mut("parent").unwrap().sources = vec![parent_source.clone()];
    state.reviewer_coverage.candidate.insert(
        "record:parent".into(),
        digest(&json!(state.analysis.records["parent"])).unwrap(),
    );
    assert!(
        validate_relationship_checks(&input, &state, &check, &mut deps)
            .unwrap_err()
            .contains("original evidence for the selected template parent")
    );
    check.relationship_checks[0]
        .sources
        .push(parent_source.clone());
    assert!(validate_relationship_checks(&input, &state, &check, &mut deps).is_err());
    tools::cover(
        state
            .reviewer_coverage
            .text
            .entry("parent-source".into())
            .or_default(),
        0,
        parent_source.end,
    );
    validate_relationship_checks(&input, &state, &check, &mut deps).unwrap();
}

#[test]
fn template_parent_misassignment_can_finish_with_a_field_finding() {
    let (input, _, mut state, mut check, mut deps) = parent_fixture();
    state.review_draft.insert(
        "wrong-parent".into(),
        Finding {
            code: "wrong_parent".into(),
            message: "The current parent conflicts with the original composition.".into(),
            correction: "Restore the source-prescribed hierarchy.".into(),
            affected: vec![ReviewedField {
                id: "child".into(),
                path: "/data/parent".into(),
            }],
            sources: vec![citation(&input)],
        },
    );
    check.status = JudgmentStatus::Findings;
    check.finding_ids = vec!["wrong-parent".into()];
    check.relationship_checks[0].status = RelationshipStatus::Findings;
    check.relationship_checks[0].finding_ids = vec!["wrong-parent".into()];
    validate_relationship_checks(&input, &state, &check, &mut deps).unwrap();
}
