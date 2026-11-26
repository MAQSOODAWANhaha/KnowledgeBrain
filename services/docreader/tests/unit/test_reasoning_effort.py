"""Explicit request options are frozen before model audit, hashing and recovery."""

from typing import TypeVar
from types import SimpleNamespace
from unittest.mock import AsyncMock
from contextlib import asynccontextmanager
from copy import deepcopy
import json

import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.budget import RequestBudget
from docreader.llm.config import ModelConfig
from docreader.llm.request import ModelConfigurationError, ModelSemanticError, build_request, request_digest
from docreader.tender.document_execution import _DocumentExecutor

from test_tender_llm import environment, model, response

_T = TypeVar("_T")


def _required(value: _T | None) -> _T:
    assert value is not None
    return value



async def test_explicit_environment_option_reaches_real_analysis_body_and_execution_identity():
    sent = []
    client, _ = model(lambda request: (sent.append(json.loads(request.content)), response())[1],
                      values=environment(KNOWLEDGEBRAIN_CHAT_REASONING_EFFORT=' medium '))
    async with client:
        await client.call('extract', {'fixture': 'same input'})
    assert sent[0].get('reasoning_effort') == 'medium'
    assert client.request_budget().spec()['reasoning_effort'] == 'medium'
    assert client.execution_spec()['chat']['reasoning_effort'] == 'medium'
    assert 'max_tokens' not in sent[0] and 'max_completion_tokens' not in sent[0]


@pytest.mark.parametrize('value', ['unsupported-secret-value', 'turbo', 'ultra'])
def test_invalid_explicit_option_is_a_configuration_error_without_echoing_value(value):
    with pytest.raises(ModelConfigurationError) as error:
        ModelConfig.from_env(environment(KNOWLEDGEBRAIN_CHAT_REASONING_EFFORT=value))
    assert value not in str(error.value)


@pytest.mark.parametrize('value', ['none', 'minimal', 'low', 'medium', 'high', 'xhigh'])
def test_all_standard_reasoning_effort_values_are_accepted_for_chat_and_outline(value):
    cfg = ModelConfig.from_env(environment(
        KNOWLEDGEBRAIN_CHAT_REASONING_EFFORT=f' {value.upper()} ',
        KNOWLEDGEBRAIN_OUTLINE_REASONING_EFFORT=f' {value} ',
    ))
    assert cfg.chat.reasoning_effort == value
    assert cfg.for_outline().chat.reasoning_effort == value
    req = build_request(cfg.chat.model, 'Return JSON.', {'fixture': True}, reasoning_effort=value)
    assert req['reasoning_effort'] == value


@pytest.mark.parametrize('value', [None, '', '   '])
def test_unconfigured_option_preserves_the_legacy_body_and_spec(value):
    values = environment()
    if value is not None:
        values['KNOWLEDGEBRAIN_CHAT_REASONING_EFFORT'] = value
    config = ModelConfig.from_env(values)
    client, _ = model(lambda _: response(), values=values)
    actual = build_request(config.chat.model, 'Return JSON.', {'fixture': True})
    expected = {'model': config.chat.model,
        'messages': [{'role': 'system', 'content': 'Return JSON.'},
                     {'role': 'user', 'content': [{'type': 'text', 'text': '{"fixture":true}'}]}],
        'response_format': {'type': 'json_object'}, 'stream': True, 'stream_options': {'include_usage': True}}
    assert actual == expected
    assert json.dumps(actual, ensure_ascii=False) == json.dumps(expected, ensure_ascii=False)
    assert 'reasoning_effort' not in client.request_budget().spec()
    assert 'reasoning_effort' not in client.execution_spec()['chat']


@pytest.mark.parametrize('replay', [False, True])
async def test_configured_replay_cannot_reuse_an_old_body_that_omits_the_option(tmp_path, replay):
    artifacts = ArtifactStore(tmp_path)
    request = build_request('fixture', 'Return JSON.', {'fixture': True})
    digest = request_digest('https://model.example.test/v1', request)
    old = artifacts.put_json({'task': 'authoring_write', 'input_digest': digest,
                              'response': {'content': 'old default response'}})
    previous = {'execution_id': 'previous', 'receipts': [{'stage': 'document-model-response',
        'scope': 'authoring_write:' + digest, 'input_digest': digest, 'artifact_ref': old}]}
    client = SimpleNamespace(config=SimpleNamespace(chat=SimpleNamespace(
        base_url='https://model.example.test/v1', reasoning_effort='medium')))
    budget = RequestBudget(model='fixture', prompts={})
    budget.reasoning_effort = 'medium'
    store = SimpleNamespace(receipt=AsyncMock(return_value=None), commit_artifact=AsyncMock())
    executor = _DocumentExecutor(client, store, artifacts, SimpleNamespace(execution_id='current'),
                                 budget, {}, previous)
    with pytest.raises(ModelConfigurationError):
        if replay:
            await executor.replay('authoring_write', request, lambda value: value)
        else:
            await executor('authoring_write', request, lambda value: value)
    store.receipt.assert_not_awaited()
    store.commit_artifact.assert_not_awaited()


