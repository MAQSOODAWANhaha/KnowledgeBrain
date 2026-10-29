"""Review input must come from the immutable saved Word, including its tables."""
from io import BytesIO

from docx import Document

from docreader.tender.word_navigation import add_bookmark


def saved_bytes():
    document = Document()
    heading = document.add_heading("人工改名章节", level=1)
    add_bookmark(heading._p, "chapter-original")
    document.add_paragraph("人工修订后的实施内容，旧稿中不存在。")
    table = document.add_table(rows=1, cols=2)
    table.cell(0, 0).text = "工期"
    table.cell(0, 1).text = "人工填写：30天"
    document.add_heading("人工新增章节", level=1)
    document.add_paragraph("此处也是实际保存内容。")
    stream = BytesIO()
    document.save(stream)
    return stream.getvalue()


def test_saved_word_records_include_human_text_tables_and_unbound_content():
    from docreader.tender.saved_word_review import read_saved_word

    records, gaps = read_saved_word(saved_bytes(), [{"chapter_id": "chapter-original", "title": "旧标题", "children": [], "targets": []}])
    text = str(records)
    assert "人工修订后的实施内容" in text and "人工填写：30天" in text and "此处也是实际保存内容" in text
    assert records[0]["chapter_id"] == "chapter-original"
    assert any(record["chapter_id"] is None for record in records)
    assert any(gap["code"] == "saved_word_unbound_content" for gap in gaps)


def test_unanchored_document_is_reviewed_without_guessing_chapter_from_title():
    from docreader.tender.saved_word_review import read_saved_word

    document = Document()
    document.add_heading("旧标题", level=1)
    document.add_paragraph("同名标题不构成原章关系。")
    stream = BytesIO()
    document.save(stream)
    records, gaps = read_saved_word(stream.getvalue(), [{"chapter_id": "original", "title": "旧标题", "children": [], "targets": []}])
    assert all(record["chapter_id"] is None for record in records)
    assert any(gap["code"] == "saved_word_missing_chapter" for gap in gaps)
