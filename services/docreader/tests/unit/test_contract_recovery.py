"""One bounded recovery policy per model work unit, with durable feedback."""
from dataclasses import replace
from copy import deepcopy
import asyncio
import json

import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.integrations.store import Cancelled, Lease, StaleExecution
from docreader.llm.request import (
    ModelBudgetError, ModelConfigurationError, ModelContractError, ModelCorrectionStalled,
    ModelInfrastructureError, ModelSemanticError, ModelWorkUnitExhausted, UncertainModelResponse,
)
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
        if isinstance(result, BaseException):
            raise result
        return result


def validate(value):
    if not isinstance(value.get('findings'), list):
        raise ModelContractError([{'path': ['findings'], 'code': 'list_type', 'detail': 'Expected an array'}])
    return value


@pytest.mark.asyncio
async def test_rejection_is_durable_and_same_execution_gets_real_feedback(tmp_path):
    store, artifacts = Store(), ArtifactStore(tmp_path)
    lease = Lease('run', 1, 1, 'command')
    payload = {'operation': 'review', 'source_views': [{'text': '完整原文'}]}
    model = Model([{'findings': None}, {'findings': []}])
    result = await call_validated(model, store, artifacts, lease, 'verify', payload, validate)
    assert result == {'findings': []}
    assert len(model.calls) == 2
    rejection = artifacts.read_json(next(iter(store.rows.values()))['artifact_ref'])
    assert rejection['errors'][0]['path'] == ['findings']
    assert artifacts.read_json(rejection['response_ref']) == {'findings': None}
    sent = model.calls[1]
    assert sent['source_views'] == payload['source_views']
    assert sent['contract_feedback']['errors'] == rejection['errors']
    assert sent['contract_feedback']['fragments'] == [{'path': ['findings'], 'value': None}]
    assert sent['contract_feedback']['rejection_count'] == 1
    assert 'contract_feedback' not in payload


@pytest.mark.asyncio
async def test_same_invalid_correction_gets_three_real_retries_before_exhaustion(tmp_path):
    store, artifacts = Store(), ArtifactStore(tmp_path)
    lease = Lease('run', 1, 1, 'command')
    model = Model([{'findings': None}] * 4 + [{'findings': []}])
    with pytest.raises(ModelWorkUnitExhausted):
        await call_validated(model, store, artifacts, lease, 'verify', {}, validate)
    assert len(model.calls) == 4 and len(store.rows) == 4
    assert [call['contract_feedback']['rejection_count'] for call in model.calls[1:]] == [1, 2, 3]
    assert all(call['contract_feedback']['fragments'] == [{'path': ['findings'], 'value': None}]
               for call in model.calls[1:])


@pytest.mark.asyncio
@pytest.mark.parametrize('error_type', [UncertainModelResponse, RuntimeError, ModelBudgetError])
async def test_uncertain_and_other_call_errors_share_the_work_unit_retry_policy(tmp_path, error_type):
    store, artifacts = Store(), ArtifactStore(tmp_path)
    lease = Lease('run', 1, 1, 'command')
    model = Model([error_type('private failure')] * 3 + [{'findings': []}])
    assert await call_validated(model, store, artifacts, lease, 'verify', {}, validate) == {'findings': []}
    assert model.calls == [{}] * 4
    assert store.rows == {}


@pytest.mark.asyncio
@pytest.mark.parametrize('error', [Cancelled('cancelled'), StaleExecution('lost fence'), asyncio.CancelledError(),
                                 ModelWorkUnitExhausted('verify')])
async def test_cancellation_lost_fence_and_exhaustion_escape_without_retry(tmp_path, error):
    store, model = Store(), Model([error, {'findings': []}])
    with pytest.raises(type(error)):
        await call_validated(model, store, ArtifactStore(tmp_path), Lease('run', 1, 1, 'command'),
                             'verify', {}, validate)
    assert len(model.calls) == 1 and not store.rows


