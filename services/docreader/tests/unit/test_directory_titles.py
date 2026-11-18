"""Business names stay independent from source headings, raw IDs and source bodies."""

from copy import deepcopy

import pytest

from docreader.tender.directory_compile import (
    DirectoryPlanRejected, MaterialPlan, ParameterPlan, apply_parameters, compile_directory, outline_from_compiled,
)
from test_directory_compile import docs, item, node, plan, point, window
from test_directory_parameter_continuations import parameter, response, source_case


SNAPSHOT = {'schema_version': 2, 'run_id': 'titles', 'source_version': 1, 'domain_revision': 1}


def source_groups(*, same_source_name=False, ids=('honors', 'certificates')):
    group = '1.1 商务文件：投标人提交的商务投标文件应包含下列材料，完整证明保留原式。'
    pages = ['\n'.join([group, '附件 8E', '企业资信及荣誉材料原式。',
                        group if same_source_name else '1.2 商务证明：投标人须按组成要求准备材料。',
                        '附件 8F', '管理体系认证证书原式。'])]
    raw = plan([{**node(ids[0], 0, 1), 'body_windows': [window(0, 1, 2)]},
                {**node(ids[1], 0, 4), 'body_windows': [window(0, 4, 5)]}], [])
    raw['scopes'] = [{'title': point(0, 0), 'items': [item(ids[0])]},
                     {'title': point(0, 3), 'items': [item(ids[1])]}]
    documents = docs(pages)
    return documents, compile_directory(raw, documents)


def titles_module():
    from docreader.tender import directory_titles
    return directory_titles


def names(catalog):
    return [{'title_id': entry['title_id'], 'title': '商务文件' if entry['kind'] == 'group'
             else '企业资信与荣誉' if entry['heading']['line'] == 1 else '管理体系认证'}
            for entry in catalog]


def test_material_contract_keeps_legacy_defaults_and_accepts_short_title_records():
    assert MaterialPlan.model_validate({}).model_dump()['chapter_titles'] == []
    raw = {'chapter_titles': [{'title_id': 'source-title', 'title': '企业资信与荣誉'}]}
    assert MaterialPlan.model_validate(raw).model_dump()['chapter_titles'] == raw['chapter_titles']


def test_source_groups_with_equal_names_are_not_one_submission_volume():
    documents, compiled = source_groups(same_source_name=True)
    outline = outline_from_compiled(SNAPSHOT, compiled)
    assert len(outline['chapters']) == 2
    assert [len(chapter['children']) for chapter in outline['chapters']] == [1, 1]


def test_same_scope_repeat_records_the_group_anchor_before_capturing_choices():
    from docreader.tender.directory_choices import apply_directory_adjudications, directory_cases

    documents = docs(['商务文件\n原式\n完整正文\n适用于甲方\n适用于乙方'])
    compiled = compile_directory(plan([{**node('form', 0, 1), 'body_windows': [window(0, 1, 2)]}],
        [item('form', condition=window(0, 3)), item('form', condition=window(0, 4))],
        title=point(0, 0)), documents)
    cases = directory_cases(compiled, documents)
    duplicate = next(case for case in cases if case['kind'] == 'duplicate_condition')
    assert duplicate['current']['group_heading'] == duplicate['alternative']['group_heading'] == point(0, 0)
    decisions = [{'case_id': case['case_id'], 'action': 'current', 'basis_windows': [window(0, 3)]}
                 for case in cases]
    apply_directory_adjudications(compiled, documents, decisions)
    assert compiled['chapters'][0]['condition'] == '适用于甲方'


def test_names_bind_source_positions_and_keep_source_text_and_aliases():
    module = titles_module()
    documents, compiled = source_groups()
    before = deepcopy(compiled)
    catalog = module.title_catalog(compiled, documents)
    assert len(catalog) == 4
    renamed_documents, renamed = source_groups(ids=('slot2-honors', 'slot2-certificates'))
    assert {entry['title_id'] for entry in catalog} == {
        entry['title_id'] for entry in module.title_catalog(renamed, renamed_documents)}
    module.apply_chapter_titles(compiled, documents, names(catalog), catalog)
    assert [chapter['title'] for chapter in compiled['chapters']] == [
        chapter['title'] for chapter in before['chapters']]
    for chapter, original in zip(compiled['chapters'], before['chapters']):
        for field in ('heading', 'text', 'lines', 'body_windows', 'group'):
            assert chapter.get(field) == original.get(field)
    outline = outline_from_compiled(SNAPSHOT, compiled)
    assert [chapter['title'] for chapter in outline['chapters']] == ['商务文件', '商务文件']
    assert [chapter['children'][0]['title'] for chapter in outline['chapters']] == ['企业资信与荣誉', '管理体系认证']
    assert [chapter['children'][0]['targets'] for chapter in outline['chapters']] == [
        chapter['children'][0]['targets'] for chapter in outline_from_compiled(SNAPSHOT, before)['chapters']]
    mappings = [issue for issue in outline['issues'] if issue['code'] == 'outline_directory_title_mapping']
    assert len(mappings) == 4
    assert any('附件 8E' in issue['detail'] and '企业资信与荣誉' in issue['detail'] for issue in mappings)


