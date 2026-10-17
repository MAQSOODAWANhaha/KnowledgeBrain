"""Version-two extraction keeps provenance and delegates business meaning to review."""

from copy import deepcopy

import pytest

from docreader.llm import ModelInfrastructureError, UncertainModelResponse
from docreader.tender.requirements import (
    CorrectionValidationError, extract_items, item_payload, validate_item_correction,
)


IDENTITY = {"run_id": "run-1", "source_version": 1, "domain_revision": 2}


def source(range_id="r1", text="交付期限为30天。", **extra):
    return {"range_id": range_id, "document_id": "main", "unit_id": range_id,
            "text": text, "locator": {"paragraph": range_id}, "kind": "paragraph",
            "start": 0, "end": len(text), **extra}


def pack(body=None, supporting=(), **extra):
    return {"pack_id": "p1", "document_id": "main", "document_version": "1",
            "body": [source()] if body is None else body,
            "supporting": list(supporting), "issues": [], **extra}


def result(items=None, dispositions=None, **extra):
    return {"schema_version": 2, "items": [{
        "item_id": "p1:i1", "kind": "obligation", "text": "应在30天内交付。",
        "evidence": [{"range_id": "r1", "quote": "交付期限为30天"}],
    }] if items is None else items,
        "dispositions": [{"range_id": "r1", "status": "items", "reason": "规定交付期限。",
                          "item_ids": ["p1:i1"]}] if dispositions is None else dispositions,
        "relations": [], "issues": [], **extra}


class FixedModel:
    def __init__(self, response):
        self.response, self.calls = response, []

    async def call(self, task, payload, images=()):
        self.calls.append((task, deepcopy(payload)))
        return deepcopy(self.response)


def test_item_payload_has_complete_versioned_envelope_and_host_problem_identity():
    problem = {"issue_id": "issue-1", "code": "missing_source", "detail": "附件不可读。", "blocking": True}
    payload = item_payload(pack(), identity=IDENTITY, issues=[problem])
    assert set(payload) == {"schema_version", "work_unit_id", "source_version", "domain_revision",
                            "source_views", "issues"}
    assert payload["schema_version"] == 2 and payload["work_unit_id"] == "p1"
    assert payload["source_version"] == 1 and payload["domain_revision"] == 2
    assert payload["source_views"][0]["body"][0]["text"] == "交付期限为30天。"
    assert "catalog" not in payload and payload["issues"] == [problem]
    payload["issues"][0]["detail"] = "changed"
    assert problem["detail"] == "附件不可读。"


def test_item_payload_retains_current_document_role_once_without_global_metadata():
    payload = item_payload(pack(), identity=IDENTITY, role="amendment")
    assert payload["source_views"][0]["role"] == "amendment"
    assert "catalog" not in payload
    assert all("role" not in entry and "locator" not in entry
               for entry in payload["source_views"][0]["body"])


def test_item_payload_retains_complete_human_scope_decisions_as_copied_input():
    decisions = [{"range_id": "r1", "decision": "local_only", "target_range_ids": [],
                  "rationale": "该句只限定当前字段，不向其他章节扩展。",
                  "evidence": [{"range_id": "r1", "quote": "交付期限为30天"}]}]
    payload = item_payload(pack(), identity=IDENTITY, scope_decisions=decisions)
    assert payload["scope_decisions"] == decisions
    payload["scope_decisions"][0]["evidence"][0]["quote"] = "changed"
    payload["scope_decisions"][0]["target_range_ids"].append("changed")
    assert decisions[0]["evidence"][0]["quote"] == "交付期限为30天"
    assert decisions[0]["target_range_ids"] == []
    assert "scope_decisions" not in item_payload(pack(), identity=IDENTITY, scope_decisions=[])


