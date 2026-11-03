"""Actual pipeline and model receipts survive plan/unit publication interruption."""
import asyncio
from copy import deepcopy
import json
from uuid import uuid4

import httpx
import pytest

from docreader.integrations.authoring_store import AuthoringStore
from docreader.tender import authoring_pipeline as pipeline
from docreader.tender.document_execution import execute_document

from test_analysis import analysis_runtime as analysis_runtime
from test_authoring_execution import config, response
from test_authoring_outline import budget


pytestmark = pytest.mark.integration


def configure(monkeypatch):
    async def organize(artifacts, context, request_budget, execute, gaps, **options):
        return {'schema_version': 2, 'authoring_input': deepcopy(context.input), 'issues': [], 'chapters': [
            {'chapter_id': 'chapter', 'title': '实施方案', 'children': [], 'targets': [
                {'target_id': f'target-{i}', 'kind': 'narrative', 'purpose': f'实施职责{i}',
                 'rule_refs': [], 'source_refs': []} for i in (1, 2)]}]}
    monkeypatch.setattr(pipeline, '_organize', organize)
    original = pipeline._fits
    def fits(request_budget, task, payload, **options):
        if task == 'authoring_write':
            return len(payload['targets']) == 1
        return original(request_budget, task, payload, **options)
    monkeypatch.setattr(pipeline, '_fits', fits)
    writes = []
    def handler(request):
        payload = json.loads(json.loads(request.content)['messages'][1]['content'][0]['text'])
        if 'units' in payload or payload.get('operation') == 'structure':
            return response({'findings': []})
        if 'targets' in payload and 'title_path' in payload:
            writes.append(payload)
            return response({'blocks': [{'kind': 'paragraph', 'text': '按照提供的方法组织部署与验收。',
                                         'target_ids': ['t1'], 'support_ids': ['k1']}]})
        return response({'shared_arrangements': []})
    return writes, handler, fits


async def run(runtime, identity, handler, *, concurrency=1):
    row = runtime[3]
    return await execute_document(runtime[0], runtime[1], config(), budget(), execution_id=identity,
        **{key: row[key] for key in ('run_id', 'source_version', 'domain_revision')},
        materials=[{'material_id': 'method', 'title': '给定方法', 'text': '组织部署、测试及验收。'}],
        concurrency=concurrency, transport=httpx.MockTransport(handler))


@pytest.mark.parametrize('after_commit', [False, True])
async def test_plan_publication_interrupts_before_writing_and_resumes_exact_tasks(
        analysis_runtime, monkeypatch, after_commit):
    writes, handler, fits = configure(monkeypatch)
    original, interrupted = AuthoringStore.commit_artifact, []
    async def publish(self, lease, stage, reference, *args, **kwargs):
        if stage == 'document-plan' and not interrupted:
            interrupted.append(True)
            if after_commit:
                await original(self, lease, stage, reference, *args, **kwargs)
            raise RuntimeError('injected plan publication interruption')
        return await original(self, lease, stage, reference, *args, **kwargs)
    monkeypatch.setattr(AuthoringStore, 'commit_artifact', publish)
    identity = uuid4().hex
    with pytest.raises(RuntimeError, match='injected plan'):
        await run(analysis_runtime, identity, handler)
    assert writes == []
    stored = await AuthoringStore(analysis_runtime[2].dsn).get_execution(identity)
    plans = [r for r in stored['receipts'] if r['stage'] == 'document-plan']
    assert len(plans) == int(after_commit)
    expected = analysis_runtime[1].read_json(plans[0]['artifact_ref']) if plans else None
    def resumed_fits(request_budget, task, payload, **options):
        assert not (after_commit and task == 'authoring_write'), 'committed plan must not be fitted again'
        return fits(request_budget, task, payload, **options)
    monkeypatch.setattr(pipeline, '_fits', resumed_fits)
    result = analysis_runtime[1].read_json(await run(analysis_runtime, identity, handler))
    stored = await AuthoringStore(analysis_runtime[2].dsn).get_execution(identity)
    plans = [r for r in stored['receipts'] if r['stage'] == 'document-plan']
    units = [r for r in stored['receipts'] if r['stage'] == 'document-unit']
    assert len(plans) == 1 and len(units) == len(result['unit_refs']) == len(writes) == 2
    actual = analysis_runtime[1].read_json(plans[0]['artifact_ref'])
    if expected:
        assert actual == expected
    assert [analysis_runtime[1].read_json(r['artifact_ref'])['task_id'] for r in units] == [
        task['task_id'] for task in actual['tasks']]
    assert all(r['input_digest'] == analysis_runtime[1].read_json(r['artifact_ref'])['input_digest'] for r in units)


