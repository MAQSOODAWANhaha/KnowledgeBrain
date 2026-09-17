use super::*;

fn order_fixture() -> (FrozenInput, AnalysisResult, Workspace) {
    order_fixture_with_targets(true)
}

fn order_fixture_with_targets(with_targets: bool) -> (FrozenInput, AnalysisResult, Workspace) {
    let (input, mut result) = rule_fixture("composition", None, None);
    let RecordData::Rule { items, .. } = &mut result.analysis.records.get_mut("rule").unwrap().data
    else {
        unreachable!()
    };
    let mut first = items[0].clone();
    first.id = "first".into();
    let mut second = first.clone();
    second.id = "second".into();
    second.targets = vec![analysis::RuleItemTarget::Record { id: "t1".into() }];
    items[0].kind = analysis::RuleItemKind::Order;
    items[0].sequence = vec![first.id.clone(), second.id.clone()];
    items[0].targets.clear();
    items.extend([first, second]);
    if !with_targets {
        for item in items.iter_mut() {
            item.targets.clear();
        }
    }
    refresh_excerpt_basis(&input, &mut result);
    let mut w = ready(&input, &result);
    for (section, ids) in [
        ("fixture-section-0", vec!["first", "rule-item"]),
        ("fixture-section-1", vec!["second"]),
    ] {
        for id in ids {
            w.draft.plan.get_mut(section).unwrap().obligation_refs.push(
                reference_key(&Reference {
                    record_id: "rule".into(),
                    target: RelationTarget::RuleItem { item_id: id.into() },
                })
                .unwrap(),
            );
        }
    }
    (input, result, w)
}

#[test]
fn rule_sequence_is_checked_against_rendered_section_order() {
    let (input, result, mut w) = order_fixture();
    compiler::compile(&input, &result, &w.draft, 1_000_000).unwrap();
    for (id, order) in [("fixture-section-0", 1), ("fixture-section-1", 0)] {
        w.draft.sections.get_mut(id).unwrap().order = order;
        w.draft.plan.get_mut(id).unwrap().order = order;
    }
    let error = compiler::compile(&input, &result, &w.draft, 1_000_000)
        .err()
        .expect("reversed source sequence must fail");
    assert!(error.contains("rule sequence"), "{error}");
}

