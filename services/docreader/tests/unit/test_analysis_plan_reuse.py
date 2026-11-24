"""Pure source-review preparation reuses exact inputs without changing model requests."""

from copy import deepcopy
from functools import partial
import json

import pytest

from docreader.integrations.artifacts import ArtifactIntegrityError
from docreader.llm import ModelInfrastructureError
from docreader.llm.budget import RequestBudget
from docreader.llm.request import request_digest
from docreader.tender.checks.analysis import _Analysis, _digest, verify_analysis
from test_analysis_checks import IDENTITY, Model, Receipts, budget, pdf_image_page, scenario


async def verify(data, model, admission, identity=IDENTITY):
    return await verify_analysis(data[1], data[2], data[0], model, admission,
                                 identity=identity, save=data[3])


def cached_reviews(receipts):
    matches = [key for key in receipts.saved if key[0] == "analysis-review-requests"]
    assert len(matches) == 1
    return matches[0]


async def test_source_review_replay_skips_preparation_and_preserves_exact_requests(tmp_path, monkeypatch):
    data = await scenario(tmp_path)
    admission = budget()
    direct = _Analysis(data[1], data[2], data[0], IDENTITY, admission).review_requests()
    model = Model()
    expected = await verify(data, model, admission)
    supplied = [payload for payload in model.calls if payload["operation"] == "review"]
    assert supplied == direct
    assert [request_digest("https://example.test/v1", admission.request("verify", payload))
            for payload in supplied] == [
        request_digest("https://example.test/v1", admission.request("verify", payload)) for payload in direct]
    # The ordinary CAS encoding sorts object fields. Request message text must keep
    # the original order instead, including request-local reference traversal.
    assert json.dumps(direct) != json.dumps(json.loads(json.dumps(direct, sort_keys=True)))
    data[3].saved.pop(cached_reviews(data[3]))
    rebuilt = Model()
    assert await verify(data, rebuilt, admission) == expected
    assert rebuilt.calls == []

    def unexpected_preparation(self):
        raise AssertionError("unchanged source review was prepared again")

    monkeypatch.setattr(_Analysis, "review_requests", unexpected_preparation)
    replay = Model()
    assert await verify(data, replay, admission) == expected
    assert replay.calls == []


async def test_review_recovery_preserves_a_plan_prepared_before_pdf_projection(tmp_path):
    data = await scenario(tmp_path)
    artifacts, plan, items, receipts = data
    admission = budget()
    context = _Analysis(plan, items, artifacts, IDENTITY, admission)
    pinned_requests = [context.review_payload([target]) for target in sorted(context.review_targets)]
    pinned_plan, plan_digest = context.operation_plan("review", pinned_requests)
    assert context.review_requests() != pinned_requests
    planning_digest = _digest({"planner": "source-review-requests-v4", "source_plan": plan,
                               "items_index": items, "identity": IDENTITY, "budget": admission.spec()})
    prepared_key = ("analysis-review-requests", planning_digest, json.dumps({"planning_digest": planning_digest}))
    receipts.saved[prepared_key] = artifacts.put_json({"requests_json": json.dumps(pinned_requests)})
    plan_key = ("analysis-review-plan", "", json.dumps({"plan_digest": plan_digest}))
    receipts.saved[plan_key] = artifacts.put_json(pinned_plan)

    async def save(stage, producer, *, scope="", inputs=None, model=False):
        if stage == "analysis-review-plan":
            assert inputs == {"plan_digest": plan_digest}, "a revision cannot replace its committed review plan"
        return await receipts(stage, producer, scope=scope, inputs=inputs, model=model)

    model = Model()
    result = await verify((*data[:3], save), model, admission)
    assert model.calls == pinned_requests and not result["coverage"]["missing"]
    assert result["review_plan_ref"] == receipts.saved[plan_key]


