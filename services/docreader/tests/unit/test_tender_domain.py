import copy

import pytest

from docreader.llm import ModelInfrastructureError, ModelSemanticError
from docreader.tender.checks import apply_applicability_decisions, verify_all, verify_pack
from docreader.tender.planning import build_plan, constraint_binding
from docreader.tender.prompts import get_prompt
from docreader.tender.requirements import CorrectionValidationError, extract_pack, extract_payload, validate_correction


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
            "documents": documents or [{"document_id": "main", "document_version": "1", "role": "main"}]}


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
    assert model.calls[1][1]["body"] == extract_payload(frozen)["body"]
    assert model.calls[1][1]["supporting"] == extract_payload(frozen)["supporting"]
    assert model.calls[1][1]["body"][0]["text"] == frozen["body"][0]["text"]
    assert "text" not in extracted["source_ranges"]["r1"]


@pytest.mark.asyncio
async def test_reviewed_scope_targets_survive_when_all_condition_text_is_in_body():
    frozen = pack()
    frozen["scope_bindings"] = [{
        "range_id": "condition-1", "source_range_id": "r1", "start": 0, "end": len(frozen["body"][0]["text"]),
        "scope_binding": {"kind": "manual_review", "status": "bound", "complete_sentence": True,
                          "applies_to_range_ids": ["r1"], "reviewed_target_range_ids": ["r1"]},
    }]
    model = FixedModel(extract=extraction_response(), verify={"reviewed_range_ids": ["r1"], "findings": []})
    extracted = await extract_pack(frozen, model)
    checked = await verify_pack(frozen, extracted, model)
    assert not extracted["issues"] and not checked["findings"]
    for _, payload in model.calls:
        assert payload["scope_bindings"] == frozen["scope_bindings"]
        assert payload["supporting"] == []
        assert "text" not in payload["scope_bindings"][0]
    assert checked["reviewed_range_ids"] == ["r1"]


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
    blank = source("blank", "", empty=True, non_requirement_reason="empty_table_row", cells=["", ""],
                   table_id="table-1", row_start=1, row_end=2, column_count=2)
    partial = source("r1", "投标人必须在30天内交付。", cells=["", "30天"])
    frozen = pack([blank, partial])
    response = extraction_response()
    response["dispositions"].append({"range_id": "blank", "status": "non_requirement",
        "reason": "Reviewed template fields inherit the stated delivery obligation; no additional row text.",
        "requirement_ids": []})
    model = FixedModel(extract=response, verify={"reviewed_range_ids": ["blank", "r1"], "findings": []})
    result = await extract_pack(frozen, model)
    checked = await verify_pack(frozen, result, model)
    assert result["dispositions"][0] == response["dispositions"][1]
    assert len(model.calls[0][1]["body"]) == 1
    assert model.calls[0][1]["body"][0]["cells"][0] == {"column": 0, "start": 0, "end": 0}
    assert model.calls[0][1]["blank_table_templates"] == [{"table_id": "table-1", "column_count": 2,
        "rows": [{"row_start": 1, "row_end": 2, "range_ids": ["blank"]}]}]
    assert model.calls[1][1]["blank_table_templates"] == model.calls[0][1]["blank_table_templates"]
    assert not result["issues"] and not checked["findings"]


@pytest.mark.asyncio
async def test_empty_template_rows_require_model_disposition_and_independent_review():
    frozen = pack([source("blank", "", empty=True, non_requirement_reason="empty_table_row",
                          table_id="table-1", row_start=1, row_end=2, column_count=2)])
    model = FixedModel(extract=extraction_response([], []), verify={"reviewed_range_ids": [], "findings": []})
    result = await extract_pack(frozen, model)
    checked = await verify_pack(frozen, result, model)
    assert len(model.calls) == 2
    assert result["dispositions"][0]["status"] == "unresolved"
    assert "range_disposition_incomplete" in codes(result)
    assert "source_not_independently_reviewed" in codes(checked, "findings")
    assert checked["reviewed_range_ids"] == []


@pytest.mark.asyncio
async def test_empty_template_label_alone_is_not_a_grounded_nonrequirement_reason():
    frozen = pack([source("blank", "", empty=True, non_requirement_reason="empty_table_row",
                          table_id="table-1", row_start=1, row_end=2, column_count=2)])
    model = FixedModel(extract=extraction_response([], [
        {"range_id": "blank", "status": "non_requirement", "reason": "empty_table_row", "requirement_ids": []}]))
    result = await extract_pack(frozen, model)
    assert result["dispositions"][0]["status"] == "unresolved"
    assert "invalid_disposition" in codes(result)


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
    assert result["counts"]["requirements"] == 1
    assert "requirements" not in result
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
    frozen_index = index(first, second, documents=[{"document_id": "main", "document_version": "1", "role": "main"},
                                                 {"document_id": "amendment", "document_version": "2", "role": "amendment", "replaces": replaces}])
    checks = [{"pack_id": "p1", "findings": []}, {"pack_id": "p2", "findings": []}]
    return verify_all(frozen_index, extracted, checks), extracted


