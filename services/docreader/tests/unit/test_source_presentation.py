"""Reviewed PDF decorations affect only source-bound final presentation."""
from copy import deepcopy
from hashlib import sha256
from io import BytesIO
from zipfile import ZipFile

import pytest
from docx import Document
from pypdf import PdfWriter
from pypdf.generic import DictionaryObject, NameObject, DecodedStreamObject
import pypdfium2 as pdfium

from docreader.integrations.artifacts import ArtifactStore
from docreader.tender import reviewed_source_facts as reviewed
from docreader.tender.directory_compile import documents_from_frozen, compile_directory, outline_from_compiled
from docreader.tender.directory_sources import SourceRefs
from docreader.tender.evidence.references import SourceIndex, fragment, selector, EvidenceRepository
from docreader.tender.schema import AuthoringInput
from test_structural_packing import source
from test_directory_compile import node, item, plan


def pdf_bytes(lines):
    writer = PdfWriter()
    page = writer.add_blank_page(width=600, height=840)
    font = DictionaryObject({NameObject('/Type'): NameObject('/Font'),
        NameObject('/Subtype'): NameObject('/Type1'), NameObject('/BaseFont'): NameObject('/Helvetica')})
    page[NameObject('/Resources')] = DictionaryObject({NameObject('/Font'):
        DictionaryObject({NameObject('/F1'): writer._add_object(font)})})
    stream = DecodedStreamObject()
    stream.set_data(('\n'.join(f'BT /F1 12 Tf 70 {780-ordinal*100} Td ({text}) Tj ET'
        for ordinal, text in enumerate(lines))).encode())
    page[NameObject('/Contents')] = writer._add_object(stream)
    output = BytesIO()
    writer.write(output)
    return output.getvalue()


def fixture(tmp_path):
    lines = ['TENDER HEADER', 'Original form', 'Signature: ______', 'Date: ______', '80']
    artifacts = ArtifactStore(tmp_path)
    original = artifacts.put_bytes(pdf_bytes(lines), 'application/pdf')
    native = source([{'text': '\n'.join(lines)}])
    index = SourceIndex({**native.document, 'original_ref': original}, native.raw)
    index_ref = artifacts.put_json(index.descriptor(artifacts.put_json(index.raw)))
    frozen = artifacts.put_json({'documents': [{**index.document, 'source_index_ref': index_ref, 'role': 'main'}]})
    snapshot = AuthoringInput.model_validate({'schema_version': 2, 'run_id': 'presentation',
        'source_version': 1, 'domain_revision': 1, 'frozen_ref': frozen}).model_dump()
    documents = documents_from_frozen(artifacts, artifacts.read_json(frozen))
    with pdfium.PdfDocument(artifacts.read_bytes(original)) as pdf:
        page = pdf[0]
        textpage = page.get_textpage()
        chars = textpage.get_text_range()
        textpage.close()
        page.close()

    def fact(line=0):
        text = lines[line]
        start = chars.index(text)
        return {'kind': 'source_decoration', 'label': 'Reviewed source header', 'document_id': 'document',
            'original_ref': original, 'role': 'page_header', 'enterprise_confirmed': False,
            'review_evidence_ref': artifacts.put_json({'review': 'exact original header, preserve form fields'}),
            'layout_fragments': [{'page': 0, 'line': line, 'start': 0, 'end': len(text),
                'text_sha256': sha256(text.encode()).hexdigest(), 'original_chars': {
                    'start': start, 'end': start + len(text), 'text_sha256': sha256(text.encode()).hexdigest()}}]}

    manifest = {'schema_version': 1, 'contract': 'reviewed_source_facts_v1', 'authoring_input': snapshot,
        'source_projection_digest': SourceRefs(documents).digest, 'facts': [fact()]}
    raw = plan([{**node('form', 0, 1), 'heading': {'document_id': 'document', 'page': 0, 'line': 1},
        'body_windows': [{'document_id': 'document', 'page': 0, 'start_line': 0, 'end_line': 4}]}], [item('form')])
    return artifacts, snapshot, documents, manifest, compile_directory(raw, documents), fact


def load(data):
    artifacts, snapshot, documents, manifest, *_ = data
    return reviewed.load_reviewed_facts(artifacts, snapshot, documents, artifacts.put_json(manifest))


def test_original_glyph_decoration_needs_no_fabricated_native_excerpt(tmp_path):
    data = fixture(tmp_path)
    facts = load(data)
    assert facts[0]['kind'] == 'source_decoration' and 'source_excerpts' not in facts[0]
    assert facts[0]['fact_id'].startswith('reviewed-fact-')


