"""Session outline keeps unnamed structure and shows unplaced rules in the document."""

from io import BytesIO
import json
from zipfile import ZipFile

from docx import Document

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.budget import RequestBudget
from docreader.tender import authoring_pipeline as pipeline
from docreader.tender.authoring_excerpts import load_source_excerpt
from docreader.tender.authoring_export import export_authoring_document
from docreader.tender.authoring_pipeline import generate_document

from test_assembly_fallback_hierarchy import organize as failed_organize
from test_authoring_pipeline_boundaries import snapshot
from test_response_table_context import REQUIREMENTS
from test_submission_export import chapter, grid, part, source, target, writing


async def test_omitted_table_context_is_not_replaced_or_copied(tmp_path):
    conditions = ["仅甲项适用：使用甲环境。", "仅乙项适用：使用乙环境。"]
    artifacts = ArtifactStore(tmp_path)
    fixed = snapshot(artifacts, [{"heading": "业务要求", "text": text}
                                 for text in [conditions[0], REQUIREMENTS[0], conditions[1], REQUIREMENTS[1]]])

    async def execute(_operation, request, validate):
        payload = json.loads(request["messages"][1]["content"][0]["text"])
        stage = payload.get("operation", _operation)
        if stage == "outline":
            by_text = {source["text"]: source["range_id"] for source in payload["sources"]}
            return validate({"schema_version": 2, "findings": [], "chapters": [{"title": "业务应答", "targets": [
                {"kind": "narrative", "purpose": f"第{number}份原业务表", "range_ids": [], "rule_ids": [],
                 "response_table": {"context_range_ids": [by_text[condition]],
                    "rows": [{"range_ids": [by_text[requirement]]}]}}
                for number, (condition, requirement) in enumerate(zip(conditions, REQUIREMENTS))]}]})
        if stage == "assemble":
            by_text = {source["text"]: source["range_id"] for source in payload["source_excerpts"]}
            return validate({"chapters": [{"title": "业务应答", "targets": [{
                "kind": "narrative", "purpose": "合并但没有重交条件",
                "target_ids": [item["target_id"] for item in payload["chapters"][0]["targets"]],
                "response_table": {"rows": [{"range_ids": [by_text[requirement]]} for requirement in REQUIREMENTS]}}]}]})
        return validate({"findings": []})

    result = artifacts.read_json(await generate_document(
        artifacts, fixed, RequestBudget(model="fixture", context_tokens=131072, output_reserve=16384), execute))
    targets = artifacts.read_json(result["outline_ref"])["chapters"][0]["targets"]
    assert len(targets) == 2
    for item, condition in zip(targets, conditions):
        assert [load_source_excerpt(artifacts, ref)["text"]
                for ref in item["response_table"]["context_source_excerpts"]] == [condition]
        assert condition not in [load_source_excerpt(artifacts, ref)["text"]
                                 for other in targets if other is not item
                                 for ref in other["response_table"].get("context_source_excerpts", [])]


async def test_unnamed_response_row_stays_on_its_table(tmp_path):
    condition = "两行共同条件：留在原表。"
    artifacts = ArtifactStore(tmp_path)
    fixed = snapshot(artifacts, [{"heading": "业务要求", "text": text}
                                 for text in [condition, *REQUIREMENTS]])

    async def execute(_operation, request, validate):
        payload = json.loads(request["messages"][1]["content"][0]["text"])
        stage = payload.get("operation", _operation)
        if stage == "outline":
            by_text = {source["text"]: source["range_id"] for source in payload["sources"]}
            return validate({"schema_version": 2, "findings": [], "chapters": [{"title": "业务应答", "targets": [{
                "kind": "narrative", "purpose": "一张两行的业务表", "range_ids": [], "rule_ids": [],
                "response_table": {"context_range_ids": [by_text[condition]],
                    "rows": [{"range_ids": [by_text[requirement]]} for requirement in REQUIREMENTS]}}]}]})
        if stage == "assemble":
            by_text = {source["text"]: source["range_id"] for source in payload["source_excerpts"]}
            return validate({"chapters": [{"title": "业务应答", "targets": [{
                "kind": "narrative", "purpose": "只点了第一行",
                "target_ids": [payload["chapters"][0]["targets"][0]["target_id"]],
                "response_table": {"rows": [{"range_ids": [by_text[REQUIREMENTS[0]]]}]}}]}]})
        return validate({"findings": []})

    result = artifacts.read_json(await generate_document(
        artifacts, fixed, RequestBudget(model="fixture", context_tokens=131072, output_reserve=16384), execute))
    table = artifacts.read_json(result["outline_ref"])["chapters"][0]["targets"][0]["response_table"]
    assert [load_source_excerpt(artifacts, ref)["text"] for ref in table["context_source_excerpts"]] == [condition]
    assert sorted(load_source_excerpt(artifacts, row["source_excerpts"][0])["text"] for row in table["rows"]) == sorted(REQUIREMENTS)


def test_unassigned_rule_is_visible_in_the_document_and_not_in_a_technical_table(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    rule_ref = {"item_ref": {"artifact_ref": artifacts.put_json({"item_id": "rule-1"}), "item_id": "rule-1"}}
    text = "协同防御必须逐项响应。"
    outline = {"chapters": [{"chapter_id": "body", "title": "正文", "targets": [], "children": []}],
               "issues": [{"code": "outline_rule_unassigned", "detail": "尚未归入响应表", "rule_refs": [rule_ref]}]}
    requirements = {"rules": [{"rule_ref": rule_ref, "rule": {"text": text, "evidence": [{"quote": text}]}}]}

    exported = export_authoring_document(artifacts, outline, [], requirements=requirements)
    document = Document(BytesIO(artifacts.read_bytes(exported["docx_ref"])))
    body = "\n".join(paragraph.text for paragraph in document.paragraphs)
    assert "尚未归入响应表" in body
    assert text in body
    assert document.tables == []


async def test_failed_assembly_keeps_source_volume_proposal_without_publishing(tmp_path, monkeypatch):
    artifacts, result, fragments = await failed_organize(tmp_path, monkeypatch)
    proposed = [chapter["title"] for chapter in pipeline._flatten(fragments[0]["chapters"]) if chapter.get("is_volume")]
    retained = {chapter["title"]: chapter.get("is_volume", False) for chapter in pipeline._flatten(result["chapters"])}
    assert proposed and all(retained[title] for title in proposed)
    exported = export_authoring_document(artifacts, result, [])
    with ZipFile(BytesIO(artifacts.read_bytes(exported["package_ref"]))) as archive:
        assert not any(name.startswith("volumes/") for name in archive.namelist())


def test_existing_form_does_not_gain_a_parallel_pending_chapter(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    selected = part(source(artifacts, "financial", text="8D 财务格式", grid=grid()))
    item = target("bidder", templates=[selected])
    outline = {"chapters": [chapter("财务", [item])]}
    units = [writing("w1", "财务", ["bidder"], status="placeholder")]

    exported = export_authoring_document(artifacts, outline, units)
    document = Document(BytesIO(artifacts.read_bytes(exported["docx_ref"])))
    paragraphs = [paragraph.text for paragraph in document.paragraphs]
    assert document.tables
    assert "（本部分内容待补）" not in paragraphs
    assert not any(paragraph.startswith("待补") for paragraph in paragraphs)
