use super::tools::Workspace;
use super::*;
use crate::tender_analysis as analysis;
use serde_json::{Value, json};

#[path = "compiler_rule_order_tests.rs"]
mod rule_order;

#[test]
fn no_response_constraint_keeps_proof_obligations_without_an_extra_response_slot() {
    let (_, mut result) = fixture();
    let RecordData::Requirement {
        response, proofs, ..
    } = &mut result.analysis.records.get_mut("r").unwrap().data
    else {
        panic!("requirement fixture")
    };
    response.clear();
    assert!(!proofs.is_empty());
    let required = compiler::required_references(&result);
    let needs: Vec<_> = required.iter().filter(|r| r.record_id == "r").collect();
    assert_eq!(needs.len(), 1);
    assert_eq!(needs[0].target, RelationTarget::Proof { index: 0 });
    assert!(required.iter().any(|r| r.record_id == "t0"));
}

fn fixture() -> (FrozenInput, AnalysisResult) {
    let texts = [
        "按要求编制技术响应及报价附件，保留表后说明。声明：所附格式供投标使用。\n注：填写证明材料名称及页码。\n投标人（盖章）：____ 日期：____",
        "按要求编制报价附件。声明：本表用于投标报价。\n注：报价需保持一致。\n附件乙明确不适用本次投标。",
    ];
    let mut input = FrozenInput {
        schema_version: 1,
        project_id: "fixture-project".into(),
        document_set_id: "frozen-set".into(),
        documents: vec![],
        document_relations: vec![],
        source_units: vec![],
        structured_forms: vec![],
        decisions: vec![],
    };
    for (i, text) in texts.iter().enumerate() {
        input.source_units.push(Source {
            source_unit_revision_id: format!("s{i}"),
            document_id: format!("d{i}"),
            text: (*text).into(),
            locator: json!({"page_ordinal":i}),
            ordinal: i,
        });
        let cells = vec![
            json!({"row":0,"column":0,"row_span":1,"col_span":3,"text":"附表甲"}),
            json!({"row":1,"column":0,"row_span":2,"col_span":1,"text":"设备\n名称"}),
            json!({"row":1,"column":1,"row_span":1,"col_span":1,"text":"报价"}),
            json!({"row":1,"column":2,"row_span":1,"col_span":1,"text":""}),
            json!({"row":2,"column":1,"row_span":1,"col_span":1,"text":"证明材料"}),
            json!({"row":2,"column":2,"row_span":1,"col_span":1,"text":""}),
        ];
        input.structured_forms.push(json!({"form_definition_revision_id":format!("f{i}"),"source_unit_revision_id":format!("s{i}"),"definition":{"schema_version":3,"kind":"grid","row_count":3,"column_count":3,"widths_mm":[100,70,50],"cells":cells}}));
    }
    let mut a = Analysis::default();
    for source in &input.source_units {
        a.coverage.text.insert(
            source.source_unit_revision_id.clone(),
            vec![(0, source.text.len())],
        );
        a.dispositions.insert(
            source.source_unit_revision_id.clone(),
            Disposition {
                state: DispositionState::Requirement,
                reason: "响应及格式".into(),
            },
        );
    }
    for i in 0..2 {
        a.coverage.form_cells.insert(format!("f{i}"), vec![(0, 9)]);
        let text = &input.source_units[i].text;
        let cut = text.find('\n').unwrap();
        let span = |start, end| Span {
            source_id: format!("s{i}"),
            start,
            end,
            view_id: None,
            grid_cell: None,
        };
        let regions = vec![
            TemplateRegion {
                source: span(0, cut),
                role: RegionRole::FixedText,
                form_id: None,
                header_rows: None,
                cells: vec![],
                blank_ranges: vec![],
                instruction: "保留声明".into(),
            },
            TemplateRegion {
                source: span(0, cut),
                role: RegionRole::FixedText,
                form_id: Some(format!("f{i}")),
                header_rows: Some(1),
                cells: vec![
                    Cell { row: 0, column: 0 },
                    Cell { row: 1, column: 0 },
                    Cell { row: 1, column: 1 },
                    Cell { row: 2, column: 1 },
                ],
                blank_ranges: vec![],
                instruction: "保留标签".into(),
            },
            TemplateRegion {
                source: span(0, cut),
                role: RegionRole::BidderBlank,
                form_id: Some(format!("f{i}")),
                header_rows: Some(1),
                cells: vec![Cell { row: 1, column: 2 }, Cell { row: 2, column: 2 }],
                blank_ranges: vec![],
                instruction: "投标方后续填写".into(),
            },
            TemplateRegion {
                source: span(cut + 1, text.len()),
                role: RegionRole::Instruction,
                form_id: None,
                header_rows: None,
                cells: vec![],
                blank_ranges: vec![],
                instruction: "保留表后说明和签章".into(),
            },
        ];
        let full = span(0, text.len());
        a.records.insert(
            format!("t{i}"),
            Record {
                id: format!("t{i}"),
                sources: vec![full.clone()],
                data: RecordData::Template {
                    label: "附表甲".into(),
                    title: "附表甲".into(),
                    parent: None,
                    order: None,
                    purpose: "本次投标格式".into(),
                    applicability: Applicability {
                        state: ApplicabilityState::Applicable,
                        scope: format!("d{i}"),
                        condition: "原文指定".into(),
                        grounds: vec![full],
                    },
                    regions,
                },
            },
        );
    }
    let grounds = vec![Span {
        source_id: "s0".into(),
        start: 0,
        end: texts[0].len(),
        view_id: None,
        grid_cell: None,
    }];
    let requirement:Record=serde_json::from_value(json!({"id":"r","sources":grounds,"data":{"kind":"requirement","text":"提交技术、报价及证明位置","categories":["technical","pricing"],"strength":"mandatory","compliance":[{"policy":"explicit_response","condition":"投标时","grounds":grounds}],"applicability":{"state":"applicable","scope":"本项目","condition":"原文指定","grounds":grounds},"response":[{"channel":"structured_form","description":"技术表格","condition":"投标时","grounds":grounds},{"channel":"quotation","description":"报价表格","condition":"投标时","grounds":grounds}],"proofs":[{"description":"证明材料","subject":"产品","validity":"原文条件","issuer":"原文出具方","condition":"投标时","grounds":grounds,"name_required":true,"page_required":true}],"criteria":[],"scoring_rule":null}})).unwrap();
    a.records.insert("r".into(), requirement);
    let mut skipped = a.records["t1"].clone();
    skipped.id = "not-applicable".into();
    if let RecordData::Template {
        applicability,
        regions,
        ..
    } = &mut skipped.data
    {
        applicability.state = ApplicabilityState::NotApplicable;
        regions.retain(|r| r.form_id.is_none());
    }
    a.records.insert(skipped.id.clone(), skipped);
    for i in 0..2 {
        let r = Relation {
            id: format!("link{i}"),
            from: "r".into(),
            to: format!("t{i}"),
            from_target: RelationTarget::Response { index: i },
            to_target: RelationTarget::Record,
            from_record_sha256: digest(&a.records["r"]).unwrap(),
            to_record_sha256: digest(&a.records[&format!("t{i}")]).unwrap(),
            kind: RelationKind::RequiresTemplate,
            state: RelationState::Explicit,
            scope: "本项目".into(),
            explanation: "响应应进入指定表格".into(),
            grounds: grounds.clone(),
        };
        a.relations.insert(r.id.clone(), r);
    }
    assert!(
        analysis::tools::gaps(&input, &a).is_empty(),
        "{:?}",
        analysis::tools::gaps(&input, &a)
    );
    let mut coverage = a.coverage.clone();
    for kind in ["all", "relation", "disposition"] {
        analysis::tools::invoke(
            &input,
            &mut a,
            &mut coverage,
            true,
            "inspect_analysis",
            &json!({"view":"detail","kind":kind,"offset":0,"limit":100}),
            100_000,
        )
        .unwrap();
    }
    let mut result = AnalysisResult {
        schema_version: 2,
        frozen_input_sha256: digest(&input).unwrap(),
        review: Review {
            analysis_sha256: digest(&a).unwrap(),
            coverage,
            findings: vec![],
            ..Default::default()
        },
        analysis: a,
        quality: "verified".into(),
        source_views: BTreeMap::new(),
    };
    seal_review(&input, &mut result);
    (input, result)
}
fn seal_review(input: &FrozenInput, result: &mut AnalysisResult) {
    use crate::tender_analysis::rule_contract::*;
    result.schema_version = 2;
    let checks: Vec<_> = ANALYSIS_GLOBAL_CHECK_KEYS
        .iter()
        .map(|key| GlobalCheck {
            key: (*key).into(),
            scope_sha256: scope_sha256(input, &result.analysis).unwrap(),
            conclusion: GlobalCheckConclusion::Pass,
            grounds: input
                .source_units
                .first()
                .map(|s| {
                    vec![Span {
                        source_id: s.source_unit_revision_id.clone(),
                        start: 0,
                        end: s.text.len(),
                        view_id: None,
                        grid_cell: None,
                    }]
                })
                .unwrap_or_default(),
            record_ids: vec![],
            finding_ids: vec![],
        })
        .collect();
    result.analysis.review_global_checks =
        checks.iter().map(|c| (c.key.clone(), c.clone())).collect();
    result.analysis.main_global_checks = result.analysis.review_global_checks.clone();
    result.review.contract_sha256 = contract_sha256().unwrap();
    result.review.global_checks = checks;
    result.review.analysis_sha256 = digest(&result.analysis).unwrap();
}
fn plan_for_section(result: &AnalysisResult, section: &Value) -> Value {
    let parsed: Section = serde_json::from_value(section.clone()).unwrap();
    let refs: Vec<_> = obligation_inventory(result)
        .iter()
        .filter(|r| {
            parsed.content.iter().any(|c| match c {
                Content::Template {
                    record_id,
                    bindings,
                    ..
                } => {
                    r.record_id == *record_id && r.target == RelationTarget::Record
                        || bindings.iter().any(|b| &b.need == *r)
                }
                Content::Placeholder { needs } | Content::ResponseTable { needs, .. } => {
                    needs.contains(r)
                }
                Content::SourceResponse { need, .. } => need == *r,
                Content::BidderBlank | Content::Preserved { .. } => false,
            })
        })
        .map(|r| reference_key(r).unwrap())
        .collect();
    json!({"id":parsed.id,"kind":"section","parent":parsed.parent,"order":parsed.order,"title":parsed.title,"placement":parsed.placement,"prescribed":true,"grounds":parsed.grounds,"obligation_refs":refs})
}
fn presentation(input: &FrozenInput) -> Value {
    json!({"title":"投标模板","toc_title":"目录","style":{"width_mm":297,"height_mm":210,"top_mm":20,"right_mm":20,"bottom_mm":20,"left_mm":20,"font_family":"Noto Sans CJK SC","body_font_pt":10.5,"line_spacing":1.5},"grounds":[{"source_id":"s0","start":0,"end":input.source_units[0].text.len()}],"explanation":"测试显式横向页面与字体，表宽来自冻结网格"})
}
fn section(input: &FrozenInput, i: usize) -> Value {
    let mut bindings = vec![
        json!({"need":{"record_id":"r","target":{"kind":"response","index":i}},"field":{"kind":"template_cell","form_id":format!("f{i}"),"row":1,"column":2}}),
    ];
    if i == 0 {
        bindings.push(json!({"need":{"record_id":"r","target":{"kind":"proof","index":0}},"field":{"kind":"template_cell","form_id":"f0","row":2,"column":2}}));
    }
    json!({"id":format!("fixture-section-{i}"),"parent":null,"order":i,"title":if i==0{"技术响应"}else{"报价附件"},"grounds":[{"source_id":format!("s{i}"),"start":0,"end":input.source_units[i].text.len()}],"content":[{"kind":"template","record_id":format!("t{i}"),"headers":[{"form_id":format!("f{i}"),"header_rows":1}],"bindings":bindings}]})
}
fn omission(input: &FrozenInput) -> Value {
    json!({"reference":{"record_id":"not-applicable","target":{"kind":"record"}},"reason":"原文明确不适用","grounds":[{"source_id":"s1","start":input.source_units[1].text.find("附件乙").unwrap(),"end":input.source_units[1].text.len()}]})
}
fn edit(
    w: &mut Workspace,
    input: &FrozenInput,
    result: &AnalysisResult,
    name: &str,
    mut args: Value,
) -> Value {
    if name == "put_section" {
        if args["id"].is_null() {
            args["id"] = json!(uuid::Uuid::new_v4().to_string());
        }
        {
            let mut plan = plan_for_section(result, &args);
            plan["expected_draft_sha256"] = json!(digest(&w.draft).unwrap());
            w.invoke(
                input,
                result,
                "put_composition_plan_item",
                &plan,
                tool_limits(100_000, 1_000_000),
            )
            .unwrap();
        }
    }
    args["expected_draft_sha256"] = json!(digest(&w.draft).unwrap());
    w.invoke(input, result, name, &args, tool_limits(100_000, 1_000_000))
        .unwrap()
}
fn ready(input: &FrozenInput, result: &AnalysisResult) -> Workspace {
    let mut w = Workspace::new(input, result).unwrap();
    w.source_coverage = result.analysis.coverage.clone();
    edit(
        &mut w,
        input,
        result,
        "set_presentation",
        presentation(input),
    );
    for i in 0..2 {
        edit(&mut w, input, result, "put_section", section(input, i));
    }
    edit(&mut w, input, result, "put_omission", omission(input));
    w
}

