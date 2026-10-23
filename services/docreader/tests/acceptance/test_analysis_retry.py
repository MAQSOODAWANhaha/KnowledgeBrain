"""Explicit same-revision analysis recovery with isolated synthetic models."""

from collections import Counter
from uuid import uuid4

import pytest

from docreader.integrations.store import Store
from docreader.llm import ModelSemanticError
from docreader.llm.request import ModelWorkUnitExhausted
from docreader.workflow.nodes import TenderNodes
from docreader.workflow.runner import execute_command

from fixture_analysis_model import AnalysisFixtureModel
from fixture_model import fixture_request_digest
from test_analysis import analysis_runtime as analysis_runtime, execute_analysis, result
from test_phase1 import output, runtime as runtime


pytestmark = pytest.mark.integration


def _executor(runtime, failure_count):
    store, artifacts, settings, run = runtime
    calls, failed_requests = [], []

    class CompleteInvalidJson(AnalysisFixtureModel):
        async def call(self, task, payload, images=()):
            documents = {view["document_id"] for view in payload.get("source_views", [])}
            if task == "extract" and "annex" in documents and len(failed_requests) < failure_count:
                self.calls.append((task, payload))
                request_id = uuid4().hex
                await self.store.begin_request(self.lease, request_id, "synthetic-analysis-fixture", task, {
                    "request_digest": fixture_request_digest(task, payload, images),
                    "test_double": True, "external_request": False,
                })
                await self.store.finish_request(request_id, "sent")
                response_ref = self.artifacts.put_json({
                    "content": '{"schema_version":2,"items":[]} }',
                    "finish_reason": "stop", "stream_complete": True,
                })
                await self.store.finish_request(request_id, "failed", response_ref=response_ref,
                                                error={"type": "invalid_json"})
                failed_requests.append(request_id)
                raise ModelSemanticError("injected complete response with invalid JSON")
            return await super().call(task, payload, images)

    async def execute(command):
        def factory(s, a, lease):
            model = CompleteInvalidJson(s, lease, artifacts=a)
            model.calls = calls
            return TenderNodes(s, a, lease, model)

        await execute_command(run["run_id"], command["command_id"], command["source_version"], settings, factory)
        return await store.get_run(run["run_id"])

    return execute, calls, failed_requests


