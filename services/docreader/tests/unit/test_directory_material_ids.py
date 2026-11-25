"""Request-local material identities must preserve sources and receipt ownership."""
from typing import Any, cast

from copy import deepcopy
import json

import pytest

from docreader.llm.budget import RequestBudget
from docreader.llm.request import ModelContractError
from docreader.tender.authoring_pipeline import _ask, _request


def payload():
    return {
        'documents': [{'document_id': 'source-long-id', 'pages': [
            {'page': 0, 'lines': [[0, 'source-long-id c1 \n'], [1, ''], [2, '原文：________']]}]}],
        'source_line_format': 'numbered_line_pairs_v1',
        'chapters': [{'id': 'chapter-long-id', 'parent_id': None, 'title': 'chapter-long-id'}],
        'applicability': {'conditions': [{'chapter_id': 'chapter-long-id', 'text': 'chapter-long-id'}]},
        'unplaced_tables': [],
        'table_header_catalog': [{'table_id': 'table-long-id', 'document_id': 'source-long-id',
            'grid': {'rows': [['chapter-long-id', '', 'c1']], 'id': 'native-table-id',
                     'document_id': 'native-data-must-not-change'}}],
        'chapter_title_catalog': [{'title_id': 'title-long-id',
            'heading': {'document_id': 'source-long-id', 'page': 0, 'line': 0}}],
        'directory_cases': [{'case_id': 'case-long-id', 'current': {
            'id': 'chapter-long-id', 'parent_id': None}, 'alternative': {
            'id': 'other-chapter-long-id', 'parent_id': 'chapter-long-id'}}],
    }


def response():
    return {'assignments': [{'chapter_id': 'c1', 'document_id': 'd1',
                            'page': 0, 'start_line': 2, 'end_line': 2}],
            'table_headers': [{'table_id': 't1', 'count': 0}],
            'directory_adjudications': [{'case_id': 'q1', 'action': 'reference',
                'chapter_ids': ['c1'], 'basis_windows': [
                    {'document_id': 'd1', 'page': 0, 'start_line': 0, 'end_line': 2}]}],
            'chapter_titles': [{'title_id': 'n1', 'title': '完整业务名称 c1'}]}


async def test_material_dispatch_uses_short_ids_and_validates_the_restored_response():
    original = payload()
    before = deepcopy(original)
    checked = []

    def validate(value):
        checked.append(value)
        assert value['assignments'][0]['chapter_id'] == 'chapter-long-id'
        assert value['assignments'][0]['document_id'] == 'source-long-id'
        assert value['table_headers'][0]['table_id'] == 'table-long-id'
        assert value['directory_adjudications'][0]['case_id'] == 'case-long-id'
        assert value['directory_adjudications'][0]['chapter_ids'] == ['chapter-long-id']
        assert value['chapter_titles'][0] == {'title_id': 'title-long-id', 'title': '完整业务名称 c1'}
        return value

    async def execute(task, request, accept):
        sent = json.loads(request['messages'][1]['content'][0]['text'])
        assert task == 'directory_material'
        assert sent.get('identity_format') == 'material_short_ids_v2'
        assert sent['chapters'][0]['id'] == 'c1'
        assert sent['documents'][0]['document_id'] == 'd1'
        assert sent['documents'][0]['pages'] == before['documents'][0]['pages']
        assert sent['table_header_catalog'][0]['grid'] == before['table_header_catalog'][0]['grid']
        return accept(response())

    result = await _ask(execute, RequestBudget(model='fixture'), 'directory_material', original, validate)
    assert checked == [result] and original == before


def test_id_projection_is_deterministic_lossless_and_bound_to_the_permanent_mapping():
    from docreader.tender.directory_material_ids import MaterialIds

    original = payload()
    one, two = MaterialIds(original), MaterialIds(deepcopy(original))
    assert one.payload == two.payload
    assert len(one.payload['identity_binding']) == 64
    assert one.payload['chapters'][0]['title'] == 'chapter-long-id'
    assert one.payload['applicability']['conditions'][0]['text'] == 'chapter-long-id'
    assert one.payload['directory_cases'][0]['alternative']['parent_id'] == 'c1'
    changed = deepcopy(original)
    changed['documents'][0]['document_id'] = 'another-source-long-id'
    assert MaterialIds(changed).payload['identity_binding'] != one.payload['identity_binding']
    assert MaterialIds(changed).payload['documents'][0]['document_id'] == 'd1'
    assert one.decode(response()) == two.decode(response())


@pytest.mark.parametrize('field,wrong', [
    ('chapter_id', 'd1'), ('chapter_id', 'c999'), ('chapter_id', 'chapter-long-id'),
    ('document_id', 'c1'), ('document_id', 'source-long-id'),
])
def test_material_response_rejects_wrong_namespace_unknown_or_permanent_ids(field, wrong):
    from docreader.tender.directory_material_ids import MaterialIds

    raw = response()
    raw['assignments'][0][field] = wrong
    with pytest.raises(ModelContractError) as caught:
        MaterialIds(payload()).decode(raw)
    assert caught.value.errors[0]['path'] == ['assignments', 0, field]


