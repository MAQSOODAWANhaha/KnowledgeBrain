"""Shared transport keeps semantic targets, complete evidence and failure boundaries."""

from copy import deepcopy

from docreader.tender.checks.analysis_batches import make_batches, member_payload, split_targets
from docreader.tender.schema import AnalysisBatchResponse, AnalysisSharedFinding


def logical(unit, ranges=("r1",), *, document="doc", operation="associate"):
    return {"schema_version": 2, "operation": operation, "work_unit_id": unit,
        "source_views": [{"pack_id": "analysis-source:" + document, "document_id": document,
            "document_version": "v1", "role": "main", "sections": [],
            "body": [{"range_id": r, "text": "原文 " + r} for r in ranges], "supporting": [],
            "blank_table_templates": [], "source_aliases": [], "scope_bindings": [],
            "pending_scope_context": []}],
        "items": [{"item_id": "i:" + ranges[0], "text": "事项 " + ranges[0]}],
        "relations": [], "decisions": [], "dispositions": [],
        "issues": [{"issue_id": "q:" + unit, "code": "unresolved", "detail": unit, "blocking": True}],
        "candidates": [{"candidate_id": "c:" + unit, "issue_ids": ["q:" + unit],
                        "range_ids": list(ranges), "item_ids": [], "reason": unit}]}


class Budget:
    def __init__(self, targets=4, source_limit=100):
        self.targets, self.source_limit = targets, source_limit

    def measure(self, task, payload):
        assert task == "verify"
        return {"fits": len(payload["target_work_unit_ids"]) <= self.targets and sum(
            len(v["body"]) for v in payload["source_views"]) <= self.source_limit}


def test_shared_source_and_items_are_deduplicated_without_merging_logical_questions():
    originals = [logical("a"), logical("b")]
    before = deepcopy(originals)
    batches = make_batches(originals, Budget())
    assert len(batches) == 1
    batch = batches[0]
    assert "work_unit_id" not in batch
    assert batch["target_work_unit_ids"] == ["a", "b"]
    assert [u["candidate_ids"] for u in batch["work_units"]] == [["c:a"], ["c:b"]]
    assert len(batch["source_views"][0]["body"]) == len(batch["items"]) == 1
    assert len(batch["issues"]) == len(batch["candidates"]) == 2
    assert originals == before
    member = member_payload(batch, "a")
    assert member["work_unit_id"] == "a" and member["candidates"] == originals[0]["candidates"]
    assert member["issues"] == batch["issues"] and member["source_views"] == batch["source_views"]


def test_same_text_across_different_documents_or_ranges_is_not_deduplicated():
    a, b = logical("a"), logical("b", ("r2",), document="annex")
    b["source_views"][0]["body"][0]["text"] = a["source_views"][0]["body"][0]["text"]
    batch = make_batches([a, b], Budget())[0]
    assert len(batch["source_views"]) == 2
    assert sum(len(v["body"]) for v in batch["source_views"]) == 2


def test_identical_source_in_body_and_supporting_is_shared_once_as_body():
    a, b = logical("a"), logical("b", ("r2",))
    b["source_views"][0]["supporting"] = deepcopy(a["source_views"][0]["body"])
    batch = make_batches([a, b], Budget())[0]
    assert {r["range_id"] for r in batch["source_views"][0]["body"]} == {"r1", "r2"}
    assert batch["source_views"][0]["supporting"] == []


def test_conflicting_body_and_supporting_text_does_not_enter_one_batch():
    a, b = logical("a"), logical("b", ("r2",))
    b["source_views"][0]["supporting"] = [{"range_id": "r1", "text": "冲突原文"}]
    assert len(make_batches([a, b], Budget())) == 2


def test_supporting_selection_reason_remains_visible_beside_the_same_body_source():
    a, b = logical("a"), logical("b", ("r2",))
    b["source_views"][0]["supporting"] = [{**deepcopy(a["source_views"][0]["body"][0]),
                                           "reason": "须结合本组的条件上下文"}]
    batches = make_batches([a, b], Budget())
    assert len(batches) == 1
    assert batches[0]["source_views"][0]["supporting"] == b["source_views"][0]["supporting"]


