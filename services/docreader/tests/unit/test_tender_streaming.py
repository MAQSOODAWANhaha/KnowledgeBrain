"""Local-only SSE, deterministic idle clocks, and durable response reuse."""

import asyncio
import json
import zlib
from copy import deepcopy

import httpx
import pytest

from docreader.integrations.store import RequestReplayBlocked
from docreader.llm import ModelConfig, ModelInfrastructureError, ModelSemanticError, OpenAIModel, UncertainModelResponse
from docreader.llm.request import ImageInput, build_request, request_digest
from docreader.llm import streaming
from test_tender_llm import Audit, CountedArtifacts, environment, model, response


def event(content=None, *, finish=None, delta=None, usage=None):
    body = {"choices": [{"index": 0, "delta": delta if delta is not None else {"content": content},
                         "finish_reason": finish}]}
    if usage is not None:
        body = {"choices": [], "usage": usage}
    return ("data: " + json.dumps(body, ensure_ascii=False) + "\n\n").encode()


DONE = b"data: [DONE]\n\n"
GOOD = event('{"ok":true}') + event(delta={}, finish="stop") + DONE


class ScheduledStream(httpx.AsyncByteStream):
    def __init__(self, clock, chunks):
        self.clock, self.chunks, self.closed = clock, chunks, False

    async def __aiter__(self):
        for elapsed, chunk in self.chunks:
            self.clock[0] += elapsed
            if isinstance(chunk, Exception):
                raise chunk
            yield chunk

    async def aclose(self):
        self.closed = True


def timed_model(monkeypatch, chunks, *, header_delay=0, artifacts=None, headers=None):
    clock = [0.0]
    monkeypatch.setattr(streaming, "monotonic", lambda: clock[0])
    source = ScheduledStream(clock, chunks)

    def handler(request):
        clock[0] += header_delay
        return httpx.Response(200, stream=source, headers={"content-type": "text/event-stream", **(headers or {})})

    client, audit = model(handler, artifacts=artifacts)
    return client, audit, clock, source


@pytest.mark.parametrize("header_delay,body_delay", [(0, 181), (170, 11), (181, 0)])
async def test_first_body_shares_180_seconds_with_headers(monkeypatch, tmp_path, header_delay, body_delay):
    artifacts = CountedArtifacts(tmp_path)
    client, audit, _, source = timed_model(monkeypatch, [(body_delay, GOOD)],
                                          header_delay=header_delay, artifacts=artifacts)
    async with client:
        with pytest.raises(ModelInfrastructureError):
            await client.call("extract", {})
    assert audit.events[-1]["status"] == "failed"
    assert audit.events[-1]["error"]["type"] == "first_data_idle_timeout"
    assert audit.events[-1]["usage"] is None
    assert artifacts.writes == 1
    if header_delay < 180:
        assert source.closed


async def test_between_raw_data_idle_preserves_partial_once(monkeypatch, tmp_path):
    artifacts = CountedArtifacts(tmp_path)
    partial = event('{"answer":')
    client, audit, _, source = timed_model(monkeypatch, [(1, partial), (181, GOOD)], artifacts=artifacts)
    async with client:
        with pytest.raises(UncertainModelResponse):
            await client.call("extract", {})
    terminal = audit.events[-1]
    assert terminal["error"]["type"] == "between_data_idle_timeout"
    assert artifacts.writes == 1 and source.closed
    saved = artifacts.read_json(terminal["response_ref"])
    assert saved["raw_body"] == partial.decode() and saved["usage_status"] == "unknown"
    assert saved["activity"]["raw_received_chunks"] == 1


