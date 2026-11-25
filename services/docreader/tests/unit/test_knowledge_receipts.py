"""Single-query receipts preserve results while rebinding host target identity."""
from typing import Any, cast

from copy import deepcopy
from types import SimpleNamespace

import pytest

from docreader.integrations.artifacts import ArtifactIntegrityError, ArtifactStore
from docreader.integrations.authoring_knowledge import KnowledgeResponseError, KnowledgeUnavailable
from docreader.tender import document_execution
from docreader.tender.document_execution import _FrozenKnowledge


class ReceiptStore:
    def __init__(self):
        self.rows = {}

    async def receipt(self, lease, stage, scope, *, input_digest):
        return self.rows.get((lease.execution_id, stage, scope, input_digest))

    async def commit_artifact(self, lease, stage, reference, *, scope, input_digest):
        self.rows[(lease.execution_id, stage, scope, input_digest)] = {"artifact_ref": reference}


class Knowledge:
    def __init__(self, *, empty=False):
        self.calls, self.empty = [], empty

    async def search(self, queries, artifacts, *, run_operation):
        async def send_once():
            return self._result(queries)
        key = document_execution._digest({"query": queries[0]["query"], "provider": "fixture",
                                          "scope": cast(Any, self).scope})
        return await run_operation("search", key, send_once)

    def _result(self, queries):
        self.calls.append(deepcopy(queries))
        targets = [entry["target_id"] for entry in queries]
        if self.empty:
            return {"materials": [], "issues": [{"code": "knowledge_no_hits", "detail": "本次未取得可用资料。",
                                                  "target_id": target} for target in targets]}
        return {"materials": [{"material_id": "same-chunk", "title": "产品功能", "text": "支持角色权限。",
            "locator": {"target_ids": targets, "version_id": "v1",
                        "searches": [{"query": entry["query"]} for entry in queries]}}], "issues": []}


def snapshots(store):
    return [row for key, row in store.rows.items() if key[1] == "knowledge-snapshot"]


def frozen(provider, store, artifacts, *, execution_id="execution", scope="collection"):
    provider.scope = scope
    return _FrozenKnowledge(provider, {"provider": "fixture", "scope": scope}, store, artifacts,
                            SimpleNamespace(execution_id=execution_id))


async def test_single_query_receipt_is_reused_and_targets_are_rebound(tmp_path):
    artifacts, store, provider = ArtifactStore(tmp_path), ReceiptStore(), Knowledge()
    first = await frozen(provider, store, artifacts).search([
        {"query": "权限", "target_id": "old-a"}, {"query": "权限", "target_id": "old-b"}], artifacts)
    resumed = await frozen(provider, store, artifacts).search([
        {"query": "权限", "target_id": "new-target"}], artifacts)
    assert len(provider.calls) == 1
    assert first["materials"][0]["locator"]["target_ids"] == ["old-a", "old-b"]
    assert resumed["materials"][0]["locator"]["target_ids"] == ["new-target"]
    assert len(snapshots(store)) == 1
    saved = artifacts.read_json(snapshots(store)[0]["artifact_ref"])
    assert "target_ids" not in saved["materials"][0]["locator"]
    assert "old-a" not in str(saved) and "old-b" not in str(saved)


async def test_adding_query_reads_only_new_query_and_merges_material_associations(tmp_path):
    artifacts, store, provider = ArtifactStore(tmp_path), ReceiptStore(), Knowledge()
    reader = frozen(provider, store, artifacts)
    await reader.search([{"query": "权限", "target_id": "a"}], artifacts)
    result = await reader.search([
        {"query": "权限", "target_id": "b"}, {"query": "角色", "target_id": "c"}], artifacts)
    assert [call[0]["query"] for call in provider.calls] == ["权限", "角色"]
    assert len(result["materials"]) == 1
    locator = result["materials"][0]["locator"]
    assert locator["target_ids"] == ["b", "c"]
    assert locator["searches"] == [{"query": "权限"}, {"query": "角色"}]


async def test_empty_query_receipt_rebinds_diagnostics_without_retrieval(tmp_path):
    artifacts, store, provider = ArtifactStore(tmp_path), ReceiptStore(), Knowledge(empty=True)
    reader = frozen(provider, store, artifacts)
    await reader.search([{"query": "权限", "target_id": "a"}], artifacts)
    result = await reader.search([
        {"query": "权限", "target_id": "b"}, {"query": "权限", "target_id": "c"}], artifacts)
    assert len(provider.calls) == 1 and not result["materials"]
    assert [issue["target_id"] for issue in result["issues"]] == ["b", "c"]
    saved = artifacts.read_json(snapshots(store)[0]["artifact_ref"])
    assert all("target_id" not in issue for issue in saved["issues"])


