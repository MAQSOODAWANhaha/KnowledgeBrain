"""Physical knowledge HTTP operations keep one durable retry budget."""

import asyncio
from collections import Counter
import hashlib
import json
from types import SimpleNamespace

import httpx
import pytest

from docreader.integrations.artifacts import ArtifactIntegrityError, ArtifactStore
from docreader.integrations.store import Cancelled, Conflict, StaleExecution
from docreader.integrations.web_knowledge import build_web_knowledge
from docreader.tender.document_execution import _FrozenKnowledge


PRODUCT = "4c78bf65-9ee2-4788-bc0c-f14769bb153f"
VERSION = "37f9257d-f030-46cb-ae5b-e34dfebcb14b"
OTHER = "d9341729-5107-40ed-83be-9da5566c15c7"
DOCUMENT = "b78d5270-5938-48c2-acb7-e9538910fea7"
RAW = b"synthetic proof original"
OBJECT = "objects/" + hashlib.sha256(RAW).hexdigest()


class Receipts:
    def __init__(self):
        self.rows = {}
        self.fail_stage = None
        self.fail_suffix = None
        self.failure = OSError("injected persistence failure")

    async def receipt(self, lease, stage, scope, *, input_digest):
        return self.rows.get((lease.execution_id, stage, scope, input_digest))

    async def commit_artifact(self, lease, stage, reference, *, scope, input_digest):
        key = lease.execution_id, stage, scope, input_digest
        row = {"artifact_ref": reference}
        if key in self.rows and self.rows[key] != row:
            raise Conflict("immutable receipt changed")
        if stage == self.fail_stage and (not self.fail_suffix or scope.endswith(self.fail_suffix)):
            raise self.failure
        self.rows[key] = row


def hit(version=VERSION):
    return {"id": "chunk", "content": "测试功能依据", "document_id": DOCUMENT,
            "product_id": PRODUCT, "version_id": version, "document_title": "测试原件"}


def source(handler, store, artifacts, versions=(VERSION,)):
    provider = build_web_knowledge(execution_id="execution", selections=[
        {"product_id": PRODUCT, "version_id": version} for version in versions],
        base_url="https://knowledge.invalid", service_secret="synthetic-secret",
        transport=httpx.MockTransport(handler))
    return _FrozenKnowledge(provider, provider.spec(), store, artifacts,
                            SimpleNamespace(execution_id="execution"))


def query(*, original=False, target="target"):
    return [{"target_id": target, "query": "完整要求", "include_original": original}]


def response(command, *, empty=False):
    if command["operation"] == "search":
        return httpx.Response(200, json={"hits": [] if empty else [hit(command["version_id"])]})
    if command["operation"] == "document":
        return httpx.Response(200, json={"id": DOCUMENT, "product_version_id": VERSION,
                                       "file_name": "proof.pdf", "object_ref": OBJECT})
    return httpx.Response(200, content=RAW)


@pytest.fixture(autouse=True)
def no_retry_delay(monkeypatch):
    async def no_wait(_seconds):
        return None
    monkeypatch.setattr("docreader.integrations.authoring_knowledge.asyncio.sleep", no_wait)


async def test_real_web_adapter_has_four_physical_attempts_not_sixteen(tmp_path):
    calls = []
    async def handler(request):
        calls.append(json.loads(request.content))
        return httpx.Response(503)
    artifacts, store = ArtifactStore(tmp_path), Receipts()
    reader = source(handler, store, artifacts)
    result = await reader.search(query(), artifacts)
    assert len(calls) == 4
    assert {issue["code"] for issue in result["issues"]} == {"knowledge_unavailable"}
    starts = [key for key in store.rows if key[1] == "knowledge-operation" and key[2].endswith(":start")]
    outcomes = [key for key in store.rows if key[1] == "knowledge-operation" and key[2].endswith(":outcome")]
    assert len(starts) == len(outcomes) == 4


