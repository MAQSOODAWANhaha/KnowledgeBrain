"""Durable group attempts on PostgreSQL; no live provider or business database."""
import asyncio
from copy import deepcopy
import json
from pathlib import Path
from uuid import uuid4

import httpx
import pytest

from docreader.integrations.store import RequestReplayBlocked
from docreader.llm import ModelConfig, OpenAIModel
from docreader.llm.recovery import call_groups_validated
from docreader.llm.request import ModelConfigurationError, ModelContractError, ModelWorkUnitExhausted
from test_store import store as store, submitted


pytestmark = pytest.mark.integration


@pytest.fixture(autouse=True)
def no_retry_delay(monkeypatch):
    real_sleep = asyncio.sleep

    async def immediate(_):
        await real_sleep(0)

    monkeypatch.setattr("docreader.llm.recovery.asyncio.sleep", immediate)


def payload():
    return {"schema_version": 2, "operation": "associate", "source_views": [],
            "items": [], "relations": [], "candidates": [], "issues": [],
            "shared_counterevidence": "The original condition still applies to both members.",
            "work_units": [{"work_unit_id": unit, "candidate_ids": [], "issue_ids": []}
                           for unit in ("associate-a", "associate-b")],
            "target_work_unit_ids": ["associate-a", "associate-b"]}


def validate(unit, value):
    if value.get("accepted") is not True:
        raise ModelContractError([{"path": ["accepted"], "code": "bool_required", "detail": "Return true"}])
    return {"accepted": True}


def shared(response, current):
    if not isinstance(response, dict) or not isinstance(response.get("work_units"), list):
        raise ModelContractError([{"path": ["work_units"], "code": "list_required", "detail": "Return an array"}])
    ids = [unit.get("work_unit_id") for unit in response["work_units"]]
    if len(set(ids)) != len(ids) or not set(ids) <= set(current["target_work_unit_ids"]):
        raise ModelContractError([{"path": ["work_units"], "code": "routing", "detail": "Use distinct target IDs"}])
    if not isinstance(response.get("shared_findings"), list):
        raise ModelContractError([{"path": ["shared_findings"], "code": "list_required", "detail": "Return an array"}])
    return response["shared_findings"]


async def setup(store):
    run, command = await submitted(store)
    lease = await store.acquire(run["run_id"], command["command_id"], 1, lease_seconds=300)
    return run, lease


async def run_groups(store, lease, model, value=None, validator=validate):
    return await call_groups_validated(model, store, store.test_artifacts, lease, "verify", value or payload(),
                                       validator, validate_shared=shared)


class Model:
    def __init__(self, replies):
        self.replies, self.calls = replies, []

    async def call(self, task, value):
        self.calls.append(deepcopy(value))
        result = self.replies[len(self.calls) - 1]
        if isinstance(result, BaseException):
            raise result
        return deepcopy(result)


def reply(*units, bad=(), findings=()):
    return {"schema_version": 2, "work_units": [{"work_unit_id": unit, "accepted": unit not in bad} for unit in units],
            "shared_findings": list(findings)}


async def test_partial_success_preserves_basis_and_prior_sibling_counterevidence(store):
    run, lease = await setup(store)
    finding = {"work_unit_ids": ["associate-a"], "finding": {"code": "counterevidence", "detail": "Original exception"}}
    model = Model([reply("associate-a", "associate-b", bad=["associate-b"]),
                   reply("associate-b", findings=[finding])])
    result = await run_groups(store, lease, model)
    assert [call["target_work_unit_ids"] for call in model.calls] == [["associate-a", "associate-b"], ["associate-b"]]
    assert all(call["work_units"] == payload()["work_units"] for call in model.calls)
    assert all(call["shared_counterevidence"] == payload()["shared_counterevidence"] for call in model.calls)
    assert result["shared_findings"] == [finding]
    assert len(result["work_units"]) == 2
    assert model.calls[1]["contract_feedback"]["work_units"][0]["errors"][0]["code"] == "bool_required"
    assert await run_groups(store, lease, model) == result
    assert len(model.calls) == 2
    records = (await store.get_run(run["run_id"]))["artifacts"]
    assert len([r for r in records if r["stage"] == "model-group-result"]) == 2


async def test_missing_group_only_retries_missing_member(store):
    _, lease = await setup(store)
    model = Model([reply("associate-a"), reply("associate-b")])
    await run_groups(store, lease, model)
    assert model.calls[1]["target_work_unit_ids"] == ["associate-b"]
    assert model.calls[1]["contract_feedback"]["work_units"][0]["errors"][0]["code"] == "missing_work_unit"


