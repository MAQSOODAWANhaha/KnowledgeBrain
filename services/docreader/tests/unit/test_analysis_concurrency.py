"""Stage scheduling preserves immutable receipts and deterministic aggregation."""

import asyncio

import pytest

from docreader.integrations.store import Cancelled, StaleExecution
from docreader.llm import ModelInfrastructureError
from docreader.llm.request import ModelWorkUnitExhausted
from docreader.tender.checks.analysis import _Analysis
from test_analysis_checks import IDENTITY, Model, budget, scenario


async def stage(tmp_path, operation="review", count=5):
    data = await scenario(tmp_path)
    context = _Analysis(data[1], data[2], data[0], IDENTITY, budget())
    payloads = [context.payload(operation, f"unit-{number}") for number in range(count)]
    return data, context, payloads


class ConcurrentModel(Model):
    def __init__(self):
        super().__init__()
        self.active = self.maximum = 0
        self.finished = []

    async def call(self, task, payload, images=()):
        self.active += 1
        self.maximum = max(self.maximum, self.active)
        try:
            # Complete the second sibling before the first, without time assertions.
            await asyncio.sleep(0.02 if payload["work_unit_id"].endswith("0") else 0)
            value = await super().call(task, payload, images)
            value["findings"] = [{"code": "source_formatting_note", "detail": payload["work_unit_id"],
                                  "blocking": False}]
            self.finished.append(payload["work_unit_id"])
            return value
        finally:
            self.active -= 1


@pytest.mark.parametrize("phase,operation", [
    ("review", "review"), ("locate", "locate"), ("associate", "associate"), ("joint", "review"),
])
async def test_stage_runs_two_requests_and_aggregates_in_plan_order(tmp_path, phase, operation):
    data, context, payloads = await stage(tmp_path, operation)
    model = ConcurrentModel()
    plan_ref, results = await context.execute(phase, payloads, model, data[3])
    expected = [p["work_unit_id"] for p in payloads]
    assert model.maximum == 2
    assert model.active == 0
    assert model.finished != expected
    assert [r["findings"][0]["detail"] for r in results] == expected
    assert [p["detail"] for p in context.problems] == expected
    assert [r["work_unit_id"] for r in context.operation_refs] == expected
    assert [p["work_unit_id"] for p in data[0].read_json(plan_ref)["requests"]] == expected
    replay = _Analysis(data[1], data[2], data[0], IDENTITY, budget())
    assert await replay.execute(phase, payloads, model, data[3]) == (plan_ref, results)
    assert len(model.calls) == len(payloads)
    assert replay.operation_refs == context.operation_refs
    assert replay.problems == context.problems


async def test_stage_respects_the_existing_node_semaphore(tmp_path):
    data, context, payloads = await stage(tmp_path)
    model = ConcurrentModel()
    model.semaphore = asyncio.Semaphore(1)
    await context.execute("review", payloads, model, data[3])
    assert model.maximum == 1
    assert len(model.calls) == len(payloads)


async def test_identical_scope_reviews_share_a_receipt_without_serializing_independent_work(tmp_path):
    data, context, independent = await stage(tmp_path, count=1)
    sources = list(context.sources.values())[:2]
    evidence = [{"range_id": source["range_id"], "quote": source["text"]} for source in sources]
    context.plan["scope_decisions"] = [{"range_id": source["range_id"], "decision": "local_only",
        "target_range_ids": [], "rationale": "仅限定本条，另一条作为边界对照。", "evidence": evidence}
        for source in sources]
    scopes = [p for p in context.review_requests() if p["work_unit_id"].startswith("scope-review-")]
    assert len(scopes) == 2 and scopes[0] == scopes[1]
    model = ConcurrentModel()
    _, results = await context.execute("review", [*scopes, *independent], model, data[3])
    assert sum(p["work_unit_id"] == scopes[0]["work_unit_id"] for p in model.calls) == 1
    assert model.maximum == 2
    assert results[0] == results[1]
    assert [r["work_unit_id"] for r in context.operation_refs] == [p["work_unit_id"] for p in [*scopes, *independent]]


