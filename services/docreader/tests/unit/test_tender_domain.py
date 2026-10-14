import copy

import pytest

from docreader.llm import ModelInfrastructureError, ModelSemanticError
from docreader.tender.checks import verify_all, verify_pack
from docreader.tender.planning import build_plan
from docreader.tender.prompts import get_prompt
from docreader.tender.requirements import extract_pack


class FixedModel:
    def __init__(self, **responses):
        self.responses = responses
        self.calls = []

    async def call(self, task, payload, images=()):
        self.calls.append((task, copy.deepcopy(payload)))
        result = self.responses[task]
        if isinstance(result, Exception):
            raise result
        return copy.deepcopy(result)


def source(range_id="r1", text="投标人必须在30天内交付。", document_id="main", **extra):
    return {"range_id": range_id, "document_id": document_id, "unit_id": range_id,
            "text": text, "locator": {"paragraph": range_id}, "kind": "paragraph",
            "start": 0, "end": len(text), **extra}


def pack(body=None, supporting=None, pack_id="p1"):
    return {"pack_id": pack_id, "body": body if body is not None else [source()],
            "supporting": supporting or [], "issues": []}


def requirement(requirement_id="p1:q1", range_id="r1", text="投标人必须在30天内交付。", **extra):
    return {"requirement_id": requirement_id, "text": text, "subject": "投标人",
            "evidence": [{"range_id": range_id, "quote": text}], "conditions": [], "exceptions": [],
            "numbers": [{"value": "30", "unit": "天", "quote": "30天"}], "scoring": {},
            "veto_risk": False, "cross_references": [], "applicable": True,
            "applicability_basis": "招标文件对投标人的明确交付要求。", **extra}


def extraction_response(requirements=None, dispositions=None):
    return {"requirements": requirements if requirements is not None else [requirement()],
            "dispositions": dispositions if dispositions is not None else [
                {"range_id": "r1", "status": "requirements", "reason": "明确要求30天交付。",
                 "requirement_ids": ["p1:q1"]}], "issues": []}


def codes(result, key="issues"):
    return {issue["code"] for issue in result[key]}


def index(*packs, documents=None):
    return {"packs": [{"pack_id": value["pack_id"]} for value in packs], "issues": [],
            "range_count": sum(len(value["body"]) for value in packs),
            "documents": documents or [{"document_id": "main", "version": "1", "role": "main"}]}


@pytest.mark.asyncio
async def test_complete_extraction_and_separate_full_source_review():
    support = source("intro", "仅适用于第一标段；报价表金额单位为万元。")
    frozen = pack(supporting=[support])
    req = requirement(conditions=["仅适用于第一标段"], evidence=[
        {"range_id": "r1", "quote": "投标人必须在30天内交付。"},
        {"range_id": "intro", "quote": support["text"]}])
    model = FixedModel(extract=extraction_response([req]), verify={"reviewed_range_ids": ["r1", "intro"], "findings": []})
    extracted = await extract_pack(frozen, model)
    checked = await verify_pack(frozen, extracted, model)
    aggregate = verify_all(index(frozen), [extracted], [checked])
    assert not extracted["issues"]
    assert not checked["findings"]
    assert not aggregate["blocking"]
    assert [call[0] for call in model.calls] == ["extract", "verify"]
    assert model.calls[1][1]["body"] == frozen["body"]
    assert model.calls[1][1]["supporting"] == frozen["supporting"]
    assert "text" not in extracted["source_ranges"]["r1"]


@pytest.mark.asyncio
async def test_missing_and_duplicate_dispositions_become_unresolved():
    frozen = pack([source(), source("r2", "技术评分要求另见附件。")])
    response = extraction_response()
    response["dispositions"] *= 2
    result = await extract_pack(frozen, FixedModel(extract=response))
    assert len(result["dispositions"]) == 2
    assert all(item["status"] == "unresolved" for item in result["dispositions"])
    assert "range_disposition_incomplete" in codes(result)


