"""Layout receipts bind the actual fixed-outline document and exported ZIP."""
from typing import Any, cast

from hashlib import sha256

import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.budget import RequestBudget
from docreader.tender import authoring_pipeline, toc_layout
from test_toc_pagination import chapter


@pytest.mark.parametrize('fail_receipt', [False, True])
async def test_layout_receipt_is_saved_before_the_document_result(tmp_path, monkeypatch, fail_receipt):
    artifacts = ArtifactStore(tmp_path / 'artifacts')
    snapshot = {'schema_version': 2, 'run_id': 'layout', 'source_version': 1, 'domain_revision': 1}
    target = {'target_id': 'body:target', 'kind': 'narrative', 'purpose': '原文职责。\n公章：________',
        'rule_refs': [], 'source_refs': [], 'manual_fill': True, 'support_origins': ['tender']}
    outline = {'schema_version': 2, 'authoring_input': snapshot, 'issues': [],
               'chapters': [chapter('body', '资格证明', targets=[target])]}
    events = []

    async def execute(task, request, validate):
        assert task not in {'authoring_write', 'authoring_basis'}
        return validate({'findings': []})

    def render(blob):
        events.append('render')
        return b'%PDF-fixture', 'fixture-engine'

    def finalized(blob, callback):
        callback(blob)
        return blob, {'contract': 'toc_layout_v1', 'status': 'fixed', 'render_count': 1,
                      'docx_sha256': sha256(blob).hexdigest(), 'pdf_sha256': sha256(b'%PDF-fixture').hexdigest()}

    async def receipt(bindings):
        assert {'layout_ref', 'outline_ref', 'docx_ref', 'package_ref',
                'export_input_digest', 'export_result_ref'} == set(bindings)
        report = artifacts.read_json(bindings['layout_ref'])
        assert report['documents'][0]['artifact_ref'] == bindings['docx_ref']
        assert artifacts.read_json(bindings['outline_ref'])['authoring_input']['run_id'] == 'layout'
        assert artifacts.read_bytes(bindings['package_ref'])
        events.append(bindings)
        if fail_receipt:
            raise RuntimeError('layout receipt unavailable')

    monkeypatch.setattr(toc_layout, 'finalize_toc', finalized)
    options = dict(outline=outline, render_pdf=render, on_layout=receipt)
    budget = RequestBudget(model='synthetic', context_tokens=131072, output_reserve=32768)
    if fail_receipt:
        with pytest.raises(RuntimeError, match='layout receipt unavailable'):
            await authoring_pipeline.generate_document(artifacts, snapshot, budget, execute, **cast(Any, options))
    else:
        ref = await authoring_pipeline.generate_document(artifacts, snapshot, budget, execute, **cast(Any, options))
        result = artifacts.read_json(ref)
        assert result['docx_ref'] == events[-1]['docx_ref']
        assert result['package_ref'] == events[-1]['package_ref']
    assert events[0] == 'render' and isinstance(events[-1], dict)


async def test_finished_layout_is_reused_after_receipt_commit_and_executor_loss(tmp_path, monkeypatch):
    artifacts = ArtifactStore(tmp_path / 'artifacts')
    snapshot = {'schema_version': 2, 'run_id': 'layout-recovery', 'source_version': 1, 'domain_revision': 1}
    outline = {'schema_version': 2, 'authoring_input': snapshot, 'issues': [],
               'chapters': [chapter('one', '资格证明')]}
    saved = {}
    renders = []

    async def execute(task, request, validate):
        return validate({'findings': []})

    def render(blob):
        assert not renders, 'A committed verified layout must not be rendered again on resume.'
        renders.append(blob)
        return b'%PDF-fixture', 'fixture-engine'

    def finalized(blob, callback):
        callback(blob)
        return blob, {'contract': 'toc_layout_v1', 'status': 'fixed', 'render_count': 1,
                      'docx_sha256': sha256(blob).hexdigest(), 'pdf_sha256': sha256(b'%PDF-fixture').hexdigest()}

    async def committed_then_lost(bindings):
        saved.update(bindings)
        raise RuntimeError('executor lost after durable layout receipt')

    monkeypatch.setattr(toc_layout, 'finalize_toc', finalized)
    budget = RequestBudget(model='synthetic', context_tokens=131072, output_reserve=32768)
    options = dict(outline=outline, render_pdf=render)
    with pytest.raises(RuntimeError, match='executor lost'):
        await authoring_pipeline.generate_document(artifacts, snapshot, budget, execute,
                                                   on_layout=committed_then_lost, **cast(Any, options))
    reused = {saved['export_input_digest']: saved['export_result_ref']}
    ref = await authoring_pipeline.generate_document(artifacts, snapshot, budget, execute,
        saved_layout_exports=reused, on_layout=committed_then_lost, **cast(Any, options))
    result = artifacts.read_json(ref)
    assert len(renders) == 1
    assert result['docx_ref'] == saved['docx_ref'] and result['package_ref'] == saved['package_ref']
