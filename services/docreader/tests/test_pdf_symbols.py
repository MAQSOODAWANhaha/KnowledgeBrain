"""Synthetic font/cmap evidence only; no private fonts or document bytes."""
from docreader.models.document import DocumentLocator, PageLocator, StructuredSourceUnit, StructuredSourceUnitKind
from docreader.parser.pdf_symbols import normalize_pdf_units, verified_checkbox


def test_checkbox_mapping_requires_exact_family_code_and_embedded_glyph_name():
    assert verified_checkbox('ABCDEF+Wingdings2,Bold', 0xF052, ['boxcheck']) == ('boxcheck', '☑')
    assert verified_checkbox('Wingdings 2', 0xF0A3, ['box1']) == ('box1', '□')
    assert verified_checkbox('Wingdings', 0xF052, ['boxcheck']) is None
    assert verified_checkbox('SimSun', 0xF052, ['boxcheck']) is None
    assert verified_checkbox('Wingdings2', 0xF052, ['box1']) is None
    assert verified_checkbox('Wingdings2', 0xF052, ['boxcheck', 'unknown']) is None
    assert verified_checkbox('Wingdings2', 0xF099, ['boxcheck']) is None


def unit(text):
    return StructuredSourceUnit(key='section:0', ordinal=0, kind=StructuredSourceUnitKind.SECTION,
                                text=text, locator=DocumentLocator(section_ordinal=0),
                                physical_locator=PageLocator(page_ordinal=0))


def test_verified_normalization_preserves_raw_glyph_receipt_and_unknown_is_partial():
    source = unit('\uf052 是 \uf0a3 否; 未知\ue012')
    evidence = [dict(page_ordinal=0, char_index=1, raw_symbol='\uf052', normalized_symbol='☑', glyph_name='boxcheck',
                     font_name='ABCDEF+Wingdings2', font_sha256='a' * 64, left=1, bottom=1, right=9, top=9, page_width=100, page_height=100),
                dict(page_ordinal=0, char_index=5, raw_symbol='\uf0a3', normalized_symbol='□', glyph_name='box1',
                     font_name='ABCDEF+Wingdings2', font_sha256='a' * 64, left=11, bottom=1, right=19, top=9, page_width=100, page_height=100)]
    receipt = normalize_pdf_units([source], evidence)
    assert source.text == '☑ 是 □ 否; 未知\ue012'
    assert receipt == evidence
    assert source.source_issues == ['unresolved_private_use_symbol_requires_visual_review']


def test_same_page_ambiguous_font_does_not_get_global_pua_replacement():
    source = unit('\uf052 两个不同字体的相同私用码')
    receipt = normalize_pdf_units([source], [dict(page_ordinal=0, char_index=1, raw_symbol='\uf052', normalized_symbol='☑'),
                                             dict(page_ordinal=0, char_index=2, raw_symbol='\uf052')])
    assert receipt == []
    assert source.text.startswith('\uf052')
    assert source.source_issues
