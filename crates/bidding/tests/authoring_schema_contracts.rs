use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

const SCHEMAS: &[(&str, &str, &str)] = &[
    (
        "content-block-v1.schema.json",
        include_str!("../schemas/content-block-v1.schema.json"),
        "030cb07f3073bb8a7e87f6b045706bc0d4673b34153e827b05f31d7a3f7dc71d",
    ),
    (
        "content-generation-input-v1.schema.json",
        include_str!("../schemas/content-generation-input-v1.schema.json"),
        "bdeed344ec9db8a6a4436aa73da1b63cb84e8eac6c58b0580eaff3eef5913acd",
    ),
    (
        "content-generation-output-v1.schema.json",
        include_str!("../schemas/content-generation-output-v1.schema.json"),
        "f049c046fdca4e272d0a0d69c0076f77a252be59218d630acbb2983b8224b037",
    ),
    (
        "composition-spine-v1.schema.json",
        include_str!("../schemas/composition-spine-v1.schema.json"),
        "1926e2efd296d5e56f8968979044980619a7f8393da0644ac9b2358cf42830c2",
    ),
    (
        "evidence-bundle-v1.schema.json",
        include_str!("../schemas/evidence-bundle-v1.schema.json"),
        "f1c708e73cd811602c3dad645bda35d5f50775654d8ab97e54a9bf77cb7a3dc2",
    ),
    (
        "outline-assessment-snapshot-v1.schema.json",
        include_str!("../schemas/outline-assessment-snapshot-v1.schema.json"),
        "d696487fd36a5faf7a106746ecf77fefd1bbb770f1373b10a7e3273abcd03c0f",
    ),
    (
        "outline-evidence-batch-v1.schema.json",
        include_str!("../schemas/outline-evidence-batch-v1.schema.json"),
        "e2ebe2aa46b756268c8d2daa1de3811e6f099bc85b8f3b853d3e63456407ff28",
    ),
    (
        "outline-evidence-batch-v2.schema.json",
        include_str!("../schemas/outline-evidence-batch-v2.schema.json"),
        "c61b93b6ed07cfa70f4a54514ad68da5dc3d7bb5344eaec16e9229b16ebf95cc",
    ),
    (
        "outline-evidence-batch-v3.schema.json",
        include_str!("../schemas/outline-evidence-batch-v3.schema.json"),
        "e91266fcc8efee425c54e04d89528a581f54ecc8a6f46287770a63c3c673efd8",
    ),
    (
        "outline-generation-input-v1.schema.json",
        include_str!("../schemas/outline-generation-input-v1.schema.json"),
        "f0ffbfdf049f2a1bb289e161eb6cb931e356e4bdedf29149c719bb074fd2b2e4",
    ),
    (
        "outline-generation-output-v1.schema.json",
        include_str!("../schemas/outline-generation-output-v1.schema.json"),
        "f958ec49bc03f9ed6f79e0f4704e6aaed273af47b7ae4a996ee61bbec57587a5",
    ),
    (
        "outline-generation-output-v2.schema.json",
        include_str!("../schemas/outline-generation-output-v2.schema.json"),
        "1b7d9d852e0956fa1717d633e087f4bfbd08ab2a8cf17693fd47d5808327a4bf",
    ),
    (
        "outline-reduce-plan-v1.schema.json",
        include_str!("../schemas/outline-reduce-plan-v1.schema.json"),
        "fe61cd232a53c3959f46ea09cd0c5bcb2de21663baed4726baefdc9d76426854",
    ),
    (
        "outline-reduce-plan-v2.schema.json",
        include_str!("../schemas/outline-reduce-plan-v2.schema.json"),
        "c52ab0a2ca5689f2e2e8f633e588cb3ed9f201f61400f030ed213825b5a6f15c",
    ),
    (
        "outline-synthesis-checkpoint-v1.schema.json",
        include_str!("../schemas/outline-synthesis-checkpoint-v1.schema.json"),
        "7d0efce7fa4f352b2d787c97fc9b226558af1fe033984c43d1a23b2a223625fc",
    ),
    (
        "outline-synthesis-checkpoint-v2.schema.json",
        include_str!("../schemas/outline-synthesis-checkpoint-v2.schema.json"),
        "2ce311b7b4a6cf9ad058f6e19663c9097dd091b4c9d689bc3c8c5342bdb75e25",
    ),
    (
        "outline-synthesis-packet-v1.schema.json",
        include_str!("../schemas/outline-synthesis-packet-v1.schema.json"),
        "002360e4355595ab00986e639d5c0dd66b080c492282d94932a560d4d4c1f4d4",
    ),
    (
        "outline-synthesis-packet-v2.schema.json",
        include_str!("../schemas/outline-synthesis-packet-v2.schema.json"),
        "ad786d1a7425efc771bbc2cfc7bbcab1de68056bfee47ad5cd1ac86e2b3b128a",
    ),
    (
        "render-document-snapshot-v2.schema.json",
        include_str!("../schemas/render-document-snapshot-v2.schema.json"),
        "4047791bd136a261e24e7c05de160ab133ffe0bc521523fc012d0aa6050096e6",
    ),
    (
        "requirement-compilation-output-v3.schema.json",
        include_str!("../schemas/requirement-compilation-output-v3.schema.json"),
        "a4756dd2e0e01c17d9fe7493f3101347861c3359c17d7c9640284fd85e5a1e7c",
    ),
    (
        "section-obligation-matrix-v1.schema.json",
        include_str!("../schemas/section-obligation-matrix-v1.schema.json"),
        "1ca5ee05ca9665f999875341889332f9360d6a7dd9359de90565209c4f742b4a",
    ),
    (
        "submission-assessment-snapshot-v1.schema.json",
        include_str!("../schemas/submission-assessment-snapshot-v1.schema.json"),
        "4702d6ccd9a70c5216dd093bb8396ae67de5baef2d3c7ed853a15bed1a8a675e",
    ),
    (
        "workspace-mutation-v1.schema.json",
        include_str!("../schemas/workspace-mutation-v1.schema.json"),
        "e68e9abcd5b122aed0e25bdd63bbc611aef05bc662c74313b698c4915fff545f",
    ),
];

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

