use super::*;
use std::io::{Cursor, Read, Write};

fn fixture(parts: &[(&str, bool)]) -> (FrozenInput, AnalysisResult) {
    let text: String = parts.iter().map(|p| p.0).collect();
    let source = Source {
        source_unit_revision_id: "source".into(),
        document_id: "document".into(),
        text: text.clone(),
        locator: json!({"page_ordinal":0}),
        ordinal: 0,
    };
    let input = FrozenInput {
        schema_version: 1,
        project_id: "project".into(),
        document_set_id: "set".into(),
        documents: vec![],
        document_relations: vec![],
        decisions: vec![],
        source_units: vec![source],
        structured_forms: vec![],
    };
    let span = |start, end| Span {
        source_id: "source".into(),
        start,
        end,
        view_id: None,
        grid_cell: None,
    };
    let mut offset = 0;
    let regions = parts
        .iter()
        .map(|(text, blank)| {
            let start = offset;
            offset += text.len();
            TemplateRegion {
                source: span(start, offset),
                role: if *blank {
                    RegionRole::BidderBlank
                } else {
                    RegionRole::FixedText
                },
                form_id: None,
                cells: vec![],
                blank_ranges: vec![],
                instruction: "source policy".into(),
            }
        })
        .collect();
    let record = Record {
        id: "template".into(),
        sources: vec![span(0, text.len())],
        data: RecordData::Template {
            label: "identity".into(),
            title: "identity".into(),
            parent: None,
            order: None,
            purpose: "source form".into(),
            applicability: Applicability {
                state: ApplicabilityState::Applicable,
                condition: "source form".into(),
                scope: "source".into(),
                grounds: vec![span(0, text.len())],
            },
            regions,
        },
    };
    let mut analysis = Analysis::default();
    analysis.records.insert(record.id.clone(), record);
    let result = AnalysisResult {
        schema_version: 1,
        frozen_input_sha256: String::new(),
        analysis,
        review: Review {
            analysis_sha256: String::new(),
            coverage: Coverage::default(),
            findings: vec![],
        },
        quality: "diagnostic_unaccepted".into(),
        source_views: BTreeMap::new(),
    };
    (input, result)
}

fn section(result: &AnalysisResult) -> Section {
    Section {
        id: "section".into(),
        placement: SectionPlacement::Body,
        parent: None,
        order: 0,
        title: "source form".into(),
        grounds: result.analysis.records["template"].sources.clone(),
        content: vec![],
    }
}

fn plan(input: &FrozenInput, result: &AnalysisResult) -> (TemplatePlan, Vec<Placement>) {
    let mut blocks = vec![];
    let mut placements = vec![];
    template(
        input,
        result,
        "template",
        &[],
        (&section(result), 0),
        &mut blocks,
        &mut placements,
    )
    .unwrap();
    (
        TemplatePlan {
            title: "carrier".into(),
            toc_title: "contents".into(),
            style: TemplateStyle {
                width_mm: 210.0,
                height_mm: 297.0,
                top_mm: 20.0,
                right_mm: 20.0,
                bottom_mm: 20.0,
                left_mm: 20.0,
                font_family: "Noto Sans CJK SC".into(),
                body_font_pt: 10.5,
                line_spacing: 1.0,
            },
            sections: vec![TemplateSection {
                title: "source form".into(),
                placement: SectionPlacement::Body,
                depth: 0,
                source_ids: input
                    .source_units
                    .iter()
                    .map(|s| s.source_unit_revision_id.clone())
                    .collect(),
                blocks,
            }],
            excluded_sources: vec![],
            excluded_forms: vec![],
            notices: vec![],
        },
        placements,
    )
}

fn xml(docx: &[u8]) -> String {
    let mut archive = zip::ZipArchive::new(Cursor::new(docx)).unwrap();
    let mut text = String::new();
    archive
        .by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    text
}

fn replace_xml(docx: &[u8], xml: &str) -> Vec<u8> {
    let mut archive = zip::ZipArchive::new(Cursor::new(docx)).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(vec![]));
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).unwrap();
        out.start_file(file.name(), zip::write::SimpleFileOptions::default())
            .unwrap();
        if file.name() == "word/document.xml" {
            out.write_all(xml.as_bytes()).unwrap();
        } else {
            std::io::copy(&mut file, &mut out).unwrap();
        }
    }
    out.finish().unwrap().into_inner()
}

