"""Source identity uses the values the loaded native parsers actually consume."""

from dataclasses import replace
import json

import pytest

from docreader.parser import docx_parser, excel_parser, pdf_parser, pdf_tables
from docreader.workflow.source_spec import source_spec


def test_source_spec_is_fresh_canonical_json_of_effective_settings():
    spec = source_spec()
    assert json.loads(json.dumps(spec, sort_keys=True, allow_nan=False)) == spec
    assert spec["pdf"]["scan_min_chars_per_page"] == pdf_parser.SCAN_MIN_CHARS_PER_PAGE
    assert spec["pdf"]["scan_image_area_ratio"] == pdf_parser.SCAN_IMAGE_AREA_RATIO
    assert spec["pdf"]["force_scanned"] is False
    assert spec["pdf"]["full_page_fallback"] is False
    assert spec["pdf"]["render_dpi"] == max(1, pdf_parser.CONFIG.pdf_render_dpi)
    assert spec["pdf"]["jpeg_quality"] == pdf_parser._normalize_image_quality(pdf_parser.CONFIG.pdf_jpeg_quality)
    assert spec["pdf"]["table_max_columns"] == pdf_tables.MAX_PDF_TABLE_COLUMNS
    assert spec["pdf"]["table_max_rows"] == pdf_tables.MAX_PDF_TABLE_ROWS
    assert spec["xlsx"]["max_materialized_cells"] == excel_parser.XLSX_MAX_MATERIALIZED_CELLS
    spec["pdf"]["scan_min_chars_per_page"] += 999
    assert source_spec()["pdf"]["scan_min_chars_per_page"] == pdf_parser.SCAN_MIN_CHARS_PER_PAGE


@pytest.mark.parametrize("constant", [
    "SCAN_IMAGE_AREA_RATIO", "SCAN_MIN_CHARS_PER_PAGE", "_LOW_TEXT_IMAGE_RATIO",
    "EXTRACT_EMBEDDED_IMAGES", "EMBED_MIN_PIXELS", "EMBED_MIN_AREA_RATIO", "EMBED_REPEAT_PAGE_FRAC",
    "EMBED_MAX_IMAGES", "LAYOUT_ORDERING", "WORD_GAP_WIDTH_RATIO", "DETECT_HEADINGS", "FILTER_HIDDEN_TEXT",
    "MARGIN_COL_WIDTH_RATIO", "MIN_HEADING_LINE_CHARS", "SANITIZE_PDF_TEXT", "STRIP_CHART_TEXT_DEBRIS",
    "RENDER_VECTOR_FIGURES", "MIN_CHART_REGION_CHARS", "MIN_CHART_REGION_AREA_RATIO",
    "MAX_CHART_REGION_AREA_RATIO", "MAX_FIGURE_HEIGHT_RATIO",
])
def test_each_effective_pdf_setting_changes_source_identity(monkeypatch, constant):
    before = source_spec()
    current = getattr(pdf_parser, constant)
    changed = not current if isinstance(current, bool) else current + 1
    monkeypatch.setattr(pdf_parser, constant, changed)
    assert source_spec() != before


@pytest.mark.parametrize("constant", [
    "XLSX_MAX_MATERIALIZED_CELLS", "XLSX_MAX_LOGICAL_ROW", "XLSX_MAX_LOGICAL_COLUMN",
    "XLSX_MAX_CELLS_PER_ROW", "XLSX_MAX_MERGES", "XLSX_MAX_TABLES", "XLSX_MAX_RANGE_CELLS",
    "XLSX_MAX_UNITS", "XLSX_MAX_TEXT_BYTES", "XLSX_MAX_CELL_PAYLOAD_BYTES", "XLSX_MAX_TABLE_CELL_SCANS",
    "XLSX_MAX_ARCHIVE_ENTRIES", "XLSX_MAX_UNCOMPRESSED_BYTES", "XLSX_MAX_XML_PART_BYTES",
    "XLSX_MAX_COMPRESSION_RATIO",
])
def test_each_effective_excel_budget_changes_source_identity(monkeypatch, constant):
    before = source_spec()
    monkeypatch.setattr(excel_parser, constant, getattr(excel_parser, constant) + 1)
    assert source_spec() != before


