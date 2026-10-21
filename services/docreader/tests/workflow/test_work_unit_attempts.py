"""Persisted work-unit attempts share one budget across response corrections."""

import asyncio

import pytest

from docreader.integrations.store import RequestReplayBlocked
from docreader.jobs.settings import Settings
from docreader.workflow.runner import execute_command
from test_request_replay import leased
from test_store import expire, request_metadata, store as store, submitted


pytestmark = pytest.mark.asyncio


def metadata(identity, logical="unit", **fields):
    return request_metadata(identity, logical_request_digest=request_metadata(logical)["request_digest"], **fields)


async def failed_attempt(store, lease, identity, logical="unit"):
    await store.begin_request(lease, identity, "fixture", "verify", metadata(identity, logical))
    await store.finish_request(identity, "sent")
    await store.finish_request(identity, "failed", error={"type": "invalid_json_response"})


async def test_command_identity_is_host_owned_and_logical_digest_is_canonical(store):
    _, command, lease = await leased(store)
    row = await store.begin_request(lease, "identity", "fixture", "verify",
                                    metadata("identity", command_id="untrusted-command"))
    assert row["metadata"]["command_id"] == command["command_id"]
    for digest in (None, "", "A" * 64, "1" * 63, 1):
        with pytest.raises(ValueError, match="logical_request_digest"):
            await store.begin_request(lease, "invalid", "fixture", "verify",
                                      request_metadata("invalid", logical_request_digest=digest))


async def test_corrections_share_four_sent_attempts_and_a_fifth_cannot_send(store):
    from docreader.llm.request import ModelWorkUnitExhausted

    run, command, lease = await leased(store)
    for attempt in range(4):
        await failed_attempt(store, lease, f"correction-{attempt}")
    await store.begin_request(lease, "fifth", "fixture", "verify", metadata("fifth"))
    with pytest.raises(ModelWorkUnitExhausted):
        await store.finish_request("fifth", "sent")
    current = await store.get_run(run["run_id"])
    assert len([row for row in current["usage"] if row["sent_at"]]) == 4
    assert current["usage"][-1]["status"] == "prepared"
    assert current["usage"][-1]["sent_at"] is None
    unchanged = await store.get_command(command["command_id"])
    assert unchanged["infrastructure_failures"] == 0
    assert unchanged["execution_count"] == 1


async def test_separate_work_units_have_independent_budgets(store):
    run, _, lease = await leased(store)
    for logical in ("unit-a", "unit-b"):
        for attempt in range(4):
            await failed_attempt(store, lease, f"{logical}-{attempt}", logical)
    assert len([row for row in (await store.get_run(run["run_id"]))["usage"] if row["sent_at"]]) == 8


async def test_attempts_survive_executor_loss_and_unsent_preparations_do_not_count(store):
    from docreader.llm.request import ModelWorkUnitExhausted

    run, command, lease = await leased(store)
    for attempt in range(2):
        await failed_attempt(store, lease, f"before-restart-{attempt}")
    await store.begin_request(lease, "abandoned-preparation", "fixture", "verify", metadata("abandoned"))
    await expire(store, run["run_id"])
    replacement = await store.acquire(run["run_id"], command["command_id"], 1)
    for attempt in range(2):
        await failed_attempt(store, replacement, f"after-restart-{attempt}")
    await store.begin_request(replacement, "fifth", "fixture", "verify", metadata("fifth"))
    with pytest.raises(ModelWorkUnitExhausted):
        await store.finish_request("fifth", "sent")
    current = await store.get_run(run["run_id"])
    abandoned = next(row for row in current["usage"] if row["request_id"] == "abandoned-preparation")
    assert abandoned["status"] == "failed" and abandoned["sent_at"] is None
    assert len([row for row in current["usage"] if row["sent_at"]]) == 4
    assert (await store.get_command(command["command_id"]))["infrastructure_failures"] == 1


async def test_success_cache_remains_reusable_after_four_sent_attempts(store):
    from docreader.llm.request import ModelWorkUnitExhausted

    run, _, lease = await leased(store)
    for attempt in range(3):
        await failed_attempt(store, lease, f"failed-{attempt}")
    await store.begin_request(lease, "accepted", "fixture", "verify", metadata("accepted"))
    await store.finish_request("accepted", "sent")
    ref = store.test_artifacts.put_json({"findings": []})
    await store.finish_request("accepted", "succeeded", response_ref=ref)
    reused = await store.begin_request(lease, "cache-read", "fixture", "verify", metadata("accepted"))
    assert reused["request_id"] == "accepted" and reused["status"] == "succeeded"
    assert len((await store.get_run(run["run_id"]))["usage"]) == 4
    await store.begin_request(lease, "new-feedback", "fixture", "verify", metadata("new-feedback"))
    with pytest.raises(ModelWorkUnitExhausted):
        await store.finish_request("new-feedback", "sent")