#[test]
fn inline_fields_keep_source_lines_and_independent_locations_without_example_values() {
    let parts = [
        ("姓名：", false),
        ("张三", true),
        (" 性别：", false),
        ("男", true),
        (" 年龄：", false),
        ("42", true),
        (" 职务：", false),
        ("经理", true),
        ("\r", false),
        ("\n系 (", false),
        ("示例企业\r\n第二行示例", true),
        (")代表。\n", false),
    ];
    let (input, result) = fixture(&parts);
    let (plan, placements) = plan(&input, &result);
    assert_eq!(plan.sections[0].blocks.len(), 1);
    let docx = compile_template(&json!(input), &plan).unwrap();
    let rendered = super::super::document::verify(&docx, &json!(input), &plan).unwrap();
    let carrier = rendered.iter().find(|b| b.bookmark == "kb_s0_b0").unwrap();
    assert_eq!(
        carrier.paragraphs,
        ["姓名：  性别：  年龄：  职务： \n系 ( \n )代表。\n"]
    );
    let xml = xml(&docx);
    for example in ["张三", "示例企业", "第二行示例", "经理", "42"] {
        assert!(!xml.contains(example));
    }
    let regions: Vec<_> = placements
        .iter()
        .filter(|p| matches!(p.reference.target, RelationTarget::TemplateRegion { .. }))
        .collect();
    assert_eq!(regions.len(), parts.len());
    assert_eq!(
        regions
            .iter()
            .map(|p| &p.location.bookmark)
            .collect::<BTreeSet<_>>()
            .len(),
        parts.len()
    );
    for p in regions {
        assert!(rendered.iter().any(|b| b.bookmark == p.location.bookmark));
    }
    assert_eq!(
        rendered
            .iter()
            .find(|b| b.bookmark == "kb_s0_b0_r10")
            .unwrap()
            .paragraphs,
        [" \n "]
    );
}

#[test]
fn source_changes_and_gaps_do_not_join_text_regions() {
    let (mut input, mut result) = fixture(&[
        ("甲", false),
        (" ", true),
        ("隔", false),
        ("乙", false),
        (" ", true),
    ]);
    let mut other = input.source_units[0].clone();
    other.source_unit_revision_id = "other".into();
    input.source_units.push(other);
    let RecordData::Template { regions, .. } =
        &mut result.analysis.records.get_mut("template").unwrap().data
    else {
        unreachable!()
    };
    regions.remove(2); // A real gap in the original must not be swallowed.
    let (first, _) = plan(&input, &result);
    assert_eq!(first.sections[0].blocks.len(), 2);
    let RecordData::Template { regions, .. } =
        &mut result.analysis.records.get_mut("template").unwrap().data
    else {
        unreachable!()
    };
    regions[2].source.source_id = "other".into();
    regions[3].source.source_id = "other".into();
    // Remove the numerical gap too: source identity independently prevents joining.
    regions[2].source.start = regions[1].source.end;
    let (second, _) = plan(&input, &result);
    assert_eq!(second.sections[0].blocks.len(), 2);
    let bytes = compile_template(&json!(input), &second).unwrap();
    super::super::document::verify(&bytes, &json!(input), &second).unwrap();
}

#[test]
fn changed_inline_bookmark_order_extent_ownership_and_content_are_rejected() {
    let (input, result) = fixture(&[
        ("姓名：", false),
        ("示例", true),
        (" 性别：", false),
        ("示例", true),
    ]);
    let (plan, _) = plan(&input, &result);
    let bytes = compile_template(&json!(input), &plan).unwrap();
    let original = xml(&bytes);
    let parsed = roxmltree::Document::parse(&original).unwrap();
    let start = parsed
        .descendants()
        .find(|n| {
            n.attribute((
                "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
                "name",
            )) == Some("kb_s0_b0_r0")
        })
        .unwrap();
    let start_xml = &original[start.range()];
    let id = start
        .attribute((
            "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
            "id",
        ))
        .unwrap();
    let end_xml = format!("<w:bookmarkEnd w:id=\"{id}\"/>");
    let swapped = original
        .replace("kb_s0_b0_r0", "TEMP")
        .replace("kb_s0_b0_r1", "kb_s0_b0_r0")
        .replace("TEMP", "kb_s0_b0_r1");
    let widened = original.replacen(&end_xml, "", 1).replacen(
        "</w:p><w:bookmarkEnd w:id=\"1\"",
        &format!("{end_xml}</w:p><w:bookmarkEnd w:id=\"1\""),
        1,
    );
    let escaped = original.replacen(start_xml, "", 1).replacen(
        "<w:bookmarkStart w:id=\"1\"",
        &format!("{start_xml}<w:bookmarkStart w:id=\"1\""),
        1,
    );
    for damaged in [
        swapped,
        widened,
        escaped,
        original.replacen("姓名：", "他人：", 1),
        original.replacen(
            "xml:space=\"preserve\"> </w:t>",
            "xml:space=\"preserve\">泄露姓名</w:t>",
            1,
        ),
    ] {
        assert_ne!(damaged, original);
        assert!(
            super::super::document::verify(&replace_xml(&bytes, &damaged), &json!(input), &plan)
                .is_err()
        );
    }
}

