"""An explicit successor repairs organization without fixing the degraded outline."""

import asyncio
from collections import Counter
from copy import deepcopy
from io import BytesIO
import json
from types import SimpleNamespace
from uuid import uuid4

from docx import Document
import httpx
import pytest

from docreader.integrations.authoring_store import AuthoringStore
from docreader.tender import authoring_pipeline as pipeline
from docreader.tender.document_execution import execute_document
from docreader.tender.evidence.references import SourceIndex
from docreader.tender.schema import AuthoringInput
from docreader.tests.submission_responses import assemble_independent_candidates

from test_analysis import analysis_runtime as analysis_runtime
from test_authoring_execution import config, response
from test_authoring_outline import budget


pytestmark = pytest.mark.integration

DECLARATION = "本单位承诺依照招标文件要求履行义务。签署人：________。"
REQUIREMENT = "系统须支持双机热备。"


def source_snapshot(artifacts, run):
    units = [{"unit_id": f"unit:{i}", "kind": "section", "ordinal": i, "text": text,
              "grid": None, "locator": {"locator_kind": "document", "section_ordinal": i,
                                        "page_ordinal": i}, "physical_path": f"native:unit:{i}"}
             for i, text in enumerate((DECLARATION, REQUIREMENT))]
    native = SourceIndex({"document_id": "synthetic", "document_version": "one",
                          "original_ref": artifacts.put_bytes(b"synthetic frozen source")}, {
        "schema_version": 2, "units": units, "images": {}, "ocr": {},
        "source_contract": {"units": [{"unit_id": u["unit_id"], "section_id": f"section-{i}",
            "parent_section_id": None, "heading_path": "", "physical_locator": u["locator"]}
            for i, u in enumerate(units)], "page_manifest": []}})
    raw_ref = artifacts.put_json(native.raw)
    index_ref = artifacts.put_json(native.descriptor(raw_ref))
    frozen_ref = artifacts.put_json({"schema_version": 2, "documents": [{**native.document,
        "role": "main", "raw_ref": raw_ref, "source_index_ref": index_ref}]})
    return AuthoringInput(schema_version=2, frozen_ref=frozen_ref,
        **{key: run[key] for key in ("run_id", "source_version", "domain_revision")}).model_dump()


