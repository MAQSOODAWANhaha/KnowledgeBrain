"""Local model defects retain usable analysis and reach independent review."""

from copy import deepcopy
import json

import pytest

from docreader.llm import ModelSemanticError
from docreader.tender.requirements import extract_items
from test_analysis_extraction import FixedModel, IDENTITY, pack, result, source


@pytest.mark.asyncio
@pytest.mark.parametrize("fields", [
    {"conditions/exceptions": ["仅限代理商，制造商无需提交。"]},
    {"conditions": "仅限代理商", "subject": "投标人"},
    {"scoring": {"points": 5}},
])
async def test_optional_field_defects_preserve_core_and_send_original_value_for_review(fields):
    response = result()
    response["items"][0].update(fields)
    before = deepcopy(response)
    extracted = await extract_items(pack(), FixedModel(response), identity=IDENTITY)
    assert len(extracted["items"]) == 1
    assert extracted["items"][0]["evidence"] == before["items"][0]["evidence"]
    if "subject" in fields:
        assert extracted["items"][0]["subject"] == "投标人"
    assert extracted["dispositions"][0]["item_ids"] == ["p1:i1"]
    diagnostic = next(d for d in extracted["diagnostics"] if d["code"] == "optional_item_fields_pending")
    assert diagnostic["fields"] and diagnostic["errors"]
    problem = next(p for p in extracted["issues"] if p["code"] == "extraction_optional_fields_unresolved")
    assert problem["item_ids"] == ["p1:i1"] and problem["range_id"] == "r1"
    assert problem["blocking"]
    for key, value in diagnostic["fields"].items():
        assert key in problem["detail"] and json.dumps(value, ensure_ascii=False, separators=(",", ":")) in problem["detail"]
    assert response == before


@pytest.mark.asyncio
async def test_optional_recovery_cannot_fill_missing_core():
    response = result()
    del response["items"][0]["text"]
    response["items"][0]["conditions/exceptions"] = ["仅限代理商"]
    extracted = await extract_items(pack(), FixedModel(response), identity=IDENTITY)
    assert extracted["items"] == []
    assert any(p["code"] == "extraction_item_unresolved" for p in extracted["issues"])


@pytest.mark.asyncio
async def test_deleted_lf_is_restored_without_mutating_model_response_or_source():
    frozen = pack([source(text="交付期限\n为30天。")])
    response = result()
    original, original_pack = deepcopy(response), deepcopy(frozen)

    class SharedResponseModel:
        async def call(self, task, payload):
            return response

    extracted = await extract_items(frozen, SharedResponseModel(), identity=IDENTITY)
    assert extracted["items"][0]["evidence"][0]["quote"] == "交付期限\n为30天"
    assert extracted["issues"] == []
    assert extracted["diagnostics"][0] == {
        "code": "evidence_quote_restored", "path": "items[0].evidence[0].quote", "range_id": "r1",
        "start": 0, "end": len("交付期限\n为30天"),
    }
    assert response == original and frozen == original_pack


@pytest.mark.asyncio
@pytest.mark.parametrize("numbers", [
    [{"value": 30, "unit": "天"}],
    [{"value": 30, "unit": "天", "quote": "另一事项才有的数值"}],
    [{"value": 30, "unit": "天", "quote": "交付期限为30天"}, {"value": 1}],
    [], None, "30天",
])
async def test_incomplete_optional_numbers_do_not_fail_the_item_or_create_a_blocker(numbers):
    response = result()
    response["items"][0]["numbers"] = numbers
    before = deepcopy(response)
    model = FixedModel(response)
    extracted = await extract_items(pack(), model, identity=IDENTITY)
    assert extracted["items"] == result()["items"]
    assert extracted["issues"] == []
    assert {d["code"] for d in extracted["diagnostics"]} == {"optional_numbers_omitted"}
    assert response == before and len(model.calls) == 1


