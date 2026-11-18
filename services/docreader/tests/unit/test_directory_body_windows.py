"""New plans own exact body coordinates; legacy raw plans retain their old replay."""

from copy import deepcopy

from pydantic import ValidationError
import pytest

from docreader.tender.directory_compile import (
    DirectoryNode, DirectoryPlanRejected, apply_parameters, compile_directory, compile_plans,
)
from test_directory_compile import docs, entry, item, node, plan, point, window


def explicit(identity, line, windows, *, parent=None, document_id='tender'):
    return {**node(identity, 0, line, parent, document_id=document_id), 'body_windows': windows}


@pytest.mark.parametrize('body', [{}, {'body_windows': None}])
def test_model_contract_requires_body_windows_even_for_empty_navigation(body):
    with pytest.raises(ValidationError):
        DirectoryNode.model_validate({'id': 'nav', 'heading': point(0, 0), 'response_kind': 'form', **body})
    assert DirectoryNode.model_validate({'id': 'nav', 'heading': point(0, 0), 'response_kind': 'form', 'body_windows': []}).body_windows == []


def test_empty_body_is_navigation_and_does_not_copy_until_the_next_heading():
    compiled = compile_directory(plan([explicit('nav', 0, [])], [item('nav')]),
                                 docs(['组成说明\n不应作为原式的评审工作表\n末行']))
    chapter = compiled['chapters'][0]
    assert chapter['text'] == '' and chapter['lines'] == []
    assert chapter['body_windows'] == []


def test_explicit_windows_copy_only_chosen_lines_in_registered_order_once():
    documents = docs(['标题\n1\n共同页眉\n正文'],
                     {'document_id': 'attachment', 'role': 'attachment', 'pages': ['附件正文']})
    chosen = [window(0, 0, document_id='attachment'), window(0, 2, 3), window(0, 1, 2)]
    compiled = compile_directory(plan([explicit('form', 0, chosen)], [item('form')]), documents)
    chapter = compiled['chapters'][0]
    assert chapter['text'] == '1\n共同页眉\n正文\n附件正文'
    assert [(line['document_id'], line['page'], line['line']) for line in chapter['lines']] == [
        ('tender', 0, 1), ('tender', 0, 2), ('tender', 0, 3), ('attachment', 0, 0)]
    assert chapter['body_windows'] is not None


def test_explicit_parent_and_child_keep_only_their_own_source_body():
    compiled = compile_directory(plan([
        explicit('parent', 0, [window(0, 1)]), explicit('child', 2, [window(0, 3)], parent='parent'),
    ], [item('parent', 'subtree')]), docs(['父标题\n父正文\n子标题\n子正文\n不得扩入']))
    assert [(chapter['text'], chapter['parent_id']) for chapter in compiled['chapters']] == [
        ('父正文', None), ('子正文', 'parent')]


def test_raw_legacy_plan_without_windows_preserves_previous_body_replay():
    compiled = compile_directory(plan([node('legacy', 0, 0)], [item('legacy')]), docs(['原式\n正文']))
    assert compiled['chapters'][0]['text'] == '原式\n正文'
    assert compiled['chapters'][0].get('body_windows') is None


@pytest.mark.parametrize('chosen', [
    window(0, 0, 8), window(0, 2, 1), window(0, 0, document_id='missing'),
])
def test_invalid_explicit_windows_remain_rejected(chosen):
    with pytest.raises(DirectoryPlanRejected, match='outside|continuous|unknown'):
        compile_directory(plan([explicit('form', 0, [chosen])], [item('form')]), docs(['原式\n正文']))


def table_documents(*, owners=True):
    unit = 'page:0:table:0'
    return [{'document_id': 'tender', 'role': 'main', 'pages': ['原式\n表头\n内容一\n内容二'],
             'line_units': [[None, unit, unit, unit] if owners else [None] * 4],
             'template_parts': {unit: {'source_ref': {'source_index_ref': {'key': 'source'}, 'unit_id': unit}, 'selectors': []}},
             'grids': [{'unit_id': unit, 'page': 0, 'rows': [['表头'], ['内容一'], ['内容二']]}]}]


def test_partial_native_table_keeps_exact_text_and_reports_the_unbound_table():
    compiled = compile_directory(plan([explicit('form', 0, [window(0, 1, 2)])], [item('form')]), table_documents())
    chapter = compiled['chapters'][0]
    assert chapter['text'] == '表头\n内容一'
    assert not chapter.get('template_parts')
    assert any(issue['code'] == 'outline_directory_partial_table'
               and 'page:0:table:0' in issue['detail'] for issue in compiled['issues'])


def test_complete_table_still_binds_without_crossing_the_explicit_window():
    compiled = compile_directory(plan([explicit('form', 0, [window(0, 1, 3)])], [item('form')]), table_documents())
    chapter = compiled['chapters'][0]
    assert chapter['text'] == '表头\n内容一\n内容二'
    assert len(chapter['template_parts']) == 1
    assert not any(issue['code'] == 'outline_directory_partial_table' for issue in compiled['issues'])


