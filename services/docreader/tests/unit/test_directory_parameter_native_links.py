"""Complete P-selected native originals are cited at their actual parent targets."""

from copy import deepcopy
from hashlib import sha256
from io import BytesIO

from docx import Document
import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.tender import authoring_pipeline as pipeline
from docreader.tender import directory_compile as compiler
from docreader.tender.authoring_coverage import pending_outline_issues
from test_authoring_pipeline_boundaries import budget, snapshot
from test_directory_compile import entry, item, node, plan, point, window


CONTRACT = 'source_bound_parameter_tables_v1'
SNAPSHOT = {'schema_version': 2, 'run_id': 'native-link', 'source_version': 1, 'domain_revision': 1}


def native_case(*, contract=CONTRACT, subjects=False, unknown_row=False, extra_prose=None):
    pages = ['\n'.join(['服务方案', '软件授权采购', '提供9个月授权，7×24服务，1小时到场、4小时方案、12小时解决。',
                       '投标人与制造商分别提供资料。', *[f'服务范围{number}' for number in range(23)]]),
             '序号 产品 性能\n1 运维安全管理与审计系统资',
             '源数\n2 态势感知与安\n全运营平台',
             '最后规格\n提供原厂授权证明。',
             '服务交付说明\n' + (extra_prose or '交付原厂授权及升级服务证明，企业值留空。')]
    index = {'relative_path': 'aa/'+'a'*64, 'sha256': 'a'*64, 'size': 1, 'media_type': 'application/json'}
    parts = {f'table-{page}': {'source_ref': {'source_index_ref': index, 'unit_id': f'table-{page}'},
                              'selectors': [], 'header_row_count': 0} for page in (1, 2, 3)}
    documents = [{'document_id': 'tender', 'role': 'main', 'pages': pages,
                  'line_units': [[f'table-{page}' if page in (1, 2, 3) else None for _ in text.splitlines()]
                                 for page, text in enumerate(pages)], 'template_parts': parts}]
    if unknown_row:
        documents[0]['line_units'][2][1] = None
    parent = {**node('service', 0, 0), 'response_kind': 'narrative',
              'body_windows': [window(page, 0, len(text.splitlines())-1) for page, text in enumerate(pages)]}
    if subjects:
        parent['subjects'] = [{'source': window(0, 3), 'role': role} for role in ('投标人', '制造商')]
    compiled = compiler.compile_directory(plan([parent], [item('service')]), documents)
    if contract is not None:
        compiled['_parameter_source_contract'] = contract
    selected = {'document_id': 'tender', 'page': 0, 'start_line': 1,
                'end_line': len(pages[0].splitlines())-1, 'content_start_line': 1, 'title': '软件授权采购',
                'continuation_windows': [window(page, 0, len(pages[page].splitlines())-1) for page in (1, 2, 3, 4)]}
    return documents, compiled, {'chapter_id': 'service', 'items': [selected]}


def expanded(documents, compiled, selection):
    compiler.apply_parameters(compiled, documents, [selection])
    return compiler.outline_from_compiled(SNAPSHOT, compiled)


def test_complete_native_parts_link_to_final_parent_targets_and_keep_non_table_terms():
    documents, compiled, selection = native_case()
    original = deepcopy(compiled['chapters'][0])
    outline = expanded(documents, compiled, selection)
    parent = outline['chapters'][0]
    child = parent['children'][0]
    originals = [target for target in parent['targets'] if target['template_refs']]
    assert len(originals) == 3 and sum(len(target['template_refs']) for target in parent['targets']) == 3
    assert child['targets'][0]['related_target_ids'] == [target['target_id'] for target in originals]
    assert not child['targets'][0]['template_refs']
    purpose = child['targets'][0]['purpose']
    for text in ('9个月', '7×24', '1小时', '4小时', '12小时', '交付原厂授权及升级服务证明'):
        assert text in purpose
    for text in ('源数', '态势感知与安', '全运营平台', 'table-1', 'table-2', 'table-3'):
        assert text not in purpose
    assert compiled['chapters'][0] == original
    assert any(ref.get('unit_id') == 'table-2' for ref in compiled['chapters'][1]['lines'])