@pytest.mark.asyncio
async def test_explicit_amendment_replaces_only_named_range_and_preserves_original_artifact():
    result, original = await amendment_case({"document_id": "main", "range_id": "r1"})
    assert not result["blocking"]
    effective = apply_applicability_decisions([req for item in original for req in item["requirements"]], result["applicability_decisions"])
    assert effective[0]["applicable"] is False
    assert "amendment" in effective[0]["applicability_basis"]
    assert effective[1]["applicable"] is True
    assert result["applicability_decisions"]["p1:q1"]["amendment_document_id"] == "amendment"
    assert "replacement_requirement_ids" not in result["applicability_decisions"]["p1:q1"]
    assert original[0]["requirements"][0]["applicable"] is True


@pytest.mark.asyncio
@pytest.mark.parametrize("replaces", [None, "main", {"document_id": [], "range_id": "r1"}, {"document_id": "main", "scope": "all newer documents supersede older"}])
async def test_ambiguous_amendment_and_conflicting_numbers_remain_blocking(replaces):
    result, _ = await amendment_case(replaces)
    assert result["blocking"]
    assert "potential_cross_document_conflict" in codes(result)
    assert result["applicability_decisions"] == {}


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
    assert result["applicability_decisions"] == {}


def good_item(req=None, **extra):
    req = req or requirement()
    return {"requirement_id": req["requirement_id"], "target_kind": "body", "target": "第3章/3.2 交付期限承诺",
            "action": "承诺投标人在30天内交付并列明各阶段里程碑。", "material_needs": ["项目负责人批准的交付计划"],
            "constraint_digest": constraint_binding(req)["constraint_digest"], **extra}


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


@pytest.mark.asyncio
@pytest.mark.parametrize("code", ["requirement_omitted", "condition_unclear", "applicability_unresolved",
                                  "evidence_quote_mismatch", "number_or_unit_unsupported", "new_unknown_problem"])
async def test_model_cannot_downgrade_correctness_or_unknown_findings(code):
    extracted = await extract_pack(pack(), FixedModel(extract=extraction_response()))
    checked = await verify_pack(pack(), extracted, FixedModel(verify={"reviewed_range_ids": ["r1"], "findings": [
        {"code": code, "detail": "This problem remains unresolved despite the requested downgrade.", "blocking": False, "range_id": "r1"}]}))
    assert checked["findings"][0]["blocking"] is True
    assert verify_all(index(pack()), [extracted], [checked])["blocking"] is True


@pytest.mark.asyncio
async def test_only_explicit_presentation_note_may_be_informational_and_model_can_raise_it():
    extracted = await extract_pack(pack(), FixedModel(extract=extraction_response()))
    for proposed in (False, True):
        checked = await verify_pack(pack(), extracted, FixedModel(verify={"reviewed_range_ids": ["r1"], "findings": [
            {"code": "source_formatting_note", "detail": "Typeface differs; all contents and obligations are unchanged.", "blocking": proposed}]}))
        assert checked["findings"][0]["blocking"] is proposed
        assert verify_all(index(pack()), [extracted], [checked])["blocking"] is proposed


@pytest.mark.asyncio
async def test_aggregate_and_extraction_apply_host_severity_even_for_stored_downgrades():
    response = extraction_response()
    response["issues"] = [{"code": "unresolved_source_range", "detail": "Unclear source obligation.", "blocking": False}]
    extracted = await extract_pack(pack(), FixedModel(extract=response))
    assert extracted["issues"][0]["blocking"] is True
    extracted["issues"].clear()
    checked = {"pack_id": "p1", "findings": [{"code": "requirement_omitted", "detail": "missing", "blocking": False}]}
    assert verify_all(index(pack()), [extracted], [checked])["blocking"] is True


def test_correction_rebuilds_source_metadata_without_model_or_input_aliasing():
    corrected = extraction_response()
    result = validate_correction(pack(), corrected)
    assert not result["issues"]
    assert result["source_ranges"]["r1"]["document_id"] == "main"
    corrected["requirements"][0]["text"] = "mutated caller input"
    assert result["requirements"][0]["text"] != "mutated caller input"


def test_correction_accepts_explicit_unresolved_but_never_clears_its_blocker():
    result = validate_correction(pack(), extraction_response([], [{
        "range_id": "r1", "status": "unresolved", "reason": "Requires the missing amendment.", "requirement_ids": []}]))
    assert "unresolved_source_range" in codes(result)
    assert all(finding["blocking"] for finding in result["issues"])


