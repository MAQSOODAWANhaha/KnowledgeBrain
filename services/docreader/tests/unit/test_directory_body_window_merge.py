"""A larger second plan cannot silently replace conflicting shared source ranges."""

import pytest

from docreader.tender.directory_compile import compile_plans, outline_from_compiled
from test_directory_compile import docs, item, node, plan, window


@pytest.mark.parametrize('same_text', [False, True])
def test_second_superset_preserves_first_shared_window_and_rebinds_extra_child(same_text):
    second_text = '首份正文' if same_text else '第二份不同正文'
    documents = docs([f'共同章\n首份正文\n{second_text}\n新增子章\n新增正文'])
    first = plan([{**node('first-parent', 0, 0), 'body_windows': [window(0, 1)]}],
                 [item('first-parent')])
    second = plan([
        {**node('second-parent', 0, 0), 'body_windows': [window(0, 2)]},
        {**node('extra-child', 0, 3, parent='second-parent'), 'body_windows': [window(0, 4)]},
    ], [item('second-parent'), item('extra-child')])

    compiled = compile_plans([first, second], documents)
    shared = next(chapter for chapter in compiled['chapters'] if chapter['title'] == '共同章')
    extra = next(chapter for chapter in compiled['chapters'] if chapter['title'] == '新增子章')
    assert shared['id'] == 'first-parent'
    assert shared['text'] == '首份正文'
    assert [ref['line'] for ref in shared['lines']] == [1]
    assert extra['parent_id'] == shared['id']
    assert any(issue['code'] == 'outline_directory_conflict' for issue in compiled['issues'])
    outline = outline_from_compiled({'schema_version': 2, 'run_id': 'run', 'source_version': 1,
                                     'domain_revision': 1}, compiled)
    assert [chapter['title'] for chapter in outline['chapters']] == ['共同章']
    assert [child['title'] for child in outline['chapters'][0]['children']] == ['新增子章']