async def test_frozen_spec_preserves_actual_fixed_selections(tmp_path):
    artifacts, store = ArtifactStore(tmp_path), Receipts()
    async def handler(request):
        return response(json.loads(request.content))
    reader = source(handler, store, artifacts)
    assert reader.spec()["selections"] == [{"product_id": PRODUCT, "version_id": VERSION}]
    changed = reader.spec()
    changed["selections"].clear()
    assert reader.spec()["selections"]


async def test_successful_scope_is_not_retried_while_failed_sibling_uses_four_attempts(tmp_path):
    calls = Counter()
    async def handler(request):
        command = json.loads(request.content)
        calls[command["version_id"], command["operation"]] += 1
        return response(command) if command["version_id"] == VERSION else httpx.Response(503)
    artifacts, store = ArtifactStore(tmp_path), Receipts()
    result = await source(handler, store, artifacts, (VERSION, OTHER)).search(query(), artifacts)
    assert calls == {(VERSION, "search"): 1, (VERSION, "document"): 1, (OTHER, "search"): 4}
    assert len(result["materials"]) == 1
    assert any(issue["code"] == "knowledge_unavailable" for issue in result["issues"])


async def test_proof_upgrade_reuses_search_and_only_fetches_original_operations(tmp_path):
    calls = Counter()
    async def handler(request):
        command = json.loads(request.content)
        calls[command["operation"]] += 1
        return response(command)
    artifacts, store = ArtifactStore(tmp_path), Receipts()
    await source(handler, store, artifacts).search(query(), artifacts)
    result = await source(handler, store, artifacts).search(query(original=True), artifacts)
    assert calls == {"search": 1, "document": 1, "file": 1}
    assert artifacts.read_bytes(result["materials"][0]["original_ref"]) == RAW


@pytest.mark.parametrize("empty", [False, True])
async def test_committed_operation_survives_failure_before_aggregate_snapshot(tmp_path, empty):
    calls = []
    async def handler(request):
        command = json.loads(request.content)
        calls.append(command["operation"])
        return response(command, empty=empty)
    artifacts, store = ArtifactStore(tmp_path), Receipts()
    store.fail_stage = "knowledge-snapshot"
    with pytest.raises(OSError, match="persistence"):
        await source(handler, store, artifacts).search(query(), artifacts)
    store.fail_stage = None
    result = await source(handler, store, artifacts).search(query(target="resumed"), artifacts)
    assert calls == (["search"] if empty else ["search", "document"])
    assert bool(result["materials"]) is not empty


@pytest.mark.parametrize("kind", ["search", "document", "file"])
async def test_unknown_second_attempt_is_charged_across_resume(tmp_path, kind):
    calls = Counter()
    async def handler(request):
        command = json.loads(request.content)
        operation = command["operation"]
        calls[operation] += 1
        if operation != kind:
            return response(command)
        if calls[operation] == 2:
            raise RuntimeError("simulated process interruption")
        return httpx.Response(503)
    artifacts, store = ArtifactStore(tmp_path), Receipts()
    with pytest.raises(RuntimeError, match="interruption"):
        await source(handler, store, artifacts).search(query(original=True), artifacts)
    result = await source(handler, store, artifacts).search(query(original=True), artifacts)
    assert calls[kind] == 4
    assert calls["search"] == (4 if kind == "search" else 1)
    assert bool(result["materials"]) is (kind != "search")
    await source(handler, store, artifacts).search(query(original=True), artifacts)
    assert calls[kind] == 4


async def test_same_operation_concurrent_calls_share_one_send(tmp_path):
    calls = []
    async def handler(request):
        command = json.loads(request.content)
        calls.append(command["operation"])
        await asyncio.sleep(0)
        return response(command)
    artifacts, store = ArtifactStore(tmp_path), Receipts()
    reader = source(handler, store, artifacts)
    first, second = await asyncio.gather(reader.search(query(target="a"), artifacts),
                                         reader.search(query(target="b"), artifacts))
    assert calls == ["search", "document"]
    assert first["materials"][0]["locator"]["target_ids"] == ["a"]
    assert second["materials"][0]["locator"]["target_ids"] == ["b"]


@pytest.mark.parametrize("failure", [asyncio.CancelledError(), Cancelled("cancelled"),
                                    StaleExecution("lost lease"), OSError("disk full")])
