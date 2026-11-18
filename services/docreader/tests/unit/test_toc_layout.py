"""Generated TOC caches are verified against actual PDF destinations."""

from io import BytesIO
from zipfile import ZipFile

from docx import Document
from lxml import etree as ET
from pypdf import PdfWriter
from pypdf.generic import (
    ArrayObject, DictionaryObject, FloatObject, NameObject, NumberObject, StreamObject,
)
import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.tender.authoring_export import export_authoring_document
from docreader.tender import toc_layout
from docreader.tender.word_navigation import bookmark_name


W = '{http://schemas.openxmlformats.org/wordprocessingml/2006/main}'


def source_word(tmp_path, titles=('Alpha',), *, cached=None):
    chapters = [{'chapter_id': f'chapter-{i}', 'title': title, 'targets': [], 'children': [],
        'is_volume': False} for i, title in enumerate(titles, 1)]
    artifacts = ArtifactStore(tmp_path / 'artifacts')
    result = export_authoring_document(artifacts, {'chapters': chapters}, [])
    data = artifacts.read_bytes(result['docx_ref'])
    if cached is not None:
        def change(root):
            for field in root.iter(W + 'fldSimple'):
                if field.get(W + 'instr', '').lstrip().startswith('PAGEREF '):
                    next(field.iter(W + 't')).text = str(cached)
        data = mutate_xml(data, change)
    return data


def mutate_xml(data, change):
    output = BytesIO()
    with ZipFile(BytesIO(data)) as original, ZipFile(output, 'w') as derived:
        for info in original.infolist():
            value = original.read(info.filename)
            if info.filename == 'word/document.xml':
                root = ET.fromstring(value)
                change(root)
                value = ET.tostring(root, xml_declaration=True, encoding='UTF-8', standalone=True)
            derived.writestr(info, value)
    return output.getvalue()


def pdf(titles=('Alpha',), *, pages=None, wrapped=False, labels=None, bad_title=False, named=False,
        producer=None, roman_pages=False, cache_texts=None, swapped_destinations=False):
    writer = PdfWriter()
    if producer is not None:
        writer.add_metadata({'/Producer': producer})
    positions = pages if pages is not None else [3] * len(titles)
    for _ in range(max(3, *positions)):
        writer.add_blank_page(612, 792)
    if roman_pages:
        writer._root_object[NameObject('/PageLabels')] = DictionaryObject({NameObject('/Nums'):
            ArrayObject([NumberObject(0), DictionaryObject({NameObject('/S'): NameObject('/r')})])})
    font = DictionaryObject({NameObject('/Type'): NameObject('/Font'),
        NameObject('/Subtype'): NameObject('/Type1'), NameObject('/BaseFont'): NameObject('/Helvetica')})
    font_ref = writer._add_object(font)
    contents = [[] for _ in writer.pages]
    for page in writer.pages:
        page[NameObject('/Resources')] = DictionaryObject({
            NameObject('/Font'): DictionaryObject({NameObject('/F1'): font_ref})})
    for ordinal, (title, destination_page) in enumerate(zip(titles, positions), 1):
        label = labels[ordinal - 1] if labels is not None else f'{ordinal} {title}'
        y = 740 - ordinal * 40
        parts = [label] if not wrapped else [label[:5], label[5:]]
        target_ordinal = 3 - ordinal if swapped_destinations else ordinal
        dest = ArrayObject([writer.pages[destination_page - 1].indirect_reference,
            NameObject('/XYZ'), NumberObject(0), NumberObject(735 - target_ordinal * 25), NumberObject(0)])
        if named:
            writer.add_named_destination_array(f'entry-{ordinal}', dest)
            from pypdf.generic import TextStringObject
            dest = TextStringObject(f'entry-{ordinal}')
        for line, part in enumerate(parts):
            bottom = y - line * 15
            contents[0].append(f'BT /F1 12 Tf 72 {bottom} Td ({part}) Tj ET')
            annotation = DictionaryObject({NameObject('/Type'): NameObject('/Annot'),
                NameObject('/Subtype'): NameObject('/Link'),
                NameObject('/Rect'): ArrayObject([FloatObject(71), FloatObject(bottom - 2),
                    FloatObject(500), FloatObject(bottom + 13)]),
                NameObject('/A'): DictionaryObject({NameObject('/S'): NameObject('/GoTo'), NameObject('/D'): dest})})
            writer.add_annotation(0, annotation)
        cache = str(destination_page) if cache_texts is None else cache_texts[ordinal - 1]
        contents[0].append(f'BT /F1 12 Tf 540 {bottom} Td (.... {cache}) Tj ET')
        for destination in range(1, len(writer.pages)):
            text = 'Other heading' if bad_title else title
            contents[destination].append(f'BT /F1 12 Tf 72 {720 - ordinal * 25} Td ({text}) Tj ET')
    for page, rows in zip(writer.pages, contents):
        stream = StreamObject()
        stream.set_data('\n'.join(rows).encode())
        page[NameObject('/Contents')] = writer._add_object(stream)
    result = BytesIO()
    writer.write(result)
    return result.getvalue()


