"""Explicit template selection survives every legal response target kind."""
from io import BytesIO
import json
from zipfile import ZipFile

from docx import Document
import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.tender.authoring_export import export_authoring_document

from test_authoring_export import chapter, document, grid, source_ref, target, writing


@pytest.mark.parametrize("kind", ["narrative", "attachment"])
@pytest.mark.parametrize("native_grid", [False, True])
def test_explicit_templates_render_in_main_and_owning_volume_without_hiding_body(tmp_path, kind, native_grid):
    artifacts = ArtifactStore(tmp_path)
    first = source_ref(artifacts, text="固定声明甲：____", document_id="first")
    second = source_ref(artifacts, grid=grid() if native_grid else None,
                        text="固定声明乙：____" if not native_grid else "", document_id="second")
    position = target("response", kind, [first, second], templates=[first, second])
    outline = {"chapters": [{**chapter("volume", "商务文件", children=[
        chapter("body", "提交内容", [position])]), "is_volume": True}]}
    result = export_authoring_document(artifacts, outline, [writing("w1", "body", ["response"], blocks=[
        {"kind": "paragraph", "text": "独立的有据说明正文。"}])])
    with ZipFile(BytesIO(artifacts.read_bytes(result["package_ref"]))) as archive:
        manifest = json.loads(archive.read("manifest.json"))
        volumes = [Document(BytesIO(archive.read(volume["package_path"]))) for volume in manifest["volumes"]]
    assert len(volumes) == 1
    for doc in [document(artifacts, result), *volumes]:
        paragraphs = [paragraph.text for paragraph in doc.paragraphs]
        assert "固定声明甲：____" in paragraphs
        assert "独立的有据说明正文。" in paragraphs
        if native_grid:
            assert len(doc.tables) == 1
            assert doc.tables[0].cell(0, 0).text == "人员安排表"
            assert doc.tables[0].cell(1, 1).text == ""
        else:
            assert "固定声明乙：____" in paragraphs
            assert paragraphs.index("固定声明甲：____") < paragraphs.index("固定声明乙：____")
    assert not any(gap["code"] in {"export_form_content_unplaced", "export_template_missing"}
                   for gap in result["gaps"])


@pytest.mark.parametrize("kind", ["narrative", "attachment"])
def test_rendering_nonform_template_does_not_grant_form_fill_permission(tmp_path, kind):
    artifacts = ArtifactStore(tmp_path)
    ref = source_ref(artifacts, grid=grid())
    result = export_authoring_document(artifacts, {"chapters": [chapter("body", "提交内容", [
        target("response", kind, [ref], templates=[ref])])]}, [writing("w1", "body", ["response"], cell_fills=[
            {"source_ref": ref, "row": 1, "column": 1, "text": "未授权填值"}])])
    assert document(artifacts, result).tables[0].cell(1, 1).text == ""
    assert any(gap["code"] == "export_cell_fill_unplaced" for gap in result["gaps"])
