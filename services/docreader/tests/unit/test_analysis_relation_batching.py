"""Batch relationship reviews without losing endpoints, evidence or replay identity."""

from copy import deepcopy

import pytest

from docreader.llm.budget import RequestBudget
from docreader.tender.checks.analysis import _Analysis
from test_analysis_checks import IDENTITY, Model, budget, range_ids, scenario
from test_analysis_discovery_relations import add_relations


class ReviewBudget:
    def __init__(self, limit, boundary):
        self.original = budget()
        self.limit, self.boundary = limit, boundary

    def spec(self):
        return {**self.original.spec(), "test_review_limit": self.limit, "boundary": self.boundary}

    def measure(self, task, payload):
        measured = self.original.measure(task, payload)
        oversized = len(payload["items"]) > self.limit
        if self.boundary == "admission":
            return {**measured, "fits": measured["fits"] and not oversized}
        return {**measured, "method": RequestBudget.TOKENIZED_METHOD,
                "input_tokens": _Analysis.REVIEW_INPUT_TOKENS + int(oversized)}


def context(data, admission=None):
    return _Analysis(data[1], data[2], data[0], IDENTITY, admission or budget())


async def related_sources(tmp_path, count, pairs, *, shared_evidence=False):
    documents = [(f"doc{number}", "main" if number == 0 else "attachment",
                  [(f"要求{number}须提供完整证明。", "obligation")]) for number in range(count)]
    if shared_evidence:
        documents.append(("shared", "attachment", [("上述要求须结合共同适用说明。", None)]))
    data = await scenario(tmp_path, documents=documents)
    original = context(data)
    baseline = original.review_requests()
    items = list(original.items.values())
    evidence = None
    if shared_evidence:
        source = next(value for value in original.sources.values() if value["document_id"] == "shared")
        evidence = [{"range_id": source["range_id"], "quote": source["text"]}]
    _, relations = add_relations(data, pairs=[(items[left], items[right]) for left, right in pairs],
                                evidence=evidence)
    return data, baseline, relations


@pytest.mark.asyncio
async def test_small_relationship_reviews_share_a_batch_with_complete_evidence_and_replay(tmp_path):
    data, baseline, relations = await related_sources(tmp_path, 6, [(0, 1), (2, 3), (4, 5)],
                                                    shared_evidence=True)
    current = context(data)
    before = deepcopy((current.items, current.relations))
    requests = current.review_requests()
    assert requests[:len(baseline)] == baseline
    assert len(requests) == len(baseline) + 1
    combined = requests[-1]
    assert combined["relations"] == relations
    assert {i["item_id"] for i in combined["items"]} == current.items.keys()
    assert {e["range_id"] for relation in relations for e in relation["evidence"]} <= set(
        range_ids(combined["source_views"]))
    assert all(current.budget.measure("verify", request)["fits"] for request in requests)
    assert (current.items, current.relations) == before
    model = Model()
    result = await current.execute("review", requests, model, data[3])
    replay = context(data)
    assert replay.review_requests() == requests
    assert await replay.execute("review", replay.review_requests(), model, data[3]) == result
    assert len(model.calls) == len(requests)


@pytest.mark.asyncio
@pytest.mark.parametrize("boundary", ["admission", "granularity"])
async def test_rejected_union_does_not_mark_its_additional_relations_as_reviewed(tmp_path, boundary):
    data, baseline, relations = await related_sources(tmp_path, 4, [(0, 1), (2, 3), (0, 2)])
    current = context(data, ReviewBudget(2, boundary))
    requests = current.review_requests()
    assert requests[:len(baseline)] == baseline
    supplements = requests[len(baseline):]
    # The rejected union of the first two pairs also contains the third relation.
    # It must still get its own complete review after that union is discarded.
    assert len(supplements) == 3
    assert {r["relation_id"] for p in requests for r in p["relations"]} == {
        r["relation_id"] for r in relations}
    assert all(len(p["items"]) == 2 for p in supplements)
    assert all(current.budget.measure("verify", p)["fits"] for p in supplements)
    if boundary == "granularity":
        assert all(current.budget.measure("verify", p)["input_tokens"] <= current.REVIEW_INPUT_TOKENS
                   for p in supplements)


@pytest.mark.asyncio
@pytest.mark.parametrize("boundary", ["admission", "granularity"])
async def test_indivisible_relation_keeps_its_complete_payload_and_existing_budget_exit(tmp_path, boundary):
    data, baseline, relations = await related_sources(tmp_path, 2, [(0, 1)])
    current = context(data, ReviewBudget(1, boundary))
    requests = current.review_requests()
    assert len(requests) == len(baseline) + 1
    supplement = requests[-1]
    assert supplement["relations"] == relations
    assert len(supplement["items"]) == 2
    model = Model()
    plan_ref, results = await current.execute("review", [supplement], model, data[3])
    descriptor = data[0].read_json(plan_ref)["requests"][0]
    assert descriptor["relation_ids"] == [relations[0]["relation_id"]]
    if boundary == "admission":
        assert descriptor["budget"]["fits"] is False
        assert results == [None] and not model.calls
        assert current.incomplete_requests == [supplement["work_unit_id"]]
        assert any(p["code"] == "analysis_budget_exceeded" for p in current.problems)
    else:
        assert descriptor["budget"]["fits"] is True
        assert len(model.calls) == 1 and results[0] is not None