async def test_control_and_storage_errors_do_not_become_business_retries(tmp_path, failure):
    calls = []
    async def handler(request):
        calls.append(request)
        return response(json.loads(request.content))
    artifacts, store = ArtifactStore(tmp_path), Receipts()
    store.fail_stage, store.fail_suffix, store.failure = "knowledge-operation", ":outcome", failure
    with pytest.raises(type(failure)):
        await source(handler, store, artifacts).search(query(), artifacts)
    assert len(calls) == 1


async def test_result_not_committed_is_unknown_and_may_be_sent_again(tmp_path):
    calls = []
    async def handler(request):
        command = json.loads(request.content)
        calls.append(command["operation"])
        return response(command)
    artifacts, store = ArtifactStore(tmp_path), Receipts()
    store.fail_stage, store.fail_suffix = "knowledge-operation", ":outcome"
    with pytest.raises(OSError):
        await source(handler, store, artifacts).search(query(), artifacts)
    store.fail_stage = None
    await source(handler, store, artifacts).search(query(), artifacts)
    assert calls == ["search", "search", "document"]


async def test_start_committed_before_send_crash_consumes_one_attempt(tmp_path):
    calls = []
    artifacts, store = ArtifactStore(tmp_path), Receipts()
    original_commit = store.commit_artifact
    async def interrupt_after_start(lease, stage, reference, *, scope, input_digest):
        await original_commit(lease, stage, reference, scope=scope, input_digest=input_digest)
        if stage == "knowledge-operation" and scope.endswith(":start"):
            raise OSError("interrupted before HTTP send")
    store.commit_artifact = interrupt_after_start
    async def handler(request):
        calls.append(request)
        return httpx.Response(503)
    with pytest.raises(OSError, match="before HTTP"):
        await source(handler, store, artifacts).search(query(), artifacts)
    assert not calls
    store.commit_artifact = original_commit
    await source(handler, store, artifacts).search(query(), artifacts)
    assert len(calls) == 3


@pytest.mark.parametrize("failure", [OSError("disk full"), ArtifactIntegrityError("corrupt content address")])
async def test_excerpt_persistence_failure_keeps_remote_result_without_retrying_http(tmp_path, monkeypatch, failure):
    calls = []
    async def handler(request):
        command = json.loads(request.content)
        calls.append(command["operation"])
        return response(command)
    artifacts, store = ArtifactStore(tmp_path), Receipts()
    put_bytes = artifacts.put_bytes
    def save(data, media_type="application/octet-stream"):
        if media_type == "text/plain; charset=utf-8":
            raise failure
        return put_bytes(data, media_type)
    with monkeypatch.context() as patch:
        patch.setattr(artifacts, "put_bytes", save)
        with pytest.raises(type(failure)):
            await source(handler, store, artifacts).search(query(), artifacts)
    result = await source(handler, store, artifacts).search(query(), artifacts)
    assert calls == ["search", "document"]
    assert result["materials"]


async def test_original_cas_integrity_error_is_not_retried_as_response_validation(tmp_path, monkeypatch):
    calls = Counter()
    async def handler(request):
        command = json.loads(request.content)
        calls[command["operation"]] += 1
        return response(command)
    artifacts, store = ArtifactStore(tmp_path), Receipts()
    put_bytes = artifacts.put_bytes
    def save(data, media_type="application/octet-stream"):
        if media_type == "application/pdf":
            raise ArtifactIntegrityError("injected CAS integrity failure")
        return put_bytes(data, media_type)
    with monkeypatch.context() as patch:
        patch.setattr(artifacts, "put_bytes", save)
        with pytest.raises(ArtifactIntegrityError, match="CAS"):
            await source(handler, store, artifacts).search(query(original=True), artifacts)
    assert calls == {"search": 1, "document": 1, "file": 1}
    result = await source(handler, store, artifacts).search(query(original=True), artifacts)
    assert calls == {"search": 1, "document": 1, "file": 2}
    assert artifacts.read_bytes(result["materials"][0]["original_ref"]) == RAW
