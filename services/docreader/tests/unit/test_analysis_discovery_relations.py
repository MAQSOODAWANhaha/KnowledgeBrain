"""Existing relation context must retain its evidence and fit complete discovery batches."""

from copy import deepcopy
from itertools import combinations

import pytest

from docreader.llm.request import ModelContractError
from docreader.llm.wire import WirePayload
from docreader.tender.checks.analysis import _Analysis
from docreader.tender.evidence.references import EvidenceRepository
from test_analysis_checks import IDENTITY, Model, OperationBudget, budget, run, scenario


def context(data, admission=None):
    artifacts, plan, index, _ = data
    return _Analysis(plan, index, artifacts, IDENTITY, admission or budget())


def add_relations(data, *, pairs=None, evidence=None):
    artifacts, _, index, _ = data
    entries = [artifacts.read_json(pack["artifact_ref"]) for pack in index["packs"]]
    items = [item for entry in entries for item in entry["items"]]
    existing = sum(len(entry["relations"]) for entry in entries)
    relations = [{"relation_id": f"original:l{number}", "from_item_id": left["item_id"],
                  "to_item_id": right["item_id"], "relation": "原有规则需结合各自适用条件核对。",
                  "evidence": deepcopy(evidence or left["evidence"])}
                 for number, (left, right) in enumerate(pairs or combinations(items, 2), existing + 1)]
    entries[0]["relations"].extend(relations)
    index["packs"][0]["artifact_ref"] = artifacts.put_json(entries[0])
    return items, relations


async def relation_evidence_scenario(tmp_path):
    data = await scenario(tmp_path, documents=[("main", "main", [
        ("人员应持A证。", "obligation"), ("技术方案完整得5分。", "scoring_rule"),
        ("关联说明的完整原文。", None), ("没有在本次请求提供的关联说明。", None)])])
    sources = EvidenceRepository(data[0]).source_index(data[1]["documents"][0]["source_index_ref"]).ranges
    citation = {"range_id": sources[2]["range_id"], "quote": sources[2]["text"]}
    items, relations = add_relations(data, evidence=[citation])
    return data, items, relations, sources


def candidate(payload, *, ranges=(), items=()):
    return {"candidate_id": payload["work_unit_id"] + ":c1", "issue_ids": [],
            "range_ids": list(ranges), "item_ids": list(items), "reason": "联合核对关联条件。"}


@pytest.mark.asyncio
async def test_discovery_supplies_complete_existing_relations_only_with_both_endpoints(tmp_path):
    data = await scenario(tmp_path)
    items, relations = add_relations(data)
    admission = OperationBudget(lambda p: p["operation"] == "locate" and len(p["items"]) > 2)
    requests = context(data, admission).locate_requests()
    covered = set()
    for payload in requests:
        ids = {item["item_id"] for item in payload["items"]}
        expected = [r for r in relations if {r["from_item_id"], r["to_item_id"]} <= ids]
        assert payload["relations"] == expected
        covered.update(r["relation_id"] for r in payload["relations"])
    assert covered == {r["relation_id"] for r in relations}
    assert {i["item_id"] for i in items} == set.union(*({i["item_id"] for i in p["items"]} for p in requests))


@pytest.mark.asyncio
async def test_relation_only_evidence_is_scoped_selectable_and_loaded_as_original_source(tmp_path):
    data, items, relations, sources = await relation_evidence_scenario(tmp_path)
    relation_range = sources[2]["range_id"]

    def locate(payload):
        assert payload["relations"] == relations
        catalog_ranges = {r for doc in payload["catalog"] for section in doc["sections"]
                          for r in section.get("range_ids", [])}
        assert catalog_ranges == {relation_range, *(e["range_id"] for i in items for e in i["evidence"])}
        return [candidate(payload, ranges=[relation_range], items=[i["item_id"] for i in items])]

    def finding(payload, value):
        if payload["operation"] == "locate":
            value["findings"] = [{"code": "relation_context_note", "detail": "关联说明需按原文核对。",
                                  "blocking": False, "range_id": relation_range}]

    model = Model(locate=locate, mutate=finding)
    result = await run(data, model)
    associated = next(p for p in model.calls if p["operation"] == "associate")
    assert associated["relations"] == relations
    assert any(entry["range_id"] == relation_range and entry["text"] == sources[2]["text"]
               for view in associated["source_views"] for entry in view["body"])
    assert result["hard_blocking"] is False
    reported = next(p for p in result["issues"] if p["code"] == "relation_context_note")
    assert reported["range_id"] == relation_range and reported["issue_id"] in result["human_issue_ids"]