def contents(data):
    with ZipFile(BytesIO(data)) as archive:
        return {entry.filename: archive.read(entry) for entry in archive.infolist()}


def test_real_link_pages_are_refilled_then_reverified_without_changing_other_xml(tmp_path):
    original = source_word(tmp_path)
    calls = []
    def render(data):
        calls.append(data)
        return pdf(), 'onlyoffice-test-8.3.3-fonts-a'
    derived, receipt = toc_layout.finalize_toc(original, render)
    assert receipt['status'] == 'fixed' and receipt['render_count'] == 2
    assert len(calls) == 2 and calls[0] == original and calls[-1] == derived
    before, after = contents(original), contents(derived)
    assert before.keys() == after.keys()
    assert all(before[name] == after[name] for name in before if name != 'word/document.xml')
    assert after['word/document.xml'] == before['word/document.xml'].replace('待计算'.encode(), b'3')
    assert receipt['entries'][0]['page'] == '3'
    assert receipt['entries'][0]['anchor'] == bookmark_name('chapter-1')
    assert receipt['pdf_cache_display_verified']
    assert receipt['entries'][0]['cache'] == receipt['entries'][0]['display_cache'] == '3'
    toc_layout.verify_toc(derived, pdf())


@pytest.mark.parametrize('wrapped,named', [(True, False), (False, True)])
def test_wrapped_or_named_pdf_links_keep_their_complete_destination(tmp_path, wrapped, named):
    original = source_word(tmp_path, ('Long Alpha',))
    result, receipt = toc_layout.finalize_toc(original,
        lambda _data: (pdf(('Long Alpha',), wrapped=wrapped, named=named), 'same-engine'))
    assert receipt['render_count'] == 2 and receipt['entries'][0]['page'] == '3'
    toc_layout.verify_toc(result, pdf(('Long Alpha',), wrapped=wrapped, named=named))


def test_same_physical_page_with_distinct_destination_coordinates_remains_two_entries(tmp_path):
    original = source_word(tmp_path, ('Alpha', 'Beta'))
    _, receipt = toc_layout.finalize_toc(original, lambda _data: (pdf(('Alpha', 'Beta')), 'same-engine'))
    assert len(receipt['entries']) == 2
    assert [entry['page'] for entry in receipt['entries']] == ['3', '3']


def test_swapped_heading_destinations_on_the_same_page_are_rejected(tmp_path):
    original = source_word(tmp_path, ('Alpha', 'Beta'), cached=3)
    with pytest.raises(toc_layout.LayoutError, match='目标|标题'):
        toc_layout.verify_toc(original, pdf(('Alpha', 'Beta'), swapped_destinations=True))


def test_already_correct_caches_are_verified_in_one_conversion(tmp_path):
    original = source_word(tmp_path, cached=3)
    result, receipt = toc_layout.finalize_toc(original, lambda _data: (pdf(), 'same-engine'))
    assert result == original and receipt['render_count'] == 1


def test_wrong_cached_page_is_rejected_by_independent_verification(tmp_path):
    original = source_word(tmp_path, cached=2)
    with pytest.raises(toc_layout.LayoutError) as error:
        toc_layout.verify_toc(original, pdf())
    assert error.value.code == 'TOC_CACHE_MISMATCH'


@pytest.mark.parametrize('display', ['pending', '99', ''])
def test_pdf_missing_or_wrong_display_cache_cannot_pass_with_correct_word_cache(tmp_path, display):
    original = source_word(tmp_path, cached=3)
    with pytest.raises(toc_layout.LayoutError) as error:
        toc_layout.verify_toc(original, pdf(cache_texts=[display]))
    assert error.value.code == 'TOC_PDF_CACHE_MISMATCH'


