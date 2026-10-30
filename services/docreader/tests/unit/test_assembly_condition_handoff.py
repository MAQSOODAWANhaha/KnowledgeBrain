"""Existing assembly choices carry conditions to writing without keyword inference."""

import json

import pytest

from docreader.llm.budget import RequestBudget
from docreader.tender import authoring_pipeline as pipeline
from test_submission_followup import SUBMISSION_TEXT, fixture_context, local_response


@pytest.mark.parametrize("selection", ["purpose_only", "rule", "candidate"])
async def test_adopted_condition_needs_its_existing_reference_to_reach_writing(tmp_path, selection):
    artifacts, context, native = fixture_context(tmp_path, with_rule=True)
    condition = native.ranges[0]
    context.payload["rules"][0]["item"]["evidence"] = [
        {"range_id": condition["range_id"], "quote": condition["text"]}]
    calls = []

    async def execute(task, request, validate):
        payload = json.loads(request["messages"][1]["content"][0]["text"])
        calls.append(payload)
        if payload.get("operation") == "assemble":
            candidates = [target for chapter in payload["chapters"] for target in chapter["targets"]]
            selected = candidates if selection == "candidate" else candidates[:1]
            return validate({"chapters": [{"title": "财务资料", "targets": [{
                "kind": "form", "purpose": "不同主体分别填写，同一主体按适用规定填写。",
                "target_ids": [target["target_id"] for target in selected],
                "rule_ids": [payload["unassigned_rules"][0]["rule_id"]] if selection == "rule" else [],
                "template_part_ids": candidates[0]["template_part_ids"]}], "children": []}]})
        value = local_response(payload)
        chapter = next(c for c in pipeline._flatten(value["chapters"]) if c["targets"])
        chapter["targets"][0].update(rule_ids=[], support_origins=["tender"])
        if selection == "candidate":
            condition_alias = next(source["range_id"] for source in payload["sources"]
                                   if source["text"] == SUBMISSION_TEXT)
            chapter["targets"].append({"kind": "narrative", "purpose": "主体适用条件",
                "rule_ids": [], "range_ids": [condition_alias], "support_origins": ["tender"]})
        return validate(value)

    outline = await pipeline._organize(artifacts, context,
        RequestBudget(model="fixture", context_tokens=131072), execute, [])
    task = pipeline.plan_writing_tasks(outline)[0]
    loaded, bindings, issues = pipeline._load_task(
        artifacts, context, task, {"response": {"shared_arrangements": []}}, [])

    assert len(calls) == 2
    assert outline["submission_refs"]  # A global reference alone does not bind the writing task.
    assert not issues
    condition_supports = [support for support in loaded["supports"] if support["text"] == SUBMISSION_TEXT]
    assert bool(condition_supports) is (selection != "purpose_only")
    assert any(support["grid"] == native.raw["units"][1]["grid"] for support in loaded["supports"])
    if condition_supports:
        support = condition_supports[0]
        reference = bindings[support["support_id"]]
        assert reference['source_ref']['unit_id'] == condition['unit_id']
        assert reference['selectors'][0]['source_range_id'] == condition['source_range_id']
        assert support["support_id"] in loaded["targets"][0]["support_ids"]
        assert support["target_ids"] == ["t1"]
    assert bool(task["targets"][0]["rule_refs"]) is (selection == "rule")
