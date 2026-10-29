"""One semantic revision preserves successful content, identities, and paid limits."""

import asyncio
from copy import deepcopy
import json

import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.budget import RequestBudget
from docreader.llm.request import ModelWorkUnitExhausted
from docreader.tender.authoring_pipeline import bind_writing_response
from docreader.tender.authoring_revision import _previous_content, revise_units


def task(identity="w1"):
    return {"task_id": identity, "chapter_id": "chapter-1", "title_path": ["技术方案"],
            "targets": [{"target_id": identity + ":target", "kind": "narrative", "purpose": "具体应答",
                         "source_refs": [], "rule_refs": []}]}


def unit(identity="w1", **fields):
    return {"schema_version": 2, "task_id": identity, "chapter_id": "chapter-1",
            "target_ids": [identity + ":target"], "status": "generated",
            "blocks": [{"kind": "paragraph", "text": "已有成功正文", "target_ids": [identity + ":target"],
                        "material_ids": ["method"]}], "cell_fills": [],
            "attachments": [], "gaps": [], "error_type": None, **fields}


def finding(identity="w1", **fields):
    return {"code": "response_imprecise", "detail": "原文已有明确步骤，当前正文过于概括。",
            "suggestion": "依据给定步骤细化。", "task_ids": [identity], "can_revise": True, **fields}


def payload(_task):
    return {"title_path": _task["title_path"], "supports": [
                {"support_id": "k1", "origin": "knowledge", "text": material()["text"], "target_ids": ["t1"]}],
            "targets": [{"target_id": "t1", "support_ids": ["k1"]}]}, {}, {
                "k1": {"origin": "knowledge", "material_id": "method"}}


def material():
    return {"material_id": "method", "title": "方法资料", "text": "已有步骤和实施方法。",
            "usage": "reference", "locator": {}, "attachment": False, "original_ref": None}


def response(text):
    return {"blocks": [{"kind": "paragraph", "text": text,
                        "target_ids": ["t1"], "support_ids": ["k1"]}]}


def bind(_task, raw):
    return bind_writing_response(_task, raw, {}, [material()])


def budget():
    return RequestBudget(context_tokens=131072, model="fixture", prompts={})


