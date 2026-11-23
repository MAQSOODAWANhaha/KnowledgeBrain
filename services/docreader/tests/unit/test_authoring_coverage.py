"""Retrieval plans and coverage settlement stay per requirement, row and subject."""

import json
from types import SimpleNamespace

import httpx
import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.integrations.authoring_knowledge import KnowledgeProvider
from docreader.llm.contracts import output_schema
from docreader.tender.authoring_coverage import (
    accept_assessments, build_ledger, coverage_headline, coverage_identity, fair_select, plan_queries,
    settle_coverage, task_coverage_refs, unit_fingerprint,
)
from docreader.tender.authoring_pipeline import _adopt_reviewed, _note_unassigned_rules
from docreader.tender.schema import AuthoringContentReviewResponse, AuthoringDocument, EffectiveItemRef


def ref(item_id):
    digest = "a" * 64
    return {"item_ref": {"artifact_ref": {
        "sha256": digest, "relative_path": f"{digest[:2]}/{digest}", "size": 1,
        "media_type": "application/json"}, "item_id": item_id}}


def target(identity, rules, **extra):
    value = {"target_id": identity, "kind": "narrative", "purpose": "响应要求", "rule_refs": rules,
             "source_refs": [], "source_excerpts": [], "support_origins": ["knowledge"], "manual_fill": False}
    value.update(extra)
    return value


def outline_for(*targets, intents=(), constraints=(), issues=()):
    return {"chapters": [{"targets": list(targets), "children": []}], "search_intents": list(intents),
            "constraint_rule_refs": list(constraints), "issues": list(issues)}


def tasks_for(*targets):
    return [{"task_id": "w1", "chapter_id": "c1", "title_path": ["技术"], "targets": list(targets)}]


def requirement(rule_ref, text, kind="obligation"):
    return {"rule_ref": rule_ref, "rule": {"text": text, "kind": kind}, "target_ids": []}


def queries_of(planned):
    return [item["query"] for item in planned["search"]]


def test_unassigned_notes_follow_coverage_resolutions_for_saved_outlines():
    row, constraint, excluded, missing = (ref(item) for item in ("row", "constraint", "excluded", "missing"))
    outline = outline_for(target("table", [], response_table={"rows": [{"row_id": "r", "rule_refs": [row]}]}),
                          constraints=[constraint], issues=[
                              {"code": "outline_rule_excluded", "rule_refs": [excluded]},
                              {"code": "outline_rule_unassigned", "rule_refs": [row, excluded]},
                          ])
    context = SimpleNamespace(rules=dict(zip(("row", "constraint", "excluded", "missing"),
                                            (row, constraint, excluded, missing), strict=True)))

    _note_unassigned_rules(outline, context)
    _note_unassigned_rules(outline, context)

    assert [issue["rule_refs"] for issue in outline["issues"] if issue["code"] == "outline_rule_unassigned"] == [[missing]]
    assert {entry["rule_ref"]["item_ref"]["item_id"]: entry["resolution"] for entry in build_ledger(outline)} == {
        "row": "content", "constraint": "constraint", "excluded": "excluded",
    }


def test_same_target_does_not_let_one_rule_intent_hide_another():
    first, second = ref("R1"), ref("R2")
    position = target("t", [first, second])
    requirements = {"rules": [requirement(first, "规则一"), requirement(second, "规则二的完整要求")]}
    outline = outline_for(position, intents=[{"query": "仅R1", "rule_refs": [first], "source_refs": [], "range_ids": []}])
    entries = build_ledger(outline, requirements)
    planned = plan_queries(outline, tasks_for(position), entries, requirements)
    assert queries_of(planned).count("仅R1") == 1
    assert "规则二的完整要求" in queries_of(planned)
    assert not planned["gaps"]