#[test]
fn rule_sequence_requires_distinct_actual_locations_and_records_its_dependencies() {
    let (input, result, w) = order_fixture();
    let compiled = compiler::compile(&input, &result, &w.draft, 1_000_000).unwrap();
    let order = compiled
        .manifest
        .rule_implementations
        .iter()
        .find(|i| i.reference == rule_reference())
        .unwrap();
    let compiler::RuleImplementationTarget::Section { dependencies, .. } = &order.implementation
    else {
        unreachable!()
    };
    let sections: Vec<_> = dependencies
        .iter()
        .map(|d| d.location.section_id.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    assert_eq!(sections, ["fixture-section-0", "fixture-section-1"]);
    assert!(dependencies.iter().all(
        |d| matches!(&d.reference.target, RelationTarget::RuleItem { item_id }
        if (item_id == "first" && d.location.section_id == "fixture-section-0")
            || (item_id == "second" && d.location.section_id == "fixture-section-1"))
    ));

    let (input, result, mut w) = order_fixture_with_targets(false);
    let second = reference_key(&Reference {
        record_id: "rule".into(),
        target: RelationTarget::RuleItem {
            item_id: "second".into(),
        },
    })
    .unwrap();
    w.draft
        .plan
        .get_mut("fixture-section-1")
        .unwrap()
        .obligation_refs
        .retain(|key| key != &second);
    w.draft
        .plan
        .get_mut("fixture-section-0")
        .unwrap()
        .obligation_refs
        .push(second);
    let error = compiler::compile(&input, &result, &w.draft, 1_000_000)
        .err()
        .expect("same heading cannot prove relative order");
    assert!(error.contains("rule sequence"), "{error}");
}

#[test]
fn rule_sequence_accepts_distinct_actual_blocks_inside_one_section() {
    let (input, result, mut w) = order_fixture();
    let second = w.draft.sections.remove("fixture-section-1").unwrap();
    let second_plan = w.draft.plan.remove("fixture-section-1").unwrap();
    w.draft
        .sections
        .get_mut("fixture-section-0")
        .unwrap()
        .content
        .extend(second.content);
    w.draft
        .plan
        .get_mut("fixture-section-0")
        .unwrap()
        .obligation_refs
        .extend(second_plan.obligation_refs);
    compiler::compile(&input, &result, &w.draft, 1_000_000).unwrap();
    w.draft
        .sections
        .get_mut("fixture-section-0")
        .unwrap()
        .content
        .reverse();
    assert!(compiler::compile(&input, &result, &w.draft, 1_000_000).is_err());
}

#[test]
fn rule_sequence_uses_hierarchy_traversal_not_sibling_order_numbers() {
    let (input, result, mut w) = order_fixture();
    w.draft.sections.get_mut("fixture-section-0").unwrap().order = 5;
    w.draft.plan.get_mut("fixture-section-0").unwrap().order = 5;
    w.draft
        .sections
        .get_mut("fixture-section-1")
        .unwrap()
        .parent = Some("fixture-section-0".into());
    w.draft.plan.get_mut("fixture-section-1").unwrap().parent = Some("fixture-section-0".into());
    compiler::compile(&input, &result, &w.draft, 1_000_000).unwrap();
}

#[test]
fn rule_sequence_cannot_be_satisfied_by_labels_over_reversed_actual_templates() {
    let (input, result, mut w) = order_fixture();
    let a = "fixture-section-0";
    let b = "fixture-section-1";
    let a_content = w.draft.sections[a].content.clone();
    let b_content = w.draft.sections[b].content.clone();
    w.draft.sections.get_mut(a).unwrap().content = b_content;
    w.draft.sections.get_mut(b).unwrap().content = a_content;
    let rule_keys: Vec<_> = ["first", "second", "rule-item"]
        .into_iter()
        .map(|item_id| {
            reference_key(&Reference {
                record_id: "rule".into(),
                target: RelationTarget::RuleItem {
                    item_id: item_id.into(),
                },
            })
            .unwrap()
        })
        .collect();
    let a_refs = w.draft.plan[a].obligation_refs.clone();
    let b_refs = w.draft.plan[b].obligation_refs.clone();
    for (section, own, other) in [(a, &a_refs, &b_refs), (b, &b_refs, &a_refs)] {
        w.draft.plan.get_mut(section).unwrap().obligation_refs = own
            .iter()
            .filter(|key| rule_keys.contains(key))
            .chain(other.iter().filter(|key| !rule_keys.contains(key)))
            .cloned()
            .collect();
    }
    assert!(
        compiler::compile(&input, &result, &w.draft, 1_000_000).is_err(),
        "rule item labels cannot prove the order of the actual target templates"
    );
}

#[test]
fn parent_heading_does_not_hide_reversed_target_order_in_children() {
    let (input, result, mut w) = order_fixture();
    let parent = "fixture-section-0";
    let child = "fixture-section-2";
    let mut content = w.draft.sections[parent].clone();
    content.id = child.into();
    content.parent = Some(parent.into());
    content.order = 1;
    let mut plan = w.draft.plan[parent].clone();
    plan.id = child.into();
    plan.parent = Some(parent.into());
    plan.order = 1;
    let rule_keys = ["first", "rule-item"].map(|item_id| {
        reference_key(&Reference {
            record_id: "rule".into(),
            target: RelationTarget::RuleItem {
                item_id: item_id.into(),
            },
        })
        .unwrap()
    });
    plan.obligation_refs.retain(|key| !rule_keys.contains(key));
    w.draft
        .plan
        .get_mut(parent)
        .unwrap()
        .obligation_refs
        .retain(|key| rule_keys.contains(key));
    w.draft.sections.get_mut(parent).unwrap().content.clear();
    w.draft.sections.insert(child.into(), content);
    w.draft.plan.insert(child.into(), plan);
    for (section, order) in [("fixture-section-1", 0), (child, 1)] {
        let s = w.draft.sections.get_mut(section).unwrap();
        s.parent = Some(parent.into());
        s.order = order;
        let p = w.draft.plan.get_mut(section).unwrap();
        p.parent = Some(parent.into());
        p.order = order;
    }
    let error = compiler::compile(&input, &result, &w.draft, 1_000_000)
        .err()
        .unwrap();
    assert!(error.contains("rule sequence"), "{error}");
}

#[test]
fn order_allows_only_a_valid_saved_exception_for_an_unplaced_item() {
    let (input, mut result, mut w) = order_fixture();
    let RecordData::Rule { items, .. } = &mut result.analysis.records.get_mut("rule").unwrap().data
    else {
        unreachable!()
    };
    let mut conditional = items[1].clone();
    conditional.id = "conditional".into();
    conditional.targets.clear();
    items[0].sequence.insert(1, conditional.id.clone());
    items.push(conditional);
    refresh_excerpt_basis(&input, &mut result);
    w.draft.analysis_sha256 = digest(&result).unwrap();
    assert!(
        compiler::compile(&input, &result, &w.draft, 1_000_000).is_err(),
        "unplaced item is not an exception"
    );
    let reference = Reference {
        record_id: "rule".into(),
        target: RelationTarget::RuleItem {
            item_id: "conditional".into(),
        },
    };
    let key = reference_key(&reference).unwrap();
    w.draft.omissions.insert(
        key.clone(),
        Omission {
            reference,
            reason: "Conditional item excluded based on frozen grounds".into(),
            grounds: result.analysis.records["rule"].sources.clone(),
        },
    );
    compiler::compile(&input, &result, &w.draft, 1_000_000).unwrap();
    w.draft.omissions.get_mut(&key).unwrap().grounds.clear();
    assert!(
        compiler::compile(&input, &result, &w.draft, 1_000_000).is_err(),
        "unsubstantiated exception must fail"
    );
}

#[test]
fn rule_sequence_follows_composition_item_links_and_rejects_cycles() {
    let (input, mut result, mut w) = order_fixture();
    let RecordData::Rule { items, .. } = &mut result.analysis.records.get_mut("rule").unwrap().data
    else {
        unreachable!()
    };
    for (id, alias, section) in [
        ("first", "alias-first", "fixture-section-0"),
        ("second", "alias-second", "fixture-section-1"),
    ] {
        let item = items.iter_mut().find(|item| item.id == id).unwrap();
        let mut nested = item.clone();
        nested.id = alias.into();
        item.targets = vec![analysis::RuleItemTarget::RuleItem {
            record_id: "rule".into(),
            item_id: alias.into(),
        }];
        items.push(nested);
        w.draft.plan.get_mut(section).unwrap().obligation_refs.push(
            reference_key(&Reference {
                record_id: "rule".into(),
                target: RelationTarget::RuleItem {
                    item_id: alias.into(),
                },
            })
            .unwrap(),
        );
    }
    refresh_excerpt_basis(&input, &mut result);
    w.draft.analysis_sha256 = digest(&result).unwrap();
    compiler::compile(&input, &result, &w.draft, 1_000_000).unwrap();
    let RecordData::Rule { items, .. } = &mut result.analysis.records.get_mut("rule").unwrap().data
    else {
        unreachable!()
    };
    for (id, target) in [("alias-first", "t1"), ("alias-second", "t0")] {
        items.iter_mut().find(|item| item.id == id).unwrap().targets =
            vec![analysis::RuleItemTarget::Record { id: target.into() }];
    }
    refresh_excerpt_basis(&input, &mut result);
    w.draft.analysis_sha256 = digest(&result).unwrap();
    assert!(
        compiler::compile(&input, &result, &w.draft, 1_000_000).is_err(),
        "indirect content targets determine sequence"
    );
    let RecordData::Rule { items, .. } = &mut result.analysis.records.get_mut("rule").unwrap().data
    else {
        unreachable!()
    };
    items
        .iter_mut()
        .find(|item| item.id == "alias-first")
        .unwrap()
        .targets = vec![analysis::RuleItemTarget::RuleItem {
        record_id: "rule".into(),
        item_id: "first".into(),
    }];
    refresh_excerpt_basis(&input, &mut result);
    w.draft.analysis_sha256 = digest(&result).unwrap();
    let error = compiler::compile(&input, &result, &w.draft, 1_000_000)
        .err()
        .unwrap();
    assert!(error.contains("cyclic"), "{error}");
}
