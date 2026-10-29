"""Oversized assembly preserves literal grids, scope and host reference ownership."""

from copy import deepcopy
import json
from types import SimpleNamespace

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.budget import RequestBudget
from docreader.tender.authoring_view import compact_assembly_payload
from docreader.tender import authoring_pipeline as pipeline


def native_grid():
    return {'row_count': 3, 'column_count': 2, 'widths_mm': [40, 20], 'cells': [
        {'row': 0, 'column': 0, 'row_span': 1, 'col_span': 2, 'text': '仅本标段填写'},
        {'row': 1, 'column': 0, 'row_span': 2, 'col_span': 1, 'text': '人员🙂\n条件不变'},
        {'row': 1, 'column': 1, 'row_span': 1, 'col_span': 1, 'text': ''},
        {'row': 2, 'column': 1, 'row_span': 1, 'col_span': 1, 'text': '备注：无资料留空'},
    ]}


def payload(grid):
    return {'schema_version': 2, 'operation': 'assemble', 'chapters': [
        {'title': '人员', 'title_path': ['技术部分', '人员'], 'is_volume': False, 'targets': [
            {'target_id': 't1', 'kind': 'form', 'purpose': '完整保留人员条件', 'manual_fill': True,
             'support_origins': ['tender', 'knowledge'], 'template_part_ids': ['p1']},
            {'target_id': 't2', 'kind': 'narrative', 'purpose': '其他要求', 'manual_fill': False,
             'support_origins': ['knowledge'], 'template_part_ids': []}]}],
        'template_parts': [{'part_id': 'p1', 'grid': grid, 'kind': 'table', 'section_id': 's1'}],
        'template_sections': [{'section_id': 's1', 'heading_path': '格式 > 人员表'}],
        'submission_sources': [{'source_id': 's1', 'text': json.dumps(grid, ensure_ascii=False)},
                               {'source_id': 's2', 'text': '不适用者无需提交。'}],
        'submission_rules': [{'rule_id': 'e1', 'text': '按原格式提交', 'source_ids': ['s1']}],
        'unassigned_rules': [{'rule_id': 'u1', 'rule': {'text': '不适用者无需提交。',
            'evidence': [{'quote': '不适用者无需提交。'}]}}],
        'findings': [{'code': 'scope_unclear', 'detail': '保留原条件，不默认适用'}]}


def restore_cells(grid):
    spans = {(row, column): (rs, cs) for row, column, rs, cs in grid['merged_cells']}
    return [{'row': row, 'column': column, 'text': text,
             'row_span': spans.get((row, column), (1, 1))[0],
             'col_span': spans.get((row, column), (1, 1))[1]}
            for row, cells in enumerate(grid['rows']) for column, text in enumerate(cells) if text is not None]


def test_matrix_roundtrip_preserves_blank_missing_merged_unicode_and_all_identities():
    original = payload(native_grid())
    before = deepcopy(original)
    view = compact_assembly_payload(original)
    assert original == before
    assert restore_cells(view['template_parts']['p1']['grid']) == native_grid()['cells']
    assert view['template_parts']['p1']['grid'] == view['submission_sources']['s1']['grid']
    assert view['template_parts']['p1']['grid']['rows'][0] == ['仅本标段填写', None]
    assert view['template_parts']['p1']['grid']['rows'][1][1] == ''
    for field, identity in [('template_parts', 'part_id'), ('submission_sources', 'source_id'),
                            ('submission_rules', 'rule_id'), ('unassigned_rules', 'rule_id')]:
        assert list(view[field]) == [entry[identity] for entry in original[field]]
    assert view['unassigned_rules']['u1']['rule'] == original['unassigned_rules'][0]['rule']
    assert view['findings'] == original['findings']
    assert view['chapters'][0]['targets'][0] == original['chapters'][0]['targets'][0]
    assert view['chapters'][0]['targets'][1] == {
        'target_id': 't2', 'kind': 'narrative', 'purpose': '其他要求'}


def test_unknown_cell_properties_and_very_sparse_grids_keep_the_complete_original():
    grid = native_grid()
    grid['cells'][0]['formula'] = 'SUM(A1:A9)'
    assert compact_assembly_payload(payload(grid))['template_parts']['p1']['grid'] == grid
    sparse = {'row_count': 1000000, 'column_count': 100, 'cells': native_grid()['cells'][:1]}
    assert compact_assembly_payload(payload(sparse))['template_parts']['p1']['grid'] == sparse


def test_duplicate_native_coordinates_are_preserved_without_overwriting_cell_text():
    grid = native_grid()
    grid['cells'].append({**grid['cells'][0], 'text': '重复位置的原文也不能丢弃'})
    view = compact_assembly_payload(payload(grid))
    assert view['template_parts']['p1']['grid'] == grid
    assert view['submission_sources']['s1']['grid'] == grid


async def test_over_budget_assembly_reaches_model_and_binds_original_template(monkeypatch, tmp_path):
    artifacts = ArtifactStore(tmp_path)
    ref = {'source_index_ref': artifacts.put_json({'fixture': True}), 'unit_id': 'original-table'}
    part = {'source_ref': ref, 'selectors': []}
    snapshot = {'schema_version': 2, 'run_id': 'compact-assembly', 'source_version': 1, 'domain_revision': 1}
    grid = {'row_count': 160, 'column_count': 8, 'cells': [
        {'row': row, 'column': column, 'row_span': 1, 'col_span': 1, 'text': ''}
        for row in range(160) for column in range(8)]}
    grid['cells'][0]['text'] = '本标段完整表格，不得删除空白行'
    fragment = {'chapters': [{'chapter_id': 'local', 'title': '人员', 'children': [], 'targets': [
        {'target_id': 'local-target', 'kind': 'form', 'purpose': '按原格式填写人员',
         'rule_refs': [], 'source_refs': [ref], 'template_refs': [part]}]}], 'issues': []}
    context = SimpleNamespace(input={**snapshot, 'issues': []}, sources={}, rules={},
                              payload={'rules': []}, validate=lambda value: deepcopy(fragment))
    monkeypatch.setattr(pipeline, '_source_records', lambda *_: [])
    monkeypatch.setattr(pipeline, 'prepare_template_parts', lambda *_: ([], []))
    monkeypatch.setattr(pipeline, 'load_template_part', lambda *_a, **_kw: {
        'unit': {'kind': 'table', 'heading_path': '人员原表'}, 'grid': grid})
    calls = []
    budget = RequestBudget(model='fixture', context_tokens=65536)

    async def execute(task, request, validate):
        value = json.loads(request['messages'][1]['content'][0]['text'])
        calls.append(value)
        budget.require_request(request, task)
        if value.get('operation') == 'assemble':
            assert value['assembly_view'] == 'compact_v1'
            assert restore_cells(value['template_parts']['p1']['grid']) == grid['cells']
            return validate({'chapters': [{'title': '人员安排', 'targets': [
                {'kind': 'form', 'purpose': '按原格式填写人员', 'target_ids': ['t1'],
                 'template_part_ids': ['p1']}]}], 'table_headers': [{'part_id': 'p1', 'header_row_count': 1}]})
        return validate({'schema_version': 2, 'chapters': [], 'findings': []})

    gaps = []
    outline = await pipeline._organize(artifacts, context, budget, execute, gaps)
    assert len(calls) == 2 and not gaps
    assert outline['chapters'][0]['title'] == '人员安排'
    assert outline['chapters'][0]['targets'][0]['template_refs'] == [{**part, 'header_row_count': 1}]
