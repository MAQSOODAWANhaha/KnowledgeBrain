"""Native-text PDF table detection from glyph boxes (optional ruling lines).

Tables are emitted as rectangular grids of cells. Glyphs never become a single
blob cell covering the whole page. Scanned pages are out of scope.
"""

from __future__ import annotations

import statistics
from dataclasses import dataclass, field

MAX_PDF_TABLE_COLUMNS = 16
MAX_PDF_TABLE_ROWS = 80


class TableExtractionLimitError(ValueError):
    code = "OUTLINE_TABLE_EXTRACTION_UNSUPPORTED"


@dataclass(frozen=True)
class PdfTableCell:
    row: int
    column: int
    row_span: int
    col_span: int
    text: str


@dataclass
class PdfTableGrid:
    left: float
    top: float
    right: float
    bottom: float
    row_count: int
    column_count: int
    cells: list[PdfTableCell] = field(default_factory=list)
    merged_ranges: list[tuple[int, int, int, int]] = field(default_factory=list)
    column_edges: list[float] = field(default_factory=list)

    def contains_point(self, x: float, y: float) -> bool:
        return self.left <= x <= self.right and self.bottom <= y <= self.top


def extract_tables_from_chars(
    chars: list[dict],
    *,
    min_columns: int = 3,
    min_rows: int = 3,
) -> list[PdfTableGrid]:
    """Detect tables from glyph positions. ``chars`` use pdfium boxes (y up)."""
    if len(chars) < min_columns * min_rows:
        return []
    rows = _group_rows(chars)
    if len(rows) < min_rows:
        return []
    bands = _tabular_bands(rows, min_columns=min_columns, min_rows=min_rows)
    tables: list[PdfTableGrid] = []
    for band in bands:
        try:
            grid = _grid_from_band(band, min_columns=min_columns)
        except TableExtractionLimitError:
            # Oversize/illegal geometry skips this table only.
            continue
        if grid is not None:
            tables.append(grid)
    return tables


def chars_outside_tables(chars: list[dict], tables: list[PdfTableGrid]) -> list[dict]:
    if not tables:
        return chars
    kept = []
    for glyph in chars:
        x = (glyph["x0"] + glyph["x1"]) / 2
        y = (glyph["y0"] + glyph["y1"]) / 2
        if any(table.contains_point(x, y) for table in tables):
            continue
        kept.append(glyph)
    return kept


def reading_order_text(glyphs: list[dict]) -> str:
    """Reconstruct visual reading order from glyph boxes (y up, x across).

    Tiny punctuation uses the line's median height so leftover clauses keep
    。 and ， next to the surrounding words instead of on a later line.
    """
    if not glyphs:
        return ""
    heights = [glyph["y1"] - glyph["y0"] for glyph in glyphs if glyph["y1"] > glyph["y0"]]
    med_h = statistics.median(heights) if heights else 10.0
    ordered = sorted(glyphs, key=lambda glyph: (-(glyph["y0"] + glyph["y1"]) / 2, glyph["x0"]))
    lines: list[list[dict]] = []
    for glyph in ordered:
        mid = (glyph["y0"] + glyph["y1"]) / 2
        if lines:
            ref = sum((item["y0"] + item["y1"]) / 2 for item in lines[-1]) / len(lines[-1])
            if abs(mid - ref) <= med_h * 0.6:
                lines[-1].append(glyph)
                continue
        lines.append([glyph])
    return "\n".join(
        "".join(item["ch"] for item in sorted(line, key=lambda glyph: glyph["x0"]))
        for line in lines
    )


def table_region_reading_text(chars: list[dict], tables: list[PdfTableGrid]) -> str:
    """Visual reading order of glyphs inside table bboxes.

    Independent of locator.cells: column fragments stay in the grid, while
    4-gram reconstruction can use leftover + this string.
    """
    if not tables:
        return ""
    inside = []
    for glyph in chars:
        x = (glyph["x0"] + glyph["x1"]) / 2
        y = (glyph["y0"] + glyph["y1"]) / 2
        if any(table.contains_point(x, y) for table in tables):
            inside.append(glyph)
    return reading_order_text(inside)


