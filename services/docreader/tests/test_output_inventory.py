"""Final-file profile regressions: no cleanup and no silently missing carriers."""
import hashlib
import json
from concurrent.futures import ThreadPoolExecutor
from io import BytesIO

import grpc
import pytest
from docx import Document
from docx.oxml import OxmlElement
from docx.oxml.ns import qn
from pypdf import PdfWriter
from pypdf.generic import DecodedStreamObject, DictionaryObject, NameObject

from docreader.main import DocReaderServicer
from docreader.parser.parser import Parser
from docreader.proto.docreader_pb2 import ReadConfig, ReadRequest
from docreader.proto.docreader_pb2_grpc import DocReaderStub, add_DocReaderServicer_to_server


def parse(name, raw):
    return Parser().parse_file(name, name.rsplit('.', 1)[-1], raw,
                              parser_engine="builtin", engine_overrides={"output_inventory": "v1"})


def manifest(document):
    return json.loads(document.metadata["output_inventory_manifest"])


def docx_fixture():
    doc = Document()
    paragraph = doc.add_paragraph("Unbookmarked body")
    bookmark = OxmlElement("w:bookmarkStart")
    bookmark.set(qn("w:id"), "1")
    bookmark.set(qn("w:name"), "section_one")
    paragraph._p.insert(0, bookmark)
    doc.add_paragraph("")
    doc.sections[0].header.paragraphs[0].text = "Actual header"
    doc.sections[0].footer.paragraphs[0].text = "Actual footer"
    table = doc.add_table(rows=2, cols=2)
    table.cell(0, 0).merge(table.cell(0, 1)).text = "Merged"
    drawing = OxmlElement("w:drawing")
    doc.add_paragraph().add_run()._r.append(drawing)
    field = OxmlElement("w:fldSimple")
    field.set(qn("w:instr"), "PAGE")
    doc.add_paragraph()._p.append(field)
    raw = BytesIO()
    doc.save(raw)
    return raw.getvalue()


def pdf_fixture(blank=False):
    writer = PdfWriter()
    for index in range(4):
        page = writer.add_blank_page(width=300, height=300)
        if not blank:
            font = writer._add_object(DictionaryObject({
                NameObject('/Type'): NameObject('/Font'), NameObject('/Subtype'): NameObject('/Type1'),
                NameObject('/BaseFont'): NameObject('/Helvetica'),
            }))
            page[NameObject('/Resources')] = DictionaryObject({NameObject('/Font'): DictionaryObject({NameObject('/F1'): font})})
            stream = DecodedStreamObject()
            stream.set_data(f"BT /F1 12 Tf 10 280 Td (Repeating header) Tj 0 -30 Td (Body {index}) Tj 0 -200 Td ({index+1}) Tj ET".encode())
            page[NameObject('/Contents')] = writer._add_object(stream)
    raw = BytesIO()
    writer.write(raw)
    return raw.getvalue()


def test_docx_preserves_stories_empty_paragraphs_grid_bookmarks_and_omissions():
    raw = docx_fixture()
    result = parse("final.docx", raw)
    receipt = manifest(result)
    assert receipt["file_sha256"] == hashlib.sha256(raw).hexdigest()
    assert receipt["profile"] == "output_inventory_v1"
    assert len(receipt["units"]) == len(result.structured_source_units)
    assert any(u.text == "Actual header" for u in result.structured_source_units)
    assert any(u.text == "Actual footer" for u in result.structured_source_units)
    assert any(e["bookmarks"] == ["section_one"] for e in receipt["units"])
    assert any(u.grid and u.grid.cells[0].col_span == 2 for u in result.structured_source_units)
    assert any(e["reason"] and "drawing" in e["reason"] for e in receipt["units"])
    assert any(e["reason"] and "field" in e["reason"] for e in receipt["units"])
    assert any(e["kind"] == "paragraphs" and not u.text for e, u in zip(receipt["units"], result.structured_source_units))


