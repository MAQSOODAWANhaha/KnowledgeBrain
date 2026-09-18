from __future__ import annotations

from pathlib import Path

import pytest

from docreader.parser import pdf_tables
from docreader.parser.pdf_parser import PDFParser, _extract_page_tables
from docreader.parser.pdf_tables import (
    MAX_PDF_TABLE_COLUMNS,
    PdfTableCell,
    PdfTableGrid,
    TableExtractionLimitError,
    _tables_from_rules,
    extract_tables_from_chars,
    extract_tables_from_page,
    grid_markdown,
    reading_order_text,
    widths_mm,
)
from docreader.models.document import (
    PageLocator,
    PageTableLocator,
    PdfTableCell as ModelCell,
    StructuredSourceUnitKind,
    sparsify_table_cells,
)

BIDDING_PDF = Path(__file__).resolve().parents[3] / "testdata/bid/BiddingFile.pdf"


def _glyph(ch: str, x: float, y: float, w: float = 8.0, h: float = 10.0) -> dict:
    return {"ch": ch, "x0": x, "x1": x + w, "y0": y, "y1": y + h}


def _cell_text(x: float, y: float, text: str) -> list[dict]:
    glyphs = []
    cursor = x
    for ch in text:
        glyphs.append(_glyph(ch, cursor, y, w=7.0))
        cursor += 7.0
    return glyphs


def test_alignment_grid_has_cells_not_page_blob() -> None:
    chars: list[dict] = []
    headers = ["列甲", "列乙", "列丙", "列丁"]
    values = [
        ["一", "值甲", "", ""],
        ["二", "值乙", "", ""],
        ["三", "值丙", "", ""],
    ]
    xs = [40.0, 120.0, 280.0, 420.0]
    y = 700.0
    for index, header in enumerate(headers):
        chars.extend(_cell_text(xs[index], y, header))
    for row in values:
        y -= 18.0
        for index, value in enumerate(row):
            if value:
                chars.extend(_cell_text(xs[index], y, value))
    tables = extract_tables_from_chars(chars, min_columns=3, min_rows=3)
    assert len(tables) == 1
    grid = tables[0]
    assert grid.column_count == 4
    assert grid.row_count >= 3
    assert len(grid.cells) == grid.row_count * grid.column_count
    header_row = [cell.text for cell in grid.cells if cell.row == 0]
    assert "列乙" in "".join(header_row)
    bidder = [cell.text for cell in grid.cells if cell.row > 0 and cell.column == 2]
    assert all(not text.strip() for text in bidder)
    mm = widths_mm(grid)
    assert len(mm) == grid.column_count
    assert sum(mm) <= 180.0 + 1e-6


def test_ruled_grid_freezes_objective_merged_span() -> None:
    horizontal = [(0.0, y, 30.0) for y in (0.0, 10.0, 20.0, 30.0)]
    vertical = [
        (0.0, 0.0, 30.0),
        (10.0, 0.0, 20.0),  # Missing across the top row: columns 0-1 are merged.
        (20.0, 0.0, 30.0),
        (30.0, 0.0, 30.0),
    ]
    chars = [
        _glyph(chr(ord("A") + row * 3 + column), column * 10 + 2, 22 - row * 10, w=2)
        for row in range(3)
        for column in range(3)
    ]
    grids = _tables_from_rules(horizontal, vertical, chars)
    assert len(grids) == 1
    assert grids[0].merged_ranges == [(0, 0, 0, 1)]
    anchor = next(cell for cell in grids[0].cells if (cell.row, cell.column) == (0, 0))
    assert (anchor.row_span, anchor.col_span) == (1, 2)
    assert anchor.text == "AB"
    covered = next(cell for cell in grids[0].cells if (cell.row, cell.column) == (0, 1))
    assert covered.text == ""


def test_ruled_grid_wins_over_fragmented_alignment(monkeypatch) -> None:
    aligned = PdfTableGrid(
        left=0,
        top=100,
        right=100,
        bottom=0,
        row_count=4,
        column_count=15,
    )
    ruled = PdfTableGrid(
        left=0,
        top=100,
        right=100,
        bottom=0,
        row_count=8,
        column_count=4,
        column_edges=[0.0, 25.0, 50.0, 75.0, 100.0],
    )
    monkeypatch.setattr(pdf_tables, "collect_rule_lines", lambda _page, _raw: ([], []))
    monkeypatch.setattr(pdf_tables, "_tables_from_rules", lambda *_args: [ruled])
    monkeypatch.setattr(pdf_tables, "extract_tables_from_chars", lambda _chars: [aligned])
    assert extract_tables_from_page(object(), object(), []) == [ruled]


