"""Real PostgreSQL fences around logical requests, including worker loss."""

import asyncio

import pytest

from docreader.integrations.store import RequestReplayBlocked, StaleExecution
from test_store import expire, request_metadata, store as store, submitted

pytestmark = pytest.mark.asyncio


async def leased(store):
    run, command = await submitted(store)
    lease = await store.acquire(run["run_id"], command["command_id"], 1)
    return run, command, lease


async def test_request_digest_is_mandatory_and_canonical(store):
    run, _, lease = await leased(store)
    for digest in (None, "", "x" * 64, "A" * 64, "1" * 63, 1):
        with pytest.raises(ValueError, match="request_digest"):
            await store.begin_request(lease, "invalid", "fixture", "extract", {"request_digest": digest})
    assert not (await store.get_run(run["run_id"]))["usage"]


async def test_sent_and_worker_lost_uncertain_requests_block_new_request_ids(store):
    run, command, old = await leased(store)
    metadata = request_metadata("same complete request")
    await store.begin_request(old, "old-request", "fixture", "extract", metadata)
    await store.finish_request("old-request", "sent")
    with pytest.raises(RequestReplayBlocked) as sent:
        await store.begin_request(old, "duplicate-request", "fixture", "extract", metadata)
    assert sent.value.request_id == "old-request" and sent.value.status == "sent"
    assert not isinstance(sent.value, ConnectionError)
    await expire(store, run["run_id"])
    new = await store.acquire(run["run_id"], command["command_id"], 1)
    partial = store.test_artifacts.put_json({"synthetic_stream": ["partial body"], "complete": False})
    await store.finish_request("old-request", "uncertain", response_ref=partial)
    with pytest.raises(RequestReplayBlocked) as uncertain:
        await store.begin_request(new, "new-executor-request", "fixture", "extract", metadata)
    assert uncertain.value.status == "uncertain"
    records = (await store.get_run(run["run_id"]))["usage"]
    assert len(records) == 1 and records[0]["status"] == "uncertain"
    assert records[0]["response_ref"] == partial  # A received partial stream is audit evidence, not a reusable success.
    assert records[0]["usage"] is None


async def test_two_prepared_requests_cannot_both_cross_the_send_fence(store):
    run, _, lease = await leased(store)
    metadata = request_metadata("parallel identical complete request")
    rows = await asyncio.gather(*(
        store.begin_request(lease, request_id, "fixture", "verify", metadata)
        for request_id in ("parallel-a", "parallel-b")
    ))
    assert all(row["status"] == "prepared" for row in rows)
    sent = []

    async def send(request_id):
        await store.finish_request(request_id, "sent")
        sent.append(request_id)  # The provider call is permitted only after this fence.

    results = await asyncio.gather(*(send(row["request_id"]) for row in rows), return_exceptions=True)
    assert len(sent) == 1
    assert sum(isinstance(result, RequestReplayBlocked) for result in results) == 1
    records = (await store.get_run(run["run_id"]))["usage"]
    assert sorted(row["status"] for row in records) == ["prepared", "sent"]
    with pytest.raises(RequestReplayBlocked):
        await store.finish_request(sent[0], "sent")  # Repeating the same request_id is not permission to send again.


async def test_abandoned_prepared_and_explicit_failed_requests_can_retry(store):
    run, command, old = await leased(store)
    metadata = request_metadata("not yet sent")
    await store.begin_request(old, "prepared-before-crash", "fixture", "extract", metadata)
    await expire(store, run["run_id"])
    new = await store.acquire(run["run_id"], command["command_id"], 1)
    with pytest.raises(StaleExecution):
        await store.finish_request("prepared-before-crash", "sent")
    retry = await store.begin_request(new, "safe-retry", "fixture", "extract", metadata)
    assert retry["status"] == "prepared"
    await store.finish_request("safe-retry", "sent")
    await store.finish_request("safe-retry", "failed", error={"type": "http_429"})
    next_try = await store.begin_request(new, "rejected-retry", "fixture", "extract", metadata)
    await store.finish_request(next_try["request_id"], "sent")
    records = (await store.get_run(run["run_id"]))["usage"]
    assert [row["status"] for row in records] == ["failed", "failed", "sent"]
    assert records[0]["sent_at"] is None


