"""Response roles and source-grounded subjects survive into real blank Word positions."""

from copy import deepcopy
from io import BytesIO

from docx import Document
from pydantic import ValidationError
import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.tender import authoring_pipeline as pipeline
from docreader.tender.directory_compile import (
    DirectoryNode, DirectoryPlanRejected, apply_parameters, apply_requirements,
    compile_directory, compile_plans, documents_from_frozen, outline_from_compiled,
)
from test_authoring_pipeline_boundaries import budget, snapshot
from test_directory_compile import docs, entry, item, node, plan, point, window


SNAPSHOT = {'schema_version': 2, 'run_id': 'run', 'source_version': 1, 'domain_revision': 1}


def response_node(identity, page, line, windows, kind='form', subjects=(), parent=None):
    return {**node(identity, page, line, parent), 'body_windows': windows,
            'response_kind': kind, 'subjects': list(subjects)}


def subject(role, document_id='tender'):
    return {'source': window(0, 0, document_id=document_id), 'role': role}


async def no_enterprise_model(task, request, validate):
    assert task == 'authoring_structure', 'Manual templates must not call an enterprise writer.'
    return validate({'findings': []})


@pytest.mark.parametrize('value', [{}, {'response_kind': None}, {'response_kind': 'guess'}])
def test_new_model_contract_requires_an_existing_response_kind(value):
    with pytest.raises(ValidationError):
        DirectoryNode.model_validate({'id': 'form', 'heading': point(0, 0), 'body_windows': [], **value})


@pytest.mark.parametrize('kind', ['narrative', 'attachment'])
async def test_source_requirements_have_real_blank_response_or_attachment_positions(tmp_path, kind):
    artifacts = ArtifactStore(tmp_path / 'artifacts')
    fixed = snapshot(artifacts, [{'text': '原文要求\n必须提供本项目的响应和证明。'}])
    documents = documents_from_frozen(artifacts, artifacts.read_json(fixed['frozen_ref']))
    chosen = response_node('required', 0, 0, [window(0, 1, document_id='document')], kind)
    chosen['heading']['document_id'] = 'document'
    outline = outline_from_compiled(fixed, compile_directory(plan([chosen], [item('required')]), documents))
    result = artifacts.read_json(await pipeline.generate_document(
        artifacts, fixed, budget(), no_enterprise_model, outline=outline))
    document = Document(BytesIO(artifacts.read_bytes(result['docx_ref'])))
    text = '\n'.join(paragraph.text for paragraph in document.paragraphs)
    assert '必须提供本项目的响应和证明。' in text
    assert '________' in text
    if kind == 'attachment':
        assert '待附' in text
    else:
        assert '我方响应：________' in text and '证明材料或附件位置：________（原条款要求时填写）' in text
    assert outline['chapters'][0]['targets'][0]['kind'] == kind


@pytest.mark.parametrize('kind', ['form', 'narrative', 'attachment'])
async def test_two_subjects_share_one_chapter_but_have_independent_native_forms_and_common_requirements(tmp_path, kind):
    artifacts = ArtifactStore(tmp_path / 'artifacts')
    rows = [['项目', '金额'], ['资产', '']]
    grid = {'row_count': 2, 'column_count': 2, 'cells': [
        {'row': row, 'column': column, 'text': text, 'row_span': 1, 'col_span': 1}
        for row, values in enumerate(rows) for column, text in enumerate(values)]}
    fixed = snapshot(artifacts, [{'text': '需分别提供投标人及原厂财务情况。\n适用范围须人工核定。'}, {'grid': grid}])
    documents = documents_from_frozen(artifacts, artifacts.read_json(fixed['frozen_ref']))
    chosen = response_node('finance', 0, 0, [window(1, 0, 1, 'document')], kind, subjects=[
        subject('投标人', 'document'), subject('原厂', 'document')])
    chosen['heading']['document_id'] = 'document'
    compiled = compile_directory(plan([chosen], [item('finance', condition=window(0, 1, document_id='document'))]), documents)
    compiled['chapters'][0]['requirements'] = ['共同要求：审计材料须完整。']
    outline = outline_from_compiled(fixed, compiled)
    targets = outline['chapters'][0]['targets']
    assert len(outline['chapters']) == 1
    originals = [target for target in targets if target.get('template_refs')]
    assert [target.get('subject') for target in originals] == ['投标人', '原厂']
    assert len({target['target_id'] for target in originals}) == 2
    assert sum(target['purpose'] == '共同要求：审计材料须完整。' for target in targets) == 1
    assert sum(target['purpose'] == '适用范围须人工核定。' for target in targets) == 1
    assert any(issue['code'] == 'outline_directory_applicability_unconfirmed' for issue in compiled['issues'])
    result = artifacts.read_json(await pipeline.generate_document(
        artifacts, fixed, budget(), no_enterprise_model, outline=outline))
    document = Document(BytesIO(artifacts.read_bytes(result['docx_ref'])))
    assert len(document.tables) == 2
    assert [p.text for p in document.paragraphs if p.style.name == 'Caption'] == ['投标人', '原厂']
    assert document.tables[0].cell(1, 1).text == document.tables[1].cell(1, 1).text == ''
    assert document.tables[0].cell(1, 1)._tc is not document.tables[1].cell(1, 1)._tc
    document.tables[0].cell(1, 1).text = '独立人工填写'
    assert document.tables[1].cell(1, 1).text == ''