@pytest.mark.asyncio
async def test_blank_row_compression_retains_disposition_and_partial_empty_cells():
    blank = source("blank", "", empty=True, non_requirement_reason="empty_table_row", cells=["", ""])
    partial = source("r1", "投标人必须在30天内交付。", cells=["", "30天"])
    frozen = pack([blank, partial])
    model = FixedModel(extract=extraction_response())
    result = await extract_pack(frozen, model)
    assert result["dispositions"][0] == {"range_id": "blank", "status": "non_requirement", "reason": "empty_table_row", "requirement_ids": []}
    assert len(model.calls[0][1]["body"]) == 1
    assert model.calls[0][1]["body"][0]["cells"] == ["", "30天"]
    assert not result["issues"]


@pytest.mark.asyncio
async def test_empty_pack_rows_need_no_model_extraction():
    frozen = pack([source("blank", "", empty=True, non_requirement_reason="empty_table_row")])
    model = FixedModel()
    result = await extract_pack(frozen, model)
    assert not model.calls and not result["issues"]
    assert result["dispositions"][0]["reason"] == "empty_table_row"


@pytest.mark.asyncio
async def test_verified_blank_pdf_page_keeps_coverage_and_parser_basis():
    frozen = pack([source("blank", "", empty=True, non_requirement_reason="blank_page")])
    model = FixedModel()
    result = await extract_pack(frozen, model)
    assert not model.calls and not result["issues"]
    assert result["dispositions"] == [{"range_id": "blank", "status": "non_requirement", "reason": "blank_page", "requirement_ids": []}]


@pytest.mark.asyncio
async def test_non_requirement_needs_source_grounded_reason():
    result = await extract_pack(pack(), FixedModel(extract=extraction_response([], [
        {"range_id": "r1", "status": "non_requirement", "reason": "no requirement", "requirement_ids": []}])))
    assert "invalid_disposition" in codes(result)


@pytest.mark.asyncio
async def test_semantic_failure_is_unresolved_but_infrastructure_failure_propagates():
    result = await extract_pack(pack(), FixedModel(extract=ModelSemanticError("sensitive raw response")))
    assert "extraction_semantic_failure" in codes(result)
    assert result["dispositions"][0]["status"] == "unresolved"
    assert "sensitive" not in str(result)
    with pytest.raises(ModelInfrastructureError):
        await extract_pack(pack(), FixedModel(extract=ModelInfrastructureError("offline")))


@pytest.mark.asyncio
async def test_schema_failure_never_silently_drops_body():
    result = await extract_pack(pack(), FixedModel(extract={"requirements": []}))
    assert result["dispositions"][0]["status"] == "unresolved"


@pytest.mark.asyncio
async def test_independent_review_detects_omission_from_nonrequirement_range():
    extracted = await extract_pack(pack(), FixedModel(extract=extraction_response([], [
        {"range_id": "r1", "status": "non_requirement", "reason": "A purported overview sentence only.", "requirement_ids": []}])))
    checked = await verify_pack(pack(), extracted, FixedModel(verify={"reviewed_range_ids": ["r1"], "findings": [
        {"code": "requirement_omitted", "range_id": "r1", "detail": "30天交付 is an obligation.", "blocking": True}]}))
    assert "requirement_omitted" in codes(checked, "findings")
    assert verify_all(index(pack()), [extracted], [checked])["blocking"]


@pytest.mark.asyncio
async def test_supporting_range_is_independently_reviewed_and_not_body_only_evidence():
    frozen = pack(supporting=[source("intro", "投标人必须在30天内交付。")])
    extracted = extraction_response([requirement(range_id="intro")])
    checked = await verify_pack(frozen, extracted, FixedModel(verify={"reviewed_range_ids": ["r1"], "findings": []}))
    assert {"missing_body_evidence", "source_not_independently_reviewed"} <= codes(checked, "findings")