/// 空正文只属于草稿骨架；官方编制路径必须拒绝 `bidder_blank`。
#[test]
fn put_section_rejects_bidder_blank_body_in_official_composition() {
    let (input, result) = fixture();
    let mut w = ready(&input, &result);
    let mut args = section(&input, 0);
    args["content"] = json!([{"kind":"bidder_blank"}]);
    args["expected_draft_sha256"] = json!(digest(&w.draft).unwrap());
    let error = w
        .invoke(
            &input,
            &result,
            "put_section",
            &args,
            tool_limits(100_000, 1_000_000),
        )
        .unwrap_err();
    assert!(
        error.contains("official composition cannot leave a chapter body blank"),
        "{error}"
    );
}

/// 回读保留正文只属于草稿填章；终稿必须自己写正文，不能夹带用户手写的文字。
#[test]
fn put_section_rejects_preserved_body_in_official_composition() {
    let (input, result) = fixture();
    let mut w = ready(&input, &result);
    let mut args = section(&input, 0);
    args["content"] = json!([{
        "kind":"preserved",
        "blocks":[{"kind":"paragraphs","unit_keys":["story:1"],"paragraphs":["用户写的"]}]
    }]);
    args["expected_draft_sha256"] = json!(digest(&w.draft).unwrap());
    let error = w
        .invoke(
            &input,
            &result,
            "put_section",
            &args,
            tool_limits(100_000, 1_000_000),
        )
        .unwrap_err();
    assert!(
        error.contains("official composition cannot carry read-back bidder text"),
        "{error}"
    );
}

#[test]
fn put_section_rejects_template_record_as_bidder_need() {
    let (input, result) = fixture();
    let mut w = ready(&input, &result);
    let mut args = section(&input, 0);
    args["content"][0]["bindings"] = json!([{
        "need":{"record_id":"t0","target":{"kind":"record"}},
        "field":{"kind":"record"}
    }]);
    args["expected_draft_sha256"] = json!(digest(&w.draft).unwrap());
    let error = w
        .invoke(
            &input,
            &result,
            "put_section",
            &args,
            tool_limits(100_000, 1_000_000),
        )
        .unwrap_err();
    assert!(
        error.contains("bidder placeholder must identify"),
        "{error}"
    );
    assert!(
        w.draft.sections.len() == 2,
        "invalid binding must not become a checkpoint"
    );
}

fn document_xml(bytes: &[u8]) -> String {
    use std::io::{Cursor, Read};
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    zip.by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    xml
}

#[test]
fn grid_region_binding_keeps_each_selected_anchor_location() {
    let (input, result) = fixture();
    let mut workspace = ready(&input, &result);
    let section = workspace
        .draft
        .sections
        .values_mut()
        .find(|s| s.order == 0)
        .unwrap();
    let Content::Template { bindings, .. } = &mut section.content[0] else {
        panic!("template fixture required")
    };
    // The reviewed region selects two input cells, not their adjacent labels.
    bindings[0].field = RelationTarget::TemplateRegion { index: 2 };
    let compiled = compiler::compile(&input, &result, &workspace.draft, 1_000_000).unwrap();
    let locations: Vec<_> = compiled
        .manifest
        .placements
        .iter()
        .filter(|p| {
            p.reference.record_id == "r"
                && p.reference.target == RelationTarget::Response { index: 0 }
        })
        .map(|p| &p.location)
        .collect();
    let coordinates: Vec<_> = locations
        .iter()
        .map(|location| {
            let cell = location
                .cell
                .as_ref()
                .expect("region binding lost its cell");
            (cell.row, cell.column)
        })
        .collect();
    assert_eq!(coordinates, [(1, 2), (2, 2)]);
    for location in locations {
        let rendered = compiled
            .rendered
            .iter()
            .find(|block| block.bookmark == location.bookmark)
            .unwrap();
        let cell = location.cell.as_ref().unwrap();
        assert!(
            rendered
                .table
                .as_ref()
                .unwrap()
                .cells
                .iter()
                .any(|c| { c.row == cell.row && c.column == cell.column && c.text.is_empty() })
        );
    }
}

#[test]
fn single_merged_anchor_region_and_cell_bind_to_the_same_native_location() {
    let (input, mut result) = fixture();
    let RecordData::Template { regions, .. } =
        &mut result.analysis.records.get_mut("t0").unwrap().data
    else {
        panic!("template fixture required")
    };
    // Partition existing fixed policies by explicit cell identity. The merged
    // anchor stays one location; covered positions are not additional fields.
    let mut merged = regions[1].clone();
    merged.cells = vec![Cell { row: 0, column: 0 }];
    regions[1].cells.retain(|c| (c.row, c.column) != (0, 0));
    regions.insert(2, merged);
    refresh_excerpt_basis(&input, &mut result);
    let mut workspace = ready(&input, &result);
    let section = workspace
        .draft
        .sections
        .values_mut()
        .find(|s| s.order == 0)
        .unwrap();
    let Content::Template { bindings, .. } = &mut section.content[0] else {
        panic!("template fixture required")
    };
    bindings[0].field = RelationTarget::TemplateRegion { index: 2 };
    let compiled = compiler::compile(&input, &result, &workspace.draft, 1_000_000).unwrap();
    let locate = |record: &str, target: RelationTarget| {
        compiled
            .manifest
            .placements
            .iter()
            .filter(|p| p.reference.record_id == record && p.reference.target == target)
            .map(|p| serde_json::to_value(&p.location).unwrap())
            .collect::<Vec<_>>()
    };
    let native = locate(
        "t0",
        RelationTarget::TemplateCell {
            form_id: "f0".into(),
            row: 0,
            column: 0,
        },
    );
    assert_eq!(native.len(), 1);
    assert_eq!(native[0]["cell"], json!({"row":0,"column":0}));
    assert_eq!(
        locate("t0", RelationTarget::TemplateRegion { index: 2 }),
        native
    );
    assert_eq!(locate("r", RelationTarget::Response { index: 0 }), native);
}

fn partial_cell_fixture() -> (FrozenInput, AnalysisResult) {
    use crate::template_grid::CellTextRange;
    let (mut input, mut result) = fixture();
    let mut ranges = vec![];
    let mut coverage = result.analysis.coverage.clone();
    for (row, text, samples) in [
        (
            1,
            "认证机关：示例甲；证书编号：ABC；有效期：____",
            vec!["示例甲", "ABC"],
        ),
        (
            2,
            "材料名称：示例证明\n页码：99（加盖单位章）",
            vec!["示例证明", "99"],
        ),
    ] {
        let cell = input.structured_forms[0]["definition"]["cells"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|c| c["row"] == row && c["column"] == 2)
            .unwrap();
        cell["text"] = json!(text);
        let offset = row * 3 + 2;
        for sample in samples {
            let read = analysis::tools::invoke(
                &input,
                &mut result.analysis,
                &mut coverage,
                false,
                "read_form",
                &json!({"form_id":"f0","offset":offset,"limit":1,"find_text":sample}),
                100_000,
            )
            .unwrap();
            let matches: Vec<CellTextRange> =
                serde_json::from_value(read["matches"][0].clone()).unwrap();
            assert_eq!(matches.len(), 1);
            ranges.extend(matches);
        }
    }
    let RecordData::Template { regions, .. } =
        &mut result.analysis.records.get_mut("t0").unwrap().data
    else {
        panic!()
    };
    regions[2].blank_ranges = ranges;
    refresh_excerpt_basis(&input, &mut result);
    (input, result)
}

