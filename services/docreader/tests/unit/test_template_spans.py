"""Complete native template spans retain interior clauses and continuation grids."""
from copy import deepcopy
import json

import pytest
from pydantic import ValidationError

from docreader.llm.budget import RequestBudget
from docreader.llm.request import ModelContractError
from docreader.llm.wire import WirePayload
from docreader.tender import authoring_templates
from docreader.tender import authoring_pipeline
from docreader.tender.schema import (
    OutlineAssemblyChapter, OutlineAssemblyTarget, OutlineChapter,
    OutlineChapterResponse, OutlineTargetResponse, ResponseTarget,
)
from test_authoring_templates import create_context, create_source
from test_structural_packing import grid


def prepared(tmp_path, units):
    artifacts, index_ref, native = create_source(tmp_path, units)
    context = create_context(artifacts, index_ref, native)
    records, views = authoring_templates.prepare_template_parts(artifacts, context)
    return context, records, views


def test_prepared_template_pool_uses_native_order_for_text_grid_and_continuation(tmp_path):
    context, _, views = prepared(tmp_path, [
        {'text': '完整原格式的标题。'}, {'grid': grid(rows=3)},
        {'grid': grid(rows=2)}, {'text': '末尾说明。\n签章：____'},
    ])
    assert [context.template_parts[view['part_id']]['source_ref']['unit_id'] for view in views] == [
        'unit:0', 'unit:1', 'unit:2', 'unit:3', 'unit:3']
    assert list(context.template_parts) == [view['part_id'] for view in views]


def test_ordering_and_span_expansion_keep_all_interior_parts_in_native_order(tmp_path):
    context, _, views = prepared(tmp_path, [
        {'text': '原函开头。\n第一条。'}, {'text': '不能遗漏的中间承诺。'},
        {'grid': grid(rows=2)}, {'text': '末尾签署：____'},
    ])
    # Request-local aliases and dict insertion order do not define source order.
    native_ids = [view['part_id'] for view in views]
    by_unit = {}
    for identity, part in context.template_parts.items():
        by_unit.setdefault(part['source_ref']['unit_id'], []).append(identity)
    expected = [identity for unit in ('unit:0', 'unit:1', 'unit:2', 'unit:3') for identity in by_unit[unit]]
    pool = {identity: context.template_parts[identity] for identity in reversed(native_ids)}
    before = deepcopy(pool)
    assert authoring_templates.ordered_template_part_ids(pool, context.repository) == expected
    selected = authoring_templates.expand_template_spans(pool, [
        {'start_part_id': expected[0], 'end_part_id': expected[-1]},
        {'start_part_id': expected[1], 'end_part_id': expected[-2]},
    ], context.repository)
    assert selected == expected
    assert pool == before


async def test_real_assembly_span_aliases_align_with_permanent_frozen_registry(tmp_path):
    artifacts, index_ref, native = create_source(tmp_path, [
        {'text': '完整函件开头。'}, {'text': '完整函件签署。'}])
    context = create_context(artifacts, index_ref, native)
    calls = []

    async def execute(stage, request, validate):
        payload = json.loads(request['messages'][1]['content'][0]['text'])
        calls.append(payload)
        if payload.get('operation') == 'assemble':
            assert [part['part_id'] for part in payload['template_parts']] == ['p1', 'p2']
            assert all(identity.startswith('part-') for identity in context.template_parts)
            return validate({'chapters': [{'title': '完整函件', 'targets': [{
                'kind': 'form', 'purpose': '保留完整原格式', 'target_ids': ['t1'],
                'template_spans': [{'start_part_id': 'p1', 'end_part_id': 'p2'}]}]}]})
        return validate({'schema_version': 2, 'chapters': [{'title': '源格式', 'targets': [{
            'kind': 'form', 'purpose': '按原格式填写', 'rule_ids': [], 'range_ids': [],
            'template_part_ids': [part['part_id'] for part in payload['template_parts']]}]}], 'findings': []})

    result = await authoring_pipeline._organize(artifacts, context,
        RequestBudget(model='fixture', context_tokens=131072), execute, [])
    assert len(calls) == 2
    assert result['chapters'][0]['targets'][0]['template_refs'] == list(context.template_parts.values())


def test_alias_span_still_rejects_unprovided_permanent_interior(tmp_path):
    context, _, views = prepared(tmp_path, [
        {'text': '函件起点。'}, {'text': '本批未读的中间承诺。'}, {'text': '函件签署。'}])
    first, _, last = [view['part_id'] for view in views]
    pool = {'p1': context.template_parts[first], 'p2': context.template_parts[last]}
    with pytest.raises(ModelContractError) as caught:
        authoring_templates.expand_template_spans(pool, [
            {'start_part_id': 'p1', 'end_part_id': 'p2'}], context.repository,
            all_parts=context.template_parts)
    assert caught.value.errors[0]['path'] == [0, 'end_part_id']
    assert caught.value.errors[0]['code'] == 'outline_invalid_template_span'


@pytest.mark.parametrize('interior', [{'text': '本批未读的中间承诺。'}, {'grid': grid(rows=2)}])
def test_span_rejects_invisible_interior_instead_of_claiming_complete_native_range(tmp_path, interior):
    context, _, views = prepared(tmp_path, [
        {'text': '原格式起点。'}, interior, {'text': '原格式签署。'}])
    first, hidden, last = [view['part_id'] for view in views]
    pool = {identity: context.template_parts[identity] for identity in (first, last)}
    before = deepcopy(pool)
    with pytest.raises(ModelContractError) as caught:
        authoring_templates.expand_template_spans(pool, [
            {'start_part_id': first, 'end_part_id': last}], context.repository,
            all_parts=context.template_parts, path=('chapters', 0, 'targets', 0, 'template_spans'))
    assert caught.value.errors[0]['path'] == ['chapters', 0, 'targets', 0, 'template_spans', 0, 'end_part_id']
    assert caught.value.errors[0]['code'] == 'outline_invalid_template_span'
    assert 'template_part_ids' in caught.value.errors[0]['detail']
    assert hidden not in pool and pool == before


