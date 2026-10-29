"""Real repository routes bind optional proof originals to their exact version."""

import asyncio
from collections import Counter
import hashlib
import json

import httpx
import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.integrations.authoring_knowledge import KnowledgeProvider


COLLECTION = "4c78bf65-9ee2-4788-bc0c-f14769bb153f"
PRODUCT = "0a61c763-5762-4113-bb77-15bf19820aaa"
VERSION = "791eb8ab-92a1-4458-8a56-d5445c9fc9a4"
DOCUMENT = "b2a496df-0e48-4a16-b609-ef6d3906c40a"
RAW = b"synthetic original certificate, not a real bidder document"
OBJECT = "objects/" + hashlib.sha256(RAW).hexdigest()
SECRET = "private-fixture-credential"


def hit(**changes):
    return {"id": "candidate-chunk", "content": "候选证明记载测试功能。", "document_id": DOCUMENT,
            "document_title": "测试证明", "product_id": PRODUCT, "version_id": VERSION, **changes}


def metadata(**changes):
    return {"id": DOCUMENT, "product_version_id": VERSION, "file_name": "测试证明.pdf",
            "object_ref": OBJECT, **changes}


def query(*, original=True, target="proof", text="测试证明"):
    return {"target_id": target, "query": text, "include_original": original}


def provider(handler, **changes):
    return KnowledgeProvider(base_url="https://knowledge.invalid/api/v1", bearer=SECRET,
        product_id=COLLECTION, transport=httpx.MockTransport(handler), **changes)


def routes(request, *, original=RAW, meta=None, hits=None):
    if request.method == "POST":
        assert request.url.path == "/api/v1/search"
        assert json.loads(request.content)["product_id"] == COLLECTION
        return httpx.Response(200, json={"hits": [hit()] if hits is None else hits})
    if request.url.path == f"/api/v1/documents/{DOCUMENT}":
        return httpx.Response(200, json=metadata() if meta is None else meta)
    assert request.url.path == f"/api/v1/products/{PRODUCT}/versions/{VERSION}/files"
    assert request.url.params["key"] == OBJECT
    return httpx.Response(200, content=original)


@pytest.fixture(autouse=True)
def no_retry_wait(monkeypatch):
    async def sleep(_):
        pass
    monkeypatch.setattr("docreader.integrations.authoring_knowledge.asyncio.sleep", sleep)


async def test_original_is_verified_from_exact_hit_scope_and_stays_a_candidate(tmp_path):
    requests = []

    def handler(request):
        requests.append(request)
        assert request.headers["authorization"] == "Bearer " + SECRET
        assert "x-api-key" not in request.headers
        return routes(request)

    artifacts = ArtifactStore(tmp_path)
    result = await provider(handler).search([query()], artifacts)
    assert len(requests) == 3
    assert result["issues"] == []
    material = result["materials"][0]
    assert artifacts.read_bytes(material["original_ref"]) == RAW
    assert material["original_ref"]["sha256"] == OBJECT.removeprefix("objects/")
    assert material["original_ref"]["media_type"] == "application/pdf"
    assert material["attachment"] is True and material["usage"] == "reference"
    assert material["locator"]["file_name"] == "测试证明.pdf"
    assert material["locator"]["object_ref"] == OBJECT
    assert SECRET not in json.dumps(result)


async def test_text_queries_never_request_originals_and_same_query_or_merges_need(tmp_path):
    requests = []

    def handler(request):
        requests.append((request.method, request.url.path))
        return routes(request)

    source = provider(handler)
    artifacts = ArtifactStore(tmp_path)
    text = await source.search([query(original=False)], artifacts)
    assert len(requests) == 1 and text["materials"][0]["original_ref"] is None
    requests.clear()
    result = await source.search([query(original=False, target="text"), query()], artifacts)
    assert len(requests) == 3 and len(result["materials"]) == 1
    assert result["materials"][0]["locator"]["target_ids"] == ["text", "proof"]
    assert result["materials"][0]["attachment"] is True