async def test_successful_raw_response_is_revalidated_when_unit_publication_failed(
        analysis_runtime, monkeypatch):
    writes, handler, _ = configure(monkeypatch)
    original, interrupted = AuthoringStore.commit_artifact, []
    async def publish(self, lease, stage, reference, *args, **kwargs):
        if stage == 'document-unit' and not interrupted:
            interrupted.append(True)
            raise RuntimeError('injected unit publication interruption')
        return await original(self, lease, stage, reference, *args, **kwargs)
    monkeypatch.setattr(AuthoringStore, 'commit_artifact', publish)
    identity = uuid4().hex
    with pytest.raises(RuntimeError, match='injected unit'):
        await run(analysis_runtime, identity, handler)
    before = await AuthoringStore(analysis_runtime[2].dsn).get_execution(identity)
    assert any(r['stage'] == 'document-model-response' and r['scope'].startswith('authoring_write:')
               for r in before['receipts'])
    result = analysis_runtime[1].read_json(await run(analysis_runtime, identity, handler))
    assert len(result['unit_refs']) == len(writes) == 2
    assert len({json.dumps(value, sort_keys=True) for value in writes}) == 2
    units = [analysis_runtime[1].read_json(ref) for ref in result['unit_refs']]
    assert all(u['input_digest'] and u['blocks'][0]['material_ids'] == ['method'] for u in units)


async def test_parallel_partial_publication_resumes_saved_unit_and_raw_response(
        analysis_runtime, monkeypatch):
    writes, handler, fits = configure(monkeypatch)
    both_started, first_committed = asyncio.Event(), asyncio.Event()
    entered, active, peak = [], 0, 0

    async def parallel_handler(request):
        nonlocal active, peak
        payload = json.loads(json.loads(request.content)['messages'][1]['content'][0]['text'])
        if 'targets' not in payload or 'title_path' not in payload:
            return handler(request)
        entered.append(payload)
        ordinal = len(entered)
        active += 1
        peak = max(peak, active)
        try:
            if ordinal == 2:
                both_started.set()
                await asyncio.wait_for(first_committed.wait(), timeout=10)
            else:
                await asyncio.wait_for(both_started.wait(), timeout=10)
            return handler(request)
        finally:
            active -= 1

    original_publish = AuthoringStore.commit_artifact
    publications, interrupted = [], []

    async def publish(self, lease, stage, reference, *args, **kwargs):
        if stage != 'document-unit':
            return await original_publish(self, lease, stage, reference, *args, **kwargs)
        unit = analysis_runtime[1].read_json(reference)
        publications.append(deepcopy(unit))
        if first_committed.is_set() and not interrupted:
            interrupted.append(unit)
            raise RuntimeError('injected second parallel unit publication interruption')
        receipt = await original_publish(self, lease, stage, reference, *args, **kwargs)
        first_committed.set()
        return receipt

    monkeypatch.setattr(AuthoringStore, 'commit_artifact', publish)
    identity = uuid4().hex
    with pytest.raises(RuntimeError, match='injected second parallel unit'):
        await run(analysis_runtime, identity, parallel_handler, concurrency=2)
    assert peak == 2 and len(entered) == len(writes) == len(publications) == 2
    store, artifacts = AuthoringStore(analysis_runtime[2].dsn), analysis_runtime[1]
    before = await store.get_execution(identity)
    before_units = [r for r in before['receipts'] if r['stage'] == 'document-unit']
    before_raw = [r for r in before['receipts'] if r['stage'] == 'document-model-response'
                  and r['scope'].startswith('authoring_write:')]
    before_plan = next(r for r in before['receipts'] if r['stage'] == 'document-plan')
    assert len(before_units) == 1 and len(before_raw) == 2
    assert before_units[0]['scope'] != interrupted[0]['task_id']
    saved_plan = artifacts.read_json(before_plan['artifact_ref'])
    assert {t['task_id'] for t in saved_plan['tasks']} == {u['task_id'] for u in publications}
    saved_materials = artifacts.read_json(saved_plan['materials_ref'])

    revalidated, original_bind = [], pipeline.bind_writing_response

    def bind(task, raw, sources, materials, **options):
        revalidated.append((task['task_id'], [m['material_id'] for m in materials]))
        return original_bind(task, raw, sources, materials, **options)

    def resumed_fits(request_budget, task, payload, **options):
        assert task != 'authoring_write', 'recovery must use the committed writing plan'
        return fits(request_budget, task, payload, **options)

    def resumed_handler(request):
        payload = json.loads(json.loads(request.content)['messages'][1]['content'][0]['text'])
        assert not ('targets' in payload and 'title_path' in payload), 'recovery must not send another write'
        return handler(request)

    monkeypatch.setattr(pipeline, 'bind_writing_response', bind)
    monkeypatch.setattr(pipeline, '_fits', resumed_fits)
    result = artifacts.read_json(await run(analysis_runtime, identity, resumed_handler, concurrency=2))
    after = await store.get_execution(identity)
    assert len(writes) == 2
    assert revalidated == [(interrupted[0]['task_id'], ['method'])]
    assert len(publications) == 3 and publications[-1] == interrupted[0]
    assert next(r for r in after['receipts'] if r['stage'] == 'document-plan') == before_plan
    assert artifacts.read_json(saved_plan['materials_ref']) == saved_materials
    assert [r for r in after['receipts'] if r['stage'] == 'document-model-response'
            and r['scope'].startswith('authoring_write:')] == before_raw
    final_units = {artifacts.read_json(ref)['task_id']: artifacts.read_json(ref)
                   for ref in result['unit_refs']}
    assert len(final_units) == 2
    assert all(final_units[u['task_id']] == u for u in publications[:2])
    assert all(u['input_digest'] and u['blocks'][0]['material_ids'] == ['method']
               for u in final_units.values())
