"""Resolve source-bound pending cases without fabricating delivery or source text."""

from copy import deepcopy
from io import BytesIO
from zipfile import ZipFile

import pytest

from docreader.tender.directory_compile import (
    ClassificationPlan, DirectoryPlanRejected, apply_requirements, compile_directory, outline_from_compiled,
)
from docreader.integrations.artifacts import ArtifactStore
from docreader.tender.authoring_export import export_authoring_document
from docreader.tender.directory_adjudication import (
    apply_requirement_adjudications, requirement_cases, validate_requirement_adjudications,
)


def docs(pages, *documents):
    return [{'document_id': 'tender', 'role': 'main', 'pages': pages}, *documents]


def window(page, start, end=None, document_id='tender'):
    return {'document_id': document_id, 'page': page, 'start_line': start,
            'end_line': start if end is None else end}


def pending(pages, selections, *extra_documents):
    documents = docs(pages, *extra_documents)
    compiled = {'chapters': [{'id': 'business', 'title': '商务应答', 'lines': []},
                             {'id': 'technical', 'title': '技术应答', 'lines': []}],
                'issues': []}
    apply_requirements(compiled, documents, [{**selection, 'scope': 'unassigned'}
                                            for selection in selections])
    return documents, compiled


def decide(cases, scope='chapter', **extra):
    return {'item_ids': [case['id'] for case in cases], 'scope': scope,
            'chapter_ids': ['business'] if scope == 'chapter' else [], **extra}


def test_classification_contract_and_case_payload_keep_host_source_identity():
    documents, compiled = pending(['前提\n需要提交材料。'], [window(0, 1)])
    cases = requirement_cases(compiled, documents)
    copied = deepcopy(compiled)
    apply_requirements(copied, documents, [{**window(0, 1), 'scope': 'unassigned'}])
    assert requirement_cases(copied, documents) == cases
    assert cases == [{'id': cases[0]['id'], 'source_windows': [window(0, 1)]}]
    stored = compiled['_requirement_cases'][0]
    assert stored['source_text'] == '需要提交材料。'
    assert stored['issue_indices'] == [0]
    assert compiled['issues'][0]['requirement_case_id'] == cases[0]['id']
    parsed = ClassificationPlan.model_validate({'items': [], 'requirement_adjudications': [
        decide(cases, 'procedure')]}).model_dump()
    assert parsed['requirement_adjudications'][0]['scope'] == 'procedure'
    assert ClassificationPlan.model_validate({'items': []}).requirement_adjudications == []


def test_split_requirement_becomes_one_complete_chapter_response():
    documents, compiled = pending(['需要由外部签署生效的资料，也', '页眉\n须提交有效原件扫描件。'],
                                  [window(0, 0), window(1, 1)])
    cases = requirement_cases(compiled, documents)
    apply_requirement_adjudications(compiled, documents, [decide(list(reversed(cases)))])
    assert compiled['chapters'][0]['requirements'] == [
        '需要由外部签署生效的资料，也\n须提交有效原件扫描件。']
    assert not compiled['issues']
    assert requirement_cases(compiled, documents) == []


def test_identical_text_at_distinct_sources_does_not_remove_another_issue():
    documents, compiled = pending(['需要提供证明。\n需要提供证明。'], [window(0, 0), window(0, 1)])
    cases = requirement_cases(compiled, documents)
    assert cases[0]['id'] != cases[1]['id']
    unrelated = {'code': 'outline_directory_requirement_unassigned', 'detail': '需要提供证明。'}
    compiled['issues'].insert(0, unrelated)
    apply_requirement_adjudications(compiled, documents, [
        decide(cases[:1]), decide(cases[1:], 'unassigned')])
    assert compiled['chapters'][0]['requirements'] == ['需要提供证明。']
    assert compiled['issues'][0] is unrelated
    assert [case['id'] for case in requirement_cases(compiled, documents)] == [cases[1]['id']]


