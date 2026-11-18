from copy import deepcopy
import hashlib
from io import BytesIO

from docx import Document
import pytest

from docreader.tender.authoring_export import export_authoring_document
from docreader.tender.evidence.references import SourceIndex
from docreader.tender.native_body_scope import NativeBodyScope
from docreader.tender.source_body import bind_source_body
from test_native_body_prose import prose_case
from test_native_body_scope import digest


def _pdf(header='HEADER'):
    content = ('BT\n/F1 12 Tf\n72 720 Td\n(' + header + ') Tj\n0 -18 Td\n(Business sentence.) Tj\nET\n').encode()
    objects = [b'<< /Type /Catalog /Pages 2 0 R >>', b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
        b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>',
        b'<< /Length ' + str(len(content)).encode() + b' >>\nstream\n' + content + b'endstream',
        b'<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>']
    data = b'%PDF-1.4\n'
    offsets = [0]
    for number, value in enumerate(objects, 1):
        offsets.append(len(data));data += f'{number} 0 obj\n'.encode() + value + b'\nendobj\n'
    xref = len(data)
    data += b'xref\n0 6\n0000000000 65535 f \n' + b''.join(f'{o:010d} 00000 n \n'.encode() for o in offsets[1:])
    return data + b'trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n' + str(xref).encode() + b'\n%%EOF\n'


def fixture(tmp_path, *, native='第一职责。\n71', wanted='第一职责。', layout=None, decoration='71', pdf=False):
    values, _, index = prose_case(tmp_path, purpose=native, native=native)
    if pdf:
        original = values[0].put_bytes(_pdf())
        index = SourceIndex({**index.document, 'original_ref': original}, index.raw)
        ref = values[0].put_json(index.descriptor(values[0].put_json(index.raw)))
        values[1] = {**values[1], 'frozen_ref': values[0].put_json({'documents': [
            {**index.document, 'source_index_ref': ref, 'role': 'main'}]})}
        values[4]['authoring_input'] = values[1]
    layout = native if layout is None else layout
    lines = layout.splitlines()
    values[2][0].update(pages=[layout], line_units=[[None] * len(lines)])
    target = values[4]['chapters'][0]['targets'][0]
    target['purpose'] = wanted
    line = next(i for i, text in enumerate(lines) if wanted in text)
    start = lines[line].index(wanted)
    origin = values[3]['_source_renderings'][0]
    origin['rows'] = [{'document_id': 'document', 'page': 0, 'line': line, 'text': lines[line],
                       'render_start': start, 'render_end': start + len(wanted), 'rendered': True}]
    origin['rows_sha256'] = digest(origin['rows'])
    origin['purpose_sha256'] = hashlib.sha256(wanted.encode()).hexdigest()
    decorated_line = next(i for i, text in enumerate(lines) if decoration in text)
    low = lines[decorated_line].index(decoration)
    fragment = {'page': 0, 'line': decorated_line, 'start': low, 'end': low + len(decoration),
                'line_text': lines[decorated_line], 'text_sha256': hashlib.sha256(decoration.encode()).hexdigest()}
    if pdf:
        fragment['original_chars'] = {'start': 0, 'end': 6, 'text_sha256': hashlib.sha256(b'HEADER').hexdigest()}
    app = {'kind': 'source_decoration', 'state': 'presentation_excluded', 'fact_id': 'approved-decoration',
           'affected_target_ids': ['different-target'], 'original_ref': index.document['original_ref'],
           'original_fragments': [fragment]}
    values[3]['_reviewed_source_fact_applications'] = [app]
    return values, index, app


@pytest.mark.parametrize('selection', ['first_sentence', 'all_available_parts'])
def test_other_target_footer_cannot_reappear_in_body_or_context_or_word(tmp_path, selection):
    values, index, _ = fixture(tmp_path)
    scope = NativeBodyScope(*values[:5]);view = scope.selection_view()
    assert view.units
    source = view.cells[0]
    last = 1 if selection == 'first_sentence' else len(view.parts[source['cell_id']])
    accepted = view.accept({'native_choices': [{'unit_id': view.units[0]['unit_id'], 'islands': [
        {'island_id': 1, 'state': 'selected', 'body': [[source['cell_id'], 1, last, 0]], 'context': []}]}]})
    unit = accepted['units'][0]
    target = deepcopy(values[4]['chapters'][0]['targets'][0])
    target['source_context_excerpts'] = [scope.cell_excerpt(p['cell_id'], p['start'], p['end']) for p in unit['context']]
    parts = [{'source_excerpt': scope.cell_excerpt(p['cell_id'], p['start'], p['end']), 'join': p['source_join']}
             for island in unit['islands'] for p in island['body']]
    bound = bind_source_body(values[0], values[1], target, parts)
    outline = deepcopy(values[4]);outline['chapters'][0]['targets'][0] = bound
    output = export_authoring_document(values[0], outline, [])
    paragraphs = [p.text for p in Document(BytesIO(values[0].read_bytes(output['docx_ref']))).paragraphs]
    assert '71' not in paragraphs
    assert '71' not in source['text']
    assert index.ranges[0]['text'] == '第一职责。\n71'


