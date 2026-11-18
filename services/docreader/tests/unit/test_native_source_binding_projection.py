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
