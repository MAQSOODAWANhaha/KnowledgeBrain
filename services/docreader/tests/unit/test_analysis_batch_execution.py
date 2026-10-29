"""Shared evidence must not merge acceptance or erase counterevidence on recovery."""
from copy import deepcopy
import threading

import pytest

from docreader.llm.budget import RequestBudget
from docreader.llm.request import ModelContractError
from docreader.tender.checks.analysis import _Analysis, verify_analysis
from docreader.tender.checks.analysis_batch_execution import (
    execute_batches, joint_batches, validate_member, validate_shared,
)
from docreader.tender.checks.analysis_batches import make_batches, member_payload
from test_analysis_checks import IDENTITY, Model, scenario


async def setup(tmp_path):
    data = await scenario(tmp_path)
    budget = RequestBudget(context_tokens=131072, tokenizer="o200k_base")
    context = _Analysis(data[1], data[2], data[0], IDENTITY, budget)
    source = next(iter(context.sources.values()))
    payloads = []
    for number in range(2):
        problem = {"issue_id": f"q{number}", "code": "semantic_condition_unclear", "blocking": True,
                   "detail": f"需核对独立条件{number}", "range_id": source["range_id"]}
        candidate = {"candidate_id": f"locate:c{number + 1}", "issue_ids": [problem["issue_id"]],
                     "range_ids": [source["range_id"]], "item_ids": [], "reason": "核对本条件。"}
        payloads.append(context.payload("associate", f"associate-{number}",
            source_views=context.views([source["range_id"]], complete=True),
            items=list(context.items.values()), issues=[problem], candidates=[candidate]))
    batches = make_batches(payloads, budget)
    assert len(batches) == 1
    return data, context, batches


def response(batch):
    values = []
    for identity in batch["target_work_unit_ids"]:
        payload = member_payload(batch, identity)
        if batch["operation"] == "associate":
            source = payload["source_views"][0]["body"][0]
            unit = next(u for u in batch["work_units"] if u["work_unit_id"] == identity)
            value = {"relations": [], "decisions": [{"decision_id": identity + ":d1",
                     "issue_ids": unit["issue_ids"], "reason": "按原文解释独立问题。",
                     "evidence": [{"range_id": source["range_id"], "quote": source["text"]}]}]}
        else:
            value = {"accepted_decision_ids": [d["decision_id"] for d in payload["decisions"]]}
        values.append({"work_unit_id": identity, **value, "findings": []})
    return {"schema_version": 2, "work_units": values, "shared_findings": []}


class BatchModel:
    def __init__(self, mutate=None):
        self.calls = []
        self.mutate = mutate

    async def call_groups_validated(self, task, payload, validate, *, validate_shared):
        self.calls.append(deepcopy(payload))
        value = response(payload)
        if self.mutate:
            self.mutate(payload, value)
        shared = validate_shared(value, payload)
        return {"work_units": [{"work_unit_id": unit["work_unit_id"],
                                **validate(unit["work_unit_id"], unit)} for unit in value["work_units"]],
                "shared_findings": shared}


async def test_two_logical_units_use_one_call_and_replay_saved_groups(tmp_path):
    data, context, batches = await setup(tmp_path)
    model = BatchModel()
    plan_ref, payloads, results = await execute_batches(context, "associate", batches, model, data[3])
    assert len(model.calls) == 1 and len(results) == 2
    assert [p["work_unit_id"] for p in payloads] == [r["work_unit_id"] for r in context.operation_refs]
    assert len(data[0].read_json(plan_ref)["requests"]) == 1
    replay = _Analysis(data[1], data[2], data[0], IDENTITY, context.budget)
    assert await execute_batches(replay, "associate", batches, model, data[3]) == (plan_ref, payloads, results)
    assert len(model.calls) == 1


async def test_joint_keeps_full_shared_basis_but_accepts_only_own_decisions(tmp_path):
    data, context, batches = await setup(tmp_path)
    _, payloads, results = await execute_batches(context, "associate", batches, BatchModel(), data[3])
    joint = joint_batches(context, batches, dict(zip([p["work_unit_id"] for p in payloads], results)))
    assert len(joint) == 1
    assert joint[0]["source_views"] == batches[0]["source_views"]
    assert joint[0]["items"] == batches[0]["items"]
    assert joint[0]["dispositions"] == []
    target = joint[0]["target_work_unit_ids"][0]
    own = next(u for u in joint[0]["work_units"] if u["work_unit_id"] == target)
    foreign = next(d["decision_id"] for d in joint[0]["decisions"] if d["decision_id"] not in own["decision_ids"])
    with pytest.raises(ModelContractError):
        validate_member(context, joint[0], target,
                        {"work_unit_id": target, "accepted_decision_ids": [foreign], "findings": []})