@pytest.mark.parametrize('contract', [None, 'unknown-future-contract'])
def test_missing_or_unknown_contract_retains_legacy_text_refs_and_targets(contract):
    documents, compiled, selection = native_case(contract=contract)
    outline = expanded(documents, compiled, selection)
    child = outline['chapters'][0]['children'][0]
    assert '源数' in child['targets'][0]['purpose']
    assert child['targets'][0]['related_target_ids'] == []
    assert all('unit_id' not in ref for ref in compiled['chapters'][1]['lines'])


@pytest.mark.parametrize('problem', ['partial', 'missing_parent_part', 'partial_parent_selection', 'different_source_version',
                                    'unbound_table_line', 'cross_subject', 'ambiguous_parent',
                                    'duplicate_part_in_one_parent_target'])
def test_incomplete_missing_different_subject_or_ambiguous_parent_keeps_source_and_pending_exit(problem):
    documents, compiled, selection = native_case(subjects=problem == 'cross_subject', unknown_row=problem == 'unbound_table_line')
    parent = compiled['chapters'][0]
    if problem == 'partial':
        selection['items'][0]['continuation_windows'][1]['end_line'] = 1
    elif problem == 'missing_parent_part':
        parent['template_parts'] = [part for part in parent['template_parts'] if part['source_ref']['unit_id'] != 'table-2']
        parent['template_segments'] = [segment for segment in parent['template_segments']
                                       if segment.get('part', {}).get('source_ref', {}).get('unit_id') != 'table-2']
    elif problem == 'partial_parent_selection':
        part = next(segment['part'] for segment in parent['template_segments']
                    if segment.get('part', {}).get('source_ref', {}).get('unit_id') == 'table-2')
        part['selectors'] = [{'range_id': 'slice', 'source_range_id': 'native-row', 'start': 0, 'end': 1}]
    elif problem == 'different_source_version':
        parent['template_segments'] = deepcopy(parent['template_segments'])
        part = next(segment['part'] for segment in parent['template_segments']
                    if segment.get('part', {}).get('source_ref', {}).get('unit_id') == 'table-2')
        part['source_ref']['source_index_ref'].update(sha256='b'*64, relative_path='bb/'+'b'*64)
    elif problem == 'ambiguous_parent':
        parent['material_segments'] = [deepcopy(next(segment for segment in parent['template_segments'] if segment.get('part')))]
    elif problem == 'duplicate_part_in_one_parent_target':
        parent.pop('template_segments')
        parent['template_parts'].append(deepcopy(parent['template_parts'][1]))
    compiler.apply_parameters(compiled, documents, [selection])
    original = compiled['chapters'][1]['text']
    if problem == 'cross_subject':
        compiled['chapters'][1]['subjects'] = [{'source': window(0, 3), 'role': '其他主体'}]
    outline = compiler.outline_from_compiled(SNAPSHOT, compiled)
    child = outline['chapters'][0]['children'][0]
    assert all(target['purpose'] == original and not target['related_target_ids'] for target in child['targets'])
    pending = pending_outline_issues(outline)
    assert any(issue['code'] in ('outline_directory_partial_table', 'outline_directory_table_unbound') for issue in pending)
    assert any('原文' in issue['detail'] and '核' in issue['detail'] for issue in pending)


def test_each_required_subject_links_only_its_own_parent_native_instances():
    documents, compiled, selection = native_case(subjects=True)
    outline = expanded(documents, compiled, selection)
    parent = outline['chapters'][0]
    child = parent['children'][0]
    for target in child['targets']:
        originals = [original for original in parent['targets']
                     if original['subject'] == target['subject'] and original['template_refs']]
        assert len(originals) == 3
        assert target['related_target_ids'] == [original['target_id'] for original in originals]
    assert {target['subject'] for target in child['targets']} == {'投标人', '制造商'}


