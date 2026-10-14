"""Real-parser contract tests; OCR text is deterministic and not semantic acceptance."""
from __future__ import annotations

import io
from dataclasses import replace

import pytest
from docx import Document
from docx.oxml import OxmlElement
from openpyxl import Workbook
from openpyxl.drawing.image import Image as ExcelImage
from PIL import Image
from pypdf import PdfWriter
from pypdf.generic import DecodedStreamObject, DictionaryObject, NameObject, NumberObject

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.budget import RequestBudget
from docreader.tender.evidence import EvidenceRepository, freeze_sources, plan_sources, register_manifest


def save_office(document):
    output = io.BytesIO()
    document.save(output)
    return output.getvalue()


def png():
    output = io.BytesIO()
    Image.new("RGB", (12, 8), "blue").save(output, format="PNG")
    return output.getvalue()


def pdf(*, tiny_image=False):
    writer = PdfWriter()
    page = writer.add_blank_page(width=300, height=300)
    font = writer._add_object(DictionaryObject({NameObject('/Type'): NameObject('/Font'),
        NameObject('/Subtype'): NameObject('/Type1'), NameObject('/BaseFont'): NameObject('/Helvetica')}))
    resources = DictionaryObject({NameObject('/Font'): DictionaryObject({NameObject('/F1'): font})})
    commands = b'BT /F1 12 Tf 10 270 Td (Bidder must submit proof within 30 days.) Tj ET'
    if tiny_image:
        pixels = Image.new('RGB', (12, 8), 'blue')
        image = DecodedStreamObject()
        image.set_data(pixels.tobytes())
        image.update({NameObject('/Type'): NameObject('/XObject'), NameObject('/Subtype'): NameObject('/Image'),
                      NameObject('/Width'): NumberObject(12), NameObject('/Height'): NumberObject(8),
                      NameObject('/ColorSpace'): NameObject('/DeviceRGB'), NameObject('/BitsPerComponent'): NumberObject(8)})
        resources[NameObject('/XObject')] = DictionaryObject({NameObject('/Im0'): writer._add_object(image.flate_encode())})
        # Two actual occurrences sharing one payload; both must have source identity.
        commands += b' q 12 0 0 8 10 10 cm /Im0 Do Q q 12 0 0 8 40 10 cm /Im0 Do Q'
    page[NameObject('/Resources')] = resources
    stream = DecodedStreamObject()
    stream.set_data(commands)
    page[NameObject('/Contents')] = writer._add_object(stream)
    output = io.BytesIO()
    writer.write(output)
    return output.getvalue()


def registration(tmp_path, artifacts, documents):
    entries = []
    for name, content in documents:
        path = tmp_path / name
        path.write_bytes(content)
        entries.append({"document_id": name, "document_version": "1", "role": "main", "path": str(path)})
    return register_manifest({"documents": entries}, artifacts)


@pytest.fixture
def artifacts(tmp_path):
    return ArtifactStore(tmp_path / "artifacts")


@pytest.fixture(autouse=True)
def single_pdf_renderer(monkeypatch):
    from docreader.config import CONFIG
    monkeypatch.setattr("docreader.parser.pdf_parser.CONFIG", replace(CONFIG, pdf_render_parallelism=1,
                       pdf_render_dpi=72, pdf_render_max_edge=300))


def test_registration_copies_inputs_and_reports_all_document_problems(tmp_path, artifacts):
    first = tmp_path / 'main.pdf'
    first.write_bytes(pdf())
    record = register_manifest({"documents": [
        {"document_id": "main", "document_version": 1, "path": str(first)},
        {"document_id": "copy", "document_version": 1, "path": str(first)},
        {"document_id": "main", "document_version": 1, "path": str(tmp_path / 'missing')},
        {"document_id": "directory", "document_version": 1, "path": str(tmp_path)},
    ], "expected_document_ids": ["amendment"]}, artifacts)
    codes = {entry["code"] for entry in record["issues"]}
    assert {"duplicate_document_content", "duplicate_document_id", "missing_document",
            "unreadable_document", "missing_expected_document"} <= codes
    first.unlink()
    assert artifacts.read_bytes(record["documents"][0]["original_ref"]) == pdf()
    assert all('path' not in document for document in record['documents'])
    replay = register_manifest(artifacts.read_json(artifacts.put_json(record)), artifacts)
    assert replay == record