def test_same_permanent_literal_in_different_namespaces_remains_distinct():
    from docreader.tender.directory_material_ids import MaterialIds

    original = payload()
    original['table_header_catalog'][0]['table_id'] = 'chapter-long-id'
    projection = MaterialIds(original)
    assert projection.payload['chapters'][0]['id'] == 'c1'
    assert projection.payload['table_header_catalog'][0]['table_id'] == 't1'
    restored = projection.decode(response())
    assert cast(Any, restored)['table_headers'][0]['table_id'] == cast(Any, restored)['assignments'][0]['chapter_id']


def test_material_source_coordinate_arrays_have_explicit_fields_and_restore_the_entire_payload():
    from docreader.tender.directory_material_ids import MaterialIds

    original = payload()
    original['directory_cases'][0]['current']['heading'] = {
        'document_id': 'source-long-id', 'page': 0, 'line': 0}
    original['directory_cases'][0]['current']['body_windows'] = [
        {'document_id': 'source-long-id', 'page': 0, 'start_line': 0, 'end_line': 2}]
    before = deepcopy(original)
    ids = MaterialIds(original)
    sent = ids.payload
    assert sent['source_coordinate_format'] == 'material_source_arrays_v1'
    assert sent['source_coordinate_fields'] == {
        'point': ['document_id', 'page', 'line'],
        'window': ['document_id', 'page', 'start_line', 'end_line']}
    assert sent['directory_cases'][0]['current']['heading'] == ['d1', 0, 0]
    assert sent['directory_cases'][0]['current']['body_windows'] == [['d1', 0, 0, 2]]
    assert sent['chapter_title_catalog'][0]['heading'] == ['d1', 0, 0]
    assert sent['documents'][0]['pages'] == before['documents'][0]['pages']
    assert sent['table_header_catalog'][0]['grid'] == before['table_header_catalog'][0]['grid']
    assert ids.restore_payload() == original == before


def test_material_binding_changes_if_original_coordinate_representation_changes():
    from docreader.tender.directory_material_ids import MaterialIds

    original = payload()
    already_array = deepcopy(original)
    already_array['chapter_title_catalog'][0]['heading'] = ['source-long-id', 0, 0]
    assert MaterialIds(original).payload['identity_binding'] != MaterialIds(already_array).payload['identity_binding']


def test_array_input_does_not_relax_the_response_window_schema():
    from docreader.tender.authoring_pipeline import _parse
    from docreader.tender.directory_compile import MaterialPlan
    from docreader.tender.directory_material_ids import MaterialIds

    raw = response()
    raw['directory_adjudications'][0]['basis_windows'] = [['d1', 0, 0, 2]]
    with pytest.raises(ModelContractError) as caught:
        MaterialIds(payload()).accept(lambda value: _parse(MaterialPlan, value))(raw)
    assert caught.value.errors[0]['path'] == ['directory_adjudications', 0, 'basis_windows', 0]


async def test_source_rejection_feedback_uses_only_the_model_visible_identities():
    def validate(_value):
        raise ModelContractError([{'path': ['assignments'], 'code': 'source_invalid',
            'detail': 'chapter chapter-long-id overlaps source-long-id and case-long-id'}])

    async def execute(_task, _request, accept):
        return accept(response())

    with pytest.raises(ModelContractError) as caught:
        await _ask(execute, RequestBudget(model='fixture'), 'directory_material', payload(), validate)
    assert caught.value.errors[0]['detail'] == 'chapter c1 overlaps d1 and q1'


async def test_exact_accepted_legacy_request_replays_before_compacting_or_sending():
    expected = {'assignments': []}

    class Execute:
        async def replay(self, task, request, validate):
            sent = json.loads(request['messages'][1]['content'][0]['text'])
            assert 'identity_format' not in sent
            return True, validate(deepcopy(expected))

        async def __call__(self, *_args):
            raise AssertionError('The exact legacy successful response must not be sent again.')

    assert await _ask(Execute(), RequestBudget(model='fixture'), 'directory_material', payload(), lambda x: x) == expected


async def test_short_response_receipt_replays_with_the_same_mapping_without_sending(tmp_path, monkeypatch):
    from docreader.integrations.artifacts import ArtifactStore
    import docreader.tender.document_execution as execution
    from test_directory_plan_replay import Receipts, executor

    artifacts, receipts = ArtifactStore(tmp_path), Receipts()
    budget, calls = RequestBudget(model='fixture'), []

    async def model(_model, request, validate):
        calls.append(request)
        return validate(response())

    monkeypatch.setattr(execution._DocumentModel, '__call__', model)
    runner = executor(artifacts, receipts, budget)
    first = await _ask(runner, budget, 'directory_material', payload(), lambda x: x)
    second = await _ask(runner, budget, 'directory_material', payload(), lambda x: x)
    assert first is not None
    assert first == second and first['assignments'][0]['chapter_id'] == 'chapter-long-id'
    assert len(calls) == len(receipts.rows) == 1


async def test_other_requests_keep_the_exact_existing_body():
    budget, original = RequestBudget(model='fixture'), payload()

    async def execute(task, request, validate):
        assert request == _request(budget, task, original)
        return validate({'items': []})

    assert await _ask(execute, budget, 'directory_classification', original, lambda x: x) == {'items': []}
