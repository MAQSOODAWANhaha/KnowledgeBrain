"""Knowledge retrieval freezes excerpts without certifying business suitability."""

import asyncio
import json

import httpx
import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.integrations.authoring_knowledge import KnowledgeProvider


PRODUCT = "4c78bf65-9ee2-4788-bc0c-f14769bb153f"


def hit(**changes):
    return {"id": "chunk-1", "content": "支持角色权限管理。", "document_id": "document-1",
            "document_title": "产品功能说明", "version_id": "version-1", "version_label": "V1",
            "product_id": PRODUCT, "start_at": 14, "end_at": 25, **changes}


def provider(handler, **changes):
    return KnowledgeProvider(base_url="https://knowledge.invalid", bearer="test-secret",
                             product_id=PRODUCT, transport=httpx.MockTransport(handler), **changes)


async def test_retrieval_deduplicates_queries_and_freezes_excerpts_as_references(tmp_path):
    requests = []

    async def handler(request):
        requests.append(request)
        assert request.url.path == "/api/v1/search"
        assert request.headers["authorization"] == "Bearer test-secret"
        return httpx.Response(200, json={"hits": [hit()], "warnings": []})

    artifacts = ArtifactStore(tmp_path)
    result = await provider(handler).search([
        {"target_id": "t1", "query": "权限管理"}, {"target_id": "t2", "query": "权限管理"},
        {"target_id": "t3", "query": "角色授权"}], artifacts)
    assert len(requests) == 2
    assert json.loads(requests[0].content)["product_id"] == PRODUCT
    assert json.loads(requests[0].content)["mode"] == "assembly"
    assert len(result["materials"]) == 1 and result["issues"] == []
    material = result["materials"][0]
    assert material["usage"] == "reference" and material["attachment"] is False
    assert artifacts.read_bytes(material["content_ref"]).decode() == material["text"]
    assert material["original_ref"] is None
    assert material["locator"]["target_ids"] == ["t1", "t2", "t3"]
    assert material["locator"]["kind"] == "search_excerpt"
    assert material["locator"]["document_id"] == "document-1"
    assert [entry["query"] for entry in material["locator"]["searches"]] == ["权限管理", "角色授权"]
    assert "test-secret" not in json.dumps(result)


async def test_absent_configuration_and_empty_results_are_nonblocking(tmp_path):
    calls = []

    async def handler(request):
        calls.append(request)
        return httpx.Response(200, json={"hits": [], "warnings": []})

    artifacts = ArtifactStore(tmp_path)
    queries = [{"target_id": "t1", "query": "未知功能"}]
    missing = await KnowledgeProvider().search(queries, artifacts)
    assert missing["materials"] == []
    assert missing["issues"][0]["code"] == "knowledge_unconfigured"
    result = await provider(handler).search(queries, artifacts)
    assert len(calls) == 1 and result["materials"] == []
    assert result["issues"][0]["code"] == "knowledge_no_hits"
    assert "本次未取得" in result["issues"][0]["detail"]


async def test_query_is_exact_and_current_version_metadata_is_preserved(tmp_path):
    requests = []

    async def handler(request):
        requests.append(json.loads(request.content)["query"])
        return httpx.Response(200, json={"hits": [hit(is_current=False)], "warnings": []})

    result = await provider(handler).search([
        {"target_id": "t1", "query": " 权限 "}, {"target_id": "t2", "query": "权限"}], ArtifactStore(tmp_path))
    assert requests == [" 权限 ", "权限"]
    assert result["materials"][0]["locator"]["is_current"] is False


@pytest.mark.parametrize("failure", ["http", "json", "shape", "network"])
async def test_failed_retrieval_retries_three_times_and_preserves_other_targets(tmp_path, monkeypatch, failure):
    requests = []
    waits = []

    async def sleep(seconds):
        waits.append(seconds)

    monkeypatch.setattr("docreader.integrations.authoring_knowledge.asyncio.sleep", sleep)

    async def handler(request):
        query = json.loads(request.content)["query"]
        requests.append(query)
        if query == "成功":
            return httpx.Response(200, json={"hits": [hit()], "warnings": []})
        if failure == "http":
            return httpx.Response(401, text="private upstream details test-secret")
        if failure == "json":
            return httpx.Response(200, text="not JSON")
        if failure == "shape":
            return httpx.Response(200, json={"hits": "invalid"})
        raise httpx.ConnectError("private upstream details test-secret")

    result = await provider(handler).search([
        {"target_id": "bad", "query": "失败"}, {"target_id": "ok", "query": "成功"}], ArtifactStore(tmp_path))
    assert requests == ["失败"] * 4 + ["成功"]
    assert waits == [1, 2, 4]
    assert len(result["materials"]) == 1
    assert result["issues"][0]["target_id"] == "bad"
    assert result["issues"][0]["code"] == "knowledge_unavailable"
    assert "private upstream details" not in json.dumps(result)
    assert "test-secret" not in json.dumps(result)


async def test_cancel_is_not_converted_to_missing_knowledge(tmp_path):
    async def handler(request):
        raise asyncio.CancelledError()

    with pytest.raises(asyncio.CancelledError):
        await provider(handler).search([{"target_id": "t", "query": "查询"}], ArtifactStore(tmp_path))


async def test_bad_hits_preserve_valid_siblings_and_do_not_become_proof(tmp_path):
    async def handler(request):
        return httpx.Response(200, json={"hits": [hit(), {"id": "bad", "content": 3}], "warnings": []})

    result = await provider(handler).search([{"target_id": "t", "query": "查询"}], ArtifactStore(tmp_path))
    assert len(result["materials"]) == 1
    assert result["materials"][0]["attachment"] is False
    assert result["issues"][0]["code"] == "knowledge_hit_invalid"


async def test_material_identity_depends_on_content_and_source_not_query(tmp_path):
    async def handler(request):
        return httpx.Response(200, json={"hits": [hit()], "warnings": []})

    artifacts = ArtifactStore(tmp_path)
    source = provider(handler)
    first = await source.search([{"target_id": "a", "query": "权限"}], artifacts)
    second = await source.search([{"target_id": "b", "query": "角色"}], artifacts)
    assert first["materials"][0]["material_id"] == second["materials"][0]["material_id"]