def grid_markdown(grid: PdfTableGrid) -> str:
    """Serialize column fragments as a GFM table. Covered merge cells stay empty."""
    by_pos = {(cell.row, cell.column): cell for cell in grid.cells}
    rows: list[str] = []
    for row in range(grid.row_count):
        values = []
        for column in range(grid.column_count):
            cell = by_pos.get((row, column))
            text = (cell.text if cell else "").replace("\n", " ").replace("|", "\\|")
            values.append(text.strip())
        rows.append("| " + " | ".join(values) + " |")
    if not rows:
        return ""
    separator = "| " + " | ".join("---" for _ in range(grid.column_count)) + " |"
    if len(rows) == 1:
        return "\n".join((rows[0], separator))
    return "\n".join((rows[0], separator, *rows[1:]))


def collect_rule_lines(page, raw) -> tuple[list[tuple[float, float, float]], list[tuple[float, float, float]]]:
    """Return (horizontal, vertical) segments as (a, pos, b) in page space."""
    horizontal: list[tuple[float, float, float]] = []
    vertical: list[tuple[float, float, float]] = []
    try:
        for obj in page.get_objects():
            if obj.type != raw.FPDF_PAGEOBJ_PATH:
                continue
            try:
                left, bottom, right, top = obj.get_bounds()
            except Exception:
                continue
            x0, x1 = (left, right) if left <= right else (right, left)
            y0, y1 = (bottom, top) if bottom <= top else (top, bottom)
            width = x1 - x0
            height = y1 - y0
            # Office exporters often draw each cell border separately. Short
            # vertical segments in blank forms are still real ruling lines.
            if width >= 4 and height <= 2.8 and width >= height * 4:
                horizontal.append((x0, (y0 + y1) / 2, x1))
            elif height >= 4 and width <= 2.8 and height >= width * 4:
                vertical.append(((x0 + x1) / 2, y0, y1))
    except Exception:
        return [], []
    return horizontal, vertical


def _grid_widths_ok(grid: PdfTableGrid) -> bool:
    try:
        widths_mm(grid)
    except TableExtractionLimitError:
        return False
    return True


def _ruled_regions(
    horizontal: list[tuple[float, float, float]],
    vertical: list[tuple[float, float, float]],
    *,
    tol: float = 6.0,
) -> list[tuple[list[tuple[float, float, float]], list[tuple[float, float, float]]]]:
    """Split ruling lines into independently extractable table regions."""
    if len(horizontal) < 3 or len(vertical) < 3:
        return []
    parent = list(range(len(horizontal) + len(vertical)))

    def find(index: int) -> int:
        while parent[index] != index:
            parent[index] = parent[parent[index]]
            index = parent[index]
        return index

    def union(left: int, right: int) -> None:
        left_root = find(left)
        right_root = find(right)
        if left_root != right_root:
            parent[right_root] = left_root

    n_h = len(horizontal)
    for hi, (x0, y, x1) in enumerate(horizontal):
        for vi, (x, y0, y1) in enumerate(vertical):
            if x0 - tol <= x <= x1 + tol and y0 - tol <= y <= y1 + tol:
                union(hi, n_h + vi)
    groups: dict[int, dict[str, list]] = {}
    for index in range(len(parent)):
        root = find(index)
        bucket = groups.setdefault(root, {"h": [], "v": []})
        if index < n_h:
            bucket["h"].append(horizontal[index])
        else:
            bucket["v"].append(vertical[index - n_h])
    regions = []
    for bucket in groups.values():
        if len(bucket["h"]) >= 3 and len(bucket["v"]) >= 3:
            regions.append((bucket["h"], bucket["v"]))
    return regions


def _grid_usable(grid: PdfTableGrid) -> bool:
    # Long wrapped cells (tech specs, notes) are valid. Only illegal widths
    # skip the table; page leftover stays in the parser text layer.
    return _grid_widths_ok(grid)


def _usable_ruled_tables(
    horizontal: list[tuple[float, float, float]],
    vertical: list[tuple[float, float, float]],
    chars: list[dict],
) -> list[PdfTableGrid]:
    try:
        candidates = _tables_from_rules(horizontal, vertical, chars)
    except TableExtractionLimitError:
        return []
    return [grid for grid in candidates if _grid_usable(grid)]


def extract_tables_from_page(page, raw, chars: list[dict]) -> list[PdfTableGrid]:
    # Explicit PDF ruling lines carry the objective row/column geometry. Prefer
    # them over glyph-start alignment, which fragments wrapped text into fake
    # columns on dense scoring tables. Limit errors skip that table only.
    # Independent ruled regions keep titles/clauses as leftover instead of one
    # page-sized grid.
    horizontal, vertical = collect_rule_lines(page, raw)
    regions = _ruled_regions(horizontal, vertical)
    if regions:
        region_tables: list[PdfTableGrid] = []
        for hs, vs in regions:
            region_tables.extend(_usable_ruled_tables(hs, vs, chars))
        if region_tables:
            return region_tables
    tables = _usable_ruled_tables(horizontal, vertical, chars)
    if tables:
        return tables
    try:
        aligned = extract_tables_from_chars(chars)
    except TableExtractionLimitError:
        return []
    return [grid for grid in aligned if _grid_usable(grid)]


