"""Project closure racing an admitted command cannot publish new business work."""
import asyncio

import psycopg
import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.integrations.store import Conflict
from docreader.integrations.web import WebBridge, WebError, handle_command
from docreader.jobs.settings import Settings
from docreader.tests.workflow.test_web_analysis import (
    analysis_runtime as analysis_runtime, confirmation, correction,
    payload as analysis_payload, review_runtime as review_runtime, waiting,
)
from docreader.tests.workflow.test_web_bridge import (
    configured, payload, runtime as runtime, upload,
)
from docreader.tests.workflow.test_web_recovery import failure, retry_body
from docreader.tests.workflow.test_web_saved_review import saved


async def end_after_authorization(runtime, monkeypatch, request, blob=b"", *, pause_at=None):
    """Pause the last authorization, commit closure, then release submission."""
    monkeypatch.setattr(Settings, "from_env", classmethod(lambda cls: runtime.bridge.settings))
    original = WebBridge.authorize
    authorized, release = asyncio.Event(), asyncio.Event()
    last = pause_at or (2 if request["action"] in {"analysis.confirm", "analysis.correct", "analysis.retry"} else 1)
    calls = 0

    async def admitted(bridge, body, *, write=False):
        nonlocal calls
        result = await original(bridge, body, write=write)
        if body["project_id"] == runtime.project:
            calls += 1
            if calls == last:
                authorized.set()
                await asyncio.wait_for(release.wait(), 15)
        return result

    monkeypatch.setattr(WebBridge, "authorize", admitted)
    task = asyncio.create_task(handle_command(request, blob))
    try:
        await asyncio.wait_for(authorized.wait(), 15)
        async with runtime.bridge.store.transaction() as conn:
            await conn.execute("UPDATE tender_projects SET status='ended' WHERE id=%s", (runtime.project,))
        release.set()
        return await asyncio.wait_for(task, 15)
    finally:
        release.set()
        if not task.done():
            task.cancel()
        await asyncio.gather(task, return_exceptions=True)


async def business_rows(runtime):
    async with runtime.bridge.store.transaction() as conn:
        return {table: await (await conn.execute(f"SELECT * FROM {table} ORDER BY 1,2")).fetchall()
                for table in ("tender_analysis_runs", "tender_analysis_commands", "tender_analysis_revisions",
                              "tender_analysis_confirmations", "tender_project_analyses", "tender_authoring_jobs",
                              "tender_document_sources")}


@pytest.mark.parametrize("action", ["authoring.start", "outline.start", "analysis.start"])
async def test_end_between_authorization_and_new_submission(runtime, monkeypatch, action):
    configured(monkeypatch)
    await upload(runtime)
    await runtime.bridge.source_run(runtime.project)
    before = await business_rows(runtime)
    with pytest.raises(WebError) as closed:
        await end_after_authorization(runtime, monkeypatch, payload(runtime, action, request_key="racing-new"))
    assert closed.value.code == "PROJECT_ENDED"
    assert await business_rows(runtime) == before


async def test_end_between_authorization_and_saved_review(runtime, monkeypatch):
    configured(monkeypatch)
    request, data = await saved(runtime)
    before = await business_rows(runtime)
    with pytest.raises(WebError) as closed:
        await end_after_authorization(runtime, monkeypatch, request, data)
    assert closed.value.code == "PROJECT_ENDED"
    assert await business_rows(runtime) == before


@pytest.mark.parametrize("replay", [False, True])
async def test_end_between_authorization_and_resume(runtime, monkeypatch, replay):
    configured(monkeypatch)
    await upload(runtime)
    execution = (await runtime.bridge.start_authoring(payload(runtime, request_key="seed")))["authoring"]["execution_id"]
    request = payload(runtime, "authoring.resume", execution_id=execution, request_key="race-resume")
    if replay:
        await runtime.bridge.update_job(request)
    async with runtime.bridge.store.transaction() as conn:
        await conn.execute("UPDATE tender_authoring_jobs SET status='failed' WHERE execution_id=%s", (execution,))
    before = await business_rows(runtime)
    if replay:
        result, _ = await end_after_authorization(runtime, monkeypatch, request)
        assert result["authoring"]["execution_id"] == execution and result["authoring"]["status"] == "failed"
    else:
        with pytest.raises(WebError) as closed:
            await end_after_authorization(runtime, monkeypatch, request)
        assert closed.value.code == "PROJECT_ENDED"
    assert await business_rows(runtime) == before


@pytest.mark.parametrize("action", ["analysis.confirm", "analysis.correct"])
@pytest.mark.parametrize("replay", [False, True])
async def test_end_between_last_authorization_and_analysis_decision(review_runtime, monkeypatch, action, replay):
    runtime = review_runtime
    _, review = await waiting(runtime, blocked=True)
    request = analysis_payload(runtime, action=action, **(confirmation(review) if action == "analysis.confirm" else correction(review)))
    monkeypatch.setattr(Settings, "from_env", classmethod(lambda cls: runtime.bridge.settings))
    if replay:
        first, _ = await handle_command(request)
    before = await business_rows(runtime)
    calls = len(runtime.calls)
    if replay:
        assert (await end_after_authorization(runtime, monkeypatch, request))[0] == first
    else:
        with pytest.raises(WebError) as closed:
            await end_after_authorization(runtime, monkeypatch, request)
        assert closed.value.code == "PROJECT_ENDED"
    assert await business_rows(runtime) == before and len(runtime.calls) == calls


