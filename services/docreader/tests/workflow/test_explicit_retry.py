"""Explicit recovery preserves immutable receipts in isolated PostgreSQL schemas."""

import asyncio
from argparse import Namespace

import pytest

from docreader.integrations.store import Conflict, RequestReplayBlocked
from test_store import request_metadata, store as store, submitted

pytestmark = pytest.mark.asyncio


async def failed_execution(store, kind="start"):
    run, command = await submitted(store)
    lease = await store.acquire(run["run_id"], command["command_id"], 1)
    if kind == "resume":
        await store.release(lease, "waiting-confirmation", checklist_digest="approved-checklist")
        command = await store.confirm(run["run_id"], 1, "approved-checklist", "approve")
        lease = await store.acquire(run["run_id"], command["command_id"], 1)
    return run, command, lease


@pytest.mark.parametrize("kind", ["start", "resume"])
async def test_retry_preserves_paid_successes_artifacts_revision_and_original_command(store, kind):
    run, command, lease = await failed_execution(store, kind)
    run_id = run["run_id"]
    ref = store.test_artifacts.put_json({"synthetic": "accepted extraction pack"})
    receipt = await store.commit_artifact(lease, "extract-pack", ref, scope="pack-one", input_digest="pack-input")
    metadata = request_metadata("successful-pack")
    response = store.test_artifacts.put_json({"synthetic": "complete model response"})
    await store.begin_request(lease, "successful-pack", "fixture", "extract", metadata)
    await store.finish_request("successful-pack", "sent")
    await store.finish_request("successful-pack", "succeeded", {"input_tokens": 8}, response_ref=response)
    await store.begin_request(lease, "failed-pack", "fixture", "extract", request_metadata("failed-pack"))
    await store.finish_request("failed-pack", "sent")
    await store.finish_request("failed-pack", "failed", error={"type": "invalid_json"}, response_ref=ref)
    await store.release(lease, "failed", error={"type": "ModelSemanticError"})
    before = await store.get_run(run_id)
    original = await store.get_command(command["command_id"])

    retried = await store.retry(run_id, 1, 1, command["command_id"], "retry-pack", "Retry complete invalid JSON")
    after = await store.get_run(run_id)
    assert after["status"] == "pending"
    assert after["source_version"] == after["domain_revision"] == 1
    assert retried["command_id"] != command["command_id"]
    assert retried["status"] == "pending" and retried["kind"] == kind
    assert retried["receipt"] is None
    assert retried["execution_count"] == retried["infrastructure_failures"] == 0
    assert retried["payload"] == {**command["payload"], "retry": {
        "failed_command_id": command["command_id"], "request_key": "retry-pack", "reason": "Retry complete invalid JSON",
    }}
    assert len(after["commands"]) == len(before["commands"]) + 1
    assert await store.get_command(command["command_id"]) == original
    for key in ("manifest_ref", "revision", "artifacts", "usage", "confirmations", "history"):
        assert after[key] == before[key]
    assert store.test_artifacts.read_json(response) == {"synthetic": "complete model response"}

    replacement = await store.acquire(run_id, retried["command_id"], 1)
    assert replacement.epoch > lease.epoch
    assert await store.receipt(replacement, "extract-pack", "pack-one", input_digest="pack-input") == receipt
    cached = await store.begin_request(replacement, "must-not-send", "fixture", "extract", metadata)
    assert cached == next(row for row in before["usage"] if row["request_id"] == "successful-pack")
    repeat = await store.begin_request(replacement, "explicitly-retried-pack", "fixture", "extract",
                                       request_metadata("failed-pack"))
    assert repeat["status"] == "prepared" and repeat["epoch"] == replacement.epoch
    assert len((await store.get_run(run_id))["usage"]) == len(before["usage"]) + 1


async def test_retry_key_is_concurrently_idempotent_and_remains_replayable_after_execution(store):
    run, command, lease = await failed_execution(store)
    await store.release(lease, "failed")
    args = (run["run_id"], 1, 1, command["command_id"], "same-retry", "Retry failed execution")
    one, two = await asyncio.gather(store.retry(*args), store.retry(*args))
    assert one == two
    assert len((await store.get_run(run["run_id"]))["commands"]) == 2
    replacement = await store.acquire(run["run_id"], one["command_id"], 1)
    assert (await store.retry(*args))["command_id"] == one["command_id"]
    await store.release(replacement, "analysis-ready")
    completed = await store.get_run(run["run_id"])
    assert (await store.retry(*args))["command_id"] == one["command_id"]
    assert await store.get_run(run["run_id"]) == completed
    for changed in (
        (*args[:-1], "A different authorization reason"),
        (*args[:3], one["command_id"], *args[4:]),
    ):
        with pytest.raises(Conflict):
            await store.retry(*changed)
    assert await store.get_run(run["run_id"]) == completed


