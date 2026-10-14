"""Actual HTTP decoding, PostgreSQL send fences and domain receipt recovery."""

import json
import zlib

import httpx
import pytest

from docreader.integrations.store import RequestReplayBlocked
from docreader.llm import ModelConfig, OpenAIModel, UncertainModelResponse
from docreader.llm.config import ModelEndpoint
from docreader.tender.requirements import extract_pack
from docreader.workflow.nodes import TenderNodes
from test_request_replay import leased
from test_store import expire, store as store

pytestmark = pytest.mark.asyncio


def event(content=None, *, finish=None, usage=None):
    body = ({"choices": [], "usage": usage} if usage is not None else
            {"choices": [{"index": 0, "delta": {"content": content} if content is not None else {},
                          "finish_reason": finish}]})
    return ("data: " + json.dumps(body) + "\n\n").encode()


class ResponseChunks(httpx.AsyncByteStream):
    def __init__(self, chunks):
        self.chunks, self.closed = chunks, False

    async def __aiter__(self):
        for chunk in self.chunks:
            yield chunk

    async def aclose(self):
        self.closed = True


def partial_response(partial, ending):
    headers = {"content-type": "text/event-stream"}
    chunks = [partial]
    if ending == "gzip_crc":
        compressor = zlib.compressobj(wbits=31)
        prefix = compressor.compress(partial) + compressor.flush(zlib.Z_SYNC_FLUSH)
        trailer = compressor.flush()
        # The first HTTP chunk decodes a real SSE prefix. The next chunk fails
        # the gzip checksum in httpx, without injecting a fake SDK exception.
        corrupt = trailer[:-8] + bytes([trailer[-8] ^ 255]) + trailer[-7:]
        chunks = [prefix, corrupt]
        headers["content-encoding"] = "gzip"
    source = ResponseChunks(chunks)
    return httpx.Response(200, stream=source, headers=headers), source


def nodes_for(store, lease, handler):
    endpoint = ModelEndpoint("https://fixture.invalid/v1", "synthetic-secret", "gpt-6-luna", 131072)
    model = OpenAIModel(ModelConfig(endpoint, None), store, lease, {"extract": "Return JSON evidence."},
                        artifacts=store.test_artifacts, transport=httpx.MockTransport(handler))
    return TenderNodes(store, store.test_artifacts, lease, model)


async def save_extraction(nodes):
    text = "Procurement notice"
    pack = {"pack_id": "pack-1", "body": [{"range_id": "range-1", "document_id": "main",
            "unit_id": "unit-1", "kind": "paragraph", "text": text, "start": 0, "end": len(text),
            "locator": {"paragraph": 0}}], "supporting": [], "issues": []}
    return await nodes._save("extract-pack", lambda: extract_pack(pack, nodes),
                             scope=pack["pack_id"], inputs=pack, model=True)


@pytest.mark.parametrize("ending", ["eof", "gzip_crc"])
@pytest.mark.parametrize("usage", [None, {"prompt_tokens": 7, "completion_tokens": 2, "total_tokens": 9}])
async def test_partial_response_cannot_reach_failed_domain_commit_or_repay(store, monkeypatch, ending, usage):
    run, command, old = await leased(store)
    partial = event('{"requirements":') + (event(usage=usage) if usage else b"")
    calls, streams, commits = [], [], []

    def handler(request):
        calls.append(request)
        response, source = partial_response(partial, ending)
        streams.append(source)
        return response

    async def broken_domain_commit(*args, **kwargs):
        commits.append(args)
        raise ConnectionError("injected domain receipt commit failure")

    monkeypatch.setattr(store, "commit_artifact", broken_domain_commit)
    first = nodes_for(store, old, handler)
    try:
        with pytest.raises(UncertainModelResponse, match="billing are uncertain"):
            await save_extraction(first)
    finally:
        await first.close()
    # The old decoding-as-semantic path attempted this failing domain commit,
    # leaving a retryable failed audit. Uncertainty must bypass it altogether.
    assert not commits and streams[0].closed
    before = await store.get_run(run["run_id"])
    assert len(before["usage"]) == 1 and not before["artifacts"]
    request = before["usage"][0]
    assert request["status"] == "uncertain" and request["usage"] == usage
    assert request["error"]["type"] == ("invalid_stream_encoding" if ending == "gzip_crc" else "stream_missing_done")
    saved = store.test_artifacts.read_json(request["response_ref"])
    assert saved["raw_body"] == partial.decode() and saved["done"] is False
    assert saved["usage_status"] == ("unknown" if usage is None else "reported")
    assert saved["activity"]["raw_received_chunks"] == (2 if ending == "gzip_crc" else 1)

    await expire(store, run["run_id"])
    new = await store.acquire(run["run_id"], command["command_id"], 1)
    resumed = nodes_for(store, new, handler)
    try:
        with pytest.raises(RequestReplayBlocked) as blocked:
            await save_extraction(resumed)
    finally:
        await resumed.close()
    assert blocked.value.status == "uncertain"
    assert len(calls) == 1 and not commits
    after = await store.get_run(run["run_id"])
    assert after["usage"] == before["usage"] and not after["artifacts"]
    assert await store.receipt(new, "extract-pack", "pack-1") is None


