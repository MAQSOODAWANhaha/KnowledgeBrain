"""Expired executors consume the same bounded budget as explicit failures."""

import asyncio

import pytest

from docreader.integrations.store import StaleExecution
from test_store import expire, request_metadata, store as store, submitted

pytestmark = pytest.mark.asyncio


@pytest.mark.parametrize("delivery_status", ["running", "claimed", "published"])
async def test_four_expired_executors_finish_without_a_fifth_execution(store, delivery_status):
    run, command = await submitted(store)
    run_id, command_id = run["run_id"], command["command_id"]
    lease = await store.acquire(run_id, command_id, 1)
    ref = store.test_artifacts.put_json({"synthetic": "accepted pack"})
    receipt = await store.commit_artifact(lease, "extract-pack", ref, scope="accepted", input_digest="input")
    await store.begin_request(lease, "successful", "fixture", "extract", request_metadata("successful"))
    await store.finish_request("successful", "sent")
    await store.finish_request("successful", "succeeded", {"input_tokens": 8}, response_ref=ref)
    successful = (await store.get_run(run_id))["usage"][0]

    for attempt in range(1, 5):
        if attempt == 4:
            await store.begin_request(lease, "prepared", "fixture", "extract", request_metadata("prepared"))
            await store.begin_request(lease, "sent", "fixture", "extract", request_metadata("sent"))
            await store.finish_request("sent", "sent")
        await expire(store, run_id)
        if delivery_status != "running":
            claimed = (await store.claim_commands())[0]
            if delivery_status == "published":
                assert await store.mark_published(command_id, claimed["claim_token"])
        replacement = await store.acquire(run_id, command_id, 1)
        current = await store.get_command(command_id)
        assert current["infrastructure_failures"] == attempt
        if attempt < 4:
            assert replacement is not None
            assert current["execution_count"] == attempt + 1
            lease = replacement
        else:
            assert replacement is None

    finished = await store.get_run(run_id)
    assert finished["status"] == "failed"
    assert finished["lease_until"] is finished["lease_command_id"] is None
    assert current["status"] == "completed" and current["execution_count"] == 4
    assert current["receipt"]["status"] == "failed"
    assert current["receipt"]["metadata"]["reason"] == "infrastructure_retries_exhausted"
    assert current["last_error"] == {"type": "executor_lost"}
    assert current["completed_at"] is not None
    assert finished["artifacts"] == [receipt]
    requests = {item["request_id"]: item for item in finished["usage"]}
    assert requests["successful"] == successful
    assert requests["prepared"]["status"] == "failed"
    assert requests["sent"]["status"] == "uncertain"
    for identity in ("prepared", "sent"):
        assert requests[identity]["error"] == {"type": "executor_lost"}
        assert requests[identity]["finished_at"] is not None
    assert not await store.claim_commands()
    assert await store.acquire(run_id, command_id, 1) is None
    assert await store.get_run(run_id) == finished
    with pytest.raises(StaleExecution):
        await store.heartbeat(lease)


async def test_concurrent_redeliveries_count_one_expired_predecessor_once(store):
    run, command = await submitted(store)
    old = await store.acquire(run["run_id"], command["command_id"], 1)
    await expire(store, run["run_id"])
    await store.claim_commands()
    results = await asyncio.gather(*(store.acquire(run["run_id"], command["command_id"], 1) for _ in range(8)))
    replacements = [lease for lease in results if lease is not None]
    assert len(replacements) == 1
    current = await store.get_command(command["command_id"])
    assert current["infrastructure_failures"] == 1
    assert current["execution_count"] == 2
    assert replacements[0].epoch == old.epoch + 1
    with pytest.raises(StaleExecution):
        await store.heartbeat(old)


async def test_publication_execution_and_expiry_share_four_failures(store):
    run, command = await submitted(store)
    run_id, command_id = run["run_id"], command["command_id"]
    for _ in range(2):
        claimed = (await store.claim_commands())[0]
        await store.publish_failed(command_id, claimed["claim_token"], "publish failed")
    lease = await store.acquire(run_id, command_id, 1)
    assert lease is not None
    assert (await store.get_command(command_id))["infrastructure_failures"] == 2
    await expire(store, run_id)
    replacement = await store.acquire(run_id, command_id, 1)
    assert replacement is not None
    assert (await store.get_command(command_id))["infrastructure_failures"] == 3
    assert (await store.release(replacement, "retry"))["status"] == "failed"
    finished = await store.get_command(command_id)
    assert finished["execution_count"] == 2 and finished["infrastructure_failures"] == 4
    assert await store.acquire(run_id, command_id, 1) is None


async def test_released_execution_is_not_counted_again_as_expired(store):
    run, command = await submitted(store)
    lease = await store.acquire(run["run_id"], command["command_id"], 1)
    await store.release(lease, "retry")
    await store.claim_commands()
    replacement = await store.acquire(run["run_id"], command["command_id"], 1)
    assert replacement is not None
    current = await store.get_command(command["command_id"])
    assert current["infrastructure_failures"] == 1 and current["execution_count"] == 2


async def test_old_unaccounted_executor_losses_do_not_start_an_eighth_execution(store):
    run, command = await submitted(store)
    lease = await store.acquire(run["run_id"], command["command_id"], 1)
    async with store.transaction() as conn:
        await conn.execute("UPDATE tender_analysis_commands SET execution_count = 7 WHERE command_id = %s", (command["command_id"],))
    await expire(store, run["run_id"])
    await store.claim_commands()
    assert await store.acquire(run["run_id"], command["command_id"], 1) is None
    finished = await store.get_run(run["run_id"])
    assert finished["status"] == "failed"
    assert finished["epoch"] == lease.epoch
    assert finished["commands"][0]["execution_count"] == 7
    assert finished["commands"][0]["infrastructure_failures"] >= 4


async def test_explicit_retry_after_expiry_exhaustion_has_a_fresh_command_budget(store):
    run, command = await submitted(store)
    lease = await store.acquire(run["run_id"], command["command_id"], 1)
    ref = store.test_artifacts.put_json({"synthetic": "accepted pack"})
    receipt = await store.commit_artifact(lease, "extract-pack", ref, scope="accepted", input_digest="input")
    for _ in range(4):
        await expire(store, run["run_id"])
        lease = await store.acquire(run["run_id"], command["command_id"], 1)
    assert lease is None
    failed = await store.get_command(command["command_id"])
    retried = await store.retry(run["run_id"], 1, 1, command["command_id"], "explicit-retry", "Operator authorized continuation")
    replacement = await store.acquire(run["run_id"], retried["command_id"], 1)
    assert replacement is not None
    current = await store.get_command(retried["command_id"])
    assert current["execution_count"] == 1 and current["infrastructure_failures"] == 0
    assert await store.get_command(command["command_id"]) == failed
    assert await store.receipt(replacement, "extract-pack", "accepted", input_digest="input") == receipt
