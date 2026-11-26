"""A source-bound response plan expands its adopted native form exactly once."""

from typing import TypeVar
from copy import deepcopy
from io import BytesIO

from docx import Document
from docx.enum.table import WD_ROW_HEIGHT_RULE
from docx.oxml.ns import qn
import pytest
from pydantic import ValidationError

from docreader.integrations.artifacts import ArtifactStore
from docreader.tender.authoring_excerpts import response_row_id
from docreader.tender.authoring_export import export_authoring_document
from docreader.tender.evidence.references import SourceIndex, fragment, selector
from docreader.tender.schema import ResponseTable, SourceExcerptRef

_T = TypeVar("_T")


def _required(value: _T | None) -> _T:
    assert value is not None
    return value



def form_grid(*, fixed=False, merged=False, clause_numbers=False):
    values = [['序号', '评价条款', '投标响应情况', '证明资料所在文件\n及页码'],
              *[[str(number), '', '', ''] for number in range(1, 6)], ['……', '', '', '']]
    if clause_numbers:
        values = [['条款号', '招标文件要求', '投标文件响应', '偏离情况', '说明'],
                  *[[''] * 5 for _ in range(5)]]
    if fixed:
        values[1][1] = '原有固定条款，不得覆盖。'
    cells = [{'row': row, 'column': column, 'row_span': 1, 'col_span': 1, 'text': text}
             for row, values in enumerate(values) for column, text in enumerate(values)]
    if merged:
        cells = [cell for cell in cells if (cell['row'], cell['column']) != (1, 2)]
        next(cell for cell in cells if (cell['row'], cell['column']) == (1, 1))['col_span'] = 2
    return {'row_count': len(values), 'column_count': len(values[0]), 'cells': cells,
            'widths_mm': [15, 50, 55, 20, 15] if clause_numbers else [15, 50, 55, 35]}


