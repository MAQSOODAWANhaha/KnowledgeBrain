"""Small model-facing projections over unchanged, replayable frozen evidence."""

from copy import deepcopy

from docreader.tender.schema import SourceView


def _fields(value: dict, names) -> dict:
    return {key: deepcopy(value[key]) for key in names
            if key in value and value[key] is not None and value[key] != ""
            and value[key] != [] and value[key] != {}}


def host_reviewed_ranges(pack: dict) -> set[str]:
    """Only independently established empty pages and reviewed decorations."""
    result = set()
    for source in [*pack["body"], *pack["supporting"]]:
        if source.get("empty") is not True or source["text"].strip():
            continue
        reason = source.get("non_requirement_reason")
        review = source.get("image_disposition", {})
        if reason == "blank_page" or (
            reason == "reviewed_decorative_image" and review.get("status") == "reviewed"
            and review.get("classification") == "decorative"
        ):
            result.add(source["range_id"])
    return result


def supplied_evidence_texts(views: list[dict], sources: dict) -> dict[str, list[str]]:
    """Visible quote candidates; callers also check the original source interval."""
    sent = {source["range_id"]:source for view in views
            for key in ("body", "supporting") for source in view[key]}
    texts = {identity:([source["text"]] if source.get("text") is not None else [])
             + [cell["text"] for cell in source.get("cells", [])]
             for identity,source in sent.items()}
    for view in views:
        for alias in view.get("source_aliases", []):
            original = sources.get(alias["range_id"])
            if original is None:
                continue
            visible, parts = [], []
            for segment in alias["segments"]:
                source = sent.get(segment["range_id"], {})
                if source.get("text") is None:
                    visible.extend(cell["text"] for cell in source.get("cells", []))
                    continue
                start = segment["start"] - source.get("start", 0)
                end = segment["end"] - source.get("start", 0)
                if 0 <= start <= end <= len(source["text"]):
                    part = source["text"][start:end]
                    visible.append(part)
                    parts.append(part)
            # A paragraph quote may span contiguous supplied fragments. Cell
            # strings are never joined into a row that the model did not see.
            if len(parts) == len(alias["segments"]) and "".join(parts) == original["text"]:
                visible.append("".join(parts))
            texts[alias["range_id"]] = visible
    return texts


def resolve_evidence_quote(range_id: str, quote: str, sources: dict, sent_texts: dict) -> dict | None:
    """Locate a visible quote, recovering only deleted source U+000A characters.

    Offsets are zero-based Unicode positions within the cited range's original
    text. Exact quotes retain the existing protocol, including repeated text;
    a recovered quote must have one unambiguous original interval. Inputs are
    unchanged, and callers keep the raw response separately for audit.
    """
    source = sources.get(range_id)
    visible = sent_texts.get(range_id, [])
    if source is None or not quote.strip():
        return None
    text = source["text"]
    start = text.find(quote)
    if start >= 0 and any(quote in candidate for candidate in visible):
        return {"quote": quote, "start": start, "end": start + len(quote)}

    # Keep original positions while locating the unchanged non-LF characters.
    # Existing quote LFs must still occur at their original character boundary;
    # extra source LFs may be skipped, never any other character.
    positions = [index for index, character in enumerate(text) if character != "\n"]
    flattened = "".join(text[index] for index in positions)
    needle = quote.replace("\n", "")
    if not needle:
        return None
    leading = len(quote) - len(quote.lstrip("\n"))
    trailing = len(quote) - len(quote.rstrip("\n"))
    matches = set()
    offset = flattened.find(needle)
    while offset >= 0:
        start = positions[offset] - leading
        end = positions[offset + len(needle) - 1] + 1 + trailing
        if 0 <= start <= end <= len(text):
            original = text[start:end]
            cursor = 0
            for character in original:
                if cursor < len(quote) and character == quote[cursor]:
                    cursor += 1
                elif character != "\n":
                    break
            else:
                if cursor == len(quote) and any(original in candidate for candidate in visible):
                    matches.add((start, end))
                    if len(matches) > 1:
                        return None
        offset = flattened.find(needle, offset + 1)
    if not matches:
        return None
    start, end = matches.pop()
    return {"quote": text[start:end], "start": start, "end": end}