async def test_total_duration_has_no_180_second_cap_with_heartbeats_and_thinking(monkeypatch, tmp_path):
    artifacts = CountedArtifacts(tmp_path)
    chunks = [(170, b": keep alive\n\n"), (170, b"event: ping\ndata: opaque heartbeat\n\n"),
              (170, event(delta={"reasoning_content": "considering evidence"})),
              (170, event('{"ok":')), (170, event('true}')),
              (170, event(delta={}, finish="stop")),
              (170, event(usage={"prompt_tokens": 9, "completion_tokens": 4, "total_tokens": 13})), (170, DONE)]
    client, audit, clock, source = timed_model(monkeypatch, chunks, artifacts=artifacts)
    async with client:
        assert await client.call("extract", {}) == {"ok": True}
    assert clock[0] == 1360 and source.closed
    saved = artifacts.read_json(audit.events[-1]["response_ref"])
    assert saved["activity"]["heartbeat_events"] == 2
    assert saved["activity"]["thinking_deltas"] == 1
    assert saved["activity"]["content_deltas"] == 2
    assert saved["done"] and saved["finish_reason"] == "stop"
    assert saved["usage_status"] == "reported" and artifacts.writes == 1


async def test_incomplete_sse_event_and_utf8_chunks_are_activity(monkeypatch):
    data = event('{"text":"中文"}') + event(delta={}, finish="stop") + DONE
    chunks = [(170, data[index:index + 1]) for index in range(len(data))]
    client, audit, clock, _ = timed_model(monkeypatch, chunks)
    async with client:
        assert await client.call("extract", {}) == {"text": "中文"}
    assert clock[0] > 180
    assert audit.events[-1]["status"] == "succeeded"


async def test_http_compression_buffer_cannot_hide_raw_activity(monkeypatch, tmp_path):
    compressor = zlib.compressobj(wbits=31)
    data = compressor.compress(GOOD) + compressor.flush()
    # Several wire bytes may pass before gzip produces its first decoded byte.
    chunks = [(170, data[index:index + 1]) for index in range(len(data))]
    artifacts = CountedArtifacts(tmp_path)
    client, audit, clock, _ = timed_model(monkeypatch, chunks, artifacts=artifacts,
                                         headers={"content-encoding": "gzip"})
    async with client:
        assert await client.call("extract", {}) == {"ok": True}
    saved = artifacts.read_json(audit.events[-1]["response_ref"])
    assert clock[0] > 180 and saved["activity"]["raw_received_chunks"] > saved["activity"]["received_chunks"]


@pytest.mark.parametrize("ending", [b"", httpx.ReadError("connection reset")])
async def test_disconnect_or_missing_done_is_uncertain(monkeypatch, tmp_path, ending):
    artifacts = CountedArtifacts(tmp_path)
    prefix = event('{"ok":true}') + event(delta={}, finish="stop")
    client, audit, _, _ = timed_model(monkeypatch, [(10, prefix), (10, ending)], artifacts=artifacts)
    async with client:
        with pytest.raises(UncertainModelResponse):
            await client.call("extract", {})
    assert audit.events[-1]["status"] == "uncertain" and audit.events[-1]["usage"] is None
    assert artifacts.writes == 1
    assert artifacts.read_json(audit.events[-1]["response_ref"])["done"] is False


@pytest.mark.parametrize("usage", [None, {"prompt_tokens": 1, "completion_tokens": 2, "total_tokens": 3}])
async def test_normal_done_with_and_without_usage(monkeypatch, tmp_path, usage):
    artifacts = CountedArtifacts(tmp_path)
    chunks = [(1, event('{"ok":true}')), (1, event(delta={}, finish="stop"))]
    if usage:
        chunks.append((1, event(usage=usage)))
    chunks.append((1, DONE))
    client, audit, _, _ = timed_model(monkeypatch, chunks, artifacts=artifacts)
    async with client:
        assert await client.call("extract", {}) == {"ok": True}
    assert audit.events[-1]["usage"] == usage
    saved = artifacts.read_json(audit.events[-1]["response_ref"])
    assert saved["usage_status"] == ("reported" if usage else "unknown")
    assert saved["outcome"] == "completed"


@pytest.mark.parametrize("tail", [event(delta={}, finish="stop") + DONE, DONE])
async def test_truncated_json_or_missing_finish_is_never_success(monkeypatch, tail):
    client, audit, _, _ = timed_model(monkeypatch, [(1, event('{"ok":')), (1, tail)])
    async with client:
        with pytest.raises(ModelSemanticError):
            await client.call("extract", {})
    assert audit.events[-1]["status"] == "failed"


