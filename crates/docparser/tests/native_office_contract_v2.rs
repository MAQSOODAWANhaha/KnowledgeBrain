//! Recorded outputs of the native producers, including their real OOXML bytes.
//! Regenerate with services/docreader/scripts/generate_native_office_fixtures.py.
use docparser::{ReadResult, SourceCompleteness, StructuredSourceLocator, parse_source_contract};
use serde_json::Value;
use sha2::{Digest, Sha256};

#[test]
fn real_native_office_grids_keep_empty_text_physical_identity_and_rendered_mapping() {
    let rows: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/python-native-office-v2.json")).unwrap();
    for row in rows {
        let source = hex::decode(row["source_hex"].as_str().unwrap()).unwrap();
        assert!(source.starts_with(b"PK"));
        let parsed = ReadResult {
            markdown: row["markdown"].as_str().unwrap().into(),
            metadata: serde_json::from_value(row["metadata"].clone()).unwrap(),
            structured_source_units: serde_json::from_value(row["structured_source_units"].clone())
                .unwrap(),
            ..Default::default()
        };
        let contract = parse_source_contract(&parsed).unwrap().unwrap();
        assert_eq!(
            contract.document_revision,
            hex::encode(Sha256::digest(source))
        );
        assert_eq!(contract.document_revision, row["file_sha256"]);
        assert!(
            contract
                .units
                .iter()
                .all(|unit| unit.completeness == SourceCompleteness::Complete)
        );
        let table = parsed
            .structured_source_units
            .iter()
            .find(|unit| unit.grid.is_some())
            .unwrap();
        assert!(table.text.is_empty());
        let grid = table.grid.as_ref().unwrap();
        assert!(
            grid.cells
                .iter()
                .any(|cell| cell.text.contains("必须提交营业执照"))
        );
        assert!(grid.cells.iter().any(|cell| cell.col_span == 2));
        let identity = contract.unit(&table.key).unwrap();
        assert_eq!(identity.table_id.as_deref(), Some(table.key.as_str()));
        assert_eq!(
            identity.grid_sha256.as_deref(),
            Some(docparser::table_grid_digest(grid).as_str())
        );
        assert!(identity.section_id.is_some());
        assert!(!identity.rendered_spans.is_empty());
        for span in &identity.rendered_spans {
            assert!(
                parsed
                    .markdown
                    .get(span.start_byte..span.end_byte)
                    .is_some()
            );
        }
        if row["file_type"] == "docx" {
            assert!(matches!(
                identity.physical_locator,
                Some(StructuredSourceLocator::Document { .. })
            ));
            assert!(
                identity
                    .physical_path
                    .as_deref()
                    .is_some_and(|path| path == "/word/document.xml/body/*[3]")
            );
            assert!(!identity.header_cells.is_empty());
            assert!(parsed.structured_source_units[0].text.contains("表前正文"));
            assert!(parsed.structured_source_units[2].text.contains("表后正文"));
        } else {
            assert!(matches!(
                identity.physical_locator,
                Some(StructuredSourceLocator::Spreadsheet { .. })
            ));
            assert!(
                parsed
                    .structured_source_units
                    .last()
                    .unwrap()
                    .text
                    .contains("空白附表")
            );
        }
    }
}