async def test_complete_but_host_rejected_response_consumes_an_attempt(store):
    from docreader.llm.request import ModelWorkUnitExhausted

    _, _, lease = await leased(store)
    for attempt in range(4):
        identity = f"host-rejected-{attempt}"
        await store.begin_request(lease, identity, "fixture", "verify", metadata(identity))
        await store.finish_request(identity, "sent")
        await store.finish_request(identity, "succeeded", response_ref=store.test_artifacts.put_json({"findings": None}))
    await store.begin_request(lease, "fifth-correction", "fixture", "verify", metadata("fifth-correction"))
    with pytest.raises(ModelWorkUnitExhausted):
        await store.finish_request("fifth-correction", "sent")


async def test_feedback_digest_cannot_bypass_an_active_send(store):
    _, _, lease = await leased(store)
    await store.begin_request(lease, "original", "fixture", "verify", metadata("original"))
    await store.finish_request("original", "sent")
    await store.begin_request(lease, "correction", "fixture", "verify", metadata("different-complete-request"))
    with pytest.raises(RequestReplayBlocked) as stopped:
        await store.finish_request("correction", "sent")
    assert stopped.value.request_id == "original" and stopped.value.status == "sent"


@pytest.mark.parametrize("same_digest", [True, False])
async def test_uncertain_attempts_can_retry_within_the_same_four_attempt_budget(store, same_digest):
    from docreader.llm.request import ModelWorkUnitExhausted

    run, _, lease = await leased(store)
    originals = []
    for attempt in range(4):
        identity = f"unknown-{attempt}"
        await store.begin_request(lease, identity, "fixture", "verify",
                                  metadata("same-request" if same_digest else identity))
        await store.finish_request(identity, "sent")
        ref = store.test_artifacts.put_json({"partial": identity})
        await store.finish_request(identity, "uncertain", usage={"input_tokens": attempt + 1}, response_ref=ref,
                                   error={"type": "between_data_idle_timeout"})
        originals.append((await store.get_run(run["run_id"]))["usage"][-1])
    await store.begin_request(lease, "fifth", "fixture", "verify",
                              metadata("same-request" if same_digest else "fifth"))
    with pytest.raises(ModelWorkUnitExhausted):
        await store.finish_request("fifth", "sent")
    usage = (await store.get_run(run["run_id"]))["usage"]
    assert usage[:-1] == originals
    assert all(row["status"] == "uncertain" for row in originals)
    assert usage[-1]["status"] == "prepared" and usage[-1]["sent_at"] is None


async def test_uncertain_history_does_not_hide_a_later_success_cache(store):
    run, _, lease = await leased(store)
    fields = metadata("same-complete-request")
    await store.begin_request(lease, "unknown", "fixture", "verify", fields)
    await store.finish_request("unknown", "sent")
    partial = store.test_artifacts.put_json({"partial": True})
    await store.finish_request("unknown", "uncertain", response_ref=partial)
    original = (await store.get_run(run["run_id"]))["usage"][0]
    await store.begin_request(lease, "accepted", "fixture", "verify", fields)
    await store.finish_request("accepted", "sent")
    complete = store.test_artifacts.put_json({"findings": []})
    await store.finish_request("accepted", "succeeded", response_ref=complete)
    reused = await store.begin_request(lease, "cache-read", "fixture", "verify", fields)
    assert reused["request_id"] == "accepted" and reused["response_ref"] == complete
    usage = (await store.get_run(run["run_id"]))["usage"]
    assert len(usage) == 2 and usage[0] == original