async def test_source_review_preserves_missing_page_hard_block_on_interrupted_replay(tmp_path):
    facts = [("第一页。", "obligation"), ("当页图片。", None)]
    data = await scenario(tmp_path, documents=[("main", "main", facts), ("annex", "attachment", facts)],
                          native_setup=partial(pdf_image_page, missing=True))

    def fail_annex(payload, value):
        if payload["source_views"][0]["document_id"] == "annex":
            raise ModelInfrastructureError("interrupted")

    first = Model(mutate=fail_annex)
    with pytest.raises(ModelInfrastructureError, match="interrupted"):
        await verify(data, first, budget())
    assert len(first.calls) == 2

    resumed = Model()
    result = await verify(data, resumed, budget())
    assert [payload["source_views"][0]["document_id"] for payload in resumed.calls] == ["annex"]
    assert result["hard_blocking"]
    missing_pages = {issue["issue_id"] for issue in result["issues"]
                     if issue["code"] == "analysis_page_source_unavailable"}
    assert len(missing_pages) == 2
    assert not missing_pages.intersection(result["human_issue_ids"])


@pytest.mark.parametrize("changed", ["source", "items", "scope", "prompt", "budget", "identity"])
async def test_source_review_cache_requires_complete_current_inputs(tmp_path, monkeypatch, changed):
    data = await scenario(tmp_path)
    admission = budget()
    await verify(data, Model(), admission)
    key = cached_reviews(data[3])
    # A different revision must not mistake an imported old preparation for its
    # current inputs. Other stage receipts are revision-owned and are not copied.
    receipts = Receipts(data[0])
    receipts.saved[key] = data[3].saved[key]
    data = data[0], deepcopy(data[1]), deepcopy(data[2]), receipts
    identity = deepcopy(IDENTITY)
    if changed == "source":
        data[1]["documents"][0]["file_name"] = "renamed-source.txt"
    elif changed == "items":
        entry = data[2]["packs"][0]
        value = data[0].read_json(entry["artifact_ref"])
        value["items"][0]["text"] += "核对适用条件。"
        entry["artifact_ref"] = data[0].put_json(value)
    elif changed == "scope":
        context = _Analysis(data[1], data[2], data[0], identity, admission)
        source = next(iter(context.sources.values()))
        data[1]["scope_decisions"] = [{"range_id": source["range_id"], "decision": "local_only",
            "target_range_ids": [], "rationale": "只适用于本处。",
            "evidence": [{"range_id": source["range_id"], "quote": source["text"]}]}]
    elif changed == "prompt":
        admission = RequestBudget(context_tokens=100000, output_reserve=0,
            prompts={**admission.prompts, "verify": "Read all supplied source evidence independently."})
    elif changed == "budget":
        admission = budget(100001)
    else:
        identity["domain_revision"] += 1
    prepared = []
    original = _Analysis.review_requests

    def prepare(self):
        prepared.append(True)
        return original(self)

    monkeypatch.setattr(_Analysis, "review_requests", prepare)
    await verify(data, Model(), admission, identity)
    assert prepared == [True]
    assert len([key for key in receipts.saved if key[0] == "analysis-review-requests"]) == 2


@pytest.mark.parametrize("broken", ["invalid_json", "missing_field", "missing_payload_field",
                                   "non_list_object", "non_list_null", "missing_reference"])
async def test_damaged_review_preparation_fails_without_model_calls_or_rebuilding(tmp_path, monkeypatch, broken):
    data = await scenario(tmp_path)
    admission = budget()
    await verify(data, Model(), admission)
    key = cached_reviews(data[3])
    if broken == "invalid_json":
        data[3].saved[key] = data[0].put_json({"requests_json": "["})
        error = json.JSONDecodeError
    elif broken == "missing_field":
        data[3].saved[key] = data[0].put_json({})
        error = KeyError
    elif broken == "missing_payload_field":
        data[3].saved[key] = data[0].put_json({"requests_json": "[{}]"})
        error = KeyError
    elif broken.startswith("non_list"):
        value = {} if broken == "non_list_object" else None
        data[3].saved[key] = data[0].put_json({"requests_json": json.dumps(value)})
        error = ValueError
    else:
        data[0].path(data[3].saved[key]).unlink()
        error = ArtifactIntegrityError

    def unexpected_preparation(self):
        raise AssertionError("damaged committed preparation must not be silently rebuilt")

    monkeypatch.setattr(_Analysis, "review_requests", unexpected_preparation)
    model = Model()
    with pytest.raises(error):
        await verify(data, model, admission)
    assert model.calls == []


