use super::*;

fn template_on_source(input: &FrozenInput, state: &mut Checkpoint, blank: bool) {
    let text = &input.source_units[0].text;
    let span = Span {
        source_id: "source".into(),
        start: 0,
        end: text.len(),
        view_id: None,
        grid_cell: None,
    };
    state.analysis.records.insert(
        "tpl".into(),
        Record {
            id: "tpl".into(),
            sources: vec![span.clone()],
            data: RecordData::Template {
                label: "乙".into(),
                title: "乙".into(),
                parent: None,
                order: None,
                purpose: "附表".into(),
                applicability: Applicability {
                    state: ApplicabilityState::Applicable,
                    condition: String::new(),
                    scope: "project".into(),
                    grounds: vec![span.clone()],
                },
                regions: vec![TemplateRegion {
                    source: span,
                    role: if blank {
                        RegionRole::BidderBlank
                    } else {
                        RegionRole::FixedText
                    },
                    form_id: None,
                    cells: vec![],
                    blank_ranges: vec![],
                    instruction: "keep labels".into(),
                }],
            },
        },
    );
    let key = "record:tpl";
    let value = context::reference(&state.analysis, key).unwrap();
    state
        .reviewer_coverage
        .candidate
        .insert(key.into(), digest(&value).unwrap());
}

#[test]
fn review_evidence_projects_blank_effect_beside_complete_candidate() {
    let (mut input, config, mut state) = fixture();
    input.source_units[0].text = "投标人名称：________________".into();
    state.analysis.coverage.text.insert(
        "source".into(),
        vec![(0, input.source_units[0].text.len())],
    );
    state.reviewer_coverage.text.insert(
        "source".into(),
        vec![(0, input.source_units[0].text.len())],
    );
    template_on_source(&input, &mut state, true);
    state.source_review = Some(initialize(&input, &config).unwrap());
    select_next(&input, &config, &mut state).unwrap();
    let bundle = evidence(&input, &config, &state).unwrap().unwrap();
    let candidates = bundle.content["assigned_evidence"]["candidates"]
        .as_array()
        .unwrap();
    let tpl = candidates
        .iter()
        .find(|row| row["reference"] == "record:tpl")
        .expect("template candidate");
    assert_eq!(
        tpl["sha256"],
        digest(&context::reference(&state.analysis, "record:tpl").unwrap()).unwrap()
    );
    assert_eq!(tpl["blank_effects"][0]["original"], input.source_units[0].text);
    assert_eq!(tpl["blank_effects"][0]["removed"], input.source_units[0].text);
    assert_eq!(tpl["blank_effects"][0]["retained"], "");
    assert_eq!(tpl["blank_effects"][0]["role"], "bidder_blank");
}

#[test]
fn review_evidence_projects_misaligned_compliance_grounds() {
    let (mut input, config, mut state) = fixture();
    let indicators = "指标与证明要求";
    let control = "满足且证明完整得该条12.5分，否则该条不得分";
    input.source_units[0].text = format!("{indicators}{control}");
    let indicator_span = Span {
        source_id: "source".into(),
        start: 0,
        end: indicators.len(),
        view_id: None,
        grid_cell: None,
    };
    let full = Span {
        source_id: "source".into(),
        start: 0,
        end: input.source_units[0].text.len(),
        view_id: None,
        grid_cell: None,
    };
    state.analysis.coverage.text.insert(
        "source".into(),
        vec![(0, input.source_units[0].text.len())],
    );
    state.reviewer_coverage.text.insert(
        "source".into(),
        vec![(0, input.source_units[0].text.len())],
    );
    state.analysis.records.insert(
        "req".into(),
        Record {
            id: "req".into(),
            sources: vec![full.clone()],
            data: RecordData::Requirement {
                text: "评分项".into(),
                categories: vec![Category::Evaluation],
                strength: Strength::Mandatory,
                compliance: vec![ComplianceClaim {
                    policy: Compliance::Scored,
                    condition: control.into(),
                    grounds: vec![indicator_span],
                }],
                applicability: Applicability {
                    state: ApplicabilityState::Applicable,
                    condition: String::new(),
                    scope: "project".into(),
                    grounds: vec![full],
                },
                response: vec![],
                scoring_rule: Some("见控制条款".into()),
                proofs: vec![],
                criteria: vec![],
            },
        },
    );
    let key = "record:req";
    let value = context::reference(&state.analysis, key).unwrap();
    state
        .reviewer_coverage
        .candidate
        .insert(key.into(), digest(&value).unwrap());
    state.source_review = Some(initialize(&input, &config).unwrap());
    select_next(&input, &config, &mut state).unwrap();
    let bundle = evidence(&input, &config, &state).unwrap().unwrap();
    let req = bundle.content["assigned_evidence"]["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["reference"] == "record:req")
        .expect("requirement candidate");
    let scoring = req["field_ground_checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["path"] == "/data/compliance/0/condition")
        .unwrap();
    assert_eq!(scoring["field_value"], control);
    assert_eq!(scoring["grounds"][0]["cited_text"], indicators);
}