@pytest.mark.asyncio
@pytest.mark.parametrize("field", ["candidate", "finding"])
async def test_relation_context_does_not_authorize_unsupplied_source_references(tmp_path, field):
    data = await scenario(tmp_path, documents=[("main", "main", [
        ("事项甲须按原条件执行。", "obligation"), ("事项乙须按原条件执行。", "obligation"),
        ("事项丙须按原条件执行。", "obligation"), ("仅供乙丙关系使用的原文。", None)])])
    sources = EvidenceRepository(data[0]).source_index(data[1]["documents"][0]["source_index_ref"]).ranges
    items = list(context(data).items.values())
    add_relations(data, pairs=[(items[1], items[2])],
                  evidence=[{"range_id": sources[3]["range_id"], "quote": sources[3]["text"]}])
    admission = OperationBudget(lambda p: p["operation"] == "locate" and len(p["items"]) > 2)
    current = context(data, admission)
    payload = next(p for p in current.locate_requests()
                   if {i["item_id"] for i in p["items"]} == {i["item_id"] for i in items[:2]})
    assert payload["relations"] == []
    hidden = sources[3]["range_id"]
    response = {"schema_version": 2, "candidates": [], "findings": []}
    if field == "candidate":
        response["candidates"] = [candidate(payload, ranges=[hidden])]
    else:
        response["findings"] = [{"code": "unknown_context", "detail": "未提供的说明不能引用。",
                                 "blocking": False, "range_id": hidden}]
    with pytest.raises(ModelContractError):
        current.validate(response, payload)


@pytest.mark.asyncio
async def test_dense_cross_block_relations_refine_complete_groups_without_losing_item_pairs(tmp_path):
    data = await scenario(tmp_path, documents=[("main", "main", [
        (f"事项{label}须遵守原有条件。", "obligation") for label in "甲乙丙丁戊己"])])
    items, relations = add_relations(data)
    admission = OperationBudget(lambda p: p["operation"] == "locate"
                                and (len(p["items"]) > 4 or len(p["relations"]) > 2))
    model = Model()
    result = await run(data, model, admission)
    requests = [p for p in model.calls if p["operation"] == "locate"]
    groups = [{i["item_id"] for i in p["items"]} for p in requests]
    assert all(any(set(pair) <= ids for ids in groups)
               for pair in combinations([i["item_id"] for i in items], 2))
    for payload, ids in zip(requests, groups, strict=True):
        assert payload["relations"] == [r for r in relations if {r["from_item_id"], r["to_item_id"]} <= ids]
        assert admission.measure("verify", payload)["fits"] is True
    assert result["blocking"] is False


@pytest.mark.asyncio
async def test_indivisible_relation_pair_keeps_budget_issue_without_sending_or_dropping_the_pair(tmp_path):
    data = await scenario(tmp_path)
    current_items = list(context(data).items.values())
    items, relations = add_relations(data, pairs=[(current_items[0], current_items[1])])
    oversized_pair = {items[0]["item_id"], items[1]["item_id"]}
    admission = OperationBudget(lambda p: p["operation"] == "locate" and bool(p["relations"]))
    model = Model()
    result = await run(data, model, admission)
    planned = data[0].read_json(result["locate_plan_ref"])["requests"]
    blocked = [p for p in planned if p["budget"]["fits"] is False]
    assert len(blocked) == 1 and set(blocked[0]["item_ids"]) == oversized_pair
    assert blocked[0]["relation_ids"] == [relations[0]["relation_id"]]
    assert not any(p["operation"] == "locate" and oversized_pair <= {i["item_id"] for i in p["items"]}
                   for p in model.calls)
    assert result["hard_blocking"] is True
    assert any(p["code"] == "analysis_budget_exceeded" for p in result["issues"])
    assert blocked[0]["work_unit_id"] in result["coverage"]["incomplete_work_unit_ids"]


@pytest.mark.asyncio
async def test_relation_only_short_references_roundtrip_and_successful_receipts_replay(tmp_path):
    data, _, relations, sources = await relation_evidence_scenario(tmp_path)
    current = context(data)
    payload = current.locate_requests()[0]
    assert payload["relations"] == relations
    wire = WirePayload("verify", payload)
    sent = wire.payload["relations"][0]
    assert sent["evidence"][0]["quote"] == sources[2]["text"]
    response = {"schema_version": 2, "findings": [], "candidates": [
        candidate(wire.payload, ranges=[sent["evidence"][0]["range_id"]],
                  items=[sent["from_item_id"], sent["to_item_id"]])]}
    accepted = current.validate(wire.decode(response), payload)
    assert accepted["candidates"][0]["range_ids"] == [sources[2]["range_id"]]
    model = Model()
    first = await run(data, model)
    calls = deepcopy(model.calls)
    replay = await run(data, model)
    assert model.calls == calls
    assert replay["locate_plan_ref"] == first["locate_plan_ref"]
    assert replay["operation_refs"] == first["operation_refs"]
