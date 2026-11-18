"""A full-source read closes unresolved block boundaries before claiming source completion."""

from copy import deepcopy
from io import BytesIO
from zipfile import ZipFile

import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.request import ModelContractError
from docreader.tender import authoring_pipeline as pipeline
from docreader.tender.authoring_export import export_authoring_document
from docreader.tender.directory_compile import compile_directory, requirement_blocks
from test_directory_compile import docs, item, node, plan, window


def restore_reading_pairs(payload):
    restored = {'documents': deepcopy(payload['documents'])}
    assert payload['source_line_format'] == 'numbered_line_pairs_v1'
    for document in restored['documents']:
        for page in document['pages']:
            page['lines'] = [{'line': line, 'text': text} for line, text in page['lines']]
    return restored


async def test_both_full_directory_reads_keep_every_source_line_as_pairs(monkeypatch):
    from docreader.tender.directory_compile import reading_payload

    documents = docs(['投标格式\n投标人：________', '\n保留原句与空白。'])
    expected = reading_payload(documents)
    calls = []

    async def ask(_execute, _budget, task, payload, validate):
        calls.append(task)
        assert restore_reading_pairs(payload) == expected
        return validate(plan([{**node('form', 0, 0), 'body_windows': [window(0, 0, 1)]}], [item('form')]))

    monkeypatch.setattr(pipeline, '_ask', ask)
    values, failures = await pipeline._request_directory_plans(documents, None, None)
    assert len(values) == len(calls) == 2 and not failures


async def test_full_requirement_closure_read_preserves_the_same_complete_source(monkeypatch):
    from docreader.tender.directory_compile import reading_payload

    documents = docs(['投标格式\n投标人：________', '\n完整未知条款。'])
    expected = reading_payload(documents)
    compiled = compile_directory(plan([
        {**node('form', 0, 0), 'body_windows': [window(0, 0, 1)]},
    ], [item('form')]), documents)
    checked = []

    async def ask(_execute, _budget, task, payload, validate):
        if task == 'directory_material':
            return validate({'assignments': [], 'chapter_titles': [
                {'title_id': entry['title_id'], 'title': '投标格式'}
                for entry in payload['chapter_title_catalog']]})
        if task == 'directory_requirement':
            return validate({'assignments': []})
        assert task == 'directory_classification'
        checked.append(restore_reading_pairs(payload))
        return validate({'items': []})

    monkeypatch.setattr(pipeline, '_ask', ask)
    await pipeline._request_directory_details(compiled, documents, None, None)
    assert checked == [expected]


async def test_full_source_classification_receives_and_closes_both_block_fragments(monkeypatch):
    documents = docs(['投标格式\n投标人：________\n签章材料应完整，其他材料也', '需提供有效原件扫描件。'])
    compiled = compile_directory(plan([
        {**node('form', 0, 0), 'body_windows': [window(0, 0, 1)]},
    ], [item('form')]), documents)
    before = deepcopy(compiled['chapters'])
    monkeypatch.setattr(pipeline, 'requirement_blocks', lambda value: requirement_blocks(value, limit=3))
    calls = []

    async def ask(_execute, _budget, task, payload, validate):
        calls.append(str(task))
        if task == 'directory_material':
            return validate({'assignments': [], 'table_headers': [], 'chapter_titles': [
                {'title_id': entry['title_id'], 'title': '投标格式'}
                for entry in payload['chapter_title_catalog']]})
        if task == 'directory_requirement':
            page = payload['block'][0]['page']
            return validate({'assignments': [{**window(page, 2 if page == 0 else 0),
                'scope': 'unassigned', 'chapter_ids': []}]})
        assert task == 'directory_classification'
        cases = payload.get('requirement_cases', [])
        assert len(cases) == 2, 'The existing full-source read must see both unresolved source fragments.'
        assert calls.count('directory_requirement') == 2
        return validate({'items': [], 'requirement_adjudications': [{
            'item_ids': [case['id'] for case in cases], 'scope': 'document',
            'chapter_ids': [], 'basis_windows': [],
        }]})

    monkeypatch.setattr(pipeline, '_ask', ask)
    await pipeline._request_directory_details(compiled, documents, None, None)
    assert calls.count('directory_classification') == 1
    assert not any(issue['code'] == 'outline_directory_requirement_unassigned' for issue in compiled['issues'])
    assert any(issue['code'] == 'outline_directory_document_requirement'
               and issue['detail'] == '签章材料应完整，其他材料也\n需提供有效原件扫描件。'
               for issue in compiled['issues'])
    for after, original in zip(compiled['chapters'], before):
        assert all(after[key] == value for key, value in original.items())
        assert after['display_title'] == '投标格式'


