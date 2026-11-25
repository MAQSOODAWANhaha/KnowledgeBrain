"""Grouped structure views preserve all semantic records and their reference edges."""
from typing import Any, cast

from copy import deepcopy
import json

from docreader.tender.authoring_view import compact_structure_payload


CHAPTER_DEFAULTS = {'is_volume': False, 'presentation_role': 'body', 'targets': [], 'children': []}
TARGET_DEFAULTS = {'manual_fill': False, 'related_target_ids': [], 'template_support_ids': [],
                   'source_support_ids': [], 'response_context_support_ids': [], 'response_rows': [],
                   'task_ids': [], 'rule_ids': []}


def expanded_structure(view):
    """Restore the grouped semantic projection; support dictionaries have no ordering semantics."""
    result = deepcopy(view)
    pool = result.pop('text_pool', [])
    result.pop('structure_view', None)

    def entry(value):
        record = {'text': value} if isinstance(value, str) else deepcopy(value)
        if 'text_lines' in record:
            record['text'] = '\n'.join(pool[line] if type(line) is int else line
                                        for line in record.pop('text_lines'))
        grid = record.get('grid')
        if grid and 'rows' in grid:
            spans = {(row, column): (rs, cs) for row, column, rs, cs in cast(Any, grid)['merged_cells']}
            cast(Any, record)['grid'] = {key: value for key, value in cast(Any, grid).items() if key not in {'rows', 'merged_cells'}}
            cast(Any, record)['grid']['cells'] = [
                {'row': row, 'column': column, 'row_span': spans.get((row, column), (1, 1))[0],
                 'col_span': spans.get((row, column), (1, 1))[1], 'text': text}
                for row, cells in enumerate(cast(Any, grid)['rows']) for column, text in enumerate(cells) if text is not None]
        if record.get('grid'):
            # Native widths remain in the host and were already absent from the F grid projection.
            cast(Any, record['grid']).pop('widths_mm', None)
        return record

    supports = result.get('supports', [])
    if isinstance(supports, list):
        supports = {support['support_id']: {key: value for key, value in support.items() if key != 'support_id'}
                    for support in supports}
    supports = {identity: entry(value) for identity, value in supports.items()}
    for section, records in result.pop('support_sections', {}).items():
        for identity, value in records.items():
            supports[identity] = {**entry(value), 'section_id': section}
    for support in supports.values():
        support.setdefault('origin', 'tender')
        support.setdefault('grid', cast(Any, None))
        support.setdefault('header_row_count', cast(Any, None))
    result['supports'] = supports
    sources = result.get('submission_sources', [])
    if isinstance(sources, list):
        sources = {source['source_id']: {key: value for key, value in source.items() if key != 'source_id'}
                   for source in sources}
    result['submission_sources'] = {identity: entry(value) for identity, value in sources.items()}
    for source in result['submission_sources'].values():
        try:
            grid = json.loads(source.get('text', ''))
        except (ValueError, TypeError):
            continue
        if isinstance(grid, dict) and {'row_count', 'column_count', 'cells'} <= grid.keys():
            source.pop('text')
            source['grid'] = grid
            source['grid'].pop('widths_mm', None)

    def chapters(entries):
        for chapter in entries:
            for name, value in CHAPTER_DEFAULTS.items():
                chapter.setdefault(name, deepcopy(value))
            for target in chapter['targets']:
                for name, value in TARGET_DEFAULTS.items():
                    target.setdefault(name, deepcopy(value))
            chapters(chapter['children'])

    chapters(result.get('chapters', []))
    return result


def payload():
    text = '\n' + '　限定Ａ、空白  与🙂\t都不改。' * 25 + '\n\n末行\r\n'
    return {'operation': 'structure', 'sections': [
        {'section_id': 'section-a', 'heading_path': '业务 > 技术'},
        {'section_id': 'section-b', 'heading_path': '业务 > 服务'}],
        'supports': [
            {'support_id': 's1', 'section_id': 'section-a', 'origin': 'tender', 'text': text,
             'grid': None, 'header_row_count': None},
            {'support_id': 's2', 'section_id': 'section-b', 'origin': 'knowledge', 'text': text,
             'subject': '原厂', 'native_metadata': {'applicability': ['仅甲标段']}},
            {'support_id': 's3', 'section_id': 'section-a', 'origin': 'tender', 'text': '独立来源，相同标题。'},
            {'support_id': 's4', 'origin': 'knowledge', 'text': '没有章节，不归入相邻章节。'},
            {'support_id': 's5', 'text': ''}],
        'submission_sources': [{'source_id': 'u1', 'text': text},
                               {'source_id': 'u2', 'text': '独立提交条件。', 'bindable': False}],
        'target_rules': {'r1': {'text': '完整要求', 'conditions': ['仅甲标段'], 'support_ids': ['s1']}},
        'chapters': [{'chapter_id': 'c1', 'title': '应答', 'is_volume': False, 'presentation_role': 'body',
                      'targets': [{'target_id': 't1', 'purpose': '响应完整要求', 'manual_fill': False,
                                   'rule_ids': ['r1'], 'source_support_ids': ['s3', 's1'],
                                   'related_target_ids': [], 'template_support_ids': [], 'task_ids': []}],
                      'children': []}],
        'units': [], 'input_issues': []}


