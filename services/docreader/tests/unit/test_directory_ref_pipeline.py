"""The persisted compact contract must dispatch real independent source selections."""

import json

import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.budget import RequestBudget
from docreader.llm.request import ModelConfigurationError, ModelContractError
from docreader.tender.authoring_pipeline import _request_directory_plans
from docreader.tender.directory_compile import compile_plans
import docreader.tender.document_execution as execution
from test_directory_plan_replay import Receipts, executor


CONTRACT = 'source_refs_compact_plan_v11'
DOCUMENTS = [{'document_id': 'source', 'role': 'main', 'pages': ['原式\n固定内容\n空白：________']}]
RESPONSE = {'nodes': [{'heading_ref': 0, 'body_ranges': [[1, 2]], 'response_kind': 'form'}],
            'scopes': [{'items': [{'node_ref': 0, 'depth': 'self', 'presentation': 'section'}]}]}


async def test_compact_plans_send_all_source_lines_and_reconstruct_original_compiler_inputs():
    sent = []

    async def execute(task, request, validate):
        sent.append(request)
        schema = json.loads(request['messages'][0]['content'].splitlines()[-1])
        assert schema['title'] == 'RefDirectoryPlan'
        return validate(RESPONSE)

    budget = RequestBudget(model='fixture', context_tokens=131072)
    plans, failures = await _request_directory_plans(DOCUMENTS, budget, execute,
        {'source_version': 1, 'domain_revision': 2}, contract=CONTRACT)
    assert not failures and len(sent) == len(plans) == 2
    payloads = [json.loads(request['messages'][1]['content'][0]['text']) for request in sent]
    assert [value['plan_slot'] for value in payloads] == [1, 2]
    assert all(value['documents'][0]['pages'][0]['lines'] == [[0, '原式'], [1, '固定内容'], [2, '空白：________']]
               for value in payloads)
    assert all(value['source_ref_format'] == 'host_bound_line_refs_v1' for value in payloads)
    assert plans[0]['nodes'][0]['heading'] == {'document_id': 'source', 'page': 0, 'line': 0}
    compiled = compile_plans(plans, DOCUMENTS)
    assert compiled['chapters'][0]['text'] == '固定内容\n空白：________'


async def test_compact_successes_replay_both_slots_without_sending_after_restart(tmp_path, monkeypatch):
    artifacts, receipts = ArtifactStore(tmp_path), Receipts()
    budget = RequestBudget(model='fixture', context_tokens=131072)
    sent = []

    async def model(_model, request, validate):
        sent.append(request)
        return validate(RESPONSE)

    monkeypatch.setattr(execution._DocumentModel, '__call__', model)
    first = await _request_directory_plans(DOCUMENTS, budget, executor(artifacts, receipts, budget), contract=CONTRACT)
    recovered = await _request_directory_plans(DOCUMENTS, budget, executor(artifacts, receipts, budget), contract=CONTRACT)
    assert recovered == first and not first[1]
    assert len(sent) == len(receipts.rows) == 2


async def test_invalid_compact_range_has_no_successful_paid_response_receipt(tmp_path, monkeypatch):
    artifacts, receipts = ArtifactStore(tmp_path), Receipts()
    budget = RequestBudget(model='fixture', context_tokens=131072)

    async def model(_model, _request, validate):
        return validate({'nodes': [{'heading_ref': 0, 'body_ranges': [[1, 99]], 'response_kind': 'form'}]})

    monkeypatch.setattr(execution._DocumentModel, '__call__', model)
    with pytest.raises(ModelContractError):
        await _request_directory_plans(DOCUMENTS, budget, executor(artifacts, receipts, budget), contract=CONTRACT)
    assert receipts.rows == {}


async def test_unknown_directory_contract_is_rejected_before_model_dispatch():
    async def execute(*_args):
        raise AssertionError('unknown contracts cannot send a request')

    with pytest.raises(ModelConfigurationError):
        await _request_directory_plans(DOCUMENTS, RequestBudget(model='fixture'), execute, contract='unknown-contract')


async def test_source_rejection_locates_model_refs_instead_of_host_generated_node_hashes():
    response = {'nodes': [{'heading_ref': reference, 'body_ranges': [[2, 2]], 'response_kind': 'form'}
                          for reference in (0, 1)],
                'scopes': [{'items': [{'node_ref': reference, 'depth': 'self', 'presentation': 'section'}
                                     for reference in (0, 1)]}]}

    async def execute(_task, _request, validate):
        return validate(response)

    with pytest.raises(ModelContractError) as caught:
        await _request_directory_plans(DOCUMENTS, RequestBudget(model='fixture'), execute, contract=CONTRACT)
    detail = caught.value.errors[0]['detail']
    assert 'heading_ref=0' in detail and 'heading_ref=1' in detail
    assert 'source-node-' not in detail
