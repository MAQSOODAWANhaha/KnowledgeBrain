"""Project-scoped same-run analysis recovery; all receipts are synthetic."""
import asyncio
from unittest.mock import AsyncMock
from uuid import uuid4

import pytest
from psycopg.types.json import Jsonb

from docreader.integrations.web import WebError, handle_command
from docreader.integrations.web_recovery import analysis_recovery, retry_analysis
from docreader.jobs.settings import Settings
from docreader.tests.workflow.test_web_bridge import runtime as runtime
from docreader.tests.workflow.test_web_bridge import payload, upload
from docreader.workflow.nodes import WORKFLOW_PROTOCOL


async def failure(runtime, *, source_artifact=False, payment=None):
    await upload(runtime)
    started = await runtime.bridge.start_analysis(payload(runtime, request_key="start-recovery-fixture"))
    run = await runtime.bridge.store.get_run(started["analysis"]["run_id"])
    command = run["commands"][-1]
    lease = await runtime.bridge.store.acquire(run["run_id"], command["command_id"], 1)
    await runtime.bridge.store.pin_execution_spec(lease, {"workflow_protocol": WORKFLOW_PROTOCOL})
    if source_artifact:
        reference = runtime.bridge.artifacts.put_json({"synthetic": "accepted native source"})
        await runtime.bridge.store.commit_artifact(lease, "freeze-document", reference, scope="first", input_digest="source-first")
        await runtime.bridge.store.begin_request(lease, "ocr-success", "fixture", "ocr", {"request_digest": "a" * 64})
        await runtime.bridge.store.finish_request("ocr-success", "sent")
        await runtime.bridge.store.finish_request("ocr-success", "succeeded", response_ref=reference)
    if payment:
        metadata = {"request_digest": "b" * 64}
        if payment == "current":
            metadata["logical_request_digest"] = "c" * 64
        await runtime.bridge.store.begin_request(lease, "unknown-payment", "fixture", "extract", metadata)
        await runtime.bridge.store.finish_request("unknown-payment", "sent")
        await runtime.bridge.store.finish_request("unknown-payment", "uncertain")
        if payment == "legacy_integer":
            async with runtime.bridge.store.transaction() as conn:
                await conn.execute("UPDATE tender_analysis_model_requests SET metadata=metadata || %s WHERE request_id='unknown-payment'",
                                   (Jsonb({"logical_request_digest": int("1" * 64)}),))
    await runtime.bridge.store.release(lease, "failed", error="SyntheticFailure")
    return await runtime.bridge.store.get_run(run["run_id"]), command


def retry_body(run, command, **extra):
    return {"run_id": run["run_id"], "source_version": run["source_version"],
            "domain_revision": run["domain_revision"], "failed_command_id": command["command_id"],
            "reason": "人工核实故障已处理，继续原分析。", "request_key": "same-recovery", **extra}


async def test_no_analysis_has_no_recovery(runtime):
    assert await analysis_recovery(runtime.bridge, payload(runtime)) == {"recovery": None}


async def test_foreign_and_unauthorized_retry_never_reads_run_artifacts(runtime, monkeypatch):
    def forbidden(*_args):
        raise AssertionError("foreign recovery cannot read analysis artifacts")
    monkeypatch.setattr(runtime.bridge.artifacts, "read_json", forbidden)
    for values in ({"owner_user_id": str(uuid4())}, {"project_id": runtime.other, "run_id": "foreign-run"}):
        with pytest.raises(WebError) as rejected:
            await retry_analysis(runtime.bridge, payload(runtime, **values))
        assert rejected.value.status == 404


@pytest.mark.parametrize("source_artifact", [False, True])
async def test_early_failure_recovers_revision_one_and_preserves_source_zero_receipts(runtime, source_artifact):
    run, command = await failure(runtime, source_artifact=source_artifact)
    view = (await analysis_recovery(runtime.bridge, payload(runtime)))["recovery"]
    assert view["can_retry"] and view["failed_command_id"] == command["command_id"]
    assert view["domain_revision"] == 1
    if source_artifact:
        assert run["artifacts"][0]["domain_revision"] == 0
        assert run["usage"][0]["domain_revision"] == 1
    submitted = await retry_analysis(runtime.bridge, payload(runtime, **retry_body(run, command)))
    current = await runtime.bridge.store.get_run(run["run_id"])
    assert submitted["analysis"] == {"run_id": run["run_id"], "source_version": 1, "domain_revision": 1, "status": "pending"}
    assert len(current["commands"]) == 2 and current["commands"][1]["command_id"] == submitted["command_id"]
    for key in ("artifacts", "usage", "revision", "history", "manifest_ref"):
        assert current[key] == run[key]
    async with runtime.bridge.store.transaction() as conn:
        count = (await (await conn.execute("SELECT count(*) AS n FROM tender_analysis_runs")).fetchone())["n"]
    assert count == 1


