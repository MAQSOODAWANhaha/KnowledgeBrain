"""Explicit template windows reach the actual Word without intervening evaluation forms."""

from copy import deepcopy
from io import BytesIO
import json
from zipfile import ZipFile

from docx import Document
import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.request import ModelContractError
from docreader.tender.authoring_pipeline import _accept_directory_plan, generate_document
from docreader.tender.directory_compile import compile_directory, documents_from_frozen, outline_from_compiled
from docreader.tender.outline_edit import apply_outline_edit

from test_authoring_pipeline_boundaries import budget, payload, snapshot
from test_directory_compile import fixture_titles


def point(page, line):
    return {"document_id": "document", "page": page, "line": line}


def window(page, start, end):
    return {"document_id": "document", "page": page, "start_line": start, "end_line": end}


def grid(rows):
    return {"row_count": len(rows), "column_count": 2, "cells": [
        {"row": row, "column": column, "text": text, "row_span": 1, "col_span": 1}
        for row, values in enumerate(rows) for column, text in enumerate(values)]}


def source_case(artifacts, groups):
    composition = ["本次提交材料按以下分组组成。"]
    for index, group in enumerate(groups, 1):
        composition.extend([group, f"响应原式{index}"])
    specs = [{"text": "\n".join(composition)}, {"grid": grid([
        ["评标委员会工作记录", "仅供评委使用"], ["EVALUATION_ONLY", "不得作为投标人表单"]])}]
    expected = []
    for index, _group in enumerate(groups, 1):
        condition = f"第{index}项材料须保留完整条件，不得删减。"
        rows = [[f"资料项目{index}", "投标响应"], [f"完整要求{index}及其适用条件。", ""], ["人工填值", ""]]
        specs.extend([{"text": f"响应原式{index}\n{condition}"}, {"grid": grid(rows)}])
        expected.append({"condition": condition, "rows": rows})
    fixed = snapshot(artifacts, specs)

    def plan(slot):
        nodes = [{"id": f"s{slot}-navigation", "heading": point(0, 0), "body_windows": []}]
        scopes = []
        for index, _group in enumerate(groups):
            header_page, table_page = 2 + 2 * index, 3 + 2 * index
            # This node names a required item in the composition. Its real form
            # is after the evaluation worksheet, not the next located heading.
            heading = point(0, 2 + 2 * index) if index == 1 else point(header_page, 0)
            identity = f"s{slot}-response-{index}"
            nodes.append({"id": identity, "heading": heading,
                          "body_windows": [window(header_page, 0, 1), window(table_page, 0, 2)]})
            scopes.append({"title": point(0, 1 + 2 * index), "items": [{
                "node_id": identity, "entry": index, "depth": "self", "presentation": "section"}]})
        nodes.append({"id": f"s{slot}-technical-source", "heading": point(4, 0), "body_windows": []})
        return {"nodes": nodes, "composition": {
            "priority_basis": [window(0, 0, 0)],
            "entries": [{"name": point(0, 2 + 2 * index), "excluded_by": []}
                        for index in range(len(groups))]},
            "scopes": scopes, "source_only": [f"s{slot}-navigation", f"s{slot}-technical-source"]}

    return fixed, plan, expected


class TemplateModel:
    def __init__(self, plans):
        self.plans = plans
        self.calls = []

    async def __call__(self, task, request, validate):
        value = payload(request)
        self.calls.append(str(task))
        if task == "directory_plan":
            chosen = deepcopy(self.plans(value["plan_slot"]))
            for node in chosen['nodes']:
                node.setdefault('response_kind', 'form')
            return validate(chosen)
        if task in {"directory_material", "directory_requirement"}:
            return validate({"assignments": [], **({"chapter_titles": fixture_titles(value)}
                                                   if task == "directory_material" else {})})
        if task == "directory_classification":
            return validate({"items": []})
        assert task == "authoring_structure", "Template compilation must not invent enterprise responses."
        return validate({"findings": []})


def editable(chapters):
    return [{"chapter_id": chapter["chapter_id"], "title": chapter["title"],
             "children": editable(chapter["children"])} for chapter in chapters]


def walk(chapters):
    for chapter in chapters:
        yield chapter
        yield from walk(chapter["children"])


