"""Material header decisions preserve native tables and remain receipt-safe."""

from copy import deepcopy
from io import BytesIO

from docx import Document
import pytest
from pydantic import ValidationError

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.request import ModelBudgetError, ModelContractError, ModelWorkUnitExhausted
from docreader.tender.authoring_export import export_authoring_document
from docreader.tender.authoring_view import _grid_view
import docreader.tender.authoring_pipeline as pipeline
import docreader.tender.directory_compile as directory
import docreader.tender.document_execution as execution

from test_authoring_pipeline_boundaries import budget, payload, snapshot
from test_directory_compile import fixture_titles, item, node, plan, window
from test_directory_plan_replay import Receipts, executor


def native_grid():
    cells = [
        {'row': 0, 'column': 0, 'row_span': 2, 'col_span': 1, 'text': '项目'},
        {'row': 0, 'column': 1, 'row_span': 1, 'col_span': 2, 'text': '近两年'},
        {'row': 1, 'column': 1, 'row_span': 1, 'col_span': 1, 'text': '本年'},
        {'row': 1, 'column': 2, 'row_span': 1, 'col_span': 1, 'text': '上年'},
    ]
    cells.extend({'row': row, 'column': column, 'row_span': 1, 'col_span': 1, 'text': text}
                 for row, values in enumerate([['资产', '', ''], ['', '', '']], start=2)
                 for column, text in enumerate(values))
    return {'row_count': 4, 'column_count': 3, 'cells': cells}


def native_case(artifacts, *, subjects=False, extra=False, native_headers=()):
    specs = [{'text': '投标人及原厂分别提交财务情况。\n财务情况表'}, {'grid': native_grid()}]
    specs[1]['headers'] = list(native_headers)
    if extra:
        specs.extend([{'grid': {**native_grid(), 'widths_mm': [30, 40, 40]}}, {'text': '附件说明'}])
    fixed = snapshot(artifacts, specs)
    documents = directory.documents_from_frozen(artifacts, artifacts.read_json(fixed['frozen_ref']))
    chosen = {**node('finance', 0, 1, document_id='document'),
              'body_windows': [window(1, 0, len(documents[0]['pages'][1].splitlines()) - 1, 'document')]}
    if subjects:
        chosen['subjects'] = [{'source': window(0, 0, document_id='document'), 'role': role}
                              for role in ('投标人', '原厂')]
    nodes = [chosen]
    if extra:
        nodes.append({**node('note', 3, 0, 'finance', document_id='document'), 'body_windows': []})
    compiled = directory.compile_directory(plan(nodes, [item('finance', 'subtree' if extra else 'self')]), documents)
    return fixed, documents, compiled


def parts(compiled):
    for chapter in compiled['chapters']:
        if chapter.get('unplaced'):
            continue
        yield from chapter.get('template_parts', [])
        for field in ('template_segments', 'material_segments'):
            yield from (segment['part'] for segment in chapter.get(field, []) if segment.get('part'))


def header_issues(compiled):
    return [issue for issue in compiled['issues'] if issue['code'] == 'outline_directory_table_header_unconfirmed']


def word(artifacts, fixed, compiled):
    outline = directory.outline_from_compiled(fixed, compiled)
    result = export_authoring_document(artifacts, outline, [])
    return outline, Document(BytesIO(artifacts.read_bytes(result['docx_ref'])))


def test_material_headers_keep_legacy_raw_and_distinguish_zero_from_unknown():
    assert directory.MaterialPlan.model_validate({'assignments': []}).model_dump()['table_headers'] == []
    for count in (0, None, 2):
        decision = {'table_id': 'source-table', 'count': count}
        assert directory.MaterialPlan.model_validate({'table_headers': [decision]}).model_dump()['table_headers'] == [decision]
    with pytest.raises(ValidationError):
        directory.MaterialPlan.model_validate({'table_headers': [{'table_id': 'source-table'}]})


