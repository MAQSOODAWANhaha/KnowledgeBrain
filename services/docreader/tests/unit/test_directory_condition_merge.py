"""Same-body directory candidates must expose differing conditions and obligations."""

from copy import deepcopy

import pytest

from docreader.tender.directory_compile import DirectoryPlanRejected, compile_directory, compile_plans
from test_directory_compile import docs, item, node, plan, window


DOCUMENTS = docs(['原式\n固定正文\n适用于代理商\n适用于制造商\n须附授权证明\n须附审计材料\n其他原式\n其他正文'])


def candidate(*, condition=None, clause=None, extra=False):
    nodes = [{**node('form', 0, 0), 'body_windows': [window(0, 1)]}]
    items = [item('form', condition=window(0, condition) if condition is not None else None,
                  clause=[window(0, clause)] if clause is not None else [])]
    if extra:
        nodes.append({**node('other', 0, 6), 'body_windows': [window(0, 7)]})
        items.append(item('other'))
    return plan(nodes, items)


@pytest.mark.parametrize('first_condition,second_condition', [(2, 3), (None, 2), (2, None)])
def test_same_body_different_conditions_keep_first_and_explain_both(first_condition, second_condition):
    first, second = candidate(condition=first_condition), candidate(condition=second_condition)
    original = deepcopy([first, second])
    compiled = compile_plans([first, second], DOCUMENTS)
    expected = DOCUMENTS[0]['pages'][0].splitlines()[first_condition] if first_condition is not None else None
    assert compiled['chapters'][0]['condition'] == expected
    conflicts = [issue['detail'] for issue in compiled['issues'] if issue['code'] == 'outline_directory_conflict']
    assert len(conflicts) == 1 and '适用条件' in conflicts[0]
    assert '保留先完成的一份' in conflicts[0] and '待核' in conflicts[0]
    for line in (first_condition, second_condition):
        assert (DOCUMENTS[0]['pages'][0].splitlines()[line] if line is not None else '未选择') in conflicts[0]
    assert [first, second] == original


def test_same_body_different_obligations_keep_first_and_explain_both():
    compiled = compile_plans([candidate(clause=4), candidate(clause=5)], DOCUMENTS)
    assert compiled['chapters'][0]['obligations'] == '须附授权证明'
    detail = next(issue['detail'] for issue in compiled['issues'] if issue['code'] == 'outline_directory_conflict')
    assert '补充义务' in detail and '须附授权证明' in detail and '须附审计材料' in detail and '待核' in detail


def test_second_heading_superset_cannot_replace_first_condition_or_obligation():
    compiled = compile_plans([candidate(condition=2, clause=4), candidate(condition=3, clause=5, extra=True)], DOCUMENTS)
    assert [(chapter['id'], chapter['condition'], chapter['obligations']) for chapter in compiled['chapters']] == [
        ('form', '适用于代理商', '须附授权证明'), ('other', None, '')]
    detail = next(issue['detail'] for issue in compiled['issues'] if issue['code'] == 'outline_directory_conflict')
    assert '适用条件' in detail and '补充义务' in detail


def test_matching_condition_and_obligation_do_not_create_a_conflict():
    compiled = compile_plans([candidate(condition=2, clause=4), candidate(condition=2, clause=4, extra=True)], DOCUMENTS)
    assert [chapter['id'] for chapter in compiled['chapters']] == ['form', 'other']
    assert not any(issue['code'] == 'outline_directory_conflict' for issue in compiled['issues'])


def duplicate_adoption(*, condition=4, clause=None, reverse=False):
    nodes = [{**node('parent', 0, 0), 'body_windows': [window(0, 1)]},
             {**node('child', 0, 2, parent='parent'), 'body_windows': [window(0, 3)]}]
    explicit = item('child', condition=window(0, condition) if condition is not None else None,
                    clause=[window(0, clause)] if clause is not None else [])
    items = [item('parent', depth='subtree'), explicit]
    return plan(nodes, list(reversed(items)) if reverse else items)


