"""Compiled source templates remain usable as fixed outlines for document authoring."""

import asyncio
from copy import deepcopy
from io import BytesIO
from zipfile import ZipFile
import pytest

from docx import Document

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.request import ModelWorkUnitExhausted
from docreader.tender.authoring_pipeline import generate_document
from docreader.tender.outline_edit import apply_outline_edit

from test_authoring_pipeline_boundaries import budget, payload, snapshot
from test_directory_compile import docs, fixture_titles, item, node, plan, window


REQUIREMENT = "系统必须支持双机热备，故障切换不得丢失业务数据。"
CONDITION = "技术指标须达到同等或以上配置。"
ANSWER = "采用双机部署，并通过切换测试核对业务数据完整性。"


def frozen_input(artifacts):
    rows = [["技术要求", "投标响应"], [REQUIREMENT, ""], ["最终报价（元）", ""]]
    grid = {"row_count": len(rows), "column_count": 2, "cells": [
        {"row": row, "column": column, "text": text, "row_span": 1, "col_span": 1}
        for row, values in enumerate(rows) for column, text in enumerate(values)]}
    return snapshot(artifacts, [
        {"text": "投标函\n致招标人：提交投标文件正本一份。"},
        {"text": "技术响应表\n" + CONDITION},
        {"text": "", "grid": grid},
    ])


class DirectoryModel:
    def __init__(self, *, fail_plans=False):
        self.calls = []
        self.fail_plans = fail_plans

    async def __call__(self, task, request, validate):
        value = payload(request)
        self.calls.append((str(task), value))
        assert task != "outline", "A persisted directory must not fall back to global organization."
        if task == "directory_plan":
            if self.fail_plans:
                raise ModelWorkUnitExhausted(task)
            return validate(plan(
                [{**node("letter", 0, 0, document_id="document"),
                  "body_windows": [window(0, 0, 1, "document")]},
                 {**node("technical", 1, 0, document_id="document"),
                  "body_windows": [window(1, 0, 1, "document"), window(2, 0, 2, "document")]}],
                [item("letter"), item("technical")]))
        if task == "directory_requirement":
            return validate({"assignments": [{"document_id": "document", "page": 1,
                "start_line": 1, "end_line": 1, "scope": "chapter", "chapter_ids": ["technical"]}]})
        if task == "directory_material":
            return validate({"assignments": [], "chapter_titles": fixture_titles(value)})
        if task == "directory_classification":
            return validate({"items": []})
        if task == "authoring_basis":
            return validate({"shared_arrangements": []})
        if task == "authoring_write":
            supports = [support for support in value["supports"] if support["origin"] == "knowledge"]
            assert supports and ANSWER in supports[0]["text"]
            return validate({"blocks": [{"kind": "paragraph", "text": ANSWER,
                "target_ids": [target["target_id"] for target in value["targets"]],
                "support_ids": [supports[0]["support_id"]]}]})
        assert task in {"authoring_structure", "authoring_review"}
        return validate({"findings": []})


async def compile_outline(artifacts, model):
    fixed_input = frozen_input(artifacts)
    outline_ref = await generate_document(artifacts, fixed_input, budget(), model, outline_only=True)
    return outline_ref, artifacts.read_json(outline_ref)


def assert_original_table(document):
    assert len(document.tables) == 1
    assert [[cell.text for cell in row.cells] for row in document.tables[0].rows] == [
        ["技术要求", "投标响应"], [REQUIREMENT, ""], ["最终报价（元）", ""]]


async def test_compiled_outline_persists_into_full_docx_and_zip_with_original_requirements_and_table(tmp_path):
    artifacts, model = ArtifactStore(tmp_path / "artifacts"), DirectoryModel()
    outline_ref, outline = await compile_outline(artifacts, model)
    assert [task for task, _ in model.calls].count("directory_plan") == 2
    assert [chapter["title"] for chapter in outline["chapters"]] == ["投标函", "技术响应表"]
    original = deepcopy(outline)
    frozen_bytes = artifacts.read_bytes(outline["authoring_input"]["frozen_ref"])
    model.calls.clear()

    result = artifacts.read_json(await generate_document(
        artifacts, outline["authoring_input"], budget(), model, outline=artifacts.read_json(outline_ref)))

    assert artifacts.read_json(result["outline_ref"]) == original
    assert artifacts.read_json(outline_ref) == original
    assert artifacts.read_bytes(outline["authoring_input"]["frozen_ref"]) == frozen_bytes
    assert not any(task.startswith("directory_") or task == "authoring_write" for task, _ in model.calls)
    document = Document(BytesIO(artifacts.read_bytes(result["docx_ref"])))
    text = "\n".join(paragraph.text for paragraph in document.paragraphs)
    assert "致招标人：提交投标文件正本一份。" in text and CONDITION in text
    assert_original_table(document)
    with ZipFile(BytesIO(artifacts.read_bytes(result["package_ref"]))) as package:
        assert package.read("投标文件.docx") == artifacts.read_bytes(result["docx_ref"])
        assert {"招标要求底稿.json", "覆盖结算.json", "待补事项.json"} <= set(package.namelist())


