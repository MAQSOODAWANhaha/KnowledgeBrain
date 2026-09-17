use super::*;
use crate::tender_analysis::rule_contract::{RuleItem, RuleItemKind, RuleItemTarget};

fn target_fixture() -> (FrozenInput, Checkpoint, Judgment, Dependencies) {
    let (input, config, mut state) = fixture();
    let source = citation(&input);
    for record in [
        Record {
            id: "target".into(),
            sources: vec![source.clone()],
            data: RecordData::Fact {
                name: "Prescribed output".into(),
                value: "Source wording".into(),
                scope: "project".into(),
            },
        },
        Record {
            id: "rule".into(),
            sources: vec![source.clone()],
            data: RecordData::Rule {
                text: "Follow the referenced composition".into(),
                scope: "project".into(),
                applicability: Applicability {
                    state: ApplicabilityState::Applicable,
                    condition: String::new(),
                    scope: "project".into(),
                    grounds: vec![source.clone()],
                },
                items: vec![RuleItem {
                    id: "i1".into(),
                    kind: RuleItemKind::Composition,
                    text: "Referenced output".into(),
                    grounds: vec![source.clone()],
                    condition: String::new(),
                    targets: vec![RuleItemTarget::Record {
                        id: "target".into(),
                    }],
                    sequence: vec![],
                    format_key: None,
                    format_value: None,
                }],
            },
        },
    ] {
        state.reviewer_coverage.candidate.insert(
            format!("record:{}", record.id),
            digest(&json!(record)).unwrap(),
        );
        state.analysis.records.insert(record.id.clone(), record);
    }
    let mut check: Judgment = serde_json::from_value(judgment(&input, &config, &state)).unwrap();
    check.relationship_checks = vec![RelationshipCheck {
        record_id: "rule".into(),
        status: RelationshipStatus::Resolved,
        related_record_ids: vec!["target".into()],
        relation_ids: vec![],
        unresolved_record_ids: vec![],
        finding_ids: vec![],
        reason: "The saved typed target matches the independently read original.".into(),
        sources: vec![source],
    }];
    let deps = state.source_review.as_ref().unwrap().dependencies.clone();
    (input, state, check, deps)
}

#[test]
fn typed_rule_target_resolves_without_a_duplicate_reference_edge() {
    let (input, state, check, mut deps) = target_fixture();
    validate_relationship_checks(&input, &state, &check, &mut deps).unwrap();
    assert!(state.analysis.relations.is_empty());
    assert!(deps.references.contains("record:target"));
}

#[test]
fn typed_target_requires_the_actual_current_independently_read_endpoint() {
    let (input, mut state, mut check, deps) = target_fixture();
    check.relationship_checks[0].related_record_ids.clear();
    assert!(validate_relationship_checks(&input, &state, &check, &mut deps.clone()).is_err());
    check.relationship_checks[0].related_record_ids = vec!["target".into()];
    state.reviewer_coverage.candidate.remove("record:target");
    assert!(
        validate_relationship_checks(&input, &state, &check, &mut deps.clone())
            .unwrap_err()
            .contains("independently inspect")
    );
    state.reviewer_coverage.candidate.insert(
        "record:target".into(),
        digest(&json!(state.analysis.records["target"])).unwrap(),
    );
    if let RecordData::Fact { value, .. } =
        &mut state.analysis.records.get_mut("target").unwrap().data
    {
        *value = "Changed target".into();
    }
    assert!(
        validate_relationship_checks(&input, &state, &check, &mut deps.clone())
            .unwrap_err()
            .contains("independently inspect")
    );
}

#[test]
fn typed_target_cannot_be_dismissed_as_not_required_or_resolve_another_record() {
    let (input, mut state, mut check, deps) = target_fixture();
    check.relationship_checks[0].status = RelationshipStatus::NotRequired;
    check.relationship_checks[0].related_record_ids.clear();
    assert!(validate_relationship_checks(&input, &state, &check, &mut deps.clone()).is_err());
    let mut other = state.analysis.records["target"].clone();
    other.id = "other".into();
    state
        .reviewer_coverage
        .candidate
        .insert("record:other".into(), digest(&json!(other)).unwrap());
    state.analysis.records.insert("other".into(), other);
    check.relationship_checks[0].status = RelationshipStatus::Resolved;
    check.relationship_checks[0].related_record_ids = vec!["target".into(), "other".into()];
    assert!(validate_relationship_checks(&input, &state, &check, &mut deps.clone()).is_err());
}

#[test]
fn typed_target_is_a_dependency_even_when_its_original_is_outside_the_task() {
    let (_, mut state, _, _) = target_fixture();
    state.analysis.records.get_mut("target").unwrap().sources[0].source_id =
        "another-source".into();
    let deps = Dependencies {
        references: BTreeSet::from(["record:rule".into()]),
        ..Default::default()
    };
    assert!(dependency_references(&state.analysis, &deps).contains("record:target"));
    let before = version(&state, &deps).unwrap();
    if let RecordData::Fact { value, .. } =
        &mut state.analysis.records.get_mut("target").unwrap().data
    {
        *value = "Revised source-backed target".into();
    }
    assert_ne!(before, version(&state, &deps).unwrap());
    state.analysis.records.remove("target");
    assert!(
        dependency_references(&state.analysis, &deps).contains("record:target"),
        "deleted targets must remain a version dependency"
    );
}