def test_registration_accepts_submit_ref_aliases(tmp_path, artifacts):
    ref = artifacts.put_bytes(pdf())
    output = register_manifest({"documents": [{"document_id": "a", "document_version": "v1", "role": "attachment",
                                                "file_ref": ref, "filename": "a.pdf"}]}, artifacts)
    assert output['documents'][0]['original_ref'] == ref
    assert output['documents'][0]['file_name'] == 'a.pdf'
    assert output['issues'] == []


async def test_text_pdf_never_renders_or_calls_ocr(tmp_path, artifacts, monkeypatch):
    def forbidden(*args, **kwargs):
        raise AssertionError('native text page must not render')
    monkeypatch.setattr('docreader.parser.pdf_parser._render_scanned_pages', forbidden)
    monkeypatch.setattr('docreader.parser.pdf_parser.FORCE_SCANNED_PDF', True)
    registered = registration(tmp_path, artifacts, [('native.pdf', pdf())])
    ref = await freeze_sources(registered, artifacts, forbidden)
    index = artifacts.read_json(ref)
    assert index['issues'] == []
    assert index['stats']['ocr_request_count'] == 0
    assert '30 days' in str(list(EvidenceRepository(artifacts).iter_packs(ref)))


async def test_pdf_tiny_shared_images_keep_each_occurrence_without_page_render(tmp_path, artifacts, monkeypatch):
    calls = []
    async def ocr(ref, source):
        calls.append((ref, source))
        assert artifacts.read_bytes(ref)
        return {'text': 'Stamped proof required', 'status': 'complete', 'issues': []}
    def forbidden(*args, **kwargs):
        raise AssertionError('text page must not render')
    monkeypatch.setattr('docreader.parser.pdf_parser._render_scanned_pages', forbidden)
    registered = registration(tmp_path, artifacts, [('native.pdf', pdf(tiny_image=True))])
    ref = await freeze_sources(registered, artifacts, ocr)
    index = artifacts.read_json(ref)
    assert index['issues'] == []
    assert len(calls) == 2
    assert len({source['unit_id'] for _, source in calls}) == 2
    assert len({item['sha256'] for item, _ in calls}) == 1
    assert all(source['locator']['left'] is not None for _, source in calls)


async def test_mixed_native_pdf_only_scanned_page_uses_ocr(tmp_path, artifacts):
    from docreader.scripts.generate_source_contract_fixtures import mixed_pdf
    calls = []
    async def ocr(ref, source):
        calls.append(source['locator']['page_ordinal'])
        return {'text': 'Scanned evidence 10 / 20', 'status': 'complete', 'issues': []}
    ref = await freeze_sources(registration(tmp_path, artifacts, [('mixed.pdf', mixed_pdf())]), artifacts, ocr)
    assert calls == [2]
    packs = list(EvidenceRepository(artifacts).iter_packs(ref))
    assert any(r.get('non_requirement_reason') == 'blank_page' for pack in packs for r in pack['body'])
    assert artifacts.read_json(ref)['issues'] == []


async def test_xlsx_images_anchors_blank_rows_and_partial_fields_survive(tmp_path, artifacts):
    workbook = Workbook()
    for sheet in [workbook.active, workbook.create_sheet('Other')]:
        sheet.append(['Field', 'Response'])
        sheet.append(['Name', None])
        sheet.append([None, None])
        sheet.append(['Value', 'required'])
        sheet.add_image(ExcelImage(io.BytesIO(png())), 'C2')
    calls = []
    async def ocr(ref, source):
        calls.append(source)
        return {'text': 'Sign here', 'status': 'complete', 'issues': []}
    frozen = await freeze_sources(registration(tmp_path, artifacts, [('forms.xlsx', save_office(workbook))]), artifacts, ocr)
    index = artifacts.read_json(frozen)
    assert index['issues'] == []
    assert len(calls) == 2
    assert {source['image_anchor']['sheet_ordinal'] for source in calls} == {0, 1}
    assert all(source['image_anchor']['from']['row'] == 2 and source['image_anchor']['from']['column'] == 3 for source in calls)
    packs = list(EvidenceRepository(artifacts).iter_packs(frozen))
    rows = [entry for pack in packs for entry in pack['body']]
    names = [entry for entry in rows if entry['text'].startswith('Name')]
    assert len(names) == 2 and all(entry['text'] == 'Name | ' for entry in names)
    blank = [entry for entry in rows if entry.get('empty')]
    assert len(blank) == 2 and all(entry['text'] == '' for entry in blank)
    assert sum(len(pack['compacted_blank_ranges']) for pack in packs) == 2
    raw = artifacts.read_json(index['documents'][0]['raw_ref'])
    grids = [entry['grid'] for entry in raw['units'] if entry.get('grid')]
    assert len(grids) == 2 and all(grid['row_count'] == 4 for grid in grids)
    assert len({entry['range_id'] for entry in rows}) == len(rows)


