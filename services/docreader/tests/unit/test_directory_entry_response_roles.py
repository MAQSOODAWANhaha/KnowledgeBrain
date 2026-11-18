"""A composition naming point does not select a complete response clause."""

import pytest

from docreader.tender.directory_compile import compile_directory, outline_from_compiled
from test_directory_compile import docs, entry, item, node, plan, point, window


SNAPSHOT = {'schema_version': 2, 'run_id': 'run', 'source_version': 1, 'domain_revision': 1}


@pytest.mark.parametrize('clause', [[], [window(0, 3)]])
def test_node_entry_anchor_keeps_its_form_and_metadata_without_a_fragment_response(clause):
    pages = ['组成清单\n(1) 投标函及法定代表\n人授权委托书。\n仅代理商适用。',
             '投标函\n致招标人：\n投标人签署：________']
    chosen = {**node('letter', 1, 0), 'body_windows': [window(1, 0, 2)]}
    compiled = compile_directory(plan([chosen], [
        item('letter', entry=0, clause=clause, condition=window(0, 3)),
    ], [entry(0, 1)]), docs(pages))
    chapter = compiled['chapters'][0]
    assert chapter['entry'] == point(0, 1)
    assert chapter['listed_title'] == '投标函及法定代表'
    assert chapter['heading_title'] == '投标函'
    assert chapter['condition'] == '仅代理商适用。'
    assert chapter['obligations'] == ('仅代理商适用。' if clause else '')
    targets = outline_from_compiled(SNAPSHOT, compiled)['chapters'][0]['targets']
    assert targets[0]['purpose'] == pages[1]
    assert targets[0]['kind'] == 'form' and targets[0]['manual_fill']
    assert not any('法定代表' in target['purpose'] for target in targets)
    assert sum(target['purpose'] == '仅代理商适用。' for target in targets) == 1


def test_explicit_composition_clause_without_a_node_keeps_its_complete_existing_outlet():
    pages = ['(1) 人员资料及授权\n须提供完整资格证明，签署：________']
    compiled = compile_directory(plan([], [item(None, entry=0, clause=[window(0, 0, 1)])],
                                      [entry(0, 0)]), docs(pages))
    chapter = compiled['chapters'][0]
    assert chapter['text'] == pages[0] and chapter['obligations'] == pages[0]
    targets = outline_from_compiled(SNAPSHOT, compiled)['chapters'][0]['targets']
    assert len(targets) == 1 and targets[0]['purpose'] == pages[0]
    assert targets[0]['manual_fill']
