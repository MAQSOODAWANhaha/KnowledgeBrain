"""Tender source characters precede display cleanup and keep physical identity."""

import hashlib
from io import BytesIO
import json
from types import SimpleNamespace

from pypdf import PdfWriter
from pypdf.generic import DecodedStreamObject, DictionaryObject, NameObject
import pytest

from docreader.tender.evidence import pdf_literal


def original_pdf():
    writer = PdfWriter()
    font = writer._add_object(DictionaryObject({
        NameObject('/Type'): NameObject('/Font'),
        NameObject('/Subtype'): NameObject('/Type1'),
        NameObject('/BaseFont'): NameObject('/Helvetica')}))
    for _ in range(2):
        page = writer.add_blank_page(width=200, height=200)
        page[NameObject('/Resources')] = DictionaryObject({
            NameObject('/Font'): DictionaryObject({NameObject('/F1'): font})})
        stream = DecodedStreamObject()
        stream.set_data(b'BT /F1 12 Tf 10 180 Td (# literal heading) Tj '
                        b'0 -25 Td (same same) Tj 0 -25 Td (71) Tj ET')
        page[NameObject('/Contents')] = writer._add_object(stream)
    output = BytesIO()
    writer.write(output)
    return output.getvalue()


class TextPage:
    def __init__(self, rows):
        self.rows, self.closed = rows, False

    def count_chars(self):
        return len(self.rows)

    def get_text_range(self, index, count, *, errors):
        assert count == 1 and errors == 'strict'
        value = self.rows[index]['text']
        if isinstance(value, Exception):
            raise value
        return value

    def get_charbox(self, index):
        value = self.rows[index].get('bbox', (1, 1, 9, 9))
        if isinstance(value, Exception):
            raise value
        return value

    def close(self):
        self.closed = True


class Page:
    def __init__(self, rows):
        self.textpage = TextPage(rows)

    def get_size(self):
        return 100, 100

    def get_textpage(self):
        return self.textpage

    def get_objects(self):
        return iter(())


def raw_for(rows):
    return SimpleNamespace(FPDFText_GetUnicode=lambda _page, index: rows[index]['codepoint'])


def test_original_hash_headings_repeated_edges_and_line_breaks_survive():
    content = original_pdf()
    result = pdf_literal.extract_pdf_literal(content)
    assert result['contract'] == pdf_literal.CONTRACT
    assert result['source_sha256'] == hashlib.sha256(content).hexdigest()
    assert result['coordinate_system'] == 'pdf_bottom_left'
    assert result['text_order'] == 'pdfium_character_stream'
    assert result['page_count'] == 2
    for page in result['pages']:
        assert page['text_stream'].startswith('# literal heading')
        assert page['text_stream'].count('same') == 2
        assert page['text_stream'].endswith('71')
        assert '\n' in page['text_stream']
        assert [c['char_index'] for c in page['characters']] == list(range(page['char_count']))
        assert any(c['text'] in ('\r', '\n', '\r\n') for c in page['characters'])
        assert page['width'] == page['height'] == 200
    assert json.loads(json.dumps(result, allow_nan=False)) == result


def test_missing_text_and_boxes_keep_every_original_index():
    rows = [
        {'text': 'A', 'codepoint': 65},
        {'text': RuntimeError('unreadable'), 'codepoint': 66},
        {'text': '\n', 'codepoint': 10, 'bbox': RuntimeError('no box')},
        {'text': 'C', 'codepoint': 67, 'bbox': (float('nan'), 1, 9, 9)},
    ]
    page = Page(rows)
    result = pdf_literal._page_literal(page, 3, raw_for(rows))
    assert [c['char_index'] for c in result['characters']] == [0, 1, 2, 3]
    assert result['characters'][1]['text'] == ''
    assert result['characters'][1]['pdfium_unicode'] == 66
    assert result['characters'][2]['text'] == '\n'
    assert result['characters'][2]['bbox'] is None
    assert result['characters'][3]['bbox'] is None
    assert {issue['char_index'] for issue in result['issues']} == {1, 2, 3}
    assert page.textpage.closed
    json.dumps(result, allow_nan=False)


def test_pdfium_text_expansion_is_not_falsely_a_single_unicode_character():
    rows = [{'text': 'ffi', 'codepoint': 0xFB03}, {'text': '', 'codepoint': 0x200B}]
    result = pdf_literal._page_literal(Page(rows), 0, raw_for(rows))
    assert result['text_stream'] == 'ffi'
    assert result['char_count'] == 2
    assert result['characters'][0]['text'] == 'ffi'
    assert result['characters'][0]['unicode_codepoint'] is None
    assert result['characters'][0]['pdfium_unicode'] == 0xFB03
    assert result['characters'][1]['text'] == ''
    assert result['characters'][1]['pdfium_unicode'] == 0x200B
    assert result['characters'][1]['unicode_codepoint'] is None
    assert any(issue['char_index'] == 1 for issue in result['issues'])


def test_visibility_candidates_do_not_delete_source_characters(monkeypatch):
    rows = [{'text': 'H', 'codepoint': 72},
            {'text': 'O', 'codepoint': 79, 'bbox': (110, 1, 119, 9)},
            {'text': '\uf06c', 'codepoint': 0xF06C}]
    monkeypatch.setattr(pdf_literal, '_collect_invisible_boxes', lambda _page, _raw: [(0, 0, 10, 10)])
    result = pdf_literal._page_literal(Page(rows), 0, raw_for(rows))
    assert result['text_stream'] == 'HO\uf06c'
    assert len(result['characters']) == 3
    assert 'invisible_text_bbox_candidate' in result['characters'][0]['flags']
    assert 'outside_page' in result['characters'][1]['flags']
    assert result['characters'][2]['text'] == '\uf06c'


def test_invalid_pdf_is_not_accepted_as_an_empty_source():
    with pytest.raises((RuntimeError, ValueError)):
        pdf_literal.extract_pdf_literal(b'not a PDF')
