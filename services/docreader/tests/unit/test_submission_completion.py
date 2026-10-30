"""Global semantic choices bind missing rules and cross-references without guessing."""
from copy import deepcopy
import json

import pytest
from pydantic import ValidationError

from docreader.llm.budget import RequestBudget
from docreader.llm.request import ModelContractError
from docreader.tender import authoring_pipeline as pipeline
from docreader.tender.schema import OutlineAssemblyTarget
from test_submission_followup import fixture_context, local_response


async def organize(tmp_path, assembly):
    artifacts, context, native = fixture_context(tmp_path, with_rule=True)

    async def execute(task, request, validate):
        payload = json.loads(request['messages'][1]['content'][0]['text'])
        if payload.get('operation') == 'assemble':
            return validate(assembly(payload))
        value = local_response(payload)
        for chapter in pipeline._flatten(value['chapters']):
            for target in chapter['targets']:
                target['rule_ids'] = []
        return validate(value)

    outline = await pipeline._organize(artifacts, context,
        RequestBudget(model='fixture', context_tokens=131072, output_reserve=16384), execute, [])
    return outline, context, artifacts, native


def response(payload, **target_fields):
    candidate = next(t for c in payload['chapters'] for t in c['targets'])
    return {'chapters': [{'title': '业务资料', 'targets': [{
        'kind': 'form', 'purpose': '按真实要求填写', 'target_ids': [candidate['target_id']],
        'template_part_ids': candidate['template_part_ids'], **target_fields}], 'children': []}]}


@pytest.mark.parametrize('new_position', [False, True])
async def test_missing_rule_can_bind_existing_or_new_position(tmp_path, new_position):
    def assemble(payload):
        assert len(payload['unassigned_rules']) == 1
        rule = payload['unassigned_rules'][0]
        assert rule['rule']['text'] == '按适用主体提供财务情况表。'
        value = response(payload, rule_ids=[rule['rule_id']])
        if new_position:
            value['chapters'][0]['targets'].append({
                'kind': 'narrative', 'purpose': '有规则依据的说明', 'target_ids': [],
                'rule_ids': [rule['rule_id']], 'support_origins': ['tender']})
            value['chapters'][0]['targets'][0]['rule_ids'] = []
        return value

    outline, context, _, _ = await organize(tmp_path, assemble)
    expected = list(context.rules.values())
    targets = outline['chapters'][0]['targets']
    assert targets[-1]['rule_refs'] == expected
    assert targets[-1]['source_refs']
    assert not any(i['code'] == 'outline_rule_unassigned' for i in outline['issues'])
    if new_position:
        assert targets[-1]['support_origins'] == ['tender']


@pytest.mark.parametrize('disposition', ['constraint', 'excluded', 'unresolved'])
async def test_missing_rule_has_distinct_disposition_and_unresolved_exit(tmp_path, disposition):
    def assemble(payload):
        value = response(payload)
        identity = payload['unassigned_rules'][0]['rule_id']
        if disposition == 'constraint':
            value['constraint_rule_ids'] = [identity]
        if disposition == 'excluded':
            value['excluded_rules'] = [{'rule_ids': [identity], 'reason': '原文明确限定另一标段。'}]
        return value

    outline, context, _, _ = await organize(tmp_path, assemble)
    assert bool([i for i in outline['issues'] if i['code'] == 'outline_rule_unassigned']) == (
        disposition == 'unresolved')
    if disposition == 'constraint':
        assert outline['constraint_rule_refs'] == list(context.rules.values())
    if disposition == 'excluded':
        assert next(i for i in outline['issues'] if i['code'] == 'outline_rule_excluded')['rule_refs'] == list(
            context.rules.values())


async def test_same_rule_cannot_be_excluded_and_assigned(tmp_path):
    def assemble(payload):
        identity = payload['unassigned_rules'][0]['rule_id']
        value = response(payload, rule_ids=[identity])
        value['excluded_rules'] = [{'rule_ids': [identity], 'reason': '冲突排除。'}]
        return value

    with pytest.raises(ModelContractError):
        await organize(tmp_path, assemble)


async def test_unknown_missing_rule_is_not_accepted(tmp_path):
    with pytest.raises(ModelContractError):
        await organize(tmp_path, lambda p: response(p, rule_ids=['unknown']))


