from typing import TypeVar, Any, cast
from io import BytesIO
from zipfile import ZipFile

from docx import Document
from pypdf import PdfReader, PdfWriter
from pypdf.generic import NameObject, create_string_object

from docreader.integrations.artifacts import ArtifactStore
from docreader.tender.authoring_coverage import pending_outline_issues
from docreader.tender.authoring_export import export_authoring_document
from docreader.tender.directory_compile import compile_directory, documents_from_frozen, outline_from_compiled
from docreader.tender.evidence import EvidenceRepository, freeze_sources
from test_directory_compile import item, node, plan
from test_pdf_literal_freeze import pdf_bytes

_T = TypeVar("_T")


def _required(value: _T | None) -> _T:
    assert value is not None
    return value



async def test_pdf_embedded_docx_is_indexed_once_with_all_carriers_and_names(tmp_path, monkeypatch):
    docx = Document()
    docx.add_paragraph('Sign every copy of this attachment.')
    table = docx.add_table(rows=1, cols=2)
    table.cell(0, 0).text = 'Minimum'
    table.cell(0, 1).text = '3 copies'
    child = BytesIO()
    docx.save(child)
    writer = PdfWriter(clone_from=PdfReader(BytesIO(pdf_bytes(['Main PDF only.']))))
    entry = writer.add_attachment('Named exhibit', child.getvalue())
    entry.pdf_object[NameObject('/UF')] = create_string_object('exhibit.docx')
    writer.add_attachment('second-copy.docx', child.getvalue())
    writer.add_attachment('unreadable.docx', b'not a docx')
    output = BytesIO()
    writer.write(output)

    artifacts = ArtifactStore(tmp_path)
    original = artifacts.put_bytes(output.getvalue(), 'application/pdf')
    registration = {'documents': [{'document_id': 'pdf', 'document_version': '1',
        'role': 'main', 'ordinal': 0, 'issues': [], 'file_name': 'parent.pdf',
        'original_ref': original}], 'issues': []}
    frozen = artifacts.read_json(await freeze_sources(registration, artifacts, plan=False))
    parent, = frozen['documents']
    raw = artifacts.read_json(parent['raw_ref'])
    carriers = raw['embedded_sources']
    first = next(item for item in carriers.values() if 'Named exhibit' in item['names'])
    second = next(item for item in carriers.values() if item['names'] == ['second-copy.docx'])
    rejected = next(item for item in carriers.values() if item['names'] == ['unreadable.docx'])
    assert first['names'] == ['Named exhibit', 'exhibit.docx']
    assert first['original_ref'] == second['original_ref']
    assert first['frozen_ref'] == second['frozen_ref']
    assert artifacts.read_bytes(first['original_ref']) == child.getvalue()
    child_record, = artifacts.read_json(first['frozen_ref'])['documents']
    index = EvidenceRepository(artifacts).source_index(child_record['source_index_ref'])
    assert any('Sign every copy' in item['text'] for item in index.ranges)
    assert any(item['text_basis'] == 'grid_row' and item['text'] == 'Minimum | 3 copies'
               for item in index.ranges if 'text_basis' in item)
    assert not any(type(item['physical_locator'].get('page_ordinal')) is int for item in index.ranges)
    assert parent['status'] == 'partial'
    assert first['status'] == second['status'] == 'applicability_pending'
    assert rejected['status'] == 'unreadable'
    assert artifacts.read_bytes(rejected['original_ref']) == b'not a docx'

    documents = documents_from_frozen(artifacts, frozen)
    assert len(documents) == 1 and len(documents[0]['pages']) == 1
    assert 'Sign every copy' not in documents[0]['pages'][0]
    directory = plan([node('main', 0, 0, document_id='pdf')], [item('main')])
    compiled = compile_directory(directory, documents)
    snapshot = {'schema_version': 2, 'run_id': 'attachment-test', 'source_version': 1, 'domain_revision': 1}
    outline = outline_from_compiled(snapshot, compiled)
    pending = pending_outline_issues(outline)
    assert any('Named exhibit' in issue['detail'] and 'exhibit.docx' in issue['detail']
               and '适用' in issue['detail'] for issue in pending)
    assert any('unreadable.docx' in issue['detail'] and '读取' in issue['detail'] for issue in pending)

    exports = export_authoring_document(artifacts, outline, [])
    word = Document(BytesIO(artifacts.read_bytes(exports['docx_ref'])))
    with ZipFile(BytesIO(artifacts.read_bytes(exports['package_ref']))) as archive:
        notes = archive.read('提交要求与待办.md').decode()
    assert notes.count('Named exhibit') == 1
    assert sum('Named exhibit' in paragraph.text for paragraph in word.paragraphs) == 1
    assert 'Sign every copy' not in notes
    assert not exports['attachments']

    monkeypatch.setattr('docreader.tender.directory_compile._layout_pages', lambda *_: (None, 'layout unavailable'))
    fallback, = documents_from_frozen(artifacts, frozen)
    assert 'Main PDF only.' in '\n'.join(fallback['pages'])
    assert not any('Named exhibit' in page or 'unreadable.docx' in page for page in fallback['pages'])
    assert fallback['source_review_issues'] == documents[0]['source_review_issues']


async def test_images_on_readable_pdf_pages_are_still_in_source_review(tmp_path):
    writer = PdfWriter(clone_from=PdfReader(BytesIO(pdf_bytes([
        'This page has a readable text layer and a separate diagram.']))))
    from pypdf.generic import DecodedStreamObject, DictionaryObject, NumberObject
    picture = DecodedStreamObject()
    picture.update({NameObject('/Type'): NameObject('/XObject'), NameObject('/Subtype'): NameObject('/Image'),
                    NameObject('/Width'): NumberObject(99), NameObject('/Height'): NumberObject(75),
                    NameObject('/ColorSpace'): NameObject('/DeviceRGB'), NameObject('/BitsPerComponent'): NumberObject(8)})
    picture.set_data(bytes((100, 17, 40)) * (99 * 75))
    page = writer.pages[0]
    cast(Any, page)['/Resources'][NameObject('/XObject')] = DictionaryObject({NameObject('/Im0'): writer._add_object(picture)})
    content = page.get_contents()
    _required(content).set_data(_required(content).get_data() + b'\nq 99 0 0 75 100 250 cm /Im0 Do Q\n')
    page.replace_contents(content)
    output = BytesIO()
    writer.write(output)
    artifacts = ArtifactStore(tmp_path)
    original = artifacts.put_bytes(output.getvalue(), 'application/pdf')
    registration = {'documents': [{'document_id': 'pdf', 'document_version': '1', 'role': 'main',
        'ordinal': 0, 'issues': [], 'file_name': 'image.pdf', 'original_ref': original}], 'issues': []}
    frozen = artifacts.read_json(await freeze_sources(registration, artifacts, plan=False))
    assert any(problem['code'] == 'ocr_provider_unavailable' for problem in frozen['issues'])
    documents = documents_from_frozen(artifacts, frozen)
    assert not documents[0]['unread_pages']
    compiled = compile_directory(plan([node('main', 0, 0, document_id='pdf')], [item('main')]), documents)
    outline = outline_from_compiled({'schema_version': 2, 'run_id': 'image-test', 'source_version': 1,
                                     'domain_revision': 1}, compiled)
    pending = pending_outline_issues(outline)
    assert any(issue['code'] == 'outline_directory_image_unconfirmed' and 'image.pdf' in issue['detail']
               and '第1页' in issue['detail'] for issue in pending)
