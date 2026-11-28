import json

import pytest

from docreader.llm.budget import RequestBudget
from docreader.llm.request import ModelContractError
from docreader.tender.bid_format_outline import select_bid_format_plan
from docreader.tender.directory_compile import compile_directory


@pytest.mark.parametrize('fault', ['carried', 'invented', 'annex', 'different_parent', 'has_child', 'grid', 'file_list'])
async def test_default_format_selection_checks_the_actual_owner_of_a_rejected_prose_heading(fault):
    lines = ['第六章 投标文件格式', '一、资格文书', '1.1 声明函', '附件 4H',
             '投标人是否有遗漏事项；如有，应提交查询截图。', '二、价格文件', '签字：']
    if fault == 'file_list':
        lines[4] = '授权书；审计报告。'
    doc = {'document_id': 's', 'role': 'main', 'pages': ['\n'.join(lines)]}
    if fault == 'grid':
        doc.update(line_units=[[None, None, None, None, 'g', None, None]], template_parts={'g': {}})

    async def model(_task, request, validate):
        refs = {text: ref for ref, text in json.loads(request['messages'][1]['content'][0]['text'])['lines']}
        parent, child, mention, price = [refs[lines[i]] for i in (1, 2, 4, 5)]
        bad = {'heading_refs': [mention], 'name': '查询截图', 'parent_heading_ref': parent}
        if fault == 'invented':
            bad['name'] = '不存在的系统查询书'
        if fault == 'annex':
            bad.update(heading_refs=[refs['附件 4H']], name='附件 4H')
        if fault == 'different_parent':
            bad['parent_heading_ref'] = refs[lines[3]]
        if fault == 'file_list':
            bad['name'] = '授权书；审计报告'
        final = {'heading_refs': [price], 'name': '价格文件'}
        if fault == 'has_child':
            final['parent_heading_ref'] = mention
        return validate({'chapters': [{'heading_refs': [parent], 'name': '资格文书'},
                         {'heading_refs': [child], 'name': '声明函', 'parent_heading_ref': parent}, bad, final]})

    if fault != 'carried':
        with pytest.raises(ModelContractError):
            await select_bid_format_plan([doc], RequestBudget(model='test'), model, {})
        return
    plan = await select_bid_format_plan([doc], RequestBudget(model='test'), model, {})
    compiled = compile_directory(plan, [doc])
    assert [c['title'] for c in compiled['chapters']] == ['资格文书', '声明函', '价格文件']
    assert lines[4] in compiled['chapters'][1]['text']
    assert any(i['code'] == 'outline_directory_name_unconfirmed' and '声明函' in i['detail']
               and '第1页' in i['detail'] for i in compiled['issues'])
