"""The final automatic structure review can move exact existing subtrees."""
from copy import deepcopy
import json

import pytest

from docreader.llm.budget import RequestBudget
from docreader.llm.contracts import output_schema
from docreader.llm.request import ModelWorkUnitExhausted
from docreader.tender.authoring_structure import review_structure
from test_structure_template_exclusions import fixture as source_fixture


def fixture(tmp_path):
    artifacts, outline, _ = source_fixture(tmp_path)
    first, second = outline['chapters'][0]['targets']
    third = deepcopy(first) | {'target_id': 'target-c', 'kind': 'narrative',
                              'template_refs': [], 'related_target_ids': ['target-a']}
    third['response_table'] = {'context_source_excerpts': third['source_excerpts'], 'rows': [
        {'row_id': 'stable-row', 'source_excerpts': third['source_excerpts'], 'rule_refs': []}]}
    def chapter(identity, title, targets=(), children=()):
        return {'chapter_id': identity, 'title': title, 'is_volume': False,
                'targets': list(targets), 'children': list(children)}
    outline['chapters'] = [chapter('root', '项目交付', children=[
        chapter('parent', '传输路径核查（乙）', [first], [chapter('child', '处理能力', [second])]),
        chapter('other', '通用质量特性', [third], [chapter('detail', '维护说明')]),
        chapter('aux', '附加事项')])]
    return artifacts, outline


def move(chapter='other', parent='parent', after='child'):
    return {'chapter_id': chapter, 'parent_chapter_id': parent,
            'after_chapter_id': after, 'reason': '实际正文明确该子项从属于所选父章。'}


def nodes(chapters):
    return {node['chapter_id']: {key: deepcopy(value) for key, value in node.items() if key != 'children'}
            for chapter in chapters for node in [chapter, *descendants(chapter)]}


def descendants(chapter):
    return [node for child in chapter['children'] for node in [child, *descendants(child)]]


async def review(artifacts, outline, raw, *, enabled=True, outline_phase=True, replay=False):
    before = deepcopy(outline)
    async def validate_request(request, validate):
        payload = json.loads(request['messages'][1]['content'][0]['text'])
        assert payload.get('hierarchy_edits_allowed', False) is enabled
        value = validate(deepcopy(raw))
        assert outline == before, 'validation must not mutate the adopted tree'
        return value
    class Executor:
        async def replay(self, task, request, validate):
            return (True, await validate_request(request, validate)) if replay else (False, None)
        async def __call__(self, task, request, validate):
            assert not replay, 'accepted replay cannot send another model request'
            return await validate_request(request, validate)
    return await review_structure(artifacts, outline, [],
        RequestBudget(model='fixture', context_tokens=131072), Executor(),
        outline_phase=outline_phase, apply_hierarchy_edits=enabled)


@pytest.mark.parametrize('replay', [False, True])
async def test_explicit_move_preserves_complete_subtrees_and_all_references(tmp_path, replay):
    artifacts, outline = fixture(tmp_path)
    before = deepcopy(outline)
    finding = {'code': 'source_hierarchy_mismatch', 'detail': '所选子章不应提升为同级。'}
    findings, gaps = await review(artifacts, outline,
        {'findings': [finding], 'chapter_moves': [move()]}, replay=replay)
    parent, remaining = outline['chapters'][0]['children']
    assert parent['chapter_id'] == 'parent' and remaining['chapter_id'] == 'aux'
    assert [c['chapter_id'] for c in parent['children']] == ['child', 'other']
    assert [c['chapter_id'] for c in parent['children'][1]['children']] == ['detail']
    assert nodes(outline['chapters']) == nodes(before['chapters'])
    assert {k: v for k, v in outline.items() if k != 'chapters'} == {
        k: v for k, v in before.items() if k != 'chapters'}
    assert findings[0]['code'] == finding['code']
    assert [g['code'] for g in gaps] == ['structure_hierarchy_edits_applied']


