use super::*;

fn fact(input: &FrozenInput, id: &str, size: usize) -> Record {
    Record {
        id: id.into(),
        sources: vec![citation(input)],
        data: RecordData::Fact {
            name: id.into(),
            value: "x".repeat(size),
            scope: "project".into(),
        },
    }
}

#[test]
fn partial_delivery_advances_after_comparison_without_replacing_source_obligation() {
    let (input, config, mut state) = fixture();
    for id in ["a", "b", "c"] {
        state.analysis.records.insert(
            id.into(),
            fact(&input, id, config.limits.max_tool_result_bytes / 2),
        );
    }
    let task = state.source_review.as_ref().unwrap().active_task.clone();
    let before = digest(&state).unwrap();
    let first = evidence(&input, &config, &state).unwrap().unwrap();
    assert_eq!(
        digest(&state).unwrap(),
        before,
        "planning does not grant delivery or progress"
    );
    let delivery = &first.content["assigned_evidence"]["candidate_delivery"];
    assert_eq!(delivery["complete"], false);
    assert!(
        delivery["capacity_blocked_group"].is_null(),
        "each complete group fits in its own packet"
    );
    let values = first.content["assigned_evidence"]["candidates"]
        .as_array()
        .unwrap();
    assert!(values.iter().any(|v| v["reference"] == "record:a"));
    assert!(!values.iter().any(|v| v["reference"] == "record:b"));
    assert!(!first.coverage.candidate.contains_key("record:b"));
    state.reviewer_coverage = first.coverage;
    context::complete_review_check(&input, &mut state,
        &json!({"reference":"record:a","summary":"Compared synthetic fact against delivered original.","sources":[citation(&input)]}),
        config.limits.max_tool_result_bytes).unwrap();
    let next = evidence(&input, &config, &state).unwrap().unwrap();
    assert!(
        next.content["assigned_evidence"]["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["reference"] == "record:b")
    );
    assert_eq!(state.source_review.as_ref().unwrap().active_task, task);
    assert!(state.source_review.as_ref().unwrap().results.is_empty());
    assert_eq!(state.turn, 0);
}

#[test]
fn relation_delivery_retains_previously_compared_endpoints_and_is_indivisible() {
    let (input, config, mut state) = fixture();
    for id in ["a", "b"] {
        state
            .analysis
            .records
            .insert(id.into(), fact(&input, id, 20));
    }
    state.analysis.relations.insert(
        "edge".into(),
        Relation {
            id: "edge".into(),
            from: "a".into(),
            to: "b".into(),
            from_target: RelationTarget::Record,
            to_target: RelationTarget::Record,
            from_record_sha256: digest(&state.analysis.records["a"]).unwrap(),
            to_record_sha256: digest(&state.analysis.records["b"]).unwrap(),
            kind: RelationKind::References,
            state: RelationState::Explicit,
            scope: "project".into(),
            explanation: "Synthetic source reference".into(),
            grounds: vec![citation(&input)],
        },
    );
    state.reviewer_coverage = evidence(&input, &config, &state).unwrap().unwrap().coverage;
    for id in ["a", "b"] {
        context::complete_review_check(&input, &mut state,
            &json!({"reference":format!("record:{id}"),"summary":"Compared current endpoint.","sources":[citation(&input)]}),
            config.limits.max_tool_result_bytes).unwrap();
    }
    let bundle = evidence(&input, &config, &state).unwrap().unwrap();
    let values = bundle.content["assigned_evidence"]["candidates"]
        .as_array()
        .unwrap();
    for key in ["record:a", "record:b", "relation:edge"] {
        assert!(
            values.iter().any(|v| v["reference"] == key),
            "missing {key}"
        );
    }
    // An endpoint too large to deliver must not leave a seemingly usable edge.
    state.analysis.records.insert(
        "b".into(),
        fact(&input, "b", config.limits.max_tool_result_bytes),
    );
    state.reviewer_coverage.candidate.clear();
    let partial = evidence(&input, &config, &state).unwrap().unwrap();
    assert!(
        !partial.content["assigned_evidence"]["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["reference"] == "relation:edge")
    );
    assert!(!partial.coverage.candidate.contains_key("relation:edge"));
    assert_eq!(
        partial.content["assigned_evidence"]["candidate_delivery"]["complete"],
        false
    );
}
