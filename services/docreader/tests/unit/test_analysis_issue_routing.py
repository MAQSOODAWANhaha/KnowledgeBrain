"""Issue continuations share work, never silently share resolution."""
from copy import deepcopy

import pytest

from docreader.llm.wire import WirePayload
from docreader.tender.checks.analysis import _Analysis, assign_issues
from docreader.tender.requirements import extract_items
from test_analysis_extraction import FixedModel
from test_analysis_checks import IDENTITY, Model, budget, run, scenario


def questions(context, count):
    item = next(iter(context.items.values()))
    return assign_issues([{
        "issue_id": f"question-{number}", "code": "condition_unclear",
        "detail": f"独立核对第{number}项适用条件，不得与其他问题合并。", "blocking": True,
        "range_id": item["evidence"][0]["range_id"], "item_ids": [item["item_id"]],
    } for number in range(count)], "analysis")


@pytest.mark.asyncio
async def test_recovered_optional_exception_reaches_review_with_source_and_core(tmp_path):
    data = await scenario(tmp_path, documents=[("main", "main", [("代理商须提交业绩，制造商免交。", "submission_requirement")])])
    artifacts, plan, index, _ = data
    context = _Analysis(plan, index, artifacts, IDENTITY, budget())
    frozen = next(iter(context.packs.values()))
    original = artifacts.read_json(index["packs"][0]["artifact_ref"])
    raw = {k: deepcopy(original[k]) for k in ("schema_version", "items", "relations", "dispositions", "issues")}
    raw["items"][0]["text"] = "应提交业绩。"
    raw["items"][0]["conditions/exceptions"] = ["代理商须提交，制造商免交。"]
    recovered = await extract_items(frozen, FixedModel(raw), identity=IDENTITY)
    index["packs"][0]["artifact_ref"] = artifacts.put_json(recovered)
    context = _Analysis(plan, index, artifacts, IDENTITY, budget())
    request, = context.review_requests()
    assert request["items"][0]["text"] == "应提交业绩。"
    assert "conditions/exceptions" not in request["items"][0]
    assert "代理商须提交，制造商免交。" in request["issues"][0]["detail"]
    assert request["issues"][0]["item_ids"] == [request["items"][0]["item_id"]]
    assert request["source_views"][0]["body"][0]["text"] == "代理商须提交业绩，制造商免交。"


@pytest.mark.asyncio
async def test_review_target_ledger_is_complete_without_claiming_context_completeness(tmp_path, monkeypatch):
    data = await scenario(tmp_path)
    context = _Analysis(data[1], data[2], data[0], IDENTITY, budget())
    target, contextual = list(context.items.values())[:2]
    target_range = target["evidence"][0]["range_id"]
    extra = context.views([contextual["evidence"][0]["range_id"]])[0]["body"][0]
    views = context.views
    def with_context(ranges):
        result = views(ranges)
        result[0]["supporting"].append(deepcopy(extra))
        return result
    monkeypatch.setattr(context, "views", with_context)
    payload = context.review_payload([target_range], "review-1")
    assert [d["range_id"] for d in payload["dispositions"]] == [target_range]
    assert payload["items"] == [target]
    assert extra in payload["source_views"][0]["supporting"]


@pytest.mark.asyncio
@pytest.mark.parametrize("route,expected", [(True, "reference"), (False, "local"), (None, "reference")])
async def test_continuations_share_one_task_and_preserve_each_record(tmp_path, route, expected):
    data = await scenario(tmp_path)
    context = _Analysis(data[1], data[2], data[0], IDENTITY, budget())
    context.problems = questions(context, 2)
    context.problems[1]["related_issue_ids"] = ["question-0"]
    if route is not None:
        context.problems[1]["needs_source"] = route
    original = deepcopy(context.problems)
    context.route_problems()
    routing = deepcopy(context.problem_routing)
    assert routing == [{"issue_ids": ["question-0", "question-1"], "route": expected,
                        "reason": routing[0]["reason"]}]
    assert len(context.local_candidates) == (expected == "local")
    if expected == "local":
        assert context.local_candidates[0]["issue_ids"] == ["question-0", "question-1"]
    assert context.problems == original
    context.route_problems()
    assert context.problem_routing == routing


