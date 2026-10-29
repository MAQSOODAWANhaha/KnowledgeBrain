"""Publish a complete outline before later authoring work, without new calls."""
from copy import deepcopy

import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.budget import RequestBudget
from docreader.tender import authoring_pipeline as pipeline
from docreader.tender.schema import AuthoringInput


async def test_complete_outline_is_published_before_later_authoring_work(tmp_path, monkeypatch):
    artifacts = ArtifactStore(tmp_path)
    snapshot = AuthoringInput(schema_version=2, run_id="fixture", source_version=1, domain_revision=1).model_dump()
    outline = {"schema_version": 2, "authoring_input": snapshot, "issues": [], "chapters": [
        {"chapter_id": "chapter", "title": "技术方案", "children": [], "targets": [
            {"target_id": "target", "kind": "narrative", "purpose": "实施方案", "rule_refs": [], "source_refs": []}]}]}
    async def organize(*args, **kwargs):
        return deepcopy(outline)
    monkeypatch.setattr(pipeline, "_organize", organize)
    class StopAfterOutline(Exception):
        pass
    published = []
    async def publish(reference):
        published.append(artifacts.read_json(reference))
        raise StopAfterOutline
    async def forbidden_model_call(*args, **kwargs):
        raise AssertionError("publishing an existing outline must not send model calls")
    with pytest.raises(StopAfterOutline):
        await pipeline.generate_document(artifacts, snapshot,
            RequestBudget(model="fixture", context_tokens=131072), forbidden_model_call, on_outline=publish)
    assert published == [outline]
