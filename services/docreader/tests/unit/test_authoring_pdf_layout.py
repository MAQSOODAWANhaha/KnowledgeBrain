"""Original PDF typography and real chapter links survive editable Word export."""

from io import BytesIO

from docx import Document
from docx.oxml.ns import qn
from docx.shared import Pt
from pypdf import PdfWriter
from pypdf.generic import DecodedStreamObject, DictionaryObject, NameObject

from docreader.integrations.artifacts import ArtifactStore
from docreader.tender.authoring_export import export_authoring_document
from docreader.tender.evidence.references import SourceIndex
from docreader.tender.word_navigation import bookmark_name


def pdf_table(artifacts):
    writer = PdfWriter()
    page = writer.add_blank_page(width=400, height=600)
    font = writer._add_object(DictionaryObject({NameObject("/Type"): NameObject("/Font"),
        NameObject("/Subtype"): NameObject("/Type1"), NameObject("/BaseFont"): NameObject("/Helvetica")}))
    page[NameObject("/Resources")] = DictionaryObject({NameObject("/Font"): DictionaryObject({NameObject("/F1"): font})})
    commands = ["1 w", "20 500 m 380 500 l S", "20 540 m 380 540 l S", "20 570 m 380 570 l S",
        "20 590 m 380 590 l S", "20 500 m 20 590 l S", "200 500 m 200 590 l S", "380 500 m 380 590 l S"]
    for x, y, text in [(22, 577, "Item"), (202, 577, "Response"), (22, 557, "First line"),
                       (22, 546, "Second line"), (22, 527, "Another item")]:
        commands.append(f"BT /F1 9 Tf {x} {y} Td ({text}) Tj ET")
    stream = DecodedStreamObject()
    stream.set_data("\n".join(commands).encode())
    page[NameObject("/Contents")] = writer._add_object(stream)
    data = BytesIO()
    writer.write(data)
    grid = {"row_count": 3, "column_count": 2, "widths_mm": [63.5, 63.5], "cells": [
        {"row": row, "column": column, "row_span": 1, "col_span": 1, "text": text}
        for row, values in enumerate([["Item", "Response"], ["First line\nSecond line", ""], ["Another item", ""]])
        for column, text in enumerate(values)]}
    locator = {"locator_kind": "page_table", "page_ordinal": 0, "table_ordinal": 0,
        "left": 20.0, "right": 380.0, "bottom": 500.0, "top": 590.0}
    unit = {"unit_id": "table", "kind": "table_region", "ordinal": 0, "text": "", "grid": grid,
        "header_cells": [], "locator": locator}
    raw = {"schema_version": 2, "units": [unit], "images": {}, "ocr": {}, "source_contract": {
        "page_manifest": [], "units": [{"unit_id": "table", "section_id": "section:0",
            "parent_section_id": None, "heading_path": "Original form", "physical_locator": locator}]}}
    source = {"document_id": "source", "document_version": "1", "original_ref": artifacts.put_bytes(data.getvalue())}
    index = SourceIndex(source, raw)
    ref = artifacts.put_json(index.descriptor(artifacts.put_json(index.raw)))
    return {"source_ref": {"source_index_ref": ref, "unit_id": "table"}, "selectors": []}, grid


def chapter(identity, title, *, targets=(), children=(), role="body"):
    return {"chapter_id": identity, "title": title, "targets": list(targets), "children": list(children),
            "presentation_role": role, "is_volume": False}


def test_cached_toc_has_real_chapter_links_without_fabricated_page_numbers(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    outline = {"chapters": [chapter("cover", "Cover", role="cover"),
        chapter("body", "Technical response", children=[chapter("child", "Conditions")]),
        chapter("content", "Source fragment", role="content"),
        chapter("note", "Submission reminder", role="submission_note")]}
    result = export_authoring_document(artifacts, outline, [])
    document = Document(BytesIO(artifacts.read_bytes(result["docx_ref"])))
    links = list(document.element.iter(qn("w:hyperlink")))
    assert [link.get(qn("w:anchor")) for link in links] == [bookmark_name("body"), bookmark_name("child")]
    assert ["".join(t.text or "" for t in link.iter(qn("w:t"))) for link in links] == ["1 Technical response", "1.1 Conditions"]
    assert "请在文字处理软件中更新目录以显示页码。" not in document.element.xml
    page_refs = [field for field in document.element.iter(qn("w:fldSimple"))
                 if "PAGEREF" in field.get(qn("w:instr"), "")]
    assert len(page_refs) == 2
    assert all("".join(text.text or "" for text in field.iter(qn("w:t"))) == "待计算" for field in page_refs)


def test_pdf_table_uses_its_source_font_and_line_spacing_without_changing_cells(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    part, grid = pdf_table(artifacts)
    target = {"target_id": "table:1", "kind": "form", "purpose": "Original form", "template_refs": [part],
        "rule_refs": [], "source_refs": [], "manual_fill": True}
    result = export_authoring_document(artifacts, {"chapters": [chapter("table", "Original form", targets=[target])]}, [])
    document = Document(BytesIO(artifacts.read_bytes(result["docx_ref"])))
    table = document.tables[0]
    assert [[cell.text for cell in row.cells] for row in table.rows] == [
        ["Item", "Response"], ["First line\nSecond line", ""], ["Another item", ""]]
    assert document.styles["Normal"].font.size == Pt(11)
    for row in table.rows:
        for cell in row.cells:
            for paragraph in cell.paragraphs:
                assert paragraph.paragraph_format.space_after == Pt(0)
                assert paragraph.paragraph_format.line_spacing == Pt(11)
                assert all(run.font.size == Pt(9) for run in paragraph.runs)
    assert table.rows[0].cells[0].paragraphs[0].paragraph_format.keep_with_next is True
    assert table.rows[1].cells[0].paragraphs[0].paragraph_format.keep_with_next is not True
    assert not table.rows[0]._tr.xpath("./w:trPr/w:tblHeader")  # Missing source semantics are not guessed.
    assert grid["cells"][2]["text"] == "First line\nSecond line"