@pytest.mark.parametrize('bad', ['wrong_original', 'wrong_line_sha', 'glyph_sha', 'glyph_position', 'bounds', 'signature', 'date', 'duplicate'])
def test_unverified_or_protected_decorations_are_rejected(tmp_path, bad):
    data = fixture(tmp_path)
    artifacts, _, _, manifest, _, fact = data
    f = manifest['facts'][0]
    fragment = f['layout_fragments'][0]
    if bad == 'wrong_original':
        f['original_ref'] = artifacts.put_bytes(b'other original', 'application/pdf')
    elif bad == 'wrong_line_sha':
        fragment['text_sha256'] = '0' * 64
    elif bad == 'glyph_sha':
        fragment['original_chars']['text_sha256'] = '0' * 64
    elif bad == 'glyph_position':
        fragment['original_chars']['start'] += 1
    elif bad == 'bounds':
        fragment['end'] += 999
    elif bad in {'signature', 'date'}:
        manifest['facts'] = [fact(2 if bad == 'signature' else 3)]
    else:
        manifest['facts'].append({**deepcopy(f), 'label': 'same source, new name'})
    with pytest.raises(ValueError):
        load(data)


def test_final_projection_changes_only_bound_header_and_preserves_word_signature_and_quantity(tmp_path):
    from docreader.tender.authoring_export import export_authoring_document
    artifacts, snapshot, documents, manifest, compiled, _ = data = fixture(tmp_path / 'source')
    source_before = deepcopy(documents)
    facts = load(data)
    outline = outline_from_compiled(snapshot, compiled)
    original = deepcopy(outline)
    reviewed.bind_reviewed_outline_facts(outline, compiled, facts)
    target = outline['chapters'][0]['targets'][0]
    assert 'TENDER HEADER' not in target['purpose']
    assert 'Signature: ______' in target['purpose'] and 'Date: ______' in target['purpose']
    assert target['purpose'].splitlines()[-1] == '80'
    assert documents == source_before and 'TENDER HEADER' in original['chapters'][0]['targets'][0]['purpose']
    record, = compiled['_reviewed_source_fact_applications']
    assert record['state'] == 'presentation_excluded' and record['target_ids'] == [target['target_id']]
    assert record['original_fragments'][0]['text'] == 'TENDER HEADER'
    assert not any(key.startswith('_source_') for key in target)
    exported = export_authoring_document(artifacts, outline, [])
    body = '\n'.join(p.text for p in Document(BytesIO(artifacts.read_bytes(exported['docx_ref']))).paragraphs)
    assert 'TENDER HEADER' not in body and 'Signature: ______' in body and '80' in body
    with ZipFile(BytesIO(artifacts.read_bytes(exported['package_ref']))) as package:
        assert package.read('投标文件.docx') == artifacts.read_bytes(exported['docx_ref'])


def test_missing_or_changed_source_mapping_preserves_original_and_marks_pending(tmp_path):
    for mutation in ('missing', 'changed', 'empty_rows', 'entry', 'condition'):
        artifacts, snapshot, documents, manifest, compiled, _ = data = fixture(tmp_path / mutation)
        facts = load(data)
        outline = outline_from_compiled(snapshot, compiled)
        if mutation == 'missing':
            compiled.pop('_source_renderings', None)
        elif mutation == 'empty_rows':
            compiled['_source_renderings'][0]['rows'] = []
        elif mutation == 'entry':
            compiled['chapters'][0]['entry'] = {'document_id': 'document', 'page': 0, 'line': 0}
        elif mutation == 'condition':
            compiled['chapters'][0]['condition_windows'] = [
                {'document_id': 'document', 'page': 0, 'start_line': 0, 'end_line': 0}]
        else:
            outline['chapters'][0]['targets'][0]['purpose'] += '\nManually preserved text'
        before = deepcopy(outline['chapters'])
        reviewed.bind_reviewed_outline_facts(outline, compiled, facts)
        assert outline['chapters'] == before
        assert compiled['_reviewed_source_fact_applications'][0]['state'] == 'unresolved'


