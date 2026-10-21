"""Actual HTTP decoding, PostgreSQL send fences and domain receipt recovery."""

import json
import zlib

import httpx
import pytest

from docreader.integrations.store import RequestReplayBlocked
from docreader.llm import ModelConfig, OpenAIModel
from docreader.llm.config import ModelEndpoint
from docreader.llm.request import ModelWorkUnitExhausted
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
async def test_partial_responses_share_four_send_budget_without_domain_commit(store, monkeypatch, ending, usage):
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
        with pytest.raises(ModelWorkUnitExhausted):
            await save_extraction(first)
    finally:
        await first.close()
    assert not commits and len(calls) == 4 and all(stream.closed for stream in streams)
    before = await store.get_run(run["run_id"])
    assert len(before["usage"]) == 4 and not before["artifacts"]
    assert len({request["request_id"] for request in before["usage"]}) == 4
    for request in before["usage"]:
        assert request["sent_at"] and request["status"] == "uncertain" and request["usage"] == usage
        assert request["metadata"]["command_id"] == command["command_id"]
        assert request["metadata"]["logical_request_digest"] == request["metadata"]["request_digest"]
        assert request["error"]["type"] == ("invalid_stream_encoding" if ending == "gzip_crc" else "stream_missing_done")
        saved = store.test_artifacts.read_json(request["response_ref"])
        assert saved["raw_body"] == partial.decode() and saved["done"] is False
        assert saved["usage"] == usage
        assert saved["usage_status"] == ("unknown" if usage is None else "reported")
        assert saved["activity"]["raw_received_chunks"] == (2 if ending == "gzip_crc" else 1)

    await expire(store, run["run_id"])
    new = await store.acquire(run["run_id"], command["command_id"], 1)
    resumed = nodes_for(store, new, handler)
    try:
        with pytest.raises(ModelWorkUnitExhausted):
            await save_extraction(resumed)
    finally:
        await resumed.close()
    assert len(calls) == 4 and not commits
    after = await store.get_run(run["run_id"])
    assert [row for row in after["usage"] if row["sent_at"]] == before["usage"]
    assert all(row["status"] == "prepared" for row in after["usage"] if not row["sent_at"])
    assert not after["artifacts"]
    assert await store.receipt(new, "extract-pack", "pack-1") is None


@pytest.mark.parametrize("fault", ["response_blob", "audit_before_commit", "audit_after_commit"])
async def test_decoding_uncertainty_survives_audit_failure_and_recovery(store, monkeypatch, fault):
    run, command, old = await leased(store)
    calls, streams = [], []
    partial = event('{"requirements":')

    def handler(request):
        calls.append(request)
        response, source = partial_response(partial, "gzip_crc")
        streams.append(source)
        return response

    finish, put = store.finish_request, store.test_artifacts.put_json
    writes, saved_refs, faults = [], [], []

    def broken_put(value):
        if value.get("protocol") == "chat.completions.sse.v1":
            writes.append(value)
            if fault == "response_blob":
                faults.append(fault)
                raise OSError("injected response artifact failure")
            reference = put(value)
            saved_refs.append(reference)
            return reference
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
            with pytest.raises(ModelWorkUnitExhausted) as stopped:
                await save_extraction(first)
        finally:
            await first.close()
    initial_calls = 4 if fault == "audit_after_commit" else 1
    assert len(calls) == initial_calls and faults == [fault] * initial_calls
    assert len(writes) == initial_calls and all(stream.closed for stream in streams)
    for envelope in writes:
        assert envelope["raw_body"] == partial.decode() and envelope["usage_status"] == "unknown"
        assert envelope["done"] is False and envelope["activity"]["raw_received_chunks"] == 2
    for reference in saved_refs:
        assert store.test_artifacts.read_json(reference)["raw_body"] == partial.decode()
    if fault != "audit_after_commit":
        assert isinstance(stopped.value.__cause__, RequestReplayBlocked)
        assert stopped.value.__cause__.status == "sent"  # Never send concurrently with the uncommitted audit.
    before = await store.get_run(run["run_id"])
    assert len(before["usage"]) == initial_calls and not before["artifacts"]
    for request in before["usage"]:
        assert request["status"] == ("uncertain" if fault == "audit_after_commit" else "sent")
        assert request["sent_at"] and request["usage"] is None
        assert bool(request["response_ref"]) == (fault == "audit_after_commit")

    await expire(store, run["run_id"])
    new = await store.acquire(run["run_id"], command["command_id"], 1)
    resumed = nodes_for(store, new, handler)
    try:
        with pytest.raises(ModelWorkUnitExhausted):
            await save_extraction(resumed)
    finally:
        await resumed.close()
    after = await store.get_run(run["run_id"])
    assert len(calls) == 4 and all(stream.closed for stream in streams) and not after["artifacts"]
    sent = [row for row in after["usage"] if row["sent_at"]]
    assert len(sent) == 4 and all(row["status"] == "uncertain" and row["usage"] is None for row in sent)
    for original, recovered in zip(before["usage"], sent):
        if fault == "audit_after_commit":
            assert recovered == original
        else:
            assert recovered["error"]["type"] == "executor_lost"
            for field in ("request_id", "sent_at", "response_ref", "metadata", "usage"):
                assert recovered[field] == original[field]
    for request in sent[initial_calls:]:
        saved = store.test_artifacts.read_json(request["response_ref"])
        assert saved["raw_body"] == partial.decode() and saved["done"] is False
    assert await store.receipt(new, "extract-pack", "pack-1") is None


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
