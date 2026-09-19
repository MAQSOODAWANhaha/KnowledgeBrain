use jsonschema::{Draft, JSONSchema};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

const SCHEMAS: &[(&str, &str, &str)] = &[
    (
        "content-block-v1.schema.json",
        include_str!("../schemas/content-block-v1.schema.json"),
        "cec5813fe6cbeb4f407df62bc63198d29a33a3d354917dc50843147fba89313d",
    ),
    (
        "content-generation-input-v1.schema.json",
        include_str!("../schemas/content-generation-input-v1.schema.json"),
        "76d427006d086be2963624af1f288631cfb35296cf729e0446fbf06242181822",
    ),
    (
        "content-generation-output-v1.schema.json",
        include_str!("../schemas/content-generation-output-v1.schema.json"),
        "14187ee75ad1c275e45f830a106273fdad92b9423f3062912ba45c7f94b0fccd",
    ),
    (
        "docx-semantic-manifest-v1.schema.json",
        include_str!("../schemas/docx-semantic-manifest-v1.schema.json"),
        "d6f9af19f02e1d378624620acc41bd2919fbcb51133878aef805c2014ecc4abc",
    ),
    (
        "evidence-bundle-v1.schema.json",
        include_str!("../schemas/evidence-bundle-v1.schema.json"),
        "f1c708e73cd811602c3dad645bda35d5f50775654d8ab97e54a9bf77cb7a3dc2",
    ),
    (
        "pdf-semantic-manifest-v1.schema.json",
        include_str!("../schemas/pdf-semantic-manifest-v1.schema.json"),
        "a3e97877de034611dbbc1161bba1337bea63813caee3fb4f1f5dd2945dcf8737",
    ),
    (
        "render-document-snapshot-v2.schema.json",
        include_str!("../schemas/render-document-snapshot-v2.schema.json"),
        "d1b7a9c891e6c206dc0962ce76a6b3d3199817775af06a470f116f95252edc9b",
    ),
    (
        "requirement-compilation-output-v3.schema.json",
        include_str!("../schemas/requirement-compilation-output-v3.schema.json"),
        "a4756dd2e0e01c17d9fe7493f3101347861c3359c17d7c9640284fd85e5a1e7c",
    ),
    (
        "submission-assessment-snapshot-v1.schema.json",
        include_str!("../schemas/submission-assessment-snapshot-v1.schema.json"),
        "4702d6ccd9a70c5216dd093bb8396ae67de5baef2d3c7ed853a15bed1a8a675e",
    ),
    (
        "workspace-mutation-v1.schema.json",
        include_str!("../schemas/workspace-mutation-v1.schema.json"),
        "bafc92c867a251c44e6144401eb43855be6fcb7459e4809edef40038d89f3f56",
    ),
];

const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn schema_value(name: &str) -> Value {
    serde_json::from_str(
        SCHEMAS
            .iter()
            .find(|(candidate, _, _)| *candidate == name)
            .unwrap_or_else(|| panic!("missing schema {name}"))
            .1,
    )
    .unwrap()
}

fn validator(name: &str) -> JSONSchema {
    validator_from(schema_value(name))
}

fn validator_from(root: Value) -> JSONSchema {
    let mut options = JSONSchema::options();
    options.with_draft(Draft::Draft202012);
    for (_, source, _) in SCHEMAS {
        let document: Value = serde_json::from_str(source).unwrap();
        let id = document["$id"].as_str().unwrap().to_owned();
        options.with_document(id, document);
    }
    options
        .compile(&root)
        .unwrap_or_else(|error| panic!("schema did not compile: {error}"))
}

fn assert_closed_objects(value: &Value, path: &str) {
    match value {
        Value::Object(map) => {
            if map.get("type").and_then(Value::as_str) == Some("object") {
                assert_eq!(
                    map.get("additionalProperties"),
                    Some(&Value::Bool(false)),
                    "object schema must deny unknown fields at {path}"
                );
                assert!(map.contains_key("required"), "missing required at {path}");
            }
            for (key, child) in map {
                assert_closed_objects(child, &format!("{path}/{key}"));
            }
        }
        Value::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                assert_closed_objects(child, &format!("{path}/{index}"));
            }
        }
        _ => {}
    }
}

#[test]
fn active_authoring_schema_inventory_and_bytes_are_golden() {
    let expected_names = BTreeSet::from([
        "content-block-v1.schema.json",
        "content-generation-input-v1.schema.json",
        "content-generation-output-v1.schema.json",
        "docx-semantic-manifest-v1.schema.json",
        "evidence-bundle-v1.schema.json",
        "pdf-semantic-manifest-v1.schema.json",
        "render-document-snapshot-v2.schema.json",
        "requirement-compilation-output-v3.schema.json",
        "submission-assessment-snapshot-v1.schema.json",
        "workspace-mutation-v1.schema.json",
    ]);
    assert_eq!(
        SCHEMAS
            .iter()
            .map(|(name, _, _)| *name)
            .collect::<BTreeSet<_>>(),
        expected_names
    );

    for (name, source, expected_sha256) in SCHEMAS {
        let parsed: Value = serde_json::from_str(source)
            .unwrap_or_else(|error| panic!("{name} is not valid JSON: {error}"));
        assert_eq!(
            parsed["$schema"],
            "https://json-schema.org/draft/2020-12/schema"
        );
        assert_eq!(
            hex::encode(Sha256::digest(source.as_bytes())),
            *expected_sha256,
            "schema bytes drifted for {name}"
        );
        assert_closed_objects(&parsed, name);
        let _ = validator(name);
    }
}