async def test_invalid_shared_routing_accepts_no_sibling_before_correction(store):
    _, lease = await setup(store)
    wrong = reply("associate-a", "associate-b")
    wrong["shared_findings"] = None
    model = Model([wrong, reply("associate-a", "associate-b")])
    await run_groups(store, lease, model)
    assert len(model.calls) == 2 and model.calls[1]["target_work_unit_ids"] == payload()["target_work_unit_ids"]


async def test_configuration_failures_persist_four_attempts_across_restart_and_split(store):
    run, lease = await setup(store)
    model = Model([ModelConfigurationError("synthetic configuration rejection")] * 4)
    with pytest.raises(ModelWorkUnitExhausted):
        await run_groups(store, lease, model)
    assert len(model.calls) == 4
    for unit in payload()["target_work_unit_ids"]:
        with pytest.raises(ModelWorkUnitExhausted):
            await run_groups(store, lease, model, {**payload(), "target_work_unit_ids": [unit]})
    assert len(model.calls) == 4
    records = (await store.get_run(run["run_id"]))["artifacts"]
    assert len([r for r in records if r["stage"] == "model-group-attempt"]) == 4
    assert not (await store.get_run(run["run_id"]))["usage"]


async def test_invalid_execution_spec_also_consumes_durable_local_attempts(store):
    run, lease = await setup(store)

    class InvalidSpec(Model):
        def execution_spec(self):
            raise ModelConfigurationError("synthetic invalid context configuration")

    model = InvalidSpec([])
    for _ in range(2):
        with pytest.raises(ModelWorkUnitExhausted):
            await run_groups(store, lease, model)
    records = (await store.get_run(run["run_id"]))["artifacts"]
    assert len([r for r in records if r["stage"] == "model-group-attempt"]) == 4
    assert not model.calls


async def test_group_result_commit_crash_replays_complete_response_without_recalling_model(store, monkeypatch):
    _, lease = await setup(store)
    model = Model([reply("associate-a", "associate-b")])
    original = store.commit_artifact
    failed = False

    async def interrupted(*args, **kwargs):
        nonlocal failed
        if args[1] == "model-group-result" and kwargs.get("metadata", {}).get("work_unit_id") == "associate-b" and not failed:
            failed = True
            raise OSError("synthetic database commit interruption")
        return await original(*args, **kwargs)

    monkeypatch.setattr(store, "commit_artifact", interrupted)
    with pytest.raises(OSError):
        await run_groups(store, lease, model)
    checked = []

    def validate_once(unit, answer):
        checked.append(unit)
        return validate(unit, answer)

    result = await run_groups(store, lease, model, validator=validate_once)
    assert len(result["work_units"]) == 2 and len(model.calls) == 1
    assert checked == ["associate-b"]


async def test_saved_member_rejection_is_not_revalidated_after_outcome_commit_crash(store, monkeypatch):
    _, lease = await setup(store)
    model = Model([reply("associate-a", "associate-b", bad=["associate-b"]), reply("associate-b")])
    original = store.commit_artifact
    failed = False
    checked = []

    async def interrupted(*args, **kwargs):
        nonlocal failed
        if args[1] == "model-group-outcome" and not failed:
            failed = True
            raise OSError("synthetic crash after durable member rejection")
        return await original(*args, **kwargs)

    def observed(unit, value):
        checked.append((unit, value["accepted"]))
        return validate(unit, value)

    monkeypatch.setattr(store, "commit_artifact", interrupted)
    with pytest.raises(OSError):
        await run_groups(store, lease, model, validator=observed)
    await run_groups(store, lease, model, validator=observed)
    assert checked == [("associate-a", True), ("associate-b", False), ("associate-b", True)]
    assert len(model.calls) == 2


async def test_reservations_are_atomic_and_shared_target_cannot_race(store):
    _, lease = await setup(store)
    ref = store.test_artifacts.put_json({"fixture": "group attempt"})
    basis = "a" * 64
    attempts = await asyncio.gather(*(store.reserve_group_attempt(lease, uuid4().hex, ref, basis, ["a", "b"])
                                      for _ in range(2)), return_exceptions=True)
    assert sum(isinstance(value, RequestReplayBlocked) for value in attempts) == 1
    reserved = next(value for value in attempts if isinstance(value, dict))
    await store.commit_artifact(lease, "model-group-outcome", ref, scope=reserved["scope"], input_digest=basis)
    for index in range(3):
        reserved = await store.reserve_group_attempt(lease, uuid4().hex, ref, basis, ["a"])
        await store.commit_artifact(lease, "model-group-outcome", ref, scope=reserved["scope"], input_digest=basis)
    with pytest.raises(ModelWorkUnitExhausted):
        await store.reserve_group_attempt(lease, uuid4().hex, ref, "b" * 64, ["a"])
    assert await store.reserve_group_attempt(lease, uuid4().hex, ref, basis, ["b"])


