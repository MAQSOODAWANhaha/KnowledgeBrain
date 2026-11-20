"""Known mathematical glyphs require the audited embedded font program, not PUA guesses."""

from copy import deepcopy

import pytest

from docreader.models.document import DocumentLocator, PageLocator, StructuredSourceUnit, StructuredSourceUnitKind
from docreader.parser import pdf_symbols
from docreader.tender.directory_compile import _normalize_layout_symbols


FONT_SHA = 'fe2dc455591187a1f55d2476fdb8cf1f2c62d6f5ff2c7867a06465c271ddf188'


def proof(char='\uf03d', glyph='uniF03D', normalized='=', index=0):
    return {'page_ordinal': 0, 'char_index': index, 'raw_symbol': char,
            'normalized_symbol': normalized, 'font_name': 'FAACDE+SymbolMT',
            'glyph_name': glyph, 'font_sha256': FONT_SHA,
            'left': 1, 'bottom': 1, 'right': 9, 'top': 9, 'page_width': 100, 'page_height': 100}


def source(text):
    return StructuredSourceUnit(key='formula', ordinal=0, kind=StructuredSourceUnitKind.SECTION,
                                text=text, locator=DocumentLocator(section_ordinal=0),
                                physical_locator=PageLocator(page_ordinal=0))


@pytest.mark.parametrize('font', ['FAACDE+SymbolMT', 'ABCDEF+SymbolMT'])
@pytest.mark.parametrize('codepoint,glyph,unicode', [
    (0xF03D, 'uniF03D', '='), (0xF0B4, 'uniF0B4', '×'),
])
def test_known_math_font_program_and_actual_glyph_are_both_required(font, codepoint, glyph, unicode):
    assert pdf_symbols.verified_symbol(font, codepoint, [glyph], FONT_SHA) == (glyph, unicode)


@pytest.mark.parametrize('font,codepoint,glyphs,fingerprint', [
    ('SymbolMT', 0xF03D, ['uniF03D'], 'a' * 64),
    ('OtherFont', 0xF03D, ['uniF03D'], FONT_SHA),
    ('SymbolMT', 0xF03D, ['equal'], FONT_SHA),
    ('SymbolMT', 0xF03D, ['uniF03D', 'another'], FONT_SHA),
    ('SymbolMT', 0xF0B4, ['uniF03D'], FONT_SHA),
    ('SymbolMT', 0xF099, ['uniF099'], FONT_SHA),
    ('SymbolMT', 0xF03D, ['uniF03D'], None),
    ('SymbolMT', 0xF03D, ['uniF03D'], ''),
])
def test_generic_private_glyph_names_cannot_define_mathematical_meaning(font, codepoint, glyphs, fingerprint):
    assert pdf_symbols.verified_symbol(font, codepoint, glyphs, fingerprint) is None


def test_registered_math_symbols_restore_native_text_and_layout_without_changing_lines_or_numbers():
    original = '失信分\n失信扣分\uf03d商务分值\uf0b4失信权重\uf0b4\n10'
    evidence = [proof(), proof('\uf0b4', 'uniF0B4', '×', 1), proof('\uf0b4', 'uniF0B4', '×', 2)]
    before = deepcopy(evidence)
    item = source(original)
    receipts = pdf_symbols.normalize_pdf_units([item], evidence)
    layout, issues = _normalize_layout_symbols([original], {'source_contract': {'glyph_normalizations': receipts}})
    expected = '失信分\n失信扣分=商务分值×失信权重×\n10'
    assert item.text == expected and layout == [expected]
    assert not item.source_issues and not issues
    assert receipts == evidence == before
    assert len(item.text) == len(original) and item.text.splitlines()[2] == '10'


@pytest.mark.parametrize('problem', [
    'wrong_hash', 'other_font', 'wrong_glyph', 'wrong_meaning', 'missing_hash', 'missing_glyph',
    'missing_proof', 'ambiguous',
])
def test_math_source_and_layout_remain_private_without_complete_unambiguous_font_proof(problem):
    original = '原式\uf03d1'
    evidence = [proof()]
    if problem == 'wrong_hash':
        evidence[0]['font_sha256'] = 'b' * 64
    elif problem == 'other_font':
        evidence[0]['font_name'] = 'UnrelatedSymbolFont'
    elif problem == 'wrong_glyph':
        evidence[0]['glyph_name'] = 'another'
    elif problem == 'wrong_meaning':
        evidence[0]['normalized_symbol'] = '×'
    elif problem == 'missing_hash':
        evidence[0].pop('font_sha256')
    elif problem == 'missing_glyph':
        evidence[0].pop('glyph_name')
    elif problem == 'missing_proof':
        evidence = []
    else:
        original += '\n另一字体\uf03d2'
        evidence.append({**proof(index=1), 'font_name': 'UnrelatedSymbolFont'})
    before = deepcopy(evidence)
    item = source(original)
    receipts = pdf_symbols.normalize_pdf_units([item], evidence)
    layout, issues = _normalize_layout_symbols([original], {'source_contract': {'glyph_normalizations': evidence}})
    assert item.text == original
    assert item.source_issues == ['unresolved_private_use_symbol_requires_visual_review']
    assert len(issues) == 1 and issues[0]['code'] == 'outline_directory_symbol_unconfirmed'
    assert evidence == before
    if problem == 'ambiguous':
        assert receipts == evidence
        assert layout == ['原式=1\n另一字体\uf03d2']
    else:
        assert layout == [original]
        assert receipts == []


def test_checkbox_old_contract_is_not_extended_by_the_math_font_registry():
    assert pdf_symbols.verified_checkbox('Wingdings2', 0xF052, ['boxcheck']) == ('boxcheck', '☑')
    assert pdf_symbols.verified_symbol('Wingdings2', 0xF052, ['boxcheck'], 'a' * 64) == ('boxcheck', '☑')
    assert pdf_symbols.verified_checkbox('SymbolMT', 0xF03D, ['uniF03D']) is None