#[test]
fn partial_cell_blanks_preserve_fixed_wording_and_native_field_locations() {
    let (input, result) = partial_cell_fixture();
    let record = &result.analysis.records["t0"];
    let schema = analysis::tools::schemas(false)
        .into_iter()
        .find(|t| t["function"]["name"] == "put_record")
        .unwrap();
    assert!(
        jsonschema::JSONSchema::compile(&schema["function"]["parameters"])
            .unwrap()
            .is_valid(&json!(record))
    );
    // Exercise the actual incremental record mutation, not only deserialization.
    let mut a = result.analysis.clone();
    let mut coverage = a.coverage.clone();
    analysis::tools::invoke(
        &input,
        &mut a,
        &mut coverage,
        false,
        "put_record",
        &json!(record),
        100_000,
    )
    .unwrap();
    let w = ready(&input, &result);
    let output = compiler::compile(&input, &result, &w.draft, 1_000_000).unwrap();
    let table = output
        .rendered
        .iter()
        .find(|b| b.bookmark == "kb_s0_b1")
        .unwrap()
        .table
        .as_ref()
        .unwrap();
    for (row, expected) in [
        (1, "认证机关：；证书编号：；有效期：____"),
        (2, "材料名称：\n页码：（加盖单位章）"),
    ] {
        assert_eq!(
            table
                .cells
                .iter()
                .find(|c| c.row == row && c.column == 2)
                .unwrap()
                .text,
            expected
        );
        assert!(output.manifest.placements.iter().any(|p| {
            p.reference.record_id == "t0"
                && p.location.bookmark == "kb_s0_b1"
                && p.location
                    .cell
                    .as_ref()
                    .is_some_and(|c| c.row == row && c.column == 2)
        }));
    }
    let xml = document_xml(&output.docx);
    for sample in ["示例甲", "ABC", "示例证明", "99"] {
        assert!(!xml.contains(sample));
    }
    if let Ok(dir) = std::env::var("KB_PARTIAL_CELL_ARTIFACT_DIR") {
        use std::io::Write;
        let path = std::path::Path::new(&dir);
        std::fs::create_dir(path).unwrap();
        for (name, data) in [
            ("partial-cell-template.docx", output.docx),
            ("source.json", serde_json::to_vec_pretty(&input).unwrap()),
            ("analysis.json", serde_json::to_vec_pretty(&result).unwrap()),
            (
                "manifest.json",
                serde_json::to_vec_pretty(&output.manifest).unwrap(),
            ),
            (
                "verification.json",
                serde_json::to_vec_pretty(
                    &json!({"synthetic":true,"real_agent_output":false,"rendered":output.rendered}),
                )
                .unwrap(),
            ),
        ] {
            std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(path.join(name))
                .unwrap()
                .write_all(&data)
                .unwrap();
        }
    }
}

#[test]
fn partial_cell_policy_rejects_unread_foreign_overlapping_and_ambiguous_ranges() {
    let (input, result) = partial_cell_fixture();
    let original = json!(result.analysis.records["t0"]);
    for mode in [
        "fixed",
        "no_form",
        "foreign_cell",
        "covered",
        "missing_cell",
        "utf8",
        "reversed",
        "overflow",
        "overlap",
        "duplicate_region",
    ] {
        let mut record = original.clone();
        let region = &mut record["data"]["regions"][2];
        match mode {
            "fixed" => region["role"] = json!("fixed_text"),
            "no_form" => region["form_id"] = Value::Null,
            "foreign_cell" => region["blank_ranges"][0]["column"] = json!(99),
            "covered" => {
                region["cells"][0] = json!({"row":0,"column":1});
                region["blank_ranges"][0]["row"] = json!(0);
                region["blank_ranges"][0]["column"] = json!(1);
            }
            "missing_cell" => region["blank_ranges"]
                .as_array_mut()
                .unwrap()
                .retain(|r| r["row"] == 1),
            "utf8" => region["blank_ranges"][0]["start"] = json!(16),
            "reversed" => region["blank_ranges"][0]["end"] = json!(0),
            "overflow" => region["blank_ranges"][0]["end"] = json!(usize::MAX),
            "overlap" => {
                let copy = region["blank_ranges"][0].clone();
                region["blank_ranges"].as_array_mut().unwrap().push(copy);
            }
            "duplicate_region" => {
                let copy = region.clone();
                record["data"]["regions"].as_array_mut().unwrap().push(copy);
            }
            _ => unreachable!(),
        }
        let record: Record = serde_json::from_value(record).unwrap();
        assert!(
            analysis::tools::validate_record(&input, &result.analysis, &record).is_err(),
            "{mode}"
        );
    }
    let mut unread = result.analysis.clone();
    unread.coverage.form_cells.remove("f0");
    assert!(
        analysis::tools::validate_record(&input, &unread, &result.analysis.records["t0"]).is_err()
    );
    // The reviewed basis is invalidated when a partial policy changes.
    let mut altered = result.clone();
    let RecordData::Template { regions, .. } =
        &mut altered.analysis.records.get_mut("t0").unwrap().data
    else {
        panic!()
    };
    regions[2].blank_ranges.clear();
    assert!(validate_basis(&input, &altered).is_err());
}

#[test]
fn primitive_partial_blanks_cannot_target_headers_or_combine_with_whole_cell_blanks() {
    use crate::docx_template::{TemplatePlan, compile_template};
    let (input, _) = partial_cell_fixture();
    let mut fixture = input_for_order_verification().1;
    fixture["sections"][0]["blocks"][0] = json!({"kind":"table","source_id":"s0","quote":null,
        "form_id":"f0","header_rows":1,"blank_cells":[],"columns":[],"blank_rows":0,
        "blank_ranges":[{"row":1,"column":2,"start":15,"end":24}]});
    fixture["excluded_forms"] = json!([{"form_id":"f1","reason":"isolated primitive test"}]);
    // Reuse an explicit printable width sufficient for the synthetic grid.
    fixture["style"] = presentation(&input)["style"].clone();
    let good: TemplatePlan = serde_json::from_value(fixture.clone()).unwrap();
    let bytes = compile_template(&json!(input), &good).unwrap();
    document::verify(&bytes, &json!(input), &good).unwrap();
    for mode in ["header", "whole", "covered", "non_table"] {
        let mut bad = fixture.clone();
        let b = &mut bad["sections"][0]["blocks"][0];
        match mode {
            "header" => b["blank_ranges"] = json!([{"row":0,"column":0,"start":0,"end":3}]),
            "whole" => b["blank_cells"] = json!([{"row":1,"column":2}]),
            "covered" => b["blank_ranges"] = json!([{"row":0,"column":1,"start":0,"end":3}]),
            "non_table" => b["kind"] = json!("blank"),
            _ => unreachable!(),
        }
        let bad: TemplatePlan = serde_json::from_value(bad).unwrap();
        assert!(compile_template(&json!(input), &bad).is_err(), "{mode}");
    }
    // A correct old full-cell blank cannot stand in for the reviewed partial policy.
    let mut wrong = good.clone();
    wrong.sections[0].blocks[0].blank_ranges.clear();
    wrong.sections[0].blocks[0]
        .blank_cells
        .push(crate::docx_template::CellRef { row: 1, column: 2 });
    let wrong_bytes = compile_template(&json!(input), &wrong).unwrap();
    assert!(document::verify(&wrong_bytes, &json!(input), &good).is_err());
}

#[test]
fn front_matter_cannot_hide_all_body_chapters_or_be_nested_or_follow_body() {
    let (input, result) = fixture();
    let w = ready(&input, &result);
    for mode in ["late", "nested", "all"] {
        let mut draft = w.draft.clone();
        let first = draft
            .sections
            .values()
            .find(|s| s.order == 0)
            .unwrap()
            .id
            .clone();
        let second = draft
            .sections
            .values()
            .find(|s| s.order == 1)
            .unwrap()
            .id
            .clone();
        draft.sections.get_mut(&second).unwrap().placement = SectionPlacement::FrontMatter;
        if mode == "nested" {
            draft.sections.get_mut(&second).unwrap().parent = Some(first.clone());
        }
        if mode == "all" {
            draft.sections.get_mut(&first).unwrap().placement = SectionPlacement::FrontMatter;
        }
        assert!(
            compiler::compile(&input, &result, &draft, 1_000_000).is_err(),
            "{mode}"
        );
    }
}

#[test]
fn native_verifier_rejects_reordered_ranges_and_changed_toc_instruction() {
    use crate::docx_template::{TemplatePlan, compile_template};
    use std::io::{Cursor, Read, Write};
    let input = input_for_order_verification();
    let (source, plan): (Value, TemplatePlan) = (input.0, serde_json::from_value(input.1).unwrap());
    let bytes = compile_template(&source, &plan).unwrap();
    document::verify(&bytes, &source, &plan).unwrap();
    let xml = document_xml(&bytes);
    let doc = roxmltree::Document::parse(&xml).unwrap();
    let ns = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
    let range = |name| {
        let start = doc
            .descendants()
            .find(|n| {
                n.has_tag_name((ns, "bookmarkStart")) && n.attribute((ns, "name")) == Some(name)
            })
            .unwrap();
        let id = start.attribute((ns, "id")).unwrap();
        let end = doc
            .descendants()
            .find(|n| n.has_tag_name((ns, "bookmarkEnd")) && n.attribute((ns, "id")) == Some(id))
            .unwrap();
        (start.range().start, end.range().end)
    };
    let a = (range("kb_s0").0, range("kb_s0_b0").1);
    let b = (range("kb_s1").0, range("kb_s1_b0").1);
    let reordered = format!(
        "{}{}{}{}{}",
        &xml[..a.0],
        &xml[b.0..b.1],
        &xml[a.1..b.0],
        &xml[a.0..a.1],
        &xml[b.1..]
    );
    let changed_toc = xml.replace("1-9", "1-1");
    for changed in [reordered, changed_toc] {
        let mut original = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
        let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for i in 0..original.len() {
            let mut entry = original.by_index(i).unwrap();
            let name = entry.name().to_owned();
            let mut content = vec![];
            entry.read_to_end(&mut content).unwrap();
            output
                .start_file(
                    &name,
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Stored),
                )
                .unwrap();
            output
                .write_all(if name == "word/document.xml" {
                    changed.as_bytes()
                } else {
                    &content
                })
                .unwrap();
        }
        assert!(document::verify(&output.finish().unwrap().into_inner(), &source, &plan).is_err());
    }
}

