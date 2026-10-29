"""Ended projects expose persisted analysis receipts without enqueueing commands."""
import copy
from uuid import uuid4

import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.integrations.store import Conflict, Store
from docreader.integrations.web import WebError, handle_command
from docreader.integrations.web_analysis import confirm_analysis, correct_analysis
from docreader.integrations.web_recovery import retry_analysis
from docreader.jobs.settings import Settings
from docreader.tests.workflow.test_web_analysis import (
    analysis_runtime as analysis_runtime, confirmation, correction, payload,
    review_runtime as review_runtime, waiting,
)
from docreader.tests.workflow.test_web_bridge import payload as bridge_payload, runtime as runtime, upload
from docreader.tests.workflow.test_web_recovery import failure, retry_body


def forbid_mutations(monkeypatch):
    def forbidden(*_args, **_kwargs):
        pytest.fail("ended receipt must not write artifacts or enqueue analysis commands")
    for method in ("confirm", "correct", "retry", "submit"):
        monkeypatch.setattr(Store, method, forbidden)
    monkeypatch.setattr(ArtifactStore, "put_json", forbidden)
    monkeypatch.setattr(ArtifactStore, "put_bytes", forbidden)


@pytest.mark.parametrize("action", ["analysis.confirm", "analysis.correct"])
async def test_ended_analysis_replays_full_original_payload_without_writes(review_runtime, monkeypatch, action):
    runtime = review_runtime
    _, review = await waiting(runtime, blocked=True)
    body = confirmation(review) if action == "analysis.confirm" else correction(review)
    operation = confirm_analysis if action == "analysis.confirm" else correct_analysis
    request = payload(runtime, action=action, **body)
    first = await operation(runtime.bridge, request)
    async with runtime.bridge.store.transaction() as conn:
        await conn.execute("UPDATE tender_projects SET status='ended' WHERE id=%s", (runtime.project,))
    monkeypatch.setattr(Settings, "from_env", classmethod(lambda cls: runtime.bridge.settings))
    before = await runtime.bridge.store.get_run(review["run_id"])
    forbid_mutations(monkeypatch)
    assert (await handle_command(request))[0] == first
    assert await runtime.bridge.store.get_run(review["run_id"]) == before
    changed = copy.deepcopy(request)
    if action == "analysis.confirm":
        changed["issue_resolutions"]["resolutions"][0]["reason"] = "another decision"
    else:
        changed["changes"][0]["text"] += "another correction"
    with pytest.raises(Conflict):
        await handle_command(changed)
    with pytest.raises(WebError) as new:
        await handle_command(request | {"request_key": "unsubmitted-action"})
    assert new.value.code == "PROJECT_ENDED"
    with pytest.raises(WebError) as foreign:
        await handle_command(request | {"owner_user_id": str(uuid4())})
    assert foreign.value.status == 404


async def test_ended_analysis_retry_reads_existing_command_only(runtime, monkeypatch):
    run, command = await failure(runtime)
    request = bridge_payload(runtime, "analysis.retry", **retry_body(run, command))
    first = await retry_analysis(runtime.bridge, request)
    async with runtime.bridge.store.transaction() as conn:
        await conn.execute("UPDATE tender_projects SET status='ended' WHERE id=%s", (runtime.project,))
    monkeypatch.setattr(Settings, "from_env", classmethod(lambda cls: runtime.settings))
    before = await runtime.bridge.store.get_run(run["run_id"])
    forbid_mutations(monkeypatch)
    assert (await handle_command(request))[0] == first
    assert await runtime.bridge.store.get_run(run["run_id"]) == before
    with pytest.raises(Conflict):
        await handle_command(request | {"reason": "changed reason"})
    with pytest.raises(WebError) as new:
        await handle_command(request | {"request_key": "new-recovery"})
    assert new.value.code == "PROJECT_ENDED"


async def test_ended_analysis_start_receipt_does_not_submit_another_run(runtime, monkeypatch):
    await upload(runtime)
    request = bridge_payload(runtime, "analysis.start", request_key="start-once")
    first = await runtime.bridge.start_analysis(request)
    async with runtime.bridge.store.transaction() as conn:
        await conn.execute("UPDATE tender_projects SET status='ended' WHERE id=%s", (runtime.project,))
    monkeypatch.setattr(Settings, "from_env", classmethod(lambda cls: runtime.settings))
    forbid_mutations(monkeypatch)
    replay, _ = await handle_command(request)
    assert replay["analysis"]["run_id"] == first["analysis"]["run_id"]
    with pytest.raises(WebError) as new:
        await handle_command(request | {"request_key": "new-start"})
    assert new.value.code == "PROJECT_ENDED"


@pytest.mark.parametrize("ended", [False, True])
async def test_analysis_start_rejects_changed_body_for_existing_key(runtime, monkeypatch, ended):
    await upload(runtime)
    request = bridge_payload(runtime, "analysis.start", request_key="start-empty-body")
    first = await runtime.bridge.start_analysis(request)
    if ended:
        async with runtime.bridge.store.transaction() as conn:
            await conn.execute("UPDATE tender_projects SET status='ended' WHERE id=%s", (runtime.project,))
    monkeypatch.setattr(Settings, "from_env", classmethod(lambda cls: runtime.settings))
    before = await runtime.bridge.store.get_run(first["analysis"]["run_id"])
    with pytest.raises(WebError) as invalid:
        await handle_command(request | {"previous_execution_id": "unexpected-body-change"})
    assert invalid.value.status == 400
    assert await runtime.bridge.store.get_run(first["analysis"]["run_id"]) == before