@pytest.mark.asyncio
@pytest.mark.parametrize("change, expected", [
    ({"evidence": [{"range_id": "r1", "quote": "60天"}]}, "evidence_quote_mismatch"),
    ({"numbers": [{"value": "30", "unit": "万元", "quote": "30天"}]}, "number_or_unit_unsupported"),
    ({"numbers": [{"value": "30", "unit": "", "quote": "30天"}]}, "number_or_unit_unsupported"),
    ({"text": "投标人必须在60天内交付。"}, "requirement_number_unsupported"),
    ({"numbers": []}, "number_not_recorded"),
])
async def test_exact_evidence_and_reverse_numeric_rules(change, expected):
    checked = await verify_pack(pack(), extraction_response([requirement(**change)]), FixedModel(verify={"reviewed_range_ids": ["r1"], "findings": []}))
    assert expected in codes(checked, "findings")


@pytest.mark.asyncio
async def test_explicit_score_total_can_be_80_and_mismatch_is_blocking():
    text = "技术60分，商务20分，总分80分。"
    req = requirement(text=text, numbers=[{"value": value, "unit": "分", "quote": value + "分"} for value in ("60", "20", "80")], scoring={"total": 80, "components": [60, 20]})
    frozen = pack([source(text=text)])
    checked = await verify_pack(frozen, extraction_response([req]), FixedModel(verify={"reviewed_range_ids": ["r1"], "findings": []}))
    assert not checked["findings"]
    req["scoring"]["components"] = [60, 60]
    checked = await verify_pack(frozen, extraction_response([req]), FixedModel(verify={"reviewed_range_ids": ["r1"], "findings": []}))
    assert "explicit_score_total_mismatch" in codes(checked, "findings")
    req["scoring"] = {"points": 60}
    checked = await verify_pack(frozen, extraction_response([req]), FixedModel(verify={"reviewed_range_ids": ["r1"], "findings": []}))
    assert not checked["findings"]  # no synthetic 100-point total


@pytest.mark.asyncio
async def test_partial_pack_failure_preserves_success_and_blocks_aggregate():
    first, second = pack(), pack([source("r2", "技术要求")], pack_id="p2")
    extracted = [await extract_pack(first, FixedModel(extract=extraction_response())),
                 await extract_pack(second, FixedModel(extract=ModelSemanticError("bad JSON")))]
    checks = [{"pack_id": "p1", "findings": [], "reviewed_range_ids": ["r1"]},
              {"pack_id": "p2", "findings": [], "reviewed_range_ids": ["r2"]}]
    result = verify_all(index(first, second), extracted, checks)
    assert result["requirements"][0]["requirement_id"] == "p1:q1"
    assert result["blocking"] and result["counts"]["ranges"] == 2


@pytest.mark.asyncio
async def test_pack_identity_and_missing_independent_result_do_not_pass_by_count():
    extracted = await extract_pack(pack(), FixedModel(extract=extraction_response()))
    result = verify_all(index(pack(pack_id="other")), [extracted], [])
    assert {"extraction_pack_count_mismatch", "verification_pack_mismatch"} <= codes(result)


async def amendment_case(replaces):
    first = pack()
    text = "投标人必须在60天内交付。"
    second = pack([source("amended", text, "amendment")], pack_id="p2")
    new_req = requirement("p2:q1", "amended", text, numbers=[{"value": "60", "unit": "天", "quote": "60天"}])
    extracted = [await extract_pack(first, FixedModel(extract=extraction_response())),
                 await extract_pack(second, FixedModel(extract=extraction_response([new_req], [
                     {"range_id": "amended", "status": "requirements", "reason": "明确修改交付期限。", "requirement_ids": ["p2:q1"]}])))]
    frozen_index = index(first, second, documents=[{"document_id": "main", "version": "1", "role": "main"},
                                                 {"document_id": "amendment", "version": "2", "role": "amendment", "replaces": replaces}])
    checks = [{"pack_id": "p1", "findings": []}, {"pack_id": "p2", "findings": []}]
    return verify_all(frozen_index, extracted, checks), extracted