@pytest.mark.asyncio
async def test_scope_decision_reaches_extraction_without_resolving_the_source_automatically():
    text = "本表适用于本地字段；字段填写义务仍需核对。"
    decisions = [{"range_id": "r1", "decision": "local_only", "target_range_ids": [],
                  "rationale": "本句只限制本表字段，未说明后续章节范围。",
                  "evidence": [{"range_id": "r1", "quote": "本表适用于本地字段"}]}]
    response = result(items=[], dispositions=[{"range_id": "r1", "status": "unresolved",
                                               "reason": "填写义务仍需核对。", "item_ids": []}])
    model = FixedModel(response)
    extracted = await extract_items(pack([source(text=text)]), model,
                                    identity=IDENTITY, scope_decisions=decisions)
    assert model.calls[0][1]["scope_decisions"] == decisions
    assert extracted["items"] == [] and extracted["dispositions"][0]["status"] == "unresolved"
    assert any(problem["code"] == "unresolved_source_range" and problem["blocking"] for problem in extracted["issues"])


@pytest.mark.asyncio
async def test_v2_extraction_preserves_minimal_items_and_source_backed_relations():
    response = result(relations=[{
        "relation_id": "p1:l1", "from_item_id": "p1:i1", "to_item_id": "p1:i1",
        "relation": "交期适用于本事项。", "evidence": [{"range_id": "r1", "quote": "交付期限为30天"}],
    }])
    model = FixedModel(response)
    extracted = await extract_items(pack(), model, identity=IDENTITY)
    assert extracted["schema_version"] == 2 and extracted["pack_id"] == "p1"
    assert extracted["items"] == response["items"]
    assert extracted["relations"] == response["relations"]
    assert extracted["source_ranges"]["r1"]["body"] is True
    assert "text" not in extracted["source_ranges"]["r1"]
    assert model.calls == [("extract", item_payload(pack(), identity=IDENTITY))]


@pytest.mark.asyncio
async def test_table_item_quotes_an_actual_supplied_cell_without_joining_columns():
    row = source(text="交付 | 30天", kind="table_region",
                 cells=[{"column": 0, "start": 0, "end": 2},
                        {"column": 1, "start": 5, "end": 8}])
    response = result()
    response["items"][0]["evidence"][0]["quote"] = "30天"
    extracted = await extract_items(pack([row]), FixedModel(response), identity=IDENTITY)
    assert extracted["items"] == response["items"]
    response["items"][0]["evidence"][0]["quote"] = row["text"]
    rejected = await extract_items(pack([row]), FixedModel(response), identity=IDENTITY)
    assert rejected["items"] == []
    assert any(p["code"] == "evidence_quote_mismatch" for p in rejected["issues"])
    with pytest.raises(CorrectionValidationError):
        validate_item_correction(pack([row]), response)


@pytest.mark.asyncio
async def test_table_fragment_cannot_quote_the_hidden_part_of_a_cell():
    row = source(text="30天", kind="table_region", source_range_id="native", start=5, end=8,
                 cells=[{"column": 1, "text": "30天以上", "start": 5, "end": 10}])
    response = result()
    response["items"][0]["evidence"][0]["quote"] = "30天以上"
    rejected = await extract_items(pack([row]), FixedModel(response), identity=IDENTITY)
    assert rejected["items"] == []
    assert any(p["code"] == "evidence_quote_mismatch" for p in rejected["issues"])


@pytest.mark.asyncio
@pytest.mark.parametrize("kind,text", [
    ("obligation", "投标人配置的技术人员不得少于三名。"),
    ("scoring_rule", "方案完整得5分、一般得2分、未提供得0分。"),
    ("consequence_rule", "未附负责人证书复印件则否决投标。"),
    ("submission_requirement", "人员表须填写姓名与证号。"),
])
async def test_all_four_business_kinds_accept_minimal_source_backed_items(kind, text):
    response = result(items=[{"item_id": "p1:i1", "kind": kind, "text": text,
                              "evidence": [{"range_id": "r1", "quote": text}]}])
    extracted = await extract_items(pack([source(text=text)]), FixedModel(response),
                                    identity=IDENTITY)
    assert extracted["items"] == response["items"] and extracted["issues"] == []


@pytest.mark.asyncio
@pytest.mark.parametrize("change", [
    {"dispositions": []},
    {"dispositions": [{"range_id": "outside", "status": "non_item", "reason": "说明。", "item_ids": []}]},
    {"dispositions": [{"range_id": "r1", "status": "non_item", "reason": "说明。", "item_ids": []}]},
    {"issues": [{"code": "unclear", "detail": "条件不清。", "blocking": "false"}]},
])
async def test_local_model_contract_defects_keep_items_and_record_review_problems(change):
    extracted = await extract_items(pack(), FixedModel(result(**change)), identity=IDENTITY)
    assert extracted["items"] == result()["items"]
    assert extracted["dispositions"][0]["status"] == "items"
    assert extracted["issues"]