def wire_model(store, lease, handler):
    config = ModelConfig.from_env({"KNOWLEDGEBRAIN_CHAT_BASE_URL": "https://fixture.invalid/v1",
        "KNOWLEDGEBRAIN_CHAT_API_KEY": "synthetic", "KNOWLEDGEBRAIN_CHAT_MODEL": "fixture",
        "KNOWLEDGEBRAIN_CHAT_MAX_CONTEXT_TOKENS": "131072"})
    return OpenAIModel(config, store, lease, prompts={"verify": "Return the requested JSON"},
                       artifacts=store.test_artifacts, transport=httpx.MockTransport(handler))


def sse(value):
    chunks = [{"choices": [{"index": 0, "delta": {"content": value}, "finish_reason": None}]},
              {"choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]}]
    return httpx.Response(200, content="".join("data: " + json.dumps(chunk) + "\n\n" for chunk in chunks) + "data: [DONE]\n\n",
                         headers={"content-type": "text/event-stream"})


async def test_joint_batch_records_host_phase_without_single_work_unit_echo(store):
    run, lease = await setup(store)
    value = payload()
    value["operation"] = "review"

    def handler(request):
        sent = json.loads(json.loads(request.content)["messages"][1]["content"][0]["text"])
        assert sent["operation"] == "review" and "work_unit_id" not in sent
        return sse(json.dumps(reply(*sent["target_work_unit_ids"])))

    async with wire_model(store, lease, handler) as model:
        await run_groups(store, lease, model, value)
    row = await store.get_run(run["run_id"])
    assert len(row["usage"]) == 1
    assert row["usage"][0]["metadata"]["operation"] == "joint"
    assert row["usage"][0]["metadata"]["work_unit_id"] == ""


async def test_physical_success_before_response_receipt_crash_is_replayed_without_payment(store, monkeypatch):
    run, lease = await setup(store)
    sent = []

    def handler(request):
        value = json.loads(json.loads(request.content)["messages"][1]["content"][0]["text"])
        sent.append(value)
        return sse(json.dumps(reply(*(unit["work_unit_id"] for unit in value["work_units"]))))

    original = store.commit_artifact
    failed = False

    async def interrupted(*args, **kwargs):
        nonlocal failed
        if args[1] == "model-group-response" and not failed:
            failed = True
            raise OSError("synthetic crash after transport audit")
        return await original(*args, **kwargs)

    monkeypatch.setattr(store, "commit_artifact", interrupted)
    async with wire_model(store, lease, handler) as model:
        with pytest.raises(OSError):
            await run_groups(store, lease, model)
    async with wire_model(store, lease, handler) as model:
        result = await run_groups(store, lease, model)
    assert len(result["work_units"]) == 2 and len(sent) == 1
    requests = (await store.get_run(run["run_id"]))["usage"]
    assert len(requests) == 1 and requests[0]["status"] == "succeeded"


async def test_uncertain_physical_failure_is_replayed_then_new_attempt_sends_once(store, monkeypatch):
    run, lease = await setup(store)
    sent = []

    def handler(request):
        sent.append(request)
        if len(sent) == 1:
            raise httpx.ReadError("synthetic interrupted transport", request=request)
        value = json.loads(json.loads(request.content)["messages"][1]["content"][0]["text"])
        return sse(json.dumps(reply(*(unit["work_unit_id"] for unit in value["work_units"]))))

    original = store.commit_artifact
    failed = False

    async def interrupted(*args, **kwargs):
        nonlocal failed
        if args[1] == "model-group-response" and not failed:
            failed = True
            raise OSError("synthetic crash after uncertain transport audit")
        return await original(*args, **kwargs)

    monkeypatch.setattr(store, "commit_artifact", interrupted)
    async with wire_model(store, lease, handler) as model:
        with pytest.raises(OSError):
            await run_groups(store, lease, model)
    async with wire_model(store, lease, handler) as model:
        assert len((await run_groups(store, lease, model))["work_units"]) == 2
    requests = (await store.get_run(run["run_id"]))["usage"]
    assert [row["status"] for row in requests] == ["uncertain", "succeeded"]
    assert len(sent) == 2


async def test_whole_invalid_json_and_member_rejection_share_the_same_group_allowance(store):
    run, lease = await setup(store)
    sent = []

    def handler(request):
        value = json.loads(json.loads(request.content)["messages"][1]["content"][0]["text"])
        sent.append(value)
        if len(sent) == 1:
            return sse('{"work_units": [')
        targets = value["target_work_unit_ids"]
        response = reply(*targets, bad=[targets[-1]])
        return sse(json.dumps(response))

    async with wire_model(store, lease, handler) as model:
        with pytest.raises(ModelWorkUnitExhausted):
            await run_groups(store, lease, model)
    assert len(sent) == 4
    assert [len(value["target_work_unit_ids"]) for value in sent] == [2, 2, 1, 1]
    assert all(value["work_units"] == sent[0]["work_units"] for value in sent)
    current = await store.get_run(run["run_id"])
    assert [row["status"] for row in current["usage"]] == ["failed", "succeeded", "succeeded", "succeeded"]
    assert len([row for row in current["artifacts"] if row["stage"] == "model-group-result"]) == 1


@pytest.mark.parametrize("failure", ["foreign_resolution", "non_object_member"])
async def test_real_analysis_normalizer_rejects_only_affected_target(store, tmp_path, monkeypatch, failure):
    monkeypatch.syspath_prepend(str(Path(__file__).resolve().parents[1] / "unit"))
    from test_analysis_batch_execution import setup as analysis_setup, response as analysis_response
    from docreader.tender.checks.analysis_batch_execution import validate_member, validate_shared

    _, lease = await setup(store)
    _, context, batches = await analysis_setup(tmp_path)
    batch = batches[0]
    first = analysis_response(batch)
    if failure == "foreign_resolution":
        first["work_units"][1]["decisions"][0]["issue_ids"] = batch["work_units"][0]["issue_ids"]
    else:
        first["work_units"][1] = None
    second = analysis_response({**batch, "target_work_unit_ids": batch["target_work_unit_ids"][1:]})
    model = Model([first, second])
    result = await call_groups_validated(model, store, store.test_artifacts, lease, "verify", batch,
        lambda unit, value: validate_member(context, batch, unit, value),
        validate_shared=lambda response, current: validate_shared(context, response, current))
    assert len(result["work_units"]) == 2
    assert model.calls[1]["target_work_unit_ids"] == batch["target_work_unit_ids"][1:]
    expected = "foreign_resolution_target" if failure == "foreign_resolution" else "missing_work_unit"
    assert model.calls[1]["contract_feedback"]["work_units"][0]["errors"][0]["code"] == expected


@pytest.mark.parametrize("nonempty", [False, True], ids=["empty-no-retry", "nonempty-explicit-correction"])
async def test_member_shared_field_keeps_raw_receipt_and_sibling_acceptance(store, tmp_path, monkeypatch, nonempty):
    monkeypatch.syspath_prepend(str(Path(__file__).resolve().parents[1] / "unit"))
    from test_analysis_batch_execution import setup as analysis_setup, response as analysis_response
    from docreader.tender.checks.analysis_batch_execution import validate_member, validate_shared

    run, lease = await setup(store)
    _, context, batches = await analysis_setup(tmp_path)
    batch = batches[0]
    first = analysis_response(batch)
    sibling, affected = batch["target_work_unit_ids"]
    shared = {"work_unit_ids": [sibling], "finding": {
        "code": "semantic_condition_unclear", "detail": "外层共同反证必须保留。", "blocking": True}}
    misplaced = {"work_unit_ids": [sibling], "finding": {
        "code": "semantic_condition_unclear", "detail": "错层意见由模型明确归属后保留。", "blocking": True}}
    first["shared_findings"] = [shared]
    first["work_units"][1]["shared_findings"] = [misplaced] if nonempty else []
    original = deepcopy(first)
    correction = analysis_response({**batch, "target_work_unit_ids": [affected]})
    correction["shared_findings"] = [misplaced] if nonempty else []
    model = Model([first, correction])
    result = await call_groups_validated(model, store, store.test_artifacts, lease, "verify", batch,
        lambda unit, value: validate_member(context, batch, unit, value),
        validate_shared=lambda response, current: validate_shared(context, response, current))
    assert len(model.calls) == (2 if nonempty else 1)
    assert first == original
    assert result["shared_findings"] == ([shared, misplaced] if nonempty else [shared])
    assert result["work_units"][0] == {"work_unit_id": sibling,
        **validate_member(context, batch, sibling, original["work_units"][0])}
    records = (await store.get_run(run["run_id"]))["artifacts"]
    physical = [store.test_artifacts.read_json(row["artifact_ref"]) for row in records
                if row["stage"] == "model-group-response"]
    assert any(value.get("response") == original for value in physical)
    results = [row for row in records if row["stage"] == "model-group-result"]
    assert len(results) == 2 and len({row["metadata"]["work_unit_id"] for row in results}) == 2
    if nonempty:
        assert model.calls[1]["target_work_unit_ids"] == [affected]
        feedback = model.calls[1]["contract_feedback"]["work_units"][0]
        assert feedback["errors"][0]["code"] == "extra_forbidden"
        assert "outer shared_findings" in feedback["errors"][0]["detail"]
        assert feedback["fragments"] == [{"path": ["shared_findings"], "value": [misplaced]}]
