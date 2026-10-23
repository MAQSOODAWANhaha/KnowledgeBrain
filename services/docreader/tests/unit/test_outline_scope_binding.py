"""Visible scope anchors remain selectable after source text deduplication."""

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.wire import WirePayload
from docreader.tender.authoring import _OutlineContext
from docreader.tender.evidence.context import condition_context
from docreader.tender.evidence.packing import SupportingIndex
from docreader.tender.evidence.references import SourceIndex

from test_outline_source_binding import form_response, outline_context
from test_structural_packing import source


def native_source(artifacts, specs):
    native = source(specs)
    return SourceIndex({**native.document,
                        "original_ref": artifacts.put_bytes(b"synthetic scope original")}, native.raw)


def with_scope_metadata(artifacts, context, **metadata):
    plan = artifacts.read_json(context.input["pack_plan_ref"])
    pack = artifacts.read_json(plan["packs"][0]["artifact_ref"])
    plan["packs"][0]["artifact_ref"] = artifacts.put_json({**pack, **metadata})
    return _OutlineContext(artifacts, {**context.input, "pack_plan_ref": artifacts.put_json(plan)})


def select_visible_anchor(context, field):
    wire = WirePayload("outline", context.payload)
    anchor = wire.payload["source_views"][0][field][0]["range_id"]
    response = form_response(context, [anchor])
    response["chapters"][0]["targets"][0]["rule_ids"] = [
        wire.mapping["e"][next(iter(context.rules))]]
    return context.validate(wire.decode(response))


def test_body_covered_scope_binding_anchor_selects_its_original_native_unit(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    index = native_source(artifacts, [{"text": "本表仅适用于联合体。"}, {"text": "必须提交证明。"}])
    _, conditions, _, _ = condition_context(index.ranges)
    candidate = next(value for entries in conditions.values() for _, value in entries)
    decision = {"range_id": candidate["range_id"], "decision": "bind",
                "target_range_ids": [index.ranges[1]["range_id"]]}
    supporting, pending, _, bindings = SupportingIndex(index.ranges, [decision], analysis=True).select(index.ranges)
    assert supporting == [] and pending == []
    assert bindings[0]["range_id"] == candidate["range_id"]
    assert candidate["range_id"] not in {entry["range_id"] for entry in index.ranges}
    context, source_ref = outline_context(artifacts, index, evidence_source=index.ranges[0])
    context = with_scope_metadata(artifacts, context, scope_bindings=bindings)
    view = context.payload["source_views"][0]
    assert view["scope_bindings"][0]["range_id"] == candidate["range_id"]
    assert any(entry.get("text") == index.ranges[0]["text"] for entry in view["body"])

    outlined = select_visible_anchor(context, "scope_bindings")
    assert outlined["chapters"][0]["targets"][0]["source_refs"] == [
        {"source_index_ref": source_ref, "unit_id": index.ranges[0]["unit_id"]}]
    assert not any(issue["code"] == "outline_source_not_read" for issue in outlined["issues"])


def test_pending_scope_metadata_anchor_is_selectable_without_claiming_unread_text(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    index = native_source(artifacts, [{"text": "必须提交证明。"}, {"text": "本表仅适用于联合体"}])
    supporting, pending, _, _ = SupportingIndex(index.ranges).select(index.ranges)
    assert supporting == [] and len(pending) == 1
    assert "text" not in pending[0]
    assert pending[0]["source_range_id"] == index.ranges[1]["range_id"]
    context, source_ref = outline_context(artifacts, index, evidence_source=index.ranges[0])
    context = with_scope_metadata(artifacts, context, pending_scope_context=pending)
    view = context.payload["source_views"][0]
    assert view["pending_scope_context"][0]["range_id"] == pending[0]["range_id"]
    assert index.ranges[1]["range_id"] not in {entry["range_id"] for entry in view["body"]}

    outlined = select_visible_anchor(context, "pending_scope_context")
    target = outlined["chapters"][0]["targets"][0]
    assert target["source_refs"] == [
        {"source_index_ref": source_ref, "unit_id": index.ranges[1]["unit_id"]}]
    assert any(issue["code"] == "outline_source_not_read" and issue["target_id"] == target["target_id"]
               for issue in outlined["issues"])
