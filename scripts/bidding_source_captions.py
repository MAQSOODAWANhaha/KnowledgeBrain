#!/usr/bin/env python3
"""Caption helpers for native source contracts.

Frozen input is produced only by the Rust prepare-tender-input pipeline, which
validates the current source contract, OCR receipts and complete ownership.
"""
import re


_SENTENCE_END = '。.!！?？;；'
_FRAGMENT_START = '）。)》」』、，,;；'
# Same short-label bound as outline attachment detection. A unit note is one
# colon with both sides inside it. An index stub is at most half that bound.
# Documented in docs/bidding/outline.md.
_LABEL_CHARS = 12
_NUMERALS = '一二三四五六七八九十'
_DIGITS = '0123456789０１２３４５６７８９'
_INDEX_TOKEN = re.compile(rf'^[{_DIGITS}]{{1,3}}[A-Za-zＡ-Ｚａ-ｚ]?$')
_PAGE_IN_KEY = re.compile(r'(?:^|:)page:(\d+)(?::|$)')
# A short line may carry a 1–3 digit page number, optionally wrapped as 第n页.
# Same width as an index token, so a four-digit year is not a page number.
# The parser's repeating-line set compares the whole edge string, so "标题第1页"
# and "标题第2页" never meet there. Folding the marker is what joins them.
# Documented in docs/bidding/outline.md.
_TRAILING_PAGE = re.compile(
    rf'(?:[\s\-—]*第)?[\s\-—]*[{_DIGITS}]{{1,3}}页?$')
_BRACKETS = (('(', ')'), ('（', '）'), ('[', ']'), ('【', '】'))


def _normalize(text):
    return ' '.join((text or '').split())


def page_ordinal(locator):
    if locator is None:
        return None
    if isinstance(locator, dict):
        value = locator.get('page_ordinal')
    else:
        value = getattr(locator, 'page_ordinal', None)
    return value if isinstance(value, int) else None


def unit_page(unit):
    """Page of a source unit.

    Table and image locators carry ``page_ordinal``. Section text does not; the
    parser records that page in the unit key (``page:<n>``). Either one is enough.
    """
    locator = unit.get('locator') if isinstance(unit, dict) else getattr(unit, 'locator', None)
    page = page_ordinal(locator)
    if page is not None:
        return page
    key = unit.get('key') if isinstance(unit, dict) else getattr(unit, 'key', None)
    if not isinstance(key, str):
        return None
    match = _PAGE_IN_KEY.search(key)
    return int(match.group(1)) if match else None


def plain_line(text):
    """Drop a leading heading mark. The mark is not part of the line."""
    line = (text or '').strip()
    if line.startswith('#'):
        line = line.lstrip('#').strip()
    return line


def _locator(unit):
    return unit.get('locator') if isinstance(unit, dict) else getattr(unit, 'locator', None)


def _heading_path(locator):
    if locator is None:
        return ''
    if isinstance(locator, dict):
        value = locator.get('heading_path') or ''
    else:
        value = getattr(locator, 'heading_path', '') or ''
    return value if isinstance(value, str) else ''


def _unit_text(unit):
    text = unit['text'] if isinstance(unit, dict) else getattr(unit, 'text', '')
    return text or ''


def _unit_grid(unit):
    return unit.get('grid') if isinstance(unit, dict) else getattr(unit, 'grid', None)


def _without_trailing_page(line):
    """Drop a trailing page number from a short line. A long line stays whole."""
    if len(line) > _LABEL_CHARS:
        return line
    stripped = _TRAILING_PAGE.sub('', line).strip()
    return stripped or line


def _compact_label(line):
    """Short non-index label with spaces removed. Index lines stay whole."""
    if len(line) > _LABEL_CHARS or _is_index_line(line):
        return ''
    compact = ''.join(line.split())
    return compact if compact and compact != line else ''


def _remember_line(seen, line, page):
    """Record a line and, for a short label, the same line without spaces."""
    line = _normalize(plain_line(line))
    if not line or page is None:
        return ''
    seen.setdefault(line, set()).add(page)
    compact = _compact_label(line)
    if compact:
        seen.setdefault(compact, set()).add(page)
    return line


