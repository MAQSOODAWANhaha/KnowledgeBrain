"""A source subject layout can change without weakening the current material or condition."""

from copy import deepcopy
from io import BytesIO

from docx import Document
import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.tender.authoring_export import export_authoring_document
from docreader.tender.directory_choices import (
    apply_directory_adjudications, decision_issues, directory_cases, source_case_issue,
)
from docreader.tender.directory_compile import (
    DirectoryAdjudication, DirectoryPlanRejected, compile_plans, documents_from_frozen, outline_from_compiled,
)
from test_authoring_pipeline_boundaries import snapshot
from test_directory_compile import docs, item, node, plan, point, window


SNAPSHOT = {'schema_version': 2, 'run_id': 'subject-layout', 'source_version': 1, 'domain_revision': 1}


def authorization_case():
    documents = docs(['\n'.join([
        '资格资料', '代理商须有原厂授权，制造商和代理商不得同时参与。', '仍须另附支持服务证明。',
        '格式供参考（如需）', '制造商授权函', '制造商授权投标人参加本项目。', '授权期限：________',
        '投标人盖章：________；制造商盖章：________', '', '附证说明：应附资格材料。',
        '其他材料', '其他正文。', '项目资料适用。', '仅参考格式适用。',
    ])])
    first = plan([
        {**node('qualification', 0, 0), 'body_windows': []},
        {**node('authorization', 0, 4, parent='qualification'), 'body_windows': [window(0, 5, 8)],
         'subjects': [{'source': window(0, 7), 'role': role} for role in ('投标人', '制造商')]},
        {**node('other', 0, 10, parent='qualification'), 'body_windows': [window(0, 11)]},
    ], [item('qualification'), item('authorization', condition=window(0, 1, 2), clause=[window(0, 9)]), item('other')])
    second = deepcopy(first)
    second['nodes'][1].update(subjects=[], response_kind='attachment', body_windows=[window(0, 5, 7)])
    second['nodes'][2]['response_kind'] = 'attachment'
    second['scopes'][0]['items'][1].update(condition=window(0, 3), clause=[window(0, 13)])
    return compile_plans([first, second], documents), documents


def authorization_issue(compiled):
    return next(issue for issue in compiled['issues'] if issue.get('_directory_case', {}).get('kind') == 'conflict'
                and issue['_directory_case']['current']['id'] == 'authorization')


def decisions(compiled, documents, action='subjects_alternative'):
    result = []
    for case in directory_cases(compiled, documents):
        row = {'case_id': case['case_id'], 'action': 'unresolved', 'basis_windows': [], 'chapter_ids': []}
        if case['current']['id'] == 'authorization':
            row.update(action='conditional' if case['kind'] == 'applicability' else action,
                       basis_windows=[window(0, 1, 2) if case['kind'] == 'applicability' else window(0, 5, 7)])
        result.append(row)
    return result


def changed_alternative(compiled, change):
    issue = authorization_issue(compiled)
    case = issue['_directory_case']
    alternative = deepcopy(case['alternative'])
    change(alternative)
    original = {key: value for key, value in issue.items() if key not in {'_directory_case', 'directory_case_id'}}
    replacement = source_case_issue(original, 'conflict', case['current'], alternative,
                                    compiled['chapters'], compiled['chapters'])
    compiled['issues'][compiled['issues'].index(issue)] = replacement


def test_closed_action_contract_adds_subject_choice_without_changing_existing_actions():
    for action in ('current', 'alternative', 'conditional', 'subjects_alternative'):
        assert DirectoryAdjudication.model_validate({'case_id': 'source-case', 'action': action}).action == action