async def test_moves_use_final_parent_and_resolve_moved_sibling_dependencies(tmp_path):
    artifacts, outline = fixture(tmp_path)
    before = deepcopy(outline)
    _, gaps = await review(artifacts, outline, {'findings': [], 'chapter_moves': [
        move('aux', 'parent', 'other'), move(), move('child', 'parent', None)]})
    assert [c['chapter_id'] for c in outline['chapters'][0]['children']] == ['parent']
    assert [c['chapter_id'] for c in outline['chapters'][0]['children'][0]['children']] == [
        'child', 'other', 'aux']
    assert nodes(outline['chapters']) == nodes(before['chapters'])
    assert len(gaps) == 3


async def test_move_to_root_preserves_chapter_and_target_ids(tmp_path):
    artifacts, outline = fixture(tmp_path)
    before = deepcopy(outline)
    await review(artifacts, outline, {'findings': [], 'chapter_moves': [move('child', None, None)]})
    assert [c['chapter_id'] for c in outline['chapters']] == ['child', 'root']
    assert nodes(outline['chapters']) == nodes(before['chapters'])


@pytest.mark.parametrize('bad', [
    [move('missing')], [move(parent='missing')], [move(after='missing')],
    [move(), move()], [move(parent='other')], [move(after='other')],
    [move(after='root')],
    [move('parent', 'other', None), move('other', 'child', None)],
    [move(), move('child', 'parent', 'other')],
    [move(), move('aux', 'parent', 'child')],
])
async def test_invalid_reference_cycle_or_position_conflict_rejects_whole_batch(tmp_path, bad):
    artifacts, outline = fixture(tmp_path)
    before = deepcopy(outline)
    finding = {'code': 'still_pending', 'detail': '保留原结构，等待核对。'}
    findings, gaps = await review(artifacts, outline, {'findings': [finding], 'chapter_moves': bad})
    assert outline == before
    assert findings[0]['code'] == finding['code']
    assert [g['code'] for g in gaps] == ['structure_hierarchy_edits_invalid']


async def test_invalid_later_action_cannot_leave_earlier_move_partly_applied(tmp_path):
    artifacts, outline = fixture(tmp_path)
    before = deepcopy(outline)
    _, gaps = await review(artifacts, outline, {'findings': [], 'chapter_moves': [
        move(), move('aux', 'missing', None)]})
    assert outline == before and gaps[0]['code'] == 'structure_hierarchy_edits_invalid'


@pytest.mark.parametrize('outline_phase', [True, False])
async def test_fixed_outline_or_saved_word_review_only_retains_move_suggestions(tmp_path, outline_phase):
    artifacts, outline = fixture(tmp_path)
    before = deepcopy(outline)
    _, gaps = await review(artifacts, outline, {'findings': [], 'chapter_moves': [move()]},
        enabled=False, outline_phase=outline_phase)
    assert outline == before
    assert [g['code'] for g in gaps] == ['structure_hierarchy_edits_not_applied']


async def test_duplicate_existing_chapter_identity_never_loses_content(tmp_path):
    artifacts, outline = fixture(tmp_path)
    outline['chapters'][0]['children'].append(deepcopy(outline['chapters'][0]['children'][1]))
    before = deepcopy(outline)
    _, gaps = await review(artifacts, outline, {'findings': [], 'chapter_moves': [move()]})
    assert outline == before and gaps[0]['code'] == 'structure_hierarchy_edits_invalid'


async def test_executor_failure_after_validation_does_not_apply_moves(tmp_path):
    artifacts, outline = fixture(tmp_path)
    before = deepcopy(outline)
    async def execute(task, request, validate):
        validate({'findings': [], 'chapter_moves': [move()]})
        assert outline == before
        raise ModelWorkUnitExhausted(task)
    _, gaps = await review_structure(artifacts, outline, [],
        RequestBudget(model='fixture', context_tokens=131072), execute,
        outline_phase=True, apply_hierarchy_edits=True)
    assert outline == before and gaps[-1]['code'] == 'structure_review_unavailable'


def test_move_contract_is_optional_and_structure_only():
    structure = output_schema('authoring_structure', {'operation': 'structure'})
    body = output_schema('authoring_review', {})
    assert 'chapter_moves' in structure['properties']
    assert 'chapter_moves' not in structure['required']
    assert 'chapter_moves' not in body['properties']
