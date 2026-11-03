"""Assembly may distribute one compound candidate without widening it again."""

from copy import deepcopy
import json

import pytest
from pydantic import ValidationError

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.budget import RequestBudget
from docreader.llm.request import ModelContractError, ModelWorkUnitExhausted
from docreader.tender import authoring_pipeline as pipeline
from docreader.tender.authoring import _OutlineContext
from docreader.tender.authoring_excerpts import load_source_excerpt
from docreader.tender.evidence.references import SourceIndex, fragment, selector
from docreader.tender.schema import OutlineAssemblyTarget
from test_assembly_source_handoff import BODY, CONDITION, context_fixture
from test_structural_packing import source


def text_of(artifacts, target):
    return [load_source_excerpt(artifacts, ref)['text'] for ref in target.get('source_excerpts', [])]


def shared_unit_fixture(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    native = source([{'text': '\n'.join(BODY), 'heading': '复合要求'}])
    native = SourceIndex({**native.document, 'original_ref': artifacts.put_bytes(b'synthetic original')}, native.raw)
    index_ref = artifacts.put_json(native.descriptor(artifacts.put_json(native.raw)))
    fragments = [fragment(native.ranges[0], start, start + len(text))
                 for text, start in zip(BODY, [0, len(BODY[0]) + 1])]
    identity = {'run_id': 'shared-scope', 'source_version': 1, 'domain_revision': 1}
    frozen = artifacts.put_json({'documents': [{**native.document, 'role': 'main', 'source_index_ref': index_ref}]})
    pack = artifacts.put_json({'schema_version': 2, 'pack_id': 'pack', **native.document,
        'source_index_ref': index_ref, 'body': [selector(row) for row in fragments], 'supporting': []})
    items = [{'item_id': f'item-{number}', 'text': text, 'evidence': [
        {'range_id': fragments[number]['range_id'], 'quote': text}]} for number, text in enumerate(BODY)]
    snapshot = {'schema_version': 2, **identity, 'frozen_ref': frozen,
        'pack_plan_ref': artifacts.put_json({'packs': [{'pack_id': 'pack', 'artifact_ref': pack}]}),
        'extract_refs': [artifacts.put_json({'items': items})]}
    return artifacts, _OutlineContext(artifacts, snapshot)


async def organize(tmp_path, choose, *, review=False, revision=None, shared_unit=False, initial_gaps=()):
    artifacts, context = shared_unit_fixture(tmp_path) if shared_unit else context_fixture(tmp_path)
    calls, structures, gaps = [], [], list(initial_gaps)

    async def execute(task, request, validate):
        value = json.loads(request['messages'][1]['content'][0]['text'])
        calls.append(deepcopy(value))
        if task == 'authoring_structure':
            structures.append(value)
            return validate({'findings': [{'code': 'check_scope', 'detail': '核对实际响应范围。'}]
                             if len(structures) == 1 else []})
        if value.get('operation') == 'assemble':
            response = (revision if value.get('review_findings') and revision else choose)(value)
            try:
                return validate(response)
            except ModelContractError:
                if revision and value.get('review_findings'):
                    raise ModelWorkUnitExhausted('fixture exhausted') from None
                raise
        aliases = {source['text']: source['range_id'] for source in value['sources']}
        rules = {rule['item']['text']: rule['rule_id'] for rule in value['rules']}
        ranges = ([rule['item']['evidence'][0]['range_id'] for rule in value['rules']]
                  if shared_unit else [aliases[text] for text in BODY])
        return validate({'schema_version': 2, 'chapters': [{'title': '原复合范围', 'targets': [{
            'kind': 'narrative', 'purpose': '同时响应两项独立要求。',
            'rule_ids': [rules[text] for text in BODY],
            'range_ids': ranges, 'support_origins': ['tender']}]}],
            'constraint_rule_ids': [rules[CONDITION]] if CONDITION in rules else [], 'findings': []})

    result = await pipeline._organize(artifacts, context,
        RequestBudget(model='fixture', context_tokens=131072), execute, gaps,
        structure_results={} if review else None)
    return artifacts, context, result, calls, gaps


def split(value, *, positions=2):
    candidate = next(t for chapter in value['chapters'] for t in chapter['targets'])
    return {'chapters': [{'title': f'业务范围{number + 1}', 'targets': [{
        'kind': 'narrative', 'purpose': f'只负责第{number + 1}项要求。',
        'target_ids': [candidate['target_id']],
        'candidate_scope': {'range_ids': [candidate['range_ids'][number]],
                            'rule_ids': [candidate['rule_ids'][number]]}}]}
        for number in range(positions)]}


async def test_assembly_exposes_candidate_rules_and_binds_each_scope_without_coarse_expansion(tmp_path):
    artifacts, context, result, calls, gaps = await organize(tmp_path, split)
    assembly = next(value for value in calls if value.get('operation') == 'assemble')
    candidate = assembly['chapters'][0]['targets'][0]
    rules = {entry['rule_id']: entry['rule'] for entry in assembly['candidate_rules']}
    assert [rules[identity]['text'] for identity in candidate['rule_ids']] == BODY
    assert 'item_id' not in json.dumps(assembly['candidate_rules'])
    assert not gaps and len(result['chapters']) == 2
    for number, task in enumerate(pipeline.plan_writing_tasks(result)):
        target, = task['targets']
        assert text_of(artifacts, target) == [BODY[number]]
        assert [ref['item_ref']['item_id'] for ref in target['rule_refs']] == [f'item-{number}']
        assert len(target['source_refs']) == 1
        loaded, _, issues = pipeline._load_task(artifacts, context, task,
            {'response': {'shared_arrangements': []}}, [])
        assert not issues
        assert [entry['text'] for entry in loaded['supports']] == [BODY[number]]


async def test_partially_selected_candidate_retains_only_its_unassigned_remainder(tmp_path):
    artifacts, _, result, _, gaps = await organize(tmp_path, lambda value: split(value, positions=1))
    first, remainder = result['chapters']
    assert first['title'] == '业务范围1' and remainder['title'] == '待整理内容'
    assert text_of(artifacts, first['targets'][0]) == [BODY[0]]
    assert text_of(artifacts, remainder['targets'][0]) == [BODY[1]]
    assert [ref['item_ref']['item_id'] for ref in remainder['targets'][0]['rule_refs']] == ['item-1']
    assert any(gap['code'] == 'outline_assembly_incomplete' for gap in gaps)


async def test_unique_previous_scope_is_inherited_when_omitted_in_revision(tmp_path):
    def revise(value):
        previous = value['previous_structure'][0]['targets'][0]
        assert previous['candidate_scope']
        response = split(value, positions=1)
        response['chapters'][0]['targets'][0].pop('candidate_scope')
        response['chapters'][0]['title'] = '修订业务名称'
        return response

    # A complete explicit scope is unique; omission on revision retains that scope.
    def whole_scope(value):
        response = split(value, positions=1)
        candidate = value['chapters'][0]['targets'][0]
        response['chapters'][0]['targets'][0]['candidate_scope'] = {
            'range_ids': candidate['range_ids'], 'rule_ids': candidate['rule_ids']}
        return response

    artifacts, _, result, calls, gaps = await organize(tmp_path, whole_scope, review=True, revision=revise)
    assert result['chapters'][0]['title'] == '修订业务名称'
    assert text_of(artifacts, result['chapters'][0]['targets'][0]) == BODY
    assert len([value for value in calls if value.get('operation') == 'assemble']) == 2
    assert not gaps


async def test_ambiguous_split_revision_cannot_expand_omitted_scope_back_to_whole_candidate(tmp_path):
    def revise(value):
        response = split(value)
        for chapter in response['chapters']:
            chapter['targets'][0].pop('candidate_scope')
        return response

    artifacts, _, result, _, _ = await organize(tmp_path, split, review=True, revision=revise)
    assert [text_of(artifacts, chapter['targets'][0]) for chapter in result['chapters']] == [[BODY[0]], [BODY[1]]]


async def test_two_scopes_in_one_native_unit_keep_exact_writing_dependencies_separate(tmp_path):
    artifacts, context, result, _, gaps = await organize(tmp_path, split, shared_unit=True)
    assert not gaps and len(result['chapters']) == 2
    targets = [chapter['targets'][0] for chapter in result['chapters']]
    assert targets[0]['source_refs'] == targets[1]['source_refs']
    for number, task in enumerate(pipeline.plan_writing_tasks(result)):
        loaded, _, _ = pipeline._load_task(artifacts, context, task, {'response': {'shared_arrangements': []}}, [])
        assert [support['text'] for support in loaded['supports']] == [BODY[number]]


@pytest.mark.parametrize('field,identity', [('range_ids', 'not-in-candidate'), ('rule_ids', 'u1')])
async def test_scope_cannot_select_outside_its_candidate(tmp_path, field, identity):
    def choose(value):
        response = split(value, positions=1)
        response['chapters'][0]['targets'][0]['candidate_scope'][field] = [identity]
        return response
    with pytest.raises(ModelContractError) as caught:
        await organize(tmp_path, choose)
    assert caught.value.errors[0]['path'] == ['chapters', 0, 'targets', 0, 'candidate_scope', field, 0]


@pytest.mark.parametrize('carrier', ['template_part_ids', 'response_table'])
async def test_scoped_target_cannot_replace_a_complete_native_carrier(tmp_path, carrier):
    def choose(value):
        response = split(value, positions=1)
        target = response['chapters'][0]['targets'][0]
        target[carrier] = ['p1'] if carrier == 'template_part_ids' else {'rows': [
            {'range_ids': target['candidate_scope']['range_ids'], 'rule_ids': []}]}
        return response
    with pytest.raises(ModelContractError) as caught:
        await organize(tmp_path, choose)
    assert caught.value.errors[0]['code'] == 'outline_scope_carrier_conflict'


@pytest.mark.parametrize('scope', [None, {}, {'range_ids': [], 'rule_ids': []},
                                  {'range_ids': ['x1']}, {'rule_ids': ['a1']}])
def test_scope_requires_an_explicit_nonempty_pair_of_selections(scope):
    with pytest.raises(ValidationError):
        OutlineAssemblyTarget(kind='narrative', purpose='实际要求', target_ids=['t1'], candidate_scope=scope)


def test_scope_is_optional_and_accepts_one_complete_selection():
    assert OutlineAssemblyTarget(kind='narrative', purpose='实际要求', target_ids=['t1']).candidate_scope is None
    target = OutlineAssemblyTarget(kind='narrative', purpose='实际要求', target_ids=['t1'],
        candidate_scope={'range_ids': ['x1'], 'rule_ids': []})
    assert target.candidate_scope.range_ids == ['x1']


def test_scope_schema_does_not_offer_null_when_only_omission_means_inheritance():
    schema = OutlineAssemblyTarget.model_json_schema()
    field = schema['properties']['candidate_scope']
    assert 'candidate_scope' not in schema.get('required', [])
    assert field.get('type') != 'null'
    assert all(option.get('type') != 'null' for option in field.get('anyOf', []))
    assert 'default' not in field
