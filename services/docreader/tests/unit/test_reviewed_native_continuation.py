"""One reviewed native body may replace only a complete union of pending origins."""
from copy import deepcopy
from hashlib import sha256
from io import BytesIO
from zipfile import ZipFile
import json

from docx import Document
import pytest

from docreader.tender import reviewed_source_facts as reviewed
from docreader.tender.directory_compile import apply_requirements, outline_from_compiled
from docreader.tender.evidence.references import EvidenceRepository, fragment, selector
from test_reviewed_source_facts import fixture, load, window


def pending(data, *, partial=False):
    artifacts, snapshot, documents, manifest, compiled, _ = data
    assignments = [{**window(3, 0, 1)}] if partial else [{**window(3, 0)}, {**window(3, 1)}]
    apply_requirements(compiled, documents, [{**w, 'scope': 'unassigned'} for w in assignments])
    return compiled


def test_multiple_complete_pending_cases_are_shadowed_by_exact_source_coverage_union(tmp_path):
    data = fixture(tmp_path)
    compiled = pending(data)
    facts = load(data)
    before = deepcopy(compiled)
    shadow, origins = reviewed.reviewed_classification_view(compiled, facts)
    assert len(origins[facts[2]['fact_id']]) == 2
    assert not any(i['code'] == 'outline_directory_requirement_unassigned' for i in shadow['issues'])
    assert compiled == before


@pytest.mark.parametrize('mode', ['partial', 'already_resolved', 'missing_piece'])
def test_partial_nonpending_or_incomplete_union_is_not_retired(tmp_path, mode):
    data = fixture(tmp_path)
    compiled = pending(data, partial=mode == 'partial')
    facts = list(load(data))
    if mode == 'partial':
        facts[2] = {**facts[2], 'source_windows': [window(3, 0)]}
    elif mode == 'already_resolved':
        compiled['issues'] = [i for i in compiled['issues'] if i['code'] != 'outline_directory_requirement_unassigned']
    else:
        compiled['_requirement_cases'].pop()
    _, origins = reviewed.reviewed_classification_view(compiled, facts)
    assert origins[facts[2]['fact_id']] == []


@pytest.mark.parametrize('scope', ['chapter', 'unassigned'])
def test_canonical_disposition_retires_both_complete_origins_only_when_bound(tmp_path, scope):
    data = fixture(tmp_path)
    artifacts, snapshot, documents, _, compiled, _ = data
    pending(data)
    facts = load(data)
    owner = next(c['id'] for c in compiled['chapters'] if c['response_kind'] == 'narrative')
    original = deepcopy(compiled['_requirement_cases'])
    reviewed.apply_reviewed_classification(compiled, documents, {'items': [], 'requirement_adjudications': [],
        'reviewed_body_dispositions': [{'fact_id': facts[2]['fact_id'], 'scope': scope,
                                      'chapter_id': owner if scope == 'chapter' else None}]}, facts)
    issues = [i for i in compiled['issues'] if i['code'] == 'outline_directory_requirement_unassigned']
    assert len(issues) == (0 if scope == 'chapter' else 2)
    record = next(r for r in compiled['_reviewed_source_fact_applications'] if r['fact_id'] == facts[2]['fact_id'])
    assert record['origin_requirement_cases'] == original
    assert compiled['_requirement_cases'] == original


def test_implicit_or_explicit_default_join_preserves_existing_host_fact_ids(tmp_path):
    data = fixture(tmp_path)
    ids = [f['fact_id'] for f in load(data)]
    assert ids == [
        'reviewed-fact-3773a4c1a246c3360a0c6c4bc004b9199c17518eeb683d199b0e4d2ac80a705b',
        'reviewed-fact-065151256df6056b4f0b4abbbc47a19facbf6d4858ba03fe95b77d517cb86914',
        'reviewed-fact-72f912f1064daa1b2f2a717b928376c009b4542c3200da6344f2bdf207f81285']
    data[3]['facts'][2]['source_join'] = 'paragraphs'
    assert [f['fact_id'] for f in load(data)] == ids


def test_native_continuation_is_identical_in_purpose_word_and_submission_guide(tmp_path):
    from docreader.tender.authoring_export import export_authoring_document
    artifacts, snapshot, documents, manifest, compiled, _ = data = fixture(tmp_path)
    ref = manifest['facts'][2]['source_excerpts'][0]['source_ref']
    index = EvidenceRepository(artifacts).source_index(ref['source_index_ref'])
    root = next(r for r in index.ranges if r['unit_id'] == ref['unit_id'])
    fact = manifest['facts'][2]
    fact['source_excerpts'] = [
        {'source_ref': ref, 'selectors': [selector(fragment(root, 0, 2))]},
        {'source_ref': ref, 'selectors': [selector(fragment(root, 2, root['end']))]},
    ]
    fact['source_join'] = 'continuation'
    facts = load(data)
    assert facts[2]['_source_text'] == root['text']
    owner = next(c['id'] for c in compiled['chapters'] if c['response_kind'] == 'narrative')
    reviewed.apply_reviewed_body_dispositions(compiled, documents, [{'fact_id': facts[2]['fact_id'],
        'scope': 'chapter', 'chapter_id': owner}], facts)
    outline = outline_from_compiled(snapshot, compiled)
    reviewed.bind_reviewed_outline_facts(outline, compiled, facts)
    target = next(t for c in outline['chapters'] for t in c['targets'] if t['purpose'] == root['text'])
    assert target['source_excerpts'] == facts[2]['source_excerpts']
    from docreader.tender.authoring_structure import _structure_payload
    from docreader.tender.authoring_view import compact_structure_payload
    full, _ = _structure_payload(artifacts, outline, [], outline_phase=True)
    def views(entries):
        for chapter in entries:
            yield from chapter.get('targets', [])
            yield from views(chapter.get('children', []))
    for payload in (full, compact_structure_payload(full)):
        selected = [t for t in views(payload['chapters']) if t['purpose'] == root['text']]
        assert len(selected) == 1 and len(selected[0]['source_support_ids']) == 2
    exported = export_authoring_document(artifacts, outline, [])
    text = '\n'.join(p.text for p in Document(BytesIO(artifacts.read_bytes(exported['docx_ref']))).paragraphs)
    assert text.count(root['text']) == 1
    with ZipFile(BytesIO(artifacts.read_bytes(exported['package_ref']))) as package:
        manifest = json.loads(package.read('manifest.json'))
        guide = package.read(manifest['submission_notes']['package_path']).decode()
        assert root['text'] in guide and '应制\n定保密' not in guide


def test_continuation_cannot_reverse_native_fragments(tmp_path):
    artifacts, _, _, manifest, _, _ = data = fixture(tmp_path)
    ref = manifest['facts'][2]['source_excerpts'][0]['source_ref']
    index = EvidenceRepository(artifacts).source_index(ref['source_index_ref'])
    root = next(r for r in index.ranges if r['unit_id'] == ref['unit_id'])
    fact = manifest['facts'][2]
    fact.update(source_join='continuation', source_excerpts=[
        {'source_ref': ref, 'selectors': [selector(fragment(root, 2, root['end']))]},
        {'source_ref': ref, 'selectors': [selector(fragment(root, 0, 2))]}],
        source_text_sha256=sha256((root['text'][2:] + root['text'][:2]).encode()).hexdigest())
    with pytest.raises(ValueError):
        load(data)
