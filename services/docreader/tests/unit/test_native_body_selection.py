from copy import deepcopy
import itertools
import json

import pytest

from docreader.llm.budget import RequestBudget
from docreader.llm.request import ModelContractError, build_request
from docreader.tender.native_body_selection import NativeBodySelectionView, plan_native_body_packs


def view():
    return NativeBodySelectionView([
        {'cell_id': 1, 'document_id': 'doc1', 'table_id': 't', 'row': 0, 'column': 0, 'text': '评分名称', 'row_span': 2},
        {'cell_id': 2, 'document_id': 'doc1', 'table_id': 't', 'row': 0, 'column': 1, 'text': '8'},
        {'cell_id': 3, 'document_id': 'doc1', 'table_id': 't', 'row': 0, 'column': 2,
         'text': '第一项软\n换行。  第二项；第三项尾句。'},
        {'cell_id': 4, 'document_id': 'doc1', 'table_id': 't', 'row': 1, 'column': 2, 'text': '相邻独立条目。'},
    ], [{'unit_id': 7, 'target_id': 'unchanged-target',
         'islands': [{'island_id': 2, 'cell_ids': [1, 2, 3, 4]}]}])


def reply(body=None, state='selected'):
    return {'native_choices': [{'unit_id': 7, 'islands': [
        {'island_id': 2, 'state': state, 'body': [[3, 2, 3, 0]] if body is None else body}]}]}


def test_exact_tail_and_context_keep_source_whitespace_without_next_row():
    v = view()
    original = deepcopy(v.payload())
    result = v.accept(reply())
    unit = result['units'][0]
    assert unit['islands'][0]['body'] == [
        {'cell_id': 3, 'start': 10, 'end': 20, 'text': '第二项；第三项尾句。', 'source_join': 'paragraphs'}]
    assert unit['context'] == [
        {'cell_id': 1, 'start': 0, 'end': 4, 'text': '评分名称'},
        {'cell_id': 2, 'start': 0, 'end': 1, 'text': '8'},
        {'cell_id': 3, 'start': 0, 'end': 10, 'text': '第一项软\n换行。  '}]
    assert v.payload() == original
    assert result['complete'] and not result['pending']


@pytest.mark.parametrize('mutation', [
    'unknown_unit', 'duplicate_unit', 'missing_unit', 'unknown_island', 'duplicate_island',
    'missing_island', 'unknown_cell', 'outside_island', 'duplicate_cell', 'reverse_interval',
    'zero_boundary', 'outside_boundary', 'bool_id', 'empty_selected', 'body_unresolved',
    'extra_text', 'reversed_cells',
])
def test_rejects_invalid_or_unbounded_selection(mutation):
    v = view()
    raw = reply()
    choice = raw['native_choices'][0]
    island = choice['islands'][0]
    if mutation == 'unknown_unit': choice['unit_id'] = 8
    if mutation == 'duplicate_unit': raw['native_choices'].append(deepcopy(choice))
    if mutation == 'missing_unit': raw['native_choices'] = []
    if mutation == 'unknown_island': island['island_id'] = 3
    if mutation == 'duplicate_island': choice['islands'].append(deepcopy(island))
    if mutation == 'missing_island': choice['islands'] = []
    if mutation == 'unknown_cell': island['body'] = [[99, 1, 1, 0]]
    if mutation == 'outside_island':
        v = NativeBodySelectionView(v.cells, [{'unit_id': 7, 'islands': [{'island_id': 2, 'cell_ids': [1]}]}])
    if mutation == 'duplicate_cell': island['body'].append([3, 1, 1, 0])
    if mutation == 'reverse_interval': island['body'] = [[3, 3, 2, 0]]
    if mutation == 'zero_boundary': island['body'] = [[3, 0, 1, 0]]
    if mutation == 'outside_boundary': island['body'] = [[3, 1, 4, 0]]
    if mutation == 'bool_id': island['body'] = [[True, 1, 1, 0]]
    if mutation == 'empty_selected': island['body'] = []
    if mutation == 'body_unresolved': island['state'] = 'unresolved'
    if mutation == 'extra_text': island['source_text'] = 'model copies source'
    if mutation == 'reversed_cells': island['body'] = [[3, 1, 1, 0], [2, 1, 1, 0]]
    with pytest.raises(ModelContractError):
        v.accept(raw)