def test_wrapped_table_fallback_cannot_expand_a_new_explicit_window():
    compiled = compile_directory(plan([explicit('form', 0, [window(0, 1, 2)])], [item('form')]),
                                 table_documents(owners=False))
    assert compiled['chapters'][0]['text'] == '表头\n内容一'
    assert not compiled['chapters'][0].get('template_parts')
    assert any(issue['code'] == 'outline_directory_table_unbound'
               and 'page:0:table:0' in issue['detail'] for issue in compiled['issues'])


def test_shared_source_is_not_mechanically_deleted_across_response_chapters():
    documents = table_documents()
    documents[0]['pages'].append('甲独立提交物\n乙独立提交物')
    compiled = compile_directory(plan([
        explicit('first', 0, [window(0, 1, 3)]), explicit('second', 1, [window(0, 1, 3)]),
    ], [item('first', entry=0), item('second', entry=1)], [entry(1, 0), entry(1, 1)]), documents)
    assert all(len(chapter['template_parts']) == 1 for chapter in compiled['chapters'])


def test_same_text_at_different_source_coordinates_remains_a_plan_conflict():
    first = plan([explicit('form', 0, [window(0, 1)])], [item('form')])
    second = plan([explicit('form', 0, [window(0, 2)])], [item('form')])
    compiled = compile_plans([first, second], docs(['原式\n相同正文\n相同正文']))
    assert compiled['chapters'][0]['lines'][0]['line'] == 1
    assert any(issue['code'] == 'outline_directory_conflict' for issue in compiled['issues'])


def test_different_composition_groups_remain_a_plan_conflict():
    nodes = [explicit('form', 0, [window(0, 1)])]
    first = plan(nodes, [item('form')], title=point(1, 0))
    second = plan(nodes, [item('form')], title=point(1, 1))
    compiled = compile_plans([first, second], docs(['原式\n正文', '甲分册\n乙分册']))
    assert compiled['chapters'][0]['group'] == '甲分册'
    assert any(issue['code'] == 'outline_directory_conflict' for issue in compiled['issues'])


def test_same_unit_ids_from_two_documents_keep_both_template_parts_in_source_order():
    documents = []
    for identity, title in [('tender', '甲原式'), ('attachment', '乙原式')]:
        unit = 'page:0:table:0'
        documents.append({'document_id': identity, 'role': 'main' if identity == 'tender' else 'attachment',
                          'pages': [title + '\n内容'], 'line_units': [[unit, unit]],
                          'template_parts': {unit: {'source_ref': {'source_index_ref': {'key': identity},
                                                                'unit_id': unit}, 'selectors': []}}})
    compiled = compile_directory(plan([explicit('form', 0, [
        window(0, 0, 1, document_id='attachment'), window(0, 0, 1),
    ])], [item('form')]), documents)
    chapter = compiled['chapters'][0]
    assert [part['source_ref']['source_index_ref']['key'] for part in chapter['template_parts']] == ['tender', 'attachment']
    assert [segment['part']['source_ref']['source_index_ref']['key']
            for segment in chapter['template_segments'] if segment.get('part')] == ['tender', 'attachment']


def test_parameter_title_and_body_must_be_inside_the_explicit_parent_source():
    pages = ['\n'.join(['隐藏标题', *[f'行{number}' for number in range(25)], '未给出的尾行'])]
    compiled = compile_directory(plan([explicit('form', 0, [window(0, 1, 25)])], [item('form')]), docs(pages))
    parent = deepcopy(compiled['chapters'][0])
    apply_parameters(compiled, docs(pages), [{'chapter_id': 'form', 'items': [
        {'document_id': 'tender', 'page': 0, 'start_line': 0, 'end_line': 2, 'content_start_line': 1},
        {'document_id': 'tender', 'page': 0, 'start_line': 24, 'end_line': 26, 'content_start_line': 24},
    ]}])
    assert compiled['chapters'] == [parent]
    assert sum(issue['code'] == 'outline_directory_parameter_rejected' for issue in compiled['issues']) == 2


def test_parameter_children_do_not_replace_the_original_parent_table():
    unit = 'page:0:table:0'
    pages = ['\n'.join(f'行{number}' for number in range(25))]
    documents = [{'document_id': 'tender', 'role': 'main', 'pages': pages, 'line_units': [[unit] * 25],
                  'template_parts': {unit: {'source_ref': {'source_index_ref': {'key': 'source'}, 'unit_id': unit}, 'selectors': []}}}]
    compiled = compile_directory(plan([explicit('form', 0, [window(0, 0, 24)])], [item('form')]), documents)
    parent = deepcopy(compiled['chapters'][0])
    apply_parameters(compiled, documents, [{'chapter_id': 'form', 'items': [
        {'document_id': 'tender', 'page': 0, 'start_line': 0, 'end_line': 3, 'content_start_line': 1}]}])
    assert compiled['chapters'][0] == parent
    assert compiled['chapters'][1]['text'] == '行1\n行2\n行3'