@pytest.mark.parametrize("change,expected", [
    ("schema", "correction_schema_invalid"),
    ("missing_disposition", "range_disposition_incomplete"),
    ("duplicate_disposition", "range_disposition_incomplete"),
    ("unknown_range", "unknown_disposition_range"),
    ("fake_quote", "evidence_quote_mismatch"),
    ("wrong_unit", "number_or_unit_unsupported"),
    ("missing_numbers", "number_not_recorded"),
])
def test_correction_rejects_malformed_coverage_or_source_claims(change, expected):
    corrected = extraction_response()
    if change == "schema":
        corrected["source_ranges"] = {"secret": "not accepted"}
    elif change == "missing_disposition":
        corrected["dispositions"] = []
    elif change == "duplicate_disposition":
        corrected["dispositions"] *= 2
    elif change == "unknown_range":
        corrected["dispositions"][0]["range_id"] = "not-frozen"
    elif change == "fake_quote":
        corrected["requirements"][0]["evidence"][0]["quote"] = "sensitive fabricated quote"
    elif change == "wrong_unit":
        corrected["requirements"][0]["numbers"][0]["unit"] = "工作日"
    elif change == "missing_numbers":
        corrected["requirements"][0]["numbers"] = []
    with pytest.raises(CorrectionValidationError) as raised:
        validate_correction(pack(), corrected)
    assert expected in raised.value.codes
    assert "sensitive" not in str(raised.value) and "not accepted" not in str(raised.value)


def test_constraint_binding_is_detached_and_digest_changes_with_source_constraint():
    req = requirement()
    binding = constraint_binding(req)
    assert binding["constraint_snapshot"]["comparators"] == [{"value": "30", "unit": "天", "comparators": ["lte"]}]
    req["numbers"][0]["unit"] = "工作日"
    req["conditions"].append("仅第一标段适用")
    assert binding["constraint_snapshot"]["numbers"][0]["unit"] == "天"
    assert binding["constraint_snapshot"]["conditions"] == []
    assert binding["constraint_digest"] != constraint_binding(req)["constraint_digest"]


@pytest.mark.asyncio
async def test_model_receives_exact_binding_and_missing_binding_is_rejected():
    model = FixedModel(plan={"items": [good_item()]})
    result = await build_plan([requirement()], model)
    assert not result["blocking"]
    assert model.calls[0][1]["constraint_bindings"]["p1:q1"] == {"constraint_digest": constraint_binding(requirement())["constraint_digest"]}
    assert "constraint_snapshot" not in result["items"][0]
    item = good_item()
    del item["constraint_digest"]
    assert (await build_plan([requirement()], FixedModel(plan={"items": [item]})))["blocking"]


@pytest.mark.asyncio
@pytest.mark.parametrize("field", ["subject", "text", "conditions", "exceptions", "numbers", "evidence", "comparators"])
async def test_model_cannot_return_duplicate_snapshot_even_with_correct_plan_words(field):
    item = good_item()
    item["constraint_snapshot"] = {field: "tampered"}
    result = await build_plan([requirement()], FixedModel(plan={"items": [item]}))
    assert "plan_semantic_failure" in codes(result)


@pytest.mark.asyncio
async def test_digest_is_checked_independently_of_snapshot_shape():
    item = good_item(constraint_digest="0" * 64)
    result = await build_plan([requirement()], FixedModel(plan={"items": [item]}))
    assert "plan_constraint_binding_mismatch" in codes(result)


@pytest.mark.asyncio
@pytest.mark.parametrize("expected_unit,changed_unit", [("工作日", "天"), ("工作日", "自然日"), ("自然日", "天"), ("天", "工作日"), ("万元", "元")])
async def test_same_bare_number_with_changed_unit_never_passes(expected_unit, changed_unit):
    req = requirement(text=f"投标人必须在30{expected_unit}内交付。",
                      numbers=[{"value": "30", "unit": expected_unit, "quote": f"30{expected_unit}"}])
    result = await build_plan([req], FixedModel(plan={"items": [good_item(req, action=f"承诺投标人在30{changed_unit}内交付。")]}))
    assert result["blocking"] and {"plan_numeric_obligation_missing", "plan_unit_changed"} <= codes(result)
    valid = await build_plan([req], FixedModel(plan={"items": [good_item(req, action=f"承诺投标人在30{expected_unit}内交付。")]}))
    assert not valid["blocking"]


@pytest.mark.asyncio
async def test_correct_snapshot_and_target_cannot_hide_wrong_unit_in_action():
    req = requirement(text="投标人必须在30工作日内交付。", numbers=[{"value": "30", "unit": "工作日", "quote": "30工作日"}])
    result = await build_plan([req], FixedModel(plan={"items": [good_item(
        req, target="第3章/投标人30工作日内交付", action="承诺投标人在30天内交付。")]}))
    assert "plan_unit_changed" in codes(result) and result["blocking"]


