"""Continuation stays in the chosen chapter. Images and embedded files stay pending."""

from io import BytesIO

from docx import Document
from pypdf import PdfReader, PdfWriter
from pypdf.generic import NameObject, create_string_object

from docreader.integrations.artifacts import ArtifactStore
from docreader.tender.directory_compile import compile_directory, documents_from_frozen, group_unread_images
from docreader.tender.evidence import freeze_sources
from test_directory_compile import item, node, plan
from test_pdf_literal_freeze import pdf_bytes


def _part(unit_id):
    return {'source_ref': {'source_index_ref': {'sha256': 'a' * 64}, 'unit_id': unit_id}, 'selectors': []}


def _documents():
    return [{
        'document_id': 'tender', 'role': 'main',
        'pages': ['报价表\n列甲\t列乙\n1\t2\n', '列甲\t列乙\n3\t4\n', '未选的须知条款\n'],
        'line_units': [[None, 'table-a', 'table-a'], ['table-b', 'table-b'], [None]],
        'template_parts': {'table-a': _part('table-a'), 'table-b': _part('table-b')},
        'continuations': [{
            'kind': 'ruled_table', 'before_page': 0, 'before_table_id': 'table-a',
            'after_page': 1, 'after_table_id': 'table-b', 'repeated_header_rows': 1,
        }],
    }]


def _ids(chapter):
    return [(part.get('source_ref') or {}).get('unit_id') for part in chapter.get('template_parts') or []]


def test_continuation_joins_only_the_chapter_that_already_holds_one_grid():
    compiled = compile_directory(plan(
        [node('quote', 0, 0), node('later', 1, 0), node('notice', 2, 0)],
        [item('quote')], source_only=['later', 'notice'],
    ), _documents())
    quote = next(chapter for chapter in compiled['chapters'] if chapter['id'] == 'quote')
    assert _ids(quote) == ['table-a', 'table-b']
    assert quote['template_parts'][1].get('header_row_count') == 1
    assert 'header_row_count' not in quote['template_parts'][0]
    assert '3\t4' not in quote['text']
    assert '未选的须知条款' not in quote['text']
    assert all('未选的须知条款' not in chapter['text'] for chapter in compiled['chapters'])


def test_selecting_only_the_later_table_keeps_source_page_order():
    documents = [{
        'document_id': 'tender', 'role': 'main',
        'pages': ['列甲\t列乙\n1\t2\n', '列甲\t列乙\n3\t4\n'],
        'line_units': [['table-a', 'table-a'], ['table-b', 'table-b']],
        'template_parts': {'table-a': _part('table-a'), 'table-b': _part('table-b')},
        'continuations': [{
            'kind': 'ruled_table', 'before_page': 0, 'before_table_id': 'table-a',
            'after_page': 1, 'after_table_id': 'table-b', 'repeated_header_rows': 1,
        }],
    }]
    compiled = compile_directory(plan([node('later', 1, 0)], [item('later')]), documents)
    later = compiled['chapters'][0]
    assert _ids(later) == ['table-a', 'table-b']
    assert [segment['part']['source_ref']['unit_id'] for segment in later['template_segments']] == ['table-a', 'table-b']
    assert '1\t2' not in later['text']


def test_same_unit_id_in_another_document_is_not_pulled_into_the_chapter():
    documents = [
        {'document_id': 'tender', 'role': 'main', 'pages': ['续页\n列甲\t列乙\n3\t4\n'],
         'line_units': [[None, 'table-b', 'table-b']],
         'template_parts': {'table-b': _part('table-b')},
         'continuations': [{'kind': 'ruled_table', 'before_page': 0, 'before_table_id': 'table-a',
                            'after_page': 0, 'after_table_id': 'table-b', 'repeated_header_rows': 0}]},
        {'document_id': 'other', 'role': 'attachment', 'pages': ['列甲\t列乙\n'],
         'line_units': [['table-a']], 'template_parts': {'table-a': _part('table-a')}, 'continuations': []},
    ]
    compiled = compile_directory(plan(
        [node('later', 0, 0), node('else', 0, 0, document_id='other')],
        [item('later')], source_only=['else']), documents)
    assert _ids(compiled['chapters'][0]) == ['table-b']


def test_continuation_does_not_move_a_grid_into_another_chapter():
    compiled = compile_directory(plan(
        [node('quote', 0, 0), node('later', 1, 0), node('notice', 2, 0)],
        [item('quote'), item('later')], source_only=['notice'],
    ), _documents())
    quote = next(chapter for chapter in compiled['chapters'] if chapter['id'] == 'quote')
    later = next(chapter for chapter in compiled['chapters'] if chapter['id'] == 'later')
    assert _ids(quote) == ['table-a']
    assert _ids(later) == ['table-b']
    assert '未选的须知条款' not in quote['text'] and '未选的须知条款' not in later['text']


def test_duplicate_image_bytes_share_one_pending_item():
    raw = {'units': [
        {'unit_id': 'image-a', 'kind': 'image_region', 'locator': {'image_key': 'k', 'page_ordinal': 1}},
        {'unit_id': 'image-b', 'kind': 'image_region', 'locator': {'image_key': 'k', 'page_ordinal': 4}},
    ], 'images': {'k': {'sha256': 'b' * 64}}, 'ocr': {}}
    group, = group_unread_images(raw)
    assert group == {'identity': 'b' * 64, 'pages': [1, 4], 'count': 2}


async def test_source_only_freeze_keeps_embedded_docx_off_the_directory_and_does_not_request_ocr(tmp_path):
    child = BytesIO()
    document = Document()
    document.add_paragraph('Embedded tender copy')
    document.save(child)
    writer = PdfWriter(clone_from=PdfReader(BytesIO(pdf_bytes(['Main PDF only.']))))
    entry = writer.add_attachment('named.docx', child.getvalue())
    entry.pdf_object[NameObject('/UF')] = create_string_object('named.docx')
    output = BytesIO()
    writer.write(output)
    artifacts = ArtifactStore(tmp_path)
    original = artifacts.put_bytes(output.getvalue(), 'application/pdf')
    registration = {'documents': [{'document_id': 'pdf', 'document_version': '1', 'role': 'main',
        'ordinal': 0, 'issues': [], 'file_name': 'parent.pdf', 'original_ref': original}], 'issues': []}
    frozen_ref = await freeze_sources(registration, artifacts, ocr=None, plan=False)
    frozen = artifacts.read_json(frozen_ref)
    assert frozen['stats']['ocr_request_count'] == 0
    documents = documents_from_frozen(artifacts, frozen)
    assert [item['document_id'] for item in documents] == ['pdf']
    assert 'Embedded tender copy' not in '\n'.join(documents[0]['pages'])
    raw = artifacts.read_json(frozen['documents'][0]['raw_ref'])
    attachment = next(iter(raw['embedded_sources'].values()))
    assert attachment['status'] == 'applicability_pending'
    assert 'named.docx' in attachment['names']
    compiled = compile_directory(plan([node('main', 0, 0, document_id='pdf')], [item('main')]), documents)
    assert all('Embedded tender copy' not in chapter['text'] for chapter in compiled['chapters'])
