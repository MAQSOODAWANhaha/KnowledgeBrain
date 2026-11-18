from copy import deepcopy
import hashlib

import pytest

from docreader.tender.authoring_excerpts import source_excerpts_text
from docreader.tender.evidence.references import SourceIndex
from docreader.tender.native_body_scope import NativeBodyScope
from docreader.tender.native_body_selection import CONTRACT, NativeBodySelectionView
from test_authoring_templates import create_source
from test_native_body_scope import case, digest


def prose_case(tmp_path, purpose='第一职责。', native=None, *, copies=1, grid_duplicate=False, physical_priority=False):
    values = list(case(tmp_path / 'previous'))
    native = purpose if native is None else native
    units = [{'text': native} for _ in range(copies)]
    if grid_duplicate:
        units.append({'grid': {'row_count': 1, 'column_count': 1, 'cells': [
            {'row': 0, 'column': 0, 'row_span': 1, 'col_span': 1, 'text': purpose}]}})
    artifacts, ref, index = create_source(tmp_path / 'prose', units)
    raw = deepcopy(index.raw)
    for unit, identity in zip(raw['units'], raw['source_contract']['units']):
        identity['physical_locator'] = {**identity['physical_locator'], 'page_ordinal': 0}
        unit['locator']['page_ordinal'] = 99 if physical_priority else 0
    index = SourceIndex(index.document, raw)
    ref = artifacts.put_json(index.descriptor(artifacts.put_json(raw)))
    snapshot = {**values[1], 'frozen_ref': artifacts.put_json({'documents': [
        {**index.document, 'source_index_ref': ref, 'role': 'main'}]})}
    # Layout may collapse native whitespace, while retaining every nonblank glyph.
    page = native if purpose in native else purpose
    values[0], values[1] = artifacts, snapshot
    values[2] = [{'document_id': 'document', 'role': 'main', 'pages': [page],
                  'line_units': [[None for _ in page.splitlines()]]}]
    target = values[4]['chapters'][0]['targets'][0]
    target['purpose'] = purpose
    values[4]['authoring_input'] = snapshot
    origin = values[3]['_source_renderings'][0]
    origin['rows'] = [{'document_id': 'document', 'page': 0, 'line': i, 'text': text}
                      for i, text in enumerate(page.splitlines())]
    if purpose != page:
        assert page.startswith(purpose)
        origin['rows'][0].update(render_start=0, render_end=len(purpose), rendered=True)
    origin['rows_sha256'] = digest(origin['rows'])
    origin['purpose_sha256'] = hashlib.sha256(purpose.encode()).hexdigest()
    return values, ref, index


def test_full_native_prose_range_is_exact_without_model_selection(tmp_path):
    values, _, _ = prose_case(tmp_path)
    scope = NativeBodyScope(*values[:5])
    assert scope.records[0]['classification'] == 'native_prose_exact'
    assert not scope.selection_view().units and not scope.pending
    binding = scope.exact_bindings['chapter-1:target-1']
    assert source_excerpts_text(values[0], [binding['parts'][0]['source_excerpt']]) == '第一职责。'


def test_substring_opens_full_native_prose_range_including_unseen_tail(tmp_path):
    values, _, _ = prose_case(tmp_path, native='第一职责。第二职责尾句。')
    scope = NativeBodyScope(*values[:5])
    assert scope.records[0]['classification'] == 'native_resolution'
    assert not scope.exact_bindings and not scope.pending
    view = scope.selection_view()
    assert CONTRACT == 'native_body_contiguous_selection_v4'
    source = view.cells[0]
    assert source['source_kind'] == 'native_prose' and source['text'] == '第一职责。第二职责尾句。'
    assert not {'table_id', 'row', 'column'} & source.keys()
    uid, island = view.units[0]['unit_id'], view.units[0]['islands'][0]['island_id']
    accepted = view.accept({'native_choices': [{'unit_id': uid, 'islands': [
        {'island_id': island, 'state': 'selected', 'body': [[source['cell_id'], 1, 2, 0]], 'context': []}]}]})
    piece = accepted['units'][0]['islands'][0]['body'][0]
    assert piece['text'] == '第一职责。第二职责尾句。'
    excerpt = scope.cell_excerpt(piece['cell_id'], piece['start'], piece['end'])
    assert source_excerpts_text(values[0], [excerpt]) == piece['text']


def test_whitespace_equivalence_only_creates_candidate_not_exact_binding(tmp_path):
    values, _, _ = prose_case(tmp_path, native='第一 职责。')
    scope = NativeBodyScope(*values[:5])
    assert not scope.exact_bindings
    assert scope.selection_view().cells[0]['text'] == '第一 职责。'
    assert values[4]['chapters'][0]['targets'][0]['purpose'] == '第一职责。'


@pytest.mark.parametrize('kind', ['native_duplicate', 'layout_duplicate', 'grid_duplicate', 'no_match'])
def test_ambiguous_or_unmatched_prose_stays_source_pending(tmp_path, kind):
    values, _, _ = prose_case(tmp_path, copies=2 if kind == 'native_duplicate' else 1,
                             grid_duplicate=kind == 'grid_duplicate')
    if kind == 'layout_duplicate': values[2][0]['pages'][0] += '\n第一职责。'
    if kind == 'no_match':
        values[2][0]['pages'][0] = '另一职责。'
        target = values[4]['chapters'][0]['targets'][0]
        target['purpose'] = '另一职责。'
        origin = values[3]['_source_renderings'][0]
        origin['rows'][0]['text'] = target['purpose']
        origin['rows_sha256'] = digest(origin['rows'])
        origin['purpose_sha256'] = hashlib.sha256(target['purpose'].encode()).hexdigest()
    scope = NativeBodyScope(*values[:5])
    assert scope.records[0]['classification'] == 'layout_pending'
    assert scope.pending and not scope.selection_view().units and not scope.exact_bindings


