"""Non-secret effective settings for one immutable native-source freeze.

The existing parsers load environment settings into module constants and CONFIG
at import time. Read those exact values, not a fresh environment interpretation:
only a restarted worker (or a changed effective constant) uses changed settings.
No proxy, paths, model credentials, gRPC or alternate-engine configuration enters
this snapshot. Worker counts affect scheduling, not the native source identity.
"""

import json

from docreader.parser import docx_parser, excel_parser, pdf_parser, pdf_tables


# Explicit allowlists; never enumerate a module or serialize the entire CONFIG.
_PDF_PARAMETERS = (
    "SCAN_IMAGE_AREA_RATIO", "SCAN_MIN_CHARS_PER_PAGE", "_LOW_TEXT_IMAGE_RATIO",
    "EXTRACT_EMBEDDED_IMAGES", "EMBED_MIN_PIXELS", "EMBED_MIN_AREA_RATIO", "EMBED_REPEAT_PAGE_FRAC",
    "EMBED_MAX_IMAGES", "LAYOUT_ORDERING", "WORD_GAP_WIDTH_RATIO", "DETECT_HEADINGS", "FILTER_HIDDEN_TEXT",
    "MARGIN_COL_WIDTH_RATIO", "MIN_HEADING_LINE_CHARS", "SANITIZE_PDF_TEXT", "STRIP_CHART_TEXT_DEBRIS",
    "RENDER_VECTOR_FIGURES", "MIN_CHART_REGION_CHARS", "MIN_CHART_REGION_AREA_RATIO",
    "MAX_CHART_REGION_AREA_RATIO", "MAX_FIGURE_HEIGHT_RATIO",
)
_XLSX_LIMITS = (
    "XLSX_MAX_MATERIALIZED_CELLS", "XLSX_MAX_LOGICAL_ROW", "XLSX_MAX_LOGICAL_COLUMN",
    "XLSX_MAX_CELLS_PER_ROW", "XLSX_MAX_MERGES", "XLSX_MAX_TABLES", "XLSX_MAX_RANGE_CELLS",
    "XLSX_MAX_UNITS", "XLSX_MAX_TEXT_BYTES", "XLSX_MAX_CELL_PAYLOAD_BYTES", "XLSX_MAX_TABLE_CELL_SCANS",
    "XLSX_MAX_ARCHIVE_ENTRIES", "XLSX_MAX_UNCOMPRESSED_BYTES", "XLSX_MAX_XML_PART_BYTES",
    "XLSX_MAX_COMPRESSION_RATIO",
)


def source_spec() -> dict:
    """Return fresh JSON primitives describing the effective native path.

    PDF thresholds, layout/image controls, render settings and table dimensions;
    XLSX structure/archive budgets and DOCX page coverage determine native data.
    The request budget is pinned separately by TenderNodes using the same model
    configuration and request builder as the actual model client.
    The new adapter always disables force-scanned mode and full-page fallback.
    """
    pdf = {name.lower().lstrip("_"): getattr(pdf_parser, name) for name in _PDF_PARAMETERS}
    pdf.update(
        render_dpi=max(1, pdf_parser.CONFIG.pdf_render_dpi),
        jpeg_quality=pdf_parser._normalize_image_quality(pdf_parser.CONFIG.pdf_jpeg_quality),
        render_max_edge=max(0, pdf_parser.CONFIG.pdf_render_max_edge),
        table_max_columns=pdf_tables.MAX_PDF_TABLE_COLUMNS,
        table_max_rows=pdf_tables.MAX_PDF_TABLE_ROWS,
        force_scanned=False,
        full_page_fallback=False,
    )
    page_limit = docx_parser.CONFIG.docx_max_pages
    result = {
        "pdf": pdf,
        "xlsx": {name.removeprefix("XLSX_").lower(): getattr(excel_parser, name) for name in _XLSX_LIMITS},
        "docx": {"max_pages": page_limit if page_limit > 0 else 100000},
        "storage": "canonical-selectors-v2",
    }
    # The legacy float reader accepts NaN/Infinity, which cannot be a canonical
    # PostgreSQL JSON configuration or an unambiguous classification threshold.
    json.dumps(result, allow_nan=False)
    return result
