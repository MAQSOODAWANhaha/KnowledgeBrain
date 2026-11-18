"""One complete business obligation keeps one response across physical source pages."""

from copy import deepcopy
from hashlib import sha256
from io import BytesIO
import json
from types import SimpleNamespace

from docx import Document
import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.request import ModelContractError
from docreader.tender import authoring_pipeline as pipeline
from docreader.tender.directory_compile import (
    RequirementPlan, apply_requirements, compile_directory, documents_from_frozen,
    outline_from_compiled, requirement_blocks,
)
from test_authoring_pipeline_boundaries import budget, payload, snapshot
from test_directory_compile import docs, fixture_titles, item, node, plan, window
from test_directory_plan_replay import Receipts


FIRST = "1.10.2 中标人不得向他人转让中标项目，接受分包的人不得再次分包。中标人应"
CONTINUED = "当就分包项目向招标人负责，接受分包的人就分包项目承担连带责任。"
COMPLETE = FIRST + "\n" + CONTINUED
SOURCE_SNAPSHOT = {"schema_version": 2, "run_id": "requirements", "source_version": 1, "domain_revision": 1}


def complete_source_rows(document_id="tender", continuation_line=1):
    return [{"document_id": document_id, "page": 0, "line": 2, "text": FIRST},
            {"document_id": document_id, "page": 1, "line": continuation_line, "text": CONTINUED}]


def assert_requirement_rendering(compiled, outline, rows):
    chapter = outline["chapters"][0]
    assert chapter["chapter_id"] == "chapter-1"
    target = chapter["targets"][1]
    assert target["target_id"] == "chapter-1:target-2" and target["purpose"] == COMPLETE
    assert [record for record in compiled["_source_renderings"]
            if record["target_id"] == "chapter-1:target-2"] == [{
        "target_id": "chapter-1:target-2", "chapter_id": "chapter-1", "rows": rows,
        "rows_sha256": sha256(json.dumps(rows, ensure_ascii=False, sort_keys=True,
                                        separators=(",", ":")).encode()).hexdigest(),
        "generated": False, "purpose_sha256": sha256(COMPLETE.encode()).hexdigest()}]


def source_case(*, covered=()):
    documents = docs(["商务文件\n原式正文\n" + FIRST,
                      "页眉\n" + CONTINUED + "\n下一独立条款。", "本次分块不可见的后续条款。"],
                     {"document_id": "other", "role": "main", "pages": ["另一份文件的条款。"]})
    body = [window(0, 1)]
    if "first" in covered:
        body.append(window(0, 2))
    if "continuation" in covered:
        body.append(window(1, 1))
    chosen = {**node("business", 0, 0), "body_windows": body}
    return documents, compile_directory(plan([chosen], [item("business")]), documents)


def requirement(**extra):
    return {"document_id": "tender", "page": 0, "start_line": 2, "end_line": 2,
            "scope": "chapter", "chapter_ids": ["business"],
            "continuation_windows": [window(1, 1)], **extra}


def test_cross_page_business_requirement_is_one_complete_response_basis():
    documents, compiled = source_case()
    source_before = deepcopy(documents)
    chapter_before = deepcopy(compiled["chapters"][0])
    apply_requirements(compiled, documents, [requirement()])
    assert compiled["chapters"][0]["requirements"] == [COMPLETE]
    rows = complete_source_rows()
    assert compiled["chapters"][0] == {**chapter_before, "requirements": [COMPLETE],
                                       "_requirement_rows": [rows]}
    assert documents == source_before
    assert_requirement_rendering(compiled, outline_from_compiled(SOURCE_SNAPSHOT, compiled), rows)


@pytest.mark.parametrize("covered", [("first",), ("continuation",), ("first", "continuation")])
def test_multi_window_requirement_keeps_complete_basis_unless_every_line_is_carried(covered):
    documents, compiled = source_case(covered=covered)
    chapter_before = deepcopy(compiled["chapters"][0])
    apply_requirements(compiled, documents, [requirement()])
    expected = [] if len(covered) == 2 else [COMPLETE]
    assert compiled["chapters"][0].get("requirements", []) == expected
    if expected:
        rows = complete_source_rows()
        assert compiled["chapters"][0] == {**chapter_before, "requirements": [COMPLETE],
                                           "_requirement_rows": [rows]}
        assert_requirement_rendering(compiled, outline_from_compiled(SOURCE_SNAPSHOT, compiled), rows)
    else:
        assert compiled["chapters"][0] == chapter_before


def test_legacy_same_page_raw_defaults_empty_and_keeps_existing_covered_line_behavior():
    documents, compiled = source_case()
    raw = requirement(start_line=1)
    raw.pop("continuation_windows")
    parsed = RequirementPlan.model_validate({"assignments": [raw]}).model_dump()
    assert parsed["assignments"][0]["continuation_windows"] == []
    apply_requirements(compiled, documents, parsed["assignments"])
    assert compiled["chapters"][0]["requirements"] == [FIRST]
    assert CONTINUED not in compiled["chapters"][0]["requirements"][0]


def visible_block(documents):
    return [page for block in requirement_blocks(documents) for page in block
            if (page["document_id"], page["page"]) != ("tender", 2)]


def paid_executor(tmp_path):
    import docreader.tender.document_execution as execution

    artifacts, store = ArtifactStore(tmp_path), Receipts()
    client = SimpleNamespace(config=SimpleNamespace(chat=SimpleNamespace(base_url="https://fixture.invalid/v1")))
    executor = execution._DocumentExecutor(client, store, artifacts,
        SimpleNamespace(execution_id="requirement-continuation", epoch=1), budget(), {}, None)
    return executor, store