def test_a_pure_decoration_target_is_removed_without_renumbering_other_targets(tmp_path):
    artifacts, snapshot, documents, manifest, compiled, _ = data = fixture(tmp_path)
    facts = load(data)
    compiled['chapters'][0]['text'] = 'TENDER HEADER'
    compiled['chapters'][0]['_source_body_rows'] = [r for r in compiled['chapters'][0]['_source_body_rows'] if r['line'] == 0]
    outline = outline_from_compiled(snapshot, compiled)
    target = outline['chapters'][0]['targets'][0]
    preserved = {**deepcopy(target), 'target_id': 'chapter-1:target-7', 'purpose': '80'}
    outline['chapters'][0]['targets'].append(preserved)
    compiled['_source_renderings'].append({'target_id': preserved['target_id'], 'chapter_id': 'chapter-1',
        'purpose_sha256': sha256(b'80').hexdigest(), 'rows': [{'document_id': 'document', 'page': 0, 'line': 4, 'text': '80'}]})
    reviewed.bind_reviewed_outline_facts(outline, compiled, facts)
    assert outline['chapters'][0]['targets'] == [preserved]
    record = compiled['_reviewed_source_fact_applications'][0]
    assert record['state'] == 'presentation_excluded'
    assert record['removed_target_ids'] == ['chapter-1:target-1']


def test_known_independent_body_cannot_be_reclassified_as_decoration(tmp_path):
    artifacts, snapshot, documents, manifest, _, _ = data = fixture(tmp_path)
    frozen = artifacts.read_json(snapshot['frozen_ref'])
    ref = frozen['documents'][0]['source_index_ref']
    index = EvidenceRepository(artifacts).source_index(ref)
    excerpt = {'source_ref': {'source_index_ref': ref, 'unit_id': 'unit:0'},
        'selectors': [selector(fragment(index.ranges[0], 0, len('TENDER HEADER')))]}
    manifest['facts'].append({'kind': 'independent_body_response', 'label': 'Known substantive body',
        'source_excerpts': [excerpt], 'source_text_sha256': sha256(b'TENDER HEADER').hexdigest(),
        'review_evidence_ref': artifacts.put_json({'review': 'independent body, not decoration'}),
        'enterprise_confirmed': False, 'source_windows': [{'document_id': 'document', 'page': 0, 'start_line': 0, 'end_line': 0}],
        'subjects': [], 'entry': None})
    with pytest.raises(ValueError):
        load(data)


def test_decorations_are_read_under_the_shared_pdfium_lock(tmp_path, monkeypatch):
    from docreader.parser import pdf_parser
    data = fixture(tmp_path)
    original = pdfium.PdfDocument
    observed = []
    def guarded(*args, **kwargs):
        observed.append(pdf_parser._PDFIUM_LOCK.locked())
        return original(*args, **kwargs)
    monkeypatch.setattr(pdfium, 'PdfDocument', guarded)
    load(data)
    assert observed and all(observed)


def test_native_grid_owned_text_cannot_be_excluded_even_when_its_glyphs_match(tmp_path):
    from docreader.tender.source_presentation import load_decoration
    from docreader.tender.directory_compile import _Source
    artifacts, snapshot, documents, manifest, *_ = fixture(tmp_path)
    documents[0]['line_units'][0][0] = 'native-protected-grid'
    with pytest.raises(ValueError, match='native grid'):
        load_decoration(artifacts, _Source(documents), artifacts.read_json(snapshot['frozen_ref']), manifest['facts'][0], {})


def test_pure_decoration_pending_case_leaves_no_false_response_work_but_keeps_its_source(tmp_path):
    artifacts, snapshot, documents, manifest, compiled, _ = data = fixture(tmp_path)
    case = {'id': 'exact-source-case', 'source_text': 'TENDER HEADER',
        'source_windows': [{'document_id': 'document', 'page': 0, 'start_line': 0, 'end_line': 0}],
        '_source_rows': [{'document_id': 'document', 'page': 0, 'line': 0, 'text': 'TENDER HEADER'}]}
    compiled['_requirement_cases'] = [case]
    compiled['issues'].append({'code': 'outline_directory_requirement_unassigned', 'detail': 'TENDER HEADER',
        'requirement_case_id': case['id']})
    facts = load(data)
    outline = outline_from_compiled(snapshot, compiled)
    reviewed.bind_reviewed_outline_facts(outline, compiled, facts)
    assert not any(i['code'] == 'outline_directory_requirement_unassigned' for i in outline['issues'])
    assert compiled['_requirement_cases'][0]['source_text'] == 'TENDER HEADER'
    record = compiled['_reviewed_source_fact_applications'][0]
    assert record['retired_case_ids'] == ['exact-source-case']
