"""Paid usage stays distinct from estimates and local persistence failures."""

from importlib.resources import files
from types import SimpleNamespace
from unittest.mock import AsyncMock

import pytest

from docreader.integrations.artifacts import ArtifactIntegrityError, ArtifactStore
from docreader.integrations.store import Lease
from docreader.llm.budget import RequestBudget
from docreader.llm.recovery import call_validated
from docreader.llm.request import build_request, request_digest
from docreader.tender import document_execution
from docreader.tender.document_execution import _DocumentExecutor, _usage, execute_document
from docreader.tender.schema import (
    AUTHORING_GROUNDING_POLICY, AuthoringBlock, CellFill, CellFillResponse, ResponseTarget,
)


MODEL_PHASES = [("outline", "source"), ("outline", "assemble"), ("authoring_basis", "basis"),
                ("authoring_write", "write"), ("authoring_review", "content"), ("authoring_review", "structure")]


def model_request(task, operation):
    prompt = "outline_assemble" if operation == "assemble" else task
    return build_request("fixture", files("docreader.tender.prompts").joinpath(prompt + ".md").read_text("utf-8"),
                         {"records": [], "operation": operation, "grounding_policy": AUTHORING_GROUNDING_POLICY})


def row(task, logical, usage, *, sent=True):
    return {"task": task, "sent_at": "sent" if sent else None, "usage": usage,
            "metadata": {"logical_request_digest": logical, "input_tokens": 123}}


def test_usage_reports_partial_and_malformed_counters_as_unknown_without_inventing_tokens():
    value = _usage([
        row("outline", "one", {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}),
        row("outline", "one", None),
        row("authoring_write", "one", {"prompt_tokens": 7}),
        row("authoring_write", "two", {"prompt_tokens": {"invalid": 2}, "total_tokens": -1}),
        row("authoring_review", "prepared", None, sent=False),
    ])
    assert value == {"sent_requests": 4, "retry_requests": 1, "estimated_input_tokens": 492,
                     "prompt_tokens": 17, "completion_tokens": 5, "total_tokens": 15, "unknown_usage_requests": 3}


@pytest.mark.parametrize("phase", ["call", "validation"])
@pytest.mark.parametrize("error_type", [OSError, ArtifactIntegrityError])
async def test_opted_in_persistence_errors_escape_without_consuming_model_retries(tmp_path, phase, error_type):
    calls = []
    store = SimpleNamespace(receipt=AsyncMock(return_value=None))

    async def call(task, payload):
        calls.append(payload)
        if phase == "call":
            raise error_type("synthetic persistence failure")
        return {}

    def validate(value):
        raise error_type("synthetic persistence failure")

    with pytest.raises(error_type, match="synthetic persistence failure"):
        await call_validated(SimpleNamespace(call=call), store, ArtifactStore(tmp_path), Lease("run", 1, 1, "cmd"),
                             "authoring_write", {}, validate, passthrough_exceptions=(OSError, ArtifactIntegrityError))
    assert calls == [{}]


