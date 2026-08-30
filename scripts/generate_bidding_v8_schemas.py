#!/usr/bin/env python3
"""Generate the immutable internal JSON Schemas for Outline Agent V8."""

import json
from copy import deepcopy
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SCHEMAS = ROOT / "crates/bidding/schemas"
DRAFT = "https://json-schema.org/draft/2020-12/schema"
UUID = {"type": "string", "format": "uuid"}
SHA = {"type": "string", "pattern": "^[a-f0-9]{64}$"}
CLIENT_REF = {"type": "string", "pattern": "^[A-Za-z0-9][A-Za-z0-9_.:-]{0,127}$"}
SEMANTIC_ROLE = {"enum": ["qualification", "technical", "commercial", "quotation", "deviation", "implementation", "evidence_index", "attachment", "other"]}
SECTION_ROLE = {"enum": ["qualification", "technical", "commercial", "quotation", "attachment", "other"]}
CHANNEL = {"enum": ["narrative_content", "response_table", "deviation_statement", "structured_form", "evidence_attachment", "quotation"]}
MATERIALIZATION = {"enum": ["explicit_child", "bind_existing", "audit_only"]}
APPLICABILITY = {"enum": ["required", "optional", "conditional", "not_applicable"]}


def write(name, value):
    (SCHEMAS / name).write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n")


def load(name):
    try:
        return json.loads((SCHEMAS / name).read_text())
    except (OSError, json.JSONDecodeError) as error:
        raise SystemExit(f"cannot load source schema {name}: {error}") from error


map_v3 = load("outline-evidence-batch-v3.schema.json")
map_v4 = deepcopy(map_v3)
map_v4["$id"] = "https://knowledgebrain.local/schemas/outline-evidence-batch-v4.schema.json"
map_v4["title"] = "OutlineEvidenceBatchV4"
map_v4["properties"]["schema_version"] = {"const": 4}
map_v4["required"].remove("requirement_route_hints")
map_v4["properties"].pop("requirement_route_hints")
map_v4["$defs"].pop("routeHint", None)
fragment = map_v4["$defs"]["structureFragment"]
fragment["required"] += ["fulfillment_group_key", "fulfillment_group_title", "materialization"]
fragment["properties"].update({
    "fulfillment_group_key": {"anyOf": [{"type": "string", "minLength": 1, "maxLength": 128}, {"type": "null"}]},
    "fulfillment_group_title": {"anyOf": [{"type": "string", "minLength": 1, "maxLength": 1024}, {"type": "null"}]},
    "materialization": MATERIALIZATION,
})
fragment["allOf"] = [{
    "if": {"properties": {"outline_usage": {"enum": ["output_child", "form_template"]}}, "required": ["outline_usage"]},
    "then": {
        "properties": {
            "fulfillment_group_key": {"type": "string", "minLength": 1, "maxLength": 128},
            "fulfillment_group_title": {"type": "string", "minLength": 1, "maxLength": 1024},
        },
        "allOf": [{
            "if": {"properties": {"applicability": {"const": "not_applicable"}}, "required": ["applicability"]},
            "then": {"properties": {"materialization": {"const": "audit_only"}}},
            "else": {"properties": {"materialization": {"enum": ["explicit_child", "bind_existing"]}}},
        }],
    },
    "else": {"properties": {"materialization": {"const": "audit_only"}}},
}]
write("outline-evidence-batch-v4.schema.json", map_v4)