def test_subject_choice_keeps_every_non_subject_field_and_other_unresolved_case():
    compiled, documents = authorization_case()
    original = deepcopy(compiled)
    before = deepcopy(next(chapter for chapter in compiled['chapters'] if chapter['id'] == 'authorization'))
    source_case = deepcopy(authorization_issue(compiled)['_directory_case'])
    other_case = next(case['case_id'] for case in directory_cases(compiled, documents) if case['current']['id'] == 'other')
    apply_directory_adjudications(compiled, documents, decisions(compiled, documents))
    after = next(chapter for chapter in compiled['chapters'] if chapter['id'] == 'authorization')
    assert after == {**before, 'subjects': []}
    assert original['chapters'][1]['subjects'] and source_case['current']['subjects']
    assert {issue.get('directory_case_id') for issue in compiled['issues']} == {other_case}
    record = next(record for record in compiled['directory_decisions'] if record['action'] == 'subjects_alternative')
    assert record['field_sources'] == {'subjects': 'alternative', 'other_fields': 'current'}
    assert record['candidates'] == {key: source_case[key] for key in ('current', 'alternative')}
    assert record['selected_candidate'] == after
    assert not record['enterprise_confirmed']
    assert '代理商须有原厂授权' in record['source_records'][0]['original_text']
    assert '附证说明：应附资格材料' in record['source_records'][0]['original_text']
    assert any(issue['code'] == 'outline_directory_conditional_requirement' for issue in decision_issues(compiled))


def test_whole_alternative_still_cannot_replace_the_project_condition_in_a_conditional_batch():
    compiled, documents = authorization_case()
    original = deepcopy(compiled)
    with pytest.raises(DirectoryPlanRejected, match='condition changed'):
        apply_directory_adjudications(compiled, documents, decisions(compiled, documents, 'alternative'))
    assert compiled == original


def test_blank_word_has_one_joint_signed_form_with_the_full_current_condition(tmp_path):
    compiled, documents = authorization_case()
    apply_directory_adjudications(compiled, documents, decisions(compiled, documents))
    outline = outline_from_compiled(SNAPSHOT, compiled)
    chapter = next(child for child in outline['chapters'][0]['children'] if child['title'] == '制造商授权函')
    assert len([target for target in chapter['targets'] if target['kind'] == 'form']) == 1
    result = export_authoring_document(ArtifactStore(tmp_path / 'artifacts'), outline, [])
    document = Document(BytesIO((tmp_path / 'artifacts' / result['docx_ref']['relative_path']).read_bytes()))
    start = next(index for index, p in enumerate(document.paragraphs)
                 if p.style.name.startswith('Heading') and p.text == chapter['title'])
    end = next(index for index in range(start + 1, len(document.paragraphs))
               if document.paragraphs[index].style.name.startswith('Heading'))
    text = '\n'.join(p.text for p in document.paragraphs[start:end])
    assert text.count('投标人盖章：________；制造商盖章：________') == 1
    assert '代理商须有原厂授权，制造商和代理商不得同时参与。\n仍须另附支持服务证明。' in text
    assert '附证说明：应附资格材料。' in text
    assert '格式供参考（如需）' not in text


@pytest.mark.parametrize('problem', ['node', 'heading', 'entry', 'body', 'same_roles'])
def test_subject_choice_rejects_a_different_material_or_unchanged_layout_atomically(problem):
    compiled, documents = authorization_case()

    def change(alternative):
        if problem == 'node':
            alternative['id'] = 'different-source-node'
        elif problem == 'heading':
            alternative['heading'] = point(0, 10)
        elif problem == 'entry':
            alternative['entry'] = point(0, 10)
        elif problem == 'body':
            alternative['lines'] = alternative['lines'][:1]
            alternative['text'] = documents[0]['pages'][0].splitlines()[5]
        else:
            alternative['subjects'] = deepcopy(authorization_issue(compiled)['_directory_case']['current']['subjects'])
            for subject in alternative['subjects']:
                subject['source'] = window(0, 5)

    changed_alternative(compiled, change)
    original = deepcopy(compiled)
    with pytest.raises(DirectoryPlanRejected, match='same source material|different subject layout'):
        apply_directory_adjudications(compiled, documents, decisions(compiled, documents))
    assert compiled == original


