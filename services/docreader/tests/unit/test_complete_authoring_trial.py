"""Complete assembly/structure calls try the provider beyond local estimates."""

from copy import deepcopy
import json

import pytest

from docreader.llm.budget import RequestBudget
from docreader.tender import authoring_pipeline as pipeline, authoring_structure
from docreader.tender.authoring_view import compact_assembly_payload
from test_authoring_compact_view import native_grid, payload as assembly_payload
from test_structure_compact_view import PROMPT, payload as structure_payload


def small_budget():
    return RequestBudget(model='fixture', context_tokens=100,
        prompts={'outline_assemble': PROMPT, 'authoring_structure': PROMPT})


@pytest.mark.parametrize('already_compact', [False, True])
async def test_complete_assembly_feedback_compacts_once_and_still_reaches_executor(already_compact):
    original = assembly_payload(native_grid())
    original['previous_structure'] = [{'title': '原位置', 'targets': [{'review_target_id': 'v1'}]}]
    original['review_findings'] = [{'code': 'check_scope', 'detail': '依据原格式保持适用范围。'}]
    if already_compact:
        original = compact_assembly_payload(original)
    before = deepcopy(original)
    admitted = small_budget()
    calls = []

    async def execute(task, request, validate):
        assert task == 'outline'
        data = json.loads(request['messages'][1]['content'][0]['text'])
        expected = before if already_compact else compact_assembly_payload(before)
        assert data == expected
        assert admitted.measure_request(request, task)['fits'] is False
        calls.append(request)
        return validate({'accepted': True})

    value = await pipeline._ask(execute, admitted, 'outline', original, lambda value: value)
    assert value == {'accepted': True} and len(calls) == 1 and original == before


@pytest.mark.parametrize('outline_phase', [False, True])
async def test_complete_structure_still_calls_executor_after_compact_exceeds_estimate(monkeypatch, outline_phase):
    original = structure_payload(large=True)
    monkeypatch.setattr(authoring_structure, '_structure_payload', lambda *_a, **_kw: (original, []))
    admitted = small_budget()
    calls = []

    async def execute(task, request, validate):
        data = json.loads(request['messages'][1]['content'][0]['text'])
        assert task == 'authoring_structure' and data['structure_view'] == 'compact_v1'
        assert data['supports'][0]['grid']['rows'][0][0] == '仅本标段填写'
        assert admitted.measure_request(request, task)['fits'] is False
        calls.append(request)
        return validate({'findings': []})

    assert await authoring_structure.review_structure(None, {'chapters': [{}]}, [], admitted,
        execute, outline_phase=outline_phase) == ([], [])
    assert len(calls) == 1