fn input_for_order_verification() -> (Value, Value) {
    let (input, _) = fixture();
    let blank = json!({"kind":"blank","source_id":null,"quote":null,"form_id":null,"header_rows":0,"blank_cells":[],"columns":[],"blank_rows":0});
    let plan = json!({"title":"结构验证","toc_title":"目录","style":presentation(&input)["style"],
        "sections":[{"title":"封面","placement":"front_matter","depth":0,"source_ids":["s0"],"blocks":[blank]},
            {"title":"正文","depth":0,"source_ids":["s1"],"blocks":[blank]}],
        "excluded_sources":[],"excluded_forms":[{"form_id":"f0","reason":"测试仅验证段落顺序"},{"form_id":"f1","reason":"测试仅验证段落顺序"}],"notices":[]});
    (json!(input), plan)
}

fn refresh_excerpt_basis(input: &FrozenInput, result: &mut AnalysisResult) {
    for relation in result.analysis.relations.values_mut() {
        relation.from_record_sha256 = digest(&result.analysis.records[&relation.from]).unwrap();
        relation.to_record_sha256 = digest(&result.analysis.records[&relation.to]).unwrap();
    }
    let mut coverage = result.analysis.coverage.clone();
    for kind in ["all", "relation", "disposition"] {
        analysis::tools::invoke(
            input,
            &mut result.analysis,
            &mut coverage,
            true,
            "inspect_analysis",
            &json!({"view":"detail","kind":kind,"offset":0,"limit":100}),
            100_000,
        )
        .unwrap();
    }
    result.frozen_input_sha256 = digest(input).unwrap();
    result.review = Review {
        analysis_sha256: digest(&result.analysis).unwrap(),
        coverage,
        findings: vec![],
        ..Default::default()
    };
    seal_review(input, result);
    assert!(
        analysis::tools::gaps(input, &result.analysis).is_empty(),
        "{:?}",
        analysis::tools::gaps(input, &result.analysis)
    );
}

fn excerpt_fixture() -> (FrozenInput, AnalysisResult, Value) {
    let (mut input, mut result) = fixture();
    let mut grounds = vec![];
    for (i, text) in ["条款甲：须逐项响应。\n需提供承", "诺及说明。"]
        .into_iter()
        .enumerate()
    {
        let id = format!("e{i}");
        input.source_units.push(Source {
            source_unit_revision_id: id.clone(),
            document_id: "d0".into(),
            text: text.into(),
            locator: json!({"page_ordinal":i+2}),
            ordinal: i + 2,
        });
        grounds.push(Span {
            source_id: id.clone(),
            start: 0,
            end: text.len(),
            view_id: None,
            grid_cell: None,
        });
        grounds.push(Span {
            source_id: id.clone(),
            start: 0,
            end: 0,
            view_id: None,
            grid_cell: Some(GridCitation {
                form_id: format!("ef{i}"),
                row: 0,
                column: 0,
            }),
        });
        result
            .analysis
            .coverage
            .text
            .insert(id.clone(), vec![(0, text.len())]);
        result.analysis.dispositions.insert(
            id.clone(),
            Disposition {
                state: DispositionState::Requirement,
                reason: "逐段响应原文".into(),
            },
        );
        input.structured_forms.push(
            json!({"form_definition_revision_id":format!("ef{i}"),"source_unit_revision_id":id,
            "definition":{"schema_version":3,"kind":"grid","row_count":1,"column_count":2,"widths_mm":[40,40],
                "cells":[{"row":0,"column":0,"row_span":1,"col_span":2,"text":text}]}}),
        );
        result
            .analysis
            .coverage
            .form_cells
            .insert(format!("ef{i}"), vec![(0, 2)]);
    }
    let mut record = json!(result.analysis.records["r"]);
    record["id"] = json!("excerpt-need");
    record["sources"] = json!(grounds);
    record["data"]["applicability"]["grounds"] = json!(grounds);
    record["data"]["compliance"][0]["grounds"] = json!(grounds);
    record["data"]["response"] = json!([{"channel":"narrative_content","description":"逐项响应","condition":"投标时","grounds":grounds}]);
    record["data"]["proofs"] = json!([]);
    result.analysis.records.insert(
        "excerpt-need".into(),
        serde_json::from_value(record).unwrap(),
    );
    refresh_excerpt_basis(&input, &mut result);
    let section = json!({"id":null,"parent":null,"order":2,"title":"逐项响应","grounds":grounds,
    "content":[{"kind":"source_response","need":{"record_id":"excerpt-need","target":{"kind":"response","index":0}},
        "paragraphs":[[
            {"kind":"text","source_id":"e0","start":0,"end":input.source_units[2].text.len()},
            {"kind":"grid_cell","source_id":"e1","form_id":"ef1","row":0,"column":0}
        ]]}]});
    (input, result, section)
}

#[test]
fn source_response_rejects_foreign_invalid_sample_and_non_response_evidence() {
    let (input, result, section) = excerpt_fixture();
    let invalid = [
        json!({"kind":"text","source_id":"e0","start":1,"end":6}),
        json!({"kind":"text","source_id":"e0","start":0,"end":9999}),
        json!({"kind":"text","source_id":"s0","start":0,"end":6}),
        json!({"kind":"grid_cell","source_id":"e1","form_id":"ef0","row":0,"column":0}),
        json!({"kind":"grid_cell","source_id":"e1","form_id":"ef1","row":0,"column":1}),
    ];
    for part in invalid {
        let mut s = section.clone();
        s["content"][0]["paragraphs"] = json!([[part]]);
        let mut w = ready(&input, &result);
        edit(&mut w, &input, &result, "put_section", s);
        assert!(compiler::compile(&input, &result, &w.draft, 1_000_000).is_err());
    }
    for target in [json!({"kind":"record"}), json!({"kind":"proof","index":0})] {
        let mut s = section.clone();
        s["content"][0]["need"] = json!({"record_id":"r","target":target});
        let mut w = ready(&input, &result);
        edit(&mut w, &input, &result, "put_section", s);
        assert!(compiler::compile(&input, &result, &w.draft, 1_000_000).is_err());
    }
    // Even grounded sample text and fixed template cells must use template copy policies.
    for part in [
        json!({"kind":"text","source_id":"s0","start":0,"end":6}),
        json!({"kind":"grid_cell","source_id":"s0","form_id":"f0","row":0,"column":0}),
    ] {
        let mut result = result.clone();
        if part["kind"] == "grid_cell" {
            let citation: Span = serde_json::from_value(json!({"source_id":"s0","start":0,"end":0,
                "grid_cell":{"form_id":"f0","row":0,"column":0}}))
            .unwrap();
            let record = result.analysis.records.get_mut("r").unwrap();
            record.sources.push(citation.clone());
            if let RecordData::Requirement { response, .. } = &mut record.data {
                response[0].grounds.push(citation);
            }
            refresh_excerpt_basis(&input, &mut result);
        }
        let mut s = section.clone();
        s["content"][0]["need"]["record_id"] = json!("r");
        s["content"][0]["paragraphs"] = json!([[part]]);
        let mut w = ready(&input, &result);
        edit(&mut w, &input, &result, "put_section", s);
        w.draft.plan.values_mut().next().unwrap().obligation_refs = obligation_inventory(&result)
            .iter()
            .map(|r| reference_key(r).unwrap())
            .collect();
        let error = compiler::compile(&input, &result, &w.draft, 1_000_000)
            .err()
            .unwrap();
        assert!(error.contains("template region"), "{error}");
    }
}

#[test]
fn source_response_grid_requires_the_exact_cell_in_both_record_and_response_evidence() {
    let (input, result, section) = excerpt_fixture();
    for remove_from_record in [false, true] {
        let mut result = result.clone();
        let record = result.analysis.records.get_mut("excerpt-need").unwrap();
        if remove_from_record {
            record.sources.retain(|s| s.grid_cell.is_none());
        } else if let RecordData::Requirement { response, .. } = &mut record.data {
            response[0].grounds.retain(|s| s.grid_cell.is_none());
        }
        refresh_excerpt_basis(&input, &mut result);
        let mut w = ready(&input, &result);
        edit(&mut w, &input, &result, "put_section", section.clone());
        assert!(
            compiler::compile(&input, &result, &w.draft, 1_000_000)
                .err()
                .unwrap()
                .contains("outside the response obligation's frozen evidence")
        );
    }
}

#[test]
fn source_response_cannot_substitute_for_prescribed_template_or_survive_a_draft_edit() {
    let (input, mut result, section) = excerpt_fixture();
    let mut relation = result.analysis.relations["link0"].clone();
    relation.id = "excerpt-format".into();
    relation.from = "excerpt-need".into();
    result
        .analysis
        .relations
        .insert(relation.id.clone(), relation);
    refresh_excerpt_basis(&input, &mut result);
    let mut w = ready(&input, &result);
    edit(&mut w, &input, &result, "put_section", section.clone());
    assert!(
        compiler::compile(&input, &result, &w.draft, 1_000_000)
            .err()
            .unwrap()
            .contains("must include prescribed template")
    );

    result.analysis.relations.remove("excerpt-format");
    refresh_excerpt_basis(&input, &mut result);
    let mut w = ready(&input, &result);
    let mut section = section;
    // Each paragraph has its own source mapping and all point to one distinct blank.
    section["content"][0]["paragraphs"] = json!([
        [{"kind":"grid_cell","source_id":"e0","form_id":"ef0","row":0,"column":0}],
        [{"kind":"text","source_id":"e1","start":0,"end":input.source_units[3].text.len()}]
    ]);
    edit(&mut w, &input, &result, "put_section", section);
    w.invoke(
        &input,
        &result,
        "compile_docx",
        &json!({}),
        tool_limits(100_000, 1_000_000),
    )
    .unwrap();
    let a = w.artifact.as_ref().unwrap();
    assert_eq!(a.manifest.source_excerpts.len(), 2);
    assert_eq!(
        a.manifest.source_excerpts[0].response_location.bookmark,
        a.manifest.source_excerpts[1].response_location.bookmark
    );
    let saved = w.draft.sections.values().find(|s| s.order == 2).unwrap();
    let mut changed = json!(saved);
    changed["title"] = json!("更新的逐项响应");
    edit(&mut w, &input, &result, "put_section", changed);
    assert!(w.artifact.is_none());
    assert!(!w.done);
}

