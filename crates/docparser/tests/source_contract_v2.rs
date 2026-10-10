//! These records are emitted by the real Python output-inventory parser, not
//! Rust-constructed Page-locator stand-ins. Regenerate with the script recorded
//! in services/docreader/scripts/generate_source_contract_fixtures.py.
use docparser::{
    ImageRef, ReadResult, StructuredSourceLocator, StructuredSourceUnit, parse_source_contract,
    rewrite_images_with_contract, validate_output_inventory,
};
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize)]
struct FixtureImage {
    original_ref: String,
    hex: String,
}
#[derive(Deserialize)]
struct Fixture {
    name: String,
    file_type: String,
    file_sha256: String,
    markdown: String,
    metadata: HashMap<String, String>,
    structured_source_units: Vec<StructuredSourceUnit>,
    images: Vec<FixtureImage>,
}
fn fixtures() -> Vec<(String, String, String, ReadResult)> {
    let records: Vec<Fixture> =
        serde_json::from_str(include_str!("fixtures/python-source-contract-v2.json")).unwrap();
    records
        .into_iter()
        .map(|fixture| {
            (
                fixture.name,
                fixture.file_type,
                fixture.file_sha256,
                ReadResult {
                    markdown: fixture.markdown,
                    metadata: fixture.metadata,
                    structured_source_units: fixture.structured_source_units,
                    images: fixture
                        .images
                        .into_iter()
                        .map(|image| ImageRef {
                            original_ref: image.original_ref,
                            data: hex::decode(image.hex).unwrap(),
                            ..Default::default()
                        })
                        .collect(),
                    ..Default::default()
                },
            )
        })
        .collect()
}

#[test]
fn real_python_profiles_validate_in_rust_with_dual_locators_and_page_manifest() {
    for (name, media, digest, parsed) in fixtures() {
        let contract = parse_source_contract(&parsed).unwrap().unwrap();
        let manifest = validate_output_inventory(&parsed, &digest, &media)
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        if media == "pdf" {
            assert_eq!(
                manifest
                    .page_manifest
                    .iter()
                    .map(|page| page.classification.as_str())
                    .collect::<Vec<_>>(),
                ["text", "blank", "scanned"]
            );
            let native = parsed
                .structured_source_units
                .iter()
                .find(|unit| matches!(unit.locator, StructuredSourceLocator::Document { .. }))
                .unwrap();
            assert!(matches!(
                contract.unit(&native.key).unwrap().physical_locator,
                Some(StructuredSourceLocator::Page {
                    page_ordinal: 0,
                    ..
                })
            ));
        } else {
            let table = parsed
                .structured_source_units
                .iter()
                .find(|unit| unit.grid.is_some())
                .unwrap();
            assert!(table.text.is_empty());
            assert!(
                table
                    .grid
                    .as_ref()
                    .unwrap()
                    .cells
                    .iter()
                    .any(|cell| cell.text == "10\n20")
            );
            let mapping = contract.unit(&table.key).unwrap();
            assert!(!mapping.rendered_spans.is_empty());
            let span = mapping.rendered_spans[0];
            assert!(parsed.markdown[span.start_byte..span.end_byte].contains("10<br>20"));
        }
    }
}

#[test]
fn source_contract_rejects_changed_text_grid_rendering_and_invalid_utf8_offsets() {
    let (_, _, _, original) = fixtures().pop().unwrap();
    let mut changed = original.clone();
    changed.markdown.push('x');
    assert!(parse_source_contract(&changed).is_err());
    let mut changed = original.clone();
    changed.structured_source_units[0].text.push('x');
    assert!(parse_source_contract(&changed).is_err());
    let mut changed = original.clone();
    changed
        .structured_source_units
        .iter_mut()
        .find_map(|unit| unit.grid.as_mut())
        .unwrap()
        .cells[0]
        .text
        .push('x');
    assert!(parse_source_contract(&changed).is_err());
    let mut changed = original.clone();
    let mut receipt = parse_source_contract(&changed).unwrap().unwrap();
    let mapping = receipt
        .units
        .iter_mut()
        .find(|unit| !unit.rendered_spans.is_empty())
        .unwrap();
    mapping.rendered_spans[0].start_byte = 1; // middle of the first Chinese character
    changed.metadata.insert(
        "source_contract".into(),
        serde_json::to_string(&receipt).unwrap(),
    );
    assert!(parse_source_contract(&changed).is_err());
}

#[test]
fn missing_blank_page_or_missing_image_coverage_cannot_validate() {
    let (_, media, digest, original) = fixtures().remove(0);
    let mut changed = original.clone();
    let mut contract = parse_source_contract(&changed).unwrap().unwrap();
    contract.page_manifest.remove(1);
    changed.metadata.insert(
        "source_contract".into(),
        serde_json::to_string(&contract).unwrap(),
    );
    assert!(validate_output_inventory(&changed, &digest, &media).is_err());
    let mut changed = original;
    changed.images.pop();
    assert!(validate_output_inventory(&changed, &digest, &media).is_err());
}

#[tokio::test]
async fn image_persistence_remaps_exact_spans_without_changing_source_identity() {
    let (_, _, _, original) = fixtures().remove(0);
    let before = parse_source_contract(&original).unwrap().unwrap();
    let (markdown, blobs, contract) = rewrite_images_with_contract(&original).await.unwrap();
    let contract = contract.unwrap();
    assert_eq!(blobs.len(), original.images.len());
    assert_eq!(contract.document_revision, before.document_revision);
    let mut rewritten = original;
    rewritten.markdown = markdown;
    contract.validate(&rewritten).unwrap();
    for unit in &contract.units {
        if unit.kind == docparser::StructuredSourceUnitKind::ImageRegion {
            for span in &unit.rendered_spans {
                assert!(rewritten.markdown[span.start_byte..span.end_byte].contains("objects/"));
            }
        }
    }
}

#[tokio::test]
async fn required_missing_images_fail_closed_before_persistence() {
    let result = ReadResult {
        markdown: "![required](images/missing.png)".into(),
        ..Default::default()
    };
    assert!(rewrite_images_with_contract(&result).await.is_err());
    let result = ReadResult {
        images: vec![ImageRef {
            original_ref: "missing.png".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    assert!(rewrite_images_with_contract(&result).await.is_err());
    let result = ReadResult {
        markdown: "![half-published](objects/missing)".into(),
        ..Default::default()
    };
    assert!(rewrite_images_with_contract(&result).await.is_err());
}