def test_same_text_is_shared_but_different_rows_and_subjects_stay_separate():
    rule = ref("R")
    same = {"source_ref": {"unit_id": "same"}, "selectors": [{"range_id": "same"}]}
    left = {"source_ref": {"unit_id": "left"}, "selectors": [{"range_id": "left"}]}
    right = {"source_ref": {"unit_id": "right"}, "selectors": [{"range_id": "right"}]}
    free = {"source_ref": {"unit_id": "free"}, "selectors": [{"range_id": "free"}]}
    subjects = [
        target("buyer", [], subject="买方", response_table={"rows": [
            {"row_id": "buyer-row", "rule_refs": [], "source_excerpts": [same]}]}),
        target("seller", [], subject="卖方", response_table={"rows": [
            {"row_id": "seller-row", "rule_refs": [], "source_excerpts": [same]}]}),
    ]
    technical = target("tech", [rule], response_table={"rows": [
        {"row_id": "left-row", "rule_refs": [rule], "source_excerpts": [left]},
        {"row_id": "right-row", "rule_refs": [rule], "source_excerpts": [right]},
        {"row_id": "free-row", "rule_refs": [], "source_excerpts": [free]},
    ]})
    texts = {json.dumps(same, ensure_ascii=False, sort_keys=True, separators=(",", ":")): "相同原文",
             json.dumps(left, ensure_ascii=False, sort_keys=True, separators=(",", ":")): "左行技术内容",
             json.dumps(right, ensure_ascii=False, sort_keys=True, separators=(",", ":")): "右行技术内容",
             json.dumps(free, ensure_ascii=False, sort_keys=True, separators=(",", ":")): "无规则行原文"}
    outline = outline_for(*subjects, technical)
    requirements = {"rules": [requirement(rule, "不能只用这条摘要")]}
    entries = build_ledger(outline, requirements)
    planned = plan_queries(outline, tasks_for(*subjects, technical), entries, requirements, texts)
    assert queries_of(planned).count("相同原文") == 2
    assert len({item["query"] for item in planned["search"] if item["query"] == "相同原文"}) == 1
    assert {"左行技术内容", "右行技术内容", "无规则行原文"} <= set(queries_of(planned))
    assert "不能只用这条摘要" not in queries_of(planned)
    identities = {(entry["target_id"], entry["row_id"]) for entry in entries}
    assert ("buyer", "buyer-row") in identities and ("seller", "seller-row") in identities
    assert ("tech", "left-row") in identities and ("tech", "free-row") in identities


def test_host_query_keeps_text_over_256_chars_and_marks_the_byte_limit():
    short, huge = ref("short"), ref("huge")
    long_text, huge_text = "甲" * 300, "甲" * 10000
    assert len(long_text.encode()) < 20000 and len(huge_text.encode()) > 20000
    positions = [target("short", [short]), target("huge", [huge])]
    requirements = {"rules": [requirement(short, long_text), requirement(huge, huge_text)]}
    outline = outline_for(*positions)
    entries = build_ledger(outline, requirements)
    planned = plan_queries(outline, tasks_for(*positions), entries, requirements)
    assert long_text in queries_of(planned)
    assert huge_text not in queries_of(planned)
    assert any(entry["target_id"] == "huge" and "knowledge_query_too_large" in entry["gap_codes"] for entry in entries)


def test_fair_selection_does_not_let_one_query_consume_the_only_other_hit():
    selected, omitted = fair_select({"left": ["a1", "a2", "a3"], "right": ["b1"]}, 2)
    assert selected == ["a1", "b1"]
    assert omitted == ["a2", "a3"]


def test_one_answer_does_not_settle_the_other_rules_or_rows():
    first, second = ref("R1"), ref("R2")
    position = target("t", [first, second])
    outline = outline_for(position)
    entries = build_ledger(outline, {"rules": [requirement(first, "一"), requirement(second, "二")]})
    unit = {"task_id": "w1", "target_ids": ["t"], "blocks": [
        {"text": "只回答了第一条", "target_ids": ["t"], "material_ids": ["m1"]}],
            "cell_fills": [], "row_fills": [], "attachments": [], "status": "generated"}
    digest = unit_fingerprint(unit)
    first_id = next(entry["coverage_id"] for entry in entries if entry["rule_ref"]["item_ref"]["item_id"] == "R1")
    settled = settle_coverage(entries, [unit], [{"sha256": "x"}], [{
        "coverage_id": first_id, "result": "supported", "content_refs": [{"task_id": "w1", "content_ids": ["b1"]}],
        "task_ids": ["w1"], "required_task_ids": ["w1"], "reviewed_task_ids": ["w1"],
        "unit_fps": {"w1": digest}, "review_round": 0, "scope_complete": True, "context_incomplete": False}])
    by_rule = {entry["rule_ref"]["item_ref"]["item_id"]: entry for entry in settled}
    assert by_rule["R1"]["assessment"] == "supported"
    assert by_rule["R2"]["assessment"] is None
    assert "coverage_unassessed" in by_rule["R2"]["gap_codes"]
    rows = [{"row_id": f"row-{number}", "rule_refs": [], "source_excerpts": []} for number in range(24)]
    table = target("table", [], response_table={"rows": rows})
    ledger = build_ledger(outline_for(table), {"rules": []})
    filled = {"task_id": "w1", "target_ids": ["table"], "blocks": [], "cell_fills": [], "attachments": [],
              "row_fills": [{"row_id": "row-0", "text": "仅第一行", "material_ids": []}], "status": "generated"}
    settled_rows = settle_coverage(ledger, [filled], [{"sha256": "y"}], [])
    assert not any(entry["content_refs"] for entry in settled_rows)
    assert sum(1 for entry in settled_rows if "coverage_unfilled" in entry["gap_codes"]) == 23
    assert all(entry["assessment"] != "supported" for entry in settled_rows)