def running_lines(units):
    """Lines that occur on at least two pages are page furniture.

    Repetition across pages is the signal, wherever the line sits on the page.
    A heading segment repeated on two pages is the same signal. The words
    themselves are not a list. A line repeated only inside one page is not
    furniture. Comparison ignores a leading heading mark. A short line and that line
    with a trailing page number (digits, or 第n页) are one line, wherever the
    line sits. A short non-index label matches with its spaces removed.
    """
    seen = {}

    def add(line, page):
        added = _remember_line(seen, line, page)
        if not added:
            return
        # A header with 第n页 is a different string on each page, so the
        # parser's 60% set misses it. Fold the marker wherever the line sits.
        folded = _without_trailing_page(added)
        if folded != added and not _is_index_line(folded):
            _remember_line(seen, folded, page)

    for unit in units:
        page = unit_page(unit)
        if page is None:
            continue
        for line in _unit_text(unit).splitlines():
            add(line, page)
        for part in _heading_path(_locator(unit)).split('>'):
            add(part, page)
    return {line for line, pages in seen.items() if len(pages) >= 2}


def _furniture(line, running):
    """True when the line, or its spaceless short form, is page furniture."""
    folded = _normalize(plain_line(line))
    if not folded or _is_index_line(folded):
        return False
    if folded in running:
        return True
    compact = _compact_label(folded)
    return bool(compact) and compact in running


def heading_banners(units):
    """Short heading segments. A section banner is not a table caption.

    The segment has to be inside the short-label bound and not an index.
    Repetition is not required: one page's own heading is enough to demote it
    when the window has another line.
    """
    banners = set()
    for unit in units:
        for part in _heading_path(_locator(unit)).split('>'):
            line = _normalize(plain_line(part))
            if line and len(line) <= _LABEL_CHARS and not _is_index_line(line):
                banners.add(line)
    return banners


def _is_banner(text, banners):
    line = _normalize(plain_line(text))
    return bool(line) and line in banners


def _is_heading_piece(text, headings):
    """Short non-index line contained in the active heading segment.

    Exact banners store the whole segment. A shorter piece of that segment is
    not in the set, even when the segment itself is inside the short-label
    bound. A full-width caption cell is not judged here.
    禁止硬编码: containment after plain_line, not a word list.
    """
    line = _normalize(plain_line(text))
    if not line or len(line) > _LABEL_CHARS or _is_index_line(line):
        return False
    for segment in headings or ():
        host = _normalize(plain_line(segment))
        if host and len(line) < len(host) and line in host:
            return True
    return False


def _outside_brackets(line):
    """Characters that are not inside a bracket pair."""
    depth = 0
    opens = {open_ for open_, _close in _BRACKETS}
    closes = {close for _open, close in _BRACKETS}
    kept = []
    for ch in line:
        if ch in opens:
            depth += 1
        elif ch in closes and depth:
            depth -= 1
        elif depth == 0:
            kept.append(ch)
    return ''.join(kept)


def is_fragment(text):
    """A line that starts mid-sentence, stops inside, or leaves a bracket open.

    A comma outside brackets is a clause break, not a caption. A comma inside
    brackets can still be a title.
    """
    line = plain_line(text)
    if not line:
        return False
    if line[0] in _FRAGMENT_START:
        return True
    if any(ch in '。！？!?' for ch in line[:-1]):
        return True
    if any(line.count(open_) != line.count(close) for open_, close in _BRACKETS):
        return True
    outside = _outside_brackets(line)
    return any(ch in outside for ch in '，、；;')


def _has_index_token(text):
    """A 1–3 digit token, optional single letter. Not a year and not a decimal."""
    line = plain_line(text)
    if line[:1] in '（([':
        line = line[1:].lstrip()
    for sep in ('：', ':'):
        line = line.replace(sep, ' ')
    for token in line.split():
        token = token.strip('、.．)）]】')
        if _INDEX_TOKEN.match(token):
            return True
    return False


def is_index_stub(text):
    """An index line no longer than half the short-label bound."""
    line = plain_line(text)
    return bool(line) and len(line) * 2 <= _LABEL_CHARS and (is_numbered(line) or _has_index_token(line))


def is_unit_line(text):
    """A parenthetical note, or one short colon annotation.

    Both sides of the colon have to be short labels. A digit makes the line an
    index, not a unit note. A numbered caption is not a unit note either.
    """
    line = plain_line(text)
    if len(line) < 3 or is_numbered(line) or any(ch in _DIGITS for ch in line):
        return False
    if (line[0], line[-1]) in {('(', ')'), ('（', '）')}:
        return True
    if any(ch in _SENTENCE_END for ch in line):
        return False
    for sep in ('：', ':'):
        if line.count(sep) != 1:
            continue
        left, right = (part.strip() for part in line.split(sep))
        if left and right and len(left) <= _LABEL_CHARS and len(right) <= _LABEL_CHARS:
            return True
    return False