@pytest.mark.skipif(not BIDDING_PDF.is_file(), reason="testdata/bid/BiddingFile.pdf is not available")
def test_bidding_file_emits_page_table_grids() -> None:
    parsed = PDFParser(file_name="BiddingFile.pdf", file_type="pdf").parse_into_text(
        BIDDING_PDF.read_bytes()
    )
    tables = [
        unit
        for unit in parsed.structured_source_units
        if unit.kind is StructuredSourceUnitKind.TABLE_REGION
    ]
    assert tables, "BiddingFile must emit TABLE_REGION units"
    page_tables = [unit for unit in tables if isinstance(unit.locator, PageTableLocator)]
    assert page_tables, "PDF tables must use page_table locators"
    # Cell bytes have one canonical owner and are never duplicated into unit text.
    assert all(unit.text == "" for unit in page_tables)
    widest = max(
        page_tables,
        key=lambda unit: unit.grid.column_count if unit.grid is not None else 0,
    )
    assert isinstance(widest.locator, PageTableLocator)
    assert widest.grid is not None
    assert widest.grid.column_count >= 6
    assert widest.grid.row_count >= 2
    for unit in page_tables:
        locator = unit.locator
        grid = unit.grid
        assert isinstance(locator, PageTableLocator)
        assert grid is not None
        assert grid.column_count >= 2
        assert grid.row_count >= 2
        occupied = 0
        for cell in grid.cells:
            occupied += cell.row_span * cell.col_span
        assert occupied == grid.row_count * grid.column_count
        assert locator.right > locator.left
        assert locator.top > locator.bottom
        assert grid.widths_mm is not None
        assert len(grid.widths_mm) == grid.column_count
        assert all(width > 0 for width in grid.widths_mm)
        assert sum(grid.widths_mm) <= 180.01
        nonempty = [cell.text for cell in grid.cells if cell.text.strip()]
        assert nonempty
        assert all(len(text) < 4000 for text in nonempty)


def test_scanned_pages_do_not_invent_tables() -> None:
    from docreader.parser.pdf_parser import _classify_page

    assert _classify_page(1.0, 0) == "scanned"
    assert _classify_page(0.0, 200) == "text"
    assert extract_tables_from_chars([], min_columns=3, min_rows=3) == []


def test_oversize_column_count_skips_that_table() -> None:
    chars: list[dict] = []
    y = 800.0
    for row in range(3):
        for column in range(MAX_PDF_TABLE_COLUMNS + 1):
            chars.extend(_cell_text(40.0 + column * 48.0, y, f"C{column}"))
        y -= 16.0
    assert extract_tables_from_chars(chars, min_columns=3, min_rows=3) == []


def test_oversize_band_does_not_wipe_valid_band() -> None:
    chars: list[dict] = []
    y = 800.0
    for row in range(3):
        for column in range(MAX_PDF_TABLE_COLUMNS + 1):
            chars.extend(_cell_text(40.0 + column * 48.0, y, f"C{column}"))
        y -= 16.0
    y -= 40.0
    chars.extend(_cell_text(40.0, y, "段落正文分隔"))
    y -= 40.0
    for row in range(3):
        for column, label in enumerate(["列甲", "列乙", "列丙", "列丁"]):
            chars.extend(_cell_text(40.0 + column * 80.0, y, f"{label}{row}"))
        y -= 18.0
    tables = extract_tables_from_chars(chars, min_columns=3, min_rows=3)
    assert tables
    assert all(grid.column_count <= MAX_PDF_TABLE_COLUMNS for grid in tables)
    assert any(grid.column_count >= 3 for grid in tables)


def test_page_limit_falls_back_without_raising(monkeypatch) -> None:
    kept = PdfTableGrid(
        left=0,
        top=100,
        right=100,
        bottom=0,
        row_count=4,
        column_count=4,
        column_edges=[0.0, 25.0, 50.0, 75.0, 100.0],
    )
    horizontal = [(0.0, float(y), 100.0) for y in (0, 10, 20, 30)]
    vertical = [(float(x), 0.0, 30.0) for x in (0, 10, 20, 30)]
    monkeypatch.setattr(
        pdf_tables, "collect_rule_lines", lambda _page, _raw: (horizontal, vertical)
    )
    monkeypatch.setattr(
        pdf_tables,
        "_tables_from_rules",
        lambda *_args: (_ for _ in ()).throw(
            TableExtractionLimitError("OUTLINE_TABLE_EXTRACTION_UNSUPPORTED")
        ),
    )
    monkeypatch.setattr(pdf_tables, "extract_tables_from_chars", lambda _chars: [kept])
    monkeypatch.setattr(pdf_tables, "_grid_widths_ok", lambda _grid: True)
    assert extract_tables_from_page(object(), object(), []) == [kept]


def test_illegal_widths_fail_named() -> None:
    grid = PdfTableGrid(
        left=0,
        top=10,
        right=30,
        bottom=0,
        row_count=3,
        column_count=3,
        column_edges=[0.0, 10.0, 9.0, 30.0],
    )
    try:
        widths_mm(grid)
        raised = False
    except TableExtractionLimitError as error:
        raised = True
        assert error.code == "OUTLINE_TABLE_EXTRACTION_UNSUPPORTED"
    assert raised


