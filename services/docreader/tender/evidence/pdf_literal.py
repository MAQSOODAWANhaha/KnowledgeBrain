"""Tender-only PDF character evidence, before Markdown and display cleanup.

``text_stream`` concatenates PDFium's per-character text results. Its offsets
need not equal ``char_index``, and its order is not a business reading order.
Original grids remain in the existing native snapshot, not reconstructed here.
"""
from __future__ import annotations

import hashlib
import math

from docreader.parser.pdf_parser import _PDFIUM_LOCK, _collect_invisible_boxes, _point_in_boxes
from docreader.parser.pdf_symbols import collect_page_symbols, is_private_symbol, pdf_glyph_bindings


CONTRACT = 'tender_pdf_literal_v1'


def present_literal_characters(characters):
    """Body presentation of one literal run. Character text stays the raw receipt."""
    presented = []
    pieces = []
    raw_code_points = []
    pending = False
    for character in characters:
        text = character.get('text') or ''
        if text == '\ufffe':
            raw_code_points.append(0xFFFE)
            if 'hyphen_evidence' in (character.get('flags') or []):
                presented.append('-')
                pieces.append('-')
            else:
                pending = True
                pieces.append('')
            continue
        presented.append(text)
        pieces.append(text)
        if len(text) == 1:
            raw_code_points.append(ord(text))
    return {'text': ''.join(presented), 'pending': pending, 'raw_code_points': raw_code_points,
            'pieces': pieces}


def _page_literal(page, page_ordinal, raw):
    width, height = page.get_size()
    if not all(math.isfinite(value) and value > 0 for value in (width, height)):
        raise ValueError('PDF literal page dimensions are unavailable')
    invisible = _collect_invisible_boxes(page, raw)
    textpage = page.get_textpage()
    characters, issues = [], []

    def problem(code, index, error=None):
        value = {'code': code, 'char_index': index}
        if error is not None:
            value['detail'] = type(error).__name__
        issues.append(value)

    try:
        count = textpage.count_chars()
        if count < 0:
            raise ValueError('PDF literal character count is unavailable')
        for index in range(count):
            character = {'char_index': index, 'text': '', 'unicode_codepoint': None,
                         'pdfium_unicode': None, 'bbox': None, 'flags': []}
            try:
                character['pdfium_unicode'] = int(raw.FPDFText_GetUnicode(textpage, index))
            except Exception as error:
                problem('pdf_literal_unicode_unreadable', index, error)
            try:
                text = textpage.get_text_range(index, 1, errors='strict')
                text.encode('utf-8', errors='strict')
                character['text'] = text
                character['unicode_codepoint'] = ord(text) if len(text) == 1 else None
                if not text:
                    problem('pdf_literal_character_text_empty', index)
                elif len(text) > 1:
                    character['flags'].append('text_range_expanded')
            except Exception as error:
                problem('pdf_literal_character_text_unreadable', index, error)
            try:
                bounds = list(textpage.get_charbox(index))
                if len(bounds) != 4 or not all(math.isfinite(value) for value in bounds):
                    raise ValueError('PDF character box is not finite')
                character['bbox'] = bounds
                left, bottom, right, top = bounds
                x0, x1 = min(left, right), max(left, right)
                y0, y1 = min(bottom, top), max(bottom, top)
                if x1 < 0 or x0 > width or y1 < 0 or y0 > height:
                    character['flags'].append('outside_page')
                if _point_in_boxes((x0 + x1) / 2, (y0 + y1) / 2, invisible):
                    # Bounding-box overlap is a candidate, not proof of hidden
                    # ownership. Neither this flag nor page bounds remove text.
                    character['flags'].append('invisible_text_bbox_candidate')
            except Exception as error:
                problem('pdf_literal_character_box_unreadable', index, error)
            if character['text'] in ('\r', '\n', '\r\n') or character['pdfium_unicode'] in (10, 13):
                character['flags'].append('line_break')
            # PDFium reports unicode 2 when a line-end hyphen has no glyph.
            if character['text'] == '\ufffe' and character.get('pdfium_unicode') == 2:
                character['flags'].append('hyphen_evidence')
            characters.append(character)
    finally:
        textpage.close()
    return {'page_ordinal': page_ordinal, 'width': width, 'height': height,
            'char_count': count, 'text_stream': ''.join(c['text'] for c in characters),
            'characters': characters, 'symbol_evidence': [], 'issues': issues}


def extract_pdf_literal(content: bytes) -> dict:
    """Read original character identities; do not choose roles or normalize text."""
    import pypdfium2 as pdfium
    import pypdfium2.raw as raw

    source_sha = hashlib.sha256(content).hexdigest()
    pages, issues, font_cache = [], [], {}
    with _PDFIUM_LOCK:
        document = pdfium.PdfDocument(content)
        try:
            if not len(document):
                raise ValueError('PDF literal source has no pages')
            bindings = pdf_glyph_bindings(content)
            for ordinal in range(len(document)):
                page = document[ordinal]
                try:
                    value = _page_literal(page, ordinal, raw)
                    if any(is_private_symbol(c['text']) for c in value['characters']):
                        try:
                            value['symbol_evidence'] = collect_page_symbols(
                                page, ordinal, raw, font_cache, font_bindings=bindings)
                            for item in value['symbol_evidence']:
                                if not item.get('normalized_symbol'):
                                    value['issues'].append({'code': 'pdf_literal_private_symbol_unresolved',
                                                            'char_index': item['char_index']})
                        except Exception as error:
                            value['issues'].append({'code': 'pdf_literal_symbol_evidence_unavailable',
                                                    'detail': type(error).__name__})
                    pages.append(value)
                    issues.extend({'page_ordinal': ordinal, **item} for item in value['issues'])
                finally:
                    page.close()
        finally:
            document.close()
    return {'contract': CONTRACT, 'source_sha256': source_sha, 'page_count': len(pages),
            'coordinate_system': 'pdf_bottom_left', 'text_order': 'pdfium_character_stream',
            'pages': pages, 'issues': issues}
