"""Frozen indexes are reused within one execution; loaded units remain isolated."""

from copy import deepcopy

from docreader.integrations.artifacts import ArtifactStore
from docreader.tender.authoring import _OutlineContext, load_outline_source
from docreader.tender.authoring_templates import load_template_part, prepare_template_parts
from docreader.tender.evidence.references import SourceIndex

from test_authoring_source_sections import frozen_source


def count_index_builds(monkeypatch):
    builds = []
    original = SourceIndex.__init__

    def counted(self, document, raw):
        builds.append(document["document_id"])
        original(self, document, raw)

    monkeypatch.setattr(SourceIndex, "__init__", counted)
    return builds


def test_execution_reuses_one_index_for_source_preparation_native_units_and_fragments(tmp_path, monkeypatch):
    artifacts = ArtifactStore(tmp_path)
    snapshot = frozen_source(artifacts)
    builds = count_index_builds(monkeypatch)
    context = _OutlineContext(artifacts, snapshot)
    assert len(builds) == 1
    _, descriptors = prepare_template_parts(artifacts, context)
    assert len(descriptors) == 3
    for part in context.template_parts.values():
        loaded = load_template_part(artifacts, part, repository=context.repository)
        assert loaded["text"]
        native = load_outline_source(artifacts, part["source_ref"], repository=context.repository)
        assert loaded["text"] in native["unit"]["text"]
    assert len(builds) == 1, "One document must not rebuild its full index for every selected paragraph."


def test_default_template_loader_uses_one_repository_for_native_source_and_selector(tmp_path, monkeypatch):
    artifacts = ArtifactStore(tmp_path)
    context = _OutlineContext(artifacts, frozen_source(artifacts))
    prepare_template_parts(artifacts, context)
    part = next(iter(context.template_parts.values()))
    builds = count_index_builds(monkeypatch)
    assert load_template_part(artifacts, part)["text"] == "第一段原文。"
    assert len(builds) == 1


def test_shared_index_never_shares_mutable_loaded_units_or_documents(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    context = _OutlineContext(artifacts, frozen_source(artifacts))
    prepare_template_parts(artifacts, context)
    part = next(iter(context.template_parts.values()))
    first = load_outline_source(artifacts, part["source_ref"], repository=context.repository)
    expected = deepcopy(first)
    first["unit"]["text"] = "不应传播的修改"
    first["unit"]["locator"]["section_ordinal"] = 999
    first["document"]["original_ref"]["sha256"] = "0" * 64
    sliced = load_template_part(artifacts, part, repository=context.repository)
    assert sliced["text"] == "第一段原文。"
    sliced["unit"]["text"] = "也不能修改冻结原文"
    assert load_outline_source(artifacts, part["source_ref"], repository=context.repository) == expected


def test_index_cache_belongs_to_each_execution_context(tmp_path, monkeypatch):
    artifacts = ArtifactStore(tmp_path)
    snapshot = frozen_source(artifacts)
    builds = count_index_builds(monkeypatch)
    first, second = _OutlineContext(artifacts, snapshot), _OutlineContext(artifacts, snapshot)
    assert first.repository is not second.repository
    prepare_template_parts(artifacts, first)
    prepare_template_parts(artifacts, second)
    for context in (first, second):
        part = next(iter(context.template_parts.values()))
        load_template_part(artifacts, part, repository=context.repository)
    assert len(builds) == 2, "A new execution must perform its own frozen-source integrity validation."
