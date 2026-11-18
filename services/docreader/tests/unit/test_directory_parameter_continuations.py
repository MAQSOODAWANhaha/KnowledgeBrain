"""A source parameter stays one response when its complete clause crosses pages."""

from copy import deepcopy
from io import BytesIO
from types import SimpleNamespace

from docx import Document
import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.request import ModelContractError
from docreader.tender import authoring_pipeline as pipeline
from docreader.tender.directory_compile import (
    ParameterPlan, apply_parameters, compile_directory, documents_from_frozen, outline_from_compiled,
)
from test_authoring_pipeline_boundaries import budget, snapshot
from test_directory_compile import docs, item, node, plan, window
from test_directory_plan_replay import Receipts


def source_case():
    pages = ['\n'.join(['技术要求', *[f'背景行{n}' for n in range(23)], '★支持数字、字']),
             '页眉（不在已选正文内）\n母及符号；否则须提供完整替代方案。\n证明材料须厂家盖章。\n下一独立要求。']
    documents = docs(pages)
    chosen = {**node('technical', 0, 0), 'response_kind': 'narrative',
              'body_windows': [window(0, 0, 24), window(1, 1, 3)]}
    compiled = compile_directory(plan([chosen], [item('technical')]), documents)
    return documents, compiled


def parameter(**extra):
    return {'document_id': 'tender', 'page': 0, 'start_line': 24, 'end_line': 24,
            'content_start_line': 24, 'continuation_windows': [window(1, 1, 2)], **extra}


def response(value):
    return {'chapters': [{'chapter_id': 'technical', 'items': [value]}]}


def test_cross_page_parameter_preserves_the_complete_condition_in_one_response():
    documents, compiled = source_case()
    original = deepcopy(compiled['chapters'][0])
    apply_parameters(compiled, documents, response(parameter())['chapters'])
    assert compiled['chapters'][0] == original
    assert len(compiled['chapters']) == 2
    child = compiled['chapters'][1]
    assert child['text'] == '★支持数字、字\n母及符号；否则须提供完整替代方案。\n证明材料须厂家盖章。'
    assert [(line['page'], line['line']) for line in child['lines']] == [(0, 24), (1, 1), (1, 2)]
    assert child['response_kind'] == 'narrative'


@pytest.mark.parametrize('windows', [
    [window(1, 0, 2)],  # The new source page contains an unselected header.
    [window(0, 23, 24)],  # Reverse and repeat the first segment.
    [window(1, 1, 2), window(1, 2, 3)],
    [window(1, 1, 2, document_id='other')],
])
def test_bad_continuations_do_not_become_successful_parameter_receipts(windows):
    documents, compiled = source_case()
    before = deepcopy(compiled)
    with pytest.raises(ModelContractError) as caught:
        pipeline._accept_parameter_plan(response(parameter(continuation_windows=windows)),
                                        compiled, documents, 'technical')
    assert any(error['code'] == 'directory_parameter_source_invalid' for error in caught.value.errors)
    assert compiled == before


def test_two_items_cannot_duplicate_the_shared_continuation_source_line():
    documents, compiled = source_case()
    value = response(parameter())
    value['chapters'][0]['items'].append({
        'document_id': 'tender', 'page': 1, 'start_line': 2, 'end_line': 3, 'content_start_line': 2})
    with pytest.raises(ModelContractError) as caught:
        pipeline._accept_parameter_plan(value, compiled, documents, 'technical')
    assert any('overlap' in error['detail'] for error in caught.value.errors)
    apply_parameters(compiled, documents, value['chapters'])
    assert len(compiled['chapters']) == 2
    assert any(issue['code'] == 'outline_directory_parameter_rejected' for issue in compiled['issues'])


@pytest.mark.parametrize('chapter_ids', [['other'], ['technical', 'technical']])
def test_parameter_receipt_is_bound_to_one_requested_parent(chapter_ids):
    documents, compiled = source_case()
    value = {'chapters': [{'chapter_id': identity, 'items': [parameter()]} for identity in chapter_ids]}
    with pytest.raises(ModelContractError):
        pipeline._accept_parameter_plan(value, compiled, documents, 'technical')


