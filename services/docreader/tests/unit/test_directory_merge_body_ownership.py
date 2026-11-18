"""Cross-slot extras cannot create a duplicate source-body submission instance."""

from copy import deepcopy

from docreader.tender.directory_compile import compile_directory, compile_plans, outline_from_compiled
from test_directory_compile import docs, entry, item, node, plan, window


PAGES = ['既有章\n候选子章\n共用正文一\n共用正文二\n候选独有正文\n候选父章\n独有子正文\n独立新章\n独立新正文']
SNAPSHOT = {'schema_version': 2, 'run_id': 'run', 'source_version': 1, 'domain_revision': 1}


def selected(identity, heading, start, end, parent=None):
    return {**node(identity, 0, heading, parent), 'body_windows': [window(0, start, end)]}


def first_plan():
    return plan([selected('owned', 0, 2, 3)], [item('owned')])


def test_exact_cross_slot_duplicate_retains_the_existing_carrier_and_names_both_sources():
    first, second = first_plan(), plan([selected('extra', 5, 2, 3)], [item('extra')])
    original = deepcopy([first, second])
    assert len(compile_directory(first, docs(PAGES))['chapters']) == 1
    assert len(compile_directory(second, docs(PAGES))['chapters']) == 1
    compiled = compile_plans([first, second], docs(PAGES))
    assert [chapter['id'] for chapter in compiled['chapters']] == ['owned']
    assert compiled['chapters'][0]['text'] == '共用正文一\n共用正文二'
    detail = next(issue['detail'] for issue in compiled['issues'] if issue['code'] == 'outline_directory_extra_unconfirmed')
    assert '既有章' in detail and '候选父章' in detail and 'page 0 line 2' in detail
    assert 'lines 2-3' in detail and '待人工确认' in detail
    assert [first, second] == original


def test_partial_overlap_does_not_silently_trim_or_merge_the_extra_unique_tail():
    second = plan([selected('extra', 5, 3, 4)], [item('extra')])
    compiled = compile_plans([first_plan(), second], docs(PAGES))
    assert [chapter['id'] for chapter in compiled['chapters']] == ['owned']
    assert compiled['chapters'][0]['text'] == '共用正文一\n共用正文二'
    detail = next(issue['detail'] for issue in compiled['issues'] if issue['code'] == 'outline_directory_extra_unconfirmed')
    assert 'lines 3-4' in detail and '完整候选' in detail


def test_omitted_parent_does_not_leave_or_reparent_its_earlier_heading_child():
    second = plan([selected('extra', 5, 2, 3), selected('child', 1, 6, 6, 'extra'), selected('other', 7, 8, 8)],
                  [item('extra', 'subtree'), item('other')])
    compiled = compile_plans([first_plan(), second], docs(PAGES))
    assert [chapter['id'] for chapter in compiled['chapters']] == ['owned', 'other']
    assert all(chapter['parent_id'] is None for chapter in compiled['chapters'])
    details = [issue['detail'] for issue in compiled['issues'] if issue['code'] == 'outline_directory_extra_unconfirmed']
    assert any('候选子章' in detail and '候选父章' in detail and 'lines 6-6' in detail for detail in details)
    outline = outline_from_compiled(SNAPSHOT, compiled)
    assert [chapter['title'] for chapter in outline['chapters']] == ['既有章', '独立新章']


def test_distinct_real_submission_entries_allow_independent_same_source_instances():
    documents = docs([*PAGES, '甲独立提交物\n乙独立提交物'])
    first = plan([selected('owned', 0, 2, 3)], [item('owned', entry=0)], [entry(1, 0)])
    second = plan([selected('extra', 5, 2, 3)], [item('extra', entry=0)], [entry(1, 1)])
    compiled = compile_plans([first, second], documents)
    assert [chapter['id'] for chapter in compiled['chapters']] == ['owned', 'extra']
    assert compiled['chapters'][0]['text'] == compiled['chapters'][1]['text']


def test_same_submission_entry_does_not_justify_cross_slot_duplicate():
    documents = docs([*PAGES, '同一个提交物'])
    first = plan([selected('owned', 0, 2, 3)], [item('owned', entry=0)], [entry(1, 0)])
    second = plan([selected('extra', 5, 2, 3)], [item('extra', entry=0)], [entry(1, 0)])
    compiled = compile_plans([first, second], documents)
    assert [chapter['id'] for chapter in compiled['chapters']] == ['owned']


def test_subject_instances_in_one_chapter_are_preserved():
    documents = docs([*PAGES, '投标人及原厂'])
    chosen = selected('owned', 0, 2, 3)
    chosen['subjects'] = [{'source': window(1, 0), 'role': role} for role in ('投标人', '原厂')]
    first = plan([chosen], [item('owned')])
    compiled = compile_plans([first, deepcopy(first)], documents)
    outline = outline_from_compiled(SNAPSHOT, compiled)
    assert len(outline['chapters']) == 1
    assert [target['subject'] for target in outline['chapters'][0]['targets']] == ['投标人', '原厂']


def test_equal_text_from_disjoint_coordinates_is_not_a_duplicate():
    pages = [PAGES[0].replace('候选独有正文', '共用正文一')]
    first = plan([selected('owned', 0, 2, 2)], [item('owned')])
    second = plan([selected('extra', 5, 4, 4)], [item('extra')])
    compiled = compile_plans([first, second], docs(pages))
    assert len(compiled['chapters']) == 2
    assert compiled['chapters'][0]['text'] == compiled['chapters'][1]['text']


def test_legacy_raw_inferred_bodies_keep_their_previous_replay():
    first = plan([node('owned', 0, 0)], [item('owned')])
    second = plan([node('extra', 0, 5)], [item('extra')])
    compiled = compile_plans([first, second], docs(PAGES))
    assert [chapter['id'] for chapter in compiled['chapters']] == ['owned', 'extra']
