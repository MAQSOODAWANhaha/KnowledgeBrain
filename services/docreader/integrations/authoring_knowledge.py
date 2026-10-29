"""Read knowledge excerpts through the existing assembly API and freeze them.

Q distinct queries, D requested proof documents and K original objects cause at
most 4(Q+D+K) HTTP attempts (first plus three retries). Empty results do not retry.
Only host-selected proof queries fetch originals. Retrieval failures become
located gaps, and neither a search hit nor its rank certifies business facts.
The caller persists the returned snapshot before writing or resuming a task.
"""

from __future__ import annotations

import asyncio
import hashlib
import json
import mimetypes
import re
from uuid import UUID

import httpx

from docreader.integrations.artifacts import ArtifactIntegrityError
from docreader.tender.schema import KnowledgeMaterial


class KnowledgeProvider:
    def __init__(self, *, base_url: str | None = None, bearer: str | None = None,
                 api_key: str | None = None, max_original_bytes: int = 50 * 1024 * 1024,
                 product_id: str | None = None, version_id: str | None = None,
                 auth: httpx.Auth | None = None, include_library: bool = True,
                 transport: httpx.AsyncBaseTransport | None = None):
        self.base_url, self.bearer = base_url, bearer
        self.api_key, self.max_original_bytes = api_key, max_original_bytes
        self.product_id, self.version_id = product_id, version_id
        self.auth, self.include_library = auth, include_library
        self.transport = transport
        self._original_refs = {}

    def spec(self) -> dict:
        """Non-secret retrieval identity; rotating credentials does not change it."""
        base_url = None
        if self.base_url:
            try:
                url = httpx.URL(self.base_url)
                if url.scheme in {"http", "https"} and url.host and not (url.userinfo or url.query or url.fragment):
                    base_url = str(url).rstrip("/")
            except httpx.InvalidURL:
                pass
        spec = {"provider": "knowledgebrain_assembly_v2", "base_url": base_url,
                "product_id": self.product_id, "version_id": self.version_id,
                "auth_mode": "service" if self.auth is not None and not (self.api_key or self.bearer) else
                             "api_key" if self.api_key and not self.bearer else
                             "bearer" if self.bearer and not self.api_key else "none",
                "max_original_bytes": self.max_original_bytes}
        if not self.include_library:
            spec["include_library"] = False
        return spec

    async def _original(self, client, api, hit, artifacts, documents, originals):
        try:
            product, version, document = (str(UUID(hit[field])) for field in
                                          ("product_id", "version_id", "document_id"))
        except (KeyError, ValueError, TypeError, AttributeError):
            return None, {}
        scope = product, version, document
        if scope not in documents:
            documents[scope] = None
            for attempt in range(4):
                try:
                    response = await client.get(f"{api}/documents/{document}")
                    response.raise_for_status()
                    meta = response.json()
                    if (not isinstance(meta, dict) or meta.get("id") != document
                            or meta.get("product_version_id") != version
                            or not isinstance(meta.get("file_name"), str)
                            or not re.fullmatch(r"objects/[0-9a-f]{64}", str(meta.get("object_ref", "")))):
                        raise ValueError("original document identity unavailable")
                    documents[scope] = meta
                    break
                except (httpx.HTTPError, ValueError):
                    if attempt < 3:
                        await asyncio.sleep(2 ** attempt)
        meta = documents[scope]
        if meta is None:
            return None, {}
        key = product, version, meta["object_ref"]
        cache_key = str(artifacts.root), api, self.spec()["auth_mode"], self.max_original_bytes, *key
        if key not in originals:
            cached = self._original_refs.get(cache_key)
            if cached is not None:
                try:
                    await asyncio.to_thread(artifacts.path, cached)
                    originals[key] = cached
                except (OSError, ArtifactIntegrityError):
                    self._original_refs.pop(cache_key, None)
        if key not in originals:
            originals[key] = None
            for attempt in range(4):
                try:
                    async with client.stream("GET", f"{api}/products/{product}/versions/{version}/files",
                                             params={"key": meta["object_ref"]}) as response:
                        response.raise_for_status()
                        if int(response.headers.get("content-length", "0")) > self.max_original_bytes:
                            raise ValueError("original exceeds size limit")
                        content = bytearray()
                        async for chunk in response.aiter_bytes():
                            if len(content) + len(chunk) > self.max_original_bytes:
                                raise ValueError("original exceeds size limit")
                            content.extend(chunk)
                    if not content or hashlib.sha256(content).hexdigest() != meta["object_ref"][8:]:
                        raise ValueError("original content digest mismatch")
                    media_type = mimetypes.guess_type(meta["file_name"])[0] or "application/octet-stream"
                    reference = await asyncio.to_thread(artifacts.put_bytes, bytes(content), media_type)
                    originals[key] = self._original_refs[cache_key] = reference
                    break
                except (httpx.HTTPError, ValueError, OSError):
                    if attempt < 3:
                        await asyncio.sleep(2 ** attempt)
        return originals[key], {field: meta[field] for field in ("file_name", "object_ref")}

    async def search(self, queries: list[dict], artifacts) -> dict:
        """Return materials and per-target diagnostics, without issuing LLM calls.

        product_id selects an existing knowledge collection for this API. It is
        not a required model/configuration selection for the bidding document.
        Every query is supplied by the caller; no keyword business routing or
        suitability inference happens here. Direct user materials need no API.
        """
        issues, grouped, materials = [], {}, {}
        original_queries, documents, originals = set(), {}, {}

        def problem(targets, code, detail):
            issues.extend({"code": code, "detail": detail, "target_id": target} for target in targets)

        for entry in queries:
            target, query = entry.get("target_id"), entry.get("query")
            if not isinstance(target, str) or not target:
                raise ValueError("knowledge queries require a host target_id")
            if not isinstance(query, str) or not query.strip():
                problem([target], "knowledge_query_missing", "未提供检索内容，使用现有资料继续编制。")
                continue
            targets = grouped.setdefault(query, [])
            if target not in targets:
                targets.append(target)
            if entry.get("include_original") is True:
                original_queries.add(query)
        if not grouped:
            return {"materials": [], "issues": issues}

        try:
            if (not self.base_url or not self.product_id
                    or sum((bool(self.bearer), bool(self.api_key), self.auth is not None)) != 1
                    or type(self.include_library) is not bool
                    or type(self.max_original_bytes) is not int or self.max_original_bytes <= 0):
                raise ValueError("missing knowledge configuration")
            url = httpx.URL(self.base_url)
            if url.scheme not in {"http", "https"} or not url.host or url.userinfo or url.query or url.fragment:
                raise ValueError("invalid knowledge service URL")
            product_id = str(UUID(self.product_id))
            base = str(url).rstrip("/")
            api = base if base.endswith("/api/v1") else base + "/api/v1"
            endpoint = api + "/search"
        except (ValueError, TypeError, httpx.InvalidURL):
            problem(list(dict.fromkeys(t for targets in grouped.values() for t in targets)),
                    "knowledge_unconfigured", "知识服务或资料集合尚未配置，使用现有资料继续编制。")
            return {"materials": [], "issues": issues}

        headers = ({"x-api-key": self.api_key} if self.api_key else
                   {"Authorization": "Bearer " + self.bearer} if self.bearer else {})
        async with httpx.AsyncClient(transport=self.transport, timeout=30, follow_redirects=False,
                                     headers=headers, auth=self.auth) as client:
            for query, targets in grouped.items():
                request = {"mode": "assembly", "product_id": product_id, "query": query,
                           "match_count": 8, "expand_wiki": False, "expand_graph": False,
                           "include_library": self.include_library}
                if self.version_id is not None:
                    request["version_id"] = self.version_id
                response = None
                for attempt in range(4):
                    try:
                        received = await client.post(endpoint, json=request)
                        received.raise_for_status()
                        parsed = received.json()
                        if not isinstance(parsed, dict) or not isinstance(parsed.get("hits"), list):
                            raise ValueError("invalid knowledge response")
                        response = parsed
                        break
                    except (httpx.HTTPError, ValueError):
                        if attempt < 3:
                            await asyncio.sleep(2 ** attempt)
                if response is None:
                    problem(targets, "knowledge_unavailable", "知识检索重试后仍未取得可用结果，相关资料待补。")
                    continue
                if not response["hits"]:
                    problem(targets, "knowledge_no_hits", "本次未取得可用资料，相关事实与证明待补。")
                for hit in response["hits"]:
                    if (not isinstance(hit, dict) or not isinstance(hit.get("content"), str)
                            or not hit["content"].strip() or not isinstance(hit.get("id"), str)
                            or not isinstance(hit.get("document_id"), str)):
                        problem(targets, "knowledge_hit_invalid", "部分检索结果缺少可追溯的原文或来源，已保留其余资料。")
                        continue
                    if not self.include_library and (hit.get("product_id") != product_id
                            or self.version_id is not None and hit.get("version_id") != self.version_id):
                        problem(targets, "knowledge_hit_out_of_scope", "部分检索结果不属于已选资料版本，未作为编制依据。")
                        continue
                    try:
                        reference = await asyncio.to_thread(artifacts.put_bytes, hit["content"].encode("utf-8"),
                                                            "text/plain; charset=utf-8")
                    except (OSError, ArtifactIntegrityError):
                        problem(targets, "knowledge_material_unavailable", "部分检索原文未能保存，未作为可用资料引用。")
                        continue
                    source = {key: hit[key] for key in ("document_id", "product_id", "version_id", "version_label", "is_current",
                                                       "start_at", "end_at", "image_object_ref") if key in hit}
                    source.update(chunk_id=hit["id"], kind="search_excerpt", service_url=base)
                    identity = hashlib.sha256(json.dumps({"source": source, "sha256": reference["sha256"]},
                        ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
                    material_id = "knowledge-" + identity[:24]
                    original_ref, original_locator = None, {}
                    if query in original_queries:
                        original_ref, original_locator = await self._original(
                            client, api, hit, artifacts, documents, originals)
                        if original_ref is None:
                            problem(targets, "knowledge_original_unavailable",
                                    "候选文字已保留，未取得可核对的同版本原件；证明材料待补。")
                    if material_id in materials:
                        locator = materials[material_id]["locator"]
                        if original_ref is not None:
                            materials[material_id]["original_ref"] = original_ref
                            materials[material_id]["attachment"] = True
                            locator.update(original_locator)
                        locator["target_ids"] = list(dict.fromkeys([*locator["target_ids"], *targets]))
                        if request not in locator["searches"]:
                            locator["searches"].append(request)
                        continue
                    title = hit.get("document_title")
                    materials[material_id] = KnowledgeMaterial.model_validate({
                        "material_id": material_id, "title": title if isinstance(title, str) and title else "知识资料",
                        "text": hit["content"], "content_ref": reference, "original_ref": original_ref,
                        "locator": {**source, **original_locator, "target_ids": targets.copy(), "searches": [request]},
                        "usage": "reference", "attachment": original_ref is not None}).model_dump()
                warnings = response.get("warnings")
                if isinstance(warnings, list) and warnings:
                    problem(targets, "knowledge_partial_results", "知识服务报告部分检索限制，现有命中不代表资料齐备。")
        return {"materials": list(materials.values()), "issues": issues}
