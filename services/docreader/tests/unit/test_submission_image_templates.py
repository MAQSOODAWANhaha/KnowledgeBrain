"""Image formats expose frozen OCR to semantic passes and keep original rendering."""

from io import BytesIO
import json

from docx import Document
from PIL import Image
import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.budget import RequestBudget
from docreader.tender.authoring import _OutlineContext
from docreader.tender.authoring_export import export_authoring_document
from docreader.tender.authoring_pipeline import _organize
from docreader.tender.authoring_structure import _structure_payload
from docreader.tender.authoring_templates import load_template_part
from docreader.tender.evidence.references import SourceIndex
from docreader.tests.submission_responses import assemble_independent_candidates


OCR_TEXT = "投标函：投标人名称____，法人签字____。"


def image_template(artifacts, text=OCR_TEXT):
    encoded = BytesIO()
    with Image.new("RGB", (20, 10), "white") as image:
        image.save(encoded, "PNG")
    unit = {"unit_id": "image-1", "kind": "image_region", "ordinal": 0, "text": "",
            "locator": {"locator_kind": "document", "section_ordinal": 0, "image_key": "image"}}
    raw = {"schema_version": 2, "units": [unit],
           "images": {"image": artifacts.put_bytes(encoded.getvalue(), "image/png")},
           "ocr": {"image-1": {"text": text, "status": "complete" if text else "unresolved"}},
           "source_contract": {"page_manifest": [], "units": [
               {"unit_id": unit["unit_id"], "section_id": "section-1", "parent_section_id": None,
                "heading_path": "投标函原格式", "physical_locator": unit["locator"]}]}}
    document = {"document_id": "tender", "document_version": "1",
                "original_ref": artifacts.put_bytes(b"synthetic original")}
    index = SourceIndex(document, raw)
    index_ref = artifacts.put_json(index.descriptor(artifacts.put_json(index.raw)))
    snapshot = {"schema_version": 2, "run_id": "image-template", "source_version": 1,
                "domain_revision": 1, "frozen_ref": artifacts.put_json({"schema_version": 2,
                    "documents": [{**index.document, "role": "main", "source_index_ref": index_ref}]})}
    part = {"source_ref": {"source_index_ref": index_ref, "unit_id": unit["unit_id"]}, "selectors": []}
    outline = {"schema_version": 2, "authoring_input": snapshot, "issues": [], "chapters": [
        {"chapter_id": "chapter-1", "title": "商务文件", "children": [], "targets": [
            {"target_id": "form-1", "kind": "form", "purpose": "保留投标函原格式", "manual_fill": True,
             "source_refs": [], "rule_refs": [], "template_refs": [part]}]}]}
    return snapshot, part, outline


def test_template_semantic_view_uses_ocr_without_mutating_native_image_text(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    _, part, _ = image_template(artifacts)
    loaded = load_template_part(artifacts, part)
    assert loaded["text"] == OCR_TEXT
    assert loaded["unit"]["text"] == "" and loaded["image_ref"]


@pytest.mark.parametrize("readable", [True, False], ids=["frozen-ocr", "no-readable-ocr"])
async def test_assembly_image_descriptor_contains_text_or_reports_unavailable(tmp_path, readable):
    artifacts = ArtifactStore(tmp_path)
    snapshot, _, _ = image_template(artifacts, OCR_TEXT if readable else "")
    context = _OutlineContext(artifacts, snapshot)
    assembly = []

    async def execute(stage, request, validate):
        assert stage == "outline"
        value = json.loads(request["messages"][1]["content"][0]["text"])
        if value.get("operation") == "assemble":
            assembly.append(value)
            return validate(assemble_independent_candidates(value))
        return validate({"schema_version": 2, "chapters": [{"title": "投标函", "children": [], "targets": [
            {"kind": "form", "purpose": "保留投标函原格式", "rule_ids": [],
             "range_ids": [value["sources"][0]["range_id"]],
             "template_part_ids": [value["template_parts"][0]["part_id"]]}]}], "findings": []})

    await _organize(artifacts, context, RequestBudget(model="fixture", context_tokens=131072), execute, [])
    descriptor = assembly[0]["template_parts"][0]
    assert descriptor["text"] == (OCR_TEXT if readable else "")
    assert bool(descriptor.get("unavailable")) is not readable


@pytest.mark.parametrize("readable", [True, False], ids=["frozen-ocr", "no-readable-ocr"])
def test_final_structure_reads_image_ocr_or_retains_a_located_gap(tmp_path, readable):
    artifacts = ArtifactStore(tmp_path)
    _, _, outline = image_template(artifacts, OCR_TEXT if readable else "")
    payload, gaps = _structure_payload(artifacts, outline, [])
    if readable:
        assert payload["supports"][0]["text"] == OCR_TEXT and not gaps
        assert payload["chapters"][0]["targets"][0]["template_support_ids"] == ["s1"]
    else:
        assert payload["supports"] == []
        assert any(gap["code"] == "structure_template_unavailable" and gap["target_id"] == "form-1"
                   for gap in gaps)


def test_export_retains_image_without_repeating_its_ocr_as_a_paragraph(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    _, _, outline = image_template(artifacts)
    result = export_authoring_document(artifacts, outline, [])
    document = Document(BytesIO(artifacts.read_bytes(result["docx_ref"])))
    assert len(document.inline_shapes) == 1
    assert OCR_TEXT not in [paragraph.text for paragraph in document.paragraphs]