def test_global_prefix_is_an_exact_fragment_and_business_blanks_remain(tmp_path):
    values, index, _ = fixture(tmp_path, native='HEADER\n年 月 日\n第一职责。', decoration='HEADER')
    scope = NativeBodyScope(*values[:5]);view = scope.selection_view()
    assert view.cells[0]['text'] == '\n年 月 日\n第一职责。'
    excerpt = scope.cell_excerpt(view.cells[0]['cell_id'], 0, len(view.cells[0]['text']))
    assert excerpt['selectors'][0]['start'] == 6
    protected = scope.records[0]['native_prose_excluded_decorations']
    assert protected[0]['fact_id'] == 'approved-decoration'
    assert (protected[0]['native_start'], protected[0]['native_end']) == (0, 6)
    assert index.ranges[0]['start'] == 0 and index.ranges[0]['text'].startswith('HEADER')


@pytest.mark.parametrize('case', ['internal', 'anchor_overlap', 'unresolved', 'bad_hash'])
def test_unproven_or_internal_global_fact_is_pending_without_clipping_business(tmp_path, case):
    native = '第一职责。71第二职责。' if case == 'internal' else '第一职责。\n71'
    wanted = '71' if case == 'anchor_overlap' else '第一职责。'
    values, index, app = fixture(tmp_path, native=native, wanted=wanted)
    if case == 'unresolved': app['state'] = 'unresolved'
    if case == 'bad_hash': app['original_fragments'][0]['text_sha256'] = '0' * 64
    scope = NativeBodyScope(*values[:5])
    assert scope.records[0]['classification'].endswith('pending') and scope.pending
    assert not scope.exact_bindings and not scope.selection_view().units
    assert index.ranges[0]['text'] == native


def test_pdf_original_character_flow_is_a_unique_equivalent_mapping(tmp_path):
    values, index, _ = fixture(tmp_path, native='HEADER\nBusiness sentence.', wanted='Business sentence.',
                               layout='Business sentence.\nHEADER', decoration='HEADER', pdf=True)
    scope = NativeBodyScope(*values[:5]);view = scope.selection_view()
    assert view.cells[0]['text'] == '\nBusiness sentence.'
    record = scope.records[0]['native_prose_excluded_decorations'][0]
    assert record['mapping'] == 'pdf_original_chars'
    assert (record['native_start'], record['native_end']) == (0, 6)
    assert index.ranges[0]['text'].startswith('HEADER')


def test_pdf_original_character_sha_is_required_for_fallback(tmp_path):
    values, _, app = fixture(tmp_path, native='HEADER\nBusiness sentence.', wanted='Business sentence.',
                             layout='Business sentence.\nHEADER', decoration='HEADER', pdf=True)
    app['original_fragments'][0]['original_chars']['text_sha256'] = '0' * 64
    scope = NativeBodyScope(*values[:5])
    assert scope.records[0]['classification'].endswith('pending') and scope.pending


def test_pdf_bytes_and_page_characters_are_cached_for_shared_ranges(tmp_path, monkeypatch):
    values, index, _ = fixture(tmp_path, native='HEADER\nBusiness sentence.', wanted='Business sentence.',
                               layout='Business sentence.\nHEADER', decoration='HEADER', pdf=True)
    second = deepcopy(values[4]['chapters'][0]['targets'][0]);second['target_id'] = 'chapter-1:target-2'
    values[4]['chapters'][0]['targets'].append(second)
    origin = deepcopy(values[3]['_source_renderings'][0]);origin['target_id'] = second['target_id']
    values[3]['_source_renderings'].append(origin)
    reads = []
    original_read = values[0].read_bytes
    def counted(ref):
        if ref == index.document['original_ref']: reads.append(ref)
        return original_read(ref)
    monkeypatch.setattr(values[0], 'read_bytes', counted)
    scope = NativeBodyScope(*values[:5])
    assert len(scope.selection_view().units) == 2 and len(reads) == 1


def test_disjoint_global_fact_does_not_block_same_page_business(tmp_path):
    values, _, _ = fixture(tmp_path, native='第一职责。', layout='第一职责。\n71')
    scope = NativeBodyScope(*values[:5])
    assert scope.records[0]['classification'] == 'native_prose_exact' and not scope.pending


def test_no_unique_layout_or_original_mapping_preserves_pending(tmp_path):
    values, _, _ = fixture(tmp_path, native='HEADER\nBusiness sentence.', wanted='Business sentence.',
                           layout='Business sentence.\nHEADER', decoration='HEADER')
    scope = NativeBodyScope(*values[:5])
    assert scope.records[0]['classification'] == 'layout_pending' and scope.pending
    assert not scope.selection_view().units