@pytest.mark.parametrize("replay", [False, True])
async def test_end_between_last_authorization_and_analysis_retry(runtime, monkeypatch, replay):
    run, command = await failure(runtime)
    request = payload(runtime, "analysis.retry", **retry_body(run, command))
    monkeypatch.setattr(Settings, "from_env", classmethod(lambda cls: runtime.settings))
    if replay:
        first, _ = await handle_command(request)
    before = await business_rows(runtime)
    if replay:
        assert (await end_after_authorization(runtime, monkeypatch, request))[0] == first
    else:
        with pytest.raises(WebError) as closed:
            await end_after_authorization(runtime, monkeypatch, request)
        assert closed.value.code == "PROJECT_ENDED"
    assert await business_rows(runtime) == before


@pytest.mark.parametrize("existing", [False, True])
async def test_end_between_authorization_and_document_source(runtime, monkeypatch, existing):
    request, data = await upload(runtime)
    if not existing:
        async with runtime.bridge.store.transaction() as conn:
            await conn.execute("DELETE FROM tender_document_sources WHERE document_id=%s", (request["document_id"],))
    before = await business_rows(runtime)
    if existing:
        result, _ = await end_after_authorization(runtime, monkeypatch, request, data)
        assert result["document"]["document_id"] == request["document_id"]
    else:
        with pytest.raises(WebError) as closed:
            await end_after_authorization(runtime, monkeypatch, request, data)
        assert closed.value.code == "PROJECT_ENDED"
    assert await business_rows(runtime) == before


async def test_analysis_submission_rolls_back_run_and_command_when_mapping_fails(runtime):
    await upload(runtime)
    before = await business_rows(runtime)
    async with runtime.bridge.store.transaction() as conn:
        await conn.execute("ALTER TABLE tender_project_analyses ADD CONSTRAINT fixture_mapping_rejection CHECK(source_only)")
    with pytest.raises(psycopg.errors.CheckViolation):
        await runtime.bridge.start_analysis(payload(runtime, "analysis.start", request_key="mapping-rejected"))
    assert await business_rows(runtime) == before


async def test_ended_document_confirmation_only_reads_matching_original_identity(runtime, monkeypatch):
    request, data = await upload(runtime)
    async with runtime.bridge.store.transaction() as conn:
        await conn.execute("UPDATE tender_projects SET status='ended' WHERE id=%s", (runtime.project,))
    monkeypatch.setattr(Settings, "from_env", classmethod(lambda cls: runtime.settings))
    def forbidden(*_args, **_kwargs):
        pytest.fail("existing document confirmation cannot write artifacts")
    monkeypatch.setattr(ArtifactStore, "put_bytes", forbidden)
    before = await business_rows(runtime)
    assert (await handle_command(request, data))[0]["document"]["document_id"] == request["document_id"]
    with pytest.raises(Conflict):
        await handle_command(request | {"role": "amendment"}, data)
    with pytest.raises(WebError) as changed:
        await handle_command(request, data + b"changed")
    assert changed.value.code == "DOCUMENT_IDENTITY_MISMATCH"
    assert await business_rows(runtime) == before


@pytest.mark.parametrize("action", ["analysis.confirm", "analysis.correct"])
async def test_existing_analysis_decision_replays_when_end_precedes_owner_recheck(review_runtime, monkeypatch, action):
    runtime = review_runtime
    _, review = await waiting(runtime, blocked=True)
    request = analysis_payload(runtime, action=action, **(confirmation(review) if action == "analysis.confirm" else correction(review)))
    monkeypatch.setattr(Settings, "from_env", classmethod(lambda cls: runtime.bridge.settings))
    first, _ = await handle_command(request)
    before = await business_rows(runtime)
    assert (await end_after_authorization(runtime, monkeypatch, request, pause_at=1))[0] == first
    assert await business_rows(runtime) == before


async def test_existing_analysis_retry_replays_when_end_precedes_owner_recheck(runtime, monkeypatch):
    run, command = await failure(runtime)
    request = payload(runtime, "analysis.retry", **retry_body(run, command))
    monkeypatch.setattr(Settings, "from_env", classmethod(lambda cls: runtime.settings))
    first, _ = await handle_command(request)
    before = await business_rows(runtime)
    assert (await end_after_authorization(runtime, monkeypatch, request, pause_at=1))[0] == first
    assert await business_rows(runtime) == before


async def test_old_confirmation_replays_before_current_revision_validation(review_runtime, monkeypatch):
    runtime = review_runtime
    _, review = await waiting(runtime, blocked=True)
    request = analysis_payload(runtime, action="analysis.confirm", **confirmation(review))
    monkeypatch.setattr(Settings, "from_env", classmethod(lambda cls: runtime.bridge.settings))
    first, _ = await handle_command(request)
    edit = analysis_payload(runtime, action="analysis.correct", **correction(review)) | {"request_key": "new-revision"}
    await handle_command(edit)
    before = await business_rows(runtime)
    receipt, _ = await end_after_authorization(runtime, monkeypatch, request, pause_at=1)
    assert receipt["command_id"] == first["command_id"] and receipt["analysis"]["domain_revision"] == 2
    assert await business_rows(runtime) == before