def widths_mm(grid: PdfTableGrid, printable_mm: float = 180.0) -> list[float]:
    """Scale objective PDF column edges without inventing fallback geometry."""
    edges = grid.column_edges
    if len(edges) != grid.column_count + 1 or any(
        right <= left for left, right in zip(edges, edges[1:])
    ):
        raise TableExtractionLimitError("OUTLINE_TABLE_EXTRACTION_UNSUPPORTED")
    page_width = edges[-1] - edges[0]
    physical_width = page_width * 25.4 / 72.0
    scale = min(printable_mm, physical_width) / page_width
    widths = [
        (edges[index + 1] - edges[index]) * scale
        for index in range(grid.column_count)
    ]
    rounded = [round(value, 2) for value in widths]
    if any(value <= 0 for value in rounded) or sum(rounded) > printable_mm + 0.01:
        raise TableExtractionLimitError("OUTLINE_TABLE_EXTRACTION_UNSUPPORTED")
    return rounded


def _group_rows(chars: list[dict]) -> list[list[dict]]:
    ordered = sorted(chars, key=lambda glyph: (-(glyph["y0"] + glyph["y1"]) / 2, glyph["x0"]))
    # Leader dots and punctuation can outnumber letters on a TOC page. Their
    # tiny ink boxes must not become the line-height reference.
    heights = [glyph["y1"] - glyph["y0"] for glyph in chars
               if glyph["ch"].isalnum() and glyph["y1"] > glyph["y0"]]
    typical_height = statistics.median(heights) if heights else 6.0
    rows: list[list[dict]] = []
    for glyph in ordered:
        mid = (glyph["y0"] + glyph["y1"]) / 2
        if rows:
            row_mid = statistics.mean((item["y0"] + item["y1"]) / 2 for item in rows[-1])
            if abs(mid - row_mid) <= typical_height * 0.6:
                rows[-1].append(glyph)
                continue
        rows.append([glyph])
    for row in rows:
        row.sort(key=lambda glyph: glyph["x0"])
    return rows