async def test_docx_raw_grid_merge_and_cross_pack_context(tmp_path, artifacts):
    document = Document()
    document.add_heading('第一章 条件', 1)
    document.add_paragraph('本章适用于联合体，必须同时提交全部成员资料。')
    document.add_heading('第一节 响应', 2)
    document.add_paragraph('以下内容属于联合体响应。')
    for i in range(8):
        document.add_paragraph(f'必须提供第{i}项相关证明。' * 10)
    table = document.add_table(rows=8, cols=2)
    table.cell(0, 0).text = '证明字段'
    table.cell(0, 1).text = '响应内容'
    table.rows[0]._tr.get_or_add_trPr().append(OxmlElement('w:tblHeader'))
    table.cell(1, 0).merge(table.cell(1, 1)).text = '合并说明'
    for row in range(2, 8):
        table.cell(row, 0).text = f'材料{row}'
        table.cell(row, 1).text = '必须盖章'
    document.add_heading('第二章 结束', 1)
    document.add_paragraph('必须提交独立附件。')
    ref = await freeze_sources(registration(tmp_path, artifacts, [('source.docx', save_office(document))]),
                               artifacts, budget=RequestBudget(context_tokens=7000, output_reserve=0))
    index = artifacts.read_json(ref)
    packs = list(EvidenceRepository(artifacts).iter_packs(ref))
    assert len(packs) > 2
    body_ids = [entry['range_id'] for pack in packs for entry in pack['body']]
    assert len(body_ids) == len(set(body_ids)) == index['range_count']
    assert any('本章适用于联合体' in entry['text'] for pack in packs[1:] for entry in pack['supporting'])
    assert any(entry.get('reason') == 'header' for pack in packs for entry in pack['supporting'])
    assert all(not ({entry['range_id'] for entry in pack['body']} &
                    {entry['range_id'] for entry in pack['supporting']}) for pack in packs)
    raw = artifacts.read_json(index['documents'][0]['raw_ref'])
    grid = next(entry['grid'] for entry in raw['units'] if entry.get('grid'))
    assert any(cell['col_span'] == 2 for cell in grid['cells'])
    assert all(entry['physical_path'] for entry in raw['units'])


async def test_failed_document_and_missing_ocr_do_not_discard_successful_ranges(tmp_path, artifacts):
    registered = registration(tmp_path, artifacts, [('good.pdf', pdf()), ('broken.docx', b'broken'), ('scan.png', png()), ('unknown.bin', b'unknown')])
    ref = await freeze_sources(registered, artifacts)
    index = artifacts.read_json(ref)
    assert {'document_parse_failed', 'ocr_provider_unavailable', 'unsupported_document'} <= {problem['code'] for problem in index['issues']}
    assert any(document['document_id'] == 'good.pdf' and document['status'] == 'complete' for document in index['documents'])
    assert any('30 days' in entry['text'] for pack in EvidenceRepository(artifacts).iter_packs(ref) for entry in pack['body'])


async def test_repository_reuses_frozen_refs_without_native_parse(tmp_path, artifacts, monkeypatch):
    ref = await freeze_sources(registration(tmp_path, artifacts, [('native.pdf', pdf())]), artifacts)
    monkeypatch.setattr('docreader.tender.evidence.freeze.parse_native', lambda *args: pytest.fail('must reuse frozen source'))
    repository = EvidenceRepository(artifacts)
    first = list(repository.iter_packs(ref))
    assert first == list(repository.iter_packs(ref))
    index = artifacts.read_json(ref)
    assert repository.load_pack(index['packs'][0]) == first[0]


async def test_small_sections_merge_while_ranges_keep_their_identities(tmp_path, artifacts):
    document = Document()
    for i in range(3):
        document.add_heading(f'Chapter {i}', 1)
        document.add_paragraph(f'Submit document {i}.')
    ref = await freeze_sources(registration(tmp_path, artifacts, [('small.docx', save_office(document))]), artifacts)
    packs = list(EvidenceRepository(artifacts).iter_packs(ref))
    assert len(packs) == 1
    assert len({entry['section_id'] for entry in packs[0]['body']}) == 3


