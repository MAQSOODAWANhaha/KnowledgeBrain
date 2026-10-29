"""Durable outline execution with isolated PostgreSQL and synthetic HTTP streams."""

from copy import deepcopy
import json
from uuid import uuid4

import httpx
import pytest

from docreader.integrations.authoring import read_authoring_input
from docreader.integrations.authoring_store import AuthoringStore
from docreader.integrations.store import Cancelled, Conflict
from docreader.llm.budget import RequestBudget
from docreader.llm.config import ModelConfig, ModelEndpoint
from docreader.llm.request import ModelBudgetError, ModelWorkUnitExhausted
from docreader.tender.authoring_execution import execute_outline

from test_analysis import analysis_runtime as analysis_runtime, execute_analysis
from test_authoring_outline import budget, expectation, outline_response


pytestmark = pytest.mark.integration


def config():
    return ModelConfig(ModelEndpoint("http://fixture.invalid/v1", "fixture-key", "synthetic-outline", 524288), None)


def response(value):
    events = [
        {"id": "synthetic-outline-response", "choices": [
            {"index": 0, "delta": {"content": json.dumps(value, ensure_ascii=False)}, "finish_reason": None}]},
        {"choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]},
        {"choices": [], "usage": {"prompt_tokens": 113, "completion_tokens": 57, "total_tokens": 170}},
    ]
    content = "".join("data: " + json.dumps(event) + "\n\n" for event in events) + "data: [DONE]\n\n"
    return httpx.Response(200, content=content, headers={"content-type": "text/event-stream"})


def invalid_outline(request):
    value = deepcopy(outline_response(request))
    value["chapters"][0]["children"][0]["targets"][0]["rule_ids"] = ["e999999"]
    return value


async def confirmed(runtime):
    store, _, _, run, _ = runtime
    waiting = await execute_analysis(runtime)
    await store.confirm(run["run_id"], 1, waiting["checklist_digest"], "durable-outline", domain_revision=1)
    ready = await execute_analysis(runtime)
    assert ready["status"] == "analysis-ready"
    return ready


async def test_success_is_reused_by_execution_id_without_touching_parent_analysis(analysis_runtime):
    store, artifacts, settings, run, calls = analysis_runtime
    ready = await confirmed(analysis_runtime)
    before = len(calls)
    execution_id, sent = uuid4().hex, []

    def handler(request):
        body = json.loads(request.content)
        sent.append(body)
        return response(outline_response(body))

    reference = await execute_outline(store, artifacts, config(), budget(), execution_id=execution_id,
                                      **expectation(ready), transport=httpx.MockTransport(handler))
    execution_store = AuthoringStore(settings.dsn)
    succeeded = await execution_store.get_execution(execution_id)
    assert succeeded["status"] == "succeeded" and succeeded["result_ref"] == reference
    assert len(sent) == len(succeeded["requests"]) == 1
    request = succeeded["requests"][0]
    assert request["task"] == "outline" and request["status"] == "succeeded"
    assert request["usage"]["total_tokens"] == 170
    assert request["metadata"]["output_reserve"] == 32768
    assert "fixture-key" not in json.dumps(succeeded["execution_spec"])
    artifacts.read_json(request["response_ref"])
    assert artifacts.read_json(reference)["authoring_input"] == await read_authoring_input(
        store, artifacts, **expectation(ready))

    assert await execute_outline(store, artifacts, config(), budget(), execution_id=execution_id,
                                 **expectation(ready), transport=httpx.MockTransport(handler)) == reference
    assert len(sent) == 1
    recovered = await execution_store.get_execution(execution_id)
    assert recovered["status"] == "succeeded" and recovered["result_ref"] == reference
    assert recovered["requests"] == succeeded["requests"]
    assert len(calls) == before and await store.get_run(run["run_id"]) == ready


async def test_contract_feedback_gets_a_new_send_digest_and_one_logical_execution(analysis_runtime):
    store, artifacts, settings, run, calls = analysis_runtime
    ready = await confirmed(analysis_runtime)
    before = len(calls)
    execution_id, sent = uuid4().hex, []

    def handler(request):
        body = json.loads(request.content)
        sent.append(body)
        return response(invalid_outline(body) if len(sent) == 1 else outline_response(body))

    reference = await execute_outline(store, artifacts, config(), budget(), execution_id=execution_id,
                                      **expectation(ready), transport=httpx.MockTransport(handler))
    assert len(sent) == 2
    original = json.loads(sent[0]["messages"][1]["content"][0]["text"])
    corrected = json.loads(sent[1]["messages"][1]["content"][0]["text"])
    assert "contract_feedback" not in original
    feedback = corrected.pop("contract_feedback")
    assert corrected == original and feedback["errors"] and feedback["rejection_count"] == 1
    assert any(error["code"] == "outline_unknown_reference" for error in feedback["errors"])

    execution = await AuthoringStore(settings.dsn).get_execution(execution_id)
    requests = execution["requests"]
    assert execution["status"] == "succeeded" and execution["result_ref"] == reference
    assert len(requests) == 2 and all(request["status"] == "succeeded" for request in requests)
    assert len({request["metadata"]["request_digest"] for request in requests}) == 2
    assert len({request["metadata"]["logical_request_digest"] for request in requests}) == 1
    rejections = [receipt for receipt in execution["receipts"] if receipt["stage"] == "model-rejection"]
    assert len(rejections) == 1
    rejection = artifacts.read_json(rejections[0]["artifact_ref"])
    assert rejection["execution_id"] == execution_id and "command_id" not in rejection
    assert rejection["errors"] == feedback["errors"]
    assert len(calls) == before and await store.get_run(run["run_id"]) == ready


async def test_execution_identity_allows_key_rotation_and_rejects_changed_nonsecret_configuration(analysis_runtime):
    store, artifacts, settings, run, calls = analysis_runtime
    ready = await confirmed(analysis_runtime)
    before = len(calls)
    execution_id, sent = uuid4().hex, []

    def handler(request):
        body = json.loads(request.content)
        sent.append(body)
        return response(outline_response(body))

    transport = httpx.MockTransport(handler)
    reference = await execute_outline(store, artifacts, config(), budget(), execution_id=execution_id,
                                      **expectation(ready), transport=transport)
    execution_store = AuthoringStore(settings.dsn)
    original = await execution_store.get_execution(execution_id)
    rotated = ModelConfig(ModelEndpoint("http://fixture.invalid/v1", "rotated-fixture-key",
                                        "synthetic-outline", 524288), None)
    assert await execute_outline(store, artifacts, rotated, budget(), execution_id=execution_id,
                                 **expectation(ready), transport=transport) == reference
    changed_endpoint = ModelConfig(ModelEndpoint("http://other-fixture.invalid/v1", "fixture-key",
                                                 "synthetic-outline", 524288), None)
    changed_reserve = RequestBudget(context_tokens=524288, model="synthetic-outline", prompts={}, output_reserve=32769)
    for endpoint, request_budget, options in (
        (changed_endpoint, budget(), {}),
        (config(), changed_reserve, {}),
        (config(), budget(), {"timeout_seconds": 181}),
    ):
        with pytest.raises(Conflict):
            await execute_outline(store, artifacts, endpoint, request_budget, execution_id=execution_id,
                                  **expectation(ready), transport=transport, **options)
    unchanged = await execution_store.get_execution(execution_id)
    assert unchanged["status"] == "succeeded" and unchanged["result_ref"] == reference
    assert unchanged["execution_spec"] == original["execution_spec"]
    assert unchanged["requests"] == original["requests"] and len(sent) == 1
    assert len(calls) == before and await store.get_run(run["run_id"]) == ready


async def test_correction_budget_failure_preserves_rejection_without_another_send(analysis_runtime, monkeypatch):
    store, artifacts, settings, run, calls = analysis_runtime
    ready = await confirmed(analysis_runtime)
    before = len(calls)
    execution_id, sent = uuid4().hex, []
    require_request = RequestBudget.require_request

    def reject_feedback(self, request, task=None):
        payload = json.loads(request["messages"][1]["content"][0]["text"])
        if "contract_feedback" in payload:
            raise ModelBudgetError("Synthetic corrected request exceeds admission capacity")
        return require_request(self, request, task)

    monkeypatch.setattr(RequestBudget, "require_request", reject_feedback)

    def handler(request):
        body = json.loads(request.content)
        sent.append(body)
        return response(invalid_outline(body))

    with pytest.raises(ModelBudgetError):
        await execute_outline(store, artifacts, config(), budget(), execution_id=execution_id,
                              **expectation(ready), transport=httpx.MockTransport(handler))
    execution = await AuthoringStore(settings.dsn).get_execution(execution_id)
    assert execution["status"] == "failed" and execution["error"]["type"] == "ModelBudgetError"
    assert not execution["result_ref"] and len(sent) == len(execution["requests"]) == 1
    assert execution["requests"][0]["status"] == "succeeded"
    rejections = [receipt for receipt in execution["receipts"] if receipt["stage"] == "model-rejection"]
    assert len(rejections) == 1
    assert artifacts.read_json(rejections[0]["artifact_ref"])["errors"]
    assert len(calls) == before and await store.get_run(run["run_id"]) == ready


@pytest.mark.parametrize("interruption", ["after_response", "after_rejection"])
async def test_persisted_response_or_rejection_recovers_without_repaying_original_request(
    analysis_runtime, monkeypatch, interruption,
):
    store, artifacts, settings, run, calls = analysis_runtime
    ready = await confirmed(analysis_runtime)
    before = len(calls)
    execution_id, sent, interrupted = uuid4().hex, [], []

    def handler(request):
        body = json.loads(request.content)
        sent.append(body)
        if interruption == "after_rejection" and len(sent) == 1:
            return response(invalid_outline(body))
        return response(outline_response(body))

    original_release = AuthoringStore.release
    original_commit = AuthoringStore.commit_artifact

    async def interrupted_release(self, lease, status, result_ref=None, error=None):
        if status == "succeeded" and not interrupted:
            interrupted.append(interruption)
            raise RuntimeError("Synthetic interruption after response before publication")
        return await original_release(self, lease, status, result_ref=result_ref, error=error)

    async def interrupted_commit(self, lease, stage, *args, **kwargs):
        receipt = await original_commit(self, lease, stage, *args, **kwargs)
        if stage == "model-rejection" and not interrupted:
            interrupted.append(interruption)
            raise RuntimeError("Synthetic interruption after rejection was persisted")
        return receipt

    transport = httpx.MockTransport(handler)
    with monkeypatch.context() as patch:
        if interruption == "after_response":
            patch.setattr(AuthoringStore, "release", interrupted_release)
        else:
            patch.setattr(AuthoringStore, "commit_artifact", interrupted_commit)
        with pytest.raises(RuntimeError, match="Synthetic interruption"):
            await execute_outline(store, artifacts, config(), budget(), execution_id=execution_id,
                                  **expectation(ready), transport=transport)
    execution_store = AuthoringStore(settings.dsn)
    failed = await execution_store.get_execution(execution_id)
    assert interrupted == [interruption]
    assert failed["status"] == "failed" and not failed["result_ref"]
    assert len(sent) == len(failed["requests"]) == 1
    assert failed["requests"][0]["status"] == "succeeded"
    if interruption == "after_rejection":
        assert len([r for r in failed["receipts"] if r["stage"] == "model-rejection"]) == 1

    reference = await execute_outline(store, artifacts, config(), budget(), execution_id=execution_id,
                                      **expectation(ready), transport=transport)
    recovered = await execution_store.get_execution(execution_id)
    expected_sends = 1 if interruption == "after_response" else 2
    assert recovered["status"] == "succeeded" and recovered["result_ref"] == reference
    assert len(sent) == len(recovered["requests"]) == expected_sends
    assert recovered["requests"][0] == failed["requests"][0]
    assert artifacts.read_json(reference)["authoring_input"] == recovered["authoring_input"]
    if interruption == "after_rejection":
        correction = json.loads(sent[-1]["messages"][1]["content"][0]["text"])["contract_feedback"]
        assert correction["rejection_count"] == 1 and correction["errors"]
        assert recovered["requests"][0]["metadata"]["request_digest"] != recovered["requests"][1]["metadata"]["request_digest"]
    assert len(calls) == before and await store.get_run(run["run_id"]) == ready


@pytest.mark.parametrize("failure", ["http", "contract"])
async def test_four_sends_exhaust_persisted_allowance_and_reentry_cannot_reset_it(analysis_runtime, failure):
    store, artifacts, settings, run, calls = analysis_runtime
    ready = await confirmed(analysis_runtime)
    before = len(calls)
    execution_id, sent = uuid4().hex, []

    def handler(request):
        body = json.loads(request.content)
        sent.append(body)
        if failure == "http":
            return httpx.Response(503, json={"error": {"message": "Synthetic unavailability", "type": "server_error"}})
        return response(invalid_outline(body))

    transport = httpx.MockTransport(handler)
    with pytest.raises(ModelWorkUnitExhausted):
        await execute_outline(store, artifacts, config(), budget(), execution_id=execution_id,
                              **expectation(ready), transport=transport)
    execution_store = AuthoringStore(settings.dsn)
    failed = await execution_store.get_execution(execution_id)
    assert len(sent) == len(failed["requests"]) == 4
    assert failed["status"] == "failed" and not failed["result_ref"]
    assert all(request["sent_at"] is not None for request in failed["requests"])
    if failure == "contract":
        assert all(request["status"] == "succeeded" for request in failed["requests"])
        assert len([r for r in failed["receipts"] if r["stage"] == "model-rejection"]) == 4
    else:
        assert all(request["status"] == "failed" for request in failed["requests"])

    with pytest.raises(ModelWorkUnitExhausted):
        await execute_outline(store, artifacts, config(), budget(), execution_id=execution_id,
                              **expectation(ready), transport=transport)
    assert len(sent) == 4
    replayed = await execution_store.get_execution(execution_id)
    assert replayed["status"] == "failed" and replayed["requests"] == failed["requests"]
    assert len(calls) == before and await store.get_run(run["run_id"]) == ready


async def test_parent_revision_during_send_preserves_fixed_snapshot_and_success_cache(analysis_runtime):
    store, artifacts, settings, run, calls = analysis_runtime
    ready = await confirmed(analysis_runtime)
    input_value = await read_authoring_input(store, artifacts, **expectation(ready))
    before = len(calls)
    execution_id, sent = uuid4().hex, []

    async def handler(request):
        body = json.loads(request.content)
        sent.append(body)
        baseline = input_value["items_ref"]
        await store.correct(run["run_id"], 1, 1, baseline["sha256"], baseline,
                            "durable-outline-correction", "Synthetic parent revision while outline is in flight")
        return response(outline_response(body))

    reference = await execute_outline(store, artifacts, config(), budget(), execution_id=execution_id,
                                      **expectation(ready), transport=httpx.MockTransport(handler))
    execution = await AuthoringStore(settings.dsn).get_execution(execution_id)
    assert len(sent) == len(execution["requests"]) == 1
    assert execution["status"] == "succeeded" and execution["result_ref"] == reference
    assert artifacts.read_json(reference)["authoring_input"] == input_value
    assert await execute_outline(store, artifacts, config(), budget(), execution_id=execution_id,
                                 **expectation(ready), transport=httpx.MockTransport(handler)) == reference
    assert len(sent) == 1
    assert execution["requests"][0]["status"] == "succeeded"
    artifacts.read_json(execution["requests"][0]["response_ref"])
    changed = await store.get_run(run["run_id"])
    assert changed["domain_revision"] == 2 and changed["status"] == "pending"
    assert len(calls) == before


async def test_cancellation_during_send_prevents_publication_and_future_sends(analysis_runtime):
    store, artifacts, settings, run, calls = analysis_runtime
    ready = await confirmed(analysis_runtime)
    before = len(calls)
    execution_id, sent = uuid4().hex, []
    execution_store = AuthoringStore(settings.dsn)

    async def handler(request):
        body = json.loads(request.content)
        sent.append(body)
        await execution_store.cancel(execution_id)
        return response(outline_response(body))

    transport = httpx.MockTransport(handler)
    with pytest.raises(Cancelled):
        await execute_outline(store, artifacts, config(), budget(), execution_id=execution_id,
                              **expectation(ready), transport=transport)
    cancelled = await execution_store.get_execution(execution_id)
    assert len(sent) == 1
    assert cancelled["status"] == "cancelled" and cancelled["cancel_requested"] and not cancelled["result_ref"]
    with pytest.raises(Cancelled):
        await execute_outline(store, artifacts, config(), budget(), execution_id=execution_id,
                              **expectation(ready), transport=transport)
    assert len(sent) == 1
    assert len(calls) == before and await store.get_run(run["run_id"]) == ready
