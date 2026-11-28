"""Editable signature lines on a cover verified against its frozen PDF page."""

from io import BytesIO
from statistics import median
from unicodedata import normalize

from docx.shared import Pt

from docreader.tender.evidence.references import EvidenceRepository


def _key(text):
    return ''.join(normalize('NFKC', text).split())


def bind_cover_source(artifacts, snapshot, compiled, nodes):
    covers = [c for c in compiled['chapters'] if c['presentation'] == 'cover' and not c.get('template_parts')]
    if not covers or not artifacts or not snapshot.get('frozen_ref'):
        return
    try:
        records = {d['document_id']: d for d in artifacts.read_json(snapshot['frozen_ref'])['documents']}
    except (KeyError, OSError, ValueError):
        compiled['issues'].append({'code': 'outline_directory_literal_pending',
                                   'detail': '未能读取冻结封面来源，请核对签署栏版式。'})
        return
    repository = EvidenceRepository(artifacts)
    for cover in covers:
        record = records.get(cover['heading']['document_id'])
        targets = nodes[cover['id']]['targets']
        if (not record or len(targets) != 1 or targets[0].get('template_refs') or targets[0]['kind'] != 'form'
                or not record.get('source_index_ref') or not record.get('original_ref')):
            continue
        try:
            with artifacts.path(record['original_ref']).open('rb') as source:
                if source.read(5) != b'%PDF-':
                    continue
            index = repository.source_index(record['source_index_ref'])
        except (KeyError, OSError, ValueError):
            compiled['issues'].append({'code': 'outline_directory_literal_pending',
                                       'detail': '未能读取原始封面页，请核对签署栏版式。'})
            continue
        page_units = [u for u in index.ranges if (u.get('physical_locator') or u['locator']).get('page_ordinal')
                      == cover['heading']['page'] and u.get('text', '').strip()]
        title = _key(cover['title'])
        matches = {u['unit_id'] for u in page_units if _key(u['text']) == title}
        if title == '投标文件':
            for i, unit in enumerate(page_units):
                if unit['text'].strip() == '投' and [
                        _key(u['text']) for u in page_units[i:i + len(title)]] == list(title):
                    matches.add(unit['unit_id'])
        if len(matches) == 1:
            targets[0]['source_refs'] = [{'source_index_ref': record['source_index_ref'], 'unit_id': matches.pop()}]
        else:
            compiled['issues'].append({'code': 'outline_directory_literal_pending',
                                       'detail': f'文档 {record["document_id"]} 第{cover["heading"]["page"] + 1}页封面标题定位待核。'})


def _line_parts(line, edges, size, source_text):
    rules = [(e['x0'], e['x1']) for e in edges if e['orientation'] == 'h' and e['width'] >= size / 2
             and abs(e['top'] - line['bottom']) <= size / 4
             and e['x1'] >= line['x0'] - 3 * size and e['x0'] <= line['x1'] + 3 * size]
    return fill_line_parts(line['chars'], source_text, rules) or (line['x0'], [source_text.strip()])


def fill_line_parts(glyphs, source_text, rules):
    """Interleave verified fixed text and blank rule endpoints in source order."""
    chars = sorted((c for c in glyphs if c['text'].strip()), key=lambda c: c['x0'])
    rules = sorted({(left, right) for left, right in rules
                    if not any(c['x0'] + 1 < right and left < c['x1'] - 1 for c in chars)})
    if not chars or not rules:
        return None
    positions = [i for i, char in enumerate(source_text) if not char.isspace()]
    if (len(positions) != len(chars)
            or _key(source_text) != _key(''.join(c['text'] for c in chars))):
        return None

    def text_between(first, end):
        return source_text[positions[first]:positions[end - 1] + 1] if end > first else ''

    first = min(chars[0]['x0'], rules[0][0])
    parts, start, cursor = [], 0, first
    for left, right in rules:
        end = start
        while end < len(chars) and chars[end]['x1'] <= left + 1:
            end += 1
        parts.append(text_between(start, end))
        if end > start:
            cursor = chars[end - 1]['x1']
        if left > cursor + 2:
            parts.append((left, False))
        parts.append((right, True))
        start = end
        cursor = right
    parts.append(text_between(start, len(chars)))
    return first, parts


def _page_layout(data, page_number, text):
    import pdfplumber

    if not data.startswith(b'%PDF'):
        return None
    with pdfplumber.open(BytesIO(data), pages=[page_number + 1]) as pdf:
        if len(pdf.pages) != 1:
            return None
        page = pdf.pages[0]
        lines = {}
        for line in page.extract_text_lines(return_chars=True):
            lines.setdefault(_key(line['text']), []).append(line)
        rows = []
        for source in (s for s in text.splitlines() if s.strip()):
            found = lines.get(_key(source), [])
            if len(found) != 1 or (rows and found[0]['top'] < rows[-1][1]):
                return None
            line = found[0]
            size = median(c['size'] for c in line['chars'])
            left, parts = _line_parts(line, page.edges, size, source)
            rows.append((line['top'], line['bottom'], left, size, parts))
        return (page.width, page.height, rows) if rows else None


def write_pdf_cover(document, artifacts, repository, snapshot, source_ref, text):
    from pdfplumber.utils.exceptions import PdfminerException

    if not (snapshot or {}).get('frozen_ref'):
        return False
    try:
        frozen = artifacts.read_json(snapshot['frozen_ref'])
        if not any(d.get('source_index_ref') == source_ref['source_index_ref'] for d in frozen['documents']):
            return False
        index = repository.source_index(source_ref['source_index_ref'])
        pages = {(u.get('physical_locator') or u['locator']).get('page_ordinal')
                 for u in index.ranges if u['unit_id'] == source_ref['unit_id']}
        if len(pages) != 1 or None in pages:
            return False
        layout = _page_layout(artifacts.read_bytes(index.document['original_ref']), pages.pop(), text)
    except (KeyError, OSError, ValueError, PdfminerException):
        return False
    if not layout:
        return False
    width, height, rows = layout
    sect = document.sections[-1]
    sx, sy = sect.page_width.pt / width, sect.page_height.pt / height
    margin, cursor = sect.left_margin.pt, sect.top_margin.pt
    for row_index, (top, bottom, left, size, parts) in enumerate(rows):
        gap = max(0, top * sy - cursor)
        if not row_index and gap >= 1:
            # Word may suppress the first paragraph's space_before at a page boundary.
            spacer = document.add_paragraph().paragraph_format
            spacer.space_before = spacer.space_after = Pt(0)
            spacer.line_spacing = Pt(gap)
            gap = 0
        p = document.add_paragraph()
        fmt = p.paragraph_format
        fmt.left_indent = Pt(left * sx - margin)
        fmt.space_before, fmt.space_after = Pt(gap), Pt(0)
        line_height = max(size, bottom - top) * sy
        fmt.line_spacing = Pt(line_height)
        for part in parts:
            if isinstance(part, str):
                run = p.add_run(part)
            else:
                position, underline = part
                fmt.tab_stops.add_tab_stop(Pt(max(0, position * sx - margin)))
                run = p.add_run('\t')
                run.underline = underline
            run.font.size = Pt(size * sy)
        cursor = top * sy + line_height
    return True
