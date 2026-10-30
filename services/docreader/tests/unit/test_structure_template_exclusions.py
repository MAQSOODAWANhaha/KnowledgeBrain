"""Structure review may exclude exact standalone page decorations only."""
from copy import deepcopy
import json

import pytest

from docreader.llm.budget import RequestBudget
from docreader.llm.contracts import output_schema
from docreader.llm.request import ModelContractError, ModelWorkUnitExhausted
from docreader.tender.authoring_structure import review_structure
from docreader.tender.evidence.references import selector
from test_authoring_templates import create_context, create_source, make_part


def fixture(tmp_path):
    artifacts, index_ref, native = create_source(tmp_path, [
        {'text': 'Repeated fixed title'}, {'text': 'Repeated fixed title'},
        {'text': 'Required form body, conditional note, and signature'}, {'text': 'Source context only'}])
    context = create_context(artifacts, index_ref, native)
    parts = [make_part(index_ref, f'unit:{i}') for i in range(4)]
    excerpt = {'source_ref': parts[3]['source_ref'], 'selectors': [selector(native.ranges[3])]}
    def target(identity, refs):
        return {'target_id': identity, 'kind': 'form', 'purpose': 'Synthetic form instance',
                'rule_refs': [], 'source_refs': [p['source_ref'] for p in parts],
                'template_refs': refs, 'source_excerpts': [excerpt]}
    outline = {'authoring_input': context.input, 'chapters': [{'chapter_id': 'chapter',
        'title': 'Forms', 'targets': [target('target-a', parts[:3]), target('target-b', parts[:1])],
        'children': []}], 'issues': [], 'constraint_rule_refs': [], 'submission_refs': []}
    return artifacts, outline, parts


def request_payload(request):
    return json.loads(request['messages'][1]['content'][0]['text'])


def action_response(payload, **extra):
    target = payload['chapters'][0]['targets'][0]
    return {'findings': [], 'source_decoration_exclusions': [{
        'target_id': target['target_id'], 'support_ids': target['template_support_ids'][:1],
        'reason': 'This exact part is a fixed source page title.'}], **extra}


@pytest.mark.parametrize('outline_phase', [True, False])
async def test_exact_exclusion_keeps_same_text_other_ref_shared_instance_and_source_context(tmp_path, outline_phase):
    artifacts, outline, parts = fixture(tmp_path)
    before = deepcopy(outline)
    async def execute(task, request, validate):
        payload = request_payload(request)
        assert payload['source_decoration_edits_allowed'] is True
        raw = action_response(payload)
        raw['source_decoration_exclusions'][0]['support_ids'] *= 2
        raw['source_decoration_exclusions'] *= 2
        return validate(raw)
    assert await review_structure(artifacts, outline, [], RequestBudget(model='fixture', context_tokens=131072),
        execute, outline_phase=outline_phase, apply_source_decoration_exclusions=True) == ([], [])
    targets = outline['chapters'][0]['targets']
    assert targets[0]['template_refs'] == parts[1:3]
    assert targets[1] == before['chapters'][0]['targets'][1]
    for field in ('source_refs', 'source_excerpts', 'rule_refs'):
        assert targets[0][field] == before['chapters'][0]['targets'][0][field]


@pytest.mark.parametrize('bad', ['source-only', 'other-target', 'unknown-target'])
async def test_invalid_action_after_valid_action_is_feedback_and_never_mutates_outline(tmp_path, bad):
    artifacts, outline, _ = fixture(tmp_path)
    before = deepcopy(outline)
    async def execute(task, request, validate):
        payload = request_payload(request)
        raw = action_response(payload)
        first, second = payload['chapters'][0]['targets']
        if bad == 'source-only':
            invalid = {'target_id': first['target_id'], 'support_ids': first['source_support_ids'][:1]}
            expected_path = ['source_decoration_exclusions', 1, 'support_ids', 0]
        elif bad == 'other-target':
            invalid = {'target_id': second['target_id'], 'support_ids': first['template_support_ids'][1:2]}
            expected_path = ['source_decoration_exclusions', 1, 'support_ids', 0]
        else:
            invalid = {'target_id': 'v999', 'support_ids': first['template_support_ids'][:1]}
            expected_path = ['source_decoration_exclusions', 1, 'target_id']
        raw['source_decoration_exclusions'].append({**invalid, 'reason': 'Synthetic invalid action.'})
        with pytest.raises(ModelContractError) as error:
            validate(raw)
        assert error.value.errors[0]['path'] == expected_path
        assert error.value.errors[0]['code'] == 'structure_unknown_reference'
        assert outline == before
        return validate({'findings': []})
    assert await review_structure(artifacts, outline, [], RequestBudget(model='fixture', context_tokens=131072),
        execute, outline_phase=True, apply_source_decoration_exclusions=True) == ([], [])
    assert outline == before


