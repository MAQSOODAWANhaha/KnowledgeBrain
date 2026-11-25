"""Subject labels stay with their own content, including a form preamble before a grid."""

from typing import TypeVar
from io import BytesIO
import json
from zipfile import ZipFile

from docx import Document
from docx.oxml.ns import qn
import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.tender.authoring_export import export_authoring_document

from test_authoring_export import chapter, document, grid, source_ref, target, writing

_T = TypeVar("_T")


def _required(value: _T | None) -> _T:
    assert value is not None
    return value



def test_subject_does_not_break_preamble_to_original_table_pagination_chain(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    ref = source_ref(artifacts, grid=grid())
    preamble = {**target("preamble", "form"), "subject": "投标人",
                "purpose": "近年财务状况表", "manual_fill": True, "support_origins": ["tender"]}
    original = {**target("original", "form", [ref], templates=[ref]), "subject": "投标人"}
    result = export_authoring_document(artifacts, {"chapters": [
        {**chapter("volume", "财务资料", [preamble, original]), "is_volume": True}]}, [])
    with ZipFile(BytesIO(artifacts.read_bytes(result["package_ref"]))) as package:
        manifest = json.loads(package.read("manifest.json"))
        volume = Document(BytesIO(package.read(manifest["volumes"][0]["package_path"])))

    for doc in [document(artifacts, result), volume]:
        labels = [paragraph for paragraph in doc.paragraphs if _required(_required(paragraph.style).name) == "Caption"]
        assert [label.text for label in labels] == ["投标人", "投标人"]
        assert all(label.paragraph_format.keep_with_next is True for label in labels)
        carried = next(paragraph for paragraph in doc.paragraphs if paragraph.text == preamble["purpose"])
        assert labels[0]._p.getnext() is carried._p
        assert carried.paragraph_format.keep_with_next is True
        assert carried._p.getnext() is labels[1]._p
        assert labels[1]._p.getnext().tag == qn("w:tbl")
        assert len(doc.tables) == 1
        assert doc.tables[0].cell(1, 1).text == ""


@pytest.mark.parametrize("content_kind", ["carried_form", "missing_form", "generated_narrative"])
def test_subject_follows_its_own_non_grid_content(tmp_path, content_kind):
    artifacts = ArtifactStore(tmp_path)
    position = {**target("subject", "narrative" if content_kind == "generated_narrative" else "form"),
                "subject": "产品原厂"}
    units = []
    expected = "（原格式待补）"
    if content_kind == "carried_form":
        expected = "原式声明及签署位置：____。"
        position.update(purpose=expected, manual_fill=True, support_origins=["tender"])
    elif content_kind == "generated_narrative":
        expected = "根据真实资料编制的独立正文。"
        units = [writing("unit", "body", ["subject"], blocks=[{"kind": "paragraph", "text": expected}])]
    result = export_authoring_document(artifacts, {"chapters": [chapter("body", "提交资料", [position])]}, units)
    doc = document(artifacts, result)
    label = next(paragraph for paragraph in doc.paragraphs if paragraph.text == "产品原厂")
    assert label.paragraph_format.keep_with_next is True
    assert "".join(node.text or "" for node in label._p.getnext().iter(qn("w:t"))) == expected
