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
import uuid


_SENTENCE_END = '。.!！?？;；'
_FRAGMENT_START = '）。)》」』、，,;；'
# Same short-label bound as outline attachment detection. A unit note is one
# colon with both sides inside it. Documented in docs/bidding/outline.md.
_LABEL_CHARS = 12
_NUMERALS = '一二三四五六七八九十'
_DIGITS = '0123456789０１２３４５６７８９'


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


def running_lines(units):
    """Lines that occur on at least two pages are page furniture.

    Repetition across pages is the signal, wherever the line sits on the page.
    The words themselves are not a list. A line repeated only inside one page
    is not furniture.
    """
    seen = {}
    for unit in units:
        page = page_ordinal(getattr(unit, 'locator', None) if not isinstance(unit, dict) else unit.get('locator'))
        text = unit['text'] if isinstance(unit, dict) else getattr(unit, 'text', '')
        if page is None:
            continue
        for line in (text or '').splitlines():
            line = _normalize(line.strip())
            if line:
                seen.setdefault(line, set()).add(page)
    return {line for line, pages in seen.items() if len(pages) >= 2}


def is_fragment(text):
    """A line that starts mid-sentence or contains an internal sentence stop."""
    line = (text or '').strip()
    if not line:
        return False
    if line[0] in _FRAGMENT_START:
        return True
    return any(ch in '。！？!?' for ch in line[:-1])


def is_unit_line(text):
    """A parenthetical note, or one short colon annotation.

    Both sides of the colon have to be short labels. A numbered caption is not
    a unit note even when it contains a colon.
    """
    line = (text or '').strip()
    if len(line) < 3 or is_numbered(line):
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
    line = (text or '').strip()
    if not line or len(line) > 80 or line[-1] in _SENTENCE_END or is_fragment(line):
        return ''
    if _normalize(line) in running or is_unit_line(line):
        return ''
    return line


def form_title(grid, previous_text, running=()):
    """Caption carried by the grid, or the numbered line nearest the table.

    A cell in row 0 whose span covers every column is the caption when it is
    not page furniture and not a unit note. Otherwise walk the previous source
    from the table backward. Detection does not read this title.
    """
    columns = getattr(grid, 'column_count', 0) or 0
    spanning = []
    for cell in getattr(grid, 'cells', None) or []:
        text = (getattr(cell, 'text', '') or '').strip()
        if getattr(cell, 'row', None) == 0 and columns and getattr(cell, 'col_span', 1) >= columns and text:
            spanning.append(text)
    if len(spanning) == 1:
        chosen = usable_line(spanning[0], running)
        if chosen:
            return chosen
    return caption_line(previous_text, running)


def caption_line(text, running=()):
    """Nearest eligible line. A numbered line beats a closer unnumbered one."""
    lines = [line.strip() for line in (text or '').splitlines() if line.strip()]
    candidates = [line for line in lines if usable_line(line, running)]
    numbered = [line for line in candidates if is_numbered(line)]
    pool = numbered or candidates
    return pool[-1] if pool else ''


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
    previous_text = ''
    running = running_lines(parsed.structured_source_units)
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
                              source_unit_revision_id=sid, title=form_title(unit.grid, previous_text, running))
            forms.append(dict(form_definition_revision_id=form_id,
                              source_unit_revision_id=sid, definition=definition))
        if unit.text.strip():
            previous_text = unit.text
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
