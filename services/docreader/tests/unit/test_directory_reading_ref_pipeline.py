"""Reading-role protocol changes do not rewrite or resend compatible successes."""

from copy import deepcopy
import json

import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.budget import RequestBudget
from docreader.llm.contracts import response_model
from docreader.llm.request import ModelContractError, ModelWorkUnitExhausted
from docreader.tender import document_execution as execution
from docreader.tender.authoring_pipeline import _request_directory_plans
from docreader.tender.directory_compile import compile_plans
from docreader.tender.directory_ref_plan import (
    DIRECTORY_REF_CONTRACT, LEGACY_DIRECTORY_REF_CONTRACT, RefDirectoryPlan, RefDirectoryPlanV2,
)
from test_directory_plan_replay import Receipts, executor


DOCUMENTS = [{'document_id': 'source', 'role': 'main', 'pages': ['投标原式\n固定内容\n签字：________\n阅读目录']}]
RESPONSE = {'nodes': [{'heading_ref': 0, 'body_ranges': [[1, 2]], 'response_kind': 'form'}],
            'scopes': [{'items': [{'node_ref': 0, 'depth': 'self', 'presentation': 'section'}]}]}


def payload(request):
    return json.loads(request['messages'][1]['content'][0]['text'])


def test_explicit_operation_dispatch_preserves_the_legacy_schema():
    assert response_model('directory_plan', {'operation': 'select_directory_refs_v1'}) is RefDirectoryPlan
    assert response_model('directory_plan', {'operation': 'select_directory_refs_v2'}) is RefDirectoryPlanV2
    assert 'reading_only_refs' not in RefDirectoryPlan.model_fields
    assert 'reading_only_refs' in RefDirectoryPlanV2.model_fields


async def test_v12_sends_two_actual_full_source_requests_when_no_accepted_v11_exists():
    requests = []

    async def model(_task, request, validate):
        requests.append(request)
        return validate({**deepcopy(RESPONSE), 'reading_only_refs': [3]})

    plans, failures = await _request_directory_plans(DOCUMENTS, RequestBudget(model='fixture'), model,
                                                    contract=DIRECTORY_REF_CONTRACT)
    assert not failures and len(plans) == len(requests) == 2
    assert [payload(request)['plan_slot'] for request in requests] == [1, 2]
    assert all(payload(request)['operation'] == 'select_directory_refs_v2' for request in requests)
    assert all(payload(request)['documents'][0]['pages'][0]['lines'] ==
               [[0, '投标原式'], [1, '固定内容'], [2, '签字：________'], [3, '阅读目录']] for request in requests)
    assert all('reading_only_refs' in request['messages'][0]['content'] for request in requests)
    compiled = compile_plans(plans, DOCUMENTS)
    assert compiled['_reading_references'] == [{'document_id': 'source', 'page': 0, 'line': 3}]
    assert len(compiled['chapters']) == 1 and '阅读目录' not in compiled['chapters'][0]['text']


@pytest.mark.parametrize('old_successes', [1, 2])
async def test_v12_only_reuses_exact_accepted_legacy_slots_and_sends_other_slots(tmp_path, monkeypatch, old_successes):
    artifacts, store = ArtifactStore(tmp_path), Receipts()
    budget = RequestBudget(model='fixture', context_tokens=131072)
    sent = []

    async def model(_self, request, validate):
        value = payload(request)
        sent.append(deepcopy(request))
        if value['operation'] == 'select_directory_refs_v1':
            if value['plan_slot'] > old_successes:
                raise ModelWorkUnitExhausted('The old second slot was never accepted.')
            return validate(deepcopy(RESPONSE))
        assert value['operation'] == 'select_directory_refs_v2'
        return validate({**deepcopy(RESPONSE), 'reading_only_refs': [3]})

    monkeypatch.setattr(execution._DocumentModel, '__call__', model)
    old, _ = await _request_directory_plans(DOCUMENTS, budget, executor(artifacts, store, budget),
                                            contract=LEGACY_DIRECTORY_REF_CONTRACT)
    previous_receipts = deepcopy(store.rows)
    before = len(sent)
    current, failures = await _request_directory_plans(DOCUMENTS, budget, executor(artifacts, store, budget),
                                                      contract=DIRECTORY_REF_CONTRACT)
    assert len(old) == old_successes and len(current) == 2 and not failures
    assert len(sent) - before == 2 - old_successes
    assert all(store.rows[key] == receipt for key, receipt in previous_receipts.items())
    assert all(payload(request)['operation'] == 'select_directory_refs_v2' for request in sent[before:])
    assert all(current[index] == old[index] for index in range(old_successes))


async def test_source_revision_never_reuses_a_merely_coordinate_compatible_old_response(tmp_path, monkeypatch):
    artifacts, store = ArtifactStore(tmp_path), Receipts()
    budget = RequestBudget(model='fixture', context_tokens=131072)
    sent = []

    async def model(_self, request, validate):
        sent.append(deepcopy(request))
        return validate(deepcopy(RESPONSE))

    monkeypatch.setattr(execution._DocumentModel, '__call__', model)
    await _request_directory_plans(DOCUMENTS, budget, executor(artifacts, store, budget),
                                   contract=LEGACY_DIRECTORY_REF_CONTRACT)
    revised = deepcopy(DOCUMENTS)
    revised[0]['pages'][0] = revised[0]['pages'][0].replace('固定内容', '修订后的固定内容')
    current, failures = await _request_directory_plans(revised, budget, executor(artifacts, store, budget),
                                                      contract=DIRECTORY_REF_CONTRACT)
    assert not failures and len(current) == 2 and len(sent) == 4
    assert all(payload(request)['operation'] == 'select_directory_refs_v2' for request in sent[2:])
    assert '修订后的固定内容' in compile_plans(current, revised)['chapters'][0]['text']


async def test_a_rejected_legacy_reading_reference_is_not_upgraded_into_a_success(tmp_path, monkeypatch):
    artifacts, store = ArtifactStore(tmp_path), Receipts()
    budget = RequestBudget(model='fixture', context_tokens=131072)
    sent = []
    invalid = {**deepcopy(RESPONSE), 'source_only': [3]}
    before = deepcopy(invalid)

    async def model(_self, request, validate):
        value = payload(request)
        sent.append(deepcopy(request))
        if value['operation'] == 'select_directory_refs_v1':
            return validate(invalid)
        return validate({**deepcopy(RESPONSE), 'reading_only_refs': [3]})

    monkeypatch.setattr(execution._DocumentModel, '__call__', model)
    with pytest.raises(ModelContractError):
        await _request_directory_plans(DOCUMENTS, budget, executor(artifacts, store, budget),
                                       contract=LEGACY_DIRECTORY_REF_CONTRACT)
    assert store.rows == {} and invalid == before
    current, failures = await _request_directory_plans(DOCUMENTS, budget, executor(artifacts, store, budget),
                                                      contract=DIRECTORY_REF_CONTRACT)
    assert not failures and len(current) == 2 and len(sent) == 3
    assert len(store.rows) == 2 and invalid == before
