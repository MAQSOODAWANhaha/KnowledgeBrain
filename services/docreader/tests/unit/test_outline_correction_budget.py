"""Unknown outline references need small, admissible correction feedback."""
from copy import deepcopy
import json

import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.budget import RequestBudget
from docreader.llm.recovery import _fragments
from docreader.llm.request import ModelContractError
from docreader.tender.authoring import _OutlineContext
from docreader.tender.authoring_pipeline import _organize
from test_authoring_pipeline_boundaries import ordinary_response, payload, snapshot


@pytest.mark.parametrize("path", [
    ["constraint_rule_ids", 0], ["constraint_range_ids", 0],
    ["chapters", 0, "targets", 0, "rule_ids", 0],
    ["chapters", 0, "targets", 0, "range_ids", 0],
    ["chapters", 0, "targets", 0, "template_part_ids", 0],
    ["chapters", 0, "children", 0, "targets", 0, "range_ids", 0],
    ["findings", 0, "rule_ids", 0], ["search_intents", 0, "for_ids", 0],
])
def test_global_outline_reference_errors_identify_one_scalar(tmp_path, path):
    artifacts = ArtifactStore(tmp_path)
    context = _OutlineContext(artifacts, snapshot(artifacts))
    target = {"kind": "narrative", "purpose": "已知响应位置", "rule_ids": [],
              "range_ids": [], "template_part_ids": []}
    response = {"schema_version": 2, "chapters": [{"title": "技术响应", "targets": [deepcopy(target)],
        "children": [{"title": "实施安排", "targets": [deepcopy(target)], "children": []}]}],
        "constraint_rule_ids": [], "constraint_range_ids": [],
        "findings": [{"code": "unresolved", "detail": "待核对", "rule_ids": []}],
        "search_intents": [{"query": "给定实现能力", "for_ids": []}] if path[0] == "search_intents" else []}
    field = response
    for key in path[:-1]:
        field = field[key]
    field.append("not-supplied")
    with pytest.raises(ModelContractError) as error:
        context.validate(response)
    assert error.value.errors[0]["path"] == path
    assert _fragments(response, error.value.errors) == [{"path": path, "value": "not-supplied"}]


async def test_source_validator_reports_constraint_reference_instead_of_copying_chapter_tree(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    context = _OutlineContext(artifacts, snapshot(artifacts, [{"text": "须按原文规定组织技术响应。"}]))
    budget = RequestBudget(model="fixture", context_tokens=131072, prompts={"outline": "Return JSON"})
    feedback = []

    async def execute(task, request, validate):
        value = payload(request)
        response = ordinary_response(task, value)
        if value.get("operation") != "assemble":
            invalid = {**response, "constraint_range_ids": ["not-in-sources"]}
            with pytest.raises(ModelContractError) as error:
                validate(invalid)
            feedback.extend(_fragments(invalid, error.value.errors))
            assert error.value.errors[0]["path"] == ["constraint_range_ids", 0]
        return validate(response)

    await _organize(artifacts, context, budget, execute, [])
    assert feedback == [{"path": ["constraint_range_ids", 0], "value": "not-in-sources"}]


async def test_source_packing_leaves_feedback_room_without_losing_original_text(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    originals = ["a" * 2800, "b" * 2800]
    context = _OutlineContext(artifacts, snapshot(artifacts, [{"text": text} for text in originals]))
    budget = RequestBudget(model="fixture", context_tokens=12000, output_reserve=2000,
                           prompts={"outline": "Return the requested outline JSON"})
    source_requests = []

    async def execute(task, request, validate):
        value = payload(request)
        if value.get("operation") != "assemble":
            source_requests.append(request)
        return validate(ordinary_response(task, value))

    await _organize(artifacts, context, budget, execute, [])
    assert len(source_requests) == 2
    assert [entry["text"] for request in source_requests for entry in payload(request)["sources"]] == originals
    for request in source_requests:
        measured = budget.measure_request(request, "outline")
        assert measured["input_tokens"] + 2048 <= measured["effective_input_budget_tokens"]
        corrected = deepcopy(request)
        value = payload(corrected)
        value["contract_feedback"] = {"errors": [{"path": ["constraint_range_ids", 0],
            "code": "outline_unknown_reference", "detail": "Select only this request's actual sources."}],
            "fragments": [{"path": ["constraint_range_ids", 0], "value": "outside"}], "rejection_count": 1}
        corrected["messages"][1]["content"][0]["text"] = json.dumps(value, ensure_ascii=False)
        assert budget.require_request(corrected, "outline")["fits"] is True
