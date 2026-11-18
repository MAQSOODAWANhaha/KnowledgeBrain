"""Only the same chapter's actually carried source obligation owns a response."""

from copy import deepcopy
from io import BytesIO
from zipfile import ZipFile

from docx import Document

from docreader.integrations.artifacts import ArtifactStore
from docreader.tender.authoring_export import export_authoring_document
from docreader.tender.directory_compile import (
    apply_requirements, compile_directory, compile_plans, outline_from_compiled,
)
from test_directory_compile import docs, entry, item, node, plan, window


SNAPSHOT = {'schema_version': 2, 'run_id': 'run', 'source_version': 1, 'domain_revision': 1}


def case():
    documents = docs(['组成依据\n须提供承诺书。\n还须另附原件。\n须提供承诺书。\n仅代理商适用。',
                      '原式甲\n签署：________', '原式乙\n签署：________'])
    nodes = [{**node('a', 1, 0), 'body_windows': [window(1, 0, 1)]},
             {**node('b', 2, 0), 'body_windows': [window(2, 0, 1)]}]
    chosen = plan(nodes, [item('a', clause=[window(0, 1)], condition=window(0, 4)), item('b')])
    return documents, chosen, compile_directory(chosen, documents)


def assignment(start=1, end=None, ids=('a',), scope='chapter'):
    return {**window(0, start, end), 'scope': scope, 'chapter_ids': list(ids)}


def test_same_source_duty_has_one_response_but_form_and_condition_keep_their_outlets():
    documents, _, compiled = case()
    form = compiled['chapters'][0]['text']
    apply_requirements(compiled, documents, [assignment()])
    assert compiled['chapters'][0].get('requirements', []) == []
    chapter = outline_from_compiled(SNAPSHOT, compiled)['chapters'][0]
    assert chapter['targets'][0]['kind'] == 'form' and chapter['targets'][0]['purpose'] == form
    assert sum(target['purpose'] == '须提供承诺书。' for target in chapter['targets']) == 1
    assert sum(target['purpose'] == '仅代理商适用。' for target in chapter['targets']) == 1


def test_shared_assignment_is_owned_separately_by_each_chapter():
    documents, _, compiled = case()
    apply_requirements(compiled, documents, [assignment(ids=('a', 'b'), scope='chapters')])
    assert compiled['chapters'][0].get('requirements', []) == []
    assert compiled['chapters'][1]['requirements'] == ['须提供承诺书。']
    assert all(chapter['requirement_cover'] for chapter in compiled['chapters'])
    assert any(issue['code'] == 'outline_directory_shared_requirement' and issue['detail'] == '须提供承诺书。'
               for issue in compiled['issues'])


def test_equal_text_at_a_different_source_coordinate_is_not_suppressed():
    documents, _, compiled = case()
    apply_requirements(compiled, documents, [assignment(3)])
    assert compiled['chapters'][0]['requirements'] == ['须提供承诺书。']


def test_partial_source_overlap_keeps_the_complete_new_duty_and_named_merge_outlet():
    documents, _, compiled = case()
    apply_requirements(compiled, documents, [assignment(1, 2)])
    assert compiled['chapters'][0]['requirements'] == ['须提供承诺书。\n还须另附原件。']
    pending = [issue for issue in compiled['issues']
               if issue['code'] == 'outline_directory_requirement_overlap_unconfirmed']
    assert len(pending) == 1 and '原式甲' in pending[0]['detail'] and 'Word' in pending[0]['detail']


def test_unadopted_plan_clause_does_not_claim_ownership_of_another_requirement():
    documents, first, _ = case()
    second = deepcopy(first)
    second['scopes'][0]['items'][0]['clause'] = [window(0, 2)]
    compiled = compile_plans([first, second], documents)
    assert compiled['chapters'][0]['obligations'] == '须提供承诺书。'
    apply_requirements(compiled, documents, [assignment(2)])
    assert compiled['chapters'][0]['requirements'] == ['还须另附原件。']


def test_partial_overlap_has_a_real_word_and_package_manual_resolution_outlet(tmp_path):
    documents, _, compiled = case()
    apply_requirements(compiled, documents, [assignment(1, 2)])
    outline = outline_from_compiled(SNAPSHOT, compiled)
    detail = next(issue['detail'] for issue in outline['issues']
                  if issue['code'] == 'outline_directory_requirement_overlap_unconfirmed')
    artifacts = ArtifactStore(tmp_path / 'artifacts')
    result = export_authoring_document(artifacts, outline, [])
    document = Document(BytesIO(artifacts.read_bytes(result['docx_ref'])))
    text = '\n'.join(paragraph.text for paragraph in document.paragraphs)
    assert detail in text and '处理结果：________（人工核定）' in text
    with ZipFile(BytesIO(artifacts.read_bytes(result['package_ref']))) as package:
        notes = package.read('提交要求与待办.md').decode()
    assert detail in notes and '处理结果：________（人工核定）' in notes


def test_clause_only_form_carrier_does_not_claim_a_narrative_response():
    documents = docs(['须说明履约服务方案。'])
    compiled = compile_directory(plan([], [item(None, entry=0, clause=[window(0, 0)])],
                                      [entry(0, 0)]), documents)
    apply_requirements(compiled, documents, [assignment(0, ids=('clause-1',))])
    assert compiled['chapters'][0]['requirements'] == ['须说明履约服务方案。']
    targets = outline_from_compiled(SNAPSHOT, compiled)['chapters'][0]['targets']
    assert [target['kind'] for target in targets] == ['form', 'narrative']


def test_clause_equal_to_form_body_is_not_owned_without_a_separate_narrative_outlet():
    documents = docs(['组成依据\n须说明履约服务方案。', '须说明履约服务方案。'])
    chosen = {**node('a', 1, 0), 'body_windows': [window(1, 0)]}
    compiled = compile_directory(plan([chosen], [item('a', clause=[window(0, 1)])]), documents)
    assert compiled['chapters'][0]['obligations'] == compiled['chapters'][0]['text']
    apply_requirements(compiled, documents, [assignment()])
    assert compiled['chapters'][0]['requirements'] == ['须说明履约服务方案。']
    targets = outline_from_compiled(SNAPSHOT, compiled)['chapters'][0]['targets']
    assert [target['kind'] for target in targets] == ['form', 'narrative']
