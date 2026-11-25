"""A visually audited filled-circle bullet requires its exact embedded program."""

from copy import deepcopy
import ctypes
import hashlib
from io import BytesIO

from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen
import pytest

from docreader.models.document import DocumentLocator, PageLocator, StructuredSourceUnit, StructuredSourceUnitKind
from docreader.parser import pdf_symbols
from docreader.tender.directory_compile import _normalize_layout_symbols


FONT_SHA = 'fa49cf8fbc5f5f9589030eaeae9dd205f750baf282eafe44135486311b2bc268'
FONT_NAME = 'FAADDB+Wingdings-Regular'
GLYPH = 'uniF06C'
BULLET = '●'


def proof(index=0):
    return {'page_ordinal': 0, 'char_index': index, 'raw_symbol': '\uf06c', 'normalized_symbol': BULLET,
            'font_name': FONT_NAME, 'glyph_name': GLYPH, 'font_sha256': FONT_SHA,
            'left': 1, 'bottom': 1, 'right': 9, 'top': 9, 'page_width': 100, 'page_height': 100}


def source(text):
    return StructuredSourceUnit(key='list', ordinal=0, kind=StructuredSourceUnitKind.SECTION,
                                text=text, locator=DocumentLocator(section_ordinal=0),
                                physical_locator=PageLocator(page_ordinal=0))


@pytest.mark.parametrize('font', [FONT_NAME, 'ABCDEF+Wingdings-Regular', 'Wingdings-Regular'])
def test_audited_bullet_requires_the_exact_program_and_actual_glyph(font):
    assert pdf_symbols.verified_symbol(font, 0xF06C, [GLYPH], FONT_SHA) == (GLYPH, BULLET)


@pytest.mark.parametrize('font,codepoint,glyphs,fingerprint', [
    (FONT_NAME, 0xF06C, [GLYPH], 'a' * 64),
    ('OtherSymbolFont', 0xF06C, [GLYPH], FONT_SHA),
    ('Wingdings2', 0xF06C, [GLYPH], FONT_SHA),
    (FONT_NAME, 0xF06C, ['circle'], FONT_SHA),
    (FONT_NAME, 0xF06C, [GLYPH, 'another'], FONT_SHA),
    (FONT_NAME, 0xF0B7, [GLYPH], FONT_SHA),
    (FONT_NAME, 0xF06C, [GLYPH], None),
    (FONT_NAME, 0xF06C, [GLYPH], ''),
])
def test_private_bullet_is_not_defined_by_code_family_or_generic_glyph_name_alone(font, codepoint, glyphs, fingerprint):
    assert pdf_symbols.verified_symbol(font, codepoint, glyphs, fingerprint) is None


def test_verified_bullet_keeps_list_lines_text_blanks_and_source_proof():
    original = '技术服务\n\uf06c第一项\n\n\uf06c第二项'
    evidence = [proof(4), proof(10)]
    before = deepcopy(evidence)
    item = source(original)
    accepted = pdf_symbols.normalize_pdf_units([item], evidence)
    layout, issues = _normalize_layout_symbols([original], {'source_contract': {'glyph_normalizations': accepted}})
    expected = '技术服务\n●第一项\n\n●第二项'
    assert item.text == expected and layout == [expected]
    assert not item.source_issues and not issues
    assert accepted == evidence == before
    assert len(item.text) == len(original) and len(item.text.splitlines()) == len(original.splitlines())


@pytest.mark.parametrize('problem', [
    'wrong_hash', 'other_font', 'wrong_glyph', 'wrong_meaning', 'missing_hash',
    'missing_glyph', 'missing_proof', 'mixed_font',
])
def test_bullet_source_and_layout_retain_unknown_or_conflicting_private_symbols(problem):
    original = '技术服务\n\uf06c第一项'
    evidence = [proof(4)]
    if problem == 'wrong_hash':
        evidence[0]['font_sha256'] = 'b' * 64
    elif problem == 'other_font':
        evidence[0]['font_name'] = 'OtherSymbolFont'
    elif problem == 'wrong_glyph':
        evidence[0]['glyph_name'] = 'other'
    elif problem == 'wrong_meaning':
        evidence[0]['normalized_symbol'] = '○'
    elif problem == 'missing_hash':
        evidence[0].pop('font_sha256')
    elif problem == 'missing_glyph':
        evidence[0].pop('glyph_name')
    elif problem == 'missing_proof':
        evidence = []
    else:
        original += '\n\uf06c另一字体'
        evidence.append({**proof(10), 'font_name': 'OtherSymbolFont'})
    before = deepcopy(evidence)
    item = source(original)
    accepted = pdf_symbols.normalize_pdf_units([item], evidence)
    layout, issues = _normalize_layout_symbols([original], {'source_contract': {'glyph_normalizations': evidence}})
    assert item.text == original
    assert item.source_issues == ['unresolved_private_use_symbol_requires_visual_review']
    assert len(issues) == 1 and issues[0]['code'] == 'outline_directory_symbol_unconfirmed'
    assert evidence == before
    if problem == 'mixed_font':
        # One verified glyph stays in the receipt and is substituted. The other font does not.
        assert accepted == evidence
        assert layout == ['技术服务\n●第一项\n\uf06c另一字体']
    else:
        assert accepted == []
        assert layout == [original]