async def test_retry_same_key_concurrently_replays_even_after_completed(runtime):
    run, command = await failure(runtime)
    request = payload(runtime, **retry_body(run, command))
    one, two = await asyncio.gather(retry_analysis(runtime.bridge, request), retry_analysis(runtime.bridge, request))
    assert one["command_id"] == two["command_id"]
    lease = await runtime.bridge.store.acquire(run["run_id"], one["command_id"], 1)
    await runtime.bridge.store.release(lease, "analysis-ready")
    assert (await retry_analysis(runtime.bridge, request))["command_id"] == one["command_id"]
    assert not (await analysis_recovery(runtime.bridge, payload(runtime)))["recovery"]["can_retry"]
    assert len((await runtime.bridge.store.get_run(run["run_id"]))["commands"]) == 2
    for changed in ({"reason": "不同的恢复理由"}, {"failed_command_id": str(uuid4())}):
        with pytest.raises(WebError) as conflict:
            await retry_analysis(runtime.bridge, request | changed)
        assert conflict.value.status == 409


@pytest.mark.parametrize("changed", [{"source_version": 2}, {"domain_revision": 2}, {"failed_command_id": str(uuid4())}])
async def test_stale_retry_cannot_advance_or_replace_run(runtime, changed):
    run, command = await failure(runtime)
    with pytest.raises(WebError) as stale:
        await retry_analysis(runtime.bridge, payload(runtime, **retry_body(run, command, **changed)))
    assert stale.value.status == 409
    assert await runtime.bridge.store.get_run(run["run_id"]) == run


@pytest.mark.parametrize("payment", ["legacy", "legacy_integer", "current"])
async def test_payment_fence_matches_existing_domain_policy(runtime, payment):
    run, command = await failure(runtime, payment=payment)
    view = (await analysis_recovery(runtime.bridge, payload(runtime)))["recovery"]
    request = payload(runtime, **retry_body(run, command))
    if payment != "current":
        assert not view["can_retry"] and view["blocker"] == "payment_reconciliation"
        with pytest.raises(WebError) as blocked:
            await retry_analysis(runtime.bridge, request)
        assert blocked.value.code == "ANALYSIS_RECONCILIATION_REQUIRED" and blocked.value.status == 409
        assert await runtime.bridge.store.get_run(run["run_id"]) == run
    else:
        assert view["can_retry"]
        await retry_analysis(runtime.bridge, request)
        assert (await runtime.bridge.store.get_run(run["run_id"]))["usage"] == run["usage"]


async def test_pending_and_cancelled_are_not_advertised_as_recoverable(runtime):
    run, command = await failure(runtime)
    await runtime.bridge.store.retry(run["run_id"], 1, 1, command["command_id"], "direct-retry", "Fixture")
    assert not (await analysis_recovery(runtime.bridge, payload(runtime)))["recovery"]["can_retry"]
    await runtime.bridge.store.cancel(run["run_id"])
    assert not (await analysis_recovery(runtime.bridge, payload(runtime)))["recovery"]["can_retry"]
    with pytest.raises(WebError) as rejected:
        await retry_analysis(runtime.bridge, payload(runtime, **retry_body(run, command)))
    assert rejected.value.status == 409


async def test_legacy_protocol_has_an_explicit_operator_exit(runtime):
    run, command = await failure(runtime)
    async with runtime.bridge.store.transaction() as conn:
        await conn.execute("UPDATE tender_analysis_revisions SET execution_spec=%s WHERE run_id=%s",
                           (Jsonb({"workflow_protocol": {"schema_version": 1}}), run["run_id"]))
    view = (await analysis_recovery(runtime.bridge, payload(runtime)))["recovery"]
    assert not view["can_retry"] and view["blocker"] == "unsupported_protocol" and "管理员" in view["notice"]
    with pytest.raises(WebError) as rejected:
        await retry_analysis(runtime.bridge, payload(runtime, **retry_body(run, command)))
    assert rejected.value.status == 409


async def test_historical_revision_zero_is_not_silently_replaced(monkeypatch):
    from docreader.integrations import web_recovery

    legacy = {"run_id": "historical", "source_version": 1, "domain_revision": 0,
              "status": "failed", "commands": [], "usage": []}
    monkeypatch.setattr(web_recovery, "_owned_run", AsyncMock(return_value=legacy))
    view = (await analysis_recovery(None, {}))["recovery"]
    assert not view["can_retry"] and view["blocker"] == "unsupported_revision" and "管理员" in view["notice"]


@pytest.mark.parametrize("extra", [{"reason": " "}, {"reason": "字" * 4001}, {"domain_revision": 0}, {"manifest_ref": {}}, {"failed_command_id": "invalid"}])
async def test_invalid_retry_payload_is_rejected_before_mutation(runtime, extra):
    run, command = await failure(runtime)
    with pytest.raises(WebError) as invalid:
        await retry_analysis(runtime.bridge, payload(runtime, **retry_body(run, command, **extra)))
    assert invalid.value.status == 400
    assert await runtime.bridge.store.get_run(run["run_id"]) == run


async def test_recovery_actions_reach_authenticated_dispatch(runtime, monkeypatch):
    run, command = await failure(runtime)
    monkeypatch.setattr(Settings, "from_env", classmethod(lambda cls: runtime.settings))
    view, blob = await handle_command(payload(runtime, "analysis.recovery"))
    assert view["recovery"]["can_retry"] and blob == b""
    accepted, _ = await handle_command(payload(runtime, "analysis.retry", **retry_body(run, command)))
    assert accepted["analysis"]["run_id"] == run["run_id"]