@pytest.mark.parametrize("outcome", ["sent", "uncertain"])
async def test_explicit_retry_continues_new_protocol_unknown_attempts_without_erasing_evidence(store, outcome):
    from docreader.llm.request import ModelWorkUnitExhausted

    run, command, lease = await leased(store)
    fields = metadata("same-complete-request")
    await store.begin_request(lease, "old-attempt", "fixture", "verify", fields)
    partial = store.test_artifacts.put_json({"partial": True})
    await store.finish_request("old-attempt", "sent", usage={"input_tokens": 7}, response_ref=partial)
    if outcome == "uncertain":
        await store.finish_request("old-attempt", "uncertain")
    original = (await store.get_run(run["run_id"]))["usage"][0]
    await store.release(lease, "failed", metadata={"reason": "model_work_unit_retries_exhausted"})
    retried = await store.retry(run["run_id"], 1, 1, command["command_id"], "retry-unknown", "Continue all failures")
    replacement = await store.acquire(run["run_id"], retried["command_id"], 1)
    for attempt in range(4):
        identity = f"new-attempt-{attempt}"
        await store.begin_request(replacement, identity, "fixture", "verify", fields)
        await store.finish_request(identity, "sent")
        await store.finish_request(identity, "failed", error={"type": "http_500"})
    await store.begin_request(replacement, "fifth", "fixture", "verify", fields)
    with pytest.raises(ModelWorkUnitExhausted):
        await store.finish_request("fifth", "sent")
    usage = (await store.get_run(run["run_id"]))["usage"]
    assert usage[0]["status"] == "uncertain"
    for field in ("request_id", "metadata", "response_ref", "usage", "sent_at"):
        assert usage[0][field] == original[field]
    assert len([row for row in usage[1:] if row["sent_at"]]) == 4
    assert all(row["metadata"]["command_id"] == retried["command_id"] for row in usage[1:])


async def test_new_request_cannot_bypass_legacy_unknown_payment(store):
    _, _, lease = await leased(store)
    await store.begin_request(lease, "legacy", "fixture", "verify", request_metadata("legacy"))
    await store.finish_request("legacy", "sent")
    await store.finish_request("legacy", "uncertain")
    with pytest.raises(RequestReplayBlocked):
        await store.begin_request(lease, "new-request", "fixture", "verify", metadata("legacy"))


async def test_concurrent_prepared_corrections_cannot_both_send(store):
    from docreader.llm.request import ModelWorkUnitExhausted

    run, _, lease = await leased(store)
    for attempt in range(3):
        await failed_attempt(store, lease, f"earlier-{attempt}")
    for identity in ("parallel-a", "parallel-b"):
        await store.begin_request(lease, identity, "fixture", "verify", metadata(identity))
    outcomes = await asyncio.gather(*(store.finish_request(identity, "sent") for identity in
                                      ("parallel-a", "parallel-b")), return_exceptions=True)
    assert sum(outcome is None for outcome in outcomes) == 1
    assert sum(isinstance(outcome, (RequestReplayBlocked, ModelWorkUnitExhausted)) for outcome in outcomes) == 1
    assert len([row for row in (await store.get_run(run["run_id"]))["usage"] if row["sent_at"]]) == 4


async def test_explicit_retry_gets_a_new_budget_and_preserves_old_audits(store):
    run, command, lease = await leased(store)
    for attempt in range(4):
        await failed_attempt(store, lease, f"failed-{attempt}")
    old = (await store.get_run(run["run_id"]))["usage"]
    await store.release(lease, "failed", metadata={"reason": "model_work_unit_retries_exhausted"})
    retried = await store.retry(run["run_id"], 1, 1, command["command_id"], "explicit-retry", "Continue failed unit")
    replacement = await store.acquire(run["run_id"], retried["command_id"], 1)
    await failed_attempt(store, replacement, "new-command-attempt")
    usage = (await store.get_run(run["run_id"]))["usage"]
    assert usage[:-1] == old
    assert usage[-1]["metadata"]["command_id"] == retried["command_id"]


async def test_work_unit_exhaustion_is_terminal_without_consuming_outer_retry_budget(store):
    from docreader.llm.request import ModelWorkUnitExhausted

    run, command = await submitted(store)

    class ExhaustedNodes:
        async def prepare(self):
            raise ModelWorkUnitExhausted("verify")

    settings = Settings(store.dsn, "redis://unused", store.test_artifacts.root)
    with pytest.raises(ModelWorkUnitExhausted):
        await execute_command(run["run_id"], command["command_id"], 1, settings,
                              lambda *_: ExhaustedNodes())
    current = await store.get_command(command["command_id"])
    assert current["infrastructure_failures"] == 0
    assert current["status"] == "completed"
    assert current["receipt"]["status"] == "failed"
    assert current["receipt"]["metadata"]["reason"] == "model_work_unit_retries_exhausted"
    assert (await store.get_run(run["run_id"]))["status"] == "failed"
    assert not await store.claim_commands()
