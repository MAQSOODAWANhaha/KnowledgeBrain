"""Source-disjoint body ranges and source-backed supporting context."""
from __future__ import annotations

import hashlib
from collections import defaultdict

from docreader.tender.evidence.context import condition_context, pending_scope_metadata, section_ancestors


def _id(document_id: str, document_version: str, unit_id: str, suffix: str) -> str:
    identity = f"{document_id}\0{document_version}\0{unit_id}\0{suffix}"
    return "range-" + hashlib.sha256(identity.encode()).hexdigest()[:24]


def _row_text(cells: list[dict], column_count: int) -> str:
    values = {cell["column"]: cell["text"] for cell in cells}
    return " | ".join(values.get(column, "") for column in range(column_count))


def _empty_row(cells: list[dict], merged_cover: bool, image_unit_ids=()) -> bool:
    return not merged_cover and not image_unit_ids and not any(
        cell["text"].strip() or cell.get("formula") or cell.get("row_span", 1) > 1
        or cell.get("col_span", 1) > 1 for cell in cells)


def source_ranges(document: dict, raw: dict) -> list[dict]:
    """Rows cover table grids once; duplicated XLSX table/row carriers stay raw."""
    identities = {entry["unit_id"]: entry for entry in raw["source_contract"].get("units", [])}
    ranges = []
    spreadsheet_tables = defaultdict(list)
    tables_by_row = defaultdict(list)
    table_rows = {}
    merged_rows = {}
    row_images = defaultdict(list)
    table_images = defaultdict(list)
    for unit in raw["units"]:
        parent = unit["locator"].get("compound_parent") or {}
        if unit["kind"] == "image_region" and parent.get("parent_kind") == "table_cell":
            table_images[(parent["section_ordinal"], parent["table_ordinal"], parent["row_ordinal"])].append(unit["unit_id"])
    for unit_id, anchor in raw.get("image_anchors", {}).items():
        if "from" in anchor:
            start = anchor["from"]["row"]
            end = anchor.get("to", anchor["from"])["row"]
            for row in range(start, end + 1):
                row_images[(anchor["sheet_ordinal"], row)].append(unit_id)
    grid_values = {(unit["unit_id"], cell["row"], cell["column"]): cell["text"]
                   for unit in raw["units"] if unit.get("grid") for cell in unit["grid"]["cells"]}

    def located_cells(locator):
        if "cell_refs" not in locator:
            return locator.get("cells", [])
        result = []
        for key in locator["cell_refs"]:
            cell = raw["spreadsheet_cells"][key]
            ref = cell.get("grid_cell_ref")
            text = grid_values[(ref["unit_id"], ref["row"], ref["column"])] if ref and "text" in cell.get("grid_value_fields", []) else cell.get("text", "")
            result.append({**cell, "text": text})
        return result
    for unit in raw["units"]:
        if unit.get("grid"):
            rows = defaultdict(list)
            covered = set()
            for cell in unit["grid"]["cells"]:
                rows[cell["row"]].append(cell)
                if cell["row_span"] > 1:
                    covered.update(range(cell["row"] + 1, cell["row"] + cell["row_span"]))
            table_rows[unit["unit_id"]], merged_rows[unit["unit_id"]] = rows, covered
        loc = unit["locator"]
        if loc["locator_kind"] == "spreadsheet":
            sheet = loc["sheet_ordinal"]
            if unit.get("grid"):
                spreadsheet_tables[sheet].append(unit)
                for row in range(loc["region"]["start_row"], loc["region"]["end_row"] + 1):
                    tables_by_row[(sheet, row)].append(unit)

    def sheet_cells(row, tables, native_cells=()):
        # Formula/display metadata lives in spreadsheet_cells, while the grid
        # owns text and merge spans. Overlay both without discarding either.
        cells = {cell["column"] - 1: {
            "row": row, "column": cell["column"] - 1, "text": cell.get("text", ""),
            "row_span": 1, "col_span": 1,
            **{key: cell[key] for key in ("formula", "formula_references", "display_complete",
                                         "display_incomplete_reason") if cell.get(key) is not None},
        } for cell in native_cells}
        width = max(cells, default=0) + 1
        for table in tables:
            region = table["locator"]["region"]
            width = max(width, region["end_column"])
            for cell in table_rows[table["unit_id"]][row - region["start_row"]]:
                column = cell["column"] + region["start_column"] - 1
                cells[column] = {**cells.get(column, {}), **cell, "row": row, "column": column}
        return list(cells.values()), width

    def append(unit, text, suffix="text", **extra):
        identity = identities.get(unit["unit_id"], {})
        record = {
                "range_id": _id(document["document_id"], document["document_version"], unit["unit_id"],
                                f"{document['original_ref']['sha256']}:{suffix}:0:{len(text)}"),
                "document_id": document["document_id"], "document_version": document["document_version"],
                "unit_id": unit["unit_id"], "kind": unit["kind"], "text": text,
                "start": 0, "end": len(text), "locator": unit["locator"],
                "physical_locator": identity.get("physical_locator") or unit.get("physical_locator"),
                "physical_path": unit.get("physical_path"),
                "section_id": identity.get("section_id"), "heading_path": identity.get("heading_path", ""),
                "parent_section_id": identity.get("parent_section_id"),
                "text_boundaries": {"start": True, "end": True},
                **extra,
            }
        record["source_range_id"] = record["range_id"]
        paths = (unit.get("physical_path") or "").split(";")
        paragraphs = text.split("\n")
        if (unit["kind"] == "section" and paths
                and all(path.startswith("/word/document.xml/body/*[") for path in paths)
                and len(paths) == len(paragraphs)):
            offset, breaks = 0, []
            for paragraph in paragraphs[:-1]:
                offset += len(paragraph)
                breaks.append(offset)
                offset += 1
            record["paragraph_breaks"] = breaks
        if "cells" in record:
            offset, intervals = 0, []
            by_column = {cell["column"]: cell for cell in record["cells"]}
            width = max(by_column, default=-1) + 1
            for column in range(width):
                value = by_column.get(column, {}).get("text", "")
                if column in by_column:
                    intervals.append({**by_column[column], "start": offset, "end": offset + len(value)})
                offset += len(value) + 3
            record["cell_intervals"] = intervals
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
                cells, width = sheet_cells(row, tables, located_cells(loc))
                text = _row_text(cells, width)
                merged_cover = any(row - item["locator"]["region"]["start_row"] in merged_rows[item["unit_id"]]
                                   for item in tables)
                image_unit_ids = row_images[(sheet, row)]
                empty = _empty_row(cells, merged_cover, image_unit_ids)
                table = tables[0] if tables else None
                header_rows = {cell["row"] for cell in table.get("header_cells", [])} if table else set()
                header_tables = [item["unit_id"] for item in tables
                                 if row - item["locator"]["region"]["start_row"] in
                                 {cell["row"] for cell in item.get("header_cells", [])}]
                leading_tables = [item["unit_id"] for item in tables if row == item["locator"]["region"]["start_row"]]
                append(unit, "" if empty else text, text_basis="spreadsheet_row", cells=cells,
                       row_start=row, row_end=row + 1, empty=empty, column_count=width,
                       **({"image_unit_ids": image_unit_ids} if image_unit_ids else {}),
                       table_id=table["unit_id"] if table else f"sheet:{sheet}",
                       table_ids=[item["unit_id"] for item in tables],
                       header_table_ids=header_tables, leading_table_ids=leading_tables,
                       header=bool(table and row - table["locator"]["region"]["start_row"] in header_rows),
                       leading_row=bool(table and row == table["locator"]["region"]["start_row"]),
                       non_requirement_reason="empty_table_row" if empty else None)
                continue
        if grid:
            header_rows = {cell["row"] for cell in unit.get("header_cells", [])}
            by_row = table_rows[unit["unit_id"]]
            for row in range(grid["row_count"]):
                cells = by_row[row]
                # Both filled and blank merged fields cover their original rows.
                merged_cover = row in merged_rows[unit["unit_id"]]
                image_unit_ids = table_images[(loc.get("section_ordinal"), loc.get("table_ordinal"), row)]
                empty = _empty_row(cells, merged_cover, image_unit_ids)
                append(unit, "" if empty else _row_text(cells, grid["column_count"]), suffix=f"row:{row}",
                       text_basis="grid_row", row_start=row, row_end=row + 1, cells=cells,
                       column_count=grid["column_count"],
                       **({"image_unit_ids": image_unit_ids} if image_unit_ids else {}),
                       table_id=unit["unit_id"], header=row in header_rows, leading_row=row == 0,
                       empty=empty, non_requirement_reason="empty_table_row" if empty else None)
        elif unit["kind"] == "image_region":
            ocr = raw.get("ocr", {}).get(unit["unit_id"], {})
            disposition = ocr.get("disposition")
            append(unit, ocr.get("text", ""), text_basis="literal_ocr", image_ref=raw["images"].get(unit["locator"]["image_key"]),
                   ocr_status=ocr.get("status", "unresolved"), image_anchor=raw.get("image_anchors", {}).get(unit["unit_id"]),
                   **({"empty": True, "non_requirement_reason": "reviewed_decorative_image", "image_disposition": disposition}
                      if disposition else {}))
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
                row_tables = tables_by_row[(sheet, row)]
                cells, width = sheet_cells(row, row_tables)
                merged_cover = any(row - item["locator"]["region"]["start_row"] in merged_rows[item["unit_id"]]
                                   for item in row_tables)
                image_unit_ids = row_images[(sheet, row)]
                empty = _empty_row(cells, merged_cover, image_unit_ids)
                append(table, "" if empty else _row_text(cells, width), suffix=f"row:{row}",
                       text_basis="grid_row", row_start=row, row_end=row + 1, empty=empty,
                       cells=cells, column_count=width, table_id=table["unit_id"],
                       table_ids=[item["unit_id"] for item in row_tables],
                       **({"image_unit_ids": image_unit_ids} if image_unit_ids else {}),
                       non_requirement_reason="empty_table_row" if empty else None)
    for page in raw["source_contract"].get("page_manifest", []):
        if page["classification"] == "blank" and not page["unit_ids"]:
            page_number = page["page_ordinal"]
            append({"unit_id": f"blank:page:{page_number}", "kind": "blank_page",
                    "locator": {"locator_kind": "page", "page_ordinal": page_number}}, "",
                   empty=True, non_requirement_reason="blank_page")
    return ranges


