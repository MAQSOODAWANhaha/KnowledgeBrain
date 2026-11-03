"""Explicit structure title edits update navigation without changing native forms."""

from copy import deepcopy
from io import BytesIO
import json
from zipfile import ZipFile

from docx import Document
import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.budget import RequestBudget
from docreader.llm.contracts import output_schema
from docreader.tender.authoring_export import export_authoring_document
from docreader.tender.authoring_pipeline import generate_document
from docreader.tender.authoring_structure import review_structure
from test_authoring_pipeline_boundaries import (
    budget, ordinary_response, outline as source_outline, payload, snapshot,
)
from test_authoring_templates import create_context, create_source, make_part


NATIVE_TEXTS = ["附件 8A 商务部分摘要表", "1.1 原格式填写条款：____",
                "法定代表人签字（盖章）：____", "附件1 投标函原文：投标人完整声明。"]


def fixture(tmp_path):
    artifacts, index_ref, native = create_source(tmp_path, [{"text": text} for text in NATIVE_TEXTS])
    context = create_context(artifacts, index_ref, native)
    parts = [make_part(index_ref, f"unit:{number}") for number in range(len(NATIVE_TEXTS))]

    def target(identity, selected):
        return {"target_id": identity, "kind": "form", "purpose": "按完整原格式填写。",
                "rule_refs": [], "source_refs": [part["source_ref"] for part in selected],
                "template_refs": selected}

    current = {"schema_version": 2, "authoring_input": context.input, "issues": [],
        "constraint_rule_refs": [], "submission_refs": [], "chapters": [{
            "chapter_id": "volume", "title": "商务文件", "is_volume": True, "targets": [],
            "children": [
                {"chapter_id": "summary", "title": "8A 附件表 商务部分摘要表", "children": [],
                 "targets": [target("summary-form", parts[:3])]},
                {"chapter_id": "letter", "title": "附件1 投标函", "children": [],
                 "targets": [target("letter-form", parts[3:])]}]}]}
    return artifacts, current, native


def rename(identity="summary", title="商务部分摘要"):
    return {"chapter_id": identity, "title": title, "reason": "正式章节表达业务内容，原格式编号保留在正文。"}


async def review(artifacts, current, raw, *, enabled=None, outline_phase=True):
    before = deepcopy(current)

    async def execute(task, request, validate):
        value = payload(request)
        assert value.get("title_edits_allowed", False) is (enabled is True)
        accepted = validate(deepcopy(raw))
        assert current == before, "validating a model response must not mutate adopted content"
        return accepted

    options = {} if enabled is None else {"apply_title_edits": enabled}
    return await review_structure(artifacts, current, [],
        RequestBudget(model="fixture", context_tokens=131072), execute,
        outline_phase=outline_phase, **options)


async def test_allowed_renames_update_main_and_volume_word_but_preserve_native_content(tmp_path):
    artifacts, current, native = fixture(tmp_path)
    before, native_before = deepcopy(current), deepcopy(native.raw)
    _, gaps = await review(artifacts, current, {"findings": [], "chapter_renames": [
        rename(), rename("letter", "投标函")]}, enabled=True)
    expected = deepcopy(before)
    expected["chapters"][0]["children"][0]["title"] = "商务部分摘要"
    expected["chapters"][0]["children"][1]["title"] = "投标函"
    assert current == expected and native.raw == native_before
    assert all(gap["code"] == "structure_title_edits_applied" for gap in gaps)
    exported = export_authoring_document(artifacts, current, [])
    documents = [artifacts.read_bytes(exported["docx_ref"])]
    with ZipFile(BytesIO(artifacts.read_bytes(exported["package_ref"]))) as archive:
        volumes = json.loads(archive.read("manifest.json"))["volumes"]
        assert len(volumes) == 1 and volumes[0]["chapter_id"] == "volume"
        documents.append(archive.read(volumes[0]["package_path"]))
    for data in documents:
        document = Document(BytesIO(data))
        headings = [p.text for p in document.paragraphs if p.style.name.startswith("Heading")]
        assert any(text.endswith(" 商务部分摘要") for text in headings)
        assert any(text.endswith(" 投标函") for text in headings)
        assert all("8A" not in text and "附件1" not in text for text in headings)
        visible = [p.text for p in document.paragraphs]
        assert all(visible.count(text) == 1 for text in NATIVE_TEXTS)


@pytest.mark.parametrize("outline_phase", [True, False])
async def test_default_review_only_records_title_suggestions(tmp_path, outline_phase):
    artifacts, current, _ = fixture(tmp_path)
    before = deepcopy(current)
    _, gaps = await review(artifacts, current,
        {"findings": [], "chapter_renames": [rename()]}, outline_phase=outline_phase)
    assert current == before
    assert [gap["code"] for gap in gaps] == ["structure_title_edits_not_applied"]


@pytest.mark.parametrize("bad", ["unknown", "duplicate_action", "duplicate_existing"])
async def test_invalid_rename_batch_never_partially_changes_titles(tmp_path, bad):
    artifacts, current, _ = fixture(tmp_path)
    actions = [rename()]
    if bad == "unknown":
        actions.append(rename("missing", "不存在的章节"))
    elif bad == "duplicate_action":
        actions.append(rename("summary", "另一标题"))
    else:
        current["chapters"][0]["children"].append(deepcopy(current["chapters"][0]["children"][0]))
    before = deepcopy(current)
    findings, gaps = await review(artifacts, current, {"findings": [{
        "code": "title_unresolved", "detail": "保留原稿继续生成，标题待核。"}],
        "chapter_renames": actions}, enabled=True)
    assert current == before and findings[0]["code"] == "title_unresolved"
    assert [gap["code"] for gap in gaps] == ["structure_title_edits_invalid"]


async def test_automatic_f0_and_f1_allow_title_edits_without_an_extra_model_round(tmp_path):
    artifacts, calls, reviews = ArtifactStore(tmp_path), [], []

    async def execute(task, request, validate):
        value = payload(request)
        calls.append(task)
        if task == "authoring_structure":
            reviews.append(value)
            assert value.get("title_edits_allowed") is True
            first = value["chapters"][0]
            return validate({"findings": ([{"code": "check_business_title", "detail": "核对正式章节标题。"}]
                if len(reviews) == 1 else []),
                "chapter_renames": [rename(first["chapter_id"])]})
        if task == "outline" and value.get("operation") != "assemble":
            return validate(source_outline(value, chapters=2))
        response = ordinary_response(task, value)
        if task == "outline":
            response["chapters"][0]["title"] = (
                "附件8A 商务摘要表" if value.get("review_findings") else "8A 商务摘要表")
        return validate(response)

    result = artifacts.read_json(await generate_document(
        artifacts, snapshot(artifacts), budget(), execute))
    final = artifacts.read_json(result["outline_ref"])
    assert final["chapters"][0]["title"] == "商务部分摘要"
    assert calls.count("outline") == 3 and calls.count("authoring_structure") == 2


def test_title_edit_contract_is_optional_and_structure_only():
    structure = output_schema("authoring_structure", {"operation": "structure"})
    body = output_schema("authoring_review", {})
    assert "chapter_renames" in structure["properties"]
    assert "chapter_renames" not in structure["required"]
    assert "chapter_renames" not in body["properties"]