def test_procedure_retains_pending_execution_and_other_compilation_findings():
    documents, compiled = pending(['收到修改后按规定途径确认。'], [window(0, 0)])
    other = {'code': 'outline_directory_conflict', 'detail': '所有权仍待核'}
    compiled['issues'].append(other)
    apply_requirement_adjudications(compiled, documents, [decide(requirement_cases(compiled, documents), 'procedure')])
    assert other in compiled['issues']
    procedure = next(issue for issue in compiled['issues']
                     if issue['code'] == 'outline_directory_procedure_requirement')
    assert '待执行（不代表已完成）' in procedure['detail']
    assert '收到修改后按规定途径确认。' in procedure['detail']
    assert not any(chapter.get('requirements') for chapter in compiled['chapters'])


def test_excluded_optional_quote_keeps_original_and_exclusion_basis_without_adding_quote():
    documents, compiled = pending(['运行备件逐项报价，计评标价但不计合同总价。', '专用附表：本次不适用。'],
                                  [window(0, 0)])
    decision = decide(requirement_cases(compiled, documents), 'not_applicable', basis_windows=[window(1, 0)])
    apply_requirement_adjudications(compiled, documents, [decision])
    assert not any(chapter.get('requirements') for chapter in compiled['chapters'])
    issue = compiled['issues'][0]
    assert issue['code'] == 'outline_directory_requirement_not_applicable'
    assert '运行备件逐项报价，计评标价但不计合同总价。' in issue['detail']
    assert '专用附表：本次不适用。' in issue['detail']


def test_not_applicable_rejects_missing_or_nonexistent_basis_without_mutation():
    documents, compiled = pending(['必须交付。\n'], [window(0, 0)])
    cases = requirement_cases(compiled, documents)
    before = deepcopy(compiled)
    for basis in ([], [window(9, 0)], [window(0, 2)]):
        with pytest.raises(DirectoryPlanRejected):
            apply_requirement_adjudications(compiled, documents, [
                decide(cases, 'not_applicable', basis_windows=basis)])
        assert compiled == before


def test_every_case_is_required_once_and_unknown_duplicate_or_missing_ids_are_rejected():
    documents, compiled = pending(['条款甲\n条款乙'], [window(0, 0), window(0, 1)])
    cases = requirement_cases(compiled, documents)
    before = deepcopy(compiled)
    for decisions in ([], [decide(cases[:1])], [decide(cases), decide(cases[:1])],
                      [decide(cases, item_ids=[cases[0]['id'], cases[0]['id']])],
                      [decide(cases), {'item_ids': ['unknown'], 'scope': 'unassigned'}]):
        with pytest.raises(DirectoryPlanRejected):
            validate_requirement_adjudications(compiled, documents, decisions)
        assert compiled == before


def test_scope_and_chapter_targets_cannot_be_silently_ignored():
    documents, compiled = pending(['独立要求'], [window(0, 0)])
    cases = requirement_cases(compiled, documents)
    for scope, ids in [('chapter', []), ('chapter', ['missing']),
                       ('chapter', ['business', 'technical']), ('chapters', []),
                       ('chapters', ['business', 'missing']), ('procedure', ['business']),
                       ('document', ['business']), ('not_applicable', ['business']),
                       ('unassigned', ['business']), ('unrecognised', [])]:
        with pytest.raises(DirectoryPlanRejected):
            validate_requirement_adjudications(compiled, documents, [
                decide(cases, scope, chapter_ids=ids, basis_windows=[window(0, 0)])])
    apply_requirement_adjudications(compiled, documents, [
        decide(cases, 'chapters', chapter_ids=['business', 'technical'])])
    assert [chapter['requirements'] for chapter in compiled['chapters']] == [['独立要求'], ['独立要求']]


def test_merged_cases_cannot_cross_registered_documents():
    documents, compiled = pending(['前文'], [window(0, 0), window(0, 0, document_id='other')],
                                  {'document_id': 'other', 'role': 'main', 'pages': ['后文']})
    with pytest.raises(DirectoryPlanRejected, match='same document'):
        validate_requirement_adjudications(compiled, documents, [decide(requirement_cases(compiled, documents))])


def test_source_order_and_overlap_are_deduplicated_by_coordinates():
    documents, compiled = pending(['甲\n乙\n丙\n丁'], [window(0, 0, 2), window(0, 1, 3)])
    cases = requirement_cases(compiled, documents)
    apply_requirement_adjudications(compiled, documents, [decide(list(reversed(cases)))])
    assert compiled['chapters'][0]['requirements'] == ['甲\n乙\n丙\n丁']


