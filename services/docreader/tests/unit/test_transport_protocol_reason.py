"""Protocol reasons are fixed labels; transport and audit semantics stay intact."""

import json

import httpcore
import httpx
import pytest

from docreader.llm import UncertainModelResponse
from docreader.llm.request import request_digest
from test_tender_llm import CountedArtifacts, model
from test_tender_streaming import ScheduledStream


CLOSED = "Server disconnected without sending a response."
PRIVATE = "synthetic-secret https://private.invalid/?access_token=private-token private-body"


async def assert_failure(tmp_path, error, expected_reason, *, cause=None, body=False):
    artifacts = CountedArtifacts(tmp_path)
    seen = []
    error.__cause__ = cause
    source = ScheduledStream([0], [(0, error)]) if body else None

    def handler(request):
        seen.append(request)
        if body:
            return httpx.Response(200, stream=source, headers={"content-type": "text/event-stream"})
        raise error

    client, audit = model(handler, artifacts=artifacts)
    async with client:
        with pytest.raises(UncertainModelResponse) as caught:
            await client.call("extract", {"source": "private-body"})

    assert len(seen) == 1
    assert [entry["status"] for entry in audit.events] == ["prepared", "sent", "uncertain"]
    assert len({entry["request_id"] for entry in audit.events}) == 1
    assert artifacts.writes == 1
    prepared, terminal = audit.events[0], audit.events[-1]
    details = terminal["error"]
    saved = artifacts.read_json(terminal["response_ref"])
    digest = request_digest(client.config.chat.base_url, json.loads(seen[0].content))
    assert prepared["metadata"]["request_digest"] == digest
    assert prepared["metadata"]["logical_request_digest"] == digest
    assert saved["request_digest"] == digest
    assert terminal["usage"] is None and saved["usage"] is None
    assert saved["usage_status"] == "unknown" and saved["raw_body"] == ""
    assert details["type"] == "transport_response_unknown"
    assert details["transport_phase"] == ("reading_body" if body else "before_headers")
    diagnostics = {key: value for key, value in details.items() if key.startswith("transport_")}
    assert {key: value for key, value in saved.items() if key.startswith("transport_")} == diagnostics
    assert client.usage.snapshot() == {
        "requests": 1, "statuses": {"uncertain": 1}, "reported_usage_requests": 0,
        "missing_usage_requests": 1, "reported_tokens": {},
    }
    encoded = json.dumps({"error": details, "response": saved, "exception": str(caught.value)})
    assert all(value not in encoded for value in (
        "synthetic-secret", "private.invalid", "private-token", "private-body", CLOSED,
    ))
    if source is not None:
        assert source.closed and saved["activity"]["raw_received_bytes"] == 0
    if expected_reason is None:
        assert "transport_protocol_reason" not in details
        assert "transport_protocol_reason" not in saved
    else:
        assert details.get("transport_protocol_reason") == expected_reason
    return details


async def test_standard_httpx_disconnect_keeps_the_wrapped_core_chain(tmp_path):
    details = await assert_failure(
        tmp_path, httpx.RemoteProtocolError(CLOSED), "server_disconnected_without_response",
        cause=httpcore.RemoteProtocolError(CLOSED),
    )
    assert details["transport_error_types"] == [
        "APIConnectionError", "RemoteProtocolError", "RemoteProtocolError",
    ]


@pytest.mark.parametrize("message", ["illegal status line: " + PRIVATE, CLOSED + " " + PRIVATE],
                         ids=["invalid-response", "non-exact-close"])
async def test_other_protocol_messages_are_not_saved_or_matched_by_substring(tmp_path, message):
    await assert_failure(
        tmp_path, httpx.RemoteProtocolError(message), "other_remote_protocol_error",
        cause=httpcore.RemoteProtocolError(PRIVATE),
    )


@pytest.mark.parametrize("error_type", [httpx.ReadError, httpcore.RemoteProtocolError])
async def test_only_httpx_remote_protocol_errors_receive_a_reason(tmp_path, error_type):
    await assert_failure(tmp_path, error_type(CLOSED), None)


async def test_standard_body_disconnect_retains_reading_body_phase_without_bytes(tmp_path):
    details = await assert_failure(
        tmp_path, httpx.RemoteProtocolError(CLOSED), "server_disconnected_without_response",
        cause=httpcore.RemoteProtocolError(PRIVATE), body=True,
    )
    assert details["transport_error_types"] == ["RemoteProtocolError", "RemoteProtocolError"]