@pytest.mark.parametrize('operation', ['source', 'assemble'])
def test_authoring_requests_carry_the_option_before_budgeting_and_hashing(operation):
    from docreader.tender.authoring_pipeline import _request

    budget = RequestBudget(model='fixture', prompts={'outline': 'Source.', 'outline_assemble': 'Assemble.'},
                           reasoning_effort='medium')
    request = _request(budget, 'outline', {'operation': operation})
    assert request.get('reasoning_effort') == 'medium'
    assert budget.measure_request(request, 'outline')['wire_bytes'] > 0


@pytest.mark.parametrize('compact', [False, True])
async def test_structure_option_survives_both_ordinary_and_compact_requests(monkeypatch, compact):
    from docreader.tender import authoring_structure
    from test_structure_compact_view import payload, budget as structure_budget

    original = payload(large=compact)
    monkeypatch.setattr(authoring_structure, '_structure_payload', lambda *_, **__: (original, []))
    budget = structure_budget()
    budget.reasoning_effort = 'medium'
    sent = []

    async def execute(task, request, validate):
        sent.append(deepcopy(request))
        assert request.get('reasoning_effort') == 'medium'
        data = json.loads(request['messages'][1]['content'][0]['text'])
        assert ('structure_view' in data) is compact
        return validate({'findings': []})

    await authoring_structure.review_structure(None, {'chapters': [{}]}, [{'task_id': 'w1'}], budget, execute)
    assert len(sent) == 1


async def test_revision_constructs_the_explicit_option_before_replay(tmp_path):
    from docreader.tender.authoring_revision import revise_units
    import test_authoring_revision as fixture

    budget = fixture.budget()
    budget.reasoning_effort = 'medium'
    captured = []

    async def execute(task, request, validate):
        captured.append(request)
        assert request.get('reasoning_effort') == 'medium'
        return validate(fixture.response('依据已有支持修订的正文'))

    units, _, _ = await revise_units(ArtifactStore(tmp_path), budget, execute,
        [fixture.task()], [fixture.unit()], [fixture.finding()], fixture.payload, fixture.bind)
    assert len(captured) == 1 and units[0]['blocks'][0]['text'] == '依据已有支持修订的正文'


async def test_web_job_uses_the_environment_snapshot_option_in_its_actual_budget(monkeypatch):
    from docreader.integrations import web
    from docreader.tender import document_execution

    config = ModelConfig.from_env(environment(KNOWLEDGEBRAIN_CHAT_REASONING_EFFORT='medium'))
    monkeypatch.setattr(ModelConfig, 'from_env', classmethod(lambda _: config))
    job = {'status': 'pending', 'project_id': 'fixture-project', 'payload': {'owner_user_id': 'fixture-owner',
        'operation': 'outline', 'request': {}, 'materials': [], 'identity': {
            'run_id': 'fixture-run', 'source_version': 1, 'domain_revision': 1}}}
    statements, requests = [], []

    async def sql(statement, *_):
        statements.append(statement)
        return SimpleNamespace(fetchone=AsyncMock(return_value=deepcopy(job)))

    @asynccontextmanager
    async def transaction():
        yield SimpleNamespace(execute=sql)

    bridge = SimpleNamespace(setup=AsyncMock(), authorize=AsyncMock(), artifacts=None,
        store=SimpleNamespace(transaction=transaction), job=AsyncMock(return_value={'status': 'running'}),
        authoring=SimpleNamespace(get_execution=AsyncMock(return_value=None)))
    monkeypatch.setattr(web, 'WebBridge', lambda _: bridge)

    async def execute(_store, _artifacts, actual_config, budget, **_):
        assert budget.reasoning_effort == actual_config.chat.reasoning_effort == 'medium'
        requests.append(budget.request('extract', {'fixture': True}))

    monkeypatch.setattr(document_execution, 'execute_document', execute)
    await web.execute_authoring_job('fixture-execution', None)
    assert len(requests) == 1 and requests[0]['reasoning_effort'] == 'medium'
    assert any("status='succeeded'" in statement for statement in statements)
    assert not any("status='failed'" in statement for statement in statements)