@pytest.mark.parametrize('bad', ['附件 8E', '附件8E 企业资信', '1.1 商务文件', '第六章 资格资料',
                                '需提供原厂经审计的财务会计报表。', '未完成的要求……', '名称\n正文', 'x' * 41,
                                '技术附表A 主要技术参数', '投标函的格式(1A)', '资信查询截图（附件8E）'])
def test_formal_business_names_reject_numbered_or_sentence_titles_without_mutation(bad):
    module = titles_module()
    documents, compiled = source_groups()
    catalog = module.title_catalog(compiled, documents)
    value = names(catalog)
    value[0]['title'] = bad
    before = deepcopy(compiled)
    with pytest.raises(DirectoryPlanRejected):
        module.apply_chapter_titles(compiled, documents, value, catalog)
    assert compiled == before


@pytest.mark.parametrize('title', ['5G网络', 'IPv6地址转换', '财务情况（投标人与原厂）'])
def test_business_names_keep_protocol_numbers_and_real_subject_qualifiers(title):
    assert titles_module().business_title(title) == title


@pytest.mark.parametrize('mode', ['missing', 'duplicate', 'unknown'])
def test_final_adopted_names_must_be_complete_unique_and_offered(mode):
    module = titles_module()
    documents, compiled = source_groups()
    catalog = module.title_catalog(compiled, documents)
    value = names(catalog)
    if mode == 'missing':
        value.pop()
    elif mode == 'duplicate':
        value.append(value[0])
    else:
        value[0]['title_id'] = 'unknown-source'
    with pytest.raises(DirectoryPlanRejected):
        module.validate_chapter_titles(compiled, documents, value, catalog)


def test_catalog_covers_source_candidates_before_choices_and_validates_final_choice():
    module = titles_module()
    documents, compiled = source_groups()
    current, alternative = map(deepcopy, compiled['chapters'])
    compiled['chapters'] = [current]
    compiled['issues'].append({'code': 'outline_directory_conflict', 'detail': 'source choice',
        '_directory_case': {'current': current, 'alternative': alternative}})
    catalog = module.title_catalog(compiled, documents)
    chosen = deepcopy(compiled)
    alternative['id'] = current['id']
    chosen['chapters'] = [alternative]
    module.validate_chapter_titles(chosen, documents, names(catalog), catalog)
    module.apply_chapter_titles(chosen, documents, names(catalog), catalog)
    assert outline_from_compiled(SNAPSHOT, chosen)['chapters'][0]['children'][0]['title'] == '管理体系认证'


def test_parameter_business_name_keeps_complete_cross_page_body_and_original_heading():
    documents, compiled = source_case()
    original = deepcopy(compiled['chapters'][0])
    apply_parameters(compiled, documents, response(parameter(title='字符处理与替代方案'))['chapters'])
    child = compiled['chapters'][1]
    assert child['title'] == '★支持数字、字'
    assert compiled['chapters'][0] == original
    final = outline_from_compiled(SNAPSHOT, compiled)['chapters'][0]['children'][0]
    assert final['title'] == '字符处理与替代方案'
    assert final['targets'][0]['purpose'] == '★支持数字、字\n母及符号；否则须提供完整替代方案。\n证明材料须厂家盖章。'
    assert any(issue['code'] == 'outline_directory_title_mapping' and '★支持数字、字' in issue['detail']
               for issue in outline_from_compiled(SNAPSHOT, compiled)['issues'])


def test_parameter_raw_is_compatible_but_new_formal_acceptance_requires_business_name():
    module = titles_module()
    legacy = ParameterPlan.model_validate(response(parameter())).model_dump()
    assert legacy['chapters'][0]['items'][0]['title'] is None
    with pytest.raises(DirectoryPlanRejected):
        module.accept_parameter_titles(legacy)
    current = ParameterPlan.model_validate(response(parameter(title='字符处理与替代方案'))).model_dump()
    assert module.accept_parameter_titles(current) == current
