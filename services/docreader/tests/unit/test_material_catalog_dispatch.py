"""Only the pinned v3 capability may compact an oversized material request."""

from copy import deepcopy
import json

import pytest

from docreader.llm.budget import ModelBudgetError, RequestBudget
from docreader.tender.authoring_pipeline import _ask, _request
from docreader.tender.directory_material_ids import MaterialIds
from test_directory_material_ids import payload, response


V2, V3 = 'material_short_ids_v2', 'material_short_ids_v3'


def case():
    original = payload()
    original['chapters'] = [{'id': f'chapter-{i:03}', 'parent_id': None, 'response_kind': 'form',
        'subjects': [], 'title': '完整材料名称与来源映射'} for i in range(100)]
    probe = RequestBudget(model='fixture')
    old = _request(probe, 'directory_material', MaterialIds(original).payload)
    compact = _request(probe, 'directory_material', MaterialIds(original, contract=V3).payload)
    first = probe.require_request(old, 'directory_material')['input_tokens']
    second = probe.require_request(compact, 'directory_material')['input_tokens']
    assert second < first
    budget = RequestBudget(model='fixture', context_tokens=(first + second) // 2 + 4096 + 1024,
                           output_reserve=1024)
    with pytest.raises(ModelBudgetError):
        budget.admit_request(_request(budget, 'directory_material', MaterialIds(original).payload), 'directory_material')
    budget.admit_request(_request(budget, 'directory_material', MaterialIds(original, contract=V3).payload), 'directory_material')
    return original, budget


class Execute:
    def __init__(self, original, contract, *, cached=False):
        self.spec = {'directory_material_identity_contract': contract}
        self.original, self.cached, self.calls, self.requests = original, cached, 0, []

    async def replay(self, task, request, validate):
        data = json.loads(request['messages'][1]['content'][0]['text'])
        self.requests.append(data.get('identity_format'))
        if self.cached and data.get('identity_format') == V2:
            return True, validate(response())
        return False, None

    async def __call__(self, task, request, validate):
        self.calls += 1
        self.sent = deepcopy(request)
        return validate(response())


async def test_oversized_material_uses_v3_once_and_restores_the_original_source_identities():
    original, budget = case()
    before = deepcopy(original)
    execute = Execute(original, V3)
    result = await _ask(execute, budget, 'directory_material', original, lambda value: value)
    assert result == MaterialIds(original).decode(response())
    assert execute.calls == 1 and original == before
    sent = json.loads(execute.sent['messages'][1]['content'][0]['text'])
    assert sent['identity_format'] == V3 and sent['catalog_record_format'] == 'column_blocks_v1'
    assert sent['documents'] == MaterialIds(original).payload['documents']
    assert 'constant_values' in execute.sent['messages'][0]['content']
    assert 'columns' in execute.sent['messages'][0]['content']


async def test_exact_legacy_success_is_revalidated_before_new_format_or_budget_admission():
    original, budget = case()
    execute = Execute(original, V3, cached=True)
    result = await _ask(execute, budget, 'directory_material', original, lambda value: value)
    assert result == MaterialIds(original).decode(response()) and execute.calls == 0
    assert execute.requests == [None, V2]


async def test_legacy_execution_does_not_silently_change_its_material_representation():
    original, budget = case()
    execute = Execute(original, V2)
    with pytest.raises(ModelBudgetError):
        await _ask(execute, budget, 'directory_material', original, lambda value: value)
    assert execute.calls == 0 and execute.requests == [None, V2]


async def test_v3_execution_keeps_legacy_wire_when_the_full_legacy_request_fits():
    original = payload()
    execute = Execute(original, V3)
    await _ask(execute, RequestBudget(model='fixture'), 'directory_material', original, lambda value: value)
    assert execute.calls == 1
    sent = json.loads(execute.sent['messages'][1]['content'][0]['text'])
    assert sent == MaterialIds(original).payload
    assert 'catalog_record_format' not in sent