def test_subject_cannot_be_an_invented_company_or_role():
    chosen = response_node('finance', 0, 1, [], subjects=[subject('不存在的公司')])
    with pytest.raises(DirectoryPlanRejected, match='subject.*source'):
        compile_directory(plan([chosen], [item('finance')]), docs(['投标人与原厂\n财务表']))


@pytest.mark.parametrize('second', [
    {'response_kind': 'attachment'}, {'subjects': [subject('原厂')]},
])
def test_plan_role_or_subject_disagreements_remain_explicit_conflicts(second):
    first_node = response_node('finance', 0, 1, [window(0, 2)], subjects=[subject('投标人')])
    other = {**deepcopy(first_node), **second}
    compiled = compile_plans([plan([first_node], [item('finance')]), plan([other], [item('finance')])],
                             docs(['投标人与原厂\n财务表\n原式正文']))
    assert any(issue['code'] == 'outline_directory_conflict' for issue in compiled['issues'])


def test_empty_adopted_leaf_has_a_source_named_manual_exit_but_container_does_not():
    compiled = compile_directory(plan([
        response_node('container', 0, 0, []), response_node('authorization', 0, 1, [], parent='container'),
    ], [item('container', 'subtree')]), docs(['资格材料\n代理人须提交授权证明']))
    outline = outline_from_compiled(SNAPSHOT, compiled)
    assert outline['chapters'][0]['targets'] == []
    leaf = outline['chapters'][0]['children'][0]
    assert len(leaf['targets']) == 1
    assert leaf['targets'][0]['kind'] == 'narrative' and leaf['targets'][0]['manual_fill']
    assert leaf['targets'][0]['purpose'] == '代理人须提交授权证明'


async def test_detail_calls_receive_applicability_and_only_narrative_leaves_expand(monkeypatch):
    lines = '\n'.join(f'要求{number}' for number in range(26))
    documents = docs([lines, lines, lines])
    compiled = compile_directory(plan([
        response_node('form', 0, 0, [window(0, 0, 25)]),
        response_node('response', 1, 0, [window(1, 0, 25)], 'narrative'),
        response_node('proof', 2, 0, [window(2, 0, 25)], 'attachment'),
    ], [item('form'), item('response', condition=window(1, 0)), item('proof')]), documents)
    exclusion = {'code': 'outline_directory_excluded', 'detail': '原文明确该附表不适用。'}
    compiled['issues'].append(exclusion)
    calls = []

    async def ask(_execute, _budget, task, value, validate):
        calls.append((str(task), deepcopy(value)))
        if task == 'directory_parameter':
            return validate({'chapters': []})
        if task == 'directory_material':
            return validate({'assignments': [], 'chapter_titles': [
                {'title_id': entry['title_id'], 'title': {0: '规定原式', 1: '技术响应', 2: '证明材料'}[entry['heading']['page']]}
                for entry in value['chapter_title_catalog']], 'directory_adjudications': [
                {'case_id': case['case_id'], 'action': 'conditional', 'basis_windows': [window(1, 0)]}
                for case in value.get('directory_cases', [])]})
        return validate({'items': []} if task == 'directory_classification' else {'assignments': []})

    monkeypatch.setattr(pipeline, '_ask', ask)
    await pipeline._request_directory_details(compiled, documents, None, None)
    assert [value['chapter']['id'] for task, value in calls if task == 'directory_parameter'] == ['response']
    for task, value in calls:
        if task not in ('directory_material', 'directory_requirement'):
            continue
        chapters = {chapter['id']: chapter for chapter in value['chapters']}
        assert chapters['response']['response_kind'] == 'narrative' and chapters['response']['subjects'] == []
        assert value['applicability']['excluded'] == [exclusion]
        assert value['applicability']['conditions'] == [{'chapter_id': 'response', 'text': '要求0'}]