assignment = {
    "type": "object", "additionalProperties": False,
    "required": ["need_occurrence_id", "channel", "section_role", "fulfillment_group_key", "fulfillment_group_title", "materialization", "applicability", "requiredness", "source_unit_revision_ids", "confidence"],
    "properties": {
        "need_occurrence_id": UUID, "channel": CHANNEL, "section_role": SECTION_ROLE,
        "fulfillment_group_key": {"type": "string", "minLength": 1, "maxLength": 128},
        "fulfillment_group_title": {"type": "string", "minLength": 1, "maxLength": 1024},
        "materialization": MATERIALIZATION, "applicability": APPLICABILITY,
        "requiredness": {"enum": ["mandatory", "optional", "informational"]},
        "source_unit_revision_ids": {"type": "array", "minItems": 1, "maxItems": 1000, "uniqueItems": True, "items": UUID},
        "confidence": {"enum": ["high", "medium", "low"]},
    },
    "allOf": [{
        "if": {"properties": {"requiredness": {"const": "mandatory"}, "applicability": {"not": {"const": "not_applicable"}}}, "required": ["requiredness", "applicability"]},
        "then": {"properties": {"materialization": {"enum": ["explicit_child", "bind_existing"]}}},
    }, {
        "if": {"properties": {"applicability": {"const": "not_applicable"}}, "required": ["applicability"]},
        "then": {"properties": {"materialization": {"const": "audit_only"}}},
    }],
}
grouping = {
    "$schema": DRAFT, "$id": "https://knowledgebrain.local/schemas/requirement-grouping-batch-v1.schema.json", "title": "RequirementGroupingBatchV1",
    "type": "object", "additionalProperties": False,
    "required": ["schema_version", "batch_ordinal", "home_need_occurrence_ids", "assignments", "notices"],
    "properties": {
        "schema_version": {"const": 1}, "batch_ordinal": {"type": "integer", "minimum": 0},
        "home_need_occurrence_ids": {"type": "array", "minItems": 1, "maxItems": 64, "uniqueItems": True, "items": UUID},
        "assignments": {"type": "array", "minItems": 1, "maxItems": 64, "items": assignment},
        "notices": {"type": "array", "maxItems": 128, "items": {"type": "object", "additionalProperties": False, "required": ["code", "message", "source_identity"], "properties": {"code": {"type": "string", "minLength": 1, "maxLength": 128}, "message": {"type": "string", "minLength": 1, "maxLength": 2048}, "source_identity": {"type": "string", "minLength": 1, "maxLength": 256}}}},
    },
    "$defs": {"assignment": assignment},
}
write("requirement-grouping-batch-v1.schema.json", grouping)

group = {
    "$schema": DRAFT, "$id": "https://knowledgebrain.local/schemas/fulfillment-group-v1.schema.json", "title": "FulfillmentGroupV1",
    "type": "object", "additionalProperties": False,
    "required": ["group_ref", "group_key", "title", "section_ref", "semantic_role", "materialization", "requiredness", "applicability", "need_occurrences", "source_unit_revision_ids", "structured_form_revision_ids", "fragment_refs"],
    "properties": {
        "group_ref": SHA, "group_key": {"type": "string", "minLength": 1, "maxLength": 128}, "title": {"type": "string", "minLength": 1, "maxLength": 1024},
        "section_ref": SHA, "semantic_role": SEMANTIC_ROLE, "materialization": MATERIALIZATION,
        "requiredness": {"enum": ["mandatory", "optional", "informational"]}, "applicability": APPLICABILITY,
        "need_occurrences": {"type": "array", "maxItems": 10000, "uniqueItems": True, "items": {"type": "object", "additionalProperties": False, "required": ["need_occurrence_id", "channel"], "properties": {"need_occurrence_id": UUID, "channel": CHANNEL}}},
        "source_unit_revision_ids": {"type": "array", "minItems": 1, "maxItems": 10000, "uniqueItems": True, "items": UUID},
        "structured_form_revision_ids": {"type": "array", "maxItems": 10000, "uniqueItems": True, "items": UUID},
        "fragment_refs": {"type": "array", "maxItems": 10000, "uniqueItems": True, "items": SHA},
    },
}
write("fulfillment-group-v1.schema.json", group)

matrix_v2 = {
    "$schema": DRAFT, "$id": "https://knowledgebrain.local/schemas/section-obligation-matrix-v2.schema.json", "title": "SectionObligationMatrixV2",
    "type": "object", "additionalProperties": False, "required": ["schema_version", "sections"],
    "properties": {"schema_version": {"const": 2}, "sections": {"type": "array", "minItems": 1, "maxItems": 64, "items": {"type": "object", "additionalProperties": False, "required": ["section_ref", "required_group_refs", "conditional_group_refs", "excluded_group_refs"], "properties": {"section_ref": SHA, "required_group_refs": {"type": "array", "maxItems": 10000, "uniqueItems": True, "items": SHA}, "conditional_group_refs": {"type": "array", "maxItems": 10000, "uniqueItems": True, "items": SHA}, "excluded_group_refs": {"type": "array", "maxItems": 10000, "uniqueItems": True, "items": SHA}}}}},
}
write("section-obligation-matrix-v2.schema.json", matrix_v2)