async def test_revise_supported_content_and_semantic_empty_once_and_preserve_old_artifact(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    original = [unit(), unit("w2"), unit("w3", status="placeholder", blocks=[])]
    before = deepcopy(original)
    calls = []

    async def execute(stage, request, validate):
        calls.append((stage, json.loads(request["messages"][1]["content"][0]["text"])))
        return validate(response("已有步骤形成的完整修订稿"))

    updated, old_refs, diagnostics = await revise_units(artifacts, budget(), execute,
        [task(), task("w2"), task("w3")], original,
        [finding(content_ids=["b1"]), finding(code="insufficient_detail"),
         finding("w2", can_revise=False), finding("w3")], payload, bind)
    assert len(calls) == 2 and calls[0][0] == "authoring_write"
    assert updated[0]["blocks"][0]["text"] == "已有步骤形成的完整修订稿"
    assert updated[1] == original[1] and original == before
    assert updated[2]["status"] == "generated"
    assert len(old_refs) == 2 and artifacts.read_json(old_refs[0]) == original[0]
    assert not diagnostics
    previous = calls[0][1]["previous_content"]
    assert previous["blocks"] == [{"kind": "paragraph", "text": "已有成功正文", "target_ids": ["t1"],
                                    "support_ids": ["k1"], "content_id": "b1"}]
    assert not {"task_id", "chapter_id", "target_ids", "artifact_ref", "source_ref"} & previous.keys()
    assert len(calls[0][1]["review_findings"]) == 2
    assert calls[0][1]["review_findings"][0]["content_ids"] == ["b1"]
    assert "w1:target" not in json.dumps(previous)
    assert "material_ids" not in json.dumps(previous)


@pytest.mark.parametrize("reason", ["manual", "unsupported", "runtime_failure"])
async def test_empty_without_semantic_revision_basis_never_spends_a_call(tmp_path, reason):
    current_task, current = task(), unit(status="placeholder", blocks=[])
    if reason == "manual":
        current_task["targets"][0]["manual_fill"] = True
    if reason == "runtime_failure":
        current["error_type"] = "ModelWorkUnitExhausted"

    def make_payload(selected):
        value, sources, bindings = payload(selected)
        if reason == "unsupported":
            value["supports"] = []
            value["targets"][0]["support_ids"] = []
            bindings = {}
        return value, sources, bindings

    async def execute(*_args):
        pytest.fail("manual, unsupported and runtime-failed blanks must not be semantically rewritten")

    result, old_refs, _ = await revise_units(ArtifactStore(tmp_path), budget(), execute,
        [current_task], [current], [finding()], make_payload, bind)
    assert result == [current] and old_refs == []


async def test_semantic_empty_result_is_revised_from_existing_support(tmp_path):
    calls = []

    async def execute(stage, request, validate):
        calls.append(stage)
        return unit()

    result, old_refs, _ = await revise_units(ArtifactStore(tmp_path), budget(), execute,
        [task()], [unit(status="placeholder", blocks=[])], [finding()], payload, bind)
    assert calls == ["authoring_write"]
    assert result[0]["status"] == "generated" and len(old_refs) == 1


async def test_previous_template_and_attachment_use_current_short_references(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    ref_a = {"source_index_ref": artifacts.put_json({"fixture": "first"}), "unit_id": "grid:a"}
    ref_b = {"source_index_ref": artifacts.put_json({"fixture": "second"}), "unit_id": "grid:b"}
    proof_ref = artifacts.put_bytes(b"real synthetic original")
    source_map = {"s7": ref_b, "s3": ref_a}
    proof_material = {"material_id": "proof", "title": "原件", "text": "原件证明本次功能适用。",
                      "original_ref": proof_ref, "attachment": True, "usage": "proof", "locator": {}}
    current = unit(cell_fills=[{"source_ref": ref_a, "row": 1, "column": 1, "text": "甲表值",
                               "target_ids": ["w1:target"], "material_ids": ["method"], "source_refs": []},
                              {"source_ref": ref_b, "row": 1, "column": 1, "text": "乙表值",
                               "target_ids": ["w1:target"], "material_ids": ["method"], "source_refs": []}],
                   attachments=[{"material_id": "proof", "title": "原件", "artifact_ref": proof_ref}])

    def make_payload(_task):
        value, _, bindings = payload(_task)
        value["targets"][0]["template_support_ids"] = ["s3", "s7"]
        template_grid = {"row_count": 2, "column_count": 2, "cells": [
            {"row": 1, "column": 1, "row_span": 1, "col_span": 1, "text": ""}]}
        value["supports"].extend([
            {"support_id": "s7", "origin": "tender", "text": "乙表", "target_ids": ["t1"],
             "grid": template_grid},
            {"support_id": "s3", "origin": "tender", "text": "甲表", "target_ids": ["t1"],
             "grid": template_grid},
            {"support_id": "k2", "origin": "knowledge", "text": proof_material["text"],
             "attachment": True, "target_ids": ["t1"]}])
        bindings.update(s7={"origin": "tender", "source_ref": ref_b},
                        s3={"origin": "tender", "source_ref": ref_a},
                        k2={"origin": "knowledge", "material_id": "proof"})
        return value, source_map, bindings

    async def execute(stage, request, validate):
        value = json.loads(request["messages"][1]["content"][0]["text"])
        previous = value["previous_content"]
        assert [f["source_id"] for f in previous["cell_fills"]] == ["s3", "s7"]
        assert [f["content_id"] for f in previous["cell_fills"]] == ["f1", "f2"]
        assert all(f["target_ids"] == ["t1"] and f["support_ids"] == ["k1"]
                   for f in previous["cell_fills"])
        assert previous["attachments"] == [{"content_id": "a1", "attachment_id": "k2", "title": "原件"}]
        assert "sha256" not in json.dumps(previous) and "grid:a" not in json.dumps(previous)
        return validate({**response("修订后正文"),
                         "cell_fills": [{key: value for key, value in fill.items() if key != "content_id"}
                                        for fill in previous["cell_fills"]], "attachment_ids": ["k2"]})

    template_task = task()
    template_task['targets'][0].update(kind='form', source_refs=[ref_a, ref_b],
        template_refs=[{"source_ref": reference, "selectors": []} for reference in (ref_a, ref_b)])
    result, old_refs, issues = await revise_units(artifacts, budget(), execute, [template_task], [current], [finding()],
        make_payload, lambda t, raw: bind_writing_response(t, raw, source_map, [material(), proof_material]))
    assert result[0]["cell_fills"] == current["cell_fills"]
    assert result[0]["attachments"] == current["attachments"] and len(old_refs) == 1 and not issues


@pytest.mark.parametrize("failed", [True, False])
async def test_exhausted_or_empty_revision_preserves_successful_original(tmp_path, failed):
    artifacts, calls = ArtifactStore(tmp_path), []
    current = unit()

    async def execute(stage, request, validate):
        calls.append(stage)
        if failed:
            raise ModelWorkUnitExhausted(stage)
        return validate({"blocks": []})

    revised, old_refs, issues = await revise_units(artifacts, budget(), execute,
        [task()], [current], [finding()], payload, bind)
    assert revised == [current] and old_refs == [] and len(calls) == 1
    assert issues and issues[0]["task_id"] == "w1" and issues[0]["chapter_id"] == "chapter-1"


async def test_unknown_price_or_certificate_never_enters_revision(tmp_path):
    async def execute(*_args):
        pytest.fail("missing evidence must not spend a revision call")

    current = unit(gaps=["缺证书原件，价格未知。"])
    result, old_refs, _ = await revise_units(ArtifactStore(tmp_path), budget(), execute, [task()], [current],
        [finding(can_revise=False, detail="缺证书原件，价格未知。")], payload, bind)
    assert result == [current] and old_refs == []


async def test_oversized_revision_keeps_original_without_a_model_send(tmp_path):
    async def execute(*_args):
        pytest.fail("failed admission must not send")

    def oversized(_task):
        value, sources, bindings = payload(_task)
        return {**value, "text": "长" * 100000}, sources, bindings

    current = unit()
    result, old_refs, issues = await revise_units(ArtifactStore(tmp_path), budget(), execute,
        [task()], [current], [finding()], oversized, bind)
    assert result == [current] and old_refs == [] and issues


async def test_cancellation_propagates_instead_of_reporting_success(tmp_path):
    async def execute(*_args):
        raise asyncio.CancelledError()

    with pytest.raises(asyncio.CancelledError):
        await revise_units(ArtifactStore(tmp_path), budget(), execute, [task()], [unit()], [finding()], payload, bind)


async def test_unbound_previous_template_keeps_original_instead_of_losing_fill(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    ref = {"source_index_ref": artifacts.put_json({"fixture": "template"}), "unit_id": "grid:a"}
    current = unit(cell_fills=[{"source_ref": ref, "row": 1, "column": 1, "text": "有效旧填值"}])

    async def execute(*_args):
        pytest.fail("cannot revise while silently dropping an unmapped old fill")

    result, old_refs, issues = await revise_units(artifacts, budget(), execute,
        [task()], [current], [finding()], payload, bind)
    assert result == [current] and not old_refs and issues


async def test_empty_generated_replacement_does_not_replace_nonempty_draft(tmp_path):
    current = unit()

    async def execute(stage, request, validate):
        return validate({})

    result, old_refs, issues = await revise_units(ArtifactStore(tmp_path), budget(), execute,
        [task()], [current], [finding()], payload, lambda *_: unit(blocks=[]))
    assert result == [current] and not old_refs and issues


async def test_one_exhausted_revision_does_not_stop_later_supported_task(tmp_path):
    artifacts, calls = ArtifactStore(tmp_path), []

    async def execute(stage, request, validate):
        calls.append(stage)
        if len(calls) == 1:
            raise ModelWorkUnitExhausted(stage)
        return validate(response("第二单元修订正文"))

    originals = [unit(), unit("w2")]
    result, old_refs, issues = await revise_units(artifacts, budget(), execute,
        [task(), task("w2")], originals, [finding(), finding("w2")], payload, bind)
    assert result[0] == originals[0] and result[1]["blocks"][0]["text"] == "第二单元修订正文"
    assert len(calls) == 2 and len(old_refs) == 1 and artifacts.read_json(old_refs[0]) == originals[1]
    assert issues[0]["task_id"] == "w1"


async def test_missing_previous_proof_binding_keeps_original_attachment(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    proof_ref = artifacts.put_bytes(b"original proof")
    original = unit(attachments=[{"material_id": "proof", "title": "真实证书", "artifact_ref": proof_ref}])

    async def execute(*_args):
        pytest.fail("an unavailable original binding must not be silently removed by rewriting")

    result, old_refs, issues = await revise_units(artifacts, budget(), execute,
        [task()], [original], [finding()], payload, bind)
    assert result == [original] and not old_refs and issues


def test_previous_support_rebinds_reordered_targets_and_preserves_materials():
    current_task = task()
    current_task["targets"].insert(0, {**current_task["targets"][0], "target_id": "other-target"})
    current_payload, sources, bindings = payload(current_task)
    current_payload["targets"] = [{"target_id": "t1"}, {"target_id": "t2"}]
    previous = _previous_content(current_task, unit(), current_payload, sources, bindings)
    assert previous["blocks"][0]["target_ids"] == ["t2"]
    assert previous["blocks"][0]["support_ids"] == ["k1"]
    assert previous["blocks"][0]["content_id"] == "b1"


def test_previous_source_background_uses_local_support_without_persistent_metadata(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    reference = {"source_index_ref": artifacts.put_json({"fixture": "background"}), "unit_id": "source:background"}
    current = unit(blocks=[{"kind": "paragraph", "text": "项目位于甲园区。", "target_ids": ["w1:target"],
                           "material_ids": [], "source_refs": [reference]}])
    value = {"targets": [{"target_id": "t1", "support_ids": ["s1"]}], "supports": [
        {"support_id": "s1", "origin": "tender", "text": "项目位于甲园区。", "target_ids": ["t1"]}]}
    previous = _previous_content(task(), current, value, {"s1": reference}, {
        "s1": {"origin": "tender", "source_ref": reference}})
    assert previous["blocks"] == [{"kind": "paragraph", "text": "项目位于甲园区。", "target_ids": ["t1"],
                                    "content_id": "b1", "support_ids": ["s1"]}]
    assert not any(key in json.dumps(previous) for key in ("sha256", "source_refs", "material_ids", "source:background"))


@pytest.mark.parametrize("field, identity", [("target_ids", "obsolete-target"), ("material_ids", "obsolete-material")])
async def test_unavailable_previous_support_keeps_original_without_silent_reference_loss(tmp_path, field, identity):
    current = unit()
    current["blocks"][0][field] = [identity]

    async def execute(*_args):
        pytest.fail("do not rewrite an old response while silently discarding unavailable support")

    result, old_refs, issues = await revise_units(ArtifactStore(tmp_path), budget(), execute,
        [task()], [current], [finding()], payload, bind)
    assert result == [current] and not old_refs
    assert issues[0]["code"] == "authoring_revision_source_unavailable"
