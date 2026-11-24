"""Independent directory reads retain stable per-slot recovery identities."""

from copy import deepcopy
from types import SimpleNamespace
import asyncio

import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.budget import RequestBudget
from docreader.llm.request import ModelWorkUnitExhausted
from docreader.tender.authoring_pipeline import _request_directory_plans
import docreader.tender.document_execution as execution
from test_directory_compile import docs, item, node, plan, window


class Receipts:
    def __init__(self):
        self.rows = {}
        self.plans = {}

    async def register_plan(self, _lease, task, scope, requests, _reference):
        self.plans[(task, scope)] = requests

    async def receipt(self, _lease, stage, scope, *, input_digest):
        return self.rows.get((stage, scope, input_digest))

    async def commit_artifact(self, _lease, stage, reference, *, scope, input_digest, **_kwargs):
        self.rows[(stage, scope, input_digest)] = {'artifact_ref': reference}


def executor(artifacts, store, budget):
    client = SimpleNamespace(config=SimpleNamespace(chat=SimpleNamespace(base_url='https://fixture.invalid/v1')))
    lease = SimpleNamespace(execution_id='directory-slots', epoch=1)
    return execution._DocumentExecutor(client, store, artifacts, lease, budget, {}, None)


async def test_directory_reads_are_independent_and_both_replay_after_restart(tmp_path, monkeypatch):
    artifacts, store = ArtifactStore(tmp_path), Receipts()
    budget = RequestBudget(model='fixture', context_tokens=131072, output_reserve=16384)
    documents = docs(['投标函\n正文'])
    identity = {'source_version': 1, 'domain_revision': 1}
    original = deepcopy(documents)
    sent = []

    async def model(_model, request, validate):
        sent.append(deepcopy(request))
        return validate(plan([{**node(f'letter-{len(sent)}', 0, 0), 'body_windows': [window(0, 0, 1)]}],
                             [item(f'letter-{len(sent)}')]))

    monkeypatch.setattr(execution._DocumentModel, '__call__', model)
    first, failures = await _request_directory_plans(documents, budget, executor(artifacts, store, budget), identity)
    assert failures == []
    assert len(sent) == 2
    assert sent[0] != sent[1]
    assert first[0]['nodes'][0]['id'] == 'letter-1'
    assert first[1]['nodes'][0]['id'] == 'letter-2'
    recovered, failures = await _request_directory_plans(documents, budget, executor(artifacts, store, budget), identity)
    assert failures == [] and recovered == first
    assert len(sent) == len(store.rows) == 2
    assert documents == original


async def test_failed_second_read_does_not_reuse_the_first_read(tmp_path, monkeypatch):
    artifacts, store = ArtifactStore(tmp_path), Receipts()
    budget = RequestBudget(model='fixture', context_tokens=131072, output_reserve=16384)
    documents = docs(['投标函\n正文'])
    calls = 0

    async def model(_model, _request, validate):
        nonlocal calls
        calls += 1
        if calls == 2:
            raise ModelWorkUnitExhausted('second independent read failed')
        return validate(plan([{**node('letter', 0, 0), 'body_windows': [window(0, 0, 1)]}], [item('letter')]))

    monkeypatch.setattr(execution._DocumentModel, '__call__', model)
    plans, failures = await _request_directory_plans(documents, budget, executor(artifacts, store, budget))
    assert len(plans) == len(failures) == 1
    assert calls == 2 and len(store.rows) == 1


async def test_restart_after_second_read_interruption_only_sends_the_unfinished_slot(tmp_path, monkeypatch):
    artifacts, store = ArtifactStore(tmp_path), Receipts()
    budget = RequestBudget(model='fixture', context_tokens=131072, output_reserve=16384)
    documents = docs(['投标函\n正文'])
    calls = 0

    async def model(_model, _request, validate):
        nonlocal calls
        calls += 1
        if calls == 2:
            raise asyncio.CancelledError()
        return validate(plan([{**node('letter', 0, 0), 'body_windows': [window(0, 0, 1)]}], [item('letter')]))

    monkeypatch.setattr(execution._DocumentModel, '__call__', model)
    with pytest.raises(asyncio.CancelledError):
        await _request_directory_plans(documents, budget, executor(artifacts, store, budget))
    assert len(store.rows) == 1
    plans, failures = await _request_directory_plans(documents, budget, executor(artifacts, store, budget))
    assert len(plans) == 2 and failures == []
    assert calls == 3 and len(store.rows) == 2