def test_multiple_islands_continue_without_body_context_overlap():
    v = view()
    v = NativeBodySelectionView(v.cells, [{'unit_id': 7, 'islands': [
        {'island_id': 2, 'cell_ids': [3]}, {'island_id': 5, 'cell_ids': [3]}]}])
    raw = reply([[3, 1, 1, 0]])
    raw['native_choices'][0]['islands'].append({'island_id': 5, 'state': 'selected', 'body': [[3, 3, 3, 1]]})
    result = v.accept(raw)
    assert [x['text'] for x in result['units'][0]['context'] if x['cell_id'] == 3] == ['第二项；']
    raw['native_choices'][0]['islands'][1]['body'] = [[3, 1, 2, 1]]
    with pytest.raises(ModelContractError): v.accept(raw)


def test_unresolved_preserves_unit_and_explicit_manual_exit():
    v = view()
    result = v.accept(reply([], state='unresolved'))
    assert not result['complete']
    assert result['pending'] == [{'unit_id': 7, 'island_id': 2, 'reason': 'native_body_unresolved',
                                  'outlet': 'manual_source_review', 'enterprise_confirmed': False}]
    assert v.payload()['units'][0]['target_id'] == 'unchanged-target'


def test_conservative_output_bound_covers_every_small_legal_selection():
    v = view()
    options = [None, *itertools.combinations_with_replacement([1, 2, 3], 2)]
    size = v.output_bound()['token_upper_bound']
    for a, b in itertools.product([False, True], options):
        body = ([[1, 1, 1, 0]] if a else []) + ([[3, *b, 0]] if b else [])
        raw = reply(body, state='selected' if body else 'unresolved')
        v.accept(raw)
        canonical = json.dumps(raw, ensure_ascii=False, sort_keys=True, separators=(',', ':'))
        assert len(canonical.encode()) <= size
    assert v.output_bound()['method'] == 'canonical_json_utf8_bytes_conservative_superset_v1'


def request(v):
    return build_request('test', 'Select supplied source IDs.', {'view': v.payload(), 'schema': v.response_schema()})


def many_units(n, text='原文。'):
    return NativeBodySelectionView([
        {'cell_id': i, 'document_id': 'doc1', 'table_id': 't', 'row': i, 'column': 0, 'text': text} for i in range(1, n + 1)
    ], [{'unit_id': i, 'islands': [{'island_id': 1, 'cell_ids': [i]}]} for i in range(1, n + 1)])


def test_packing_checks_full_request_and_output_separately():
    v = many_units(8)
    budget = RequestBudget(16384, model='test', prompts={}, output_reserve=230)
    planned = plan_native_body_packs(v, budget, request, task='directory_parameter')
    assert planned['complete'] and len(planned['packs']) > 1
    assert [u['unit_id'] for p in planned['packs'] for u in p['view'].units] == list(range(1, 9))
    for p in planned['packs']:
        assert p['budget']['fits'] is True
        assert p['output_bound']['token_upper_bound'] <= p['budget']['output_reserve']
        assert p['request'] == request(p['view'])
    assert planned['model_calls'] == 0


def test_single_oversized_unit_keeps_complete_source_and_named_pending():
    v = many_units(1, '不裁切完整原文' * 10000)
    budget = RequestBudget(4096, model='test', prompts={}, output_reserve=512)
    planned = plan_native_body_packs(v, budget, request, task='directory_parameter')
    assert not planned['complete'] and not planned['packs']
    pending = planned['pending_units'][0]
    assert pending['unit_id'] == 1 and pending['reason'] == 'complete_native_body_unit_over_budget'
    assert pending['outlet'] == 'manual_source_review' and not pending['enterprise_confirmed']
    assert v.payload()['cells'][0]['text'] == '不裁切完整原文' * 10000


def test_rejects_cross_island_reversal_to_an_earlier_cell():
    base = view()
    v = NativeBodySelectionView(base.cells, [{'unit_id': 7, 'islands': [
        {'island_id': 2, 'cell_ids': [2, 3]}, {'island_id': 5, 'cell_ids': [2, 3]}]}])
    raw = reply([[3, 1, 1, 0]])
    raw['native_choices'][0]['islands'].append({'island_id': 5, 'state': 'selected', 'body': [[2, 1, 1, 0]]})
    with pytest.raises(ModelContractError):
        v.accept(raw)


