"""Physical source slices cannot become separate responses after parameter expansion."""

from typing import TypeVar
from copy import deepcopy
from io import BytesIO

from docx import Document
from docx.oxml.ns import qn
import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.tender import authoring_pipeline as pipeline
from docreader.tender.directory_compile import (
    apply_parameters, compile_directory, documents_from_frozen, outline_from_compiled,
)
from test_authoring_pipeline_boundaries import budget, snapshot
from test_directory_compile import docs, item, node, plan, window
from test_directory_parameter_continuations import parameter, response, source_case

_T = TypeVar("_T")


def _required(value: _T | None) -> _T:
    assert value is not None
    return value



SNAPSHOT = {'schema_version': 2, 'run_id': 'run', 'source_version': 1, 'domain_revision': 1}


def test_expanded_parent_keeps_source_and_one_whole_response_for_unexpanded_requirements():
    documents, compiled = source_case()
    original = deepcopy(compiled['chapters'][0])
    apply_parameters(compiled, documents, response(parameter())['chapters'])
    parent = outline_from_compiled(SNAPSHOT, compiled)['chapters'][0]
    assert parent['targets'][0]['kind'] == 'form'
    assert parent['targets'][0]['purpose'] == original['text']
    whole = [target for target in parent['targets'] if target['kind'] == 'narrative']
    assert len(whole) == 1 and original['title'] in whole[0]['purpose']
    assert '整体响应' in whole[0]['purpose']
    assert all(target['manual_fill'] for target in parent['targets'])
    assert parent['children'][0]['targets'][0]['kind'] == 'narrative'
    assert parent['children'][0]['targets'][0]['purpose'] == compiled['chapters'][1]['text']
    assert compiled['chapters'][0] == original


def test_expanded_parent_preserves_independent_obligation_requirement_and_condition_responses():
    documents, compiled = source_case()
    compiled['chapters'][0].update(obligations='独立投标义务。', requirements=['独立章节要求。'],
                                   condition='独立适用条件。', materials=['独立材料要求。'],
                                   material_segments=[{'text': '独立材料原文。'}])
    apply_parameters(compiled, documents, response(parameter())['chapters'])
    parent = outline_from_compiled(SNAPSHOT, compiled)['chapters'][0]
    for text in ('独立投标义务。', '独立章节要求。', '独立适用条件。', '独立材料要求。', '独立材料原文。'):
        matches = [target for target in parent['targets'] if target['purpose'] == text
                   and not target['target_id'].endswith(':answer')]
        assert len(matches) == 1 and matches[0]['kind'] == 'narrative'
    assert parent['targets'][0]['kind'] == 'form'


def test_ordinary_children_and_rejected_parameter_plans_do_not_change_parent_response_roles():
    documents = docs(['父章\n父章独立要求\n子章\n子章独立要求'])
    chosen = [{**node('parent', 0, 0), 'response_kind': 'narrative'},
              {**node('parent:parameter:1', 0, 2, parent='parent'), 'response_kind': 'narrative'}]
    compiled = compile_directory(plan(chosen, [item('parent', 'subtree')]), documents)
    parent = outline_from_compiled(SNAPSHOT, compiled)['chapters'][0]
    assert [target['kind'] for target in parent['targets']] == ['narrative']
    documents, compiled = source_case()
    before = outline_from_compiled(SNAPSHOT, compiled)['chapters']
    apply_parameters(compiled, documents, response(parameter(continuation_windows=[window(0, 23, 24)]))['chapters'])
    assert outline_from_compiled(SNAPSHOT, compiled)['chapters'] == before


@pytest.mark.parametrize('kind', ['form', 'attachment'])
def test_parameter_expansion_preserves_non_narrative_parent_and_child_roles(kind):
    documents, compiled = source_case()
    compiled['chapters'][0]['response_kind'] = kind
    before = outline_from_compiled(SNAPSHOT, compiled)['chapters'][0]['targets']
    apply_parameters(compiled, documents, response(parameter())['chapters'])
    parent = outline_from_compiled(SNAPSHOT, compiled)['chapters'][0]
    assert parent['targets'] == before
    assert all(target['kind'] == kind for target in parent['children'][0]['targets'])


def test_parameter_marker_cannot_change_an_unrelated_parent():
    documents, compiled = source_case()
    before = outline_from_compiled(SNAPSHOT, compiled)['chapters'][0]['targets']
    apply_parameters(compiled, documents, response(parameter())['chapters'])
    compiled['chapters'][1]['parameter_parent_id'] = 'unrelated'
    assert outline_from_compiled(SNAPSHOT, compiled)['chapters'][0]['targets'] == before


async def test_native_continuation_tables_have_no_intervening_response_and_keep_all_outlets(tmp_path):
    artifacts = ArtifactStore(tmp_path / 'artifacts')

    def grid(text):
        return {'row_count': 2, 'column_count': 2, 'cells': [
            {'row': row, 'column': column, 'text': value, 'row_span': 1, 'col_span': 1}
            for row, values in enumerate([['技术要求', '投标响应'], [text, '']])
            for column, value in enumerate(values)]}

    fixed = snapshot(artifacts, [
        {'text': '\n'.join(['技术规格', *[f'其他要求{index}' for index in range(25)]])},
        {'grid': grid('支持一对一、一对多、多对一')},
        {'grid': grid('转换的源、目的地址转换；须提供证明。')},
    ])
    documents = documents_from_frozen(artifacts, artifacts.read_json(fixed['frozen_ref']))
    pages = documents[0]['pages']
    chosen = {**node('technical', 0, 0, document_id='document'), 'response_kind': 'narrative',
              'body_windows': [window(page, 0, len(text.splitlines()) - 1, 'document')
                               for page, text in enumerate(pages)]}
    compiled = compile_directory(plan([chosen], [item('technical')]), documents)
    apply_parameters(compiled, documents, [{'chapter_id': 'technical', 'items': [{
        'document_id': 'document', 'page': 1, 'start_line': 1, 'end_line': 1,
        'content_start_line': 1, 'continuation_windows': [window(2, 1, document_id='document')],
    }]}])
    outline = outline_from_compiled(fixed, compiled)

    async def model(task, _request, validate):
        assert task == 'authoring_structure'
        return validate({'findings': []})

    result = artifacts.read_json(await pipeline.generate_document(
        artifacts, fixed, budget(), model, outline=outline))
    document = Document(BytesIO(artifacts.read_bytes(result['docx_ref'])))
    assert len(document.tables) == 2
    assert [table.cell(1, 1).text for table in document.tables] == ['', '']
    assert document.tables[0].cell(1, 0).text == '支持一对一、一对多、多对一'
    assert document.tables[1].cell(1, 0).text == '转换的源、目的地址转换；须提供证明。'
    body = list(document.element.body)
    tables = [index for index, element in enumerate(body) if element.tag == qn('w:tbl')]
    between = '\n'.join(''.join(element.itertext()) for element in body[tables[0] + 1:tables[1]])
    assert '我方响应' not in between and '证明材料或附件位置' not in between
    child = outline['chapters'][0]['children'][0]
    paragraphs = [paragraph.text for paragraph in document.paragraphs]
    child_start = next(index for index, paragraph in enumerate(document.paragraphs)
                       if _required(_required(paragraph.style).name).startswith('Heading') and paragraph.text == child['title'])
    assert sum('我方响应：' in text for text in paragraphs[:child_start]) == 1
    assert sum('我方响应：' in text for text in paragraphs[child_start:]) == 1
    assert '转换的源、目的地址转换；须提供证明。' in '\n'.join(paragraphs[child_start:])
