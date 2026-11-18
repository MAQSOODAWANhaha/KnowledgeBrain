"""Independent location plans cannot share their local chapter identities."""

from docreader.tender.directory_compile import compile_plans, outline_from_compiled
from test_directory_compile import docs, item, node, plan


SNAPSHOT = {'schema_version': 2, 'run_id': 'run', 'source_version': 1, 'domain_revision': 1}


def test_colliding_plan_ids_preserve_distinct_source_chapters():
    pages = ['甲章\n甲正文\n乙章\n乙正文\n丙章\n丙正文']
    first = plan([node('n1', 0, 0), node('n2', 0, 2)], [item('n1'), item('n2')])
    second = plan([node('n1', 0, 0), node('n2', 0, 4)], [item('n1'), item('n2')])

    compiled = compile_plans([first, second], docs(pages))
    outline = outline_from_compiled(SNAPSHOT, compiled)

    assert [chapter['title'] for chapter in outline['chapters']] == ['甲章', '乙章', '丙章']
    assert len({chapter['id'] for chapter in compiled['chapters']}) == 3
    assert compiled['chapters'][1]['id'] == 'n2'
    assert '乙正文' in outline['chapters'][1]['targets'][0]['purpose']
    assert '丙正文' in outline['chapters'][2]['targets'][0]['purpose']


def test_colliding_extra_parent_and_child_keep_their_own_relationship():
    pages = ['甲章\n甲正文\n乙章\n乙正文\n丙章\n丙正文\n丙子章\n子正文\n丁章\n丁正文']
    first = plan([node('n1', 0, 0), node('n2', 0, 2), node('n3', 0, 8)],
                 [item('n1'), item('n2'), item('n3')])
    second = plan([node('n1', 0, 0), node('n2', 0, 4), node('n3', 0, 6, 'n2')],
                  [item('n1'), item('n2', 'subtree')])

    compiled = compile_plans([first, second], docs(pages))
    outline = outline_from_compiled(SNAPSHOT, compiled)
    by_title = {chapter['title']: chapter for chapter in compiled['chapters']}

    assert len({chapter['id'] for chapter in compiled['chapters']}) == 5
    assert by_title['丙子章']['parent_id'] == by_title['丙章']['id']
    assert by_title['乙章']['parent_id'] is None and by_title['丁章']['parent_id'] is None
    assert [(chapter['title'], [child['title'] for child in chapter['children']])
            for chapter in outline['chapters']] == [('甲章', []), ('乙章', []), ('丁章', []), ('丙章', ['丙子章'])]


def test_colliding_extra_child_uses_the_shared_parents_first_plan_identity():
    pages = ['共同父章\n父正文\n既有子章\n既有正文\n新增子章\n新增正文\n独有章\n独有正文']
    first = plan([node('parent-first', 0, 0), node('child-first', 0, 2, 'parent-first'), node('n2', 0, 6)],
                 [item('parent-first', 'subtree'), item('n2')])
    second = plan([node('parent-second', 0, 0), node('n2', 0, 4, 'parent-second')],
                  [item('parent-second', 'subtree')])

    compiled = compile_plans([first, second], docs(pages))
    outline = outline_from_compiled(SNAPSHOT, compiled)
    by_title = {chapter['title']: chapter for chapter in compiled['chapters']}

    assert len({chapter['id'] for chapter in compiled['chapters']}) == 4
    assert by_title['新增子章']['parent_id'] == 'parent-first'
    assert by_title['独有章']['id'] == 'n2' and by_title['独有章']['parent_id'] is None
    assert [chapter['title'] for chapter in outline['chapters']] == ['共同父章', '独有章']
    assert [child['title'] for child in outline['chapters'][0]['children']] == ['既有子章', '新增子章']