@pytest.mark.parametrize('ambiguous', [False, True])
async def test_cross_reference_maps_only_unique_final_instance(tmp_path, ambiguous):
    def assemble(payload):
        value = response(payload)
        first = value['chapters'][0]['targets'][0]
        # One source candidate may represent two actual form instances.
        second = deepcopy(first)
        second['purpose'] = '跨卷参见既有表单'
        second['related_target_ids'] = first['target_ids']
        second['target_ids'] = []
        second['rule_ids'] = [payload['unassigned_rules'][0]['rule_id']]
        value['chapters'].append({'title': '另一卷', 'is_volume': True, 'targets': [second]})
        if ambiguous:
            value['chapters'][0]['targets'].append(deepcopy(first))
        return value

    outline, _, _, _ = await organize(tmp_path, assemble)
    first = outline['chapters'][0]['targets'][0]
    second = outline['chapters'][1]['targets'][0]
    assert second['related_target_ids'] == ([] if ambiguous else [first['target_id']])
    assert bool([i for i in outline['issues'] if i['code'] == 'outline_reference_ambiguous']) == ambiguous


def test_narrative_reference_can_use_existing_position_without_borrowing_its_content():
    target = OutlineAssemblyTarget.model_validate({
        'kind': 'narrative', 'purpose': '按原文引用本稿已有响应位置。',
        'related_target_ids': ['t1']})
    assert target.target_ids == target.rule_ids == []


@pytest.mark.parametrize('kind, related', [('narrative', []), ('form', ['t1']), ('attachment', ['t1'])])
def test_reference_alone_does_not_invent_a_form_attachment_or_ungrounded_position(kind, related):
    with pytest.raises(ValidationError):
        OutlineAssemblyTarget.model_validate({
            'kind': kind, 'purpose': '没有自己的原格式、证明或规则。', 'related_target_ids': related})


@pytest.mark.parametrize('ambiguous', [False, True])
async def test_pure_reference_does_not_inherit_rules_templates_or_role_instances(tmp_path, ambiguous):
    def assemble(payload):
        value = response(payload, rule_ids=[payload['unassigned_rules'][0]['rule_id']])
        original = value['chapters'][0]['targets'][0]
        value['chapters'].append({'title': '参见位置', 'targets': [{
            'kind': 'narrative', 'purpose': '参见实际资料位置，不重复该表的填写要求。',
            'related_target_ids': original['target_ids']}]})
        if ambiguous:
            second_role = deepcopy(original)
            second_role['purpose'] = '为另一适用主体填写独立实例。'
            value['chapters'][0]['targets'].append(second_role)
        return value

    outline, context, _, _ = await organize(tmp_path, assemble)
    originals = outline['chapters'][0]['targets']
    reference = outline['chapters'][1]['targets'][0]
    assert len(originals) == (2 if ambiguous else 1)
    assert all(t['rule_refs'] == list(context.rules.values()) and t['template_refs'] for t in originals)
    assert reference['rule_refs'] == reference['source_refs'] == reference['template_refs'] == []
    assert reference['related_target_ids'] == ([] if ambiguous else [originals[0]['target_id']])
    assert bool([i for i in outline['issues'] if i['code'] == 'outline_reference_ambiguous']) == ambiguous
    assert not any(i['code'] == 'outline_rule_unassigned' for i in outline['issues'])
    planned = [t['target_id'] for task in pipeline.plan_writing_tasks(outline) for t in task['targets']]
    assert (reference['target_id'] in planned) == ambiguous
    assert all(t['target_id'] in planned for t in originals)


@pytest.mark.parametrize('own_content', ['rule', 'source', 'template', 'manual', 'unknown', 'self', 'partial'])
async def test_references_with_own_work_or_unresolved_destinations_still_get_writing_tasks(tmp_path, own_content):
    def assemble(payload):
        value = response(payload, rule_ids=[payload['unassigned_rules'][0]['rule_id']])
        value['chapters'].append({'title': '参见位置', 'targets': [{
            'kind': 'narrative', 'purpose': '对照实际位置安排工作。',
            'related_target_ids': value['chapters'][0]['targets'][0]['target_ids']}]})
        return value

    outline, _, _, _ = await organize(tmp_path, assemble)
    original = outline['chapters'][0]['targets'][0]
    reference = outline['chapters'][1]['targets'][0]
    if own_content in {'rule', 'source', 'template'}:
        field = {'rule': 'rule_refs', 'source': 'source_refs', 'template': 'template_refs'}[own_content]
        reference[field] = deepcopy(original[field])
        assert reference[field]
    elif own_content == 'manual':
        reference['manual_fill'] = True
    elif own_content == 'self':
        reference['related_target_ids'] = [reference['target_id']]
    elif own_content == 'unknown':
        reference['related_target_ids'] = ['unavailable']
    else:
        reference['related_target_ids'].append('unavailable')

    planned = [t['target_id'] for task in pipeline.plan_writing_tasks(outline) for t in task['targets']]
    assert reference['target_id'] in planned


