//! Real DocReader parse of the replay inputs.
//!
//! Built only with `--features docreader-contract-tests`. CI starts the
//! service in `scripts/docreader_replay_acceptance.py` and points this test at
//! two files. The same bytes must parse to the same document tree. A different
//! file must parse to a different tree.

#![cfg(feature = "docreader-contract-tests")]

use docparser::convert_tender_source;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn same_bytes_parse_identically_and_a_different_file_does_not() {
    let docx_path = std::env::var("KB_DOCREADER_TEST_DOCX").expect("KB_DOCREADER_TEST_DOCX");
    let pdf_path = std::env::var("KB_DOCREADER_TEST_PDF").expect("KB_DOCREADER_TEST_PDF");
    let docx = std::fs::read(&docx_path).unwrap_or_else(|error| panic!("{docx_path}: {error}"));
    let pdf = std::fs::read(&pdf_path).unwrap_or_else(|error| panic!("{pdf_path}: {error}"));
    assert_ne!(docx, pdf, "the two replay inputs must be different bytes");

    let cancel = CancellationToken::new();
    let first = parse("input.docx", docx.clone(), &cancel).await;
    let second = parse("input.docx", docx, &cancel).await;
    let other = parse("input.pdf", pdf, &cancel).await;

    let docx_text = tree_text(&first.1);
    let pdf_text = tree_text(&other.1);
    assert!(
        docx_text.contains("Replay acceptance input A"),
        "docx tree missing the heading: {docx_text}"
    );
    assert!(
        docx_text.contains("Item"),
        "docx tree missing the table: {docx_text}"
    );
    assert!(
        pdf_text.contains("Different PDF input B"),
        "pdf tree missing its sentence: {pdf_text}"
    );
    assert_eq!(first, second, "the same bytes must parse to the same tree");
    assert_ne!(
        first, other,
        "a different file must parse to a different tree"
    );
}

async fn parse(
    file_name: &str,
    bytes: Vec<u8>,
    cancel: &CancellationToken,
) -> (String, Vec<docparser::StructuredSourceUnit>) {
    let result = convert_tender_source(file_name, bytes, cancel)
        .await
        .unwrap_or_else(|error| panic!("{file_name}: {error}"));
    assert!(
        result.error.is_empty(),
        "{file_name} parse error: {}",
        result.error
    );
    assert!(
        !result.structured_source_units.is_empty(),
        "{file_name} produced no document tree"
    );
    (result.markdown, result.structured_source_units)
}

fn tree_text(units: &[docparser::StructuredSourceUnit]) -> String {
    let mut parts = Vec::new();
    for unit in units {
        parts.push(unit.text.clone());
        if let Some(grid) = &unit.grid {
            parts.extend(grid.cells.iter().map(|cell| cell.text.clone()));
        }
    }
    parts.join("\n")
}
