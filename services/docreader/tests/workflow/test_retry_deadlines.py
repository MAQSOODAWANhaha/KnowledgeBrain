"""Retry deadlines survive crashes in existing PostgreSQL request/artifact journals."""
import asyncio
import json
import time
from uuid import uuid4

import httpx
import pytest

from docreader.integrations.store import Cancelled, StaleExecution
from docreader.llm import recovery
from docreader.llm.budget import RequestBudget
from docreader.llm.request import ModelContractError, ModelRateLimitError, build_request
from test_authoring_store import authoring_store as authoring_store, metadata, submitted as authoring_submitted
from docreader.llm import ModelConfig, OpenAIModel
from test_store import store as store, submitted


async def setup(store):
    run, command = await submitted(store)
    lease = await store.acquire(run["run_id"], command["command_id"], 1, lease_seconds=300)
    return run, lease


def wire_model(store, lease, handler):
    config = ModelConfig.from_env({"KNOWLEDGEBRAIN_CHAT_BASE_URL": "https://fixture.invalid/v1",
        "KNOWLEDGEBRAIN_CHAT_API_KEY": "synthetic", "KNOWLEDGEBRAIN_CHAT_MODEL": "fixture",
        "KNOWLEDGEBRAIN_CHAT_MAX_CONTEXT_TOKENS": "131072"})
    return OpenAIModel(config, store, lease, prompts={"verify": "Return the requested JSON"},
                       artifacts=store.test_artifacts, transport=httpx.MockTransport(handler))