def case(artifacts, count=8, *, fixed=False, merged=False, extra_parts=False, version='1', clause_numbers=False):
    def grid_unit(identity, grid, ordinal):
        return {'unit_id': identity, 'kind': 'table_region', 'ordinal': ordinal, 'text': '', 'grid': grid,
                'locator': {'locator_kind': 'page_table', 'page_ordinal': ordinal, 'table_ordinal': 0,
                            'left': 100.0, 'right': 450.0, 'top': 700.0, 'bottom': 300.0}}

    units = [grid_unit('form', form_grid(fixed=fixed, merged=merged, clause_numbers=clause_numbers), 0)]
    cell_text = '1.3.3.2 支持x86架构（海光、兆' if clause_numbers else '支持x86架构（海光、兆'
    for identity, values in [('start', ['4', '客户端信', '5.00', cell_text, '']),
                             ('end', ['', '创适配', '', '芯），须提供兼容证书。', ''])]:
        units.append(grid_unit(identity, {'row_count': 1, 'column_count': 5, 'cells': [
            {'row': 0, 'column': column, 'row_span': 1, 'col_span': 1, 'text': text}
            for column, text in enumerate(values)]}, len(units)))
    units.append(grid_unit('other-form', {'row_count': 2, 'column_count': 2, 'cells': [
        {'row': row, 'column': column, 'row_span': 1, 'col_span': 1, 'text': text}
        for row, values in enumerate([['其他原表', '固定内容'], ['证明编号', '']])
        for column, text in enumerate(values)]}, len(units)))
    for identity, text in [('before', '原式前置固定说明。'), ('signature', '投标人签章：________\n日期：________'),
                           ('alpha', 'Alpha '), ('beta', 'Beta'),
                           *([('inline-number', '参见 1.2.1.5 服务时限要求')] if clause_numbers else []),
                           *[(f'prose-{number}', f'独立条款{number}；须提供完整证明。') for number in range(count)]]:
        if clause_numbers and identity.startswith('prose-'):
            text = f'1.2.1.{5 + 2 * int(identity[6:])} {text}'
        units.append({'unit_id': identity, 'kind': 'section', 'ordinal': len(units), 'text': text,
                      'locator': {'locator_kind': 'document', 'section_ordinal': len(units)}})
    raw = {'schema_version': 2, 'units': units, 'images': {}, 'ocr': {},
           'source_contract': {'page_manifest': [], 'units': [
               {'unit_id': unit['unit_id'], 'section_id': 'source', 'parent_section_id': None,
                'heading_path': '评审原文', 'physical_locator': unit['locator']} for unit in units]}}
    document = {'document_id': 'source', 'document_version': version,
                'original_ref': artifacts.put_bytes(b'one frozen synthetic source')}
    index = SourceIndex(document, raw)
    index_ref = artifacts.put_json(index.descriptor(artifacts.put_json(raw)))

    def ref(identity):
        return {'source_index_ref': index_ref, 'unit_id': identity}

    def excerpt(identity, column=None):
        source = next(row for row in index.ranges if row['unit_id'] == identity)
        selected = source
        if column is not None:
            cell = next(cell for cell in source['cell_intervals'] if cell['column'] == column)
            selected = fragment(source, cell['start'], cell['end'])
        return {'source_ref': ref(identity), 'selectors': [selector(selected)]}

    layout = {'source_ref': ref('form'), 'repeat_start_row': 1, 'repeat_end_row': 5,
              'sequence_column': 0, 'source_column': 1, 'response_column': 2, 'proof_column': 3}
    rows = [{'source_excerpts': [excerpt('start', 3), excerpt('end', 3)],
             'context_source_excerpts': [excerpt('start', 1), excerpt('start', 2)],
             'source_join': 'continuation', 'rule_refs': []},
            *[{'source_excerpts': [excerpt(f'prose-{number}')], 'rule_refs': []} for number in range(count - 1)]]
    if clause_numbers:
        rows = [{'source_excerpts': [excerpt(f'prose-{number}')], 'rule_refs': []} for number in range(count)]
    for row in rows:
        row['row_id'] = response_row_id('native', row['source_excerpts'], row['rule_refs'],
            context_source_excerpts=row.get('context_source_excerpts', []),
            source_join=row.get('source_join', 'paragraphs'), native_layout=layout)
    parts = [{'source_ref': ref('form'), 'selectors': [], 'header_row_count': 1}]
    if extra_parts:
        parts = [{'source_ref': ref('before'), 'selectors': []}, *parts,
                 {'source_ref': ref('other-form'), 'selectors': []},
                 {'source_ref': ref('signature'), 'selectors': []}]
    target = {'target_id': 'native', 'kind': 'form', 'purpose': '逐条填写原式。', 'source_refs': [],
              'rule_refs': [], 'template_refs': parts, 'manual_fill': True, 'support_origins': ['tender'],
              'response_table': {'native_layout': layout, 'rows': rows}}
    return target, excerpt, index


def render(artifacts, target, units=()):
    outline = {'chapters': [{'chapter_id': 'chapter', 'title': '逐条响应', 'targets': [target], 'children': []}]}
    result = export_authoring_document(artifacts, outline, list(units))
    return Document(BytesIO(artifacts.read_bytes(result['docx_ref']))), result


def rebind(target):
    for row in target['response_table']['rows']:
        row['row_id'] = response_row_id(target['target_id'], row['source_excerpts'], row['rule_refs'],
            context_source_excerpts=row.get('context_source_excerpts', []),
            source_join=row.get('source_join', 'paragraphs'), native_layout=target['response_table']['native_layout'])


