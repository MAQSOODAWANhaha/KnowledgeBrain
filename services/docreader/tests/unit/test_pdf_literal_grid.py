"""Grid geometry is a candidate relationship, never proof that it carries glyphs."""
from copy import deepcopy
from hashlib import sha256

import pytest

from docreader.tender.evidence.native import bind_pdf_literal
from docreader.tender.evidence.packing import source_ranges


def fixture(*, grid_values=('A',), text='AB', outside=False):
    original = {'sha256': sha256(b'original PDF identity').hexdigest()}
    grids = []
    for number, value in enumerate(grid_values):
        locator = {'locator_kind': 'page', 'page_ordinal': 0,
                   'left': 0, 'bottom': 0, 'right': 150, 'top': 100}
        grids.append({'unit_id': f'grid-{number}', 'kind': 'table_region', 'ordinal': number,
            'text': '', 'locator': locator, 'physical_locator': locator,
            'grid': {'row_count': 1, 'column_count': 1, 'cells': [
                {'row': 0, 'column': 0, 'row_span': 1, 'col_span': 1, 'text': value}]}})
    characters = [{'char_index': number, 'text': value,
        'bbox': [120, 10, 130, 20] if outside else [10 + number * 10, 10, 18 + number * 10, 20],
        'flags': ['outside_page'] if outside else []}
        for number, value in enumerate(text)]
    raw = {'original_ref': original, 'units': grids,
        'source_contract': {'units': [{'unit_id': unit['unit_id']} for unit in grids],
            'page_manifest': [{'page_ordinal': 0, 'classification': 'text',
                               'unit_ids': [unit['unit_id'] for unit in grids]}]},
        'metadata': {}, 'tree': None}
    literal = {'contract': 'tender_pdf_literal_v1', 'source_sha256': original['sha256'],
        'page_count': 1, 'pages': [{'page_ordinal': 0, 'width': 100, 'height': 100,
                                   'characters': characters}], 'issues': []}
    return raw, literal, {'document_id': 'main', 'document_version': '1', 'original_ref': original}


def literal_ranges(raw, document):
    return [entry for entry in source_ranges(document, raw) if entry.get('text_basis') == 'pdf_literal']


def test_grid_without_all_source_text_does_not_consume_original_glyphs():
    raw, literal, document = fixture()
    bind_pdf_literal(raw, literal)
    assert [entry['text'] for entry in literal_ranges(raw, document)] == ['AB']
    assert [entry['reason'] for entry in raw['literal_projection_dispositions']] == [
        'native_grid_candidate', 'native_grid_candidate']
    assert raw['units'][0]['grid']['cells'][0]['text'] == 'A'


def test_overlapping_grid_boxes_keep_all_candidates_without_assigning_ownership():
    raw, literal, document = fixture(grid_values=('A', 'B'))
    bind_pdf_literal(raw, literal)
    assert [entry['text'] for entry in literal_ranges(raw, document)] == ['AB']
    assert all(entry['reason'] == 'native_grid_candidate'
               and entry['unit_ids'] == ['grid-0', 'grid-1']
               for entry in raw['literal_projection_dispositions'])


def test_outside_page_flag_does_not_allow_a_grid_to_hide_literal_characters():
    raw, literal, document = fixture(outside=True)
    bind_pdf_literal(raw, literal)
    assert [entry['text'] for entry in literal_ranges(raw, document)] == ['AB']
    assert all(entry['flags'] == ['outside_page'] for entry in raw['literal_units'].values())


def test_blank_original_grid_and_fill_position_are_preserved():
    raw, literal, document = fixture(grid_values=('',))
    before = deepcopy(raw['units'][0]['grid'])
    bind_pdf_literal(raw, literal)
    grid = next(unit for unit in raw['units'] if unit.get('grid'))
    assert grid['grid'] == before
    rows = [entry for entry in source_ranges(document, raw) if entry.get('text_basis') == 'grid_row']
    assert len(rows) == 1 and rows[0]['empty'] is True and rows[0]['text'] == ''
    assert [entry['text'] for entry in literal_ranges(raw, document)] == ['AB']


@pytest.mark.parametrize('newline', ['\n', '\r'])
def test_physical_newline_stays_a_separator_without_dropping_adjacent_source_text(newline):
    raw, literal, document = fixture(text='A' + newline + 'B')
    bind_pdf_literal(raw, literal)
    assert [entry['text'] for entry in literal_ranges(raw, document)] == ['A', 'B']
    indices = [index for entry in raw['literal_units'].values() for index in entry['char_indices']]
    assert indices == [0, 2]
    assert next(entry for entry in raw['literal_projection_dispositions']
                if entry['char_index'] == 1)['reason'] == 'physical_line_separator'