@pytest.mark.parametrize('value,expected', [(' LOW ', 'low'), ('medium', 'medium'), ('HIGH', 'high')])
def test_explicit_supported_values_are_normalized_at_configuration_boundary(value, expected):
    config = ModelConfig.from_env(environment(KNOWLEDGEBRAIN_CHAT_REASONING_EFFORT=value))
    assert config.chat.reasoning_effort == expected


@pytest.mark.parametrize('same_model', [False, True])
def test_chat_option_is_not_inherited_by_the_separately_configured_vlm(same_model):
    values = environment(KNOWLEDGEBRAIN_CHAT_REASONING_EFFORT='medium',
        KNOWLEDGEBRAIN_VLM_BASE_URL='https://chat.invalid/v1' if same_model else 'https://vision.invalid/v1',
        KNOWLEDGEBRAIN_VLM_API_KEY='synthetic-vision-secret',
        KNOWLEDGEBRAIN_VLM_MODEL='gpt-4o' if same_model else 'vision-fixture')
    client, _ = model(lambda _: response(), values=values)
    assert client.config.chat.reasoning_effort == 'medium'
    assert _required(client.config.vision).reasoning_effort is None
    assert 'reasoning_effort' not in client.request_budget(images=True).request('ocr', {'fixture': True})


def test_explicit_option_changes_full_identity_and_wire_size_without_changing_output_reserves():
    plain = RequestBudget(model='fixture', prompts={'extract': 'Return JSON.'})
    medium = RequestBudget(model='fixture', prompts={'extract': 'Return JSON.'}, reasoning_effort='medium')
    original, configured = [budget.request('extract', {'fixture': True}) for budget in (plain, medium)]
    assert {key: value for key, value in configured.items() if key != 'reasoning_effort'} == original
    assert request_digest('https://model.invalid/v1', original) != request_digest('https://model.invalid/v1', configured)
    before, after = [budget.measure_request(request, 'extract')
                     for budget, request in ((plain, original), (medium, configured))]
    assert after['wire_bytes'] > before['wire_bytes']
    assert all(after[key] == before[key] for key in ('input_tokens', 'output_reserve', 'safety'))
    assert {key: value for key, value in medium.spec().items() if key != 'reasoning_effort'} == plain.spec()


@pytest.mark.parametrize('current', ['medium', 'high'])
async def test_exact_option_can_replay_an_oversized_paid_response_but_other_strength_cannot(tmp_path, current):
    artifacts = ArtifactStore(tmp_path)
    previous_request = build_request('fixture', 'Return JSON.', {'source': 'same source' * 100},
                                     reasoning_effort='medium')
    digest = request_digest('https://model.invalid/v1', previous_request)
    artifact = artifacts.put_json({'task': 'authoring_write', 'input_digest': digest,
                                  'response': {'content': 'accepted medium response'}})
    previous = {'execution_id': 'previous', 'receipts': [{'stage': 'document-model-response',
        'scope': 'authoring_write:' + digest, 'input_digest': digest, 'artifact_ref': artifact}]}
    request = {**previous_request, 'reasoning_effort': current}
    budget = RequestBudget(context_tokens=100, model='fixture', prompts={}, reasoning_effort=current)
    assert budget.measure_request(request, 'authoring_write')['fits'] is False
    client = SimpleNamespace(config=SimpleNamespace(chat=SimpleNamespace(
        base_url='https://model.invalid/v1', reasoning_effort=current)))
    store = SimpleNamespace(receipt=AsyncMock(return_value=None), commit_artifact=AsyncMock())
    executor = _DocumentExecutor(client, store, artifacts, SimpleNamespace(execution_id='current'), budget, {}, previous)
    result = await executor.replay('authoring_write', request, lambda value: value['content'])
    assert result == ((True, 'accepted medium response') if current == 'medium' else (False, None))
    assert store.commit_artifact.await_count == (1 if current == 'medium' else 0)


