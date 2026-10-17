"""Localization batching preserves evidence, independent questions and pair coverage."""

from collections import Counter
from copy import deepcopy
from itertools import combinations

import pytest

from docreader.llm import ModelSemanticError
from docreader.tender.checks.analysis import _Analysis, assign_issues
from docreader.tender.schema import CatalogEntry
from test_analysis_checks import IDENTITY, Model, OperationBudget, budget, scenario


def questions(context, count):
    item = next(iter(context.items.values()))
    return assign_issues([{
        "issue_id": f"question-{number}", "code": "condition_unclear",
        "detail": f"独立核对第{number}项适用条件，不得与其他问题合并。", "blocking": True,
        "range_id": item["evidence"][0]["range_id"], "item_ids": [item["item_id"]],
    } for number in range(count)], "analysis")


@pytest.mark.asyncio
@pytest.mark.parametrize("section", [None, "shared-section"])
async def test_discovery_retains_full_items_and_only_their_source_catalog(tmp_path, section):
    data = await scenario(tmp_path, sections=[section, "unrelated"], documents=[
        ("main", "main", [("应提供人员证书。", "obligation"), ("无关目录说明。", None)]),
        ("change", "amendment", [("人员应持B证。", "obligation"), ("其他格式说明。", None)]),
        ("empty", "attachment", [("封面。", None)]),
    ])
    context = _Analysis(data[1], data[2], data[0], IDENTITY, budget())
    context.catalog[0]["page_count"] = 12
    # One item can cite multiple documents; shared/null section IDs are document-local.
    items = list(context.items.values())
    items[0]["evidence"].extend(deepcopy(items[1]["evidence"]))
    original_items, original_catalog = deepcopy(items), deepcopy(context.catalog)
    payload, = context.locate_requests()
    assert payload["items"] == original_items
    assert context.catalog == original_catalog
    assert [doc["document_id"] for doc in payload["catalog"]] == ["main", "change"]
    for doc in payload["catalog"]:
        original = next(d for d in original_catalog if d["document_id"] == doc["document_id"])
        assert {k: v for k, v in doc.items() if k != "sections"} == {
            k: v for k, v in original.items() if k != "sections"}
        assert len(doc["sections"]) == 1
        actual = doc["sections"][0]
        expected = next(s for s in original["sections"] if s["section_id"] == section)
        assert actual == {**expected, "range_ids": list(dict.fromkeys(
            e["range_id"] for item in original_items for e in item["evidence"]
            if context.sources[e["range_id"]]["document_id"] == doc["document_id"]))}
        CatalogEntry.model_validate(doc)


@pytest.mark.asyncio
async def test_reference_batches_share_full_source_and_keep_every_question_once(tmp_path):
    data = await scenario(tmp_path)
    context = _Analysis(data[1], data[2], data[0], IDENTITY, budget())
    context.problems = questions(context, 17)
    original = deepcopy(context.problems)
    requests = context.locate_requests()
    references = [p for p in requests if p["issues"]]
    assert [len(p["issues"]) for p in references] == [8, 8, 1]
    assert [q for p in references for q in p["issues"]] == original
    assert context.problems == original
    item = next(iter(context.items.values()))
    expected_views = context.views([e["range_id"] for e in item["evidence"]])
    for payload in references:
        assert payload["catalog"] == context.catalog
        assert payload["source_views"] == expected_views
        assert payload["items"] == [item]
    assert requests == context.locate_requests()
    assert context.operation_plan("locate", requests) == context.operation_plan("locate", context.locate_requests())
    model = Model()
    await context.execute("locate", requests, model, data[3])
    await context.execute("locate", requests, model, data[3])
    assert len(model.calls) == len(requests)


@pytest.mark.asyncio
async def test_reference_input_budget_splits_batches_and_preserves_oversized_question(tmp_path):
    data = await scenario(tmp_path)
    admission = OperationBudget(lambda p: len(p["issues"]) > 2 or any(
        q["issue_id"] == "question-2" for q in p["issues"]))
    context = _Analysis(data[1], data[2], data[0], IDENTITY, admission)
    context.problems = questions(context, 6)
    context.problems[2]["detail"] *= 10
    original = deepcopy(context.problems)
    references = [p for p in context.locate_requests() if p["issues"]]
    assert [len(p["issues"]) for p in references] == [2, 1, 2, 1]
    assert [q for p in references for q in p["issues"]] == original
    model = Model()
    _, results = await context.execute("locate", references, model, data[3])
    assert results[1] is None and len(model.calls) == 3
    assert any(p["code"] == "analysis_budget_exceeded" for p in context.problems)