async def test_pure_reference_rejects_unknown_candidate_identity(tmp_path):
    def assemble(payload):
        value = response(payload)
        value['chapters'].append({'title': '参见位置', 'targets': [{
            'kind': 'narrative', 'purpose': '未知目标不能作为可解析参见。',
            'related_target_ids': ['unknown']}]})
        return value

    with pytest.raises(ModelContractError) as caught:
        await organize(tmp_path, assemble)
    assert any(error['code'] == 'outline_unknown_reference' for error in caught.value.errors)


async def test_reference_does_not_discard_an_unarranged_candidate(tmp_path):
    def assemble(payload):
        candidate = next(t for c in payload['chapters'] for t in c['targets'])
        return {'chapters': [{'title': '参见位置', 'targets': [{
            'kind': 'narrative', 'purpose': '原候选尚未安排，引用范围保留待核。',
            'related_target_ids': [candidate['target_id']]}]}]}

    outline, _, _, _ = await organize(tmp_path, assemble)
    reference = outline['chapters'][0]['targets'][0]
    pending = next(c for c in outline['chapters'] if c['title'] == '待整理内容')
    assert pending['targets'] and pending['targets'][0]['template_refs']
    assert reference['rule_refs'] == reference['source_refs'] == reference['template_refs'] == []
    assert reference['related_target_ids'] == [pending['targets'][0]['target_id']]
    assert not any(i['code'] == 'outline_reference_ambiguous' for i in outline['issues'])


async def test_cover_and_explicit_native_table_header_are_bound(tmp_path):
    def assemble(payload):
        value = response(payload)
        value['chapters'][0]['presentation_role'] = 'cover'
        part = next(p for p in payload['template_parts'] if p.get('grid'))
        value['table_headers'] = [{'part_id': part['part_id'], 'header_row_count': 2}]
        return value

    outline, _, _, _ = await organize(tmp_path, assemble)
    assert outline['chapters'][0]['presentation_role'] == 'cover'
    assert outline['chapters'][0]['targets'][0]['template_refs'][0]['header_row_count'] == 2


async def test_invalid_optional_header_keeps_original_table(tmp_path):
    def assemble(payload):
        value = response(payload)
        part = next(p for p in payload['template_parts'] if p.get('grid'))
        value['table_headers'] = [{'part_id': part['part_id'], 'header_row_count': 99}]
        return value

    outline, _, _, _ = await organize(tmp_path, assemble)
    assert outline['chapters'][0]['targets'][0]['template_refs']
    assert any(i['code'] == 'outline_table_header_unverified' for i in outline['issues'])


@pytest.mark.parametrize('counts', [[4], [1, 2, 2]])
async def test_whole_table_or_conflicting_header_stays_unconfirmed(tmp_path, counts):
    def assemble(payload):
        value = response(payload)
        part = next(p for p in payload['template_parts'] if p.get('grid'))
        value['table_headers'] = [{'part_id': part['part_id'], 'header_row_count': n} for n in counts]
        return value

    outline, _, _, _ = await organize(tmp_path, assemble)
    assert outline['chapters'][0]['targets'][0]['template_refs'][0].get('header_row_count') is None
    assert any(i['code'] == 'outline_table_header_unverified' for i in outline['issues'])


async def test_new_rule_position_can_supply_its_own_retrieval_intent(tmp_path):
    def assemble(payload):
        identity = payload['unassigned_rules'][0]['rule_id']
        value = response(payload, rule_ids=[identity])
        value['search_intents'] = [{'query': '财务证明', 'for_ids': [identity]}]
        return value

    outline, context, _, _ = await organize(tmp_path, assemble)
    intent = outline['search_intents'][0]
    assert intent['rule_refs'] == list(context.rules.values())
    assert intent['source_refs']
    gaps = []
    queries = pipeline._knowledge_queries(outline, pipeline.plan_writing_tasks(outline), gaps)
    assert queries and not gaps and queries[0]['include_original'] is False