@pytest.mark.asyncio
@pytest.mark.parametrize("original,changed", [("至少", "至多"), ("不超过", "超过"), ("大于", "不少于"), ("不少于", ""), ("≥", "≤")])
async def test_comparison_direction_inclusivity_and_omission_are_checked(original, changed):
    req = requirement(text=f"投标人须提供{original}30人。", numbers=[{"value": "30", "unit": "人", "quote": f"{original}30人"}])
    bad = await build_plan([req], FixedModel(plan={"items": [good_item(req, action=f"承诺投标人提供{changed}30人并提交人员名单。")]}))
    assert "plan_comparator_changed" in codes(bad)
    good = await build_plan([req], FixedModel(plan={"items": [good_item(req, action=f"承诺投标人提供{original}30人并提交人员名单。")]}))
    assert not good["blocking"]


@pytest.mark.asyncio
async def test_english_unit_and_comparison_are_preserved_without_embedded_word_matches():
    req = requirement(text="Supplier must deliver within 30 business days.", subject="Supplier",
                      numbers=[{"value": "30", "unit": "business days", "quote": "30 business days"}])
    valid = await build_plan([req], FixedModel(plan={"items": [good_item(req, action="State that Supplier will deliver within 30 business days.")]}))
    assert not valid["blocking"]
    invalid = await build_plan([req], FixedModel(plan={"items": [good_item(req, action="State that Supplier will deliver within 30 calendar days.")]}))
    assert "plan_unit_changed" in codes(invalid)


@pytest.mark.asyncio
@pytest.mark.parametrize("action,expected", [
    ("承诺招标人在30天内交付。仅第一标段适用；不可抗力除外。", "plan_subject_changed"),
    ("承诺非投标人在30天内交付。仅第一标段适用；不可抗力除外。", "plan_subject_changed"),
    ("承诺投标人在30天内交付。仅第二标段适用；不可抗力除外。", "plan_condition_changed"),
    ("承诺投标人在30天内交付。仅第一标段适用。", "plan_exception_changed"),
])
async def test_subject_conditions_and_exceptions_cannot_be_changed_or_omitted(action, expected):
    req = requirement(conditions=["仅第一标段适用"], exceptions=["不可抗力除外"])
    result = await build_plan([req], FixedModel(plan={"items": [good_item(req, action=action)]}))
    assert expected in codes(result)
    valid = await build_plan([req], FixedModel(plan={"items": [good_item(req, action="承诺投标人在30天内交付。仅第一标段适用；不可抗力除外。")]}))
    assert not valid["blocking"]


def pending_scope():
    return [{"range_id": "scope-fragment-1", "parent_range_id": "earlier-body-range",
             "document_id": "main", "document_version": "1", "unit_id": "original-unit",
             "start": 20, "end": 12020, "locator": {"page": 1}, "physical_path": ["page", 1],
             "scope_binding": {"status": "unresolved", "complete_sentence": False,
                               "kind": "unresolved", "applies_to_range_ids": ["r1"]},
             "reason": "incomplete_condition_sentence", "future_locator_metadata": {"anchor": "kept"}}]


@pytest.mark.asyncio
async def test_pending_scope_metadata_reaches_both_calls_without_becoming_read_evidence():
    frozen = {**pack(), "pending_scope_context": pending_scope()}
    model = FixedModel(extract=extraction_response(), verify={"reviewed_range_ids": ["r1"], "findings": []})
    extracted = await extract_pack(frozen, model)
    checked = await verify_pack(frozen, extracted, model)
    assert len(model.calls) == 2  # metadata forwarding never adds a model call
    for _, payload in model.calls:
        assert payload["pending_scope_context"][0]["scope_binding"] == frozen["pending_scope_context"][0]["scope_binding"]
        assert not {"text", "cells", "images", "locator", "physical_path", "unit_id", "document_version"} & payload["pending_scope_context"][0].keys()
        assert payload["supporting"] == []
    assert extracted["pending_scope_context"] == checked["pending_scope_context"] == frozen["pending_scope_context"]
    assert extracted["pending_scope_context"] is not frozen["pending_scope_context"]
    assert set(extracted["source_ranges"]) == {"r1"}
    assert [item["range_id"] for item in extracted["dispositions"]] == ["r1"]
    assert "scope_context_pending" in codes(extracted)
    assert "scope_context_pending" in codes(checked, "findings")
    result = verify_all(index(frozen), [extracted], [checked])
    assert result["blocking"] and result["counts"]["ranges"] == 1