/// Offline carrier acceptance only. The supplied plan must contain frozen
/// source excerpts, frozen tables and blanks; this never modifies or approves an analysis.
#[test]
#[ignore = "requires explicit frozen source, primitive plan and new output directory"]
fn export_source_excerpt_carrier() {
    use crate::docx_template::{TemplatePlan, compile_template};
    use std::{io::Write, path::PathBuf};
    let read = |name| -> Value {
        serde_json::from_slice(&std::fs::read(std::env::var(name).unwrap()).unwrap()).unwrap()
    };
    let input = read("KB_EXCERPT_FROZEN_INPUT");
    let fixture = read("KB_EXCERPT_CARRIER_PLAN");
    let plan: TemplatePlan = serde_json::from_value(fixture["plan"].clone()).unwrap();
    assert!(
        plan.sections
            .iter()
            .flat_map(|s| &s.blocks)
            .all(|b| matches!(b.kind.as_str(), "source_excerpt" | "blank" | "table"))
    );
    let docx = compile_template(&input, &plan).unwrap();
    let rendered = document::verify(&docx, &input, &plan).unwrap();
    let frozen: FrozenInput = serde_json::from_value(input.clone()).unwrap();
    let mut analysis = Analysis::default();
    let mut coverage = Coverage::default();
    let mut source_citations = vec![];
    for part in plan
        .sections
        .iter()
        .flat_map(|s| &s.blocks)
        .flat_map(|b| &b.source_parts)
    {
        let citation = match part {
            SourcePart::Text {
                source_id,
                start,
                end,
            } => {
                let read = analysis::tools::invoke(
                    &frozen,
                    &mut analysis,
                    &mut coverage,
                    false,
                    "read_source",
                    &json!({"source_id":source_id,"start":start,"max_bytes":end-start}),
                    1_000_000,
                )
                .unwrap();
                assert_eq!(read["end"], *end);
                Span {
                    source_id: source_id.clone(),
                    start: *start,
                    end: *end,
                    view_id: None,
                    grid_cell: None,
                }
            }
            SourcePart::GridCell {
                source_id,
                form_id,
                row,
                column,
            } => {
                let form = frozen
                    .structured_forms
                    .iter()
                    .find(|f| f["form_definition_revision_id"] == *form_id)
                    .unwrap();
                let columns = form["definition"]["column_count"].as_u64().unwrap() as usize;
                let index = *row * columns + *column;
                let read = analysis::tools::invoke(
                    &frozen,
                    &mut analysis,
                    &mut coverage,
                    false,
                    "read_form",
                    &json!({"form_id":form_id,"offset":index,"limit":1}),
                    1_000_000,
                )
                .unwrap();
                let citation: Span = serde_json::from_value(read["citations"][0].clone()).unwrap();
                assert_eq!(citation.source_id, *source_id);
                citation
            }
        };
        analysis::tools::validate_span(&frozen, &coverage, &citation).unwrap();
        source_citations.push(citation);
    }
    let paragraphs: Vec<_> = rendered
        .iter()
        .filter(|b| b.bookmark.contains("_b"))
        .flat_map(|b| b.paragraphs.clone())
        .collect();
    assert_eq!(json!(paragraphs), fixture["expected_paragraphs"]);
    let output = PathBuf::from(std::env::var("KB_EXCERPT_CARRIER_OUTPUT").unwrap());
    std::fs::create_dir(&output).unwrap();
    let report = json!({"acceptance":"frozen_source_primitive_only","agent_output":false,
        "full_tender_semantic_acceptance":false,"source_sha256":digest(&input).unwrap(),
        "plan_sha256":digest(&plan).unwrap(),"docx_sha256":hex::encode(<sha2::Sha256 as sha2::Digest>::digest(&docx)),
        "source_citations":source_citations,"rendered":rendered});
    for (name, bytes) in [
        ("source-excerpt-carrier.docx", docx),
        (
            "verification.json",
            serde_json::to_vec_pretty(&report).unwrap(),
        ),
    ] {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output.join(name))
            .unwrap()
            .write_all(&bytes)
            .unwrap();
    }
}

#[test]
fn actual_docx_preserves_chapters_dense_merges_notes_and_blank_field_mappings() {
    let (input, result) = fixture();
    let w = ready(&input, &result);
    let output = compiler::compile(&input, &result, &w.draft, 1_000_000).unwrap();
    assert_eq!(output.manifest.status, "needs_review");
    assert_eq!(output.manifest.sections.len(), 2);
    let tables: Vec<_> = output
        .rendered
        .iter()
        .filter_map(|b| b.table.as_ref())
        .collect();
    assert_eq!(tables.len(), 2);
    assert_eq!(tables[0].cells.len(), 6);
    assert_eq!(tables[0].headers, [0]);
    assert!(
        tables[0]
            .cells
            .iter()
            .any(|c| c.row == 1 && c.column == 0 && c.rowspan == 2 && c.text == "设备\n名称")
    );
    assert!(
        tables[0]
            .cells
            .iter()
            .filter(|c| c.column == 2)
            .all(|c| c.text.is_empty())
    );
    assert!(
        output
            .rendered
            .iter()
            .flat_map(|b| &b.paragraphs)
            .any(|p| p == "投标人（盖章）：____ 日期：____")
    );
    let proof = output
        .manifest
        .placements
        .iter()
        .find(|p| {
            p.reference.record_id == "r" && p.reference.target == RelationTarget::Proof { index: 0 }
        })
        .unwrap();
    assert_eq!(proof.location.cell.as_ref().unwrap().row, 2);
    assert_eq!(proof.location.cell.as_ref().unwrap().column, 2);
    assert_eq!(
        compiler::compile(&input, &result, &w.draft, 1_000_000)
            .unwrap()
            .docx,
        output.docx
    );
    if let Ok(dir) = std::env::var("KB_COMPOSITION_ARTIFACT_DIR") {
        let p = std::path::Path::new(&dir);
        std::fs::create_dir_all(p).unwrap();
        std::fs::write(p.join("chapter-template.docx"), &output.docx).unwrap();
        std::fs::write(
            p.join("manifest.json"),
            serde_json::to_vec_pretty(&output.manifest).unwrap(),
        )
        .unwrap();
    }
}
#[test]
fn composition_rejects_unaccounted_needs_wrong_template_and_partial_grid() {
    let (input, result) = fixture();
    let original = ready(&input, &result);
    let first = original
        .draft
        .sections
        .values()
        .find(|s| s.order == 0)
        .unwrap()
        .id
        .clone();
    let mut w = original.clone();
    if let Content::Template { bindings, .. } =
        &mut w.draft.sections.get_mut(&first).unwrap().content[0]
    {
        bindings.retain(|b| !matches!(b.need.target, RelationTarget::Proof { .. }));
    }
    assert!(
        compiler::compile(&input, &result, &w.draft, 1_000_000)
            .unwrap_err_string()
            .contains("dispositions missing")
    );
    let mut w = original.clone();
    if let Content::Template { bindings, .. } =
        &mut w.draft.sections.get_mut(&first).unwrap().content[0]
    {
        bindings.retain(|b| !matches!(b.need.target, RelationTarget::Response { .. }));
    }
    w.draft
        .sections
        .get_mut(&first)
        .unwrap()
        .content
        .push(Content::Placeholder {
            needs: vec![Reference {
                record_id: "r".into(),
                target: RelationTarget::Response { index: 0 },
            }],
        });
    assert!(
        compiler::compile(&input, &result, &w.draft, 1_000_000)
            .unwrap_err_string()
            .contains("prescribed")
    );
    let mut w = original.clone();
    w.draft.presentation.as_mut().unwrap().style.width_mm = 210.0;
    assert!(
        compiler::compile(&input, &result, &w.draft, 1_000_000)
            .unwrap_err_string()
            .contains("page width")
    );
    let mut changed = result.clone();
    if let RecordData::Template { regions, .. } =
        &mut changed.analysis.records.get_mut("t0").unwrap().data
    {
        regions[1].cells.pop();
    }
    assert!(compiler::compile(&input, &changed, &original.draft, 1_000_000).is_err());
}
trait ErrString {
    fn unwrap_err_string(self) -> String;
}
impl ErrString for Result<compiler::Compiled, String> {
    fn unwrap_err_string(self) -> String {
        match self {
            Ok(_) => panic!("expected error"),
            Err(e) => e,
        }
    }
}
#[test]
fn invalidation_and_budget_failures_never_acknowledge_unseen_output() {
    let (input, result) = fixture();
    let mut w = ready(&input, &result);
    let before = digest(&w).unwrap();
    assert!(
        w.invoke(
            &input,
            &result,
            "compile_docx",
            &json!({}),
            tool_limits(5, 1_000_000),
        )
        .is_err()
    );
    assert_eq!(digest(&w).unwrap(), before);
    assert!(
        w.invoke(
            &input,
            &result,
            "compile_docx",
            &json!({}),
            tool_limits(100_000, 1),
        )
        .is_err()
    );
    assert_eq!(digest(&w).unwrap(), before);
    w.invoke(
        &input,
        &result,
        "compile_docx",
        &json!({}),
        tool_limits(100_000, 1_000_000),
    )
    .unwrap();
    let first = w.draft.sections.values().next().unwrap().id.clone();
    let mut s = serde_json::to_value(&w.draft.sections[&first]).unwrap();
    s["title"] = json!("修订章节");
    edit(&mut w, &input, &result, "put_section", s);
    assert!(w.artifact.is_none());
    assert!(
        w.invoke(
            &input,
            &result,
            "request_composition_review",
            &json!({}),
            tool_limits(100_000, 1_000_000),
        )
        .is_err()
    );
}

#[test]
fn composition_review_is_read_only_and_oversized_inspections_do_not_count() {
    let (input, result) = fixture();
    let mut w = ready(&input, &result);
    w.invoke(
        &input,
        &result,
        "compile_docx",
        &json!({}),
        tool_limits(100_000, 1_000_000),
    )
    .unwrap();
    w.invoke(
        &input,
        &result,
        "request_composition_review",
        &json!({}),
        tool_limits(100_000, 1_000_000),
    )
    .unwrap();
    let prior = digest(&w).unwrap();
    assert!(
        w.invoke(
            &input,
            &result,
            "inspect_rendered",
            &json!({"offset":0,"limit":100}),
            tool_limits(5, 1_000_000),
        )
        .is_err()
    );
    assert_eq!(digest(&w).unwrap(), prior);
    assert!(
        w.invoke(
            &input,
            &result,
            "put_section",
            &section(&input, 0),
            tool_limits(100_000, 1_000_000),
        )
        .is_err()
    );
    assert_eq!(digest(&w).unwrap(), prior);
    assert!(
        w.invoke(
            &input,
            &result,
            "submit_composition_review",
            &json!({"findings":[]}),
            tool_limits(100_000, 1_000_000),
        )
        .is_err()
    );
}

fn tool_limits(max_result_bytes: usize, max_docx_bytes: usize) -> tools::ToolLimits {
    tools::ToolLimits {
        max_result_bytes,
        max_docx_bytes,
        max_review_rounds: 3,
    }
}

