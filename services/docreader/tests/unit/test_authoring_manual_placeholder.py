"""Manual slots preserve their instructions as metadata, not adopted bid prose."""

from copy import deepcopy
from io import BytesIO
import json
from zipfile import ZipFile

from docx import Document

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.budget import RequestBudget
from docreader.tender.authoring_pipeline import generate_document

from test_authoring_export import chapter, source_ref, target


async def test_manual_narrative_keeps_blank_and_native_form_without_exporting_its_purpose(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    purpose = "内部编制任务：最终报价最后由人工补充，不代填或按零计算。"
    fixed_text = "请按真实资料填写并签字盖章。投标总价：________元。"
    reference = source_ref(artifacts, text=fixed_text)
    original = artifacts.read_json(reference["source_index_ref"])
    raw_before = artifacts.read_json(original["raw_ref"])
    snapshot = {"schema_version": 2, "run_id": "manual-placeholder",
                "source_version": 1, "domain_revision": 1}
    outline = {"schema_version": 2, "authoring_input": snapshot, "issues": [], "chapters": [
        {**chapter("price-volume", "报价文件", [
            {**target("manual-price"), "purpose": purpose, "manual_fill": True},
            {**target("price-form", "form", [reference], templates=[reference]),
             "purpose": fixed_text, "manual_fill": True},
        ]), "is_volume": True}]}
    before, calls = deepcopy(outline), []

    async def execute(stage, request, validate):
        calls.append(stage)
        assert stage == "authoring_structure", "Manual slots need no writing or body-review model call."
        return validate({"findings": []})

    result = artifacts.read_json(await generate_document(artifacts, snapshot,
        RequestBudget(model="fixture", context_tokens=131072, output_reserve=16384), execute,
        outline=outline))
    assert calls == ["authoring_structure"]
    adopted_outline = artifacts.read_json(result["outline_ref"])
    assert {entry["target_id"] for entry in adopted_outline["chapters"][0]["targets"]} == {
        "manual-price", "price-form"}
    units = [artifacts.read_json(ref) for ref in result["unit_refs"]]
    manual, = [unit for unit in units if "manual-price" in unit["target_ids"]]
    assert manual["status"] == "placeholder" and manual["gaps"]
    assert any("人工填写" in gap for gap in manual["gaps"])
    assert any(gap.get("target_id") == "manual-price" for gap in result["gaps"])
    assert outline == before and artifacts.read_json(original["raw_ref"]) == raw_before

    with ZipFile(BytesIO(artifacts.read_bytes(result["package_ref"]))) as archive:
        manifest = json.loads(archive.read("manifest.json"))
        assert len(manifest["volumes"]) == 1
        assert archive.read(manifest["document"]["package_path"]) == artifacts.read_bytes(result["docx_ref"])
        notes = archive.read(manifest["submission_notes"]["package_path"]).decode()
        assert purpose in notes, "The separate manual todo must retain its full instruction."
        documents = [artifacts.read_bytes(result["docx_ref"]),
                     archive.read(manifest["volumes"][0]["package_path"])]
        for data in documents:
            document = Document(BytesIO(data))
            text = "\n".join(paragraph.text for paragraph in document.paragraphs)
            assert fixed_text in text and "________（人工填写）" in text
            assert purpose not in text