def is_numbered(text):
    """A caption index: a digit, or a numeral token closed by a delimiter."""
    line = (text or '').strip()
    if line[:1] in '（([':
        line = line[1:].lstrip()
    if not line:
        return False
    if line[0] in _DIGITS:
        return True
    return line[0] in _NUMERALS and (len(line) == 1 or line[1] in '、.．)）')


def usable_line(text, running):
    line = plain_line(text)
    if not line or len(line) > 80 or line[-1] in _SENTENCE_END or is_fragment(line):
        return ''
    if is_unit_line(line):
        return ''
    # A repeated index line is a caption, not page furniture. A repeated
    # heading or book line is furniture. A short label matches with spaces
    # removed, so one surviving copy of a stripped header still drops.
    if _furniture(line, running):
        return ''
    return line


def _is_index_line(text):
    return is_numbered(text) or _has_index_token(text)


def _demote_banner(chosen, candidates, banners, headings=()):
    """A short heading segment, or a piece of one, loses to another usable line."""
    def blocked(text):
        return _is_banner(text, banners) or _is_heading_piece(text, headings)

    if not chosen or not blocked(chosen):
        return chosen
    others = [line for line in candidates if not blocked(line)]
    return others[-1] if others else chosen


def _longer_than_stub(chosen, candidates):
    """Fuller index first, otherwise the nearest longer usable line."""
    if not is_index_stub(chosen):
        return chosen
    indexed = [line for line in candidates if _is_index_line(line)]
    fuller = [line for line in indexed if not is_index_stub(line)]
    if fuller:
        return fuller[-1]
    longer = [line for line in candidates if len(plain_line(line)) > len(plain_line(chosen))]
    return longer[-1] if longer else chosen


def _heading_segments(path):
    return [plain_line(part) for part in (path or '').split('>') if plain_line(part)]


def _index_tokens(text):
    line = plain_line(text)
    if line[:1] in '（([':
        line = line[1:].lstrip()
    for sep in ('：', ':'):
        line = line.replace(sep, ' ')
    tokens = []
    for token in line.split():
        token = token.strip('、.．)）]】')
        if _INDEX_TOKEN.match(token):
            tokens.append(token)
    return tokens


def _lettered_tokens(text):
    """Index tokens that include a letter. A bare digit is shared too widely."""
    return [token for token in _index_tokens(text) if any(ch.isalpha() for ch in token)]


def _cell_texts(grid):
    return [
        (getattr(cell, 'text', '') or '').strip()
        for cell in (getattr(grid, 'cells', None) or [])
        if (getattr(cell, 'text', '') or '').strip()
    ]


def _label_cells(grid):
    """Short non-index cells that are not a full-width caption row.

    A line above the table that only repeats one of these is a field label.
    """
    columns = getattr(grid, 'column_count', 0) or 0
    labels = set()
    for cell in getattr(grid, 'cells', None) or []:
        text = _normalize(plain_line(getattr(cell, 'text', '') or ''))
        # Longer than an index stub: this is a caption cell, not a field label.
        if not text or len(text) * 2 > _LABEL_CHARS or _is_index_line(text):
            continue
        span = getattr(cell, 'col_span', 1) or 1
        if getattr(cell, 'row', None) == 0 and columns and span >= columns:
            continue
        labels.add(text)
        compact = _compact_label(text)
        if compact:
            labels.add(compact)
    return labels


def _is_cell_label(text, labels):
    line = _normalize(plain_line(text))
    if not line or _is_index_line(line):
        return False
    if line in labels:
        return True
    compact = _compact_label(line)
    return bool(compact) and compact in labels


def _drop_cell_label(chosen, labels, candidates):
    """A short label copied from the grid is not the table caption.

    Another usable line that is not itself a cell label wins. With none, the
    caption is empty. An index stub is left for the stub rule.
    """
    if not chosen or not _is_cell_label(chosen, labels):
        return chosen
    others = [line for line in candidates if line and not _is_cell_label(line, labels)]
    return others[-1] if others else ''


def _recover_sentence_end(line, running):
    """A line that is usable once its one trailing sentence mark is removed.

    Stub extension is the only caller. A mark still inside the line, a
    fragment, furniture, or the length bound still reject the stripped line.
    禁止硬编码: the mark is the documented sentence-end set, not a word list.
    """
    text = plain_line(line)
    if len(text) < 2 or text[-1] not in _SENTENCE_END:
        return ''
    return usable_line(text[:-1].rstrip(), running)