@pytest.mark.asyncio
async def test_numbers_cannot_borrow_a_quote_from_another_item_in_the_same_pack():
    response = result()
    other = source("r2", "报价不超过60万元。")
    response["items"][0]["numbers"] = [{"value": 60, "unit": "万元", "quote": "60万元"}]
    extracted = await extract_items(pack([source(), other]), FixedModel(response), identity=IDENTITY)
    assert "numbers" not in extracted["items"][0]


@pytest.mark.asyncio
async def test_valid_numbers_with_legitimate_zero_are_retained():
    text = "未提供方案得0分。"
    response = result(items=[{"item_id": "p1:i1", "kind": "scoring_rule", "text": text,
                              "evidence": [{"range_id": "r1", "quote": text}],
                              "numbers": [{"value": 0, "unit": "分", "quote": "0分"}]}])
    extracted = await extract_items(pack([source(text=text)]), FixedModel(response), identity=IDENTITY)
    assert extracted["items"] == response["items"] and extracted["issues"] == []


@pytest.mark.asyncio
async def test_number_quote_can_complement_item_quote_within_the_same_supplied_range():
    response = result()
    response["items"][0]["evidence"][0]["quote"] = "交付期限"
    response["items"][0]["numbers"] = [{"value": 30, "unit": "天", "quote": "30天"}]
    extracted = await extract_items(pack(), FixedModel(response), identity=IDENTITY)
    assert extracted["items"] == response["items"] and extracted["issues"] == []


@pytest.mark.asyncio
@pytest.mark.parametrize("declared", [None, [], ["p1:i1", "p1:i2"], ["p1:i1", "p1:i1"]])
async def test_disposition_reverse_index_is_derived_from_evidence(declared):
    response = result()
    if declared is None:
        del response["dispositions"][0]["item_ids"]
    else:
        response["dispositions"][0]["item_ids"] = declared
    extracted = await extract_items(pack(), FixedModel(response), identity=IDENTITY)
    assert extracted["dispositions"] == result()["dispositions"]
    assert extracted["items"] == response["items"] and extracted["issues"] == []


@pytest.mark.asyncio
@pytest.mark.parametrize("declared", [None, "p1:i1", [1], {"item_id": "p1:i1"}, False])
@pytest.mark.parametrize("has_items", [True, False])
async def test_legacy_disposition_index_type_cannot_block_valid_status_and_reason(declared, has_items):
    response = result()
    if not has_items:
        response["items"] = []
        response["dispositions"][0].update(status="non_item", reason="本段仅为背景说明。")
    response["dispositions"][0]["item_ids"] = declared
    before = deepcopy(response)
    model = FixedModel(response)
    extracted = await extract_items(pack(), model, identity=IDENTITY)
    assert extracted["items"] == response["items"] and extracted["issues"] == []
    assert extracted["dispositions"][0] == {**response["dispositions"][0],
                                           "item_ids": ["p1:i1"] if has_items else []}
    assert {diagnostic["code"] for diagnostic in extracted["diagnostics"]} == {"disposition_index_rebuilt"}
    assert response == before and len(model.calls) == 1


@pytest.mark.asyncio
@pytest.mark.parametrize("dispositions", [[], [None], [
    {"range_id": "r1", "status": "non_item", "reason": "本段仅为说明。"},
]])
async def test_incomplete_or_conflicting_disposition_keeps_items_for_review(dispositions):
    response = result(dispositions=dispositions)
    extracted = await extract_items(pack(), FixedModel(response), identity=IDENTITY)
    assert extracted["items"] == response["items"]
    assert extracted["dispositions"][0]["item_ids"] == ["p1:i1"]
    assert extracted["dispositions"][0]["status"] == "items"
    assert extracted["issues"]


