"""Generation describes semantic responses, leaving host identity and recovery alone."""
from copy import deepcopy
import json

import pytest
from pydantic import ValidationError

from docreader.llm.request import build_request
from docreader.tender.schema import AnalysisBatchResponse, AnalysisExtractionResponse


def sent_schema(request):
    return json.loads(request['messages'][0]['content'].splitlines()[-1])


def test_schema_is_sent_once_without_rewriting_payload_or_exposing_source_ids():
    payload = {'operation': 'assemble', 'chapters': [], 'source_excerpts': [
        {'range_id': 'opaque-source-identity', 'text': '必要原文'}]}
    original = deepcopy(payload)
    request = build_request('configured-model', '按实际原文组织目录。', payload, task='outline')
    schema = sent_schema(request)
    assert 'chapters' in schema['required']
    assert json.loads(request['messages'][1]['content'][0]['text']) == original
    assert payload == original
    assert 'opaque-source-identity' not in json.dumps(schema)
    assert '必要原文' not in json.dumps(schema, ensure_ascii=False)
    assert request['response_format'] == {'type': 'json_object'}
    assert request['stream'] and request['stream_options'] == {'include_usage': True}
    assert request['messages'][0]['content'].count('"$defs"') == 1


@pytest.mark.parametrize('task,payload,expected', [
    ('outline', {'schema_version': 2}, 'OutlineResponse'),
    ('outline', {'operation': 'assemble'}, 'OutlineAssemblyResponse'),
    ('authoring_basis', {}, 'AuthoringBasisResponse'),
    ('authoring_write', {'previous_content': {}}, 'WritingResponse'),
    ('authoring_review', {}, 'AuthoringReviewResponse'),
    ('authoring_review', {'operation': 'review_saved_word'}, 'AuthoringReviewResponse'),
    ('authoring_structure', {'operation': 'structure', 'phase': 'outline'}, 'AuthoringReviewResponse'),
    ('verify', {'schema_version': 2, 'operation': 'locate'}, 'LocateResult'),
    ('verify', {'schema_version': 2, 'operation': 'associate'}, 'AssociationResult'),
    ('verify', {'schema_version': 2, 'operation': 'review'}, 'AnalysisReviewResponse'),
    ('ocr', {}, 'OCRResponse'),
    ('plan', {}, 'Plan'),
    ('extract', {}, 'Extraction'),
    ('verify', {}, 'Verification'),
])
def test_each_operation_uses_its_existing_semantic_response(task, payload, expected):
    request = build_request('configured-model', '遵循原文。', payload, task=task)
    assert sent_schema(request)['title'] == expected


def test_generation_is_typed_while_a_bad_extraction_sibling_remains_recoverable():
    from docreader.llm.contracts import output_schema, response_model
    payload = {'schema_version': 2}
    schema = output_schema('extract', payload)
    assert schema['properties']['items']['items']['$ref'].endswith('/TenderItem')
    disposition = schema['$defs']['ItemDispositionOutput']
    assert 'item_ids' not in disposition['properties']
    assert disposition['properties']['status']['enum'] == ['non_item', 'unresolved']
    assert 'blocking' in schema['$defs']['AnalysisIssue']['required']
    valid = {'item_id': 'p1:i1', 'kind': 'obligation', 'text': '保留完整要求。',
             'evidence': [{'range_id': 'r1', 'quote': '完整要求'}]}
    raw = {'schema_version': 2, 'items': [valid, {'text': '坏的兄弟项'}],
           'dispositions': [], 'relations': [], 'issues': []}
    original = deepcopy(raw)
    assert AnalysisExtractionResponse.model_validate(raw).items == original['items']
    with pytest.raises(ValidationError):
        response_model('extract', payload).model_validate(raw)
    assert raw == original


@pytest.mark.parametrize('operation,fields', [
    ('review', {'accepted_decision_ids': [], 'findings': []}),
    ('associate', {'relations': [], 'decisions': [], 'findings': []}),
])
def test_batch_members_have_operation_fields_without_host_review_coverage(operation, fields):
    from docreader.llm.contracts import output_schema, response_model
    payload = {'schema_version': 2, 'operation': operation, 'work_units': []}
    schema = output_schema('verify', payload)
    member_ref = schema['properties']['work_units']['items']['$ref'].split('/')[-1]
    member = schema['$defs'][member_ref]
    assert set(member['properties']) == {'work_unit_id', *fields}
    assert not any(name.startswith('reviewed_') for name in member['properties'])
    raw = {'schema_version': 2, 'work_units': [{'work_unit_id': 'w1', **fields}], 'shared_findings': []}
    assert response_model('verify', payload).model_validate(raw).model_dump(exclude_unset=True) == raw
    damaged = {**raw, 'work_units': [raw['work_units'][0], {'unexpected': 'member'}]}
    assert AnalysisBatchResponse.model_validate(damaged).work_units == damaged['work_units']


def test_contract_feedback_does_not_change_the_selected_return_schema():
    from docreader.llm.contracts import output_schema
    initial = {'schema_version': 2, 'operation': 'review', 'work_units': []}
    assert output_schema('verify', initial) == output_schema('verify', {
        **initial, 'contract_feedback': {'errors': [{'code': 'missing'}]}})


def test_generic_request_without_a_tender_task_keeps_its_existing_contract():
    request = build_request('configured-model', '独立调用', {'value': 1})
    assert request['messages'][0]['content'] == '独立调用'


def test_execution_spec_pins_generated_contracts_as_well_as_prompt_files(monkeypatch):
    from docreader.llm.budget import RequestBudget
    from docreader.llm.contracts import _schema_text
    from docreader.tender.schema import OutlineResponse
    budget = RequestBudget(prompts={})
    before = budget.spec()
    assert 'output_contracts' in before
    original = OutlineResponse.model_json_schema
    monkeypatch.setattr(OutlineResponse, 'model_json_schema', lambda **kwargs: {
        **original(**kwargs), 'description': '字段合同发生了真实变化。'})
    _schema_text.cache_clear()
    try:
        assert budget.spec()['output_contracts'] != before['output_contracts']
    finally:
        _schema_text.cache_clear()
