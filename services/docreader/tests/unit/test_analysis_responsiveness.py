"""Analysis planning must leave the event loop available for lease heartbeats."""

import asyncio
from contextvars import ContextVar
from threading import Event

import pytest

from docreader.llm.budget import RequestBudget
from docreader.tender.checks.analysis import _Analysis
from test_analysis_checks import IDENTITY, Model, Receipts, all_source_candidate, budget, run, scenario


@pytest.mark.asyncio
@pytest.mark.parametrize("planning_step", [
    "review_requests", "locate_requests", "association_requests", "descriptors",
])
async def test_planning_keeps_loop_responsive_and_preserves_request_receipts(tmp_path, monkeypatch, planning_step):
    data = await scenario(tmp_path)
    admission = budget()
    expected_model = Model(locate=all_source_candidate)
    expected = await run(data, expected_model, admission)
    assert expected["blocking"] is False
    assert {payload["operation"] for payload in expected_model.calls} == {"review", "locate", "associate"}
    loop = asyncio.get_running_loop()
    responses = []

    def wait_for_loop():
        """A lease heartbeat can run only if the loop acknowledges this callback."""
        acknowledged = Event()

        def heartbeat():
            responses.append(planning_step)
            acknowledged.set()

        loop.call_soon_threadsafe(heartbeat)
        assert acknowledged.wait(5), f"{planning_step} blocked event-loop heartbeat scheduling"

    if planning_step == "descriptors":
        descriptor_context = ContextVar("analysis_descriptor_planning", default=False)
        original_execute, original_measure = _Analysis.execute, RequestBudget.measure

        async def execute(self, *args, **kwargs):
            token = descriptor_context.set(True)
            try:
                return await original_execute(self, *args, **kwargs)
            finally:
                descriptor_context.reset(token)

        def measure(self, *args, **kwargs):
            if self is admission and descriptor_context.get():
                wait_for_loop()
            return original_measure(self, *args, **kwargs)

        monkeypatch.setattr(_Analysis, "execute", execute)
        monkeypatch.setattr(RequestBudget, "measure", measure)
    else:
        original = getattr(_Analysis, planning_step)

        def plan(self, *args, **kwargs):
            wait_for_loop()
            return original(self, *args, **kwargs)

        monkeypatch.setattr(_Analysis, planning_step, plan)

    artifacts, source_plan, items, _ = data
    fresh = (artifacts, source_plan, items, Receipts(artifacts))
    model = Model(locate=all_source_candidate)
    actual = await run(fresh, model, admission)
    assert responses, "the selected synchronous planning path must actually execute"
    assert actual == expected
    assert model.calls == expected_model.calls
    assert all(payload["source_version"] == IDENTITY["source_version"]
               and payload["domain_revision"] == IDENTITY["domain_revision"] for payload in model.calls)
    before_calls, before_responses = len(model.calls), len(responses)
    assert await run(fresh, model, admission) == actual
    assert len(model.calls) == before_calls
    assert len(responses) > before_responses
