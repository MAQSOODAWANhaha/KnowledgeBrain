"""The new source-entry binding rule is explicit and immutable per execution."""
from typing import Any, cast

from copy import deepcopy

import pytest

from docreader.llm.budget import RequestBudget
from docreader.llm.request import ModelConfigurationError
from docreader.tender import authoring_pipeline as pipeline
from test_directory_compile import item, node, plan
from test_document_outline_execution import runtime as runtime
from test_reviewed_source_facts import fixture, point, window
from test_source_outline_publication import source_runtime as source_runtime, chapter, target


CONTRACT = 'reviewed_body_unique_entry_v1'


@pytest.mark.parametrize('prior_contract', [None, CONTRACT])
async def test_new_entry_policy_is_pinned_and_legacy_execution_absence_is_preserved(source_runtime, prior_contract):
    source_runtime.outline['chapters'] = [chapter(targets=[target()])]
    ref = source_runtime.artifacts.put_json({'schema_version': 1, 'contract': 'reviewed_source_facts_v1',
        'authoring_input': source_runtime.snapshot, 'source_projection_digest': 'a' * 64, 'facts': []})
    result = await source_runtime.execute(outline_only=True, reviewed_source_facts_ref=ref)
    spec = source_runtime.rows['execution']['execution_spec']
    assert spec['reviewed_body_entry_contract'] == CONTRACT
    assert source_runtime.observed[0][1]['reviewed_body_entry_contract'] == CONTRACT
    if prior_contract is None:
        spec.pop('reviewed_body_entry_contract')
    assert await source_runtime.execute(outline_only=True, reviewed_source_facts_ref=ref) == result
    assert source_runtime.rows['execution']['execution_spec'].get('reviewed_body_entry_contract') == prior_contract
    assert len(source_runtime.observed) == 1


@pytest.mark.parametrize('contract', [None, CONTRACT, 'unknown-entry-rule'])
async def test_pipeline_forwards_only_the_explicit_supported_entry_rule(tmp_path, monkeypatch, contract):
    artifacts, snapshot, documents, manifest, _, _ = fixture(tmp_path)
    manifest = {**deepcopy(manifest), 'facts': []}
    ref = artifacts.put_json(manifest)
    raw = plan([{**node('solution', 2, 0), 'heading': point(2),
        'body_windows': [window(2, 0, 1)], 'response_kind': 'narrative'}], [item('solution')])
    observed = []

    async def plans(*_args, **_options):
        return [raw], []

    async def details(compiled, *_args, **options):
        observed.append(options.get('reviewed_body_entry_contract'))
        return compiled

    async def no_model(*_args, **_options):
        pytest.fail('Policy forwarding must not send a model request.')

    monkeypatch.setattr(pipeline, '_request_directory_plans', plans)
    monkeypatch.setattr(pipeline, '_request_directory_details', details)
    options = dict(outline_only=True, directory_documents=documents,
        source_carrier_contract='source_carriers_closed_v1', reviewed_source_facts_ref=ref)
    if contract is not None:
        options['reviewed_body_entry_contract'] = contract
    if contract == 'unknown-entry-rule':
        with pytest.raises(ModelConfigurationError):
            await pipeline.generate_document(artifacts, snapshot, RequestBudget(model='synthetic'), no_model, **cast(Any, options))
        assert observed == []
    else:
        await pipeline.generate_document(artifacts, snapshot, RequestBudget(model='synthetic'), no_model, **cast(Any, options))
        assert observed == [contract]


@pytest.mark.parametrize('contract', [None, CONTRACT])
@pytest.mark.parametrize('unrelated_pending', [False, True])
@pytest.mark.parametrize('entry', [False, True])
async def test_failed_classification_preserves_failure_and_archives_only_host_bound_origins(tmp_path, monkeypatch, contract, unrelated_pending, entry):
    from docreader.llm.request import ModelWorkUnitExhausted
    from docreader.tender.directory_compile import apply_requirements
    from test_reviewed_body_entry_resolution import source_case

    artifacts, _, documents, _, _, facts, compiled, _ = source_case(tmp_path, entry=entry)
    apply_requirements(compiled, documents, [
        {**window(2, line), 'scope': 'unassigned'} for line in (0, 1)])
    origins = deepcopy(compiled['_requirement_cases'])
    if unrelated_pending:
        apply_requirements(compiled, documents, [{**window(4, 1), 'scope': 'unassigned'}])
    original = deepcopy(compiled['_requirement_cases'])

    async def local_results(_execute, _budget, task, _payload, _validate):
        if task in ('directory_material', 'directory_classification'):
            raise ModelWorkUnitExhausted(task)
        assert task == 'directory_requirement'
        return {'assignments': []}

    monkeypatch.setattr(pipeline, '_ask', local_results)
    options = dict(artifacts=artifacts, source_carrier_contract='source_carriers_closed_v1', reviewed_source_facts=facts)
    if contract is not None:
        options['reviewed_body_entry_contract'] = contract
    await pipeline._request_directory_details(compiled, documents, RequestBudget(model='synthetic'), object(), **cast(Any, options))
    assert any(i['code'] == 'outline_directory_classification_rejected' for i in compiled['issues'])
    record, = compiled['_reviewed_source_fact_applications']
    pending = [i for i in compiled['issues'] if i['code'] == 'outline_directory_requirement_unassigned']
    bound = bool(contract and entry)
    assert record['state'] == ('body_bound' if bound else 'unresolved')
    assert len(pending) == (0 if bound else 2) + int(unrelated_pending)
    if contract:
        assert record['origin_requirement_cases'] == origins
        assert record['input_disposition']['scope'] == 'unassigned'
    if bound:
        assert record['host_resolution']['kind'] == 'unique_entry'
        if unrelated_pending:
            assert pending[0]['requirement_case_id'] == original[-1]['id']
    assert compiled['_requirement_cases'] == original