async def revised_scenario(tmp_path):
    facts = [(f"第{i}项义务。", "obligation") for i in range(1, 7)]
    artifacts, plan, items, _ = await scenario(tmp_path, documents=[("main", "main", facts)])
    plan = {**plan, "run_identity": dict(IDENTITY)}
    identity = {**IDENTITY, "domain_revision": IDENTITY["domain_revision"] + 1}
    previous = _Analysis(plan, items, artifacts, IDENTITY, budget())
    ranges = previous._expand_review_ranges(previous.review_targets)
    assert len(ranges) == 6
    history = [previous.review_payload(ranges[i:i+2]) for i in range(0, 6, 2)]
    changed = deepcopy(items)
    entry = changed["packs"][0]
    pack = artifacts.read_json(entry["artifact_ref"])
    pack["items"][0]["text"] += "新修订说明。"
    entry["artifact_ref"] = artifacts.put_json(pack)
    current = {**plan, "run_identity": identity}
    return artifacts, plan, items, current, changed, identity, history, ranges


async def test_same_source_correction_splits_only_the_changed_historical_group(tmp_path, monkeypatch):
    artifacts, old, baseline, plan, items, identity, history, ranges = await revised_scenario(tmp_path)
    admission = budget()
    original_measure = RequestBudget.measure

    def measure(self, task, payload, *args, **kwargs):
        result = original_measure(self, task, payload, *args, **kwargs)
        enlarged = any("新修订说明" in row["text"] for row in payload["items"])
        if task == "verify" and enlarged and len(payload["dispositions"]) > 1:
            return {**result, "method": RequestBudget.TOKENIZED_METHOD, "input_tokens": 32769}
        return result

    def no_full_plan(self):
        pytest.fail("a verified historical partition should avoid full replanning")

    monkeypatch.setattr(RequestBudget, "measure", measure)
    monkeypatch.setattr(_Analysis, "review_requests", no_full_plan)
    calls = []

    async def previous_review():
        calls.append(True)
        return old, baseline, history

    receipts, model = Receipts(artifacts), Model()
    result = await verify_analysis(plan, items, artifacts, model, admission, identity=identity,
                                   save=receipts, previous_review=previous_review)
    assert not result["coverage"]["missing"]
    assert [[d["range_id"] for d in p["dispositions"]] for p in model.calls] == [
        ranges[:1], ranges[1:2], ranges[2:4], ranges[4:]]
    assert [json.dumps(p, ensure_ascii=False) for p in model.calls[-2:]] == [
        json.dumps(p, ensure_ascii=False) for p in history[-2:]]
    await verify_analysis(plan, items, artifacts, Model(), admission, identity=identity,
                          save=receipts, previous_review=previous_review)
    assert calls == [True]


@pytest.mark.parametrize("bad_history", ["missing_group", "changed_payload", "changed_source"])
async def test_unverified_history_falls_back_to_current_full_plan(tmp_path, bad_history):
    artifacts, old, baseline, plan, items, identity, history, _ = await revised_scenario(tmp_path)
    admission = budget()
    expected = _Analysis(plan, items, artifacts, identity, admission).review_requests()
    if bad_history == "missing_group":
        history.pop(1)
    elif bad_history == "changed_payload":
        history[0]["items"][0]["text"] = "历史材料不能通过重建证明。"
    else:
        old = deepcopy(old)
        old["documents"][0]["file_name"] = "historical-source.txt"

    async def previous_review():
        return old, baseline, history

    model = Model()
    result = await verify_analysis(plan, items, artifacts, model, admission, identity=identity,
                                   save=Receipts(artifacts), previous_review=previous_review)
    assert model.calls == expected
    assert not result["coverage"]["missing"]