CHILD_DOCUMENTS = docs(['资格材料\n父章原文\n项目授权\n授权空位\n仅代理商适用\n须附来源证明\n仅制造商适用'])


@pytest.mark.parametrize('reverse', [False, True])
def test_explicit_child_condition_survives_ancestor_subtree_deduplication(reverse):
    value = duplicate_adoption(reverse=reverse)
    original = deepcopy(value)
    compiled = compile_directory(value, CHILD_DOCUMENTS)
    child = next(c for c in compiled['chapters'] if c['id'] == 'child')
    assert child['condition'] == '仅代理商适用' and child['text'] == '授权空位'
    assert len(compiled['chapters']) == 2
    assert any(i['code'] == 'outline_directory_applicability_unconfirmed'
               and '项目授权' in i['detail'] and '仅代理商适用' in i['detail'] for i in compiled['issues'])
    assert value == original


def test_repeated_child_differing_condition_keeps_first_and_exposes_both():
    value = duplicate_adoption()
    value['scopes'][0]['items'].append(item('child', condition=window(0, 6)))
    compiled = compile_directory(value, CHILD_DOCUMENTS)
    assert next(c for c in compiled['chapters'] if c['id'] == 'child')['condition'] == '仅代理商适用'
    detail = next(i['detail'] for i in compiled['issues'] if i['code'] == 'outline_directory_conflict')
    assert '仅代理商适用' in detail and '仅制造商适用' in detail and '待核' in detail


def test_repeated_child_clause_has_a_named_review_exit_instead_of_being_dropped():
    compiled = compile_directory(duplicate_adoption(clause=5), CHILD_DOCUMENTS)
    assert any(i['code'] == 'outline_directory_conflict' and '项目授权' in i['detail']
               and '须附来源证明' in i['detail'] for i in compiled['issues'])


@pytest.mark.parametrize(('condition', 'clause'), [(None, None), (4, None), (4, 3)])
def test_repeated_child_does_not_clear_or_duplicate_existing_source_decisions(condition, clause):
    value = duplicate_adoption()
    value['scopes'][0]['items'].append(item(
        'child', condition=window(0, condition) if condition is not None else None,
        clause=[window(0, clause)] if clause is not None else []))
    compiled = compile_directory(value, CHILD_DOCUMENTS)
    assert next(c for c in compiled['chapters'] if c['id'] == 'child')['condition'] == '仅代理商适用'
    assert len([i for i in compiled['issues'] if i['code'] == 'outline_directory_applicability_unconfirmed']) == 1
    assert not any(i['code'] == 'outline_directory_conflict' for i in compiled['issues'])


@pytest.mark.parametrize('field', ['condition', 'clause'])
def test_repeated_child_still_validates_selected_source_windows(field):
    value = duplicate_adoption()
    value['scopes'][0]['items'][-1][field] = window(0, 999) if field == 'condition' else [window(0, 999)]
    with pytest.raises(DirectoryPlanRejected):
        compile_directory(value, CHILD_DOCUMENTS)


def test_same_body_condition_conflicts_follow_source_keys_instead_of_hash_order():
    titles = ['原式甲', '原式乙', '原式丙', '原式丁', '原式戊', '原式己']
    lines = [text for title in titles for text in [title, title + '固定正文']]
    documents = docs(['\n'.join([*lines, '条件甲', '条件乙'])])
    nodes = [{**node(f'form-{index}', 0, index * 2), 'body_windows': [window(0, index * 2 + 1)]}
             for index in range(len(titles))]
    candidates = [plan(nodes, [item(f'form-{index}', condition=window(0, condition))
                               for index in range(len(titles))]) for condition in (12, 13)]
    compiled = compile_plans(candidates, documents)
    details = [issue['detail'] for issue in compiled['issues'] if issue['code'] == 'outline_directory_conflict']
    assert len(details) == len(titles)
    assert all(f'“{title}”' in detail for title, detail in zip(titles, details))