@pytest.mark.parametrize("fault", ["response_blob", "audit_before_commit", "audit_after_commit"])
async def test_decoding_uncertainty_survives_audit_failure_and_recovery(store, monkeypatch, fault):
    run, command, old = await leased(store)
    calls = []
    partial = event('{"requirements":')

    def handler(request):
        calls.append(request)
        return partial_response(partial, "gzip_crc")[0]

    finish, put = store.finish_request, store.test_artifacts.put_json
    writes, faults = [], []

    def broken_put(value):
        writes.append(value)
        if fault == "response_blob":
            faults.append(fault)
            raise OSError("injected response artifact failure")
        return put(value)

    async def broken_finish(request_id, status, *args, **kwargs):
        if status == "uncertain":
            faults.append(fault)
            if fault == "audit_after_commit":
                await finish(request_id, status, *args, **kwargs)
            raise ConnectionError("injected audit commit failure")
        return await finish(request_id, status, *args, **kwargs)

    with monkeypatch.context() as patched:
        patched.setattr(store.test_artifacts, "put_json", broken_put)
        patched.setattr(store, "finish_request", broken_finish)
        first = nodes_for(store, old, handler)
        try:
            with pytest.raises(UncertainModelResponse, match="decoding and audit failed"):
                await save_extraction(first)
        finally:
            await first.close()
    assert faults == [fault] and len(writes) == 1
    assert writes[0]["raw_body"] == partial.decode() and writes[0]["usage_status"] == "unknown"
    before = await store.get_run(run["run_id"])
    assert len(before["usage"]) == 1 and not before["artifacts"]
    request = before["usage"][0]
    assert request["status"] == ("uncertain" if fault == "audit_after_commit" else "sent")
    assert request["usage"] is None
    assert bool(request["response_ref"]) == (fault == "audit_after_commit")

    await expire(store, run["run_id"])
    new = await store.acquire(run["run_id"], command["command_id"], 1)
    resumed = nodes_for(store, new, handler)
    try:
        with pytest.raises(RequestReplayBlocked):
            await save_extraction(resumed)
    finally:
        await resumed.close()
    after = await store.get_run(run["run_id"])
    assert len(calls) == len(after["usage"]) == 1 and not after["artifacts"]
    assert after["usage"][0]["status"] == "uncertain"
    assert after["usage"][0]["usage"] is None


async def test_complete_response_reused_after_actual_domain_receipt_commit_failure(store, monkeypatch):
    run, command, old = await leased(store)
    calls, commits = [], []
    result = {"requirements": [], "dispositions": [{"range_id": "range-1", "status": "non_requirement",
              "reason": "This is an informational document title.", "requirement_ids": []}], "issues": []}
    complete = event(json.dumps(result)) + event(finish="stop") + b"data: [DONE]\n\n"

    def handler(request):
        calls.append(request)
        return httpx.Response(200, stream=ResponseChunks([complete]), headers={"content-type": "text/event-stream"})

    async def broken_commit(*args, **kwargs):
        commits.append(args)
        raise ConnectionError("injected domain receipt commit failure")

    with monkeypatch.context() as patched:
        patched.setattr(store, "commit_artifact", broken_commit)
        first = nodes_for(store, old, handler)
        try:
            with pytest.raises(ConnectionError, match="domain receipt"):
                await save_extraction(first)
        finally:
            await first.close()
    assert len(commits) == 1
    before = await store.get_run(run["run_id"])
    assert len(before["usage"]) == 1 and before["usage"][0]["status"] == "succeeded"
    assert before["usage"][0]["usage"] is None and not before["artifacts"]

    await expire(store, run["run_id"])
    new = await store.acquire(run["run_id"], command["command_id"], 1)
    resumed = nodes_for(store, new, handler)
    try:
        reference = await save_extraction(resumed)
        assert await save_extraction(resumed) == reference
    finally:
        await resumed.close()
    extracted = store.test_artifacts.read_json(reference)
    assert not extracted["issues"] and extracted["dispositions"] == result["dispositions"]
    after = await store.get_run(run["run_id"])
    assert len(calls) == 1 and after["usage"] == before["usage"]
    assert len(after["artifacts"]) == 1
    receipt = after["artifacts"][0]
    assert (receipt["stage"], receipt["scope"], receipt["artifact_ref"]) == ("extract-pack", "pack-1", reference)
