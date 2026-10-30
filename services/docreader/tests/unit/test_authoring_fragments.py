"""Capacity splitting preserves complete response obligations and source scopes."""
from copy import deepcopy

from docreader.tender.authoring_fragments import split_writing_task


def source_ref(unit):
    return {'source_index_ref': {'sha256': 'a' * 64}, 'unit_id': unit}


def excerpt(unit, *ranges):
    return {'source_ref': source_ref(unit), 'selectors': [
        {'range_id': identity, 'source_range_id': identity, 'start': 0, 'end': 10}
        for identity in ranges]}


def rule(identity):
    return {'item_ref': {'artifact_ref': {'sha256': 'b' * 64}, 'item_id': identity}}


def task(target):
    return {'task_id': 'original', 'chapter_id': 'chapter', 'title_path': ['技术'],
            'targets': [{'target_id': 'target', 'kind': 'narrative', 'purpose': '逐项响应',
                         'source_refs': [], 'rule_refs': [], **target}]}


def test_response_rows_remain_whole_with_cross_page_continuation_and_common_conditions():
    shared = excerpt('condition', 'scope')
    rows = [
        {'row_id': 'row-a', 'source_excerpts': [excerpt('page-a', 'start'), excerpt('page-b', 'end')],
         'rule_refs': [rule('a')]},
        {'row_id': 'row-b', 'source_excerpts': [excerpt('page-b', 'independent-proof')],
         'rule_refs': [rule('b')]},
        {'row_id': 'row-c', 'source_excerpts': [excerpt('page-c', 'service')], 'rule_refs': [rule('c')]},
    ]
    original = task({'source_excerpts': [shared],
        'source_refs': [source_ref(unit) for unit in ('condition', 'page-a', 'page-b', 'page-c', 'whole-common')],
        'rule_refs': [rule(identity) for identity in ('common', 'a', 'b', 'c')],
        'response_table': {'rows': rows}})
    before = deepcopy(original)
    parts = split_writing_task(original)
    assert len(parts) == 2 and original == before
    assert [row for part in parts for row in part['targets'][0]['response_table']['rows']] == rows
    for part in parts:
        target = part['targets'][0]
        assert part['task_id'] == original['task_id'] and target['target_id'] == 'target'
        assert target['source_excerpts'] == [shared]
        selected = target['response_table']['rows']
        assert target['rule_refs'] == [rule('common'), *(r for row in selected for r in row['rule_refs'])]
        assert source_ref('whole-common') in target['source_refs']
        if not any(row['row_id'] == 'row-c' for row in selected):
            assert source_ref('page-c') not in target['source_refs']
    assert parts == split_writing_task(original)
    parts[0]['targets'][0]['response_table']['rows'][0]['source_excerpts'][0]['selectors'].clear()
    assert original == before


def test_single_composite_row_does_not_split_cross_page_or_native_selectors():
    original = task({'response_table': {'rows': [{
        'row_id': 'row', 'source_excerpts': [excerpt('a', 'r1', 'r2'), excerpt('b', 'r3')],
        'rule_refs': [rule('a'), rule('b')]}]}, 'rule_refs': [rule('a'), rule('b')]})
    assert split_writing_task(original) == []


def test_derived_response_table_can_split_without_a_native_form_grid():
    original = task({'kind': 'form', 'response_table': {'rows': [
        {'row_id': identity, 'source_excerpts': [excerpt(identity, 'r-' + identity)], 'rule_refs': []}
        for identity in ('a', 'b')]}})
    parts = split_writing_task(original)
    assert [[row['row_id'] for row in part['targets'][0]['response_table']['rows']] for part in parts] == [['a'], ['b']]
    assert all(split_writing_task(part) == [] for part in parts)


def test_narrative_splits_complete_rule_references_and_keeps_shared_source_context():
    original = task({'rule_refs': [rule(str(i)) for i in range(5)],
                     'source_excerpts': [excerpt('condition', 'scope', 'exception')]})
    parts = split_writing_task(original)
    assert [ref for part in parts for ref in part['targets'][0]['rule_refs']] == original['targets'][0]['rule_refs']
    assert all(part['targets'][0]['source_excerpts'] == original['targets'][0]['source_excerpts'] for part in parts)
    assert all(len(part['targets'][0]['rule_refs']) < 5 for part in parts)


def test_source_only_narrative_splits_logical_excerpts_without_breaking_their_selectors():
    selections = [excerpt('a', 'header', 'row', 'notes'), excerpt('b', 'cross-page'), excerpt('c', 'proof')]
    original = task({'source_excerpts': selections,
                     'source_refs': [source_ref(unit) for unit in ('a', 'b', 'c', 'common-legacy')]})
    parts = split_writing_task(original)
    assert [ref for part in parts for ref in part['targets'][0]['source_excerpts']] == selections
    for part in parts:
        target = part['targets'][0]
        assert target['source_refs'] == [*(ref['source_ref'] for ref in target['source_excerpts']), source_ref('common-legacy')]


def test_native_forms_and_other_indivisible_scopes_remain_intact():
    native = task({'kind': 'form', 'template_refs': [{'source_ref': source_ref('grid'), 'selectors': []}],
                   'rule_refs': [rule('a'), rule('b')], 'source_excerpts': [excerpt('a', 'r1'), excerpt('b', 'r2')]})
    before = deepcopy(native)
    assert split_writing_task(native) == [] and native == before
    assert split_writing_task(task({'rule_refs': [rule('only')]})) == []
    assert split_writing_task(task({'source_refs': [source_ref('whole-a'), source_ref('whole-b')]})) == []
    assert split_writing_task(task({'source_excerpts': [excerpt('a', 'r1', 'r2')]})) == []
    assert split_writing_task({**native, 'targets': native['targets'] * 2}) == []
