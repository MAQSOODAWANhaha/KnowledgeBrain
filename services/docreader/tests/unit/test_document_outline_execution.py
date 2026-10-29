"""Fixed outline inputs and execution modes remain immutable across retries."""

from copy import deepcopy
from types import SimpleNamespace
from unittest.mock import AsyncMock

import pytest

from docreader.integrations.artifacts import ArtifactIntegrityError, ArtifactStore
from docreader.integrations.store import Conflict
from docreader.llm.budget import RequestBudget
from docreader.tender import document_execution
from docreader.tender.schema import AuthoringInput, AuthoringOutline


@pytest.fixture
def runtime(tmp_path, monkeypatch):
    artifacts = ArtifactStore(tmp_path)
    snapshot = AuthoringInput(schema_version=2, run_id="fixed-run", source_version=1,
                              domain_revision=1).model_dump()
    outline = AuthoringOutline(schema_version=2, authoring_input=snapshot, issues=[], chapters=[]).model_dump()
    outline_ref = artifacts.put_json(outline)
    rows = {}
    store = SimpleNamespace(setup=AsyncMock(), commit_artifact=AsyncMock(), heartbeat=AsyncMock())

    async def get(execution_id, **_kwargs):
        return deepcopy(rows.get(execution_id))

    async def submit(execution_id, authoring_input, spec):
        if execution_id in rows:
            row = rows[execution_id]
            if row["authoring_input"] != authoring_input or row["execution_spec"] != spec:
                raise Conflict("execution_id already identifies another fixed input")
        else:
            rows[execution_id] = {"authoring_input": deepcopy(authoring_input), "execution_spec": deepcopy(spec),
                "status": "queued", "cancel_requested": False, "requests": [], "receipts": []}

    async def acquire(execution_id, **_kwargs):
        return None if rows[execution_id]["status"] == "succeeded" else SimpleNamespace(execution_id=execution_id)

    async def release(lease, status, **kwargs):
        rows[lease.execution_id].update(status=status, **kwargs)

    store.get_execution, store.submit, store.acquire, store.release = get, submit, acquire, release
    monkeypatch.setattr(document_execution, "AuthoringStore", lambda _dsn: store)
    read_current = AsyncMock(return_value=snapshot)
    monkeypatch.setattr(document_execution, "read_authoring_input", read_current)
    monkeypatch.setattr(document_execution, "assert_authoring_active", AsyncMock())

    class Client:
        def __init__(self, *_args, **_kwargs):
            pass

        async def __aenter__(self):
            return self

        async def __aexit__(self, *_args):
            pass

    monkeypatch.setattr(document_execution, "OpenAIModel", Client)
    observed = []

    async def generate(artifacts, fixed, _budget, _execute, **options):
        observed.append((deepcopy(fixed), deepcopy(options)))
        if options.get("outline_only"):
            return artifacts.put_json(outline)
        ref = artifacts.put_json({"fixture": "artifact"})
        result_outline_ref = artifacts.put_json(options["outline"]) if options.get("outline") is not None else ref
        return artifacts.put_json({"schema_version": 2, "authoring_input": fixed,
            "outline_ref": result_outline_ref,
            "basis_ref": ref, "unit_refs": [], "review_ref": ref, "docx_ref": ref,
            "package_ref": ref, "gaps": [], "usage": {}})

    monkeypatch.setattr(document_execution, "generate_document", generate)

    async def execute(execution_id="execution", **options):
        return await document_execution.execute_document(SimpleNamespace(dsn="fixture"), artifacts,
            SimpleNamespace(chat=SimpleNamespace(model="fixture", context_tokens=131072, base_url="http://fixture")),
            RequestBudget(context_tokens=131072, model="fixture", prompts={}), execution_id=execution_id,
            run_id="fixed-run", source_version=1, domain_revision=1, **options)

    return SimpleNamespace(artifacts=artifacts, snapshot=snapshot, outline=outline, outline_ref=outline_ref,
        execute=execute, rows=rows, observed=observed, read_current=read_current)


async def test_outline_execution_returns_outline_and_reuses_completed_mode(runtime):
    reference = await runtime.execute(outline_only=True)
    assert runtime.artifacts.read_json(reference) == runtime.outline
    assert await runtime.execute(outline_only=True) == reference
    assert len(runtime.observed) == 1
    assert runtime.rows["execution"]["execution_spec"]["outline_only"] is True
    with pytest.raises(Conflict):
        await runtime.execute()


async def test_fixed_outline_pins_its_original_snapshot_without_reading_new_analysis(runtime):
    runtime.read_current.side_effect = AssertionError("A later analysis root must not replace the fixed outline input.")
    reference = await runtime.execute(outline_ref=runtime.outline_ref)
    assert runtime.artifacts.read_json(reference)["authoring_input"] == runtime.snapshot
    assert runtime.observed[0][1]["outline"] == runtime.outline
    spec = runtime.rows["execution"]["execution_spec"]
    assert spec["outline_ref"] == runtime.outline_ref and spec["outline_only"] is False
    assert await runtime.execute(outline_ref=runtime.outline_ref) == reference
    changed = {**runtime.outline, "issues": [{"code": "manual", "detail": "changed", "rule_refs": []}]}
    with pytest.raises(Conflict):
        await runtime.execute(outline_ref=runtime.artifacts.put_json(changed))
    assert len(runtime.observed) == 1


async def test_outline_from_another_analysis_identity_fails_before_execution_submission(runtime):
    other = deepcopy(runtime.outline)
    other["authoring_input"]["domain_revision"] = 2
    with pytest.raises(Conflict, match="input|snapshot|identity"):
        await runtime.execute(outline_ref=runtime.artifacts.put_json(other))
    assert runtime.rows == {} and runtime.observed == []


async def test_completed_outline_result_must_match_pinned_snapshot(runtime):
    await runtime.execute(outline_only=True)
    other = deepcopy(runtime.outline)
    other["authoring_input"]["source_version"] = 2
    runtime.rows["execution"]["result_ref"] = runtime.artifacts.put_json(other)
    with pytest.raises(ArtifactIntegrityError, match="snapshot"):
        await runtime.execute(outline_only=True)


async def test_outline_only_execution_cannot_replace_a_document_predecessor(runtime):
    await runtime.execute("planning", outline_only=True)
    with pytest.raises(Conflict, match="predecessor"):
        await runtime.execute("body", outline_ref=runtime.outline_ref, previous_execution_id="planning")
    assert "body" not in runtime.rows and len(runtime.observed) == 1


async def test_queued_execution_uses_its_submitted_snapshot_after_analysis_changes(runtime):
    reference = runtime.artifacts.put_json(runtime.snapshot)
    runtime.read_current.side_effect = AssertionError('Later analysis must not replace queued input')
    result = await runtime.execute(authoring_input_ref=reference, outline_only=True)
    assert runtime.artifacts.read_json(result)['authoring_input'] == runtime.snapshot
    assert runtime.rows['execution']['execution_spec']['authoring_input_ref'] == reference
    assert await runtime.execute(authoring_input_ref=reference, outline_only=True) == result
    changed = {**runtime.snapshot, 'domain_revision': 2}
    with pytest.raises(Conflict):
        await runtime.execute(authoring_input_ref=runtime.artifacts.put_json(changed), outline_only=True)
