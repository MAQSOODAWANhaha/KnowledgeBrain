"""Source-disjoint body ranges and source-backed supporting context."""
from __future__ import annotations

import hashlib
import re
from bisect import bisect_left
from collections import defaultdict

_CONDITION = re.compile(r"适用|适用于|除非|仅限|如果|如属|条件|联合体|except|unless|applicab|provided that", re.I)


def _id(document_id: str, version: str, unit_id: str, suffix: str) -> str:
    identity = f"{document_id}\0{version}\0{unit_id}\0{suffix}"
    return "range-" + hashlib.sha256(identity.encode()).hexdigest()[:24]


def _pieces(text: str, maximum: int):
    start = 0
    while start < len(text):
        end = min(len(text), start + maximum)
        if end < len(text):
            boundary = text.rfind("\n", start + maximum // 2, end)
            if boundary > start:
                end = boundary + 1
        yield start, end, text[start:end]
        start = end
    if not text:
        yield 0, 0, ""


def _row_text(cells: list[dict], column_count: int) -> str:
    values = {cell["column"]: cell["text"] for cell in cells}
    return " | ".join(values.get(column, "") for column in range(column_count))


def source_ranges(document: dict, raw: dict, maximum: int = 6000) -> list[dict]:
    """Rows cover table grids once; duplicated XLSX table/row carriers stay raw."""
    identities = {entry["unit_id"]: entry for entry in raw["source_contract"].get("units", [])}
    ranges = []
    spreadsheet_tables = defaultdict(list)
    tables_by_row = defaultdict(list)
    table_rows = {}
    merged_rows = {}
    for unit in raw["units"]:
        if unit.get("grid"):
            rows = defaultdict(list)
            covered = set()
            for cell in unit["grid"]["cells"]:
                rows[cell["row"]].append(cell)
                if cell["text"].strip():
                    covered.update(range(cell["row"] + 1, cell["row"] + cell["row_span"]))
            table_rows[unit["key"]], merged_rows[unit["key"]] = rows, covered
        loc = unit["locator"]
        if loc["locator_kind"] == "spreadsheet":
            sheet = loc["sheet_ordinal"]
            if unit.get("grid"):
                spreadsheet_tables[sheet].append(unit)
                for row in range(loc["region"]["start_row"], loc["region"]["end_row"] + 1):
                    tables_by_row[(sheet, row)].append(unit)

    def append(unit, text, suffix="text", **extra):
        identity = identities.get(unit["key"], {})
        for start, end, part in _pieces(text, maximum):
            record = {
                "range_id": _id(document["document_id"], document["version"], unit["key"],
                                f"{document['original_ref']['sha256']}:{suffix}:{start}:{end}"),
                "document_id": document["document_id"], "document_version": document["version"],
                "unit_id": unit["key"], "kind": unit["kind"], "text": part,
                "start": start, "end": end, "locator": unit["locator"],
                "physical_locator": identity.get("physical_locator") or unit.get("physical_locator"),
                "physical_path": unit.get("physical_path"),
                "section_id": identity.get("section_id"), "heading_path": identity.get("heading_path", ""),
                **extra,
            }
            if len(text) > maximum:
                record.pop("cells", None)  # full cells remain in raw; fragments retain row/column identity.
            ranges.append(record)

    # Preserve the full rectangular row width (including partially blank fields).
    emitted_sheet_rows = set()
    for unit in raw["units"]:
        loc, grid = unit["locator"], unit.get("grid")
        if loc["locator_kind"] == "spreadsheet":
            if unit["kind"] == "section":
                append(unit, unit["text"])
                continue
            if grid:
                continue  # emitted through row units below, including explicit blank rows
            if unit["kind"] == "table_row":
                sheet, row = loc["sheet_ordinal"], loc["region"]["start_row"]
                if (sheet, row) in emitted_sheet_rows:
                    continue
                emitted_sheet_rows.add((sheet, row))
                # A worksheet can have several tables side-by-side. Use all row cells
                # plus each table's full extent, not only the first matching table.
                tables = tables_by_row[(sheet, row)]
                raw_cells = {cell["column"] - 1: {"column": cell["column"] - 1, "text": cell["text"],
                                                "row_span": 1, "col_span": 1}
                             for cell in loc.get("cells", [])}
                width = max(raw_cells, default=0) + 1
                for table in tables:
                    region = table["locator"]["region"]
                    width = max(width, region["end_column"])
                    for cell in table_rows[table["key"]][row - region["start_row"]]:
                        column = cell["column"] + region["start_column"] - 1
                        raw_cells[column] = {**cell, "column": column}
                cells = list(raw_cells.values())
                text = _row_text(cells, width)
                empty = not any(cell["text"].strip() or cell.get("row_span", 1) > 1 for cell in cells)
                table = tables[0] if tables else None
                header_rows = {cell["row"] for cell in table.get("header_cells", [])} if table else set()
                header_tables = [item["key"] for item in tables
                                 if row - item["locator"]["region"]["start_row"] in
                                 {cell["row"] for cell in item.get("header_cells", [])}]
                leading_tables = [item["key"] for item in tables if row == item["locator"]["region"]["start_row"]]
                append(unit, "" if empty else text, text_basis="spreadsheet_row", cells=cells,
                       row_start=row, row_end=row + 1, empty=empty,
                       table_id=table["key"] if table else f"sheet:{sheet}",
                       table_ids=[item["key"] for item in tables],
                       header_table_ids=header_tables, leading_table_ids=leading_tables,
                       header=bool(table and row - table["locator"]["region"]["start_row"] in header_rows),
                       leading_row=bool(table and row == table["locator"]["region"]["start_row"]),
                       non_requirement_reason="empty_table_row" if empty else None)
                continue
        if grid:
            header_rows = {cell["row"] for cell in unit.get("header_cells", [])}
            by_row = table_rows[unit["key"]]
            for row in range(grid["row_count"]):
                cells = by_row[row]
                # A vertically merged nonempty cell semantically covers each spanned row.
                merged_cover = row in merged_rows[unit["key"]]
                empty = not merged_cover and not any(cell["text"].strip() for cell in cells)
                append(unit, "" if empty else _row_text(cells, grid["column_count"]), suffix=f"row:{row}",
                       text_basis="grid_row", row_start=row, row_end=row + 1, cells=cells,
                       table_id=unit["key"], header=row in header_rows, leading_row=row == 0,
                       empty=empty, non_requirement_reason="empty_table_row" if empty else None)
        elif unit["kind"] == "image_region":
            ocr = raw.get("ocr", {}).get(unit["key"], {})
            append(unit, ocr.get("text", ""), text_basis="literal_ocr", image_ref=raw["images"].get(unit["locator"]["original_ref"]),
                   ocr_status=ocr.get("status", "unresolved"), image_anchor=raw.get("image_anchors", {}).get(unit["key"]))
        else:
            append(unit, unit.get("text", ""))

    # Empty worksheet grid rows are not emitted by the original row parser.
    # Keep their identities explicitly while omitting their whitespace payload.
    for sheet, tables in spreadsheet_tables.items():
        for table in tables:
            region = table["locator"]["region"]
            for row in range(region["start_row"], region["end_row"] + 1):
                if (sheet, row) in emitted_sheet_rows:
                    continue
                emitted_sheet_rows.add((sheet, row))
                cells = table_rows[table["key"]][row - region["start_row"]]
                merged_cover = row - region["start_row"] in merged_rows[table["key"]]
                empty = not merged_cover and not any(cell["text"].strip() for cell in cells)
                append(table, "" if empty else _row_text(cells, table["grid"]["column_count"]), suffix=f"row:{row}",
                       text_basis="grid_row", row_start=row, row_end=row + 1, empty=empty,
                       cells=cells, table_id=table["key"], non_requirement_reason="empty_table_row" if empty else None)
    for page in raw["source_contract"].get("page_manifest", []):
        if page["classification"] == "blank" and not page["unit_ids"]:
            page_number = page["page_ordinal"]
            append({"key": f"blank:page:{page_number}", "kind": "blank_page",
                    "locator": {"locator_kind": "page", "page_ordinal": page_number}}, "",
                   empty=True, non_requirement_reason="blank_page")
    return ranges


def make_packs(document: dict, ranges: list[dict], maximum: int = 12000) -> list[dict]:
    """Merge small sections and split large ones without duplicating body coverage."""
    introductions, headers = {}, defaultdict(list)
    conditions, condition_positions = defaultdict(list), defaultdict(list)
    position = {entry["range_id"]: index for index, entry in enumerate(ranges)}
    for entry in ranges:
        path = entry.get("heading_path", "")
        if entry["kind"] == "section" and entry["text"]:
            introductions.setdefault(path, entry)
        if _CONDITION.search(entry["text"]):
            conditions[path].append(entry)
            condition_positions[path].append(position[entry["range_id"]])
        table_headers = set(entry.get("header_table_ids", []) + entry.get("leading_table_ids", []))
        if entry.get("table_id") and (entry.get("header") or entry.get("leading_row")):
            table_headers.add(entry["table_id"])
        for table_id in table_headers:
            headers[table_id].append(entry)
    groups, current, size = [], [], 0
    for entry in ranges:
        cost = len(entry["text"]) + 200
        if current and size + cost > maximum:
            groups.append(current)
            current, size = [], 0
        current.append(entry)
        size += cost
    if current:
        groups.append(current)
    packs = []
    for ordinal, body in enumerate(groups):
        body_ids = {entry["range_id"] for entry in body}
        context = {}
        for entry in body:
            heading = entry.get("heading_path", "")
            paths = [heading]
            while " > " in heading:
                heading = heading.rsplit(" > ", 1)[0]
                paths.append(heading)
            for path in paths:
                introduction = introductions.get(path)
                if introduction and position[introduction["range_id"]] < position[entry["range_id"]]:
                    context[introduction["range_id"]] = {**introduction, "reason": "introduction"}
                condition_index = bisect_left(condition_positions[path], position[entry["range_id"]]) - 1
                if condition_index >= 0:
                    candidate = conditions[path][condition_index]
                    context[candidate["range_id"]] = {**candidate, "reason": "condition"}
            for table_id in entry.get("table_ids") or [entry.get("table_id")]:
                for candidate in headers.get(table_id, []):
                    context[candidate["range_id"]] = {**candidate, "reason": "header"}
        context = {key: value for key, value in context.items() if key not in body_ids}
        pack_id = "pack-" + hashlib.sha256(
            (f"{document['document_id']}\0{document['version']}\0{ordinal}\0" +
             ",".join(entry["range_id"] for entry in body)).encode()).hexdigest()[:24]
        issues = []
        if sum(len(item["text"]) + 200 for item in body + list(context.values())) > maximum * 2:
            issues.append({"code": "supporting_context_exceeds_pack_budget", "blocking": True,
                           "document_id": document["document_id"], "pack_id": pack_id,
                           "detail": "Required source context retained; reduce body grouping or review source scope."})
        packs.append({"pack_id": pack_id, "document_id": document["document_id"],
                      "document_version": document["version"], "body": body,
                      "supporting": list(context.values()), "issues": issues,
                      "compacted_blank_ranges": [entry["range_id"] for entry in body if entry.get("empty")]})
    return packs
