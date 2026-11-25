"""Word export keeps both continuation grids and does not invent layout."""

from io import BytesIO
from zipfile import ZipFile

from docx import Document
import lxml.etree as etree

from docreader.integrations.artifacts import ArtifactStore
from docreader.tender.authoring_export import export_authoring_document
from docreader.tender.directory_compile import compile_directory, outline_from_compiled
from docreader.tender.evidence.references import SourceIndex
from test_directory_compile import item, node, plan

W = '{http://schemas.openxmlformats.org/wordprocessingml/2006/main}'


def _store(artifacts, units):
    raw = {'schema_version': 2, 'units': units, 'images': {}, 'ocr': {},
           'source_contract': {'page_manifest': [], 'units': [
               {'unit_id': unit['unit_id'], 'section_id': 'section:0', 'parent_section_id': None,
                'heading_path': '投标格式', 'physical_locator': unit['locator']} for unit in units]}}
    document = {'document_id': 'template', 'document_version': '1',
                'original_ref': artifacts.put_bytes(b'synthetic original')}
    index = SourceIndex(document, raw)
    ref = artifacts.put_json(index.descriptor(artifacts.put_json(index.raw)))
    return ref


def _grid(text, widths=None):
    grid = {'row_count': 2, 'column_count': 2, 'cells': [
        {'row': 0, 'column': 0, 'row_span': 1, 'col_span': 1, 'text': '列甲'},
        {'row': 0, 'column': 1, 'row_span': 1, 'col_span': 1, 'text': '列乙'},
        {'row': 1, 'column': 0, 'row_span': 1, 'col_span': 1, 'text': text},
        {'row': 1, 'column': 1, 'row_span': 1, 'col_span': 1, 'text': ''},
    ]}
    if widths is not None:
        grid['widths_mm'] = widths
    return grid


