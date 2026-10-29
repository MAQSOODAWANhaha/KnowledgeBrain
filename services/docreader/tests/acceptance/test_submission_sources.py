"""Supplemental tender references are fixed inputs of the durable document entry."""

from copy import deepcopy
from uuid import uuid4

import httpx
import pytest

from docreader.integrations.authoring import read_authoring_input
from docreader.integrations.authoring_store import AuthoringStore
from docreader.integrations.store import Conflict
from docreader.tender import document_execution
from docreader.tender.authoring import _OutlineContext
from docreader.tender.schema import AuthoringUsage

from test_analysis import analysis_runtime as analysis_runtime, execute_analysis
from test_authoring_execution import config
from test_authoring_outline import budget


pytestmark = pytest.mark.integration


async def test_submission_sources_are_deduplicated_frozen_forwarded_and_conflict_on_same_id_change(
    analysis_runtime, monkeypatch,
):
    store, artifacts, settings, run, _ = analysis_runtime
    await execute_analysis(analysis_runtime)
    identity = {key: run[key] for key in ("run_id", "source_version", "domain_revision")}
    snapshot = await read_authoring_input(store, artifacts, **identity)
    context = _OutlineContext(artifacts, snapshot)
    refs = {(ref["source_index_ref"]["sha256"], ref["unit_id"]): ref
            for group in context.sources.values() for ref in group}
    first, second = list(refs.values())[:2]
    observed = []

    async def generate(artifacts, snapshot, request_budget, execute, *, materials=(), knowledge=None,
                       concurrency=2, submission_sources=(), on_outline=None):
        observed.append(deepcopy(submission_sources))
        ref = artifacts.put_json({"fixture": "entry-contract-only"})
        return artifacts.put_json({"schema_version": 2, "authoring_input": snapshot,
            "outline_ref": ref, "basis_ref": ref, "unit_refs": [], "review_ref": ref,
            "docx_ref": ref, "package_ref": ref, "gaps": [], "usage": AuthoringUsage().model_dump()})

    def unexpected_model_call(request):
        raise AssertionError("The entry-contract fixture must not issue model requests")

    monkeypatch.setattr(document_execution, "generate_document", generate)
    execution_id = uuid4().hex

    async def execute(submission_sources):
        return await document_execution.execute_document(store, artifacts, config(), budget(),
            execution_id=execution_id, **identity, submission_sources=submission_sources,
            transport=httpx.MockTransport(unexpected_model_call))

    reference = await execute([first, deepcopy(first)])
    row = await AuthoringStore(settings.dsn).get_execution(execution_id)
    assert row["status"] == "succeeded" and row["requests"] == []
    assert row["execution_spec"]["submission_sources"] == [first]
    assert observed == [[first]]
    assert await execute([first]) == reference
    assert observed == [[first]]
    with pytest.raises(Conflict):
        await execute([second])
    assert observed == [[first]]