def test_four_column_form_is_expanded_once_from_exact_cells_without_enterprise_answers(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    target, _, _ = case(artifacts)
    doc, _ = render(artifacts, target)
    assert len(doc.tables) == 1
    table = doc.tables[0]
    assert len(table.columns) == 4 and len(table.rows) == 10
    assert [cell.text for cell in table.rows[0].cells] == [
        '序号', '评价条款', '投标响应情况', '证明资料所在文件\n及页码']
    assert '海光、兆芯' in table.cell(1, 1).text
    assert table.cell(1, 1).text.endswith('支持x86架构（海光、兆芯），须提供兼容证书。')
    assert '创适配' not in table.cell(1, 1).text
    assert [row.cells[0].text for row in table.rows[1:-1]] == [str(number) for number in range(1, 9)]
    assert table.cell(9, 0).text == '……'
    assert all(not row.cells[2].text and not row.cells[3].text for row in table.rows[1:])


@pytest.mark.parametrize('count', [2, 7])
def test_empty_clause_number_column_copies_source_labels_and_extends_the_five_column_form(tmp_path, count):
    artifacts = ArtifactStore(tmp_path)
    target, _, _ = case(artifacts, count=count, clause_numbers=True)
    doc, result = render(artifacts, target)
    table = doc.tables[0]
    labels = [f'1.2.1.{5 + 2 * index}' for index in range(count)]
    assert [cell.text for cell in table.rows[0].cells] == ['条款号', '招标文件要求', '投标文件响应', '偏离情况', '说明']
    assert [row.cells[0].text for row in table.rows[1:]] == labels + [''] * max(0, 5 - count)
    assert [row.cells[1].text for row in table.rows[1:]] == [
        f'{label} 独立条款{index}；须提供完整证明。' for index, label in enumerate(labels)] + [''] * max(0, 5 - count)
    assert all(not cell.text for row in table.rows[1:] for cell in row.cells[2:])
    assert not any(gap['code'] == 'export_native_response_table_unconfirmed' for gap in result['gaps'])


@pytest.mark.parametrize('problem', ['missing', 'truncated', 'mid_line'])
def test_unproven_clause_number_keeps_all_rows_of_the_original_form_blank(tmp_path, problem):
    artifacts = ArtifactStore(tmp_path)
    target, excerpt, index = case(artifacts, count=2, clause_numbers=True)
    identity = {'missing': 'alpha', 'truncated': 'prose-0', 'mid_line': 'inline-number'}[problem]
    selected = excerpt(identity)
    if problem != 'missing':
        base = index.by_id[selected['selectors'][0]['source_range_id']]
        start, end = (0, 5) if problem == 'truncated' else (3, base['end'])
        selected['selectors'] = [selector(fragment(base, start, end))]
    target['response_table']['rows'][0]['source_excerpts'] = [selected]
    rebind(target)
    doc, result = render(artifacts, target)
    assert len(doc.tables[0].rows) == 6
    assert all(not cell.text for row in doc.tables[0].rows[1:] for cell in row.cells)
    assert any(gap['code'] == 'export_native_response_table_unconfirmed' and '条款号' in gap['detail']
               for gap in result['gaps'])


def test_clause_number_uses_the_selected_native_cell_start(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    target, excerpt, _ = case(artifacts, count=1, clause_numbers=True)
    row = target['response_table']['rows'][0]
    row['source_excerpts'] = [excerpt('start', 3), excerpt('end', 3)]
    row['source_join'] = 'continuation'
    rebind(target)
    doc, _ = render(artifacts, target)
    assert doc.tables[0].cell(1, 0).text == '1.3.3.2'
    assert doc.tables[0].cell(1, 1).text == '1.3.3.2 支持x86架构（海光、兆芯），须提供兼容证书。'


def test_native_response_row_identity_survives_formal_item_reference_round_trip(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    target, _, _ = case(artifacts, count=1, clause_numbers=True)
    row = target['response_table']['rows'][0]
    row['rule_refs'] = [{'item_ref': {'artifact_ref': artifacts.put_json({'items': ['pack:i1']}),
                                     'item_id': 'pack:i1'}}]
    rebind(target)
    doc, _ = render(artifacts, target)
    assert doc.tables[0].cell(1, 0).text == '1.2.1.5'

    target['response_table'] = ResponseTable.model_validate(target['response_table']).model_dump()
    doc, _ = render(artifacts, target)
    assert doc.tables[0].cell(1, 0).text == '1.2.1.5'

    target['response_table']['rows'][0]['rule_refs'][0]['item_ref']['item_id'] = 'pack:i2'
    doc, result = render(artifacts, target)
    assert doc.tables[0].cell(1, 0).text == ''
    assert any(gap['code'] == 'export_native_response_table_unconfirmed' and '条款行身份' in gap['detail']
               for gap in result['gaps'])


def test_fewer_requirements_keep_all_original_numbered_rows_and_other_source_parts(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    target, _, _ = case(artifacts, count=3, extra_parts=True)
    doc, _ = render(artifacts, target)
    assert len(doc.tables) == 2 and len(doc.tables[0].rows) == 7
    assert [row.cells[0].text for row in doc.tables[0].rows[1:]] == ['1', '2', '3', '4', '5', '……']
    assert not doc.tables[0].cell(4, 1).text and not doc.tables[0].cell(5, 1).text
    assert [[cell.text for cell in row.cells] for row in doc.tables[1].rows] == [
        ['其他原表', '固定内容'], ['证明编号', '']]
    paragraphs = [paragraph.text for paragraph in doc.paragraphs]
    assert paragraphs.count('原式前置固定说明。') == 1
    assert paragraphs.count('投标人签章：________\n日期：________') == 1
    body = list(doc.element.body)
    table_position = next(index for index, node in enumerate(body) if node.tag == qn('w:tbl'))
    before = '\n'.join(''.join(node.itertext()) for node in body[:table_position])
    after = '\n'.join(''.join(node.itertext()) for node in body[table_position + 1:])
    assert '原式前置固定说明。' in before and '投标人签章：________' in after


@pytest.mark.parametrize('problem', ['fixed', 'merged', 'not_bound', 'whole_native_row'])
def test_invalid_native_plan_keeps_original_form_and_named_pending_gap(tmp_path, problem):
    artifacts = ArtifactStore(tmp_path)
    target, excerpt, _ = case(artifacts, fixed=problem == 'fixed', merged=problem == 'merged')
    if problem == 'not_bound':
        target['response_table']['native_layout']['source_ref'] = excerpt('start', 3)['source_ref']
    if problem == 'whole_native_row':
        target['response_table']['rows'][0]['source_excerpts'] = [excerpt('start')]
        rebind(target)
    doc, result = render(artifacts, target)
    assert len(doc.tables) == 1 and len(doc.tables[0].rows) == 7 and len(doc.tables[0].columns) == 4
    assert any(gap['code'] == 'export_native_response_table_unconfirmed' and gap['target_id'] == 'native'
               for gap in result['gaps'])
    assert not doc.tables[0].cell(1, 2).text and not doc.tables[0].cell(1, 3).text
    assert ('原有固定条款，不得覆盖。' in doc.tables[0].cell(1, 1).text) == (problem == 'fixed')


def test_manual_form_omits_both_row_fills_and_native_cell_fills_after_expansion(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    target, _, _ = case(artifacts)
    row = target['response_table']['rows'][0]
    unit = {'task_id': 'write', 'chapter_id': 'chapter', 'target_ids': ['native'], 'status': 'generated',
            'blocks': [], 'cell_fills': [{'source_ref': target['template_refs'][0]['source_ref'], 'row': 1,
                                        'column': 3, 'text': '证明第999页', 'target_ids': ['native']}],
            'row_fills': [{'row_id': row['row_id'], 'text': '已完全满足', 'material_ids': ['invented']}],
            'attachments': [], 'gaps': []}
    doc, result = render(artifacts, target, [unit])
    assert len(doc.tables) == 1 and len(doc.tables[0].rows) == 10
    assert '证明第999页' not in doc.element.xml and '已完全满足' not in doc.element.xml
    assert all(not row.cells[2].text and not row.cells[3].text for row in doc.tables[0].rows[1:])
    assert any(gap['code'] == 'export_manual_content_omitted' for gap in result['gaps'])


def test_native_expansion_reuses_pdf_row_heights_typography_widths_and_header(tmp_path, monkeypatch):
    import docreader.tender.authoring_export as export

    artifacts = ArtifactStore(tmp_path)
    target, _, _ = case(artifacts)
    monkeypatch.setattr(export, '_pdf_row_heights', lambda *_: [11, 21, 22, 23, 24, 25, 31])
    monkeypatch.setattr(export, '_pdf_table_typography', lambda *_: {'font_size': 12, 'padding': 2, 'line_spacing': 14})
    monkeypatch.setattr(export, '_pdf_table_borders', lambda *_: {
        'top': 12, 'left': 12, 'bottom': 12, 'right': 12, 'insideH': 6, 'insideV': 6,
    })
    doc, _ = render(artifacts, target)
    table = doc.tables[0]
    assert [_required(row.height).pt for row in table.rows] == [11, 21, 22, 23, 24, 25, 25, 25, 25, 31]
    assert all(row.height_rule == WD_ROW_HEIGHT_RULE.AT_LEAST for row in table.rows)
    assert all(_required(run.font.size).pt == 12 for row in table.rows for cell in row.cells for run in cell.paragraphs[0].runs)
    assert [round(_required(column.width).mm) for column in table.columns] == [15, 50, 55, 35]
    assert table.rows[0]._tr.find('./' + qn('w:trPr') + '/' + qn('w:tblHeader')) is not None
    borders = table._tbl.tblPr.find(qn('w:tblBorders'))
    assert borders is not None and borders.find(qn('w:top')).get(qn('w:sz')) == '12' and borders.find(qn('w:insideH')).get(qn('w:sz')) == '6'


@pytest.mark.parametrize('changed', ['source', 'context', 'join', 'layout'])
def test_changed_source_grouping_or_layout_cannot_reuse_a_previous_native_row_identity(tmp_path, changed):
    artifacts = ArtifactStore(tmp_path)
    target, excerpt, _ = case(artifacts)
    row = target['response_table']['rows'][0]
    old_id = row['row_id']
    if changed == 'source':
        row['source_excerpts'] = [excerpt('prose-0')]
    elif changed == 'context':
        row['context_source_excerpts'] = [excerpt('start', 2)]
    elif changed == 'join':
        row['source_join'] = 'paragraphs'
    else:
        layout = target['response_table']['native_layout']
        layout['response_column'], layout['proof_column'] = layout['proof_column'], layout['response_column']
    fresh = deepcopy(target)
    rebind(fresh)
    assert fresh['response_table']['rows'][0]['row_id'] != old_id
    doc, result = render(artifacts, target)
    assert len(doc.tables) == 1 and len(doc.tables[0].rows) == 7
    assert any(gap['code'] == 'export_native_response_table_unconfirmed' for gap in result['gaps'])


@pytest.mark.parametrize('join,expected', [('paragraphs', 'Alpha \nBeta'), ('continuation', 'Alpha Beta')])
def test_continuation_is_explicit_and_never_strips_original_english_spaces(tmp_path, join, expected):
    artifacts = ArtifactStore(tmp_path)
    target, excerpt, _ = case(artifacts, count=1)
    row = target['response_table']['rows'][0]
    row.update(source_excerpts=[excerpt('alpha'), excerpt('beta')], context_source_excerpts=[], source_join=join)
    rebind(target)
    doc, _ = render(artifacts, target)
    assert doc.tables[0].cell(1, 1).text == expected


def test_explicit_continuation_rejects_reversed_frozen_source_order(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    target, excerpt, _ = case(artifacts, count=1)
    target['response_table']['rows'][0].update(source_excerpts=[excerpt('beta'), excerpt('alpha')],
                                              context_source_excerpts=[], source_join='continuation')
    rebind(target)
    doc, result = render(artifacts, target)
    assert len(doc.tables) == 1 and len(doc.tables[0].rows) == 7
    assert any(gap['code'] == 'export_native_response_table_unconfirmed' for gap in result['gaps'])


def test_legacy_row_identity_and_serialized_table_shape_remain_unchanged():
    reference = {'source_ref': {'source_index_ref': {'sha256': 'a' * 64, 'relative_path': 'aa/' + 'a' * 64,
                                                  'size': 1, 'media_type': 'application/json'}, 'unit_id': 'source'},
                 'selectors': [{'range_id': 'range', 'source_range_id': 'range', 'start': 0, 'end': 5}]}
    row = {'row_id': response_row_id('target', [reference], []), 'source_excerpts': [reference], 'rule_refs': []}
    assert row['row_id'] == 'target:row-7470d6d7e20b0c582022d2f6'
    assert response_row_id('target', [reference], [], context_source_excerpts=[],
                           source_join='paragraphs', native_layout=None) == row['row_id']
    expected = {**row, 'source_excerpts': [SourceExcerptRef.model_validate(reference).model_dump()]}
    assert ResponseTable.model_validate({'rows': [row]}).model_dump() == {'context_source_excerpts': [], 'rows': [expected]}


@pytest.mark.parametrize('problem', ['overlapping_columns', 'out_of_bounds', 'header_overlap'])
def test_invalid_roles_or_repeat_boundaries_never_fall_back_to_a_generic_table(tmp_path, problem):
    artifacts = ArtifactStore(tmp_path)
    target, _, _ = case(artifacts)
    layout = target['response_table']['native_layout']
    if problem == 'overlapping_columns':
        layout['proof_column'] = layout['response_column']
    elif problem == 'out_of_bounds':
        layout['repeat_end_row'] = 100
    else:
        target['template_refs'][0]['header_row_count'] = 2
    rebind(target)
    doc, result = render(artifacts, target)
    assert len(doc.tables) == 1 and len(doc.tables[0].rows) == 7
    assert not any(row.cells[1].text for row in doc.tables[0].rows[1:])
    assert any(gap['code'] == 'export_native_response_table_unconfirmed' for gap in result['gaps'])


def test_same_unit_name_from_another_frozen_version_is_not_an_adopted_template(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    target, _, _ = case(artifacts)
    later, _, _ = case(artifacts, version='2')
    reference = later['response_table']['native_layout']['source_ref']
    assert reference['unit_id'] == target['template_refs'][0]['source_ref']['unit_id']
    assert reference != target['template_refs'][0]['source_ref']
    target['response_table']['native_layout']['source_ref'] = reference
    rebind(target)
    doc, result = render(artifacts, target)
    assert len(doc.tables) == 1 and len(doc.tables[0].rows) == 7
    assert any(gap['code'] == 'export_native_response_table_unconfirmed' for gap in result['gaps'])


def test_same_unit_with_distinct_selected_parts_is_not_an_unambiguous_native_form(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    target, _, index = case(artifacts)
    selected = next(row for row in index.ranges if row['unit_id'] == 'form' and row['row_start'] == 1)
    target['template_refs'].append({'source_ref': target['template_refs'][0]['source_ref'],
                                    'selectors': [selector(selected)]})
    doc, result = render(artifacts, target)
    assert len(doc.tables) == 1 and len(doc.tables[0].rows) == 7
    assert any(gap['code'] == 'export_native_response_table_unconfirmed' for gap in result['gaps'])
    assert not any(row.cells[1].text for row in doc.tables[0].rows[1:])


def test_a_selected_prose_part_is_preserved_without_expanding_back_to_its_whole_unit(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    target, excerpt, index = case(artifacts)
    base = next(row for row in index.ranges if row['unit_id'] == 'alpha')
    source = excerpt('alpha')['source_ref']
    target['template_refs'] = [{'source_ref': source, 'selectors': [selector(fragment(base, 0, 3))]}]
    target['response_table']['native_layout']['source_ref'] = source
    rebind(target)
    doc, result = render(artifacts, target)
    paragraphs = [paragraph.text for paragraph in doc.paragraphs]
    assert not doc.tables and 'Alp' in paragraphs
    assert 'Alpha ' not in paragraphs and 'Beta' not in doc.element.xml
    assert any(gap['code'] == 'export_native_response_table_unconfirmed' for gap in result['gaps'])


@pytest.mark.parametrize('option', ['context', 'continuation'])
def test_native_row_options_require_the_native_renderer_in_the_canonical_contract(tmp_path, option):
    artifacts = ArtifactStore(tmp_path)
    target, _, _ = case(artifacts, count=1)
    if option == 'context':
        target['response_table']['rows'][0]['source_join'] = 'paragraphs'
    else:
        target['response_table']['rows'][0]['context_source_excerpts'] = []
    target['response_table'].pop('native_layout')
    with pytest.raises(ValidationError, match='native_layout'):
        ResponseTable.model_validate(target['response_table'])