#[test]
fn cross_schema_references_resolve_to_checked_in_contracts() {
    let documents: BTreeMap<String, Value> = SCHEMAS
        .iter()
        .map(|(_, source, _)| {
            let document: Value = serde_json::from_str(source).unwrap();
            (document["$id"].as_str().unwrap().to_owned(), document)
        })
        .collect();

    fn visit(value: &Value, documents: &BTreeMap<String, Value>, owner: &str) {
        match value {
            Value::Object(map) => {
                if let Some(reference) = map.get("$ref").and_then(Value::as_str)
                    && !reference.starts_with('#')
                {
                    let (id, fragment) = reference.split_once('#').unwrap_or((reference, ""));
                    let document = documents
                        .get(id)
                        .unwrap_or_else(|| panic!("{owner}: missing $ref {reference}"));
                    assert!(
                        fragment.is_empty() || document.pointer(fragment).is_some(),
                        "{owner}: missing $ref fragment {reference}"
                    );
                }
                for child in map.values() {
                    visit(child, documents, owner);
                }
            }
            Value::Array(values) => {
                for child in values {
                    visit(child, documents, owner);
                }
            }
            _ => {}
        }
    }

    for (name, source, _) in SCHEMAS {
        visit(&serde_json::from_str(source).unwrap(), &documents, name);
    }
}

#[test]
fn semantic_manifests_are_closed_at_every_object_level() {
    let docx_schema = validator("docx-semantic-manifest-v1.schema.json");
    let docx = json!({
        "schema_version":1,"renderer_contract_id":"docx-renderer-v1","renderer_contract_sha256":SHA,
        "parts":[{"path":"/word/document.xml","kind":"xml","canonical_sha256":SHA,"uncompressed_byte_length":42}]
    });
    assert!(docx_schema.is_valid(&docx));
    let mut extra_docx_part_field = docx;
    extra_docx_part_field["parts"][0]["excluded"] = json!(false);
    assert!(!docx_schema.is_valid(&extra_docx_part_field));

    let pdf_schema = validator("pdf-semantic-manifest-v1.schema.json");
    let pdf = json!({
        "schema_version":1,"renderer_contract_id":"pdf-renderer-v1","renderer_contract_sha256":SHA,
        "extractor_contract_sha256":SHA,"pages":[{
            "page_ordinal":0,"media_box_mpt":[0,0,612000,792000],"crop_box_mpt":[0,0,612000,792000],
            "rotation":0,"glyphs":[{"glyph_ordinal":0,"unicode_scalar_utf8_sha256":SHA,
                "x_mpt":1000,"y_mpt":2000,"w_mpt":3000,"h_mpt":4000}],
            "assets":[{"occurrence_ordinal":0,"asset_sha256":SHA,"attachment_page_id":null}]
        }]
    });
    assert!(pdf_schema.is_valid(&pdf));
    let mut missing_nullable_asset_field = pdf.clone();
    missing_nullable_asset_field["pages"][0]["assets"][0]
        .as_object_mut()
        .unwrap()
        .remove("attachment_page_id");
    assert!(!pdf_schema.is_valid(&missing_nullable_asset_field));
    let mut extra_glyph_field = pdf;
    extra_glyph_field["pages"][0]["glyphs"][0]["text"] = json!("A");
    assert!(!pdf_schema.is_valid(&extra_glyph_field));
}

#[test]
fn outline_material_tools_use_compact_nodes_and_strict_need_categories() {
    let tools: Value = serde_json::from_str(include_str!(
        "../schemas/tender-draft-outline-tools-v1.schema.json"
    ))
    .unwrap();
    let schema = validator_from(tools[0]["function"]["parameters"].clone());
    let mut batch = json!({"items":[{"id":"tmp-letter","parent":null,"order":0,"title":"投标函","prescribed":true,"requirement_ids":["requirement-1"],"purpose":"response"}],"remove_ids":[]});
    assert!(schema.is_valid(&batch));
    batch["items"][0]["grounds"] = json!([]);
    assert!(!schema.is_valid(&batch));
    let flow: Value = serde_json::from_str(include_str!(
        "../schemas/tender-outline-flow-v1.schema.json"
    ))
    .unwrap();
    let requirement = validator_from(
        flow[0]["function"]["parameters"]["properties"]["requirements"]["items"].clone(),
    );
    let mut need = json!({"id":"","description":"投标函","kind":"submission","submission_name":"投标函","classification_reason":"","format_required":true,"applicability":"required","condition":"","grounds":[],"format_grounds":[],"order_constraints":[]});
    assert!(requirement.is_valid(&need));
    need["kind"] = json!("prescribed_format");
    assert!(!requirement.is_valid(&need));
    need["kind"] = json!("submission");
    need.as_object_mut().unwrap().remove("format_required");
    assert!(!requirement.is_valid(&need));
}

#[test]
fn scan_submission_and_compact_repair_are_exclusive() {
    let flow: Value = serde_json::from_str(include_str!(
        "../schemas/tender-outline-flow-v1.schema.json"
    ))
    .unwrap();
    let schema = validator_from(flow[0]["function"]["parameters"].clone());
    let full = json!({"text":{},"forms":{},"metadata":{},"empty_sources":[],"requirements":[],"references":[],"issues":[],"review_fragments":[]});
    assert!(schema.is_valid(&full));
    let mut repair = json!({"repair":{"call_id":"failed","arguments_sha256":SHA,"changes":[{"path":"/requirements/0/condition","value":"代理人签字时"}]}});
    assert!(schema.is_valid(&repair));
    repair["requirements"] = json!([]);
    assert!(!schema.is_valid(&repair));
    repair.as_object_mut().unwrap().remove("requirements");
    repair["repair"]["changes"] = json!([]);
    assert!(!schema.is_valid(&repair));
}