def test_stale_pdf_is_rejected_after_word_cache_was_refilled(tmp_path):
    original = source_word(tmp_path)
    calls = []
    def stale(data):
        calls.append(data)
        return pdf(cache_texts=['pending']), 'same-engine'
    with pytest.raises(toc_layout.LayoutError) as error:
        toc_layout.finalize_toc(original, stale)
    assert error.value.code == 'TOC_PDF_CACHE_MISMATCH' and len(calls) == 2
    assert error.value.candidate_docx != original


def test_concurrent_toc_reads_use_the_parser_pdfium_lock_but_http_callbacks_do_not(tmp_path, monkeypatch):
    from concurrent.futures import ThreadPoolExecutor
    import threading
    from docreader.parser import pdf_parser
    original = source_word(tmp_path, cached=3)
    output = pdf()
    rendezvous = threading.Barrier(2)

    class SharedLock:
        def __init__(self):
            self.lock, self.owner = threading.Lock(), None
        def __enter__(self):
            self.lock.acquire()
            self.owner = threading.get_ident()
        def __exit__(self, *_args):
            self.owner = None
            self.lock.release()

    shared = SharedLock()
    document = toc_layout.pdfium.PdfDocument
    def checked_document(*args, **kwargs):
        assert shared.owner == threading.get_ident(), 'PDFium entered without the shared parser lock'
        return document(*args, **kwargs)
    def render(_data):
        assert shared.owner is None, 'The renderer must run outside the PDFium lock'
        rendezvous.wait(timeout=5)
        return output, 'same-engine'
    monkeypatch.setattr(pdf_parser, '_PDFIUM_LOCK', shared)
    monkeypatch.setattr(toc_layout.pdfium, 'PdfDocument', checked_document)
    with ThreadPoolExecutor(max_workers=2) as threads:
        futures = [threads.submit(toc_layout.finalize_toc, original, render) for _ in range(2)]
        results = [future.result(timeout=10) for future in futures]
    assert all(data == original and report['render_count'] == 1 for data, report in results)


@pytest.mark.parametrize('kind', ['label', 'title', 'count', 'no_pdf'])
def test_unproved_pdf_mapping_never_publishes_estimated_pages(tmp_path, kind):
    original = source_word(tmp_path)
    data = {'label': pdf(labels=['Wrong source label']), 'title': pdf(bad_title=True),
        'count': pdf(('Alpha', 'Beta')), 'no_pdf': b'not a PDF'}[kind]
    with pytest.raises(toc_layout.LayoutError):
        toc_layout.finalize_toc(original, lambda _data: (data, 'same-engine'))
    assert '待计算'.encode() in contents(original)['word/document.xml']


def test_engine_change_during_fixed_point_is_rejected(tmp_path):
    original = source_word(tmp_path)
    identities = iter(['fonts-a', 'fonts-b'])
    with pytest.raises(toc_layout.LayoutError) as error:
        toc_layout.finalize_toc(original, lambda _data: (pdf(), next(identities)))
    assert error.value.code == 'TOC_ENGINE_CHANGED'
    assert error.value.candidate_docx != original
    assert error.value.receipt['render_count'] == 2


def test_actual_pdf_producer_change_is_rejected_even_with_same_callback_identity(tmp_path):
    original = source_word(tmp_path)
    producers = iter(['layout-a', 'layout-b'])
    with pytest.raises(toc_layout.LayoutError) as error:
        toc_layout.finalize_toc(original, lambda _data: (pdf(producer=next(producers)), 'same-adapter'))
    assert error.value.code == 'TOC_ENGINE_CHANGED'


def test_pagination_cycle_stops_within_four_layout_conversions(tmp_path):
    original = source_word(tmp_path)
    calls = []
    def render(data):
        calls.append(data)
        return pdf(pages=[3 if len(calls) % 2 else 2]), 'same-engine'
    with pytest.raises(toc_layout.LayoutError) as error:
        toc_layout.finalize_toc(original, render)
    assert error.value.code in {'TOC_LAYOUT_CYCLE', 'TOC_LAYOUT_NOT_CONVERGED'}
    assert 2 <= len(calls) <= 4


def test_distinct_nonconverging_destinations_stop_after_exactly_four_conversions(tmp_path):
    original = source_word(tmp_path)
    calls = []
    def render(data):
        calls.append(data)
        return pdf(pages=[len(calls) + 1]), 'same-engine'
    with pytest.raises(toc_layout.LayoutError) as error:
        toc_layout.finalize_toc(original, render)
    assert error.value.code == 'TOC_LAYOUT_NOT_CONVERGED' and len(calls) == 4
    assert error.value.receipt['render_count'] == 4