async def test_explicit_retry_reuses_successful_packs_after_three_automatic_retries(runtime):
    store, artifacts, _, run = runtime
    execute, calls, failed_requests = _executor(runtime, 4)
    initial = (await store.get_run(run["run_id"]))["commands"][-1]
    with pytest.raises(ModelWorkUnitExhausted):
        await execute(initial)
    failed = await store.get_run(run["run_id"])
    assert failed["status"] == "failed"
    assert failed["commands"][-1]["execution_count"] == 1
    assert failed["commands"][-1]["infrastructure_failures"] == 0
    assert failed["commands"][-1]["status"] == "completed"
    assert failed["commands"][-1]["receipt"]["status"] == "failed"
    assert failed["commands"][-1]["receipt"]["metadata"]["reason"] == "model_work_unit_retries_exhausted"
    saved = {(item["stage"], item["scope"]): item for item in failed["artifacts"]}
    successful_packs = {scope for stage, scope in saved if stage == "extract-pack"}
    plan = output(artifacts, failed, "pack-plan")
    assert successful_packs and len(successful_packs) == len(plan["packs"]) - 1
    assert not any(item["stage"] == "verify" for item in failed["artifacts"])
    assert len(failed_requests) == 4
    failed_audits = [request for request in failed["usage"] if request["request_id"] in failed_requests]
    assert len(failed_audits) == 4
    assert all(request["status"] == "failed" and request["response_ref"] for request in failed_audits)
    failed_responses = {request["request_id"]: artifacts.read_json(request["response_ref"])
                        for request in failed_audits}
    assert len([request for request in failed["usage"] if request["task"] == "extract"]) == len(plan["packs"]) + 3

    await execute(initial)
    assert (await store.get_run(run["run_id"]))["usage"] == failed["usage"]

    arguments = (run["run_id"], failed["source_version"], failed["domain_revision"],
                 initial["command_id"], "retry-complete-invalid-json", "Retry the confirmed complete invalid response")
    await store.retry(*arguments)
    pending = await store.get_run(run["run_id"])
    command = pending["commands"][-1]
    assert pending["status"] == "pending"
    assert (pending["source_version"], pending["domain_revision"]) == (
        failed["source_version"], failed["domain_revision"])
    assert pending["manifest_ref"] == failed["manifest_ref"]
    assert pending["artifacts"] == failed["artifacts"]
    assert pending["usage"] == failed["usage"]
    assert command["command_id"] != initial["command_id"]
    assert len(pending["commands"]) == len(failed["commands"]) + 1
    await store.retry(*arguments)
    assert (await store.get_run(run["run_id"]))["commands"] == pending["commands"]

    recovered = await execute(command)
    checked = output(artifacts, recovered, "verify")
    assert recovered["status"] == "waiting-confirmation", checked["issues"]
    assert recovered["source_version"] == failed["source_version"]
    assert recovered["domain_revision"] == failed["domain_revision"]
    assert recovered["revision"] == failed["revision"]
    for item in recovered["artifacts"]:
        if (item["stage"], item["scope"]) in saved:
            assert item == saved[(item["stage"], item["scope"])]
    assert not recovered["confirmations"]
    assert not any(item["stage"] == "confirm" for item in recovered["artifacts"])
    assert any(task == "verify" for task, _ in calls)
    extraction_calls = Counter(payload["work_unit_id"] for task, payload in calls if task == "extract")
    assert set(extraction_calls) == {entry["pack_id"] for entry in plan["packs"]}
    assert all(extraction_calls[pack_id] == 1 for pack_id in successful_packs)
    failed_pack = (extraction_calls.keys() - successful_packs).pop()
    assert extraction_calls[failed_pack] == 5
    extraction_requests = [request for request in recovered["usage"] if request["task"] == "extract"]
    assert len(extraction_requests) == len(plan["packs"]) + 4
    same_digest = [request for request in extraction_requests
                   if request["metadata"]["request_digest"] == failed_audits[0]["metadata"]["request_digest"]]
    assert Counter(request["status"] for request in same_digest) == {"failed": 4, "succeeded": 1}
    for request in failed_audits:
        assert next(row for row in recovered["usage"] if row["request_id"] == request["request_id"]) == request
        assert artifacts.read_json(request["response_ref"]) == failed_responses[request["request_id"]]
    assert all(request["metadata"]["external_request"] is False for request in recovered["usage"])

    await store.retry(*arguments)
    await execute(command)
    final = await store.get_run(run["run_id"])
    assert final["status"] == "waiting-confirmation"
    assert final["commands"] == recovered["commands"]
    assert final["usage"] == recovered["usage"]
    assert final["artifacts"] == recovered["artifacts"]


async def test_temporary_invalid_json_recovers_on_automatic_retry_without_new_command(runtime):
    store, artifacts, _, run = runtime
    execute, calls, failed_requests = _executor(runtime, 1)
    command = (await store.get_run(run["run_id"]))["commands"][-1]
    waiting = await execute(command)
    assert waiting["status"] == "waiting-confirmation", output(artifacts, waiting, "verify")["issues"]
    assert len(waiting["commands"]) == 1
    assert waiting["commands"][0]["command_id"] == command["command_id"]
    assert waiting["commands"][0]["execution_count"] == 1
    assert waiting["commands"][0]["infrastructure_failures"] == 0
    assert len(failed_requests) == 1
    attempts = Counter(payload["work_unit_id"] for task, payload in calls if task == "extract")
    plan = output(artifacts, waiting, "pack-plan")
    assert sorted(attempts.values()) == [1] * (len(plan["packs"]) - 1) + [2]
    await execute(command)
    unchanged = await store.get_run(run["run_id"])
    assert unchanged["artifacts"] == waiting["artifacts"]
    assert unchanged["usage"] == waiting["usage"]
    assert unchanged["commands"] == waiting["commands"]
    assert not waiting["confirmations"]
    assert all(request["metadata"]["external_request"] is False for request in waiting["usage"])