reduce_v2 = load("outline-reduce-plan-v2.schema.json")
reduce_v3 = deepcopy(reduce_v2)
reduce_v3["$id"] = "https://knowledgebrain.local/schemas/outline-reduce-plan-v3.schema.json"
reduce_v3["title"] = "OutlineReducePlanV3"
reduce_v3["properties"]["schema_version"] = {"const": 3}
reduce_v3["required"].remove("requirement_routes")
reduce_v3["properties"].pop("requirement_routes")
reduce_v3["$defs"].pop("routeHint", None)
reduce_v3["required"].append("fulfillment_groups")
reduce_v3["properties"]["fulfillment_groups"] = {"type": "array", "minItems": 1, "maxItems": 10000, "items": {"$ref": group["$id"]}}
reduce_v3["properties"]["section_obligation_matrix"] = {"$ref": matrix_v2["$id"]}
write("outline-reduce-plan-v3.schema.json", reduce_v3)

final_output = load("outline-generation-output-v2.schema.json")
draft_node = deepcopy(final_output["$defs"]["node"])
draft_node["required"].append("coverage_group_refs")
draft_node["properties"]["coverage_group_refs"] = {"type": "array", "maxItems": 10000, "uniqueItems": True, "items": SHA}
patch = {
    "$schema": DRAFT, "$id": "https://knowledgebrain.local/schemas/outline-draft-patch-v1.schema.json", "title": "OutlineDraftPatchV1",
    "type": "object", "additionalProperties": False, "required": ["schema_version", "patch_ref", "base_draft_sha256", "add_nodes", "replace_nodes", "delete_node_refs"],
    "properties": {"schema_version": {"const": 1}, "patch_ref": CLIENT_REF, "base_draft_sha256": SHA,
        "add_nodes": {"type": "array", "maxItems": 64, "items": {"$ref": "#/$defs/draftNode"}},
        "replace_nodes": {"type": "array", "maxItems": 64, "items": {"type": "object", "additionalProperties": False, "required": ["client_node_ref", "replacement"], "properties": {"client_node_ref": CLIENT_REF, "replacement": {"$ref": "#/$defs/draftNode"}}}},
        "delete_node_refs": {"type": "array", "maxItems": 64, "uniqueItems": True, "items": CLIENT_REF}},
    "$defs": {**deepcopy(final_output["$defs"]), "draftNode": draft_node},
}
write("outline-draft-patch-v1.schema.json", patch)

invalid_assignment = {
    "type": "object", "additionalProperties": False,
    "required": ["code", "identity", "message"],
    "properties": {"code": {"type": "string", "minLength": 1, "maxLength": 128}, "identity": {"type": "string", "minLength": 1, "maxLength": 256}, "message": {"type": "string", "minLength": 1, "maxLength": 2048}},
}
patch_receipt = {
    "type": "object", "additionalProperties": False,
    "required": ["patch_ref", "patch_sha256", "base_draft_sha256", "result_draft_sha256", "accepted"],
    "properties": {"patch_ref": CLIENT_REF, "patch_sha256": SHA, "base_draft_sha256": SHA, "result_draft_sha256": SHA, "accepted": {"type": "boolean"}},
}
closure = {
    "type": "object", "additionalProperties": False,
    "required": ["required_groups_total", "required_groups_assigned", "missing_group_refs", "invalid_assignments", "draft_sha256"],
    "properties": {
        "required_groups_total": {"type": "integer", "minimum": 0}, "required_groups_assigned": {"type": "integer", "minimum": 0},
        "missing_group_refs": {"type": "array", "maxItems": 10000, "uniqueItems": True, "items": SHA},
        "invalid_assignments": {"type": "array", "maxItems": 10000, "items": {"$ref": "#/$defs/invalidAssignment"}},
        "draft_sha256": SHA,
    },
}
packet_v2 = load("outline-synthesis-packet-v2.schema.json")
packet_defs = deepcopy(packet_v2["$defs"])
for obsolete in ["route", "nodeIndex", "draft"]:
    packet_defs.pop(obsolete, None)
