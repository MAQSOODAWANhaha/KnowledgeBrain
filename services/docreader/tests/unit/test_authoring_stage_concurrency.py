"""Source jobs own their validation state and settle in deterministic plan order."""

from copy import deepcopy
import asyncio
import hashlib
import json

import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.integrations.store import Conflict
from docreader.llm.budget import RequestBudget
from docreader.llm.request import ModelBudgetError, ModelWorkUnitExhausted
from docreader.tender.authoring import _OutlineContext
import docreader.tender.authoring_pipeline as pipeline
from docreader.tests.submission_responses import assemble_independent_candidates

from test_authoring_source_sections import frozen_source, source_response


def source_context(monkeypatch, tmp_path):
    artifacts = ArtifactStore(tmp_path)
    context = _OutlineContext(artifacts, frozen_source(artifacts))
    fits = pipeline._fits

    def one_source(budget, task, payload, **kwargs):
        if task == "outline" and "sources" in payload:
            return len(payload["sources"]) <= 1
        return fits(budget, task, payload, **kwargs)

    monkeypatch.setattr(pipeline, "_fits", one_source)
    return artifacts, context, RequestBudget(model="fixture", context_tokens=131072)


async def test_source_validators_are_frozen_and_do_not_mutate_shared_read_state(monkeypatch, tmp_path):
    artifacts, context, budget = source_context(monkeypatch, tmp_path)
    callbacks, results, observed = [], [], []

    async def execute(task, request, validate):
        payload = json.loads(request["messages"][1]["content"][0]["text"])
        if payload.get("operation") == "assemble":
            # Retrying an earlier callback after every source batch has run must
            # still decode that batch's aliases and bind its original template.
            for callback, raw, expected in callbacks:
                assert callback(deepcopy(raw)) == expected
            return validate(assemble_independent_candidates(payload))
        raw = source_response(payload)
        before = set(context.read_units)
        value = validate(raw)
        observed.append((before, set(context.read_units)))
        callbacks.append((validate, deepcopy(raw), deepcopy(value)))
        results.append(value)
        return value

    await pipeline._organize(artifacts, context, budget, execute, [])
    assert len(results) == 3
    assert all(before == after for before, after in observed)
    assert all(not any(issue["code"] == "outline_source_not_read" for issue in value["issues"])
               for value in results)
    assert context.read_units == {(ref["source_index_ref"]["sha256"], ref["unit_id"])
                                  for refs in context.sources.values() for ref in refs}


async def test_source_concurrency_preserves_requests_and_plan_order(monkeypatch, tmp_path):
    artifacts, original_context, budget = source_context(monkeypatch, tmp_path)

    async def run(concurrency):
        context = _OutlineContext(artifacts, original_context.input)
        second_finished = asyncio.Event()
        requests, finished = [], []
        active = peak = 0

        async def execute(task, request, validate):
            nonlocal active, peak
            payload = json.loads(request["messages"][1]["content"][0]["text"])
            requests.append(deepcopy(request))
            if payload.get("operation") == "assemble":
                return validate(assemble_independent_candidates(payload))
            text = payload["sources"][0]["text"]
            active += 1
            peak = max(peak, active)
            try:
                if concurrency > 1 and text == "第一段原文。":
                    await asyncio.wait_for(second_finished.wait(), timeout=2)
                await asyncio.sleep(0)
                result = validate(source_response(payload))
                finished.append(text)
                if text == "第二段原文。":
                    second_finished.set()
                return result
            finally:
                active -= 1

        gaps = []
        result = await pipeline._organize(artifacts, context, budget, execute, gaps, concurrency=concurrency)
        return result, gaps, requests, finished, peak

    serial, concurrent = await run(1), await run(2)
    assert serial[:3] == concurrent[:3]
    assert serial[3] == ["第一段原文。", "第二段原文。", "另一完整章节原文。"]
    assert concurrent[3].index("第二段原文。") < concurrent[3].index("第一段原文。")
    assert serial[4] == 1 and concurrent[4] == 2
    def digest(value):
        return hashlib.sha256(pipeline._json(value).encode()).hexdigest()
    assert digest(serial[0]) == digest(concurrent[0])


@pytest.mark.parametrize("error", [ModelWorkUnitExhausted("outline"), ModelBudgetError("too large")])
async def test_source_local_failure_settles_independent_siblings(monkeypatch, tmp_path, error):
    artifacts, context, budget = source_context(monkeypatch, tmp_path)
    seen = []

    async def execute(task, request, validate):
        payload = json.loads(request["messages"][1]["content"][0]["text"])
        if payload.get("operation") == "assemble":
            return validate(assemble_independent_candidates(payload))
        text = payload["sources"][0]["text"]
        seen.append(text)
        await asyncio.sleep(0)
        if text == "第一段原文。":
            raise error
        return validate(source_response(payload))

    gaps = []
    result = await pipeline._organize(artifacts, context, budget, execute, gaps, concurrency=2)
    assert len(seen) == 3
    assert [gap["code"] for gap in gaps] == ["outline_batch_unavailable"]
    excerpts = [ref for chapter in pipeline._flatten(result["chapters"])
                for target in chapter["targets"] for ref in target.get("source_excerpts", [])]
    assert len(excerpts) == 2


@pytest.mark.parametrize("error", [Conflict("lease lost"), OSError("storage failed")])
async def test_source_infrastructure_failure_stops_assignment_and_awaits_siblings(monkeypatch, tmp_path, error):
    artifacts, context, budget = source_context(monkeypatch, tmp_path)
    second_started, settled = asyncio.Event(), asyncio.Event()
    started = []

    async def execute(task, request, validate):
        payload = json.loads(request["messages"][1]["content"][0]["text"])
        assert "operation" not in payload
        text = payload["sources"][0]["text"]
        started.append(text)
        if text == "第一段原文。":
            await second_started.wait()
            raise error
        second_started.set()
        try:
            await asyncio.Future()
        finally:
            await asyncio.sleep(0)
            settled.set()

    gaps = []
    with pytest.raises(type(error)) as caught:
        await asyncio.wait_for(pipeline._organize(artifacts, context, budget, execute, gaps, concurrency=2), 2)
    assert caught.value is error
    assert len(started) == 2 and settled.is_set()
    assert gaps == []