async def test_invalid_continuation_is_rejected_before_the_paid_success_receipt(tmp_path, monkeypatch):
    import docreader.tender.document_execution as execution

    documents, compiled = source_case()
    artifacts, store = ArtifactStore(tmp_path), Receipts()
    client = SimpleNamespace(config=SimpleNamespace(chat=SimpleNamespace(base_url='https://fixture.invalid/v1')))
    executor = execution._DocumentExecutor(client, store, artifacts,
        SimpleNamespace(execution_id='invalid-parameter', epoch=1), budget(), {}, None)

    async def model(_model, _request, validate):
        return validate(response(parameter(continuation_windows=[window(1, 0, 2)])))

    monkeypatch.setattr(execution._DocumentModel, '__call__', model)
    with pytest.raises(ModelContractError):
        await pipeline._ask(executor, budget(), 'directory_parameter', {'chapter': {'id': 'technical'}},
                            lambda raw: pipeline._accept_parameter_plan(raw, compiled, documents, 'technical'))
    assert store.rows == {}


def test_legacy_same_page_parameter_replays_without_inventing_continuations():
    documents, compiled = source_case()
    value = response(parameter())
    value['chapters'][0]['items'][0].pop('continuation_windows')
    parsed = ParameterPlan.model_validate(value).model_dump()
    assert parsed['chapters'][0]['items'][0]['continuation_windows'] == []
    apply_parameters(compiled, documents, parsed['chapters'])
    assert compiled['chapters'][1]['text'] == '★支持数字、字'


async def test_cross_page_response_reaches_word_without_replacing_the_original_parent_table(tmp_path):
    artifacts = ArtifactStore(tmp_path / 'artifacts')
    grid = {'row_count': 2, 'column_count': 2, 'cells': [
        {'row': row, 'column': column, 'text': text, 'row_span': 1, 'col_span': 1}
        for row, values in enumerate([['项目', '响应'], ['原表要求', '']])
        for column, text in enumerate(values)]}
    fixed = snapshot(artifacts, [
        {'text': '\n'.join(['技术要求', *[f'背景行{n}' for n in range(23)], '★支持数字、字'])},
        {'text': '母及符号；否则须提供替代方案。\n证明材料须厂家盖章。'}, {'grid': grid}])
    documents = documents_from_frozen(artifacts, artifacts.read_json(fixed['frozen_ref']))
    chosen = {**node('technical', 0, 0, document_id='document'), 'response_kind': 'narrative',
              'body_windows': [window(0, 0, 24, 'document'), window(1, 0, 1, 'document'),
                               window(2, 0, 1, 'document')]}
    compiled = compile_directory(plan([chosen], [item('technical')]), documents)
    parent = deepcopy(compiled['chapters'][0])
    raw = parameter(document_id='document', continuation_windows=[window(1, 0, 1, 'document')])
    apply_parameters(compiled, documents, response(raw)['chapters'])
    assert compiled['chapters'][0] == parent
    outline = outline_from_compiled(fixed, compiled)
    child = outline['chapters'][0]['children'][0]
    assert child['targets'][0]['purpose'] == '★支持数字、字\n母及符号；否则须提供替代方案。\n证明材料须厂家盖章。'

    async def manual_model(task, _request, validate):
        assert task == 'authoring_structure'
        return validate({'findings': []})

    result = artifacts.read_json(await pipeline.generate_document(
        artifacts, fixed, budget(), manual_model, outline=outline))
    document = Document(BytesIO(artifacts.read_bytes(result['docx_ref'])))
    assert len(document.tables) == 1
    assert [[cell.text for cell in row.cells] for row in document.tables[0].rows] == [['项目', '响应'], ['原表要求', '']]
    paragraphs = [p.text for p in document.paragraphs]
    start = next(index for index, paragraph in enumerate(document.paragraphs)
                 if paragraph.style.name.startswith('Heading') and paragraph.text == child['title'])
    tail = '\n'.join(paragraphs[start:])
    assert '母及符号；否则须提供替代方案。' in tail and '证明材料须厂家盖章。' in tail
    assert '我方响应：________（人工填写）' in tail
    assert '证明材料或附件位置：________（原条款要求时填写）' in tail