def _scope(binding: dict) -> dict:
    result = _fields(binding, (
        "section_id", "kind", "relationship", "status", "table_ids", "complete_sentence",
        "decision_range_id",
    ))
    for key in ("applies_to_range_ids", "reviewed_target_range_ids"):
        if key in binding:
            result[key] = list(binding[key])
    return result


def _cells(source: dict) -> list:
    cells = source.get("cell_intervals", source.get("cells", []))
    result = []
    offset = 0
    lower = source.get("start", 0)
    upper = lower + len(source["text"])
    for index, cell in enumerate(cells):
        # Native sources use dictionaries. Keep narrow hand-built callers usable
        # without adding their repeated row text to the sending view.
        if isinstance(cell, str):
            cell = {"column": index, "text": cell}
        start = cell.get("start", offset)
        end = cell.get("end", start + len(cell.get("text", "")))
        offset = end + 3  # Native row separators are " | ".
        if not ((start < upper and lower < end) or (start == end and lower <= start <= upper)):
            continue
        start, end = max(lower, start), min(upper, end)
        value = _fields(cell, ("column", "formula", "formula_references", "display_complete",
                               "display_incomplete_reason", "image_anchor"))
        value["text"] = source["text"][start - lower:end - lower]
        for key in ("row_span", "col_span"):
            if cell.get(key, 1) != 1:
                value[key] = cell[key]
        if "row" in cell and cell["row"] != source.get("row_start"):
            value["row"] = cell["row"]
        result.append(value)
    return result


def _template_row(source: dict) -> bool:
    if not (source.get("empty") is True and source.get("non_requirement_reason") == "empty_table_row"
            and not source["text"].strip() and not source.get("image_unit_ids") and source.get("table_id")
            and "row_start" in source and "row_end" in source):
        return False
    # An incomplete or older caller cannot turn a formula/image/merged value into
    # a blank template merely by labelling it empty.
    return not any(isinstance(cell, dict) and (
        cell.get("text", "").strip() or cell.get("formula") or cell.get("image_anchor")
        or cell.get("row_span", 1) > 1
    ) for cell in source.get("cell_intervals", source.get("cells", [])))


def _templates(rows: list[dict]) -> list[dict]:
    tables = {}
    for source in rows:
        table_id = source["table_id"]
        cells = _cells(source)
        width = source.get("column_count") or max(
            (cell["column"] + cell.get("col_span", 1) for cell in cells), default=0)
        table = tables.setdefault(table_id, {
            "table_id": table_id, "column_count": width, "rows": [],
            **_fields(source, ("section_id",)),
        })
        table["column_count"] = max(table["column_count"], width)
        if source.get("table_ids") and source["table_ids"] != [table_id]:
            table["table_ids"] = list(source["table_ids"])
        intervals = table["rows"]
        if intervals and intervals[-1]["row_end"] == source["row_start"]:
            intervals[-1]["row_end"] = source["row_end"]
            intervals[-1]["range_ids"].append(source["range_id"])
        else:
            intervals.append({"row_start": source["row_start"], "row_end": source["row_end"],
                              "range_ids": [source["range_id"]]})
        for cell in cells:
            if cell.get("row_span", 1) > 1 or cell.get("col_span", 1) > 1:
                table.setdefault("merges", []).append({
                    "row": cell.get("row", source["row_start"]), "column": cell["column"],
                    "row_span": cell.get("row_span", 1), "col_span": cell.get("col_span", 1),
                })
    return list(tables.values())