@pytest.mark.asyncio
async def test_pending_scope_cannot_be_downgraded_or_counted_as_independently_reviewed():
    frozen = {**pack(), "pending_scope_context": pending_scope()}
    extracted = await extract_pack(frozen, FixedModel(extract=extraction_response()))
    checked = await verify_pack(frozen, extracted, FixedModel(verify={
        "reviewed_range_ids": ["r1", "scope-fragment-1"], "findings": [{
            "code": "scope_context_pending", "detail": "Model attempts to claim resolved.", "blocking": False}]}))
    assert {"scope_context_pending", "unknown_review_range"} <= codes(checked, "findings")
    assert all(finding["blocking"] for finding in checked["findings"])


def test_correction_preserves_host_pending_metadata_and_cannot_clear_its_issue():
    frozen = {**pack(), "pending_scope_context": pending_scope()}
    corrected = validate_correction(frozen, extraction_response())
    assert corrected["pending_scope_context"][0]["future_locator_metadata"] == {"anchor": "kept"}
    assert "scope_context_pending" in codes(corrected)
    assert all(finding["blocking"] for finding in corrected["issues"])
    payload = {**extraction_response(), "pending_scope_context": []}
    with pytest.raises(CorrectionValidationError, match="correction_schema_invalid"):
        validate_correction(frozen, payload)


@pytest.mark.asyncio
async def test_verification_keeps_original_requirement_reference_and_digest_without_echo():
    extracted = await extract_pack(pack(), FixedModel(extract=extraction_response()))
    ref = {"sha256": "a" * 64, "relative_path": "aa/requirements-index"}
    result = verify_all(index(pack()), [extracted], [{"pack_id": "p1", "findings": []}], requirements_ref=ref)
    assert result["requirements_ref"] == ref
    assert "requirements" not in result and "upstream_requirements" not in result
    assert len(result["requirements_digest"]) == 64
    changed = copy.deepcopy(extracted)
    changed["requirements"][0]["evidence"][0]["quote"] += "原件证据修订"
    updated = verify_all(index(pack()), [changed], [{"pack_id": "p1", "findings": []}])
    assert result["requirements_digest"] != updated["requirements_digest"]


@pytest.mark.asyncio
async def test_plan_saves_only_host_requirement_ref_and_digest_not_snapshot_or_source_copies():
    artifact = {"sha256": "b" * 64, "relative_path": "bb/extracted-pack"}
    refs = {"p1:q1": {"artifact_ref": artifact, "requirement_id": "p1:q1"}}
    model = FixedModel(plan={"items": [good_item()]})
    result = await build_plan([requirement()], model, requirement_refs=refs)
    assert not result["blocking"]
    assert result["items"][0]["requirement_ref"] == refs["p1:q1"]
    assert "constraint_snapshot" not in result["items"][0]
    assert "evidence" not in result["items"][0]
    assert "constraint_snapshot" not in model.calls[0][1]["constraint_bindings"]["p1:q1"]
    missing = await build_plan([requirement()], model, requirement_refs={})
    assert "plan_requirement_reference_missing" in codes(missing)
    stale = await build_plan([requirement()], model, requirement_refs={"p1:q1": {"artifact_ref": artifact, "requirement_id": "other"}})
    assert "plan_requirement_reference_missing" in codes(stale)


@pytest.mark.asyncio
async def test_reviewed_image_disposition_is_deterministic_and_request_probe_matches_send():
    image = source("image", "", kind="image_region", empty=True,
                   non_requirement_reason="reviewed_decorative_image",
                   image_disposition={"status": "reviewed", "classification": "decorative", "sha256": "a" * 64})
    frozen = pack([image, source()])
    model = FixedModel(extract=extraction_response(), verify={"reviewed_range_ids": ["r1"], "findings": []})
    result = await extract_pack(frozen, model)
    checked = await verify_pack(frozen, result, model)
    assert model.calls[0][1] == extract_payload(frozen)
    assert result["dispositions"][0]["status"] == "non_requirement"
    assert result["dispositions"][0]["reason"] == "reviewed_decorative_image"
    assert checked["host_reviewed_range_ids"] == ["image"]
    assert checked["reviewed_range_ids"] == ["r1"]
    assert not checked["findings"]
    assert model.calls[1][1]["body"] == model.calls[0][1]["body"]
    assert [item["range_id"] for item in model.calls[1][1]["dispositions"]] == ["r1"]
    image["image_disposition"]["status"] = "unreviewed"
    assert extract_payload(frozen)["body"][0]["range_id"] == "image"


def context_source(range_id, text, kind="section", **extra):
    return source(range_id, text, kind=kind, section_id="s1", heading_path="第一章 适用范围",
                  parent_section_id=None, **extra)


@pytest.mark.parametrize("kind", ["section", "table_region", "paragraph"])
@pytest.mark.parametrize("text", [
    "如果设备发生故障，投标人须提供维修服务。",
    "联合体成员须在签名字段填写名称。",
    "备注：本表作为审核依据，不适用或没有内容填写的，请在空格处标示斜线。",
    "如果技术评分相同，按照评审前附表的规定确定先后顺序。",
    "如果发生延期，应遵守本章第3条的通知要求。",
    "If a component fails, the bidder must provide a replacement.",
])
def test_local_conditions_are_body_obligations_not_chapter_scope(kind, text):
    from docreader.tender.evidence.context import condition_context
    entry = context_source("local", text, kind=kind)
    before = copy.deepcopy(entry)
    _, conditions, issues, _ = condition_context([entry])
    assert not any(conditions.values()) and not issues
    assert entry == before