def test_export_writes_both_grids_and_only_the_evidenced_later_header(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    units = [
        {'unit_id': 'table-a', 'kind': 'table_region', 'ordinal': 0, 'text': '',
         'grid': _grid('1', [40, 80]), 'locator': {'locator_kind': 'document', 'section_ordinal': 0}},
        {'unit_id': 'table-b', 'kind': 'table_region', 'ordinal': 1, 'text': '',
         'grid': _grid('3'), 'locator': {'locator_kind': 'document', 'section_ordinal': 0}},
    ]
    ref = _store(artifacts, units)
    from docreader.parser.pdf_tables import continuations_for_extracted_pages
    _extracted, links = continuations_for_extracted_pages([
        {'page': 0, 'height': 800, 'horizontal': [(10, 20, 210), (10, 100, 210), (10, 180, 210)],
         'vertical': [(10, 20, 180), (110, 20, 180), (210, 20, 180)],
         'chars': [{'ch': '列', 'x0': 20, 'x1': 28, 'y0': 150, 'y1': 160},
                   {'ch': '甲', 'x0': 28, 'x1': 36, 'y0': 150, 'y1': 160},
                   {'ch': '1', 'x0': 20, 'x1': 28, 'y0': 40, 'y1': 50}]},
        {'page': 1, 'height': 800, 'horizontal': [(12, 620, 208), (12, 700, 208), (12, 780, 208)],
         'vertical': [(12, 620, 780), (110, 620, 780), (208, 620, 780)],
         'chars': [{'ch': '列', 'x0': 22, 'x1': 30, 'y0': 740, 'y1': 750},
                   {'ch': '甲', 'x0': 30, 'x1': 38, 'y0': 740, 'y1': 750},
                   {'ch': '3', 'x0': 22, 'x1': 30, 'y0': 650, 'y1': 660}]},
    ])
    assert links and links[0]['repeated_header_rows'] >= 1
    link = {**links[0], 'before_table_id': 'table-a', 'after_table_id': 'table-b'}
    documents = [{
        'document_id': 'tender', 'role': 'main',
        'pages': ['报价表\n列甲\t1\n', '列甲\t3\n'],
        'line_units': [[None, 'table-a'], ['table-b']],
        'template_parts': {
            'table-a': {'source_ref': {'source_index_ref': ref, 'unit_id': 'table-a'}, 'selectors': []},
            'table-b': {'source_ref': {'source_index_ref': ref, 'unit_id': 'table-b'}, 'selectors': []},
        },
        'continuations': [link],
    }]
    compiled = compile_directory(plan(
        [node('quote', 0, 0), node('stop', 1, 0)], [item('quote')], source_only=['stop'],
    ), documents)
    quote = compiled['chapters'][0]
    assert [part['source_ref']['unit_id'] for part in quote['template_parts']] == ['table-a', 'table-b']
    assert quote['template_parts'][1].get('header_row_count') == link['repeated_header_rows']
    assert 'header_row_count' not in quote['template_parts'][0]
    snapshot = {'schema_version': 2, 'run_id': 'continuation-export', 'source_version': 1, 'domain_revision': 1}
    outline = outline_from_compiled(snapshot, compiled)
    result = export_authoring_document(artifacts, outline, [])
    package = artifacts.read_bytes(result['docx_ref'])
    word = Document(BytesIO(package))
    assert len(word.tables) == 2
    assert word.tables[0].rows[1].cells[0].text == '1'
    assert word.tables[1].rows[1].cells[0].text == '3'
    xml = etree.fromstring(ZipFile(BytesIO(package)).read('word/document.xml'))
    tables = xml.findall('.//' + W + 'tbl')
    assert tables[0].find('.//' + W + 'tblHeader') is None
    assert tables[1].find('.//' + W + 'tblHeader') is not None
    from docx.shared import Mm
    expected = [Mm(40).twips, Mm(80).twips]
    sourced = [int(item.get(W + 'w')) for item in tables[0].find(W + 'tblGrid').findall(W + 'gridCol')]
    sourced_cells = [int(item.get(W + 'w')) for item in tables[0].findall('.//' + W + 'tcW')]
    assert sourced == expected
    assert sourced_cells[:2] == expected
    plain = tables[1].find(W + 'tblGrid').findall(W + 'gridCol')
    assert len(plain) == 2
    assert all(item.get(W + 'w') is None for item in plain)
    assert tables[1].find('.//' + W + 'tcW') is None
    titles = [paragraph for paragraph in word.paragraphs if paragraph.text == '报价表']
    assert titles and any(paragraph.paragraph_format.keep_with_next for paragraph in titles)
    caches = [text.text for text in xml.findall('.//' + W + 'fldSimple')
              if 'PAGEREF' in (text.get(W + 'instr') or '')
              for text in text.findall('.//' + W + 't')]
    assert caches == ['待计算']
    assert all(not cache.isdigit() for cache in caches)


def test_only_the_selected_later_table_repeats_its_header_in_the_docx(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    units = [
        {'unit_id': 'table-b', 'kind': 'table_region', 'ordinal': 0, 'text': '',
         'grid': _grid('3', [40, 80]), 'locator': {'locator_kind': 'document', 'section_ordinal': 0}},
        {'unit_id': 'table-b', 'kind': 'table_region', 'ordinal': 1, 'text': '',
         'grid': _grid('9'), 'locator': {'locator_kind': 'document', 'section_ordinal': 0}},
    ]
    # Two source indexes share the unit id. Only the tender document is selected.
    tender_units = [units[0]]
    other_units = [{**units[1], 'unit_id': 'table-b'}]
    tender_ref = _store(artifacts, tender_units)
    other_ref = _store(artifacts, other_units)
    documents = [
        {'document_id': 'tender', 'role': 'main', 'pages': ['列甲\t3\n'],
         'line_units': [['table-b']],
         'template_parts': {'table-b': {'source_ref': {'source_index_ref': tender_ref, 'unit_id': 'table-b'}, 'selectors': []}},
         'continuations': [{'kind': 'ruled_table', 'before_page': 0, 'before_table_id': 'table-a',
                            'after_page': 0, 'after_table_id': 'table-b', 'repeated_header_rows': 1}]},
        {'document_id': 'other', 'role': 'attachment', 'pages': ['列甲\t9\n'],
         'line_units': [['table-b']],
         'template_parts': {'table-b': {'source_ref': {'source_index_ref': other_ref, 'unit_id': 'table-b'}, 'selectors': []}},
         'continuations': []},
    ]
    compiled = compile_directory(plan(
        [node('later', 0, 0), node('else', 0, 0, document_id='other')],
        [item('later')], source_only=['else']), documents)
    later = compiled['chapters'][0]
    assert later['template_parts'][0]['header_row_count'] == 1
    assert later['template_parts'][0]['source_ref']['source_index_ref'] == tender_ref
    snapshot = {'schema_version': 2, 'run_id': 'later-header', 'source_version': 1, 'domain_revision': 1}
    outline = outline_from_compiled(snapshot, compiled)
    result = export_authoring_document(artifacts, outline, [])
    xml = etree.fromstring(ZipFile(BytesIO(artifacts.read_bytes(result['docx_ref']))).read('word/document.xml'))
    tables = xml.findall('.//' + W + 'tbl')
    assert len(tables) == 1
    assert tables[0].find('.//' + W + 'tblHeader') is not None
    assert '9' not in ''.join(node.text or '' for node in tables[0].findall('.//' + W + 't'))


def _exported_text(tmp_path, monkeypatch, evidence):
    from docreader.tender import directory_compile as module

    pages = ['投标函\n\uf052已核对\n']
    raw = {'source_contract': {'glyph_normalizations': evidence}, 'units': []}
    monkeypatch.setattr(module, '_layout_pages', lambda *_args, **_kwargs: (list(pages), None))
    reading, _owners, issues, _unread = module._reading_pages(None, raw)
    compiled = compile_directory(plan([node('letter', 0, 0)], [item('letter')]),
                                 [{'document_id': 'tender', 'role': 'main', 'pages': reading,
                                   'body_projection': raw.get('body_projection') or []}])
    snapshot = {'schema_version': 2, 'run_id': 'symbol-export', 'source_version': 1, 'domain_revision': 1}
    outline = outline_from_compiled(snapshot, compiled)
    artifacts = ArtifactStore(tmp_path)
    result = export_authoring_document(artifacts, outline, [])
    word = Document(BytesIO(artifacts.read_bytes(result['docx_ref'])))
    return '\n'.join(paragraph.text for paragraph in word.paragraphs), issues, reading


def _symbol_font(family, glyph_name):
    from fontTools.fontBuilder import FontBuilder
    from fontTools.pens.ttGlyphPen import TTGlyphPen

    builder = FontBuilder(1000, isTTF=True)
    names = ['.notdef', glyph_name]
    builder.setupGlyphOrder(names)
    builder.setupCharacterMap({0xF052: glyph_name})
    pen = TTGlyphPen(None)
    pen.moveTo((100, 0))
    pen.lineTo((500, 0))
    pen.lineTo((500, 700))
    pen.lineTo((100, 700))
    pen.closePath()
    missing = TTGlyphPen(None)
    missing.moveTo((0, 0))
    missing.lineTo((1, 0))
    missing.lineTo((1, 1))
    missing.closePath()
    builder.setupGlyf({'.notdef': missing.glyph(), glyph_name: pen.glyph()})
    builder.setupHorizontalMetrics({name: (600, 0) for name in names})
    builder.setupHorizontalHeader(ascent=800, descent=-200)
    builder.setupNameTable({'familyName': family, 'styleName': 'Regular',
                             'uniqueFontIdentifier': family + glyph_name, 'fullName': family,
                             'psName': family.replace(' ', '')})
    builder.setupOS2(sTypoAscender=800, sTypoDescender=-200, usWinAscent=800, usWinDescent=200)
    builder.setupPost()
    builder.setupMaxp()
    payload = BytesIO()
    builder.save(payload)
    return payload.getvalue()


def _mixed_private_pdf():
    """One embedded Wingdings2 checkbox and one other embedded private glyph."""
    from pypdf import PdfWriter
    from pypdf.generic import (
        ArrayObject, DecodedStreamObject, DictionaryObject, NameObject, NumberObject, create_string_object,
    )

    def cid_font(writer, base, payload):
        descriptor = DictionaryObject({
            NameObject('/Type'): NameObject('/FontDescriptor'),
            NameObject('/FontName'): NameObject('/' + base),
            NameObject('/Flags'): NumberObject(32),
            NameObject('/FontBBox'): ArrayObject([NumberObject(value) for value in (0, -200, 1000, 800)]),
            NameObject('/ItalicAngle'): NumberObject(0),
            NameObject('/Ascent'): NumberObject(800),
            NameObject('/Descent'): NumberObject(-200),
            NameObject('/CapHeight'): NumberObject(700),
            NameObject('/StemV'): NumberObject(80),
        })
        program = DecodedStreamObject()
        program.set_data(payload)
        descriptor[NameObject('/FontFile2')] = writer._add_object(program)
        cid = DictionaryObject({
            NameObject('/Type'): NameObject('/Font'),
            NameObject('/Subtype'): NameObject('/CIDFontType2'),
            NameObject('/BaseFont'): NameObject('/' + base),
            NameObject('/CIDSystemInfo'): DictionaryObject({
                NameObject('/Registry'): create_string_object('Adobe'),
                NameObject('/Ordering'): create_string_object('Identity'),
                NameObject('/Supplement'): NumberObject(0),
            }),
            NameObject('/FontDescriptor'): writer._add_object(descriptor),
            NameObject('/CIDToGIDMap'): NameObject('/Identity'),
            NameObject('/DW'): NumberObject(600),
        })
        unicode_map = DecodedStreamObject()
        unicode_map.set_data(
            b'/CIDInit /ProcSet findresource begin 12 dict begin begincmap '
            b'/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def '
            b'/CMapName /Adobe-Identity-UCS def /CMapType 2 def '
            b'1 begincodespacerange <0000> <FFFF> endcodespacerange '
            b'1 beginbfchar <0001> <F052> endbfchar endcmap '
            b'CMapName currentdict /CMap defineresource pop end end')
        return writer._add_object(DictionaryObject({
            NameObject('/Type'): NameObject('/Font'),
            NameObject('/Subtype'): NameObject('/Type0'),
            NameObject('/BaseFont'): NameObject('/' + base),
            NameObject('/Encoding'): NameObject('/Identity-H'),
            NameObject('/DescendantFonts'): ArrayObject([writer._add_object(cid)]),
            NameObject('/ToUnicode'): writer._add_object(unicode_map),
        }))

    writer = PdfWriter()
    helvetica = writer._add_object(DictionaryObject({
        NameObject('/Type'): NameObject('/Font'),
        NameObject('/Subtype'): NameObject('/Type1'),
        NameObject('/BaseFont'): NameObject('/Helvetica')}))
    checked = cid_font(writer, 'Wingdings2', _symbol_font('Wingdings2', 'boxcheck'))
    other = cid_font(writer, 'OtherSymbol', _symbol_font('OtherSymbol', 'unknown'))
    page = writer.add_blank_page(width=400, height=300)
    page[NameObject('/Resources')] = DictionaryObject({NameObject('/Font'): DictionaryObject({
        NameObject('/F0'): helvetica, NameObject('/F1'): checked, NameObject('/F2'): other})})
    stream = DecodedStreamObject()
    stream.set_data(b'BT /F0 14 Tf 20 200 Td (checked ) Tj /F1 14 Tf <0001> Tj '
                    b'/F0 14 Tf ( open ) Tj /F2 14 Tf <0001> Tj ET')
    page[NameObject('/Contents')] = writer._add_object(stream)
    output = BytesIO()
    writer.write(output)
    return output.getvalue()


async def test_word_keeps_an_unverified_copy_of_a_verified_private_character(tmp_path):
    from docreader.tender.directory_compile import documents_from_frozen
    from docreader.tender.evidence import freeze_sources

    artifacts = ArtifactStore(tmp_path)
    original = artifacts.put_bytes(_mixed_private_pdf(), 'application/pdf')
    registration = {'documents': [{'document_id': 'tender', 'document_version': '1', 'role': 'main',
        'ordinal': 0, 'issues': [], 'file_name': 'mixed.pdf', 'original_ref': original}], 'issues': []}
    frozen_ref = await freeze_sources(registration, artifacts, ocr=None, plan=False)
    frozen = artifacts.read_json(frozen_ref)
    assert frozen['stats']['ocr_request_count'] == 0
    raw = artifacts.read_json(frozen['documents'][0]['raw_ref'])
    glyphs = sorted(raw['source_contract']['glyph_normalizations'], key=lambda item: item['char_index'])
    assert [glyph['raw_symbol'] for glyph in glyphs] == ['\uf052', '\uf052']
    assert glyphs[0]['normalized_symbol'] == '☑'
    assert glyphs[0]['glyph_name'] == 'boxcheck' and 'Wingdings2' in glyphs[0]['font_name']
    assert 'normalized_symbol' not in glyphs[1]
    carriers = [unit['text'] for unit in raw['units'] if '\uf052' in (unit.get('text') or '')]
    assert carriers and all('☑' not in text and text.count('\uf052') == 2 for text in carriers)
    documents = documents_from_frozen(artifacts, frozen)
    assert documents[0]['pages'][0].splitlines() == ['checked \uf052 open \uf052']
    assert '☑' not in documents[0]['pages'][0]
    compiled = compile_directory(plan([node('letter', 0, 0)], [item('letter')]), documents)
    assert compiled['chapters'][0]['text'] == 'checked ☑ open \uf052'
    snapshot = {'schema_version': 2, 'run_id': 'symbol-position', 'source_version': 1, 'domain_revision': 1}
    outline = outline_from_compiled(snapshot, compiled)
    result = export_authoring_document(artifacts, outline, [])
    paragraphs = [paragraph.text for paragraph in Document(BytesIO(artifacts.read_bytes(result['docx_ref']))).paragraphs]
    assert 'checked ☑ open \uf052' in paragraphs
    assert 'checked ☑ open ☑' not in '\n'.join(paragraphs)
    assert any(issue['code'] == 'outline_directory_symbol_unconfirmed' for issue in documents[0]['ocr_issues'])


def _checkbox_glyph(index, box, verified=True):
    left, bottom, right, top = box
    glyph = {'page_ordinal': 0, 'char_index': index, 'raw_symbol': '\uf052',
             'left': left, 'bottom': bottom, 'right': right, 'top': top}
    if verified:
        glyph.update(normalized_symbol='☑', font_name='Wingdings2', glyph_name='boxcheck', font_sha256='a' * 64)
    return glyph


def test_symbol_edits_follow_the_lower_glyph_when_the_stream_starts_there():
    from docreader.tender.directory_compile import bind_symbol_edits

    lines = ['upper       \uf052', 'lower   \uf052']
    words = [
        (20, 68, 56, 84, 'upper'), (160, 68, 170, 84, '\uf052'),
        (20, 218, 54, 233, 'lower'), (110, 218, 118, 233, '\uf052'),
    ]
    # char_index 0 is the lower verified glyph. Layout still reads the upper line first.
    edits = bind_symbol_edits(lines, [
        _checkbox_glyph(0, (110, 70, 116, 80)),
        _checkbox_glyph(1, (160, 220, 166, 230), verified=False),
    ], words, 300, {'source_contract': {}})
    assert edits == [(1, lines[1].index('\uf052'), '☑')]


def test_symbol_edits_keep_layout_spaces_and_drop_ambiguous_boxes():
    from docreader.tender.directory_compile import bind_symbol_edits

    line = 'name    \uf052'
    words = [(10, 10, 40, 22, 'name'), (90, 10, 102, 22, '\uf052')]
    edits = bind_symbol_edits([line], [_checkbox_glyph(0, (92, 78, 100, 90))], words, 100, {'source_contract': {}})
    assert edits == [(0, line.index('\uf052'), '☑')]
    assert edits[0][1] > len('name')

    lines = ['upper \uf052', 'lower \uf052']
    stacked = [
        (20, 10, 60, 22, 'upper'), (70, 10, 82, 22, '\uf052'),
        (20, 40, 60, 52, 'lower'), (70, 40, 82, 52, '\uf052'),
    ]
    assert bind_symbol_edits(lines, [
        _checkbox_glyph(0, (72, 78, 80, 90)),
        _checkbox_glyph(1, (73, 79, 81, 89)),
    ], stacked, 100, {'source_contract': {}}) == []
    assert bind_symbol_edits(lines, [_checkbox_glyph(0, (1, 1, 2, 2))], stacked, 100, {'source_contract': {}}) == []


def _reversed_private_pdf():
    """Lower verified glyph is first in the content stream; the unverified copy sits above it."""
    from pypdf import PdfWriter
    from pypdf.generic import DecodedStreamObject, DictionaryObject, NameObject

    writer = PdfWriter()
    helvetica = writer._add_object(DictionaryObject({
        NameObject('/Type'): NameObject('/Font'),
        NameObject('/Subtype'): NameObject('/Type1'),
        NameObject('/BaseFont'): NameObject('/Helvetica')}))
    checked = _cid_symbol_font(writer, 'Wingdings2', _symbol_font('Wingdings2', 'boxcheck'))
    other = _cid_symbol_font(writer, 'OtherSymbol', _symbol_font('OtherSymbol', 'unknown'))
    page = writer.add_blank_page(width=400, height=300)
    page[NameObject('/Resources')] = DictionaryObject({NameObject('/Font'): DictionaryObject({
        NameObject('/F0'): helvetica, NameObject('/F1'): checked, NameObject('/F2'): other})})
    stream = DecodedStreamObject()
    stream.set_data(b'BT /F0 14 Tf 20 70 Td (lower) Tj 90 0 Td /F1 14 Tf <0001> Tj ET '
                    b'BT /F0 14 Tf 20 220 Td (upper) Tj 140 0 Td /F2 14 Tf <0001> Tj ET')
    page[NameObject('/Contents')] = writer._add_object(stream)
    output = BytesIO()
    writer.write(output)
    return output.getvalue()


def _cid_symbol_font(writer, base, payload):
    from pypdf.generic import (
        ArrayObject, DecodedStreamObject, DictionaryObject, NameObject, NumberObject, create_string_object,
    )

    descriptor = DictionaryObject({
        NameObject('/Type'): NameObject('/FontDescriptor'),
        NameObject('/FontName'): NameObject('/' + base),
        NameObject('/Flags'): NumberObject(32),
        NameObject('/FontBBox'): ArrayObject([NumberObject(value) for value in (0, -200, 1000, 800)]),
        NameObject('/ItalicAngle'): NumberObject(0),
        NameObject('/Ascent'): NumberObject(800),
        NameObject('/Descent'): NumberObject(-200),
        NameObject('/CapHeight'): NumberObject(700),
        NameObject('/StemV'): NumberObject(80),
    })
    program = DecodedStreamObject()
    program.set_data(payload)
    descriptor[NameObject('/FontFile2')] = writer._add_object(program)
    cid = DictionaryObject({
        NameObject('/Type'): NameObject('/Font'),
        NameObject('/Subtype'): NameObject('/CIDFontType2'),
        NameObject('/BaseFont'): NameObject('/' + base),
        NameObject('/CIDSystemInfo'): DictionaryObject({
            NameObject('/Registry'): create_string_object('Adobe'),
            NameObject('/Ordering'): create_string_object('Identity'),
            NameObject('/Supplement'): NumberObject(0),
        }),
        NameObject('/FontDescriptor'): writer._add_object(descriptor),
        NameObject('/CIDToGIDMap'): NameObject('/Identity'),
        NameObject('/DW'): NumberObject(600),
    })
    unicode_map = DecodedStreamObject()
    unicode_map.set_data(
        b'/CIDInit /ProcSet findresource begin 12 dict begin begincmap '
        b'/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def '
        b'/CMapName /Adobe-Identity-UCS def /CMapType 2 def '
        b'1 begincodespacerange <0000> <FFFF> endcodespacerange '
        b'1 beginbfchar <0001> <F052> endbfchar endcmap '
        b'CMapName currentdict /CMap defineresource pop end end')
    return writer._add_object(DictionaryObject({
        NameObject('/Type'): NameObject('/Font'),
        NameObject('/Subtype'): NameObject('/Type0'),
        NameObject('/BaseFont'): NameObject('/' + base),
        NameObject('/Encoding'): NameObject('/Identity-H'),
        NameObject('/DescendantFonts'): ArrayObject([writer._add_object(cid)]),
        NameObject('/ToUnicode'): writer._add_object(unicode_map),
    }))


async def test_word_binds_the_verified_symbol_when_the_content_stream_is_reversed(tmp_path):
    from subprocess import run
    from docreader.tender.directory_compile import documents_from_frozen
    from docreader.tender.evidence import freeze_sources

    artifacts = ArtifactStore(tmp_path)
    payload = _reversed_private_pdf()
    original = artifacts.put_bytes(payload, 'application/pdf')
    registration = {'documents': [{'document_id': 'tender', 'document_version': '1', 'role': 'main',
        'ordinal': 0, 'issues': [], 'file_name': 'reversed.pdf', 'original_ref': original}], 'issues': []}
    frozen_ref = await freeze_sources(registration, artifacts, ocr=None, plan=False)
    frozen = artifacts.read_json(frozen_ref)
    assert frozen['stats']['ocr_request_count'] == 0
    raw = artifacts.read_json(artifacts.read_json(frozen_ref)['documents'][0]['raw_ref'])
    glyphs = sorted(raw['source_contract']['glyph_normalizations'], key=lambda item: item['char_index'])
    assert glyphs[0]['normalized_symbol'] == '☑' and glyphs[0]['bottom'] < 150
    assert 'normalized_symbol' not in glyphs[1]
    documents = documents_from_frozen(artifacts, artifacts.read_json(frozen_ref))
    source_path = tmp_path / 'reversed.pdf'
    source_path.write_bytes(payload)
    layout = run(['pdftotext', '-layout', '-enc', 'UTF-8', str(source_path), '-'], capture_output=True, check=True)
    request = layout.stdout.decode('utf-8').removesuffix('\f')
    assert documents[0]['pages'] == [request]
    assert '☑' not in request
    compiled = compile_directory(plan([node('letter', 0, 0)], [item('letter')]), documents)
    body = compiled['chapters'][0]['text'].splitlines()
    upper = next(line for line in body if line.startswith('upper'))
    lower = next(line for line in body if line.startswith('lower'))
    assert upper == 'upper       \uf052'
    assert lower == 'lower   ☑'
    snapshot = {'schema_version': 2, 'run_id': 'symbol-position', 'source_version': 1, 'domain_revision': 1}
    outline = outline_from_compiled(snapshot, compiled)
    result = export_authoring_document(artifacts, outline, [])
    paragraphs = [paragraph.text for paragraph in Document(BytesIO(artifacts.read_bytes(result['docx_ref']))).paragraphs]
    shown = [line.replace('\xa0', ' ').replace('\t', '') for line in paragraphs]
    assert 'upper       \uf052' in shown
    assert 'lower   ☑' in shown
    assert not any(line.startswith('upper') and '☑' in line for line in paragraphs)
    assert not any(line.startswith('lower') and '\uf052' in line for line in paragraphs)


def test_word_export_substitutes_a_symbol_only_with_verified_font_evidence(tmp_path, monkeypatch):
    verified = [{'page_ordinal': 0, 'char_index': 0, 'raw_symbol': '\uf052', 'normalized_symbol': '☑',
                 'font_name': 'ABCDEF+Wingdings2,Bold', 'glyph_name': 'boxcheck', 'font_sha256': 'a' * 64}]
    text, issues, reading = _exported_text(tmp_path / 'verified', monkeypatch, verified)
    assert reading == ['投标函\n\uf052已核对\n']
    assert '☑已核对' in text
    assert '\uf052' not in text
    assert not any(issue['code'] == 'outline_directory_symbol_unconfirmed' for issue in issues)

    unverified = [{**verified[0], 'glyph_name': 'unknown', 'normalized_symbol': '☑'}]
    text, issues, reading = _exported_text(tmp_path / 'unverified', monkeypatch, unverified)
    assert reading == ['投标函\n\uf052已核对\n']
    assert '\uf052已核对' in text
    assert '☑' not in text
    assert any(issue['code'] == 'outline_directory_symbol_unconfirmed' for issue in issues)
