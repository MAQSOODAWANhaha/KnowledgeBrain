"""Durable semantic observations never manufacture human ground truth."""

from copy import deepcopy
import json
from pathlib import Path
from types import SimpleNamespace

import pytest
import psycopg

from docreader.integrations.artifacts import ArtifactStore
from docreader.integrations.store import Cancelled, Conflict, Lease
from docreader.llm.budget import RequestBudget
from docreader.llm.request import ModelInfrastructureError, request_digest
from docreader.tender.prompts import get_prompt
from docreader.tender.semantic_evaluation import (
    _usage, execute_semantic_evaluation, prepare_semantic_evaluation, validate_review_response,
)


CORPUS = Path(__file__).resolve().parents[4] / "docs/design/python-tender-phase1/semantic-cases.json"


def cases(count=2):
    return deepcopy(json.loads(CORPUS.read_bytes())["cases"][:count])


class ReceiptStore:
    def __init__(self, artifacts):
        self.artifacts, self.receipts, self.requests = artifacts, {}, []

    async def receipt(self, _lease, stage, scope="", *, input_digest=None):
        value = self.receipts.get((stage, scope))
        if value is not None and input_digest is not None and value["input_digest"] != input_digest:
            raise Conflict("immutable receipt changed")
        return deepcopy(value)

    async def commit_artifact(self, _lease, stage, artifact_ref, scope="", *, input_digest="", metadata=None):
        value = {"stage": stage, "scope": scope, "artifact_ref": artifact_ref,
                 "input_digest": input_digest, "metadata": metadata or {}}
        previous = self.receipts.setdefault((stage, scope), value)
        if previous != value:
            raise Conflict("immutable receipt changed")
        return value

    async def get_run(self, _run_id):
        return {"usage": deepcopy(self.requests), "artifacts": deepcopy(list(self.receipts.values()))}


class Model:
    def __init__(self, store, *, context_tokens=131072, outcome=None, unknown_usage=False):
        self.store, self.calls, self.outcome = store, [], outcome
        self.unknown_usage = unknown_usage
        self.config = SimpleNamespace(chat=SimpleNamespace(base_url="http://fixture.invalid/v1", api_key="private-key"))
        self.budget = RequestBudget(context_tokens=context_tokens, model="fixture",
                                   prompts={"verify": get_prompt("verify")}, tokenizer="o200k_base")

    def request_budget(self):
        return self.budget

    def execution_spec(self):
        return {"protocol": "synthetic-evaluation", "budget": self.budget.spec()}

    async def call(self, task, payload):
        self.calls.append(deepcopy(payload))
        baseline = {key: value for key, value in payload.items() if key != "contract_feedback"}
        logical = request_digest(self.config.chat.base_url, self.budget.request(task, baseline))
        request = self.budget.request(task, payload)
        digest = request_digest(self.config.chat.base_url, request)
        value = {"schema_version": 2, "accepted_decision_ids": [], "findings": []}
        if self.outcome is not None:
            value = self.outcome(payload)
        row = {"request_id": str(len(self.store.requests)), "task": task, "sent_at": "sent",
               "status": "failed" if isinstance(value, BaseException) else "succeeded",
               "metadata": {**self.budget.measure_request(request, task),
                            "request_digest": digest, "logical_request_digest": logical},
               "usage": None if self.unknown_usage else
                   {"prompt_tokens": 11, "completion_tokens": 7, "total_tokens": 18},
               "response_ref": self.store.artifacts.put_json({"received": str(type(value).__name__)})}
        self.store.requests.append(row)
        if isinstance(value, BaseException):
            raise value
        return value


def runtime(tmp_path, **options):
    artifacts = ArtifactStore(tmp_path)
    store = ReceiptStore(artifacts)
    return artifacts, store, Model(store, **options), Lease("evaluation-run", 1, 1, "evaluation-command")


async def invoke(values, runtime, identity="evaluation"):
    artifacts, store, model, lease = runtime
    reference = await execute_semantic_evaluation(values, evaluation_id=identity, store=store,
        artifacts=artifacts, lease=lease, model=model, concurrency=2)
    return reference, artifacts.read_json(reference)