@pytest.mark.asyncio
async def test_duplicate_disposition_is_reviewed_without_discarding_items():
    response = result()
    response["dispositions"] *= 2
    extracted = await extract_items(pack(), FixedModel(response), identity=IDENTITY)
    assert extracted["items"] == response["items"]
    assert len(extracted["dispositions"]) == 1
    assert any(p["code"] == "range_disposition_incomplete" for p in extracted["issues"])


@pytest.mark.parametrize("change", [
    {"item_id": "other:i1"},
    {"item_id": "p1:i0"},
    {"evidence": [{"range_id": "r1", "quote": "不存在的证据"}]},
    {"evidence": [{"range_id": "support", "quote": "仅限第一标段"}]},
])
def test_correction_checks_protocol_identity_and_exact_body_provenance(change):
    response = result()
    response["items"][0].update(change)
    with pytest.raises(CorrectionValidationError):
        validate_item_correction(pack(supporting=[source("support", "仅限第一标段。")]), response)


def test_correction_rebuilds_host_issue_identity_without_trusting_edited_ids():
    problem = {"issue_id": "host-1", "code": "source_partial", "detail": "必要附件仍缺页。", "blocking": True}
    response = result(issues=[{**problem, "issue_id": "forged-host-id"},
        {"issue_id": "edited-new-id", "code": "condition_unclear", "detail": "交期适用条件不明。", "blocking": True}])
    corrected = validate_item_correction(pack(issues=[problem]), response)
    assert corrected["issues"][0] == problem
    assert len(corrected["issues"]) == 2
    assert "issue_id" not in corrected["issues"][1]
    assert response["issues"][0]["issue_id"] == "forged-host-id"


def test_correction_restores_global_host_issues_outside_the_local_model_contract():
    problem = {"issue_id": "source-1", "code": "source_partial", "detail": "另一附件仍缺页。",
               "document_id": "other", "range_id": "outside", "blocking": True}
    corrected = validate_item_correction(pack(), result(issues=[problem]), issues=[problem])
    assert corrected["issues"] == [problem]
    omitted = validate_item_correction(pack(), result(), issues=[problem])
    assert omitted["issues"] == [problem]


@pytest.mark.asyncio
async def test_model_still_cannot_assign_host_issue_ids():
    response = result(issues=[{"issue_id": "invented", "code": "condition_unclear",
                              "detail": "交期适用条件不明。", "blocking": True}])
    extracted = await extract_items(pack(), FixedModel(response), identity=IDENTITY)
    assert extracted["items"] == response["items"]
    assert all("issue_id" not in p for p in extracted["issues"])
    assert any("交期适用条件不明" in p["detail"] for p in extracted["issues"])


@pytest.mark.asyncio
async def test_partial_uncertainty_keeps_known_items_and_host_blockers():
    problem = {"issue_id": "host-1", "code": "source_partial", "detail": "必要附件仍缺页。", "blocking": True}
    response = result(issues=[{"code": "condition_unclear", "detail": "交期适用条件尚不明确。",
                              "range_id": "r1", "item_ids": ["p1:i1"], "blocking": False}])
    extracted = await extract_items(pack(issues=[problem]), FixedModel(response),
                                    identity=IDENTITY, issues=[problem])
    assert extracted["items"] == response["items"]
    assert extracted["dispositions"][0]["status"] == "items"
    assert all(item["blocking"] for item in extracted["issues"])
    assert sum(item.get("issue_id") == "host-1" for item in extracted["issues"]) == 1


@pytest.mark.asyncio
async def test_unresolved_source_is_blocking_without_inventing_an_item():
    response = result(items=[], dispositions=[{
        "range_id": "r1", "status": "unresolved", "reason": "引用附件尚未读取。", "item_ids": [],
    }])
    extracted = await extract_items(pack(), FixedModel(response), identity=IDENTITY)
    assert extracted["items"] == [] and extracted["dispositions"] == response["dispositions"]
    assert any(item["code"] == "unresolved_source_range" and item["blocking"] for item in extracted["issues"])


