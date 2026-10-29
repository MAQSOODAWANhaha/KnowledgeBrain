"""An invalid short reference must not copy the complete assembly into feedback."""

from copy import deepcopy
import json
from types import SimpleNamespace

import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.budget import RequestBudget
from docreader.llm.recovery import _fragments, _rejection_feedback
from docreader.llm.request import ModelContractError
from docreader.tender import authoring_pipeline as pipeline
from docreader.tests.submission_responses import assemble_independent_candidates


@pytest.mark.parametrize("path,code,conflict", [
    (["chapters", 0, "targets", 0, "target_ids", 0], "outline_unknown_reference", False),
    (["chapters", 0, "targets", 0, "related_target_ids", 0], "outline_unknown_reference", False),
    (["chapters", 0, "children", 0, "targets", 0, "target_ids", 0], "outline_unknown_reference", False),
    (["chapters", 0, "targets", 0, "template_part_ids", 0], "outline_unknown_template", False),
    (["chapters", 0, "targets", 0, "rule_ids", 0], "outline_unknown_reference", False),
    (["constraint_rule_ids", 0], "outline_unknown_reference", False),
    (["excluded_rules", 0, "rule_ids", 0], "outline_unknown_reference", False),
    (["excluded_targets", 0, "target_ids", 0], "outline_unknown_reference", False),
    (["search_intents", 0, "for_ids", 0], "outline_unknown_reference", False),
    (["excluded_rules", 0, "rule_ids", 0], "outline_conflicting_disposition", True),
    (["excluded_targets", 0, "target_ids", 0], "outline_conflicting_disposition", True),
])
async def test_large_assembly_reports_one_leaf_within_existing_correction_room(
        monkeypatch, tmp_path, path, code, conflict):
    artifacts = ArtifactStore(tmp_path)
    reference = {"item_ref": {"artifact_ref": artifacts.put_json({"fixture": True}), "item_id": "item-1"}}
    fragment = {"chapters": [{"chapter_id": f"local-{i}", "title": f"响应位置{i}", "children": [],
        "targets": [{"target_id": f"target-{i}", "kind": "narrative",
                     "purpose": "应答已确认的人员资格及适用条件，不扩大或丢失原要求。" * 8,
                     "rule_refs": [], "source_refs": [], "template_refs": []}]} for i in range(60)],
        "issues": []}
    context = SimpleNamespace(input={"schema_version": 2, "run_id": "assembly-feedback",
        "source_version": 1, "domain_revision": 1, "issues": []}, sources={}, rules={"rule-1": reference},
        payload={"rules": [{"rule_id": "rule-1", "item": {"text": "保留原要求。", "evidence": []}}]},
        validate=lambda _: deepcopy(fragment))
    monkeypatch.setattr(pipeline, "_source_records", lambda *_: [])
    monkeypatch.setattr(pipeline, "prepare_template_parts", lambda *_: ([], []))
    budget = RequestBudget(model="fixture", context_tokens=131072, tokenizer="o200k_base",
                           tokenized_authoring=True)
    checked = []

    async def execute(task, request, validate):
        payload = json.loads(request["messages"][1]["content"][0]["text"])
        if payload.get("operation") != "assemble":
            return validate({"schema_version": 2, "chapters": [], "findings": []})
        response = assemble_independent_candidates(payload)
        invalid = deepcopy(response)
        invalid["constraint_rule_ids"] = ["u1"] if conflict and path[0] == "excluded_rules" else []
        invalid["excluded_rules"] = [{"rule_ids": [], "reason": "根据给定条件排除。"}] if path[0] == "excluded_rules" else []
        invalid["excluded_targets"] = [{"target_ids": [], "reason": "根据给定条件排除。"}] if path[0] == "excluded_targets" else []
        invalid["search_intents"] = [{"query": "人员资格", "for_ids": []}] if path[0] == "search_intents" else []
        if "children" in path:
            invalid["chapters"][0]["children"] = [deepcopy(response["chapters"][0])]
        current = invalid
        for key in path[:-2]:
            current = current[key]
        value = ("u1" if path[0] == "excluded_rules" else "t1") if conflict else "not-supplied"
        current[path[-2]] = [value]
        assert len(json.dumps(invalid, ensure_ascii=False).encode()) > 30000
        with pytest.raises(ModelContractError) as error:
            validate(invalid)
        assert error.value.errors[0]["path"] == path
        assert error.value.errors[0]["code"] == code
        fragments = _fragments(invalid, error.value.errors)
        assert fragments == [{"path": path, "value": value}]
        feedback = _rejection_feedback({"errors": error.value.errors, "fragments": fragments}, 1, "parsed")
        corrected = pipeline._request(budget, task, {**payload, "contract_feedback": feedback})
        before, after = budget.measure_request(request, task), budget.require_request(corrected, task)
        assert after["input_tokens"] - before["input_tokens"] <= 767
        checked.append(path)
        return validate(response)

    await pipeline._organize(artifacts, context, budget, execute, [])
    assert checked == [path]
