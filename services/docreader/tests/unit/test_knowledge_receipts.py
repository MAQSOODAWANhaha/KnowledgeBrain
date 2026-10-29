"""Single-query receipts preserve results while rebinding host target identity."""

from copy import deepcopy
from types import SimpleNamespace

import pytest

from docreader.integrations.artifacts import ArtifactIntegrityError, ArtifactStore
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

    async def search(self, queries, artifacts):
        self.calls.append(deepcopy(queries))
        targets = [entry["target_id"] for entry in queries]
        if self.empty:
            return {"materials": [], "issues": [{"code": "knowledge_no_hits", "detail": "本次未取得可用资料。",
                                                  "target_id": target} for target in targets]}
        return {"materials": [{"material_id": "same-chunk", "title": "产品功能", "text": "支持角色权限。",
            "locator": {"target_ids": targets, "version_id": "v1",
                        "searches": [{"query": entry["query"]} for entry in queries]}}], "issues": []}


def frozen(provider, store, artifacts, *, execution_id="execution", scope="collection"):
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
    assert len(store.rows) == 1
    saved = artifacts.read_json(next(iter(store.rows.values()))["artifact_ref"])
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
    saved = artifacts.read_json(next(iter(store.rows.values()))["artifact_ref"])
    assert all("target_id" not in issue for issue in saved["issues"])


async def test_query_identity_preserves_exact_query_and_provider_scope(tmp_path):
    artifacts, store, provider = ArtifactStore(tmp_path), ReceiptStore(), Knowledge()
    for query, scope in (("权限", "first"), (" 权限 ", "first"), ("权限", "second")):
        await frozen(provider, store, artifacts, scope=scope).search([
            {"query": query, "target_id": "target"}], artifacts)
    assert [call[0]["query"] for call in provider.calls] == ["权限", " 权限 ", "权限"]
    assert len(store.rows) == 3


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
    row = next(iter(store.rows.values()))
    saved = artifacts.read_json(row["artifact_ref"])
    saved["query"] = "another query"
    row["artifact_ref"] = artifacts.put_json(saved)
    with pytest.raises(ArtifactIntegrityError):
        await reader.search([{"query": "权限", "target_id": "target"}], artifacts)


async def test_proof_query_mode_is_not_replayed_from_text_only_receipt(tmp_path):
    artifacts, store, provider = ArtifactStore(tmp_path), ReceiptStore(), Knowledge()
    reader = frozen(provider, store, artifacts)
    await reader.search([{"query": "证明", "target_id": "body"}], artifacts)
    await reader.search([{"query": "证明", "target_id": "proof", "include_original": True}], artifacts)
    await reader.search([{"query": "证明", "target_id": "new-proof", "include_original": True}], artifacts)
    assert len(provider.calls) == 2
    assert provider.calls[1][0]['include_original'] is True
    assert len(store.rows) == 2


async def test_same_query_mixed_targets_retrieves_original_only_once(tmp_path):
    artifacts, store, provider = ArtifactStore(tmp_path), ReceiptStore(), Knowledge()
    reader = frozen(provider, store, artifacts)
    result = await reader.search([
        {"query": "证明", "target_id": "body"},
        {"query": "证明", "target_id": "proof", "include_original": True}], artifacts)
    assert len(provider.calls) == 1
    assert provider.calls[0][0]['include_original'] is True
    assert result['materials'][0]['locator']['target_ids'] == ['body', 'proof']