def test_span_allows_invisible_parts_outside_selected_native_range(tmp_path):
    context, _, views = prepared(tmp_path, [{'text': text} for text in (
        '未读前文。', '格式开头。', '中间条款。', '格式签署。', '未读后文。')])
    identities = [view['part_id'] for view in views]
    visible = identities[1:4]
    pool = {identity: context.template_parts[identity] for identity in reversed(visible)}
    assert authoring_templates.expand_template_spans(pool, [
        {'start_part_id': visible[0], 'end_part_id': visible[-1]}], context.repository,
        all_parts=context.template_parts) == visible


@pytest.mark.parametrize('field', ['start_part_id', 'end_part_id'])
def test_span_endpoints_must_be_visible_even_when_registered_in_all_parts(tmp_path, field):
    context, _, views = prepared(tmp_path, [{'text': text} for text in ('起点。', '未读端点。', '终点。')])
    first, hidden, last = [view['part_id'] for view in views]
    pool = {identity: context.template_parts[identity] for identity in (first, last)}
    span = {'start_part_id': first, 'end_part_id': last, field: hidden}
    with pytest.raises(ModelContractError) as caught:
        authoring_templates.expand_template_spans(pool, [span], context.repository,
            all_parts=context.template_parts)
    assert caught.value.errors[0]['path'] == [0, field]
    assert caught.value.errors[0]['code'] == 'outline_unknown_template'


@pytest.mark.parametrize('invalid', ['unknown_start', 'unknown_end', 'reversed', 'cross_document'])
def test_span_rejects_invisible_reversed_and_cross_document_endpoints(tmp_path, invalid):
    context, _, views = prepared(tmp_path, [{'text': '第一段。'}, {'text': '最后一段。'}])
    identities = [view['part_id'] for view in views]
    pool = dict(context.template_parts)
    start, end = identities
    expected_field = 'start_part_id'
    if invalid == 'unknown_start':
        start = 'not-in-this-request'
    elif invalid == 'unknown_end':
        end, expected_field = 'not-in-this-request', 'end_part_id'
    elif invalid == 'reversed':
        start, end = end, start
        expected_field = 'end_part_id'
    else:
        artifacts, index_ref, native = create_source(tmp_path, [{'text': '另一个冻结文档。'}])
        other = create_context(artifacts, index_ref, native)
        _, other_views = authoring_templates.prepare_template_parts(artifacts, other)
        pool.update(other.template_parts)
        end, expected_field = other_views[0]['part_id'], 'end_part_id'
    with pytest.raises(ModelContractError) as caught:
        authoring_templates.expand_template_spans(pool, [
            {'start_part_id': start, 'end_part_id': end}], context.repository,
            path=('chapters', 0, 'targets', 0, 'template_spans'))
    assert caught.value.errors[0]['path'] == [
        'chapters', 0, 'targets', 0, 'template_spans', 0, expected_field]


def test_span_endpoint_wire_references_use_the_supplied_template_namespace():
    wire = WirePayload('outline', {'schema_version': 2, 'template_parts': [
        {'part_id': 'native-first'}, {'part_id': 'native-last'}]})
    restored = wire.decode({'template_spans': [{'start_part_id': 'p1', 'end_part_id': 'p2'}]})
    assert restored == {'template_spans': [{'start_part_id': 'native-first', 'end_part_id': 'native-last'}]}
    assert wire.decode({'template_spans': [{'start_part_id': 'native-first', 'end_part_id': 'p999'}]}) == {
        'template_spans': [{'start_part_id': '!unbound:native-first', 'end_part_id': '!unbound:p999'}]}


@pytest.mark.parametrize('model', [OutlineTargetResponse, OutlineAssemblyTarget])
def test_outline_contract_accepts_compact_template_bounds_and_explicit_subject(model):
    value = {'kind': 'form', 'purpose': '分别保留原文要求的填写实例',
             'rule_ids': [], 'range_ids': [], 'subject': '投标人',
             'template_spans': [{'start_part_id': 'p1', 'end_part_id': 'p7'}]}
    if model is OutlineAssemblyTarget:
        value.pop('range_ids')
        value['target_ids'] = ['t1']
    parsed = model.model_validate(value)
    assert parsed.subject == '投标人'
    assert parsed.template_spans[0].model_dump() == {'start_part_id': 'p1', 'end_part_id': 'p7'}
    with pytest.raises(ValidationError):
        model.model_validate({**value, 'subject': ' '})


def test_persistent_subject_and_submission_note_role_are_additive(tmp_path):
    target = {'target_id': 'target', 'kind': 'form', 'purpose': '按原格式填写',
              'rule_refs': [], 'source_refs': [], 'subject': '原厂'}
    assert ResponseTarget.model_validate(target).subject == '原厂'
    historical = {key: value for key, value in target.items() if key != 'subject'}
    assert ResponseTarget.model_validate(historical).subject is None
    for model in (OutlineChapterResponse, OutlineAssemblyChapter, OutlineChapter):
        chapter = {'title': '仅为外部操作待办', 'targets': [], 'children': [],
                   'presentation_role': 'submission_note'}
        if model is OutlineChapter:
            chapter['chapter_id'] = 'chapter'
        assert model.model_validate(chapter).presentation_role == 'submission_note'
