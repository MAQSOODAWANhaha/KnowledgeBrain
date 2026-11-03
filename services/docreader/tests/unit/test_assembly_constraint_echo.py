"""Already retained submission constraints are echoes only in their own field."""

from copy import deepcopy
import json

import pytest

from docreader.llm.budget import RequestBudget
from docreader.llm.request import ModelContractError
from docreader.tender import authoring_pipeline as pipeline
from docreader.tests.submission_responses import assemble_independent_candidates

from test_assembly_source_handoff import BODY, CONDITION, context_fixture


def rule_ids(value):
    return list(value) if isinstance(value, dict) else [entry["rule_id"] for entry in value]


async def organize(tmp_path, monkeypatch, choose, *, compact=False, review=False):
    artifacts, context = context_fixture(tmp_path)
    before, calls, structures = deepcopy(context.payload), [], []
    original_fits = pipeline._fits
    if compact:
        monkeypatch.setattr(pipeline, "_fits", lambda budget, task, value, **options:
            False if value.get("operation") == "assemble" else original_fits(budget, task, value, **options))

    async def execute(stage, request, validate):
        value = json.loads(request["messages"][1]["content"][0]["text"])
        if stage == "authoring_structure":
            structures.append(value)
            return validate({"findings": [{"code": "check_structure", "detail": "核对真实层级。"}]
                             if len(structures) == 1 else []})
        if value.get("operation") == "assemble":
            calls.append(deepcopy(value))
            return validate(choose(value))
        sources = {entry["text"]: entry["range_id"] for entry in value["sources"]}
        rules = {entry["item"]["text"]: entry["rule_id"] for entry in value["rules"]}
        return validate({"schema_version": 2, "chapters": [{"title": "技术应答", "targets": [{
            "kind": "narrative", "purpose": "应答第一项，其他要求独立判断。",
            "range_ids": [sources[BODY[0]]], "rule_ids": [rules[BODY[0]]]}]}],
            "constraint_rule_ids": [rules[CONDITION]], "findings": []})

    result = await pipeline._organize(artifacts, context, RequestBudget(
        model="fixture", context_tokens=131072), execute, [], structure_results={} if review else None)
    assert context.payload == before
    return result, calls


@pytest.mark.parametrize("compact", [False, True])
@pytest.mark.parametrize("mixed", [False, True])
async def test_retained_constraint_echo_preserves_originals_without_becoming_target_evidence(
        tmp_path, monkeypatch, compact, mixed):
    def choose(value):
        response = assemble_independent_candidates(value)
        echo, = rule_ids(value["submission_rules"])
        response["constraint_rule_ids"] = ([value["candidate_rules"][0]["rule_id"], echo,
            rule_ids(value["unassigned_rules"])[0], echo] if mixed else [echo, echo])
        return response

    result, calls = await organize(tmp_path, monkeypatch, choose, compact=compact)
    assert len(calls) == 1 and (calls[0].get("assembly_view") == "compact_v1") is compact
    assert [ref["item_ref"]["item_id"] for ref in result["chapters"][0]["targets"][0]["rule_refs"]] == ["item-0"]
    constraints = [ref["item_ref"]["item_id"] for ref in result["constraint_rule_refs"]]
    assert set(constraints) == ({"condition", "item-0", "item-1"} if mixed else {"condition"})
    assert len(constraints) == len(set(constraints))


@pytest.mark.parametrize("unknown", ["e999", "x1"])
async def test_unknown_constraint_after_a_visible_echo_keeps_its_original_error_index(tmp_path, monkeypatch, unknown):
    def choose(value):
        response = assemble_independent_candidates(value)
        response["constraint_rule_ids"] = [rule_ids(value["submission_rules"])[0], unknown]
        return response

    with pytest.raises(ModelContractError) as caught:
        await organize(tmp_path, monkeypatch, choose)
    assert caught.value.errors[0]["path"] == ["constraint_rule_ids", 1]


@pytest.mark.parametrize("field,path", [
    ("target", ["chapters", 0, "targets", 0, "rule_ids", 0]),
    ("row", ["chapters", 0, "targets", 0, "response_table", "rows", 0, "rule_ids", 0]),
    ("excluded", ["excluded_rules", 0, "rule_ids", 0]),
    ("search", ["search_intents", 0, "for_ids", 0]),
])
async def test_visible_submission_constraint_is_not_authorized_in_other_rule_fields(tmp_path, monkeypatch, field, path):
    def choose(value):
        response = assemble_independent_candidates(value)
        echo, = rule_ids(value["submission_rules"])
        target = response["chapters"][0]["targets"][0]
        if field == "target":
            target["rule_ids"] = [echo]
        elif field == "row":
            target["response_table"] = {"rows": [{
                "range_ids": value["chapters"][0]["targets"][0]["range_ids"], "rule_ids": [echo]}]}
        elif field == "excluded":
            response["excluded_rules"] = [{"rule_ids": [echo], "reason": "本次不适用。"}]
        else:
            response["search_intents"] = [{"query": "实际功能证明", "for_ids": [echo]}]
        return response

    with pytest.raises(ModelContractError) as caught:
        await organize(tmp_path, monkeypatch, choose)
    assert caught.value.errors[0]["path"] == path


@pytest.mark.parametrize("visible", [False, True])
async def test_revision_echo_domain_is_the_actual_submission_snapshot_not_updated_constraint_count(
        tmp_path, monkeypatch, visible):
    snapshots = []

    def choose(value):
        snapshots.append(rule_ids(value["submission_rules"]))
        response = assemble_independent_candidates(value)
        response["constraint_rule_ids"] = ([snapshots[-1][0] if visible else "e2"]
            if value.get("review_findings") else [rule_ids(value["unassigned_rules"])[0]])
        return response

    if visible:
        result, calls = await organize(tmp_path, monkeypatch, choose, review=True)
        assert len(calls) == 2
        assert {ref["item_ref"]["item_id"] for ref in result["constraint_rule_refs"]} == {"condition", "item-1"}
    else:
        with pytest.raises(ModelContractError) as caught:
            await organize(tmp_path, monkeypatch, choose, review=True)
        assert caught.value.errors[0]["path"] == ["constraint_rule_ids", 0]
    assert snapshots == [["e1"], ["e1"]]