async def test_formula_display_incomplete_is_not_silent_success(tmp_path, artifacts):
    workbook = Workbook()
    workbook.active.append(['Total', '=SUM(B2:B3)'])
    frozen = await freeze_sources(registration(tmp_path, artifacts, [('formula.xlsx', save_office(workbook))]), artifacts)
    index = artifacts.read_json(frozen)
    assert any(problem['code'] == 'spreadsheet_display_incomplete' and problem['detail'] == 'formula_cached_value_missing'
               for problem in index['issues'])
    assert index['documents'][0]['status'] == 'partial'
    rows = [entry for pack in EvidenceRepository(artifacts).iter_packs(frozen) for entry in pack['body']
            if entry.get('text_basis') == 'spreadsheet_row']
    formula = next(cell for row in rows for cell in row['cells'] if cell.get('formula'))
    assert formula['formula'] == '=SUM(B2:B3)'
    assert formula['formula_references'] == ['B2:B3']
    assert formula['display_complete'] is False
    assert formula['display_incomplete_reason'] == 'formula_cached_value_missing'


async def test_blank_spreadsheet_row_with_actual_image_anchor_is_not_compacted(tmp_path, artifacts):
    workbook = Workbook()
    sheet = workbook.active
    sheet.append(['Field', 'Response'])
    sheet.append([None, None])
    sheet.append([None, None])
    sheet.append(['Final', 'Value'])
    sheet.add_image(ExcelImage(io.BytesIO(png())), 'B2')
    frozen = await freeze_sources(registration(tmp_path, artifacts, [('anchored.xlsx', save_office(workbook))]), artifacts)
    packs = list(EvidenceRepository(artifacts).iter_packs(frozen))
    rows = {entry['row_start']: entry for pack in packs for entry in pack['body'] if 'row_start' in entry}
    assert rows[2]['empty'] is False and rows[3]['empty'] is True
    assert rows[2]['image_unit_ids'] == ['sheet:0:image:0']
    image = next(entry for pack in packs for entry in pack['body'] if entry['kind'] == 'image_region')
    assert image['unit_id'] == rows[2]['image_unit_ids'][0]
    assert image['ocr_status'] == 'unresolved'


async def test_blank_docx_table_row_with_image_parent_is_not_compacted(tmp_path, artifacts):
    document = Document()
    table = document.add_table(rows=3, cols=2)
    table.cell(0, 0).text = 'Proof'
    table.cell(1, 1).paragraphs[0].add_run().add_picture(io.BytesIO(png()))
    frozen = await freeze_sources(registration(tmp_path, artifacts, [('proof.docx', save_office(document))]), artifacts)
    packs = list(EvidenceRepository(artifacts).iter_packs(frozen))
    rows = {entry['row_start']: entry for pack in packs for entry in pack['body'] if 'row_start' in entry}
    assert rows[1]['empty'] is False and rows[2]['empty'] is True
    image = next(entry for pack in packs for entry in pack['body'] if entry['kind'] == 'image_region')
    assert rows[1]['image_unit_ids'] == [image['unit_id']]
    assert image['locator']['compound_parent']['row_ordinal'] == 1
    assert image['ocr_status'] == 'unresolved'


def test_formula_with_blank_display_is_retained_from_canonical_spreadsheet_cells():
    from copy import deepcopy
    from docreader.tender.evidence.references import SourceIndex, selector
    locator = {'locator_kind': 'spreadsheet', 'sheet_ordinal': 0,
               'region': {'start_row': 2, 'end_row': 2, 'start_column': 1, 'end_column': 2},
               'cell_refs': ['sheet:0:cell:2:2']}
    raw = {'units': [{'unit_id': 'row:2', 'kind': 'table_row', 'locator': locator}],
           'source_contract': {'units': []},
           'spreadsheet_cells': {'sheet:0:cell:2:2': {'row': 2, 'column': 2, 'text': '',
                                 'formula': '=IF(A2="","",A2)', 'formula_references': ['A2'],
                                 'display_complete': True}}}
    snapshot = deepcopy(raw)
    index = SourceIndex({'document_id': 'book', 'document_version': '1', 'original_ref': {'sha256': '1' * 64}}, raw)
    row = index.materialize(selector(index.ranges[0]))
    assert row['empty'] is False and row['text'] == ' | '
    assert row['column_count'] == 2
    assert row['cells'][0]['formula'] == '=IF(A2="","",A2)'
    assert row['cells'][0]['column'] == 1
    assert raw == snapshot


