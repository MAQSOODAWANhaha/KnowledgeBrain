"""All batch members have request-local references and independent declaration namespaces."""

from copy import deepcopy
import json

from docreader.llm.budget import RequestBudget
from docreader.llm.request import request_digest
from docreader.llm.wire import WirePayload


def batch():
    return {"schema_version": 2, "operation": "associate", "work_units": [
        {"work_unit_id": "associate-alpha", "candidate_ids": ["candidate-alpha"],
         "issue_ids": [], "decision_ids": ["decision-alpha"]},
        {"work_unit_id": "associate-beta", "candidate_ids": ["candidate-beta"], "issue_ids": []}],
        "target_work_unit_ids": ["associate-beta"], "source_views": [], "items": [],
        "candidates": [{"candidate_id": "candidate-alpha"}, {"candidate_id": "candidate-beta"}]}


def test_batch_selector_lists_encode_and_each_member_can_declare_new_ids():
    wire = WirePayload("verify", batch())
    members = wire.payload["work_units"]
    assert members[0]["candidate_ids"] == ["c1"]
    assert members[0]["decision_ids"] == ["d1"]
    assert wire.payload["target_work_unit_ids"] == ["w2"]
    restored = wire.decode({"work_units": [
        {"work_unit_id": "w1", "relations": [{"relation_id": "w1:l1"}]},
        {"work_unit_id": "w2", "decisions": [{"decision_id": "w2:d1"}],
         "accepted_decision_ids": ["d1"]}],
        "shared_findings": [{"work_unit_ids": ["w1", "w2"], "finding": {"detail": "w1 原文"}}]})
    assert restored["work_units"][0]["relations"][0]["relation_id"] == "associate-alpha:l1"
    assert restored["work_units"][1]["decisions"][0]["decision_id"] == "associate-beta:d1"
    assert restored["work_units"][1]["accepted_decision_ids"] == ["decision-alpha"]
    assert restored["shared_findings"][0]["work_unit_ids"] == ["associate-alpha", "associate-beta"]
    assert restored["shared_findings"][0]["finding"]["detail"] == "w1 原文"


def test_feedback_cannot_register_unknown_namespace_or_permanent_reference():
    data = batch()
    data["contract_feedback"] = {"work_unit_id": "evil", "decision_id": "evil:d1"}
    wire = WirePayload("verify", data)
    for identity in ("w3:d1", "associate-alpha:d1", "evil:d1", "w2:d0"):
        assert wire.decode({"decision_id": identity})["decision_id"] == "!unbound:" + identity


def test_cas_sorted_json_roundtrip_preserves_wire_bytes_digest_and_admission():
    original = batch()
    restored = json.loads(json.dumps(original, sort_keys=True))
    budget = RequestBudget(prompts={"verify": "Read complete shared evidence"}, tokenizer="o200k_base")
    first = budget.request("verify", original)
    replay = budget.request("verify", restored)
    assert WirePayload("verify", original).mapping == WirePayload("verify", restored).mapping
    assert first == replay
    assert request_digest("https://example.invalid", first) == request_digest("https://example.invalid", replay)
    assert budget.measure("verify", original) == budget.measure("verify", restored)


def test_target_subset_never_renumbers_original_members_or_shared_references():
    original = batch()
    original["target_work_unit_ids"] = ["associate-alpha", "associate-beta"]
    correction = json.loads(json.dumps(deepcopy(original), sort_keys=True))
    correction["target_work_unit_ids"] = ["associate-beta"]
    before, after = WirePayload("verify", original), WirePayload("verify", correction)
    assert before.mapping == after.mapping
    assert before.payload["work_units"] == after.payload["work_units"]
    assert after.payload["target_work_unit_ids"] == ["w2"]


def test_joint_proposals_never_renumber_the_complete_associate_basis():
    original = batch()
    original["work_units"][0].pop("decision_ids")
    original["source_views"] = [{"document_id": "document-main", "body": [
        {"range_id": "range-title", "text": "第二卷 技术规范"},
        {"range_id": "range-table", "text": "设备技术参数"}], "supporting": []}]
    original["items"] = [{"item_id": "item-title", "evidence": [{"range_id": "range-title"}]},
                         {"item_id": "item-table", "evidence": [{"range_id": "range-table"}]}]
    original["issues"] = [{"issue_id": "issue-title", "range_id": "range-title"}]
    original["relations"] = [{"relation_id": "relation-original", "from_item_id": "item-title",
                              "to_item_id": "item-table"}]
    associate = WirePayload("verify", original)
    title_ref = associate.mapping["r"]["range-title"]
    joint = deepcopy(original)
    joint.update(operation="review", decisions=[{
        "decision_id": "associate-alpha:d1", "issue_ids": ["issue-title"],
        "reason": f"{title_ref}为‘第二卷 技术规范’", "evidence": [{"range_id": "range-table"}]}])
    for member in joint["work_units"]:
        member["work_unit_id"] = member["work_unit_id"].replace("associate-", "joint-")
        member["decision_ids"] = ["associate-alpha:d1"] if member["candidate_ids"] == ["candidate-alpha"] else []
    joint["target_work_unit_ids"] = ["joint-alpha"]
    joint["issues"].append({"issue_id": "new-finding", "range_id": "range-table"})
    joint["relations"].append({"relation_id": "associate-alpha:l1", "from_item_id": "item-table",
                               "to_item_id": "item-title"})
    joint["contract_feedback"] = {"rejected_response": {"range_id": "not-in-basis"}}
    review = WirePayload("verify", json.loads(json.dumps(joint, sort_keys=True)))
    for prefix in ("r", "i", "l", "c", "q", "doc"):
        for permanent, reference in associate.mapping[prefix].items():
            assert review.mapping[prefix][permanent] == reference
    assert review.payload["decisions"][0]["reason"] == f"{title_ref}为‘第二卷 技术规范’"
    assert review.mapping["w"] == {"joint-alpha": "w1", "joint-beta": "w2"}
    assert review.decode({"range_id": title_ref}) == {"range_id": "range-title"}
