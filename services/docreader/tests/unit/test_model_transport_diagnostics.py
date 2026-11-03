"""Transport diagnostics retain safe causes without changing failure semantics."""
import asyncio
import errno
import json

import httpx
import pytest

from docreader.llm import ModelInfrastructureError, UncertainModelResponse
from test_tender_llm import CountedArtifacts, model
from test_tender_streaming import event, timed_model


PRIVATE = "synthetic-secret https://private.invalid/?access_token=private-token private-prompt"


@pytest.mark.parametrize("error_type", [httpx.RemoteProtocolError, httpx.ReadError, httpx.WriteError])
async def test_before_headers_keeps_distinct_causes_and_errno_without_private_messages(tmp_path, error_type):
    artifacts = CountedArtifacts(tmp_path)

    def handler(request):
        raise error_type(PRIVATE, request=request) from OSError(errno.ECONNRESET, PRIVATE)

    client, audit = model(handler, artifacts=artifacts)
    async with client:
        with pytest.raises(UncertainModelResponse):
            await client.call("extract", {})
    terminal = audit.events[-1]
    details = terminal["error"]
    assert terminal["status"] == "uncertain" and terminal["usage"] is None
    assert details["type"] == "transport_response_unknown"
    assert details["transport_phase"] == "before_headers"
    assert details["transport_error_types"] == ["APIConnectionError", error_type.__name__, "ConnectionResetError"]
    assert details["transport_errno"] == errno.ECONNRESET
    saved = artifacts.read_json(terminal["response_ref"])
    for key in ("transport_phase", "transport_error_types", "transport_errno"):
        assert saved[key] == details[key]
    assert saved["activity"]["raw_received_bytes"] == 0 and saved["provider_error"] is None
    encoded = json.dumps({"error": details, "response": saved})
    assert all(value not in encoded for value in ("synthetic-secret", "private.invalid", "private-token", "private-prompt"))


@pytest.mark.parametrize("partial", [b"", event('{"partial":')])
async def test_body_failure_uses_acquired_headers_even_without_body_bytes(monkeypatch, tmp_path, partial):
    artifacts = CountedArtifacts(tmp_path)
    chunks = ([(1, partial)] if partial else []) + [(1, httpx.ReadError(PRIVATE))]
    client, audit, _, source = timed_model(monkeypatch, chunks, artifacts=artifacts)
    async with client:
        with pytest.raises(UncertainModelResponse):
            await client.call("extract", {})
    terminal = audit.events[-1]
    assert terminal["error"]["type"] == "transport_response_unknown"
    assert terminal["error"]["transport_phase"] == "reading_body"
    assert terminal["error"]["transport_error_types"] == ["ReadError"]
    saved = artifacts.read_json(terminal["response_ref"])
    assert saved["raw_body"] == partial.decode() and saved["usage"] is None
    assert saved["transport_phase"] == "reading_body" and source.closed


@pytest.mark.parametrize("cycle", [False, True])
async def test_transport_cause_chain_is_bounded_and_cycle_safe(cycle):
    causes = [httpx.ReadError(PRIVATE) for _ in range(6)]
    for first, second in zip(causes, causes[1:]):
        first.__cause__ = second
    if cycle:
        causes[1].__cause__ = causes[0]

    def handler(request):
        raise causes[0]

    client, audit = model(handler)
    async with client:
        with pytest.raises(UncertainModelResponse):
            await client.call("extract", {})
    assert audit.events[-1]["error"]["transport_error_types"] == ["APIConnectionError"] + ["ReadError"] * (2 if cycle else 3)


@pytest.mark.parametrize("number", [None, True, "104"])
async def test_transport_errno_ignores_non_integer_values(number):
    cause = OSError(PRIVATE)
    cause.errno = number

    def handler(request):
        raise httpx.ReadError(PRIVATE, request=request) from cause

    client, audit = model(handler)
    async with client:
        with pytest.raises(UncertainModelResponse):
            await client.call("extract", {})
    details = audit.events[-1]["error"]
    assert details["transport_error_types"][-1] == "OSError"
    assert "transport_errno" not in details


@pytest.mark.parametrize("partial", [b"", event('{"partial":')])
async def test_idle_timeout_keeps_existing_status_and_gains_precise_phase(monkeypatch, tmp_path, partial):
    artifacts = CountedArtifacts(tmp_path)
    chunks = ([(1, partial)] if partial else []) + [(181, b"late")]
    client, audit, _, _ = timed_model(monkeypatch, chunks, artifacts=artifacts)
    expected = UncertainModelResponse if partial else ModelInfrastructureError
    async with client:
        with pytest.raises(expected):
            await client.call("extract", {})
    terminal = audit.events[-1]
    assert terminal["status"] == ("uncertain" if partial else "failed")
    assert terminal["error"]["type"] == ("between_data_idle_timeout" if partial else "first_data_idle_timeout")
    assert terminal["error"]["transport_phase"] == "reading_body"
    assert terminal["error"]["transport_error_types"][0] == "TimeoutError"


async def test_cancel_does_not_invent_a_transport_failure():
    def handler(request):
        raise asyncio.CancelledError

    client, audit = model(handler)
    async with client:
        with pytest.raises(asyncio.CancelledError):
            await client.call("extract", {})
    terminal = audit.events[-1]
    assert terminal["status"] == "uncertain" and terminal["error"]["type"] == "cancelled_after_send"
    assert not any(key.startswith("transport_") for key in terminal["error"])
