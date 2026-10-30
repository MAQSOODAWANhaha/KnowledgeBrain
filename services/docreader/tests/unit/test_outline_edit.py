from copy import deepcopy

import pytest

from docreader.tender.outline_edit import apply_outline_edit
from docreader.tender.schema import AuthoringInput, AuthoringOutline


MANUAL = 'manual-328f852d-3d53-4b7a-851d-fc0572420173'


def outline():
    ref = {'sha256': 'a' * 64, 'relative_path': 'aa/' + 'a' * 64, 'size': 1, 'media_type': 'application/json'}
    source = {'source_index_ref': ref, 'unit_id': 'source-1'}
    target = {'target_id': 'form-a', 'kind': 'form', 'purpose': '保留规定原表',
              'rule_refs': [{'item_ref': {'artifact_ref': ref, 'item_id': 'rule-a'}}],
              'source_refs': [source], 'template_refs': [{'source_ref': source, 'selectors': []}],
              'manual_fill': True}
    snapshot = AuthoringInput(schema_version=2, run_id='r', source_version=1, domain_revision=1)
    return AuthoringOutline(schema_version=2, authoring_input=snapshot, issues=[], chapters=[
        {'chapter_id': 'a', 'title': '同名', 'targets': [target], 'children': []},
        {'chapter_id': 'b', 'title': '同名', 'targets': [], 'children': []},
    ]).model_dump(exclude_unset=True)


def test_reorder_reparent_and_rename_preserve_the_exact_targets_and_snapshot():
    base = outline()
    original = deepcopy(base)
    changed = apply_outline_edit(base, [{'chapter_id': 'b', 'title': '技术分册', 'is_volume': True,
        'children': [{'chapter_id': 'a', 'title': '修改标题', 'children': []}]}])
    assert base == original
    child = changed['chapters'][0]['children'][0]
    assert child['chapter_id'] == 'a' and child['title'] == '修改标题'
    assert child['targets'] == base['chapters'][0]['targets']
    assert changed['authoring_input'] == base['authoring_input']
    assert changed['chapters'][0]['is_volume'] is True


def test_delete_keeps_original_requirements_as_explicit_unassigned_issues():
    base = outline()
    changed = apply_outline_edit(base, [{'chapter_id': 'b', 'title': '保留', 'children': []}])
    removed = next(issue for issue in changed['issues'] if issue['code'] == 'outline_edit_target_removed')
    assert removed['target_id'] == 'form-a'
    assert removed['rule_refs'] == base['chapters'][0]['targets'][0]['rule_refs']
    assert '保留规定原表' in removed['detail']
    assert changed['chapters'][0]['targets'] == []


def test_new_empty_chapter_is_a_heading_until_the_user_supplies_a_writing_goal():
    empty = apply_outline_edit(outline(), [{'chapter_id': MANUAL, 'title': '补充章节', 'children': []}])
    assert empty['chapters'][0]['targets'] == []
    written = apply_outline_edit(empty, [{'chapter_id': MANUAL, 'title': '补充章节',
        'writing_goal': '根据已有企业资料编写交付安排', 'children': []}])
    target = written['chapters'][0]['targets'][0]
    assert target['kind'] == 'narrative' and target['purpose'] == '根据已有企业资料编写交付安排'
    assert target['rule_refs'] == target['source_refs'] == target['template_refs'] == []
    assert target['support_origins'] == ['knowledge']


def test_empty_tree_is_preserved_instead_of_recreating_deleted_chapters():
    changed = apply_outline_edit(outline(), [])
    assert changed['chapters'] == []
    assert any(issue['code'] == 'outline_edit_target_removed' for issue in changed['issues'])


def test_submission_note_role_round_trips_through_human_edit_without_losing_evidence():
    base = outline()
    changed = apply_outline_edit(base, [{'chapter_id': 'a', 'title': '平台操作',
        'presentation_role': 'submission_note', 'children': []}])
    assert changed['chapters'][0]['presentation_role'] == 'submission_note'
    assert changed['chapters'][0]['targets'] == base['chapters'][0]['targets']
    restored = apply_outline_edit(changed, [{'chapter_id': 'a', 'title': '正式正文',
        'presentation_role': 'body', 'children': []}])
    assert restored['chapters'][0]['targets'] == base['chapters'][0]['targets']