async def test_human_writing_goal_uses_knowledge_without_replacing_compiled_template_targets(tmp_path):
    artifacts, model = ArtifactStore(tmp_path / "artifacts"), DirectoryModel()
    outline_ref, compiled = await compile_outline(artifacts, model)
    original = deepcopy(compiled)
    goal_id = "manual-86f408bc-2d8b-43f7-a9e0-f344a2e30b83"
    edited = apply_outline_edit(compiled, [
        {"chapter_id": compiled["chapters"][1]["chapter_id"], "title": "技术响应表", "children": []},
        {"chapter_id": goal_id, "title": "实施安排", "writing_goal": "依据已给资料说明双机实施及切换验证。", "children": []},
        {"chapter_id": compiled["chapters"][0]["chapter_id"], "title": "投标函", "children": []},
    ])
    edited_ref = artifacts.put_json(edited)
    model.calls.clear()

    result = artifacts.read_json(await generate_document(
        artifacts, edited["authoring_input"], budget(), model, outline=artifacts.read_json(edited_ref),
        materials=[{"material_id": "controlled-method", "title": "受控实施资料", "text": ANSWER}]))

    adopted_outline = artifacts.read_json(result["outline_ref"])
    assert adopted_outline == edited and compiled == original
    assert artifacts.read_json(outline_ref) == original and artifacts.read_json(edited_ref) == edited
    assert [chapter["chapter_id"] for chapter in adopted_outline["chapters"]] == [
        original["chapters"][1]["chapter_id"], goal_id, original["chapters"][0]["chapter_id"]]
    writes = [value for task, value in model.calls if task == "authoring_write"]
    assert len(writes) == 1 and writes[0]["title_path"] == ["实施安排"]
    units = [artifacts.read_json(reference) for reference in result["unit_refs"]]
    generated = next(unit for unit in units if unit["chapter_id"] == goal_id)
    assert generated["status"] == "generated"
    assert generated["target_ids"] == ["manual-response-" + goal_id]
    assert generated["blocks"][0]["material_ids"] == ["controlled-method"]
    document = Document(BytesIO(artifacts.read_bytes(result["docx_ref"])))
    assert ANSWER in [paragraph.text for paragraph in document.paragraphs]
    assert_original_table(document)


async def test_failed_directory_remains_explicitly_pending_when_exported_as_a_fixed_outline(tmp_path):
    artifacts, model = ArtifactStore(tmp_path / "artifacts"), DirectoryModel(fail_plans=True)
    outline_ref, outline = await compile_outline(artifacts, model)
    assert outline["chapters"] == []
    assert "outline_directory_plan_failed" in {issue["code"] for issue in outline["issues"]}
    model.calls.clear()

    result = artifacts.read_json(await generate_document(
        artifacts, outline["authoring_input"], budget(), model, outline=artifacts.read_json(outline_ref)))

    assert artifacts.read_json(result["outline_ref"]) == outline
    assert result["unit_refs"] == [] and model.calls == []
    assert {"outline_empty", "outline_directory_plan_failed"} <= {gap["code"] for gap in result["gaps"]}
    assert artifacts.read_bytes(result["docx_ref"]).startswith(b"PK")
    with ZipFile(BytesIO(artifacts.read_bytes(result["package_ref"]))) as package:
        assert "待补事项.json" in package.namelist()


@pytest.mark.parametrize('failed_middle', [False, True])
@pytest.mark.parametrize('failed_classification', [False, True])
async def test_parallel_requirement_completion_does_not_change_source_order_or_issue_phase(
        monkeypatch, failed_middle, failed_classification):
    from docreader.tender import authoring_pipeline as module
    from docreader.tender.directory_compile import compile_directory

    documents = docs(['响应章\n第一来源要求', '第二来源页\n第二来源要求', '第三来源页\n第三来源要求'])
    initial = compile_directory(plan([{**node('response', 0, 0), 'body_windows': [window(0, 0)]}],
                                     [item('response')]), documents)
    monkeypatch.setattr(module, 'requirement_blocks', lambda _: [{'index': index} for index in range(3)])
    outputs = []
    for first_completed in (0, 1):
        first_returned = asyncio.Event()
        completion = []

        async def ask(_execute, _budget, task, value, validate):
            if task == 'directory_material':
                return validate({'assignments': [], 'table_headers': [], 'chapter_titles': fixture_titles(value)})
            if task == 'directory_classification':
                assert len(completion) == 3, 'Source closure follows all requirement blocks.'
                if failed_classification:
                    raise ModelWorkUnitExhausted(task)
                return validate({'items': [{'kind': 'goods', **window(0, 0)}]})
            assert task == 'directory_requirement'
            index = value['block']['index']
            if index in (0, 1) and index != first_completed:
                await first_returned.wait()
            elif index == first_completed:
                first_returned.set()
            completion.append(index)
            if failed_middle and index == 1:
                raise ModelWorkUnitExhausted(task)
            # This test supplies a synthetic block index rather than source block records.
            return {'assignments': [{**window(index, 1), 'scope': 'chapter', 'chapter_ids': ['response']}]}

        monkeypatch.setattr(module, '_ask', ask)
        compiled = await module._request_directory_details(deepcopy(initial), documents, None, None)
        assert completion[0] == first_completed, 'The two requirement calls must still overlap.'
        expected = ['第一来源要求', '第三来源要求'] if failed_middle else ['第一来源要求', '第二来源要求', '第三来源要求']
        assert compiled['chapters'][0]['requirements'] == expected
        phases = [i for i in compiled['issues'] if i['code'] in {
            'outline_directory_requirement_rejected', 'outline_directory_classification',
            'outline_directory_classification_rejected'}]
        classification_code = ('outline_directory_classification_rejected' if failed_classification
                               else 'outline_directory_classification')
        assert [i['code'] for i in phases] == ([
            'outline_directory_requirement_rejected', classification_code] if failed_middle else [classification_code])
        if failed_middle:
            assert '分块2' in phases[0]['detail']
        outputs.append(compiled)
    assert outputs[0] == outputs[1]