async def test_complete_response_survives_worker_loss_before_domain_receipt(store):
    run, command, old = await leased(store)
    metadata = request_metadata("successful provider response")
    response_ref = store.test_artifacts.put_json({"synthetic_stream": ["complete JSON", "normal finish", "DONE"]})
    await store.begin_request(old, "completed-provider-call", "fixture", "extract", metadata)
    await store.finish_request("completed-provider-call", "sent")
    await store.finish_request("completed-provider-call", "succeeded", {"input_tokens": 4}, response_ref=response_ref)
    # No domain artifact or checkpoint has been committed when the process dies.
    await expire(store, run["run_id"])
    new = await store.acquire(run["run_id"], command["command_id"], 1)
    reused = await store.begin_request(new, "would-be-paid-duplicate", "fixture", "extract", metadata)
    assert reused["request_id"] == "completed-provider-call"
    assert reused["status"] == "succeeded" and reused["response_ref"] == response_ref
    assert reused["usage"] == {"input_tokens": 4}
    assert store.test_artifacts.read_json(reused["response_ref"])["synthetic_stream"][-1] == "DONE"
    row = await store.get_run(run["run_id"])
    assert len(row["usage"]) == 1 and not row["artifacts"]
    assert await store.receipt(new, "extract") is None


async def test_success_without_response_and_send_race_after_success_never_repay(store):
    _, _, lease = await leased(store)
    metadata = request_metadata("success audit before blob reference")
    await store.begin_request(lease, "success-no-ref", "fixture", "extract", metadata)
    await store.begin_request(lease, "prepared-duplicate", "fixture", "extract", metadata)
    await store.finish_request("success-no-ref", "sent")
    await store.finish_request("success-no-ref", "succeeded")
    with pytest.raises(RequestReplayBlocked) as blocked:
        await store.begin_request(lease, "new-request", "fixture", "extract", metadata)
    assert blocked.value.status == "succeeded"
    with pytest.raises(RequestReplayBlocked):
        await store.finish_request("prepared-duplicate", "sent")
    response_ref = store.test_artifacts.put_json({"complete": True})
    await store.finish_request("success-no-ref", "succeeded", response_ref=response_ref)
    cached = await store.begin_request(lease, "recovered-request", "fixture", "extract", metadata)
    assert cached["request_id"] == "success-no-ref" and cached["response_ref"] == response_ref


async def test_replay_identity_is_scoped_to_task_revision_and_full_request_digest(store):
    run, _, lease = await leased(store)
    metadata = request_metadata("same request hash")
    await store.begin_request(lease, "uncertain-extract", "fixture", "extract", metadata)
    await store.finish_request("uncertain-extract", "sent")
    await store.finish_request("uncertain-extract", "uncertain")
    assert (await store.begin_request(lease, "other-task", "fixture", "verify", metadata))["status"] == "prepared"
    assert (await store.begin_request(lease, "changed-request", "fixture", "extract",
                                      request_metadata("different complete request")))["status"] == "prepared"
    baseline = store.test_artifacts.put_json({"packs": []})
    await store.commit_artifact(lease, "freeze", baseline)
    await store.commit_artifact(lease, "extract", baseline)
    correction = store.test_artifacts.put_json({"packs": [], "correction": "explicit operator change"})
    await store.correct(run["run_id"], 1, 1, baseline["sha256"], correction, "explicit-correction", "Operator corrected requirements")
    row = await store.get_run(run["run_id"])
    newer = await store.acquire(run["run_id"], row["commands"][0]["command_id"], 1)
    assert (await store.begin_request(newer, "explicit-new-revision", "fixture", "extract", metadata))["domain_revision"] == 2