@pytest.mark.asyncio
async def test_explicit_amendment_replaces_only_named_range_and_preserves_original_artifact():
    result, original = await amendment_case({"document_id": "main", "range_id": "r1"})
    assert not result["blocking"]
    assert result["requirements"][0]["applicable"] is False
    assert "amendment" in result["requirements"][0]["applicability_basis"]
    assert result["requirements"][1]["applicable"] is True
    assert original[0]["requirements"][0]["applicable"] is True


@pytest.mark.asyncio
@pytest.mark.parametrize("replaces", [None, "main", {"document_id": [], "range_id": "r1"}, {"document_id": "main", "scope": "all newer documents supersede older"}])
async def test_ambiguous_amendment_and_conflicting_numbers_remain_blocking(replaces):
    result, _ = await amendment_case(replaces)
    assert result["blocking"]
    assert "potential_cross_document_conflict" in codes(result)
    assert all(req["applicable"] for req in result["requirements"])


@pytest.mark.asyncio
async def test_cyclic_amendment_targets_cannot_deactivate_all_requirements():
    frozen = pack()
    extracted = await extract_pack(frozen, FixedModel(extract=extraction_response()))
    frozen_index = index(frozen, documents=[
        {"document_id": "main", "role": "amendment", "replaces": {"document_id": "other", "range_id": "r2"}},
        {"document_id": "other", "role": "amendment", "replaces": {"document_id": "main", "range_id": "r1"}},
    ])
    result = verify_all(frozen_index, [extracted], [{"pack_id": "p1", "findings": []}])
    assert result["blocking"] and "cyclic_amendments" in codes(result)
    assert result["requirements"][0]["applicable"]


def good_item(**extra):
    return {"requirement_id": "p1:q1", "target_kind": "body", "target": "第3章/3.2 交付期限承诺",
            "action": "承诺在30天内交付并列明各阶段里程碑。", "material_needs": ["项目负责人批准的交付计划"], **extra}


@pytest.mark.asyncio
async def test_response_plan_requires_complete_action_and_materials():
    result = await build_plan([requirement()], FixedModel(plan={"items": [good_item()]}))
    assert not result["blocking"] and not result["issues"]
    result = await build_plan([requirement()], FixedModel(plan={"items": [good_item(target_kind="table_field", target="交付计划表/字段:交付期限")]}))
    assert not result["blocking"]


@pytest.mark.asyncio
@pytest.mark.parametrize("changes,expected", [
    ({"target": "技术方案"}, "plan_body_location_missing"),
    ({"action": "响应要求"}, "plan_action_placeholder"),
    ({"action": "交付计划与30天期限"}, "plan_action_placeholder"),
    ({"target_kind": "table_field", "target": "报价表"}, "plan_table_field_missing"),
    ({"material_needs": ["无需外部材料"]}, "plan_no_material_basis"),
    ({"material_needs": ["待补充"]}, "plan_material_need_missing"),
    ({"action": "承诺在约定期限内交付并列明里程碑。"}, "plan_numeric_obligation_missing"),
])
async def test_response_plan_rejects_empty_slots_and_loss_of_obligation(changes, expected):
    result = await build_plan([requirement()], FixedModel(plan={"items": [good_item(**changes)]}))
    assert expected in codes(result) and result["blocking"]


@pytest.mark.asyncio
async def test_plan_missing_duplicate_unknown_and_inapplicable_requirements():
    model = FixedModel(plan={"items": [good_item(requirement_id="obsolete"), good_item(requirement_id="unknown")]})
    result = await build_plan([requirement(), requirement("obsolete", applicable=False)], model)
    assert {"plan_requirement_missing", "plan_unknown_requirement"} <= codes(result)
    assert len(model.calls[0][1]["requirements"]) == 1
    result = await build_plan([requirement()], FixedModel(plan={"items": [good_item(), good_item()]}))
    assert "plan_duplicate_requirement" in codes(result)


def test_versioned_prompt_resources_are_present_and_input_is_data():
    for task in ("extract", "verify", "plan", "ocr"):
        content = get_prompt(task)
        assert "tender-phase1-v1" in content
        assert "source data" in content or "untrusted" in content
    with pytest.raises(ValueError):
        get_prompt("../../secrets")