def test_preflight_uses_current_contract_and_never_leaks_draft_answers(tmp_path):
    _, _, model, _ = runtime(tmp_path)
    values = cases(32)
    values[0]["expected"]["rationale"] = "PRIVATE_DRAFT_EXPECTATION"
    plan = prepare_semantic_evaluation(values, model=model)
    assert plan["planned_requests"] == 32 and plan["maximum_sends"] == 128
    assert len(model.calls) == 0
    for entry in plan["cases"]:
        payload = json.loads(entry["request"]["messages"][1]["content"][0]["text"])
        assert not set(payload) & {"case_id", "expected", "category", "provenance"}
        assert "PRIVATE_DRAFT_EXPECTATION" not in json.dumps(entry["request"])
        assert payload["work_unit_id"] == "w1"
        assert entry["budget"]["fits"] is True


async def test_success_has_durable_sources_usage_and_no_semantic_score(tmp_path):
    state = runtime(tmp_path)
    _, report = await invoke(cases(), state)
    artifacts, _, model, _ = state
    assert report["counts"] == {"succeeded": 2, "exhausted": 0, "budget_exceeded": 0}
    assert report["total_cases"] == 2
    assert report["usage"]["sent_requests"] == 2 and report["usage"]["total_tokens"] == 36
    assert not report["human_review_completed"] and not report["semantic_acceptance"]
    assert report["model_execution_completed"] is True
    assert "accuracy" not in report
    assert "private-key" not in json.dumps(report)
    for record in report["cases"]:
        assert record["adjudication"] is None and record["annotation_review"] is None
        assert artifacts.read_json(record["response_ref"])["findings"] == []
        assert artifacts.read_json(record["request_ref"])["response_format"] == {"type": "json_object"}
        assert record["requests"][0]["response_ref"]
    assert all("expected" not in payload for payload in model.calls)


async def test_resume_returns_identical_report_without_new_calls(tmp_path):
    state = runtime(tmp_path)
    first, before = await invoke(cases(), state)
    second, after = await invoke(cases(), state)
    assert first == second and before == after
    assert len(state[2].calls) == 2


async def test_one_changed_input_reuses_other_cases_in_new_evaluation(tmp_path):
    state = runtime(tmp_path)
    values = cases()
    await invoke(values, state)
    values[0]["payload"]["items"][0]["text"] += "（修订后的语义候选）"
    _, report = await invoke(values, state, "revision-2")
    assert len(state[2].calls) == 3
    assert sum(record["reused"] for record in report["cases"]) == 1
    assert report["new_usage"]["sent_requests"] == 1


async def test_same_identity_rejects_changed_inputs_before_a_new_send(tmp_path):
    state = runtime(tmp_path)
    values = cases()
    await invoke(values, state)
    values[0]["payload"]["items"][0]["text"] += "changed"
    with pytest.raises(Conflict):
        await invoke(values, state)
    assert len(state[2].calls) == 2


async def test_rejected_contract_gets_feedback_and_keeps_received_rejection(tmp_path, monkeypatch):
    async def no_wait(_seconds):
        return None
    monkeypatch.setattr("docreader.llm.recovery.asyncio.sleep", no_wait)
    def outcome(payload):
        if "contract_feedback" not in payload:
            return {"schema_version": 2, "accepted_decision_ids": ["unknown-decision"], "findings": []}
        return {"schema_version": 2, "accepted_decision_ids": [], "findings": []}
    state = runtime(tmp_path, outcome=outcome)
    _, report = await invoke(cases(1), state)
    assert len(state[2].calls) == 2
    assert state[2].calls[1]["contract_feedback"]["errors"]
    assert any(stage == "model-rejection" for stage, _ in state[1].receipts)
    assert report["usage"]["retry_requests"] == 1
    assert report["cases"][0]["status"] == "succeeded"
    assert len(report["cases"][0]["rejection_refs"]) == 1


async def test_exhausted_case_stays_in_denominator_and_does_not_stop_other_cases(tmp_path, monkeypatch):
    async def no_wait(_seconds):
        return None
    monkeypatch.setattr("docreader.llm.recovery.asyncio.sleep", no_wait)
    values = cases()
    failed = values[0]["payload"]["work_unit_id"]
    def outcome(payload):
        return (ModelInfrastructureError("private transport details") if payload["work_unit_id"] == failed
                else {"schema_version": 2, "accepted_decision_ids": [], "findings": []})
    state = runtime(tmp_path, outcome=outcome, unknown_usage=True)
    _, report = await invoke(values, state)
    assert report["total_cases"] == 2 and report["counts"]["exhausted"] == 1
    assert report["counts"]["succeeded"] == 1
    assert len(state[2].calls) == 5
    assert report["usage"]["unknown_usage_requests"] == 5
    assert "private transport details" not in json.dumps(report)
    await invoke(values, state)
    assert len(state[2].calls) == 5


