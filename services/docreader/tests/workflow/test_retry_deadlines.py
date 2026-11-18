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
from docreader.llm.request import ModelConfigurationError, ModelContractError, ModelRateLimitError, ModelWorkUnitExhausted, build_request
from test_authoring_store import authoring_store as authoring_store, metadata, submitted as authoring_submitted
from test_group_recovery import (Model, payload, reply, run_groups, setup, sse, wire_model)
from test_store import store as store


pytestmark = pytest.mark.integration


@pytest.mark.parametrize("delay_before_resume", [10, 50])
async def test_group_reserved_retry_waits_only_until_original_deadline(store, monkeypatch, delay_before_resume):
    _, lease = await setup(store)
    now, waits = [time.time()], []
    monkeypatch.setattr(time, "time", lambda: now[0])
    model = Model([ModelRateLimitError(45), reply("associate-a", "associate-b")])

    async def crash_in_wait(seconds):
        assert seconds == pytest.approx(45)
        now[0] += delay_before_resume
        raise asyncio.CancelledError()

    monkeypatch.setattr(recovery.asyncio, "sleep", crash_in_wait)
    with pytest.raises(asyncio.CancelledError):
        await run_groups(store, lease, model)

    async def wait(seconds):
        waits.append(seconds)
        now[0] += seconds

    monkeypatch.setattr(recovery.asyncio, "sleep", wait)
    assert len((await run_groups(store, lease, model))["work_units"]) == 2
    assert waits == ([35] if delay_before_resume == 10 else [])
    assert len(model.calls) == 2


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


@pytest.mark.parametrize("failure,delay,elapsed", [("429", 45, 10), ("invalid_json", 1, .25)])
async def test_group_http_failure_before_response_journal_resumes_original_deadline(
        store, monkeypatch, failure, delay, elapsed):
    run, lease = await setup(store)
    now, waits, sent = [time.time()], [], []
    monkeypatch.setattr(time, "time", lambda: now[0])

    def handler(request):
        sent.append(request)
        if len(sent) == 1:
            if failure == "invalid_json":
                return sse('{"work_units": [')
            return httpx.Response(429, headers={"retry-after": "45"}, json={"error": "busy"})
        value = json.loads(json.loads(request.content)["messages"][1]["content"][0]["text"])
        return sse(json.dumps(reply(*value["target_work_unit_ids"])))

    original = store.commit_artifact
    crashed = False

    async def crash_after_audit(*args, **kwargs):
        nonlocal crashed
        if args[1] == "model-group-response" and not crashed:
            crashed = True
            raise OSError("synthetic crash after failure audit")
        return await original(*args, **kwargs)

    monkeypatch.setattr(store, "commit_artifact", crash_after_audit)
    async with wire_model(store, lease, handler) as model:
        with pytest.raises(OSError):
            await run_groups(store, lease, model)
    now[0] += elapsed

    async def wait(seconds):
        waits.append(seconds)
        # This update would block if recovery held the run transaction lock.
        await store.heartbeat(lease, lease_seconds=300)
        now[0] += seconds

    monkeypatch.setattr(recovery.asyncio, "sleep", wait)
    async with wire_model(store, lease, handler) as model:
        assert len((await run_groups(store, lease, model))["work_units"]) == 2
    assert waits == [delay - elapsed] and len(sent) == 2
    row = await store.get_run(run["run_id"])
    assert [request["status"] for request in row["usage"]] == ["failed", "succeeded"]
    assert len([entry for entry in row["artifacts"] if entry["stage"] == "model-group-attempt"]) == 2


@pytest.mark.parametrize("stop", ["cancel", "stale"])
async def test_group_execution_stopped_during_deadline_wait_never_calls_model_again(store, monkeypatch, stop):
    run, lease = await setup(store)
    model = Model([ModelRateLimitError(45), reply("associate-a", "associate-b")])

    async def stopped(seconds):
        if stop == "cancel":
            await store.cancel(run["run_id"])
        else:
            async with store.transaction() as conn:
                await conn.execute("UPDATE tender_analysis_runs SET epoch = epoch + 1 WHERE run_id = %s",
                                   (run["run_id"],))

    monkeypatch.setattr(recovery.asyncio, "sleep", stopped)
    with pytest.raises(Cancelled if stop == "cancel" else StaleExecution):
        await run_groups(store, lease, model)
    assert len(model.calls) == 1


async def test_group_deadline_restart_preserves_successful_sibling_and_attempt_ordinal(store, monkeypatch):
    _, lease = await setup(store)
    now, waits = [time.time()], []
    monkeypatch.setattr(time, "time", lambda: now[0])
    model = Model([reply("associate-a", "associate-b", bad=["associate-b"]),
                   ModelRateLimitError(45), reply("associate-b")])

    async def interrupted(seconds):
        waits.append(seconds)
        if len(waits) == 2:
            now[0] += 10
            raise asyncio.CancelledError()
        now[0] += seconds

    monkeypatch.setattr(recovery.asyncio, "sleep", interrupted)
    with pytest.raises(asyncio.CancelledError):
        await run_groups(store, lease, model)
    assert waits == [1, 60]

    async def wait(seconds):
        waits.append(seconds)
        now[0] += seconds

    monkeypatch.setattr(recovery.asyncio, "sleep", wait)
    assert len((await run_groups(store, lease, model))["work_units"]) == 2
    assert waits == [1, 60, 50]
    assert [call["target_work_unit_ids"] for call in model.calls] == [
        ["associate-a", "associate-b"], ["associate-b"], ["associate-b"]]


async def test_capacity_deadlines_do_not_grant_additional_group_attempts_on_restart(store, monkeypatch):
    run, lease = await setup(store)
    now = [time.time()]
    monkeypatch.setattr(time, "time", lambda: now[0])
    model = Model([ModelRateLimitError()] * 4)

    async def crash_after_wait(seconds):
        now[0] += seconds
        raise asyncio.CancelledError()

    monkeypatch.setattr(recovery.asyncio, "sleep", crash_after_wait)
    for _ in range(3):
        with pytest.raises(asyncio.CancelledError):
            await run_groups(store, lease, model)
    for _ in range(2):
        with pytest.raises(ModelWorkUnitExhausted):
            await run_groups(store, lease, model)
    assert len(model.calls) == 4
    row = await store.get_run(run["run_id"])
    assert len([entry for entry in row["artifacts"] if entry["stage"] == "model-group-attempt"]) == 4


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


async def test_group_preflight_failure_cannot_inherit_an_older_physical_deadline(store, monkeypatch):
    _, lease = await setup(store)
    now, waits, sent = [time.time()], [], []
    monkeypatch.setattr(time, "time", lambda: now[0])

    def handler(request):
        sent.append(request)
        if len(sent) == 1:
            return httpx.Response(429, headers={"retry-after": "45"}, json={"error": "busy"})
        value = json.loads(json.loads(request.content)["messages"][1]["content"][0]["text"])
        return sse(json.dumps(reply(*value["target_work_unit_ids"])))

    async def wait(seconds):
        waits.append(seconds)
        now[0] += seconds

    monkeypatch.setattr(asyncio, "sleep", wait)
    async with wire_model(store, lease, handler) as model:
        original, calls = model.call, 0

        async def call(*args, **kwargs):
            nonlocal calls
            calls += 1
            if calls == 2:
                raise ModelConfigurationError("synthetic preflight failure")
            return await original(*args, **kwargs)

        monkeypatch.setattr(model, "call", call)
        assert len((await run_groups(store, lease, model))["work_units"]) == 2
    assert waits == [45, 2] and len(sent) == 2 and calls == 3