async def test_explicit_retry_preserves_failed_resume_confirmation_without_model_calls(analysis_runtime, monkeypatch):
    store, artifacts, _, run, calls = analysis_runtime
    waiting = await execute_analysis(analysis_runtime)
    assert waiting["status"] == "waiting-confirmation"
    approved = await store.confirm(run["run_id"], waiting["source_version"], waiting["checklist_digest"],
                                   "synthetic-confirm-before-terminal-failure", domain_revision=waiting["domain_revision"])
    assert approved["kind"] == "resume"
    commit, failures = Store.commit_artifact, []

    async def fail_confirmation_four_times(self, lease, stage, *args, **kwargs):
        if stage == "confirm" and len(failures) < 4:
            failures.append(stage)
            raise ValueError("injected terminal confirmation commit failure")
        return await commit(self, lease, stage, *args, **kwargs)

    monkeypatch.setattr(Store, "commit_artifact", fail_confirmation_four_times)
    before = len(calls)
    for attempt in range(4):
        with pytest.raises(ValueError, match="injected terminal confirmation commit failure"):
            await execute_analysis(analysis_runtime)
        current = await store.get_run(run["run_id"])
        assert current["status"] == ("pending" if attempt < 3 else "failed")
        assert current["commands"][-1]["infrastructure_failures"] == attempt + 1
        assert len(calls) == before
    failed = await store.get_run(run["run_id"])
    assert failed["status"] == "failed"
    assert failed["commands"][-1]["command_id"] == approved["command_id"]
    assert failed["commands"][-1]["receipt"]["status"] == "failed"
    assert not any(item["stage"] == "confirm" for item in failed["artifacts"])
    retry = await store.retry(run["run_id"], failed["source_version"], failed["domain_revision"],
                             approved["command_id"], "retry-failed-confirmation", "Retry the synthetic commit failure")
    assert retry["kind"] == "resume"
    assert {key: value for key, value in retry["payload"].items() if key != "retry"} == approved["payload"]
    ready = await execute_analysis(analysis_runtime)
    assert ready["status"] == "analysis-ready"
    assert ready["source_version"] == failed["source_version"]
    assert ready["domain_revision"] == failed["domain_revision"]
    assert ready["confirmations"] == failed["confirmations"]
    assert ready["usage"] == failed["usage"]
    assert len(calls) == before
    assert result(artifacts, ready, "confirm") == approved["payload"]


async def test_complete_host_rejection_uses_shared_work_unit_retry_budget(runtime):
    store, artifacts, settings, run = runtime
    replies, calls = {}, []

    class RejectedReview(AnalysisFixtureModel):
        def response(self, task, payload, images):
            result = super().response(task, payload, images)
            if task == 'verify' and payload['operation'] == 'review':
                responses = replies.setdefault(payload['work_unit_id'], iter([None, {}, 'invalid', False]))
                result['findings'] = next(responses)
            return result

    command = (await store.get_run(run['run_id']))['commands'][-1]
    def factory(s, a, lease):
        model = RejectedReview(s, lease, artifacts=a)
        model.calls = calls
        return TenderNodes(s, a, lease, model)

    with pytest.raises(ModelWorkUnitExhausted):
        await execute_command(run['run_id'], command['command_id'], command['source_version'], settings, factory)
    current = await store.get_run(run['run_id'])
    assert current['commands'][-1]['infrastructure_failures'] == 0
    assert current['commands'][-1]['execution_count'] == 1
    assert current['status'] == 'failed'
    assert [row for row in current['artifacts'] if row['stage'] == 'extract-pack']
    extractions = Counter(payload['work_unit_id'] for task, payload in calls if task == 'extract')
    assert all(count == 1 for count in extractions.values())
    reviews = [payload for task, payload in calls if task == 'verify']
    plan = output(artifacts, current, 'analysis-review-plan')
    planned_units = {entry['work_unit_id'] for entry in plan['requests'] if entry['budget']['fits'] is not False}
    units = set(replies)
    assert units <= planned_units and 1 <= len(units) <= 2
    assert Counter(value['work_unit_id'] for value in reviews) == Counter({unit: 4 for unit in units})
    for unit in units:
        attempts = [value for value in reviews if value['work_unit_id'] == unit]
        assert 'contract_feedback' not in attempts[0]
        assert all(value['contract_feedback']['errors'][0]['path'] == ['findings'] for value in attempts[1:])
        assert [value['contract_feedback']['rejection_count'] for value in attempts[1:]] == [1, 2, 3]
    rejections = [row for row in current['artifacts'] if row['stage'] == 'model-rejection']
    assert len(rejections) == 4 * len(units)
    rejected_units = Counter(artifacts.read_json(artifacts.read_json(row['artifact_ref'])['request_ref'])['work_unit_id']
                             for row in rejections)
    assert rejected_units == Counter({unit: 4 for unit in units})
    review_usage = [row for row in current['usage'] if row['task'] == 'verify']
    logical_digests = {fixture_request_digest('verify', next(value for value in reviews
        if value['work_unit_id'] == unit)) for unit in units}
    assert Counter(row['metadata']['logical_request_digest'] for row in review_usage) == Counter({
        digest: 4 for digest in logical_digests})
    assert len(current['usage']) == len(calls)
    assert all(row['status'] == 'succeeded' for row in current['usage'])


