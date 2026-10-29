"""Recoverable, job-scoped knowledge reads without persisting user credentials."""

from __future__ import annotations

import base64
import hashlib
import hmac
import json
import os
import time
from urllib.parse import quote
from uuid import UUID

import httpx

from docreader.integrations.authoring_knowledge import KnowledgeProvider


def normalize_knowledge_selections(values):
    if not isinstance(values, list) or len(values) > 20:
        raise ValueError("最多选择20个明确的知识资产版本。")
    selected = set()
    for value in values:
        if not isinstance(value, dict) or set(value) != {"product_id", "version_id"}:
            raise ValueError("知识资产选择仅接受产品与固定版本标识。")
        try:
            selected.add((str(UUID(value["product_id"])), str(UUID(value["version_id"]))))
        except (ValueError, TypeError, AttributeError):
            raise ValueError("请选择明确的知识资产版本，不能使用动态current版本。") from None
    return [{"product_id": product, "version_id": version} for product, version in sorted(selected)]


def _segment(value):
    return base64.urlsafe_b64encode(value).rstrip(b"=").decode("ascii")


class _WorkerAuth(httpx.Auth):
    def __init__(self, execution_id, secret):
        self.execution_id, self.secret = execution_id, secret

    def auth_flow(self, request):
        header = _segment(b'{"alg":"HS256","typ":"JWT"}')
        claims = _segment(json.dumps({"purpose": "tender_knowledge_v1", "execution_id": self.execution_id,
            "exp": int(time.time()) + 60}, separators=(",", ":")).encode())
        signed = header + "." + claims
        signature = hmac.new(self.secret.encode(), signed.encode(), hashlib.sha256).digest()
        request.headers["authorization"] = "Bearer " + signed + "." + _segment(signature)
        yield request


class _ExecutionTransport(httpx.AsyncBaseTransport):
    def __init__(self, base_url, execution_id, selection, transport):
        self.endpoint = f"{base_url}/api/internal/tender-knowledge/{quote(execution_id, safe='')}"
        self.selection, self.transport = selection, transport

    async def handle_async_request(self, request):
        path = request.url.path.split("/")
        if request.method == "POST" and request.url.path == "/api/v1/search":
            body = json.loads(request.content)
            command = {"operation": "search", **self.selection, "query": body["query"]}
        elif request.method == "GET" and len(path) == 5 and path[1:4] == ["api", "v1", "documents"]:
            command = {"operation": "document", **self.selection, "document_id": path[4]}
        elif (request.method == "GET" and len(path) == 8 and path[1:4] == ["api", "v1", "products"]
              and path[5] == "versions" and path[7] == "files"
              and path[4] == self.selection["product_id"] and path[6] == self.selection["version_id"]):
            command = {"operation": "file", **self.selection, "key": request.url.params.get("key")}
        else:
            raise ValueError("unsupported scoped knowledge request")
        # This client sends only the short-lived service token to the configured
        # origin. Redirects never receive it, and no user credentials are copied.
        async with httpx.AsyncClient(transport=self.transport, timeout=30, follow_redirects=False) as client:
            response = await client.post(self.endpoint, json=command,
                headers={"authorization": request.headers["authorization"]})
        headers = {key: value for key, value in response.headers.items()
                   if key.lower() not in {"content-encoding", "content-length", "transfer-encoding"}}
        return httpx.Response(response.status_code, content=response.content, headers=headers, request=request)


class WebKnowledgeProvider:
    def __init__(self, *, execution_id, selections, base_url, service_secret, transport):
        self.selections = normalize_knowledge_selections(selections)
        self.base_url = None
        try:
            url = httpx.URL(base_url or "")
            if (url.scheme in {"http", "https"} and url.host and not (url.userinfo or url.query or url.fragment)
                    and url.path in {"", "/"}):
                self.base_url = str(url).rstrip("/")
        except httpx.InvalidURL:
            pass
        configured = bool(self.base_url and service_secret and service_secret.strip())
        self.providers = [KnowledgeProvider(base_url=self.base_url,
            auth=_WorkerAuth(execution_id, service_secret) if configured else None,
            product_id=selection["product_id"], version_id=selection["version_id"], include_library=False,
            transport=_ExecutionTransport(self.base_url, execution_id, selection, transport) if configured else None)
            for selection in self.selections]

    def spec(self):
        return {"provider": "knowledgebrain_web_selection_v1", "base_url": self.base_url,
                "selections": self.selections, "max_original_bytes": 50 * 1024 * 1024}

    async def search(self, queries, artifacts):
        materials, issues = {}, []
        for provider in self.providers:
            result = await provider.search(queries, artifacts)
            issues.extend(result["issues"])
            for material in result["materials"]:
                identity = material["material_id"]
                if identity not in materials:
                    materials[identity] = material
                else:
                    locator = materials[identity]["locator"]
                    locator["target_ids"] = list(dict.fromkeys([*locator["target_ids"], *material["locator"]["target_ids"]]))
        issues = list({json.dumps(issue, sort_keys=True): issue for issue in issues}.values())
        return {"materials": list(materials.values()), "issues": issues}


def build_web_knowledge(*, execution_id, selections, base_url=None, service_secret=None, transport=None):
    selected = normalize_knowledge_selections(selections)
    if not selected:
        return None
    return WebKnowledgeProvider(execution_id=execution_id, selections=selected,
        base_url=base_url if base_url is not None else os.environ.get("KNOWLEDGEBRAIN_INTERNAL_API_URL"),
        service_secret=service_secret if service_secret is not None else os.environ.get("GRPC_AUTH_TOKEN"),
        transport=transport)