@pytest.mark.parametrize('problem', ['missing', 'duplicate', 'unknown', 'no_basis', 'bad_basis', 'wrong_chapter', 'no_alternative'])
def test_subject_choice_keeps_unique_case_full_batch_source_and_target_checks(problem):
    compiled, documents = authorization_case()
    batch = decisions(compiled, documents)
    choice = next(row for row in batch if row['action'] == 'subjects_alternative')
    if problem == 'missing':
        batch = [choice]
    elif problem == 'duplicate':
        batch.append(deepcopy(choice))
    elif problem == 'unknown':
        choice['case_id'] = 'unknown-case'
    elif problem == 'no_basis':
        choice['basis_windows'] = []
    elif problem == 'bad_basis':
        choice['basis_windows'] = [window(0, 999)]
    elif problem == 'wrong_chapter':
        choice['chapter_ids'] = ['other']
    else:
        choice['case_id'] = next(row['case_id'] for row in batch if row['action'] == 'conditional')
        batch = [row for row in batch if row is choice or row['action'] != 'conditional']
    original = deepcopy(compiled)
    with pytest.raises(DirectoryPlanRejected):
        apply_directory_adjudications(compiled, documents, batch)
    assert compiled == original


@pytest.mark.parametrize('reverse', [False, True])
def test_conflicting_whole_and_subject_selections_of_one_node_are_order_independent_and_atomic(reverse):
    compiled, documents = authorization_case()
    source_case = deepcopy(authorization_issue(compiled)['_directory_case'])
    other = deepcopy(source_case['alternative'])
    other['response_kind'] = 'form'
    issue = source_case_issue({'code': 'outline_directory_conflict', 'detail': '另一完整方案待核'},
                             'conflict', source_case['current'], other, compiled['chapters'], compiled['chapters'])
    compiled['issues'].append(issue)
    batch = decisions(compiled, documents)
    next(row for row in batch if row['case_id'] == issue['directory_case_id'])['action'] = 'alternative'
    original = deepcopy(compiled)
    with pytest.raises(DirectoryPlanRejected):
        apply_directory_adjudications(compiled, documents, list(reversed(batch)) if reverse else batch)
    assert compiled == original


def test_subject_choice_preserves_the_basic_form_native_cells_and_blank_fields(tmp_path):
    artifacts = ArtifactStore(tmp_path / 'artifacts')
    grid = {'row_count': 2, 'column_count': 2, 'cells': [
        {'row': row, 'column': column, 'text': text, 'row_span': 1, 'col_span': 1}
        for row, values in enumerate([['投标人名称', ''], ['制造商名称', '']])
        for column, text in enumerate(values)]}
    fixed = snapshot(artifacts, [{'text': '基本情况表\n投标人和制造商的资质应附于本表后。'}, {'grid': grid}])
    documents = documents_from_frozen(artifacts, artifacts.read_json(fixed['frozen_ref']))
    chosen = {**node('basic', 0, 0, document_id='document'), 'body_windows': [window(1, 0, 1, 'document')],
              'subjects': [{'source': window(0, 1, document_id='document'), 'role': role} for role in ('投标人', '制造商')]}
    first = plan([chosen], [item('basic', clause=[window(0, 1, document_id='document')])])
    second = deepcopy(first)
    second['nodes'][0]['subjects'] = []
    compiled = compile_plans([first, second], documents)
    before = deepcopy(compiled['chapters'][0])
    case, = directory_cases(compiled, documents)
    apply_directory_adjudications(compiled, documents, [{'case_id': case['case_id'], 'action': 'subjects_alternative',
        'basis_windows': [window(0, 1, document_id='document')]}])
    assert compiled['chapters'][0] == {**before, 'subjects': []}
    result = export_authoring_document(artifacts, outline_from_compiled(fixed, compiled), [])
    document = Document(BytesIO(artifacts.read_bytes(result['docx_ref'])))
    assert len(document.tables) == 1
    assert [[cell.text for cell in row.cells] for row in document.tables[0].rows] == [['投标人名称', ''], ['制造商名称', '']]
    assert '投标人和制造商的资质应附于本表后。' in '\n'.join(p.text for p in document.paragraphs)


