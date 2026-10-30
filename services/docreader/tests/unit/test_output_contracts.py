"""Generation describes semantic responses, leaving host identity and recovery alone."""
from copy import deepcopy
from importlib.resources import files
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


def test_source_outline_request_explains_rule_and_source_reference_domains():
    from docreader.tender.schema import OutlineResponse

    payload = {'schema_version': 2, 'rules': [{'rule_id': 'opaque-rule-identity'}],
               'related_rules': [{'item': {'item_id': 'opaque-item-identity'}}],
               'sources': [{'range_id': 'opaque-range-identity', 'text': '本批实际原文'}]}
    prompt = files('docreader.tender.prompts').joinpath('outline.md').read_text('utf-8')
    request = build_request('configured-model', prompt, payload, task='outline')
    schema = sent_schema(request)
    target = schema['$defs']['OutlineTargetResponse']
    finding = schema['$defs']['OutlineFinding']
    rule_fields = [target['properties']['rule_ids'], finding['properties']['rule_ids'],
                   schema['properties']['constraint_rule_ids']]
    for field in rule_fields:
        description = field['description']
        assert 'top-level rules[].rule_id' in description
        assert 'related_rules' in description and 'item_id' in description
        assert 'read-only' in description and 'prefix' in description
    target_sources = target['properties']['range_ids']
    assert 'bindable' in target_sources['description']
    assert 'evidence/context' in target_sources['description']
    constraint_sources = schema['properties']['constraint_range_ids']
    assert 'sources[].range_id' in constraint_sources['description']
    assert 'read in this batch' in constraint_sources['description']
    assert 'evidence' in constraint_sources['description']
    assert {'rule_ids', 'range_ids'} <= set(target['required'])
    assert 'rule_ids' in finding['required']
    assert not {'constraint_rule_ids', 'constraint_range_ids'} & set(schema['required'])
    defaults = OutlineResponse.model_validate({'schema_version': 2, 'chapters': [], 'findings': []})
    assert defaults.constraint_rule_ids == defaults.constraint_range_ids == []
    for field in [*rule_fields, target_sources, constraint_sources]:
        assert field['type'] == 'array' and field['items']['type'] == 'string'
        assert 'enum' not in field['items']
    assert all(value not in json.dumps(schema, ensure_ascii=False)
               for value in ['opaque-rule-identity', 'opaque-item-identity',
                             'opaque-range-identity', '本批实际原文'])
    prompt = request['messages'][0]['content'].split('\n\n返回JSON须符合以下JSON Schema；')[0]
    reference_domain = next(paragraph for paragraph in prompt.split('\n\n')
                            if paragraph.startswith('引用域：'))
    for term in ['rules[].rule_id', 'related_rules.item.item_id', '只读', '前缀',
                 'range_ids', '证据上下文', 'constraint_range_ids', 'sources[].range_id', '实际阅读']:
        assert term in reference_domain


@pytest.mark.parametrize('task,payload,expected', [
    ('outline', {'schema_version': 2}, 'OutlineResponse'),
    ('outline', {'operation': 'assemble'}, 'OutlineAssemblyResponse'),
    ('authoring_basis', {}, 'AuthoringBasisResponse'),
    ('authoring_write', {'previous_content': {}}, 'WritingResponse'),
    ('authoring_review', {}, 'AuthoringReviewResponse'),
    ('authoring_review', {'operation': 'review_saved_word'}, 'AuthoringReviewResponse'),
    ('authoring_structure', {'operation': 'structure', 'phase': 'outline'}, 'AuthoringStructureResponse'),
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
