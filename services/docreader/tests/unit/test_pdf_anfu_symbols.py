"""A cmap-less font needs the audited PDF encoding as well as its font program."""

from copy import deepcopy

import pytest

from docreader.models.document import (
    PageTableLocator, PdfTableCell, StructuredSourceUnit, StructuredSourceUnitKind, TableGrid,
)
from docreader.parser import pdf_symbols
from docreader.tender.directory_compile import _normalize_layout_symbols
from test_pdf_bullet_symbols import source as section_source


FONT_NAME = 'CSKSUK+Wingdings-Regular'
FONT_SHA = '9baff418df77a3d6013f8f3214f85e7a2c20de4dbae9a41768f820f0e6da3a53'
ENCODING_SHA = '2d004858e0ab7f5c9d37ccf7c60e588d514465e675afc5ea0669b7b543cdd09f'
SOURCE_SHA = 'a9860eec35a514a760c23a721e650304b4e2ee1f2a0baa9e80e8ae9e81c7afde'
SYMBOLS = [(0xF06C, 122, 'glyph00122', '●'), (0xF0D8, 190, 'glyph00190', '➢')]


def binding(codepoint=0xF06C):
    _, glyph_index, glyph_name, _ = next(entry for entry in SYMBOLS if entry[0] == codepoint)
    return {'source_document_sha256': SOURCE_SHA, 'font_sha256': FONT_SHA,
            'encoding': 'Identity-H', 'cid_to_gid_map': 'Identity', 'to_unicode_sha256': ENCODING_SHA,
            'source_code': glyph_index, 'glyph_index': glyph_index,
            'unicode_codepoint': codepoint, 'glyph_name': glyph_name}


def proof(codepoint=0xF06C, index=0):
    _, _, glyph_name, unicode = next(entry for entry in SYMBOLS if entry[0] == codepoint)
    return {'page_ordinal': 0, 'char_index': index, 'raw_symbol': chr(codepoint), 'normalized_symbol': unicode,
            'font_name': FONT_NAME, 'glyph_name': glyph_name, 'font_sha256': FONT_SHA,
            'left': 1, 'bottom': 1, 'right': 9, 'top': 9, 'page_width': 100, 'page_height': 100,
            'pdf_glyph_binding': binding(codepoint)}


@pytest.mark.parametrize('codepoint,index,glyph,unicode', SYMBOLS)
def test_cmapless_font_and_pua_alone_never_define_a_symbol(codepoint, index, glyph, unicode):
    assert pdf_symbols.verified_symbol(FONT_NAME, codepoint, [glyph], FONT_SHA) is None


@pytest.mark.parametrize('font', [FONT_NAME, 'ABCDEF+Wingdings-Regular'])
@pytest.mark.parametrize('codepoint,index,glyph,unicode', SYMBOLS)
def test_exact_pdf_encoding_and_actual_cid_gid_prove_the_selected_symbol(font, codepoint, index, glyph, unicode):
    assert pdf_symbols.verified_symbol(font, codepoint, [glyph], FONT_SHA,
                                       glyph_binding=binding(codepoint)) == (glyph, unicode)


@pytest.mark.parametrize('problem', [
    'font', 'font_hash', 'inner_font_hash', 'glyph', 'ambiguous_glyph', 'encoding',
    'cid_map', 'encoding_hash', 'missing_encoding_hash', 'source_code', 'glyph_index',
    'unicode', 'missing_binding',
])
def test_registration_rejects_wrong_font_glyph_or_pdf_encoding_provenance(problem):
    selected, font, fingerprint, glyphs = binding(), FONT_NAME, FONT_SHA, ['glyph00122']
    if problem == 'font':
        font = 'OtherFont'
    elif problem == 'font_hash':
        fingerprint = 'b' * 64
    elif problem == 'inner_font_hash':
        selected['font_sha256'] = 'b' * 64
    elif problem == 'glyph':
        glyphs = ['uniF06C']
    elif problem == 'ambiguous_glyph':
        glyphs.append('glyph00190')
    elif problem == 'encoding':
        selected['encoding'] = 'Identity-V'
    elif problem == 'cid_map':
        selected['cid_to_gid_map'] = 'Custom'
    elif problem == 'encoding_hash':
        selected['to_unicode_sha256'] = 'b' * 64
    elif problem == 'missing_encoding_hash':
        selected.pop('to_unicode_sha256')
    elif problem == 'source_code':
        selected['source_code'] = 190
    elif problem == 'glyph_index':
        selected['glyph_index'] = 190
    elif problem == 'unicode':
        selected['unicode_codepoint'] = 0xF0D8
    else:
        selected = None
    assert pdf_symbols.verified_symbol(font, 0xF06C, glyphs, fingerprint, glyph_binding=selected) is None


def test_source_list_and_heading_path_use_the_two_distinct_audited_symbols():
    original = '\uf0d8前附表说明\n\n\uf06c技术服务'
    item = section_source(original)
    item.locator.heading_path = '\uf0d8前附表说明'
    evidence = [proof(0xF0D8), proof(0xF06C, 9)]
    before = deepcopy(evidence)
    accepted = pdf_symbols.normalize_pdf_units([item], evidence)
    layout, issues = _normalize_layout_symbols([original], {'source_contract': {'glyph_normalizations': accepted}})
    assert item.text == layout[0] == '➢前附表说明\n\n●技术服务'
    assert item.locator.heading_path == '➢前附表说明'
    assert not item.source_issues and not issues
    assert accepted == evidence == before
    assert len(item.text) == len(original) and len(item.text.splitlines()) == len(original.splitlines())


def test_table_cell_symbol_projection_preserves_blank_cells_merges_and_coordinates():
    grid = TableGrid(row_count=2, column_count=2, widths_mm=[30, 70], cells=[
        PdfTableCell(row=0, column=0, row_span=1, col_span=2, text='\uf06c资格要求'),
        PdfTableCell(row=1, column=0, row_span=1, col_span=1, text=''),
        PdfTableCell(row=1, column=1, row_span=1, col_span=1, text='填写________')])
    item = StructuredSourceUnit(key='table', ordinal=0, kind=StructuredSourceUnitKind.TABLE_REGION,
        locator=PageTableLocator(page_ordinal=0, table_ordinal=0, left=0, top=100, right=100, bottom=0), grid=grid)
    before = item.model_dump()
    accepted = pdf_symbols.normalize_pdf_units([item], [proof()])
    expected = deepcopy(before)
    expected['grid']['cells'][0]['text'] = '●资格要求'
    assert item.model_dump() == expected and not item.source_issues
    assert len(accepted) == 1 and accepted[0]['pdf_glyph_binding'] == binding()


@pytest.mark.parametrize('problem', ['missing_binding', 'encoding_hash', 'mixed_meaning', 'occurrence_count'])
def test_layout_never_restores_a_bullet_without_complete_matching_pdf_proof(problem):
    original, evidence = '\uf06c服务要求', [proof()]
    if problem == 'missing_binding':
        evidence[0].pop('pdf_glyph_binding')
    elif problem == 'encoding_hash':
        evidence[0]['pdf_glyph_binding']['to_unicode_sha256'] = 'b' * 64
    else:
        original += '\n\uf06c另一要求'
        if problem == 'mixed_meaning':
            evidence.append({**proof(index=6), 'normalized_symbol': '○'})
    before = deepcopy(evidence)
    layout, issues = _normalize_layout_symbols([original], {'source_contract': {'glyph_normalizations': evidence}})
    assert layout == [original] and len(issues) == 1
    assert issues[0]['code'] == 'outline_directory_symbol_unconfirmed'
    assert evidence == before
