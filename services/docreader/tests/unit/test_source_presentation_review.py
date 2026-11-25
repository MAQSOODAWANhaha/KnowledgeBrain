"""Source projection must retain protected duties and reject stale row identities."""
from typing import Any, cast

from copy import deepcopy
from hashlib import sha256

import pytest

from docreader.tender import reviewed_source_facts as reviewed
from docreader.tender.directory_compile import apply_requirements, compile_directory, outline_from_compiled
from test_directory_compile import item, node, plan
from test_source_presentation import fixture, load


@pytest.mark.parametrize('guard', ['condition', 'clause'])
def test_protected_pending_source_duty_cannot_be_retired_as_decoration(tmp_path, guard):
    _, snapshot, documents, _, _, _ = data = fixture(tmp_path)
    window = {'document_id': 'document', 'page': 0, 'start_line': 0, 'end_line': 0}
    adoption = item('form', **cast(Any, {guard: window if guard == 'condition' else [window]}))
    raw = plan([{**node('form', 0, 1, document_id='document'), 'body_windows': [
        {'document_id': 'document', 'page': 0, 'start_line': 1, 'end_line': 4}]}], [adoption])
    compiled = compile_directory(raw, documents)
    apply_requirements(compiled, documents, [{**window, 'scope': 'unassigned'}])
    outline = outline_from_compiled(snapshot, compiled)
    before = deepcopy(outline['chapters'])
    reviewed.bind_reviewed_outline_facts(outline, compiled, load(data))
    assert outline['chapters'] == before
    assert any(i['code'] == 'outline_directory_requirement_unassigned' for i in outline['issues'])
    record = compiled['_reviewed_source_fact_applications'][0]
    assert record['state'] == 'unresolved' and record['retired_case_ids'] == []


@pytest.mark.parametrize('stale', ['coordinate', 'chapter'])
def test_stale_private_source_coordinates_or_chapter_keep_the_entire_target(tmp_path, stale):
    _, snapshot, _, _, compiled, _ = data = fixture(tmp_path)
    facts = load(data)
    outline = outline_from_compiled(snapshot, compiled)
    origin = compiled['_source_renderings'][0]
    if stale == 'coordinate':
        origin['rows'][1]['line'] = 0
    else:
        origin['chapter_id'] = 'wrong-owner'
    before = deepcopy(outline['chapters'])
    reviewed.bind_reviewed_outline_facts(outline, compiled, facts)
    assert outline['chapters'] == before
    record = compiled['_reviewed_source_fact_applications'][0]
    assert record['state'] == 'unresolved'
    assert record['affected_target_ids'] == [] and record['removed_target_ids'] == []


def test_empty_context_approvals_preserve_the_preexisting_fact_id(tmp_path):
    data = fixture(tmp_path)
    original_id = load(data)[0]['fact_id']
    data[3]['facts'][0]['context_approvals'] = []
    assert load(data)[0]['fact_id'] == original_id


