from io import BytesIO
import json
from zipfile import ZipFile

from docx import Document
from pypdf import PdfReader, PdfWriter

from docreader.integrations.artifacts import ArtifactStore
from docreader.tender.authoring_export import export_authoring_document
from docreader.tender.authoring_pipeline import _request_directory_details
from docreader.tender.directory_compile import compile_directory, documents_from_frozen, outline_from_compiled
from docreader.tender.evidence import EvidenceRepository, freeze_sources
from docreader.tender.native_requirement_sources import CLASSIFICATION_OPERATION, CONTRACT, REQUIREMENT_OPERATION
from docreader.tender.source_body import load_source_body
from docreader.tender.word_navigation import bookmark_name
from test_directory_compile import fixture_titles, item, node, plan
from test_native_requirement_sources import budget
from test_source_presentation import pdf_bytes


async def test_frozen_two_page_literal_requirement_reaches_word_once(tmp_path):
    title = 'Technical response'
    start = '# Form 7 must be signed'
    tail = '; include the original date.'
    neighbor = 'Neighbor reference is evaluator guidance only.'
    writer = PdfWriter()
    for lines in ([title, start], [tail, neighbor]):
        writer.append(PdfReader(BytesIO(pdf_bytes(lines))))
    pdf = BytesIO()
    writer.write(pdf)

    artifacts = ArtifactStore(tmp_path)
    source = {'document_id': 'pdf', 'document_version': '1', 'role': 'main', 'ordinal': 0,
              'file_name': 'original.pdf', 'issues': [], 'original_ref': artifacts.put_bytes(pdf.getvalue(), 'application/pdf')}
    frozen_ref = await freeze_sources({'documents': [source]}, artifacts, plan=False)
    snapshot = {'schema_version': 2, 'run_id': 'native-two-page', 'source_version': 1,
                'domain_revision': 1, 'frozen_ref': frozen_ref}
    documents = documents_from_frozen(artifacts, artifacts.read_json(frozen_ref))
    heading_line = documents[0]['pages'][0].splitlines().index(title)
    chapter = node('technical', 0, heading_line, document_id='pdf')
    chapter['body_windows'] = [{'document_id': 'pdf', 'page': 0, 'start_line': heading_line, 'end_line': heading_line}]
    compiled = compile_directory(plan([chapter], [item('technical')]), documents)
    requests = []
    expected = start + tail

    async def execute(task, request, validate):
        requests.append(str(task))
        value = json.loads(request['messages'][1]['content'][0]['text'])
        if task == 'directory_material':
            return validate({'assignments': [], 'chapter_titles': fixture_titles(value)})
        if task == 'directory_requirement':
            assert value['operation'] == REQUIREMENT_OPERATION
            atoms = {text: ref for row in value['source_scope'] for ref, text in row['lines']}
            certified = {text for row in value['source_scope'] if 'literal_body_line' in row for _, text in row['lines']}
            assert {start, tail, neighbor} <= certified
            return validate({'assignments': [], 'native_dispositions': [
                {'refs': [atoms[title]], 'kind': 'navigation'},
                {'refs': [atoms[start], atoms[tail]], 'kind': 'requirement'},
                {'refs': [atoms[neighbor]], 'kind': 'reference'},
            ], 'native_assignments': [{'body_refs': [atoms[start], atoms[tail]], 'source_join': 'continuation',
                                       'scope': 'unassigned', 'chapter_ids': []}]})
        assert task == 'directory_classification' and value['operation'] == CLASSIFICATION_OPERATION
        case, = value['requirement_cases']
        assert case['body'] == expected
        return validate({'items': [], 'basis_groups': [], 'requirement_adjudications': [
            {'item_ids': [case['id']], 'scope': 'chapter', 'chapter_ids': ['c1'], 'basis_id': None,
             'source_join': 'continuation'},
        ]})

    await _request_directory_details(compiled, documents, budget(), execute, snapshot,
                                     artifacts=artifacts, native_requirement_contract=CONTRACT)
    assert requests == ['directory_material', 'directory_requirement', 'directory_classification']
    outline = outline_from_compiled(snapshot, compiled, artifacts=artifacts)
    target, = [target for chapter in outline['chapters'] for target in chapter['targets']
               if target.get('source_body_ref')]
    assert target['manual_fill'] is True
    assert load_source_body(artifacts, snapshot, target) == expected
    binding = artifacts.read_json(target['source_body_ref'])
    repository = EvidenceRepository(artifacts)
    locations = []
    for part in binding['parts']:
        excerpt = part['source_excerpt']
        index = repository.source_index(excerpt['source_ref']['source_index_ref'])
        selection, = excerpt['selectors']
        material = index.materialize(selection)
        locations.append((material['text_basis'], material['physical_locator']['page_ordinal'], material['text']))
    assert locations == [('pdf_literal', 0, start), ('pdf_literal', 1, tail)]

    delivery = export_authoring_document(artifacts, outline, [])
    word = artifacts.read_bytes(delivery['docx_ref'])
    docx = Document(BytesIO(word))
    binder = bookmark_name(target['target_id'], 'target')
    anchored, = [paragraph for paragraph in docx.paragraphs
                 if binder in paragraph._p.xpath('./w:bookmarkStart/@w:name')]
    assert anchored.text == expected
    assert anchored.style.name == 'Normal'
    assert [paragraph.text for paragraph in docx.paragraphs].count(expected) == 1
    assert neighbor not in docx.element.body.xml
    with ZipFile(BytesIO(artifacts.read_bytes(delivery['package_ref']))) as package:
        assert package.read('投标文件.docx') == word
