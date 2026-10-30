"""HTTP capacity rejections retain bounded attempts and honor retry timing."""

import asyncio
from datetime import datetime, timedelta, timezone
from email.utils import format_datetime
import json

import httpx
import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.integrations.store import Cancelled, Lease, StaleExecution
from docreader.llm import client as client_module
from docreader.llm import request as request_module
from docreader.llm.recovery import call_validated
from docreader.llm.request import ModelInfrastructureError, ModelWorkUnitExhausted
from test_contract_recovery import Model, Store, validate
from test_tender_llm import model


NOW = datetime(2026, 10, 30, 6, 0, tzinfo=timezone.utc)


@pytest.mark.parametrize("header,expected", [
    (None, None), ("", None), ("12", 12), (" 12.5 ", 12.5), ("0", 0), ("600", 600),
    ("-1", None), ("NaN", None), ("Infinity", None), ("1e309", None),
    ("private-header-value", None), ("Fri, 99 Oct 2026 06:00:00 GMT", None),
    (format_datetime(NOW + timedelta(seconds=45), usegmt=True), 45),
    (format_datetime(NOW - timedelta(seconds=45), usegmt=True), 0),
])
async def test_http_429_carries_only_parsed_retry_delay(monkeypatch, tmp_path, header, expected):
    monkeypatch.setattr(client_module.time, "time", lambda: NOW.timestamp())
    artifacts = ArtifactStore(tmp_path)
    headers = {"x-private-header": "private-header-value"}
    if header is not None:
        headers["retry-after"] = header
    instance, audit = model(lambda request: httpx.Response(
        429, headers=headers, json={"error": {"code": "capacity_exceeded"}}), artifacts=artifacts)

    async with instance:
        with pytest.raises(ModelInfrastructureError) as rejected:
            await instance.call("extract", {})

    assert type(rejected.value).__name__ == "ModelRateLimitError"
    assert rejected.value.retry_after_seconds == expected
    terminal = audit.events[-1]
    assert terminal["status"] == "failed" and terminal["error"]["type"] == "http_429"
    saved = artifacts.read_json(terminal["response_ref"])
    assert saved.get("retry_after_seconds") == expected
    assert ("retry_after_seconds" in saved) is (expected is not None)
    assert "headers" not in saved
    assert "private-header-value" not in json.dumps(saved)
    assert "private-header-value" not in str(rejected.value)


async def test_other_http_failures_keep_their_existing_recovery_class(tmp_path):
    instance, audit = model(lambda request: httpx.Response(
        503, headers={"retry-after": "45"}, json={"error": "unavailable"}), artifacts=ArtifactStore(tmp_path))
    async with instance:
        with pytest.raises(ModelInfrastructureError) as rejected:
            await instance.call("extract", {})
    assert type(rejected.value) is ModelInfrastructureError
    assert audit.events[-1]["error"]["type"] == "http_503"


@pytest.mark.parametrize("hint,expected", [
    (None, [30, 60, 120]), (45, [45, 60, 120]), (0, [30, 60, 120]), (600, [120, 120, 120]),
    (1, [30, 60, 120]),
])
async def test_rate_limit_recovery_waits_then_accepts_fourth_attempt(monkeypatch, tmp_path, hint, expected):
    delays = []

    async def sleep(seconds):
        delays.append(seconds)

    monkeypatch.setattr("docreader.llm.recovery.asyncio.sleep", sleep)
    error = request_module.ModelRateLimitError(hint)
    instance = Model([error] * 3 + [{"findings": []}])
    assert await call_validated(instance, Store(), ArtifactStore(tmp_path), Lease("run", 1, 1, "cmd"),
                                "verify", {}, validate) == {"findings": []}
    assert len(instance.calls) == 4 and delays == expected
    assert error.retry_after_seconds == hint


async def test_capacity_rejections_still_stop_after_initial_and_three_retries(monkeypatch, tmp_path):
    delays = []

    async def sleep(seconds):
        delays.append(seconds)

    monkeypatch.setattr("docreader.llm.recovery.asyncio.sleep", sleep)
    instance = Model([request_module.ModelRateLimitError()] * 4 + [{"findings": []}])
    with pytest.raises(ModelWorkUnitExhausted):
        await call_validated(instance, Store(), ArtifactStore(tmp_path), Lease("run", 1, 1, "cmd"),
                             "verify", {}, validate)
    assert len(instance.calls) == 4 and delays == [30, 60, 120]


async def test_other_infrastructure_errors_keep_short_backoff(monkeypatch, tmp_path):
    delays = []

    async def sleep(seconds):
        delays.append(seconds)

    monkeypatch.setattr("docreader.llm.recovery.asyncio.sleep", sleep)
    instance = Model([ModelInfrastructureError("offline")] * 3 + [{"findings": []}])
    await call_validated(instance, Store(), ArtifactStore(tmp_path), Lease("run", 1, 1, "cmd"),
                         "verify", {}, validate)
    assert len(instance.calls) == 4 and delays == [1, 2, 4]


async def test_cancellation_during_capacity_wait_stops_without_another_send(monkeypatch, tmp_path):
    async def sleep(seconds):
        raise asyncio.CancelledError()

    monkeypatch.setattr("docreader.llm.recovery.asyncio.sleep", sleep)
    instance = Model([request_module.ModelRateLimitError(), {"findings": []}])
    with pytest.raises(asyncio.CancelledError):
        await call_validated(instance, Store(), ArtifactStore(tmp_path), Lease("run", 1, 1, "cmd"),
                             "verify", {}, validate)
    assert len(instance.calls) == 1


@pytest.mark.parametrize("error", [Cancelled("cancelled"), StaleExecution("lease lost")])
async def test_execution_stop_does_not_enter_capacity_backoff(monkeypatch, tmp_path, error):
    delays = []

    async def sleep(seconds):
        delays.append(seconds)

    monkeypatch.setattr("docreader.llm.recovery.asyncio.sleep", sleep)
    instance = Model([error, {"findings": []}])
    with pytest.raises(type(error)):
        await call_validated(instance, Store(), ArtifactStore(tmp_path), Lease("run", 1, 1, "cmd"),
                             "verify", {}, validate)
    assert len(instance.calls) == 1 and delays == []
