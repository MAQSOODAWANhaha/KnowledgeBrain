"""Directory decisions select supplied source candidates and retain unresolved work."""

from copy import deepcopy

import pytest

from docreader.tender.directory_compile import DirectoryPlanRejected, MaterialPlan, compile_plans
from test_directory_compile import docs, entry, item, node, plan, window


def differing_bodies():
    documents = docs(['原式\n原正文\n扩展正文\n适用于代理商\n采用该原式'])
    values = [plan([{**node('form', 0, 0), 'body_windows': [window(0, 1, end)]}],
                   [item('form', condition=window(0, 3))]) for end in (1, 2)]
    return compile_plans(values, documents), documents


def test_material_plan_has_an_optional_closed_directory_decision_contract():
    assert MaterialPlan.model_validate({}).model_dump()['directory_adjudications'] == []


def test_actual_differences_and_conditions_have_stable_source_case_ids():
    first, documents = differing_bodies()
    second, _ = differing_bodies()
    issues = [i for i in first['issues'] if i['code'] in {
        'outline_directory_conflict', 'outline_directory_applicability_unconfirmed'}]
    assert len(issues) == 2
    assert all(i.get('directory_case_id') for i in issues)
    assert [i['directory_case_id'] for i in issues] == [i['directory_case_id'] for i in second['issues']]
    from docreader.tender.directory_choices import directory_cases
    cases = directory_cases(first, documents)
    assert {c['case_id'] for c in cases} == {i['directory_case_id'] for i in issues}
    conflict = next(c for c in cases if c['kind'] == 'conflict')
    assert conflict['current']['body_windows'] != conflict['alternative']['body_windows']
    assert 'text' not in conflict['current'] and 'text' not in conflict['alternative']


def decide_all(compiled, documents, action='unresolved'):
    from docreader.tender.directory_choices import directory_cases
    return [{'case_id': case['case_id'], 'action': action} for case in directory_cases(compiled, documents)]


def test_unresolved_retains_original_issues_and_no_basis_is_not_an_implicit_current_choice():
    from docreader.tender.directory_choices import apply_directory_adjudications
    compiled, documents = differing_bodies()
    original = deepcopy(compiled)
    apply_directory_adjudications(compiled, documents, decide_all(compiled, documents))
    assert compiled['issues'] == original['issues'] and compiled['chapters'] == original['chapters']
    choices = decide_all(compiled, documents, 'current')
    with pytest.raises(DirectoryPlanRejected, match='basis'):
        apply_directory_adjudications(compiled, documents, choices)


def test_alternative_keeps_stable_id_and_conditional_does_not_claim_enterprise_satisfaction():
    from docreader.tender.directory_choices import apply_directory_adjudications, directory_cases
    compiled, documents = differing_bodies()
    choices = [{'case_id': case['case_id'], 'action': 'conditional' if case['kind'] == 'applicability'
                else 'alternative', 'basis_windows': [window(0, 3)]} for case in directory_cases(compiled, documents)]
    apply_directory_adjudications(compiled, documents, choices)
    assert compiled['chapters'][0]['id'] == 'form'
    assert compiled['chapters'][0]['text'] == '原正文\n扩展正文'
    assert compiled['chapters'][0]['condition'] == '适用于代理商'
    assert not compiled['issues']
    conditional = next(d for d in compiled['directory_decisions'] if d['action'] == 'conditional')
    assert conditional['enterprise_confirmed'] is False
    assert conditional['source_records'] and conditional['basis_text'] == '适用于代理商'


@pytest.mark.parametrize('bad', ['missing', 'duplicate', 'unknown', 'bad_basis', 'unknown_chapter'])
def test_invalid_batch_is_rejected_atomically(bad):
    from docreader.tender.directory_choices import apply_directory_adjudications
    compiled, documents = differing_bodies()
    original = deepcopy(compiled)
    choices = decide_all(compiled, documents)
    if bad == 'missing':
        choices.pop()
    elif bad == 'duplicate':
        choices.append(deepcopy(choices[0]))
    elif bad == 'unknown':
        choices[0]['case_id'] = 'invented'
    elif bad == 'bad_basis':
        choices[0].update(action='current', basis_windows=[window(0, 900)])
    else:
        choices[0].update(action='current', basis_windows=[window(0, 4)], chapter_ids=['invented'])
    with pytest.raises(DirectoryPlanRejected):
        apply_directory_adjudications(compiled, documents, choices)
    assert compiled == original


def overlapping_extra(*, unique=False):
    documents = docs(['附件总标题\n表格标题\n第一行\n第二行\n独有范围\n选用本格式'])
    first = plan([{**node('owned', 0, 0), 'body_windows': [window(0, 1, 3)]}], [item('owned')])
    second = plan([{**node('child', 0, 1), 'body_windows': [window(0, 2, 4 if unique else 3)]}], [item('child')])
    return compile_plans([first, second], documents), documents


