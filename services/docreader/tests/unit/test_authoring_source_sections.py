"""Request-local heading projection preserves every original source and anchor."""

from copy import deepcopy
import json
from types import SimpleNamespace

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.budget import RequestBudget
from docreader.llm.wire import WirePayload
from docreader.tender.authoring import _OutlineContext, organize_outline
from docreader.tender.authoring_pipeline import _organize, _outline_payload
from docreader.tender.authoring_templates import prepare_template_parts
from docreader.tender.evidence.references import SourceIndex
from docreader.tests.submission_responses import assemble_independent_candidates

from test_structural_packing import source


SHARED_PATH = "招标文件 > 规定格式 > 人员及服务安排"
OTHER_PATH = "补充文件 > 规定格式 > 人员及服务安排"


def assert_sections(payload, expected):
    paths = {entry["section_id"]: entry["heading_path"] for entry in payload["sections"]}
    assert len(paths) == len(set(expected.values()))
    assert [paths[entry["section_id"]] for entry in payload["sources"]] == [
        expected[entry["text"]] for entry in payload["sources"]]
    assert all("heading_path" not in entry for entry in payload["sources"])


def test_request_shares_complete_paths_without_mutating_sources_or_text():
    records = [{"source": {"range_id": f"range-{i}", "text": text, "heading_path": path}, "rules": []}
               for i, (text, path) in enumerate([
                   ("原始第一段\n保留换行和  空白。", SHARED_PATH),
                   ("原始第二段", SHARED_PATH), ("同名末级但不同完整路径", OTHER_PATH), ("无标题原文", "")])]
    before = deepcopy(records)
    context = SimpleNamespace(input={"issues": []}, payload={"rules": []})
    payload = _outline_payload(context, records)
    expected = {record["source"]["text"]: record["source"]["heading_path"] for record in records}
    assert_sections(payload, expected)
    assert payload["sources"] == [
        {**{key: value for key, value in record["source"].items() if key != "heading_path"},
         "section_id": payload["sources"][i]["section_id"]} for i, record in enumerate(records)]
    assert records == before
    encoded = json.dumps(payload, ensure_ascii=False)
    assert encoded.count(SHARED_PATH) == encoded.count(OTHER_PATH) == 1
    wire = WirePayload("outline", payload)
    assert_sections(wire.payload, expected)
    assert wire.decode({"range_ids": [row["range_id"] for row in wire.payload["sources"]]}) == {
        "range_ids": [row["source"]["range_id"] for row in records]}


def frozen_source(artifacts):
    native = source([{"text": "第一段原文。\n第二段原文。", "heading": SHARED_PATH},
                     {"text": "另一完整章节原文。", "heading": OTHER_PATH}])
    native = SourceIndex({**native.document, "original_ref": artifacts.put_bytes(b"synthetic original")}, native.raw)
    index_ref = artifacts.put_json(native.descriptor(artifacts.put_json(native.raw)))
    snapshot = {"schema_version": 2, "run_id": "section-projection", "source_version": 1, "domain_revision": 1,
                "frozen_ref": artifacts.put_json({"schema_version": 2, "documents": [
                    {**native.document, "role": "main", "source_index_ref": index_ref}]})}
    return snapshot


def source_response(payload):
    return {"schema_version": 2, "chapters": [{"title": "需填写的规定格式", "children": [], "targets": [
        {"kind": "form", "purpose": "按给定原格式保留填写位置", "rule_ids": [],
         "range_ids": [entry["range_id"] for entry in payload["sources"]],
         "template_part_ids": [entry["part_id"] for entry in payload["template_parts"]]}]}], "findings": []}


def inspect_source_payload(payload):
    assert_sections(payload, {"第一段原文。": SHARED_PATH, "第二段原文。": SHARED_PATH,
                              "另一完整章节原文。": OTHER_PATH})
    assert [entry["text"] for entry in payload["sources"]] == ["第一段原文。", "第二段原文。", "另一完整章节原文。"]
    supplied = {entry["range_id"] for entry in payload["sources"]}
    assert all(set(part["range_ids"]) <= supplied for part in payload["template_parts"])


async def test_document_source_pass_retains_template_anchors_after_heading_projection(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    snapshot = frozen_source(artifacts)
    expected_context = _OutlineContext(artifacts, snapshot)
    prepare_template_parts(artifacts, expected_context)
    calls = []

    async def execute(stage, request, validate):
        assert stage == "outline"
        payload = json.loads(request["messages"][1]["content"][0]["text"])
        calls.append(payload.get("operation", "source"))
        if payload.get("operation") == "assemble":
            return validate(assemble_independent_candidates(payload))
        inspect_source_payload(payload)
        return validate(source_response(payload))

    outline = await _organize(artifacts, _OutlineContext(artifacts, snapshot),
        RequestBudget(model="fixture", context_tokens=131072), execute, [])
    assert calls == ["source", "assemble"]
    assert outline["chapters"][0]["targets"][0]["template_refs"] == list(expected_context.template_parts.values())


async def test_independent_outline_entry_uses_the_same_shared_sections(tmp_path, monkeypatch):
    artifacts = ArtifactStore(tmp_path)
    snapshot = frozen_source(artifacts)
    expected_context = _OutlineContext(artifacts, snapshot)
    prepare_template_parts(artifacts, expected_context)

    async def active(*args):
        return None

    async def execute(request, validate):
        payload = json.loads(request["messages"][1]["content"][0]["text"])
        inspect_source_payload(payload)
        return validate(source_response(payload))

    monkeypatch.setattr("docreader.tender.authoring.assert_authoring_active", active)
    reference = await organize_outline(None, artifacts, execute,
        RequestBudget(model="fixture", context_tokens=131072, output_reserve=16384),
        run_id=snapshot["run_id"], source_version=1, domain_revision=1, authoring_input=snapshot)
    outline = artifacts.read_json(reference)
    assert outline["chapters"][0]["targets"][0]["template_refs"] == list(expected_context.template_parts.values())
