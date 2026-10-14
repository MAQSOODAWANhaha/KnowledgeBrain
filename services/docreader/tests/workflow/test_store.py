"""Real PostgreSQL contracts. Each test owns an isolated disposable schema."""

import asyncio
import os
from uuid import uuid4

import psycopg
import pytest
import pytest_asyncio
from psycopg import sql
from psycopg.conninfo import make_conninfo

from docreader.integrations.artifacts import ArtifactStore
from docreader.integrations.store import Cancelled, Conflict, InvalidConfirmation, StaleExecution, Store


@pytest_asyncio.fixture
async def store(tmp_path):
    dsn = os.environ.get("TENDER_TEST_DSN")
    if not dsn:
        pytest.skip("TENDER_TEST_DSN is required for real PostgreSQL tests")
    schema = "tender_test_" + uuid4().hex
    async with await psycopg.AsyncConnection.connect(dsn, autocommit=True) as conn:
        await conn.execute(sql.SQL("CREATE SCHEMA {}").format(sql.Identifier(schema)))
    adapter = Store(make_conninfo(dsn, options=f"-c search_path={schema}"))
    await adapter.setup()
    adapter.test_artifacts = ArtifactStore(tmp_path)
    try:
        yield adapter
    finally:
        async with await psycopg.AsyncConnection.connect(dsn, autocommit=True) as conn:
            await conn.execute(sql.SQL("DROP SCHEMA {} CASCADE").format(sql.Identifier(schema)))


async def submitted(store):
    ref = store.test_artifacts.put_json({"documents": [], "test": str(uuid4())})
    run = await store.submit(ref, str(uuid4()))
    command = (await store.get_run(run["run_id"]))["commands"][0]
    return run, command


async def expire(store, run_id):
    async with store.transaction() as conn:
        await conn.execute(
            "UPDATE tender_runs SET lease_until = clock_timestamp() - interval '1 second' WHERE run_id = %s",
            (run_id,),
        )


@pytest.mark.asyncio
async def test_submission_replay_and_conflicting_idempotency_key(store):
    ref = store.test_artifacts.put_json({"documents": []})
    one, two = await asyncio.gather(store.submit(ref, "same"), store.submit(ref, "same"))
    assert one["run_id"] == two["run_id"]
    assert len((await store.get_run(one["run_id"]))["commands"]) == 1
    with pytest.raises(Conflict):
        await store.submit(store.test_artifacts.put_json({"documents": ["different"]}), "same")


@pytest.mark.asyncio
async def test_publish_failure_token_fencing_and_unexecuted_redelivery(store):
    _, command = await submitted(store)
    first = (await store.claim_commands())[0]
    assert await store.publish_failed(command["command_id"], first["claim_token"], "RedisUnavailable")
    second = (await store.claim_commands())[0]
    assert first["claim_token"] != second["claim_token"]
    assert not await store.mark_published(command["command_id"], first["claim_token"])
    assert await store.mark_published(command["command_id"], second["claim_token"])
    assert await store.mark_published(command["command_id"], second["claim_token"])
    assert not await store.claim_commands()
    async with store.transaction() as conn:
        await conn.execute("UPDATE tender_commands SET published_at = clock_timestamp() - interval '2 minutes'")
    third = (await store.claim_commands())[0]
    assert third["delivery_count"] == 3
    assert third["receipt"] is None
    assert not await store.mark_published(command["command_id"], second["claim_token"])


@pytest.mark.asyncio
async def test_duplicate_execution_one_owner_and_completion_before_publish_ack(store):
    run, command = await submitted(store)
    claim = (await store.claim_commands())[0]
    leases = await asyncio.gather(*(
        store.acquire(run["run_id"], command["command_id"], 1) for _ in range(4)
    ))
    assert sum(lease is not None for lease in leases) == 1
    lease = next(lease for lease in leases if lease is not None)
    await store.release(lease, "blocked", metadata={"blocking_issues": 1})
    assert await store.mark_published(command["command_id"], claim["claim_token"])
    assert await store.acquire(run["run_id"], command["command_id"], 1) is None
    assert not await store.claim_commands()


@pytest.mark.asyncio
async def test_expired_executor_cannot_commit_and_receipt_survives_checkpoint_loss(store):
    run, command = await submitted(store)
    lease = await store.acquire(run["run_id"], command["command_id"], 1)
    ref = store.test_artifacts.put_json({"stage": "frozen"})
    original = await store.commit_artifact(lease, "freeze", ref)
    await expire(store, run["run_id"])
    assert (await store.claim_commands())[0]["command_id"] == command["command_id"]
    replacement = await store.acquire(run["run_id"], command["command_id"], 1)
    assert replacement.epoch > lease.epoch
    for action in (
        store.commit_artifact(lease, "freeze", ref),
        store.heartbeat(lease),
        store.release(lease, "blocked"),
    ):
        with pytest.raises(StaleExecution):
            await action
    assert await store.receipt(replacement, "freeze") == original
    assert await store.commit_artifact(replacement, "freeze", ref) == original
    with pytest.raises(Conflict):
        await store.commit_artifact(replacement, "freeze", store.test_artifacts.put_json({"different": True}))


