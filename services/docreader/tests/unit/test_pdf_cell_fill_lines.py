from io import BytesIO

import pytest
from pypdf import PdfWriter
from pypdf.generic import DecodedStreamObject, DictionaryObject, NameObject

from docreader.integrations.artifacts import ArtifactStore
from docreader.tender.authoring_export import export_authoring_document
from docreader.tender.evidence.references import SourceIndex
from test_authoring_export import chapter, document, target


def _pdf_form(printed_school):
    writer = PdfWriter()
    page = writer.add_blank_page(width=320, height=160)
    font = DictionaryObject({NameObject('/Type'): NameObject('/Font'), NameObject('/Subtype'): NameObject('/Type1'),
                             NameObject('/BaseFont'): NameObject('/Helvetica')})
    page[NameObject('/Resources')] = DictionaryObject({NameObject('/Font'): DictionaryObject({
        NameObject('/F1'): writer._add_object(font)})})
    # The 2nd and 3rd columns merge in the upper row. Its field line spans the lower column edge.
    commands = [f'20 {y} m 300 {y} l S' for y in (20, 80, 140)]
    commands.extend(f'{x} 20 m {x} 140 l S' for x in (20, 100, 300))
    commands.extend(['200 20 m 200 80 l S', '145 107.5 m 221 107.5 l S', '26 107.5 m 57 107.5 l S'])
    for x, y, text in [(26, 110, 'Name'), (110, 110, 'Year'), (225, 110, printed_school),
                       (26, 50, 'Record'), (205, 50, 'Year'), (265, 50, 'School')]:
        commands.append(f'BT /F1 12 Tf {x} {y} Td ({text}) Tj ET')
    stream = DecodedStreamObject()
    stream.set_data('\n'.join(commands).encode('ascii'))
    page[NameObject('/Contents')] = writer._add_object(stream)
    out = BytesIO()
    writer.write(out)
    return out.getvalue()


@pytest.mark.parametrize('printed_school, expected', [('School', 'Year|School'), ('College', 'YearSchool')])
def test_whole_pdf_table_exports_only_verifiable_editable_fill_lines(tmp_path, printed_school, expected):
    artifacts = ArtifactStore(tmp_path)
    original = artifacts.put_bytes(_pdf_form(printed_school), 'application/pdf')
    record = {'document_id': 'form', 'document_version': '1', 'original_ref': original}
    locator = {'locator_kind': 'page_table', 'page_ordinal': 0, 'table_ordinal': 0,
               'left': 20., 'right': 300., 'bottom': 20., 'top': 140.}
    grid = {'row_count': 2, 'column_count': 3, 'widths_mm': [28., 35., 35.], 'cells': [
        {'row': row, 'column': column, 'row_span': 1, 'col_span': span, 'text': text}
        for row, column, span, text in [(0, 0, 1, 'Name'), (0, 1, 2, 'Year School'),
            (1, 0, 1, 'Record'), (1, 1, 1, ''), (1, 2, 1, 'Year School')]]}
    unit = {'unit_id': 'table', 'kind': 'table_region', 'ordinal': 0, 'text': '', 'grid': grid, 'locator': locator}
    raw = {'schema_version': 2, 'units': [unit], 'images': {}, 'ocr': {}, 'original_ref': original,
           'source_contract': {'page_manifest': [], 'units': [{'unit_id': 'table', 'section_id': 'form',
               'parent_section_id': None, 'heading_path': 'Form', 'physical_locator': locator}]}}
    index = SourceIndex(record, raw)
    index_ref = artifacts.put_json(index.descriptor(artifacts.put_json(index.raw)))
    ref = {'source_index_ref': index_ref, 'unit_id': 'table'}
    frozen = artifacts.put_json({'documents': [{**record, 'source_index_ref': index_ref}]})
    form = target('template', 'form', [ref], templates=[ref])
    outline = {'authoring_input': {'frozen_ref': frozen}, 'chapters': [chapter('form', 'Form', [form])]}
    result = export_authoring_document(artifacts, outline, [])
    table, = document(artifacts, result).tables

    def marked(row, column):
        return ''.join('|' if run.underline and run.text.isspace() else ''.join(run.text.split())
                       for paragraph in table.cell(row, column).paragraphs for run in paragraph.runs)

    assert table.cell(0, 1)._tc is table.cell(0, 2)._tc
    assert marked(0, 1) == expected
    assert marked(0, 0) == 'Name'  # Line beneath printed text.
    assert [marked(1, column) for column in range(3)] == ['Record', '', 'YearSchool']


def test_table_check_preserves_fixed_words_around_editable_tabs():
    from docx import Document
    from docx.shared import Pt
    from docreader.tender.acceptance_eval import _compare_table, _parse_docx_table

    cell = {'row': 0, 'column': 0, 'row_span': 1, 'col_span': 1, 'text': 'YearSchool'}
    source = {'row_count': 1, 'column_count': 1, 'cells': [cell]}
    table = Document().add_table(rows=1, cols=1)
    paragraph = table.cell(0, 0).paragraphs[0]
    paragraph.paragraph_format.tab_stops.add_tab_stop(Pt(35))
    paragraph.paragraph_format.tab_stops.add_tab_stop(Pt(80))
    paragraph.add_run('Year')
    paragraph.add_run('\t')  # Move to the start of the drawn line.
    blank = paragraph.add_run('\t')
    blank.underline = True
    end = paragraph.add_run('School')

    def mismatches():
        return _compare_table(source, {**_parse_docx_table(table._tbl), 'table_index': 0}, {})

    assert not mismatches()
    cell['text'] = 'Year School'
    assert not mismatches()
    end.text = 'Schoo'
    assert mismatches()
    end.text = 'School'
    blank.underline = False
    cell['text'] = 'YearSchool'
    assert mismatches()
