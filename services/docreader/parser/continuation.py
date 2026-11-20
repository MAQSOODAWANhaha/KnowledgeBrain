"""Annotations that join adjacent pages without rewriting page-local text.

A continuation records that two ruled tables, or two prose lines, read as one
item. The grids and the line strings stay as they were extracted.
"""
from __future__ import annotations

import re

COLUMN_TOLERANCE = 4.0
EDGE_FRACTION = 0.18
_SENTENCE_END = ("。", "！", "？", "；", ".", "!", "?")
_NEW_ITEM = re.compile(r"^(?:\d+[.、．]|[（(]\d+[）)]|[一二三四五六七八九十]+、|#)")


def ruled_table_continuations(pages):
    """Link a bottom ruled table to the top ruled table on the next page.

    The page dicts and their table dicts are not modified. A glyph-alignment
    table, a split row, body text in the gap, a scanned page, or a missing
    adjacent page produces no link.
    """
    by_page = {page["page"]: page for page in pages}
    links = []
    for page in pages:
        following = by_page.get(page["page"] + 1)
        if following is None or page.get("scanned") or following.get("scanned"):
            continue
        before = _edge_table(page, "bottom")
        after = _edge_table(following, "top")
        if before is None or after is None:
            continue
        if not before.get("ruled") or not after.get("ruled"):
            continue
        if before.get("split_row") or after.get("split_row"):
            continue
        if before.get("column_count") != after.get("column_count"):
            continue
        if not _edges_close(before.get("column_edges") or [], after.get("column_edges") or []):
            continue
        if _body_past(page, before, "below") or _body_past(following, after, "above"):
            continue
        links.append({
            "kind": "ruled_table",
            "before_page": page["page"],
            "before_table_id": before["table_id"],
            "after_page": following["page"],
            "after_table_id": after["table_id"],
            "repeated_header_rows": _repeated_header_rows(before, after),
        })
    return links


def _repeated_header_rows(before, after):
    """Leading rows of the next grid that repeat the previous grid's header text."""
    before_rows = [row for row in before.get("rows") or [] if any(str(cell).strip() for cell in row)]
    after_rows = list(after.get("rows") or [])
    if len(after_rows) < 2 or not before_rows:
        return 0
    count = 0
    for earlier, later in zip(before_rows, after_rows):
        if [str(cell).strip() for cell in earlier] != [str(cell).strip() for cell in later]:
            break
        if not any(str(cell).strip() for cell in later):
            break
        count += 1
        if count >= len(after_rows) - 1:
            break
    return count


def paragraph_continuations(pages):
    """Note a prose line that continues on the next page. Do not join the strings."""
    by_page = {page["page"]: page for page in pages}
    links = []
    for page in pages:
        following = by_page.get(page["page"] + 1)
        if following is None or page.get("scanned") or following.get("scanned"):
            continue
        before = _content_line(page.get("lines") or [], last=True)
        after = _content_line(following.get("lines") or [], last=False)
        if before is None or after is None:
            continue
        if _ends_sentence(before["text"]) or _NEW_ITEM.match(after["text"].strip()):
            continue
        links.append({
            "kind": "paragraph",
            "before_page": page["page"],
            "before_line": before["index"],
            "after_page": following["page"],
            "after_line": after["index"],
        })
    return links


def mark_layout_decoration(pages):
    """Flag repeated running edges. Every original line stays in place."""
    split = [page.splitlines() for page in pages]
    text_indexes = [index for index, lines in enumerate(split) if any(line.strip() for line in lines)]
    repeating = set()
    if len(text_indexes) >= 4:
        from collections import Counter

        counter = Counter()
        for index in text_indexes:
            lines = [line.strip() for line in split[index] if line.strip()]
            if not lines:
                continue
            for edge in {lines[0], lines[-1]}:
                if len(edge) <= 80:
                    counter[edge] += 1
        threshold = max(2, int(len(text_indexes) * 0.6))
        repeating = {line for line, count in counter.items() if count >= threshold}
    return [[bool(line.strip()) and line.strip() in repeating for line in lines] for lines in split]


def _edge_table(page, edge):
    """The ruled table actually on this edge, including one whose row is cut."""
    tables = [table for table in page.get("tables") or [] if table.get("ruled")]
    if not tables:
        return None
    height = page.get("height") or 0
    margin = height * EDGE_FRACTION if height else 72
    if edge == "bottom":
        candidate = min(tables, key=lambda table: table["bottom"])
        if candidate["bottom"] > margin:
            return None
        return candidate
    candidate = max(tables, key=lambda table: table["top"])
    if height and candidate["top"] < height - margin:
        return None
    return candidate


def _edges_close(left, right):
    if len(left) < 2 or len(left) != len(right):
        return False
    return all(abs(a - b) <= COLUMN_TOLERANCE for a, b in zip(left, right))


def _body_past(page, table, side):
    for span in page.get("body_spans") or []:
        if side == "below" and span.get("top", 0) < table["bottom"] - 1:
            return True
        if side == "above" and span.get("bottom", 0) > table["top"] + 1:
            return True
    return False


def _content_line(lines, *, last):
    indexed = [(index, line) for index, line in enumerate(lines)
               if not line.get("decoration") and str(line.get("text") or "").strip()]
    if not indexed:
        return None
    index, line = indexed[-1] if last else indexed[0]
    return {"index": index, "text": line["text"]}


def _ends_sentence(text):
    stripped = text.strip()
    return not stripped or stripped.endswith(_SENTENCE_END)