def test_old_content_artifacts_remain_readable_without_claiming_material_support(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    block_ref = artifacts.put_json({"kind": "paragraph", "text": "历史正文"})
    fill_ref = artifacts.put_json({"source_ref": {
        "source_index_ref": artifacts.put_json({"fixture": "source"}), "unit_id": "grid"},
        "row": 0, "column": 1, "text": "历史填值"})
    block = AuthoringBlock.model_validate(artifacts.read_json(block_ref))
    fill = CellFill.model_validate(artifacts.read_json(fill_ref))
    response = CellFillResponse.model_validate({"source_id": "s1", "row": 0, "column": 1, "text": "新回包"})
    assert block.target_ids == block.material_ids == block.source_refs == []
    assert fill.target_ids == fill.material_ids == fill.source_refs == []
    assert response.target_ids == response.support_ids == []
    old_target = ResponseTarget.model_validate({"target_id": "old-form", "kind": "form", "purpose": "历史表单",
        "rule_refs": [], "source_refs": [fill.source_ref.model_dump()]})
    assert old_target.template_refs == [], "Historical context is not automatically an insertion template."


@pytest.mark.parametrize("task,operation", MODEL_PHASES)
async def test_v2_contract_does_not_reuse_v1_authoring_or_source_responses(tmp_path, monkeypatch, task, operation):
    artifacts = ArtifactStore(tmp_path)
    client = SimpleNamespace(config=SimpleNamespace(chat=SimpleNamespace(base_url="http://fixture")))
    old_request = build_request("fixture", "v1 source and authoring contract",
                                {"records": [], "grounding_policy": "knowledge_required_v1"})
    request = model_request(task, operation)
    old_digest = request_digest(client.config.chat.base_url, old_request)
    reference = artifacts.put_json({"task": task, "input_digest": old_digest, "response": {"content": "old"}})
    previous = {"execution_id": "old-execution", "receipts": [{"stage": "document-model-response",
                "scope": f"{task}:{old_digest}", "input_digest": old_digest, "artifact_ref": reference}]}
    store = SimpleNamespace(receipt=AsyncMock(return_value=None), commit_artifact=AsyncMock())
    calls = []

    class Model:
        def __init__(self, *_args):
            pass

        async def __call__(self, sent, validate):
            calls.append(sent)
            return validate({"content": "grounded"})

    monkeypatch.setattr(document_execution, "_DocumentModel", Model)
    executor = _DocumentExecutor(client, store, artifacts, SimpleNamespace(execution_id="new-execution"),
        RequestBudget(context_tokens=131072, model="fixture", prompts={}),
        {"grounding_policy": AUTHORING_GROUNDING_POLICY}, previous)
    result = await executor(task, request, lambda value: value)
    assert result == {"content": "grounded"}
    assert calls == [request]


@pytest.mark.parametrize("task,operation", MODEL_PHASES)
async def test_unchanged_v2_requests_reuse_predecessor_and_rebind_host_identity(tmp_path, monkeypatch, task, operation):
    artifacts = ArtifactStore(tmp_path)
    client = SimpleNamespace(config=SimpleNamespace(chat=SimpleNamespace(base_url="http://fixture")))
    request = model_request(task, operation)
    digest = request_digest(client.config.chat.base_url, request)
    reference = artifacts.put_json({"task": task, "input_digest": digest, "response": {"content": "accepted v2"}})
    previous = {"execution_id": "previous-v2", "receipts": [{"stage": "document-model-response",
                "scope": f"{task}:{digest}", "input_digest": digest, "artifact_ref": reference}]}
    store = SimpleNamespace(receipt=AsyncMock(return_value=None), commit_artifact=AsyncMock())

    class Model:
        def __init__(self, *_args):
            raise AssertionError("An unchanged accepted request must not issue a new model call.")

    monkeypatch.setattr(document_execution, "_DocumentModel", Model)
    executor = _DocumentExecutor(client, store, artifacts, SimpleNamespace(execution_id="current-v2"),
        RequestBudget(context_tokens=131072, model="fixture", prompts={}),
        {"grounding_policy": AUTHORING_GROUNDING_POLICY}, previous)
    result = await executor(task, request, lambda value: {"text": value["content"], "target_id": "current-target"})
    assert result == {"text": "accepted v2", "target_id": "current-target"}
    assert artifacts.read_json(reference)["response"] == {"content": "accepted v2"}
    store.commit_artifact.assert_awaited_once_with(executor.lease, "document-model-response", reference,
        scope=f"{task}:{digest}", input_digest=digest, metadata={"reused_from_execution_id": "previous-v2"})


async def test_document_execution_pins_v2_grounding_policy_without_changing_execution_protocol(tmp_path, monkeypatch):
    class Submitted(RuntimeError):
        pass

    store = SimpleNamespace(setup=AsyncMock(), get_execution=AsyncMock(return_value=None),
                            submit=AsyncMock(side_effect=Submitted))
    monkeypatch.setattr(document_execution, "AuthoringStore", lambda _dsn: store)
    monkeypatch.setattr(document_execution, "read_authoring_input", AsyncMock(return_value={"fixture": "input"}))
    monkeypatch.setattr(document_execution, "assert_authoring_active", AsyncMock())
    config = SimpleNamespace(chat=SimpleNamespace(model="fixture", context_tokens=131072, base_url="http://fixture"))
    with pytest.raises(Submitted):
        await execute_document(SimpleNamespace(dsn="fixture"), ArtifactStore(tmp_path), config,
            RequestBudget(context_tokens=131072, model="fixture", prompts={}), execution_id="new-execution",
            run_id="fixture-run", source_version=1, domain_revision=1)
    spec = store.submit.await_args.args[2]
    assert spec["grounding_policy"] == AUTHORING_GROUNDING_POLICY == "knowledge_required_v2"
    assert spec["protocol"] == "tender_document_execution_v1"
