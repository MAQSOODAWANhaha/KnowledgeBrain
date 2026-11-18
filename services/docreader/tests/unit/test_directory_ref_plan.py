"""Compact selections must compile to the same source-owned directory contract."""

from copy import deepcopy

import pytest

from docreader.llm.request import ModelContractError
from docreader.tender.directory_compile import compile_directory
from docreader.tender.directory_sources import SourceRefs


def decode(raw, index, **options):
    from docreader.tender.directory_ref_plan import decode_plan
    return decode_plan(raw, index, **options)


def plan(*, body=None):
    return {'nodes': [{'heading_ref': 1, 'body_ranges': [[2, 4]] if body is None else body,
                      'response_kind': 'form', 'subjects': [{'source': [2, 2], 'role': '投标人'}]}],
            'composition': {'entries': [{'name_ref': 1}]},
            'scopes': [{'title_ref': 0, 'items': [{'node_ref': 1, 'entry_ref': 1,
                         'depth': 'self', 'presentation': 'section', 'condition': [5, 5], 'clause': [[5, 5]]}]}]}


def documents():
    return [{'document_id': 'original', 'role': 'main', 'pages': [
        '商务文件\n投标函\n签署主体：投标人\n项目：________\n\n代理商投标时提供', '第二页正文\n签字：________']}]


def test_compact_plan_keeps_source_body_subject_condition_and_group():
    source = documents()
    raw = plan()
    before = deepcopy(raw)
    old = decode(raw, SourceRefs(source))
    compiled = compile_directory(old, source)
    chapter = compiled['chapters'][0]
    assert chapter['title'] == '投标函' and chapter['group'] == '商务文件'
    assert chapter['subjects'][0]['role'] == '投标人'
    assert chapter['condition'] == '代理商投标时提供'
    assert old['nodes'][0]['body_windows'] == [{'document_id': 'original', 'page': 0, 'start_line': 2, 'end_line': 4}]
    assert chapter['body_windows'] == [('original', 0, 2, 4)]
    assert [entry['line'] for entry in chapter['lines']] == [2, 3, 4]
    assert raw == before


def test_ref_range_reconstructs_cross_page_body_without_inventing_a_boundary():
    source = [{'document_id': 'original', 'role': 'main', 'pages': ['原式\n首行', '续行\n末行']}]
    raw = {'nodes': [{'heading_ref': 0, 'body_ranges': [[1, 3]], 'response_kind': 'form'}],
           'scopes': [{'items': [{'node_ref': 0, 'depth': 'self', 'presentation': 'section'}]}]}
    compiled = compile_directory(decode(raw, SourceRefs(source)), source)
    assert compiled['chapters'][0]['text'] == '首行\n续行\n末行'
    assert len(compiled['chapters'][0]['body_windows']) == 2


def test_empty_explicit_body_never_expands_to_the_next_title():
    raw = plan(body=[])
    compiled = compile_directory(decode(raw, SourceRefs(documents())), documents())
    assert compiled['chapters'][0]['text'] == ''


def test_host_node_ids_stay_bound_to_source_points_when_unrelated_sources_precede_them():
    source = documents()
    shifted = [{'document_id': 'unrelated', 'role': 'attachment', 'pages': ['独立原文']}, *source]
    first = decode(plan(), SourceRefs(source))
    raw = plan()
    raw['nodes'][0].update(heading_ref=2, body_ranges=[[3, 5]], subjects=[{'source': [3, 3], 'role': '投标人'}])
    raw['composition']['entries'][0]['name_ref'] = 2
    raw['scopes'] = [{'title_ref': 1, 'items': [{'node_ref': 2, 'entry_ref': 2, 'depth': 'self',
                          'presentation': 'section', 'condition': [6, 6], 'clause': [[6, 6]]}]}]
    second = decode(raw, SourceRefs(shifted))
    assert first == second


@pytest.mark.parametrize('problem', ['unknown', 'hidden', 'cross_page_condition', 'unknown_entry',
                                    'unknown_node', 'boolean', 'copied_identity', 'duplicate_node'])
def test_invalid_ref_plan_is_rejected_before_a_business_receipt(problem):
    raw, options = plan(), {}
    if problem == 'unknown':
        raw['nodes'][0]['body_ranges'] = [[2, 99]]
    elif problem == 'hidden':
        options['visible'] = {0, 1, 2, 3, 5}
    elif problem == 'cross_page_condition':
        raw['scopes'][0]['items'][0]['condition'] = [5, 6]
    elif problem == 'unknown_entry':
        raw['scopes'][0]['items'][0]['entry_ref'] = 3
    elif problem == 'unknown_node':
        raw['scopes'][0]['items'][0]['node_ref'] = 3
    elif problem == 'boolean':
        raw['nodes'][0]['heading_ref'] = True
    elif problem == 'copied_identity':
        raw['nodes'][0]['id'] = 'model-created-identity'
    else:
        raw['nodes'].append(deepcopy(raw['nodes'][0]))
    with pytest.raises(ModelContractError):
        decode(raw, SourceRefs(documents()), **options)