fn enum_literals(value: &Value) -> BTreeSet<&str> {
    value
        .as_array()
        .expect("enum array")
        .iter()
        .map(|item| item.as_str().expect("enum string"))
        .collect()
}

#[test]
fn authoring_schema_bytes_and_hashes_are_golden() {
    assert_eq!(SCHEMAS.len(), 23);
    for (name, source, expected_sha256) in SCHEMAS {
        let parsed: Value = serde_json::from_str(source).unwrap_or_else(|error| {
            panic!("{name} is not valid JSON: {error}");
        });
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
    }
}

#[test]
fn cross_schema_references_resolve_to_checked_in_contracts() {
    let available_ids: BTreeSet<String> = SCHEMAS
        .iter()
        .map(|(_, source, _)| {
            serde_json::from_str::<Value>(source).unwrap()["$id"]
                .as_str()
                .expect("schema id")
                .to_owned()
        })
        .collect();
    for (name, source, _) in SCHEMAS {
        let parsed: Value = serde_json::from_str(source).unwrap();
        fn visit(value: &Value, available_ids: &BTreeSet<String>, owner: &str) {
            match value {
                Value::Object(map) => {
                    if let Some(reference) = map.get("$ref").and_then(Value::as_str)
                        && !reference.starts_with('#')
                    {
                        assert!(
                            available_ids.contains(reference),
                            "{owner}: missing $ref {reference}"
                        );
                    }
                    for child in map.values() {
                        visit(child, available_ids, owner);
                    }
                }
                Value::Array(values) => {
                    for child in values {
                        visit(child, available_ids, owner);
                    }
                }
                _ => {}
            }
        }
        visit(&parsed, &available_ids, name);
    }
}

