#!/usr/bin/env python3
"""Draft 2020-12 positive/negative contract tests for all Phase 0 schemas."""
from __future__ import annotations

import copy
import hashlib
import json
from pathlib import Path
from typing import Any

from jsonschema import Draft202012Validator, FormatChecker
from referencing import Registry, Resource  # type: ignore[import-not-found]

ROOT = Path(__file__).resolve().parents[1]
SCHEMA_DIR = ROOT / "crates/bidding/schemas"
UUID = "00000000-0000-4000-8000-000000000001"
SHA = "a" * 64


def load_schema(path: Path) -> dict[str, Any]:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise SystemExit(f"cannot load JSON Schema {path}: {exc}") from exc


SCHEMAS = {
    path.name: load_schema(path)
    for path in sorted(SCHEMA_DIR.glob("*.schema.json"))
}
SCHEMAS_BY_ID = {schema["$id"]: schema for schema in SCHEMAS.values()}
REGISTRY = Registry().with_resources(
    (schema_id, Resource.from_contents(schema))
    for schema_id, schema in SCHEMAS_BY_ID.items()
)


def deref(ref: str, root: dict[str, Any]) -> tuple[dict[str, Any], dict[str, Any]]:
    if ref.startswith("#/$defs/"):
        return root["$defs"][ref.rsplit("/", 1)[1]], root
    target = SCHEMAS_BY_ID[ref]
    return target, target


def merge_schema(base: dict[str, Any], overlay: dict[str, Any]) -> dict[str, Any]:
    merged = copy.deepcopy(base)
    for key, value in overlay.items():
        if isinstance(value, dict) and isinstance(merged.get(key), dict):
            merged[key] = merge_schema(merged[key], value)
        else:
            merged[key] = copy.deepcopy(value)
    return merged


def sample(schema: dict[str, Any], root: dict[str, Any]) -> Any:
    if "$ref" in schema:
        target, target_root = deref(schema["$ref"], root)
        return sample(target, target_root)
    if "const" in schema:
        return schema["const"]
    if "enum" in schema:
        return schema["enum"][0]
    if "allOf" in schema:
        merged = {key: value for key, value in schema.items() if key != "allOf"}
        for item in schema["allOf"]:
            merged = merge_schema(merged, item)
        return sample(merged, root)
    if "oneOf" in schema:
        if "type" in schema:
            merged = {key: value for key, value in schema.items() if key != "oneOf"}
            return sample(merge_schema(merged, schema["oneOf"][0]), root)
        return sample(schema["oneOf"][0], root)
    if "anyOf" in schema:
        non_null = next(
            (item for item in schema["anyOf"] if item.get("type") != "null"),
            schema["anyOf"][0],
        )
        if "type" in schema:
            merged = {key: value for key, value in schema.items() if key != "anyOf"}
            return sample(merge_schema(merged, non_null), root)
        return sample(non_null, root)
    kind = schema.get("type")
    if isinstance(kind, list):
        non_null_kind = next((item for item in kind if item != "null"), kind[0])
        return sample({**schema, "type": non_null_kind}, root)
    if kind == "object":
        properties = schema.get("properties", {})
        return {name: sample(properties[name], root) for name in schema.get("required", [])}
    if kind == "array":
        return [sample(schema["items"], root) for _ in range(schema.get("minItems", 0))]
    if kind == "string":
        if schema.get("format") == "uuid":
            return UUID
        if schema.get("format") == "date-time":
            return "2026-01-01T00:00:00Z"
        if schema.get("pattern") in {"^[0-9a-f]{64}$", "^[a-f0-9]{64}$"}:
            return SHA
        if schema.get("pattern", "").startswith("^objects/"):
            return "objects/" + SHA
        return "x" * max(1, schema.get("minLength", 1))
    if kind == "integer":
        return schema.get("minimum", 0)
    if kind == "number":
        return schema.get("minimum", schema.get("exclusiveMinimum", 0) + 1)
    if kind == "boolean":
        return False
    if kind == "null":
        return None
    raise AssertionError(f"cannot synthesize sample for {schema}")


def validator(
    name: str,
) -> tuple[Draft202012Validator, dict[str, Any], dict[str, Any]]:
    schema = SCHEMAS[name]
    Draft202012Validator.check_schema(schema)
    instance = sample(schema, schema)
    if name == "render-document-snapshot-v2.schema.json":
        instance["mode_options"]["watermark"] = None
    if name == "evidence-bundle-v1.schema.json":
        quote = instance["items"][0]["quote_utf8"].encode("utf-8")
        instance["items"][0]["quote_sha256"] = hashlib.sha256(quote).hexdigest()
    return (
        Draft202012Validator(
            schema,
            registry=REGISTRY,
            format_checker=FormatChecker(),
        ),
        schema,
        instance,
    )


def rejected(v: Draft202012Validator, instance: dict[str, Any], label: str) -> None:
    errors = list(v.iter_errors(instance))
    assert errors, f"negative instance unexpectedly accepted: {label}"