def test_guide_distinguishes_unexecuted_procedures_and_source_backed_exclusions(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    procedure = '待执行（不代表已完成）：投标截止前通过指定平台提出问题。'
    excluded = '商业运行备件定义。\n本次不适用依据：技术附表B（本次不适用）。'
    result = export_authoring_document(artifacts, {'chapters': [], 'issues': [
        {'code': 'outline_directory_procedure_requirement', 'detail': procedure, 'rule_refs': []},
        {'code': 'outline_directory_requirement_not_applicable', 'detail': excluded, 'rule_refs': []},
    ]}, [])
    with ZipFile(BytesIO(artifacts.read_bytes(result['package_ref']))) as package:
        guide = package.read('提交要求与待办.md').decode()
    assert '## 外部程序待执行' in guide and '- [ ] ' + procedure in guide
    assert '## 本次不适用依据' in guide and excluded in guide
    assert '## 目录待确认' not in guide


def test_source_decisions_keep_the_complete_reference_and_basis_in_the_guide(tmp_path):
    from docreader.tender import directory_choices

    original = '银行履约保证函\n申请人：________\n银行签章：________'
    compiled = {'directory_decisions': [{
        'case_id': 'bank-case', 'action': 'not_applicable_reference',
        'status': '本次不作为提交响应；完整范本仅供参考，未履行。',
        'basis_text': '本次不要求提交履约保证金。',
        'basis_windows': [window(0, 5)],
        'source_records': [{'heading': {'document_id': 'tender', 'page': 1, 'line': 0},
            'source_windows': [window(1, 0, 2)], 'original_text': original,
            'response_kind': 'form', 'subjects': []}],
    }]}
    issues = directory_choices.decision_issues(compiled)
    assert len(issues) == 1 and issues[0]['code'] == 'outline_directory_source_decision'
    assert original in issues[0]['detail'] and '本次不要求提交履约保证金。' in issues[0]['detail']
    assert 'tender 第2页 第1–3行' in issues[0]['detail']
    artifacts = ArtifactStore(tmp_path)
    result = export_authoring_document(artifacts, {'chapters': [], 'issues': issues}, [])
    with ZipFile(BytesIO(artifacts.read_bytes(result['package_ref']))) as package:
        guide = package.read('提交要求与待办.md').decode()
    assert '## 来源核定记录' in guide and original in guide
    assert '## 目录待确认' not in guide and '未履行' in guide


def test_known_source_condition_still_has_an_unconfirmed_enterprise_exit():
    from docreader.tender import directory_choices

    compiled = {'directory_decisions': [{
        'case_id': 'identity-case', 'action': 'conditional',
        'status': '源条件明确；企业是否满足或采用未确认。',
        'basis_text': '仅代理商需提交制造商授权。', 'basis_windows': [window(0, 2)],
        'source_records': [{'heading': {'document_id': 'tender', 'page': 0, 'line': 0},
            'source_windows': [window(0, 0, 2)],
            'original_text': '代理商授权证明\n制造商：________\n仅代理商适用。',
            'response_kind': 'attachment', 'subjects': []}],
    }, {'case_id': 'unresolved', 'action': 'unresolved'}]}
    issues = directory_choices.decision_issues(compiled)
    assert [issue['code'] for issue in issues] == [
        'outline_directory_source_decision', 'outline_directory_conditional_requirement']
    assert '企业身份和适用情况未确认' in issues[1]['detail']
    assert '仅代理商需提交制造商授权。' in issues[1]['detail']


@pytest.mark.parametrize('title', [None, '附件8E', '失信情况说明：须提供完整查询截图。',
                                  '技术附表A 主要技术参数', '投标函的格式(1A)', '资信查询截图（附件8E）'])
def test_formal_material_receipt_cannot_silently_fall_back_to_a_source_locator(title):
    from docreader.tender.directory_titles import title_catalog

    documents = docs(['附件8E\n提供失信情况查询截图：________'])
    compiled = compile_directory(plan([
        {**node('credit', 0, 0), 'body_windows': [window(0, 0, 1)]},
    ], [item('credit')]), documents)
    before = deepcopy(compiled)
    raw = {'assignments': [], 'chapter_titles': ([] if title is None else [
        {'title_id': title_catalog(compiled, documents)[0]['title_id'], 'title': title}])}
    with pytest.raises(ModelContractError) as error:
        pipeline._accept_material_plan(raw, [], compiled, documents)
    assert error.value.errors[0]['code'] == 'directory_title_invalid'
    assert compiled == before


def test_material_reading_removes_repeated_keys_without_losing_any_source_line():
    from docreader.tender.directory_compile import reading_payload

    documents = docs(['原文标题\n\n完整原句：须提交“证明”\t____\n尾行', '跨页续句。'])
    reading = reading_payload(documents, {'source_version': 3, 'domain_revision': 2})
    before = deepcopy(reading)
    material = pipeline._full_source_reading(reading)
    assert material['source_line_format'] == 'numbered_line_pairs_v1'
    restored = deepcopy(material)
    restored.pop('source_line_format')
    for document in restored['documents']:
        for page in document['pages']:
            page['lines'] = [{'line': line, 'text': text} for line, text in page['lines']]
    assert restored == before == reading


@pytest.mark.parametrize('task', ['material', 'classification'])
@pytest.mark.parametrize('page', [0, 99])
def test_material_and_classification_source_errors_are_rejected_before_a_success_receipt(task, page):
    from docreader.tender.directory_titles import title_catalog

    documents = docs(['投标格式\n投标人：________\n额外来源要求'])
    compiled = compile_directory(plan([
        {**node('form', 0, 0), 'body_windows': [window(0, 0, 1)]},
    ], [item('form')]), documents)
    before = deepcopy(compiled)

    def accept():
        if task == 'classification':
            return pipeline._accept_classification_plan({'items': [{'kind': 'goods', **window(page, 2)}]},
                                                         compiled, documents)
        return pipeline._accept_material_plan({
            'assignments': [{'chapter_id': 'form', **window(page, 2)}],
            'chapter_titles': [{'title_id': title_catalog(compiled, documents)[0]['title_id'], 'title': '投标格式'}],
        }, [], compiled, documents)

    if page == 0:
        assert accept()
    else:
        with pytest.raises(ModelContractError) as error:
            accept()
        assert error.value.errors[0]['code'] == 'directory_' + task + '_source_invalid'
    assert compiled == before


@pytest.mark.parametrize('scope,line,rejected', [
    ('chapter', 1, False), ('chapter', 3, True), ('document', 3, False), ('unassigned', 3, False),
])
def test_later_requirement_requests_preserve_an_existing_no_submission_reference(scope, line, rejected):
    from docreader.tender.directory_choices import apply_directory_adjudications, directory_cases
    from test_directory_choices import promise_and_bank

    compiled, documents = promise_and_bank()
    apply_directory_adjudications(compiled, documents, [
        {'case_id': case['case_id'], 'action': 'not_applicable_reference' if case['kind'] == 'extra' else 'alternative',
         'basis_windows': [window(0, 4)]} for case in directory_cases(compiled, documents)])
    before = deepcopy(compiled)
    application = pipeline._directory_applicability(compiled)
    assert len(application['source_reference_exclusions']) == 1
    assert application['source_reference_exclusions'][0]['body_windows'] == [window(0, 3)]
    assignment = {**window(0, line), 'scope': scope, 'chapter_ids': ['promise'] if scope == 'chapter' else []}

    def accept():
        return pipeline._accept_requirement_plan({'assignments': [assignment]}, documents,
                                                requirement_blocks(documents)[0], compiled)

    if rejected:
        with pytest.raises(ModelContractError) as error:
            accept()
        assert error.value.errors[0]['code'] == 'directory_requirement_source_invalid'
    else:
        assert accept()
    assert compiled == before