def _range(source: dict, interval_sources: set[str], image_units: set[str]) -> dict:
    result = {"range_id": source["range_id"], "text": source["text"], **_fields(source, (
        "kind", "section_id", "table_id", "row_start", "row_end", "column_count", "ocr_status",
        "reason", "image_unit_ids",
    ))}
    for key in ("header", "leading_row"):
        if source.get(key):
            result[key] = True
    if source.get("unit_id") in image_units:
        result["unit_id"] = source["unit_id"]
    if source.get("kind") == "image_region":
        result.update(_fields(source, ("image_anchor",)))
        result.update(_fields(source.get("locator", {}), ("compound_parent",)))
    source_id = source.get("source_range_id", source["range_id"])
    if source_id != source["range_id"] or source_id in interval_sources or source.get("start", 0):
        result.update(source_range_id=source_id, start=source.get("start", 0),
                      end=source.get("end", len(source["text"])))
    if source.get("table_ids") and source["table_ids"] != [source.get("table_id")]:
        result["table_ids"] = list(source["table_ids"])
    if source.get("cells") or source.get("cell_intervals"):
        cells = _cells(source)
        if cells:
            result["cells"] = cells
            # Only native row separators may disappear. Any uncovered prose
            # keeps the original row text alongside its visible cells.
            if " | ".join(cell["text"] for cell in cells) == source["text"]:
                result.pop("text")
    if source.get("text_boundaries") and not all(source["text_boundaries"].values()):
        result["text_boundaries"] = deepcopy(source["text_boundaries"])
    if source.get("scope_binding"):
        result["scope_binding"] = _scope(source["scope_binding"])
    return result


def source_view(pack: dict) -> dict:
    """One source contract for both extraction and independent verification.

    The planner already makes body/support character intervals disjoint. The view
    retains that union and explicit bindings, while dropping storage/debug data.
    """
    host = host_reviewed_ranges(pack)
    templates = [source for source in pack["body"] if _template_row(source)]
    template_ids = {source["range_id"] for source in templates}
    bindings = pack.get("scope_bindings", [])
    interval_sources = {binding["source_range_id"] for binding in bindings}
    body = [source for source in pack["body"] if source["range_id"] not in host | template_ids]
    body_ids = {source["range_id"] for source in pack["body"]}
    supporting = [source for source in pack["supporting"]
                  if source["range_id"] not in host | body_ids]
    # Replanning can put a previously cited excerpt beside its full body range.
    # Keep the cited identity as a text-free alias of the exact supplied union.
    from docreader.tender.evidence.packing import SupportingIndex

    selected, aliases = [], []
    for source in supporting:
        if "source_range_id" not in source or any("source_range_id" not in item for item in body):
            selected.append(source)
            continue
        pieces = SupportingIndex._uncovered(source, [*body, *selected])
        selected.extend(pieces)
        if len(pieces) == 1 and pieces[0]["range_id"] == source["range_id"]:
            continue
        segments = []
        for item in sorted([*body, *selected], key=lambda item: item.get("start", 0)):
            if item.get("source_range_id") != source["source_range_id"]:
                continue
            start, end = max(item["start"], source["start"]), min(item["end"], source["end"])
            if start < end or (source["start"] == source["end"] and start == end):
                segments.append({"range_id": item["range_id"], "start": start, "end": end})
        if segments:
            aliases.append({"range_id": source["range_id"], "segments": segments})
    supporting = selected
    pending = pack.get("pending_scope_context", [])
    sections = {}
    for source in [*pack["body"], *pack["supporting"], *pending,
                   *(value["scope_binding"] for value in bindings)]:
        if source.get("section_id") and source.get("heading_path"):
            sections.setdefault(source["section_id"], _fields(
                source, ("section_id", "parent_section_id", "heading_path")))
    image_units = {unit_id for source in [*body, *supporting] for unit_id in source.get("image_unit_ids", [])}
    result = {"pack_id": pack["pack_id"], "body": [_range(source, interval_sources, image_units) for source in body],
              "supporting": [_range(source, interval_sources, image_units) for source in supporting],
              "pending_scope_context": [], "scope_bindings": []}
    identity = pack if pack.get("document_id") else next(iter(pack["body"] or pack["supporting"]), {})
    result.update(_fields(identity, ("document_id", "document_version", "role")))
    if sections:
        result["sections"] = list(sections.values())
    if templates:
        result["blank_table_templates"] = _templates(templates)
    if aliases:
        result["source_aliases"] = aliases
    for source in pending:
        result["pending_scope_context"].append({
            **_fields(source, ("range_id", "source_range_id", "parent_range_id", "start", "end", "reason", "section_id")),
            "scope_binding": _scope(source["scope_binding"]),
        })
    for source in bindings:
        result["scope_bindings"].append({
            **_fields(source, ("range_id", "source_range_id", "start", "end")),
            "scope_binding": _scope(source["scope_binding"]),
        })
    return SourceView.model_validate(result).model_dump(exclude_unset=True)