async def test_embedded_pdf_attachment_is_explicitly_unresolved(tmp_path, artifacts):
    from pypdf import PdfReader
    writer = PdfWriter()
    writer.append(PdfReader(io.BytesIO(pdf())))
    writer.add_attachment('terms.txt', b'Additional conditions apply.')
    output = io.BytesIO()
    writer.write(output)
    frozen = await freeze_sources(registration(tmp_path, artifacts, [('attachment.pdf', output.getvalue())]), artifacts)
    index = artifacts.read_json(frozen)
    assert any(problem['code'] == 'attachment_content_not_extracted' for problem in index['issues'])
    raw = artifacts.read_json(index['documents'][0]['raw_ref'])
    assert any(unit['locator'].get('part_name') == 'terms.txt' for unit in raw['units'])


async def test_storage_corruption_remains_technical_failure(tmp_path, artifacts):
    from docreader.integrations.artifacts import ArtifactIntegrityError
    registered = registration(tmp_path, artifacts, [('source.pdf', pdf())])
    path = artifacts.path(registered['documents'][0]['original_ref'])
    path.chmod(0o644)
    path.write_bytes(b'corrupt')
    with pytest.raises(ArtifactIntegrityError):
        await freeze_sources(registered, artifacts)


async def test_changed_original_gets_new_range_identity_even_when_manifest_version_is_reused(tmp_path, artifacts):
    initial = registration(tmp_path, artifacts, [('source.pdf', pdf())])
    frozen = await freeze_sources(initial, artifacts)
    changed = registration(tmp_path, artifacts, [('source.pdf', pdf().replace(b'30 days', b'60 days'))])
    revised = await freeze_sources(changed, artifacts)
    repository = EvidenceRepository(artifacts)
    original_ranges = [entry for pack in repository.iter_packs(frozen) for entry in pack['body']]
    revised_ranges = [entry for pack in repository.iter_packs(revised) for entry in pack['body']]
    assert not {entry['range_id'] for entry in original_ranges} & {entry['range_id'] for entry in revised_ranges}
    assert any('30 days' in entry['text'] for entry in original_ranges)
    assert any('60 days' in entry['text'] for entry in revised_ranges)


async def test_offset_adjacent_excel_tables_each_keep_their_own_headers(tmp_path, artifacts):
    from openpyxl.worksheet.table import Table
    workbook = Workbook()
    sheet = workbook.active
    sheet.append(['First field', 'First value'])
    for row in range(2, 8):
        sheet.cell(row, 1).value = f'First {row}'
        sheet.cell(row, 2).value = 'Required'
    sheet['D3'], sheet['E3'] = 'Second field', 'Second value'
    for row in range(4, 8):
        sheet.cell(row, 4).value = f'Second {row}'
        sheet.cell(row, 5).value = 'Required'
    sheet.add_table(Table(displayName='FirstTable', ref='A1:B7'))
    sheet.add_table(Table(displayName='SecondTable', ref='D3:E7'))
    frozen = await freeze_sources(registration(tmp_path, artifacts, [('tables.xlsx', save_office(workbook))]),
                                   artifacts, budget=RequestBudget(context_tokens=7000, output_reserve=0))
    packs = list(EvidenceRepository(artifacts).iter_packs(frozen))
    later = next(pack for pack in packs if any('Second 7' in entry['text'] for entry in pack['body']))
    assert any('Second field' in entry['text'] and entry['reason'] == 'header' for entry in later['supporting'])
    assert any('First field' in entry['text'] and entry['reason'] == 'header' for entry in later['supporting'])


