"""Lossless semantic views for an oversized outline assembly request."""

from copy import deepcopy
from collections import Counter
import json
import re


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


def slim_revision_payload(payload, previous, findings):
    """Keep decision text; already adopted bodies stay on the host.

    Call count is unchanged: one revision still replaces one A1. The request grows with
    unassigned rules and targets named by findings, not with every adopted form.
    The worst input is still the first full assembly. A model that cannot see a stripped
    form keeps that form by omission; a later finding that names its v-id sends it again.
    Old full-tree revision responses do not replay, because this request is different.
    """
    named = set()
    for finding in findings or []:
        for field in ('detail', 'suggestion'):
            named.update(re.findall(r'\bv\d+\b', finding.get(field) or ''))
    keep_parts, keep_excerpts = set(), set()

    def walk(chapters):
        for chapter in chapters or []:
            for target in chapter.get('targets') or []:
                if target.get('review_target_id') not in named:
                    continue
                keep_parts.update(target.get('template_part_ids') or [])
                keep_parts.update(target.get('excluded_template_part_ids') or [])
                table = target.get('response_table') or {}
                keep_excerpts.update(table.get('context_range_ids') or [])
                for row in table.get('rows') or []:
                    keep_excerpts.update(row.get('range_ids') or [])
                    keep_excerpts.update(row.get('context_range_ids') or [])
                for guidance in table.get('source_guidance') or []:
                    keep_excerpts.update(guidance.get('range_ids') or [])
            walk(chapter.get('children'))
    walk(previous)
    for rule in payload.get('unassigned_rules') or []:
        for evidence in (rule.get('rule') or {}).get('evidence') or []:
            keep_excerpts.update(evidence.get('range_ids') or [])
    result = deepcopy(payload)
    result['previous_structure'] = previous
    result['revision_view'] = 'bound_v1'

    def strip(entry, identity, keep):
        if identity in keep:
            return entry
        kept = {key: value for key, value in entry.items() if key not in {'text', 'text_lines', 'grid'}}
        kept['retained'] = True
        return kept
    if isinstance(result.get('template_parts'), list):
        result['template_parts'] = [strip(part, part.get('part_id'), keep_parts) for part in result['template_parts']]
    if isinstance(result.get('source_excerpts'), list):
        result['source_excerpts'] = [strip(excerpt, excerpt.get('range_id'), keep_excerpts)
                                     for excerpt in result['source_excerpts']]
    return result


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
    result['structure_view'] = 'compact_v2'
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

    def text_entry(record):
        return record['text'] if set(record) == {'text'} else record

    grouped, unsectioned = {}, {}
    for support in result['supports']:
        identity = support.pop('support_id')
        section = support.get('section_id')
        destination = unsectioned
        if isinstance(section, str) and section:
            support.pop('section_id')
            destination = grouped.setdefault(section, {})
        for field, default in (('origin', 'tender'), ('grid', None), ('header_row_count', None)):
            if field in support and support[field] == default:
                support.pop(field)
        destination[identity] = text_entry(support)
    result['supports'] = unsectioned
    if grouped:
        result['support_sections'] = grouped
    result['submission_sources'] = {
        source['source_id']: text_entry({key: value for key, value in source.items() if key != 'source_id'})
        for source in result['submission_sources']}

    def chapters(entries):
        for chapter in entries:
            chapters(chapter.get('children', []))
            for target in chapter.get('targets', []):
                if target.get('manual_fill') is False:
                    target.pop('manual_fill')
                for field in ('related_target_ids', 'template_support_ids', 'source_support_ids',
                              'response_context_support_ids', 'response_rows', 'task_ids', 'rule_ids'):
                    if target.get(field) == []:
                        target.pop(field)
            if chapter.get('is_volume') is False:
                chapter.pop('is_volume')
            if chapter.get('presentation_role') == 'body':
                chapter.pop('presentation_role')
            for field in ('targets', 'children'):
                if chapter.get(field) == []:
                    chapter.pop(field)

    chapters(result.get('chapters', []))
    return result
