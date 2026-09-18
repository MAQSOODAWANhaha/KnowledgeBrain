"""Full-document coverage for testdata/bid/BiddingFile.pdf."""

from __future__ import annotations

import re
import subprocess
from collections import Counter
from functools import lru_cache
from pathlib import Path

import pytest

from docreader.models.document import (
    PageLocator,
    PageTableLocator,
    StructuredSourceUnitKind,
    TableGrid,
)
from docreader.parser.pdf_parser import PDFParser, _close_pdfium_resource, _page_chars
from docreader.parser.pdf_tables import extract_tables_from_page, table_region_reading_text

BIDDING_PDF = Path(__file__).resolve().parents[3] / "testdata/bid/BiddingFile.pdf"
pytestmark = pytest.mark.skipif(
    not BIDDING_PDF.is_file(), reason="testdata/bid/BiddingFile.pdf is not available"
)
PAGE_COUNT = 106
KEY_PHRASES = ("招标文件", "投标价格表", "商务和技术偏差")
PAGE_HEADER = "华盾公司 2024-2025 年广域网防火墙框架采购招标文件"
OUTSIDE_CLAUSE = "未如实填写"


def compact(text: str) -> str:
    """Fold whitespace, markdown hashes, and fill-in blanks."""
    text = re.sub(r"(?m)^#+\s*", "", text or "")
    text = re.sub(r"_+", "", text)
    return re.sub(r"\s+", "", text)


def fourgrams(text: str, n: int = 4) -> Counter:
    folded = compact(text)
    if len(folded) < n:
        return Counter()
    return Counter(folded[i : i + n] for i in range(len(folded) - n + 1))


def gold_fourgrams(pdftotext_layout: str, n: int = 4) -> Counter:
    """Compact 4-grams that do not use 2-character wrap fragments as gold."""
    out = Counter()
    for line in (pdftotext_layout or "").splitlines():
        columns = re.split(r" {2,}", line.strip()) if line.strip() else []
        for column in columns:
            parts = [compact(token) for token in column.split() if compact(token)]
            if not parts:
                continue
            buf = ""
            for part in parts:
                if len(part) <= 2:
                    if len(buf) >= n:
                        out.update(buf[i : i + n] for i in range(len(buf) - n + 1))
                    buf = ""
                else:
                    buf += part
            if len(buf) >= n:
                out.update(buf[i : i + n] for i in range(len(buf) - n + 1))
    return out


def fourgram_recall(gold: Counter, got: Counter) -> float:
    if not gold:
        return 1.0
    hit = sum(min(got[key], value) for key, value in gold.items())
    total = sum(gold.values())
    return hit / total if total else 1.0


@lru_cache(maxsize=1)
def _parsed():
    return PDFParser(file_name="BiddingFile.pdf", file_type="pdf").parse_into_text(
        BIDDING_PDF.read_bytes()
    )