def test_pdf_roman_labels_are_not_silently_replaced_with_physical_page_numbers(tmp_path):
    original = source_word(tmp_path)
    with pytest.raises(toc_layout.LayoutError) as error:
        toc_layout.finalize_toc(original, lambda _data: (pdf(roman_pages=True), 'same-engine'))
    assert error.value.code == 'TOC_PAGE_POLICY_UNSUPPORTED'


@pytest.mark.parametrize('damage', ['missing_bookmark', 'duplicate_bookmark', 'missing_cache', 'two_sections', 'number_reset'])
def test_broken_word_mapping_or_unsupported_number_policy_fails_before_render(tmp_path, damage):
    original = source_word(tmp_path)
    def change(root):
        if damage == 'missing_bookmark':
            bookmark = next(b for b in root.iter(W + 'bookmarkStart') if b.get(W + 'name') == bookmark_name('chapter-1'))
            bookmark.getparent().remove(bookmark)
        elif damage == 'duplicate_bookmark':
            from copy import deepcopy
            bookmark = next(b for b in root.iter(W + 'bookmarkStart') if b.get(W + 'name') == bookmark_name('chapter-1'))
            bookmark.getparent().append(deepcopy(bookmark))
        elif damage == 'missing_cache':
            field = next(f for f in root.iter(W + 'fldSimple') if 'PAGEREF' in f.get(W + 'instr', ''))
            field.getparent().remove(field)
        elif damage == 'two_sections':
            root.find(W + 'body').append(ET.Element(W + 'sectPr'))
        else:
            root.find('.//' + W + 'sectPr').append(ET.Element(W + 'pgNumType', {W + 'start': '3'}))
    broken = mutate_xml(original, change)
    def no_render(_data):
        pytest.fail('Invalid Word source must fail before sending a converter request.')
    with pytest.raises(toc_layout.LayoutError):
        toc_layout.finalize_toc(broken, no_render)


def test_document_without_generated_toc_is_unchanged_and_does_not_call_renderer():
    word = Document()
    word.add_paragraph('One original source statement with a signature: __________')
    buffer = BytesIO()
    word.save(buffer)
    original = buffer.getvalue()
    def no_render(_data):
        pytest.fail('No generated TOC requires no layout conversion.')
    result, receipt = toc_layout.finalize_toc(original, no_render)
    assert result == original and receipt['status'] == 'no_toc' and receipt['render_count'] == 0


@pytest.mark.parametrize('limit', ['MAX_DOCX_BYTES', 'MAX_PDF_BYTES', 'MAX_XML_BYTES', 'MAX_TOC_ENTRIES',
    'MAX_PDF_PAGES', 'MAX_ZIP_MEMBERS', 'MAX_PDF_LINKS'])
def test_explicit_size_or_complexity_limits_fail_closed(tmp_path, monkeypatch, limit):
    titles = ('Alpha', 'Beta') if limit in {'MAX_TOC_ENTRIES', 'MAX_PDF_LINKS'} else ('Alpha',)
    original = source_word(tmp_path, titles)
    monkeypatch.setattr(toc_layout, limit, 1)
    with pytest.raises(toc_layout.LayoutError):
        toc_layout.finalize_toc(original, lambda _data: (pdf(titles), 'same-engine'))


def test_renderer_failure_retains_candidate_without_reporting_untrusted_exception_text(tmp_path):
    original = source_word(tmp_path)
    def render(_data):
        raise RuntimeError('private bearer token and converter URL must not be reported')
    with pytest.raises(toc_layout.LayoutError) as error:
        toc_layout.finalize_toc(original, render)
    assert error.value.code == 'TOC_RENDER_FAILED' and error.value.candidate_docx == original
    assert 'private bearer' not in str(error.value) + str(error.value.receipt)


def test_non_word_xml_in_a_zip_is_not_a_legal_no_toc_document():
    value = BytesIO()
    with ZipFile(value, 'w') as archive:
        archive.writestr('word/document.xml', '<something-else/>')
    with pytest.raises(toc_layout.LayoutError) as error:
        toc_layout.finalize_toc(value.getvalue(), lambda _data: pytest.fail('Invalid DOCX must not render.'))
    assert error.value.code == 'TOC_DOCX_INVALID'