def test_supports_group_by_existing_section_keep_unsectioned_sources_and_rule_edges():
    original = payload()
    before = deepcopy(original)
    view = compact_structure_payload(original)
    assert view['structure_view'] == 'compact_v2'
    assert set(view['support_sections']) == {'section-a', 'section-b'}
    assert list(view['support_sections']['section-a']) == ['s1', 's3']
    assert set(view['supports']) == {'s4', 's5'}
    assert view['support_sections']['section-b']['s2']['origin'] == 'knowledge'
    assert view['target_rules'] == before['target_rules']
    assert expanded_structure(view) == expanded_structure(before)
    assert original == before
    assert len(json.dumps(view, ensure_ascii=False)) < len(json.dumps(before, ensure_ascii=False))


def test_known_defaults_are_omitted_but_actual_manual_volume_role_and_unknown_fields_remain():
    original = payload()
    chapter = original['chapters'][0]
    chapter['native_flag'] = False
    chapter['children'] = [{'chapter_id': 'c2', 'title': '封面', 'is_volume': True,
                            'presentation_role': 'cover', 'targets': [], 'children': []}]
    chapter['targets'][0].update({'manual_fill': True, 'unrecognized_empty': []})
    view = compact_structure_payload(original)
    compact = view['chapters'][0]
    assert 'is_volume' not in compact and 'presentation_role' not in compact
    assert compact['native_flag'] is False
    assert compact['targets'][0]['manual_fill'] is True
    assert compact['targets'][0]['unrecognized_empty'] == []
    assert compact['children'][0]['is_volume'] is True
    assert compact['children'][0]['presentation_role'] == 'cover'
    assert expanded_structure(view) == expanded_structure(original)


def test_only_literal_text_entries_use_string_short_form_and_existing_pool_is_not_replaced():
    original = {'sections': [], 'supports': [
        {'support_id': 's1', 'origin': 'tender', 'text': '完整原文。', 'grid': None, 'header_row_count': None},
        {'support_id': 's2', 'origin': 'tender', 'text_lines': [0, '', '123', 0]},
        {'support_id': 's3', 'origin': 'knowledge', 'text': '完整原文。'}],
        'submission_sources': [{'source_id': 'u1', 'text': '完整原文。'}], 'text_pool': ['先前已固定原文。']}
    view = compact_structure_payload(original)
    assert view['supports']['s1'] == '完整原文。'
    assert view['supports']['s2'] == {'text_lines': [0, '', '123', 0]}
    assert view['supports']['s3'] == {'origin': 'knowledge', 'text': '完整原文。'}
    assert view['submission_sources']['u1'] == '完整原文。'
    assert view['text_pool'] == original['text_pool']
    assert expanded_structure(view) == expanded_structure(original)


def test_grouping_keeps_native_grid_conditions_blank_cells_unknown_attributes_and_section_identity():
    original = payload()
    grid = {'row_count': 2, 'column_count': 2, 'cells': [
        {'row': 0, 'column': 0, 'row_span': 1, 'col_span': 2, 'text': '共同条件🙂'},
        {'row': 1, 'column': 0, 'row_span': 1, 'col_span': 1, 'text': ''},
        {'row': 1, 'column': 1, 'row_span': 1, 'col_span': 1, 'text': '  '}]}
    unknown = deepcopy(grid)
    unknown['cells'][0]['formula'] = 'SUM(A1:A3)'
    original['supports'][0].update({'text': '', 'grid': grid, 'header_row_count': 1})
    original['supports'][2].update({'text': '', 'grid': unknown})
    original['submission_sources'][0]['text'] = json.dumps(grid, ensure_ascii=False)
    view = compact_structure_payload(original)
    records = view['support_sections']['section-a']
    assert records['s1']['grid']['rows'] == [['共同条件🙂', None], ['', '  ']]
    assert records['s1']['grid']['merged_cells'] == [[0, 0, 1, 2]]
    assert records['s1']['header_row_count'] == 1
    assert records['s3']['grid'] == unknown
    assert expanded_structure(view) == expanded_structure(original)
