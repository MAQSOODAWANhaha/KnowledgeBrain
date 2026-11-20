from copy import deepcopy
import hashlib

import pytest

from docreader.parser.pdf_parser import _sectionize_pages
from docreader.parser.source_contract import parser_version
from docreader.tender.authoring_excerpts import source_excerpts_text
from docreader.tender.evidence.references import SourceIndex
from docreader.tender.native_body_scope import NativeBodyScope
from test_native_body_global_decorations import fixture, _pdf
from test_native_body_scope import digest


def source_case(tmp_path, *, invalid=None, whole_suffix=False):
    native = '## HEADER\nBusiness sentence.'
    values, index, _ = fixture(tmp_path, native=native, wanted='Business sentence.',
                               layout='HEADER\nBusiness sentence.', decoration='HEADER', pdf=True)
    values[3]['_reviewed_source_fact_applications'] = []
    promoted, pieces = _sectionize_pages([native])
    assert pieces[0][0][2] == native
    raw = deepcopy(index.raw)
    raw['source_contract'].update(schema_version=2, parser_version=parser_version())
    raw['units'][0]['locator']['heading_path'] = 'HEADER'
    identity = raw['source_contract']['units'][0]
    identity.update(kind='section', heading_path='HEADER', text_sha256=hashlib.sha256(native.encode()).hexdigest())
    raw['tree'] = {'title': '', 'keys': [], 'children': [
        {'title': 'HEADER', 'heading_path': 'HEADER', 'keys': ['unit:0'], 'children': []}]}
    original = index.document['original_ref']
    if invalid == 'version':
        raw['source_contract']['parser_version'] = 'docreader-source-v2/' + '0' * 64
    if invalid == 'unit_heading':
        raw['units'][0]['locator']['heading_path'] = 'OTHER'
    if invalid == 'tree_heading':
        raw['tree']['children'][0]['title'] = 'OTHER'
    if invalid == 'text_sha':
        identity['text_sha256'] = '0' * 64
    if invalid == 'no_pdf':
        original = values[0].put_bytes(b'no original PDF proof')
    if invalid == 'literal_hash':
        original = values[0].put_bytes(_pdf('# HEADER'))
    if invalid == 'noninitial_hash':
        raw['units'][0]['text'] = 'HEADER\n## Business sentence.'
        identity['text_sha256'] = hashlib.sha256(raw['units'][0]['text'].encode()).hexdigest()
    if invalid == 'hash_after_prefix':
        raw['units'][0]['text'] = '## HEADER\nBusiness sentence. # later'
        identity['text_sha256'] = hashlib.sha256(raw['units'][0]['text'].encode()).hexdigest()
    index = SourceIndex({**index.document, 'original_ref': original}, raw)
    ref = values[0].put_json(index.descriptor(values[0].put_json(raw)))
    snapshot = {**values[1], 'frozen_ref': values[0].put_json({'documents': [
        {**index.document, 'source_index_ref': ref, 'role': 'main'}]})}
    values[1], values[4]['authoring_input'] = snapshot, snapshot
    if whole_suffix:
        target = values[4]['chapters'][0]['targets'][0]
        target['purpose'] = 'HEADER\nBusiness sentence.'
        origin = values[3]['_source_renderings'][0]
        origin['rows'] = [{'document_id': 'document', 'page': 0, 'line': i, 'text': text}
                          for i, text in enumerate(values[2][0]['pages'][0].splitlines())]
        origin['rows_sha256'] = digest(origin['rows'])
        origin['purpose_sha256'] = hashlib.sha256(target['purpose'].encode()).hexdigest()
    return values, index


def test_producer_generated_atx_exits_the_range_but_original_heading_stays(tmp_path):
    values, index = source_case(tmp_path)
    scope = NativeBodyScope(*values[:5])
    view = scope.selection_view()
    assert scope.records[0]['classification'] == 'native_resolution'
    cell = view.cells[0]
    assert cell['text'] == 'HEADER\nBusiness sentence.'
    excerpt = scope.cell_excerpt(cell['cell_id'], 0, len(cell['text']))
    assert excerpt['selectors'][0]['start'] == 3 and excerpt['selectors'][0]['text_boundaries']['start'] is False
    assert source_excerpts_text(values[0], [excerpt]) == cell['text']
    syntax = scope.records[0]['native_prose_generated_syntax'][0]
    assert (syntax['native_start'], syntax['native_end']) == (0, 3)
    assert syntax['kind'] == 'pdf_atx_heading_prefix' and 'fact_id' not in syntax
    assert index.ranges[0]['text'] == '## HEADER\nBusiness sentence.'


def test_same_bytes_after_syntax_projection_do_not_claim_original_whole_range_boundary(tmp_path):
    values, _ = source_case(tmp_path, whole_suffix=True)
    scope = NativeBodyScope(*values[:5])
    assert scope.records[0]['classification'] == 'native_resolution'
    assert not scope.exact_bindings


@pytest.mark.parametrize('invalid', ['literal_hash', 'noninitial_hash', 'hash_after_prefix', 'version',
                                     'unit_heading', 'tree_heading', 'text_sha', 'no_pdf'])
def test_real_hash_or_unproven_producer_structure_is_never_stripped(tmp_path, invalid):
    values, index = source_case(tmp_path, invalid=invalid)
    before = deepcopy(index.raw)
    scope = NativeBodyScope(*values[:5])
    assert scope.records[0]['classification'].endswith('pending') and scope.pending
    assert not scope.selection_view().units and not scope.exact_bindings
    assert index.raw == before and '#' in index.ranges[0]['text']