def test_partial_assessment_does_not_delete_text_or_become_complete():
    unit = {"task_id": "w1", "target_ids": ["t"], "blocks": [
        {"kind": "paragraph", "text": "部分正确应答", "target_ids": ["t"]}],
            "cell_fills": [], "row_fills": [], "attachments": [], "gaps": [], "status": "generated"}
    adopted = _adopt_reviewed([unit], [{
        "code": "coverage_partial", "detail": "只回答了一部分。", "task_ids": ["w1"],
        "content_ids": ["b1"], "reject_content": False}], None, [])
    assert adopted[0]["blocks"][0]["text"] == "部分正确应答"
    entry = {"coverage_id": "c-1", "rule_ref": ref("R"), "target_id": "t", "row_id": None,
             "source_excerpts": [], "resolution": "content", "query_ids": [], "material_ids": [],
             "content_refs": [], "assessment": None, "review_status": "unreviewed", "gap_codes": []}
    digest = unit_fingerprint(unit)
    partial = {"coverage_id": "c-1", "result": "partial", "content_refs": [{"task_id": "w1", "content_ids": ["b1"]}],
               "task_ids": ["w1"], "required_task_ids": ["w1"], "reviewed_task_ids": ["w1"],
               "unit_fps": {"w1": digest}, "review_round": 0, "scope_complete": False, "context_incomplete": True}
    settled = settle_coverage([entry], [unit], [{"sha256": "z"}], [partial, {**partial, "result": "supported"}])
    assert settled[0]["assessment"] == "partial"
    assert "coverage_context_incomplete" in settled[0]["gap_codes"]
    assert settled[0]["content_refs"]


def test_bad_assessment_does_not_reject_the_valid_sibling():
    batch = [{"task_id": "w1", "content": {"blocks": [{"content_id": "b1", "text": "有依据正文"}],
              "cell_fills": [], "row_fills": [], "attachments": []}}]
    mapping = {"c1": {"coverage_id": "persistent", "scope_complete": True}}
    accepted = accept_assessments([
        {"coverage_id": "c1", "result": "supported", "content_refs": [{"task_id": "missing", "content_ids": ["b1"]}]},
        {"coverage_id": "c9", "result": "supported", "content_refs": [{"task_id": "w1", "content_ids": ["b1"]}]},
        {"coverage_id": "c1", "result": "partial", "content_refs": [{"task_id": "w1", "content_ids": ["b1"]}]},
        "not-an-assessment",
    ], mapping, batch)
    assert [item["result"] for item in accepted] == ["partial"]
    assert accepted[0]["coverage_id"] == "persistent"


