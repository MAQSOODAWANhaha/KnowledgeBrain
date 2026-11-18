"""TOC page references remain explicit until calculated, with no floating break paragraph."""

from io import BytesIO

from docx import Document
from docx.oxml.ns import qn

from docreader.integrations.artifacts import ArtifactStore
from docreader.tender.authoring_export import export_authoring_document
from docreader.tender.word_navigation import bookmark_name

from test_authoring_layout import native_part


def chapter(identity, title, *, role="body", targets=(), children=()):
    return {"chapter_id": identity, "title": title, "targets": list(targets), "children": list(children),
            "presentation_role": role, "is_volume": False}


def word(artifacts, outline):
    result = export_authoring_document(artifacts, outline, [])
    return Document(BytesIO(artifacts.read_bytes(result["docx_ref"])))


def test_each_toc_page_reference_uses_its_real_bookmark_and_never_a_fake_page_one(tmp_path):
    document = word(ArtifactStore(tmp_path), {"chapters": [
        chapter("one", "Technical", children=[chapter("two", "Service")]),
        chapter("content", "Source excerpt", role="content")]})
    fields = [node for node in document.element.iter(qn("w:fldSimple"))
              if node.get(qn("w:instr"), "").strip().startswith("PAGEREF")]
    assert [node.get(qn("w:instr")).strip() for node in fields] == [
        f'PAGEREF {bookmark_name(identity)} \\h' for identity in ("one", "two")]
    assert ["".join(text.text or "" for text in node.iter(qn("w:t"))) for node in fields] == ["待计算", "待计算"]
    assert all(node.get(qn("w:dirty")) in {"true", "1"} for node in fields)
    names = {node.get(qn("w:name")) for node in document.element.iter(qn("w:bookmarkStart"))}
    assert all(bookmark_name(identity) in names for identity in ("one", "two"))


def test_toc_body_boundary_is_on_the_real_first_heading_not_an_empty_break_paragraph(tmp_path):
    document = word(ArtifactStore(tmp_path), {"chapters": [chapter("one", "Actual body")]})
    anchor = next(node for node in document.element.iter(qn("w:bookmarkStart"))
                  if node.get(qn("w:name")) == bookmark_name("one"))
    first = anchor.getparent()
    assert first.find(f"{qn('w:pPr')}/{qn('w:pageBreakBefore')}") is not None
    assert not [node for node in document.element.iter(qn("w:br")) if node.get(qn("w:type")) == "page"]
    assert not [paragraph for paragraph in document.paragraphs if not paragraph.text]


def test_unnumbered_source_content_keeps_its_bookmark_and_stable_page_start(tmp_path):
    target = {"target_id": "source:1", "kind": "form", "purpose": "Actual original statement.\nSignature: ____",
        "rule_refs": [], "source_refs": [], "manual_fill": True, "support_origins": ["tender"]}
    document = word(ArtifactStore(tmp_path), {"chapters": [
        chapter("source", "Original", role="content", targets=[target]), chapter("later", "Later chapter")]})
    anchor = next(node for node in document.element.iter(qn("w:bookmarkStart"))
                  if node.get(qn("w:name")) == bookmark_name("source"))
    first = anchor.getparent()
    assert first.find(f"{qn('w:pPr')}/{qn('w:pageBreakBefore')}") is not None
    assert "".join(node.text or "" for node in first.iter(qn("w:t"))) == "Actual original statement."
    assert [paragraph.text for paragraph in document.paragraphs].count("Signature: ____") == 1


def test_original_table_at_body_start_keeps_both_anchors_without_a_break_carrier(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    part, _ = native_part(artifacts)
    target = {"target_id": "source:1", "kind": "form", "purpose": "Original table", "template_refs": [part],
        "rule_refs": [], "source_refs": [], "manual_fill": True}
    document = word(artifacts, {"chapters": [
        chapter("source", "Original", role="content", targets=[target]), chapter("later", "Later chapter")]})
    first = document.tables[0].cell(0, 0).paragraphs[0]._p
    assert first.find(f"{qn('w:pPr')}/{qn('w:pageBreakBefore')}") is not None
    names = [node.get(qn("w:name")) for node in first.iter(qn("w:bookmarkStart"))]
    assert names.count(bookmark_name("source")) == names.count(bookmark_name("source:1", "target")) == 1
    assert not [paragraph for paragraph in document.paragraphs if not paragraph.text]