async def test_query_identity_preserves_exact_query_and_provider_scope(tmp_path):
    artifacts, store, provider = ArtifactStore(tmp_path), ReceiptStore(), Knowledge()
    for query, scope in (("权限", "first"), (" 权限 ", "first"), ("权限", "second")):
        await frozen(provider, store, artifacts, scope=scope).search([
            {"query": query, "target_id": "target"}], artifacts)
    assert [call[0]["query"] for call in provider.calls] == ["权限", " 权限 ", "权限"]
    assert len(snapshots(store)) == 3


async def test_new_execution_without_proven_snapshot_searches_again(tmp_path):
    artifacts, store, provider = ArtifactStore(tmp_path), ReceiptStore(), Knowledge()
    for execution_id in ("first", "second"):
        await frozen(provider, store, artifacts, execution_id=execution_id).search([
            {"query": "权限", "target_id": "target"}], artifacts)
    assert len(provider.calls) == 2


async def test_receipt_rejects_query_identity_mismatch(tmp_path):
    artifacts, store, provider = ArtifactStore(tmp_path), ReceiptStore(), Knowledge()
    reader = frozen(provider, store, artifacts)
    await reader.search([{"query": "权限", "target_id": "target"}], artifacts)
    row = snapshots(store)[0]
    saved = artifacts.read_json(row["artifact_ref"])
    saved["query"] = "another query"
    row["artifact_ref"] = artifacts.put_json(saved)
    with pytest.raises(ArtifactIntegrityError):
        await reader.search([{"query": "权限", "target_id": "target"}], artifacts)


async def test_proof_snapshot_is_distinct_but_reuses_the_same_search_operation(tmp_path):
    artifacts, store, provider = ArtifactStore(tmp_path), ReceiptStore(), Knowledge()
    reader = frozen(provider, store, artifacts)
    await reader.search([{"query": "证明", "target_id": "body"}], artifacts)
    await reader.search([{"query": "证明", "target_id": "proof", "include_original": True}], artifacts)
    await reader.search([{"query": "证明", "target_id": "new-proof", "include_original": True}], artifacts)
    assert len(provider.calls) == 1
    assert len(snapshots(store)) == 2


async def test_same_query_mixed_targets_retrieves_original_only_once(tmp_path):
    artifacts, store, provider = ArtifactStore(tmp_path), ReceiptStore(), Knowledge()
    reader = frozen(provider, store, artifacts)
    result = await reader.search([
        {"query": "证明", "target_id": "body"},
        {"query": "证明", "target_id": "proof", "include_original": True}], artifacts)
    assert len(provider.calls) == 1
    assert provider.calls[0][0]['include_original'] is True
    assert result['materials'][0]['locator']['target_ids'] == ['body', 'proof']


async def test_unknown_attempts_count_and_are_not_reset_by_resume(tmp_path):
    artifacts, store = ArtifactStore(tmp_path), ReceiptStore()

    class Failing:
        def __init__(self):
            self.calls = 0

        async def search(self, queries, artifacts, *, run_operation):
            async def send_once():
                self.calls += 1
                raise KnowledgeResponseError("synthetic failed response")
            key = document_execution._digest({"query": queries[0]["query"], "provider": "fixture"})
            try:
                return await run_operation("search", key, send_once)
            except KnowledgeUnavailable:
                return {"materials": [], "issues": [{"code": "knowledge_unavailable", "detail": "失败",
                                                        "target_id": queries[0]["target_id"]}]}

    provider = Failing()
    digest = document_execution._digest({"query": "权限", "provider": "fixture"})
    for number in (1, 2):
        reference = artifacts.put_json({"kind": "search", "key": digest, "attempt": number})
        await store.commit_artifact(SimpleNamespace(execution_id="execution"), "knowledge-operation", reference,
                                    scope=f"search:{digest}:attempt:{number}:start", input_digest=digest)
    reader = frozen(provider, store, artifacts)
    result = await reader.search([{"query": "权限", "target_id": "target"}], artifacts)
    assert provider.calls == 2
    assert result["issues"][0]["code"] == "knowledge_unavailable"
    await reader.search([{"query": "权限", "target_id": "other"}], artifacts)
    assert provider.calls == 2