def test_manual_constraint_submission_and_exclusion_are_not_forced_searches():
    manual = target("price", [], manual_fill=True, purpose="投标报价")
    background = target("background", [ref("background")], support_origins=["tender"])
    constraint, submission, excluded = ref("constraint"), ref("submission"), ref("excluded")
    placed = target("submit", [submission])
    outline = outline_for(manual, background, constraints=[constraint], issues=[{
        "code": "outline_rule_excluded", "rule_refs": [excluded], "detail": "本次不适用"}])
    outline["chapters"].append({"presentation_role": "submission_note", "targets": [placed], "children": []})
    requirements = {"rules": [
        requirement(constraint, "评分约束"), requirement(submission, "外部提交", kind="submission_requirement"),
        requirement(excluded, "不适用"), requirement(ref("missing"), "尚未落位")]}
    entries = build_ledger(outline, requirements)
    planned = plan_queries(outline, tasks_for(manual, background, placed), entries, requirements)
    assert planned["search"] == []
    resolutions = {((entry.get("rule_ref") or {}).get("item_ref") or {}).get("item_id") or entry["target_id"]: entry["resolution"]
                   for entry in entries}
    assert resolutions["price"] == "manual"
    assert resolutions["constraint"] == "constraint"
    assert resolutions["submission"] == "submission"
    assert resolutions["excluded"] == "excluded"
    assert resolutions["missing"] == "pending"
    settled = settle_coverage(entries, [], [], [])
    manual_entry = next(entry for entry in settled if entry["target_id"] == "price")
    assert manual_entry["review_status"] == "not_required"
    assert manual_entry["assessment"] is None
    assert "coverage_unfilled" in manual_entry["gap_codes"]


def test_old_document_without_coverage_is_unknown_and_new_schema_is_optional():
    assert coverage_headline(None)["headline"] == "覆盖未知"
    assert coverage_headline({"entries": []})["headline"] == "草稿已生成，0项待补、0项待核"
    assert AuthoringDocument.model_fields["coverage_ref"].default is None
    content = output_schema("authoring_review", {})
    saved = output_schema("authoring_review", {"operation": "review_saved_word"})
    structure = output_schema("authoring_structure", {"operation": "structure"})
    assert content["title"] == "AuthoringContentReviewResponse"
    assert "assessments" in content["properties"] and "assessments" not in content["required"]
    assert "assessments" not in saved["properties"]
    assert "assessments" not in structure["properties"]
    assert AuthoringContentReviewResponse.model_validate({"findings": []}).assessments == []


def test_headline_counts_open_items_without_calling_the_export_complete():
    headline = coverage_headline({"entries": [
        {"resolution": "content", "gap_codes": ["knowledge_no_hits"], "review_status": "unreviewed", "content_refs": []},
        {"resolution": "manual", "gap_codes": ["coverage_unfilled"], "review_status": "not_required", "content_refs": []},
    ]})
    assert headline["headline"] == "草稿已生成，2项待补、1项待核"
    assert "全部要求已完成" not in headline["headline"]


@pytest.mark.parametrize("query, sent", [("甲" * 300, True), ("甲" * 10000, False)])
async def test_provider_sends_long_text_and_stops_before_the_byte_limit(tmp_path, query, sent):
    calls = []

    async def handler(request):
        calls.append(json.loads(request.content)["query"])
        return httpx.Response(200, json={"hits": [], "warnings": []})

    provider = KnowledgeProvider(base_url="https://knowledge.invalid", bearer="test-secret",
                                 product_id="4c78bf65-9ee2-4788-bc0c-f14769bb153f",
                                 transport=httpx.MockTransport(handler))
    result = await provider.search([{"target_id": "t", "query": query}], ArtifactStore(tmp_path))
    assert (query in calls) is sent
    if not sent:
        assert result["issues"][0]["code"] == "knowledge_query_too_large"
        assert calls == []