@pytest.mark.asyncio
async def test_validation_code_errors_retry_without_sending_the_stack_to_the_model(tmp_path):
    store, model = Store(), Model([{}] * 4)
    validations = []
    def broken(value):
        validations.append(value)
        if len(validations) < 4:
            raise RuntimeError('private programming fault')
        return value
    assert await call_validated(model, store, ArtifactStore(tmp_path), Lease('run', 1, 1, 'command'),
                                'verify', {'source': 'keep'}, broken) == {}
    assert len(validations) == 4 and model.calls == [{'source': 'keep'}] * 4
    assert store.rows == {}


@pytest.mark.asyncio
async def test_validation_code_errors_exhaust_without_a_fifth_call(tmp_path):
    store, model = Store(), Model([{}] * 5)
    def broken(value):
        raise RuntimeError('private programming fault')
    with pytest.raises(ModelWorkUnitExhausted) as exhausted:
        await call_validated(model, store, ArtifactStore(tmp_path), Lease('run', 1, 1, 'command'),
                             'verify', {}, broken)
    assert len(model.calls) == 4 and not store.rows
    assert isinstance(exhausted.value.__cause__, RuntimeError)


@pytest.mark.asyncio
async def test_corrupt_cache_gets_real_safe_feedback_and_a_new_correction_request(tmp_path):
    store, artifacts = Store(), ArtifactStore(tmp_path)
    model = Model([ModelCorrectionStalled('private cached response fragment'), {'findings': []}])
    assert await call_validated(model, store, artifacts, Lease('run', 1, 1, 'command'),
                                'verify', {}, validate) == {'findings': []}
    feedback = model.calls[1]['contract_feedback']
    assert feedback['rejection_count'] == 1 and feedback['fragments'] == []
    assert feedback['errors'][0]['code'] == 'cached_response_invalid'
    assert 'private' not in json.dumps(feedback)
    rejection = artifacts.read_json(next(iter(store.rows.values()))['artifact_ref'])
    assert rejection['response_kind'] == 'invalid_cache' and rejection['feedback'] == feedback


@pytest.mark.asyncio
async def test_separate_work_units_each_recover_from_transient_failures(tmp_path):
    store, artifacts = Store(), ArtifactStore(tmp_path)
    lease = Lease('run', 1, 1, 'command')
    model = Model([ModelSemanticError('invalid JSON'), ModelInfrastructureError('HTTP 500'),
                   {'findings': []}, ModelInfrastructureError('HTTP 500'),
                   ModelSemanticError('invalid JSON'), {'findings': []}])
    for identity in ('first', 'second'):
        assert await call_validated(model, store, artifacts, lease, 'verify',
                                    {'work_unit_id': identity}, validate) == {'findings': []}
    assert len(model.calls) == 6


@pytest.mark.asyncio
async def test_work_unit_stops_after_four_confirmed_failures(tmp_path):
    from docreader.llm.request import ModelWorkUnitExhausted

    model = Model([ModelInfrastructureError('HTTP 500')] * 4 + [{'findings': []}])
    with pytest.raises(ModelWorkUnitExhausted):
        await call_validated(model, Store(), ArtifactStore(tmp_path), Lease('run', 1, 1, 'command'),
                             'verify', {}, validate)
    assert len(model.calls) == 4


@pytest.mark.asyncio
async def test_configuration_http_rejections_retry_within_the_work_unit(tmp_path):
    store = Store()
    model = Model([ModelConfigurationError(f'HTTP {status}') for status in (401, 403, 404)]
                  + [{'findings': []}])
    payload = {'work_unit_id': 'review-1'}
    assert await call_validated(model, store, ArtifactStore(tmp_path), Lease('run', 1, 1, 'command'),
                                'verify', payload, validate) == {'findings': []}
    assert model.calls == [payload] * 4
    assert store.rows == {}


@pytest.mark.asyncio
async def test_configuration_and_transport_failures_share_the_four_attempt_limit(tmp_path):
    from docreader.llm.request import ModelWorkUnitExhausted

    model = Model([ModelConfigurationError('HTTP 401'), ModelInfrastructureError('HTTP 500'),
                   ModelConfigurationError('HTTP 403'), ModelConfigurationError('HTTP 404'),
                   {'findings': []}])
    with pytest.raises(ModelWorkUnitExhausted) as exhausted:
        await call_validated(model, Store(), ArtifactStore(tmp_path), Lease('run', 1, 1, 'command'),
                             'verify', {}, validate)
    assert len(model.calls) == 4
    assert isinstance(exhausted.value.__cause__, ModelConfigurationError)


