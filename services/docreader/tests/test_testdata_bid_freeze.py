"""Freeze-shaped parse of real files under testdata/bid."""

from __future__ import annotations

from collections import Counter
from pathlib import Path

import pytest

from docreader.main import _structured_unit_to_proto
from docreader.models.document import (
    PageTableLocator,
    StructuredSourceUnitKind,
)
from docreader.parser.docx2_parser import Docx2Parser
from docreader.parser.excel_parser import ExcelParser
from docreader.parser.pdf_parser import PDFParser

BID_DIR = Path("/opt/github/KnowledgeBrain/testdata/bid")
TENDER_EXTS = frozenset({"pdf", "docx", "xlsx"})


def _files(*, large: bool) -> list[Path]:
    files = sorted(p for p in BID_DIR.iterdir() if p.is_file())
    if large:
        return [p for p in files if p.stat().st_size > 10_000_000]
    return [p for p in files if p.stat().st_size <= 10_000_000]


def _parse(path: Path):
    ext = path.suffix.lower().lstrip(".")
    data = path.read_bytes()
    if ext == "pdf":
        return PDFParser(file_name=path.name, file_type="pdf").parse_into_text(data)
    if ext == "docx":
        return Docx2Parser(file_name=path.name, file_type="docx").parse_into_text(data)
    if ext in {"xlsx", "xlsm"}:
        return ExcelParser(file_name=path.name, file_type="xlsx").parse_into_text(data)
    raise AssertionError(f"unexpected testdata file {path.name}")


def _assert_freeze_contract(path: Path) -> None:
    ext = path.suffix.lower().lstrip(".")
    document = _parse(path)
    units = document.structured_source_units
    assert units, f"{path.name} produced no structured units"
    kinds = Counter(unit.kind for unit in units)

    for unit in units:
        if unit.kind is StructuredSourceUnitKind.TABLE_REGION:
            assert unit.text == "", f"{path.name} {unit.key} TABLE_REGION text must be empty"
        if unit.grid is not None:
            occupied = sum(cell.row_span * cell.col_span for cell in unit.grid.cells)
            assert occupied == unit.grid.row_count * unit.grid.column_count, (
                f"{path.name} {unit.key} does not tile"
            )
        if isinstance(unit.locator, PageTableLocator):
            proto = _structured_unit_to_proto(unit)
            locator = proto.page_table
            assert locator.row_count == 0
            assert locator.column_count == 0
            assert list(locator.cells) == []
            assert list(locator.merged_ranges) == []
            assert list(locator.column_edges) == []
            assert list(locator.widths_mm) == []
            assert unit.grid is not None

    if ext == "docx":
        assert kinds.get(StructuredSourceUnitKind.TABLE_ROW, 0) == 0
        assert kinds.get(StructuredSourceUnitKind.TABLE_REGION, 0) > 0
        assert any(unit.grid is not None for unit in units)

    if ext == "pdf":
        tables = [unit for unit in units if isinstance(unit.locator, PageTableLocator)]
        if tables:
            assert all(unit.grid is not None and unit.grid.widths_mm for unit in tables)

    if ext == "xlsm":
        assert kinds.get(StructuredSourceUnitKind.TABLE_ROW, 0) > 0
        used = [
            unit
            for unit in units
            if unit.key.endswith(":used") and unit.grid is not None
        ]
        assert used, f"{path.name} used-range forms missing"
    elif ext in TENDER_EXTS:
        assert ext in TENDER_EXTS


@pytest.mark.parametrize("path", _files(large=False), ids=lambda p: p.name)
def test_testdata_bid_freeze_contract(path: Path) -> None:
    _assert_freeze_contract(path)


@pytest.mark.parametrize("path", _files(large=True), ids=lambda p: p.name)
def test_testdata_bid_large_pdf_freeze_contract(path: Path) -> None:
    _assert_freeze_contract(path)
