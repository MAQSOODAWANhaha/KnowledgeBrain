"""Client cleanup and heartbeat shutdown precede successful outline publication."""

import asyncio
from types import SimpleNamespace
from unittest.mock import AsyncMock

import pytest

from docreader.integrations.authoring_store import AuthoringLease
from docreader.integrations.store import StaleExecution
from docreader.llm.budget import RequestBudget
from docreader.llm.config import ModelConfig, ModelEndpoint
from docreader.tender import authoring_execution


@pytest.fixture
async def execution(monkeypatch):
    fixture = SimpleNamespace(
        reference={"sha256": "a" * 64, "relative_path": "aa/" + "a" * 64,
                   "size": 2, "media_type": "application/json"},
        state={"status": "running", "result_ref": None}, releases=[], tasks=[], heart=None,
        organizing=asyncio.Event(), allow_organize=asyncio.Event(), closing=asyncio.Event(),
        allow_close=asyncio.Event(), closed=asyncio.Event(), publishing=asyncio.Event(),
        allow_publish=asyncio.Event(), heartbeat_started=asyncio.Event(), stopping=asyncio.Event(),
        allow_stop=asyncio.Event(), ticks=asyncio.Queue(), heartbeat_checks=asyncio.Queue(),
        close_error=None, heartbeat_error=None, block_stopping=False,
    )
    fixture.allow_publish.set()

    async def sleep(seconds):
        fixture.heart = asyncio.current_task()
        fixture.heartbeat_started.set()
        try:
            await fixture.ticks.get()
        except asyncio.CancelledError:
            fixture.stopping.set()
            if fixture.block_stopping:
                await fixture.allow_stop.wait()
            raise

    async def heartbeat(*args, **kwargs):
        fixture.heartbeat_checks.put_nowait(fixture.state["status"])
        if fixture.state["status"] != "running":
            raise StaleExecution("successful publication already ended the execution lease")
        if fixture.heartbeat_error:
            raise fixture.heartbeat_error

    async def release(lease, status, *, result_ref=None, error=None):
        if fixture.state["status"] == "succeeded":
            raise StaleExecution("successful execution is terminal")
        fixture.releases.append(status)
        fixture.state.update(status=status, result_ref=result_ref)
        if status == "succeeded":
            fixture.publishing.set()
            # Model an asynchronous connection cleanup after the DB commit.
            await fixture.allow_publish.wait()

    async def organize(*args, **kwargs):
        fixture.organizing.set()
        await fixture.allow_organize.wait()
        return fixture.reference

    class Client:
        def __init__(self, *args, **kwargs):
            pass

        async def __aenter__(self):
            return self

        async def __aexit__(self, exc_type, *args):
            fixture.closing.set()
            if exc_type is not asyncio.CancelledError:
                await fixture.allow_close.wait()
            if fixture.close_error:
                raise fixture.close_error
            fixture.closed.set()

    store = SimpleNamespace(setup=AsyncMock(), submit=AsyncMock(), get_execution=AsyncMock(return_value=None),
                            acquire=AsyncMock(return_value=AuthoringLease("outline-race", 1)),
                            heartbeat=heartbeat, release=release)
    monkeypatch.setattr(authoring_execution, "read_authoring_input", AsyncMock(return_value={}))
    monkeypatch.setattr(authoring_execution, "assert_authoring_active", AsyncMock())
    monkeypatch.setattr(authoring_execution, "AuthoringStore", lambda dsn: store)
    monkeypatch.setattr(authoring_execution, "OpenAIModel", Client)
    monkeypatch.setattr(authoring_execution, "organize_outline", organize)
    monkeypatch.setattr(authoring_execution.asyncio, "sleep", sleep)
    config = ModelConfig(ModelEndpoint("http://fixture.invalid/v1", "fixture-key", "fixture", 131072), None)
    budget = RequestBudget(context_tokens=131072, model="fixture", prompts={}, output_reserve=16384)

    def start():
        task = asyncio.create_task(authoring_execution.execute_outline(
            SimpleNamespace(dsn="fixture"), None, config, budget, execution_id="outline-race",
            run_id="analysis", source_version=1, domain_revision=1, checklist_digest="b" * 64,
        ))
        fixture.tasks.append(task)
        return task

    fixture.start = start
    yield fixture
    for event in (fixture.allow_organize, fixture.allow_close, fixture.allow_publish, fixture.allow_stop):
        event.set()
    tasks = [*fixture.tasks, *([fixture.heart] if fixture.heart else [])]
    for task in tasks:
        task.cancel()
    await asyncio.gather(*tasks, return_exceptions=True)


async def test_cleanup_renews_lease_and_publication_waits_for_heartbeat_shutdown(execution):
    task = execution.start()
    await asyncio.wait_for(execution.heartbeat_started.wait(), timeout=3)
    execution.allow_organize.set()
    await asyncio.wait_for(execution.closing.wait(), timeout=3)
    execution.ticks.put_nowait(None)
    assert await asyncio.wait_for(execution.heartbeat_checks.get(), timeout=3) == "running"
    assert execution.state == {"status": "running", "result_ref": None}

    execution.allow_publish.clear()
    execution.allow_close.set()
    await asyncio.wait_for(execution.publishing.wait(), timeout=3)
    assert execution.closed.is_set() and execution.heart.done()
    assert not task.done()
    execution.allow_publish.set()
    assert await asyncio.wait_for(task, timeout=3) == execution.reference
    assert execution.state == {"status": "succeeded", "result_ref": execution.reference}


async def test_client_close_failure_does_not_publish_success(execution):
    execution.close_error = RuntimeError("client close failed")
    execution.allow_organize.set()
    execution.allow_close.set()
    execution.allow_publish.set()
    with pytest.raises(RuntimeError, match="client close failed"):
        await asyncio.wait_for(execution.start(), timeout=3)
    assert execution.releases == ["failed"]
    assert execution.state == {"status": "failed", "result_ref": None}


@pytest.mark.parametrize("phase", ["organizing", "closing"])
async def test_heartbeat_failure_stops_work_without_publication(execution, phase):
    execution.heartbeat_error = StaleExecution("execution ownership lost")
    task = execution.start()
    await asyncio.wait_for(execution.heartbeat_started.wait(), timeout=3)
    await asyncio.wait_for(execution.organizing.wait(), timeout=3)
    if phase == "closing":
        execution.allow_organize.set()
        await asyncio.wait_for(execution.closing.wait(), timeout=3)
    execution.ticks.put_nowait(None)
    with pytest.raises(StaleExecution, match="execution ownership lost"):
        await asyncio.wait_for(task, timeout=3)
    assert execution.releases == ["failed"]
    assert execution.state == {"status": "failed", "result_ref": None}


async def test_caller_cancellation_during_heartbeat_shutdown_does_not_publish(execution):
    execution.block_stopping = True
    task = execution.start()
    await asyncio.wait_for(execution.heartbeat_started.wait(), timeout=3)
    execution.allow_organize.set()
    execution.allow_close.set()
    execution.allow_publish.set()
    await asyncio.wait_for(execution.stopping.wait(), timeout=3)
    task.cancel()
    with pytest.raises(asyncio.CancelledError):
        await asyncio.wait_for(task, timeout=3)
    assert execution.releases == ["failed"]
    assert execution.state == {"status": "failed", "result_ref": None}