@pytest.mark.asyncio
async def test_json_contract_and_transport_recovery_share_four_attempts(tmp_path):
    from docreader.llm.request import ModelJSONError, ModelWorkUnitExhausted

    invalid = ModelJSONError('json_syntax_error', 'Extra data', '{"findings":[]}]}',
                             line=1, column=16, offset=15)
    model = Model([invalid, {'findings': None}, ModelInfrastructureError('HTTP 500'),
                   {'findings': {}}, {'findings': []}])
    store, artifacts = Store(), ArtifactStore(tmp_path)
    with pytest.raises(ModelWorkUnitExhausted):
        await call_validated(model, store, artifacts, Lease('run', 1, 1, 'command'),
                             'verify', {'source_views': [{'text': '必须保留原文'}]}, validate)
    assert len(model.calls) == 4
    feedback = model.calls[1]['contract_feedback']
    assert len(json.dumps(feedback, ensure_ascii=False, separators=(',', ':'))) <= 512
    assert feedback['rejection_count'] == 1
    assert feedback['errors'][0]['offset'] == 15
    assert ']}' in feedback['fragments'][0]['value']
    assert len(store.rows) == 3
    assert all(call['source_views'] == [{'text': '必须保留原文'}] for call in model.calls)


@pytest.mark.asyncio
async def test_identical_invalid_json_feedback_can_retry_a_failed_transport_receipt(tmp_path):
    from docreader.llm.request import ModelJSONError

    invalid = ModelJSONError('json_syntax_error', 'Extra data', '{"findings":[]}]}',
                             line=1, column=16, offset=15)
    model = Model([invalid, invalid, {'findings': []}])
    result = await call_validated(model, Store(), ArtifactStore(tmp_path), Lease('run', 1, 1, 'command'),
                                  'verify', {}, validate)
    assert result == {'findings': []}
    assert len(model.calls) == 3
    assert model.calls[1]['contract_feedback']['rejection_count'] == 1
    assert model.calls[2]['contract_feedback']['rejection_count'] == 2
    assert model.calls[1]['contract_feedback']['errors'] == model.calls[2]['contract_feedback']['errors']


@pytest.mark.asyncio
async def test_json_feedback_stays_bounded_after_adding_the_rejection_count(tmp_path):
    from docreader.llm.request import ModelJSONError

    invalid = ModelJSONError('json_syntax_error', 'Extra data', '中' * 1000,
                             line=999999, column=999999, offset=500)
    model = Model([invalid, {'findings': []}])
    assert await call_validated(model, Store(), ArtifactStore(tmp_path), Lease('run', 1, 1, 'command'),
                                'verify', {}, validate) == {'findings': []}
    feedback = model.calls[1]['contract_feedback']
    assert feedback['rejection_count'] == 1
    assert len(json.dumps(feedback, ensure_ascii=False, separators=(',', ':'))) <= 512


@pytest.mark.asyncio
async def test_restart_uses_persisted_feedback_after_receipt_write_ack_is_lost(tmp_path):
    class LostAcknowledgement(Store):
        async def commit_artifact(self, *args, **kwargs):
            await super().commit_artifact(*args, **kwargs)
            if len(self.rows) == 1:
                raise ConnectionError('receipt committed but acknowledgement lost')

    store, artifacts = LostAcknowledgement(), ArtifactStore(tmp_path)
    lease = Lease('run', 1, 1, 'command')
    model = Model([{'findings': None}, {'findings': []}])
    with pytest.raises(ConnectionError):
        await call_validated(model, store, artifacts, lease, 'verify', {}, validate)
    assert len(model.calls) == 1
    assert await call_validated(model, store, artifacts, replace(lease, epoch=2),
                                'verify', {}, validate) == {'findings': []}
    assert model.calls[-1]['contract_feedback']['fragments'] == [{'path': ['findings'], 'value': None}]
    assert model.calls[-1]['contract_feedback']['rejection_count'] == 1
