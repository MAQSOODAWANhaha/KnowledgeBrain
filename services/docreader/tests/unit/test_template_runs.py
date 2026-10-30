"""Assembly exposes native continuity without admitting hidden template parts."""
from copy import deepcopy
import json

import pytest

from docreader.llm.budget import RequestBudget
from docreader.llm.request import ModelContractError
from docreader.tender import authoring_pipeline, authoring_templates
from docreader.tender.authoring_view import compact_assembly_payload
from docreader.tender.evidence.references import SourceIndex

from test_authoring_compact_view import native_grid, payload
from test_authoring_templates import create_context, create_source


def prepared(tmp_path, texts, *, image_at=None):
    artifacts, index_ref, native = create_source(tmp_path, [{'text': text} for text in texts])
    if image_at is not None:
        raw = deepcopy(native.raw)
        raw['units'][image_at].update(kind='image_region', text='')
        raw['units'][image_at]['locator']['image_key'] = 'hidden-image'
        native = SourceIndex(native.document, raw)
        index_ref = artifacts.put_json(native.descriptor(artifacts.put_json(raw)))
    context = create_context(artifacts, index_ref, native)
    authoring_templates.prepare_template_parts(artifacts, context)
    return artifacts, context, list(context.template_parts.values())


def test_hidden_empty_image_splits_runs_and_original_span_guard_still_rejects(tmp_path):
    _, context, parts = prepared(tmp_path, ['开头。', '前半末尾。', '', '后半起点。', '签署。'], image_at=2)
    visible = dict(zip(('p284', 'p285', 'p310', 'p311'), (parts[0], parts[1], parts[3], parts[4])))
    before = deepcopy(visible)
    assert authoring_templates.template_part_runs(visible, context.repository, all_parts=context.template_parts) == [
        ['p284', 'p285'], ['p310', 'p311']]
    with pytest.raises(ModelContractError) as caught:
        authoring_templates.expand_template_spans(visible, [
            {'start_part_id': 'p284', 'end_part_id': 'p311'}], context.repository, all_parts=context.template_parts)
    assert caught.value.errors[0]['code'] == 'outline_invalid_template_span'
    assert authoring_templates.expand_template_spans(visible, [
        {'start_part_id': 'p284', 'end_part_id': 'p285'},
        {'start_part_id': 'p310', 'end_part_id': 'p311'}], context.repository,
        all_parts=context.template_parts) == list(visible)
    assert visible == before


def test_runs_never_join_adjacent_parts_from_different_frozen_documents(tmp_path):
    _, first, first_parts = prepared(tmp_path, ['第一文件开头。', '第一文件末尾。'])
    _, second, second_parts = prepared(tmp_path, ['第二文件开头。', '第二文件末尾。'])
    all_parts = {**first.template_parts, **second.template_parts}
    visible = dict(zip(('p1', 'p2', 'p3', 'p4'), first_parts + second_parts))
    assert authoring_templates.template_part_runs(visible, first.repository, all_parts=all_parts) == [
        ['p1', 'p2'], ['p3', 'p4']]


def test_isolated_parts_and_empty_visible_pool_do_not_advertise_spans(tmp_path):
    _, context, parts = prepared(tmp_path, ['孤立一。', '未提供。', '孤立二。', '未提供。', '孤立三。'])
    visible = dict(zip(('p1', 'p2', 'p3'), parts[::2]))
    assert authoring_templates.template_part_runs(visible, context.repository, all_parts=context.template_parts) == []
    assert authoring_templates.template_part_runs({}, context.repository, all_parts=context.template_parts) == []


def test_late_alias_inserted_between_existing_parts_uses_native_order(tmp_path):
    _, context, parts = prepared(tmp_path, ['首段。\n后来补入的中段。\n末段。'])
    visible = {'p1': parts[0], 'p2': parts[2], 'p844': parts[1]}
    assert authoring_templates.template_part_runs(visible, context.repository, all_parts=context.template_parts) == [
        ['p1', 'p2']]
    assert authoring_templates.expand_template_spans(visible, [
        {'start_part_id': 'p1', 'end_part_id': 'p2'}], context.repository,
        all_parts=context.template_parts) == ['p1', 'p844', 'p2']