def _sentence_after_stub(chosen, window, running):
    """Immediate next non-furniture window line after a thin stub.

    Used only when a compact prefix and a lettered token both miss. Furniture
    is skipped. That next line has to become usable by dropping one trailing
    sentence mark, and it has to be longer than the stub. Any other failure
    stops the scan.
    """
    if not is_index_stub(chosen):
        return ''
    stub = _normalize(plain_line(chosen))
    if not stub:
        return ''
    idx = None
    for i, line in enumerate(window or ()):
        if _normalize(plain_line(line)) == stub:
            idx = i
    if idx is None:
        return ''
    for line in list(window)[idx + 1:]:
        if not plain_line(line):
            continue
        if _furniture(line, running):
            continue
        recovered = _recover_sentence_end(line, running)
        if recovered and len(plain_line(recovered)) > len(plain_line(chosen)):
            return recovered
        return ''
    return ''


def _stub_extensions(chosen, lines, running, window=()):
    """Longer lines that continue a thin stub.

    Spaces are ignored, so a heading written without the stub's space still
    matches. A shared lettered index token matches too (``8F`` with ``8F``),
    which a bare digit does not. A parent heading with neither stays out.
    A line that fails only because it ends on a sentence mark is eligible
    after that mark is removed. If neither the prefix nor the token matches,
    the immediate next non-furniture line after the stub can, on the same terms.
    """
    if not is_index_stub(chosen):
        return []
    stub = plain_line(chosen)
    stub_compact = ''.join(stub.split())
    letters = set(_lettered_tokens(stub))
    found = []
    for line in lines:
        text = usable_line(line, running) or _recover_sentence_end(line, running)
        if not text or len(plain_line(text)) <= len(stub):
            continue
        compact = ''.join(plain_line(text).split())
        if compact.startswith(stub_compact) and len(compact) > len(stub_compact):
            found.append(text)
            continue
        if letters and letters & set(_lettered_tokens(text)):
            found.append(text)
    if found:
        return found
    adjacent = _sentence_after_stub(chosen, window, running)
    return [adjacent] if adjacent else []


def _prefer_extension(chosen, lines, running, window=()):
    found = _stub_extensions(chosen, lines, running, window)
    if not found:
        return chosen
    return max(found, key=lambda text: len(plain_line(text)))


def _joined_row(grid, row):
    """Non-empty cells of one row, in column order, as one line."""
    found = [cell for cell in (getattr(grid, 'cells', None) or []) if getattr(cell, 'row', None) == row]
    found.sort(key=lambda cell: getattr(cell, 'column', 0))
    texts = [plain_line(getattr(cell, 'text', '') or '') for cell in found]
    texts = [text for text in texts if text]
    if len(texts) < 2:
        return ''
    return ' '.join(texts)


def form_title(grid, previous_text, running=(), banners=(), headings=()):
    """Caption carried by the grid, or the numbered line nearest the table.

    A cell in row 0 whose span covers every column is the caption when it is
    not page furniture, not a unit note, and not a short heading banner. That
    cell is kept even when it is a piece of the heading. A stub index loses to
    a fuller line in the previous source, a heading, or a grid row that
    continues the stub. A short line that only repeats a grid label is dropped.
    With no full-width caption cell, a short non-index line that is only a
    piece of the active heading is empty when nothing else is usable.
    Detection does not read this title.
    """
    columns = getattr(grid, 'column_count', 0) or 0
    labels = _label_cells(grid)
    extra = [line for line in list(headings) + _cell_texts(grid) if line]
    for row in (0, 1):
        joined = _joined_row(grid, row)
        if joined:
            extra.append(joined)
    raw_window = [line for line in (previous_text or '').splitlines() if line.strip()]
    window = [line for line in raw_window if usable_line(line, running)]

    def finish(title, clear_heading_piece):
        title = _prefer_extension(title, extra + raw_window, running, raw_window)
        title = _drop_cell_label(title, labels, window)
        if clear_heading_piece and _is_heading_piece(title, headings):
            return ''
        return title

    spanning = []
    for cell in getattr(grid, 'cells', None) or []:
        text = (getattr(cell, 'text', '') or '').strip()
        if getattr(cell, 'row', None) == 0 and columns and getattr(cell, 'col_span', 1) >= columns and text:
            spanning.append(text)
    if len(spanning) == 1:
        chosen = usable_line(spanning[0], running)
        fallback = caption_line(previous_text, running, banners, headings)
        if chosen and not is_index_stub(chosen) and not _is_banner(chosen, banners):
            return finish(chosen, False)
        if chosen and is_index_stub(chosen):
            promoted = _prefer_extension(
                _longer_than_stub(chosen, [line for line in (fallback, chosen) if line]),
                extra + raw_window, running, raw_window)
            if promoted != chosen:
                return promoted
            return finish(chosen, False)
        if chosen and _is_banner(chosen, banners):
            if fallback and not _is_banner(fallback, banners):
                return finish(fallback, False)
            return finish(chosen, False)
        if fallback:
            return finish(fallback, False)
    return finish(caption_line(previous_text, running, banners, headings), True)