@pytest.mark.asyncio
async def test_true_wins_and_different_questions_sharing_source_do_not_merge(tmp_path):
    data = await scenario(tmp_path)
    context = _Analysis(data[1], data[2], data[0], IDENTITY, budget())
    context.problems = questions(context, 3)
    context.problems[0]["needs_source"] = True
    context.problems[1].update(related_issue_ids=["question-0"], needs_source=False)
    context.problems[2]["needs_source"] = False
    context.route_problems()
    routes = {tuple(r["issue_ids"]): r["route"] for r in context.problem_routing}
    assert routes == {("question-0", "question-1"): "reference", ("question-2",): "local"}
    assert context.local_candidates[0]["issue_ids"] == ["question-2"]


@pytest.mark.asyncio
async def test_informational_notes_skip_work_but_missing_local_location_stays_visible(tmp_path):
    data = await scenario(tmp_path)
    context = _Analysis(data[1], data[2], data[0], IDENTITY, budget())
    context.problems = questions(context, 2)
    context.problems[0].update(code="source_formatting_note", blocking=False)
    context.problems[1].update(needs_source=False)
    del context.problems[1]["range_id"], context.problems[1]["item_ids"]
    context.route_problems()
    assert context.local_candidates == []
    assert len(context.problems) == 2
    assert any(d["route"] == "unresolved" for d in context.problem_routing)


@pytest.mark.asyncio
@pytest.mark.parametrize("links", [["absent"], ["question-0", "absent"], [], "question-0"])
async def test_invalid_optional_links_preserve_finding_and_force_unknown_route(tmp_path, links):
    data = await scenario(tmp_path)
    context = _Analysis(data[1], data[2], data[0], IDENTITY, budget())
    context.problems = questions(context, 1)
    payload = context.review_requests()[0]
    response = {"schema_version": 2, "accepted_decision_ids": [], "findings": [{
        "code": "condition_unclear", "detail": "应核对条件。", "blocking": True,
        "related_issue_ids": links, "needs_source": False,
    }]}
    original = deepcopy(response)
    value = context.validate(response, payload)
    assert len(value["findings"]) == 1
    assert "related_issue_ids" not in value["findings"][0]
    assert "needs_source" not in value["findings"][0]
    assert value["diagnostics"] and response == original


@pytest.mark.asyncio
async def test_null_optional_issue_links_do_not_erase_explicit_local_routing(tmp_path):
    data = await scenario(tmp_path)
    context = _Analysis(data[1], data[2], data[0], IDENTITY, budget())
    payload = context.review_requests()[0]
    response = {'schema_version': 2, 'accepted_decision_ids': [], 'findings': [{
        'code': 'condition_unclear', 'detail': '本地核对即可。', 'blocking': True,
        'related_issue_ids': None, 'range_id': None, 'item_ids': None, 'needs_source': False}]}
    original = deepcopy(response)
    value = context.validate(response, payload)
    assert value['findings'] == [{'code': 'condition_unclear', 'detail': '本地核对即可。',
                                 'blocking': True, 'needs_source': False}]
    assert 'diagnostics' not in value and response == original


def test_issue_link_short_references_and_feedback_roundtrip():
    payload = {"schema_version": 2, "work_unit_id": "review-long", "issues": [
        {"issue_id": "question-long", "code": "condition_unclear", "detail": "待核对", "blocking": True}]}
    wire = WirePayload("verify", payload)
    assert wire.decode({"related_issue_ids": ["q1"]}) == {"related_issue_ids": ["question-long"]}
    corrected = WirePayload("verify", {**payload, "contract_feedback": {
        "fragments": [{"value": {"related_issue_ids": ["question-long"]}}]}})
    assert corrected.payload["contract_feedback"]["fragments"][0]["value"]["related_issue_ids"] == ["q1"]


@pytest.mark.asyncio
async def test_false_routes_to_local_review_and_decision_does_not_clear_unlisted_continuation(tmp_path):
    data = await scenario(tmp_path, analysis_issue=True)
    def mutate(payload, value):
        if payload["operation"] == "review" and payload["work_unit_id"].startswith("review-") and payload["issues"]:
            prior = payload["issues"][0]
            value["findings"] = [{"code": "condition_unclear", "detail": "现有原文足以核对。", "blocking": True,
                "range_id": prior["range_id"], "related_issue_ids": [prior["issue_id"]], "needs_source": False}]
    model = Model(mutate=mutate)
    verified = await run(data, model)
    assert {p["operation"] for p in model.calls} == {"review"}
    assert verified["blocking"] is True
    assert any(p["detail"] == "现有原文足以核对。" for p in verified["issues"])
    calls = len(model.calls)
    assert await run(data, model) == verified
    assert len(model.calls) == calls