@pytest.mark.parametrize('problem', ['missing_body', 'budget_mismatch', 'explicit_null', 'logical_mismatch'])
async def test_prepared_option_mismatch_is_unsent_and_cannot_create_an_audit_attempt(problem):
    sent = []
    expected = None if problem == 'explicit_null' else 'medium'
    client, audit = model(lambda request: (sent.append(request), response())[1],
        values=environment(KNOWLEDGEBRAIN_CHAT_REASONING_EFFORT=expected or ''))
    budget = RequestBudget(context_tokens=32768, model=client.config.chat.model, prompts={},
                           reasoning_effort=None if problem == 'budget_mismatch' else expected)
    request = build_request(budget.model, 'Return JSON.', {'fixture': True},
                            reasoning_effort=None if problem == 'missing_body' else expected)
    if problem == 'explicit_null':
        request['reasoning_effort'] = None
    logical = build_request(budget.model, 'Return JSON.', {'fixture': True}) if problem == 'logical_mismatch' else None
    async with client:
        with pytest.raises(ModelConfigurationError):
            await client.call_request(request, budget, task='authoring_write',
                                      execution_spec={'budget': budget.spec()}, logical_request=logical)
    assert not sent and not audit.events


async def test_configured_prepared_request_records_the_sent_body_and_reuses_only_exact_paid_identity(tmp_path):
    from test_tender_streaming import ReplayAudit

    sent, audit = [], ReplayAudit()
    client, _ = model(lambda request: (sent.append(json.loads(request.content)), response())[1],
        audit, values=environment(KNOWLEDGEBRAIN_CHAT_REASONING_EFFORT='medium'), artifacts=ArtifactStore(tmp_path))
    budget = RequestBudget(context_tokens=32768, model=client.config.chat.model, prompts={}, reasoning_effort='medium')
    request = build_request(budget.model, 'Return JSON.', {'fixture': True}, reasoning_effort='medium')
    async with client:
        for _ in range(2):
            assert await client.call_request(request, budget, task='authoring_write',
                                             execution_spec={'budget': budget.spec()}) == {'result': 'ok'}
    assert sent == [request] and len(audit.rows) == 1
    row = next(iter(audit.rows.values()))
    assert row['metadata']['request_digest'] == request_digest(client.config.chat.base_url, request)
    assert row['metadata']['logical_request_digest'] == row['metadata']['request_digest']


async def test_feedback_preserves_the_option_in_both_actual_and_logical_requests(tmp_path):
    from docreader.tender.authoring_execution import _OutlineModel

    request = build_request('fixture', 'Return JSON.', {'fixture': True}, reasoning_effort='medium')
    original = deepcopy(request)
    client = SimpleNamespace(config=SimpleNamespace(chat=SimpleNamespace(base_url='https://model.invalid/v1')),
                             call_request=AsyncMock(return_value={'result': 'ok'}))
    store = SimpleNamespace(commit_artifact=AsyncMock())
    budget = RequestBudget(model='fixture', prompts={}, reasoning_effort='medium')
    adapter = _OutlineModel(client, store, ArtifactStore(tmp_path), object(), budget, {})
    await adapter.call('authoring_write', {'request': request, 'contract_feedback': {'fixture': 'correct this'}})
    sent = client.call_request.call_args.args[0]
    assert sent['reasoning_effort'] == 'medium'
    assert client.call_request.call_args.kwargs['logical_request'] == original == request
    assert 'contract_feedback' in json.loads(sent['messages'][1]['content'][0]['text'])


async def test_provider_rejection_does_not_resend_after_removing_the_explicit_option():
    import httpx

    sent = []
    client, audit = model(lambda request: (sent.append(json.loads(request.content)),
        httpx.Response(400, json={'error': {'message': 'unsupported reasoning parameter'}}))[1],
        values=environment(KNOWLEDGEBRAIN_CHAT_REASONING_EFFORT='medium'))
    async with client:
        with pytest.raises(ModelSemanticError):
            await client.call('extract', {'fixture': True})
    assert len(sent) == 1 and sent[0]['reasoning_effort'] == 'medium'
    assert audit.events[-1]['status'] == 'failed'


async def test_document_option_mismatch_is_rejected_before_any_input_or_cached_execution_is_read():
    from docreader.tender.document_execution import execute_document

    config = ModelConfig.from_env(environment(KNOWLEDGEBRAIN_CHAT_REASONING_EFFORT='medium'))
    budget = RequestBudget(model=config.chat.model, context_tokens=config.chat.context_tokens, prompts={})
    with pytest.raises(ModelConfigurationError):
        await execute_document(None, None, config, budget, execution_id='fixture-execution',
                               run_id='fixture-run', source_version=1, domain_revision=1)


