from io import BytesIO

from docx import Document
from pypdf import PdfWriter
from pypdf.generic import DecodedStreamObject, DictionaryObject, NameObject
import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.tender.authoring_export import export_authoring_document
from docreader.tender.directory_compile import compile_directory, outline_from_compiled
from docreader.tender.evidence.references import SourceIndex
from test_phase1_bid_format import _snapshot


@pytest.mark.parametrize('pages', [['投标文件'], ['投标文件投标文件'], ['投标', '文件']])
def test_vertical_cover_binds_only_one_complete_title_on_its_frozen_page(tmp_path, pages):
    from docreader.tender.signed_cover_layout import bind_cover_source

    store = ArtifactStore(tmp_path)
    writer = PdfWriter()
    units, source_units = [], []
    for number, letters in enumerate(pages):
        writer.add_blank_page(width=595, height=842)
        for letter in letters:
            ordinal = len(units)
            identity = f'glyph-{ordinal}'
            loc = {'locator_kind': 'page', 'page_ordinal': number}
            units.append({'unit_id': identity, 'kind': 'section', 'text': letter, 'ordinal': ordinal, 'locator': loc})
            source_units.append({'unit_id': identity, 'section_id': identity, 'parent_section_id': None,
                                 'heading_path': letter, 'physical_locator': loc})
    buf = BytesIO()
    writer.write(buf)
    original = store.put_bytes(buf.getvalue(), 'application/pdf')
    record = {'document_id': 'tender', 'document_version': '1', 'original_ref': original}
    raw = {'schema_version': 2, 'units': units, 'images': {}, 'ocr': {}, 'original_ref': original,
           'source_contract': {'page_manifest': [], 'units': source_units}}
    index = SourceIndex(record, raw)
    index_ref = store.put_json(index.descriptor(store.put_json(index.raw)))
    snapshot = {'frozen_ref': store.put_json({'documents': [{**record, 'source_index_ref': index_ref}]})}
    compiled = {'chapters': [{'id': 'cover', 'presentation': 'cover', 'title': '投标文件',
                              'heading': {'document_id': 'tender', 'page': 0, 'line': 0}}], 'issues': []}
    target = {'kind': 'form', 'source_refs': []}
    bind_cover_source(store, snapshot, compiled, {'cover': {'targets': [target]}})
    if pages == ['投标文件']:
        assert target['source_refs'] == [{'source_index_ref': index_ref, 'unit_id': 'glyph-0'}]
        assert not compiled['issues']
    else:
        assert not target['source_refs'] and compiled['issues'][0]['code'] == 'outline_directory_literal_pending'


def test_bound_cover_keeps_the_original_signature_position_and_only_the_drawn_rule(tmp_path):
    writer = PdfWriter()
    page = writer.add_blank_page(width=595, height=842)
    font = DictionaryObject({NameObject('/Type'): NameObject('/Font'), NameObject('/Subtype'): NameObject('/Type1'),
                             NameObject('/BaseFont'): NameObject('/Helvetica')})
    page[NameObject('/Resources')] = DictionaryObject({NameObject('/Font'): DictionaryObject({
        NameObject('/F1'): writer._add_object(font)})})
    content = DecodedStreamObject()
    content.set_data(b'BT /F1 24 Tf 190 620 Td (Submission) Tj ET '
                     b'BT /F1 14 Tf 72 200 Td (Bidder:) Tj 270 0 Td (Seal) Tj ET '
                     b'120 197 m 338 197 l S '
                     b'BT /F1 12 Tf 72 155 Td (Date:) Tj ET')
    page[NameObject('/Contents')] = writer._add_object(content)
    output = BytesIO()
    writer.write(output)
    store = ArtifactStore(tmp_path)
    original = store.put_bytes(output.getvalue(), 'application/pdf')
    point = {'document_id': 'main', 'page': 0, 'line': 0}
    locator = {'locator_kind': 'page', 'page_ordinal': 0, 'left': 190., 'right': 320., 'top': 644., 'bottom': 620.}
    unit = {'unit_id': 'heading', 'kind': 'section', 'text': 'Submission', 'ordinal': 0, 'locator': locator}
    raw = {'schema_version': 2, 'units': [unit], 'images': {}, 'ocr': {}, 'original_ref': original,
           'source_contract': {'page_manifest': [], 'units': [{'unit_id': 'heading', 'section_id': 's1',
           'parent_section_id': None, 'heading_path': 'Submission', 'physical_locator': locator}]}}
    record = {'document_id': 'main', 'document_version': '1', 'original_ref': original}
    index = SourceIndex(record, raw)
    index_ref = store.put_json(index.descriptor(store.put_json(index.raw)))
    snapshot = {**_snapshot(), 'frozen_ref': store.put_json({'documents': [{**record, 'source_index_ref': index_ref}]})}
    docs = [{'document_id': 'main', 'role': 'main', 'pages': ['Submission\n\nBidder:          Seal\nDate:']}]
    plan = {'nodes': [{'id': 'cover', 'heading': point, 'response_kind': 'form',
                       'body_windows': [{'document_id': 'main', 'page': 0, 'start_line': 0, 'end_line': 3}]}],
            'scopes': [{'items': [{'node_id': 'cover', 'depth': 'self', 'presentation': 'cover'}]}]}
    outline = outline_from_compiled(snapshot, compile_directory(plan, docs), artifacts=store)
    exported = export_authoring_document(store, outline, [])
    word = Document(BytesIO(store.read_bytes(exported['docx_ref'])))
    bidder = next(p for p in word.paragraphs if p.text.startswith('Bidder:'))
    assert any(r.underline and '\t' in r.text for r in bidder.runs)
    spacing = bidder.paragraph_format.space_before
    assert spacing is not None and spacing.pt > 250
    date = next(p for p in word.paragraphs if p.text == 'Date:')
    assert not any(r.underline for r in date.runs)

    outline['chapters'][0]['targets'][0]['source_refs'][0]['unit_id'] = 'unbound'
    fallback = export_authoring_document(store, outline, [])
    plain = Document(BytesIO(store.read_bytes(fallback['docx_ref'])))
    assert any(p.text.startswith('Bidder:') for p in plain.paragraphs)
    assert any(gap['code'] == 'export_cover_layout_unverified' for gap in fallback['gaps'])
