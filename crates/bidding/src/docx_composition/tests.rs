use super::tools::Workspace;
use super::*;
use crate::tender_analysis as analysis;
use serde_json::{Value, json};
use std::{collections::VecDeque, sync::Mutex};

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
        input.structured_forms.push(json!({"form_definition_revision_id":format!("f{i}"),"source_unit_revision_id":format!("s{i}"),"definition":{"kind":"grid","row_count":3,"column_count":3,"widths_mm":[100,70,50],"cells":cells}}));
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
                cells: vec![],
                blank_ranges: vec![],
                instruction: "保留声明".into(),
            },
            TemplateRegion {
                source: span(0, cut),
                role: RegionRole::FixedText,
                form_id: Some(format!("f{i}")),
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
                cells: vec![Cell { row: 1, column: 2 }, Cell { row: 2, column: 2 }],
                blank_ranges: vec![],
                instruction: "投标方后续填写".into(),
            },
            TemplateRegion {
                source: span(cut + 1, text.len()),
                role: RegionRole::Instruction,
                form_id: None,
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
    let result = AnalysisResult {
        schema_version: 1,
        frozen_input_sha256: digest(&input).unwrap(),
        review: Review {
            analysis_sha256: digest(&a).unwrap(),
            coverage,
            findings: vec![],
        },
        analysis: a,
        quality: "verified".into(),
        source_views: BTreeMap::new(),
    };
    (input, result)
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
    json!({"id":null,"parent":null,"order":i,"title":if i==0{"技术响应"}else{"报价附件"},"grounds":[{"source_id":format!("s{i}"),"start":0,"end":input.source_units[i].text.len()}],"content":[{"kind":"template","record_id":format!("t{i}"),"headers":[{"form_id":format!("f{i}"),"header_rows":1}],"bindings":bindings}]})
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
    args["expected_draft_sha256"] = json!(digest(&w.draft).unwrap());
    w.invoke(input, result, name, &args, tool_limits(100_000, 1_000_000))
        .unwrap()
}
fn ready(input: &FrozenInput, result: &AnalysisResult) -> Workspace {
    let mut w = Workspace::new(input, result).unwrap();
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

#[test]
fn put_section_rejects_template_record_as_bidder_need() {
    let (input, result) = fixture();
    let mut w = Workspace::new(&input, &result).unwrap();
    edit(
        &mut w,
        &input,
        &result,
        "set_presentation",
        presentation(&input),
    );
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
        w.draft.sections.is_empty(),
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
fn front_matter_preserves_template_fields_before_toc_without_body_heading_style() {
    let (input, result) = fixture();
    let mut w = ready(&input, &result);
    let first = w.draft.sections.values().find(|s| s.order == 0).unwrap();
    let mut changed = json!(first);
    changed["placement"] = json!("front_matter");
    changed["title"] = json!("封面结构测试");
    let schema = agent::schemas(false)
        .into_iter()
        .find(|t| t["function"]["name"] == "put_section")
        .unwrap();
    let mut args = changed.clone();
    args["expected_draft_sha256"] = json!(digest(&w.draft).unwrap());
    assert!(
        jsonschema::JSONSchema::compile(&schema["function"]["parameters"])
            .unwrap()
            .is_valid(&args)
    );
    edit(&mut w, &input, &result, "put_section", changed);
    let output = compiler::compile(&input, &result, &w.draft, 1_000_000).unwrap();
    assert_eq!(output.rendered[0].paragraph_styles, [Some("Title".into())]);
    assert_eq!(
        output
            .rendered
            .iter()
            .find(|b| b.bookmark == "kb_s1")
            .unwrap()
            .paragraph_styles,
        [Some("Heading1".into())]
    );
    let xml = document_xml(&output.docx);
    let doc = roxmltree::Document::parse(&xml).unwrap();
    let ns = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
    let body = doc
        .descendants()
        .find(|n| n.has_tag_name((ns, "body")))
        .unwrap();
    let nodes: Vec<_> = body.children().filter(|n| n.is_element()).collect();
    let toc = nodes
        .iter()
        .position(|n| n.descendants().any(|c| c.has_tag_name((ns, "instrText"))))
        .unwrap();
    let end = toc
        + nodes[toc..]
            .iter()
            .position(|n| {
                n.descendants().any(|c| {
                    c.has_tag_name((ns, "fldChar"))
                        && c.attribute((ns, "fldCharType")) == Some("end")
                })
            })
            .unwrap();
    let cached: Vec<_> = nodes[toc + 1..end]
        .iter()
        .flat_map(|n| {
            n.descendants()
                .filter(|c| c.has_tag_name((ns, "t")))
                .map(|c| c.text().unwrap())
        })
        .collect();
    assert_eq!(cached, ["报价附件"]);
    let heading_position = |name| {
        nodes
            .iter()
            .position(|n| n.attribute((ns, "name")) == Some(name))
            .unwrap()
    };
    assert!(heading_position("kb_s0") < toc && heading_position("kb_s1") > end);
    assert!(
        output
            .manifest
            .placements
            .iter()
            .any(|p| p.reference.record_id == "r" && p.location.bookmark == "kb_s0_b1")
    );
    assert_eq!(
        output.rendered.iter().filter(|b| b.table.is_some()).count(),
        2
    );
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
    };
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
            "definition":{"kind":"grid","row_count":1,"column_count":2,"widths_mm":[40,40],
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
fn source_response_preserves_cross_page_wording_and_separate_empty_response() {
    let (input, result, section) = excerpt_fixture();
    let schema = agent::schemas(false)
        .into_iter()
        .find(|t| t["function"]["name"] == "put_section")
        .unwrap();
    let mut args = section.clone();
    args["expected_draft_sha256"] = json!("0".repeat(64));
    assert!(
        jsonschema::JSONSchema::compile(&schema["function"]["parameters"])
            .unwrap()
            .is_valid(&args)
    );
    let mut w = ready(&input, &result);
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
    let excerpt = &a.manifest.source_excerpts[0];
    assert_eq!(
        a.rendered
            .iter()
            .find(|b| b.bookmark == excerpt.location.bookmark)
            .unwrap()
            .paragraphs,
        ["条款甲：须逐项响应。\n需提供承诺及说明。"]
    );
    assert_eq!(
        a.rendered
            .iter()
            .find(|b| b.bookmark == excerpt.response_location.bookmark)
            .unwrap()
            .paragraphs,
        [""]
    );
    let placements: Vec<_> = a
        .manifest
        .placements
        .iter()
        .filter(|p| p.reference.record_id == "excerpt-need")
        .collect();
    assert_eq!(placements.len(), 1);
    assert_eq!(
        placements[0].location.bookmark,
        excerpt.response_location.bookmark
    );
    assert_ne!(
        excerpt.location.bookmark,
        excerpt.response_location.bookmark
    );
    w.reviewing = true;
    assert!(
        w.review_gaps(&input, &result)
            .unwrap()
            .iter()
            .any(|g| g["key"] == "source_excerpt:0")
    );
    w.invoke(
        &input,
        &result,
        "inspect_composition",
        &json!({"offset":0,"limit":100}),
        tool_limits(100_000, 1_000_000),
    )
    .unwrap();
    assert!(
        !w.review_gaps(&input, &result)
            .unwrap()
            .iter()
            .any(|g| g["key"] == "source_excerpt:0")
    );
    w.artifact.as_mut().unwrap().manifest.source_excerpts[0]
        .response_location
        .bookmark = "wrong".into();
    assert!(
        w.review_gaps(&input, &result)
            .unwrap()
            .iter()
            .any(|g| g["key"] == "source_excerpt:0")
    );
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
        assert!(
            compiler::compile(&input, &result, &w.draft, 1_000_000)
                .err()
                .unwrap()
                .contains("template region")
        );
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

#[derive(Default)]
struct Journal {
    sdk_turns: Mutex<Vec<usize>>,
    fail_boundary_ack: Mutex<Option<usize>>,
    reject_reservation: Mutex<bool>,
    state: Mutex<Option<Value>>,
    bodies: Mutex<BTreeMap<usize, Vec<u8>>>,
    calls: Mutex<usize>,
    fail_ack_at: Mutex<Option<usize>>,
}
#[async_trait::async_trait]
impl agent::Journal for Journal {
    async fn load(&self) -> Result<Option<agent::Checkpoint>, crate::agent_error::AgentError> {
        Ok(self
            .state
            .lock()
            .unwrap()
            .clone()
            .map(|v| serde_json::from_value(v).unwrap()))
    }
    async fn reserve(
        &self,
        state: &agent::Checkpoint,
        body: &[u8],
    ) -> Result<usize, crate::agent_error::AgentError> {
        if *self.reject_reservation.lock().unwrap() {
            return Err(crate::agent_error::AgentError::new(
                "INTERNAL",
                "reservation transaction rejected",
            ));
        }
        self.sdk_turns
            .lock()
            .unwrap()
            .push(state.journal.session.as_ref().unwrap().turn());
        let turn = state.turn;
        let mut bodies = self.bodies.lock().unwrap();
        if let Some(prior) = bodies.get(&turn) {
            assert_eq!(prior, body);
        } else {
            bodies.insert(turn, body.to_vec());
        }
        let mut calls = self.calls.lock().unwrap();
        *calls += 1;
        *self.state.lock().unwrap() = Some(serde_json::to_value(state).unwrap());
        let mut boundary = self.fail_boundary_ack.lock().unwrap();
        if *boundary == Some(state.journal.sequence) {
            *boundary = None;
            return Err(crate::agent_error::AgentError::new(
                "INTERNAL",
                "lost boundary acknowledgement",
            ));
        }
        Ok(*calls)
    }
    async fn save(&self, state: &agent::Checkpoint) -> Result<(), crate::agent_error::AgentError> {
        *self.state.lock().unwrap() = Some(serde_json::to_value(state).unwrap());
        let mut boundary = self.fail_boundary_ack.lock().unwrap();
        if *boundary == Some(state.journal.sequence) {
            *boundary = None;
            return Err(crate::agent_error::AgentError::new(
                "INTERNAL",
                "lost boundary acknowledgement",
            ));
        }
        let mut ack = self.fail_ack_at.lock().unwrap();
        if *ack == Some(state.turn) && state.journal.pending.is_none() {
            *ack = None;
            return Err(crate::agent_error::AgentError::new(
                "INTERNAL",
                "injected lost checkpoint ACK",
            ));
        }
        Ok(())
    }
}
struct Model {
    turns: Mutex<VecDeque<(&'static str, Value)>>,
    invalid_replies: Mutex<usize>,
}
#[async_trait::async_trait]
impl agent::Model for Model {
    async fn turn(
        &self,
        _: &agent::Config,
        body: &[u8],
    ) -> Result<knowledge::models::ChatTurn, crate::agent_error::AgentError> {
        {
            let mut invalid = self.invalid_replies.lock().unwrap();
            if *invalid > 0 {
                *invalid -= 1;
                return Ok(knowledge::models::ChatTurn {
                    usage: None,
                    content: String::new(),
                    finish_reason: "stop".into(),
                    tool_calls: vec![],
                });
            }
        }
        let (name, mut args) = self
            .turns
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected model call");
        let body: Value = serde_json::from_slice(body).unwrap();
        let context: Value =
            serde_json::from_str(body["messages"][1]["content"].as_str().unwrap()).unwrap();
        if matches!(name, "put_section" | "set_presentation" | "put_omission") {
            args["expected_draft_sha256"] = context["draft_sha256"].clone();
        }
        let mut calls = vec![knowledge::models::ChatToolCall {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.into(),
            arguments: args.to_string(),
        }];
        if name == "inspect_rendered" {
            calls.push(knowledge::models::ChatToolCall {
                id: uuid::Uuid::new_v4().to_string(),
                name: "submit_composition_review".into(),
                arguments: json!({"findings":[]}).to_string(),
            });
        }
        Ok(knowledge::models::ChatTurn {
            usage: None,
            content: String::new(),
            finish_reason: "tool_calls".into(),
            tool_calls: calls,
        })
    }
}
fn config() -> agent::Config {
    let provider=serde_json::from_value(json!({"schema_version":1,"base_url":"https://example.invalid/v1","endpoint":"https://example.invalid/v1/chat/completions","protocol":"openai_chat_completions_sse","model_id":"scripted-fixture","credential_ref":"env:LLM_API_KEY","stream":true,"max_tokens":8192,"timeout_ms":180000,"response_mode":"tool_calls","transport_retries":0,"temperature":null,"reasoning_effort":null})).unwrap();
    agent::Config {
        provider,
        limits: agent::Limits {
            max_no_progress_turns: 6,
            max_focus_turns: 24,
            max_focus_replans: 2,
            max_turns: 50,
            max_tool_calls: 60,
            max_physical_calls: 60,
            max_read_bytes: 2_000_000,
            max_context_bytes: 500_000,
            max_context_tokens: 131072,
            image_token_reserve: 16384,
            token_safety_margin: 4096,
            max_tool_result_bytes: 100_000,
            max_review_rounds: 3,
            max_docx_bytes: 1_000_000,
        },
    }
}

#[tokio::test]
async fn composition_defers_oversized_images_without_review_credit_or_budget_reset() {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use sha2::{Digest, Sha256};
    let (input, mut result) = fixture();
    let image = image::RgbImage::from_pixel(8, 8, image::Rgb([255, 255, 255]));
    let mut bytes = vec![];
    image::codecs::jpeg::JpegEncoder::new(&mut bytes)
        .encode_image(&image)
        .unwrap();
    let mut calls = vec![];
    let mut output = vec![];
    let mut ids = vec![];
    for i in 0..2 {
        let view = analysis::views::SourceView {
            identity: analysis::views::ViewIdentity {
                source_id: format!("s{i}"),
                original_sha256: "a".repeat(64),
                image_sha256: hex::encode(Sha256::digest(&bytes)),
                page_ordinal: i,
                width: 8,
                height: 8,
                renderer: "docreader-source-view-v1/test".into(),
            },
            jpeg_base64: STANDARD.encode(&bytes),
        };
        let id = view.id().unwrap();
        calls.push(json!({"id":format!("image-{i}"),"type":"function","function":{
            "name":"read_source_view","arguments":json!({"source_id":format!("s{i}")}).to_string()}}));
        output.push(
            json!({"role":"tool","tool_call_id":format!("image-{i}"),"content":json!({"ok":true,
            "result":{"source_view_id":id,"identity":view.identity}}).to_string()}),
        );
        result.source_views.insert(id.clone(), view);
        ids.push(id);
    }
    let mut workspace = Workspace::new(&input, &result).unwrap();
    workspace.reviewing = true;
    for (id, view) in &result.source_views {
        workspace
            .review_coverage
            .views
            .insert(id.clone(), view.identity.clone());
    }
    let mut transcript = vec![json!({"role":"assistant","content":null,"tool_calls":calls})];
    transcript.extend(output);
    transcript.push(json!({"role":"user","source_view_refs":ids}));
    let mut config = config();
    let mut state = agent::Checkpoint {
        journal: Default::default(),
        contract_sha256: config.contract_sha256().unwrap(),
        workspace,
        turn: 5,
        tool_calls: 9,
        read_bytes: 12345,
        transcript,
        main_work: None,
        review_work: None,
        main_progress: Default::default(),
        review_progress: Default::default(),
    };
    let complete = agent::request(&mut state, &result, &config).await.unwrap();
    config.limits.max_tool_result_bytes = 1024;
    config.limits.max_context_bytes = complete.len() - 1;
    state.contract_sha256 = config.contract_sha256().unwrap();
    let before = state.clone();
    let body = agent::request(&mut state, &result, &config).await.unwrap();
    assert!(body.len() <= config.limits.max_context_bytes);
    assert_eq!(state.workspace.review_coverage.views.len(), 1);
    assert!(state.workspace.source_coverage.views.is_empty());
    assert_eq!(state.read_bytes, before.read_bytes);
    assert_eq!(state.tool_calls, before.tool_calls);
    let request: Value = serde_json::from_slice(&body).unwrap();
    let content = request["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["role"] == "tool")
        .map(|m| m["content"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        content
            .iter()
            .filter(|v| v.contains("NOT delivered"))
            .count(),
        1
    );
    let mut replay: agent::Checkpoint =
        serde_json::from_value(serde_json::to_value(&before).unwrap()).unwrap();
    assert_eq!(
        body,
        agent::request(&mut replay, &result, &config).await.unwrap()
    );
}

fn model(input: &FrozenInput) -> Model {
    let mut turns = vec![
        ("set_presentation", presentation(input)),
        ("put_section", section(input, 0)),
        ("put_section", section(input, 1)),
        ("put_omission", omission(input)),
        ("compile_docx", json!({})),
        ("request_composition_review", json!({})),
        ("submit_composition_review", json!({"findings":[]})),
    ];
    for i in 0..2 {
        turns.push((
            "read_source",
            json!({"source_id":format!("s{i}"),"start":0,"max_bytes":10000}),
        ));
        turns.push((
            "read_form",
            json!({"form_id":format!("f{i}"),"offset":0,"limit":100}),
        ));
    }
    for kind in ["all", "relation", "disposition"] {
        turns.push((
            "inspect_analysis",
            json!({"view":"detail","kind":kind,"offset":0,"limit":100}),
        ));
    }
    for name in [
        "inspect_composition",
        "inspect_rendered",
        "inspect_placements",
    ] {
        turns.push((name, json!({"offset":0,"limit":100})));
    }
    for i in 0..2 {
        for offset in [0, 3] {
            turns.push((
                "inspect_rendered_cells",
                json!({"bookmark":format!("kb_s{i}_b1"),"offset":offset,"limit":3}),
            ));
        }
    }
    turns.push(("submit_composition_review", json!({"findings":[]})));
    Model {
        turns: Mutex::new(turns.into()),
        invalid_replies: Mutex::new(0),
    }
}
#[tokio::test]
async fn malformed_tool_calls_are_retried_without_abandoning_the_checkpoint() {
    let (input, result) = fixture();
    let config = config();
    let journal = Journal::default();
    let model = model(&input);
    *model.invalid_replies.lock().unwrap() = 2;
    let output = agent::run(
        &input,
        &result,
        &config,
        &journal,
        &model,
        &tokio_util::sync::CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(output.manifest.status, "reviewed_template");
    assert!(*journal.calls.lock().unwrap() >= 3);
    assert_eq!(*model.invalid_replies.lock().unwrap(), 0);
}

#[tokio::test]
async fn agent_incrementally_generates_reviews_and_replays_checkpoint_ack_loss() {
    let (input, result) = fixture();
    let config = config();
    let journal = Journal::default();
    *journal.fail_ack_at.lock().unwrap() = Some(5);
    let model = model(&input);
    let cancel = tokio_util::sync::CancellationToken::new();
    assert!(
        agent::run(&input, &result, &config, &journal, &model, &cancel)
            .await
            .is_err()
    );
    let output = agent::run(&input, &result, &config, &journal, &model, &cancel)
        .await
        .unwrap();
    assert_eq!(output.manifest.status, "reviewed_template");
    let before = *journal.calls.lock().unwrap();
    assert_eq!(
        agent::run(&input, &result, &config, &journal, &model, &cancel)
            .await
            .unwrap()
            .docx_base64,
        output.docx_base64
    );
    assert_eq!(*journal.calls.lock().unwrap(), before);
    let state: agent::Checkpoint =
        serde_json::from_value(journal.state.lock().unwrap().clone().unwrap()).unwrap();
    assert!(state.workspace.done);
    assert!(model.turns.lock().unwrap().is_empty());
    assert!(
        journal
            .bodies
            .lock()
            .unwrap()
            .values()
            .any(|b| String::from_utf8_lossy(b).contains("inspection gaps remain"))
    );
    let mut changed = config.clone();
    changed.limits.max_turns += 1;
    assert_eq!(
        agent::run(&input, &result, &changed, &journal, &model, &cancel)
            .await
            .unwrap_err()
            .code,
        "FROZEN_INPUT_DIGEST_MISMATCH"
    );
    assert!(
        journal
            .bodies
            .lock()
            .unwrap()
            .values()
            .any(|b| String::from_utf8_lossy(b).contains("only tool call"))
    );
    {
        let mut stored = journal.state.lock().unwrap();
        let state = stored.as_mut().unwrap();
        state["workspace"]["artifact"]["manifest"]["placements"][0]["location"]["bookmark"] =
            json!("fake_location");
        let changed = state["workspace"]["artifact"]["manifest"]["placements"][0].clone();
        state["workspace"]["inspected"]["placement:0"] = json!(digest(&changed).unwrap());
    }
    assert!(
        agent::run(&input, &result, &config, &journal, &model, &cancel)
            .await
            .unwrap_err()
            .message
            .contains("frozen composition")
    );
}
#[test]
fn composition_tool_contracts_accept_incremental_inputs() {
    let (input, _) = fixture();
    let mut examples = BTreeMap::from([
        ("set_presentation", presentation(&input)),
        ("put_section", section(&input, 0)),
        ("put_omission", omission(&input)),
    ]);
    for v in examples.values_mut() {
        v["expected_draft_sha256"] = json!("0".repeat(64));
    }
    for t in agent::schemas(false) {
        let name = t["function"]["name"].as_str().unwrap();
        let schema = jsonschema::JSONSchema::compile(&t["function"]["parameters"]).unwrap();
        if let Some(args) = examples.get(name) {
            assert!(schema.is_valid(args), "{name}");
        }
    }
    assert!(
        agent::schemas(true)
            .iter()
            .all(|v| v["function"]["name"] != "put_section")
    );
}

#[tokio::test]
async fn composition_budget_and_cancellation_keep_unfinished_state() {
    let (input, result) = fixture();
    let mut config = config();
    config.limits.max_turns = 2;
    let journal = Journal::default();
    let model = model(&input);
    let cancel = tokio_util::sync::CancellationToken::new();
    let error = agent::run(&input, &result, &config, &journal, &model, &cancel)
        .await
        .unwrap_err();
    assert_eq!(error.code, "AGENT_TURN_BUDGET_EXCEEDED");
    let state: agent::Checkpoint =
        serde_json::from_value(journal.state.lock().unwrap().clone().unwrap()).unwrap();
    assert_eq!(state.turn, 2);
    assert!(!state.workspace.done);
    assert!(state.workspace.artifact.is_none());
    let fresh = Journal::default();
    cancel.cancel();
    assert!(
        agent::run(&input, &result, &config, &fresh, &model, &cancel)
            .await
            .is_err()
    );
    assert_eq!(*fresh.calls.lock().unwrap(), 0);
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
    };
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
    };
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
        };
        result.quality = result.expected_quality(&input).into();
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

#[tokio::test]
async fn acknowledged_missing_sources_produce_a_reviewed_draft_with_a_separate_open_item_report() {
    let (mut input, mut result) = fixture();
    let start = input.source_units[0].text.len();
    input.source_units[0]
        .text
        .push_str("\n招标编号见另行提供的招标公告。");
    let end = input.source_units[0].text.len();
    result
        .analysis
        .coverage
        .text
        .insert("s0".into(), vec![(0, end)]);
    let record = Record {
        id: "missing-notice".into(),
        sources: vec![Span {
            source_id: "s0".into(),
            start,
            end,
            view_id: None,
            grid_cell: None,
        }],
        data: RecordData::Unresolved {
            problem: "当前集合未提供所引用的公告，招标编号待确认".into(),
            affected: vec!["招标编号".into()],
            candidates: vec![],
        },
    };
    result
        .analysis
        .records
        .insert(record.id.clone(), record.clone());
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
    };
    result.quality = result.expected_quality(&input).into();
    assert_eq!(result.quality, "needs_review");
    assert!(validate_basis(&input, &result).is_ok());
    let mut forged = result.clone();
    forged.quality = "verified".into();
    assert!(validate_basis(&input, &forged).is_err());
    let mut erroneous = result.clone();
    erroneous.review.findings.push(Finding {
        code: "OMITTED_TEMPLATE".into(),
        message: "规定格式尚未完整提取".into(),
        correction: "依据完整原文补齐格式区域".into(),
        affected: vec![crate::tender_analysis::ReviewedField {
            id: "t0".into(),
            path: "/data".into(),
        }],
        sources: record.sources.clone(),
    });
    assert!(validate_basis(&input, &erroneous).is_err());
    let mut unread = result.clone();
    unread.review.coverage.candidate.clear();
    assert!(validate_basis(&input, &unread).is_err());
    let journal = Journal::default();
    let output = agent::run(
        &input,
        &result,
        &config(),
        &journal,
        &model(&input),
        &tokio_util::sync::CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(output.manifest.status, "reviewed_template_with_open_items");
    assert_eq!(output.manifest.source_quality, "needs_review");
    assert_eq!(output.manifest.source_open_items.len(), 1);
    let SourceOpenItem::Record { record: saved } = &output.manifest.source_open_items[0] else {
        panic!("missing source record lost");
    };
    assert_eq!(digest(saved).unwrap(), digest(&record).unwrap());
    assert!(
        output
            .rendered
            .iter()
            .flat_map(|b| &b.paragraphs)
            .all(|p| !p.contains("招标编号")),
        "open-item report must not inject a guessed value or report text into the bid body"
    );
    let mut checkpoint: agent::Checkpoint =
        serde_json::from_value(journal.state.lock().unwrap().clone().unwrap()).unwrap();
    assert!(
        checkpoint
            .workspace
            .inspected
            .contains_key("source_report:item:0")
    );
    checkpoint
        .workspace
        .artifact
        .as_mut()
        .unwrap()
        .manifest
        .source_open_items
        .clear();
    *journal.state.lock().unwrap() = Some(serde_json::to_value(checkpoint).unwrap());
    assert!(
        agent::run(
            &input,
            &result,
            &config(),
            &journal,
            &model(&input),
            &tokio_util::sync::CancellationToken::new()
        )
        .await
        .is_err(),
        "resume must reject a stripped source report"
    );
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
    };
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

#[tokio::test]
async fn composition_prepared_and_received_boundaries_do_not_repeat_model_work() {
    for boundary in [1, 2] {
        let (input, result) = fixture();
        let config = config();
        let journal = Journal::default();
        *journal.fail_boundary_ack.lock().unwrap() = Some(boundary);
        let model = model(&input);
        let expected_calls = model.turns.lock().unwrap().len();
        let cancel = tokio_util::sync::CancellationToken::new();
        assert!(
            agent::run(&input, &result, &config, &journal, &model, &cancel)
                .await
                .is_err()
        );
        let saved: agent::Checkpoint =
            serde_json::from_value(journal.state.lock().unwrap().clone().unwrap()).unwrap();
        assert_eq!(saved.turn, 0);
        assert_eq!(saved.tool_calls, 0);
        assert_eq!(saved.journal.sequence, boundary);
        assert_eq!(saved.journal.response().is_some(), boundary == 2);
        assert_eq!(
            model.turns.lock().unwrap().len(),
            expected_calls - usize::from(boundary == 2)
        );
        let output = agent::run(&input, &result, &config, &journal, &model, &cancel)
            .await
            .unwrap();
        assert_eq!(output.manifest.status, "reviewed_template");
        assert!(model.turns.lock().unwrap().is_empty());
        assert!(
            journal
                .sdk_turns
                .lock()
                .unwrap()
                .iter()
                .any(|turn| *turn > 1),
            "production composition must reuse SDK multi-turn state"
        );
        assert_eq!(
            *journal.calls.lock().unwrap(),
            expected_calls + usize::from(boundary == 1)
        );
    }
}

#[tokio::test]
async fn rejected_prepared_transaction_never_calls_the_composition_model() {
    let (input, result) = fixture();
    let model = model(&input);
    let expected = model.turns.lock().unwrap().len();
    let journal = Journal::default();
    *journal.reject_reservation.lock().unwrap() = true;
    assert!(
        agent::run(
            &input,
            &result,
            &config(),
            &journal,
            &model,
            &tokio_util::sync::CancellationToken::new()
        )
        .await
        .is_err()
    );
    assert_eq!(model.turns.lock().unwrap().len(), expected);
    assert_eq!(*journal.calls.lock().unwrap(), 0);
    assert!(journal.state.lock().unwrap().is_none());
}

#[test]
fn composition_progress_is_role_local_durable_and_blocks_publication() {
    use super::agent_work;
    use crate::agent_runtime::progress::Recovery;
    let (input, result) = fixture();
    let mut config = config();
    config.limits.max_no_progress_turns = 2;
    config.limits.max_focus_replans = 1;
    let mut state = agent::Checkpoint {
        journal: Default::default(),
        contract_sha256: config.contract_sha256().unwrap(),
        workspace: ready(&input, &result),
        turn: 0,
        tool_calls: 0,
        read_bytes: 0,
        transcript: vec![],
        main_work: None,
        review_work: None,
        main_progress: Default::default(),
        review_progress: Default::default(),
    };
    let source = input.source_units[0].source_unit_revision_id.clone();
    let work = json!({"source_scope":[source],"section_scope":[],"action":"compose","objective":"one source-backed section","note":"","status":"active"});
    agent_work::set_work(&input, &result, &mut state, &work, 100000).unwrap();
    for _ in 0..5 {
        agent_work::observe(&mut state, false, vec![], None, &config.limits).unwrap();
    }
    assert_eq!(state.main_progress.watch.recovery, Recovery::Blocked);
    assert_eq!(state.main_progress.blockers.len(), 1);
    let mut restored: agent::Checkpoint = serde_json::from_value(json!(state)).unwrap();
    assert!(
        agent_work::set_work(&input, &result, &mut restored, &work, 100000)
            .unwrap_err()
            .contains("dependencies unchanged")
    );
    restored.workspace.done = true;
    assert!(
        agent::reviewed_artifact(&input, &result, &config, &restored)
            .unwrap_err()
            .message
            .contains("review is incomplete")
    );
    restored.workspace.done = false;
    restored.workspace.reviewing = true;
    assert_eq!(restored.execution().watch.recovery, Recovery::Running);
    assert!(restored.workspace.review_coverage.text.is_empty());
    let mut work = work;
    work["action"] = json!("review");
    work["status"] = json!("complete");
    assert!(
        agent_work::set_work(&input, &result, &mut restored, &work, 100000).is_err(),
        "primary records and progress cannot establish reviewer coverage"
    );
}

#[tokio::test]
async fn composition_replan_removes_only_redundant_history_before_budget_pressure() {
    use crate::agent_runtime::progress::Recovery;
    let (input, result) = fixture();
    let config = config();
    let group = |id: &str, name: &str, value: Value| {
        vec![
            json!({"role":"assistant","content":null,"tool_calls":[{"id":id,"type":"function","function":{"name":name,"arguments":"{}"}}]}),
            json!({"role":"tool","tool_call_id":id,"content":json!({"ok":true,"result":value}).to_string()}),
        ]
    };
    let original = group(
        "original",
        "read_source",
        json!({"source_id":input.source_units[0].source_unit_revision_id,"text":"required source evidence","start":0,"end":24}),
    );
    let latest = group("latest", "check_gaps", json!({"items":[]}));
    let report = group(
        "report",
        "inspect_composition",
        json!({"items":[{"key":"source_report:quality","value":"acknowledged_gaps"}]}),
    );
    let mut state = agent::Checkpoint {
        journal: Default::default(),
        contract_sha256: config.contract_sha256().unwrap(),
        workspace: ready(&input, &result),
        turn: 7,
        tool_calls: 9,
        read_bytes: 24,
        transcript: [
            group("navigation", "source_index", json!({"items":[]})),
            original.clone(),
            report.clone(),
            latest.clone(),
        ]
        .concat(),
        main_work: None,
        review_work: None,
        main_progress: Default::default(),
        review_progress: Default::default(),
    };
    agent::request(&mut state, &result, &config).await.unwrap();
    assert!(
        state
            .transcript
            .iter()
            .any(|m| m["tool_call_id"] == "navigation")
    );
    for reviewing in [false, true] {
        let mut state = state.clone();
        state.workspace.reviewing = reviewing;
        let progress = if reviewing {
            &mut state.review_progress
        } else {
            &mut state.main_progress
        };
        // New reading may restore Running without completing the failed action.
        progress.watch.recovery = Recovery::Running;
        progress.watch.replans = 1;
        let before = json!(state);
        agent::request(&mut state, &result, &config).await.unwrap();
        assert_eq!(
            state.transcript,
            [original.clone(), report.clone(), latest.clone()].concat()
        );
        let mut after = json!(state);
        after["transcript"] = before["transcript"].clone();
        assert_eq!(
            after, before,
            "recovery must not reset durable state or budgets"
        );
    }
}

#[test]
fn composition_token_defaults_are_serialized_into_the_frozen_contract() {
    let mut raw = json!(config());
    for key in [
        "max_context_tokens",
        "image_token_reserve",
        "token_safety_margin",
    ] {
        raw["limits"].as_object_mut().unwrap().remove(key);
    }
    let config: agent::Config = serde_json::from_value(raw).unwrap();
    let contract = config.contract_definition();
    assert_eq!(contract["config"]["limits"]["max_context_tokens"], 131072);
    assert_eq!(contract["config"]["limits"]["image_token_reserve"], 16384);
    assert_eq!(contract["config"]["limits"]["token_safety_margin"], 4096);
}

#[tokio::test]
async fn composition_token_limit_protects_both_roles_below_the_byte_limit() {
    let (input, result) = fixture();
    let config = config();
    let mut state = agent::Checkpoint {
        journal: Default::default(),
        contract_sha256: config.contract_sha256().unwrap(),
        workspace: ready(&input, &result),
        turn: 0,
        tool_calls: 0,
        read_bytes: 0,
        transcript: vec![],
        main_work: None,
        review_work: None,
        main_progress: Default::default(),
        review_progress: Default::default(),
    };
    for reviewing in [false, true] {
        state.workspace.reviewing = reviewing;
        let full = agent::request(&mut state, &result, &config).await.unwrap();
        assert!(full.len() < config.limits.max_context_bytes);
        let mut raw = json!(config);
        raw["limits"]["max_context_tokens"] =
            json!(full.len() + config.provider.max_tokens as usize);
        raw["limits"]["image_token_reserve"] = json!(16384);
        raw["limits"]["token_safety_margin"] = json!(1);
        let limited: agent::Config = serde_json::from_value(raw).unwrap();
        limited.contract_sha256().unwrap();
        assert_eq!(
            agent::request(&mut state, &result, &limited)
                .await
                .unwrap_err()
                .code,
            "AGENT_TURN_BUDGET_EXCEEDED"
        );
    }
}