async def test_tool_parameters_remain_activity_but_cannot_be_business_json(monkeypatch, tmp_path):
    artifacts = CountedArtifacts(tmp_path)
    chunks = [(170, event(delta={"tool_calls": [{"index": 0, "function": {"arguments": '{"ok":'}}]})),
              (170, event(delta={"tool_calls": [{"index": 0, "function": {"arguments": 'true}'}}]})),
              (170, event(delta={}, finish="tool_calls")), (170, DONE)]
    client, audit, clock, _ = timed_model(monkeypatch, chunks, artifacts=artifacts)
    async with client:
        with pytest.raises(ModelSemanticError):
            await client.call("extract", {})
    assert clock[0] == 680
    saved = artifacts.read_json(audit.events[-1]["response_ref"])
    assert saved["activity"]["tool_deltas"] == 2 and saved["activity"]["content_deltas"] == 0
    assert audit.events[-1]["error"]["type"] == "tool_call_response"


@pytest.mark.parametrize("error", [httpx.ConnectError("connect"), httpx.ConnectTimeout("connect"), httpx.PoolTimeout("pool")])
async def test_definitely_unsent_connections_remain_infrastructure_failures(error):
    def handler(request):
        raise error

    client, audit = model(handler)
    async with client:
        with pytest.raises(ModelInfrastructureError):
            await client.call("extract", {})
    assert audit.events[-1]["status"] == "failed"
    assert audit.events[-1]["error"]["type"] == "connection_before_send"


class ReplayAudit(Audit):
    def __init__(self):
        super().__init__()
        self.rows = {}

    async def begin_request(self, lease, request_id, model, kind, metadata=None):
        for row in self.rows.values():
            if row["metadata"]["request_digest"] == metadata["request_digest"] and row["kind"] == kind:
                if row["status"] in {"sent", "uncertain"}:
                    raise RequestReplayBlocked(row["request_id"], row["status"])
                if row["status"] == "succeeded":
                    return deepcopy(row)
        row = await super().begin_request(lease, request_id, model, kind, metadata)
        self.rows[request_id] = deepcopy(row)
        return row

    async def finish_request(self, request_id, status, usage=None, error=None, *, response_ref=None, activity=None):
        await super().finish_request(request_id, status, usage, error, response_ref=response_ref, activity=activity)
        self.rows[request_id].update(status=status, usage=usage, response_ref=response_ref, activity=activity)


async def test_complete_response_cache_reused_without_new_request_or_usage(tmp_path):
    artifacts, audit, calls = CountedArtifacts(tmp_path), ReplayAudit(), []

    def handler(request):
        calls.append(request)
        return response()

    first, _ = model(handler, audit, artifacts=artifacts)
    async with first:
        assert await first.call("extract", {}) == {"result": "ok"}
    events = len(audit.events)
    second, _ = model(handler, audit, artifacts=artifacts, values=environment(KNOWLEDGEBRAIN_CHAT_API_KEY="rotated"))
    async with second:
        assert await second.call("extract", {}) == {"result": "ok"}
    assert len(calls) == 1 and artifacts.writes == 1 and len(audit.events) == events
    assert second.usage.snapshot()["requests"] == 0


@pytest.mark.parametrize("corruption", ["file", "missing", "wrong_digest", "incomplete_stream", "invalid_json", "no_artifact_store"])
async def test_corrupt_cache_fails_without_repaying(tmp_path, corruption):
    artifacts, audit, calls = CountedArtifacts(tmp_path), ReplayAudit(), []
    first, _ = model(lambda request: (calls.append(request), response())[1], audit, artifacts=artifacts)
    async with first:
        await first.call("extract", {})
    row = next(iter(audit.rows.values()))
    if corruption in {"file", "missing"}:
        path = artifacts.path(row["response_ref"])
        if corruption == "missing":
            path.unlink()
        else:
            path.chmod(0o600)
            path.write_bytes(b"corrupted")
    elif corruption != "no_artifact_store":
        saved = artifacts.read_json(row["response_ref"])
        if corruption == "wrong_digest":
            saved["request_digest"] = "0" * 64
        elif corruption == "incomplete_stream":
            saved["raw_body"] = event('{"ok":true}').decode()
        else:
            saved["raw_body"] = (event('{"ok":') + event(delta={}, finish="stop") + DONE).decode()
        row["response_ref"] = artifacts.put_json(saved)
    second, _ = model(lambda request: calls.append(request), audit,
                      artifacts=None if corruption == "no_artifact_store" else artifacts)
    async with second:
        with pytest.raises(ModelSemanticError, match="cached model response"):
            await second.call("extract", {})
    assert len(calls) == 1