def test_outline_model_tiering_reuses_chat_endpoint_with_optional_model_and_effort():
    plain = ModelConfig.from_env(environment(KNOWLEDGEBRAIN_CHAT_REASONING_EFFORT='high'))
    assert plain.outline is None and plain.for_outline() is plain

    lighter = ModelConfig.from_env(environment(
        KNOWLEDGEBRAIN_CHAT_REASONING_EFFORT='high',
        KNOWLEDGEBRAIN_OUTLINE_REASONING_EFFORT='none',
    ))
    assert lighter.chat.reasoning_effort == 'high'
    outline_only = lighter.for_outline()
    assert outline_only.chat.model == lighter.chat.model
    assert outline_only.chat.base_url == lighter.chat.base_url
    assert outline_only.chat.api_key == lighter.chat.api_key
    assert outline_only.chat.context_tokens == lighter.chat.context_tokens
    assert outline_only.chat.reasoning_effort == 'none'

    dedicated = ModelConfig.from_env(environment(
        KNOWLEDGEBRAIN_CHAT_REASONING_EFFORT='high',
        KNOWLEDGEBRAIN_OUTLINE_MODEL='fast-outline-model',
    ))
    fast_cfg = dedicated.for_outline()
    assert fast_cfg.chat.model == 'fast-outline-model'
    assert fast_cfg.chat.base_url == dedicated.chat.base_url
    assert fast_cfg.chat.api_key == dedicated.chat.api_key
    assert fast_cfg.chat.context_tokens == dedicated.chat.context_tokens
    assert fast_cfg.chat.reasoning_effort is None  # Never silently inherits chat.reasoning_effort.
    assert dedicated.chat.model == 'gpt-4o' and dedicated.chat.reasoning_effort == 'high'

    with pytest.raises(ModelConfigurationError):
        ModelConfig.from_env(environment(KNOWLEDGEBRAIN_OUTLINE_MODEL='stub-chat'))

    with pytest.raises(ModelConfigurationError) as bad:
        ModelConfig.from_env(environment(KNOWLEDGEBRAIN_OUTLINE_REASONING_EFFORT='secret-bad-effort'))
    assert 'secret-bad-effort' not in str(bad.value)


@pytest.mark.parametrize('operation,expected_model,expected_effort', [
    ('outline', 'fast-outline-model', 'none'),
    ('document', 'gpt-4o', 'high'),
])
async def test_web_job_routes_only_outline_operation_to_configured_outline_tier(
    monkeypatch, operation, expected_model, expected_effort,
):
    from docreader.integrations import web
    from docreader.tender import document_execution

    config = ModelConfig.from_env(environment(
        KNOWLEDGEBRAIN_CHAT_REASONING_EFFORT='high',
        KNOWLEDGEBRAIN_OUTLINE_MODEL='fast-outline-model',
        KNOWLEDGEBRAIN_OUTLINE_REASONING_EFFORT='none',
    ))
    monkeypatch.setattr(ModelConfig, 'from_env', classmethod(lambda _: config))
    job = {'status': 'pending', 'project_id': 'fixture-project', 'payload': {'owner_user_id': 'fixture-owner',
        'operation': operation, 'request': {}, 'materials': [], 'identity': {
            'run_id': 'fixture-run', 'source_version': 1, 'domain_revision': 1}}}
    captured = []

    async def sql(*_):
        return SimpleNamespace(fetchone=AsyncMock(return_value=deepcopy(job)))

    @asynccontextmanager
    async def transaction():
        yield SimpleNamespace(execute=sql)

    bridge = SimpleNamespace(setup=AsyncMock(), authorize=AsyncMock(), artifacts=None,
        store=SimpleNamespace(transaction=transaction), job=AsyncMock(return_value={'status': 'running'}),
        authoring=SimpleNamespace(get_execution=AsyncMock(return_value=None)))
    monkeypatch.setattr(web, 'WebBridge', lambda _: bridge)
    if operation == 'document':
        from docreader.integrations import web_layout
        monkeypatch.setattr(web_layout, 'build_web_layout', lambda **_: SimpleNamespace(spec=lambda: {}))

    async def execute(_store, _artifacts, actual_config, budget, **_):
        captured.append((actual_config.chat.model, actual_config.chat.reasoning_effort, budget.model, budget.reasoning_effort))

    monkeypatch.setattr(document_execution, 'execute_document', execute)
    await web.execute_authoring_job('fixture-execution', None)
    assert captured == [(expected_model, expected_effort, expected_model, expected_effort)]