async def test_document_and_original_are_not_redownloaded_for_sibling_chunks_queries(tmp_path):
    requests = []

    def handler(request):
        requests.append((request.method, request.url.path))
        return routes(request, hits=[hit(), hit(id="second-chunk", content="另一个候选片段。")])

    result = await provider(handler).search([query(), query(text="另一证明", target="other")], ArtifactStore(tmp_path))
    assert len(result["materials"]) == 2
    assert Counter(path for method, path in requests if method == "GET") == {
        f"/api/v1/documents/{DOCUMENT}": 1,
        f"/api/v1/products/{PRODUCT}/versions/{VERSION}/files": 1}
    assert all(m["attachment"] for m in result["materials"])


@pytest.mark.parametrize("original_first", [False, True])
async def test_same_excerpt_from_different_text_and_original_queries_is_not_duplicated(tmp_path, original_first):
    entries = [query(original=False, target="text", text="功能说明"), query()]
    if original_first:
        entries.reverse()
    artifacts = ArtifactStore(tmp_path)
    result = await provider(routes).search(entries, artifacts)
    assert len(result["materials"]) == 1
    material = result["materials"][0]
    assert material["attachment"] is True
    assert artifacts.read_bytes(material["original_ref"]) == RAW
    assert material["locator"]["object_ref"] == OBJECT
    assert set(material["locator"]["target_ids"]) == {"text", "proof"}


@pytest.mark.parametrize("failure", ["http", "shape", "identity", "version", "object", "network"])
async def test_failed_metadata_retries_four_times_without_downloading_or_losing_excerpt(tmp_path, failure):
    requests = []

    def handler(request):
        requests.append(request)
        if request.method == "POST":
            return routes(request)
        if failure == "http":
            return httpx.Response(401, text=SECRET)
        if failure == "network":
            raise httpx.ConnectError(SECRET)
        data = {"shape": [], "identity": metadata(id=COLLECTION),
                "version": metadata(product_version_id=COLLECTION),
                "object": metadata(object_ref="https://other.invalid/" + SECRET)}[failure]
        return httpx.Response(200, json=data)

    result = await provider(handler).search([query()], ArtifactStore(tmp_path))
    assert len(requests) == 5
    assert all(request.url.path == f"/api/v1/documents/{DOCUMENT}" for request in requests[1:])
    assert len(result["materials"]) == 1 and result["materials"][0]["original_ref"] is None
    assert result["materials"][0]["attachment"] is False
    assert result["issues"][0]["target_id"] == "proof"
    assert result["issues"][0]["code"] == "knowledge_original_unavailable"
    assert SECRET not in json.dumps(result)


@pytest.mark.parametrize("failure", ["404", "hash", "redirect", "network", "too_large"])
async def test_original_failure_is_bounded_retried_and_never_replaces_candidate(tmp_path, failure):
    requests = []

    def handler(request):
        requests.append(request)
        if request.method == "POST" or request.url.path.endswith(DOCUMENT):
            return routes(request)
        if failure == "404":
            return httpx.Response(404, text=SECRET)
        if failure == "redirect":
            return httpx.Response(302, headers={"Location": "https://other.invalid/" + SECRET})
        if failure == "network":
            raise httpx.ConnectError(SECRET)
        return httpx.Response(200, content=b"wrong object" if failure == "hash" else RAW)

    result = await provider(handler, **({"max_original_bytes": 5} if failure == "too_large" else {})).search(
        [query()], ArtifactStore(tmp_path))
    assert len(requests) == 6
    assert all(request.url.host == "knowledge.invalid" for request in requests)
    assert result["materials"][0]["original_ref"] is None
    assert result["materials"][0]["text"] == hit()["content"]
    assert result["issues"][0]["code"] == "knowledge_original_unavailable"
    assert SECRET not in json.dumps(result)


async def test_missing_hit_version_keeps_excerpt_without_forging_scope(tmp_path):
    requests = []

    def handler(request):
        requests.append(request)
        return routes(request, hits=[hit(version_id=None)])

    result = await provider(handler).search([query()], ArtifactStore(tmp_path))
    assert len(requests) == 1
    assert len(result["materials"]) == 1 and not result["materials"][0]["attachment"]
    assert result["issues"][0]["code"] == "knowledge_original_unavailable"


