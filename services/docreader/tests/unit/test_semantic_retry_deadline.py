"""Host rejection feedback and its short retry deadline resume together."""
import asyncio
from datetime import datetime, timezone
import time

import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.integrations.store import Lease
from docreader.llm.recovery import call_validated
from docreader.llm.request import ModelWorkUnitExhausted
from test_contract_recovery import Model, Store, validate


@pytest.mark.parametrize("elapsed,remaining", [(.25, [.75]), (2, [])])
async def test_semantic_retry_restart_waits_only_remaining_time(tmp_path, monkeypatch, elapsed, remaining):
    store, artifacts, lease = Store(), ArtifactStore(tmp_path), Lease("run", 1, 1, "cmd")
    model, now, waits = Model([{"findings": None}, {"findings": []}]), [1000.0], []
    monkeypatch.setattr(time, "time", lambda: now[0])

    async def interrupted(seconds):
        now[0] += elapsed
        raise asyncio.CancelledError()

    monkeypatch.setattr(asyncio, "sleep", interrupted)
    with pytest.raises(asyncio.CancelledError):
        await call_validated(model, store, artifacts, lease, "verify", {}, validate)

    async def wait(seconds):
        waits.append(seconds)
        now[0] += seconds

    monkeypatch.setattr(asyncio, "sleep", wait)
    assert await call_validated(model, store, artifacts, lease, "verify", {}, validate) == {"findings": []}
    assert waits == remaining and len(model.calls) == 2


@pytest.mark.parametrize("timestamp_known", [True, False])
async def test_legacy_semantic_retry_records_missing_timing_once(tmp_path, monkeypatch, timestamp_known):
    store, artifacts, lease = Store(), ArtifactStore(tmp_path), Lease("run", 1, 1, "cmd")
    model, now, waits = Model([{"findings": None}, {"findings": []}]), [1000.0], []
    monkeypatch.setattr(time, "time", lambda: now[0])

    async def interrupted(seconds):
        raise asyncio.CancelledError()

    monkeypatch.setattr(asyncio, "sleep", interrupted)
    with pytest.raises(asyncio.CancelledError):
        await call_validated(model, store, artifacts, lease, "verify", {}, validate)
    row = next(iter(store.rows.values()))
    old = artifacts.read_json(row["artifact_ref"])
    old.pop("not_before", None)
    row["artifact_ref"] = artifacts.put_json(old)
    if timestamp_known:
        row["committed_at"] = datetime.fromtimestamp(1000, timezone.utc)
    now[0] += .25
    # A second worker can also die while waiting; unknown legacy time must not
    # create another full delay for the third worker.
    with pytest.raises(asyncio.CancelledError):
        await call_validated(model, store, artifacts, lease, "verify", {}, validate)
    now[0] += .25

    async def wait(seconds):
        waits.append(seconds)
        now[0] += seconds

    monkeypatch.setattr(asyncio, "sleep", wait)
    assert await call_validated(model, store, artifacts, lease, "verify", {}, validate) == {"findings": []}
    assert waits == ([.5] if timestamp_known else [.75])
    if not timestamp_known:
        fallback = next(value for (stage, _, _), value in store.rows.items() if stage == "model-rejection-wait")
        assert artifacts.read_json(fallback["artifact_ref"])["timing_origin"] == "legacy_unavailable"


async def test_semantic_retry_restart_never_grants_a_third_correction(tmp_path, monkeypatch):
    store, artifacts, lease = Store(), ArtifactStore(tmp_path), Lease("run", 1, 1, "cmd")
    model, now = Model([{"findings": None}] * 3), [1000.0]
    monkeypatch.setattr(time, "time", lambda: now[0])

    async def interrupted(seconds):
        now[0] += seconds
        raise asyncio.CancelledError()

    monkeypatch.setattr(asyncio, "sleep", interrupted)
    for _ in range(2):
        with pytest.raises(asyncio.CancelledError):
            await call_validated(model, store, artifacts, lease, "verify", {}, validate)
    for _ in range(2):
        with pytest.raises(ModelWorkUnitExhausted):
            await call_validated(model, store, artifacts, lease, "verify", {}, validate)
    assert len(model.calls) == len(store.rows) == 3
