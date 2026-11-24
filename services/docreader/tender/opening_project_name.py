"""Derive one procurement fact from the frozen announcement and its native form."""

import re
from collections import defaultdict

from docreader.tender.authoring_templates import load_template_part
from docreader.tender.evidence.references import EvidenceRepository, selector
from docreader.tender.schema import SourceExcerptRef
from docreader.tender.status import DocumentRole

_NAME = re.compile(r'项目名称\s*[：:]\s*(.+)')
_ANNOUNCEMENT = re.compile(r'(?:第[一1１]章)?招标公告')
_SECTION = re.compile(r'^[一二三四五六七八九十]+[、．.]')
_NEXT_FIELD = re.compile(r'(?:预算金额|最高投标限价|采购需求|招标编号|项目编号|采购方式|包号)\s*[：:]')


def _compact(text):
    return ''.join(text.split())


def _opening_part(target):
    parts = target.get('template_refs', [])
    if (target['kind'] == 'form' and target.get('manual_fill') is True
            and target.get('support_origins') == ['tender'] and _compact(target['purpose']) == '开标一览表'
            and not target.get('response_table') and not target.get('response_form_pending')
            and len(parts) == 1 and not parts[0]['selectors']):
        return parts[0]
    return None


def _name_from_announcement(index, source_ref):
    pages = defaultdict(list)
    for entry in index.ranges:
        if entry.get('text_basis') == 'pdf_literal' and entry['text'].strip():
            pages[entry['physical_locator']['page_ordinal']].append(entry)
    labels, names = [], []
    for lines in pages.values():
        plain = [entry['text'].strip() for entry in lines]
        compact = [_compact(line) for line in plain]
        for ordinal, line in enumerate(plain):
            if match := _NAME.fullmatch(line):
                names.append((lines[ordinal], match[1]))
        headings = [i for i, line in enumerate(compact) if _ANNOUNCEMENT.fullmatch(line)]
        sections = [i for i, line in enumerate(compact) if line in {'一、项目基本情况', '项目基本情况'}]
        if len(headings) == len(sections) == 1 and headings[0] < sections[0]:
            start = sections[0] + 1
            stop = next((i for i in range(start, len(lines)) if _SECTION.match(compact[i])), len(lines))
            labels.extend(lines[i] for i in range(start, stop - 1)
                          if _NAME.fullmatch(plain[i]) and _NEXT_FIELD.match(plain[i + 1]))
    if len(names) != 1 or len(labels) != 1:
        return None
    entry, name = names[0]
    if (entry != labels[0] or not any(character.isalnum() for character in name)
            or name in {'待定', '待填写', '无', '不适用'} or name.startswith(('详见', '参见'))):
        return None
    return {'source_ref': {'source_index_ref': source_ref, 'unit_id': entry['unit_id']},
            'selectors': [selector(entry)]}, name


def _project_name_anchor(grid, index, unit_id):
    cells = grid.get('cells', [])
    headers = [cell for cell in cells if _compact(cell['text']) == '项目名称']
    if len(headers) != 1:
        return None
    header, = headers
    if header['row'] != 0 or header['row_span'] != 1 or header['col_span'] != 1:
        return None
    column = header['column']
    covered = [cell for cell in cells if cell['row'] > 0
               and cell['column'] <= column < cell['column'] + cell['col_span']]
    if len(covered) != 1:
        return None
    cell, = covered
    if cell['row'] != 1 or cell['column'] != column or cell['col_span'] != 1 or cell['text'] != '':
        return None
    intervals = [interval for entry in index.ranges
                 if entry['unit_id'] == unit_id and entry.get('text_basis') == 'grid_row'
                 for interval in entry['cell_intervals']
                 if (interval['row'], interval['column']) == (1, column)]
    if (len(intervals) != 1 or intervals[0]['start'] != intervals[0]['end'] or intervals[0]['text'] != ''
            or intervals[0]['row_span'] != cell['row_span'] or intervals[0]['col_span'] != 1):
        return None
    return 1, column


def _opening_fact(artifacts, snapshot, part, repository):
    if not snapshot.get('frozen_ref'):
        return None
    try:
        documents = artifacts.read_json(snapshot['frozen_ref'])['documents']
        mains = [source for source in documents if source['role'] == DocumentRole.MAIN]
        if len(mains) != 1 or any(source['role'] in (DocumentRole.AMENDMENT, DocumentRole.CLARIFICATION)
                                  for source in documents):
            return None
        main, = mains
        reference = part['source_ref']
        if reference['source_index_ref'] != main['source_index_ref']:
            return None
        index = repository.source_index(main['source_index_ref'])
        if any(index.document[key] != main[key] for key in ('document_id', 'original_ref')):
            return None
        name = _name_from_announcement(index, main['source_index_ref'])
        if name is None:
            return None
        loaded = load_template_part(artifacts, part, repository=repository)
        anchor = _project_name_anchor(loaded['unit'].get('grid') or {}, index, reference['unit_id'])
        if anchor is not None:
            excerpt, value = name
            return excerpt, {'row': anchor[0], 'column': anchor[1], 'text': value}
    except (OSError, ValueError, KeyError):
        return None
    return None


def attach_opening_project_name(artifacts, outline):
    """Persist the proof only for the single source-owned opening form."""
    from docreader.tender.item_placement import _walk

    candidates = [(target, part) for _, target in _walk(outline['chapters'])
                  if (part := _opening_part(target)) is not None]
    if len(candidates) == 1:
        target, part = candidates[0]
        fact = _opening_fact(artifacts, outline['authoring_input'], part, EvidenceRepository(artifacts))
        if fact is not None:
            target['opening_project_name_excerpt'] = fact[0]


def opening_project_name_fill(artifacts, snapshot, target, part, *, repository=None):
    """Revalidate the saved proof before copying the value into the native cell."""
    excerpt = target.get('opening_project_name_excerpt')
    if excerpt is None or _opening_part(target) != part:
        return None
    fact = _opening_fact(artifacts, snapshot, part, repository or EvidenceRepository(artifacts))
    if fact is None:
        return None
    try:
        matches_source = SourceExcerptRef.model_validate(excerpt) == SourceExcerptRef.model_validate(fact[0])
    except ValueError:
        return None
    if matches_source:
        return fact[1]
    return None