def test_equal_digits_on_the_same_page_do_not_conflict_when_their_source_ranges_are_distinct(tmp_path):
    from docreader.integrations.artifacts import ArtifactStore
    from docreader.tender.authoring_excerpts import whole_source_excerpts
    from docreader.tender.directory_compile import documents_from_frozen
    from docreader.tender.directory_sources import SourceRefs
    from docreader.tender.evidence.references import SourceIndex
    from docreader.tender.schema import AuthoringInput
    from test_source_presentation import pdf_bytes
    from test_structural_packing import source

    artifacts = ArtifactStore(tmp_path)
    lines = ['80', 'Capacity 80', 'Original form']
    original = artifacts.put_bytes(pdf_bytes(lines), 'application/pdf')
    native = source([{'text': text} for text in lines])
    raw = deepcopy(native.raw)
    for ordinal, (unit, identity) in enumerate(zip(raw['units'], raw['source_contract']['units'])):
        unit['locator'].update(page_ordinal=0, left=70, bottom=775-ordinal*100, right=160, top=795-ordinal*100)
        identity['physical_locator'] = deepcopy(unit['locator'])
    index = SourceIndex({**native.document, 'original_ref': original}, raw)
    index_ref = artifacts.put_json(index.descriptor(artifacts.put_json(index.raw)))
    frozen = artifacts.put_json({'documents': [{**index.document, 'source_index_ref': index_ref, 'role': 'main'}]})
    snapshot = AuthoringInput.model_validate({'schema_version': 2, 'run_id': 'distinct-source',
        'source_version': 1, 'domain_revision': 1, 'frozen_ref': frozen}).model_dump()
    documents = documents_from_frozen(artifacts, artifacts.read_json(frozen))
    evidence = artifacts.put_json({'review': 'distinct original glyph and body ranges'})
    header = {'kind': 'source_decoration', 'label': 'Header digits', 'document_id': 'document',
        'original_ref': original, 'role': 'page_header', 'review_evidence_ref': evidence,
        'enterprise_confirmed': False, 'layout_fragments': [{'page': 0, 'line': 0, 'start': 0, 'end': 2,
            'text_sha256': sha256(b'80').hexdigest(), 'original_chars': {
                'start': 0, 'end': 2, 'text_sha256': sha256(b'80').hexdigest()}}]}
    body = {'kind': 'independent_body_response', 'label': 'Actual capacity',
        'source_excerpts': whole_source_excerpts(artifacts, {'source_index_ref': index_ref, 'unit_id': 'unit:1'}),
        'source_text_sha256': sha256(b'Capacity 80').hexdigest(), 'review_evidence_ref': evidence,
        'enterprise_confirmed': False, 'subjects': [], 'entry': None,
        'source_windows': [{'document_id': 'document', 'page': 0, 'start_line': 1, 'end_line': 1}]}
    manifest = {'schema_version': 1, 'contract': 'reviewed_source_facts_v1', 'authoring_input': snapshot,
        'source_projection_digest': SourceRefs(documents).digest, 'facts': [header, body]}
    assert len(reviewed.load_reviewed_facts(artifacts, snapshot, documents, artifacts.put_json(manifest))) == 2


@pytest.mark.parametrize('bad', [None, 'owner', 'kind', 'window', 'window_sha', 'evidence', 'heading', 'entry'])
def test_explicit_context_approval_is_exact_and_cannot_override_other_protections(tmp_path, bad):
    artifacts, snapshot, documents, manifest, compiled, _ = data = fixture(tmp_path)
    window = {'document_id': 'document', 'page': 0, 'start_line': 0, 'end_line': 0}
    compiled['chapters'][0]['clause_windows'] = [window]
    approval = {'owner_heading': deepcopy(compiled['chapters'][0]['heading']), 'context_kind': 'clause',
        'context_window': window, 'context_raw_rows_sha256': sha256(b'TENDER HEADER').hexdigest(),
        'review_evidence_ref': artifacts.put_json({'review': 'exact clause header only; preserve all remaining source'}),
        'enterprise_confirmed': False}
    if bad == 'owner':
        approval['owner_heading'] = {'document_id': 'document', 'page': 0, 'line': 4}
    elif bad == 'kind':
        approval['context_kind'] = 'condition'
    elif bad == 'window':
        approval['context_window'] = {**window, 'end_line': 1}
        approval['context_raw_rows_sha256'] = sha256(b'TENDER HEADER\nOriginal form').hexdigest()
    elif bad == 'window_sha':
        approval['context_raw_rows_sha256'] = '0' * 64
    elif bad == 'evidence':
        approval['review_evidence_ref'] = {**approval['review_evidence_ref'], 'sha256': '0' * 64}
    elif bad in {'heading', 'entry'}:
        compiled['chapters'][0][bad] = {'document_id': 'document', 'page': 0, 'line': 0}
        approval['owner_heading'] = deepcopy(compiled['chapters'][0]['heading'])
    manifest['facts'][0]['context_approvals'] = [approval]
    if bad in {'window_sha', 'evidence'}:
        with pytest.raises(ValueError):
            load(data)
        return
    outline = outline_from_compiled(snapshot, compiled)
    before = deepcopy(outline['chapters'])
    reviewed.bind_reviewed_outline_facts(outline, compiled, load(data))
    record = compiled['_reviewed_source_fact_applications'][0]
    if bad is None:
        assert 'TENDER HEADER' not in outline['chapters'][0]['targets'][0]['purpose']
        assert 'Signature: ______' in outline['chapters'][0]['targets'][0]['purpose']
        assert record['state'] == 'presentation_excluded'
        assert compiled['chapters'][0]['clause_windows'] == [window]
    else:
        assert outline['chapters'] == before and record['state'] == 'unresolved'