@pytest.mark.parametrize("failure", [ModelInfrastructureError, ModelWorkUnitExhausted])
async def test_failed_stage_settles_successful_siblings_and_reuses_their_receipts(tmp_path, failure):
    data, context, payloads = await stage(tmp_path, count=4)
    model = ConcurrentModel()
    original = model.call
    failed = payloads[0]["work_unit_id"]
    second_started = asyncio.Event()

    async def call(task, payload, images=()):
        if payload["work_unit_id"] == failed:
            await asyncio.wait_for(second_started.wait(), timeout=3)
            raise failure("verify")
        if payload is payloads[1]:
            second_started.set()
            await asyncio.sleep(0.02)
        return await original(task, payload, images)

    model.call = call
    with pytest.raises(failure):
        await context.execute("review", payloads, model, data[3])
    assert model.active == 0
    assert model.finished == [payloads[1]["work_unit_id"]]
    assert {key[1] for key in data[3].saved if key[0] == "analysis-review"} == set(model.finished)
    recovered = Model()
    replay = _Analysis(data[1], data[2], data[0], IDENTITY, budget())
    _, results = await replay.execute("review", payloads, recovered, data[3])
    assert {p["work_unit_id"] for p in recovered.calls} == {
        p["work_unit_id"] for p in [payloads[0], *payloads[2:]]}
    assert len(results) == len(payloads)
    assert [r["work_unit_id"] for r in replay.operation_refs] == [p["work_unit_id"] for p in payloads]


@pytest.mark.parametrize("failure,stop", [
    (ModelWorkUnitExhausted, Cancelled), (ModelWorkUnitExhausted, StaleExecution),
    (ModelWorkUnitExhausted, asyncio.CancelledError), (ModelInfrastructureError, ModelWorkUnitExhausted),
])
async def test_stage_prioritizes_stop_and_exhaustion_over_other_failures(tmp_path, failure, stop):
    data, context, payloads = await stage(tmp_path, count=2)
    completed = []
    both_started = asyncio.Event()

    class FailingModel:
        async def call(self, task, payload):
            completed.append(payload["work_unit_id"])
            if len(completed) == 2:
                both_started.set()
            await asyncio.wait_for(both_started.wait(), timeout=3)
            if payload is payloads[0]:
                raise failure("verify")
            raise stop("verify")

    with pytest.raises(stop):
        await context.execute("review", payloads, FailingModel(), data[3])
    assert set(completed) == {p["work_unit_id"] for p in payloads}


async def test_stage_keeps_over_budget_position_without_sending_it(tmp_path):
    data, context, payloads = await stage(tmp_path, count=3)
    payloads[1]["issues"] = [{"issue_id": "oversized", "code": "source_formatting_note",
                              "detail": "x" * 100000, "blocking": False}]
    model = ConcurrentModel()
    _, results = await context.execute("review", payloads, model, data[3])
    assert results[1] is None
    assert results[0] is not None and results[2] is not None
    assert {p["work_unit_id"] for p in model.calls} == {payloads[0]["work_unit_id"], payloads[2]["work_unit_id"]}
    assert context.incomplete_requests == [payloads[1]["work_unit_id"]]
    assert any(p["code"] == "analysis_budget_exceeded" for p in context.problems)


async def test_external_cancellation_drains_inflight_requests_and_does_not_start_queued_work(tmp_path):
    data, context, payloads = await stage(tmp_path, count=4)
    started, finished = [], []
    two_started = asyncio.Event()

    class WaitingModel:
        async def call(self, task, payload):
            started.append(payload["work_unit_id"])
            if len(started) == 2:
                two_started.set()
            try:
                await asyncio.Event().wait()
            finally:
                await asyncio.sleep(0)
                finished.append(payload["work_unit_id"])

    task = asyncio.create_task(context.execute("review", payloads, WaitingModel(), data[3]))
    try:
        await asyncio.wait_for(two_started.wait(), timeout=3)
    finally:
        task.cancel()
        with pytest.raises(asyncio.CancelledError):
            await task
    assert len(started) == 2
    assert set(finished) == set(started)
    assert not any(key[0] == "analysis-review" for key in data[3].saved)