def test_prose_physical_locator_takes_precedence_over_document_locator(tmp_path):
    values, _, _ = prose_case(tmp_path, physical_priority=True)
    assert NativeBodyScope(*values[:5]).records[0]['classification'] == 'native_prose_exact'


def test_prose_context_only_contains_explicitly_selected_source_ranges(tmp_path):
    values, _, _ = prose_case(tmp_path, native='第一职责。第二职责尾句。')
    scope = NativeBodyScope(*values[:5])
    view = scope.selection_view()
    source = view.cells[0]
    subset = view.subset([view.units[0]['unit_id']])
    assert subset.cells == view.cells
    raw = {'native_choices': [{'unit_id': 1, 'islands': [{'island_id': 1, 'state': 'selected',
            'body': [[source['cell_id'], 1, 1, 0]], 'context': []}]}]}
    result = view.accept(raw)
    assert result['units'][0]['context'] == []
    assert source['text'] == '第一职责。第二职责尾句。'


def test_same_unit_cannot_mix_grid_and_prose_sources():
    cells = [{'cell_id': 1, 'document_id': 'd', 'table_id': 1, 'row': 0, 'column': 0, 'text': '原格。'},
             {'cell_id': 2, 'document_id': 'd', 'source_kind': 'native_prose', 'native_unit_id': 'section',
              'native_range_id': 'range', 'page': 0, 'text': '原生段落。'}]
    with pytest.raises(ValueError):
        NativeBodySelectionView(cells, [{'unit_id': 1, 'islands': [{'island_id': 1, 'cell_ids': [1, 2]}]}])


def test_approved_footer_on_other_page_does_not_defeat_full_range_identity(tmp_path):
    values, _, index = prose_case(tmp_path)
    values[2][0]['pages'].append('71')
    values[2][0]['line_units'].append([None])
    origin = values[3]['_source_renderings'][0]
    origin['rows'].append({'document_id': 'document', 'page': 1, 'line': 0, 'text': '71'})
    origin['rows_sha256'] = digest(origin['rows'])
    origin['purpose_sha256'] = hashlib.sha256('第一职责。\n71'.encode()).hexdigest()
    app = {'kind': 'source_decoration', 'state': 'presentation_excluded', 'fact_id': 'footer',
           'affected_target_ids': ['chapter-1:target-1'], 'original_ref': index.document['original_ref'],
           'original_fragments': [{'page': 1, 'line': 0, 'start': 0, 'end': 2, 'line_text': '71',
                                  'text_sha256': hashlib.sha256(b'71').hexdigest()}]}
    result = NativeBodyScope(*values[:5], decoration_applications=[app])
    assert result.records[0]['classification'] == 'native_prose_exact'
    assert len(result.records[0]['source_islands']) == 2 and not result.pending


def test_prose_domain_keeps_business_but_excludes_a_proven_decoration_suffix(tmp_path):
    values, _, index = prose_case(tmp_path, purpose='第一职责。', native='第一职责。71')
    origin = values[3]['_source_renderings'][0]
    origin['rows'][0].pop('render_start')
    origin['rows'][0].pop('render_end')
    origin['rows'][0].pop('rendered')
    origin['rows_sha256'] = digest(origin['rows'])
    origin['purpose_sha256'] = hashlib.sha256('第一职责。71'.encode()).hexdigest()
    app = {'kind': 'source_decoration', 'state': 'presentation_excluded', 'fact_id': 'footer',
           'affected_target_ids': ['chapter-1:target-1'], 'original_ref': index.document['original_ref'],
           'original_fragments': [{'page': 0, 'line': 0, 'start': 5, 'end': 7, 'line_text': '第一职责。71',
                                  'text_sha256': hashlib.sha256(b'71').hexdigest()}]}
    result = NativeBodyScope(*values[:5], decoration_applications=[app])
    assert result.records[0]['classification'] == 'native_resolution' and not result.pending
    view = result.selection_view()
    assert view.cells[0]['text'] == '第一职责。'
    assert result.cell_excerpt(view.cells[0]['cell_id'], 0, 5)['selectors'][0]['end'] == 5
    assert index.ranges[0]['text'] == '第一职责。71'


def test_this_contract_does_not_order_multiple_prose_ranges_in_one_unit():
    cells = [{'cell_id': i, 'document_id': 'd', 'source_kind': 'native_prose', 'native_unit_id': f'section-{i}',
              'native_range_id': f'range-{i}', 'page': 0, 'text': f'原生段落{i}。'} for i in (1, 2)]
    with pytest.raises(ValueError):
        NativeBodySelectionView(cells, [{'unit_id': 1, 'islands': [{'island_id': 1, 'cell_ids': [1, 2]}]}])


def test_multiple_effective_source_pages_remain_pending(tmp_path):
    values, _, _ = prose_case(tmp_path, purpose='甲职责\n乙职责')
    values[2][0].update(pages=['甲职责', '乙职责'], line_units=[[None], [None]])
    origin = values[3]['_source_renderings'][0]
    origin['rows'] = [{'document_id': 'document', 'page': i, 'line': 0, 'text': text}
                      for i, text in enumerate(('甲职责', '乙职责'))]
    origin['rows_sha256'] = digest(origin['rows'])
    result = NativeBodyScope(*values[:5])
    assert result.records[0]['classification'] == 'layout_pending'
    assert len(result.records[0]['source_islands']) == 2 and result.pending