@pytest.mark.asyncio
async def test_confirm_binds_digest_is_idempotent_and_source_revision_invalidates(store):
    run, command = await submitted(store)
    run_id = run["run_id"]
    with pytest.raises(InvalidConfirmation):
        await store.confirm(run_id, 1, "checklist", "confirm")
    lease = await store.acquire(run_id, command["command_id"], 1)
    await store.release(lease, "waiting-confirmation", checklist_digest="checklist")
    with pytest.raises(InvalidConfirmation):
        await store.confirm(run_id, 1, "obsolete-checklist", "bad-confirm")
    first = await store.confirm(run_id, 1, "checklist", "confirm")
    assert first == await store.confirm(run_id, 1, "checklist", "confirm")
    assert first["payload"] == {"version": 1, "checklist_digest": "checklist"}
    second_ref = store.test_artifacts.put_json({"documents": ["revision"]})
    revised = await store.revise(run_id, second_ref, "revision-key")
    assert revised["current_version"] == 2
    assert revised["checklist_digest"] is None
    assert (await store.revise(run_id, second_ref, "revision-key"))["current_version"] == 2
    with pytest.raises(InvalidConfirmation):
        await store.confirm(run_id, 1, "checklist", "confirm")
    assert await store.acquire(run_id, first["command_id"], 1) is None
    assert await store.get_manifest(run_id, 1) == run["manifest_ref"]
    assert await store.get_manifest(run_id, 2) == second_ref


@pytest.mark.asyncio
async def test_cancel_is_immediate_and_sent_model_request_remains_uncertain(store):
    run, command = await submitted(store)
    lease = await store.acquire(run["run_id"], command["command_id"], 1)
    await store.begin_request(lease, "request", "model", "chat")
    await store.finish_request("request", "sent")
    await store.cancel(run["run_id"])
    with pytest.raises(Cancelled):
        await store.heartbeat(lease)
    assert await store.acquire(run["run_id"], command["command_id"], 1) is None
    assert not await store.claim_commands()
    status = await store.get_run(run["run_id"])
    assert status["status"] == "cancelled"
    assert status["usage"][0]["status"] == "uncertain"
    assert status["commands"][0]["receipt"] is None


@pytest.mark.asyncio
async def test_model_usage_and_recovery_preserve_actual_and_uncertain_records(store):
    run, command = await submitted(store)
    lease = await store.acquire(run["run_id"], command["command_id"], 1)
    await store.begin_request(lease, "sent", "model", "chat", {"api_key": "must-not-persist", "input_tokens": 4})
    await store.finish_request("sent", "sent")
    await store.begin_request(lease, "prepared", "model", "chat")
    await expire(store, run["run_id"])
    replacement = await store.acquire(run["run_id"], command["command_id"], 1)
    records = {row["request_id"]: row for row in (await store.get_run(run["run_id"]))["usage"]}
    assert records["sent"]["status"] == "uncertain"
    assert records["prepared"]["status"] == "failed"
    assert records["sent"]["metadata"] == {"input_tokens": 4}
    with pytest.raises(StaleExecution):
        await store.finish_request("sent", "sent")
    await store.begin_request(replacement, "new", "model", "chat")
    await store.finish_request("new", "sent")
    await store.finish_request("new", "succeeded", usage={"input_tokens": 4, "output_tokens": 2})
    latest = (await store.get_run(run["run_id"]))["usage"][-1]
    assert latest["usage"] == {"input_tokens": 4, "output_tokens": 2}


@pytest.mark.asyncio
async def test_infrastructure_failures_have_bounded_retry_and_terminal_receipt(store):
    run, command = await submitted(store)
    for attempt in range(4):
        lease = await store.acquire(run["run_id"], command["command_id"], 1)
        released = await store.release(lease, "retry", error="ConnectionError")
        assert released["status"] == ("failed" if attempt == 3 else "pending")
    result = await store.get_command(command["command_id"])
    assert result["infrastructure_failures"] == 4
    assert result["execution_count"] == 4
    assert result["receipt"]["status"] == "failed"
    assert await store.acquire(run["run_id"], command["command_id"], 1) is None