async def test_cumulative_lot_and_consortium_conditions_survive_nested_packages(tmp_path, artifacts):
    document = Document()
    document.add_heading('第一章 适用范围', 1)
    document.add_paragraph('仅第一标段适用。')
    document.add_paragraph('联合体还需提供全部成员共同签章的授权书。')
    document.add_heading('第一节 证明材料', 2)
    for index in range(12):
        document.add_paragraph(f'必须提交第{index}项证明材料，包含有效期和签发人。' * 6)
    document.add_heading('第二章 独立范围', 1)
    document.add_paragraph('本章内容独立处理。')
    frozen = await freeze_sources(registration(tmp_path, artifacts, [('scopes.docx', save_office(document))]),
                                   artifacts, budget=RequestBudget(context_tokens=7000, output_reserve=0))
    packs = list(EvidenceRepository(artifacts).iter_packs(frozen))
    later = next(pack for pack in packs if any('第11项证明' in entry['text'] for entry in pack['body']))
    conditions = [entry for entry in later['supporting'] if entry['reason'] == 'condition']
    assert {entry['text'] for entry in conditions} >= {'仅第一标段适用。', '联合体还需提供全部成员共同签章的授权书。'}
    assert all(entry['scope_binding']['status'] == 'bound' for entry in conditions)
    assert all(len(entry['text']) < 100 for entry in conditions)
    body_by_id = {entry['range_id']: entry for pack in packs for entry in pack['body']}
    for entry in conditions:
        parent = body_by_id[entry['parent_range_id']]
        assert entry['text'] == parent['text'][entry['start'] - parent['start']:entry['end'] - parent['start']]
        assert entry['range_id'] not in body_by_id
    separate = next(pack for pack in packs if any('本章内容独立处理' in entry['text'] for entry in pack['body']))
    independent = {entry['range_id'] for entry in separate['body'] if '本章内容独立处理' in entry['text']}
    assert not any(independent.intersection(entry['scope_binding']['applies_to_range_ids'])
                   for entry in separate['supporting'] if entry['text'] == '仅第一标段适用。')


async def test_ambiguous_replacement_condition_is_preserved_and_blocks(tmp_path, artifacts):
    document = Document()
    document.add_heading('第一章 条件', 1)
    document.add_paragraph('仅第一标段适用。')
    document.add_paragraph('上述范围改为第二标段适用。')
    for index in range(8):
        document.add_paragraph(f'必须提交第{index}项证明。' * 120)
    frozen = await freeze_sources(registration(tmp_path, artifacts, [('ambiguous.docx', save_office(document))]),
                                   artifacts, budget=RequestBudget(context_tokens=12000, output_reserve=0))
    index = artifacts.read_json(frozen)
    assert any(problem['code'] == 'condition_scope_unresolved' and problem['blocking'] for problem in index['issues'])
    packs = list(EvidenceRepository(artifacts).iter_packs(frozen))
    later = next(pack for pack in packs if any('第7项' in entry['text'] for entry in pack['body']))
    assert any(entry['text'] == '仅第一标段适用。' for entry in later['supporting'])
    assert any(entry['text'] == '上述范围改为第二标段适用。' and entry['scope_binding']['status'] == 'unresolved'
               for entry in later['supporting'])


@pytest.mark.parametrize('corruption_target', ['image', 'cached_ocr'])
async def test_ocr_storage_corruption_propagates_as_technical_failure(tmp_path, artifacts, corruption_target):
    from docreader.integrations.artifacts import ArtifactIntegrityError
    cached = artifacts.put_json({'text': 'cached text', 'status': 'complete', 'issues': []})
    async def corrupting_ocr(image_ref, source):
        ref = image_ref if corruption_target == 'image' else cached
        path = artifacts.path(ref)
        path.chmod(0o644)
        path.write_bytes(b'corrupted')
        return artifacts.read_json(cached) if corruption_target == 'cached_ocr' else artifacts.read_bytes(image_ref)
    registered = registration(tmp_path, artifacts, [('image.png', png())])
    with pytest.raises(ArtifactIntegrityError):
        await freeze_sources(registered, artifacts, corrupting_ocr)


async def test_incomplete_long_condition_is_pending_reference_not_repeated_text(tmp_path, artifacts):
    document = Document()
    document.add_heading('第一章 范围', 1)
    document.add_paragraph('仅第一标段适用' + '连续无明确断句内容' * 300)
    for index in range(8):
        document.add_paragraph(f'必须提交第{index}项证明。' * 10)
    frozen = await freeze_sources(registration(tmp_path, artifacts, [('long-scope.docx', save_office(document))]),
                                   artifacts, budget=RequestBudget(context_tokens=12000, output_reserve=0))
    index = artifacts.read_json(frozen)
    packs = list(EvidenceRepository(artifacts).iter_packs(frozen))
    later = next(pack for pack in packs if any('第7项证明' in entry['text'] for entry in pack['body']))
    assert later['pending_scope_context']
    assert not any('连续无明确断句内容' in entry['text'] for entry in later['supporting'])
    assert all('text' not in entry and 'cells' not in entry for entry in later['pending_scope_context'])
    assert all(entry['scope_binding']['status'] == 'unresolved' for entry in later['pending_scope_context'])
    assert any(problem['code'] == 'condition_scope_unresolved' for problem in index['issues'])
    raw = artifacts.read_json(index['documents'][0]['raw_ref'])
    by_unit = {}
    for pack in packs:
        for entry in pack['body']:
            by_unit.setdefault(entry['unit_id'], []).append(entry)
    for unit in raw['units']:
        if unit['kind'] == 'section':
            assert ''.join(entry['text'] for entry in sorted(by_unit[unit['unit_id']], key=lambda item: item['start'])) == unit['text']


