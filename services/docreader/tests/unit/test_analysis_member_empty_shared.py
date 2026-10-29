"""Only a precisely empty misplaced shared array is safe to omit locally."""

from copy import deepcopy

import pytest

from docreader.llm.recovery import _fragments
from docreader.llm.request import ModelContractError
from docreader.tender.checks.analysis_batch_execution import joint_batches, validate_member, validate_shared
from docreader.workflow.state import WORKFLOW_PROTOCOL
from test_analysis_batch_execution import response, setup


@pytest.mark.parametrize("operation", ["associate", "review"])
async def test_exact_empty_member_shared_array_preserves_local_shared_and_sibling_results(tmp_path, operation):
    _, context, batches = await setup(tmp_path)
    batch = batches[0]
    if operation == "review":
        accepted = {unit["work_unit_id"]: validate_member(context, batch, unit["work_unit_id"], unit)
                    for unit in response(batch)["work_units"]}
        batch = joint_batches(context, batches, accepted)[0]
    raw = response(batch)
    first, second = raw["work_units"]
    first["findings"] = [{"code": "semantic_condition_unclear", "detail": "仅本组待核条件。", "blocking": True}]
    raw["shared_findings"] = [{"work_unit_ids": [second["work_unit_id"]], "finding": {
        "code": "semantic_condition_unclear", "detail": "仅明确指向另一组的共同反证。", "blocking": True}}]
    expected = [validate_member(context, batch, unit["work_unit_id"], unit) for unit in raw["work_units"]]
    first["shared_findings"] = []
    before = deepcopy(raw)
    assert validate_shared(context, raw, batch) == raw["shared_findings"]
    assert [validate_member(context, batch, unit["work_unit_id"], unit) for unit in raw["work_units"]] == expected
    assert raw == before, "The physical response and accepted sibling must stay immutable."


@pytest.mark.parametrize("misplaced", [None, False, "", {}, [{
    "work_unit_ids": ["associate-1"], "finding": {"code": "semantic_condition_unclear",
        "detail": "不能丢弃的跨组反证。", "blocking": True}}]])
async def test_nonempty_or_wrong_type_member_shared_field_is_rejected_with_preserving_feedback(tmp_path, misplaced):
    _, context, batches = await setup(tmp_path)
    batch = batches[0]
    raw = response(batch)
    member, sibling = raw["work_units"]
    member["shared_findings"] = deepcopy(misplaced)
    before = deepcopy(raw)
    expected_sibling = validate_member(context, batch, sibling["work_unit_id"], sibling)
    with pytest.raises(ModelContractError) as caught:
        validate_member(context, batch, member["work_unit_id"], member)
    assert caught.value.errors[0]["code"] == "extra_forbidden"
    assert caught.value.errors[0]["path"] == ["shared_findings"]
    assert "outer shared_findings" in caught.value.errors[0]["detail"]
    assert "work_unit_ids" in caught.value.errors[0]["detail"]
    assert _fragments(member, caught.value.errors) == [{"path": ["shared_findings"], "value": misplaced}]
    assert validate_member(context, batch, sibling["work_unit_id"], sibling) == expected_sibling
    assert raw == before


async def test_empty_shared_field_does_not_relax_foreign_issue_routing_or_other_extra_fields(tmp_path):
    _, context, batches = await setup(tmp_path)
    batch = batches[0]
    member, sibling = response(batch)["work_units"]
    member["shared_findings"] = []
    member["decisions"][0]["issue_ids"] = sibling["decisions"][0]["issue_ids"]
    with pytest.raises(ModelContractError) as caught:
        validate_member(context, batch, member["work_unit_id"], member)
    assert caught.value.errors[0]["code"] == "foreign_resolution_target"
    sibling["other_shared_findings"] = []
    with pytest.raises(ModelContractError) as caught:
        validate_member(context, batch, sibling["work_unit_id"], sibling)
    assert caught.value.errors[0]["path"] == ["other_shared_findings"]


def test_receive_normalization_has_its_own_frozen_protocol_version():
    assert WORKFLOW_PROTOCOL["normalization_version"] == 12
