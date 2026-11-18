"""A title-only source parent groups intact children of different response kinds."""

from copy import deepcopy

import pytest

from docreader.tender.directory_compile import DirectoryPlanRejected, compile_plans
from docreader.tender.directory_choices import apply_directory_adjudications, directory_cases
from test_directory_compile import docs, item, node, plan, window


def container_case(*, protection=None):
    documents = docs(['项目响应\n编排依据\n申请表\n固定原式\n实施方案\n实施要求\n附证\n原件说明\n投标人\n共同条件'])
    nodes = [{**node(identity, 0, line), 'body_windows': [window(0, line + 1)], 'response_kind': kind}
             for identity, line, kind in [('form', 2, 'form'), ('text', 4, 'narrative'), ('proof', 6, 'attachment')]]
    first = plan(nodes, [item(n['id']) for n in nodes])
    parent = {**node('parent', 0, 0), 'body_windows': []}
    if protection == 'subject':
        parent['subjects'] = [{'source': window(0, 8), 'role': '投标人'}]
    if protection == 'body':
        parent['body_windows'] = [window(0, 9)]
    adoption = item('parent', 'subtree', condition=window(0, 9) if protection == 'condition' else None)
    second = plan([parent, *[{**n, 'parent_id': 'parent'} for n in nodes]], [adoption])
    compiled = compile_plans([first, second], documents)
    decisions = [{'case_id': c['case_id'], 'action': 'container' if c['kind'] == 'extra' else 'current',
                  'chapter_ids': ['form', 'text', 'proof'] if c['kind'] == 'extra' else [],
                  'basis_windows': [window(0, 1)]} for c in directory_cases(compiled, documents)]
    return compiled, documents, decisions


def test_pure_parent_keeps_each_child_kind_and_all_child_content():
    compiled, documents, decisions = container_case()
    before = {c['id']: deepcopy(c) for c in compiled['chapters'] if c['id'] in {'form', 'text', 'proof'}}
    source_before = deepcopy(documents)
    apply_directory_adjudications(compiled, documents, decisions)
    children = {c['id']: c for c in compiled['chapters'] if c['id'] in before}
    parent_ids = {c['parent_id'] for c in children.values()}
    assert len(parent_ids) == 1
    parent = next(c for c in compiled['chapters'] if c['id'] in parent_ids)
    assert len([c for c in compiled['chapters'] if c['heading'] == parent['heading']]) == 1
    assert parent['text'] == '' and parent['subjects'] == [] and parent['body_windows'] == []
    assert children == {identity: {**c, 'parent_id': parent['id']} for identity, c in before.items()}
    assert not compiled['issues'] and documents == source_before


@pytest.mark.parametrize('protection', ['subject', 'body', 'condition'])
def test_parent_with_an_independent_response_is_not_treated_as_a_pure_container(protection):
    compiled, documents, decisions = container_case(protection=protection)
    before = deepcopy(compiled)
    with pytest.raises(DirectoryPlanRejected):
        apply_directory_adjudications(compiled, documents, decisions)
    assert compiled == before
