"""Independent requests share transport without inheriting analysis prompts."""

import json

import pytest

from docreader.llm.budget import RequestBudget
from docreader.llm.request import ModelBudgetError, build_request, request_digest

from test_tender_llm import CountedArtifacts, model, response
from test_tender_streaming import ReplayAudit


async def test_prepared_request_preserves_body_and_reuses_paid_response(tmp_path, monkeypatch):
    calls, audit = [], ReplayAudit()
    artifacts = CountedArtifacts(tmp_path)
    budget = RequestBudget(context_tokens=32768, model="gpt-4o", prompts={}, output_reserve=4096)
    request = build_request(budget.model, "独立编制提示", {"schema_version": 2, "rules": []})
    spec = {"protocol": "outline_execution_v1", "budget": budget.spec()}
    client, _ = model(lambda sent: (calls.append(json.loads(sent.content)), response())[1],
                      audit, artifacts=artifacts)
    monkeypatch.setattr(client, "execution_spec", lambda: pytest.fail("Must not pin analysis execution spec"))
    async with client:
        for _ in range(2):
            assert await client.call_request(request, budget, task="outline", execution_spec=spec) == {"result": "ok"}
    assert calls == [request]
    assert len(audit.rows) == 1
    row = next(iter(audit.rows.values()))
    assert row["metadata"]["request_digest"] == row["metadata"]["logical_request_digest"]
    assert row["metadata"]["request_digest"] == request_digest(client.config.chat.base_url, request)


async def test_prepared_request_budget_rejection_never_creates_a_paid_attempt():
    calls = []
    client, audit = model(lambda sent: (calls.append(sent), response())[1])
    budget = RequestBudget(context_tokens=100, model="gpt-4o", prompts={}, output_reserve=32)
    request = build_request(budget.model, "Return JSON.", {"source": "过长" * 100})
    async with client:
        with pytest.raises(ModelBudgetError):
            await client.call_request(request, budget, task="outline", execution_spec={"protocol": "outline"})
    assert not calls and not audit.events
