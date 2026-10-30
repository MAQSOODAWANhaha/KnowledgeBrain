"""Lossless semantic views for an oversized outline assembly request."""

from copy import deepcopy
from collections import Counter
import json


def _grid_view(grid):
    # Unknown cell attributes (for example formulae) retain their original view.
    if (set(grid) - {'row_count', 'column_count', 'cells', 'widths_mm'}
            or not {'row_count', 'column_count', 'cells'} <= grid.keys()
            or any(set(cell) != {'row', 'column', 'row_span', 'col_span', 'text'}
                   for cell in grid['cells'])):
        return grid
    # Never expand a sparse spreadsheet into a matrix larger than its source JSON.
    if grid['row_count'] * grid['column_count'] > len(json.dumps(grid, ensure_ascii=False)):
        return grid
    if len({(cell['row'], cell['column']) for cell in grid['cells']}) != len(grid['cells']):
        return grid
    rows = [[None] * grid['column_count'] for _ in range(grid['row_count'])]
    merges = []
    for cell in grid['cells']:
        rows[cell['row']][cell['column']] = cell['text']
        if cell['row_span'] != 1 or cell['col_span'] != 1:
            merges.append([cell[key] for key in ('row', 'column', 'row_span', 'col_span')])
    return {'row_count': grid['row_count'], 'column_count': grid['column_count'],
            'rows': rows, 'merged_cells': merges}


def _share_text_lines(payload, fields=('template_parts', 'source_excerpts', 'submission_sources')):
    """Share exact prose, retaining independent descriptors and original newlines."""
    if 'text_pool' in payload:
        return

    def size(value):
        return len(json.dumps(value, ensure_ascii=False, separators=(',', ':')).encode('utf-8'))

    entries = []
    for field in fields:
        records = payload.get(field, {})
        if isinstance(records, dict):
            records = records.values()
        entries.extend(entry for entry in records if isinstance(entry.get('text'), str))
    counts = Counter(line for entry in entries for line in entry['text'].split('\n') if line)
    shared = {}
    for line, count in counts.items():
        identity = len(shared)
        if (count - 1) * size(line) > count * (len(str(identity)) + 1):
            shared[line] = identity
    plans = []
    for entry in entries:
        lines = [shared.get(line, line) for line in entry['text'].split('\n')]
        if size({'text_lines': lines}) < size({'text': entry['text']}):
            plans.append((entry, lines))
    used = {line for _, lines in plans for line in lines if type(line) is int}
    pool = [line for line, identity in shared.items() if identity in used]
    remap = {shared[line]: identity for identity, line in enumerate(pool)}
    plans = [(entry, [remap[line] if type(line) is int else line for line in lines])
             for entry, lines in plans]
    saving = sum(size({'text': entry['text']}) - size({'text_lines': lines}) for entry, lines in plans)
    if not pool or saving <= size({'text_pool': pool}):
        return
    payload['text_pool'] = pool
    for entry, lines in plans:
        entry.pop('text')
        entry['text_lines'] = lines


def compact_assembly_payload(payload):
    """Keep all prose and IDs; native layout metadata remains with the host."""
    result = deepcopy(payload)
    result['assembly_view'] = 'compact_v1'
    for part in result['template_parts']:
        if part.get('grid'):
            part['grid'] = _grid_view(part['grid'])
    for source in result['submission_sources']:
        # _constraint_context serialized native grids as text; expose the same grid.
        try:
            grid = json.loads(source.get('text', ''))
        except (ValueError, TypeError):
            continue
        if isinstance(grid, dict) and {'row_count', 'column_count', 'cells'} <= grid.keys():
            source['grid'] = _grid_view(grid)
            source.pop('text')
    for chapter in result['chapters']:
        if chapter.get('is_volume') is False:
            chapter.pop('is_volume')
        for target in chapter['targets']:
            for key, default in (('manual_fill', False), ('support_origins', ['knowledge']),
                                 ('template_part_ids', [])):
                if target.get(key) == default:
                    target.pop(key)
    for field, key in (('template_parts', 'part_id'), ('source_excerpts', 'range_id'), ('submission_sources', 'source_id'),
                       ('submission_rules', 'rule_id'), ('unassigned_rules', 'rule_id')):
        if field not in result:
            continue
        result[field] = {entry[key]: {name: value for name, value in entry.items() if name != key}
                         for entry in result[field]}
    sections, excerpts = result.get('source_sections', []), result.get('source_excerpts', {})
    if len(sections) == 1 and excerpts and all(
            entry.get('section_id') == sections[0]['section_id'] for entry in excerpts.values()):
        for entry in excerpts.values():
            entry.pop('section_id')
    _share_text_lines(result)
    return result


def compact_structure_payload(payload):
    """Share exact prose and project grids; preserve the complete adopted structure."""
    result = deepcopy(payload)
    result['structure_view'] = 'compact_v1'
    for support in result['supports']:
        if support.get('grid'):
            support['grid'] = _grid_view(support['grid'])
    for source in result['submission_sources']:
        try:
            grid = json.loads(source.get('text', ''))
        except (ValueError, TypeError):
            continue
        if isinstance(grid, dict) and {'row_count', 'column_count', 'cells'} <= grid.keys():
            source['grid'] = _grid_view(grid)
            source.pop('text')
    _share_text_lines(result, fields=('supports', 'submission_sources'))
    return result
