"""Regression tests for BUG-348: confirmation resumes across prompt/budget deploys while mid-run drift fails fast."""
from __future__ import annotations

import copy
from types import SimpleNamespace
from unittest.mock import AsyncMock

import pytest

from docreader.integrations.store import Conflict, Lease
from docreader.integrations import web_recovery
from docreader.integrations.web_recovery import analysis_recovery
from docreader.llm.budget import RequestBudget
from docreader.workflow.nodes import TenderNodes
from docreader.workflow.runner import _config_conflict_reason
from docreader.workflow.state import WORKFLOW_PROTOCOL


class _FakeStore:
    def __init__(self, *, pinned_spec: dict, plan_budget_digest: str | None = None):
        self.pinned_spec = copy.deepcopy(pinned_spec)
        self.plan_budget_digest = plan_budget_digest
        self.pin_calls: list[dict] = []
        self.receipt_calls = 0

    async def get_revision(self, lease: Lease) -> dict:
        return {"execution_spec": copy.deepcopy(self.pinned_spec)}

    async def get_run(self, run_id: str) -> dict:
        artifacts = [{"stage": "pack-plan", "scope": "", "metadata": {"budget_digest": self.plan_budget_digest}}] if self.plan_budget_digest else []
        return {
            "artifacts": artifacts,
            "history": {"revisions": []},
        }

    async def pin_execution_spec(self, lease: Lease, spec: dict) -> dict:
        self.pin_calls.append(copy.deepcopy(spec))
        if self.pinned_spec is not None and self.pinned_spec != spec:
            raise Conflict("execution configuration changed within a domain revision")
        self.pinned_spec = copy.deepcopy(spec)
        return copy.deepcopy(self.pinned_spec)

    async def receipt(self, lease: Lease, stage: str, scope: str = "", *, input_digest: str = "") -> dict | None:
        self.receipt_calls += 1
        return {"artifact_ref": {"sha256": "f" * 64}}


class _FakeArtifacts:
    pass


class _ChangedModel:
    def __init__(self, *, prompt_hash: str, budget: RequestBudget):
        self._prompt_hash = prompt_hash
        self._budget = budget

    def execution_spec(self) -> dict:
        return {"prompts": {"extract": self._prompt_hash}}

    def request_budget(self) -> RequestBudget:
        return self._budget


async def test_bug348_prepare_allows_confirm_resume_on_prompt_or_budget_change_only():
    lease = Lease("run-1", 1, 1, "cmd-1", 1)
    pinned = {"prompts": {"extract": "old-prompt"}, "workflow_protocol": WORKFLOW_PROTOCOL}
    old_budget = RequestBudget(context_tokens=131072)
    import hashlib
    import json

    old_budget_digest = hashlib.sha256(json.dumps(old_budget.spec(), sort_keys=True, separators=(",", ":")).encode()).hexdigest()

    store = _FakeStore(pinned_spec=pinned, plan_budget_digest=old_budget_digest)
    model = _ChangedModel(prompt_hash="new-deployed-prompt", budget=RequestBudget(context_tokens=65536))
    nodes = TenderNodes(store, _FakeArtifacts(), lease, model=model)

    # Normal preparation still strictly rejects prompt/budget drift.
    with pytest.raises(Conflict, match="execution configuration changed"):
        await nodes.prepare()
    store.pin_calls.clear()

    # Confirmation resume skips model/parser/budget checks without mutating pinned execution_spec.
    await nodes.prepare(resuming_confirmation=True)
    assert store.pin_calls == []
    assert store.receipt_calls == 0
    assert store.pinned_spec == pinned

    # Changed workflow_protocol must still be rejected even on confirmation resume.
    legacy_store = _FakeStore(pinned_spec={**pinned, "workflow_protocol": {"schema_version": 1}})
    legacy_nodes = TenderNodes(legacy_store, _FakeArtifacts(), lease, model=model)
    with pytest.raises(Conflict, match="read-only"):
        await legacy_nodes.prepare(resuming_confirmation=True)


@pytest.mark.parametrize(
    ("message", "expected"),
    [
        ("execution configuration changed within a domain revision", "execution_config_changed"),
        ("request budget changed within an immutable domain revision", "execution_config_changed"),
        ("execution protocol changed; prior revisions are read-only, submit a new analysis run", "execution_config_changed"),
        ("artifact inputs changed within an immutable stage identity", "execution_config_changed"),
        ("retry baseline is stale or the run is cancelled", None),
    ],
)
def test_bug348_config_conflict_reason_classification(message: str, expected: str | None):
    assert _config_conflict_reason(Conflict(message)) == expected


async def test_bug348_web_recovery_reports_execution_config_changed_notice(monkeypatch):
    run = {
        "run_id": "run-1",
        "source_version": 1,
        "domain_revision": 1,
        "status": "failed",
        "cancel_requested": False,
        "lease_live": False,
        "checklist_digest": None,
        "commands": [
            {
                "command_id": "11111111-1111-4111-8111-111111111111",
                "created_at": "2026-11-26T00:00:00Z",
                "status": "completed",
                "receipt": {"status": "failed", "metadata": {"reason": "execution_config_changed"}},
            }
        ],
        "usage": [],
    }

    class _FakeTx:
        async def __aenter__(self):
            return self

        async def __aexit__(self, *_args):
            return False

    fake_bridge = SimpleNamespace(
        store=SimpleNamespace(
            transaction=lambda: _FakeTx(),
            _assert_analysis_revision=AsyncMock(return_value=None),
        )
    )
    monkeypatch.setattr(web_recovery, "_owned_run", AsyncMock(return_value=run))

    view = (await analysis_recovery(fake_bridge, {}))["recovery"]
    assert view is not None
    assert view["failure_reason"] == "execution_config_changed"
    assert view["can_retry"] is True
    assert "执行配置" in view["notice"]
    assert "已用尽重试" not in view["notice"]