def sse(value):
    chunks = [{"choices": [{"index": 0, "delta": {"content": value}, "finish_reason": None}]},
              {"choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]}]
    return httpx.Response(200, content="".join("data: " + json.dumps(chunk) + "\n\n" for chunk in chunks) + "data: [DONE]\n\n",
                         headers={"content-type": "text/event-stream"})


pytestmark = pytest.mark.integration


async def test_analysis_failure_audit_alone_preserves_deadline_and_attempt_progress(store):
    run, lease = await setup(store)
    digest = "a" * 64
    for index, delay in enumerate([45, 60, 120]):
        request_id = uuid4().hex
        await store.begin_request(lease, request_id, "fixture", "verify",
                                  {"request_digest": digest, "logical_request_digest": digest})
        await store.finish_request(request_id, "sent")
        await store.finish_request(request_id, "failed", error={"type": "http_429", "retry_after_seconds": 45})
        row = next(row for row in (await store.get_run(run["run_id"]))["usage"] if row["request_id"] == request_id)
        deadline = row["metadata"]["not_before"]
        assert deadline - row["finished_at"].timestamp() == pytest.approx(delay, abs=.1)
        assert await store.request_not_before(lease, "verify", digest) == deadline
        assert await store.request_not_before(lease, "verify", "b" * 64) is None
        # Repeated terminal writes and fresh Store instances cannot move the deadline.
        await store.finish_request(request_id, "failed", error={"type": "http_429", "retry_after_seconds": 600})
        restarted = type(store)(store.dsn)
        assert await restarted.request_not_before(lease, "verify", digest) == deadline


async def test_authoring_failure_audit_alone_preserves_deadline_and_attempt_progress(authoring_store):
    store = authoring_store
    identity, lease = await authoring_submitted(store)
    fields = metadata()
    for delay in [45, 60, 120]:
        request_id = uuid4().hex
        await store.begin_request(lease, request_id, "fixture", "outline", fields)
        await store.finish_request(request_id, "sent")
        await store.finish_request(request_id, "failed", error={"type": "http_429", "retry_after_seconds": 45})
        async with store.transaction() as conn:
            row = await (await conn.execute("SELECT * FROM tender_authoring_model_requests WHERE request_id = %s",
                                            (request_id,))).fetchone()
        deadline = row["metadata"]["not_before"]
        assert deadline - row["finished_at"].timestamp() == pytest.approx(delay, abs=.1)
        restarted = type(store)(store.dsn)
        assert await restarted.request_not_before(lease, "outline", fields["logical_request_digest"]) == deadline
        assert await restarted.request_not_before(lease, "outline", "b" * 64) is None
        await store.finish_request(request_id, "failed", error={"type": "http_429", "retry_after_seconds": 600})
        assert await restarted.request_not_before(lease, "outline", fields["logical_request_digest"]) == deadline


@pytest.mark.parametrize("owner", ["analysis", "authoring"])
@pytest.mark.parametrize("stop", [None, "cancel", "stale"])
async def test_ordinary_http_retry_after_crash_waits_then_rechecks_execution_fence(
        store, authoring_store, monkeypatch, owner, stop):
    if owner == "analysis":
        run, lease = await setup(store)
        identity = run["run_id"]
    else:
        store = authoring_store
        identity, lease = await authoring_submitted(store)
    now, waits, sent = [time.time()], [], []
    monkeypatch.setattr(time, "time", lambda: now[0])

    def handler(request):
        sent.append(request)
        return (httpx.Response(429, headers={"retry-after": "45"}, json={"error": "busy"})
                if len(sent) == 1 else sse(json.dumps({"findings": []})))

    async def call(model):
        if owner == "analysis":
            return await model.call("verify", {})
        return await model.call_request(build_request("fixture", "Return JSON", {}, task="outline"),
            RequestBudget(model="fixture"), task="outline", execution_spec={"protocol": "outline-v1", "model": "fixture"})

    async with wire_model(store, lease, handler) as model:
        with pytest.raises(ModelRateLimitError):
            await call(model)
    now[0] += 10  # Worker died after failure audit, before entering recovery.sleep.

    async def wait(seconds):
        waits.append(seconds)
        await store.heartbeat(lease, lease_seconds=300)
        if stop == "cancel":
            await store.cancel(identity)
        elif stop == "stale":
            async with store.transaction() as conn:
                table, column = ("tender_analysis_runs", "run_id") if owner == "analysis" else (
                    "tender_authoring_executions", "execution_id")
                await conn.execute(f"UPDATE {table} SET epoch = epoch + 1 WHERE {column} = %s", (identity,))
        now[0] += seconds

    monkeypatch.setattr(recovery.asyncio, "sleep", wait)
    async with wire_model(store, lease, handler) as model:
        if stop:
            with pytest.raises(Cancelled if stop == "cancel" else StaleExecution):
                await call(model)
        else:
            assert await call(model) == {"findings": []}
    assert waits == [35]
    assert len(sent) == (1 if stop else 2)


@pytest.mark.parametrize("owner", ["analysis", "authoring"])
async def test_legacy_failed_audit_derives_once_from_failure_timestamp(store, authoring_store, owner):
    if owner == "analysis":
        _, lease = await setup(store)
        table, task = "tender_analysis_model_requests", "verify"
    else:
        store = authoring_store
        _, lease = await authoring_submitted(store)
        table, task = "tender_authoring_model_requests", "outline"
    fields, request_id = metadata(), uuid4().hex
    await store.begin_request(lease, request_id, "fixture", task, fields)
    await store.finish_request(request_id, "sent")
    await store.finish_request(request_id, "failed", error={"type": "http_429"})
    async with store.transaction() as conn:
        row = await (await conn.execute(f"UPDATE {table} SET metadata = metadata - 'not_before' "
            "WHERE request_id = %s RETURNING *", (request_id,))).fetchone()
    expected = row["finished_at"].timestamp() + 30
    assert await store.request_not_before(lease, task, fields["logical_request_digest"]) == expected
    assert await store.request_not_before(lease, task, fields["logical_request_digest"]) == expected
    async with store.transaction() as conn:
        row = await (await conn.execute(f"SELECT metadata FROM {table} WHERE request_id = %s", (request_id,))).fetchone()
    assert row["metadata"]["retry_deadline_origin"] == "legacy_failure_time"


@pytest.mark.parametrize("stop", [None, "cancel", "stale"])
async def test_semantic_receipt_crash_waits_with_fence_and_accepted_cache_never_resends(store, monkeypatch, stop):
    run, lease = await setup(store)
    now, waits, sent = [time.time()], [], []
    monkeypatch.setattr(time, "time", lambda: now[0])

    def handler(request):
        sent.append(request)
        return sse(json.dumps({"findings": None if len(sent) == 1 else []}))

    def validate(value):
        if value["findings"] is None:
            raise ModelContractError([{"path": ["findings"], "code": "array", "detail": "Return an array"}])
        return value

    original, crashed = store.commit_artifact, False

    async def lost_acknowledgement(*args, **kwargs):
        nonlocal crashed
        result = await original(*args, **kwargs)
        if args[1] == "model-rejection" and not crashed:
            crashed = True
            raise OSError("synthetic crash after rejection receipt commit")
        return result

    monkeypatch.setattr(store, "commit_artifact", lost_acknowledgement)
    async with wire_model(store, lease, handler) as model:
        with pytest.raises(OSError):
            await recovery.call_validated(model, store, store.test_artifacts, lease, "verify", {}, validate)
    now[0] += .25

    async def wait(seconds):
        waits.append(seconds)
        await store.heartbeat(lease, lease_seconds=300)
        if stop == "cancel":
            await store.cancel(run["run_id"])
        elif stop == "stale":
            async with store.transaction() as conn:
                await conn.execute("UPDATE tender_analysis_runs SET epoch = epoch + 1 WHERE run_id = %s",
                                   (run["run_id"],))
        now[0] += seconds

    monkeypatch.setattr(asyncio, "sleep", wait)
    async with wire_model(store, lease, handler) as model:
        if stop:
            with pytest.raises(Cancelled if stop == "cancel" else StaleExecution):
                await recovery.call_validated(model, store, store.test_artifacts, lease, "verify", {}, validate)
        else:
            for _ in range(2):
                assert await recovery.call_validated(model, store, store.test_artifacts, lease, "verify", {}, validate) == {
                    "findings": []}
    assert waits == [.75] and len(sent) == (1 if stop else 2)