def test_catalog_has_complete_lossless_unique_bound_tables_and_stable_source_identity(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    _fixed, documents, compiled = native_case(artifacts, extra=True)
    original = deepcopy(documents)
    chapter = compiled['chapters'][0]
    chapter['material_segments'] = [{'part': deepcopy(chapter['template_parts'][0])}]
    catalog = directory.table_header_catalog(compiled, documents)
    assert len(catalog) == 1, 'Unplaced tables are not adopted for header selection.'
    assert catalog[0]['grid'] == _grid_view(native_grid())
    assert len(catalog[0]['grid']['rows']) == 4
    assert catalog[0]['grid']['rows'][-1] == ['', '', '']
    assert catalog[0]['document_id'] == 'document' and catalog[0]['page'] == 1
    assert documents == original
    legacy_documents = deepcopy(documents)
    for table in legacy_documents[0]['grids']:
        table.pop('grid')
    assert directory.reading_payload(documents) == directory.reading_payload(legacy_documents)

    other = deepcopy(documents[0])
    other['document_id'] = 'amendment'
    other['role'] = 'amendment'
    for part in other['template_parts'].values():
        part['source_ref']['source_index_ref']['sha256'] = 'b' * 64
    copied = deepcopy(chapter)
    copied['id'] = 'other'
    copied['template_parts'] = [deepcopy(next(iter(other['template_parts'].values())))]
    copied.pop('template_segments', None)
    copied.pop('material_segments', None)
    compiled['chapters'].append(copied)
    both = directory.table_header_catalog(compiled, [*documents, other])
    assert len(both) == 2 and len({entry['table_id'] for entry in both}) == 2
    assert both[0]['unit_id'] == both[1]['unit_id'], 'Document-local unit IDs must stay distinct.'
    assert {entry['table_id'] for entry in directory.table_header_catalog(compiled, [other, *documents])} == {
        entry['table_id'] for entry in both}


@pytest.mark.parametrize('field', ['template_segments', 'material_segments'])
def test_catalog_and_selection_include_tables_carried_only_by_segments(tmp_path, field):
    artifacts = ArtifactStore(tmp_path)
    _fixed, documents, compiled = native_case(artifacts)
    chapter = compiled['chapters'][0]
    part = chapter.pop('template_parts')[0]
    chapter.pop('template_segments', None)
    chapter[field] = [{'part': part}]
    catalog = directory.table_header_catalog(compiled, documents)
    assert len(catalog) == 1
    directory.apply_table_headers(compiled, documents, catalog, [{'table_id': catalog[0]['table_id'], 'count': 2}])
    assert part['header_row_count'] == 2


def test_catalog_keeps_uncompactable_grid_attributes_and_checks_original_merge_boundary(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    _fixed, documents, compiled = native_case(artifacts)
    grid = documents[0]['grids'][0]['grid']
    grid['cells'][0]['source_note'] = 'extra native metadata'
    catalog = directory.table_header_catalog(compiled, documents)
    assert catalog[0]['grid'] == grid and 'cells' in catalog[0]['grid']
    with pytest.raises(directory.DirectoryPlanRejected, match='merged cell'):
        directory.validate_table_headers(catalog, [{'table_id': catalog[0]['table_id'], 'count': 1}])


@pytest.mark.parametrize('count', [0, 2])
def test_known_count_is_not_offered_or_overwritten_and_unknown_copies_inherit(tmp_path, count):
    artifacts = ArtifactStore(tmp_path)
    _fixed, documents, compiled = native_case(artifacts)
    unknown = deepcopy(compiled['chapters'][0]['template_parts'][0])
    compiled['chapters'][0]['material_segments'] = [{'part': unknown}]
    all_parts = list(parts(compiled))
    all_parts[0]['header_row_count'] = count
    assert directory.table_header_catalog(compiled, documents) == []
    directory.apply_table_headers(compiled, documents, [], [])
    assert all(part['header_row_count'] == count for part in all_parts)
    assert unknown['header_row_count'] == count
    assert not header_issues(compiled)


def test_reliable_native_header_metadata_is_retained_without_model_restatement(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    headers = [{'row': cell['row'], 'column': cell['column'], 'role': 'column_header'}
               for cell in native_grid()['cells'] if cell['row'] < 2]
    fixed, documents, compiled = native_case(artifacts, native_headers=headers)
    assert directory.table_header_catalog(compiled, documents) == []
    directory.apply_table_headers(compiled, documents, [], [])
    assert all(part['header_row_count'] == 2 for part in parts(compiled))
    assert not header_issues(compiled)
    _outline, document = word(artifacts, fixed, compiled)
    assert [bool(row._tr.xpath('./w:trPr/w:tblHeader')) for row in document.tables[0].rows] == [True, True, False, False]


@pytest.mark.parametrize('headers', [
    [{'row': 0, 'column': 0, 'role': 'column_header'}],
    [{'row': 1, 'column': 1, 'role': 'column_header'}],
])
def test_incomplete_or_nonleading_native_header_metadata_does_not_resolve_unknown(tmp_path, headers):
    artifacts = ArtifactStore(tmp_path)
    _fixed, documents, compiled = native_case(artifacts, native_headers=headers)
    assert len(directory.table_header_catalog(compiled, documents)) == 1
    assert all(part.get('header_row_count') is None for part in parts(compiled))


@pytest.mark.parametrize('count', [0, 2])
def test_valid_selection_reaches_every_part_and_subject_word_without_changing_grid(tmp_path, count):
    artifacts = ArtifactStore(tmp_path)
    fixed, documents, compiled = native_case(artifacts, subjects=True)
    chapter = compiled['chapters'][0]
    chapter['material_segments'] = [{'part': deepcopy(chapter['template_parts'][0])}]
    catalog = directory.table_header_catalog(compiled, documents)
    directory.apply_table_headers(compiled, documents, catalog, [{'table_id': catalog[0]['table_id'], 'count': count}])
    assert all(part['header_row_count'] == count for part in parts(compiled))
    assert not header_issues(compiled)
    outline, document = word(artifacts, fixed, compiled)
    forms = [target for target in outline['chapters'][0]['targets'] if target.get('template_refs')]
    assert {target['subject'] for target in forms} == {'投标人', '原厂'}
    assert all(part['header_row_count'] == count for target in forms for part in target['template_refs'])
    assert document.tables
    for table in document.tables:
        assert len(table.rows) == 4
        assert [bool(row._tr.xpath('./w:trPr/w:tblHeader')) for row in table.rows] == [row < count for row in range(4)]
        assert table.cell(0, 0)._tc is table.cell(1, 0)._tc
        assert table.cell(0, 1)._tc is table.cell(0, 2)._tc
        assert [cell.text for cell in table.rows[-1].cells] == ['', '', '']
        assert table.cell(2, 0).text == '资产' and table.cell(2, 1).text == ''
    assert documents[0]['grids'][0]['grid'] == native_grid()


@pytest.mark.parametrize('kind', ['unknown', 'too_many', 'negative', 'boolean', 'split_merge', 'contradiction'])
async def test_invalid_header_decision_never_commits_successful_response_receipt(tmp_path, monkeypatch, kind):
    artifacts, store = ArtifactStore(tmp_path), Receipts()
    _fixed, documents, compiled = native_case(artifacts)
    offered = directory.table_header_catalog(compiled, documents)[0]['table_id']
    decisions = [{'table_id': offered, 'count': 2}]
    if kind == 'unknown':
        decisions[0]['table_id'] = 'not-offered'
    elif kind == 'contradiction':
        decisions.append({'table_id': offered, 'count': 0})
    else:
        decisions[0]['count'] = {'too_many': 4, 'negative': -1, 'boolean': True, 'split_merge': 1}[kind]

    async def model(_model, request, validate):
        value = payload(request)
        if 'table_header_catalog' not in value:
            return validate({'assignments': []} if 'block' in value else {'items': []})
        wire_table = value['table_header_catalog'][0]
        assert wire_table['grid'] == directory.table_header_catalog(compiled, documents)[0]['grid']
        wire_decisions = [{**decision, 'table_id': wire_table['table_id']
                          if decision['table_id'] == offered else decision['table_id']} for decision in decisions]
        return validate({'assignments': [], 'table_headers': wire_decisions})

    monkeypatch.setattr(execution._DocumentModel, '__call__', model)
    with pytest.raises(ModelContractError):
        await pipeline._request_directory_details(compiled, documents, budget(), executor(artifacts, store, budget()))
    assert store.rows == {}
    assert all(part.get('header_row_count') is None for part in parts(compiled))


@pytest.mark.parametrize('selection', ['missing', 'null'])
async def test_unknown_selection_preserves_table_and_creates_specific_real_word_exit(tmp_path, selection):
    artifacts = ArtifactStore(tmp_path)
    fixed, documents, compiled = native_case(artifacts)
    offered = directory.table_header_catalog(compiled, documents)[0]

    async def execute(task, request, validate):
        if task == 'directory_material':
            headers = [] if selection == 'missing' else [
                {'table_id': payload(request)['table_header_catalog'][0]['table_id'], 'count': None}]
            return validate({'assignments': [], 'table_headers': headers, 'chapter_titles': fixture_titles(payload(request))})
        return validate({'assignments': []} if task == 'directory_requirement' else {'items': []})

    await pipeline._request_directory_details(compiled, documents, budget(), execute)
    issues = header_issues(compiled)
    assert len(issues) == 1
    assert all(value in issues[0]['detail'] for value in ('document', '第2页', offered['unit_id'], offered['table_id'],
                                                        '在 Word 确认重复标题行后保存'))
    _outline, document = word(artifacts, fixed, compiled)
    texts = [paragraph.text for paragraph in document.paragraphs]
    assert '适用性与来源待核' in texts
    assert issues[0]['detail'] in texts
    assert '处理结果：________（人工核定）' in texts
    assert all(not row._tr.xpath('./w:trPr/w:tblHeader') for row in document.tables[0].rows)
    assert [cell.text for cell in document.tables[0].rows[-1].cells] == ['', '', '']


@pytest.mark.parametrize('error', [ModelWorkUnitExhausted, ModelBudgetError])
async def test_material_failure_preserves_native_table_and_has_source_located_header_exit(tmp_path, error):
    artifacts = ArtifactStore(tmp_path)
    _fixed, documents, compiled = native_case(artifacts)
    offered = directory.table_header_catalog(compiled, documents)[0]

    async def execute(task, _request, validate):
        if task == 'directory_material':
            raise error('fixture')
        return validate({'assignments': []} if task == 'directory_requirement' else {'items': []})

    await pipeline._request_directory_details(compiled, documents, budget(), execute)
    assert len(header_issues(compiled)) == 1
    assert offered['table_id'] in header_issues(compiled)[0]['detail']
    assert all(part.get('header_row_count') is None for part in parts(compiled))
    assert documents[0]['grids'][0]['grid'] == native_grid()


async def test_new_material_table_is_preserved_and_explicitly_unresolved_without_catalog_adoption(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    _fixed, documents, compiled = native_case(artifacts, extra=True)
    offered = directory.table_header_catalog(compiled, documents)
    assert len(offered) == 1

    async def execute(task, request, validate):
        if task == 'directory_material':
            value = payload(request)
            assert len(value['table_header_catalog']) == len(offered)
            assert all(value['table_header_catalog'][0][key] == offered[0][key]
                       for key in ('grid', 'page', 'unit_id'))
            return validate({'assignments': [{'chapter_id': value['chapters'][0]['id'], **window(
                2, 0, len(documents[0]['pages'][2].splitlines()) - 1, value['documents'][0]['document_id'])}],
                'table_headers': [{'table_id': value['table_header_catalog'][0]['table_id'], 'count': 2}],
                'chapter_titles': fixture_titles(value)})
        return validate({'assignments': []} if task == 'directory_requirement' else {'items': []})

    await pipeline._request_directory_details(compiled, documents, budget(), execute)
    current = directory.table_header_catalog(compiled, documents)
    assert len(current) == 1, 'The resolved original table no longer needs another model decision.'
    issues = header_issues(compiled)
    assert len(issues) == 1
    added = current[0]
    assert added['table_id'] != offered[0]['table_id']
    assert added['table_id'] in issues[0]['detail'] and '第3页' in issues[0]['detail']
    assert any(part.get('header_row_count') == 2 for part in parts(compiled))
    assert any(part.get('header_row_count') is None for part in parts(compiled))