#[test]
fn text_marked_as_bidder_input_is_not_copied_as_a_filled_response() {
    let (input, mut result) = fixture();
    if let RecordData::Template { regions, .. } =
        &mut result.analysis.records.get_mut("t0").unwrap().data
    {
        regions[0].role = RegionRole::BidderBlank;
    }
    for relation in result.analysis.relations.values_mut() {
        relation.to_record_sha256 = digest(&result.analysis.records[&relation.to]).unwrap();
    }
    let mut coverage = result.analysis.coverage.clone();
    for kind in ["all", "relation", "disposition"] {
        analysis::tools::invoke(
            &input,
            &mut result.analysis,
            &mut coverage,
            true,
            "inspect_analysis",
            &json!({"view":"detail","kind":kind,"offset":0,"limit":100}),
            100_000,
        )
        .unwrap();
    }
    result.review = Review {
        analysis_sha256: digest(&result.analysis).unwrap(),
        coverage,
        findings: vec![],
        ..Default::default()
    };
    seal_review(&input, &mut result);
    let w = ready(&input, &result);
    let compiled = compiler::compile(&input, &result, &w.draft, 1_000_000).unwrap();
    let slot = compiled
        .rendered
        .iter()
        .find(|b| b.bookmark == "kb_s0_b0")
        .unwrap();
    assert_eq!(slot.paragraphs, [String::new()]);
}

#[test]
fn an_old_empty_review_cannot_authorize_overlapping_template_text() {
    let (input, mut result) = fixture();
    let RecordData::Template { regions, .. } =
        &mut result.analysis.records.get_mut("t0").unwrap().data
    else {
        unreachable!()
    };
    let mut overlapping = regions[0].clone();
    overlapping.role = RegionRole::BidderBlank;
    regions.push(overlapping);
    for relation in result.analysis.relations.values_mut() {
        relation.to_record_sha256 = digest(&result.analysis.records[&relation.to]).unwrap();
    }
    let mut coverage = result.analysis.coverage.clone();
    for kind in ["all", "relation", "disposition"] {
        analysis::tools::invoke(
            &input,
            &mut result.analysis,
            &mut coverage,
            true,
            "inspect_analysis",
            &json!({"view":"detail","kind":kind,"offset":0,"limit":100}),
            100_000,
        )
        .unwrap();
    }
    result.review = Review {
        analysis_sha256: digest(&result.analysis).unwrap(),
        coverage,
        findings: vec![],
        ..Default::default()
    };
    seal_review(&input, &mut result);
    assert!(
        analysis::tools::review_gaps(&input, &result.analysis, &result.review.coverage).is_empty()
    );
    let gaps = analysis::tools::gaps(&input, &result.analysis);
    assert_eq!(gaps.len(), 1);
    assert!(gaps[0]["error"].as_str().unwrap().contains("overlap"));
    assert!(validate_basis(&input, &result).is_err());
    assert!(Workspace::new(&input, &result).is_err());
}

#[test]
fn conditional_template_renders_its_reviewed_condition_and_preserves_field_bindings() {
    for state in [
        ApplicabilityState::Conditional,
        ApplicabilityState::Unknown,
        ApplicabilityState::NotApplicable,
    ] {
        let (mut input, mut result) = fixture();
        let condition = "代理商投标时提供授权书；\n授权范围须包括设备 & 服务。";
        let start = input.source_units[0].text.len();
        input.source_units[0].text.push_str(condition);
        let end = input.source_units[0].text.len();
        result
            .analysis
            .coverage
            .text
            .insert("s0".into(), vec![(0, end)]);
        let RecordData::Template { applicability, .. } =
            &mut result.analysis.records.get_mut("t0").unwrap().data
        else {
            panic!("template fixture expected");
        };
        applicability.state = state.clone();
        applicability.condition = condition.into();
        applicability.grounds = vec![Span {
            source_id: "s0".into(),
            start,
            end,
            view_id: None,
            grid_cell: None,
        }];
        result
            .analysis
            .relations
            .get_mut("link0")
            .unwrap()
            .to_record_sha256 = digest(&result.analysis.records["t0"]).unwrap();
        let mut coverage = result.analysis.coverage.clone();
        for kind in ["all", "relation", "disposition"] {
            analysis::tools::invoke(
                &input,
                &mut result.analysis,
                &mut coverage,
                true,
                "inspect_analysis",
                &json!({"view":"detail","kind":kind,"offset":0,"limit":100}),
                100_000,
            )
            .unwrap();
        }
        result.frozen_input_sha256 = digest(&input).unwrap();
        result.review = Review {
            analysis_sha256: digest(&result.analysis).unwrap(),
            coverage,
            findings: vec![],
            ..Default::default()
        };
        result.quality = result.expected_quality(&input).into();
        seal_review(&input, &mut result);
        let w = ready(&input, &result);
        let compiled = compiler::compile(&input, &result, &w.draft, 1_000_000);
        if state != ApplicabilityState::Conditional {
            assert!(
                compiled
                    .err()
                    .expect("invalid applicability must fail")
                    .contains("cannot be placed")
            );
            continue;
        }
        let compiled = compiled.unwrap();
        if let Ok(dir) = std::env::var("KB_COMPOSITION_ARTIFACT_DIR") {
            let path = std::path::Path::new(&dir);
            std::fs::create_dir_all(path).unwrap();
            std::fs::write(path.join("conditional-template.docx"), &compiled.docx).unwrap();
        }
        let annotation = compiled
            .rendered
            .iter()
            .find(|b| b.bookmark == "kb_s0_b0")
            .unwrap();
        assert_eq!(annotation.paragraphs, [format!("适用条件：{condition}")]);
        assert!(annotation.table.is_none());
        let binding = compiled
            .manifest
            .placements
            .iter()
            .find(|p| {
                p.reference.record_id == "r"
                    && p.reference.target == (RelationTarget::Response { index: 0 })
            })
            .unwrap();
        let table = compiled
            .rendered
            .iter()
            .find(|b| b.bookmark == binding.location.bookmark)
            .unwrap()
            .table
            .as_ref()
            .unwrap();
        assert_eq!(table.rows, 3);
        assert!(
            table
                .cells
                .iter()
                .find(|c| c.row == 1 && c.column == 2)
                .unwrap()
                .text
                .is_empty()
        );
        assert_eq!(compiled.manifest.status, "needs_review");
    }
}

#[test]
fn conditional_template_alternatives_require_explicit_reviewable_disposition() {
    let (mut input, mut result) = fixture();
    let start = input.source_units[0].text.len();
    input.source_units[0]
        .text
        .push_str("\n技术响应可使用附表甲，或使用报价表中的相应位置，二者择一。");
    let end = input.source_units[0].text.len();
    result
        .analysis
        .coverage
        .text
        .insert("s0".into(), vec![(0, end)]);
    let grounds = vec![Span {
        source_id: "s0".into(),
        start,
        end,
        view_id: None,
        grid_cell: None,
    }];
    let mut alternate = result.analysis.relations["link1"].clone();
    alternate.id = "alternate".into();
    alternate.from_target = RelationTarget::Response { index: 0 };
    alternate.grounds = grounds.clone();
    alternate.explanation = "原文允许的替代格式".into();
    result
        .analysis
        .relations
        .insert(alternate.id.clone(), alternate);
    let mut coverage = result.analysis.coverage.clone();
    for kind in ["all", "relation", "disposition"] {
        analysis::tools::invoke(
            &input,
            &mut result.analysis,
            &mut coverage,
            true,
            "inspect_analysis",
            &json!({"view":"detail","kind":kind,"offset":0,"limit":100}),
            100_000,
        )
        .unwrap();
    }
    result.frozen_input_sha256 = digest(&input).unwrap();
    result.review = Review {
        analysis_sha256: digest(&result.analysis).unwrap(),
        coverage,
        findings: vec![],
        ..Default::default()
    };
    seal_review(&input, &mut result);
    let mut w = ready(&input, &result);
    assert!(compiler::compile(&input, &result, &w.draft, 1_000_000).is_err());
    edit(
        &mut w,
        &input,
        &result,
        "omit_template_relation",
        json!({"relation_id":"alternate","reason":"采用原文允许的附表甲，保留该替代格式未选用的依据","grounds":grounds}),
    );
    let compiled = compiler::compile(&input, &result, &w.draft, 1_000_000).unwrap();
    assert_eq!(compiled.manifest.relation_omissions.len(), 1);
    assert_eq!(compiled.manifest.status, "needs_review");
}

#[test]
fn put_composition_review_requires_active_review_and_known_chapter() {
    let (input, result) = fixture();
    let mut w = ready(&input, &result);
    let err = w
        .invoke(
            &input,
            &result,
            "put_composition_review",
            &json!({"item_id":"missing","conclusion":"pass","grounds":[]}),
            tool_limits(100_000, 1_000_000),
        )
        .unwrap_err();
    assert!(err.contains("independent review is not active"), "{err}");
    w.invoke(
        &input,
        &result,
        "compile_docx",
        &json!({}),
        tool_limits(100_000, 1_000_000),
    )
    .unwrap();
    w.invoke(
        &input,
        &result,
        "request_composition_review",
        &json!({}),
        tool_limits(100_000, 1_000_000),
    )
    .unwrap();
    let err = w
        .invoke(
            &input,
            &result,
            "put_composition_review",
            &json!({"item_id":"missing","conclusion":"pass","grounds":[]}),
            tool_limits(100_000, 1_000_000),
        )
        .unwrap_err();
    assert!(err.contains("unknown plan item"), "{err}");
    let id = w.draft.sections.keys().next().unwrap().clone();
    w.invoke(
        &input,
        &result,
        "inspect_composition",
        &json!({"offset":0,"limit":100}),
        tool_limits(100_000, 1_000_000),
    )
    .unwrap();
    w.invoke(
        &input,
        &result,
        "read_source",
        &json!({"source_id":"s0","start":0,"max_bytes":10000}),
        tool_limits(100_000, 1_000_000),
    )
    .unwrap();
    w.invoke(
        &input,
        &result,
        "put_composition_review",
        &json!({"item_id":id,"conclusion":"pass","grounds":[{"source_id":"s0","start":0,"end":input.source_units[0].text.len()}]}),
        tool_limits(100_000, 1_000_000),
    )
    .unwrap();
    assert_eq!(
        w.plan_reviews[&id].conclusion,
        tools::PlanReviewConclusion::Pass
    );
}