def test_subject_choice_cannot_ignore_an_empty_row_owned_by_the_original_native_form():
    unit = 'page:0:table:0'
    documents = [{'document_id': 'tender', 'role': 'main', 'pages': ['原式\n固定字段\n\n投标人和制造商'],
                  'line_units': [[None, unit, unit, None]],
                  'template_parts': {unit: {'source_ref': {'source_index_ref': {'key': 'source'}, 'unit_id': unit},
                                             'selectors': []}},
                  'grids': [{'unit_id': unit, 'page': 0, 'rows': [['固定字段'], ['']]}]}]
    chosen = {**node('form', 0, 0), 'body_windows': [window(0, 1, 2)],
              'subjects': [{'source': window(0, 3), 'role': role} for role in ('投标人', '制造商')]}
    first = plan([chosen], [item('form')])
    second = deepcopy(first)
    second['nodes'][0].update(subjects=[], body_windows=[window(0, 1)])
    compiled = compile_plans([first, second], documents)
    original = deepcopy(compiled)
    case, = directory_cases(compiled, documents)
    with pytest.raises(DirectoryPlanRejected, match='same source material'):
        apply_directory_adjudications(compiled, documents, [{'case_id': case['case_id'], 'action': 'subjects_alternative',
            'basis_windows': [window(0, 3)]}])
    assert compiled == original


async def test_material_wire_acceptance_restores_subject_choice_and_keeps_the_saved_input_immutable():
    import json

    from docreader.llm.budget import RequestBudget
    from docreader.tender import authoring_pipeline as pipeline
    from docreader.tender.directory_compile import reading_payload
    from docreader.tender.directory_material_ids import MaterialIds
    from docreader.tender.directory_titles import title_catalog
    from test_directory_compile import fixture_titles

    compiled, documents = authorization_case()
    original = deepcopy(compiled)
    payload = {**pipeline._full_source_reading(reading_payload(documents)),
               'chapters': pipeline._directory_chapters(compiled), 'applicability': pipeline._directory_applicability(compiled),
               'unplaced_tables': [], 'table_header_catalog': [], 'chapter_title_catalog': title_catalog(compiled, documents),
               'directory_cases': directory_cases(compiled, documents)}
    raw = {'assignments': [], 'table_headers': [], 'chapter_titles': fixture_titles(payload),
           'directory_adjudications': decisions(compiled, documents)}
    identities = MaterialIds(payload)
    wire = deepcopy(raw)
    for row in wire['chapter_titles']:
        row['title_id'] = identities.forward['n'][row['title_id']]
    for row in wire['directory_adjudications']:
        row['case_id'] = identities.forward['q'][row['case_id']]
        for selected in row['basis_windows']:
            selected['document_id'] = identities.forward['d'][selected['document_id']]

    async def execute(task, request, validate):
        assert task == 'directory_material'
        assert json.loads(request['messages'][1]['content'][0]['text']) == identities.payload
        assert 'subjects_alternative' in request['messages'][0]['content']
        return validate(wire)

    accepted = await pipeline._ask(execute, RequestBudget(model='fixture'), 'directory_material', payload,
                                   lambda value: pipeline._accept_material_plan(value, [], compiled, documents))
    assert compiled == original
    assert accepted['directory_adjudications'] == raw['directory_adjudications']


def test_subject_choice_can_preserve_separate_financial_role_positions_without_any_financial_matrix():
    documents = docs(['投标人和制造商均须提供年度审计资料。', '近年财务状况资料\n后附审计报告。'])
    chosen = {**node('finance', 1, 0), 'body_windows': [window(1, 0, 1)], 'response_kind': 'attachment',
              'subjects': [{'source': window(0, 0), 'role': '投标人'}]}
    first = plan([chosen], [item('finance')])
    second = deepcopy(first)
    second['nodes'][0]['subjects'].append({'source': window(0, 0), 'role': '制造商'})
    compiled = compile_plans([first, second], documents)
    before = deepcopy(compiled['chapters'][0])
    case, = directory_cases(compiled, documents)
    apply_directory_adjudications(compiled, documents, [{'case_id': case['case_id'], 'action': 'subjects_alternative',
        'basis_windows': [window(0, 0)]}])
    after = compiled['chapters'][0]
    assert {key: value for key, value in after.items() if key != 'subjects'} == {key: value for key, value in before.items() if key != 'subjects'}
    targets = outline_from_compiled(SNAPSHOT, compiled)['chapters'][0]['targets']
    assert {target['subject'] for target in targets} == {'投标人', '制造商'}
    assert all(target['kind'] == 'attachment' and not target.get('template_refs') and target['manual_fill'] for target in targets)
