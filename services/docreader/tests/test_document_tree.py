"""The parse result is one document tree, not a flat page list."""

from docreader.models.document import (
    Document,
    DocumentLocator,
    ImageLocator,
    PageTableLocator,
    SpreadsheetLocator,
    SpreadsheetRange,
    StructuredSourceUnit,
    StructuredSourceUnitKind,
)
from docreader.parser.document_tree import build_document_tree


def _section(key, ordinal, section_ordinal, heading, text):
    return StructuredSourceUnit(
        key=key,
        ordinal=ordinal,
        kind=StructuredSourceUnitKind.SECTION,
        text=text,
        locator=DocumentLocator(section_ordinal=section_ordinal, heading_path=heading),
    )


def test_chapters_nest_and_tables_hang_on_the_owning_chapter():
    units = [
        _section("section:0:page:0", 0, 0, "第一章 招标公告", "投标人应具备资格。"),
        StructuredSourceUnit(
            key="page:0:table:0",
            ordinal=1,
            kind=StructuredSourceUnitKind.TABLE_REGION,
            locator=PageTableLocator(
                page_ordinal=0, table_ordinal=0, left=0, top=100, right=10, bottom=90
            ),
        ),
        _section("section:0:page:1", 2, 0, "第一章 招标公告", "详见下页。"),
        _section("section:1:page:1", 3, 1, "第一章 招标公告 > 一、投标函", "投标人名称。"),
        StructuredSourceUnit(
            key="page:2:image:0",
            ordinal=4,
            kind=StructuredSourceUnitKind.IMAGE_REGION,
            locator=ImageLocator(
                original_ref="images/scan.jpg",
                width=10,
                height=10,
                media_type="image/jpeg",
                page_ordinal=2,
            ),
        ),
    ]
    tree = build_document_tree(units)
    chapter = tree.children[0]
    assert chapter.kind == "section"
    assert chapter.heading_path == "第一章 招标公告"
    assert chapter.section_ordinal == 0
    assert chapter.keys == ["section:0:page:0", "section:0:page:1"]
    assert [child.kind for child in chapter.children] == ["table", "section"]
    clause = chapter.children[1]
    assert clause.heading_path == "第一章 招标公告 > 一、投标函"
    assert [child.kind for child in clause.children] == ["image"]
    document = Document(structured_source_units=units, content="ignored")
    assert document.tree.kind == "document"
    assert document.tree.children[0].keys == chapter.keys
    assert "第一章 招标公告" in document.metadata["document_tree"]


def test_markdown_without_units_becomes_a_chapter_tree():
    tree = build_document_tree([], "# 第一章\n资格条件。\n## 投标函\n投标人名称。")
    assert tree.children[0].title == "第一章"
    assert tree.children[0].children[0].kind == "text"
    assert tree.children[0].children[1].title == "投标函"
    assert tree.children[0].children[1].heading_path == "第一章 > 投标函"


def test_sheet_rows_hang_on_the_sheet():
    sheet = StructuredSourceUnit(
        key="sheet:0",
        ordinal=0,
        kind=StructuredSourceUnitKind.SECTION,
        text="报价",
        locator=SpreadsheetLocator(
            sheet_ordinal=0,
            sheet_name="报价",
            region=SpreadsheetRange(
                a1_range="A1:B2", start_row=1, start_column=1, end_row=2, end_column=2
            ),
        ),
    )
    row = StructuredSourceUnit(
        key="sheet:0:row:1",
        ordinal=1,
        kind=StructuredSourceUnitKind.TABLE_ROW,
        text="名称 | 单价",
        locator=SpreadsheetLocator(
            sheet_ordinal=0,
            sheet_name="报价",
            region=SpreadsheetRange(
                a1_range="A1:B1", start_row=1, start_column=1, end_row=1, end_column=2
            ),
        ),
    )
    tree = build_document_tree([sheet, row])
    assert tree.children[0].kind == "sheet"
    assert tree.children[0].title == "报价"
    assert tree.children[0].children[0].kind == "row"
    assert tree.children[0].children[0].key == "sheet:0:row:1"
