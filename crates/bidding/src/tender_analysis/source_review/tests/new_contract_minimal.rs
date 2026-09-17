//! New-contract comparison against testdata/bid/minimal.
//! Host projections are evidence, never semantic acceptance.
use super::*;
use crate::export_review::schemas as export_schemas;
use crate::tender_analysis::semantic_compare;
use crate::tender_analysis::{ANALYSIS_GLOBAL_CHECK_KEYS, Source};
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path};

const MINIMAL_SOURCE_SHA256: &str =
    "d743182e1f67ad7f453127562fdda92dd0c2c1c898aefd4800e27c09a66c8a85";

fn minimal_docx() -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/bid/minimal/source/minimal-security-tender.docx");
    std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

// Test-only inspection of the source fixture, not the product output inventory.
// Product parsing and its coverage contract are exercised through DocReader.
fn source_xml_text(bytes: &[u8]) -> String {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    document
        .descendants()
        .filter(|node| {
            node.has_tag_name((
                "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
                "t",
            ))
        })
        .filter_map(|node| node.text())
        .collect()
}

#[test]
fn new_contract_minimal_is_host_evidence_not_semantic_acceptance() {
    let bytes = minimal_docx();
    assert_eq!(hex::encode(Sha256::digest(&bytes)), MINIMAL_SOURCE_SHA256);

    let joined = source_xml_text(&bytes);
    assert!(
        joined.contains("附表甲"),
        "composition table must appear in original OOXML fixture"
    );
    assert!(
        joined.contains("空白金额由投标人填写"),
        "bidder-fill instruction must appear in original OOXML fixture"
    );
    assert!(
        joined.contains("总分100分"),
        "scoring clause must appear in original OOXML fixture"
    );

    assert_eq!(
        ANALYSIS_GLOBAL_CHECK_KEYS,
        [
            "source_coverage",
            "collection_consistency",
            "composition_order_format",
            "cross_references",
            "rule_items",
        ]
    );
    assert!(
        export_schemas()
            .iter()
            .any(|tool| tool["function"]["name"] == "read_output_evidence")
    );

    let scoring = "总分100分：技术响应50分";
    let start = joined.find(scoring).expect("scoring sentence");
    let blank = "空白金额由投标人填写";
    let blank_start = joined.find(blank).expect("blank instruction");

    let input = FrozenInput {
        schema_version: 1,
        project_id: "project".into(),
        document_set_id: "set".into(),
        documents: vec![],
        document_relations: vec![],
        decisions: vec![],
        structured_forms: vec![],
        source_units: vec![Source {
            source_unit_revision_id: "source".into(),
            document_id: "document".into(),
            text: joined.clone(),
            locator: json!({"heading_path":"minimal"}),
            ordinal: 0,
        }],
    };
    let mut coverage = Coverage::default();
    tools::cover(
        coverage.text.entry("source".into()).or_default(),
        0,
        joined.len(),
    );
    let scope = vec!["source".into()];
    let scoring_span = Span {
        source_id: "source".into(),
        start,
        end: start + scoring.len(),
        view_id: None,
        grid_cell: None,
    };
    let blank_span = Span {
        source_id: "source".into(),
        start: blank_start,
        end: blank_start + blank.len(),
        view_id: None,
        grid_cell: None,
    };

    let requirement = Record {
        id: "score".into(),
        sources: vec![scoring_span.clone()],
        data: RecordData::Requirement {
            text: "技术响应".into(),
            categories: vec![crate::tender_analysis::Category::Evaluation],
            strength: crate::tender_analysis::Strength::Mandatory,
            compliance: vec![],
            applicability: Applicability {
                state: ApplicabilityState::Applicable,
                condition: "本项目".into(),
                scope: "评审".into(),
                grounds: vec![scoring_span.clone()],
            },
            response: vec![],
            scoring_rule: Some("技术响应50分".into()),
            proofs: vec![],
            criteria: vec![],
        },
    };
    let mut analysis = Analysis::default();
    analysis.records.insert("score".into(), requirement.clone());
    let wrapper = semantic_compare::attach(
        &input,
        &analysis,
        &coverage,
        &scope,
        "record:score",
        json!({"reference":"record:score","value":requirement}),
    );
    let checks = wrapper["field_ground_checks"].as_array().unwrap();
    assert!(
        checks.iter().any(|row| {
            row["path"] == "/data/scoring_rule"
                && row["grounds"][0]["cited_text"]
                    .as_str()
                    .is_some_and(|text| text.contains("总分100分"))
        }),
        "host must project scoring grounds against cited bytes: {checks:?}"
    );

    let template = Record {
        id: "price".into(),
        sources: vec![blank_span.clone()],
        data: RecordData::Template {
            label: "丙".into(),
            title: "费用".into(),
            parent: None,
            order: None,
            purpose: "报价".into(),
            applicability: Applicability {
                state: ApplicabilityState::Applicable,
                condition: "本项目".into(),
                scope: "报价".into(),
                grounds: vec![blank_span.clone()],
            },
            regions: vec![TemplateRegion {
                source: blank_span,
                role: RegionRole::BidderBlank,
                form_id: None,
                cells: vec![],
                blank_ranges: vec![],
                instruction: "保留说明".into(),
            }],
        },
    };
    analysis.records.insert("price".into(), template.clone());
    let wrapper = semantic_compare::attach(
        &input,
        &analysis,
        &coverage,
        &scope,
        "record:price",
        json!({"reference":"record:price","value":template}),
    );
    let effects = wrapper["blank_effects"].as_array().unwrap();
    assert!(
        effects.iter().any(|row| {
            row["selected_source_text"]
                .as_str()
                .is_some_and(|text| text.contains("空白金额由投标人填写"))
        }),
        "whole-line bidder_blank must project removed source bytes: {effects:?}"
    );
}

#[test]
fn minimal_source_ooxml_contains_matrix_needles() {
    let bytes = minimal_docx();
    let joined = source_xml_text(&bytes);
    for (id, needle) in [
        ("M01", "不采购新增硬件"),
        ("M01", "日志管理软件1套"),
        ("M02", "投标声明与资格文件"),
        ("M02", "交付方案及逐项响应"),
        ("M02", "费用明细与服务承诺"),
        ("M04", "2000条/秒"),
        ("M04", "180天"),
        ("M05", "Syslog"),
        ("M05", "任选其一"),
        ("M08", "30个自然日"),
        ("M09", "每季度"),
        ("M10", "不启用云托管"),
        ("M11", "不接受联合体"),
        ("M16", "技术响应50分"),
        ("M16", "价格30分"),
    ] {
        assert!(
            joined.contains(needle),
            "{id} needle {needle:?} missing from original OOXML fixture"
        );
    }
}

#[test]
fn new_contract_minimal_rule_and_plan_tools_are_advertised() {
    let analysis_tools = crate::tender_analysis::tools::schemas(false);
    let put_record = analysis_tools
        .iter()
        .find(|tool| tool["function"]["name"] == "put_record")
        .unwrap();
    let dump = put_record.to_string();
    assert!(
        dump.contains("composition"),
        "Rule.items kinds must be on put_record"
    );
    assert!(
        dump.contains("submission_hint"),
        "Rule.items kinds must be on put_record"
    );
    let composition = crate::docx_composition::agent::schemas(false);
    assert!(
        composition
            .iter()
            .any(|tool| tool["function"]["name"] == "put_composition_plan_item")
    );
    assert!(
        crate::export_review::schemas()
            .iter()
            .any(|tool| tool["function"]["name"] == "put_composition_review")
    );
}
