from io import BytesIO

from docx import Document

from docreader.tender.authoring_export import _prepare_document, xml_text


def test_xml_text_drops_pdfium_noncharacter_without_changing_other_glyphs():
    assert xml_text("业务系统-电子招投标\ufffe参与的项目") == "业务系统-电子招投标参与的项目"
    assert xml_text("甲🙂") == "甲🙂"


def test_word_export_accepts_pending_text_that_contains_xml_noncharacters():
    document = _prepare_document()
    document.add_paragraph("业务系统-电子招投标\ufffe参与的项目-招标业务管理-标前质疑")
    output = BytesIO()
    document.save(output)
    text = "\n".join(paragraph.text for paragraph in Document(output).paragraphs)
    assert "电子招投标参与的项目" in text
    assert "\ufffe" not in text
