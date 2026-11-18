"""Only proven group inheritance and blank source lines are equivalent."""

from copy import deepcopy

import pytest

from docreader.tender.directory_compile import compile_plans
from test_directory_compile import docs, item, node, plan, point, window


PAGES = ['资格材料\n父正文\n原式\n正文甲\n\n正文乙\n投标人与原厂\n商务分册\n其他分册\n须附审计材料']


def candidates():
    nodes = [{**node('parent', 0, 0), 'body_windows': [window(0, 1)]},
             {**node('child', 0, 2, 'parent'), 'body_windows': [window(0, 3)]}]
    first = plan(deepcopy(nodes), [item('parent', 'subtree')], title=point(0, 7))
    second = plan(deepcopy(nodes), [item('parent'), item('child')], title=point(0, 7))
    return first, second


def conflicts(compiled):
    return [issue['detail'] for issue in compiled['issues'] if issue['code'] == 'outline_directory_conflict']


def test_child_repeating_its_valid_ancestor_group_is_equivalent_without_changing_the_carrier():
    first, second = candidates()
    original = deepcopy([first, second])
    merged = compile_plans([first, second], docs(PAGES))
    assert not conflicts(merged)
    child = next(c for c in merged['chapters'] if c['id'] == 'child')
    assert child['group'] is None and child['parent_id'] == 'parent'
    assert child['text'] == '正文甲' and child['body_windows'] == [('tender', 0, 3, 3)]
    assert [first, second] == original


def test_distinct_root_groups_remain_visible_for_root_and_inherited_child():
    first, second = candidates()
    second['scopes'][0]['title'] = point(0, 8)
    details = conflicts(compile_plans([first, second], docs(PAGES)))
    assert len(details) == 2
    assert all('group' in detail and '商务分册' in detail and '其他分册' in detail for detail in details)


def test_child_group_that_differs_from_its_ancestor_is_not_erased():
    first, second = candidates()
    second['scopes'][0]['items'].pop()
    second['scopes'].append({'title': point(0, 8), 'items': [item('child')]})
    detail, = conflicts(compile_plans([first, second], docs(PAGES)))
    assert 'group' in detail and '商务分册' in detail and '其他分册' in detail


def test_equal_group_captions_at_distinct_source_anchors_remain_a_real_difference():
    nodes = [{**node('form', 0, 0), 'body_windows': [window(0, 1)]}]
    first = plan(deepcopy(nodes), [item('form')], title=point(1, 0))
    second = plan(deepcopy(nodes), [item('form')], title=point(1, 1))
    detail, = conflicts(compile_plans([first, second], docs(['原式\n正文', '商务分册\n商务分册'])))
    assert 'group_heading' in detail


def test_unadopted_parent_is_not_treated_as_an_existing_root_or_group_proof():
    first, second = candidates()
    first['source_only'] = ['parent']
    first['scopes'][0]['items'] = [item('child')]
    second['nodes'][1]['parent_id'] = None
    second['source_only'] = ['parent']
    second['scopes'][0]['items'] = [item('child')]
    detail, = conflicts(compile_plans([first, second], docs(PAGES)))
    assert 'parent_id' in detail and 'parent' in detail


@pytest.mark.parametrize(('field', 'value'), [
    ('response_kind', 'attachment'),
    ('subjects', [{'source': window(0, 6), 'role': '原厂'}]),
    ('presentation', 'content'),
])
def test_group_alias_cannot_hide_subject_role_or_presentation_disagreement(field, value):
    first, second = candidates()
    if field == 'presentation':
        second['scopes'][0]['items'][1][field] = value
    else:
        second['nodes'][1][field] = value
    detail, = conflicts(compile_plans([first, second], docs(PAGES)))
    assert field in detail and '待核' in detail


def test_only_extra_blank_source_lines_do_not_create_a_body_conflict_or_change_the_body():
    first, second = candidates()
    second['nodes'][1]['body_windows'] = [window(0, 3, 4)]
    merged = compile_plans([first, second], docs(PAGES))
    assert not conflicts(merged)
    child = next(c for c in merged['chapters'] if c['id'] == 'child')
    assert child['text'] == '正文甲' and child['lines'] == [{'document_id': 'tender', 'page': 0, 'line': 3}]


def test_blank_source_line_owned_by_a_unit_is_not_discarded_as_empty_prose():
    first, second = candidates()
    second['nodes'][1]['body_windows'] = [window(0, 3, 4)]
    documents = docs(PAGES)
    documents[0]['line_units'] = [[None, None, None, None, 'page:0:table:0', None, None, None, None, None]]
    detail, = conflicts(compile_plans([first, second], documents))
    assert '正文范围' in detail


def test_equal_text_at_distinct_source_coordinates_remains_a_body_conflict():
    first, second = candidates()
    second['nodes'][1]['body_windows'] = [window(0, 5)]
    documents = docs([PAGES[0].replace('正文乙', '正文甲')])
    merged = compile_plans([first, second], documents)
    detail, = conflicts(merged)
    assert '正文范围' in detail
    assert next(c for c in merged['chapters'] if c['id'] == 'child')['lines'][0]['line'] == 3


def test_equal_obligations_at_distinct_source_coordinates_remain_visible():
    _, second = candidates()
    first = deepcopy(second)
    first['scopes'][0]['items'][1]['clause'] = [window(0, 9)]
    second['scopes'][0]['items'][1]['clause'] = [window(0, 10)]
    merged = compile_plans([first, second], docs([PAGES[0] + '\n须附审计材料']))
    detail, = conflicts(merged)
    assert 'obligation_lines' in detail and '补充义务来源' in detail
    assert next(c for c in merged['chapters'] if c['id'] == 'child')['obligation_lines'][0]['line'] == 9


def test_nonempty_body_and_obligation_differences_survive_a_group_alias():
    first, second = candidates()
    second['nodes'][1]['body_windows'] = [window(0, 3, 5)]
    second['scopes'][0]['items'][1]['clause'] = [window(0, 9)]
    merged = compile_plans([first, second], docs(PAGES))
    detail, = conflicts(merged)
    assert '正文范围' in detail and '补充义务' in detail and 'obligations' in detail
    assert '须附审计材料' in detail and '未选择' in detail
    assert 'group' not in detail
    child = next(c for c in merged['chapters'] if c['id'] == 'child')
    assert child['text'] == '正文甲' and child['obligations'] == ''


def test_structural_conflict_diagnostics_also_show_the_body_and_obligation_differences():
    first, second = candidates()
    second['nodes'][1]['body_windows'] = [window(0, 3, 5)]
    second['scopes'][0]['items'][1].update(presentation='content', clause=[window(0, 9)])
    detail, = conflicts(compile_plans([first, second], docs(PAGES)))
    assert 'presentation' in detail and '正文范围' in detail and 'obligations' in detail
    assert 'section' in detail and 'content' in detail and '须附审计材料' in detail