@pytest.mark.parametrize('chapters', [
    [{'chapter_id': 'unknown', 'title': '未知标识', 'children': []}],
    [{'chapter_id': 'a', 'title': '重复', 'children': [{'chapter_id': 'a', 'title': '循环', 'children': []}]}],
    [{'chapter_id': 'a', 'title': '   ', 'children': []}],
    [{'chapter_id': 'a', 'title': '篡改', 'targets': [], 'children': []}],
    [{'chapter_id': 'a', 'title': '篡改', 'writing_goal': '覆盖规定原表', 'children': []}],
])
def test_invalid_or_reference_overwriting_edits_are_rejected(chapters):
    with pytest.raises(ValueError):
        apply_outline_edit(outline(), chapters)


def test_duplicate_base_ids_cannot_be_guessed_into_a_new_binding():
    base = outline()
    base['chapters'][1]['chapter_id'] = 'a'
    with pytest.raises(ValueError):
        apply_outline_edit(base, [{'chapter_id': 'a', 'title': '无法确定', 'children': []}])


def test_excessive_nesting_is_rejected_with_an_editable_structure_error():
    roots, node = [], []
    roots = node
    for index in range(20):
        child = {'chapter_id': f'manual-{index:08x}-0000-4000-8000-000000000000',
                 'title': str(index), 'children': []}
        node.append(child)
        node = child['children']
    with pytest.raises(ValueError, match='层级'):
        apply_outline_edit(outline(), roots)


def test_deleting_a_referenced_target_keeps_a_single_structural_warning_on_resave():
    base = outline()
    base['chapters'][1]['targets'] = [{'target_id': 'reference', 'kind': 'narrative',
        'purpose': '参见规定原表', 'rule_refs': [], 'source_refs': [], 'related_target_ids': ['form-a']}]
    tree = [{'chapter_id': 'b', 'title': '参见', 'children': []}]
    changed = apply_outline_edit(base, tree)
    repeated = apply_outline_edit(changed, tree)
    warnings = [issue for issue in repeated['issues'] if issue['code'] == 'outline_edit_reference_unbound']
    assert len(warnings) == 1 and warnings[0]['target_id'] == 'reference'
    assert repeated['chapters'][0]['targets'][0]['related_target_ids'] == ['form-a']


def test_tree_node_limit_rejects_oversized_edits_before_binding():
    tree = [{'chapter_id': str(index), 'title': str(index), 'children': []} for index in range(1001)]
    with pytest.raises(ValueError, match='1000'):
        apply_outline_edit(outline(), tree)


def test_move_a_response_preserves_the_complete_server_owned_target():
    base = outline()
    changed = apply_outline_edit(base, [
        {'chapter_id': 'a', 'title': '原章节', 'target_ids': [], 'children': []},
        {'chapter_id': 'b', 'title': '新位置', 'target_ids': ['form-a'], 'children': []},
    ])
    assert changed['chapters'][0]['targets'] == []
    assert changed['chapters'][1]['targets'] == base['chapters'][0]['targets']
    assert not changed['issues']


def test_restore_a_deleted_response_from_the_same_server_catalog():
    base = outline()
    removed = apply_outline_edit(base, [{'chapter_id': 'b', 'title': '保留', 'children': []}])
    changed = apply_outline_edit(removed, [
        {'chapter_id': 'b', 'title': '恢复位置', 'target_ids': ['form-a'], 'children': []},
    ], target_catalog=base['chapters'][0]['targets'])
    assert changed['chapters'][0]['targets'] == base['chapters'][0]['targets']
    assert not changed['issues']


@pytest.mark.parametrize('target_ids', [['unknown'], ['form-a', 'form-a']])
def test_response_reassignment_rejects_unknown_and_duplicate_ids(target_ids):
    with pytest.raises(ValueError):
        apply_outline_edit(outline(), [{'chapter_id': 'b', 'title': '目标',
                                      'target_ids': target_ids, 'children': []}])