async def test_only_latest_failed_command_can_be_retried_and_retry_audit_is_replaced(store):
    run, original, lease = await failed_execution(store)
    run_id = run["run_id"]
    await store.release(lease, "failed")
    first = await store.retry(run_id, 1, 1, original["command_id"], "first", "First explicit attempt")
    replacement = await store.acquire(run_id, first["command_id"], 1)
    await store.release(replacement, "failed")
    before = await store.get_run(run_id)
    with pytest.raises(Conflict, match="latest"):
        await store.retry(run_id, 1, 1, original["command_id"], "obsolete", "Wrong failed command")
    assert await store.get_run(run_id) == before
    second = await store.retry(run_id, 1, 1, first["command_id"], "second", "Second explicit attempt")
    assert second["payload"] == {**original["payload"], "retry": {
        "failed_command_id": first["command_id"], "request_key": "second", "reason": "Second explicit attempt",
    }}
    assert (await store.get_run(run_id))["commands"][:-1] == before["commands"]


@pytest.mark.parametrize("status", ["pending", "running", "blocked", "waiting-confirmation", "analysis-ready"])
async def test_nonfailed_run_cannot_create_explicit_retry(store, status):
    run, command = await submitted(store)
    if status != "pending":
        lease = await store.acquire(run["run_id"], command["command_id"], 1)
        if status != "running":
            await store.release(lease, status, checklist_digest="checklist" if status == "waiting-confirmation" else None)
    before = await store.get_run(run["run_id"])
    with pytest.raises(Conflict):
        await store.retry(run["run_id"], 1, 1, command["command_id"], "not-failed", "Cannot retry this state")
    assert await store.get_run(run["run_id"]) == before


@pytest.mark.parametrize("status", ["sent", "uncertain"])
async def test_sent_or_uncertain_model_request_blocks_retry_without_mutating_audit(store, status):
    run, command, lease = await failed_execution(store)
    await store.begin_request(lease, "possibly-paid", "fixture", "extract", request_metadata("possibly-paid"))
    await store.finish_request("possibly-paid", "sent")
    if status == "uncertain":
        await store.finish_request("possibly-paid", "uncertain")
    await store.release(lease, "failed")
    before = await store.get_run(run["run_id"])
    with pytest.raises(RequestReplayBlocked) as blocked:
        await store.retry(run["run_id"], 1, 1, command["command_id"], "unsafe-retry", "Must reconcile first")
    assert blocked.value.request_id == "possibly-paid" and blocked.value.status == status
    assert await store.get_run(run["run_id"]) == before


@pytest.mark.parametrize("source_version,domain_revision", [(2, 1), (1, 2)])
async def test_retry_requires_current_source_and_domain_versions(store, source_version, domain_revision):
    run, command, lease = await failed_execution(store)
    await store.release(lease, "failed")
    before = await store.get_run(run["run_id"])
    with pytest.raises(Conflict):
        await store.retry(run["run_id"], source_version, domain_revision, command["command_id"], "stale", "Stale identity")
    assert await store.get_run(run["run_id"]) == before


@pytest.mark.parametrize("invalidation", ["cancel", "revise"])
async def test_cancelled_or_revised_run_rejects_retry_even_for_previously_accepted_key(store, invalidation):
    run, command, lease = await failed_execution(store)
    run_id = run["run_id"]
    await store.release(lease, "failed")
    args = (run_id, 1, 1, command["command_id"], "accepted-key", "Explicit recovery")
    await store.retry(*args)
    if invalidation == "cancel":
        await store.cancel(run_id)
    else:
        changed = store.test_artifacts.put_json({"documents": ["revised-source"]})
        await store.revise(run_id, changed, "new-source")
    before = await store.get_run(run_id)
    with pytest.raises(Conflict):
        await store.retry(*args)
    assert await store.get_run(run_id) == before


async def test_retry_cli_parses_explicit_identity_key_and_reason(monkeypatch):
    from docreader.integrations.cli import parse_args

    argv = ["tender", "retry", "run-one", "--source-version", "2", "--domain-revision", "3",
            "--failed-command-id", "failed-one", "--request-key", "retry-one", "--reason", "Operator approved retry"]
    monkeypatch.setattr("sys.argv", argv)
    assert vars(parse_args()) == {
        "command": "retry", "run_id": "run-one", "source_version": 2, "domain_revision": 3,
        "failed_command_id": "failed-one", "request_key": "retry-one", "reason": "Operator approved retry",
    }
    monkeypatch.setattr("sys.argv", argv[:-2])
    with pytest.raises(SystemExit) as rejected:
        parse_args()
    assert rejected.value.code == 2


async def test_retry_cli_routes_to_store_and_returns_new_command_idempotently(store, monkeypatch):
    from docreader.integrations.cli import run as cli_run
    from docreader.jobs.settings import Settings

    run, original, lease = await failed_execution(store)
    await store.release(lease, "failed")
    monkeypatch.setattr(Settings, "from_env", lambda: Settings(store.dsn, "redis://unused.invalid", store.test_artifacts.root))
    args = Namespace(command="retry", run_id=run["run_id"], source_version=1, domain_revision=1,
                     failed_command_id=original["command_id"], request_key="cli-retry", reason="Operator approved retry")
    result = await cli_run(args)
    current = await store.get_run(run["run_id"])
    command = current["commands"][-1]
    assert result == {"command_id": command["command_id"], "status": "pending", "source_version": 1, "domain_revision": 1}
    assert result["command_id"] != original["command_id"]
    assert current["status"] == "pending"
    assert result == await cli_run(args)
    assert await store.get_run(run["run_id"]) == current
