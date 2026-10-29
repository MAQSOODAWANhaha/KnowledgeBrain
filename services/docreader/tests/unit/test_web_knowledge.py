"""Worker knowledge authentication is temporary and bound to fixed job selections."""

import asyncio
import base64
import hashlib
import hmac
import json
import gzip
from types import SimpleNamespace

import httpx
import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.integrations.web_knowledge import build_web_knowledge, normalize_knowledge_selections


PRODUCT = "4c78bf65-9ee2-4788-bc0c-f14769bb153f"
VERSION = "37f9257d-f030-46cb-ae5b-e34dfebcb14b"
DOCUMENT = "b78d5270-5938-48c2-acb7-e9538910fea7"
OTHER_PRODUCT = "328f852d-3d53-4b7a-851d-fc0572420173"
OTHER_VERSION = "d9341729-5107-40ed-83be-9da5566c15c7"
SELECTION = {"product_id": PRODUCT, "version_id": VERSION}
SECRET = "synthetic-service-only-not-a-user-credential"


def hit(**changes):
    return {"id": "chunk", "content": "可用企业资料", "document_id": DOCUMENT, "document_title": "资料原件",
            "product_id": PRODUCT, "version_id": VERSION, "version_label": "已选固定版本", **changes}


def provider(handler, **options):
    return build_web_knowledge(execution_id="job-fixed", selections=[SELECTION], base_url="https://api.invalid",
        service_secret=SECRET, transport=httpx.MockTransport(handler), **options)


def decode(token):
    parts = token.removeprefix("Bearer ").split(".")
    header, claims = [json.loads(base64.urlsafe_b64decode(value + "=" * (-len(value) % 4))) for value in parts[:2]]
    signature = base64.urlsafe_b64decode(parts[2] + "=" * (-len(parts[2]) % 4))
    assert hmac.compare_digest(signature, hmac.new(SECRET.encode(), ".".join(parts[:2]).encode(), hashlib.sha256).digest())
    assert header == {"alg": "HS256", "typ": "JWT"}
    return claims


@pytest.mark.parametrize("selection", [
    [{"product_id": PRODUCT, "version_id": "current"}],
    [{**SELECTION, "bearer": "forbidden"}], [{"product_id": PRODUCT}], [SELECTION] * 21,
])
def test_selection_requires_only_explicit_bounded_product_and_version_ids(selection):
    with pytest.raises(ValueError):
        normalize_knowledge_selections(selection)


def test_empty_selection_needs_no_service_and_duplicate_ids_are_normalized():
    assert build_web_knowledge(execution_id="job", selections=[]) is None
    assert normalize_knowledge_selections([SELECTION, SELECTION]) == [SELECTION]


async def test_search_and_proof_original_use_only_temporary_job_scoped_authentication(tmp_path, monkeypatch):
    monkeypatch.setattr("docreader.integrations.web_knowledge.time", SimpleNamespace(time=lambda: 1800000000))
    content, requests = b"synthetic knowledge proof original", []
    key = "objects/" + hashlib.sha256(content).hexdigest()

    async def handler(request):
        assert request.url == "https://api.invalid/api/internal/tender-knowledge/job-fixed"
        assert "x-api-key" not in request.headers
        assert decode(request.headers["authorization"]) == {
            "purpose": "tender_knowledge_v1", "execution_id": "job-fixed", "exp": 1800000060}
        body = json.loads(request.content)
        requests.append(body)
        assert body["product_id"] == PRODUCT and body["version_id"] == VERSION
        if body["operation"] == "search":
            assert body == {"operation": "search", **SELECTION, "query": "企业能力"}
            return httpx.Response(200, json={"hits": [hit()]})
        if body["operation"] == "document":
            assert body["document_id"] == DOCUMENT
            return httpx.Response(200, json={"id": DOCUMENT, "product_version_id": VERSION,
                "file_name": "证明.pdf", "object_ref": key})
        assert body == {"operation": "file", **SELECTION, "key": key}
        return httpx.Response(200, content=content)

    selected, artifacts = provider(handler), ArtifactStore(tmp_path)
    result = await selected.search([{"target_id": "target", "query": "企业能力", "include_original": True}], artifacts)
    assert result["issues"] == []
    material = result["materials"][0]
    assert material["attachment"] is True and artifacts.read_bytes(material["original_ref"]) == content
    assert [request["operation"] for request in requests] == ["search", "document", "file"]
    persisted = json.dumps({"spec": selected.spec(), "result": result}, ensure_ascii=False)
    assert SECRET not in persisted and "Bearer " not in persisted
    assert material["locator"]["searches"][0]["include_library"] is False