def test_run_projection_orders_the_full_registry_once(monkeypatch, tmp_path):
    _, context, parts = prepared(tmp_path, [f'原文{number}。' for number in range(30)])
    visible = {f'p{number}': part for number, part in enumerate(parts, 1)}
    ordered = authoring_templates.ordered_template_part_ids
    calls = []

    def record_order(parts, repository):
        calls.append(len(parts))
        return ordered(parts, repository)

    monkeypatch.setattr(authoring_templates, 'ordered_template_part_ids', record_order)
    assert authoring_templates.template_part_runs(visible, context.repository, all_parts=context.template_parts) == [
        ['p1', 'p30']]
    assert calls == [30]


def test_compact_assembly_keeps_runs_and_discrete_part_identities():
    original = payload(native_grid())
    original['template_parts'].append({**deepcopy(original['template_parts'][0]), 'part_id': 'p2'})
    original['template_runs'] = [['p1', 'p2']]
    before = deepcopy(original)
    compact = compact_assembly_payload(original)
    assert compact['template_runs'] == [['p1', 'p2']]
    assert list(compact['template_parts']) == ['p1', 'p2']
    assert original == before


@pytest.mark.parametrize('compact', [False, True])
async def test_assembly_request_advertises_runs_without_changing_source_payload(monkeypatch, tmp_path, compact):
    artifacts, context, _ = prepared(tmp_path, ['前段。', '前段签署。', '', '后段。', '后段签署。'], image_at=2)
    fits = authoring_pipeline._fits
    if compact:
        monkeypatch.setattr(authoring_pipeline, '_fits', lambda budget, task, value, **options:
            False if value.get('operation') == 'assemble' else fits(budget, task, value, **options))
    calls = []

    async def execute(stage, request, validate):
        value = json.loads(request['messages'][1]['content'][0]['text'])
        calls.append(value)
        if value.get('operation') != 'assemble':
            assert 'template_runs' not in value
            return validate({'schema_version': 2, 'chapters': [{'title': '原格式', 'targets': [{
                'kind': 'form', 'purpose': '仅选择实际可见的前后格式', 'rule_ids': [], 'range_ids': [],
                'template_part_ids': [part['part_id'] for part in value['template_parts'] if part['part_id'] != 'p3']}]}],
                'findings': []})
        assert value['template_runs'] == [['p1', 'p2'], ['p3', 'p4']]
        assert (value.get('assembly_view') == 'compact_v1') is compact
        parts = list(value['template_parts']) if compact else [part['part_id'] for part in value['template_parts']]
        assert parts == ['p1', 'p2', 'p3', 'p4']
        with pytest.raises(ModelContractError):
            validate({'chapters': [{'title': '格式', 'targets': [{
                'kind': 'form', 'purpose': '不可跨过未提供图片', 'target_ids': ['t1'],
                'template_spans': [{'start_part_id': 'p1', 'end_part_id': 'p4'}]}]}]})
        return validate({'chapters': [{'title': '格式', 'targets': [{
            'kind': 'form', 'purpose': '按可见连续范围保留', 'target_ids': ['t1'],
            'template_spans': [{'start_part_id': start, 'end_part_id': end}
                               for start, end in value['template_runs']]}]}]})

    result = await authoring_pipeline._organize(artifacts, context,
        RequestBudget(model='fixture', context_tokens=131072), execute, [])
    assert len(calls) == 2
    selected = result['chapters'][0]['targets'][0]['template_refs']
    assert [part['source_ref']['unit_id'] for part in selected] == ['unit:0', 'unit:1', 'unit:3', 'unit:4']