@pytest.mark.parametrize('compact', [False, True])
async def test_accepted_replay_applies_exclusion_without_fresh_execute_or_validation_side_effects(tmp_path, compact):
    artifacts, outline, parts = fixture(tmp_path)
    before = deepcopy(outline)
    class Executor:
        async def replay(self, task, request, validate):
            payload = request_payload(request)
            if compact and 'structure_view' not in payload:
                return False, None
            value = validate(action_response(payload))
            assert outline == before
            return True, value
        async def __call__(self, *unused):
            raise AssertionError('Accepted replay must not send a model request')
    budget = RequestBudget(model='fixture', context_tokens=2500 if compact else 131072)
    assert await review_structure(artifacts, outline, [], budget, Executor(), outline_phase=True,
        apply_source_decoration_exclusions=True) == ([], [])
    assert outline['chapters'][0]['targets'][0]['template_refs'] == parts[1:3]


@pytest.mark.parametrize('raw', [{'findings': []}, {'findings': [], 'source_decoration_exclusions': []}])
async def test_no_explicit_actions_do_not_change_selection(tmp_path, raw):
    artifacts, outline, _ = fixture(tmp_path)
    before = deepcopy(outline)
    async def execute(task, request, validate): return validate(deepcopy(raw))
    assert await review_structure(artifacts, outline, [], RequestBudget(model='fixture', context_tokens=131072),
        execute, outline_phase=True, apply_source_decoration_exclusions=True) == ([], [])
    assert outline == before


async def test_fixed_outline_default_is_read_only_and_retains_action_as_diagnostic(tmp_path):
    artifacts, outline, _ = fixture(tmp_path)
    before = deepcopy(outline)
    async def execute(task, request, validate):
        payload = request_payload(request)
        assert 'source_decoration_edits_allowed' not in payload
        return validate(action_response(payload))
    findings, gaps = await review_structure(artifacts, outline, [],
        RequestBudget(model='fixture', context_tokens=131072), execute, outline_phase=True)
    assert findings == []
    assert any(g['code'] == 'structure_source_decoration_exclusion_not_applied' for g in gaps)
    assert outline == before


def test_structure_only_contract_exposes_exact_optional_actions():
    structure = output_schema('authoring_structure', {'operation': 'structure'})
    body = output_schema('authoring_review', {})
    assert structure['title'] == 'AuthoringStructureResponse'
    assert 'source_decoration_exclusions' in structure['properties']
    assert 'source_decoration_exclusions' not in structure['required']
    assert 'source_decoration_exclusions' not in body['properties']


async def test_legacy_broad_exclusion_is_rejected_without_removing_substantive_parts(tmp_path):
    artifacts, outline, _ = fixture(tmp_path)
    before = deepcopy(outline)
    async def execute(task, request, validate):
        payload = request_payload(request)
        target = payload['chapters'][0]['targets'][0]
        with pytest.raises(ModelContractError) as error:
            validate({'findings': [], 'template_exclusions': [{
                'target_id': target['target_id'],
                'support_ids': target['template_support_ids'][2:3],
                'reason': 'This substantive condition is not applicable to this project.'}]})
        assert error.value.errors[0]['path'] == ['template_exclusions']
        assert error.value.errors[0]['code'] == 'extra_forbidden'
        return validate({'findings': []})
    assert await review_structure(artifacts, outline, [],
        RequestBudget(model='fixture', context_tokens=131072), execute,
        outline_phase=True) == ([], [])
    assert outline == before


async def test_executor_failure_after_validation_keeps_original_selection(tmp_path):
    artifacts, outline, _ = fixture(tmp_path)
    before = deepcopy(outline)
    async def execute(task, request, validate):
        validate(action_response(request_payload(request)))
        assert outline == before
        raise ModelWorkUnitExhausted(task)
    findings, gaps = await review_structure(artifacts, outline, [],
        RequestBudget(model='fixture', context_tokens=131072), execute, outline_phase=True,
        apply_source_decoration_exclusions=True)
    assert findings == [] and [g['code'] for g in gaps] == ['structure_review_unavailable']
    assert outline == before


async def test_action_application_retains_original_model_finding(tmp_path):
    artifacts, outline, parts = fixture(tmp_path)
    async def execute(task, request, validate):
        return validate(action_response(request_payload(request), findings=[{
            'code': 'explicit_observation', 'detail': 'Synthetic observation stays visible.',
            'suggestion': 'Synthetic follow-up stays visible.', 'can_revise': True}]))
    findings, gaps = await review_structure(artifacts, outline, [],
        RequestBudget(model='fixture', context_tokens=131072), execute, outline_phase=True,
        apply_source_decoration_exclusions=True)
    assert findings[0]['code'] == 'explicit_observation'
    assert findings[0]['detail'] == 'Synthetic observation stays visible.'
    assert findings[0]['suggestion'] == 'Synthetic follow-up stays visible.'
    assert findings[0]['can_revise'] is True and not gaps
    assert outline['chapters'][0]['targets'][0]['template_refs'] == parts[1:3]
