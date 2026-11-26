"""Bounded concurrency for knowledge retrieval preserves order, specs, and retry budgets."""

from __future__ import annotations

import asyncio
import json
from types import SimpleNamespace

import httpx
import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.integrations.authoring_knowledge import KnowledgeProvider
from docreader.integrations.web_knowledge import build_web_knowledge
from docreader.tender.document_execution import _FrozenKnowledge


PRODUCT = "4c78bf65-9ee2-4788-bc0c-f14769bb153f"
VERSIONS = [
    "11111111-1111-4111-8111-111111111111",
    "22222222-2222-4222-8222-222222222222",
    "33333333-3333-4333-8333-333333333333",
    "44444444-4444-4444-8444-444444444444",
]
SHARED_DOC = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"


class ReceiptStore:
    def __init__(self):
        self.rows = {}

    async def receipt(self, lease, stage, scope, *, input_digest):
        return self.rows.get((lease.execution_id, stage, scope, input_digest))

    async def commit_artifact(self, lease, stage, reference, *, scope, input_digest):
        self.rows[(lease.execution_id, stage, scope, input_digest)] = {"artifact_ref": reference}


def _hit(index: int, *, document_id: str | None = None, version_id: str | None = None) -> dict:
    return {
        "id": f"chunk-{index}",
        "content": f"依据片段 {index}",
        "document_id": document_id or f"doc-{index}",
        "document_title": f"文档 {index}",
        "version_id": version_id or "version-1",
        "version_label": "V1",
        "product_id": PRODUCT,
    }


async def test_knowledge_provider_runs_queries_with_bounded_concurrency_and_deterministic_order(tmp_path):
    queries = [f"query-{i}" for i in range(6)]
    delays = {q: 0.06 - i * 0.01 for i, q in enumerate(queries)}
    active = 0
    max_active = 0
    calls = 0

    async def handler(request):
        nonlocal active, max_active, calls
        calls += 1
        if request.url.path == "/api/v1/search":
            body = json.loads(request.content)
            q = body["query"]
            active += 1
            max_active = max(max_active, active)
            try:
                await asyncio.sleep(delays[q])
            finally:
                active -= 1
            index = queries.index(q)
            return httpx.Response(200, json={"hits": [_hit(index)], "warnings": []})
        return httpx.Response(404)

    for bad in (0, -1, True):
        with pytest.raises(ValueError):
            KnowledgeProvider(
                base_url="https://knowledge.invalid",
                bearer="test-secret",
                product_id=PRODUCT,
                concurrency=bad,
            )

    p1 = KnowledgeProvider(
        base_url="https://knowledge.invalid",
        bearer="test-secret",
        product_id=PRODUCT,
        transport=httpx.MockTransport(handler),
        concurrency=1,
    )
    res1 = await p1.search(
        [{"target_id": f"t-{i}", "query": q, "query_id": f"qid-{i}"} for i, q in enumerate(queries)],
        ArtifactStore(tmp_path / "c1"),
    )
    assert max_active == 1
    assert calls == 6

    active = max_active = calls = 0
    p3 = KnowledgeProvider(
        base_url="https://knowledge.invalid",
        bearer="test-secret",
        product_id=PRODUCT,
        transport=httpx.MockTransport(handler),
        concurrency=3,
    )
    res3 = await p3.search(
        [{"target_id": f"t-{i}", "query": q, "query_id": f"qid-{i}"} for i, q in enumerate(queries)],
        ArtifactStore(tmp_path / "c3"),
    )
    assert max_active == 3
    assert calls == 6
    assert p1.spec() == p3.spec()
    assert [m["text"] for m in res3["materials"]] == [f"依据片段 {i}" for i in range(6)]
    assert [m["text"] for m in res3["materials"]] == [m["text"] for m in res1["materials"]]
    assert [m["locator"]["target_ids"] for m in res3["materials"]] == [[f"t-{i}"] for i in range(6)]
    assert [m["locator"]["searches"][0]["query"] for m in res3["materials"]] == queries