async def test_sent_or_uncertain_recovery_never_replays_transport(tmp_path):
    calls, audit = [], ReplayAudit()

    def handler(request):
        calls.append(request)
        raise httpx.ReadError("lost response")

    client, _ = model(handler, audit, artifacts=CountedArtifacts(tmp_path))
    async with client:
        with pytest.raises(UncertainModelResponse):
            await client.call("extract", {})
        with pytest.raises(RequestReplayBlocked):
            await client.call("extract", {})
    assert len(calls) == 1


@pytest.mark.parametrize("provider_type,code", [
    ("server_error", "request_timeout"),
    ("service_unavailable_error", "server_is_overloaded"),
])
@pytest.mark.parametrize("done", [False, True])
@pytest.mark.parametrize("event_prefix", ["", "event: error\n"])
async def test_provider_stream_errors_preserve_cause_without_authorizing_replay(tmp_path, provider_type, code, done, event_prefix):
    calls, audit, artifacts = [], ReplayAudit(), CountedArtifacts(tmp_path)
    error = {"error": {"type": provider_type, "code": code, "message": "private provider detail"}}
    body = event('{"partial":') + (event_prefix + "data: " + json.dumps(error) + "\n\n").encode() + (DONE if done else b"")

    def handler(request):
        calls.append(request)
        return httpx.Response(200, content=body, headers={"content-type": "text/event-stream"})

    client, _ = model(handler, audit, artifacts=artifacts)
    async with client:
        with pytest.raises(UncertainModelResponse):
            await client.call("extract", {})
        with pytest.raises(RequestReplayBlocked):
            await client.call("extract", {})
    terminal = audit.events[-1]
    assert terminal["status"] == "uncertain"
    assert terminal["error"]["type"] == "provider_stream_error"
    assert terminal["error"]["provider_error"] == {"type": provider_type, "code": code}
    assert "private provider detail" not in json.dumps(terminal["error"])
    assert artifacts.read_json(terminal["response_ref"])["provider_error"] == {"type": provider_type, "code": code}
    assert len(calls) == artifacts.writes == 1


async def test_provider_error_identifiers_reject_prose_and_redact_credentials(tmp_path):
    artifacts = CountedArtifacts(tmp_path)
    body = {"error": {"type": "request included private details", "code": "synthetic-secret"}}
    client, audit = model(lambda _: httpx.Response(200, content="data: " + json.dumps(body) + "\n\n",
                                                   headers={"content-type": "text/event-stream"}), artifacts=artifacts)
    async with client:
        with pytest.raises(UncertainModelResponse):
            await client.call("extract", {})
    terminal = audit.events[-1]
    assert terminal["error"]["provider_error"] == {"code": "[REDACTED]"}
    saved = artifacts.read_json(terminal["response_ref"])
    assert saved["provider_error"] == {"code": "[REDACTED]"}
    assert "synthetic-secret" not in json.dumps(saved)