def test_unassigned_stays_unresolved_and_document_keeps_global_outlet():
    documents, compiled = pending(['确实未决\n全稿规则'], [window(0, 0), window(0, 1)])
    cases = requirement_cases(compiled, documents)
    original = compiled['issues'][0]
    apply_requirement_adjudications(compiled, documents, [
        decide(cases[:1], 'unassigned'), decide(cases[1:], 'document')])
    assert compiled['issues'][0] is original
    assert {'code': 'outline_directory_document_requirement', 'detail': '全稿规则'} in compiled['issues']
    assert requirement_cases(compiled, documents) == cases[:1]


def test_changed_source_snapshot_is_rejected_before_any_original_issue_is_removed():
    documents, compiled = pending(['原条款'], [window(0, 0)])
    decisions = [decide(requirement_cases(compiled, documents))]
    documents[0]['pages'][0] = '已变化的条款'
    before = deepcopy(compiled)
    with pytest.raises(DirectoryPlanRejected, match='source'):
        apply_requirement_adjudications(compiled, documents, decisions)
    assert compiled == before


def test_merged_unassigned_has_one_complete_pending_case_and_keeps_source_case_history():
    documents, compiled = pending(['其他主体签署生效的证明，也', '页眉\n须提交有效原件。'],
                                  [window(0, 0), window(1, 1)])
    original_cases = deepcopy(compiled['_requirement_cases'])
    cases = requirement_cases(compiled, documents)
    apply_requirement_adjudications(compiled, documents, [decide(cases, 'unassigned')])
    active = requirement_cases(compiled, documents)
    assert len(active) == 1 and active[0]['id'] not in {case['id'] for case in cases}
    assert active[0]['source_windows'] == [window(0, 0), window(1, 1)]
    issue = compiled['issues'][0]
    assert len(compiled['issues']) == 1
    assert issue['code'] == 'outline_directory_requirement_unassigned'
    assert issue['detail'] == '其他主体签署生效的证明，也\n须提交有效原件。'
    merged = next(case for case in compiled['_requirement_cases'] if case['id'] == active[0]['id'])
    assert set(merged['merged_from_case_ids']) == {case['id'] for case in cases}
    assert compiled['_requirement_cases'][:2] == original_cases


@pytest.mark.parametrize('scope', ['document', 'chapter', 'chapters'])
def test_adjudicated_one_page_basis_stays_complete_when_its_first_line_is_already_form_body(scope):
    documents, compiled = pending(['凡需他方签章生效的资料，\n必须交有效原件扫描件。'], [])
    compiled['chapters'][0]['lines'] = [{'document_id': 'tender', 'page': 0, 'line': 0}]
    apply_requirements(compiled, documents, [{**window(0, 0, 1), 'scope': 'unassigned'}])
    cases = requirement_cases(compiled, documents)
    ids = ['business', 'technical'] if scope == 'chapters' else ['business'] if scope == 'chapter' else []
    original_body = deepcopy(compiled['chapters'][0]['lines'])
    apply_requirement_adjudications(compiled, documents, [decide(cases, scope, chapter_ids=ids)])
    complete = '凡需他方签章生效的资料，\n必须交有效原件扫描件。'
    if scope == 'document':
        assert {'code': 'outline_directory_document_requirement', 'detail': complete} in compiled['issues']
    else:
        for chapter in compiled['chapters']:
            if chapter['id'] in ids:
                assert chapter['requirements'] == [complete]
    assert compiled['chapters'][0]['lines'] == original_body


def test_complete_adjudication_basis_respects_same_chapter_obligation_ownership_only():
    documents, compiled = pending(['应提交方案，\n并逐项说明。'], [window(0, 0, 1)])
    compiled['chapters'][0]['obligation_lines'] = [
        {'document_id': 'tender', 'page': 0, 'line': line} for line in (0, 1)]
    apply_requirement_adjudications(compiled, documents, [
        decide(requirement_cases(compiled, documents), 'chapters', chapter_ids=['business', 'technical'])])
    assert not compiled['chapters'][0].get('requirements')
    assert compiled['chapters'][1]['requirements'] == ['应提交方案，\n并逐项说明。']