async def test_web_knowledge_provider_runs_selections_with_bounded_concurrency(tmp_path):
    delays = {version: 0.05 - i * 0.01 for i, version in enumerate(VERSIONS)}
    active = 0
    max_active = 0
    search_calls = 0

    async def handler(request):
        nonlocal active, max_active, search_calls
        command = json.loads(request.content)
        if command["operation"] == "search":
            search_calls += 1
            active += 1
            max_active = max(max_active, active)
            try:
                await asyncio.sleep(delays[command["version_id"]])
            finally:
                active -= 1
            index = VERSIONS.index(command["version_id"])
            return httpx.Response(200, json={"hits": [_hit(index, version_id=command["version_id"])]})
        return httpx.Response(404)

    selections = [{"product_id": PRODUCT, "version_id": version} for version in VERSIONS]
    p1 = build_web_knowledge(
        execution_id="exec-1",
        selections=selections,
        base_url="https://api.invalid",
        service_secret="secret",
        transport=httpx.MockTransport(handler),
        concurrency=1,
    )
    p2 = build_web_knowledge(
        execution_id="exec-1",
        selections=selections,
        base_url="https://api.invalid",
        service_secret="secret",
        transport=httpx.MockTransport(handler),
        concurrency=2,
    )
    assert p1 is not None and p2 is not None
    assert p1.spec() == p2.spec()

    result = await p2.search([{"target_id": "t1", "query": "权限控制"}], ArtifactStore(tmp_path))
    assert max_active == 2
    assert search_calls == 4
    assert [m["locator"]["version_id"] for m in result["materials"]] == sorted(VERSIONS)


async def test_frozen_knowledge_runs_queries_concurrently_without_changing_receipts_or_retry_budget(
    tmp_path, monkeypatch
):
    real_sleep = asyncio.sleep

    async def no_sleep(seconds):
        if seconds >= 1:
            return None
        await real_sleep(seconds)

    monkeypatch.setattr("docreader.tender.document_execution.asyncio.sleep", no_sleep)
    monkeypatch.setattr("docreader.integrations.authoring_knowledge.asyncio.sleep", no_sleep)

    queries = [f"fq-{i}" for i in range(5)]
    delays = {q: 0.05 - i * 0.01 for i, q in enumerate(queries)}
    active = 0
    max_active = 0
    doc_attempts = 0

    async def handler(request):
        nonlocal active, max_active, doc_attempts
        if request.url.path == "/api/v1/search":
            q = json.loads(request.content)["query"]
            active += 1
            max_active = max(max_active, active)
            try:
                await real_sleep(delays.get(q, 0.01))
            finally:
                active -= 1
            if q.startswith("shared-fail"):
                return httpx.Response(200, json={"hits": [_hit(0, document_id=SHARED_DOC, version_id=VERSIONS[0])], "warnings": []})
            return httpx.Response(200, json={"hits": [_hit(queries.index(q))], "warnings": []})
        if request.url.path == f"/api/v1/documents/{SHARED_DOC}":
            doc_attempts += 1
            return httpx.Response(503)
        return httpx.Response(404)

    artifacts = ArtifactStore(tmp_path / "snap")
    store = ReceiptStore()
    inner = KnowledgeProvider(
        base_url="https://knowledge.invalid",
        bearer="test-secret",
        product_id=PRODUCT,
        transport=httpx.MockTransport(handler),
        concurrency=1,
    )
    reader = _FrozenKnowledge(
        inner,
        inner.spec(),
        store,
        artifacts,
        SimpleNamespace(execution_id="exec-frozen"),
        concurrency=3,
    )
    entries = [{"target_id": f"t-{i}", "query": q, "query_id": f"fqid-{i}"} for i, q in enumerate(queries)]
    first = await reader.search(entries, artifacts)
    assert max_active == 3
    snapshots = [k for k in store.rows if k[1] == "knowledge-snapshot"]
    assert len(snapshots) == 5
    assert [m["text"] for m in first["materials"]] == [f"依据片段 {i}" for i in range(5)]

    max_active = 0
    reused = await reader.search(entries, artifacts)
    assert max_active == 0
    assert [m["text"] for m in reused["materials"]] == [f"依据片段 {i}" for i in range(5)]

    # Concurrent queries sharing one failing document must share one 4-attempt budget via _operation_locks.
    doc_attempts = 0
    shared = await reader.search(
        [
            {"target_id": "s1", "query": "shared-fail-a"},
            {"target_id": "s2", "query": "shared-fail-b"},
        ],
        artifacts,
    )
    assert doc_attempts == 4
    assert len(shared["materials"]) == 1
    assert shared["materials"][0]["locator"]["target_ids"] == ["s1", "s2"]