def caption_line(text, running=(), banners=(), headings=()):
    """Nearest eligible line. An index line beats a closer unnumbered one.

    A stub index (at most half the short-label bound) loses to a longer index
    line in the same text, and otherwise to a longer usable line. A stub also
    loses to a longer heading or window line that continues it (same letters
    once spaces are removed, or a shared lettered index token). A window line
    that failed only on a trailing sentence mark can continue it too; otherwise
    the next non-furniture line after the stub, once that mark is removed.
    A short heading banner, including a short piece of the active heading,
    loses to another usable line. A stub with neither is kept.
    """
    lines = [line.strip() for line in (text or '').splitlines() if line.strip()]
    candidates = [line for line in lines if usable_line(line, running)]
    indexed = [line for line in candidates if _is_index_line(line)]
    pool = indexed or candidates
    if not pool:
        return ''
    chosen = _demote_banner(
        _longer_than_stub(pool[-1], candidates), candidates, banners, headings)
    return _prefer_extension(chosen, list(headings) + lines, running, lines)


def _header_labels(grid):
    """Header row, skipping a full-width caption cell in row 0."""
    columns = getattr(grid, 'column_count', 0) or 0
    cells = list(getattr(grid, 'cells', None) or [])

    def row_cells(row):
        found = [cell for cell in cells if getattr(cell, 'row', None) == row]
        found.sort(key=lambda cell: getattr(cell, 'column', 0))
        return found

    row = 0
    found = row_cells(0)
    if len(found) == 1 and columns and getattr(found[0], 'col_span', 1) >= columns:
        found = row_cells(1)
    return [_normalize(getattr(cell, 'text', '') or '') for cell in found]


def _continues(previous, grid):
    """Same column count, and the header repeats or this header row is empty."""
    if previous is None:
        return False
    columns = getattr(grid, 'column_count', 0) or 0
    if columns < 2 or columns != (getattr(previous, 'column_count', 0) or 0):
        return False
    current = _header_labels(grid)
    if not current:
        return False
    if not any(current):
        return True
    return current == _header_labels(previous)


def _only_furniture(text, running):
    """Every line was page furniture or a unit note, and at least one was."""
    lines = [plain_line(line) for line in (text or '').splitlines() if plain_line(line)]
    if not lines:
        return False
    saw = False
    for line in lines:
        if usable_line(line, running):
            return False
        if _normalize(line) in running or is_unit_line(line):
            saw = True
            continue
        return False
    return saw


def _union_running(running, extra):
    """Parser repeating lines are furniture even when only one copy remains."""
    seen = {line: {0, 1} for line in running}
    for line in extra or ():
        _remember_line(seen, line, 0)
        _remember_line(seen, line, 1)
    return {line for line, pages in seen.items() if len(pages) >= 2}


def form_captions(units, extra_running=()):
    """One title per grid. Text since the previous grid is the candidate window.

    When that window is only furniture, a continuation keeps the previous title.
    ``extra_running`` is the parser's repeating-line set. The table uses the
    nearest earlier heading path, the same way a page table hangs on a section.
    """
    running = _union_running(running_lines(units), extra_running)
    banners = heading_banners(units)
    gap = []
    titles = []
    previous_grid = None
    previous_title = ''
    heading = ''
    for unit in units:
        path = _heading_path(_locator(unit)).strip()
        if path:
            heading = path
        grid = _unit_grid(unit)
        if grid is not None:
            window = '\n'.join(gap)
            title = form_title(grid, window, running, banners, _heading_segments(heading))
            if not title and previous_title and _only_furniture(window, running) and _continues(previous_grid, grid):
                title = previous_title
            titles.append(title)
            previous_grid = grid
            previous_title = title
            gap = []
        else:
            text = _unit_text(unit).strip()
            if text:
                gap.append(text)
    return titles