@pytest.mark.parametrize("text", [
    "目录\n第一章 联合体适用规定 ........ 2\n第二章 如果异常处理 ........ 3",
    "目 录\n附件 1 投标函\n附件 2 联合体协议书（如有）\n附件 3 报价表",
    "Table of contents\nChapter 1 Applicability ........ 2\nChapter 2 Joint venture .... 4",
])
def test_navigation_structure_does_not_create_scope_even_with_condition_keywords(text):
    from docreader.tender.evidence.context import condition_context
    _, conditions, issues, _ = condition_context([context_source("nav", text)])
    assert not any(conditions.values()) and not issues


def test_trusted_preamble_cumulates_exact_excerpts_and_explicit_scope_after_local_body():
    from docreader.tender.evidence.context import condition_context
    entries = [context_source("preamble", "仅第一标段适用。\n联合体还需提供共同授权书。"),
               context_source("ordinary", "投标人须提供技术资料。"),
               context_source("explicit", "本章适用于境内交付，全部条款均须响应。")]
    _, conditions, issues, _ = condition_context(entries)
    assert not issues
    fragments = [fragment for _, fragment in conditions["s1"]]
    assert [fragment["text"] for fragment in fragments] == ["仅第一标段适用。", "联合体还需提供共同授权书。", "本章适用于境内交付，全部条款均须响应。"]
    assert all(fragment["scope_binding"]["status"] == "bound" for fragment in fragments)
    parents = {entry["range_id"]: entry for entry in entries}
    for fragment in fragments:
        parent = parents[fragment["parent_range_id"]]
        assert parent["text"][fragment["start"]:fragment["end"]] == fragment["text"]


@pytest.mark.parametrize("text,kind", [
    ("本表仅适用于境内项目。", "table_region"),
    ("身份文件格式\n（适用于无委托代理人的情况）\n投标人名称：", "section"),
    ("是否接受联合体投标：见前附表。", "section"),
    ("如接受联合体投标，还应遵守以下规定：", "section"),
    ("上述范围改为第二标段适用。", "section"),
    ("附件之间按名称列明的先后顺序解释。", "section"),
])
def test_explicit_cross_range_target_without_reliable_binding_stays_blocking(text, kind):
    from docreader.tender.evidence.context import condition_context
    _, conditions, issues, _ = condition_context([context_source("scope", text, kind)])
    assert issues and all(item["blocking"] for items in issues.values() for item in items)
    assert all(fragment["scope_binding"]["status"] == "unresolved" for values in conditions.values() for _, fragment in values)


def test_incomplete_cross_range_declaration_uses_pending_locator_not_synthetic_quote():
    from docreader.tender.evidence.context import condition_context, pending_scope_metadata
    original = context_source("long", "仅第一标段适用" + "没有可靠句界" * 500)
    _, conditions, issues, _ = condition_context([original])
    fragment = conditions["s1"][0][1]
    pending = pending_scope_metadata(fragment)
    assert issues and pending["scope_binding"]["complete_sentence"] is False
    assert pending["parent_range_id"] == "long"
    assert pending["start"] == 0 and pending["end"] == len(original["text"])
    assert not {"text", "cells", "image_ref"} & pending.keys()


def test_native_paragraph_break_prevents_later_sentence_from_completing_long_scope():
    from docreader.tender.evidence.context import condition_context
    first = "仅第一标段适用" + "缺少可证句界" * 100
    second = "投标人须提交项目计划。"
    text = first + "\n" + second
    entry = context_source("docx", text, paragraph_breaks=[len(first), len(text)])
    _, conditions, issues, _ = condition_context([entry])
    fragment = conditions["s1"][0][1]
    assert fragment["text"] == first
    assert fragment["scope_binding"]["complete_sentence"] is False
    assert fragment["scope_binding"]["status"] == "unresolved" and issues


def test_visual_pdf_line_break_is_not_invented_paragraph_boundary():
    from docreader.tender.evidence.context import condition_context
    entry = context_source("pdf", "本章适用于\n第一标段。")
    _, conditions, issues, _ = condition_context([entry])
    assert not issues
    assert conditions["s1"][0][1]["text"] == entry["text"]
    assert conditions["s1"][0][1]["scope_binding"]["status"] == "bound"


