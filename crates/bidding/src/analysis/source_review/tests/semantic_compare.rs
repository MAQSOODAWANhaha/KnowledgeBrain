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
    state
        .analysis
        .coverage
        .text
        .insert("source".into(), vec![(0, input.source_units[0].text.len())]);
    state
        .reviewer_coverage
        .text
        .insert("source".into(), vec![(0, input.source_units[0].text.len())]);
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
    assert_eq!(tpl["blank_effects"][0]["status"], "delivered");
    assert_eq!(
        tpl["blank_effects"][0]["selected_source_text"],
        input.source_units[0].text
    );
    assert_eq!(
        tpl["blank_effects"][0]["operation"],
        "replace_with_bidder_blank"
    );
    assert_eq!(tpl["blank_effects"][0]["role"], "bidder_blank");
    assert_eq!(tpl["blank_effects"][0]["generated_text"], "");
    assert_eq!(
        tpl["blank_effects"][0]["removed_ranges"],
        json!([{"start":0,"end":input.source_units[0].text.len(),
            "text":input.source_units[0].text}])
    );
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
    state
        .analysis
        .coverage
        .text
        .insert("source".into(), vec![(0, input.source_units[0].text.len())]);
    state
        .reviewer_coverage
        .text
        .insert("source".into(), vec![(0, input.source_units[0].text.len())]);
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

#[test]
fn reviewer_cross_source_ground_projection_uses_only_own_exact_text_and_grid_delivery() {
    let (mut input, config, mut state) = fixture();
    input.source_units.push(Source {
        source_unit_revision_id: "support".into(),
        document_id: "document".into(),
        text: "支持依据甲；未读余文".into(),
        locator: json!({}),
        ordinal: 1,
    });
    input.structured_forms.push(json!({
        "form_definition_revision_id":"support-form","source_unit_revision_id":"support",
        "definition":{"schema_version":3,"kind":"grid","row_count":1,"column_count":2,
            "cells":[
                {"row":0,"column":0,"row_span":1,"col_span":1,"text":"已读网格"},
                {"row":0,"column":1,"row_span":1,"col_span":1,"text":"未读网格"}
            ]}
    }));
    template_on_source(&input, &mut state, false);
    let text_span = Span {
        source_id: "support".into(),
        start: 0,
        end: "支持依据甲".len(),
        view_id: None,
        grid_cell: None,
    };
    let cell_span = |column| Span {
        source_id: "support".into(),
        start: 0,
        end: 0,
        view_id: None,
        grid_cell: Some(GridCitation {
            form_id: "support-form".into(),
            row: 0,
            column,
        }),
    };
    if let RecordData::Template { applicability, .. } =
        &mut state.analysis.records.get_mut("tpl").unwrap().data
    {
        applicability.grounds = vec![
            text_span.clone(),
            cell_span(0),
            cell_span(1),
            Span {
                end: input.source_units[1].text.len(),
                ..text_span
            },
        ];
    }
    state.source_review = Some(initialize(&input, &config).unwrap());
    select_next(&input, &config, &mut state).unwrap();
    assert_eq!(
        state.reviewer_work.as_ref().unwrap().source_scope,
        ["source"]
    );
    let task = tasks(&input, config.limits.max_tool_result_bytes)
        .unwrap()
        .into_iter()
        .find(|task| task.source_id == "source")
        .unwrap();
    // Main receipts and this reviewer's image receipt are not text/cell receipts.
    state.analysis.coverage.text.insert(
        "support".into(),
        vec![(0, input.source_units[1].text.len())],
    );
    state
        .analysis
        .coverage
        .form_cells
        .insert("support-form".into(), vec![(0, 2)]);
    state.reviewer_coverage.views.insert(
        "support-view".into(),
        views::ViewIdentity {
            source_id: "support".into(),
            original_sha256: "a".repeat(64),
            image_sha256: "b".repeat(64),
            page_ordinal: 0,
            width: 1,
            height: 1,
            renderer: "docreader-source-view-v1/test".into(),
        },
    );
    let project = |state: &Checkpoint, mut coverage: Coverage| {
        let mut packet = json!({"assigned_evidence":{"candidates":[]}});
        evidence_candidates::append(&input, state, &task, &mut packet, &mut coverage, usize::MAX)
            .unwrap();
        let candidate = packet["assigned_evidence"]["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["reference"] == "record:tpl")
            .unwrap();
        candidate["field_ground_checks"][0]["grounds"].clone()
    };
    let unread = project(&state, state.reviewer_coverage.clone());
    assert!(
        unread
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["cited_text"].is_null())
    );
    let before = digest(&state).unwrap();
    let mut delivered = state.reviewer_coverage.clone();
    delivered
        .text
        .insert("support".into(), vec![(0, "支持依据甲".len())]);
    delivered
        .form_cells
        .insert("support-form".into(), vec![(0, 1)]);
    let staged = project(&state, delivered.clone());
    assert_eq!(staged[0]["cited_text"], "支持依据甲");
    assert_eq!(staged[1]["cited_text"], "已读网格");
    assert!(
        staged[2]["cited_text"].is_null(),
        "another cell is not delivered"
    );
    assert!(
        staged[3]["cited_text"].is_null(),
        "partial text does not cover the whole source"
    );
    assert_eq!(
        digest(&state).unwrap(),
        before,
        "projection cannot acknowledge delivery or change work"
    );
    state.reviewer_coverage = delivered;
    assert_eq!(project(&state, state.reviewer_coverage.clone()), staged);
    assert_eq!(
        state.reviewer_work.as_ref().unwrap().source_scope,
        ["source"]
    );
    state.role = Role::Main;
    state.main_work = state.reviewer_work.clone();
    let main = project(&state, state.analysis.coverage.clone());
    assert!(
        main.as_array()
            .unwrap()
            .iter()
            .all(|row| row["cited_text"].is_null()),
        "Main projection remains limited to its work scope"
    );
}