@pytest.mark.parametrize('mismatch', [None, 'source', 'image', 'page', 'original_ref'])
async def test_reviewed_decorative_images_require_exact_source_and_occurrence(tmp_path, artifacts, mismatch):
    from copy import deepcopy
    registered = registration(tmp_path, artifacts, [('reviewed.pdf', pdf(tiny_image=True))])
    initial = await freeze_sources(registered, artifacts)
    raw = artifacts.read_json(artifacts.read_json(initial)['documents'][0]['raw_ref'])
    images = [unit for unit in raw['units'] if unit['kind'] == 'image_region']
    review = {'source_sha256': registered['documents'][0]['original_ref']['sha256'],
              'sha256': raw['images'][images[0]['locator']['image_key']]['sha256'],
              'classification': 'decorative', 'status': 'reviewed', 'reason_code': 'reviewed_brand_identity',
              'rationale': 'Deterministic review fixture; inspected occurrences contain no business content.',
              'reviewed_positions': [{'document_id': 'reviewed.pdf', 'unit_id': unit['unit_id'],
                                      'page_ordinal': unit['locator']['page_ordinal'],
                                      'image_key': unit['locator']['image_key'],
                                      'locator': deepcopy(unit['locator'])} for unit in images]}
    if mismatch == 'source':
        review['source_sha256'] = '0' * 64
    elif mismatch == 'image':
        review['sha256'] = '0' * 64
    elif mismatch == 'page':
        review['reviewed_positions'][0]['page_ordinal'] = 99
    elif mismatch == 'original_ref':
        review['reviewed_positions'][0]['image_key'] = 'different occurrence'
    submitted = register_manifest({**registered, 'image_dispositions': [review]}, artifacts)
    assert submitted['image_dispositions'] == [review]
    calls = []
    async def ocr(ref, context):
        calls.append(context['unit_id'])
        return {'text': 'Unreviewed image content', 'status': 'complete', 'issues': []}
    frozen = await freeze_sources(submitted, artifacts, ocr)
    final = artifacts.read_json(frozen)
    if mismatch:
        assert calls
        assert any(item['code'] == 'image_disposition_identity_mismatch' for item in final['issues'])
    else:
        assert calls == [] and final['issues'] == []
        body = [entry for pack in EvidenceRepository(artifacts).iter_packs(frozen) for entry in pack['body']
                if entry['kind'] == 'image_region']
        assert len(body) == 2
        assert all(item['empty'] and item['non_requirement_reason'] == 'reviewed_decorative_image' for item in body)
        assert all(artifacts.read_json(item['image_disposition']['review_ref']) == review for item in body)