def test_field_instruction_does_not_hide_explicit_table_scope():
    from docreader.tender.evidence.context import condition_context
    entry = context_source("table", "本表仅适用于联合体，投标人须在名称字段填写全称。", kind="table_region")
    _, conditions, issues, _ = condition_context([entry])
    assert issues and conditions["s1"][0][1]["scope_binding"]["status"] == "unresolved"


class ScopeRepository:
    def __init__(self, sources):
        from types import SimpleNamespace
        self.sources = {key: SimpleNamespace(ranges=entries, by_id={entry["range_id"]: entry for entry in entries})
                        for key, entries in sources.items()}
        self.pack_loads = 0

    def source_index(self, reference):
        return self.sources[reference["sha256"]]

    def load_pack(self, entry):
        self.pack_loads += 1
        return {"body": [], "supporting": []}


def scope_review_case():
    from docreader.tender.evidence.context import condition_context
    entries = [context_source("scope-parent", "本表仅适用于境内项目。", source_range_id="scope-parent"),
               context_source("target-a", "投标人须提交材料甲。", source_range_id="target-a"),
               context_source("target-b", "投标人须提交材料乙。", source_range_id="target-b")]
    _, conditions, issues, _ = condition_context(entries)
    candidate = conditions["s1"][0][1]
    frozen = {"documents": [{"document_id": "main", "source_index_ref": {"sha256": "source"}}]}
    plan = {"issues": [item for group in issues.values() for item in group], "packs": []}
    decision = {"range_id": candidate["range_id"], "decision": "bind",
                "evidence": [{"range_id": candidate["range_id"], "quote": candidate["text"]}],
                "rationale": "表单适用条件明确覆盖材料甲与材料乙的行组，按其完整来源范围绑定。",
                "target_range_ids": ["target-b", "target-a"]}
    return entries, frozen, plan, decision, ScopeRepository({"source": entries})


def test_scope_bind_with_exact_evidence_targets_is_small_and_changes_only_matching_issue():
    from docreader.tender.evidence.context import condition_context
    from docreader.tender.evidence.scope import validate_scope_decisions
    entries, frozen, plan, decision, repo = scope_review_case()
    original = copy.deepcopy(entries)
    unrelated = context_source("unrelated", "本项目不接受联合体投标。", source_range_id="unrelated")
    validated = validate_scope_decisions(frozen, plan, [decision], repo)
    assert validated == {"schema_version": 1, "scope_decisions": [{**decision, "target_range_ids": ["target-a", "target-b"]}]}
    assert repo.pack_loads == 0
    _, conditions, issues, _ = condition_context([*entries, unrelated], validated["scope_decisions"])
    bound = next(fragment for values in conditions.values() for _, fragment in values if fragment["range_id"] == decision["range_id"])
    assert bound["scope_binding"]["status"] == "bound"
    assert bound["scope_binding"]["reviewed_target_range_ids"] == ["target-a", "target-b"]
    assert bound["scope_binding"]["kind"] == "reviewed_targets"
    assert "scope-parent" not in issues and "unrelated" in issues
    assert entries == original


def test_local_only_requires_source_backed_explanation_and_preserves_body():
    from docreader.tender.evidence.context import condition_context
    from docreader.tender.evidence.scope import validate_scope_decisions
    entries, frozen, plan, decision, repo = scope_review_case()
    decision.update(decision="local_only", target_range_ids=[],
                    rationale="该表单只约束当前行的填写对象，没有向后续章节扩展的表达；本行仍作为要求正文核验。")
    validated = validate_scope_decisions(frozen, plan, [decision], repo)
    original = copy.deepcopy(entries)
    _, conditions, issues, _ = condition_context(entries, validated["scope_decisions"])
    assert not any(conditions.values()) and not issues
    assert entries == original and entries[0]["text"]


@pytest.mark.parametrize("change,code", [
    ({"evidence": []}, "scope_decision_schema_invalid"),
    ({"evidence": [{"range_id": "scope-parent", "quote": "不存在的原文"}]}, "scope_evidence_mismatch"),
    ({"evidence": [{"range_id": "target-a", "quote": "投标人须提交材料甲。"}]}, "scope_issue_evidence_missing"),
    ({"target_range_ids": ["missing"]}, "scope_target_not_canonical"),
    ({"target_range_ids": []}, "scope_decision_schema_invalid"),
    ({"target_range_ids": ["target-a", "target-a"]}, "scope_decision_schema_invalid"),
    ({"range_id": "not-an-active-scope-issue"}, "scope_issue_not_current"),
    ({"rationale": "   "}, "scope_decision_schema_invalid"),
    ({"rationale": "已确认"}, "scope_rationale_unspecific"),
    ({"decision": "local_only", "target_range_ids": [], "rationale": "误报"}, "scope_rationale_unspecific"),
    ({"decision": "local_only", "target_range_ids": [], "rationale": "已认真查看原文件并同意结果"}, "scope_local_reason_missing"),
    ({"action": "bind"}, "scope_decision_schema_invalid"),
])
def test_scope_decision_rejects_missing_fake_irrelevant_evidence_and_unbounded_targets(change, code):
    from docreader.tender.evidence.scope import ScopeDecisionValidationError, validate_scope_decisions
    _, frozen, plan, decision, repo = scope_review_case()
    decision.update(change)
    with pytest.raises(ScopeDecisionValidationError) as raised:
        validate_scope_decisions(frozen, plan, [decision], repo)
    assert code in raised.value.codes
    assert "不存在的原文" not in str(raised.value)