async def test_successor_reuses_accepted_source_and_rebuilds_failed_source_and_assembly(
        analysis_runtime, monkeypatch):
    analysis, artifacts, settings, run, _ = analysis_runtime
    snapshot = source_snapshot(artifacts, run)
    input_ref = artifacts.put_json(snapshot)
    first, second = uuid4().hex, uuid4().hex
    recovered = False
    calls = Counter()
    original_fits = pipeline._fits

    def fits(request_budget, task, payload, **options):
        # Two real source units are sufficient; no huge filler is needed to
        # exercise the production planner's batch boundary.
        if task == "outline" and "sources" in payload:
            return len(payload["sources"]) <= 1
        return original_fits(request_budget, task, payload, **options)

    async def no_delay(_seconds):
        return None

    monkeypatch.setattr(pipeline, "_fits", fits)
    monkeypatch.setattr("docreader.llm.recovery.asyncio",
                        SimpleNamespace(sleep=no_delay, to_thread=asyncio.to_thread))

    def handler(request):
        payload = json.loads(json.loads(request.content)["messages"][1]["content"][0]["text"])
        operation = payload.get("operation")
        if operation == "assemble":
            calls["assemble"] += 1
            if not recovered:
                return httpx.Response(500, json={"error": "synthetic assembly outage"})
            return response(assemble_independent_candidates(payload))
        if operation == "structure":
            calls["structure"] += 1
            return response({"findings": []})
        assert "sources" in payload, "Missing knowledge must leave bidder responses blank."
        assert len(payload["sources"]) == 1
        source = payload["sources"][0]
        declaration = source["text"] == DECLARATION
        calls["declaration" if declaration else "requirement"] += 1
        if not declaration and not recovered:
            return httpx.Response(500, json={"error": "synthetic source outage"})
        target = {"kind": "form" if declaration else "narrative", "rule_ids": [],
                  "range_ids": [], "purpose": "保留签署位置" if declaration else "逐项技术应答"}
        if declaration:
            assert payload["template_parts"]
            target.update(manual_fill=True, template_part_ids=[p["part_id"] for p in payload["template_parts"]])
        else:
            assert source["text"] == REQUIREMENT
            target["response_table"] = {"rows": [{"range_ids": [source["range_id"]]}]}
        return response({"schema_version": 2, "chapters": [{
            "title": "投标声明" if declaration else "技术响应", "children": [], "targets": [target]}],
            "findings": []})

    async def execute(identity, **options):
        return await execute_document(analysis, artifacts, config(), budget(), execution_id=identity,
            **{key: snapshot[key] for key in ("run_id", "source_version", "domain_revision")},
            authoring_input_ref=input_ref, transport=httpx.MockTransport(handler), **options)

    first_ref = await execute(first)
    first_document = artifacts.read_json(first_ref)
    original_word = artifacts.read_bytes(first_document["docx_ref"])
    store = AuthoringStore(settings.dsn)
    original_execution = deepcopy(await store.get_execution(first))
    original_plan = next(r for r in original_execution["receipts"] if r["stage"] == "document-plan")
    assert original_execution["status"] == "succeeded"
    assert calls == {"declaration": 1, "requirement": 4, "assemble": 4, "structure": 1}
    assert {"outline_batch_unavailable", "outline_assembly_unavailable"} <= {
        gap["code"] for gap in first_document["gaps"]}
    assert await execute(first) == first_ref
    assert sum(calls.values()) == 10, "Reopening a completed execution must not create new paid calls."

    recovered = True
    second_ref = await execute(second, previous_execution_id=first)
    document = artifacts.read_json(second_ref)
    successor = await store.get_execution(second)
    assert successor["authoring_input"] == original_execution["authoring_input"] == snapshot
    assert successor["execution_spec"]["authoring_input_ref"] == input_ref
    assert successor["execution_spec"].get("outline_ref") is None
    assert calls == {"declaration": 1, "requirement": 5, "assemble": 5, "structure": 2}
    assert document["usage"]["sent_requests"] == 3
    inherited = [r for r in successor["receipts"] if r["stage"] == "document-model-response"
                 and r["metadata"].get("reused_from_execution_id") == first]
    accepted_source = next(r for r in original_execution["receipts"]
                           if r["stage"] == "document-model-response" and r["scope"].startswith("outline:"))
    assert len(inherited) == 1
    assert inherited[0]["artifact_ref"] == accepted_source["artifact_ref"]
    new_plan = next(r for r in successor["receipts"] if r["stage"] == "document-plan")
    assert new_plan["artifact_ref"] != original_plan["artifact_ref"]
    assert len(artifacts.read_json(new_plan["artifact_ref"])["tasks"]) == 2
    assert len(artifacts.read_json(original_plan["artifact_ref"])["tasks"]) == 1
    assert [c["title"] for c in artifacts.read_json(document["outline_ref"])["chapters"]] == [
        "投标声明", "技术响应"]
    assert not {"outline_batch_unavailable", "outline_assembly_unavailable"} & {
        gap["code"] for gap in document["gaps"]}
    word = Document(BytesIO(artifacts.read_bytes(document["docx_ref"])))
    assert sum(p.text.count(DECLARATION) for p in word.paragraphs) == 1
    assert len(word.tables) == 1
    assert [cell.text for cell in word.tables[0].rows[1].cells] == [REQUIREMENT, ""]
    assert artifacts.read_bytes(first_document["docx_ref"]) == original_word
    assert await store.get_execution(first) == original_execution