#[test]
fn compiled_response_binding_uses_its_independent_inline_region() {
    let (input, mut result) = fixture(&[
        ("姓名：", false),
        ("示例姓名", true),
        (" 性别：", false),
        (" ", true),
    ]);
    let grounds = result.analysis.records["template"].sources.clone();
    let requirement: Record = serde_json::from_value(json!({"id":"requirement","sources":grounds,
        "data":{"kind":"requirement","text":"填写姓名","categories":["attachment"],"strength":"mandatory",
        "compliance":[{"policy":"explicit_response","condition":"使用指定格式","grounds":grounds}],
        "applicability":{"state":"applicable","condition":"使用指定格式","scope":"source","grounds":grounds},
        "response":[{"channel":"response_table","description":"填写姓名","condition":"使用指定格式","grounds":grounds}],
        "proofs":[],"criteria":[],"scoring_rule":null}})).unwrap();
    result
        .analysis
        .records
        .insert(requirement.id.clone(), requirement);
    result.analysis.relations.insert(
        "mapping".into(),
        Relation {
            id: "mapping".into(),
            from: "requirement".into(),
            to: "template".into(),
            from_target: RelationTarget::Response { index: 0 },
            to_target: RelationTarget::Record,
            from_record_sha256: digest(&result.analysis.records["requirement"]).unwrap(),
            to_record_sha256: digest(&result.analysis.records["template"]).unwrap(),
            kind: RelationKind::RequiresTemplate,
            state: RelationState::Explicit,
            scope: "source".into(),
            explanation: "specified name field".into(),
            grounds: grounds.clone(),
        },
    );
    result
        .analysis
        .coverage
        .text
        .insert("source".into(), vec![(0, input.source_units[0].text.len())]);
    result.analysis.dispositions.insert(
        "source".into(),
        Disposition {
            state: DispositionState::Requirement,
            reason: "specified form".into(),
        },
    );
    let mut coverage = result.analysis.coverage.clone();
    for kind in ["all", "relation", "disposition"] {
        crate::tender_analysis::tools::invoke(
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
    result.frozen_input_sha256 = digest(&input).unwrap();
    result.quality = result.expected_quality(&input).into();
    let mut draft = Draft::new(&input, &result).unwrap();
    let (carrier, _) = plan(&input, &result);
    draft.presentation = Some(Presentation {
        title: carrier.title,
        toc_title: carrier.toc_title,
        style: carrier.style,
        grounds,
        explanation: "explicit fixture presentation".into(),
    });
    let mut section = section(&result);
    section.content.push(Content::Template {
        record_id: "template".into(),
        headers: vec![],
        bindings: vec![FieldBinding {
            need: Reference {
                record_id: "requirement".into(),
                target: RelationTarget::Response { index: 0 },
            },
            field: RelationTarget::TemplateRegion { index: 1 },
        }],
    });
    draft.sections.insert(section.id.clone(), section);
    let compiled = compile(&input, &result, &draft, 1_000_000).unwrap();
    let need = compiled
        .manifest
        .placements
        .iter()
        .find(|p| p.reference.record_id == "requirement")
        .unwrap();
    assert_eq!(need.location.bookmark, "kb_s0_b0_r1");
    assert_eq!(
        compiled
            .rendered
            .iter()
            .find(|r| r.bookmark == need.location.bookmark)
            .unwrap()
            .paragraphs,
        [" "]
    );
    assert_ne!(
        need.location.bookmark,
        compiled
            .manifest
            .placements
            .iter()
            .find(|p| p.reference.target == RelationTarget::TemplateRegion { index: 3 })
            .unwrap()
            .location
            .bookmark
    );
}