def test_scope_bind_cannot_target_derived_partial_range_or_other_document():
    from docreader.tender.evidence.scope import ScopeDecisionValidationError, validate_scope_decisions
    entries, frozen, plan, decision, repo = scope_review_case()
    decision["target_range_ids"] = [decision["range_id"]]
    with pytest.raises(ScopeDecisionValidationError, match="scope_target_not_canonical"):
        validate_scope_decisions(frozen, plan, [decision], repo)
    other = context_source("foreign-target", "另一文件的完整条款。", document_id="other", source_range_id="foreign-target")
    repo = ScopeRepository({"source": entries, "other": [other]})
    frozen["documents"].append({"document_id": "other", "source_index_ref": {"sha256": "other"}})
    decision["target_range_ids"] = ["foreign-target"]
    with pytest.raises(ScopeDecisionValidationError, match="scope_target_document_mismatch"):
        validate_scope_decisions(frozen, plan, [decision], repo)


def test_scope_duplicate_decisions_are_not_last_write_wins():
    from docreader.tender.evidence.scope import ScopeDecisionValidationError, validate_scope_decisions
    _, frozen, plan, decision, repo = scope_review_case()
    with pytest.raises(ScopeDecisionValidationError, match="scope_duplicate_decision"):
        validate_scope_decisions(frozen, plan, [decision, decision], repo)
    with pytest.raises(ScopeDecisionValidationError, match="scope_decisions_empty"):
        validate_scope_decisions(frozen, plan, [], repo)


def test_manual_scope_target_does_not_make_following_local_condition_a_chapter_preamble():
    from docreader.tender.evidence.context import condition_context
    entries, _, _, decision, _ = scope_review_case()
    following = context_source("local", "联合体还需提供授权书。", source_range_id="local")
    _, conditions, _, _ = condition_context([entries[0], following, *entries[1:]], [decision])
    assert [fragment["range_id"] for values in conditions.values() for _, fragment in values] == [decision["range_id"]]


def test_pending_scope_locator_keeps_identity_but_drops_large_cell_reference_lists():
    from docreader.tender.evidence.context import condition_context, pending_scope_metadata
    entry = context_source("pending", "仅第一标段适用而没有句界", source_range_id="pending",
                           physical_locator={"page_ordinal": 0, "cell_refs": ["large-ref"], "rendered_spans": [1]})
    entry["locator"].update(cell_refs=["large-ref"], cells=["duplicate-grid"], rendered_spans=[1])
    _, conditions, _, _ = condition_context([entry])
    pending = pending_scope_metadata(conditions["s1"][0][1])
    assert pending["source_range_id"] == "pending"
    assert pending["parent_section_id"] is None
    assert pending["physical_locator"] == {"page_ordinal": 0}
    assert not {"cell_refs", "cells", "rendered_spans"} & pending["locator"].keys()


def test_applicability_decision_identity_must_match_effective_requirement():
    with pytest.raises(ValueError, match="applicability_decision_identity_mismatch"):
        apply_applicability_decisions([requirement()], {"p1:q1": {
            "requirement_id": "different", "applicable": False, "applicability_basis": "明确来源修订。"}})


@pytest.mark.parametrize("text", [
    "This form applies only to the local response field.",
    "This form is applicable only to local delivery.",
    "This form and its clauses apply only to local delivery.",
    "This form contains conditions applying only to local delivery.",
])
def test_english_form_applicability_word_forms_remain_explicit_scope(text):
    from docreader.tender.evidence.context import condition_context
    _, conditions, issues, _ = condition_context([context_source("english", text)])
    assert issues
    assert conditions["s1"][0][1]["scope_binding"]["status"] == "unresolved"


def test_scope_review_of_valid_source_survives_unreadable_sibling_without_source_index():
    from docreader.tender.evidence.scope import validate_scope_decisions
    _, frozen, plan, decision, repo = scope_review_case()
    frozen["documents"].append({"document_id": "unreadable", "status": "unresolved"})
    plan["issues"].append({"code": "source_unreadable", "document_id": "unreadable", "blocking": True})
    result = validate_scope_decisions(frozen, plan, [decision], repo)
    assert result["scope_decisions"][0]["range_id"] == decision["range_id"]
    assert plan["issues"][-1]["code"] == "source_unreadable"