packet_defs.update({"draftNode": draft_node, "invalidAssignment": invalid_assignment, "patchReceipt": patch_receipt, "closureFacts": closure})
packet_v3 = {
    "$schema": DRAFT, "$id": "https://knowledgebrain.local/schemas/outline-synthesis-packet-v3.schema.json", "title": "OutlineSynthesisPacketV3",
    "type": "object", "additionalProperties": False,
    "required": ["schema_version", "request_artifact_id", "frozen_input_sha256", "reduce_plan_sha256", "map_evidence_set_sha256", "grouping_evidence_set_sha256", "composition_spine", "section_obligation_matrix", "fulfillment_groups", "deterministic_spine_nodes", "manifest", "selected_evidence", "selected_facts", "draft"],
    "properties": {
        "schema_version": {"const": 3}, "request_artifact_id": UUID, "frozen_input_sha256": SHA, "reduce_plan_sha256": SHA,
        "map_evidence_set_sha256": SHA, "grouping_evidence_set_sha256": SHA,
        "composition_spine": deepcopy(packet_v2["properties"]["composition_spine"]),
        "section_obligation_matrix": {"$ref": matrix_v2["$id"]},
        "fulfillment_groups": {"type": "array", "minItems": 1, "maxItems": 10000, "items": {"$ref": group["$id"]}},
        "deterministic_spine_nodes": {"type": "array", "minItems": 1, "maxItems": 64, "items": {"$ref": "#/$defs/node"}},
        "manifest": deepcopy(packet_v2["properties"]["manifest"]),
        "selected_evidence": deepcopy(packet_v2["properties"]["selected_evidence"]),
        "selected_facts": deepcopy(packet_v2["properties"]["selected_facts"]),
        "draft": {"type": "object", "additionalProperties": False, "required": ["draft_sha256", "nodes", "patch_receipts", "closure_facts"], "properties": {"draft_sha256": SHA, "nodes": {"type": "array", "maxItems": 10000, "items": {"$ref": "#/$defs/draftNode"}}, "patch_receipts": {"type": "array", "maxItems": 10000, "items": {"$ref": "#/$defs/patchReceipt"}}, "closure_facts": {"$ref": "#/$defs/closureFacts"}}},
    },
    "$defs": packet_defs,
}
write("outline-synthesis-packet-v3.schema.json", packet_v3)

checkpoint_v2 = load("outline-synthesis-checkpoint-v2.schema.json")
checkpoint_defs = deepcopy(checkpoint_v2["$defs"])
for obsolete in ["route", "obligationBinding", "nodeChunk", "routeChunk", "obligationBindingChunk"]:
    checkpoint_defs.pop(obsolete, None)
checkpoint_defs.update({"draftNode": draft_node, "invalidAssignment": invalid_assignment, "patchReceipt": patch_receipt, "closureFacts": closure})
checkpoint_v3 = {
    "$schema": DRAFT, "$id": "https://knowledgebrain.local/schemas/outline-synthesis-checkpoint-v3.schema.json", "title": "OutlineSynthesisCheckpointV3",
    "type": "object", "additionalProperties": False,
    "required": ["schema_version", "attempt", "phase", "reduce_plan_sha256", "selected_evidence", "selected_facts", "nodes", "patch_receipts", "closure_facts", "total_turns", "total_tool_calls", "text_bytes_read", "images_read"],
    "properties": {
        "schema_version": {"const": 3}, "attempt": {"type": "integer", "minimum": 1}, "phase": {"enum": ["collecting", "drafting", "verifying", "repairing"]}, "reduce_plan_sha256": SHA,
        "selected_evidence": deepcopy(checkpoint_v2["properties"]["selected_evidence"]), "selected_facts": deepcopy(checkpoint_v2["properties"]["selected_facts"]),
        "nodes": {"type": "array", "maxItems": 10000, "items": {"$ref": "#/$defs/draftNode"}},
        "patch_receipts": {"type": "array", "maxItems": 10000, "items": {"$ref": "#/$defs/patchReceipt"}}, "closure_facts": {"$ref": "#/$defs/closureFacts"},
        "total_turns": {"type": "integer", "minimum": 0}, "total_tool_calls": {"type": "integer", "minimum": 0}, "text_bytes_read": {"type": "integer", "minimum": 0}, "images_read": {"type": "integer", "minimum": 0},
    },
    "$defs": checkpoint_defs,
}
write("outline-synthesis-checkpoint-v3.schema.json", checkpoint_v3)