async def test_sse_success_rejection_correction_and_restart_cache(runtime):
    import json
    import httpx
    from docreader.llm import ModelConfig, OpenAIModel
    from docreader.llm.request import ModelContractError
    from docreader.tender.schema import AnalysisReviewResponse

    store, artifacts, _, run = runtime
    command = (await store.get_run(run['run_id']))['commands'][-1]
    lease = await store.acquire(run['run_id'], command['command_id'], run['source_version'])
    sent = []
    def handler(request):
        body = json.loads(request.content)
        payload = json.loads(body['messages'][1]['content'][0]['text'])
        sent.append(payload)
        reply = {'schema_version': 2, 'accepted_decision_ids': [],
                 'findings': [] if 'contract_feedback' in payload else None}
        chunks = [{'choices': [{'index': 0, 'delta': {'content': json.dumps(reply)}, 'finish_reason': None}]},
                  {'choices': [{'index': 0, 'delta': {}, 'finish_reason': 'stop'}]},
                  {'choices': [], 'usage': {'prompt_tokens': 20, 'completion_tokens': 10, 'total_tokens': 30}}]
        stream = ''.join('data: ' + json.dumps(chunk) + '\n\n' for chunk in chunks) + 'data: [DONE]\n\n'
        return httpx.Response(200, content=stream, headers={'content-type': 'text/event-stream'})

    config = ModelConfig.from_env({'KNOWLEDGEBRAIN_CHAT_BASE_URL': 'https://fixture.invalid/v1',
        'KNOWLEDGEBRAIN_CHAT_API_KEY': 'synthetic', 'KNOWLEDGEBRAIN_CHAT_MODEL': 'fixture',
        'KNOWLEDGEBRAIN_CHAT_MAX_CONTEXT_TOKENS': '131072'})
    def nodes(lease):
        return TenderNodes(store, artifacts, lease, OpenAIModel(config, store, lease,
            prompts={'verify': 'Return review JSON'}, artifacts=artifacts, transport=httpx.MockTransport(handler)))
    def validate(value):
        from pydantic import ValidationError
        try:
            return AnalysisReviewResponse.model_validate(value).model_dump()
        except ValidationError as error:
            raise ModelContractError([{'path': list(e['loc']), 'code': e['type'], 'detail': e['msg']}
                                      for e in error.errors(include_input=False, include_url=False)]) from None
    payload = {'schema_version': 2, 'work_unit_id': 'review-1', 'operation': 'review',
               'source_views': [], 'items': [], 'relations': [], 'decisions': []}
    first = nodes(lease)
    try:
        result = await first.call_validated('verify', payload, validate)
        assert result['findings'] == []
    finally:
        await first.close()
    before = await store.get_run(run['run_id'])
    original = before['usage'][0]
    assert original['status'] == 'succeeded' and original['response_ref']
    restarted = nodes(lease)
    try:
        assert await restarted.call_validated('verify', payload, validate) == result
    finally:
        await restarted.close()
    after = await store.get_run(run['run_id'])
    assert len(sent) == 2 and len(after['usage']) == 2
    assert next(row for row in after['usage'] if row['request_id'] == original['request_id']) == original
    assert sent[1]['contract_feedback']['fragments'] == [{'path': ['findings'], 'value': None}]
    assert len([row for row in after['artifacts'] if row['stage'] == 'model-wire']) == 2
    assert after['commands'][-1]['infrastructure_failures'] == 0
    assert after['commands'][-1]['execution_count'] == 1
    assert len({row['metadata']['logical_request_digest'] for row in after['usage']}) == 1
