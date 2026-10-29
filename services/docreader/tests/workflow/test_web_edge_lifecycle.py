"""Isolated PostgreSQL regressions for task recovery and ended-project receipts."""
import copy
from uuid import uuid4

import pytest

from docreader.integrations.store import Conflict
from docreader.integrations.web import WebError, dispatch_authoring, handle_command
from docreader.jobs.settings import Settings
from docreader.tests.workflow.test_web_bridge import (
    configured, finish_outline, outline_tree, payload, runtime as runtime, upload,
)


@pytest.mark.parametrize("outline_only", [False, True])
async def test_resumed_failed_execution_projects_queued_then_running(runtime, monkeypatch, outline_only):
    configured(monkeypatch)
    await upload(runtime)
    start = runtime.bridge.start_outline if outline_only else runtime.bridge.start_authoring
    result_key = "generation" if outline_only else "authoring"
    execution_id = (await start(payload(runtime, request_key="resume-fixture")))[result_key]["execution_id"]
    job = await runtime.bridge.job(runtime.project, execution_id)
    snapshot = runtime.bridge.artifacts.read_json(job["payload"]["authoring_input_ref"])
    await runtime.bridge.authoring.submit(execution_id, snapshot, {"synthetic": True})
    lease = await runtime.bridge.authoring.acquire(execution_id)
    reference = runtime.bridge.artifacts.put_json({"synthetic": "accepted outline response"})
    await runtime.bridge.authoring.commit_artifact(lease, "document-model-response", reference, scope="outline:fixture")
    await runtime.bridge.authoring.begin_request(lease, "synthetic-request", "fixture", "outline",
        {"request_digest": "a" * 64, "logical_request_digest": "b" * 64})
    await runtime.bridge.authoring.finish_request("synthetic-request", "sent")
    await runtime.bridge.authoring.finish_request("synthetic-request", "succeeded", response_ref=reference)
    await runtime.bridge.authoring.release(lease, "failed", error={"type": "SyntheticFailure"})
    async with runtime.bridge.store.transaction() as conn:
        await conn.execute("UPDATE tender_authoring_jobs SET status='failed',error='{}' WHERE execution_id=%s", (execution_id,))
    before = await runtime.bridge.authoring.get_execution(execution_id)
    resumed = (await runtime.bridge.update_job(payload(runtime, execution_id=execution_id, request_key="resume-once")))["authoring"]
    assert resumed["status"] == "pending" and resumed["progress"]["state"] == "pending"
    assert resumed["progress"]["accepted_units"] == 1 and resumed["progress"]["request_states"]["received"] == 1
    assert resumed["error"] is None
    assert (await runtime.bridge.authoring_view(await runtime.bridge.job(runtime.project, execution_id))) == resumed
    projected = (await runtime.bridge.outline_view(runtime.project))["generation"] if outline_only else (await runtime.bridge.workbench(runtime.project))["authoring"]
    assert projected == resumed
    assert await dispatch_authoring(runtime.bridge.store, lambda _execution: None) == 1
    published = await runtime.bridge.authoring_view(await runtime.bridge.job(runtime.project, execution_id))
    assert published["status"] == "published" and published["error"] is None
    await runtime.bridge.authoring.acquire(execution_id)
    running = await runtime.bridge.authoring_view(await runtime.bridge.job(runtime.project, execution_id))
    assert running["status"] == "running" and running["progress"]["state"] == "running"
    after = await runtime.bridge.authoring.get_execution(execution_id)
    assert after["receipts"] == before["receipts"] and after["requests"] == before["requests"]


@pytest.mark.parametrize("action", ["authoring.start", "outline.start"])
async def test_ended_project_replays_exact_existing_job_without_requeue(runtime, monkeypatch, action):
    configured(monkeypatch)
    await upload(runtime)
    monkeypatch.setattr(Settings, "from_env", classmethod(lambda cls: runtime.settings))
    request = payload(runtime, action, request_key="known-submit")
    first, _ = await handle_command(request)
    key = "authoring" if action == "authoring.start" else "generation"
    execution_id = first[key]["execution_id"]
    async with runtime.bridge.store.transaction() as conn:
        await conn.execute("UPDATE tender_authoring_jobs SET status='failed' WHERE execution_id=%s", (execution_id,))
        await conn.execute("UPDATE tender_projects SET status='ended' WHERE id=%s", (runtime.project,))
    before = await runtime.bridge.job(runtime.project, execution_id)
    replay, _ = await handle_command(request)
    assert replay[key]["execution_id"] == execution_id and replay[key]["status"] == "failed"
    assert await runtime.bridge.job(runtime.project, execution_id) == before
    with pytest.raises((WebError, Conflict)):
        await handle_command(request | {"review_request": {"target_ids": ["other"], "reason": "changed"}})
    with pytest.raises(WebError) as new:
        await handle_command(request | {"request_key": "never-submitted"})
    assert new.value.code == "PROJECT_ENDED"
    assert await dispatch_authoring(runtime.bridge.store, lambda _execution: pytest.fail("replay requeued a failed job")) == 0


