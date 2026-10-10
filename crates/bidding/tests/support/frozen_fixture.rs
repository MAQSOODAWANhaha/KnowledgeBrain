//! Complete parser receipt for integration tests that need a minimal tender.
pub fn valid_text_input(project_id: &str, text: &str) -> bidding::analysis::FrozenInput {
    use bidding::outline::frozen::{FrozenBuildInput, ParsedDocument, build_frozen_input};
    use sha2::{Digest, Sha256};
    let digest = hex::encode(Sha256::digest(text.as_bytes()));
    let locator = docparser::StructuredSourceLocator::Document {
        section_ordinal: 0,
        table_ordinal: None,
        row_ordinal: None,
        form_ordinal: None,
        heading_path: "Tender".into(),
    };
    let unit = docparser::StructuredSourceUnit {
        key: "body".into(),
        ordinal: 0,
        kind: docparser::StructuredSourceUnitKind::Section,
        text: text.into(),
        locator: locator.clone(),
        grid: None,
    };
    let contract = serde_json::json!({"schema_version":2,"document_revision":digest,"parser_version":"test-parser-v2","markdown_sha256":digest,"page_manifest":[],"units":[{
        "unit_id":"body","ordinal":0,"kind":"section","text_sha256":digest,"grid_sha256":null,"section_id":"section:0","parent_section_id":null,"heading_level":1,"heading_path":"Tender","physical_locator":locator,"physical_path":"body/p:0","physical_locator_unavailable_reason":null,"rendered_spans":if text.is_empty(){vec![]}else{vec![serde_json::json!({"start_byte":0,"end_byte":text.len()})]},"completeness":"complete","reasons":[],"table_id":null,"header_cells":[]
    }]});
    let parsed = docparser::ReadResult {
        markdown: text.into(),
        structured_source_units: vec![unit],
        metadata: std::collections::HashMap::from([(
            "source_contract".into(),
            contract.to_string(),
        )]),
        ..Default::default()
    };
    build_frozen_input(
        FrozenBuildInput {
            project_id: project_id.into(),
            document_set_id: "set".into(),
            documents: vec![ParsedDocument {
                document_id: "document".into(),
                parsed,
            }],
            document_relations: vec![],
            decisions: vec![],
        },
        vec![],
        "source-v2",
    )
    .unwrap()
}