def test_conflicting_source_or_item_identity_keeps_complete_separate_requests():
    for field in ("source", "item"):
        a, b = logical("a"), logical("b")
        if field == "source":
            b["source_views"][0]["body"][0]["text"] = "同一身份的不同原文，不能覆盖"
        else:
            b["items"][0]["text"] = "同一事项的不同内容，不能覆盖"
        batches = make_batches([a, b], Budget())
        assert len(batches) == 2
        assert [member_payload(batch, batch["target_work_unit_ids"][0]) for batch in batches] == [a, b]


def test_overlap_order_is_deterministic_and_source_union_uses_frozen_order():
    originals = [logical("a", ("r3", "r4")), logical("b", ("r1",)),
                 logical("c", ("r2", "r3", "r4"))]
    options = {"source_order": lambda identity: int(identity[1:]),
               "document_order": lambda identity: 0}
    first = make_batches(originals, Budget(targets=2), **options)
    assert first == make_batches(list(reversed(originals)), Budget(targets=2), **options)
    assert first[0]["target_work_unit_ids"] == ["c", "a"]
    assert [r["range_id"] for r in first[0]["source_views"][0]["body"]] == ["r2", "r3", "r4"]


def test_generated_context_ids_sort_by_their_frozen_source_without_entering_the_source_registry():
    a, b = logical("a", ("r1",)), logical("b", ("r2",))
    a["source_views"][0]["supporting"] = [
        {"range_id": "context-later", "source_range_id": "r3", "start": 5, "end": 9, "text": "后半条件"},
        {"range_id": "context-earlier", "source_range_id": "r3", "start": 0, "end": 5, "text": "前半条件"}]
    ranks = {"r1": 0, "r2": 1, "r3": 2}
    batch = make_batches([a, b], Budget(), source_order=ranks.__getitem__)[0]
    assert [r["range_id"] for r in batch["source_views"][0]["supporting"]] == [
        "context-earlier", "context-later"]


def test_maximum_four_and_budget_split_never_drop_an_indivisible_group():
    inputs = [logical(str(i)) for i in range(9)]
    assert [len(b["work_units"]) for b in make_batches(inputs, Budget())] == [4, 4, 1]
    assert [len(b["work_units"]) for b in make_batches(inputs[:5], Budget(targets=2))] == [2, 2, 1]
    oversized = logical("large", ("r1", "r2"))
    batch = make_batches([oversized], Budget(source_limit=1))[0]
    assert member_payload(batch, "large") == oversized
    assert Budget(source_limit=1).measure("verify", batch)["fits"] is False


def test_template_alias_conditions_and_unselected_shared_decisions_survive_target_split():
    a, b = logical("a", operation="review"), logical("b", ("r2",), operation="review")
    a["source_views"][0]["source_aliases"] = [{"range_id": "alias", "segments": [
        {"range_id": "r1", "start": 0, "end": 2}]}]
    a["source_views"][0]["blank_table_templates"] = [{"table_id": "table", "column_count": 2,
        "rows": [{"row_start": 1, "row_end": 2, "range_ids": ["blank"]}]}]
    a["scope_decisions"] = b["scope_decisions"] = [{"range_id": "r1", "rationale": "条件不可丢失"}]
    for p in (a, b):
        p["decisions"] = [{"decision_id": "d:" + p["work_unit_id"], "reason": "完整提案"}]
    batch = make_batches([a, b], Budget())[0]
    pieces = split_targets(batch, Budget(targets=1))
    assert len(pieces) == 2
    for piece in pieces:
        assert piece["work_units"] == batch["work_units"]
        assert piece["source_views"] == batch["source_views"]
        assert piece["decisions"] == batch["decisions"]
        assert piece["scope_decisions"] == batch["scope_decisions"]
        unit = piece["target_work_unit_ids"][0]
        assert member_payload(piece, unit)["decisions"] == [{"decision_id": "d:" + unit,
                                                             "reason": "完整提案"}]


def test_outer_schema_preserves_bad_member_for_local_validation_and_shared_routing():
    raw = {"schema_version": 2, "work_units": [
        {"work_unit_id": "a", "relations": [], "decisions": [], "findings": []},
        {"work_unit_id": "b", "decisions": "broken"}],
        "shared_findings": [{"work_unit_ids": ["a", "b"], "finding": {
            "code": "contradiction", "detail": "同一共享依据产生反证", "blocking": True}}]}
    assert AnalysisBatchResponse.model_validate(raw).model_dump() == raw
    assert AnalysisSharedFinding.model_validate(raw["shared_findings"][0]).work_unit_ids == ["a", "b"]
