"""Independent outline planning and compilation from a fixed human outline."""

from copy import deepcopy

import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.request import ModelBudgetError
from docreader.tender import authoring_pipeline as pipeline
from docreader.tender.schema import AuthoringInput, AuthoringOutline

from test_authoring_pipeline_boundaries import budget, ordinary_response, payload, snapshot


async def test_outline_only_uses_current_source_and_assembly_without_writing_or_retrieval(tmp_path):
    artifacts, calls = ArtifactStore(tmp_path), []
    fixed = AuthoringInput.model_validate(snapshot(artifacts)).model_dump()

    class Knowledge:
        async def search(self, *_args):
            raise AssertionError("Outline planning must not retrieve bidder materials.")

    async def execute(task, request, validate):
        value = payload(request)
        calls.append((task, value.get("operation")))
        assert task in {"outline", "authoring_structure"}, "Outline planning must stop before basis and writing."
        return validate(ordinary_response(task, value))

    reference = await pipeline.generate_document(artifacts, fixed, budget(), execute,
                                                  outline_only=True, knowledge=Knowledge())
    outline = AuthoringOutline.model_validate(artifacts.read_json(reference))
    assert outline.authoring_input.model_dump() == fixed
    assert calls == [("outline", None), ("outline", "assemble"), ("authoring_structure", "structure")]
    assert "docx_ref" not in artifacts.read_json(reference)


async def test_outline_only_preserves_local_failure_gaps_in_its_own_artifact(tmp_path):
    artifacts = ArtifactStore(tmp_path)

    async def execute(*_args):
        raise ModelBudgetError("synthetic oversized indivisible source")

    reference = await pipeline.generate_document(artifacts, snapshot(artifacts), budget(), execute,
                                                  outline_only=True)
    outline = AuthoringOutline.model_validate(artifacts.read_json(reference))
    assert "outline_batch_unavailable" in {issue.code for issue in outline.issues}


async def test_fixed_outline_skips_organization_and_preserves_identity_order_and_nested_goals(tmp_path):
    artifacts, requests = ArtifactStore(tmp_path), []
    fixed = AuthoringInput.model_validate(snapshot(artifacts)).model_dump()
    outline = AuthoringOutline(schema_version=2, authoring_input=fixed, issues=[], chapters=[
        {"chapter_id": "human-parent", "title": "人工调整的技术篇", "targets": [], "children": [
            {"chapter_id": "stable-chapter", "title": "实施方法", "children": [], "targets": [
                {"target_id": "stable-target", "kind": "narrative", "purpose": "按人工要求编写实施方法",
                 "rule_refs": [], "source_refs": []}]}]},
        {"chapter_id": "kept-empty", "title": "人工留空", "targets": [], "children": []},
    ]).model_dump()
    before = deepcopy(outline)

    async def execute(task, request, validate):
        assert task != "outline", "A fixed outline must never be reorganized by the model."
        requests.append(payload(request))
        if task == "authoring_structure":
            return validate({"findings": [{"code": "check_structure", "detail": "人工目录位置仍需核对。",
                "suggestion": "由人工决定是否调整已有章节。", "can_revise": False}]})
        return validate(ordinary_response(task, payload(request)))

    result = artifacts.read_json(await pipeline.generate_document(
        artifacts, fixed, budget(), execute, outline=outline,
        materials=[{"material_id": "provided", "title": "实施资料", "text": "已有实施方法供参考。"}]))
    assert artifacts.read_json(result["outline_ref"]) == before
    assert outline == before
    assert any(value.get("title_path") == ["人工调整的技术篇", "实施方法"] for value in requests)
    unit = artifacts.read_json(result["unit_refs"][0])
    assert unit["chapter_id"] == "stable-chapter" and unit["target_ids"] == ["stable-target"]


async def test_fixed_empty_outline_is_not_repopulated_with_fallback_content(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    fixed = AuthoringInput.model_validate(snapshot(artifacts)).model_dump()
    outline = AuthoringOutline(schema_version=2, authoring_input=fixed, issues=[], chapters=[]).model_dump()

    async def forbidden_model_call(*_args):
        raise AssertionError("A manually empty outline has no writing targets.")

    result = artifacts.read_json(await pipeline.generate_document(
        artifacts, fixed, budget(), forbidden_model_call, outline=outline))
    assert artifacts.read_json(result["outline_ref"]) == outline
    assert result["unit_refs"] == []
    assert "outline_empty" in {gap["code"] for gap in result["gaps"]}
    assert artifacts.read_bytes(result["docx_ref"]).startswith(b"PK")


async def test_fixed_outline_from_another_snapshot_is_rejected_before_model_work(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    fixed = AuthoringInput.model_validate(snapshot(artifacts)).model_dump()
    other = {**fixed, "source_version": 2}
    outline = AuthoringOutline(schema_version=2, authoring_input=other, issues=[], chapters=[]).model_dump()

    async def forbidden_model_call(*_args):
        raise AssertionError("Mismatched outline input must not reach model work.")

    with pytest.raises(ValueError, match="snapshot"):
        await pipeline.generate_document(artifacts, fixed, budget(), forbidden_model_call, outline=outline)


async def test_fixed_outline_preserves_real_rule_reference_optional_fields(tmp_path):
    from test_outline_organization import context, response, target

    artifacts, fixed, source_context = await context(tmp_path)
    outline = source_context.validate(response([
        target([next(iter(source_context.rules))], range_ids=[next(iter(source_context.sources))])]))

    async def forbidden_model_call(*_args):
        raise AssertionError("A supplied outline needs no additional planning call.")

    reference = await pipeline.generate_document(artifacts, fixed, budget(), forbidden_model_call,
                                                  outline=outline, outline_only=True)
    saved = artifacts.read_json(reference)
    validated = AuthoringOutline.model_validate(saved)
    assert saved["chapters"] == outline["chapters"]
    assert validated.chapters[0].targets[0].rule_refs
