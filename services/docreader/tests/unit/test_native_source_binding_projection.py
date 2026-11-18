"""First native decisions remain authoritative through outline and Word."""
from copy import deepcopy
from io import BytesIO

from docx import Document
import pytest

from docreader.integrations.artifacts import ArtifactIntegrityError
from docreader.tender.authoring_export import export_authoring_document
from docreader.tender.directory_compile import compile_directory, outline_from_compiled
from docreader.tender.source_body import load_source_body
from docreader.tender.schema import SourceExcerptRef
from test_directory_compile import node, item, plan
from test_native_requirement_sources import synthetic, reply, native_source


def source_outline(tmp_path):
    ledger, artifacts, _, frozen = synthetic(tmp_path)
    accepted = ledger.view().accept(reply(ledger))
    source = native_source(accepted)
    documents = [{'document_id': frozen['document_id'], 'role': 'main',
                  'pages': ['Technical', 'Continuation']}]
    compiled = compile_directory(plan([
        node('technical', 0, 0, document_id=frozen['document_id'])], [item('technical')]), documents)
    compiled['_native_requirements'] = [{'source': source, 'scope': 'chapter',
                                        'chapter_ids': [compiled['chapters'][0]['id']]}]
    snapshot = {'schema_version': 2, 'run_id': 'first-native-choice',
                'source_version': 1, 'domain_revision': 1,
                'frozen_ref': artifacts.put_json({'documents': [frozen]})}
    outline = outline_from_compiled(snapshot, compiled, artifacts=artifacts)
    return artifacts, outline, outline['chapters'][0]['targets'][-1], source


def test_native_outline_binds_selected_body_and_context_without_second_model_choice(tmp_path):
    artifacts, outline, target, source = source_outline(tmp_path)
    assert target['purpose'] == source['body']['text'] == 'Alpha  Beta  0.5 / 5'
    assert target['source_body_ref']
    assert target['source_context_excerpts'] == [SourceExcerptRef.model_validate(e).model_dump()
        for g in source['contexts'] for e in g['source_excerpts']]
    assert load_source_body(artifacts, outline['authoring_input'], target) == source['body']['text']
    assert target['manual_fill'] is True


def test_native_word_preserves_body_context_roles_and_each_context_join(tmp_path):
    artifacts, outline, _, source = source_outline(tmp_path)
    result = export_authoring_document(artifacts, outline, [])
    word = Document(BytesIO(artifacts.read_bytes(result['docx_ref'])))
    paragraphs = [p.text.replace('\u00a0', ' ') for p in word.paragraphs]
    assert paragraphs.count(source['body']['text']) == 1
    assert '原文上下文（非企业应答）：' in paragraphs
    assert '\n'.join(g['text'] for g in source['contexts']) in paragraphs
    assert '5.00\nname tag\nAlpha  Beta  0.5 / 5' not in paragraphs


@pytest.mark.parametrize('corruption', ['body', 'context'])
def test_native_word_rejects_target_changes_after_the_first_source_binding(tmp_path, corruption):
    artifacts, outline, target, _ = source_outline(tmp_path)
    if corruption == 'body':
        target['purpose'] += ' changed source text'
    else:
        target['source_context_excerpts'] = []
    with pytest.raises(ArtifactIntegrityError):
        export_authoring_document(artifacts, outline, [])


def test_native_binding_rejects_stale_accepted_source_cache(tmp_path):
    from docreader.tender.native_requirement_flow import bind_native_targets
    ledger, artifacts, _, frozen = synthetic(tmp_path)
    source = deepcopy(native_source(ledger.view().accept(reply(ledger))))
    source['body']['text'] = 'Not the selected original'
    compiled = {'_native_requirements': [{'source': source, 'scope': 'chapter', 'chapter_ids': ['source']} ]}
    snapshot = {'schema_version': 2, 'run_id': 'first-native-choice', 'source_version': 1,
                'domain_revision': 1, 'frozen_ref': artifacts.put_json({'documents': [frozen]})}
    with pytest.raises(ArtifactIntegrityError):
        bind_native_targets(compiled, {'source': {'chapter_id': 'chapter-1', 'targets': []}},
                            artifacts=artifacts, snapshot=snapshot)


def test_native_binding_reuses_source_index_and_frozen_cache_across_targets(tmp_path):
    from collections import Counter
    from docreader.tender.native_requirement_flow import bind_native_targets

    ledger, artifacts, _, frozen = synthetic(tmp_path)
    source = native_source(ledger.view().accept(reply(ledger)))
    frozen_ref = artifacts.put_json({'documents': [frozen]})
    snapshot = {'schema_version': 2, 'run_id': 'first-native-choice', 'source_version': 1,
                'domain_revision': 1, 'frozen_ref': frozen_ref}
    compiled = {'_native_requirements': [
        {'source': deepcopy(source), 'scope': 'chapter', 'chapter_ids': ['c1', 'c2']}
        for _ in range(4)
    ]}
    nodes = {'c1': {'chapter_id': 'chapter-1', 'targets': []},
             'c2': {'chapter_id': 'chapter-2', 'targets': []}}
    reads = Counter()
    original_read_json = artifacts.read_json

    def counting_read_json(ref):
        reads[ref['sha256']] += 1
        return original_read_json(ref)

    artifacts.read_json = counting_read_json
    issues = bind_native_targets(compiled, nodes, artifacts=artifacts, snapshot=snapshot)
    assert len(issues) == 8
    assert len(nodes['c1']['targets']) == 4 and len(nodes['c2']['targets']) == 4
    assert reads[frozen_ref['sha256']] == 1
    assert reads[frozen['source_index_ref']['sha256']] == 1


@pytest.mark.asyncio
async def test_outline_compilation_runs_off_event_loop_thread_for_heartbeat(tmp_path, monkeypatch):
    import asyncio
    import time
    from docreader.llm.budget import RequestBudget
    from docreader.tender import authoring_pipeline as pipeline

    _, artifacts, _, frozen = synthetic(tmp_path)
    frozen = {**frozen, 'role': 'main'}
    snapshot = {'schema_version': 2, 'run_id': 'heartbeat-outline-compile', 'source_version': 1,
                'domain_revision': 1, 'frozen_ref': artifacts.put_json({'documents': [frozen]})}
    original = pipeline.outline_from_compiled
    ticks = 0
    ticks_during_compile = 0

    def slow_outline_from_compiled(*args, **kwargs):
        nonlocal ticks_during_compile
        start = ticks
        time.sleep(0.06)
        ticks_during_compile = ticks - start
        return original(*args, **kwargs)

    monkeypatch.setattr(pipeline, 'outline_from_compiled', slow_outline_from_compiled)

    async def ticker():
        nonlocal ticks
        while True:
            await asyncio.sleep(0.01)
            ticks += 1

    async def unused_model(*_args, **_kwargs):
        raise AssertionError('unexpected model call')

    documents = [{'document_id': frozen['document_id'], 'role': 'main', 'pages': ['Technical', 'Continuation']}]
    plans = [plan([node('technical', 0, 0, document_id=frozen['document_id'])], [item('technical')])]
    heart = asyncio.create_task(ticker())
    try:
        await pipeline.generate_document(
            artifacts, snapshot, RequestBudget(context_tokens=131072, model='grok/grok-4.7@500k'),
            unused_model, outline_only=True, directory_documents=documents, directory_plans=plans,
        )
    finally:
        heart.cancel()
        await asyncio.gather(heart, return_exceptions=True)
    assert ticks_during_compile >= 2