def test_render_and_word_settings_use_effective_normalized_values(monkeypatch):
    monkeypatch.setattr(pdf_parser, "CONFIG", replace(pdf_parser.CONFIG, pdf_render_dpi=-1,
                                                    pdf_jpeg_quality=120, pdf_render_max_edge=-99))
    monkeypatch.setattr(docx_parser, "CONFIG", replace(docx_parser.CONFIG, docx_max_pages=0))
    first = source_spec()
    assert first["pdf"]["render_dpi"] == 1
    assert first["pdf"]["jpeg_quality"] == 95
    assert first["pdf"]["render_max_edge"] == 0
    assert first["docx"]["max_pages"] == 100000
    monkeypatch.setattr(pdf_parser, "CONFIG", replace(pdf_parser.CONFIG, pdf_render_dpi=200,
                                                    pdf_jpeg_quality=80, pdf_render_max_edge=2000))
    monkeypatch.setattr(docx_parser, "CONFIG", replace(docx_parser.CONFIG, docx_max_pages=10))
    assert source_spec() != first
    assert source_spec()["docx"]["max_pages"] == 10


def test_source_spec_identifies_structure_planner_without_character_limits():
    assert source_spec()["storage"] == "canonical-selectors-v2"
    assert "packing" not in source_spec()


def test_environment_changes_alone_do_not_claim_loaded_parser_configuration_changed(monkeypatch):
    before = source_spec()
    monkeypatch.setenv("DOCREADER_PDF_SCAN_MIN_CHARS", "1234")
    monkeypatch.setenv("DOCREADER_XLSX_MAX_CELLS", "1234")
    monkeypatch.setenv("DOCREADER_PDF_RENDER_DPI", "1234")
    monkeypatch.setenv("DOCREADER_DOCX_MAX_PAGES", "1234")
    assert source_spec() == before
    # A restarted worker imports new effective constants, represented here by a patch.
    monkeypatch.setattr(pdf_parser, "SCAN_MIN_CHARS_PER_PAGE", 1234)
    assert source_spec() != before


def test_secret_and_unrelated_process_settings_do_not_enter_the_source_identity(monkeypatch):
    before = source_spec()
    monkeypatch.setenv("KNOWLEDGEBRAIN_CHAT_API_KEY", "synthetic-chat-secret")
    monkeypatch.setenv("DOCREADER_PDF_FORCE_SCANNED", "true")
    monkeypatch.setattr(pdf_parser, "FORCE_SCANNED_PDF", True)
    changed = replace(pdf_parser.CONFIG, external_http_proxy="http://user:proxy-secret@proxy.invalid",
                      external_https_proxy="http://user:proxy-secret@proxy.invalid", grpc_port=1234,
                      image_output_dir="/private/not-an-input", odl_hybrid_url="https://secret-endpoint.invalid",
                      odl_max_workers=99, markitdown_max_workers=99, pdf_render_max_workers=99,
                      pdf_render_parallelism=99)
    monkeypatch.setattr(pdf_parser, "CONFIG", changed)
    monkeypatch.setattr(docx_parser, "CONFIG", changed)
    after = source_spec()
    assert after == before
    encoded = json.dumps(after)
    for forbidden in ("proxy-secret", "synthetic-chat-secret", "secret-endpoint", "private/not-an-input", "api_key", "proxy"):
        assert forbidden not in encoded


def test_nonfinite_effective_threshold_cannot_be_persisted_as_canonical_json(monkeypatch):
    monkeypatch.setattr(pdf_parser, "SCAN_IMAGE_AREA_RATIO", float("nan"))
    with pytest.raises(ValueError):
        source_spec()
