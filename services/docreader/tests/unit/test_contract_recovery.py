"""Rejected complete responses recover across executions, never in a local loop."""
from dataclasses import replace
from copy import deepcopy

import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.integrations.store import Lease
from docreader.llm.request import ModelContractError, ModelCorrectionStalled, UncertainModelResponse
from docreader.llm.recovery import call_validated


class Store:
    def __init__(self):
        self.rows = {}

    async def receipt(self, lease, stage, scope='', *, input_digest=''):
        return self.rows.get((stage, scope, input_digest))

    async def commit_artifact(self, lease, stage, artifact_ref, scope='', *, input_digest='', **kwargs):
        self.rows[stage, scope, input_digest] = {'artifact_ref': artifact_ref}


class Model:
    def __init__(self, values):
        self.values, self.calls = iter(values), []

    async def call(self, task, payload):
        self.calls.append(deepcopy(payload))
        result = next(self.values)
        if isinstance(result, Exception):
            raise result
        return result


def validate(value):
    if not isinstance(value.get('findings'), list):
        raise ModelContractError([{'path': ['findings'], 'code': 'list_type', 'detail': 'Expected an array'}])
    return value


@pytest.mark.asyncio
async def test_rejection_is_durable_and_next_execution_gets_real_feedback(tmp_path):
    store, artifacts = Store(), ArtifactStore(tmp_path)
    lease = Lease('run', 1, 1, 'command')
    payload = {'operation': 'review', 'source_views': [{'text': '完整原文'}]}
    model = Model([{'findings': None}, {'findings': []}])
    with pytest.raises(ModelContractError):
        await call_validated(model, store, artifacts, lease, 'verify', payload, validate)
    assert len(model.calls) == 1
    rejection = artifacts.read_json(next(iter(store.rows.values()))['artifact_ref'])
    assert rejection['errors'][0]['path'] == ['findings']
    assert artifacts.read_json(rejection['response_ref']) == {'findings': None}
    # A repeat inside this execution cannot become an unbudgeted local retry.
    with pytest.raises(ModelContractError):
        await call_validated(model, store, artifacts, lease, 'verify', payload, validate)
    assert len(model.calls) == 1
    result = await call_validated(model, store, artifacts, replace(lease, epoch=2), 'verify', payload, validate)
    assert result == {'findings': []}
    sent = model.calls[1]
    assert sent['source_views'] == payload['source_views']
    assert sent['contract_feedback']['errors'] == rejection['errors']
    assert sent['contract_feedback']['fragments'] == [{'path': ['findings'], 'value': None}]
    assert 'contract_feedback' not in payload


@pytest.mark.asyncio
async def test_same_invalid_correction_stops_without_replaying_paid_cache(tmp_path):
    store, artifacts = Store(), ArtifactStore(tmp_path)
    lease = Lease('run', 1, 1, 'command')
    model = Model([{'findings': None}, {'findings': None}])
    for epoch in (1, 2):
        with pytest.raises(ModelContractError):
            await call_validated(model, store, artifacts, replace(lease, epoch=epoch), 'verify', {}, validate)
    with pytest.raises(ModelCorrectionStalled):
        await call_validated(model, store, artifacts, replace(lease, epoch=3), 'verify', {}, validate)
    assert len(model.calls) == 2


@pytest.mark.asyncio
async def test_uncertain_and_code_errors_do_not_become_contract_corrections(tmp_path):
    store, artifacts = Store(), ArtifactStore(tmp_path)
    lease = Lease('run', 1, 1, 'command')
    with pytest.raises(UncertainModelResponse):
        await call_validated(Model([UncertainModelResponse()]), store, artifacts, lease, 'verify', {}, validate)
    assert store.rows == {}
    def broken(value):
        raise RuntimeError('programming fault')
    with pytest.raises(RuntimeError):
        await call_validated(Model([{}]), store, artifacts, lease, 'verify', {}, broken)
    assert store.rows == {}