def test_layout_requires_a_proof_for_every_occurrence_of_the_bullet():
    original = '\uf06c第一项\n\uf06c第二项'
    layout, issues = _normalize_layout_symbols([original], {'source_contract': {'glyph_normalizations': [proof()]}})
    assert layout == [original]
    assert len(issues) == 1 and issues[0]['code'] == 'outline_directory_symbol_unconfirmed'


def test_collector_records_the_audited_bullet_font_and_physical_geometry(monkeypatch):
    class TextPage:
        def count_chars(self):
            return 2

        def get_charbox(self, index):
            return 1, 2, 8, 9

        def close(self):
            self.closed = True

    textpage = TextPage()

    class Page:
        def get_textpage(self):
            return textpage

        def get_size(self):
            return 100, 200

    class Raw:
        @staticmethod
        def FPDFText_GetUnicode(_page, index):
            return ord('\uf06c甲'[index])

    monkeypatch.setattr(pdf_symbols, '_embedded_font', lambda *_: (FONT_NAME, FONT_SHA, {0xF06C: {GLYPH}}))
    actual = pdf_symbols.collect_page_symbols(Page(), 7, Raw, {})
    assert actual == [{'page_ordinal': 7, 'char_index': 0, 'raw_symbol': '\uf06c',
                       'normalized_symbol': BULLET, 'glyph_name': GLYPH, 'font_name': FONT_NAME,
                       'font_sha256': FONT_SHA, 'left': 1, 'bottom': 2, 'right': 8, 'top': 9,
                       'page_width': 100, 'page_height': 200}]
    assert textpage.closed


def test_embedded_font_reader_collects_f06c_from_a_synthetic_cmap():
    builder = FontBuilder(1000, isTTF=True)
    names = ['.notdef', GLYPH]
    builder.setupGlyphOrder(names)
    builder.setupCharacterMap({0xF06C: GLYPH})
    builder.setupGlyf({name: TTGlyphPen(None).glyph() for name in names})
    builder.setupHorizontalMetrics({name: (500, 0) for name in names})
    builder.setupHorizontalHeader(ascent=800, descent=-200)
    builder.setupNameTable({'familyName': 'SyntheticSymbol', 'styleName': 'Regular',
                           'uniqueFontIdentifier': 'synthetic-bullet-cmap', 'fullName': 'SyntheticSymbol Regular',
                           'psName': 'SyntheticSymbol-Regular'})
    builder.setupOS2(sTypoAscender=800, sTypoDescender=-200, usWinAscent=800, usWinDescent=200)
    builder.setupPost()
    builder.setupMaxp()
    output = BytesIO()
    builder.save(output)
    payload = output.getvalue()

    class Raw:
        @staticmethod
        def FPDFText_GetTextObject(*_):
            return ctypes.c_void_p(1)

        @staticmethod
        def FPDFTextObj_GetFont(*_):
            return ctypes.c_void_p(2)

        @staticmethod
        def FPDFFont_GetIsEmbedded(*_):
            return 1

        @staticmethod
        def FPDFFont_GetBaseFontName(_font, destination, _size):
            name = b'SyntheticSymbol\0'
            if destination is not None:
                ctypes.memmove(destination, name, len(name))
            return len(name)

        @staticmethod
        def FPDFFont_GetFontData(_font, destination, _size, length):
            ctypes.cast(length, ctypes.POINTER(ctypes.c_size_t)).contents.value = len(payload)
            if destination is not None:
                ctypes.memmove(destination, payload, len(payload))
            return 1

    found = pdf_symbols._embedded_font(Raw, None, 0, {})
    assert found is not None
    name, fingerprint, cmap = found
    assert name == 'SyntheticSymbol' and fingerprint == hashlib.sha256(payload).hexdigest()
    assert cmap[0xF06C] == {GLYPH}
    assert pdf_symbols.verified_symbol(name, 0xF06C, [GLYPH], fingerprint) is None


def test_bullet_registration_does_not_change_the_checkbox_contract():
    assert pdf_symbols.verified_checkbox(FONT_NAME, 0xF06C, [GLYPH]) is None
    assert pdf_symbols.verified_checkbox('Wingdings2', 0xF052, ['boxcheck']) == ('boxcheck', '☑')
