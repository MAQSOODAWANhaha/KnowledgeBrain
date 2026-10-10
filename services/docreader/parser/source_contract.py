"""V2 source identity and exact UTF-8 rendering map, emitted with source text.

Mappings are recorded during rendering, never guessed by global text search.
The native grid is authoritative; Markdown is only its display representation.
"""
import hashlib
import json
import struct
from pathlib import Path

from docreader.models.document import (
    DocumentLocator, ImageLocator, SpreadsheetLocator, StructuredSourceUnitKind,
)

SCHEMA_VERSION = 2


def parser_version():
    digest = hashlib.sha256()
    for name in ("source_contract.py", "ooxml_text.py", "docx_parser.py", "pdf_parser.py",
                 "pdf_tables.py", "pdf_symbols.py", "structure.py", "excel_parser.py", "output_inventory.py"):
        digest.update(Path(__file__).with_name(name).read_bytes())
    return f"docreader-source-v2/{digest.hexdigest()}"


def grid_digest(grid):
    if grid is None:
        return None
    digest = hashlib.sha256(b"docreader-grid-v2\0")
    digest.update(struct.pack(">III", grid.row_count, grid.column_count, len(grid.cells)))
    for cell in sorted(grid.cells, key=lambda cell: (cell.row, cell.column)):
        text = cell.text.encode("utf-8")
        digest.update(struct.pack(">IIIIQ", cell.row, cell.column, cell.row_span, cell.col_span, len(text)))
        digest.update(text)
    widths = grid.widths_mm or []
    digest.update(struct.pack(">I", len(widths)))
    for width in widths:
        digest.update(struct.pack(">d", width))
    return digest.hexdigest()


def _grid_markdown(grid):
    cells = {(cell.row, cell.column): cell.text for cell in grid.cells}
    def escaped(text):
        return text.replace("&", "&amp;").replace("<", "&lt;").replace(
            ">", "&gt;").replace("|", "&#124;").replace("\t", "&#9;").replace("\n", "<br>")
    rows = []
    for row in range(grid.row_count):
        rows.append("| " + " | ".join(escaped(cells.get((row, column), ""))
                                     for column in range(grid.column_count)) + " |")
        if row == 0:
            rows.append("| " + " | ".join("---" for _ in range(grid.column_count)) + " |")
    return "\n".join(rows)


def render_units(units, media):
    parts, spans, offset = [], {}, 0
    emitted_headings = set()
    # XLSX rows already include every value. Tables map to those same row
    # displays, avoiding a second contradictory rendering of the same cells.
    for unit in units:
        if media in {"xlsx", "xls"}:
            rendered = unit.text if unit.kind == StructuredSourceUnitKind.TABLE_ROW else ""
        elif unit.grid is not None:
            rendered = _grid_markdown(unit.grid)
        elif isinstance(unit.locator, ImageLocator):
            ref = unit.locator.original_ref
            rendered = f"![{Path(ref).name}]({ref})"
        else:
            rendered = unit.text
            locator = unit.locator
            if media in {"docx", "doc"} and isinstance(locator, DocumentLocator):
                headings = locator.heading_path.split(" > ") if locator.heading_path else []
                first, separator, rest = rendered.partition("\n")
                if headings and first == headings[-1] and locator.heading_path not in emitted_headings:
                    rendered = "#" * min(6, len(headings)) + " " + first + separator + rest
                    emitted_headings.add(locator.heading_path)
        spans[unit.key] = []
        if not rendered:
            continue
        if parts:
            parts.append("\n\n")
            offset += 2
        start = offset
        parts.append(rendered)
        offset += len(rendered.encode("utf-8"))
        spans[unit.key].append({"start_byte": start, "end_byte": offset})
    if media in {"xlsx", "xls"}:
        for unit in units:
            if unit.grid is None or not isinstance(unit.locator, SpreadsheetLocator):
                continue
            target = unit.locator
            for row in units:
                source = row.locator
                if (row.kind == StructuredSourceUnitKind.TABLE_ROW
                        and isinstance(source, SpreadsheetLocator)
                        and source.sheet_ordinal == target.sheet_ordinal
                        and target.region.start_row <= source.region.start_row <= target.region.end_row):
                    spans[unit.key].extend(spans[row.key])
    return "".join(parts), spans