@pytest.mark.parametrize("blank", [False, True])
def test_pdf_preserves_all_pages_and_repeated_header_page_numbers(blank):
    result = parse("final.pdf", pdf_fixture(blank))
    receipt = manifest(result)
    pages = [unit for unit in result.structured_source_units if unit.key.endswith(":section:0")]
    assert receipt["page_count"] == len(pages) == 4
    assert len(receipt["image_sha256"]) >= 4
    assert [p.locator.page_ordinal for p in pages] == list(range(4))
    if blank:
        assert all(not page.text for page in pages)
    else:
        assert all("Repeating header" in page.text and str(i + 1) in page.text for i, page in enumerate(pages))


def test_readstream_keeps_all_blank_pdf_profile_receipt():
    server = grpc.server(ThreadPoolExecutor(max_workers=1))
    add_DocReaderServicer_to_server(DocReaderServicer(), server)
    port = server.add_insecure_port("127.0.0.1:0")
    server.start()
    try:
        with grpc.insecure_channel(f"127.0.0.1:{port}") as channel:
            frames = list(DocReaderStub(channel).ReadStream(ReadRequest(
                file_content=pdf_fixture(True), file_name="final.pdf", file_type="pdf",
                config=ReadConfig(parser_engine="builtin", parser_engine_overrides={"output_inventory": "v1"}),
            )))
        assert not frames[0].meta.error
        assert len(frames[0].meta.structured_source_units) == 8
        assert frames[0].meta.image_count == 4
        assert len(frames) == 5
        assert json.loads(frames[0].meta.metadata["output_inventory_manifest"])["page_count"] == 4
    finally:
        server.stop(0).wait()


def test_unknown_inventory_profile_does_not_fall_back():
    with pytest.raises(ValueError, match="unsupported output inventory"):
        Parser().parse_file("final.pdf", "pdf", pdf_fixture(True), engine_overrides={"output_inventory": "v2"})


def test_docx_used_notes_and_picture_bytes_have_immutable_inventory_identity():
    import zipfile
    from lxml import etree
    from PIL import Image
    doc = Document()
    paragraph = doc.add_paragraph("Notes follow")
    for kind in ("footnote", "endnote"):
        reference = OxmlElement(f"w:{kind}Reference")
        reference.set(qn("w:id"), "7")
        paragraph.add_run()._r.append(reference)
    image = BytesIO()
    Image.new("RGB", (10, 10), "blue").save(image, format="PNG")
    table = doc.add_table(rows=1, cols=1)
    table.cell(0, 0).paragraphs[0].add_run().add_picture(BytesIO(image.getvalue()))
    raw = BytesIO()
    doc.save(raw)
    with zipfile.ZipFile(BytesIO(raw.getvalue())) as archive:
        parts = {name: archive.read(name) for name in archive.namelist()}
    rels = etree.fromstring(parts["word/_rels/document.xml.rels"])
    content_types = etree.fromstring(parts["[Content_Types].xml"])
    w = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
    for kind in ("footnote", "endnote"):
        etree.SubElement(rels, "{http://schemas.openxmlformats.org/package/2006/relationships}Relationship", {
            "Id": f"rId{kind}", "Type": f"http://schemas.openxmlformats.org/officeDocument/2006/relationships/{kind}s",
            "Target": f"{kind}s.xml",
        })
        etree.SubElement(content_types, "{http://schemas.openxmlformats.org/package/2006/content-types}Override", {
            "PartName": f"/word/{kind}s.xml",
            "ContentType": f"application/vnd.openxmlformats-officedocument.wordprocessingml.{kind}s+xml",
        })
        parts[f"word/{kind}s.xml"] = (
            f'<w:{kind}s xmlns:w="{w}"><w:{kind} w:id="7"><w:p><w:r><w:t>{kind} content</w:t>'
            f'</w:r></w:p></w:{kind}><w:{kind} w:id="8"><w:p><w:r><w:t>Unused note</w:t>'
            f'</w:r></w:p></w:{kind}></w:{kind}s>'
        ).encode()
    parts["word/_rels/document.xml.rels"] = etree.tostring(rels)
    parts["[Content_Types].xml"] = etree.tostring(content_types)
    output = BytesIO()
    with zipfile.ZipFile(output, "w") as archive:
        for name, payload in parts.items():
            archive.writestr(name, payload)
    result = parse("notes.docx", output.getvalue())
    texts = [unit.text for unit in result.structured_source_units]
    assert "footnote content" in texts and "endnote content" in texts
    assert "Unused note" not in texts
    receipt = manifest(result)
    assert hashlib.sha256(image.getvalue()).hexdigest() in receipt["image_sha256"].values()
    pictures = [unit for unit in result.structured_source_units if unit.kind.value == "image_region"]
    assert len(pictures) == 1
    assert pictures[0].locator.compound_parent.parent_kind == "table_cell"