def test_covered_reference_keeps_full_source_record_without_an_extra_word_instance():
    from docreader.tender.directory_choices import apply_directory_adjudications, directory_cases
    compiled, documents = overlapping_extra()
    case, = directory_cases(compiled, documents)
    apply_directory_adjudications(compiled, documents, [{'case_id': case['case_id'], 'action': 'reference',
        'basis_windows': [window(0, 5)], 'chapter_ids': ['owned']}])
    assert len(compiled['chapters']) == 1 and not compiled['issues']
    record, = compiled['directory_decisions']
    assert record['selected_candidate']['heading']['line'] == 1
    assert record['source_records'][0]['original_text'] == '表格标题\n第一行\n第二行'


def test_covered_reference_cannot_swallow_a_unique_source_tail():
    from docreader.tender.directory_choices import apply_directory_adjudications, directory_cases
    compiled, documents = overlapping_extra(unique=True)
    original = deepcopy(compiled)
    case, = directory_cases(compiled, documents)
    with pytest.raises(DirectoryPlanRejected, match='not covered'):
        apply_directory_adjudications(compiled, documents, [{'case_id': case['case_id'], 'action': 'reference',
            'basis_windows': [window(0, 5)], 'chapter_ids': ['owned']}])
    assert compiled == original


def promise_and_bank():
    documents = docs(['附件格式\n投标人承诺\n银行范本\n银行不可撤销保证\n本次不要求实际担保'])
    first = plan([{**node('promise', 0, 0), 'body_windows': [window(0, 1, 3)]}], [item('promise')])
    second = plan([{**node('promise-alt', 0, 0), 'body_windows': [window(0, 1)]},
                   {**node('bank', 0, 2, 'promise-alt'), 'body_windows': [window(0, 3)]}],
                  [item('promise-alt', 'subtree')])
    return compile_plans([first, second], documents), documents


@pytest.mark.parametrize('action', ['reference', 'not_applicable_reference'])
def test_not_applicable_bank_reference_is_distinct_from_current_word_coverage(action):
    from docreader.tender.directory_choices import apply_directory_adjudications, directory_cases
    compiled, documents = promise_and_bank()
    original = deepcopy(compiled)
    decisions = [{'case_id': c['case_id'], 'action': action if c['kind'] == 'extra' else 'alternative',
                  'basis_windows': [window(0, 4)], 'chapter_ids': ['promise'] if c['kind'] == 'extra' else []}
                 for c in directory_cases(compiled, documents)]
    if action == 'reference':
        with pytest.raises(DirectoryPlanRejected, match='not covered'):
            apply_directory_adjudications(compiled, documents, decisions)
        assert compiled == original
    else:
        apply_directory_adjudications(compiled, documents, decisions)
        assert len(compiled['chapters']) == 1 and compiled['chapters'][0]['text'] == '投标人承诺'
        assert compiled['chapters'][0]['id'] == 'promise' and not compiled['issues']
        bank = next(r for r in compiled['directory_decisions'] if r['action'] == action)
        assert bank['source_records'][0]['original_text'] == '银行范本\n银行不可撤销保证'
        assert '未履行' in bank['status'] and bank['basis_text'] == '本次不要求实际担保'


def test_new_child_alternative_is_rejected_if_it_duplicates_an_existing_body_owner():
    from docreader.tender.directory_choices import apply_directory_adjudications, directory_cases
    compiled, documents = promise_and_bank()
    choices = [{'case_id': c['case_id'], 'action': 'alternative' if c['kind'] == 'extra' else 'current',
                'basis_windows': [window(0, 4)]} for c in directory_cases(compiled, documents)]
    with pytest.raises(DirectoryPlanRejected, match='duplicate body ownership'):
        apply_directory_adjudications(compiled, documents, choices)


def test_parent_title_container_preserves_the_existing_child_and_full_body():
    from docreader.tender.directory_choices import apply_directory_adjudications, directory_cases
    documents = docs(['附件价格表\nA价格总表\n表头\n投标人签章\n应采用附件价格表'])
    first = plan([{**node('price', 0, 1), 'body_windows': [window(0, 2, 3)]}], [item('price')])
    second = plan([{**node('annex', 0, 0), 'body_windows': [window(0, 1, 3)]}], [item('annex')])
    compiled = compile_plans([first, second], documents)
    case, = directory_cases(compiled, documents)
    apply_directory_adjudications(compiled, documents, [{'case_id': case['case_id'], 'action': 'container',
        'basis_windows': [window(0, 4)], 'chapter_ids': ['price']}])
    child = next(c for c in compiled['chapters'] if c['id'] == 'price')
    parent = next(c for c in compiled['chapters'] if c['id'] == child['parent_id'])
    assert child['text'] == '表头\n投标人签章' and child['heading']['line'] == 1
    assert parent['title'] == '附件价格表' and parent['text'] == '' and parent['body_windows'] == []
    assert not compiled['issues']


