"""Complete assembly/review may send over an estimate without changing its ledger."""

from copy import deepcopy
import json
from types import SimpleNamespace
from unittest.mock import AsyncMock

import httpx
import pytest

from docreader.llm.budget import RequestBudget
from docreader.llm.request import (
    ModelBudgetError, ModelContractError, ModelWorkUnitExhausted, build_request,
)
from docreader.tender.authoring_execution import _OutlineModel

from test_tender_llm import CountedArtifacts, model, response
from test_tender_streaming import ReplayAudit


OPERATIONS = [("outline", "assemble"), ("authoring_structure", "structure")]


def oversized_request(operation):
    budget = RequestBudget(context_tokens=1024, model="gpt-4o", prompts={}, output_reserve=128)
    request = build_request(budget.model, "Return JSON.", {
        "operation": operation, "source": "完整原文" * 100,
    })
    return budget, request


class JournalAudit(ReplayAudit):
    def __init__(self):
        super().__init__()
        self.receipts = {}

    async def receipt(self, lease, stage, scope="", *, input_digest=""):
        return self.receipts.get((stage, scope, input_digest))

    async def commit_artifact(self, lease, stage, artifact_ref, scope="", *, input_digest="", **kwargs):
        self.receipts[stage, scope, input_digest] = {"artifact_ref": artifact_ref}


@pytest.mark.parametrize("task,operation", OPERATIONS)
def test_send_admission_retains_strict_measurement_and_spec(task, operation):
    budget, request = oversized_request(operation)
    original, spec = deepcopy(request), budget.spec()
    measured = budget.measure_request(request, task)
    assert measured["fits"] is False
    with pytest.raises(ModelBudgetError):
        budget.require_request(request, task)
    admitted = budget.admit_request(request, task)
    assert admitted == {**measured, "admission_warning": "local_context_estimate_exceeded"}
    assert budget.spec() == spec
    assert budget.measure_request(request, task) == measured
    assert request == original
    with pytest.raises(ModelBudgetError):
        budget.require_request(request, task)


def test_fitting_request_keeps_original_admission_metadata():
    budget, request = oversized_request("assemble")
    request["messages"][1]["content"][0]["text"] = '{"operation":"assemble"}'
    assert budget.admit_request(request, "outline") == budget.require_request(request, "outline")
    assert "admission_warning" not in budget.admit_request(request, "outline")


@pytest.mark.parametrize("task,payload", [
    ("extract", {"operation": "assemble"}),
    ("verify", {"operation": "structure"}),
    ("authoring_write", {"operation": "structure"}),
    ("outline", {"operation": "skeleton"}),
    ("outline", {"operation": "structure"}),
    ("outline", {"operation": ["assemble"]}),
    ("outline", {"nested": {"operation": "assemble"}}),
    ("authoring_structure", {"operation": "assemble"}),
    (None, {"operation": "assemble"}),
    ("outline", [{"operation": "assemble"}]),
])
async def test_send_admission_is_scoped_to_exact_task_and_top_level_operation(task, payload):
    budget, request = oversized_request("assemble")
    request["messages"][1]["content"][0]["text"] = json.dumps({
        **payload, "source": "完整原文" * 100,
    }, ensure_ascii=False) if isinstance(payload, dict) else json.dumps(payload + ["超长" * 500])
    calls = []
    client, audit = model(lambda sent: (calls.append(sent), response())[1])
    async with client:
        with pytest.raises(ModelBudgetError):
            await client.call_request(request, budget, task=task, execution_spec={"protocol": "test"})
    assert not calls and not audit.events


@pytest.mark.parametrize("task,operation", OPERATIONS)
async def test_over_estimate_complete_request_is_sent_intact_and_cached(tmp_path, task, operation):
    budget, request = oversized_request(operation)
    calls, audit = [], ReplayAudit()
    artifacts = CountedArtifacts(tmp_path)
    client, _ = model(lambda sent: (calls.append(json.loads(sent.content)), response())[1],
                      audit, artifacts=artifacts)
    spec = {"protocol": "test", "budget": budget.spec()}
    async with client:
        for _ in range(2):
            assert await client.call_request(request, budget, task=task, execution_spec=spec) == {"result": "ok"}
    assert calls == [request]
    assert len(audit.rows) == 1
    metadata = next(iter(audit.rows.values()))["metadata"]
    assert metadata["fits"] is False
    assert metadata["input_tokens"] == budget.measure_request(request, task)["input_tokens"]
    assert metadata["admission_warning"] == "local_context_estimate_exceeded"
    assert "admission_warning" not in calls[0]
    assert "max_tokens" not in calls[0] and "max_completion_tokens" not in calls[0]


@pytest.mark.parametrize("task,operation", OPERATIONS)
async def test_over_estimate_contract_correction_reaches_provider(tmp_path, monkeypatch, task, operation):
    budget, request = oversized_request(operation)
    baseline, calls, audit = deepcopy(request), [], JournalAudit()
    artifacts = CountedArtifacts(tmp_path)

    def handler(sent):
        calls.append(json.loads(sent.content))
        return response('{"result":"bad"}' if len(calls) == 1 else '{"result":"ok"}')

    def validate(value):
        if value["result"] != "ok":
            raise ModelContractError([{"path": ["result"], "code": "bad_result", "detail": "Return ok"}])
        return value

    monkeypatch.setattr("docreader.llm.recovery.asyncio.sleep", AsyncMock())
    client, _ = model(handler, audit, artifacts=artifacts)
    lease = SimpleNamespace(execution_id="test-execution", epoch=1)
    adapter = _OutlineModel(client, audit, artifacts, lease, budget, {"protocol": "test"}, task=task)
    async with client:
        assert await adapter(request, validate) == {"result": "ok"}
    assert len(calls) == 2 and calls[0] == baseline
    corrected = json.loads(calls[1]["messages"][1]["content"][0]["text"])
    assert corrected.pop("contract_feedback")["errors"][0]["code"] == "bad_result"
    assert corrected == json.loads(baseline["messages"][1]["content"][0]["text"])
    assert "contract_feedback" not in calls[1]
    assert request == baseline
    metadata = [row["metadata"] for row in audit.rows.values()]
    assert all(row["fits"] is False for row in metadata)
    assert len({row["logical_request_digest"] for row in metadata}) == 1
    assert len({row["request_digest"] for row in metadata}) == 2


@pytest.mark.parametrize("task,operation", OPERATIONS)
async def test_provider_context_rejection_uses_only_four_attempts(tmp_path, monkeypatch, task, operation):
    budget, request = oversized_request(operation)
    calls, audit = [], JournalAudit()
    artifacts = CountedArtifacts(tmp_path)

    def handler(sent):
        calls.append(json.loads(sent.content))
        return httpx.Response(400, json={"error": {"code": "context_length_exceeded", "message": "Too long"}})

    sleep = AsyncMock()
    monkeypatch.setattr("docreader.llm.recovery.asyncio.sleep", sleep)
    client, _ = model(handler, audit, artifacts=artifacts)
    lease = SimpleNamespace(execution_id="test-execution", epoch=1)
    adapter = _OutlineModel(client, audit, artifacts, lease, budget, {"protocol": "test"}, task=task)
    async with client:
        with pytest.raises(ModelWorkUnitExhausted):
            await adapter(request, lambda value: value)
    assert calls == [request] * 4
    assert [row["status"] for row in audit.rows.values()] == ["failed"] * 4
    assert [call.args[0] for call in sleep.await_args_list] == [1, 2, 4]