@lru_cache(maxsize=1)
def _pdftotext_pages() -> tuple[str, ...]:
    raw = subprocess.check_output(
        ["pdftotext", "-layout", str(BIDDING_PDF), "-"],
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    pages = raw.split("\f")
    if pages and not pages[-1].strip():
        pages = pages[:-1]
    return tuple(pages)


def _page_units(parsed, page_ordinal: int):
    leftover = []
    cells = []
    for unit in parsed.structured_source_units:
        locator = unit.locator
        if isinstance(locator, PageLocator) and locator.page_ordinal == page_ordinal:
            leftover.append(unit.text or "")
        elif isinstance(locator, PageTableLocator) and locator.page_ordinal == page_ordinal:
            if unit.grid is not None:
                for cell in unit.grid.cells:
                    cells.append(cell.text or "")
    return leftover, cells


def _owner_cell(grid: TableGrid, row: int, column: int):
    for cell in grid.cells:
        if (
            cell.row <= row < cell.row + cell.row_span
            and cell.column <= column < cell.column + cell.col_span
        ):
            return cell
    raise AssertionError(f"no cell covers ({row}, {column})")


@lru_cache(maxsize=1)
def _table_reconstructions() -> tuple[str, ...]:
    """Independent in-table reading order; not written into locator.cells."""
    import pypdfium2 as pdfium
    import pypdfium2.raw as pdfium_r

    pdf = pdfium.PdfDocument(BIDDING_PDF.read_bytes())
    out: list[str] = []
    try:
        for index in range(len(pdf)):
            page = pdf[index]
            textpage = None
            try:
                textpage = page.get_textpage()
                chars, _width = _page_chars(textpage, page, pdfium_r)
                tables = extract_tables_from_page(page, pdfium_r, chars) if chars else []
                out.append(table_region_reading_text(chars, tables) if tables else "")
            finally:
                _close_pdfium_resource(textpage)
                _close_pdfium_resource(page)
    finally:
        _close_pdfium_resource(pdf)
    return tuple(out)


def test_bidding_file_has_section_per_page_without_table_error() -> None:
    parsed = _parsed()
    assert "table_extraction_error" not in parsed.metadata
    assert parsed.metadata.get("page_count") == PAGE_COUNT
    sections = [
        unit
        for unit in parsed.structured_source_units
        if unit.kind is StructuredSourceUnitKind.SECTION
        and isinstance(unit.locator, PageLocator)
    ]
    pages = {
        unit.locator.page_ordinal
        for unit in sections
        if isinstance(unit.locator, PageLocator)
    }
    assert len(sections) == PAGE_COUNT
    assert pages == set(range(PAGE_COUNT))


def test_original_blank_forms_have_complete_independent_grids() -> None:
    # Dimensions and labels checked against the actual physical PDF pages,
    # not against historical outline/parser output JSON.
    expected = {
        (70, 0): (14, 5), (70, 1): (14, 5),
        (82, 0): (3, 8), (95, 0): (18, 5),
        (96, 0): (4, 10), (96, 1): (4, 8), (96, 2): (4, 2),
        (98, 1): (7, 11), (103, 0): (4, 12), (104, 0): (4, 12),
    }
    grids = {
        (u.locator.page_ordinal + 1, u.locator.table_ordinal): u.grid
        for u in _parsed().structured_source_units
        if isinstance(u.locator, PageTableLocator) and u.grid is not None
    }
    for key, shape in expected.items():
        grid = grids[key]
        assert (grid.row_count, grid.column_count) == shape, key
        occupied = sum(cell.row_span * cell.col_span for cell in grid.cells)
        assert occupied == shape[0] * shape[1]
    for index in (0, 1):
        grid = grids[70, index]
        assert _owner_cell(grid, 0, 0).row_span == 2
        assert _owner_cell(grid, 0, 1).col_span == 2
        assert _owner_cell(grid, 0, 3).col_span == 2
        assert all(not c.text for c in grid.cells if c.row >= 2)
    assert "经营活动现金净流量" in _owner_cell(grids[96, 2], 0, 1).text
    for page in (103, 104):
        grid = grids[page, 0]
        assert compact(_owner_cell(grid, 0, 9).text) == "签约合同价"
        assert all(not c.text for c in grid.cells if c.row >= 1)
    assert not any(page == 100 for page, _ in grids), "declaration prose is not a table"
    text = next(u.text for u in _parsed().structured_source_units
                if isinstance(u.locator, PageLocator) and u.locator.page_ordinal == 99)
    assert '6.我公司未被“信用中国”网站（www.creditchina.gov.cn）列入“失信被执行' in compact(text)


def test_key_phrases_present() -> None:
    parsed = _parsed()
    blob = compact(
        "".join(
            (unit.text or "")
            + "".join(
                cell.text or ""
                for cell in (unit.grid.cells if unit.grid is not None else [])
            )
            for unit in parsed.structured_source_units
        )
    )
    for phrase in KEY_PHRASES:
        assert compact(phrase) in blob, phrase


def test_pdftotext_compact_4gram_coverage() -> None:
    parsed = _parsed()
    gold_pages = _pdftotext_pages()
    reconstructions = _table_reconstructions()
    assert len(gold_pages) == PAGE_COUNT
    assert len(reconstructions) == PAGE_COUNT
    all_gold = Counter()
    all_got = Counter()
    below = []
    for page_ordinal, gold in enumerate(gold_pages):
        leftover, _cells = _page_units(parsed, page_ordinal)
        got = fourgrams("".join(leftover) + reconstructions[page_ordinal])
        gold_grams = gold_fourgrams(gold)
        all_gold.update(gold_grams)
        all_got.update(got)
        score = fourgram_recall(gold_grams, got)
        if score < 0.95:
            below.append((page_ordinal + 1, round(score, 4)))
    global_score = fourgram_recall(all_gold, all_got)
    assert not below, f"pages below 0.95: {below}"
    assert global_score >= 0.99, global_score


def test_leftover_section_keeps_outside_clause() -> None:
    parsed = _parsed()
    sections = {
        unit.locator.page_ordinal: unit.text or ""
        for unit in parsed.structured_source_units
        if unit.kind is StructuredSourceUnitKind.SECTION
        and isinstance(unit.locator, PageLocator)
    }
    leftover = compact("".join(sections.values()))
    assert compact(OUTSIDE_CLAUSE) in leftover
    assert compact(PAGE_HEADER) in compact(sections[91])


def test_markdown_serializes_leftover_and_cells() -> None:
    parsed = _parsed()
    folded = compact(parsed.content)
    assert compact(PAGE_HEADER) in folded
    assert compact("技术参数表") in folded
    assert compact("网络处理能力") in folded
    assert "|" in (parsed.content or "")