def model_response(monkeypatch, documents, assignments):
    import docreader.tender.document_execution as execution

    monkeypatch.setattr(pipeline, "requirement_blocks", lambda _documents: [visible_block(documents)])

    async def model(_model, request, validate):
        value = payload(request)
        if "block" in value:
            return validate({"assignments": deepcopy(assignments)})
        if "table_header_catalog" in value:
            return validate({"assignments": [], "table_headers": [], "chapter_titles": fixture_titles(value)})
        return validate({"items": []})

    monkeypatch.setattr(execution._DocumentModel, "__call__", model)


async def test_valid_cross_page_requirement_is_accepted_once_by_the_real_detail_stage(tmp_path, monkeypatch):
    documents, compiled = source_case()
    executor, store = paid_executor(tmp_path)
    model_response(monkeypatch, documents, [requirement()])
    await pipeline._request_directory_details(compiled, documents, budget(), executor)
    assert compiled["chapters"][0]["requirements"] == [COMPLETE]
    receipts = [key for key in store.rows if key[1].startswith("directory_requirement:")]
    assert len(receipts) == 1


@pytest.mark.parametrize("bad_windows", [
    pytest.param([window(0, 1)], id="reverse"),
    pytest.param([window(0, 2)], id="repeat-first-window"),
    pytest.param([window(1, 1), window(1, 1)], id="duplicate-continuation"),
    pytest.param([window(1, 1, 2), window(1, 2)], id="overlapping-continuations"),
    pytest.param([window(0, 0, document_id="other")], id="different-visible-document"),
    pytest.param([window(2, 0)], id="continuation-outside-request-block"),
    pytest.param([window(1, 99)], id="source-line-does-not-exist"),
    pytest.param(None, id="legacy-first-window-outside-request-block"),
])
async def test_invalid_source_never_becomes_a_paid_success_receipt(tmp_path, monkeypatch, bad_windows):
    documents, compiled = source_case()
    source_before = deepcopy(documents)
    invalid = requirement(continuation_windows=bad_windows)
    if bad_windows is None:
        invalid.update(page=2, start_line=0, end_line=0)
        invalid.pop("continuation_windows")
    executor, store = paid_executor(tmp_path)
    model_response(monkeypatch, documents, [invalid])
    caught = None
    try:
        await pipeline._request_directory_details(compiled, documents, budget(), executor)
    except ModelContractError as error:
        caught = error
    assert not [key for key in store.rows if key[1].startswith("directory_requirement:")]
    assert caught is not None
    assert any(error["code"] == "directory_requirement_source_invalid" for error in caught.errors)
    assert documents == source_before


async def test_complete_cross_page_business_clause_has_one_word_response_and_preserves_original_table(tmp_path):
    artifacts = ArtifactStore(tmp_path / "artifacts")
    native_grid = {"row_count": 3, "column_count": 2, "cells": [
        {"row": 0, "column": 0, "text": "商务原表", "row_span": 1, "col_span": 2},
        {"row": 1, "column": 0, "text": "项目名称", "row_span": 1, "col_span": 1},
        {"row": 1, "column": 1, "text": "", "row_span": 1, "col_span": 1},
        {"row": 2, "column": 0, "text": "", "row_span": 1, "col_span": 1},
        {"row": 2, "column": 1, "text": "", "row_span": 1, "col_span": 1},
    ]}
    fixed = snapshot(artifacts, [{"text": "商务文件\n原式说明\n" + FIRST},
                                 {"text": CONTINUED + "\n下一独立条款。"}, {"grid": native_grid}])
    documents = documents_from_frozen(artifacts, artifacts.read_json(fixed["frozen_ref"]))
    last = len(documents[0]["pages"][2].splitlines()) - 1
    chosen = {**node("business", 0, 0, document_id="document"),
              "body_windows": [window(2, 0, last, "document")]}
    compiled = compile_directory(plan([chosen], [item("business")]), documents)
    original_chapter = deepcopy(compiled["chapters"][0])
    apply_requirements(compiled, documents, [requirement(document_id="document",
        continuation_windows=[window(1, 0, document_id="document")])])
    rows = complete_source_rows("document", continuation_line=0)
    assert compiled["chapters"][0] == {**original_chapter, "requirements": [COMPLETE],
                                       "_requirement_rows": [rows]}
    outline = outline_from_compiled(fixed, compiled)
    assert_requirement_rendering(compiled, outline, rows)

    async def manual_model(task, _request, validate):
        assert task == "authoring_structure"
        return validate({"findings": []})

    result = artifacts.read_json(await pipeline.generate_document(
        artifacts, fixed, budget(), manual_model, outline=outline))
    document = Document(BytesIO(artifacts.read_bytes(result["docx_ref"])))
    assert len(document.tables) == 1
    table = document.tables[0]
    assert [[cell.text for cell in row.cells] for row in table.rows] == [
        ["商务原表", "商务原表"], ["项目名称", ""], ["", ""]]
    assert table.cell(0, 0)._tc is table.cell(0, 1)._tc
    paragraphs = [line for paragraph in document.paragraphs for line in paragraph.text.splitlines()]
    response = "我方响应：________（人工填写）"
    evidence = "证明材料或附件位置：________（原条款要求时填写）"
    assert paragraphs.count(response) == paragraphs.count(evidence) == 1
    response_index = paragraphs.index(response)
    assert "\n".join(paragraphs[response_index - 2:response_index]) == COMPLETE
    assert paragraphs.count(FIRST) == paragraphs.count(CONTINUED) == 1
