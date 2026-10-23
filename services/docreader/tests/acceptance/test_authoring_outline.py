"""Outline organization over real SQL analysis with synthetic execution only."""

import json

import pytest

from docreader.integrations.authoring import read_authoring_input
from docreader.integrations.store import InvalidConfirmation
from docreader.llm.budget import RequestBudget
from docreader.tender.authoring import organize_outline
from docreader.tender.checks.analysis import current_items

from test_analysis import analysis_runtime as analysis_runtime, execute_analysis, result


pytestmark = pytest.mark.integration


def expectation(row):
    return {key: row[key] for key in ("run_id", "source_version", "domain_revision", "checklist_digest")}


def budget():
    return RequestBudget(context_tokens=524288, model="synthetic-outline", prompts={}, output_reserve=32768)


def outline_response(request):
    assert request["model"] == "synthetic-outline"
    assert request["messages"][0]["role"] == "system" and request["messages"][0]["content"]
    assert request["response_format"] == {"type": "json_object"}
    payload = json.loads(request["messages"][1]["content"][0]["text"])
    rules = {entry["item"]["text"]: entry for entry in payload["rules"]}
    assert len(rules) == 5
    assert all(entry["rule_id"].startswith("e") and entry["rule_id"][1:].isdigit() for entry in rules.values())
    personnel = rules["负责人须持A证"]
    score = rules["持B证加2分"]
    service = rules["服务响应不超过2小时"]
    submission = rules["须附A证复印件"]
    consequence = rules["未附A证复印件则否决投标"]

    def target(kind, purpose, selected):
        ranges = [quote["range_id"] for rule in selected for quote in rule["item"]["evidence"]]
        assert all(identity.startswith("r") and identity[1:].isdigit() for identity in ranges)
        return {"kind": kind, "purpose": purpose,
                "rule_ids": [rule["rule_id"] for rule in selected], "range_ids": list(dict.fromkeys(ranges))}

    return {"schema_version": 2, "chapters": [
        {"title": "技术与服务方案", "targets": [], "children": [
            {"title": "人员技术能力", "children": [], "targets": [
                target("narrative", "说明负责人能力与人员评分对应关系", [personnel, score])]},
            {"title": "服务保障", "children": [], "targets": [
                target("narrative", "说明服务响应安排与时限", [service])]},
            {"title": "人员证明附件", "children": [], "targets": [
                target("attachment", "提供资格与加分证明并核对否决条件", [personnel, score, submission, consequence])]},
        ]}], "findings": [
            {"code": "score_evidence_needed", "detail": "后续应核对拟投入人员及其加分证明。",
             "rule_ids": [score["rule_id"]]}]}


def chapters(tree):
    for chapter in tree:
        yield chapter
        yield from chapters(chapter["children"])


async def test_confirmed_analysis_organizes_one_outline_with_durable_scoring_and_source_refs(analysis_runtime):
    store, artifacts, _, run, calls = analysis_runtime
    executed = []

    async def execute(request, validate):
        executed.append(request)
        return validate(outline_response(request))

    waiting = await execute_analysis(analysis_runtime)
    before = len(calls)
    with pytest.raises(InvalidConfirmation):
        await organize_outline(store, artifacts, execute, budget(), **expectation(waiting))
    assert executed == [] and len(calls) == before

    await store.confirm(run["run_id"], 1, waiting["checklist_digest"], "outline-confirm", domain_revision=1)
    ready = await execute_analysis(analysis_runtime)
    assert ready["status"] == "analysis-ready" and len(calls) == before
    authoring_input = await read_authoring_input(store, artifacts, **expectation(ready))
    reference = await organize_outline(store, artifacts, execute, budget(), **expectation(ready))
    value = artifacts.read_json(reference)
    assert value["schema_version"] == 2 and value["authoring_input"] == authoring_input
    assert len(executed) == 1

    ordered = list(chapters(value["chapters"]))
    assert [chapter["title"] for chapter in ordered] == ["技术与服务方案", "人员技术能力", "服务保障", "人员证明附件"]
    assert len({chapter["chapter_id"] for chapter in ordered}) == len(ordered)
    targets = [target for chapter in ordered for target in chapter["targets"]]
    assert len(targets) == len({target["target_id"] for target in targets}) == 3
    assert all(chapter["chapter_id"] for chapter in ordered)
    assert all(target["target_id"] for target in targets)
    verification = result(artifacts, ready, "verify")
    effective = current_items(artifacts, verification)
    score_ref = next(entry["item_ref"] for entry in effective if entry["item"]["kind"] == "scoring_rule")
    assert sum({"item_ref": score_ref} in target["rule_refs"] for target in targets) == 2
    expected_refs = {json.dumps(entry["item_ref"], sort_keys=True) for entry in effective}
    selected_refs = {json.dumps(ref["item_ref"], sort_keys=True) for target in targets for ref in target["rule_refs"]}
    assert selected_refs == expected_refs
    assert all(target["source_refs"] for target in targets)
    for target in targets:
        for source_ref in target["source_refs"]:
            source = artifacts.read_json(source_ref["source_index_ref"])
            assert source_ref["unit_id"] in {entry["unit_id"] for entry in source["ranges"]}
    assert next(issue for issue in value["issues"] if issue["code"] == "score_evidence_needed")["rule_refs"] == [
        {"item_ref": score_ref}]
    assert len(calls) == before
    assert await store.get_run(run["run_id"]) == ready


async def test_outline_rechecks_current_confirmation_after_execution_before_publishing(analysis_runtime):
    store, artifacts, _, run, calls = analysis_runtime
    waiting = await execute_analysis(analysis_runtime)
    await store.confirm(run["run_id"], 1, waiting["checklist_digest"], "outline-inflight", domain_revision=1)
    ready = await execute_analysis(analysis_runtime)
    input_value = await read_authoring_input(store, artifacts, **expectation(ready))
    before = len(calls)
    executed = []

    async def execute(request, validate):
        executed.append(request)
        value = validate(outline_response(request))
        baseline = input_value["items_ref"]
        await store.correct(run["run_id"], 1, 1, baseline["sha256"], baseline,
                            "outline-inflight-correction", "Synthetic revision while outline execution is in progress")
        return value

    with pytest.raises(InvalidConfirmation):
        await organize_outline(store, artifacts, execute, budget(), **expectation(ready))
    changed = await store.get_run(run["run_id"])
    assert changed["domain_revision"] == 2 and changed["status"] == "pending"
    assert not changed["confirmations"] and changed["history"]["confirmations"]
    assert len(executed) == 1 and len(calls) == before
