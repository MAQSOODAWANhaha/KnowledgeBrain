"""Unplaced requirements stay in the complete draft, outside scoped volumes."""

from io import BytesIO
import json
from zipfile import ZipFile

from docx import Document
import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.tender.authoring_export import export_authoring_document


@pytest.mark.parametrize("volume_count", [1, 2])
def test_unassigned_requirement_appendix_is_only_in_complete_draft(tmp_path, volume_count):
    artifacts = ArtifactStore(tmp_path)
    requirement = "未确定所属文件的要求，应保留原文等待人工安排。"
    rule_ref = {"item_ref": {
        "artifact_ref": artifacts.put_json({"item_id": "unassigned-rule"}),
        "item_id": "unassigned-rule",
    }}
    outline = {"chapters": [
        {"chapter_id": f"volume-{number}", "title": f"业务文件{number}",
         "is_volume": True, "targets": [], "children": []}
        for number in range(1, volume_count + 1)
    ], "issues": []}
    requirements = {"rules": [{
        "rule_ref": rule_ref,
        "rule": {"text": requirement, "evidence": [{"quote": requirement}]},
    }]}

    result = export_authoring_document(artifacts, outline, [], requirements=requirements)

    with ZipFile(BytesIO(artifacts.read_bytes(result["package_ref"]))) as package:
        manifest = json.loads(package.read("manifest.json"))
        complete = Document(BytesIO(package.read(manifest["document"]["package_path"])))
        paragraphs = [paragraph.text for paragraph in complete.paragraphs]
        assert paragraphs.count("尚未归入响应表") == 1
        assert paragraphs.count(requirement) == 1
        assert len(manifest["volumes"]) == volume_count
        for volume in manifest["volumes"]:
            scoped = Document(BytesIO(package.read(volume["package_path"])))
            text = "\n".join(paragraph.text for paragraph in scoped.paragraphs)
            assert "尚未归入响应表" not in text
            assert requirement not in text