def test_grid_markdown_serializes_cells() -> None:
    grid = PdfTableGrid(
        left=0,
        top=30,
        right=30,
        bottom=0,
        row_count=2,
        column_count=2,
        cells=[
            PdfTableCell(row=0, column=0, row_span=1, col_span=1, text="甲"),
            PdfTableCell(row=0, column=1, row_span=1, col_span=1, text="乙"),
            PdfTableCell(row=1, column=0, row_span=1, col_span=1, text="丙"),
            PdfTableCell(row=1, column=1, row_span=1, col_span=1, text="丁"),
        ],
        column_edges=[0.0, 15.0, 30.0],
    )
    lines = grid_markdown(grid).splitlines()
    assert lines[0] == "| 甲 | 乙 |"
    assert lines[1] == "| --- | --- |"
    assert lines[2] == "| 丙 | 丁 |"


@pytest.mark.skipif(not BIDDING_PDF.is_file(), reason="testdata/bid/BiddingFile.pdf is not available")
def test_price_title_stays_outside_grid_and_parameters_keep_their_cell() -> None:
    parsed = PDFParser(file_name="BiddingFile.pdf", file_type="pdf").parse_into_text(
        BIDDING_PDF.read_bytes()
    )
    price = next(
        unit for unit in parsed.structured_source_units
        if isinstance(unit.locator, PageTableLocator) and unit.locator.page_ordinal == 81
    )
    assert price.grid is not None
    assert (price.grid.row_count, price.grid.column_count) == (3, 8)
    cells = {(c.row, c.column): c.text for c in price.grid.cells}
    assert cells[0, 0] == "序号"
    assert cells[1, 1] == "防火墙" and cells[1, 5] == "80"
    assert "20Gbps" in cells[1, 2] and "软件及特征库升级" in cells[1, 2]
    assert all(cells[1, column] == "" for column in (3, 4, 6, 7))
    page = next(u.text for u in parsed.structured_source_units
                if isinstance(u.locator, PageLocator) and u.locator.page_ordinal == 81)
    assert "单位：元人民币" in "".join(page.split())
    assert "单位：元人民币" not in "".join(cells.values())


def test_extract_page_tables_pdfium_failure_returns_empty() -> None:
    import pypdfium2 as pdfium

    class FakePage:
        def get_textpage(self):
            raise pdfium.PdfiumError("fail")

    tables, leftover = _extract_page_tables(FakePage(), object())
    assert tables == []
    assert leftover == ""


def test_extract_page_tables_does_not_swallow_non_pdfium_errors() -> None:
    class FakePage:
        def get_textpage(self):
            raise ValueError("geometry boom")

    try:
        _extract_page_tables(FakePage(), object())
        raised = False
    except ValueError:
        raised = True
    assert raised


def test_reading_order_keeps_tiny_punctuation_on_the_line() -> None:
    glyphs = [
        _glyph("标", 40.0, 100.0, w=10.0, h=10.0),
        _glyph("示", 52.0, 100.0, w=10.0, h=10.0),
        _glyph("）", 64.0, 99.0, w=4.0, h=10.0),
        _glyph("。", 70.0, 101.0, w=2.5, h=2.6),
        _glyph("未", 78.0, 100.0, w=10.0, h=10.0),
        _glyph("如", 90.0, 100.0, w=10.0, h=10.0),
    ]
    assert "）。未如" in reading_order_text(glyphs).replace(" ", "")


def test_ruled_blank_template_keeps_unfilled_rows() -> None:
    horizontal = [(0.0, float(y), 30.0) for y in range(0, 81, 10)]
    vertical = [(float(x), 0.0, 80.0) for x in range(0, 31, 10)]
    chars = [_glyph(label, 2 + index * 10, 73, w=2, h=3)
             for index, label in enumerate("ABC")]
    grid, = _tables_from_rules(horizontal, vertical, chars)
    assert (grid.row_count, grid.column_count) == (8, 3)
    assert all(not cell.text for cell in grid.cells if cell.row > 0)


def test_merged_wrapped_text_does_not_concatenate_virtual_columns() -> None:
    horizontal = [(0.0, float(y), 30.0) for y in (0, 10, 20, 40)]
    vertical = [(0.0, 0.0, 40.0), (10.0, 0.0, 20.0),
                (20.0, 0.0, 40.0), (30.0, 0.0, 40.0)]
    chars = [_glyph(label, x, y, w=2, h=3)
             for label, x, y in [('A', 2, 33), ('B', 12, 33),
                                 ('C', 2, 23), ('D', 12, 23)]]
    grid, = _tables_from_rules(horizontal, vertical, chars)
    assert grid.cells[0].text == 'ABCD'
    assert grid.cells[0].col_span == 2
    assert grid.cells[1].text == ''


def test_sparsify_drops_covered_shells_and_tiles() -> None:
    dense = [
        ModelCell(row=0, column=0, row_span=1, col_span=2, text="甲"),
        ModelCell(row=0, column=1, row_span=1, col_span=1, text=""),
        ModelCell(row=1, column=0, row_span=1, col_span=1, text="乙"),
        ModelCell(row=1, column=1, row_span=1, col_span=1, text="丙"),
    ]
    sparse = sparsify_table_cells(2, 2, dense, [(0, 0, 0, 1)])
    assert [(c.row, c.column, c.col_span) for c in sparse] == [
        (0, 0, 2),
        (1, 0, 1),
        (1, 1, 1),
    ]