@pytest.mark.asyncio
async def test_empty_table_template_has_contextual_disposition_without_fabricated_quote():
    instruction = source("header", "人员表应填写姓名、证号并附证书。", table_id="t1", header=True)
    blank = source("blank", "", empty=True, non_requirement_reason="empty_table_row", table_id="t1",
                   row_start=1, row_end=2, column_count=2, cells=["", ""])
    response = result(items=[{"item_id": "p1:i1", "kind": "submission_requirement",
        "text": instruction["text"], "evidence": [{"range_id": "header", "quote": instruction["text"]}]}],
        dispositions=[
            {"range_id": "header", "status": "items", "reason": "规定人员表字段及证明文件。", "item_ids": ["p1:i1"]},
            {"range_id": "blank", "status": "non_item", "reason": "该空行是上述人员表的填写槽位，填写义务由表头表达，不增加独立事项。", "item_ids": []},
        ])
    model = FixedModel(response)
    extracted = await extract_items(pack([instruction, blank]), model, identity=IDENTITY)
    assert extracted["dispositions"] == response["dispositions"]
    sent = model.calls[0][1]["source_views"][0]
    assert sent["blank_table_templates"][0]["rows"][0]["range_ids"] == ["blank"]
    assert extracted["issues"] == []


@pytest.mark.asyncio
async def test_only_host_proven_blank_pages_skip_model_and_keep_source_identity():
    blank = source("blank", "", empty=True, non_requirement_reason="blank_page")
    model = FixedModel(None)
    extracted = await extract_items(pack([blank]), model, identity=IDENTITY)
    assert model.calls == [] and extracted["items"] == []
    assert extracted["dispositions"] == [{"range_id": "blank", "status": "non_item",
        "reason": "blank_page", "item_ids": []}]
    assert extracted["source_ranges"]["blank"]["body"] is True


@pytest.mark.asyncio
async def test_known_indivisible_extraction_budget_failure_keeps_unresolved_coverage_without_a_call():
    blank = source("blank", "", empty=True, non_requirement_reason="blank_page")
    problem = {"issue_id": "source-1", "code": "source_partial", "detail": "必要附件仍缺页。", "blocking": True}
    model = FixedModel(None)
    frozen = pack([source(), blank], issues=[problem], planning_details={"budget": {"fits": False}})
    extracted = await extract_items(frozen, model, identity=IDENTITY, issues=[problem])
    assert model.calls == [] and extracted["items"] == [] and extracted["relations"] == []
    assert [(entry["range_id"], entry["status"]) for entry in extracted["dispositions"]] == [
        ("r1", "unresolved"), ("blank", "non_item")]
    assert all(entry["item_ids"] == [] for entry in extracted["dispositions"])
    assert set(extracted["source_ranges"]) == {"r1", "blank"}
    assert {"extraction_budget_exceeded", "unresolved_source_range", "source_partial"} <= {
        problem["code"] for problem in extracted["issues"] if problem["blocking"]}


@pytest.mark.asyncio
@pytest.mark.parametrize("error", [ModelInfrastructureError, UncertainModelResponse])
async def test_extraction_does_not_turn_technical_or_uncertain_failures_into_business_results(error):
    class FailedModel:
        async def call(self, task, payload, images=()):
            raise error("synthetic model transport failure")

    with pytest.raises(error):
        await extract_items(pack(), FailedModel(), identity=IDENTITY)


def test_stored_host_blank_disposition_is_rebuilt_for_correction():
    blank = source("blank", "", empty=True, non_requirement_reason="blank_page")
    response = result(dispositions=[*result()["dispositions"], {
        "range_id": "blank", "status": "non_item", "reason": "blank_page", "item_ids": [],
    }])
    corrected = validate_item_correction(pack([source(), blank]), response)
    assert corrected["dispositions"] == response["dispositions"]
    response["dispositions"][-1]["reason"] = "编辑不能改写宿主已证实的空白页事实。"
    with pytest.raises(CorrectionValidationError, match="invalid_host_disposition"):
        validate_item_correction(pack([source(), blank]), response)