@pytest.mark.parametrize("groups", [
    ("商务文件", "技术文件", "报价文件"),
    ("主体资质材料", "实施和服务材料"),
    ("第一封装", "第二封装", "第三封装", "第四封装"),
])
async def test_explicit_body_survives_saved_outline_into_source_defined_volumes(tmp_path, groups):
    artifacts = ArtifactStore(tmp_path / "artifacts")
    fixed, plans, expected = source_case(artifacts, groups)
    original_frozen = artifacts.read_bytes(fixed["frozen_ref"])
    model = TemplateModel(plans)
    outline_ref = await generate_document(artifacts, fixed, budget(), model, outline_only=True)
    outline = artifacts.read_json(outline_ref)
    original_outline = deepcopy(outline)
    assert model.calls.count("directory_plan") == 2
    assert [chapter["title"] for chapter in outline["chapters"]] == list(groups)
    assert all(chapter["is_volume"] for chapter in outline["chapters"])
    assert [len(chapter["children"]) for chapter in outline["chapters"]] == [1] * len(groups)
    selected = [part["source_ref"]["unit_id"] for chapter in walk(outline["chapters"])
                for target in chapter["targets"] for part in target.get("template_refs", [])]
    assert selected == [f"unit:{3 + 2 * index}" for index in range(len(groups))]
    assert len(selected) == len(set(selected))

    saved = apply_outline_edit(outline, editable(outline["chapters"]))
    saved_ref = artifacts.put_json(saved)
    model.calls.clear()
    result = artifacts.read_json(await generate_document(
        artifacts, saved["authoring_input"], budget(), model, outline=artifacts.read_json(saved_ref)))
    assert artifacts.read_json(result["outline_ref"]) == saved
    assert artifacts.read_json(outline_ref) == original_outline
    assert artifacts.read_bytes(fixed["frozen_ref"]) == original_frozen
    assert not any(task.startswith("directory_") for task in model.calls)
    document = Document(BytesIO(artifacts.read_bytes(result["docx_ref"])))
    assert len(document.tables) == len(groups)
    assert "EVALUATION_ONLY" not in document.element.xml and "评标委员会工作记录" not in document.element.xml
    assert "本次提交材料按以下分组组成。" not in document.element.xml
    for table, source in zip(document.tables, expected):
        assert [[cell.text for cell in row.cells] for row in table.rows] == source["rows"]
        assert source["condition"] in document.element.xml
    assert all(artifacts.read_json(reference)["status"] == "placeholder" for reference in result["unit_refs"])
    with ZipFile(BytesIO(artifacts.read_bytes(result["package_ref"]))) as package:
        manifest = json.loads(package.read("manifest.json"))
        assert [volume["title"] for volume in manifest["volumes"]] == list(groups)
        assert package.read("投标文件.docx") == artifacts.read_bytes(result["docx_ref"])
        for volume, source in zip(manifest["volumes"], expected):
            part = Document(BytesIO(package.read(volume["package_path"])))
            assert len(part.tables) == 1
            assert [[cell.text for cell in row.cells] for row in part.tables[0].rows] == source["rows"]
            assert "EVALUATION_ONLY" not in part.element.xml


async def test_adopted_composition_item_with_empty_body_does_not_absorb_intervening_forms(tmp_path):
    artifacts = ArtifactStore(tmp_path / "artifacts")
    fixed = snapshot(artifacts, [{"text": "响应材料\n技术方案\n"},
        {"grid": grid([["评委专用评分表", "EVALUATION_ONLY"]])},
        {"text": "投标函\n致招标人：保留此处规定原文。"}])
    plan = {"nodes": [{"id": "required", "heading": point(0, 1), "body_windows": []},
                      {"id": "letter", "heading": point(2, 0), "body_windows": [window(2, 0, 1)]}],
            "composition": {"entries": [{"name": point(0, 1)}]},
            "scopes": [{"title": point(0, 0), "items": [
                {"node_id": "required", "entry": 0, "depth": "self", "presentation": "section"},
                {"node_id": "letter", "depth": "self", "presentation": "section"}]}]}
    model = TemplateModel(lambda _slot: deepcopy(plan))
    outline_ref = await generate_document(artifacts, fixed, budget(), model, outline_only=True)
    outline = artifacts.read_json(outline_ref)
    required = outline["chapters"][0]["children"][0]
    assert required["title"] == "技术方案"
    assert not any(target.get("template_refs") for target in required["targets"])
    assert all("EVALUATION_ONLY" not in target["purpose"] for target in required["targets"])
    result = artifacts.read_json(await generate_document(
        artifacts, outline["authoring_input"], budget(), model, outline=outline))
    document = Document(BytesIO(artifacts.read_bytes(result["docx_ref"])))
    assert document.tables == [] and "EVALUATION_ONLY" not in document.element.xml
    assert "致招标人：保留此处规定原文。" in document.element.xml


@pytest.mark.parametrize("explicit_null", [False, True])
def test_fresh_model_plan_requires_explicit_non_null_body_windows(explicit_null):
    node = {"id": "source", "heading": point(0, 0), "response_kind": "form"}
    if explicit_null:
        node["body_windows"] = None
    with pytest.raises(ModelContractError) as error:
        _accept_directory_plan({"nodes": [node]})
    assert any(issue["path"] == ["nodes", 0, "body_windows"] and issue["code"] == (
        "list_type" if explicit_null else "missing") for issue in error.value.errors)


@pytest.mark.parametrize("explicit_null", [False, True])
async def test_raw_legacy_plan_replay_retains_inferred_body_without_new_model_work(tmp_path, explicit_null):
    artifacts = ArtifactStore(tmp_path / "artifacts")
    fixed = snapshot(artifacts, [{"text": "投标函\n致招标人：历史原格式仍须保留。"}])
    node = {"id": "letter", "heading": point(0, 0)}
    if explicit_null:
        node["body_windows"] = None
    legacy = {"nodes": [node], "scopes": [{"items": [
        {"node_id": "letter", "depth": "self", "presentation": "section"}]}]}
    documents = documents_from_frozen(artifacts, artifacts.read_json(fixed["frozen_ref"]))
    compiled = compile_directory(legacy, documents)
    outline = outline_from_compiled(fixed, compiled)
    assert "历史原格式仍须保留" in compiled["chapters"][0]["text"]
    model = TemplateModel(lambda _slot: pytest.fail("Legacy replay must not send a new directory request."))
    result = artifacts.read_json(await generate_document(
        artifacts, outline["authoring_input"], budget(), model, outline=outline))
    document = Document(BytesIO(artifacts.read_bytes(result["docx_ref"])))
    assert "致招标人：历史原格式仍须保留。" in document.element.xml
    assert not any(task.startswith("directory_") for task in model.calls)