def variant(schema: dict[str, Any], kind: str) -> dict[str, Any]:
    for candidate in schema["oneOf"]:
        if candidate.get("properties", {}).get("kind", {}).get("const") == kind:
            return candidate
    raise AssertionError(f"missing {kind} variant")


def main() -> None:
    assert len(SCHEMAS) == 38
    cases: dict[str, tuple[Draft202012Validator, dict[str, Any], dict[str, Any]]] = {}
    for name in SCHEMAS:
        v, schema, instance = validator(name)
        v.validate(instance)
        bad = copy.deepcopy(instance)
        bad["unexpected"] = True
        rejected(v, bad, f"{name} closes its root object")
        cases[name] = (v, schema, instance)

    v, _, base = cases["outline-generation-input-v1.schema.json"]
    bad = copy.deepcopy(base)
    bad.pop("workspace_scope_revision_id")
    rejected(v, bad, "outline scope revision required")
    bad = copy.deepcopy(base)
    bad["source_units"] = [{
        "source_unit_revision_id": UUID,
        "kind": "section",
        "text": "x",
        "source_span_sha256": SHA,
        "disposition": "accepted",
    }]
    rejected(v, bad, "closed disposition")
    for identity in [
        "prompt_contract_id",
        "template_contract_id",
        "template_contract_sha256",
        "model_contract_id",
        "agent_contract_id",
        "agent_contract_sha256",
    ]:
        bad = copy.deepcopy(base)
        bad.pop(identity)
        rejected(v, bad, f"outline generation identity required: {identity}")

    v, schema, base = cases["outline-generation-output-v1.schema.json"]
    bad = copy.deepcopy(base)
    bad["nodes"] = []
    rejected(v, bad, "outline output requires a node")
    bad = copy.deepcopy(base)
    bad["nodes"][0]["semantic_role"] = "fixed_part"
    rejected(v, bad, "dynamic semantic role is closed")

    v, _, base = cases["content-generation-input-v1.schema.json"]
    bad = copy.deepcopy(base)
    bad.pop("outline_checkpoint_id")
    rejected(v, bad, "checkpoint required")
    bad = copy.deepcopy(base)
    bad["target"].pop("node_revision_id")
    rejected(v, bad, "typed target revision required")
    bad = copy.deepcopy(base)
    bad["selection_mode"] = "user_pick_set"
    rejected(v, bad, "selection mode and frozen input must agree")
    for identity in [
        "prompt_contract_id",
        "template_contract_id",
        "template_contract_sha256",
        "model_contract_id",
        "agent_contract_id",
        "agent_contract_sha256",
    ]:
        bad = copy.deepcopy(base)
        bad.pop(identity)
        rejected(v, bad, f"content generation identity required: {identity}")
    bad = copy.deepcopy(base)
    bad["evidence_selection_input"].pop("matching_policy_id")
    rejected(v, bad, "system matching policy ID required")

    v, schema, base = cases["content-block-v1.schema.json"]
    bad = copy.deepcopy(base)
    bad["kind"] = "image"
    rejected(v, bad, "content block kind and payload must agree")
    bad = copy.deepcopy(base)
    bad["content_sha256"] = "not-a-digest"
    rejected(v, bad, "content block digest is closed")

    v, schema, base = cases["content-generation-output-v1.schema.json"]
    block_schema = SCHEMAS["content-block-v1.schema.json"]
    block = sample(block_schema, block_schema)
    insert = sample(schema["$defs"]["operation"], schema)
    assert insert["kind"] == "insert_block"
    insert["block"] = block
    good = copy.deepcopy(base)
    good["operations"] = [insert]
    v.validate(good)  # proves the external content-block $ref resolves
    bad = copy.deepcopy(good)
    bad["operations"][0]["block"].pop("content_sha256")
    rejected(v, bad, "external content block contract is enforced")

    v, schema, base = cases["evidence-bundle-v1.schema.json"]
    bad = copy.deepcopy(base)
    bad["items"] = []
    rejected(v, bad, "evidence bundle cannot be empty")
    bad = copy.deepcopy(base)
    bad["items"][0]["kind"] = "tender_input"
    rejected(v, bad, "evidence provenance kinds are closed")
    media = sample(schema["$defs"]["mediaItem"], schema)
    good = copy.deepcopy(base)
    good["items"] = [media]
    v.validate(good)
    for page_ordinal in [-1, 1.5]:
        bad = copy.deepcopy(good)
        bad["items"][0]["page_ordinal"] = page_ordinal
        rejected(v, bad, f"image page ordinal rejected: {page_ordinal}")
    for key in ["kind", "evidence_item_id", "media_type"]:
        bad = copy.deepcopy(good)
        bad["items"][0][key] = None
        rejected(v, bad, f"required image scalar cannot be null: {key}")

    v, schema, base = cases["workspace-mutation-v1.schema.json"]
    insert = sample(variant(schema["$defs"]["operation"], "insert_block"), schema)
    good = copy.deepcopy(base)
    good["operations"] = [insert]
    v.validate(good)  # second external $ref traversal
    bad = copy.deepcopy(good)
    bad["operations"][0]["block"].pop("content_sha256")
    rejected(v, bad, "workspace block mutation validates external block")
    remap = sample(variant(schema["$defs"]["operation"], "remap_fulfillment"), schema)
    remap.pop("binding_lineage_id")
    bad = copy.deepcopy(base)
    bad["operations"] = [remap]
    rejected(v, bad, "remap lineage required")

    for name in [
        "outline-assessment-snapshot-v1.schema.json",
        "submission-assessment-snapshot-v1.schema.json",
    ]:
        v, _, base = cases[name]
        bad = copy.deepcopy(base)
        bad.pop("status")
        rejected(v, bad, f"{name} status required")
        bad = copy.deepcopy(base)
        bad["status"] = "blocked"
        rejected(v, bad, f"{name} advisory status closed")

    v, _, base = cases["render-document-snapshot-v2.schema.json"]
    base["form_definition_occurrences"] = [{
        "form_definition_revision_id": UUID,
        "canonical_sha256": SHA,
    }]
    base["attachment_preparation_occurrences"] = [{
        "attachment_preparation_revision_id": UUID,
        "status": "ready",
        "canonical_sha256": SHA,
    }]
    v.validate(base)

    bad = copy.deepcopy(base)
    bad.pop("workspace_scope_revision_id")
    rejected(v, bad, "render workspace scope revision required")
    for status in ["pending", "failed"]:
        bad = copy.deepcopy(base)
        bad["attachment_preparation_occurrences"][0]["status"] = status
        rejected(v, bad, f"attachment preparation status rejected: {status}")

    bad = copy.deepcopy(base)
    bad.pop("mode_options")
    rejected(v, bad, "mode options required")
    bad = copy.deepcopy(base)
    bad["mode_options"]["watermark"] = "DRAFT"
    rejected(v, bad, "preview watermark forbidden")
    bad = copy.deepcopy(base)
    bad.update(output_mode="submission", format="html")
    rejected(v, bad, "submission html forbidden")
    for key in [
        "content_block_schema_version",
        "render_operation_contract_version",
        "docx_renderer_contract_id",
        "docx_renderer_contract_sha256",
        "pdf_renderer_contract_id",
        "pdf_renderer_contract_sha256",
    ]:
        bad = copy.deepcopy(base)
        bad.pop(key)
        rejected(v, bad, f"render identity required: {key}")
    bad = copy.deepcopy(base)
    bad["content_block_schema_version"] = 2
    rejected(v, bad, "content block schema version frozen")
    bad = copy.deepcopy(base)
    bad["render_operation_contract_version"] = 0
    rejected(v, bad, "render operation contract version positive")
    for key in ["docx_renderer_contract_sha256", "pdf_renderer_contract_sha256"]:
        bad = copy.deepcopy(base)
        bad[key] = "missing-digest"
        rejected(v, bad, f"renderer digest closed: {key}")
    for collection in [
        "form_definition_occurrences",
        "attachment_preparation_occurrences",
    ]:
        bad = copy.deepcopy(base)
        bad[collection][0].pop("canonical_sha256")
        rejected(v, bad, f"occurrence digest required: {collection}")
        bad = copy.deepcopy(base)
        bad[collection][0]["canonical_sha256"] = "missing-digest"
        rejected(v, bad, f"occurrence digest closed: {collection}")

    good = copy.deepcopy(base)
    good["assets"] = [{
        "asset_revision_id": UUID,
        "object_ref": f"objects/{SHA}",
        "sha256": SHA,
        "media_type": "application/json",
        "provenance": "quote_snapshot",
    }]
    v.validate(good)
    bad = copy.deepcopy(good)
    bad["assets"][0]["provenance"] = "render_font"
    rejected(v, bad, "fonts are not render assets")
    bad = copy.deepcopy(good)
    bad["assets"][0]["unknown"] = True
    rejected(v, bad, "render asset is closed")

    good = copy.deepcopy(base)
    good.update(output_mode="review_draft", format="pdf")
    good["mode_options"]["watermark"] = "DRAFT"
    v.validate(good)
    good = copy.deepcopy(base)
    good.update(output_mode="submission", format="pdf")
    good["mode_options"] = {"watermark": None}
    v.validate(good)
    bad = copy.deepcopy(good)
    bad["mode_options"]["watermark"] = "DRAFT"
    rejected(v, bad, "clean submission")

    assert set(cases) == set(SCHEMAS), "every frozen schema must have engine cases"
    print(f"Draft 2020-12 validation: {len(SCHEMAS)} schemas, positive and negative cases: PASS")


if __name__ == "__main__":
    main()