async def test_api_key_has_explicit_header_and_nonsecret_v2_spec(tmp_path):
    requests = []

    def handler(request):
        requests.append(request)
        assert request.headers["x-api-key"] == SECRET
        assert "authorization" not in request.headers
        return routes(request)

    source = KnowledgeProvider(base_url="https://knowledge.invalid", api_key=SECRET,
        product_id=COLLECTION, max_original_bytes=1024, transport=httpx.MockTransport(handler))
    result = await source.search([query()], ArtifactStore(tmp_path))
    assert result["materials"][0]["attachment"] is True
    assert source.spec()["provider"] == "knowledgebrain_assembly_v2"
    assert source.spec()["auth_mode"] == "api_key"
    assert source.spec()["max_original_bytes"] == 1024
    assert SECRET not in json.dumps(source.spec())


async def test_cancel_during_download_propagates(tmp_path):
    def handler(request):
        if request.method == "GET" and request.url.path.endswith("/files"):
            raise asyncio.CancelledError()
        return routes(request)

    with pytest.raises(asyncio.CancelledError):
        await provider(handler).search([query()], ArtifactStore(tmp_path))


async def test_verified_object_cache_still_checks_metadata_on_a_new_search(tmp_path):
    requests = []

    def handler(request):
        requests.append((request.method, request.url.path))
        return routes(request)

    source, artifacts = provider(handler), ArtifactStore(tmp_path)
    first = await source.search([query()], artifacts)
    second = await source.search([query(text="新查询")], artifacts)
    assert first["materials"][0]["original_ref"] == second["materials"][0]["original_ref"]
    assert Counter(path for method, path in requests if method == "GET") == {
        f"/api/v1/documents/{DOCUMENT}": 2,
        f"/api/v1/products/{PRODUCT}/versions/{VERSION}/files": 1}


async def test_cached_object_cannot_hide_a_changed_document_version(tmp_path):
    requests = []
    changed = False

    def handler(request):
        requests.append(request)
        return routes(request, meta=metadata(product_version_id=COLLECTION) if changed else None)

    source, artifacts = provider(handler), ArtifactStore(tmp_path)
    assert (await source.search([query()], artifacts))["materials"][0]["attachment"] is True
    changed = True
    requests.clear()
    result = await source.search([query()], artifacts)
    assert len(requests) == 5
    assert result["materials"][0]["original_ref"] is None


async def test_deleted_cached_object_is_downloaded_again(tmp_path):
    requests = []

    def handler(request):
        requests.append(request)
        return routes(request)

    source, artifacts = provider(handler), ArtifactStore(tmp_path)
    first = await source.search([query()], artifacts)
    artifacts.path(first["materials"][0]["original_ref"]).unlink()
    requests.clear()
    result = await source.search([query()], artifacts)
    assert len(requests) == 3
    assert artifacts.read_bytes(result["materials"][0]["original_ref"]) == RAW


async def test_streaming_limit_is_enforced_without_content_length(tmp_path):
    class Stream(httpx.AsyncByteStream):
        async def __aiter__(self):
            yield b"first"
            yield b"second"

    requests = []

    def handler(request):
        requests.append(request)
        if request.method == "GET" and request.url.path.endswith("/files"):
            return httpx.Response(200, stream=Stream())
        return routes(request)

    artifacts = ArtifactStore(tmp_path)
    result = await provider(handler, max_original_bytes=5).search([query()], artifacts)
    assert len(requests) == 6
    assert not result["materials"][0]["attachment"]
    assert len(list(tmp_path.glob("*/*"))) == 1  # The source excerpt alone was saved.


async def test_ambiguous_auth_does_not_send_either_credential(tmp_path):
    def handler(request):
        pytest.fail("Ambiguous credentials must not be sent")

    result = await provider(handler, api_key="other-private-key").search([query()], ArtifactStore(tmp_path))
    assert result["materials"] == [] and result["issues"][0]["code"] == "knowledge_unconfigured"
    assert SECRET not in json.dumps(result) and "other-private-key" not in json.dumps(result)


def test_auth_mode_is_nonsecret_and_distinguishes_provider_receipt_identity():
    empty = KnowledgeProvider().spec()
    bearer = KnowledgeProvider(bearer=SECRET).spec()
    api_key = KnowledgeProvider(api_key=SECRET).spec()
    assert [spec["auth_mode"] for spec in (empty, bearer, api_key)] == ["none", "bearer", "api_key"]
    assert len({json.dumps(spec, sort_keys=True) for spec in (empty, bearer, api_key)}) == 3
    assert SECRET not in json.dumps([empty, bearer, api_key])