def test_shared_requirements_create_a_visible_target_in_each_chosen_chapter():
    documents = docs(['甲章\n甲正文\n乙章\n乙正文\n两处都应提供完整证明'])
    compiled = compile_directory(plan([
        response_node('first', 0, 0, [window(0, 1)]), response_node('second', 0, 2, [window(0, 3)]),
    ], [item('first'), item('second')]), documents)
    apply_requirements(compiled, documents, [{'document_id': 'tender', 'page': 0, 'start_line': 4, 'end_line': 4,
                                            'scope': 'chapters', 'chapter_ids': ['first', 'second']}])
    outline = outline_from_compiled(SNAPSHOT, compiled)
    assert all(any(t['purpose'] == '两处都应提供完整证明' for t in c['targets']) for c in outline['chapters'])
    assert any(issue['code'] == 'outline_directory_shared_requirement' for issue in compiled['issues'])


def test_parameter_children_inherit_response_role_and_subjects():
    documents = docs(['投标人和原厂\n' + '\n'.join(f'要求{number}' for number in range(25))])
    compiled = compile_directory(plan([
        response_node('response', 0, 1, [window(0, 1, 25)], 'narrative', [subject('投标人'), subject('原厂')]),
    ], [item('response')]), documents)
    apply_parameters(compiled, documents, [{'chapter_id': 'response', 'items': [
        {'document_id': 'tender', 'page': 0, 'start_line': 1, 'end_line': 3, 'content_start_line': 2}]}])
    child = compiled['chapters'][1]
    assert child['response_kind'] == 'narrative' and child['subjects'] == compiled['chapters'][0]['subjects']


@pytest.mark.parametrize('parented', [False, True])
def test_explicit_body_overlap_without_independent_submission_entries_is_rejected(parented):
    nodes = [response_node('first', 0, 0, [window(0, 2)]),
             response_node('second', 0, 1, [window(0, 2)], parent='first' if parented else None)]
    with pytest.raises(DirectoryPlanRejected, match='overlap|duplicate'):
        compile_directory(plan(nodes, [item('first', 'subtree')] if parented else [item('first'), item('second')]),
                          docs(['甲原式\n乙原式\n同一来源正文']))


def test_independent_source_entries_allow_two_explicit_submission_instances():
    nodes = [response_node('first', 1, 0, [window(1, 1)]), response_node('second', 1, 0, [window(1, 1)])]
    compiled = compile_directory(plan(nodes, [item('first', entry=0), item('second', entry=1)],
                                      [entry(0, 1), entry(0, 2)]), docs(['组成\n甲提交物\n乙提交物', '原式\n填写位']))
    assert len(compiled['chapters']) == 2


def test_shared_submission_entry_does_not_allow_two_copies_of_the_same_body():
    nodes = [response_node('first', 1, 0, [window(1, 1)]), response_node('second', 1, 0, [window(1, 1)])]
    with pytest.raises(DirectoryPlanRejected, match='duplicate body ownership'):
        compile_directory(plan(nodes, [item('first', entry=0), item('second', entry=0)], [entry(0, 1)]),
                          docs(['组成\n一个提交物', '原式\n填写位']))


def test_legacy_raw_node_defaults_to_form_without_subject_instances():
    legacy = plan([{**node('form', 0, 0), 'body_windows': [window(0, 1)]}], [item('form')])
    legacy['nodes'][0].pop('response_kind')
    compiled = compile_directory(legacy, docs(['原式\n空位']))
    target = outline_from_compiled(SNAPSHOT, compiled)['chapters'][0]['targets'][0]
    assert target['kind'] == 'form' and target['subject'] is None
