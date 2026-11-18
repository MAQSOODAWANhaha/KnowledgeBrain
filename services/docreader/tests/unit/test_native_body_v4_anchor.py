"""Independent v4 boundary cases: explicit prose roles and old-source coverage."""
from copy import deepcopy

import pytest

from docreader.llm.request import ModelContractError
from docreader.tender.native_body_selection import NativeBodySelectionView


def prose(text, start, end):
    return NativeBodySelectionView([
        {'cell_id': 1, 'source_kind': 'native_prose', 'document_id': 'doc',
         'native_unit_id': 'section', 'native_range_id': 'range', 'page': 0, 'text': text}
    ], [{'unit_id': 1, 'anchor_spans': [[1, start, end]],
         'islands': [{'island_id': 1, 'cell_ids': [1]}]}])


def response(body, context, state='selected'):
    return {'native_choices': [{'unit_id': 1, 'islands': [
        {'island_id': 1, 'state': state, 'body': body, 'context': context}]}]}


def test_explicit_context_covers_old_source_without_adding_the_next_independent_item():
    text = '来源说明。应提交正文。另行提交保函。'
    view = prose(text, 0, text.index('另行'))
    result = view.accept(response([[1, 2, 2, 0]], [[1, 1, 1]]))
    unit = result['units'][0]
    assert result['complete']
    assert [piece['text'] for piece in unit['islands'][0]['body']] == ['应提交正文。']
    assert [piece['text'] for piece in unit['context']] == ['来源说明。']
    assert all('保函' not in piece['text'] for piece in unit['context'])


def test_body_and_explicit_context_cannot_own_the_same_original_characters():
    text = '来源说明。应提交正文。'
    view = prose(text, text.index('应提交'), len(text))
    with pytest.raises(ModelContractError) as rejected:
        view.accept(response([[1, 2, 2, 0]], [[1, 2, 2]]))
    assert rejected.value.feedback_fragments == []


@pytest.mark.parametrize('context', [
    [[1, 1, 1], [1, 1, 1]],
    [[1, 3, 3], [1, 1, 1]],
    [[1, 1, 1], [1, 3, 3], [1, 4, 4]],
], ids=['duplicate', 'reverse_source_order', 'more_than_two_intervals'])
def test_context_intervals_are_finite_unique_and_in_original_order(context):
    text = '来源说明。应提交正文。相关条件。另一独立事项。'
    view = prose(text, 0, text.index('相关'))
    with pytest.raises(ModelContractError) as rejected:
        view.accept(response([[1, 2, 2, 0]], context))
    assert rejected.value.feedback_fragments == []


def test_unresolved_unit_keeps_both_roles_empty_without_claiming_anchor_coverage():
    text = '尚待核定的原要求。'
    view = prose(text, 0, len(text))
    raw = response([], [], state='unresolved')
    result = view.accept(raw)
    assert not result['complete'] and result['pending']
    assert result['units'][0]['context'] == []
    invalid = deepcopy(raw)
    invalid['native_choices'][0]['islands'][0]['context'] = [[1, 1, 1]]
    with pytest.raises(ModelContractError):
        view.accept(invalid)


def test_anchor_coverage_ignores_native_whitespace_but_never_original_unicode_glyphs():
    text = '甲。 \n乙🙂。  丙𠀀。'
    start, body_end = text.index('。') + 1, text.index('丙')
    selected = response([[1, 2, 2, 0]], [])
    result = prose(text, start, body_end).accept(selected)
    assert result['complete'] and result['units'][0]['context'] == []
    with pytest.raises(ModelContractError) as rejected:
        prose(text, start, len(text)).accept(selected)
    assert rejected.value.feedback_fragments == []


def test_grid_context_remains_host_owned_and_cannot_be_replaced_by_model_context():
    view = NativeBodySelectionView([
        {'cell_id': 1, 'document_id': 'doc', 'table_id': 1, 'row': 0, 'column': 0, 'text': '评分名称'},
        {'cell_id': 2, 'document_id': 'doc', 'table_id': 1, 'row': 0, 'column': 1, 'text': '应提交正文。'},
    ], [{'unit_id': 1, 'islands': [{'island_id': 1, 'cell_ids': [1, 2]}]}])
    raw = {'native_choices': [{'unit_id': 1, 'islands': [
        {'island_id': 1, 'state': 'selected', 'body': [[2, 1, 1, 0]]}]}]}
    result = view.accept(raw)
    assert [piece['text'] for piece in result['units'][0]['context']] == ['评分名称']
    raw['native_choices'][0]['islands'][0]['context'] = []
    with pytest.raises(ModelContractError):
        view.accept(raw)