async def test_ended_project_replays_saved_outline_without_new_revision(runtime, monkeypatch):
    await upload(runtime)
    execution_id, _, outline = await finish_outline(runtime, monkeypatch)
    request = payload(runtime, "outline.save", request_key="known-save", expected_revision_id=None,
                      source_execution_id=execution_id, chapters=outline_tree(outline))
    monkeypatch.setattr(Settings, "from_env", classmethod(lambda cls: runtime.settings))
    saved, _ = await handle_command(request)
    async with runtime.bridge.store.transaction() as conn:
        await conn.execute("UPDATE tender_projects SET status='ended' WHERE id=%s", (runtime.project,))
    assert (await handle_command(request))[0] == saved
    changed = copy.deepcopy(request)
    changed["chapters"][0]["title"] = "different"
    with pytest.raises(Conflict):
        await handle_command(changed)
    with pytest.raises(WebError) as new:
        await handle_command(request | {"request_key": "new-save"})
    assert new.value.code == "PROJECT_ENDED"


async def test_ended_project_confirms_old_resume_but_only_allows_cancellation_as_new_action(runtime, monkeypatch):
    configured(monkeypatch)
    await upload(runtime)
    execution_id = (await runtime.bridge.start_authoring(payload(runtime, request_key="resume-before-end")))["authoring"]["execution_id"]
    request = payload(runtime, "authoring.resume", execution_id=execution_id, request_key="old-resume")
    await runtime.bridge.update_job(request)
    async with runtime.bridge.store.transaction() as conn:
        await conn.execute("UPDATE tender_authoring_jobs SET status='failed' WHERE execution_id=%s", (execution_id,))
        await conn.execute("UPDATE tender_projects SET status='ended' WHERE id=%s", (runtime.project,))
    monkeypatch.setattr(Settings, "from_env", classmethod(lambda cls: runtime.settings))
    before = await runtime.bridge.job(runtime.project, execution_id)
    receipt, _ = await handle_command(request)
    assert receipt["authoring"]["status"] == "failed"
    assert await runtime.bridge.job(runtime.project, execution_id) == before
    with pytest.raises(WebError) as new:
        await handle_command(request | {"request_key": "new-resume"})
    assert new.value.code == "PROJECT_ENDED"
    with pytest.raises(WebError) as foreign:
        await handle_command(request | {"owner_user_id": str(uuid4())})
    assert foreign.value.status == 404
    stopped, _ = await handle_command(payload(runtime, "authoring.cancel", execution_id=execution_id))
    assert stopped["authoring"]["status"] == "cancelled"


async def test_ended_saved_review_only_reads_exact_original_receipt(runtime, monkeypatch):
    from docreader.integrations.artifacts import ArtifactStore
    from docreader.tests.workflow.test_web_saved_review import saved

    configured(monkeypatch)
    request, data = await saved(runtime)
    monkeypatch.setattr(Settings, "from_env", classmethod(lambda cls: runtime.settings))
    first, _ = await handle_command(request, data)
    execution_id = first["review"]["execution_id"]
    async with runtime.bridge.store.transaction() as conn:
        await conn.execute("UPDATE tender_authoring_jobs SET status='failed' WHERE execution_id=%s", (execution_id,))
        await conn.execute("UPDATE tender_projects SET status='ended' WHERE id=%s", (runtime.project,))
    before = await runtime.bridge.job(runtime.project, execution_id)
    def forbidden(*_args, **_kwargs):
        pytest.fail("ended receipt cannot write artifacts or admit another model task")
    from docreader.llm.config import ModelConfig
    monkeypatch.setattr(ModelConfig, "from_env", forbidden)
    monkeypatch.setattr(ArtifactStore, "put_json", forbidden)
    monkeypatch.setattr(ArtifactStore, "put_bytes", forbidden)
    replay, _ = await handle_command(request)  # No original Word bytes are needed for a receipt.
    assert replay["review"]["execution_id"] == execution_id and replay["review"]["status"] == "failed"
    assert await runtime.bridge.job(runtime.project, execution_id) == before
    with pytest.raises(Conflict):
        await handle_command(request | {"reason": "changed"})
    with pytest.raises(WebError) as new:
        await handle_command(request | {"request_key": "new-review"})
    assert new.value.code == "PROJECT_ENDED"
