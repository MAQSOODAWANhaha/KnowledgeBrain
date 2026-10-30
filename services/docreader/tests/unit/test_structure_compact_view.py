"""Oversized structure review keeps source semantics and its one-call boundary."""

from copy import deepcopy
import json

from docreader.llm.budget import RequestBudget
from docreader.llm.request import ModelWorkUnitExhausted, build_request
from docreader.tender import authoring_structure
from test_authoring_compact_view import native_grid, restore_cells


PROMPT = "按完整原文核对实际提交结构，未知信息留空。"


def payload(large=False):
    grid = native_grid()
    if large:
        grid['row_count'] = 163
        grid['cells'].extend({'row': row, 'column': column, 'row_span': 1, 'col_span': 1, 'text': ''}
                            for row in range(3, 163) for column in range(2))
    unknown = native_grid()
    unknown['cells'][0]['formula'] = 'SUM(A1:A9)'
    return {'operation': 'structure', 'grounding_policy': 'knowledge_required_v2',
        'chapters': [{'chapter_id': 'c1', 'title': '人员资料', 'is_volume': True,
            'presentation_role': 'body', 'children': [], 'targets': [
                {'target_id': 't1', 'kind': 'form', 'purpose': '本标段适用，其他标段不提交。',
                 'manual_fill': True, 'template_support_ids': ['s1'], 'related_target_ids': [], 'task_ids': ['w1']}]}],
        'sections': [{'section_id': 'section-1', 'heading_path': '实际原格式 > 人员资料'}],
        'supports': [{'support_id': 's1', 'origin': 'tender', 'text': '原表说明，不删除空白行。',
                      'section_id': 'section-1', 'header_row_count': 1, 'grid': grid},
                     {'support_id': 's2', 'origin': 'tender', 'text': '含未知公式，须保留。', 'grid': unknown}],
        'submission_sources': [{'source_id': 's1', 'text': json.dumps(grid, ensure_ascii=False)},
                               {'source_id': 's2', 'text': json.dumps(unknown, ensure_ascii=False)},
                               {'source_id': 's3', 'text': '不适用者不提交，也不编造资料。'},
                               {'source_id': 's4', 'text': '{"ordinary":"不是网格"}'}],
        'submission_rules': [{'rule_id': 'e1', 'text': '保持原条件。', 'source_ids': ['s1', 's3']}],
        'rule_dispositions': [{'code': 'outline_rule_unassigned', 'reason': '适用性待核。',
                              'rules': [{'text': '仅本标段适用。', 'evidence': [{'quote': '仅本标段'}]}]}],
        'input_issues': [], 'units': [{'task_id': 'w1', 'chapter_id': 'c1', 'target_ids': ['t1'],
            'status': 'placeholder', 'blocks_count': 0, 'cell_fills_count': 0,
            'attachment_titles': [], 'error_type': None}]}


def budget():
    return RequestBudget(model='fixture', context_tokens=65536, prompts={
        'authoring_review': PROMPT, 'authoring_structure': PROMPT})


async def test_over_budget_structure_preserves_every_cell_and_only_projects_grid_views(monkeypatch):
    original = payload(large=True)
    before = deepcopy(original)
    source_gap = {'code': 'fixture_source_gap', 'detail': '原有来源缺口保留。'}
    monkeypatch.setattr(authoring_structure, '_structure_payload', lambda *_: (original, [source_gap]))
    admitted = budget()
    assert admitted.measure_request(build_request(admitted.model, PROMPT, original), 'authoring_review')['fits'] is False
    calls = []

    async def execute(task, request, validate):
        calls.append(request)
        admitted.require_request(request, task)
        assert request['messages'][0]['content'].startswith(PROMPT)
        assert 'grid.rows' in request['messages'][0]['content']
        assert 'merged_cells' in request['messages'][0]['content']
        view = json.loads(request['messages'][1]['content'][0]['text'])
        assert view['structure_view'] == 'compact_v1'
        assert restore_cells(view['supports'][0]['grid']) == before['supports'][0]['grid']['cells']
        assert view['supports'][0]['grid'] == view['submission_sources'][0]['grid']
        assert view['supports'][0]['grid']['rows'][0][1] is None
        assert view['supports'][0]['grid']['rows'][1][1] == ''
        assert view['supports'][1]['grid'] == before['supports'][1]['grid']
        assert view['submission_sources'][1]['grid'] == before['supports'][1]['grid']
        restored = deepcopy(view)
        restored.pop('structure_view')
        for number in range(2):
            restored['supports'][number]['grid'] = before['supports'][number]['grid']
            restored['submission_sources'][number].pop('grid')
            restored['submission_sources'][number]['text'] = before['submission_sources'][number]['text']
        assert restored == before
        return validate({'findings': [{'code': 'check_scope', 'detail': '适用性保留待核。',
            'task_ids': ['w1'], 'can_revise': True}]})

    findings, gaps = await authoring_structure.review_structure(None, {'chapters': [{}]},
        [{'task_id': 'w1'}], admitted, execute)
    assert len(calls) == 1 and gaps == [source_gap]
    assert findings[0]['can_revise'] is False
    assert original == before and admitted.prompts['authoring_review'] == PROMPT


async def test_admitted_structure_keeps_exact_original_request_and_shared_prompt(monkeypatch):
    original = payload()
    before = deepcopy(original)
    monkeypatch.setattr(authoring_structure, '_structure_payload', lambda *_: (original, []))
    admitted = budget()
    expected = build_request(admitted.model, PROMPT, original, task="authoring_structure")
    admitted.require_request(expected, 'authoring_review')
    calls = []

    async def execute(task, request, validate):
        calls.append(request)
        assert request == expected
        return validate({'findings': []})

    assert await authoring_structure.review_structure(None, {'chapters': [{}]},
        [{'task_id': 'w1'}], admitted, execute) == ([], [])
    assert len(calls) == 1 and original == before and admitted.prompts['authoring_review'] == PROMPT


async def test_still_oversized_structure_tries_complete_source_before_provider_failure_gap(monkeypatch):
    original = payload(large=True)
    original['submission_sources'].append({'source_id': 's5', 'text': '完整原文' * 10000})
    before = deepcopy(original)
    monkeypatch.setattr(authoring_structure, '_structure_payload', lambda *_: (original, []))
    calls = []

    async def execute(task, request, _validate):
        data = json.loads(request['messages'][1]['content'][0]['text'])
        assert task == 'authoring_structure' and data['structure_view'] == 'compact_v1'
        assert data['submission_sources'][-1] == before['submission_sources'][-1]
        assert budget().measure_request(request, task)['fits'] is False
        calls.append(request)
        raise ModelWorkUnitExhausted(task)

    findings, gaps = await authoring_structure.review_structure(None, {'chapters': [{}]},
        [{'task_id': 'w1'}], budget(), execute)
    assert not findings and len(calls) == 1
    assert [gap['code'] for gap in gaps] == ['structure_review_unavailable']
    assert original == before