#[test]
fn approved_v2_contract_invariants_are_frozen() {
    let by_name = |name: &str| -> Value {
        serde_json::from_str(
            SCHEMAS
                .iter()
                .find(|(candidate, _, _)| *candidate == name)
                .expect("schema exists")
                .1,
        )
        .unwrap()
    };

    let outline_input = by_name("outline-generation-input-v1.schema.json");
    assert_eq!(
        outline_input["properties"]["workspace_scope"]["const"],
        "project_wide"
    );
    let source_kinds =
        enum_literals(&outline_input["$defs"]["sourceUnit"]["properties"]["kind"]["enum"]);
    assert!(source_kinds.contains("attachment_region"));
    assert!(source_kinds.contains("image_ocr_region"));
    assert_eq!(
        enum_literals(&outline_input["$defs"]["sourceUnit"]["properties"]["disposition"]["enum"]),
        BTreeSet::from(["non_requirement", "requirement", "unresolved"])
    );
    for axis in ["requiredness", "compliance_policy", "lifecycle"] {
        assert!(
            outline_input["$defs"]["requirement"]["required"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value == axis)
        );
    }
    for identity in [
        "prompt_contract_id",
        "template_contract_id",
        "template_contract_sha256",
        "model_contract_id",
        "agent_contract_id",
        "agent_contract_sha256",
    ] {
        assert!(
            outline_input["required"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value == identity)
        );
    }

    let requirement_compile = by_name("requirement-compilation-output-v3.schema.json");
    assert_eq!(
        requirement_compile["properties"]["schema_version"]["const"],
        3
    );
    assert_eq!(
        enum_literals(&requirement_compile["$defs"]["channel"]["enum"]),
        BTreeSet::from([
            "deviation_statement",
            "evidence_attachment",
            "narrative_content",
            "quotation",
            "response_table",
            "structured_form",
        ])
    );

    let map_v3 = by_name("outline-evidence-batch-v3.schema.json");
    for property in [
        "outline_usage",
        "applicability",
        "composition_parent_role",
        "source_numbering",
    ] {
        assert!(
            map_v3["$defs"]["structureFragment"]["required"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value == property),
            "Map V3 fragment missing {property}"
        );
    }

    let reduce_v2 = by_name("outline-reduce-plan-v2.schema.json");
    assert_eq!(
        reduce_v2["properties"]["composition_spine"]["$ref"],
        "urn:knowledgebrain:bid:composition-spine:v1"
    );
    assert_eq!(
        reduce_v2["properties"]["section_obligation_matrix"]["$ref"],
        "urn:knowledgebrain:bid:section-obligation-matrix:v1"
    );

    let packet_v2 = by_name("outline-synthesis-packet-v2.schema.json");
    assert_eq!(packet_v2["properties"]["schema_version"]["const"], 2);
    assert!(
        packet_v2["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "section_obligation_matrix")
    );
    let checkpoint_v2 = by_name("outline-synthesis-checkpoint-v2.schema.json");
    assert!(
        checkpoint_v2["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "accepted_obligation_bindings")
    );

    let outline_output_v2 = by_name("outline-generation-output-v2.schema.json");
    assert_eq!(
        outline_output_v2["properties"]["schema_version"]["const"],
        2
    );
    assert!(
        outline_output_v2["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "section_obligation_bindings")
    );

    let content_input = by_name("content-generation-input-v1.schema.json");
    assert_eq!(
        enum_literals(&content_input["properties"]["fill_policy"]["enum"]),
        BTreeSet::from([
            "append_candidate",
            "empty_only",
            "missing_requirements_only"
        ])
    );
    assert!(content_input.to_string().contains("insertion_anchor"));
    for identity in [
        "workspace_scope_revision_id",
        "outline_checkpoint_id",
        "requirement_projection_sha256",
        "evidence_selection_sha256",
        "prompt_contract_id",
        "prompt_contract_sha256",
        "template_contract_id",
        "template_contract_sha256",
        "model_contract_id",
        "model_contract_sha256",
        "agent_contract_id",
        "agent_contract_sha256",
        "render_style_contract_id",
        "render_style_contract_sha256",
    ] {
        assert!(
            content_input["required"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value == identity)
        );
    }
    assert!(
        content_input["$defs"]["systemSelection"]["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "matching_policy_id")
    );

    let evidence = by_name("evidence-bundle-v1.schema.json");
    assert!(evidence.to_string().contains("evidence_item_id"));
    assert!(evidence.to_string().contains("image_artifact_revision_id"));

    let render = by_name("render-document-snapshot-v2.schema.json");
    assert_eq!(
        enum_literals(&render["properties"]["output_mode"]["enum"]),
        BTreeSet::from(["preview", "review_draft", "submission"])
    );
    assert!(render.to_string().contains("include_knowledge_sources"));
    for identity in [
        "workspace_scope_revision_id",
        "form_definition_occurrences",
        "attachment_preparation_occurrences",
        "content_block_schema_version",
        "render_operation_contract_version",
        "docx_renderer_contract_id",
        "docx_renderer_contract_sha256",
        "pdf_renderer_contract_id",
        "pdf_renderer_contract_sha256",
    ] {
        assert!(
            render["required"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value == identity),
            "missing render identity {identity}"
        );
    }
    assert_eq!(
        render["$defs"]["attachmentPreparationOccurrence"]["properties"]["status"]["const"],
        "ready"
    );
    assert!(
        !render["properties"]
            .as_object()
            .unwrap()
            .contains_key("renderer_contract_id")
    );
    assert!(
        render["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "mode_options")
    );
    for identity in [
        "font_artifact_id",
        "object_ref",
        "sha256",
        "media_type",
        "family",
        "script",
    ] {
        assert!(
            render["$defs"]["font"]["required"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value == identity)
        );
    }

    for assessment in [
        "outline-assessment-snapshot-v1.schema.json",
        "submission-assessment-snapshot-v1.schema.json",
    ] {
        let schema = by_name(assessment);
        assert_eq!(
            enum_literals(&schema["properties"]["status"]["enum"]),
            BTreeSet::from(["has_critical_warnings", "has_warnings", "ready"])
        );
    }

    let mutation = by_name("workspace-mutation-v1.schema.json");
    for operation in [
        "bind_fulfillment",
        "remap_fulfillment",
        "unbind_fulfillment",
        "update_document_settings",
    ] {
        assert!(mutation.to_string().contains(operation));
    }
    assert_eq!(
        mutation.to_string().matches("binding_lineage_id").count(),
        4
    );
    for target in ["outline_node", "response_table", "structured_form", "quote"] {
        assert!(mutation.to_string().contains(target));
    }
}