async def test_missing_server_configuration_is_nonblocking_and_never_uses_user_credentials(tmp_path, monkeypatch):
    monkeypatch.delenv("KNOWLEDGEBRAIN_INTERNAL_API_URL", raising=False)
    monkeypatch.delenv("GRPC_AUTH_TOKEN", raising=False)
    selected = build_web_knowledge(execution_id="job", selections=[SELECTION])
    result = await selected.search([{"target_id": "target", "query": "查询"}], ArtifactStore(tmp_path))
    assert result["materials"] == [] and result["issues"][0]["code"] == "knowledge_unconfigured"


async def test_selected_versions_reject_out_of_scope_hits_and_keep_valid_siblings(tmp_path):
    selected = [{"product_id": OTHER_PRODUCT, "version_id": OTHER_VERSION}, SELECTION]
    sent = []

    async def handler(request):
        command = json.loads(request.content)
        sent.append(command)
        if command["product_id"] == OTHER_PRODUCT:
            return httpx.Response(200, json={"hits": [hit()]})
        return httpx.Response(200, json={"hits": [hit(), hit(version_id=OTHER_VERSION)]})

    source = build_web_knowledge(execution_id="job-fixed", selections=selected, base_url="https://api.invalid",
        service_secret=SECRET, transport=httpx.MockTransport(handler))
    result = await source.search([{"target_id": "target", "query": "查询"}], ArtifactStore(tmp_path))
    assert len(sent) == 2 and len(result["materials"]) == 1
    assert result["materials"][0]["locator"]["version_id"] == VERSION
    assert {issue["code"] for issue in result["issues"]} == {"knowledge_hit_out_of_scope"}


async def test_internal_failures_retry_with_fresh_tokens_and_preserve_gap_without_secret_details(tmp_path, monkeypatch):
    clock = iter([1800000000, 1800000010, 1800000020, 1800000030])
    monkeypatch.setattr("docreader.integrations.web_knowledge.time", SimpleNamespace(time=lambda: next(clock)))
    async def no_sleep(_seconds):
        pass
    monkeypatch.setattr("docreader.integrations.authoring_knowledge.asyncio.sleep", no_sleep)
    expires = []

    async def handler(request):
        expires.append(decode(request.headers["authorization"])["exp"])
        return httpx.Response(403, text="private service details " + SECRET)

    result = await provider(handler).search([{"target_id": "target", "query": "查询"}], ArtifactStore(tmp_path))
    assert expires == [1800000060, 1800000070, 1800000080, 1800000090]
    assert result["materials"] == [] and result["issues"][0]["code"] == "knowledge_unavailable"
    assert SECRET not in json.dumps(result) and "private service details" not in json.dumps(result)


async def test_worker_cancellation_propagates_instead_of_becoming_a_knowledge_gap(tmp_path):
    async def handler(_request):
        raise asyncio.CancelledError()

    with pytest.raises(asyncio.CancelledError):
        await provider(handler).search([{"target_id": "target", "query": "查询"}], ArtifactStore(tmp_path))


async def test_decoded_internal_responses_are_not_decompressed_twice(tmp_path, monkeypatch):
    calls = []
    async def no_sleep(_seconds):
        pass
    monkeypatch.setattr("docreader.integrations.authoring_knowledge.asyncio.sleep", no_sleep)

    async def handler(request):
        calls.append(request)
        content = gzip.compress(json.dumps({"hits": [hit()]}).encode())
        return httpx.Response(200, content=content, headers={"content-encoding": "gzip",
            "content-length": str(len(content)), "content-type": "application/json"})

    result = await provider(handler).search([{"target_id": "target", "query": "查询"}], ArtifactStore(tmp_path))
    assert len(calls) == 1 and len(result["materials"]) == 1 and result["issues"] == []