class SupportingIndex:
    """Index structural dependencies once; a candidate only visits its own ranges."""
    def __init__(self, ranges: list[dict], scope_decisions=()):
        self.introductions, self.conditions, self.issues, self.sections = condition_context(ranges, scope_decisions)
        self.position = {entry["range_id"]: index for index, entry in enumerate(ranges)}
        self.pending_by_parent = defaultdict(list)
        self.reviewed_targets = defaultdict(list)
        for conditions in self.conditions.values():
            for _, candidate in conditions:
                if candidate["scope_binding"].get("kind") == "reviewed_targets":
                    for target in candidate["scope_binding"]["reviewed_target_range_ids"]:
                        self.reviewed_targets[target].append(candidate)
                elif not candidate["scope_binding"]["complete_sentence"]:
                    self.pending_by_parent[candidate["parent_range_id"]].append(candidate)
        self.headers, self.merged = defaultdict(list), defaultdict(list)
        for entry in ranges:
            tables = set(entry.get("header_table_ids", []) + entry.get("leading_table_ids", []))
            if entry.get("table_id") and (entry.get("header") or entry.get("leading_row")):
                tables.add(entry["table_id"])
            for table in tables:
                self.headers[table].append(entry)
            for cell in entry.get("cell_intervals", []):
                if cell.get("row_span", 1) > 1:
                    self.merged[entry.get("table_id")].append((entry, cell))
        self._ancestors = {entry["range_id"]: section_ancestors(entry, self.sections) for entry in ranges}

    @staticmethod
    def _uncovered(candidate: dict, occupied: list[dict]) -> list[dict]:
        from docreader.tender.evidence.references import fragment
        source = candidate.get("source_range_id", candidate.get("parent_range_id", candidate["range_id"]))
        if candidate["start"] == candidate["end"]:
            return [] if any(entry["source_range_id"] == source for entry in occupied) else [candidate]
        cursor = candidate["start"]
        missing = []
        for entry in sorted((entry for entry in occupied if entry["source_range_id"] == source), key=lambda entry: entry["start"]):
            if entry["start"] >= candidate["end"]:
                break
            if entry["start"] > cursor:
                missing.append((cursor, entry["start"]))
            cursor = max(cursor, entry["end"])
        if cursor < candidate["end"]:
            missing.append((cursor, candidate["end"]))
        return [candidate if (start, end) == (candidate["start"], candidate["end"]) else fragment(candidate, start, end)
                for start, end in missing]

    def select(self, body: list[dict]) -> tuple[list, list, list, list]:
        context, pending, problems = {}, {}, {}

        def bind(candidate, entry):
            incomplete = not candidate["scope_binding"]["complete_sentence"] and candidate["scope_binding"].get("kind") != "reviewed_targets"
            target = pending if incomplete else context
            key = candidate["range_id"]
            if key not in target:
                value = pending_scope_metadata(candidate) if incomplete else {**candidate, "reason": "condition"}
                target[key] = {**value, "scope_binding": {**candidate["scope_binding"], "applies_to_range_ids": []}}
            targets = target[key]["scope_binding"]["applies_to_range_ids"]
            if entry["range_id"] not in targets:
                targets.append(entry["range_id"])

        for entry in body:
            original = entry["source_range_id"]
            for problem in self.issues[original]:
                problems[problem["range_id"]] = problem
            for candidate in self.pending_by_parent[original]:
                bind(candidate, entry)
            for candidate in self.reviewed_targets[original]:
                bind(candidate, entry)
            for section_id in self._ancestors[original]:
                introduction = self.introductions.get(section_id)
                if introduction and self.position[introduction["parent_range_id"]] <= self.position[original]:
                    context.setdefault(introduction["range_id"], {**introduction, "reason": "introduction"})
                for position, candidate in self.conditions[section_id]:
                    if candidate["scope_binding"].get("kind") == "reviewed_targets":
                        continue  # Explicit decisions bind only their canonical target IDs.
                    if position > self.position[original]:
                        break
                    if position == self.position[original] and candidate["end"] > entry["start"]:
                        continue
                    table_scope = candidate["scope_binding"]["table_ids"]
                    if table_scope and not set(table_scope).intersection(entry.get("table_ids") or [entry.get("table_id")]):
                        continue
                    bind(candidate, entry)
            for table_id in entry.get("table_ids") or [entry.get("table_id")]:
                for candidate in self.headers[table_id]:
                    context.setdefault(candidate["range_id"], {**candidate, "reason": "header"})
                for candidate, cell in self.merged[table_id]:
                    row = entry.get("row_start", -1)
                    if candidate["row_start"] < row < candidate["row_start"] + cell["row_span"]:
                        context.setdefault(candidate["range_id"], {**candidate, "reason": "merged_row"})
        # The union of body and support still supplies every required source
        # character, once per request. Keep bound conditions before generic
        # introductions/headers so their target metadata remains available.
        selected = []
        for value in sorted(context.values(), key=lambda item: item.get("reason") != "condition"):
            selected.extend(self._uncovered(value, [*body, *selected]))
        bindings = [{key: value[key] for key in ("range_id", "source_range_id", "start", "end", "scope_binding")}
                    for value in context.values() if value.get("reason") == "condition"]
        return selected, list(pending.values()), list(problems.values()), bindings