def test_single_unassigned_restores_complete_human_outlet_without_changing_known_source_identity():
    documents, compiled = pending(['仅在实际触发时，\n必须附原件证明。'], [])
    compiled['chapters'][0]['lines'] = [{'document_id': 'tender', 'page': 0, 'line': 0}]
    apply_requirements(compiled, documents, [{**window(0, 0, 1), 'scope': 'unassigned'}])
    cases = requirement_cases(compiled, documents)
    case_before = deepcopy(compiled['_requirement_cases'][0])
    apply_requirement_adjudications(compiled, documents, [decide(cases, 'unassigned')])
    assert compiled['issues'][0]['detail'] == '仅在实际触发时，\n必须附原件证明。'
    assert compiled['issues'][0]['code'] == 'outline_directory_requirement_unassigned'
    assert requirement_cases(compiled, documents) == cases
    assert compiled['_requirement_cases'][0] == case_before


@pytest.mark.parametrize('scope,targets', [('chapter', ['cover']), ('chapters', ['business', 'cover'])])
def test_strict_adjudication_rejects_cover_targets_but_document_and_legacy_B_have_outlets(scope, targets):
    documents, compiled = pending(['全稿须按规定签章。'], [window(0, 0)])
    compiled['chapters'].append({'id': 'cover', 'title': '封面', 'presentation': 'cover', 'lines': []})
    cases = requirement_cases(compiled, documents)
    before = deepcopy(compiled)
    with pytest.raises(DirectoryPlanRejected, match='document'):
        apply_requirement_adjudications(compiled, documents, [decide(cases, scope, chapter_ids=targets)])
    assert compiled == before
    apply_requirement_adjudications(compiled, documents, [decide(cases, 'document')])
    assert {'code': 'outline_directory_document_requirement', 'detail': '全稿须按规定签章。'} in compiled['issues']
    legacy = deepcopy(before)
    apply_requirements(legacy, documents, [{**window(0, 0), 'scope': scope, 'chapter_ids': targets}])
    assert {'code': 'outline_directory_document_requirement', 'detail': '全稿须按规定签章。'} in legacy['issues']


def test_merged_requirement_decision_keeps_case_source_and_basis_trace_in_final_outline_and_zip_guide(tmp_path):
    documents = docs(['待核完整义务的前半，', '同一义务的后半。', '本次专用条款明确排除该材料。',
                      '应答章节\n保留原式内容。'])
    compiled = compile_directory({
        'nodes': [{'id': 'business', 'heading': {'document_id': 'tender', 'page': 3, 'line': 0},
                   'body_windows': [window(3, 1)], 'response_kind': 'form'}],
        'composition': {'entries': [], 'priority_basis': []},
        'scopes': [{'items': [{'node_id': 'business', 'depth': 'self', 'presentation': 'section'}]}],
    }, documents)
    apply_requirements(compiled, documents, [{**window(page, 0), 'scope': 'unassigned'} for page in (0, 1)])
    original = requirement_cases(compiled, documents)
    apply_requirement_adjudications(compiled, documents, [decide(original, 'unassigned')])
    merged = requirement_cases(compiled, documents)
    apply_requirement_adjudications(compiled, documents, [
        decide(merged, 'not_applicable', basis_windows=[window(2, 0)])])
    outline = outline_from_compiled({'schema_version': 2, 'run_id': 'source-fixture',
                                    'source_version': 1, 'domain_revision': 1}, compiled)
    published = '\n'.join(issue['detail'] for issue in outline['issues']
                          if issue['code'] == 'outline_directory_source_decision')
    required = [original[0]['id'], original[1]['id'], merged[0]['id'], 'not_applicable',
                'tender 第1页 第1–1行', 'tender 第2页 第1–1行', 'tender 第3页 第1–1行',
                '待核完整义务的前半，', '同一义务的后半。', '本次专用条款明确排除该材料。']
    assert all(value in published for value in required)
    assert all(not record['enterprise_confirmed'] for record in compiled['requirement_decisions'])
    artifacts = ArtifactStore(tmp_path)
    result = export_authoring_document(artifacts, outline, [])
    with ZipFile(BytesIO(artifacts.read_bytes(result['package_ref']))) as package:
        guide = package.read('提交要求与待办.md').decode()
    assert '来源核定记录' in guide
    assert all(value in guide for value in required)