def test_coverage_and_unassigned_notes_unify_default_null_rule_ref_fields():
    sparse_form, sparse_row, sparse_proof = ref("form-1"), ref("row-1"), ref("proof-1")
    sparse_constraint, sparse_excluded, sparse_pending = ref("constraint-1"), ref("excluded-1"), ref("pending-1")
    dec_a = {**ref("shared-1"), "decision_id": "dec-1", "rule_index": 0}
    dec_b = {**ref("shared-1"), "decision_id": "dec-2", "rule_index": 0}
    dec_a_second = {**ref("shared-1"), "decision_id": "dec-1", "rule_index": 1}

    def dense(reference):
        return EffectiveItemRef.model_validate(reference).model_dump()

    assert coverage_identity(sparse_form, "t-form", None) == coverage_identity(dense(sparse_form), "t-form", None)
    assert coverage_identity(dec_a, "t-proof", None) != coverage_identity(dec_b, "t-proof", None)
    assert coverage_identity(dec_a, "t-proof", None) != coverage_identity(dec_a_second, "t-proof", None)

    form_target = target("t-form", [dense(sparse_form)], kind="form", response_table={"rows": [
        {"row_id": "r1", "rule_refs": [dense(sparse_row)], "source_excerpts": []}]})
    proof_target = target("t-proof", [dense(sparse_proof), dense(dec_a)], kind="attachment")
    outline = outline_for(
        form_target,
        proof_target,
        intents=[{"query": "证明材料原件", "rule_refs": [sparse_proof], "source_refs": [], "range_ids": []}],
        constraints=[dense(sparse_constraint)],
        issues=[{"code": "outline_rule_excluded", "rule_refs": [dense(sparse_excluded)]}],
    )
    requirements = {"rules": [
        requirement(sparse_form, "必交表完整要求"),
        requirement(sparse_row, "表格条款完整要求"),
        requirement(sparse_proof, "证明材料完整要求"),
        requirement(sparse_constraint, "编制约束"),
        requirement(sparse_excluded, "不适用条款"),
        requirement(sparse_pending, "尚未落位要求"),
        requirement(dec_a, "决议一第零条"),
        requirement(dec_b, "决议二第零条"),
        requirement(dec_a_second, "决议一第一条"),
    ]}

    entries = build_ledger(outline, requirements)
    by_identity = {}
    for entry in entries:
        rule_ref = entry["rule_ref"]
        key = (rule_ref["item_ref"]["item_id"], rule_ref.get("decision_id"), rule_ref.get("rule_index"))
        assert key not in by_identity, f"duplicate coverage entry for {key}"
        by_identity[key] = (entry["resolution"], entry["target_id"], entry["row_id"])

    assert by_identity == {
        ("form-1", None, None): ("content", "t-form", None),
        ("row-1", None, None): ("content", "t-form", "r1"),
        ("proof-1", None, None): ("content", "t-proof", None),
        ("constraint-1", None, None): ("constraint", None, None),
        ("excluded-1", None, None): ("excluded", None, None),
        ("pending-1", None, None): ("pending", None, None),
        ("shared-1", "dec-1", 0): ("content", "t-proof", None),
        ("shared-1", "dec-2", 0): ("pending", None, None),
        ("shared-1", "dec-1", 1): ("pending", None, None),
    }

    planned = plan_queries(outline, tasks_for(form_target, proof_target), entries, requirements)
    assert "必交表完整要求" in queries_of(planned)
    assert "证明材料原件" in queries_of(planned)
    assert "决议一第零条" in queries_of(planned)
    assert not any(gap["code"] == "knowledge_query_missing" and gap["target_id"] == "t-proof" for gap in planned["gaps"])

    refs_map = {"rule-1": sparse_form, "rule-2": sparse_proof, "rule-3": dec_a}
    payload = {"targets": [
        {"target_id": "t1", "rule_ids": ["rule-1"]},
        {"target_id": "t2", "rule_ids": ["rule-2", "rule-3"]},
    ], "response_rows": [{"row_id": "q1", "rule_ids": []}]}
    coverage_refs = task_coverage_refs(tasks_for(form_target, proof_target)[0], payload, refs_map)
    assert coverage_identity(sparse_form, "t-form", None) in coverage_refs
    assert coverage_identity(sparse_proof, "t-proof", None) in coverage_refs
    assert coverage_identity(dec_a, "t-proof", None) in coverage_refs

    context = SimpleNamespace(rules={
        "rule-1": sparse_form, "rule-2": sparse_row, "rule-3": sparse_proof,
        "rule-4": sparse_constraint, "rule-5": sparse_excluded, "rule-6": sparse_pending,
        "rule-7": dec_a, "rule-8": dec_b, "rule-9": dec_a_second,
    })
    _note_unassigned_rules(outline, context)
    unassigned = [
        (r["item_ref"]["item_id"], r.get("decision_id"), r.get("rule_index"))
        for issue in outline["issues"] if issue["code"] == "outline_rule_unassigned"
        for r in issue["rule_refs"]
    ]
    assert sorted(unassigned) == [
        ("pending-1", None, None),
        ("shared-1", "dec-1", 1),
        ("shared-1", "dec-2", 0),
    ]