def test_reference_cannot_combine_two_source_subjects_even_when_body_is_identical():
    from docreader.tender.directory_choices import apply_directory_adjudications, directory_cases
    documents = docs(['附件\n子表\n共用正文\n投标人与原厂\n选用本格式'])
    values = [plan([{**node(identity, 0, heading), 'body_windows': [window(0, 2)],
                     'subjects': [{'source': window(0, 3), 'role': role}]}], [item(identity)])
              for identity, heading, role in [('owned', 0, '投标人'), ('extra', 1, '原厂')]]
    compiled = compile_plans(values, documents)
    case, = directory_cases(compiled, documents)
    with pytest.raises(DirectoryPlanRejected, match='roles or subjects'):
        apply_directory_adjudications(compiled, documents, [{'case_id': case['case_id'], 'action': 'reference',
            'basis_windows': [window(0, 4)], 'chapter_ids': ['owned']}])


def test_reference_cannot_merge_distinct_explicit_source_submission_entries():
    from docreader.tender.directory_choices import apply_directory_adjudications, directory_cases
    documents = docs(['总格式\n子格式\n共用正文\n采用来源格式', '甲提交位置\n乙提交位置'])
    values = [plan([{**node(identity, 0, heading), 'body_windows': [window(0, heading + 1, 2)]}],
                   [item(identity, entry=0)], [entry(1, source_entry)])
              for identity, heading, source_entry in [('owned', 0, 0), ('extra', 1, 1)]]
    compiled = compile_plans(values, documents)
    case, = directory_cases(compiled, documents)
    with pytest.raises(DirectoryPlanRejected, match='submission'):
        apply_directory_adjudications(compiled, documents, [{'case_id': case['case_id'], 'action': 'reference',
            'basis_windows': [window(0, 3)], 'chapter_ids': ['owned']}])


def test_alternative_parent_is_mapped_by_source_heading_not_other_slot_local_id():
    from docreader.tender.directory_choices import apply_directory_adjudications, directory_cases
    documents = docs(['父章\n父正文\n子章\n正文甲\n正文乙\n选用另一方案'])
    values = [plan([{**node(parent, 0, 0), 'body_windows': [window(0, 1)]},
                    {**node(child, 0, 2, parent), 'body_windows': [window(0, body)]}], [item(parent, 'subtree')])
              for parent, child, body in [('parent-a', 'child-a', 3), ('parent-b', 'child-b', 4)]]
    compiled = compile_plans(values, documents)
    case, = directory_cases(compiled, documents)
    apply_directory_adjudications(compiled, documents, [{'case_id': case['case_id'], 'action': 'alternative',
        'basis_windows': [window(0, 5)]}])
    child = next(c for c in compiled['chapters'] if c['id'] == 'child-a')
    assert child['parent_id'] == 'parent-a' and child['text'] == '正文乙'


def test_container_cannot_discard_a_candidate_body_line_not_carried_by_the_child():
    from docreader.tender.directory_choices import apply_directory_adjudications, directory_cases
    compiled, documents = overlapping_extra(unique=True)
    case, = directory_cases(compiled, documents)
    with pytest.raises(DirectoryPlanRejected):
        apply_directory_adjudications(compiled, documents, [{'case_id': case['case_id'], 'action': 'container',
            'basis_windows': [window(0, 5)], 'chapter_ids': ['owned']}])


def test_source_condition_change_invalidates_the_stored_case():
    from docreader.tender.directory_choices import directory_cases
    compiled, documents = differing_bodies()
    documents[0]['pages'][0] = documents[0]['pages'][0].replace('适用于代理商', '适用于制造商')
    with pytest.raises(DirectoryPlanRejected, match='source changed'):
        directory_cases(compiled, documents)


def test_repeated_adoption_alternative_rebinds_the_same_complete_native_table():
    from docreader.tender.directory_choices import apply_directory_adjudications, directory_cases
    from test_directory_body_windows import table_documents

    documents = table_documents()
    condition_line = len(documents[0]['pages'][0].splitlines())
    documents[0]['pages'][0] += '\n仅代理商适用\n仅制造商适用'
    chosen = plan([{**node('form', 0, 0), 'body_windows': [window(0, 1, 3)]}],
                  [item('form', condition=window(0, condition_line)),
                   item('form', condition=window(0, condition_line + 1))])
    compiled = compile_plans([chosen], documents)
    original_parts = deepcopy(compiled['chapters'][0]['template_parts'])
    choices = [{'case_id': case['case_id'], 'action': 'alternative' if case['kind'] == 'duplicate_condition'
                else 'unresolved', 'basis_windows': [window(0, condition_line + 1)]}
               for case in directory_cases(compiled, documents)]
    apply_directory_adjudications(compiled, documents, choices)
    assert compiled['chapters'][0]['template_parts'] == original_parts
