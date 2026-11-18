"""Source roles keep complete words across Chinese PDF line wraps."""

from copy import deepcopy

import pytest

from docreader.tender.directory_compile import DirectoryPlanRejected, compile_directory, outline_from_compiled
from test_directory_compile import docs, item, node, plan, window


def source_case(text, roles, *, line_units=None):
    documents = docs(['材料\n固定原式\n' + text])
    if line_units is not None:
        documents[0]['line_units'] = [[None, None, *line_units]]
    selected = window(0, 2, len(documents[0]['pages'][0].splitlines()) - 1)
    raw = plan([{**node('form', 0, 0), 'body_windows': [window(0, 1)],
        'subjects': [{'source': selected, 'role': role} for role in roles]}], [item('form')])
    return raw, documents


@pytest.mark.parametrize('role', ['非涉密环境运维人员', '非涉密环境运维人\n员', '非涉密环境运维人\n   员'])
def test_wrapped_role_keeps_the_complete_word_and_same_final_identity(role):
    raw, documents = source_case('非涉密环境运维人\n   员及其项目相关人员。', [role])
    before = deepcopy((raw, documents))
    compiled = compile_directory(raw, documents)
    assert compiled['chapters'][0]['subjects'] == [{'source': raw['nodes'][0]['subjects'][0]['source'],
                                                  'role': '非涉密环境运维人员'}]
    outline = outline_from_compiled({'schema_version': 2, 'run_id': 'wrapped-role',
        'source_version': 1, 'domain_revision': 1}, compiled)
    assert [(t['target_id'], t['subject']) for t in outline['chapters'][0]['targets']] == [
        ('chapter-1:target-1', '非涉密环境运维人员')]
    assert (raw, documents) == before


def test_wrapped_role_sorting_uses_real_source_order_instead_of_literal_index():
    raw, documents = source_case('投标\n  人签署。\n制造商签署。', ['制造商', '投标人'])
    compiled = compile_directory(raw, documents)
    assert [s['role'] for s in compiled['chapters'][0]['subjects']] == ['投标人', '制造商']


def test_two_aliases_for_the_same_physical_role_cannot_create_two_instances():
    raw, documents = source_case('非涉密环境运维人\n员签署。',
                                 ['非涉密环境运维人员', '非涉密环境运维人\n员'])
    with pytest.raises(DirectoryPlanRejected, match='duplicate subject'):
        compile_directory(raw, documents)


@pytest.mark.parametrize(('text', 'alias'), [('投标人\n其他文字', '投标人\n'), ('前行\n投标人', '\n投标人')])
def test_wrap_whitespace_at_role_edges_cannot_bypass_physical_span_uniqueness(text, alias):
    raw, documents = source_case(text, ['投标人', alias])
    with pytest.raises(DirectoryPlanRejected, match='duplicate subject'):
        compile_directory(raw, documents)


def test_subject_order_uses_the_matching_role_position_rather_than_its_context_window_start():
    raw, documents = source_case('引导\n制造商\n另行\n投标人', ['投标人', '制造商'])
    raw['nodes'][0]['subjects'][0]['source'] = window(0, 2, 5)
    raw['nodes'][0]['subjects'][1]['source'] = window(0, 3, 5)
    assert [s['role'] for s in compile_directory(raw, documents)['chapters'][0]['subjects']] == ['制造商', '投标人']


def test_overlapping_context_windows_cannot_duplicate_one_physical_role():
    raw, documents = source_case('引导\n制造商\n另行\n投标人', ['投标人', '投标人'])
    raw['nodes'][0]['subjects'][0]['source'] = window(0, 2, 5)
    raw['nodes'][0]['subjects'][1]['source'] = window(0, 5)
    with pytest.raises(DirectoryPlanRejected, match='duplicate subject'):
        compile_directory(raw, documents)


@pytest.mark.parametrize(('text', 'role'), [
    ('投标人\n\n制造商', '投标人制造商'),
    ('项目：\n经理', '项目：经理'),
    ('project\nmanager', 'projectmanager'),
    ('12\n34', '1234'),
    ('项目 经理', '项目经理'),
    ('非涉密环境运维人\n员', '非涉密环境维保人员'),
    ('非涉密环境运维人\n员', '非涉密环境运维员'),
])
def test_source_wrap_matching_never_invents_characters_or_erases_other_boundaries(text, role):
    raw, documents = source_case(text, [role])
    with pytest.raises(DirectoryPlanRejected, match='exact fragment'):
        compile_directory(raw, documents)


def test_grid_line_boundary_is_not_assumed_to_be_a_soft_word_wrap():
    raw, documents = source_case('投标\n人', ['投标人'], line_units=['grid-a', 'grid-b'])
    with pytest.raises(DirectoryPlanRejected, match='exact fragment'):
        compile_directory(raw, documents)
    raw['nodes'][0]['subjects'][0]['role'] = '投标\n人'
    assert compile_directory(raw, documents)['chapters'][0]['subjects'][0]['role'] == '投标\n人'