async def test_budget_exceeded_is_unsent_and_does_not_block_admitted_cases(tmp_path):
    state = runtime(tmp_path)
    values = cases()
    values[0]["payload"]["source_views"][0]["body"][0]["text"] += "x" * 800000
    _, report = await invoke(values, state)
    assert report["counts"]["budget_exceeded"] == 1
    assert report["counts"]["succeeded"] == 1 and len(state[2].calls) == 1
    assert report["cases"][0]["requests"] == []


async def test_cancellation_propagates_without_becoming_a_semantic_result(tmp_path):
    state = runtime(tmp_path, outcome=lambda _payload: Cancelled("cancelled"))
    with pytest.raises(Cancelled):
        await invoke(cases(1), state)
    assert not any(stage == "semantic-evaluation-report" for stage, _ in state[1].receipts)


@pytest.mark.parametrize("failure", [OSError, psycopg.OperationalError])
async def test_persistence_failure_escapes_without_exhausting_model_allowance(tmp_path, failure):
    state = runtime(tmp_path, outcome=lambda _payload: failure("synthetic persistence failure"))
    with pytest.raises(failure):
        await invoke(cases(1), state)
    assert len(state[2].calls) == 1
    assert not any(stage == "semantic-evaluation-report" for stage, _ in state[1].receipts)


async def test_identical_requests_are_paid_once_even_when_the_case_labels_differ(tmp_path):
    state = runtime(tmp_path)
    values = cases(1)
    duplicate = deepcopy(values[0])
    duplicate["case_id"] += "-different-local-label"
    duplicate["expected"]["verdict"] = "unresolved"
    _, report = await invoke([*values, duplicate], state)
    assert report["total_cases"] == 2 and report["counts"]["succeeded"] == 2
    assert len(state[2].calls) == 1
    assert report["usage"]["sent_requests"] == 1
    assert report["cases"][0]["response_ref"] == report["cases"][1]["response_ref"]


def test_partial_or_invalid_usage_remains_unknown():
    rows = [{"sent_at": "sent", "metadata": {"logical_request_digest": "case"},
             "usage": {"prompt_tokens": 11}},
            {"sent_at": "sent", "metadata": {"logical_request_digest": "case"},
             "usage": {"prompt_tokens": {"invalid": 3}, "completion_tokens": -1}},
            {"sent_at": None, "metadata": {"logical_request_digest": "unsent"}, "usage": None}]
    value = _usage(rows)
    assert value["sent_requests"] == 2 and value["retry_requests"] == 1
    assert value["prompt_tokens"] == 11
    assert value["completion_tokens"] == value["total_tokens"] == 0
    assert value["unknown_usage_requests"] == 2


@pytest.mark.parametrize("damage", ["expected", "operation", "duplicate", "empty"])
def test_invalid_inputs_are_rejected_before_sending(tmp_path, damage):
    values = cases()
    if damage == "expected":
        values[0]["payload"]["expected"] = {"verdict": "incorrect"}
    elif damage == "operation":
        values[0]["payload"]["operation"] = "locate"
    elif damage == "duplicate":
        values[1]["case_id"] = values[0]["case_id"]
    else:
        values = []
    with pytest.raises(ValueError):
        prepare_semantic_evaluation(values, model=runtime(tmp_path)[2])


def test_host_review_scope_is_derived_and_findings_must_use_supplied_references():
    payload = cases(1)[0]["payload"]
    value = validate_review_response({"schema_version": 2, "accepted_decision_ids": [], "findings": [],
                                     "reviewed_range_ids": ["wrong-echo"]}, payload)
    assert "wrong-echo" not in value["reviewed_range_ids"]
    assert payload["source_views"][0]["body"][0]["range_id"] in value["reviewed_range_ids"]
    with pytest.raises(ValueError):
        validate_review_response({"schema_version": 2, "accepted_decision_ids": [], "findings": [
            {"code": "free-form-code", "detail": "Semantic concern", "blocking": True,
             "item_ids": ["not-supplied"]}]}, payload)