def attach_source_contract(document, source, media, *, render=True):
    media = {"xlsm": "xlsx"}.get(media, media)
    units = document.structured_source_units
    if not units and "page_count" not in document.metadata:
        return document
    if render:
        required_images = {unit.locator.original_ref for unit in units if isinstance(unit.locator, ImageLocator)}
        # Package thumbnails and unused relationship images are not document
        # occurrences. Only explicit carriers become OCR/persistence obligations.
        document.images = {ref: payload for ref, payload in document.images.items() if ref in required_images}
        document.content, spans = render_units(units, media)
    else:
        # A non-native parser must supply its own mapping. Never invent exact
        # matches by searching duplicate strings in opaque rendered output.
        spans = {unit.key: [] for unit in units}
    revision = hashlib.sha256(source).hexdigest()
    version = parser_version()
    identities = []
    section_paths = {}
    current_section = None
    section_owners = {}
    for ordinal, unit in enumerate(units):
        if unit.ordinal != ordinal:
            raise ValueError("source contract requires contiguous parser ordinals")
        locator = unit.locator
        heading_path = ""
        section = None
        if isinstance(locator, DocumentLocator):
            section = unit.source_section_id or section_owners.get(locator.section_ordinal, f"section:{locator.section_ordinal}")
            section_owners[locator.section_ordinal] = section
            heading_path = locator.heading_path
        elif isinstance(locator, SpreadsheetLocator):
            section = f"sheet:{locator.sheet_ordinal}"
            heading_path = locator.sheet_name
        elif isinstance(locator, ImageLocator) and locator.compound_parent is not None:
            section = section_owners.get(locator.compound_parent.section_ordinal, f"section:{locator.compound_parent.section_ordinal}")
            heading_path = section_paths.get(section, "")
        if section is not None:
            current_section = section
            section_paths[section] = heading_path
        else:
            section = current_section
            heading_path = section_paths.get(section, "")
        physical = unit.physical_locator or locator
        reasons = list(unit.source_issues)
        if not render:
            reasons.append("render_mapping_unavailable")
        if unit.kind == StructuredSourceUnitKind.ATTACHMENT_REGION:
            reasons.append("attachment_content_not_extracted")
        if isinstance(locator, ImageLocator):
            if locator.original_ref not in document.images:
                reasons.append("image_payload_missing")
        identity = {
            "unit_id": unit.key, "ordinal": ordinal,
            "kind": unit.kind.value, "text_sha256": hashlib.sha256(unit.text.encode("utf-8")).hexdigest(),
            "grid_sha256": grid_digest(unit.grid),
            "section_id": section, "parent_section_id": None,
            "heading_level": len(heading_path.split(" > ")) if heading_path else None,
            "heading_path": heading_path,
            "physical_locator": physical.model_dump(mode="json"),
            "physical_path": unit.physical_path,
            "physical_locator_unavailable_reason": None,
            "rendered_spans": spans[unit.key],
            "completeness": "partial" if reasons else "complete", "reasons": reasons,
            "table_id": unit.key if unit.grid is not None else None,
            "header_cells": unit.header_cells,
        }
        identities.append(identity)
    if len({unit.key for unit in units}) != len(units):
        raise ValueError("source contract requires unique unit identities")
    # Parent identity comes from the nearest earlier matching heading path.
    by_path = {}
    for unit in identities:
        path = unit["heading_path"]
        if path:
            parent_path = path.rsplit(" > ", 1)[0] if " > " in path else ""
            unit["parent_section_id"] = by_path.get(parent_path)
            by_path[path] = unit["section_id"]
    pages = []
    classifications = document.metadata.get("page_classifications", [])
    for page in range(document.metadata.get("page_count", 0)):
        owners = [unit for unit in identities
                  if (unit["physical_locator"] or {}).get("page_ordinal") == page]
        pages.append({
            "page_ordinal": page,
            "classification": classifications[page] if page < len(classifications) else "scanned",
            "unit_ids": [unit["unit_id"] for unit in owners],
            "image_unit_ids": [unit["unit_id"] for unit in owners
                               if unit["physical_locator"]["locator_kind"] == "image"],
        })
    contract = {
        "schema_version": SCHEMA_VERSION, "document_revision": revision,
        "parser_version": version,
        "markdown_sha256": hashlib.sha256(document.content.encode("utf-8")).hexdigest(),
        "units": identities, "page_manifest": pages,
        "glyph_normalizations": document.metadata.get("glyph_normalizations", []),
    }
    document.metadata["source_contract"] = json.dumps(contract, ensure_ascii=False, separators=(",", ":"))
    from docreader.parser.document_tree import build_document_tree
    document.tree = build_document_tree(units, document.content)
    document.metadata["document_tree"] = document.tree.model_dump_json()
    return document