async def test_pack_plan_identity_retries_and_domain_replans_keep_canonical_source(tmp_path, artifacts, monkeypatch):
    from docreader.tender.evidence.context import condition_context
    from docreader.tender.evidence.scope import validate_scope_decisions
    document = Document()
    document.add_heading('第一章 范围', 1)
    document.add_paragraph('本表仅适用于联合体。')
    document.add_paragraph('必须提交证明文件。' * 800)
    frozen_ref = await freeze_sources(registration(tmp_path, artifacts, [('plans.docx', save_office(document))]),
                                      artifacts, plan=False)
    frozen = artifacts.read_json(frozen_ref)
    assert 'packs' not in frozen and 'budget_spec_ref' not in frozen
    raw_ref = frozen['documents'][0]['raw_ref']
    raw = artifacts.read_json(raw_ref)
    assert 'content' not in raw and all('unit_id' in item and 'key' not in item for item in raw['units'])
    source_ref = frozen['documents'][0]['source_index_ref']
    repository = EvidenceRepository(artifacts)
    canonical = repository.source_index(source_ref)
    original_ids = list(canonical.by_id)
    monkeypatch.setattr('docreader.tender.evidence.freeze.parse_native', lambda *args: pytest.fail('replan must reuse raw'))
    small = RequestBudget(context_tokens=12000, output_reserve=0)
    first_ref = await plan_sources(frozen, artifacts, budget=small)
    assert first_ref == await plan_sources(frozen_ref, artifacts, budget=small)
    first = artifacts.read_json(first_ref)
    persisted_bindings = []
    for entry in first['packs']:
        descriptor = artifacts.read_json(entry['artifact_ref'])
        assert 'scope_bindings' in descriptor
        assert repository.load_pack(entry)['scope_bindings'] == descriptor['scope_bindings']
        assert all('text' not in binding for binding in descriptor['scope_bindings'])
        persisted_bindings.extend(descriptor['scope_bindings'])
    assert persisted_bindings
    second = artifacts.read_json(await plan_sources(frozen, artifacts, budget=RequestBudget(context_tokens=40000, output_reserve=0)))
    assert first['input_digest'] == second['input_digest']
    assert first['budget_digest'] != second['budget_digest'] and first['plan_digest'] != second['plan_digest']
    assert first['frozen_ref'] == second['frozen_ref'] == frozen_ref
    _, conditions, _, _ = condition_context(canonical.ranges)
    candidate = next(item for values in conditions.values() for _, item in values)
    decisions = validate_scope_decisions(frozen, first, [{
        'range_id': candidate['range_id'], 'decision': 'local_only',
        'rationale': '该表说明仅约束本表自身，不适用于其他段落中的证明义务。',
        'evidence': [{'range_id': candidate['range_id'], 'quote': candidate['text']}],
        'target_range_ids': [],
    }], repository)['scope_decisions']
    revised = artifacts.read_json(await plan_sources(frozen, artifacts, budget=small, scope_decisions=decisions))
    assert revised['input_digest'] != first['input_digest'] and revised['plan_digest'] != first['plan_digest']
    assert revised['frozen_ref'] == frozen_ref and revised['documents'][0]['raw_ref'] == raw_ref
    assert not any(item['code'] == 'condition_scope_unresolved' for item in revised['issues'])
    assert list(EvidenceRepository(artifacts).source_index(source_ref).by_id) == original_ids


async def test_ocr_rejects_model_request_status_alias(tmp_path, artifacts):
    async def obsolete_status(image_ref, source):
        return {'text': 'Must not be accepted.', 'status': 'succeeded', 'issues': []}
    frozen = await freeze_sources(registration(tmp_path, artifacts, [('image.png', png())]),
                                  artifacts, obsolete_status, plan=False)
    index = artifacts.read_json(frozen)
    assert any(problem['code'] == 'ocr_response_invalid' for problem in index['issues'])
    raw = artifacts.read_json(index['documents'][0]['raw_ref'])
    assert all(value['status'] == 'unresolved' and not value['text'] for value in raw['ocr'].values())


async def test_plan_keeps_unrelated_documents_separate(tmp_path, artifacts):
    documents = []
    for number in range(2):
        document = Document()
        document.add_paragraph(f'Document {number}: submit its own proof.')
        documents.append((f'doc{number}.docx', save_office(document)))
    planned = await freeze_sources(registration(tmp_path, artifacts, documents), artifacts)
    packs = list(EvidenceRepository(artifacts).iter_packs(planned))
    assert len(packs) == 2
    for pack in packs:
        assert {entry['document_id'] for entry in [*pack['body'], *pack['supporting']]} == {pack['document_id']}


def test_external_image_review_identity_is_adapted_once_at_registration(tmp_path, artifacts):
    registered = registration(tmp_path, artifacts, [('image.png', png())])
    external = {'source_sha256': registered['documents'][0]['original_ref']['sha256'],
                'sha256': '1' * 64, 'reviewed_positions': [
                    {'document_id': 'image.png', 'unit_id': 'image:1', 'original_ref': 'image-payload',
                     'locator': {'locator_kind': 'image', 'original_ref': 'image-payload'}}]}
    submitted = register_manifest({**registered, 'image_dispositions': [external]}, artifacts)
    position = submitted['image_dispositions'][0]['reviewed_positions'][0]
    assert position['image_key'] == position['locator']['image_key'] == 'image-payload'
    assert 'original_ref' not in position and 'original_ref' not in position['locator']
    assert external['reviewed_positions'][0]['original_ref'] == 'image-payload'
    assert isinstance(submitted['documents'][0]['original_ref'], dict)
    assert register_manifest(submitted, artifacts) == submitted
