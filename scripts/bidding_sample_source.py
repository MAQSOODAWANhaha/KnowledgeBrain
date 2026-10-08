#!/usr/bin/env python3
"""Freeze a local acceptance source using the existing Python docreader service.

No chapter inference, document association or bidder-data defaults. Run with the
service virtualenv and PYTHONPATH=services. All budgets come from the run file.
"""
import argparse
import base64
import hashlib
import json
from pathlib import Path
import re
import uuid


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


def running_lines(units):
    """Lines that occur on at least two pages are page furniture.

    Repetition across pages is the signal, wherever the line sits on the page.
    A heading segment repeated on two pages is the same signal. The words
    themselves are not a list. A line repeated only inside one page is not
    furniture. Comparison ignores a leading heading mark.
    """
    seen = {}

    def add(line, page):
        line = _normalize(plain_line(line))
        if line and page is not None:
            seen.setdefault(line, set()).add(page)

    for unit in units:
        page = unit_page(unit)
        if page is None:
            continue
        for line in _unit_text(unit).splitlines():
            add(line, page)
        for part in _heading_path(_locator(unit)).split('>'):
            add(part, page)
    return {line for line, pages in seen.items() if len(pages) >= 2}


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
    # heading or book line is furniture.
    if _normalize(line) in running and not _is_index_line(line):
        return ''
    return line


def _is_index_line(text):
    return is_numbered(text) or _has_index_token(text)


def form_title(grid, previous_text, running=()):
    """Caption carried by the grid, or the numbered line nearest the table.

    A cell in row 0 whose span covers every column is the caption when it is
    not page furniture and not a unit note. A stub index there loses to a fuller
    index line in the previous source. Otherwise walk that source from the table
    backward. Detection does not read this title.
    """
    columns = getattr(grid, 'column_count', 0) or 0
    spanning = []
    for cell in getattr(grid, 'cells', None) or []:
        text = (getattr(cell, 'text', '') or '').strip()
        if getattr(cell, 'row', None) == 0 and columns and getattr(cell, 'col_span', 1) >= columns and text:
            spanning.append(text)
    if len(spanning) == 1:
        chosen = usable_line(spanning[0], running)
        if chosen and not is_index_stub(chosen):
            return chosen
        fallback = caption_line(previous_text, running)
        if chosen and (not fallback or not _is_index_line(fallback) or is_index_stub(fallback)):
            return chosen
        if fallback:
            return fallback
    return caption_line(previous_text, running)


def caption_line(text, running=()):
    """Nearest eligible line. An index line beats a closer unnumbered one.

    A stub index (at most half the short-label bound) loses to a longer index
    line in the same text. A stub with no fuller index line is kept.
    """
    lines = [line.strip() for line in (text or '').splitlines() if line.strip()]
    candidates = [line for line in lines if usable_line(line, running)]
    indexed = [line for line in candidates if _is_index_line(line)]
    pool = indexed or candidates
    if not pool:
        return ''
    chosen = pool[-1]
    if is_index_stub(chosen):
        fuller = [line for line in indexed if not is_index_stub(line)]
        if fuller:
            return fuller[-1]
    return chosen


def form_captions(units):
    """One title per grid. Text since the previous grid is the candidate window."""
    running = running_lines(units)
    gap = []
    titles = []
    for unit in units:
        grid = _unit_grid(unit)
        if grid is not None:
            titles.append(form_title(grid, '\n'.join(gap), running))
            gap = []
        else:
            text = _unit_text(unit).strip()
            if text:
                gap.append(text)
    return titles


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--limits', type=Path, required=True)
    args = parser.parse_args()
    from docreader.main import DocReaderServicer
    from docreader.proto.docreader_pb2 import ReadConfig, ReadRequest, SourceViewRequest
    from docreader.parser.source_view import source_view

    raw = args.source.read_bytes()
    sha = hashlib.sha256(raw).hexdigest()
    limits = json.loads(args.limits.read_text())['extraction']
    out = args.output
    out.mkdir(parents=True, exist_ok=True)
    if (out / 'frozen-input.json').exists():
        raise SystemExit('frozen input exists; reuse it or choose a new output directory')
    parsed, _ = DocReaderServicer()._parse_request(ReadRequest(
        file_content=raw, file_name=args.source.name, file_type=args.source.suffix.lstrip('.'),
        config=ReadConfig(parser_engine='builtin')))
    if not parsed.is_valid() or parsed.metadata.get('table_extraction_error'):
        raise SystemExit('shared parser did not produce a valid complete result')
    identity = lambda key: str(uuid.uuid5(uuid.NAMESPACE_URL, f'{sha}/{key}'))
    document = identity('document')
    sources, forms = [], []
    captions = iter(form_captions(parsed.structured_source_units))
    for unit in parsed.structured_source_units:
        sid = identity(unit.key)
        locator = unit.locator.model_dump(mode='json')
        sources.append(dict(source_unit_revision_id=sid, document_id=document,
                            text=unit.text, locator=locator, ordinal=unit.ordinal))
        if unit.grid is not None:
            form_id = identity('form/' + unit.key)
            definition = dict(unit.grid.model_dump(mode='json', exclude_none=True),
                              schema_version=3, kind='grid',
                              form_definition_revision_id=form_id,
                              source_unit_revision_id=sid, title=next(captions))
            forms.append(dict(form_definition_revision_id=form_id,
                              source_unit_revision_id=sid, definition=definition))
    frozen = dict(schema_version=1, project_id=identity('sample-project'),
                  document_set_id=identity('document-set'),
                  documents=[dict(document_id=document, file_name=args.source.name,
                                  sha256=sha, byte_length=len(raw), role='main_tender')],
                  document_relations=[], decisions=[], source_units=sources, structured_forms=forms)
    (out / 'parsed-source.json').write_text(parsed.model_dump_json(), encoding='utf-8')
    if args.source.suffix.lower() == '.pdf':
        views = out / 'source-views'
        views.mkdir(exist_ok=True)
        for page in range(parsed.metadata['page_count']):
            result = source_view(SourceViewRequest(file_content=raw, source_sha256=sha,
                media_type='application/pdf', page_ordinal=page,
                max_edge=limits['max_source_view_edge'], max_image_bytes=limits['max_source_view_bytes']))
            payload = dict(identity=dict(source_id='', original_sha256=sha,
                image_sha256=result.image_sha256, page_ordinal=page, width=result.width,
                height=result.height, renderer=result.renderer),
                jpeg_base64=base64.b64encode(result.image_data).decode('ascii'))
            (views / f'page-{page}.json').write_text(json.dumps(payload), encoding='utf-8')
    # Publish only after all original views exist. A failed freeze can be
    # retried; the runner never sees a supposedly complete partial collection.
    (out / 'frozen-input.json').write_text(json.dumps(frozen, ensure_ascii=False), encoding='utf-8')
    print(json.dumps(dict(source_sha256=sha, sources=len(sources), forms=len(forms))))


if __name__ == '__main__':
    main()
