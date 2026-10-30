"""Structure compression shares literal prose without changing its reference view."""
from copy import deepcopy
import json

import pytest

from docreader.tender.authoring_view import compact_structure_payload
from docreader.tender import authoring_structure
from docreader.llm.budget import RequestBudget
from test_authoring_shared_text import restore_text


def payload():
    line = '　条件ＡＢＣ e\u0301  123；保留\t空白、标点与🙂。' * 12 + '\r'
    text = '\n' + line + '\n\n123\n' + line + '\n'
    return {'operation': 'structure', 'sections': [{'section_id': 'section-b', 'heading_path': '甲 > 乙'}],
        'chapters': [{'chapter_id': 'c1', 'children': [], 'targets': [
            {'target_id': 'v1', 'template_support_ids': ['s2', 's1'],
             'source_support_ids': ['s1'], 'response_context_support_ids': ['s2'],
             'response_rows': [{'row_id': 'q1', 'source_support_ids': ['s1']}]}]}],
        'supports': [{'support_id': 's2', 'text': text, 'section_id': 'section-b'},
                     {'support_id': 's1', 'text': line, 'section_id': 'section-b'}],
        'submission_sources': [{'source_id': 'source-b', 'text': line},
                               {'source_id': 'source-a', 'text': text}],
        'submission_rules': [{'rule_id': 'e1', 'conditions': ['仅本范围'], 'source_ids': ['source-a']}],
        'units': [], 'input_issues': []}


def restore(view):
    result = deepcopy(view)
    for field in ('supports', 'submission_sources'):
        for entry in result[field]:
            if 'text_lines' in entry:
                entry['text'] = restore_text(entry, view)
                entry.pop('text_lines')
    result.pop('text_pool', None)
    result.pop('structure_view')
    return result


def test_structure_pool_restores_every_character_identity_order_condition_and_section():
    original = payload()
    before = deepcopy(original)
    view = compact_structure_payload(original)
    assert 'text_pool' in view
    assert '\n123\n' in restore_text(view['supports'][0], view)
    assert restore(view) == before
    assert original == before
    assert len(json.dumps(view, ensure_ascii=False).encode()) < len(json.dumps(before, ensure_ascii=False).encode())


def test_structure_without_net_saving_keeps_the_complete_inline_view():
    original = {'supports': [{'support_id': 's2', 'text': '一'}, {'support_id': 's1', 'text': '一'}],
                'submission_sources': [{'source_id': 'source-1', 'text': '独有原文。'}], 'sections': []}
    assert compact_structure_payload(original) == {**original, 'structure_view': 'compact_v1'}


@pytest.mark.parametrize('unknown', ['grid', 'cell'])
def test_structure_sharing_preserves_unknown_native_grid_metadata(unknown):
    original = payload()
    grid = {'row_count': 1, 'column_count': 1, 'cells': [
        {'row': 0, 'column': 0, 'row_span': 1, 'col_span': 1, 'text': '实际条件'}]}
    if unknown == 'grid':
        grid['native_metadata'] = {'keep': True}
    else:
        grid['cells'][0]['formula'] = 'SUM(A1:A2)'
    original['supports'][0]['grid'] = grid
    view = compact_structure_payload(original)
    assert 'text_pool' in view
    assert view['supports'][0]['grid'] == grid
    assert restore(view) == original


def test_structure_compaction_never_replaces_an_existing_pool_or_its_indices():
    original = payload()
    original['text_pool'] = ['Existing source line.']
    original['supports'].append({'support_id': 's3', 'text_lines': [0, '', '123', 0]})
    assert compact_structure_payload(original) == {**original, 'structure_view': 'compact_v1'}


async def test_compact_structure_request_explains_the_same_lossless_text_representation(monkeypatch):
    original = payload()
    monkeypatch.setattr(authoring_structure, '_structure_payload', lambda *_, **_kw: (original, []))
    budget = RequestBudget(model='fixture', context_tokens=65536, prompts={'authoring_structure': '原文核对。'})

    class Executor:
        async def replay(self, _task, request, validate):
            view = json.loads(request['messages'][1]['content'][0]['text'])
            if 'structure_view' not in view:
                return False, None
            assert 'text_pool' in view
            prompt = request['messages'][0]['content']
            assert all(term in prompt for term in ('text_pool', 'text_lines', '从0', '换行'))
            assert restore(view) == original
            return True, validate({'findings': []})

        async def __call__(self, *_args):
            raise AssertionError('Accepted compact replay must not send a request')

    def require_compact(*_args):
        raise authoring_structure.ModelBudgetError('fixture requests compact replay')

    monkeypatch.setattr(budget, 'require_request', require_compact)
    assert await authoring_structure.review_structure(None, {'chapters': [{}]}, [], budget, Executor()) == ([], [])