@pytest.mark.asyncio
async def test_pair_capacity_accounts_for_both_different_local_catalogs(tmp_path):
    data = await scenario(tmp_path, documents=[("main", "main", [
        (f"条款{number}应遵守。", "obligation") for number in range(5)])])
    admission = OperationBudget(lambda p: len(p["items"]) + 3 * sum(
        len(d["sections"]) for d in p.get("catalog", [])) > 16)
    context = _Analysis(data[1], data[2], data[0], IDENTITY, admission)
    requests = context.locate_requests()
    assert all(admission.measure("verify", p)["fits"] for p in requests)
    groups = [{i["item_id"] for i in p["items"]} for p in requests]
    assert set.union(*groups) == set(context.items)
    assert all(any(set(pair) <= group for group in groups) for pair in combinations(context.items, 2))


@pytest.mark.asyncio
@pytest.mark.parametrize("count,capacity,expected", [
    (0, 4, 0), (1, 4, 1), (4, 4, 1), (5, 4, 3), (12, 4, 15),
    (1, 0, 1), (2, 1, 1),
])
async def test_discovery_omits_redundant_blocks_without_losing_items_evidence_or_pairs(
        tmp_path, count, capacity, expected):
    facts = [(f"条款{number}应遵守，另有适用例外。", "obligation") for number in range(count)]
    data = await scenario(tmp_path, documents=[("main", "main", facts or [("封面。", None)])])
    admission = OperationBudget(lambda p: len(p["items"]) > capacity)
    context = _Analysis(data[1], data[2], data[0], IDENTITY, admission)
    original_items, original_catalog = deepcopy(context.items), deepcopy(context.catalog)
    requests = context.locate_requests()
    assert len(requests) == expected
    groups = [{item["item_id"] for item in p["items"]} for p in requests]
    assert set().union(*groups) == set(original_items)
    assert all(any(set(pair) <= group for group in groups) for pair in combinations(original_items, 2))
    for payload in requests:
        assert payload["items"] == [original_items[item["item_id"]] for item in payload["items"]]
        cited = {e["range_id"] for item in payload["items"] for e in item["evidence"]}
        assert {r for doc in payload["catalog"] for section in doc["sections"]
                for r in section["range_ids"]} == cited
        assert admission.measure("verify", payload)["fits"] is (capacity >= 4)
    assert context.items == original_items and context.catalog == original_catalog
    assert requests == context.locate_requests()


@pytest.mark.asyncio
async def test_reference_batch_unions_different_items_and_deduplicates_shared_evidence(tmp_path):
    data = await scenario(tmp_path)
    context = _Analysis(data[1], data[2], data[0], IDENTITY, budget())
    context.problems = questions(context, 2)
    other = list(context.items.values())[-1]
    context.problems[1].update(item_ids=[other["item_id"]], range_id=other["evidence"][0]["range_id"])
    references = [p for p in context.locate_requests() if p["issues"]]
    assert len(references) == 1
    payload = references[0]
    expected = [next(iter(context.items.values())), other]
    assert payload["items"] == expected
    assert payload["source_views"] == context.views([e["range_id"] for i in expected for e in i["evidence"]])
    assert Counter(q["issue_id"] for q in payload["issues"]) == Counter(q["issue_id"] for q in context.problems)


@pytest.mark.asyncio
async def test_navigation_range_mapping_does_not_authorize_unseen_evidence(tmp_path):
    data = await scenario(tmp_path)
    context = _Analysis(data[1], data[2], data[0], IDENTITY, budget())
    payload = context.locate_requests()[0]
    payload["catalog"][0]["sections"][0]["range_ids"] = ["unseen-range"]
    response = {"schema_version": 2, "findings": [], "candidates": [{
        "candidate_id": payload["work_unit_id"] + ":c1", "issue_ids": [],
        "range_ids": ["unseen-range"], "item_ids": [], "reason": "待定位原文。",
    }]}
    with pytest.raises(ModelSemanticError, match="host contract"):
        context.validate(response, payload)