#[test]
fn plan_complete_requires_every_obligation_or_exception() {
    let (input, result) = fixture();
    let mut w = ready(&input, &result);
    w.draft.plan.clear();
    assert!(!plan_complete(&result, &w.draft).unwrap());
    for r in obligation_inventory(&result) {
        let key = reference_key(&r).unwrap();
        w.draft.plan.insert(
            key.clone(),
            PlanItem {
                id: key.clone(),
                kind: PlanItemKind::Section,
                parent: None,
                order: w.draft.plan.len(),
                title: "义务".into(),
                placement: Default::default(),
                prescribed: true,
                grounds: vec![Span {
                    source_id: input.source_units[0].source_unit_revision_id.clone(),
                    start: 0,
                    end: input.source_units[0].text.len(),
                    view_id: None,
                    grid_cell: None,
                }],
                obligation_refs: vec![key.clone()],
                exception: None,
            },
        );
    }
    assert!(plan_complete(&result, &w.draft).unwrap());
    assert!(!implementation_complete(&w.draft, None));
}

#[test]
fn required_references_include_each_rule_item() {
    let (_, mut result) = fixture();
    result.analysis.records.insert(
        "rule".into(),
        Record {
            id: "rule".into(),
            sources: vec![],
            data: RecordData::Rule {
                text: "投标函在前".into(),
                scope: "本次".into(),
                applicability: Applicability {
                    state: ApplicabilityState::Applicable,
                    condition: "按须知".into(),
                    scope: "本次".into(),
                    grounds: vec![],
                },
                items: vec![RuleItem {
                    id: "i1".into(),
                    kind: RuleItemKind::Composition,
                    text: "投标函".into(),
                    grounds: vec![],
                    condition: String::new(),
                    targets: vec![],
                    sequence: vec![],
                    format_key: None,
                    format_value: None,
                }],
            },
        },
    );
    assert!(compiler::required_references(&result).iter().any(|r| {
        r.record_id == "rule"
            && r.target
                == RelationTarget::RuleItem {
                    item_id: "i1".into(),
                }
    }));
}

#[test]
fn compiled_docx_contains_the_rendered_paragraphs() {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    let (input, result) = fixture();
    let mut w = ready(&input, &result);
    w.invoke(
        &input,
        &result,
        "compile_docx",
        &json!({}),
        tool_limits(100_000, 1_000_000),
    )
    .unwrap();
    let artifact = w.artifact.as_ref().unwrap();
    let bytes = STANDARD.decode(&artifact.docx_base64).unwrap();
    // File assertion only. Full output inventories are produced by DocReader.
    use std::io::Read;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    let actual: String = document
        .descendants()
        .filter(|node| {
            node.has_tag_name((
                "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
                "t",
            ))
        })
        .filter_map(|node| node.text())
        .collect();
    for block in &artifact.rendered {
        for paragraph in &block.paragraphs {
            if paragraph.trim().is_empty() {
                continue;
            }
            assert!(
                actual.contains(paragraph),
                "rendered {paragraph:?} missing from actual DOCX"
            );
        }
    }
}

#[test]
fn planning_gate_rejects_unplanned_compile() {
    let (input, result) = fixture();
    let mut w = ready(&input, &result);
    w.draft.plan.clear();
    assert!(
        w.invoke(
            &input,
            &result,
            "compile_docx",
            &json!({}),
            tool_limits(100_000, 1_000_000)
        )
        .is_err()
    );
}

#[test]
fn planning_gate_rejects_unknown_obligation() {
    let (input, result) = fixture();
    let mut w = ready(&input, &result);
    let args = json!({"id":"plan-invalid","kind":"section","parent":null,"order":99,"title":"bad","prescribed":true,"grounds":[{"source_id":"s0","start":0,"end":3}],"obligation_refs":["invented"],"expected_draft_sha256":digest(&w.draft).unwrap()});
    assert!(
        w.invoke(
            &input,
            &result,
            "put_composition_plan_item",
            &args,
            tool_limits(100_000, 1_000_000)
        )
        .is_err()
    );
}

#[test]
fn planning_gate_rejects_section_order_divergence() {
    let (input, result) = fixture();
    let mut w = ready(&input, &result);
    let id = w.draft.sections.keys().next().unwrap().clone();
    let mut args = json!(w.draft.sections[&id]);
    args["order"] = json!(99);
    args["expected_draft_sha256"] = json!(digest(&w.draft).unwrap());
    assert!(
        w.invoke(
            &input,
            &result,
            "put_section",
            &args,
            tool_limits(100_000, 1_000_000)
        )
        .is_err()
    );
}

fn review_workspace(input: &FrozenInput, result: &AnalysisResult) -> Workspace {
    let mut w = ready(input, result);
    for name in ["compile_docx", "request_composition_review"] {
        w.invoke(
            input,
            result,
            name,
            &json!({}),
            tool_limits(100_000, 1_000_000),
        )
        .unwrap();
    }
    for i in 0..2 {
        w.invoke(
            input,
            result,
            "read_source",
            &json!({"source_id":format!("s{i}"),"start":0,"max_bytes":10000}),
            tool_limits(100_000, 1_000_000),
        )
        .unwrap();
        w.invoke(
            input,
            result,
            "read_form",
            &json!({"form_id":format!("f{i}"),"offset":0,"limit":100}),
            tool_limits(100_000, 1_000_000),
        )
        .unwrap();
    }
    for kind in ["all", "relation", "disposition"] {
        w.invoke(
            input,
            result,
            "inspect_analysis",
            &json!({"view":"detail","kind":kind,"offset":0,"limit":100}),
            tool_limits(100_000, 1_000_000),
        )
        .unwrap();
    }
    for name in [
        "inspect_composition",
        "inspect_rendered",
        "inspect_placements",
    ] {
        w.invoke(
            input,
            result,
            name,
            &json!({"offset":0,"limit":100}),
            tool_limits(100_000, 1_000_000),
        )
        .unwrap();
    }
    let bookmarks: Vec<_> = w
        .artifact
        .as_ref()
        .unwrap()
        .rendered
        .iter()
        .filter(|b| b.table.is_some())
        .map(|b| b.bookmark.clone())
        .collect();
    for bookmark in bookmarks {
        w.invoke(
            input,
            result,
            "inspect_rendered_cells",
            &json!({"bookmark":bookmark,"offset":0,"limit":100}),
            tool_limits(100_000, 1_000_000),
        )
        .unwrap();
    }
    w
}
fn plan_judgment(input: &FrozenInput, i: usize) -> Value {
    json!({"item_id":format!("fixture-section-{i}"),"conclusion":"pass","grounds":[{"source_id":format!("s{i}"),"start":0,"end":input.source_units[i].text.len()}]})
}
#[test]
fn plan_review_rejects_unread_original_and_unknown_finding() {
    let (input, result) = fixture();
    let mut w = review_workspace(&input, &result);
    w.review_coverage.text.clear();
    let before = digest(&w).unwrap();
    assert!(
        w.invoke(
            &input,
            &result,
            "put_composition_review",
            &plan_judgment(&input, 0),
            tool_limits(100_000, 1_000_000)
        )
        .is_err()
    );
    assert_eq!(digest(&w).unwrap(), before);
    w.review_coverage = result.review.coverage.clone();
    let mut args = plan_judgment(&input, 0);
    args["conclusion"] = json!("findings");
    args["finding_ids"] = json!(["unknown-finding"]);
    assert!(
        w.invoke(
            &input,
            &result,
            "put_composition_review",
            &args,
            tool_limits(100_000, 1_000_000)
        )
        .unwrap_err()
        .contains("unknown saved")
    );
}
#[test]
fn saved_plan_findings_survive_empty_aggregate_submission() {
    let (input, result) = fixture();
    let mut w = review_workspace(&input, &result);
    let mut args = plan_judgment(&input, 0);
    args["conclusion"] = json!("findings");
    args["findings"] = json!([{"message":"原文与成稿字段不一致","section_ids":["fixture-section-0"],"record_ids":["t0"],"sources":args["grounds"]}]);
    w.invoke(
        &input,
        &result,
        "put_composition_review",
        &args,
        tool_limits(100_000, 1_000_000),
    )
    .unwrap();
    w.invoke(
        &input,
        &result,
        "put_composition_review",
        &plan_judgment(&input, 1),
        tool_limits(100_000, 1_000_000),
    )
    .unwrap();
    assert!(w.review_gaps(&input, &result).unwrap().is_empty());
    let out = w
        .invoke(
            &input,
            &result,
            "submit_composition_review",
            &json!({"findings":[]}),
            tool_limits(100_000, 1_000_000),
        )
        .unwrap();
    assert_eq!(out["done"], false);
    assert_eq!(w.findings.len(), 1);
    assert!(!w.done);
}
#[test]
fn current_plan_judgments_are_required_after_recompilation() {
    let (input, result) = fixture();
    let mut w = review_workspace(&input, &result);
    for i in 0..2 {
        w.invoke(
            &input,
            &result,
            "put_composition_review",
            &plan_judgment(&input, i),
            tool_limits(100_000, 1_000_000),
        )
        .unwrap();
    }
    assert!(w.validate_plan_reviews(&input, &result, true).is_ok());
    w.plan_reviews
        .get_mut("fixture-section-0")
        .unwrap()
        .draft_sha256 = "old-draft".into();
    assert!(
        w.invoke(
            &input,
            &result,
            "submit_composition_review",
            &json!({"findings":[]}),
            tool_limits(100_000, 1_000_000)
        )
        .is_err()
    );
    assert!(!w.done);
}
#[test]
fn plan_item_requires_main_original_receipt_and_v2_basis() {
    let (input, mut result) = fixture();
    let mut w = Workspace::new(&input, &result).unwrap();
    let mut args = plan_for_section(&result, &section(&input, 0));
    args["expected_draft_sha256"] = json!(digest(&w.draft).unwrap());
    assert!(
        w.invoke(
            &input,
            &result,
            "put_composition_plan_item",
            &args,
            tool_limits(100_000, 1_000_000)
        )
        .is_err()
    );
    result.schema_version = 1;
    assert!(Workspace::new(&input, &result).is_err());
}

fn rule_fixture(
    kind: &str,
    property: Option<&str>,
    value: Option<&str>,
) -> (FrozenInput, AnalysisResult) {
    let (input, mut result) = fixture();
    let grounds = result.analysis.records["t0"].sources.clone();
    let record: Record = serde_json::from_value(json!({"id":"rule","sources":grounds,"data":{
        "kind":"rule","text":"本项目编制规则","scope":"本次投标","applicability":{"state":"applicable","scope":"本项目","condition":"按原文","grounds":grounds},
        "items":[{"id":"rule-item","kind":kind,"text":"明确原文规则","grounds":grounds,"condition":"本次投标","targets":[{"kind":"record","id":"t0"}],"sequence":[],"format_key":property,"format_value":value}]
    }})).unwrap();
    result.analysis.records.insert(record.id.clone(), record);
    refresh_excerpt_basis(&input, &mut result);
    (input, result)
}
fn rule_reference() -> Reference {
    Reference {
        record_id: "rule".into(),
        target: RelationTarget::RuleItem {
            item_id: "rule-item".into(),
        },
    }
}