async def test_association_cannot_resolve_a_sibling_issue(tmp_path):
    _, context, batches = await setup(tmp_path)
    batch = batches[0]
    value = response(batch)["work_units"][0]
    other = next(u for u in batch["work_units"] if u["work_unit_id"] != value["work_unit_id"])
    value["decisions"][0]["issue_ids"] = other["issue_ids"]
    with pytest.raises(ModelContractError):
        validate_member(context, batch, value["work_unit_id"], value)


async def test_shared_finding_can_address_successful_non_target_member(tmp_path):
    _, context, batches = await setup(tmp_path)
    batch = deepcopy(batches[0])
    sibling = batch["target_work_unit_ids"].pop(0)
    value = response(batch)
    value["shared_findings"] = [{"work_unit_ids": [sibling], "finding": {
        "code": "semantic_condition_unclear", "detail": "共同例外同时影响先前结论。", "blocking": True}}]
    assert validate_shared(context, value, batch) == value["shared_findings"]
    value["shared_findings"][0]["work_unit_ids"] = ["unknown"]
    with pytest.raises(ModelContractError):
        validate_shared(context, value, batch)


async def test_joint_split_does_not_lose_later_counterevidence_against_earlier_target(tmp_path):
    data, context, batches = await setup(tmp_path)
    _, payloads, results = await execute_batches(context, "associate", batches, BatchModel(), data[3])
    joint = joint_batches(context, batches, dict(zip([p["work_unit_id"] for p in payloads], results)))[0]
    first, second = joint["target_work_unit_ids"]
    split = [{**deepcopy(joint), "target_work_unit_ids": [target]} for target in (first, second)]

    def counterevidence(payload, value):
        if payload["target_work_unit_ids"] == [second]:
            value["shared_findings"] = [{"work_unit_ids": [first], "finding": {
                "code": "semantic_condition_unclear", "detail": "共同反证否定先前兄弟组。", "blocking": True}}]

    _, _, reviewed = await execute_batches(context, "joint", split, BatchModel(counterevidence), data[3])
    assert any(p["blocking"] for p in reviewed[0]["findings"])
    assert reviewed[1]["findings"] == []
    assert reviewed[0]["accepted_decision_ids"]  # Final acceptance applies findings; receipt stays intact.


async def test_full_verification_keeps_unrelated_decision_accepted(tmp_path):
    data = await scenario(tmp_path)
    budget = RequestBudget(context_tokens=131072, tokenizer="o200k_base")
    seed = _Analysis(data[1], data[2], data[0], IDENTITY, budget)
    source = next(iter(seed.sources.values()))

    def findings(payload, value):
        if payload["operation"] == "review" and source["range_id"] in {
                d["range_id"] for d in payload["dispositions"]}:
            value["findings"] = [{"code": "semantic_condition_unclear", "blocking": True,
                "detail": f"独立条件{number}须解释。", "range_id": source["range_id"], "needs_source": False}
                for number in range(2)]

    class IntegratedModel(Model, BatchModel):
        def __init__(self):
            Model.__init__(self, mutate=findings)
            self.batches = []

        async def call_groups_validated(self, task, payload, validate, *, validate_shared):
            self.batches.append(deepcopy(payload))
            value = response(payload)
            if payload["operation"] == "review":
                value["work_units"][0]["findings"] = [{"code": "semantic_condition_unclear",
                    "detail": "仅否定本组决议，另一独立解释成立。", "blocking": True}]
            shared = validate_shared(value, payload)
            return {"work_units": [{"work_unit_id": unit["work_unit_id"],
                **validate(unit["work_unit_id"], unit)} for unit in value["work_units"]],
                "shared_findings": shared}

    model = IntegratedModel()
    result = await verify_analysis(data[1], data[2], data[0], model, budget, identity=IDENTITY, save=data[3])
    assert len(model.batches) == 2  # One associate and one independent joint call.
    assert len(result["accepted_decision_ids"]) == 1
    assert result["coverage"]["missing"] == {}
    assert len(result["resolved_issue_ids"]) == 1
    assert result["blocking"] and not result["hard_blocking"]
    assert result["counts"]["model_calls"] == len(model.calls) + len(model.batches)


async def test_batch_budget_planning_runs_off_the_heartbeat_event_loop(tmp_path):
    data, context, batches = await setup(tmp_path)
    main_thread = threading.get_ident()
    measure = context.budget.measure
    calls = []

    def checked(*args, **kwargs):
        calls.append(threading.get_ident())
        return measure(*args, **kwargs)

    context.budget.measure = checked
    await execute_batches(context, "associate", batches, BatchModel(), data[3])
    assert calls and all(identity != main_thread for identity in calls)
