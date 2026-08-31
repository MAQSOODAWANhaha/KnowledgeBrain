use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

const SCHEMAS: &[(&str, &str, &str)] = &[
    (
        "content-block-v1.schema.json",
        include_str!("../schemas/content-block-v1.schema.json"),
        "cec5813fe6cbeb4f407df62bc63198d29a33a3d354917dc50843147fba89313d",
    ),
    (
        "content-generation-input-v1.schema.json",
        include_str!("../schemas/content-generation-input-v1.schema.json"),
        "a7d7a0eef5295da68243d07cf2a001588ea8b28e8d3bee2247cb08cdcc2da290",
    ),
    (
        "content-generation-output-v1.schema.json",
        include_str!("../schemas/content-generation-output-v1.schema.json"),
        "14187ee75ad1c275e45f830a106273fdad92b9423f3062912ba45c7f94b0fccd",
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
        "outline-evidence-batch-v4.schema.json",
        include_str!("../schemas/outline-evidence-batch-v4.schema.json"),
        "7bbce8e569fd3ebdc5c359eed40ced5ff956ebde16afb619f933d64ede48d491",
    ),
    (
        "requirement-grouping-batch-v1.schema.json",
        include_str!("../schemas/requirement-grouping-batch-v1.schema.json"),
        "8e14274f36be10b3bbfd5e5977e84f91fdacddaba62517f93668d5ed4148e4b3",
    ),
    (
        "requirement-grouping-batch-v2.schema.json",
        include_str!("../schemas/requirement-grouping-batch-v2.schema.json"),
        "c910ab4ad445561246e77b7f41dcf7c43d60d3584b6a08618b628db53ee19485",
    ),
    (
        "requirement-grouping-batch-v3.schema.json",
        include_str!("../schemas/requirement-grouping-batch-v3.schema.json"),
        "404797ad65f3f28943d3bd8d64c54eec4f4cafee46004e3f070304a6e3bec62a",
    ),
    (
        "requirement-grouping-batch-v4.schema.json",
        include_str!("../schemas/requirement-grouping-batch-v4.schema.json"),
        "34b23ad45ea7b246f486f7608699b740eb5259df2fdb6852eaaa98ff63c9c818",
    ),
    (
        "requirement-grouping-batch-v5.schema.json",
        include_str!("../schemas/requirement-grouping-batch-v5.schema.json"),
        "0ffa1eeef1d559d519faa67026c62384eb27915c91c4f5cf68e7ecb5b30daa24",
    ),
    (
        "fulfillment-group-v1.schema.json",
        include_str!("../schemas/fulfillment-group-v1.schema.json"),
        "a193c82ee2679c7ffb3740bc2a09007b9e8faaceb5a7008f3aa572f5f1d55ade",
    ),
    (
        "section-obligation-matrix-v2.schema.json",
        include_str!("../schemas/section-obligation-matrix-v2.schema.json"),
        "3fa4301e84041f0c81bddcdff2542f441754303abe0418f0daf964d3923d5367",
    ),
    (
        "outline-reduce-plan-v3.schema.json",
        include_str!("../schemas/outline-reduce-plan-v3.schema.json"),
        "1a9a1d4a77135f1766620ed986a644ed1d0a100f758025d91dd48b1afbd8e96d",
    ),
    (
        "outline-draft-patch-v1.schema.json",
        include_str!("../schemas/outline-draft-patch-v1.schema.json"),
        "f6e8dfd46010f7ac4b32b47521ece77a7a71f0bab8efd9a7072475b44a027835",
    ),
    (
        "outline-synthesis-packet-v3.schema.json",
        include_str!("../schemas/outline-synthesis-packet-v3.schema.json"),
        "2744a867ae0296026eca18a161caef637f98f30d03382d82b9edf3313b2706bd",
    ),
    (
        "outline-synthesis-packet-v4.schema.json",
        include_str!("../schemas/outline-synthesis-packet-v4.schema.json"),
        "360820632b42ee9b9d14ef589ffbd36aab965244db1935be8b9537ff2793cec8",
    ),
    (
        "outline-synthesis-packet-v5.schema.json",
        include_str!("../schemas/outline-synthesis-packet-v5.schema.json"),
        "f7bb17fd01090667afc2d84b50cc357b276019d06cb0215c76a18419b3ad73a0",
    ),
    (
        "outline-synthesis-checkpoint-v3.schema.json",
        include_str!("../schemas/outline-synthesis-checkpoint-v3.schema.json"),
        "04ab83f88bd55e78ccb642bcd43284ff10101e8119c590e7e6da4a687b9cd2ba",
    ),
    (
        "outline-synthesis-checkpoint-v4.schema.json",
        include_str!("../schemas/outline-synthesis-checkpoint-v4.schema.json"),
        "9dd1014eb5c6a7dd18de46310738fc2d9cf38141632653639d4a3c1757fbf10f",
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
        "bafc92c867a251c44e6144401eb43855be6fcb7459e4809edef40038d89f3f56",
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
    assert_eq!(SCHEMAS.len(), 38);
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

    let map_v4 = by_name("outline-evidence-batch-v4.schema.json");
    assert_eq!(map_v4["properties"]["schema_version"]["const"], 4);
    assert!(
        map_v4["properties"]
            .get("requirement_route_hints")
            .is_none()
    );
    for property in [
        "fulfillment_group_key",
        "fulfillment_group_title",
        "materialization",
    ] {
        assert!(
            map_v4["$defs"]["structureFragment"]["required"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value == property)
        );
    }
    let grouping_v1 = by_name("requirement-grouping-batch-v1.schema.json");
    assert_eq!(grouping_v1["properties"]["assignments"]["maxItems"], 64);
    let grouping_v2 = by_name("requirement-grouping-batch-v2.schema.json");
    assert_eq!(grouping_v2["properties"]["assignments"]["maxItems"], 48);
    assert!(
        grouping_v2["$defs"]["assignment"]["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "section_ref")
    );
    let grouping_v3 = by_name("requirement-grouping-batch-v3.schema.json");
    let v3_required = grouping_v3["$defs"]["assignment"]["required"]
        .as_array()
        .unwrap();
    assert!(v3_required.iter().any(|value| value == "section_ref"));
    for frozen_echo in [
        "channel",
        "applicability",
        "requiredness",
        "source_unit_revision_ids",
    ] {
        assert!(!v3_required.iter().any(|value| value == frozen_echo));
    }
    let grouping_v4 = by_name("requirement-grouping-batch-v4.schema.json");
    assert_eq!(
        grouping_v4["properties"]["structure_placements"]["maxItems"],
        48
    );
    assert!(
        grouping_v4["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "home_structure_fragment_refs")
    );
    let grouping_v5 = by_name("requirement-grouping-batch-v5.schema.json");
    for semantic_field in [
        "fulfillment_group_key",
        "fulfillment_group_title",
        "materialization",
    ] {
        assert!(
            grouping_v5["$defs"]["structurePlacement"]["required"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value == semantic_field)
        );
    }
    let group_v1 = by_name("fulfillment-group-v1.schema.json");
    assert!(
        group_v1["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "need_occurrences")
    );
    let matrix_v2 = by_name("section-obligation-matrix-v2.schema.json");
    assert!(matrix_v2.to_string().contains("required_group_refs"));
    let reduce_v3 = by_name("outline-reduce-plan-v3.schema.json");
    assert!(reduce_v3["properties"].get("requirement_routes").is_none());
    assert!(
        reduce_v3["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "fulfillment_groups")
    );
    let patch_v1 = by_name("outline-draft-patch-v1.schema.json");
    assert!(patch_v1.to_string().contains("coverage_group_refs"));
    assert!(patch_v1.to_string().contains("base_draft_sha256"));
    let packet_v3 = by_name("outline-synthesis-packet-v3.schema.json");
    let checkpoint_v3 = by_name("outline-synthesis-checkpoint-v3.schema.json");
    let packet_v4 = by_name("outline-synthesis-packet-v4.schema.json");
    let packet_v5 = by_name("outline-synthesis-packet-v5.schema.json");
    let checkpoint_v4 = by_name("outline-synthesis-checkpoint-v4.schema.json");
    assert!(
        packet_v5["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "non_output_fragments")
    );
    assert_eq!(
        packet_v5["properties"]["non_output_fragments"]["items"]["properties"]["outline_usage"]["enum"],
        json!(["requirement_context", "reference_only"])
    );
    assert!(
        packet_v4["$defs"]["closureFacts"]["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "empty_section_refs")
    );
    assert!(
        checkpoint_v4["$defs"]["closureFacts"]["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "empty_section_refs")
    );
    assert!(!packet_v3.to_string().contains("route_chunk_count"));
    assert!(!checkpoint_v3.to_string().contains("accepted_routes"));
    assert!(
        !checkpoint_v3
            .to_string()
            .contains("accepted_obligation_bindings")
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
    assert!(!render.to_string().contains("include_knowledge_sources"));
    assert!(!render.to_string().contains("include_assessment_notices"));
    let workspace_mutation = by_name("workspace-mutation-v1.schema.json").to_string();
    assert!(!workspace_mutation.contains("acknowledge_stale"));
    assert!(!workspace_mutation.contains("insertion_anchor"));
    assert!(!content_input.to_string().contains("utf8_offset"));
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

    let content_output = by_name("content-generation-output-v1.schema.json");
    assert_eq!(
        content_output["$defs"]["operation"]["properties"]["kind"]["const"],
        "insert_block"
    );
    assert!(!content_output.to_string().contains("append_to_block"));
    assert!(!content_output.to_string().contains("insert_at_anchor"));

    let content_block = by_name("content-block-v1.schema.json");
    let link_pattern = content_block["$defs"]["mark"]["oneOf"][1]["properties"]["href"]["pattern"]
        .as_str()
        .expect("link pattern");
    assert!(link_pattern.contains("[^/\\s:@?#]+"));

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