def test_docx_unused_first_story_is_excluded_until_inherited_section_uses_it():
    from docx.enum.section import WD_SECTION_START
    doc = Document()
    doc.add_paragraph("Body")
    doc.sections[0].first_page_header.paragraphs[0].text = "Conditional first header"
    raw = BytesIO()
    doc.save(raw)
    assert all(u.text != "Conditional first header" for u in parse("first.docx", raw.getvalue()).structured_source_units)
    section = doc.add_section(WD_SECTION_START.NEW_PAGE)
    section.different_first_page_header_footer = True
    raw = BytesIO()
    doc.save(raw)
    assert any(u.text == "Conditional first header" for u in parse("first.docx", raw.getvalue()).structured_source_units)


def test_docx_duplicate_bookmarks_are_explicit_not_checked():
    doc = Document()
    for index in range(2):
        p = doc.add_paragraph("Body")
        bookmark = OxmlElement("w:bookmarkStart")
        bookmark.set(qn("w:id"), str(index))
        bookmark.set(qn("w:name"), "duplicate_name")
        p._p.insert(0, bookmark)
    raw = BytesIO()
    doc.save(raw)
    entries = manifest(parse("duplicate.docx", raw.getvalue()))["units"]
    assert len([e for e in entries if "duplicate bookmarks" in (e["reason"] or "")]) == 2


@pytest.mark.parametrize("mixed", [False, True])
def test_exact_picture_has_one_visual_carrier_but_mixed_shape_stays_unchecked(mixed):
    from PIL import Image
    doc = Document()
    image = BytesIO()
    Image.new("RGB", (12, 8), "blue").save(image, format="PNG")
    paragraph = doc.add_paragraph("Adjacent text does not prove the picture")
    paragraph.add_run().add_picture(BytesIO(image.getvalue()))
    if mixed:
        # Keep the valid picture but add a second content carrier inside the
        # same graphic. Finding its image must not erase the shape/text story.
        graphic_data = paragraph._p.xpath(".//*[local-name()='graphicData']")[0]
        text_box = OxmlElement("w:txbxContent")
        text = OxmlElement("w:p")
        run = OxmlElement("w:r")
        value = OxmlElement("w:t")
        value.text = "Additional shape text"
        run.append(value)
        text.append(run)
        text_box.append(text)
        graphic_data.append(text_box)
    raw = BytesIO()
    doc.save(raw)
    result = parse("picture.docx", raw.getvalue())
    receipt = manifest(result)
    pictures = [entry for entry in receipt["units"] if entry["kind"] == "image"]
    assert len(pictures) == 1 and pictures[0]["status"] == "not_checked"
    drawings = [entry for entry in receipt["units"] if "}drawing" in (entry["reason"] or "")]
    textboxes = [entry for entry in receipt["units"] if "}txbxContent" in (entry["reason"] or "")]
    if mixed:
        assert drawings and textboxes
    else:
        assert not drawings and not textboxes
