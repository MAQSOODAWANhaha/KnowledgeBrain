"""Fixed outline inputs and execution modes remain immutable across retries."""

from copy import deepcopy
from types import SimpleNamespace
from unittest.mock import AsyncMock

import pytest

from docreader.integrations.artifacts import ArtifactIntegrityError, ArtifactStore
from docreader.integrations.store import Conflict
from docreader.llm.budget import RequestBudget
from docreader.tender import document_execution
from docreader.tender.bid_format_outline import BID_FORMAT_OUTLINE_CONTRACT
from docreader.tender.item_placement import REQUIREMENT_CHAIN_CONTRACT
from docreader.tender.schema import AuthoringInput, AuthoringOutline


@pytest.fixture
def runtime(tmp_path, monkeypatch):
    artifacts = ArtifactStore(tmp_path)
    snapshot = AuthoringInput(schema_version=2, run_id="fixed-run", source_version=1,
                              domain_revision=1).model_dump()
    outline = AuthoringOutline.model_validate({"schema_version": 2, "authoring_input": snapshot, "issues": [], "chapters": [{
        "chapter_id": "chapter-1", "title": "响应章节", "children": [], "targets": [{
            "target_id": "response", "kind": "form", "purpose": "企业信息待人工填写",
            "rule_refs": [], "source_refs": [], "manual_fill": True, "support_origins": ["tender"],
        }],
    }]}).model_dump()
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


async def test_fixed_outline_preview_does_not_start_source_resolution(runtime):
    runtime.snapshot['frozen_ref'] = runtime.artifacts.put_json({'documents': []})
    runtime.outline['authoring_input'] = deepcopy(runtime.snapshot)
    reference = runtime.artifacts.put_json(runtime.outline)
    await runtime.execute(outline_only=True, outline_ref=reference)
    assert 'native_body_contract' not in runtime.observed[0][1]
    assert 'directory_native_body_contract' not in runtime.rows['execution']['execution_spec']


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


async def test_candidate_joint_contract_is_pinned_at_first_outline_execution(runtime, monkeypatch):
    from docreader.tender.native_requirement_sources import CONTRACT

    runtime.snapshot['frozen_ref'] = runtime.artifacts.put_json({'documents': []})
    runtime.outline['authoring_input'] = deepcopy(runtime.snapshot)
    monkeypatch.setattr(document_execution, '_has_outline_responses', lambda _chapters: True)
    monkeypatch.setenv('KNOWLEDGEBRAIN_TENDER_NATIVE_REQUIREMENT_CONTRACT', CONTRACT)
    result = await runtime.execute(outline_only=True)
    spec = runtime.rows['execution']['execution_spec']
    assert spec['native_requirement_contract'] == CONTRACT
    assert 'directory_native_body_contract' not in spec
    assert runtime.observed[0][1]['native_requirement_contract'] == CONTRACT

    monkeypatch.delenv('KNOWLEDGEBRAIN_TENDER_NATIVE_REQUIREMENT_CONTRACT')
    assert await runtime.execute(outline_only=True) == result
    assert len(runtime.observed) == 1

    await runtime.execute(execution_id='legacy', outline_only=True)
    assert 'native_requirement_contract' not in runtime.rows['legacy']['execution_spec']
    monkeypatch.setenv('KNOWLEDGEBRAIN_TENDER_NATIVE_REQUIREMENT_CONTRACT', CONTRACT)
    assert await runtime.execute(execution_id='legacy', outline_only=True) == result


@pytest.fixture
def frozen_runtime(runtime):
    runtime.snapshot['frozen_ref'] = runtime.artifacts.put_json({'documents': []})
    runtime.outline['authoring_input'] = deepcopy(runtime.snapshot)
    return runtime


@pytest.mark.parametrize('legacy_env', [None, 'native_requirement_sources_v1'])
async def test_w2_outline_dispatches_only_its_pinned_contracts(frozen_runtime, monkeypatch, legacy_env):
    if legacy_env is None:
        monkeypatch.delenv('KNOWLEDGEBRAIN_TENDER_NATIVE_REQUIREMENT_CONTRACT', raising=False)
    else:
        monkeypatch.setenv('KNOWLEDGEBRAIN_TENDER_NATIVE_REQUIREMENT_CONTRACT', legacy_env)

    await frozen_runtime.execute(outline_only=True, requirement_chain_contract=REQUIREMENT_CHAIN_CONTRACT)
    spec = frozen_runtime.rows['execution']['execution_spec']
    options = frozen_runtime.observed[0][1]
    assert spec['requirement_chain_contract'] == options['requirement_chain_contract'] == REQUIREMENT_CHAIN_CONTRACT
    assert spec['directory_planning_contract'] == options['directory_contract'] == BID_FORMAT_OUTLINE_CONTRACT
    assert {'directory_native_body_contract', 'native_requirement_contract'}.isdisjoint(spec)
    assert {'native_body_contract', 'native_requirement_contract', 'on_source_bodies'}.isdisjoint(options)
    assert set(spec['prompts']) == {'bid_format_outline', 'item_placement'}
    assert set(spec['budget']['output_contracts']) == {'BidFormatOutline', 'ItemPlacementResponse'}


async def test_resumed_w2_outline_uses_stored_chain_without_opt_in(frozen_runtime, monkeypatch):
    from docreader.tender.native_requirement_sources import CONTRACT

    monkeypatch.delenv('KNOWLEDGEBRAIN_TENDER_NATIVE_REQUIREMENT_CONTRACT', raising=False)
    first = await frozen_runtime.execute(outline_only=True, requirement_chain_contract=REQUIREMENT_CHAIN_CONTRACT)
    frozen_runtime.rows['execution']['status'] = 'failed'
    monkeypatch.setenv('KNOWLEDGEBRAIN_TENDER_NATIVE_REQUIREMENT_CONTRACT', CONTRACT)

    assert await frozen_runtime.execute(outline_only=True) == first
    assert len(frozen_runtime.observed) == 2
    resumed = frozen_runtime.observed[1][1]
    assert resumed['requirement_chain_contract'] == REQUIREMENT_CHAIN_CONTRACT
    assert resumed['directory_contract'] == BID_FORMAT_OUTLINE_CONTRACT
    assert {'native_body_contract', 'native_requirement_contract', 'on_source_bodies'}.isdisjoint(resumed)


@pytest.mark.parametrize(('prompt', 'affected', 'unaffected'), [
    ('directory_material', 'legacy', 'w2'),
    ('item_placement', 'w2', 'legacy'),
])
async def test_outline_replays_ignore_a_prompt_from_the_other_chain(
        frozen_runtime, monkeypatch, prompt, affected, unaffected):
    monkeypatch.delenv('KNOWLEDGEBRAIN_TENDER_NATIVE_REQUIREMENT_CONTRACT', raising=False)
    load_prompts = document_execution.load_prompts
    changed = {}
    monkeypatch.setattr(document_execution, 'load_prompts', lambda: {**load_prompts(), **changed})

    results = {
        'legacy': await frozen_runtime.execute('legacy', outline_only=True),
        'w2': await frozen_runtime.execute('w2', outline_only=True, requirement_chain_contract=REQUIREMENT_CHAIN_CONTRACT),
    }
    legacy_spec = frozen_runtime.rows['legacy']['execution_spec']
    assert 'directory_native_body_contract' in legacy_spec
    assert 'native_body_contract' in frozen_runtime.observed[0][1]

    changed[prompt] = 'changed prompt'
    assert await frozen_runtime.execute(unaffected, outline_only=True) == results[unaffected]
    with pytest.raises(Conflict):
        await frozen_runtime.execute(affected, outline_only=True)
