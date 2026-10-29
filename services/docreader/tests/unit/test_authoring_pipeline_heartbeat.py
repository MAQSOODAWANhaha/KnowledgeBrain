"""Slow local preparation must leave the execution heartbeat responsive."""

import asyncio
from contextlib import asynccontextmanager
import json
import threading

import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.budget import RequestBudget
from docreader.tender import authoring_pipeline as pipeline
from docreader.tests.submission_responses import assemble_independent_candidates

from test_authoring_pipeline_boundaries import ordinary_response, outline, snapshot
from test_submission_organization import fixture_context, response
from test_authoring_revision import bind, finding, payload as revision_payload, response as revision_response, task, unit


@asynccontextmanager
async def heartbeat_during(monkeypatch, *names):
    loop, ready, observations = asyncio.get_running_loop(), asyncio.Queue(), []

    async def heartbeat():
        while True:
            (await ready.get()).set()

    def observe(name):
        original = getattr(pipeline, name)

        def slow(*args, **kwargs):
            progressed = threading.Event()
            loop.call_soon_threadsafe(ready.put_nowait, progressed)
            observations.append((name, progressed.wait(timeout=0.25)))
            return original(*args, **kwargs)

        monkeypatch.setattr(pipeline, name, slow)

    for name in names:
        observe(name)
    work = asyncio.create_task(heartbeat())
    try:
        yield observations
    finally:
        work.cancel()
        await asyncio.gather(work, return_exceptions=True)


def budget():
    return RequestBudget(model="fixture", context_tokens=131072, prompts={}, output_reserve=16384)


async def test_source_planning_and_assembly_loading_allow_heartbeat_without_changing_requests(monkeypatch, tmp_path):
    artifacts = ArtifactStore(tmp_path)
    context, _ = fixture_context(monkeypatch, artifacts)
    sent = []

    async def execute(task, request, validate):
        sent.append(request)
        value = json.loads(request["messages"][1]["content"][0]["text"])
        if value.get("operation") == "assemble":
            return validate(assemble_independent_candidates(value))
        return validate(response(value["sources"][0]["range_id"], value["template_parts"][0]["part_id"]))

    original = await pipeline._organize(artifacts, context, budget(), execute, [])
    expected, sent = sent, []
    async with heartbeat_during(monkeypatch, "_source_records", "load_template_part") as observed:
        actual = await pipeline._organize(artifacts, context, budget(), execute, [])
    assert {name for name, _ in observed} == {"_source_records", "load_template_part"}
    assert all(progressed for _, progressed in observed), observed
    assert actual == original and sent == expected and len(sent) == 2


@pytest.mark.parametrize("manual", [False, True])
async def test_fit_and_placeholder_loading_allow_heartbeat_without_new_calls(monkeypatch, tmp_path, manual):
    artifacts = ArtifactStore(tmp_path)
    fixed = snapshot(artifacts, [{"text": "原格式或项目背景", "heading": "响应位置"}])
    sent = []

    async def execute(task, request, validate):
        sent.append(request)
        value = json.loads(request["messages"][1]["content"][0]["text"])
        if task == "outline" and value.get("operation") != "assemble":
            result = outline(value, kind="form" if manual else "narrative")
            result["chapters"][0]["targets"][0]["manual_fill"] = manual
            return validate(result)
        return validate(ordinary_response(task, value))

    original = artifacts.read_json(await pipeline.generate_document(artifacts, fixed, budget(), execute))
    expected, sent = sent, []
    async with heartbeat_during(monkeypatch, "_load_task") as observed:
        actual = artifacts.read_json(await pipeline.generate_document(artifacts, fixed, budget(), execute))
    assert len(observed) == (1 if manual else 2)
    assert all(progressed for _, progressed in observed), observed
    assert sent == expected
    assert actual["outline_ref"] == original["outline_ref"]
    assert actual["unit_refs"] == original["unit_refs"]
    assert actual["gaps"] == original["gaps"]


async def test_all_pipeline_source_loads_share_context_repository(monkeypatch, tmp_path):
    artifacts = ArtifactStore(tmp_path)
    context, source = fixture_context(monkeypatch, artifacts)
    context.repository, context.artifacts = object(), artifacts
    seen = []

    def load_source(_artifacts, _reference, *, repository):
        seen.append(("source", repository))
        return {"document": {"role": "main"}, "unit": {"kind": "section", "text": "原文"}}

    def load_part(_artifacts, _part, *, repository):
        seen.append(("part", repository))
        return {"document": {"role": "main"}, "unit": {"kind": "section", "text": "原格式"},
                "text": "原格式", "grid": None}

    monkeypatch.setattr(pipeline, "load_outline_source", load_source)
    monkeypatch.setattr(pipeline, "load_template_part", load_part)

    async def execute(task, request, validate):
        value = json.loads(request["messages"][1]["content"][0]["text"])
        if value.get("operation") == "assemble":
            return validate(assemble_independent_candidates(value))
        return validate(response(value["sources"][0]["range_id"], value["template_parts"][0]["part_id"]))

    await pipeline._organize(artifacts, context, budget(), execute, [])
    submission = pipeline._constraint_context(context, [], [source])
    assert submission["submission_sources"][0]["text"] == "原文"
    task = {"task_id": "w1", "chapter_id": "chapter-1", "title_path": ["响应"], "targets": [
        {"target_id": "target", "kind": "form", "purpose": "填写原格式", "rule_refs": [],
         "source_refs": [source], "template_refs": [{"source_ref": source,
            "selectors": [{"range_id": "source-a", "start": 0, "end": 3}]}]}]}
    payload, _, issues = pipeline._load_task(artifacts, context, task, {"response": {"shared_arrangements": []}}, [])
    assert not issues and [s["text"] for s in payload["supports"]] == ["原文", "原格式"]
    assert [name for name, _ in seen] == ["part", "source", "source", "part"]
    assert all(repository is context.repository for _, repository in seen)


@pytest.mark.parametrize("large", [False, True])
async def test_revision_preparation_allows_heartbeat_and_retains_tokenized_admission(monkeypatch, tmp_path, large):
    from docreader.tender.authoring_revision import revise_units

    def load(selected):
        value, sources, bindings = revision_payload(selected)
        if large:
            value["supports"][0]["text"] = "服务能力" * 18000
        return value, sources, bindings

    monkeypatch.setattr(pipeline, "_load_task", load)
    request_budget = RequestBudget(model="fixture", context_tokens=131072, prompts={}, output_reserve=32768,
                                   tokenizer="o200k_base", tokenized_authoring=True)
    sent = []

    async def execute(stage, request, validate):
        sent.append((stage, request))
        assert request_budget.require_request(request, stage)["method"] == RequestBudget.TOKENIZED_METHOD
        return validate(revision_response("已有步骤形成的完整修订稿"))

    async with heartbeat_during(monkeypatch, "_load_task") as observed:
        actual, _, gaps = await revise_units(ArtifactStore(tmp_path), request_budget, execute,
            [task()], [unit()], [finding()], pipeline._load_task, bind)
    assert len(sent) == 1 and sent[0][0] == "authoring_write"
    assert actual[0]["blocks"][0]["text"] == "已有步骤形成的完整修订稿" and not gaps
    assert observed == [("_load_task", True)]
