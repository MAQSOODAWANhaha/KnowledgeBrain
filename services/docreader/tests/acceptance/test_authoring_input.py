"""Confirmed analysis input with real PostgreSQL and explicitly synthetic platform_models."""

import pytest

from docreader.integrations.authoring import read_authoring_input
from docreader.integrations.store import InvalidConfirmation

from test_analysis import analysis_runtime as analysis_runtime, execute_analysis, result


pytestmark = pytest.mark.integration


def expectation(row):
    return {key: row[key] for key in ("run_id", "source_version", "domain_revision", "checklist_digest")}


def root_reference(row, stage):
    return next(entry["artifact_ref"] for entry in row["artifacts"]
                if entry["stage"] == stage and not entry["scope"])


async def test_authoring_reads_available_current_analysis_without_model_or_state_changes(analysis_runtime):
    store, artifacts, _, run, calls = analysis_runtime
    waiting = await execute_analysis(analysis_runtime)
    assert waiting["status"] == "waiting-confirmation"
    before_confirm = len(calls)
    pending_input = await read_authoring_input(store, artifacts, **expectation(waiting))
    assert pending_input["confirmation_ref"] is None and pending_input["issues"]

    await store.confirm(run["run_id"], 1, waiting["checklist_digest"], "authoring-input", domain_revision=1)
    ready = await execute_analysis(analysis_runtime)
    assert ready["status"] == "analysis-ready" and len(calls) == before_confirm
    files = set(artifacts.root.rglob("*"))
    value = await read_authoring_input(store, artifacts, **expectation(ready))
    assert value == {
        "schema_version": 2, "extract_refs": [], "issues": [],
        **expectation(ready),
        "frozen_ref": root_reference(ready, "freeze"),
        "pack_plan_ref": root_reference(ready, "pack-plan"),
        "items_ref": root_reference(ready, "extract"),
        "verification_ref": root_reference(ready, "verify"),
        "confirmation_ref": root_reference(ready, "confirm"),
    }
    assert await read_authoring_input(store, artifacts, **expectation(ready)) == value
    assert await store.get_run(run["run_id"]) == ready
    assert set(artifacts.root.rglob("*")) == files
    assert len(calls) == before_confirm
    assert all(request["metadata"]["external_request"] is False for request in ready["usage"])


async def test_acknowledged_semantic_issues_remain_visible_to_authoring(analysis_runtime):
    store, artifacts, _, run, calls = analysis_runtime
    waiting = await execute_analysis(analysis_runtime, blocked=True)
    verification = result(artifacts, waiting, "verify")
    assert verification["blocking"] is True and verification["hard_blocking"] is False
    pending = verification["human_issue_ids"]
    assert pending
    resolutions = {"schema_version": 1, "resolutions": [
        {"issue_id": issue_id, "verdict": "acknowledged", "reason": "合成验收：人工知悉并保留原问题。"}
        for issue_id in sorted(pending)]}
    before = len(calls)
    await store.confirm(run["run_id"], 1, waiting["checklist_digest"], "authoring-retained-issues",
                        domain_revision=1, issue_resolutions=resolutions)
    ready = await execute_analysis(analysis_runtime)
    assert ready["status"] == "analysis-ready"

    value = await read_authoring_input(store, artifacts, **expectation(ready))
    retained = artifacts.read_json(value["verification_ref"])
    assert retained == verification
    assert retained["blocking"] is True
    assert retained["human_issue_ids"] == pending
    assert artifacts.read_json(value["confirmation_ref"])["issue_resolutions"] == resolutions
    assert len(calls) == before
    assert (await store.get_run(run["run_id"]))["usage"] == ready["usage"]


async def test_authoring_rejects_old_expected_revision_after_correction_and_reconfirmation(analysis_runtime):
    store, artifacts, _, run, calls = analysis_runtime
    waiting = await execute_analysis(analysis_runtime)
    await store.confirm(run["run_id"], 1, waiting["checklist_digest"], "authoring-old", domain_revision=1)
    old = await execute_analysis(analysis_runtime)
    snapshot = await read_authoring_input(store, artifacts, **expectation(old))
    baseline = snapshot["items_ref"]

    # The existing valid index is a complete unchanged correction; the node
    # revalidates every pack before creating the new revision's analysis.
    await store.correct(run["run_id"], 1, 1, baseline["sha256"], baseline,
                        "authoring-correction", "Reconfirm the unchanged synthetic source interpretation")
    with pytest.raises(InvalidConfirmation):
        await read_authoring_input(store, artifacts, **expectation(old))
    revised = await execute_analysis(analysis_runtime)
    assert revised["domain_revision"] == 2 and revised["status"] == "waiting-confirmation"
    assert revised["history"]["confirmations"] and not revised["confirmations"]
    await store.confirm(run["run_id"], 1, revised["checklist_digest"], "authoring-new", domain_revision=2)
    current = await execute_analysis(analysis_runtime)
    assert current["status"] == "analysis-ready"
    before = len(calls)

    with pytest.raises(InvalidConfirmation):
        await read_authoring_input(store, artifacts, **expectation(old))
    for stale in ({"source_version": 2}, {"checklist_digest": old["checklist_digest"]}):
        with pytest.raises(InvalidConfirmation):
            await read_authoring_input(store, artifacts, **{**expectation(current), **stale})
    value = await read_authoring_input(store, artifacts, **expectation(current))
    assert value["domain_revision"] == 2
    assert value["checklist_digest"] != snapshot["checklist_digest"]
    assert value["frozen_ref"] == snapshot["frozen_ref"]
    assert value["verification_ref"] == root_reference(current, "verify")
    assert value["confirmation_ref"] == root_reference(current, "confirm")
    assert len(calls) == before
