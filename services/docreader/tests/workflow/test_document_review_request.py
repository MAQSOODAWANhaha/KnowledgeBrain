"""Explicit review scope is pinned before execution and passed to compilation."""

import hashlib
from importlib.resources import files
from types import SimpleNamespace
from unittest.mock import AsyncMock
from uuid import uuid4

from pydantic import ValidationError
import pytest

from docreader.integrations.store import Conflict
from docreader.llm.budget import RequestBudget
from docreader.tender import document_execution
from docreader.tender.document_execution import execute_document
from docreader.tender.schema import AuthoringDocument

from test_authoring_store import authoring_store as authoring_store, input_value, parent


@pytest.fixture
async def review_runtime(authoring_store, monkeypatch):
    store = authoring_store
    await parent(store)
    snapshot = input_value(store)
    monkeypatch.setattr(document_execution, "AuthoringStore", lambda _dsn: store)
    monkeypatch.setattr(document_execution, "read_authoring_input", AsyncMock(return_value=snapshot))
    monkeypatch.setattr(document_execution, "assert_authoring_active", AsyncMock())
    observed = []

    class Client:
        def __init__(self, *_args, **_kwargs):
            pass

        async def __aenter__(self):
            return self

        async def __aexit__(self, *_args):
            return None

    async def generate(artifacts, current, budget, executor, **kwargs):
        observed.append({"kwargs": kwargs, "previous": executor.previous})
        reference = artifacts.put_json({"synthetic": "document component"})
        return artifacts.put_json(AuthoringDocument(authoring_input=current,
            outline_ref=reference, basis_ref=reference, unit_refs=[reference], review_ref=reference,
            docx_ref=reference, package_ref=reference, gaps=[], usage={}).model_dump())

    monkeypatch.setattr(document_execution, "OpenAIModel", Client)
    monkeypatch.setattr(document_execution, "generate_document", generate)

    async def execute(identity, **kwargs):
        config = SimpleNamespace(chat=SimpleNamespace(model="fixture", context_tokens=131072,
                                                       base_url="http://fixture.invalid"))
        return await execute_document(SimpleNamespace(dsn=store.dsn), store.test_artifacts, config,
            RequestBudget(context_tokens=131072, model="fixture"), execution_id=identity,
            run_id=snapshot["run_id"], source_version=1, domain_revision=2, **kwargs)

    return store, observed, execute


@pytest.mark.parametrize("review_request", [None, {"target_ids": ["target-1"], "reason": "资料已提供，请重新核对该功能。"}])
async def test_review_request_is_pinned_and_only_explicit_scope_reaches_pipeline(review_runtime, review_request):
    store, observed, execute = review_runtime
    identity = "review-" + uuid4().hex
    reference = await execute(identity, review_request=review_request)
    execution = await store.get_execution(identity)
    assert execution["status"] == "succeeded" and execution["requests"] == []
    for prompt in ("outline", "outline_assemble", "authoring_review"):
        assert execution["execution_spec"]["prompts"][prompt] == hashlib.sha256(
            files("docreader.tender.prompts").joinpath(prompt + ".md").read_bytes()).hexdigest()
    if review_request is None:
        assert "review_request" not in execution["execution_spec"]
        assert "review_request" not in observed[0]["kwargs"]
    else:
        assert execution["execution_spec"]["review_request"] == review_request
        assert observed[0]["kwargs"]["review_request"] == review_request
    changed = {"target_ids": ["target-1"], "reason": "本次范围或重审理由发生变化。"}
    with pytest.raises(Conflict):
        await execute(identity, review_request=changed)
    assert len(observed) == 1 and (await store.get_execution(identity))["result_ref"] == reference


async def test_new_review_execution_inherits_predecessor_without_replacing_it(review_runtime):
    store, observed, execute = review_runtime
    original, revision = uuid4().hex, uuid4().hex
    before = await execute(original)
    review_request = {"target_ids": ["target-2"], "reason": "该目标已有功能资料，重新核对先前留空是否正确。"}
    await execute(revision, previous_execution_id=original, review_request=review_request)
    assert observed[-1]["previous"]["execution_id"] == original
    assert observed[-1]["kwargs"]["review_request"] == review_request
    assert (await store.get_execution(original))["result_ref"] == before
    assert (await store.get_execution(revision))["execution_spec"]["review_request"] == review_request


@pytest.mark.parametrize("review_request", [
    {"target_ids": [], "reason": "重审"}, {"target_ids": ["target"], "reason": ""},
    {"target_ids": ["target"], "reason": "  "}, {"target_ids": [1], "reason": "重审"},
])
async def test_invalid_review_request_does_not_enter_persistence(monkeypatch, review_request):
    def store(_dsn):
        raise AssertionError("Review review_request validation must precede persistence.")

    monkeypatch.setattr(document_execution, "AuthoringStore", store)
    config = SimpleNamespace(chat=SimpleNamespace(model="fixture", context_tokens=131072))
    with pytest.raises(ValidationError):
        await execute_document(SimpleNamespace(dsn="fixture"), None, config,
            RequestBudget(context_tokens=131072, model="fixture"), execution_id="invalid-review",
            run_id="analysis", source_version=1, domain_revision=1, review_request=review_request)