@pytest.mark.parametrize("headers", [{}, {"content-type": "text/plain"}])
async def test_parsed_provider_error_retains_uncertainty_with_invalid_content_type(tmp_path, headers):
    calls, audit, artifacts = [], ReplayAudit(), CountedArtifacts(tmp_path)
    error_frame = b'data: {"error":{"type":"server_error","code":"request_timeout"}}\n\n'

    def handler(request):
        calls.append(request)
        return httpx.Response(200, content=error_frame + GOOD, headers=headers)

    client, _ = model(handler, audit, artifacts=artifacts)
    async with client:
        with pytest.raises(UncertainModelResponse):
            await client.call("extract", {})
        with pytest.raises(RequestReplayBlocked):
            await client.call("extract", {})
    terminal = audit.events[-1]
    assert terminal["status"] == "uncertain"
    assert terminal["error"]["type"] == "provider_stream_error"
    assert terminal["error"]["provider_error"] == {"type": "server_error", "code": "request_timeout"}
    assert len(calls) == artifacts.writes == 1


@pytest.mark.parametrize("headers", [{}, {"content-type": "text/plain"}])
async def test_invalid_content_type_without_provider_error_remains_semantic_failure(headers):
    client, audit = model(lambda _: httpx.Response(200, content=GOOD, headers=headers))
    async with client:
        with pytest.raises(ModelSemanticError):
            await client.call("extract", {})
    assert audit.events[-1]["status"] == "failed"
    assert audit.events[-1]["error"]["type"] == "invalid_stream_content_type"


@pytest.mark.parametrize("error_frame", [
    b"event: error\n\n",
    b"event: error\ndata: not JSON\n\n",
    b"event: error\ndata: \xff\n\n",
    b"event: error\ndata: {}\n\n",
    b"event: error\ndata: []\n\n",
    b"event: error\ndata: null\n\n",
    b"event: error\ndata: [DONE]\n\n",
    b'data: {"error":"private provider prose"}\n\n',
    b'data: {"error":null}\n\n',
])
async def test_explicit_provider_error_is_uncertain_even_without_diagnostic_object(tmp_path, error_frame):
    calls, audit, artifacts = [], ReplayAudit(), CountedArtifacts(tmp_path)

    def handler(request):
        calls.append(request)
        return httpx.Response(200, content=error_frame + GOOD, headers={"content-type": "text/event-stream"})

    client, _ = model(handler, audit, artifacts=artifacts)
    async with client:
        with pytest.raises(UncertainModelResponse):
            await client.call("extract", {})
        with pytest.raises(RequestReplayBlocked):
            await client.call("extract", {})
    terminal = audit.events[-1]
    assert terminal["status"] == "uncertain"
    assert terminal["error"]["type"] == "provider_stream_error"
    assert terminal["error"]["provider_error"] == {}
    assert artifacts.read_json(terminal["response_ref"])["provider_error"] == {}
    assert len(calls) == artifacts.writes == 1


def test_request_digest_binds_full_body_and_normalized_endpoint_without_keys():
    request = build_request("gpt-6-luna", "Read JSON", {"a": "中文"}, [ImageInput(b"img", "image/png")])
    base = request_digest("https://CHAT.invalid:443/v1", request)
    assert base == request_digest("https://chat.invalid/v1/chat/completions", dict(reversed(list(request.items()))))
    for field, value in [("model", "other"), ("stream", False), ("stream_options", {"include_usage": False})]:
        assert base != request_digest("https://chat.invalid/v1", {**request, field: value})
    changed = deepcopy(request)
    changed["messages"][1]["content"][1]["image_url"]["url"] += "a"
    assert base != request_digest("https://chat.invalid/v1", changed)
    assert base != request_digest("https://other.invalid/v1", request)