async def test_source_cancellation_awaits_all_inflight_work(monkeypatch, tmp_path):
    artifacts, context, budget = source_context(monkeypatch, tmp_path)
    both_started = asyncio.Event()
    started, settled = [], []

    async def execute(task, request, validate):
        payload = json.loads(request["messages"][1]["content"][0]["text"])
        text = payload["sources"][0]["text"]
        started.append(text)
        if len(started) == 2:
            both_started.set()
        try:
            await asyncio.Future()
        finally:
            await asyncio.sleep(0)
            settled.append(text)

    running = asyncio.create_task(pipeline._organize(artifacts, context, budget, execute, [], concurrency=2))
    try:
        await asyncio.wait_for(both_started.wait(), 2)
        running.cancel()
        with pytest.raises(asyncio.CancelledError):
            await running
        assert len(started) == 2 and sorted(settled) == sorted(started)
    finally:
        running.cancel()
        await asyncio.gather(running, return_exceptions=True)


@pytest.mark.parametrize("concurrency", [1, 2])
async def test_generate_document_compiles_a_directory_instead_of_organizing(monkeypatch, tmp_path, concurrency):
    artifacts, context, budget = source_context(monkeypatch, tmp_path)

    async def organize(*_args, **_kwargs):
        raise AssertionError("model organization no longer builds the directory")

    async def execute(task, _request, validate):
        if task == "directory_plan":
            return validate({"chapters": []})
        raise AssertionError(task)

    monkeypatch.setattr(pipeline, "_organize", organize)
    reference = await pipeline.generate_document(
        artifacts, context.input, budget, execute, outline_only=True, concurrency=concurrency)
    outline = artifacts.read_json(reference)
    assert "outline_empty" in {issue["code"] for issue in outline["issues"]}


async def test_authoring_review_runs_multiple_batches_concurrently_and_preserves_order(tmp_path):
    from docreader.integrations.artifacts import ArtifactStore
    from docreader.llm.budget import RequestBudget
    from docreader.tender.schema import AuthoringInput

    artifacts = ArtifactStore(tmp_path)
    snapshot = AuthoringInput.model_validate({
        "schema_version": 2, "run_id": "rev-conc", "source_version": 1, "domain_revision": 1,
    }).model_dump()
    outline = {
        "schema_version": 2,
        "authoring_input": snapshot,
        "issues": [],
        "chapters": [
            {
                "chapter_id": f"ch-{idx}",
                "title": f"方案章节{idx}",
                "children": [],
                "is_volume": False,
                "presentation_role": "body",
                "targets": [{
                    "target_id": f"ch-{idx}:t1",
                    "kind": "narrative",
                    "purpose": f"编制方案章节{idx}",
                    "manual_fill": False,
                    "support_origins": ["knowledge"],
                    "rule_refs": [],
                    "source_refs": [],
                }],
            }
            for idx in (1, 2)
        ],
    }
    materials = [
        {"material_id": f"m-{idx}", "title": f"资料{idx}", "text": ("企业服务规范正文内容" * 260),
         "usage": "reference", "attachment": False, "locator": {"target_ids": [f"ch-{idx}:t1"]}}
        for idx in (1, 2)
    ]

    in_flight = 0
    max_review_in_flight = 0
    both_reviews = asyncio.Event()

    async def execute(task, request, validate):
        nonlocal in_flight, max_review_in_flight
        body = json.loads(request["messages"][1]["content"][0]["text"])
        if task == "authoring_structure":
            return validate({"findings": []})
        if task == "authoring_basis":
            return validate({"shared_arrangements": [], "gaps": []})
        if task == "authoring_write":
            tid = body["targets"][0]["target_id"]
            sid = body["targets"][0]["support_ids"][0]
            return validate({
                "blocks": [{"kind": "paragraph", "text": "依据企业规范编制。" * 40, "target_ids": [tid], "support_ids": [sid]}],
                "cell_fills": [], "row_fills": [], "attachment_ids": [], "gaps": [],
            })
        assert task == "authoring_review"
        in_flight += 1
        max_review_in_flight = max(max_review_in_flight, in_flight)
        if in_flight >= 2:
            both_reviews.set()
        await asyncio.wait_for(both_reviews.wait(), timeout=2.0)
        in_flight -= 1
        wids = [unit["task_id"] for unit in body["units"]]
        return validate({"findings": [{
            "code": "review_note", "detail": f"checked-{wids[0]}", "task_ids": wids,
            "content_ids": [], "suggestion": "", "can_revise": False, "reject_content": False,
        }]})

    # Short prompts + tight budget forces 2 separate review batches (1 writing unit per review batch).
    prompts = {
        task: "Return JSON."
        for task in ("authoring_structure", "authoring_basis", "authoring_write", "authoring_review")
    }
    budget = RequestBudget(context_tokens=16000, output_reserve=1000, model="unused", prompts=prompts)
    doc_ref = await pipeline.generate_document(
        artifacts, snapshot, budget, execute,
        outline=outline, materials=materials, concurrency=2,
    )
    doc = artifacts.read_json(doc_ref)
    review = artifacts.read_json(doc["review_ref"])
    assert max_review_in_flight == 2
    assert [f["detail"] for f in review["findings"]] == ["checked-w1", "checked-w2"]