@pytest.mark.parametrize('text', ['资源数不足时必须提供替代路径，4小时内完成核查。',
                                '需提交态势感知与安全运营平台威胁情报库授权许可证明。'])
def test_same_words_in_actual_non_table_requirements_are_preserved(text):
    documents, compiled, selection = native_case(extra_prose=text)
    outline = expanded(documents, compiled, selection)
    purpose = outline['chapters'][0]['children'][0]['targets'][0]['purpose']
    assert text in purpose
    assert '源数' not in [line.strip() for line in purpose.splitlines()]


def test_a_complete_table_only_item_keeps_a_human_readable_response_purpose():
    documents, compiled, selection = native_case()
    selection['items'][0].update(page=1,start_line=0,content_start_line=0,end_line=1,
                                  continuation_windows=[window(2,0,2),window(3,0,1)])
    outline = expanded(documents, compiled, selection)
    target = outline['chapters'][0]['children'][0]['targets'][0]
    assert len(target['related_target_ids']) == 3
    assert target['purpose'] == '原表见关联响应位置。'
    rendering = next(r for r in compiled['_source_renderings'] if r['target_id'] == target['target_id'])
    assert rendering['generated'] is True


@pytest.mark.parametrize('subjects', [False, True])
def test_reviewed_footer_projects_after_native_links_without_removing_a_real_quantity(subjects):
    from docreader.tender.source_presentation import project_decorations

    documents, compiled, selection = native_case(subjects=subjects,
        extra_prose='交付71份证明，未知企业值留空。\n71')
    outline = expanded(documents, compiled, selection)
    parent = outline['chapters'][0]
    child = parent['children'][0]
    templates = deepcopy([t for t in parent['targets'] if t['template_refs']])
    links = [deepcopy(t['related_target_ids']) for t in child['targets']]
    digest = sha256(b'71').hexdigest()
    reference = {'relative_path': 'aa/' + 'a' * 64, 'sha256': 'a' * 64,
                 'size': 1, 'media_type': 'application/json'}
    # This is the already validated decoration consumed by the presentation layer.
    fact = {'kind': 'source_decoration', 'fact_id': 'reviewed-footer', 'label': 'Exact reviewed footer',
        'enterprise_confirmed': False, 'document_id': 'tender', 'original_ref': reference,
        'review_evidence_ref': reference, '_fragments': [{
            'page': 4, 'line': 2, 'start': 0, 'end': 2, 'text_sha256': digest,
            'text': '71', 'line_text': '71', 'bbox': [1, 1, 2, 2]}]}
    project_decorations(outline, compiled, [fact])

    for target in child['targets']:
        assert '71' not in target['purpose'].splitlines()
        assert '交付71份证明，未知企业值留空。' in target['purpose']
        assert '9个月授权' in target['purpose']
    assert [t['related_target_ids'] for t in child['targets']] == links
    assert [t for t in parent['targets'] if t['template_refs']] == templates
    record, = compiled['_reviewed_source_fact_applications']
    assert record['state'] == 'presentation_excluded'
    assert set(t['target_id'] for t in child['targets']) <= set(record['affected_target_ids'])
    origins = [r for r in compiled['_source_renderings'] if r['chapter_id'] == child['chapter_id']]
    assert all(any(row.get('unit_id') == 'table-2' for row in r['rows']) for r in origins)


def test_same_source_part_in_an_independent_original_is_not_used_instead_of_the_real_parent():
    documents, _, selection = native_case()
    pages = documents[0]['pages']
    owned = [window(page, 0, len(text.splitlines())-1) for page, text in enumerate(pages)]
    pages.append('第一份方案独立提交\n第二份方案独立提交\n服务方案')
    documents[0]['line_units'].append([None]*3)
    parent = {**node('service',0,0),'response_kind':'narrative','body_windows':owned}
    other = {**node('independent-original',5,2),'response_kind':'narrative','body_windows':owned}
    compiled = compiler.compile_directory(plan([parent,other],
        [item('independent-original',entry=0),item('service',entry=1)],
        entries=[entry(5,0),entry(5,1)]),documents)
    compiled['_parameter_source_contract'] = CONTRACT
    outline = expanded(documents, compiled, selection)
    real_parent = outline['chapters'][1]
    child = real_parent['children'][0]
    allowed = {target['target_id'] for target in real_parent['targets'] if target['template_refs']}
    forbidden = {target['target_id'] for target in outline['chapters'][0]['targets'] if target['template_refs']}
    assert set(child['targets'][0]['related_target_ids']) == allowed
    assert not allowed.intersection(forbidden)