def _word_clusters(row: list[dict]) -> list[dict]:
    if not row:
        return []
    widths = [max(glyph["x1"] - glyph["x0"], 0.5) for glyph in row]
    widths.sort()
    typical = widths[len(widths) // 2]
    gap = max(typical * 1.7, 5.5)
    clusters = [[row[0]]]
    for glyph in row[1:]:
        if glyph["x0"] - clusters[-1][-1]["x1"] > gap:
            clusters.append([glyph])
        else:
            clusters[-1].append(glyph)
    words = []
    for cluster in clusters:
        text = "".join(item["ch"] for item in cluster).strip()
        if not text:
            continue
        words.append(
            {
                "x0": cluster[0]["x0"],
                "x1": cluster[-1]["x1"],
                "y0": min(item["y0"] for item in cluster),
                "y1": max(item["y1"] for item in cluster),
                "text": text,
            }
        )
    return words


def _cluster_positions(values: list[float], gap: float) -> list[float]:
    if not values:
        return []
    ordered = sorted(values)
    groups = [[ordered[0]]]
    for value in ordered[1:]:
        if value - groups[-1][-1] <= gap:
            groups[-1].append(value)
        else:
            groups.append([value])
    return [sum(group) / len(group) for group in groups]


def _tabular_bands(
    rows: list[list[dict]],
    *,
    min_columns: int,
    min_rows: int,
) -> list[list[list[dict]]]:
    flags = []
    for row in rows:
        words = _word_clusters(row)
        flags.append(len(words) >= 2)
    bands: list[list[list[dict]]] = []
    index = 0
    while index < len(rows):
        if not flags[index]:
            index += 1
            continue
        start = index
        while index < len(rows) and flags[index]:
            index += 1
        if index - start >= min_rows:
            band = rows[start:index]
            words_per_row = [_word_clusters(row) for row in band]
            max_words = max((len(words) for words in words_per_row), default=0)
            if max_words >= min_columns:
                bands.append(band)
    return bands


def _grid_from_band(band: list[list[dict]], *, min_columns: int) -> PdfTableGrid | None:
    word_rows = [_word_clusters(row) for row in band]
    starts = [word["x0"] for words in word_rows for word in words]
    if not starts:
        return None
    typical = 10.0
    widths = [
        max(word["x1"] - word["x0"], 1.0) for words in word_rows for word in words
    ]
    if widths:
        widths.sort()
        typical = widths[len(widths) // 2]
    col_starts = _cluster_positions(starts, max(typical * 1.4, 8.0))
    if len(col_starts) < min_columns:
        return None
    if len(col_starts) > MAX_PDF_TABLE_COLUMNS:
        raise TableExtractionLimitError("OUTLINE_TABLE_EXTRACTION_UNSUPPORTED")
    rights = [word["x1"] for words in word_rows for word in words]
    column_edges = col_starts + [max(rights) + 2.0]
    # Snap trailing edge so last column has width.
    for index in range(len(column_edges) - 1):
        if column_edges[index + 1] <= column_edges[index] + 4:
            column_edges[index + 1] = column_edges[index] + 12
    column_count = len(col_starts)
    row_count = len(band)
    occupied: dict[tuple[int, int], str] = {}
    for row_index, words in enumerate(word_rows):
        for word in words:
            mid_x = (word["x0"] + word["x1"]) / 2
            column = _assign_column(mid_x, col_starts, column_edges)
            key = (row_index, column)
            if key in occupied:
                occupied[key] = f"{occupied[key]} {word['text']}".strip()
            else:
                occupied[key] = word["text"]
    filled = len(occupied)
    needed = min_columns * 2
    density = (row_count * column_count) // 4
    if density > needed:
        needed = density
    if filled < needed:
        return None
    cells = [
        PdfTableCell(
            row=row,
            column=column,
            row_span=1,
            col_span=1,
            text=occupied.get((row, column), ""),
        )
        for row in range(row_count)
        for column in range(column_count)
    ]
    ys = []
    for row in band:
        ys.append(max(glyph["y1"] for glyph in row))
        ys.append(min(glyph["y0"] for glyph in row))
    return PdfTableGrid(
        left=min(column_edges),
        top=max(ys),
        right=max(column_edges),
        bottom=min(ys),
        row_count=row_count,
        column_count=column_count,
        cells=cells,
        column_edges=column_edges,
    )


def _assign_column(x0: float, col_starts: list[float], column_edges: list[float] | None = None) -> int:
    if column_edges and len(column_edges) == len(col_starts) + 1:
        for index in range(len(col_starts)):
            if column_edges[index] <= x0 < column_edges[index + 1]:
                return index
        if x0 >= column_edges[-1]:
            return len(col_starts) - 1
        return 0
    best = 0
    best_delta = abs(x0 - col_starts[0])
    for index, start in enumerate(col_starts[1:], start=1):
        delta = abs(x0 - start)
        if delta < best_delta:
            best = index
            best_delta = delta
    return best


def _merged_ranges_from_rules(
    xs: list[float],
    y_visual: list[float],
    horizontal: list[tuple[float, float, float]],
    vertical: list[tuple[float, float, float]],
) -> list[tuple[int, int, int, int]]:
    """Derive rectangular merged cells from objectively missing rule segments."""
    row_count = len(y_visual) - 1
    column_count = len(xs) - 1
    parent = {(row, column): (row, column) for row in range(row_count) for column in range(column_count)}

    def root(cell: tuple[int, int]) -> tuple[int, int]:
        while parent[cell] != cell:
            parent[cell] = parent[parent[cell]]
            cell = parent[cell]
        return cell

    def union(left: tuple[int, int], right: tuple[int, int]) -> None:
        left_root = root(left)
        right_root = root(right)
        if left_root != right_root:
            parent[right_root] = left_root

    tolerance = 3.5
    for row in range(row_count):
        top = y_visual[row]
        bottom = y_visual[row + 1]
        for column in range(column_count - 1):
            boundary = xs[column + 1]
            separated = any(
                abs(x - boundary) <= tolerance
                and y0 <= bottom + tolerance
                and y1 >= top - tolerance
                for x, y0, y1 in vertical
            )
            if not separated:
                union((row, column), (row, column + 1))
    for row in range(row_count - 1):
        boundary = y_visual[row + 1]
        for column in range(column_count):
            left = xs[column]
            right = xs[column + 1]
            separated = any(
                abs(y - boundary) <= tolerance
                and x0 <= left + tolerance
                and x1 >= right - tolerance
                for x0, y, x1 in horizontal
            )
            if not separated:
                union((row, column), (row + 1, column))

    components: dict[tuple[int, int], set[tuple[int, int]]] = {}
    for cell in parent:
        components.setdefault(root(cell), set()).add(cell)
    merged = []
    for cells in components.values():
        if len(cells) == 1:
            continue
        rows = [cell[0] for cell in cells]
        columns = [cell[1] for cell in cells]
        start_row, end_row = min(rows), max(rows)
        start_column, end_column = min(columns), max(columns)
        expected = {
            (row, column)
            for row in range(start_row, end_row + 1)
            for column in range(start_column, end_column + 1)
        }
        if cells != expected:
            # Non-rectangular unions skip this merge only; the grid stays.
            continue
        merged.append((start_row, start_column, end_row, end_column))
    return sorted(merged)


def _tables_from_rules(
    horizontal: list[tuple[float, float, float]],
    vertical: list[tuple[float, float, float]],
    chars: list[dict],
) -> list[PdfTableGrid]:
    if len(horizontal) < 3 or len(vertical) < 3 or not chars:
        return []
    # vertical stored as (x, y0, y1)
    xs = _cluster_positions([line[0] for line in vertical], 3.5)
    ys = _cluster_positions([line[1] for line in horizontal], 3.5)
    if len(xs) < 3 or len(ys) < 3:
        return []
    xs = sorted(xs)
    ys = sorted(ys, reverse=True)  # top to bottom in visual order, y still up
    # Rebuild y edges increasing for bounds, but rows top-first.
    y_visual = sorted(ys, reverse=True)
    if len(xs) - 1 < 2 or len(y_visual) - 1 < 2:
        return []
    column_count = len(xs) - 1
    row_count = len(y_visual) - 1
    if column_count > MAX_PDF_TABLE_COLUMNS or row_count > MAX_PDF_TABLE_ROWS:
        return []
    cells: list[PdfTableCell] = []
    for row in range(row_count):
        y_top = y_visual[row]
        y_bottom = y_visual[row + 1]
        for column in range(column_count):
            x0 = xs[column]
            x1 = xs[column + 1]
            text = "".join(
                glyph["ch"]
                for glyph in chars
                if x0 <= (glyph["x0"] + glyph["x1"]) / 2 <= x1
                and y_bottom <= (glyph["y0"] + glyph["y1"]) / 2 <= y_top
            ).strip()
            cells.append(
                PdfTableCell(
                    row=row,
                    column=column,
                    row_span=1,
                    col_span=1,
                    text=text,
                )
            )
    filled = sum(1 for cell in cells if cell.text)
    # A prescribed blank form can contain only its header. Requiring two
    # populated rows discards exactly the templates we need to preserve.
    if filled == 0:
        return []
    merged_ranges = _merged_ranges_from_rules(
        xs, y_visual, horizontal, vertical
    )
    spans = {
        (start_row, start_column): (
            end_row - start_row + 1,
            end_column - start_column + 1,
        )
        for start_row, start_column, end_row, end_column in merged_ranges
    }
    texts = {(cell.row, cell.column): cell.text for cell in cells}
    for start_row, start_column, end_row, end_column in merged_ranges:
        anchor = (start_row, start_column)
        # Read the merged rectangle as a whole. Concatenating virtual cells
        # column-first interleaves wrapped lines and corrupts declarations.
        texts[anchor] = "".join(
            glyph["ch"] for glyph in chars
            if xs[start_column] <= (glyph["x0"] + glyph["x1"]) / 2 <= xs[end_column + 1]
            and y_visual[end_row + 1] <= (glyph["y0"] + glyph["y1"]) / 2 <= y_visual[start_row]
        ).strip()
        for row in range(start_row, end_row + 1):
            for column in range(start_column, end_column + 1):
                if (row, column) != anchor:
                    texts[(row, column)] = ""
    cells = [
        PdfTableCell(
            row=cell.row,
            column=cell.column,
            row_span=spans.get((cell.row, cell.column), (1, 1))[0],
            col_span=spans.get((cell.row, cell.column), (1, 1))[1],
            text=texts[(cell.row, cell.column)],
        )
        for cell in cells
    ]
    return [
        PdfTableGrid(
            left=xs[0],
            top=y_visual[0],
            right=xs[-1],
            bottom=y_visual[-1],
            row_count=row_count,
            column_count=column_count,
            cells=cells,
            merged_ranges=merged_ranges,
            column_edges=xs,
        )
    ]