#[test]
fn section_rule_implementation_records_actual_bookmark_and_target_dependencies() {
    let (input, result) = rule_fixture("composition", None, None);
    let mut w = ready(&input, &result);
    let key = reference_key(&rule_reference()).unwrap();
    w.draft
        .plan
        .get_mut("fixture-section-0")
        .unwrap()
        .obligation_refs
        .push(key);
    let compiled = compiler::compile(&input, &result, &w.draft, 1_000_000).unwrap();
    let implementation = &compiled.manifest.rule_implementations[0];
    assert_eq!(implementation.reference, rule_reference());
    let compiler::RuleImplementationTarget::Section {
        location,
        dependencies,
    } = &implementation.implementation
    else {
        panic!("real section location required")
    };
    assert_eq!(location.section_id, "fixture-section-0");
    assert!(
        compiled
            .manifest
            .sections
            .iter()
            .any(|s| s.bookmark == location.bookmark)
    );
    assert!(dependencies.iter().any(|d| d.reference.record_id == "t0"));
}

#[test]
fn format_rule_requires_actual_style_property_not_a_chapter_bookmark() {
    let (input, result) = rule_fixture("format", Some("font_family"), Some("Noto Sans CJK SC"));
    let mut w = ready(&input, &result);
    let key = reference_key(&rule_reference()).unwrap();
    w.draft
        .plan
        .get_mut("fixture-section-0")
        .unwrap()
        .obligation_refs
        .push(key.clone());
    assert!(
        compiler::compile(&input, &result, &w.draft, 1_000_000)
            .err()
            .unwrap()
            .contains("actual presentation")
    );
    w.draft
        .plan
        .get_mut("fixture-section-0")
        .unwrap()
        .obligation_refs
        .retain(|r| r != &key);
    edit(
        &mut w,
        &input,
        &result,
        "put_composition_plan_item",
        json!({"id":"presentation-plan","kind":"presentation","parent":null,"order":0,"title":"投标模板","prescribed":true,"grounds":result.analysis.records["rule"].sources,"obligation_refs":[key]}),
    );
    let compiled = compiler::compile(&input, &result, &w.draft, 1_000_000).unwrap();
    assert!(implementation_complete(&w.draft, Some(&compiled)));
    let compiler::RuleImplementationTarget::Presentation { property, value } =
        &compiled.manifest.rule_implementations[0].implementation
    else {
        panic!("explicit style dependency required")
    };
    assert_eq!(property, "font_family");
    assert_eq!(value, "Noto Sans CJK SC");
    w.draft.presentation.as_mut().unwrap().style.font_family = "Different Font".into();
    assert!(
        compiler::compile(&input, &result, &w.draft, 1_000_000)
            .err()
            .unwrap()
            .contains("does not match")
    );
}

#[test]
fn report_rule_requires_a_real_matching_omission() {
    let (input, result) = rule_fixture("submission_hint", None, None);
    let mut w = ready(&input, &result);
    let key = reference_key(&rule_reference()).unwrap();
    let grounds = result.analysis.records["rule"].sources.clone();
    edit(
        &mut w,
        &input,
        &result,
        "put_composition_plan_item",
        json!({"id":"submission-report","kind":"report_note","parent":null,"order":0,"title":"递交说明","prescribed":true,"grounds":grounds,"obligation_refs":[key],"exception":"保留在报告，递交操作不生成正文"}),
    );
    assert!(
        compiler::compile(&input, &result, &w.draft, 1_000_000)
            .err()
            .unwrap()
            .contains("actual omission")
    );
    edit(
        &mut w,
        &input,
        &result,
        "put_omission",
        json!({"reference":rule_reference(),"reason":"保留在报告，递交操作不生成正文","grounds":grounds}),
    );
    let compiled = compiler::compile(&input, &result, &w.draft, 1_000_000).unwrap();
    assert!(implementation_complete(&w.draft, Some(&compiled)));
    assert!(matches!(
        compiled.manifest.rule_implementations[0].implementation,
        compiler::RuleImplementationTarget::ReportNote { .. }
    ));
    w.draft.plan.get_mut("submission-report").unwrap().exception = Some("不同的未实施理由".into());
    assert!(compiler::compile(&input, &result, &w.draft, 1_000_000).is_err());
}

#[test]
fn unsupported_format_property_is_an_implementation_error() {
    let (input, result) = rule_fixture("format", Some("unknown_style_property"), Some("value"));
    let mut w = ready(&input, &result);
    edit(
        &mut w,
        &input,
        &result,
        "put_composition_plan_item",
        json!({"id":"presentation-plan","kind":"presentation","parent":null,"order":0,"title":"投标模板","prescribed":true,"grounds":result.analysis.records["rule"].sources,"obligation_refs":[reference_key(&rule_reference()).unwrap()]}),
    );
    let error = compiler::compile(&input, &result, &w.draft, 1_000_000)
        .err()
        .unwrap();
    assert!(error.contains("not supported by TemplateStyle"), "{error}");
}

#[test]
fn implementation_requires_obligations_at_the_planned_section() {
    let (input, result) = fixture();
    let mut w = ready(&input, &result);
    let first = w.draft.plan["fixture-section-0"].obligation_refs.clone();
    let second = w.draft.plan["fixture-section-1"].obligation_refs.clone();
    w.draft
        .plan
        .get_mut("fixture-section-0")
        .unwrap()
        .obligation_refs = second;
    w.draft
        .plan
        .get_mut("fixture-section-1")
        .unwrap()
        .obligation_refs = first;
    assert!(plan_complete(&result, &w.draft).unwrap());
    let compiled = compiler::compile(&input, &result, &w.draft, 1_000_000).unwrap();
    assert!(!implementation_complete(&w.draft, Some(&compiled)));
    assert!(
        w.invoke(
            &input,
            &result,
            "compile_docx",
            &json!({}),
            tool_limits(100_000, 1_000_000)
        )
        .unwrap_err()
        .contains("implementation is incomplete")
    );
}

#[test]
fn composition_review_rejects_orphaned_plan_judgment() {
    let (input, result) = fixture();
    let mut workspace = review_workspace(&input, &result);
    for i in 0..2 {
        workspace
            .invoke(
                &input,
                &result,
                "put_composition_review",
                &plan_judgment(&input, i),
                tool_limits(100000, 1000000),
            )
            .unwrap();
    }
    let mut orphan = workspace.plan_reviews["fixture-section-0"].clone();
    orphan.item_id = "deleted-plan".into();
    workspace
        .plan_reviews
        .insert(orphan.item_id.clone(), orphan);
    assert!(
        workspace
            .validate_plan_reviews(&input, &result, true)
            .unwrap_err()
            .contains("unknown plan item")
    );
}

#[test]
fn preserved_paragraphs_and_table_round_trip_from_the_readback_receipt() {
    use crate::docx_template::{TemplatePlan, compile_template};
    let (source, plan) = input_for_order_verification();
    let mut source = source;
    let mut plan = plan;
    source["preserved_units"] = json!({
        "story:7":{"kind":"paragraphs","unit_keys":["story:7","story:8"],
            "paragraphs":["我方郑重承诺如下。","按招标文件要求提供全部资料。"]},
        "story:9":{"kind":"table","unit_key":"story:9","row_count":2,"column_count":2,"widths_twips":[2000,4000],"header_rows":1,
            "cells":[{"row":0,"column":0,"row_span":1,"col_span":1,"text":"项目"},
                     {"row":0,"column":1,"row_span":1,"col_span":1,"text":"报价"},
                     {"row":1,"column":0,"row_span":1,"col_span":1,"text":"总价"},
                     {"row":1,"column":1,"row_span":1,"col_span":1,"text":"壹万元整"}]}
    });
    let paragraphs = json!({"kind":"preserved_paragraphs","source_id":null,"quote":null,"form_id":null,
        "header_rows":0,"blank_cells":[],"columns":[],"blank_rows":0,"preserved_key":"story:7"});
    let table = json!({"kind":"preserved_table","source_id":null,"quote":null,"form_id":null,
        "header_rows":0,"blank_cells":[],"columns":[],"blank_rows":0,"preserved_key":"story:9"});
    plan["sections"][1]["blocks"] = json!([paragraphs, table]);
    let good: TemplatePlan = serde_json::from_value(plan.clone()).unwrap();
    let bytes = compile_template(&source, &good).unwrap();
    let rendered = document::verify(&bytes, &source, &good).unwrap();
    let xml = document_xml(&bytes);
    assert!(xml.contains("我方郑重承诺如下。"), "保留段落必须在文档里");
    assert!(xml.contains("壹万元整"), "用户手填的报价必须原样回去");
    assert!(xml.contains("<w:gridCol w:w=\"2000\"/>") && xml.contains("<w:gridCol w:w=\"4000\"/>"));
    assert!(xml.contains("<w:tblHeader/>"));
    assert!(
        rendered.iter().any(|block| block.table.is_some()),
        "保留表格必须渲染成表格"
    );
    // 收据缺失、kind 不符、块自带正文字段都必须编译失败，而不是渲染出无出处的文本。
    let mut orphan = plan.clone();
    orphan["sections"][1]["blocks"][0]["preserved_key"] = json!("story:404");
    let orphan: TemplatePlan = serde_json::from_value(orphan).unwrap();
    let err = compile_template(&source, &orphan).unwrap_err();
    assert!(format!("{err:?}").contains("readback receipt"), "{err:?}");
    let mut swapped = plan.clone();
    swapped["sections"][1]["blocks"][0]["preserved_key"] = json!("story:9");
    let swapped: TemplatePlan = serde_json::from_value(swapped).unwrap();
    assert!(
        compile_template(&source, &swapped).is_err(),
        "kind 必须对上"
    );
    let mut inline = plan.clone();
    inline["sections"][1]["blocks"][1]["header_rows"] = json!(1);
    let inline: TemplatePlan = serde_json::from_value(inline).unwrap();
    assert!(
        compile_template(&source, &inline).is_err(),
        "保留表格不接受额外表字段"
    );
    let mut mislabeled = plan.clone();
    mislabeled["sections"][1]["blocks"][0]["kind"] = json!("blank");
    let mislabeled: TemplatePlan = serde_json::from_value(mislabeled).unwrap();
    assert!(
        compile_template(&source, &mislabeled).is_err(),
        "收据 key 不得挂在别的原语上"
    );
}