@pytest.mark.asyncio
async def test_missing_disposition_without_items_stays_unresolved_instead_of_non_item():
    extracted = await extract_items(pack(), FixedModel(result(items=[], dispositions=[])), identity=IDENTITY)
    assert extracted["dispositions"][0]["status"] == "unresolved"
    assert extracted["dispositions"][0]["item_ids"] == [] and extracted["issues"]
    assert any(p["code"] == "range_disposition_incomplete" for p in extracted["issues"])
    assert not any(p["code"] == "unresolved_source_range" for p in extracted["issues"])


@pytest.mark.asyncio
async def test_bad_local_item_preserves_sibling_and_reviewable_content():
    response = result()
    response["items"].append({"item_id": "p1:i2", "kind": "obligation", "text": "应提供售后服务。",
                              "evidence": [{"range_id": "r1", "quote": "原文并没有这句话"}]})
    extracted = await extract_items(pack(), FixedModel(response), identity=IDENTITY)
    assert extracted["items"] == result()["items"]
    assert any("应提供售后服务" in problem["detail"] for problem in extracted["issues"])
    assert all(not p.get("item_ids") or set(p["item_ids"]) <= {"p1:i1"} for p in extracted["issues"])


@pytest.mark.asyncio
async def test_bad_relation_does_not_discard_valid_items():
    response = result(relations=[{"relation_id": "p1:l1", "from_item_id": "p1:i1",
        "to_item_id": "p1:i2", "relation": "交付后提供售后。",
        "evidence": [{"range_id": "r1", "quote": "交付期限为30天"}]}])
    extracted = await extract_items(pack(), FixedModel(response), identity=IDENTITY)
    assert extracted["items"] == result()["items"] and extracted["relations"] == []
    assert any("交付后提供售后" in p["detail"] for p in extracted["issues"])
    assert next(p for p in extracted["issues"] if "交付后提供售后" in p["detail"])["item_ids"] == ["p1:i1"]


@pytest.mark.asyncio
async def test_irrecoverable_envelope_and_duplicate_item_identity_still_fail():
    for response in ({"schema_version": 2}, result(schema_version=1), result(items=[
            *result()["items"], *result()["items"]])):
        with pytest.raises(ModelSemanticError):
            await extract_items(pack(), FixedModel(response), identity=IDENTITY)


@pytest.mark.asyncio
async def test_local_issue_null_reference_is_preserved_as_a_problem_without_null_in_artifact():
    from docreader.tender.schema import AnalysisExtraction

    extracted = await extract_items(pack(), FixedModel(result(issues=[{
        "code": "unclear", "detail": "适用范围需要核对。", "blocking": True, "range_id": None,
    }])), identity=IDENTITY)
    payload = {key: extracted[key] for key in ("schema_version", "items", "dispositions", "relations", "issues")}
    AnalysisExtraction.model_validate(payload)
    assert any("适用范围需要核对" in p["detail"] for p in extracted["issues"])


@pytest.mark.asyncio
async def test_untraceable_relation_retains_both_known_endpoints_for_joint_review():
    second = source("r2", "售后响应不超过2小时。")
    response = result(items=[*result()["items"], {
        "item_id": "p1:i2", "kind": "obligation", "text": second["text"],
        "evidence": [{"range_id": "r2", "quote": second["text"]}],
    }], dispositions=[*result()["dispositions"], {
        "range_id": "r2", "status": "items", "reason": "规定售后响应。",
    }], relations=[{"relation_id": "p1:l1", "from_item_id": "p1:i1", "to_item_id": "p1:i2",
        "relation": "交付要求与售后要求存在适用关系。",
        "evidence": [{"range_id": "r1", "quote": "不在原文的关系依据"}]}])
    extracted = await extract_items(pack([source(), second]), FixedModel(response), identity=IDENTITY)
    assert len(extracted["items"]) == 2 and extracted["relations"] == []
    problem = next(p for p in extracted["issues"] if p["code"] == "evidence_quote_mismatch")
    assert problem["range_id"] == "r1" and problem["item_ids"] == ["p1:i1", "p1:i2"]