async def test_native_originals_appear_once_in_word_and_child_has_one_response_without_column_fragments(tmp_path):
    artifacts = ArtifactStore(tmp_path/'artifacts')
    grid = lambda label: {'row_count': 3, 'column_count': 3, 'cells': [
        {'row': 0,'column': 0,'row_span': 1,'col_span': 3,'text': '采购规格'},
        {'row': 1,'column': 0,'row_span': 1,'col_span': 1,'text': '产品'},
        {'row': 1,'column': 1,'row_span': 2,'col_span': 1,'text': label},
        {'row': 1,'column': 2,'row_span': 2,'col_span': 1,'text': ''},
        {'row': 2,'column': 0,'row_span': 1,'col_span': 1,'text': '数量1'}]}
    fixed = snapshot(artifacts, [{'text': '\n'.join(['服务方案', '软件授权采购', '9个月授权，7×24服务，1/4/12小时响应。',
                                                     *[f'服务范围{number}' for number in range(25)]])},
                                 {'grid': grid('源数')}, {'grid': grid('态势感知与安')}, {'grid': grid('全运营平台')},
                                 {'text': '完整交付授权证明，未知企业值仍留空。'}])
    documents = compiler.documents_from_frozen(artifacts, artifacts.read_json(fixed['frozen_ref']))
    pages = documents[0]['pages']
    for part in documents[0]['template_parts'].values():part['header_row_count'] = 1
    parent = {**node('service', 0, 0, document_id='document'), 'response_kind': 'narrative',
              'body_windows': [window(page, 0, len(text.splitlines())-1, document_id='document') for page,text in enumerate(pages)]}
    compiled = compiler.compile_directory(plan([parent], [item('service')]), documents)
    compiled['_parameter_source_contract'] = CONTRACT
    parameter = {'document_id':'document','page':0,'start_line':1,'end_line':len(pages[0].splitlines())-1,
                 'content_start_line':1,'title':'软件授权采购','continuation_windows':[
                     window(page,0,len(text.splitlines())-1,document_id='document') for page,text in enumerate(pages) if page]}
    compiler.apply_parameters(compiled, documents, [{'chapter_id':'service','items':[parameter]}])
    outline = compiler.outline_from_compiled(fixed, compiled)

    async def model(task, _request, validate):
        assert task == 'authoring_structure'
        return validate({'findings':[]})

    result = artifacts.read_json(await pipeline.generate_document(artifacts,fixed,budget(),model,outline=outline))
    word = Document(BytesIO(artifacts.read_bytes(result['docx_ref'])))
    assert len(word.tables) == 3
    for table,label in zip(word.tables,('源数','态势感知与安','全运营平台')):
        assert table.cell(0,0)._tc is table.cell(0,2)._tc
        assert table.cell(1,1)._tc is table.cell(2,1)._tc and table.cell(1,1).text == label
        assert table.cell(1,2)._tc is table.cell(2,2)._tc and table.cell(1,2).text == ''
    start = next(i for i,paragraph in enumerate(word.paragraphs)
                 if paragraph.style.name.startswith('Heading') and paragraph.text == '软件授权采购')
    child_text = '\n'.join(paragraph.text for paragraph in word.paragraphs[start:])
    assert '参见' in child_text and '完整交付授权证明' in child_text
    assert all(fragment not in child_text for fragment in ('源数','态势感知与安','全运营平台'))
    assert child_text.count('我方响应：________（人工填写）') == 1
    assert child_text.count('证明材料或附件位置：________（原条款要求时填写）') == 1
