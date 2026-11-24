"""Bind historical review preparation to the direct same-source correction."""

from copy import deepcopy
from hashlib import sha256
import json

import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.integrations.store import Lease
from docreader.tender.checks import analysis
from docreader.workflow.nodes import TenderNodes


class Store:
    def __init__(self, revision, history):
        self.revision = revision
        self.history = history
        self.saved = {}
        self.history_reads = 0

    async def get_revision(self, lease):
        return self.revision

    async def get_run(self, run_id):
        self.history_reads += 1
        return {"history": self.history}

    async def pin_execution_spec(self, lease, spec):
        pass

    async def receipt(self, lease, stage, scope, *, input_digest):
        value = self.saved.get((stage, scope))
        if value:
            assert value["input_digest"] == input_digest
        return value

    async def commit_artifact(self, lease, stage, ref, *, scope, metadata, input_digest):
        self.saved[(stage, scope)] = {"artifact_ref": ref, "input_digest": input_digest}


def history_fixture(tmp_path, broken=None):
    artifacts = ArtifactStore(tmp_path)
    current = {"run_id": "same-run", "source_version": 2, "domain_revision": 3}
    previous = {**current, "domain_revision": 2}
    old_plan = {"schema_version": 2, "frozen_ref": artifacts.put_json({"documents": []}),
                "scope_decisions": [], "packs": [], "run_identity": previous}
    old_items = {"schema_version": 2, "packs": []}
    source_plan = {**old_plan, "run_identity": current}
    current_items = {"schema_version": 2, "packs": [], "counts": {"packs": 0}}
    historical_budget = {"context_tokens": 131072}
    scope = analysis._digest({
        "planner": "source-review-requests-v4", "source_plan": old_plan,
        "items_index": old_items, "identity": previous, "budget": historical_budget,
    })
    digest = sha256(json.dumps({"upstream": {"planning_digest": scope}},
                               sort_keys=True, separators=(",", ":")).encode()).hexdigest()
    prior_requests = [{"operation": "review", "work_unit_id": "review-previous",
                       "dispositions": [{"range_id": "range-1"}], "source_views": []}]
    prepared = {"requests_json": json.dumps(prior_requests)}
    revision = {**current, "corrected_items_ref": artifacts.put_json(current_items),
                "upstream_digest": artifacts.put_json(old_items)["sha256"], "scope_decisions_ref": None}
    old_revision = {**previous, "scope_decisions_ref": None,
                    "execution_spec": {"budget": {"chat": historical_budget}}}
    old_rows = [
        {**previous, "stage": "pack-plan", "scope": "", "artifact_ref": artifacts.put_json(old_plan)},
        {**previous, "stage": "extract", "scope": "", "artifact_ref": artifacts.put_json(old_items)},
        {**previous, "stage": "analysis-review-requests", "scope": scope, "input_digest": digest,
         "artifact_ref": artifacts.put_json(prepared)},
    ]
    if broken == "scope_review":
        revision["corrected_items_ref"] = None
    elif broken == "scope_decision":
        revision["scope_decisions_ref"] = artifacts.put_json({"scope_decisions": []})
    elif broken == "source":
        source_plan["frozen_ref"] = artifacts.put_json({"documents": ["other"]})
    elif broken == "scope_plan":
        source_plan["scope_decisions"] = [{"different_scope": True}]
    elif broken == "source_version":
        old_rows[-1]["source_version"] = 1
    elif broken == "ancestor":
        old_rows[-1]["domain_revision"] = 1
    elif broken == "baseline":
        revision["upstream_digest"] = "wrong"
    elif broken == "execution_budget":
        old_revision["execution_spec"] = {}
    elif broken == "scope_key":
        old_rows[-1]["scope"] = "wrong"
    elif broken == "input_digest":
        old_rows[-1]["input_digest"] = "wrong"
    elif broken == "missing_artifact":
        artifacts.path(old_rows[-1]["artifact_ref"]).unlink()
    elif broken == "malformed_requests":
        old_rows[-1]["artifact_ref"] = artifacts.put_json({"requests_json": "[1]"})
    store = Store(revision, {"artifacts": old_rows, "revisions": [old_revision]})
    state = {"pack_plan_ref": artifacts.put_json(source_plan), "items_ref": artifacts.put_json(current_items),
             "frozen_ref": source_plan["frozen_ref"]}
    return TenderNodes(store, artifacts, Lease("same-run", 2, 1, "command", 3), object()), state, (
        old_plan, old_items, prior_requests,
    )


async def test_verify_loads_exact_history_lazily_and_only_before_its_current_receipt(tmp_path, monkeypatch):
    nodes, state, expected = history_fixture(tmp_path)
    seen = []

    async def verify(source_plan, items, artifacts, model, budget, *, identity, save, previous_review=None):
        assert previous_review is not None
        assert seen == [] and nodes.store.history_reads == 0
        seen.append(await previous_review())
        return {"hard_blocking": False}

    monkeypatch.setattr(analysis, "verify_analysis", verify)
    first = await nodes.verify(state)
    assert seen == [expected]
    assert nodes.store.history_reads == 1
    # The regular verify receipt still settles same-revision recovery before
    # history discovery, even if the predecessor is now unavailable.
    monkeypatch.setattr(nodes.store, "history", None)
    assert await nodes.verify(deepcopy(state)) == first
    assert nodes.store.history_reads == 1


@pytest.mark.parametrize("broken", [
    "scope_review", "scope_decision", "source", "scope_plan", "source_version", "ancestor", "baseline",
    "execution_budget", "scope_key", "input_digest", "missing_artifact", "malformed_requests",
])
async def test_verify_discards_ineligible_or_unavailable_history(tmp_path, monkeypatch, broken):
    nodes, state, _ = history_fixture(tmp_path, broken)

    async def verify(source_plan, items, artifacts, model, budget, *, identity, save, previous_review=None):
        assert previous_review is not None
        assert await previous_review() is None
        return {"hard_blocking": False}

    monkeypatch.setattr(analysis, "verify_analysis", verify)
    await nodes.verify(state)
    if broken == "scope_review":
        assert nodes.store.history_reads == 0