async def test_local_http_sse_heartbeat_and_body(tmp_path):
    seen = []

    async def handle(reader, writer):
        headers = await reader.readuntil(b"\r\n\r\n")
        length = next(int(line.split(b":", 1)[1]) for line in headers.split(b"\r\n")
                      if line.lower().startswith(b"content-length:"))
        seen.append(json.loads(await reader.readexactly(length)))
        writer.write(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n")
        for chunk in [b": local heartbeat\n\n", event('{"ok":true}'), event(delta={}, finish="stop"), DONE]:
            writer.write(chunk)
            await writer.drain()
            await asyncio.sleep(0)
        writer.close()
        await writer.wait_closed()

    server = await asyncio.start_server(handle, "127.0.0.1", 0)
    port = server.sockets[0].getsockname()[1]
    artifacts, audit = CountedArtifacts(tmp_path), Audit()
    client = OpenAIModel(ModelConfig.from_env(environment(KNOWLEDGEBRAIN_CHAT_BASE_URL=f"http://127.0.0.1:{port}/v1")),
                         audit, object(), {"extract": "Return JSON"}, artifacts=artifacts)
    async with server, client:
        assert await client.call("extract", {}) == {"ok": True}
    assert len(seen) == 1 and seen[0]["stream"] is True and seen[0]["stream_options"]["include_usage"] is True
    saved = artifacts.read_json(audit.events[-1]["response_ref"])
    assert saved["activity"]["heartbeat_events"] == 1
    assert saved["activity"]["raw_received_chunks"] >= 1


@pytest.mark.parametrize("with_partial", [False, True])
async def test_real_wait_for_enforces_idle_on_a_stalled_stream(tmp_path, with_partial):
    class StalledStream(httpx.AsyncByteStream):
        async def __aiter__(self):
            if with_partial:
                yield b": heartbeat\n\n"
            await asyncio.Event().wait()

    client, audit = model(lambda _: httpx.Response(200, stream=StalledStream(),
                                                   headers={"content-type": "text/event-stream"}),
                          timeout_seconds=0.02, artifacts=CountedArtifacts(tmp_path))
    async with client:
        with pytest.raises(UncertainModelResponse if with_partial else ModelInfrastructureError):
            await client.call("extract", {})
    expected = "between_data_idle_timeout" if with_partial else "first_data_idle_timeout"
    assert audit.events[-1]["error"]["type"] == expected
    assert audit.events[-1]["status"] == ("uncertain" if with_partial else "failed")


async def test_cancellation_preserves_received_partial_stream_once(tmp_path):
    receiving = asyncio.Event()
    partial = event('{"partial":')

    class StalledStream(httpx.AsyncByteStream):
        async def __aiter__(self):
            yield partial
            receiving.set()
            await asyncio.Event().wait()

    artifacts = CountedArtifacts(tmp_path)
    client, audit = model(lambda _: httpx.Response(200, stream=StalledStream(),
                                                   headers={"content-type": "text/event-stream"}), artifacts=artifacts)
    async with client:
        task = asyncio.create_task(client.call("extract", {}))
        await receiving.wait()
        task.cancel()
        with pytest.raises(asyncio.CancelledError):
            await task
    terminal = audit.events[-1]
    assert terminal["status"] == "uncertain" and terminal["error"]["type"] == "cancelled_after_send"
    assert artifacts.writes == 1 and artifacts.read_json(terminal["response_ref"])["raw_body"] == partial.decode()


@pytest.mark.parametrize("task", ["extract", "verify", "plan", "ocr"])
async def test_every_domain_task_uses_the_same_streaming_builder(task):
    requests = []
    client = OpenAIModel(ModelConfig.from_env(environment()), Audit(), object(),
                         {kind: "Return JSON" for kind in ("extract", "verify", "plan", "ocr")},
                         transport=httpx.MockTransport(lambda request: (requests.append(json.loads(request.content)), response())[1]))
    async with client:
        assert await client.call(task, {}) == {"result": "ok"}
        assert all(sdk.max_retries == 0 for sdk in client._clients.values())
    assert requests[0]["stream"] is True and requests[0]["stream_options"] == {"include_usage": True}


@pytest.mark.parametrize("prefix,error_class,failure", [
    (b"event: error\n\n", UncertainModelResponse, "provider_stream_error"),
    (b"data: invalid JSON\n\n", ModelSemanticError, "invalid_stream_envelope"),
])
async def test_protocol_error_cannot_be_overridden_by_later_valid_completion(monkeypatch, prefix, error_class, failure):
    client, audit, _, _ = timed_model(monkeypatch, [(1, prefix), (1, GOOD)])
    async with client:
        with pytest.raises(error_class):
            await client.call("extract", {})
    assert audit.events[-1]["error"]["type"] == failure