def test_merged_row_context_is_carried_without_unrelated_score_row():
    result = view().accept(reply([[4, 1, 1, 0]]))
    assert result['units'][0]['context'] == [{'cell_id': 1, 'start': 0, 'end': 4, 'text': '评分名称'}]


def test_packing_splits_input_while_output_capacity_alone_would_fit():
    v = many_units(6, '完整原字' * 60)
    cells = [{**cell, 'table_id': cell['cell_id']} for cell in v.cells]
    v = NativeBodySelectionView(cells, v.units)
    budget = RequestBudget(8192, model='test', prompts={}, output_reserve=1024)
    assert v.output_bound()['token_upper_bound'] <= 1024
    planned = plan_native_body_packs(v, budget, request, task='directory_parameter')
    assert planned['complete'] and len(planned['packs']) > 1
    assert [u['unit_id'] for p in planned['packs'] for u in p['view'].units] == list(range(1, 7))
    assert all(p['budget']['fits'] is True for p in planned['packs'])


def test_blank_native_cell_cannot_claim_a_selected_body():
    v = NativeBodySelectionView([
        {'cell_id': 1, 'document_id': 'doc1', 'table_id': 't', 'row': 0, 'column': 0, 'text': '   \n'}
    ], [{'unit_id': 7, 'islands': [{'island_id': 2, 'cell_ids': [1]}]}])
    with pytest.raises(ModelContractError):
        v.accept(reply([[1, 1, 1, 0]]))
    assert not v.accept(reply([], state='unresolved'))['complete']


@pytest.mark.parametrize('kind', ['first_continuation', 'unknown_join', 'bool_join', 'missing_join',
                                  'cross_document', 'missing_document'])
def test_join_must_be_explicit_and_continuation_must_have_same_document(kind):
    base = view()
    cells = base.cells
    if kind == 'cross_document': cells[3]['document_id'] = 'doc2'
    if kind == 'missing_document': cells[3].pop('document_id')
    v = NativeBodySelectionView(cells, base.units)
    raw = reply([[3, 1, 1, 0], [4, 1, 1, 1]])
    if kind == 'first_continuation': raw['native_choices'][0]['islands'][0]['body'][0][3] = 1
    if kind == 'unknown_join': raw['native_choices'][0]['islands'][0]['body'][0][3] = 2
    if kind == 'bool_join': raw['native_choices'][0]['islands'][0]['body'][0][3] = False
    if kind == 'missing_join': raw['native_choices'][0]['islands'][0]['body'][0].pop()
    with pytest.raises(ModelContractError): v.accept(raw)


def test_explicit_continuation_survives_materialization_without_guessing():
    result = view().accept(reply([[3, 3, 3, 0], [4, 1, 1, 1]]))
    body = result['units'][0]['islands'][0]['body']
    assert [p['source_join'] for p in body] == ['paragraphs', 'continuation']
    assert [p['text'] for p in body] == ['第三项尾句。', '相邻独立条目。']


def test_invalid_r_response_does_not_copy_an_unbounded_output_into_correction_feedback():
    with pytest.raises(ModelContractError) as rejected:
        view().accept({'copied_source': '原文' * 100000})
    assert rejected.value.feedback_fragments == []
    assert len(json.dumps(rejected.value.errors, ensure_ascii=False).encode()) < 512


def test_packer_preserves_proven_space_for_correction_input():
    from docreader.tender.native_body_selection import CORRECTION_HEADROOM_TOKENS
    v = many_units(1)
    b = RequestBudget(8192, model='test', prompts={}, output_reserve=512)
    planned = plan_native_body_packs(v, b, request, task='directory_parameter')
    assert planned['complete']
    pack = planned['packs'][0]
    assert pack['correction_input_upper_bound_tokens'] == pack['budget']['input_tokens'] + CORRECTION_HEADROOM_TOKENS
    assert pack['correction_input_upper_bound_tokens'] <= pack['budget']['effective_input_budget_tokens']
